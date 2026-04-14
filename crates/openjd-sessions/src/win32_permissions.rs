// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Windows ACL/DACL permission helper — mirrors Python `_windows_permission_helper.py`.

use windows::core::PWSTR;
use windows::Win32::Foundation::HLOCAL;
use windows::Win32::Security::Authorization::{
    SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W, NO_MULTIPLE_TRUSTEE, SET_ACCESS,
    SE_FILE_OBJECT, TRUSTEE_IS_NAME, TRUSTEE_W,
};
use windows::Win32::Security::{
    ACE_FLAGS, ACL, CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, OBJECT_INHERIT_ACE,
};

/// FILE_ALL_ACCESS = 0x1F01FF
const FILE_ALL_ACCESS: u32 = 0x001F01FF;

/// Modify access: FILE_GENERIC_READ | FILE_GENERIC_WRITE | FILE_GENERIC_EXECUTE | DELETE | FILE_DELETE_CHILD
const FILE_MODIFY_ACCESS: u32 = 0x001301FF;

fn make_entry(name_ptr: *mut u16, access_mask: u32, inherit: ACE_FLAGS) -> EXPLICIT_ACCESS_W {
    EXPLICIT_ACCESS_W {
        grfAccessPermissions: access_mask,
        grfAccessMode: SET_ACCESS,
        grfInheritance: inherit,
        Trustee: TRUSTEE_W {
            TrusteeForm: TRUSTEE_IS_NAME,
            TrusteeType: Default::default(),
            ptstrName: PWSTR(name_ptr),
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
        },
    }
}

/// Set DACL permissions on a file or directory.
///
/// Creates a new DACL (replaces existing) with:
/// - Full Control for each principal in `principals_full_control`
/// - Modify access for each principal in `principals_modify_access`
///
/// Child files and directories inherit via `OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE`.
pub fn set_permissions(
    path: &str,
    principals_full_control: &[&str],
    principals_modify_access: &[&str],
) -> Result<(), String> {
    let inherit = CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE;
    let mut wide_strings: Vec<Vec<u16>> = Vec::new();
    let mut entries: Vec<EXPLICIT_ACCESS_W> = Vec::new();

    for &principal in principals_full_control {
        let wide: Vec<u16> = principal.encode_utf16().chain(std::iter::once(0)).collect();
        wide_strings.push(wide);
        let ptr = wide_strings.last().unwrap().as_ptr() as *mut u16;
        entries.push(make_entry(ptr, FILE_ALL_ACCESS, inherit));
    }

    for &principal in principals_modify_access {
        let wide: Vec<u16> = principal.encode_utf16().chain(std::iter::once(0)).collect();
        wide_strings.push(wide);
        let ptr = wide_strings.last().unwrap().as_ptr() as *mut u16;
        entries.push(make_entry(ptr, FILE_MODIFY_ACCESS, inherit));
    }

    let mut new_acl = std::ptr::null_mut::<ACL>();
    let result = unsafe { SetEntriesInAclW(Some(&entries), None, &mut new_acl) };
    if result.is_err() {
        return Err(format!("Could not build ACL for '{path}': {result:?}"));
    }

    let path_w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let result = unsafe {
        SetNamedSecurityInfoW(
            PWSTR(path_w.as_ptr() as *mut _),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(new_acl as *const ACL),
            None,
        )
    };

    if !new_acl.is_null() {
        unsafe {
            windows::Win32::Foundation::LocalFree(HLOCAL(new_acl as *mut _));
        }
    }

    if result.is_err() {
        return Err(format!("Could not set permissions on '{path}': {result:?}"));
    }

    Ok(())
}
