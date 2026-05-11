// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Windows permission tests for TempDir and embedded files.
//!
//! Mirrors Python TestTempDirWindows and TestMaterializeFileWindows from
//! test_tempdir.py and test_embedded_files.py.
//!
//! These tests require:
//!   - Windows OS
//!   - OPENJD_TEST_WIN_USER_NAME and OPENJD_TEST_WIN_USER_PASSWORD env vars
//!     set to a valid local user (matching the Python test convention)
//!
//! Run with: cargo test -p openjd-sessions --test test_windows_permissions -- --include-ignored
#![cfg(windows)]

use openjd_sessions::embedded_files::*;
use openjd_sessions::session_user::WindowsSessionUser;
use openjd_sessions::tempdir::TempDir;
use openjd_sessions::SessionUser;
use std::sync::Arc;

use windows::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
use windows::Win32::Security::{ACL, DACL_SECURITY_INFORMATION};

/// FILE_ALL_ACCESS = 0x1F01FF (Full Control)
const FULL_CONTROL_MASK: u32 = 0x001F01FF;
/// Modify access: FILE_GENERIC_READ | FILE_GENERIC_WRITE | FILE_GENERIC_EXECUTE | DELETE | FILE_DELETE_CHILD
const MODIFY_READ_WRITE_MASK: u32 = 0x001301FF;

fn require_windows_user() -> Arc<WindowsSessionUser> {
    let user =
        std::env::var("OPENJD_TEST_WIN_USER_NAME").expect("OPENJD_TEST_WIN_USER_NAME must be set");
    let password = std::env::var("OPENJD_TEST_WIN_USER_PASSWORD")
        .expect("OPENJD_TEST_WIN_USER_PASSWORD must be set");
    Arc::new(
        WindowsSessionUser::with_password(&user, &password)
            .expect("Failed to create WindowsSessionUser — check credentials"),
    )
}

fn windows_user_name() -> String {
    std::env::var("OPENJD_TEST_WIN_USER_NAME").expect("OPENJD_TEST_WIN_USER_NAME must be set")
}

/// Get the DACL ACE masks for all principals on a path.
/// Returns Vec<(principal_name, allowed_masks, denied_masks)>.
///
/// Mirrors Python `windows_acl_helper.get_aces_for_object`: iterates raw ACEs
/// via `GetAce`, extracts the SID, resolves via `LookupAccountSidW`.
fn get_aces_for_object(path: &str) -> Vec<(String, Vec<u32>, Vec<u32>)> {
    use windows::Win32::Security::{GetAce, LookupAccountSidW, ACE_HEADER, PSID, SID_NAME_USE};

    // ACE type constants (not exported by the windows crate)
    const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
    const ACCESS_DENIED_ACE_TYPE: u8 = 1;

    let path_w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut dacl_ptr = std::ptr::null_mut::<ACL>();
    let mut sd = windows::Win32::Security::PSECURITY_DESCRIPTOR::default();

    unsafe {
        let result = GetNamedSecurityInfoW(
            windows::core::PCWSTR(path_w.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(&mut dacl_ptr),
            None,
            &mut sd,
        );
        assert!(result.is_ok(), "GetNamedSecurityInfoW failed: {result:?}");
    }

    assert!(
        !dacl_ptr.is_null(),
        "GetNamedSecurityInfoW returned null DACL pointer"
    );
    let acl_ref = unsafe { &*dacl_ptr };
    let ace_count = acl_ref.AceCount as u32;

    let mut results: Vec<(String, Vec<u32>, Vec<u32>)> = Vec::new();

    for i in 0..ace_count {
        let mut ace_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let ok = unsafe { GetAce(dacl_ptr as *const ACL, i, &mut ace_ptr) };
        assert!(ok.is_ok(), "GetAce failed for index {i}");
        assert!(
            !ace_ptr.is_null(),
            "GetAce returned null ACE pointer for index {i}"
        );

        let header = unsafe { &*(ace_ptr as *const ACE_HEADER) };
        let ace_type = header.AceType;

        // ACCESS_ALLOWED_ACE and ACCESS_DENIED_ACE have the same layout:
        //   ACE_HEADER (4 bytes) + ACCESS_MASK (4 bytes) + SidStart (variable)
        let access_mask = unsafe { *((ace_ptr as *const u8).add(4) as *const u32) };
        let sid_ptr = unsafe { (ace_ptr as *const u8).add(8) };

        // Resolve SID to account name — mirrors Python's LookupAccountSid
        let mut name_size: u32 = 0;
        let mut domain_size: u32 = 0;
        let mut sid_type = SID_NAME_USE::default();
        unsafe {
            let _ = LookupAccountSidW(
                None,
                PSID(sid_ptr as *mut _),
                Some(windows::core::PWSTR(std::ptr::null_mut())),
                &mut name_size,
                Some(windows::core::PWSTR(std::ptr::null_mut())),
                &mut domain_size,
                &mut sid_type,
            );
        }
        let mut name_buf = vec![0u16; name_size as usize];
        let mut domain_buf = vec![0u16; domain_size as usize];
        unsafe {
            let _ = LookupAccountSidW(
                None,
                PSID(sid_ptr as *mut _),
                Some(windows::core::PWSTR(name_buf.as_mut_ptr())),
                &mut name_size,
                Some(windows::core::PWSTR(domain_buf.as_mut_ptr())),
                &mut domain_size,
                &mut sid_type,
            );
        }
        let account_name = String::from_utf16_lossy(&name_buf[..name_size as usize]);

        let existing = results
            .iter_mut()
            .find(|(n, _, _)| n.eq_ignore_ascii_case(&account_name));
        match existing {
            Some((_, allowed, denied)) => {
                if ace_type == ACCESS_ALLOWED_ACE_TYPE {
                    allowed.push(access_mask);
                } else if ace_type == ACCESS_DENIED_ACE_TYPE {
                    denied.push(access_mask);
                }
            }
            None => {
                let (mut allowed, mut denied) = (Vec::new(), Vec::new());
                if ace_type == ACCESS_ALLOWED_ACE_TYPE {
                    allowed.push(access_mask);
                } else if ace_type == ACCESS_DENIED_ACE_TYPE {
                    denied.push(access_mask);
                }
                results.push((account_name, allowed, denied));
            }
        }
    }

    if !sd.0.is_null() {
        unsafe {
            windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(sd.0)));
        }
    }

    results
}

fn principal_has_access(path: &str, principal: &str, expected_mask: u32) -> bool {
    let aces = get_aces_for_object(path);
    aces.iter().any(|(name, allowed, denied)| {
        name.eq_ignore_ascii_case(principal) && allowed == &[expected_mask] && denied.is_empty()
    })
}

// === TempDir Windows tests ===

/// Mirrors Python TestTempDirWindows::test_windows_object_permissions.
/// Verifies DACL has exactly 2 ACEs: process user=FULL_CONTROL, session user=MODIFY.
#[test]
#[ignore]
fn test_tempdir_windows_object_permissions() {
    let user = require_windows_user();
    let user_name = windows_user_name();
    let process_user = openjd_sessions::win32::get_process_user().unwrap();
    // Extract bare username from DOMAIN\user or user@domain
    let process_user_bare = if process_user.contains('\\') {
        process_user.split('\\').next_back().unwrap().to_string()
    } else if process_user.contains('@') {
        process_user.split('@').next().unwrap().to_string()
    } else {
        process_user.clone()
    };

    eprintln!("process_user raw: {process_user:?}, bare: {process_user_bare:?}");
    eprintln!("session user: {user_name:?}");
    eprintln!("is_process_user: {}", user.is_process_user());

    let tempdir = TempDir::new(None, None, Some(user.as_ref() as &dyn SessionUser)).unwrap();
    let aces = get_aces_for_object(&tempdir.path().to_string_lossy());

    eprintln!("ACEs on tempdir ({}):", tempdir.path().display());
    for (name, allowed, denied) in &aces {
        eprintln!("  principal={name:?} allowed={allowed:?} denied={denied:?}");
    }

    assert_eq!(
        aces.len(),
        2,
        "Only process user & session user should have ACEs"
    );
    assert!(
        principal_has_access(
            &tempdir.path().to_string_lossy(),
            &process_user_bare,
            FULL_CONTROL_MASK
        ),
        "Process user should have Full Control"
    );
    assert!(
        principal_has_access(
            &tempdir.path().to_string_lossy(),
            &user_name,
            MODIFY_READ_WRITE_MASK
        ),
        "Session user should have Modify access"
    );
}

/// Mirrors Python TestTempDirWindows::test_windows_permissions_inherited.
/// Verifies child dirs and files inherit the session user's permissions.
#[test]
#[ignore]
fn test_tempdir_windows_permissions_inherited() {
    let user = require_windows_user();
    let user_name = windows_user_name();

    let tempdir = TempDir::new(None, None, Some(user.as_ref() as &dyn SessionUser)).unwrap();
    std::fs::create_dir(tempdir.path().join("child_dir")).unwrap();
    std::fs::create_dir(tempdir.path().join("child_dir").join("grandchild_dir")).unwrap();
    std::fs::write(tempdir.path().join("child_file"), "").unwrap();
    std::fs::write(tempdir.path().join("child_dir").join("grandchild_file"), "").unwrap();

    for sub in &[
        "child_dir",
        "child_file",
        "child_dir/grandchild_dir",
        "child_dir/grandchild_file",
    ] {
        let p = tempdir.path().join(sub);
        assert!(
            principal_has_access(&p.to_string_lossy(), &user_name, MODIFY_READ_WRITE_MASK),
            "Session user should have Modify access on {sub}"
        );
    }
}

/// Mirrors Python TestTempDirWindows::test_nonvalid_windows_principal_raises_exception.
#[test]
fn test_tempdir_windows_nonvalid_principal_raises_error() {
    // A non-existent user should cause TempDir creation to fail when it tries
    // to set DACL permissions for the invalid principal.
    // We can't call win32_permissions::set_permissions directly (pub(crate)),
    // so we test through TempDir::new which calls it internally.
    //
    // WindowsSessionUser::with_password will fail for a non-existent user,
    // so we use for_process_user and rely on the DACL step failing for a bad name.
    // Instead, test that set_permissions propagates errors by attempting to create
    // a TempDir — if the user doesn't exist, the DACL call fails.
    //
    // Since we can't easily construct a WindowsSessionUser for a non-existent user
    // (the constructor validates credentials), we verify the error path by checking
    // that WindowsSessionUser::with_password rejects a non-existent user.
    let result = WindowsSessionUser::with_password("non_existent_user_12345", "bad_password");
    assert!(
        result.is_err(),
        "Non-existent user should fail credential validation"
    );
}

// === Embedded files Windows tests ===

/// Mirrors Python TestMaterializeFileWindows::test_changes_owner.
#[test]
#[ignore]
fn test_embedded_file_windows_user_permissions() {
    let user = require_windows_user();
    let user_name = windows_user_name();

    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("testfile.txt");
    write_embedded_file_with_options(&path, "some text data", false, None).unwrap();
    chown_for_user(&path, user.as_ref(), false).unwrap();

    assert!(
        principal_has_access(&path.to_string_lossy(), &user_name, MODIFY_READ_WRITE_MASK),
        "Windows user should have Modify access on embedded file"
    );
}

// === Single-user (no cross-user) Windows DACL tests ===
//
// These mirror the POSIX `test_tempdir_posix_permissions` /
// `test_tempdir_posix_default_ownership` and
// `test_writes_file_posix_ownership` / `test_writes_file_posix_no_group_or_other_permissions`
// tests. On Windows, when no `SessionUser` is supplied, the sessions crate
// intentionally leaves DACL setup to the parent directory (the newly created
// directory inherits ACEs from its parent, and file creation inherits those
// same inheritable ACEs). These tests confirm that behavior — no *additional*
// explicit session-user ACE is injected.
//
// They do NOT require OPENJD_TEST_WIN_USER_NAME / _PASSWORD, so they run in
// the default Windows `build-test` CI job without `--include-ignored`.

/// Mirrors Python `TestTempDirPosix::test_defaults` — a TempDir created with
/// no user should only contain ACEs inherited from its parent; the crate
/// must not inject any explicit additional ACEs.
#[test]
fn test_tempdir_windows_no_user_has_only_inherited_aces() {
    // Create in a parent we control so inheritance is deterministic.
    let parent = tempfile::TempDir::new().unwrap();
    let td = TempDir::new(Some(parent.path()), None, None).unwrap();

    let parent_aces = get_aces_for_object(&parent.path().to_string_lossy());
    let child_aces = get_aces_for_object(&td.path().to_string_lossy());

    // Every ACE on the child must appear on the parent (purely inherited).
    // This confirms the crate did not add any principal-specific ACEs.
    for (child_name, child_allowed, child_denied) in &child_aces {
        let matching = parent_aces
            .iter()
            .find(|(n, _, _)| n.eq_ignore_ascii_case(child_name));
        match matching {
            Some((_, parent_allowed, parent_denied)) => {
                for mask in child_allowed {
                    assert!(
                        parent_allowed.contains(mask),
                        "child has allowed mask 0x{mask:X} for {child_name:?} not present on parent"
                    );
                }
                for mask in child_denied {
                    assert!(
                        parent_denied.contains(mask),
                        "child has denied mask 0x{mask:X} for {child_name:?} not present on parent"
                    );
                }
            }
            None => panic!(
                "child tempdir has an ACE for {child_name:?} that is not present on the parent — \
                 single-user TempDir must only rely on inheritance, got child aces: {child_aces:?}"
            ),
        }
    }
}

/// Mirrors Python `TestMaterializeFilePosix::test_writes_file` (owner-only
/// ACE assertions) — an embedded file written with no user should only have
/// ACEs inherited from its parent directory.
#[test]
fn test_embedded_file_windows_no_user_has_only_inherited_aces() {
    let parent = tempfile::TempDir::new().unwrap();
    let path = parent.path().join("testfile.txt");
    write_embedded_file_with_options(&path, "data", false, None).unwrap();

    let parent_aces = get_aces_for_object(&parent.path().to_string_lossy());
    let file_aces = get_aces_for_object(&path.to_string_lossy());

    for (name, allowed, denied) in &file_aces {
        let matching = parent_aces
            .iter()
            .find(|(n, _, _)| n.eq_ignore_ascii_case(name));
        assert!(
            matching.is_some(),
            "embedded file has an ACE for {name:?} not present on parent — \
             no-user path must rely purely on inheritance, got file aces: {file_aces:?}"
        );
        let (_, parent_allowed, parent_denied) = matching.unwrap();
        for mask in allowed {
            assert!(
                parent_allowed.contains(mask),
                "embedded file has allowed mask 0x{mask:X} for {name:?} not present on parent"
            );
        }
        for mask in denied {
            assert!(
                parent_denied.contains(mask),
                "embedded file has denied mask 0x{mask:X} for {name:?} not present on parent"
            );
        }
    }
}

/// Runnable embedded file with no user — identical DACL story to the
/// non-runnable case on Windows. Mirrors POSIX
/// `test_writes_file_runnable_posix_ownership_and_permissions`.
#[test]
fn test_embedded_file_windows_runnable_no_user_has_only_inherited_aces() {
    let parent = tempfile::TempDir::new().unwrap();
    let path = parent.path().join("testfile.bat");
    write_embedded_file_with_options(&path, "@echo test", true, None).unwrap();

    let parent_aces = get_aces_for_object(&parent.path().to_string_lossy());
    let file_aces = get_aces_for_object(&path.to_string_lossy());

    for (name, allowed, _denied) in &file_aces {
        let matching = parent_aces
            .iter()
            .find(|(n, _, _)| n.eq_ignore_ascii_case(name));
        assert!(
            matching.is_some(),
            "runnable embedded file has an ACE for {name:?} not present on parent, got file aces: {file_aces:?}"
        );
        for mask in allowed {
            assert!(
                matching.unwrap().1.contains(mask),
                "runnable embedded file has allowed mask 0x{mask:X} for {name:?} not present on parent"
            );
        }
    }
}
