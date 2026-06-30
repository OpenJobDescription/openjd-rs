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

use std::sync::atomic::{AtomicBool, Ordering};

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

/// ERROR_NONE_MAPPED (1332): raised when the LSA service hasn't finished
/// initializing on freshly started EC2 instances.
const ERROR_NONE_MAPPED: u32 = 1332;

/// Maximum retry attempts for transient LookupAccountNameW failures.
const LOOKUP_MAX_RETRIES: u32 = 3;

/// Set once `LookupAccountNameW` is observed to function — either via a
/// successful resolution or a successful probe of the current process user.
/// The LSA service never goes back down within a process lifetime, so once
/// this latches `true` a subsequent `ERROR_NONE_MAPPED` is known to mean the
/// *name* is genuinely unmapped rather than the service being mid-init.
static LSA_AVAILABLE: AtomicBool = AtomicBool::new(false);

/// `true` if `err` is the Win32 `ERROR_NONE_MAPPED` (1332) failure, accepting
/// either the bare Win32 code or its `HRESULT_FROM_WIN32` wrapping
/// (`0x8007_0000 | code`, i.e. `0x8007_0534`). The `windows` crate surfaces
/// `LookupAccountNameW` failures as the wrapped HRESULT, but we match both
/// forms defensively.
///
/// Note this code is overloaded by Windows: it is returned both when the LSA
/// service has not finished initializing (transient, only at instance startup)
/// *and* when a name genuinely does not map to any account (permanent). Callers
/// must disambiguate via [`lsa_is_available`] before treating it as retryable.
fn is_error_none_mapped(err: &windows::core::Error) -> bool {
    let code = err.code().0 as u32;
    code == ERROR_NONE_MAPPED || code == (0x8007_0000 | ERROR_NONE_MAPPED)
}

/// `true` if the Local Security Authority is confirmed up. Cached after the
/// first positive observation (LSA never restarts within a process lifetime).
///
/// When not yet confirmed, this probes by resolving the current process user's
/// name — which always maps to a SID once LSA is running. This is what lets a
/// genuine "no such account" lookup fail fast instead of waiting out the
/// startup backoff: if the probe succeeds, the LSA is up and the original
/// `ERROR_NONE_MAPPED` must mean the name is unmapped, not that the service is
/// initializing.
fn lsa_is_available() -> bool {
    if LSA_AVAILABLE.load(Ordering::Relaxed) {
        return true;
    }
    let probed = match crate::win32::get_process_user() {
        Ok(user) => {
            let name_w: Vec<u16> = user.encode_utf16().chain(std::iter::once(0)).collect();
            lookup_sid_once(&name_w).is_ok()
        }
        Err(_) => false,
    };
    if probed {
        LSA_AVAILABLE.store(true, Ordering::Relaxed);
    }
    probed
}

/// Resolve a principal name to a SID via `LookupAccountNameW`.
///
/// Retries with exponential backoff (up to 3 attempts, sleeping 1s then 2s)
/// when — and only when — the LSA service has not finished initializing on a
/// freshly started EC2 instance. Because `ERROR_NONE_MAPPED` (1332) is also
/// returned for names that genuinely don't map to an account, the retry is
/// gated on [`lsa_is_available`]: a steady-state "no such user" lookup (e.g.
/// the `WindowsSessionUser::is_process_user` fallback path) fails immediately
/// rather than burning the backoff window. Any other error also fails fast.
///
/// Visibility mirrors [`set_permissions`]: declared `pub` so that integration
/// tests can reach it when the module is exposed under the `test-utils`
/// feature; without that feature the enclosing module is `pub(crate)`, so this
/// stays crate-private.
pub fn lookup_sid(principal: &str) -> Result<Vec<u8>, String> {
    let name_w: Vec<u16> = principal.encode_utf16().chain(std::iter::once(0)).collect();

    for attempt in 0..LOOKUP_MAX_RETRIES {
        match lookup_sid_once(&name_w) {
            Ok(sid) => {
                // A successful resolution proves LSA is up; latch it so future
                // ERROR_NONE_MAPPED failures skip the probe and fail fast.
                LSA_AVAILABLE.store(true, Ordering::Relaxed);
                return Ok(sid);
            }
            Err(e) => {
                let lsa_still_initializing = is_error_none_mapped(&e) && !lsa_is_available();
                if lsa_still_initializing && attempt < LOOKUP_MAX_RETRIES - 1 {
                    std::thread::sleep(std::time::Duration::from_secs(1 << attempt));
                    continue;
                }
                return Err(format!("Could not look up account '{principal}': {e}"));
            }
        }
    }

    unreachable!("loop returns on the final attempt")
}

/// Single `LookupAccountNameW` resolution (size query + populated call) with no
/// retry. `name_w` is the NUL-terminated wide form of the principal name.
fn lookup_sid_once(name_w: &[u16]) -> Result<Vec<u8>, windows::core::Error> {
    let mut sid_size: u32 = 0;
    let mut domain_size: u32 = 0;
    let mut sid_type = SID_NAME_USE::default();

    // First call to get buffer sizes. This is expected to fail with
    // ERROR_INSUFFICIENT_BUFFER; we only care about the returned sizes. A
    // genuine lookup failure (e.g. ERROR_NONE_MAPPED) surfaces here as an Err
    // that the caller inspects for retry eligibility.
    let size_query = unsafe {
        LookupAccountNameW(
            None,
            windows::core::PCWSTR(name_w.as_ptr()),
            Some(PSID(std::ptr::null_mut())),
            &mut sid_size,
            Some(windows::core::PWSTR(std::ptr::null_mut())),
            &mut domain_size,
            &mut sid_type,
        )
    };
    if sid_size == 0 {
        // No buffer size means the name was not resolved; propagate the real
        // error so the caller can decide whether it is retryable.
        return Err(size_query
            .err()
            .unwrap_or_else(windows::core::Error::from_thread));
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
        )?;
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
