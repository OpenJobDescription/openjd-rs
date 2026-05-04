// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Cross-user Windows helper tests for openjd-sessions.
//!
//! All tests are `#[ignore]` — they require a Windows test user with:
//!   OPENJD_TEST_WIN_USER_NAME / OPENJD_TEST_WIN_USER_PASSWORD
//!
//! Run with: cargo test -p openjd-sessions --features test-utils --test test_cross_user_windows -- --include-ignored --test-threads=1
//!
//! These tests mirror the Linux cross-user tests in `test_cross_user.rs` and
//! the Python `TestLoggingSubprocessWindowsCrossUser` tests.
//!
//! IMPORTANT: These tests MUST run with --test-threads=1 because
//! CreateProcessWithLogonW fails with ERROR_SERVICE_ALREADY_RUNNING (0x80070420)
//! when multiple concurrent logons for the same user are attempted.
#![cfg(windows)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use openjd_sessions::action::ActionState;
use openjd_sessions::session::{Session, SessionConfig};
use openjd_sessions::session_user::WindowsSessionUser;
use openjd_sessions::SessionUser;

// ---------------------------------------------------------------------------
// DACL inspection helpers (mirror Python utils/windows_acl_helper.py).
//
// Duplicated here (and in tests/test_windows_permissions.rs) because Rust
// integration tests are each their own crate — there is no convenient
// cross-file shared module without a `tests/support/` mod that every test
// file must `mod support;` into. The helpers are small and stable.
// ---------------------------------------------------------------------------

/// FILE_ALL_ACCESS = 0x1F01FF — Full Control.
const FULL_CONTROL_MASK: u32 = 0x001F01FF;
/// Modify access: FILE_GENERIC_READ | FILE_GENERIC_WRITE | FILE_GENERIC_EXECUTE | DELETE | FILE_DELETE_CHILD.
const MODIFY_READ_WRITE_MASK: u32 = 0x001301FF;

/// Return all ACEs on a path: `Vec<(principal_name, allowed_masks, denied_masks)>`.
///
/// Mirrors Python `windows_acl_helper.get_aces_for_object`.
fn get_aces_for_object(path: &str) -> Vec<(String, Vec<u32>, Vec<u32>)> {
    use windows::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows::Win32::Security::{
        GetAce, LookupAccountSidW, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, PSID, SID_NAME_USE,
    };

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

    let acl_ref = unsafe { &*dacl_ptr };
    let ace_count = acl_ref.AceCount as u32;
    let mut results: Vec<(String, Vec<u32>, Vec<u32>)> = Vec::new();

    for i in 0..ace_count {
        let mut ace_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let ok = unsafe { GetAce(dacl_ptr as *const ACL, i, &mut ace_ptr) };
        assert!(ok.is_ok(), "GetAce failed for index {i}");

        let header = unsafe { &*(ace_ptr as *const ACE_HEADER) };
        let ace_type = header.AceType;
        let access_mask = unsafe { *((ace_ptr as *const u8).add(4) as *const u32) };
        let sid_ptr = unsafe { (ace_ptr as *const u8).add(8) };

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

/// Mirrors Python `windows_acl_helper.principal_has_access_to_object`: true iff
/// the principal has exactly one allowed ACE with `expected_mask` and no denied ACEs.
fn principal_has_access(path: &str, principal: &str, expected_mask: u32) -> bool {
    let aces = get_aces_for_object(path);
    aces.iter().any(|(name, allowed, denied)| {
        name.eq_ignore_ascii_case(principal) && allowed == &[expected_mask] && denied.is_empty()
    })
}

/// Return the process user's bare username (without DOMAIN\ or @domain).
fn process_user_bare() -> String {
    let process_user = openjd_sessions::win32::get_process_user().unwrap();
    if process_user.contains('\\') {
        process_user.split('\\').next_back().unwrap().to_string()
    } else if process_user.contains('@') {
        process_user.split('@').next().unwrap().to_string()
    } else {
        process_user
    }
}

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

fn make_session(user: Arc<WindowsSessionUser>) -> Session {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    std::mem::forget(tmp);
    let config = SessionConfig {
        session_id: "cross-user-win-test".into(),
        job_parameter_values: HashMap::new(),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: None,
        os_env_vars: None,
        session_root_directory: Some(root),
        user: Some(user),
        revision_extensions: None,
        cancel_token: None,
        debug_collect_stdout: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    };
    Session::with_config(config).unwrap()
}

// ── Helper-level tests ──
// These spawn CrossUserHelperWin directly via the helper binary protocol,
// driving it the same way the Linux tests drive CrossUserHelper via sudo.

fn helper_path() -> PathBuf {
    let p = PathBuf::from(env!("OPENJD_HELPER_BINARY_PATH"));
    if !p.exists() {
        panic!(
            "Helper binary not found at {}. Build openjd-sessions first.",
            p.display()
        );
    }
    p
}

/// Spawn the helper as the test user via `runas`-style CreateProcessWithLogonW.
/// We reuse the session's `spawn_as_user_with_stdin` to launch the helper binary
/// as the target user, then communicate over stdin/stdout JSON protocol.
struct CrossUserTestHelper {
    process_handle: windows::Win32::Foundation::HANDLE,
    stdin: std::io::BufWriter<std::fs::File>,
    reader: BufReader<std::fs::File>,
}

impl CrossUserTestHelper {
    fn spawn(user: &WindowsSessionUser) -> Self {
        let spawned = openjd_sessions::win32::spawn_as_user_with_stdin(
            &[helper_path().to_string_lossy().to_string()],
            &HashMap::new(),
            None,
            user.password(),
            user.user(),
            user.logon_token(),
        )
        .expect("Failed to spawn helper as test user");

        let stdin_file: std::fs::File = spawned.stdin_write.expect("stdin pipe").into();
        let stdout_file: std::fs::File = spawned.stdout_read.into();

        Self {
            process_handle: spawned.process_handle,
            stdin: std::io::BufWriter::new(stdin_file),
            reader: BufReader::new(stdout_file),
        }
    }

    fn send(&mut self, msg: &str) {
        writeln!(self.stdin, "{msg}").expect("write to helper stdin");
        self.stdin.flush().expect("flush helper stdin");
    }

    fn send_run(&mut self, command: &str, args: &[&str], env: &serde_json::Value) {
        let cmd = serde_json::json!({
            "command": command,
            "args": args,
            "env": env,
            "cwd": cwd(),
        });
        let msg = serde_json::to_string(&cmd).unwrap();
        self.send(&msg);
    }

    fn read_line(&mut self) -> serde_json::Value {
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .expect("read from helper stdout");
        serde_json::from_str(line.trim()).expect("parse helper response JSON")
    }

    fn read_until_done(&mut self) -> Vec<serde_json::Value> {
        let mut responses = Vec::new();
        loop {
            let v = self.read_line();
            let done = v.get("exited").is_some() || v.get("error").is_some();
            responses.push(v);
            if done {
                break;
            }
        }
        responses
    }

    fn shutdown(mut self) {
        self.send("\"shutdown\"");
        unsafe {
            let _ =
                windows::Win32::System::Threading::WaitForSingleObject(self.process_handle, 5000);
            let _ = windows::Win32::Foundation::CloseHandle(self.process_handle);
        }
    }
}

/// Use C:\Windows\Temp as the cwd for helper commands — it's universally
/// accessible, unlike the current user's temp dir which the test user may
/// not have permission to access.
fn cwd() -> String {
    r"C:\Windows\Temp".to_string()
}

// === Cross-user helper: identity ===

/// Run `whoami` as the target user — verify stdout contains the target username.
/// Mirrors: Linux test_cross_user_subprocess_basic, Python test_basic_operation_success
#[test]
#[ignore]
fn test_cross_user_helper_whoami() {
    let user = require_windows_user();
    let user_name = windows_user_name();
    let mut h = CrossUserTestHelper::spawn(&user);

    h.send_run("whoami", &[], &serde_json::json!({}));
    let resp = h.read_until_done();

    assert!(
        resp.iter().any(|v| v
            .get("out")
            .and_then(|o| o.as_str())
            .is_some_and(|s| s.to_lowercase().contains(&user_name.to_lowercase()))),
        "Expected '{}' in output, got: {resp:?}",
        user_name
    );
    assert_eq!(resp.last().unwrap()["exited"], 0, "whoami should exit 0");

    h.shutdown();
}

// === Cross-user helper: exit code ===

/// Verify non-zero exit codes propagate through the helper.
/// Mirrors: Python test_basic_operation_failure
#[test]
#[ignore]
fn test_cross_user_helper_exit_code() {
    let user = require_windows_user();
    let mut h = CrossUserTestHelper::spawn(&user);

    h.send_run("cmd", &["/c", "exit 42"], &serde_json::json!({}));
    let resp = h.read_until_done();
    assert_eq!(
        resp.last().unwrap()["exited"],
        42,
        "should get exit code 42, got: {resp:?}"
    );

    h.shutdown();
}

// === Cross-user helper: env vars ===

/// Verify env vars are propagated to the cross-user subprocess.
/// Mirrors: Linux test_cross_user_runner_env_vars
#[test]
#[ignore]
fn test_cross_user_helper_env_vars() {
    let user = require_windows_user();
    let mut h = CrossUserTestHelper::spawn(&user);

    h.send_run(
        "cmd",
        &["/c", "echo %OPENJD_TEST_VAR%"],
        &serde_json::json!({"OPENJD_TEST_VAR": "test_value_123"}),
    );
    let resp = h.read_until_done();
    assert!(
        resp.iter().any(|v| v
            .get("out")
            .and_then(|o| o.as_str())
            .is_some_and(|s| s.contains("test_value_123"))),
        "Env var not found in output: {resp:?}"
    );
    assert_eq!(resp.last().unwrap()["exited"], 0);

    h.shutdown();
}

// === Cross-user helper: no env inheritance ===

/// Verify host-process env vars do NOT leak into the cross-user subprocess.
/// Mirrors: Linux test_cross_user_no_env_inheritance, Python test_does_not_inherit_env_vars_windows
#[test]
#[ignore]
fn test_cross_user_helper_no_env_inheritance() {
    let unique_var = format!("OPENJD_UNIQUE_{}", std::process::id());
    unsafe { std::env::set_var(&unique_var, "should_not_appear") };

    let user = require_windows_user();
    let mut h = CrossUserTestHelper::spawn(&user);

    h.send_run("cmd", &["/c", "set"], &serde_json::json!({}));
    let resp = h.read_until_done();
    assert!(
        !resp.iter().any(|v| v
            .get("out")
            .and_then(|o| o.as_str())
            .is_some_and(|s| s.contains(&unique_var))),
        "Host env var leaked: {}",
        unique_var
    );

    unsafe { std::env::remove_var(&unique_var) };
    h.shutdown();
}

// === Cross-user helper: TERMINATE cancellation ===

/// Send TERMINATE cancel — process should die quickly.
/// Mirrors: Linux test_cross_user_subprocess_terminate, Python test_terminate_ends_process
#[test]
#[ignore]
fn test_cross_user_helper_terminate() {
    let user = require_windows_user();
    let mut h = CrossUserTestHelper::spawn(&user);

    h.send_run("ping", &["-n", "31", "127.0.0.1"], &serde_json::json!({}));
    let pid_resp = h.read_line();
    assert!(
        pid_resp.get("pid").is_some(),
        "should get pid, got: {pid_resp:?}"
    );

    std::thread::sleep(Duration::from_millis(500));

    let start = std::time::Instant::now();
    h.send(r#"{"cancel": "TERMINATE"}"#);
    let resp = h.read_until_done();
    let elapsed = start.elapsed();

    let last = resp.last().unwrap();
    assert!(last.get("exited").is_some(), "got: {resp:?}");
    assert!(
        elapsed < Duration::from_secs(10),
        "TERMINATE should kill quickly, took {elapsed:?}"
    );

    h.shutdown();
}

// === Cross-user helper: NOTIFY_THEN_TERMINATE cancellation ===

/// Send NOTIFY_THEN_TERMINATE — process should exit within grace period.
/// Mirrors: Linux test_cross_user_subprocess_notify, Python test_notify_ends_process
#[test]
#[ignore]
fn test_cross_user_helper_notify_then_terminate() {
    let user = require_windows_user();
    let mut h = CrossUserTestHelper::spawn(&user);

    h.send_run("ping", &["-n", "31", "127.0.0.1"], &serde_json::json!({}));
    let pid_resp = h.read_line();
    assert!(
        pid_resp.get("pid").is_some(),
        "should get pid, got: {pid_resp:?}"
    );

    std::thread::sleep(Duration::from_millis(500));

    let start = std::time::Instant::now();
    h.send(r#"{"cancel": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": 5}"#);
    let resp = h.read_until_done();
    let elapsed = start.elapsed();

    let last = resp.last().unwrap();
    assert!(last.get("exited").is_some(), "got: {resp:?}");
    assert!(
        elapsed < Duration::from_secs(10),
        "cancel should finish within the grace window, took {elapsed:?}"
    );

    h.shutdown();
}

// === Cross-user helper: command not found ===

/// Nonexistent command should return an error response.
/// Mirrors: Python test_failed_run_as_windows_user
#[test]
#[ignore]
fn test_cross_user_helper_command_not_found() {
    let user = require_windows_user();
    let mut h = CrossUserTestHelper::spawn(&user);

    h.send_run("nonexistent_binary_xyz_12345", &[], &serde_json::json!({}));
    let resp = h.read_until_done();
    assert!(
        resp.last().unwrap().get("error").is_some(),
        "should get error for nonexistent binary, got: {resp:?}"
    );

    h.shutdown();
}

// === Session-level: run_subprocess as Windows user ===

/// Verify Session::run_subprocess runs as the configured target user.
/// Mirrors: Linux test_cross_user_session_run_subprocess
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_session_run_subprocess() {
    let user = require_windows_user();
    let user_name = windows_user_name();
    let mut session = make_session(user);
    let r = session
        .run_subprocess("whoami", None, None, None, true, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    assert!(
        r.stdout.to_lowercase().contains(&user_name.to_lowercase()),
        "Expected '{}' in stdout: {}",
        user_name,
        r.stdout
    );
    session.cleanup();
}

// === Session-level: cleanup with cross-user files ===

/// Session cleanup deletes the working directory even when files were created
/// by the target user (the DACL grants the process user Full Control).
/// Mirrors: Linux test_cross_user_session_cleanup
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_session_cleanup() {
    let user = require_windows_user();
    let mut session = make_session(user.clone());
    let working_dir = session.working_directory().to_path_buf();

    // Create a file as the target user via the session's helper
    let r = session
        .run_subprocess(
            "cmd",
            Some(&[
                "/c".into(),
                format!("echo test > {}", working_dir.join("testfile.txt").display()),
            ]),
            None,
            None,
            true,
            None,
        )
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    assert!(working_dir.join("testfile.txt").exists());

    session.cleanup();
    assert!(!working_dir.exists(), "Working directory should be deleted");
}

// === Cross-user helper: process tree termination ===

/// Terminate a process that has spawned children — both parent and children should die.
/// Mirrors: Linux test_cross_user_subprocess_terminate_tree, Python test_terminate_ends_process_tree
#[test]
#[ignore]
fn test_cross_user_helper_terminate_tree() {
    let user = require_windows_user();
    let mut h = CrossUserTestHelper::spawn(&user);

    // Use cmd /c to spawn a child: "start /b ping ..." runs a background child
    h.send_run(
        "cmd",
        &[
            "/c",
            "start /b ping -n 31 127.0.0.1 >nul & ping -n 31 127.0.0.1",
        ],
        &serde_json::json!({}),
    );
    let pid_resp = h.read_line();
    assert!(
        pid_resp.get("pid").is_some(),
        "should get pid, got: {pid_resp:?}"
    );

    std::thread::sleep(Duration::from_secs(1));

    let start = std::time::Instant::now();
    h.send(r#"{"cancel": "TERMINATE"}"#);
    let resp = h.read_until_done();
    let elapsed = start.elapsed();

    let last = resp.last().unwrap();
    assert!(last.get("exited").is_some(), "got: {resp:?}");
    assert!(
        elapsed < Duration::from_secs(10),
        "TERMINATE should kill tree quickly, took {elapsed:?}"
    );

    h.shutdown();
}

// === Cross-user helper: env var casing ===

/// Verify env vars passed to the cross-user subprocess are uppercased on Windows.
/// Mirrors: Python test_environment_casing
#[test]
#[ignore]
fn test_cross_user_helper_env_casing() {
    let user = require_windows_user();
    let mut h = CrossUserTestHelper::spawn(&user);

    // Pass a lowercase env var — the helper should uppercase it
    h.send_run(
        "cmd",
        &["/c", "echo %TESTLOWER%"],
        &serde_json::json!({"testlower": "lower_value"}),
    );
    let resp = h.read_until_done();
    assert!(
        resp.iter().any(|v| v
            .get("out")
            .and_then(|o| o.as_str())
            .is_some_and(|s| s.contains("lower_value"))),
        "Env var value not found in output: {resp:?}"
    );
    assert_eq!(resp.last().unwrap()["exited"], 0);

    h.shutdown();
}

// === Cross-user TempDir cleanup ===

/// TempDir cleanup works when a file inside has a DACL that excludes the process user.
///
/// Mirrors Python `TestTempDirWindows::test_cleanup` — after creating a file
/// inside the tempdir, the file's DACL is tightened so only the session user has
/// an ACE. The tempdir's parent DACL still grants the process user
/// `FILE_DELETE_CHILD`, so cleanup succeeds.
#[test]
#[ignore]
fn test_cross_user_tempdir_cleanup() {
    let user = require_windows_user();
    let user_name = windows_user_name();

    let mut td = openjd_sessions::tempdir::TempDir::new(None, None, Some(&*user)).unwrap();
    let testfile = td.path().join("testfile.txt");
    std::fs::write(&testfile, "test content").unwrap();
    assert!(testfile.exists());

    // Reset the file's DACL to only grant session_user Full Control, and
    // disable inheritance from the parent. After this, the process user has
    // no ACE on the file itself — but the parent tempdir's DACL still grants
    // the process user Full Control (including `FILE_DELETE_CHILD`), so
    // cleanup must still succeed.
    //
    // Mirrors Python:
    //   WindowsPermissionHelper.set_permissions(
    //       testfilename, principals_full_control=[windows_user.user])
    let status = std::process::Command::new("icacls")
        .args([
            testfile.to_string_lossy().as_ref(),
            "/inheritance:r",
            "/grant",
            &format!("{user_name}:(F)"),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("icacls invocation failed");
    assert!(status.success(), "icacls should succeed to tighten DACL");

    // Sanity check: session user has Full Control, process user has no ACE.
    let aces = get_aces_for_object(&testfile.to_string_lossy());
    assert!(
        principal_has_access(&testfile.to_string_lossy(), &user_name, FULL_CONTROL_MASK),
        "Session user should have Full Control after DACL tightening, got aces: {aces:?}"
    );
    assert!(
        !aces
            .iter()
            .any(|(n, _, _)| n.eq_ignore_ascii_case(&process_user_bare())),
        "Process user should not appear in the file DACL after /inheritance:r, got aces: {aces:?}"
    );

    // Cleanup should still succeed via the parent dir's FILE_DELETE_CHILD right.
    td.cleanup().unwrap();
    assert!(
        !td.path().exists(),
        "TempDir should be deleted after cleanup"
    );
}

// === Cross-user embedded file permissions ===

/// Embedded file with cross-user: process user gets Full Control, session user gets Modify.
/// Mirrors: Linux test_cross_user_embedded_file_permissions, Python TestMaterializeFileWindows::test_changes_owner
#[test]
#[ignore]
fn test_cross_user_embedded_file_permissions() {
    let user = require_windows_user();
    let user_name = windows_user_name();
    let proc_user = process_user_bare();

    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("testfile.txt");

    openjd_sessions::embedded_files::write_embedded_file_with_options(
        &path,
        "some text data",
        false,
        None,
    )
    .unwrap();
    openjd_sessions::embedded_files::chown_for_user(&path, &*user, false).unwrap();

    assert!(path.exists());
    let contents = std::fs::read_to_string(&path).unwrap();
    assert_eq!(contents, "some text data");

    // Mirrors Python's chown/DACL assertion: process user has Full Control,
    // session user has Modify, and these are the only ACEs.
    let aces = get_aces_for_object(&path.to_string_lossy());
    assert_eq!(
        aces.len(),
        2,
        "Only process user & session user should have ACEs, got {aces:?}"
    );
    assert!(
        principal_has_access(&path.to_string_lossy(), &proc_user, FULL_CONTROL_MASK),
        "Process user should have Full Control on embedded file"
    );
    assert!(
        principal_has_access(&path.to_string_lossy(), &user_name, MODIFY_READ_WRITE_MASK),
        "Session user should have Modify access on embedded file"
    );
}

/// Runnable embedded file with cross-user — DACL is identical to non-runnable
/// because Windows file-system permissions don't include an "execute" bit
/// analogous to POSIX `+x`.
///
/// This test mirrors Linux `test_cross_user_embedded_file_runnable_permissions`
/// (which checks 0o770 vs 0o660) by verifying the same DACL is applied when
/// `runnable=true`.
#[test]
#[ignore]
fn test_cross_user_embedded_file_runnable_permissions() {
    let user = require_windows_user();
    let user_name = windows_user_name();
    let proc_user = process_user_bare();

    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("testfile.bat");

    openjd_sessions::embedded_files::write_embedded_file_with_options(
        &path,
        "@echo test",
        true, // runnable
        None,
    )
    .unwrap();
    openjd_sessions::embedded_files::chown_for_user(&path, &*user, true).unwrap();

    assert!(path.exists());

    let aces = get_aces_for_object(&path.to_string_lossy());
    assert_eq!(
        aces.len(),
        2,
        "Only process user & session user should have ACEs, got {aces:?}"
    );
    assert!(
        principal_has_access(&path.to_string_lossy(), &proc_user, FULL_CONTROL_MASK),
        "Process user should have Full Control on runnable embedded file"
    );
    assert!(
        principal_has_access(&path.to_string_lossy(), &user_name, MODIFY_READ_WRITE_MASK),
        "Session user should have Modify access on runnable embedded file"
    );
}

// === Session-level: run embedded script from session dir ===

/// Create a bat file in the session working dir and run it as the cross-user.
/// Mirrors: Python test_run_file_in_session_dir_as_windows_user
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_session_run_embedded_script() {
    let user = require_windows_user();
    let mut session = make_session(user);
    let working_dir = session.working_directory().to_path_buf();

    // Write a bat file into the working directory
    let bat_path = working_dir.join("test.bat");
    std::fs::write(&bat_path, "@echo Hello from bat").unwrap();

    let r = session
        .run_subprocess(&bat_path.to_string_lossy(), None, None, None, true, None)
        .await
        .unwrap();
    assert_eq!(
        r.state,
        ActionState::Success,
        "bat script should succeed, got: {:?} stdout: {}",
        r.state,
        r.stdout
    );
    assert!(
        r.stdout.contains("Hello from bat"),
        "Expected 'Hello from bat' in stdout: {}",
        r.stdout
    );
    session.cleanup();
}

// === Cross-user helper binary DACL protection ===
//
// The session's `.helpers-<uuid>/` directory and the helper binary inside
// it must be protected from tampering by the job user. Even though the
// parent session working directory's DACL grants the session user Modify
// (so actions can write files into the working dir), the helper binary
// must be read-only for the session user — otherwise a malicious job
// could overwrite the helper binary to escalate.
//
// We verify this by:
//   1. Locating the `.helpers-*` subdirectory inside the session working dir.
//   2. Asserting both the directory and the helper binary have an explicit
//      DACL with process user Full Control and session user Read & Execute
//      (mask 0x1200A9 = FILE_GENERIC_READ | FILE_GENERIC_EXECUTE).

/// Read & Execute mask: FILE_GENERIC_READ (0x120089) | FILE_GENERIC_EXECUTE (0x1200A0).
const READ_EXECUTE_MASK: u32 = 0x001200A9;

/// Find the `.helpers-<uuid>` subdirectory under `working_dir`.
///
/// Panics if exactly one isn't found — `create_helpers_dir` always creates
/// one per cross-user session.
fn find_helpers_dir(working_dir: &std::path::Path) -> std::path::PathBuf {
    let mut matches = Vec::new();
    for entry in std::fs::read_dir(working_dir).expect("read session working dir") {
        let entry = entry.expect("read dir entry");
        let name = entry.file_name();
        if let Some(s) = name.to_str() {
            if s.starts_with(".helpers-") {
                matches.push(entry.path());
            }
        }
    }
    assert_eq!(
        matches.len(),
        1,
        "Expected exactly one .helpers-<uuid> directory under {}, found: {matches:?}",
        working_dir.display()
    );
    matches.remove(0)
}

/// The helpers directory must grant the session user Read & Execute only —
/// no write, no delete. This prevents the job user from replacing the
/// helper binary or writing auxiliary files that would be picked up by
/// subsequent execution.
#[test]
#[ignore]
fn test_cross_user_helpers_dir_dacl_protects_from_session_user() {
    let user = require_windows_user();
    let user_name = windows_user_name();
    let proc_user = process_user_bare();

    let mut session = make_session(user);
    let working_dir = session.working_directory().to_path_buf();
    let helpers_dir = find_helpers_dir(&working_dir);

    let aces = get_aces_for_object(&helpers_dir.to_string_lossy());
    assert_eq!(
        aces.len(),
        2,
        "Only process user & session user should have ACEs on helpers dir, got {aces:?}"
    );
    assert!(
        principal_has_access(
            &helpers_dir.to_string_lossy(),
            &proc_user,
            FULL_CONTROL_MASK
        ),
        "Process user should have Full Control on helpers dir, got aces: {aces:?}"
    );
    assert!(
        principal_has_access(
            &helpers_dir.to_string_lossy(),
            &user_name,
            READ_EXECUTE_MASK
        ),
        "Session user should have Read & Execute only on helpers dir (mask 0x{READ_EXECUTE_MASK:X}), got aces: {aces:?}"
    );

    session.cleanup();
}

/// The helper binary itself must grant the session user Read & Execute
/// only — even if the parent dir's DACL is ever weakened, the binary
/// cannot be overwritten.
#[test]
#[ignore]
fn test_cross_user_helper_binary_dacl_protects_from_session_user() {
    let user = require_windows_user();
    let user_name = windows_user_name();
    let proc_user = process_user_bare();

    let mut session = make_session(user);
    let working_dir = session.working_directory().to_path_buf();
    let helpers_dir = find_helpers_dir(&working_dir);

    // The helper binary is the sole `.exe` in the helpers dir.
    let helper_exe = std::fs::read_dir(&helpers_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("exe"))
        })
        .expect("helper .exe in helpers dir");

    let aces = get_aces_for_object(&helper_exe.to_string_lossy());
    assert_eq!(
        aces.len(),
        2,
        "Only process user & session user should have ACEs on helper binary, got {aces:?}"
    );
    assert!(
        principal_has_access(&helper_exe.to_string_lossy(), &proc_user, FULL_CONTROL_MASK),
        "Process user should have Full Control on helper binary, got aces: {aces:?}"
    );
    assert!(
        principal_has_access(
            &helper_exe.to_string_lossy(),
            &user_name,
            READ_EXECUTE_MASK
        ),
        "Session user should have Read & Execute only on helper binary (mask 0x{READ_EXECUTE_MASK:X}), got aces: {aces:?}"
    );

    session.cleanup();
}

/// End-to-end functional check: attempting to overwrite the helper binary
/// as the session user must fail with access-denied, even though the
/// session user has Modify on the session working directory.
///
/// This drives the helper — already running as the session user — to try
/// to open the helper binary with write access. `OpenFile`/`Write-File`
/// in PowerShell returns a `UnauthorizedAccessException` if the DACL
/// denies write, which we detect by the non-zero exit code.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_session_user_cannot_overwrite_helper_binary() {
    let user = require_windows_user();
    let mut session = make_session(user);
    let working_dir = session.working_directory().to_path_buf();
    let helpers_dir = find_helpers_dir(&working_dir);
    let helper_exe = std::fs::read_dir(&helpers_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("exe"))
        })
        .expect("helper .exe in helpers dir");

    // Run as the session user: open the helper binary for write. PowerShell's
    // `Set-Content` surfaces access-denied as a non-zero exit code when
    // invoked with `-ErrorAction Stop`.
    let ps_cmd = format!(
        "Set-Content -Path '{}' -Value 'tampered' -ErrorAction Stop",
        helper_exe.display()
    );
    let r = session
        .run_subprocess(
            "powershell",
            Some(&["-NoProfile".into(), "-Command".into(), ps_cmd]),
            None,
            None,
            true,
            None,
        )
        .await
        .unwrap();
    assert_ne!(
        r.state,
        ActionState::Success,
        "Session user must NOT be able to overwrite the helper binary. \
         state={:?} stdout={} stderr hint in stdout (merged)",
        r.state,
        r.stdout
    );

    // Helper binary must still be intact — same size as originally written.
    // Compare against the authoritative source's file size.
    let host_reference = std::path::PathBuf::from(env!("OPENJD_HELPER_BINARY_PATH"));
    let host_size = std::fs::metadata(&host_reference).unwrap().len();
    let disk_size = std::fs::metadata(&helper_exe).unwrap().len();
    assert_eq!(
        host_size, disk_size,
        "Helper binary on disk has been tampered with! (expected {host_size} bytes, got {disk_size})"
    );

    session.cleanup();
}
