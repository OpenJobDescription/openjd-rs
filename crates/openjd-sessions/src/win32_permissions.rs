// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Windows ACL/DACL permission helper — mirrors Python `_windows_permission_helper.py`.
//!
//! Uses the same Win32 call sequence as the Python implementation:
//! 1. `LookupAccountNameW` to resolve each principal name to a SID
//! 2. `AddAccessAllowedAceEx` to build the DACL with SID-based ACEs
//! 3. `InitializeSecurityDescriptor` / `SetSecurityDescriptorDacl` / `SetFileSecurityW`

use windows::Win32::Security::{
    AddAccessAllowedAceEx, InitializeAcl, InitializeSecurityDescriptor, LookupAccountNameW,
    SetFileSecurityW, SetSecurityDescriptorDacl, ACE_REVISION, ACL, CONTAINER_INHERIT_ACE,
    DACL_SECURITY_INFORMATION, OBJECT_INHERIT_ACE, PSECURITY_DESCRIPTOR, PSID, SECURITY_DESCRIPTOR,
    SID_NAME_USE,
};

/// SECURITY_DESCRIPTOR_REVISION = 1 (not exported by the windows crate)
const SECURITY_DESCRIPTOR_REVISION: u32 = 1;

/// FILE_ALL_ACCESS = 0x1F01FF
const FILE_ALL_ACCESS: u32 = 0x001F01FF;

/// Modify access: FILE_GENERIC_READ | FILE_GENERIC_WRITE | FILE_GENERIC_EXECUTE | DELETE | FILE_DELETE_CHILD
const FILE_MODIFY_ACCESS: u32 = 0x001301FF;

/// Resolve a principal name to a SID via `LookupAccountNameW`.
fn lookup_sid(principal: &str) -> Result<Vec<u8>, String> {
    let name_w: Vec<u16> = principal.encode_utf16().chain(std::iter::once(0)).collect();
    let mut sid_size: u32 = 0;
    let mut domain_size: u32 = 0;
    let mut sid_type = SID_NAME_USE::default();

    // First call to get buffer sizes
    unsafe {
        let _ = LookupAccountNameW(
            None,
            windows::core::PCWSTR(name_w.as_ptr()),
            Some(PSID(std::ptr::null_mut())),
            &mut sid_size,
            Some(windows::core::PWSTR(std::ptr::null_mut())),
            &mut domain_size,
            &mut sid_type,
        );
    }
    if sid_size == 0 {
        return Err(format!(
            "Could not look up account '{principal}': LookupAccountNameW returned zero SID size"
        ));
    }

    let mut sid_buf = vec![0u8; sid_size as usize];
    let mut domain_buf = vec![0u16; domain_size as usize];

    unsafe {
        LookupAccountNameW(
            None,
            windows::core::PCWSTR(name_w.as_ptr()),
            Some(PSID(sid_buf.as_mut_ptr() as *mut _)),
            &mut sid_size,
            Some(windows::core::PWSTR(domain_buf.as_mut_ptr())),
            &mut domain_size,
            &mut sid_type,
        )
        .map_err(|e| format!("Could not look up account '{principal}': {e}"))?;
    }

    Ok(sid_buf)
}

/// Set DACL permissions on a file or directory.
///
/// Creates a new DACL (replaces existing) with:
/// - Full Control for each principal in `principals_full_control`
/// - Modify access for each principal in `principals_modify_access`
///
/// Child files and directories inherit via `OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE`.
///
/// Mirrors the Python `WindowsPermissionHelper.set_permissions` call-for-call:
/// `LookupAccountName` → `AddAccessAllowedAceEx` → `SetSecurityDescriptorDacl` →
/// `SetFileSecurity`.
pub fn set_permissions(
    path: &str,
    principals_full_control: &[&str],
    principals_modify_access: &[&str],
) -> Result<(), String> {
    let inherit = CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE;

    // Resolve all principal names to SIDs first (fail early on bad names)
    let mut ace_entries: Vec<(Vec<u8>, u32)> = Vec::new();
    for &principal in principals_full_control {
        ace_entries.push((lookup_sid(principal)?, FILE_ALL_ACCESS));
    }
    for &principal in principals_modify_access {
        ace_entries.push((lookup_sid(principal)?, FILE_MODIFY_ACCESS));
    }

    // Allocate an ACL large enough for all ACEs.
    // Each ACE: sizeof(ACE_HEADER)(4) + sizeof(ACCESS_MASK)(4) + SID length
    let acl_header_size = std::mem::size_of::<ACL>();
    let mut acl_size = acl_header_size;
    for (sid, _) in &ace_entries {
        let ace_size = 8 + sid.len();
        acl_size += (ace_size + 3) & !3; // DWORD-align
    }

    let mut acl_buf = vec![0u8; acl_size];
    unsafe {
        InitializeAcl(
            acl_buf.as_mut_ptr() as *mut ACL,
            acl_size as u32,
            ACE_REVISION(2),
        )
        .map_err(|e| format!("InitializeAcl failed: {e}"))?;
    }

    // Add ACEs — mirrors Python's dacl.AddAccessAllowedAceEx() calls
    for (sid, mask) in &ace_entries {
        unsafe {
            AddAccessAllowedAceEx(
                acl_buf.as_mut_ptr() as *mut ACL,
                ACE_REVISION(2),
                inherit,
                *mask,
                PSID(sid.as_ptr() as *mut _),
            )
            .map_err(|e| format!("AddAccessAllowedAceEx failed: {e}"))?;
        }
    }

    // Create a new absolute security descriptor and set the DACL on it.
    // Python: sd = GetFileSecurity(...); sd.SetSecurityDescriptorDacl(1, dacl, 0)
    // The Python win32security object handles the self-relative → absolute conversion
    // internally. In Rust we create a fresh absolute SD instead.
    let mut sd = SECURITY_DESCRIPTOR::default();
    unsafe {
        InitializeSecurityDescriptor(
            PSECURITY_DESCRIPTOR(&mut sd as *mut _ as *mut _),
            SECURITY_DESCRIPTOR_REVISION,
        )
        .map_err(|e| format!("InitializeSecurityDescriptor failed: {e}"))?;

        // bDaclPresent=true, bDaclDefaulted=false — mirrors Python's sd.SetSecurityDescriptorDacl(1, dacl, 0)
        SetSecurityDescriptorDacl(
            PSECURITY_DESCRIPTOR(&mut sd as *mut _ as *mut _),
            true,
            Some(acl_buf.as_ptr() as *const ACL),
            false,
        )
        .map_err(|e| format!("SetSecurityDescriptorDacl failed: {e}"))?;
    }

    // Apply the security descriptor — mirrors Python's SetFileSecurity()
    let path_w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let ret = unsafe {
        SetFileSecurityW(
            windows::core::PCWSTR(path_w.as_ptr()),
            DACL_SECURITY_INFORMATION,
            PSECURITY_DESCRIPTOR(&sd as *const _ as *mut _),
        )
    };
    if !ret.as_bool() {
        return Err(format!(
            "SetFileSecurityW failed for '{path}': {}",
            std::io::Error::last_os_error()
        ));
    }

    Ok(())
}
