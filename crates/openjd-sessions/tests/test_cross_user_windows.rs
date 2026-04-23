// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

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
        collect_stdout: true,
    };
    Session::with_config(config).unwrap()
}

// ── Helper-level tests ──
// These spawn CrossUserHelperWin directly via the helper binary protocol,
// driving it the same way the Linux tests drive CrossUserHelper via sudo.

fn helper_path() -> PathBuf {
    let binary = "openjd_helper.exe";
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/helper/target/release")
        .join(binary);
    if !p.exists() {
        panic!(
            "Helper binary not found at {}. Build it first.",
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

/// TempDir cleanup works when the directory has cross-user ACLs.
/// Mirrors: Linux test_cross_user_tempdir_cleanup, Python test_tempdir_cleanup (Windows)
#[test]
#[ignore]
fn test_cross_user_tempdir_cleanup() {
    let user = require_windows_user();
    let mut td = openjd_sessions::tempdir::TempDir::new(None, None, Some(&*user)).unwrap();
    let testfile = td.path().join("testfile.txt");
    std::fs::write(&testfile, "test content").unwrap();
    assert!(testfile.exists());
    td.cleanup().unwrap();
    assert!(
        !td.path().exists(),
        "TempDir should be deleted after cleanup"
    );
}

// === Cross-user embedded file permissions ===

/// Embedded file with cross-user: session user gets modify access via ACL.
/// Mirrors: Linux test_cross_user_embedded_file_permissions, Python TestMaterializeFileWindows::test_changes_owner
#[test]
#[ignore]
fn test_cross_user_embedded_file_permissions() {
    let user = require_windows_user();
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
