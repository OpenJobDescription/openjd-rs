// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Windows ACL/DACL permission helper — mirrors Python `_windows_permission_helper.py`.
//!
//! Uses the same Win32 call sequence as the Python implementation:
//! 1. `LookupAccountNameW` to resolve each principal name to a SID
//! 2. `AddAccessAllowedAceEx` to build the DACL with SID-based ACEs
//! 3. `InitializeSecurityDescriptor` / `SetSecurityDescriptorDacl` / `SetFileSecurityW`
//!
//! [`set_permissions`] is the Python-compatible API — the file continues to
//! inherit DACL entries from its parent. [`set_permissions_protected`] is a
//! Rust-specific variant that additionally marks the DACL as *protected*,
//! severing inheritance from the parent. This is required for the embedded
//! helper binary: its parent session working directory grants the session
//! user Modify access via an inheritable ACE, so without protection the
//! session user would still have Modify on the helper binary through
//! inheritance and could overwrite it.

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

/// Read & Execute access: FILE_GENERIC_READ | FILE_GENERIC_EXECUTE.
///
/// This grants traverse / read / execute but explicitly withholds write,
/// delete, and delete-child — used for the embedded helper binary and its
/// containing directory so the session user can run the helper but cannot
/// modify or replace it.
const FILE_READ_EXECUTE_ACCESS: u32 = 0x001200A9;

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

/// Build an in-memory DACL buffer populated with the requested allowed
/// ACEs. The buffer is heap-allocated; the caller must keep it alive
/// while the ACL pointer derived from it is used.
fn build_dacl(
    principals_full_control: &[&str],
    principals_modify_access: &[&str],
    principals_read_execute: &[&str],
) -> Result<Vec<u8>, String> {
    let inherit = CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE;

    // Resolve all principal names to SIDs first (fail early on bad names)
    let mut ace_entries: Vec<(Vec<u8>, u32)> = Vec::new();
    for &principal in principals_full_control {
        ace_entries.push((lookup_sid(principal)?, FILE_ALL_ACCESS));
    }
    for &principal in principals_modify_access {
        ace_entries.push((lookup_sid(principal)?, FILE_MODIFY_ACCESS));
    }
    for &principal in principals_read_execute {
        ace_entries.push((lookup_sid(principal)?, FILE_READ_EXECUTE_ACCESS));
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

    Ok(acl_buf)
}

/// Set DACL permissions on a file or directory, leaving inheritance from
/// the parent intact (Python-compatible).
///
/// Creates a new DACL (replaces the previous explicit DACL) with:
/// - Full Control for each principal in `principals_full_control`
/// - Modify access for each principal in `principals_modify_access`
/// - Read & Execute (no write, no delete) for each principal in
///   `principals_read_execute`
///
/// Child files and directories inherit via `OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE`.
///
/// Mirrors the Python `WindowsPermissionHelper.set_permissions` call-for-call:
/// `LookupAccountName` → `AddAccessAllowedAceEx` → `SetSecurityDescriptorDacl` →
/// `SetFileSecurity`. The Python helper only exposes full-control and modify
/// buckets; the read-execute bucket is Rust-specific.
pub fn set_permissions(
    path: &str,
    principals_full_control: &[&str],
    principals_modify_access: &[&str],
    principals_read_execute: &[&str],
) -> Result<(), String> {
    let acl_buf = build_dacl(
        principals_full_control,
        principals_modify_access,
        principals_read_execute,
    )?;

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

/// Set DACL permissions on a file or directory *and* mark the DACL as
/// protected — severing inheritance from the parent.
///
/// Same semantics as [`set_permissions`] for the principal buckets, but
/// after the new DACL is applied the parent's inheritable ACEs no longer
/// apply to this object. This is critical for the embedded helper binary
/// (and its containing directory): without protection, the session
/// user's Modify ACE on the parent session working directory would be
/// inherited by the helper binary, defeating the restriction.
///
/// Uses `SetNamedSecurityInfoW` (instead of `SetFileSecurityW`) because
/// that is the only documented API that accepts
/// `PROTECTED_DACL_SECURITY_INFORMATION`.
pub fn set_permissions_protected(
    path: &str,
    principals_full_control: &[&str],
    principals_modify_access: &[&str],
    principals_read_execute: &[&str],
) -> Result<(), String> {
    use windows::Win32::Security::Authorization::{SetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows::Win32::Security::PROTECTED_DACL_SECURITY_INFORMATION;

    let acl_buf = build_dacl(
        principals_full_control,
        principals_modify_access,
        principals_read_execute,
    )?;

    let path_w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let result = unsafe {
        SetNamedSecurityInfoW(
            windows::core::PCWSTR(path_w.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None, // owner
            None, // group
            Some(acl_buf.as_ptr() as *const ACL),
            None, // sacl
        )
    };
    if result.is_err() {
        return Err(format!(
            "SetNamedSecurityInfoW failed for '{path}': WIN32_ERROR({})",
            result.0
        ));
    }

    Ok(())
}
