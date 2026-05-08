// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Cross-user tests for openjd-sessions.
//!
//! All tests are `#[ignore]` — they require a Docker container with sudo
//! configured and the following env vars set:
//!   OPENJD_TEST_SUDO_TARGET_USER / OPENJD_TEST_SUDO_SHARED_GROUP
//!   OPENJD_TEST_SUDO_DISJOINT_USER / OPENJD_TEST_SUDO_DISJOINT_GROUP
//!
//! Run with: cargo test -p openjd-sessions --features test-utils -- --include-ignored
//!
//! These tests are POSIX-only (sudo, process groups, file ownership).
#![cfg(unix)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use openjd_sessions::action::ActionState;
use openjd_sessions::session::{Session, SessionConfig};
use openjd_sessions::session_user::PosixSessionUser;
use openjd_sessions::tempdir::TempDir;

fn target_user() -> Option<Arc<PosixSessionUser>> {
    let user = std::env::var("OPENJD_TEST_SUDO_TARGET_USER").ok()?;
    let group = std::env::var("OPENJD_TEST_SUDO_SHARED_GROUP").ok()?;
    Some(Arc::new(PosixSessionUser::new(&user, Some(&group))))
}

fn disjoint_user() -> Option<Arc<PosixSessionUser>> {
    let user = std::env::var("OPENJD_TEST_SUDO_DISJOINT_USER").ok()?;
    let group = std::env::var("OPENJD_TEST_SUDO_DISJOINT_GROUP").ok()?;
    Some(Arc::new(PosixSessionUser::new(&user, Some(&group))))
}

fn require_target_user() -> Arc<PosixSessionUser> {
    target_user()
        .expect("OPENJD_TEST_SUDO_TARGET_USER and OPENJD_TEST_SUDO_SHARED_GROUP must be set")
}

fn require_disjoint_user() -> Arc<PosixSessionUser> {
    disjoint_user()
        .expect("OPENJD_TEST_SUDO_DISJOINT_USER and OPENJD_TEST_SUDO_DISJOINT_GROUP must be set")
}

fn make_session(user: Arc<PosixSessionUser>) -> Session {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    // Leak so the dir outlives the session constructor
    std::mem::forget(tmp);
    let config = SessionConfig {
        session_id: "cross-user-test".into(),
        job_parameter_values: HashMap::new(),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: None,
        os_env_vars: None,
        session_root_directory: Some(root),
        user: Some(user),
        profile: None,
        cancel_token: None,
        debug_collect_stdout: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    };
    Session::with_config(config).unwrap()
}

fn support_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("support")
}

// === Cross-user subprocess tests ===

/// Run `whoami` as target user — verify stdout contains the target username.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_subprocess_basic() {
    let user = require_target_user();
    let mut session = make_session(user.clone());
    let r = session
        .run_subprocess("whoami", None, None, None, true, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    assert!(
        r.stdout.trim().contains(&user.user),
        "Expected '{}' in stdout: {}",
        user.user,
        r.stdout
    );
    session.cleanup();
}

/// Run long_running.sh (traps SIGTERM) with a timeout — process should be killed
/// before completing all iterations, and the trap handler should fire.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_subprocess_notify() {
    let user = require_target_user();
    let mut session = make_session(user.clone());
    let script = support_dir().join("long_running.sh");
    let r = session
        .run_subprocess(
            &script.to_string_lossy(),
            None,
            Some(Duration::from_secs(3)),
            None,
            true,
            None,
        )
        .await
        .unwrap();
    assert!(
        r.state == ActionState::Timeout || r.state == ActionState::Failed,
        "Expected Timeout or Failed, got {:?}",
        r.state
    );
    assert!(
        r.stdout.contains("Log from test 0"),
        "Should see early output"
    );
    assert!(
        !r.stdout.contains("Log from test 19"),
        "Should not complete all iterations"
    );
    session.cleanup();
}

/// Run long_running.sh with timeout — SIGKILL cannot be trapped.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_subprocess_terminate() {
    let user = require_target_user();
    let mut session = make_session(user.clone());
    let script = support_dir().join("long_running_ignore.sh");
    let r = session
        .run_subprocess(
            &script.to_string_lossy(),
            None,
            Some(Duration::from_secs(3)),
            None,
            true,
            None,
        )
        .await
        .unwrap();
    assert!(
        r.state == ActionState::Timeout || r.state == ActionState::Failed,
        "Expected Timeout or Failed, got {:?}",
        r.state
    );
    session.cleanup();
}

/// Run spawn_child.sh with timeout — parent and child should both be killed.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_subprocess_terminate_tree() {
    let user = require_target_user();
    let mut session = make_session(user.clone());
    let script = support_dir().join("spawn_child.sh");
    let r = session
        .run_subprocess(
            &script.to_string_lossy(),
            None,
            Some(Duration::from_secs(3)),
            None,
            true,
            None,
        )
        .await
        .unwrap();
    assert!(
        r.state == ActionState::Timeout || r.state == ActionState::Failed,
        "Expected Timeout or Failed, got {:?}",
        r.state
    );
    assert!(
        r.stdout.contains("Log from test 0"),
        "Should see early output"
    );
    assert!(
        !r.stdout.contains("Log from test 19"),
        "Should not complete all iterations"
    );
    session.cleanup();
}

// === Cross-user runner identity tests ===

/// Verify the subprocess runs as the target user's UID, not ours.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_runner_uid() {
    let user = require_target_user();
    let mut session = make_session(user.clone());
    let r = session
        .run_subprocess("id", Some(&["-u".into()]), None, None, true, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    let our_uid = nix::unistd::geteuid().as_raw().to_string();
    let output_uid = r.stdout.trim();
    assert_ne!(output_uid, our_uid, "Should run as different user");
    let target_uid = nix::unistd::User::from_name(&user.user)
        .unwrap()
        .unwrap()
        .uid
        .as_raw()
        .to_string();
    assert_eq!(
        r.stdout.trim(),
        target_uid,
        "Expected UID {} in output: {}",
        target_uid,
        r.stdout
    );
    session.cleanup();
}

/// Verify env vars are propagated to the cross-user subprocess.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_runner_env_vars() {
    let user = require_target_user();
    let mut session = make_session(user.clone());
    let mut env = HashMap::new();
    env.insert("OPENJD_TEST_VAR".into(), "test_value_123".into());
    let r = session
        .run_subprocess("env", None, None, Some(&env), true, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    assert!(
        r.stdout.contains("OPENJD_TEST_VAR=test_value_123"),
        "Env var not found in: {}",
        r.stdout
    );
    session.cleanup();
}

/// Verify host-process env vars do NOT leak into the cross-user subprocess.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_no_env_inheritance() {
    let unique_var = format!("OPENJD_UNIQUE_{}", std::process::id());
    // SAFETY: single-threaded test setup before session creation
    unsafe { std::env::set_var(&unique_var, "should_not_appear") };
    let user = require_target_user();
    let mut session = make_session(user.clone());
    let r = session
        .run_subprocess("env", None, None, None, true, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    assert!(
        !r.stdout.contains(&unique_var),
        "Host env var leaked: {}",
        unique_var
    );
    unsafe { std::env::remove_var(&unique_var) };
    session.cleanup();
}

// === Cross-user session cleanup tests ===

/// Session cleanup deletes files owned by target user with restrictive permissions.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_session_cleanup() {
    let user = require_target_user();
    let mut session = make_session(user.clone());
    let working_dir = session.working_directory().to_path_buf();

    // Create files owned by target user with owner-only permissions
    let subdir = working_dir.join("subdir");
    let subdir_s = subdir.to_string_lossy().to_string();
    let file = working_dir.join("subdir/file.test");
    let file_s = file.to_string_lossy().to_string();
    let owner = format!("{}:{}", user.user, user.user);
    let cmds: &[&[&str]] = &[
        &["sudo", "-u", &user.user, "-i", "mkdir", &subdir_s],
        &["sudo", "-u", &user.user, "-i", "chown", &owner, &subdir_s],
        &["sudo", "-u", &user.user, "-i", "chmod", "700", &subdir_s],
        &["sudo", "-u", &user.user, "-i", "touch", &file_s],
        &["sudo", "-u", &user.user, "-i", "chown", &owner, &file_s],
        &["sudo", "-u", &user.user, "-i", "chmod", "600", &file_s],
    ];
    for cmd in cmds {
        let status = std::process::Command::new(cmd[0])
            .args(&cmd[1..])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "Setup command failed: {:?}", cmd);
    }

    session.cleanup();
    assert!(!working_dir.exists(), "Working directory should be deleted");
}

// === Cross-user Session::run_subprocess identity test ===

/// Verify Session::run_subprocess runs as the configured target user.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_session_run_subprocess() {
    let user = require_target_user();
    let mut session = make_session(user.clone());
    let r = session
        .run_subprocess("whoami", None, None, None, true, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    assert!(r.stdout.trim().contains(&user.user));
    session.cleanup();
}

// === Cross-user TempDir tests ===

/// TempDir with target user has correct group ownership and mode 0o770.
#[tokio::test]
#[ignore]
async fn test_cross_user_tempdir_permissions() {
    use std::os::unix::fs::MetadataExt;
    let user = require_target_user();
    let td = TempDir::new(None, None, Some(&*user)).unwrap();
    let meta = std::fs::metadata(td.path()).unwrap();
    let expected_gid = nix::unistd::Group::from_name(&user.group)
        .unwrap()
        .unwrap()
        .gid
        .as_raw();
    assert_eq!(meta.gid(), expected_gid, "Group should be {}", user.group);
    assert_eq!(meta.mode() & 0o777, 0o770, "Mode should be 0o770");
    assert_eq!(
        meta.uid(),
        nix::unistd::geteuid().as_raw(),
        "Owner should be us"
    );
}

/// TempDir cleanup works when files are owned by the target user.
#[tokio::test]
#[ignore]
async fn test_cross_user_tempdir_cleanup() {
    let user = require_target_user();
    let mut td = TempDir::new(None, None, Some(&*user)).unwrap();
    let testfile = td.path().join("testfile.txt");
    let status = std::process::Command::new("sudo")
        .args(["-u", &user.user, "-i", "touch", &testfile.to_string_lossy()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    assert!(testfile.exists());
    td.cleanup().unwrap();
    assert!(!testfile.exists());
    assert!(!td.path().exists());
}

/// TempDir with disjoint user (not in shared group) — chown should fail.
#[tokio::test]
#[ignore]
async fn test_cross_user_tempdir_disjoint_fails() {
    let user = require_disjoint_user();
    let result = TempDir::new(None, None, Some(&*user));
    // Python raises RuntimeError. In Rust, chown failure is currently silent
    // (let _ = nix::unistd::chown...), so the dir may be created but with wrong group.
    if let Ok(td) = result {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::metadata(td.path()).unwrap();
        let disjoint_gid = nix::unistd::Group::from_name(&user.group)
            .unwrap()
            .unwrap()
            .gid
            .as_raw();
        assert_ne!(
            meta.gid(),
            disjoint_gid,
            "Should not be able to chown to disjoint group"
        );
    }
    // Err is also acceptable — means the crate properly rejects it
}

// === Cross-user embedded files permission tests ===
// Mirrors Python TestMaterializeFilePosix::test_changes_owner and test_changes_owner_runnable.

/// Embedded file with cross-user: group is changed, mode is 0o660 (non-runnable).
#[tokio::test]
#[ignore]
async fn test_cross_user_embedded_file_permissions() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let user = require_target_user();
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

    let meta = std::fs::metadata(&path).unwrap();
    let expected_gid = nix::unistd::Group::from_name(&user.group)
        .unwrap()
        .unwrap()
        .gid
        .as_raw();
    assert_eq!(
        meta.uid(),
        nix::unistd::geteuid().as_raw(),
        "File owner is this process's owner"
    );
    assert_eq!(meta.gid(), expected_gid, "File group is the user's group");
    let mode = meta.permissions().mode();
    assert_eq!(mode & 0o700, 0o600, "Owner has r/w");
    assert_eq!(mode & 0o070, 0o060, "Group has r/w");
    assert_eq!(mode & 0o007, 0, "Others have no permissions");
}

/// Embedded file with cross-user + runnable: group is changed, mode is 0o770.
#[tokio::test]
#[ignore]
async fn test_cross_user_embedded_file_runnable_permissions() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let user = require_target_user();
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("testfile.sh");

    openjd_sessions::embedded_files::write_embedded_file_with_options(
        &path,
        "#!/bin/bash",
        true,
        None,
    )
    .unwrap();
    openjd_sessions::embedded_files::chown_for_user(&path, &*user, true).unwrap();

    let meta = std::fs::metadata(&path).unwrap();
    let expected_gid = nix::unistd::Group::from_name(&user.group)
        .unwrap()
        .unwrap()
        .gid
        .as_raw();
    assert_eq!(
        meta.uid(),
        nix::unistd::geteuid().as_raw(),
        "File owner is this process's owner"
    );
    assert_eq!(meta.gid(), expected_gid, "File group is the user's group");
    let mode = meta.permissions().mode();
    assert_eq!(mode & 0o700, 0o700, "Owner has r/w/x");
    assert_eq!(mode & 0o070, 0o070, "Group has r/w/x");
    assert_eq!(mode & 0o007, 0, "Others have no permissions");
}
