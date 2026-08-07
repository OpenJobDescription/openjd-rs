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
use openjd_sessions::session::{Session, SessionCancelHandle, SessionConfig};
use openjd_sessions::session_user::PosixSessionUser;
use openjd_sessions::tempdir::TempDir;
use openjd_sessions::SessionError;

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
        echo_openjd_directives: true,
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

/// Run long_running.sh (traps SIGTERM) with a timeout — the process should be
/// killed before completing all iterations. The trap handler does NOT fire: the
/// timeout path kills the process group with SIGKILL directly (see the NOTE
/// below), so this test asserts a mid-flight kill, not graceful trap handling.
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
    // NOTE: run_subprocess's timeout uses CancelMethod::Terminate (immediate
    // SIGKILL), so the workload's SIGTERM trap is intentionally NOT expected to
    // fire here — asserting "Trapped" would be wrong. Assert instead that the
    // workload ran as the target user and was cut off mid-flight (distinguishing
    // a real kill from a failure-to-launch, which also yields Failed). The
    // SIGTERM/notify path is covered by test_cross_user_notify_delivers_sigterm.
    assert!(
        r.stdout.contains("Log from test 0"),
        "Should see early output; stdout: {:?}",
        r.stdout
    );
    assert!(
        !r.stdout.contains("Log from test 19"),
        "Should not complete all iterations; stdout: {:?}",
        r.stdout
    );
    session.cleanup();
}

/// Run long_running_ignore.sh with a timeout — the workload traps SIGTERM and
/// never exits on it, so only an untrappable signal can stop it.
///
/// NOTE: there is no SIGTERM->SIGKILL escalation on this path. `run_subprocess`'s
/// timeout uses `CancelMethod::Terminate`, which sends SIGKILL to the process
/// group directly (subprocess.rs: "Action timed out, sending SIGKILL to process
/// group"), so the workload's TERM trap never fires. That makes the outcome here
/// identical to long_running.sh; this test's value is proving that a workload
/// which *ignores* SIGTERM is still reaped cross-user, which is the property that
/// would regress if the timeout path ever switched to a trappable signal without
/// a follow-up kill. Genuine SIGTERM delivery is covered by
/// test_cross_user_notify_delivers_sigterm.
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
    // Assert the workload actually ran and was cut off mid-flight, rather than
    // failing to launch as the target user (which would also yield `Failed` and
    // is otherwise indistinguishable from a successful kill). The script prints
    // bare iteration numbers.
    assert!(
        r.stdout.contains('0'),
        "Should see early output; stdout: {:?}",
        r.stdout
    );
    assert!(
        !r.stdout.contains("19"),
        "SIGKILL should have stopped the loop before completion; stdout: {:?}",
        r.stdout
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
    // spawn_child.sh launches long_running.sh as a child ("Log from test N")
    // then runs its own loop ("Log from runner N"). Seeing the child's early
    // output confirms the whole tree launched; neither loop completing confirms
    // the process-GROUP kill reached both parent and child, rather than only the
    // parent dying and the child being orphaned (which is the failure mode a
    // pgid-based kill exists to prevent).
    assert!(
        r.stdout.contains("Log from test 0"),
        "Child process should have produced early output; stdout: {:?}",
        r.stdout
    );
    assert!(
        !r.stdout.contains("Log from test 19"),
        "Child should have been killed with the group, not completed; stdout: {:?}",
        r.stdout
    );
    assert!(
        !r.stdout.contains("Log from runner 19"),
        "Parent should have been killed before completion; stdout: {:?}",
        r.stdout
    );
    session.cleanup();
}

// === Cross-user runner identity tests ===

/// Verify the subprocess runs as the target user's UID, not ours.
///
/// Note on including UIDs in assertion messages:
/// A numeric POSIX UID is not sensitive information. UIDs are public on any
/// POSIX system — visible via `id`, `ps`, `stat`, `/proc`, and `/etc/passwd`.
/// This test runs in an isolated Docker container (`#[cfg(unix)] #[ignore]`,
/// executed only by the Cross-User (Linux) CI job) against a throwaway test
/// account whose UID has no meaning outside that container.
///
/// The entire purpose of this test is to verify that the subprocess runs as
/// the expected UID, so including both the observed and expected UIDs in the
/// assertion message is essential for debugging failures. Static analyzers
/// that flag `assert_eq!` with a UID as "cleartext logging of sensitive
/// information" are producing a false positive here.
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

// === Cross-user Session cancel-handle test ===

/// Repeatedly attempt to deliver a cancel through `handle` until it finds a
/// running action, starting after a short delay so the cancel lands
/// mid-action in the healthy case. Resolves to whether delivery succeeded.
///
/// A single fixed-delay cancel is racy in both directions: a slowly starting
/// subprocess may not be registered yet (the cancel finds nothing), and a
/// subprocess that exits early ends the run before the delay elapses,
/// leaving the cancel result unrecorded when the assertions execute.
fn spawn_cancel_with_retry(
    handle: SessionCancelHandle,
    mark_action_failed: bool,
) -> tokio::task::JoinHandle<bool> {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if handle.cancel(None, mark_action_failed) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
}

/// A notify-then-terminate cancel must actually deliver SIGTERM to the
/// cross-user workload's process group — not go straight to SIGKILL. The trap
/// script (`long_running.sh`, `trap 'echo Trapped; exit 1' TERM`) prints
/// "Trapped" only if SIGTERM reaches it; cancelling with a grace period gives
/// the trap time to fire and flush before escalation. This is the only test
/// that pins the SIGTERM path across the sudo boundary (the timeout tests use
/// CancelMethod::Terminate, i.e. immediate SIGKILL, so the trap never fires
/// there).
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_notify_delivers_sigterm() {
    let user = require_target_user();
    let mut session = make_session(user);
    // Grace period: SIGTERM, wait up to 3s for the trap to run + flush, then SIGKILL.
    let canceller = {
        let handle = session.cancel_handle();
        tokio::spawn(async move {
            for _ in 0..60 {
                tokio::time::sleep(Duration::from_millis(500)).await;
                if handle.cancel(Some(Duration::from_secs(3)), false) {
                    return true;
                }
            }
            false
        })
    };

    let script = support_dir().join("long_running.sh");
    let r = session
        .run_subprocess(&script.to_string_lossy(), None, None, None, true, None)
        .await
        .unwrap();
    let delivered = canceller.await.expect("canceller task must not panic");

    assert!(
        delivered,
        "handle must find the in-flight action and cancel it"
    );
    assert!(
        r.stdout.contains("Log from test 0"),
        "workload should have run as the target user; stdout: {:?}",
        r.stdout
    );
    assert!(
        r.stdout.contains("Trapped"),
        "SIGTERM should have reached the workload's process group and fired its \
         trap before SIGKILL escalation; stdout: {:?}",
        r.stdout
    );
    session.cleanup();
}

/// A `SessionCancelHandle` must be able to cancel a subprocess that runs via
/// the cross-user helper: the helper path registers per-action cancel state,
/// and the handle delivers the cancel command over the helper pipe. This is
/// the cross-user counterpart of the same-user handle tests in
/// `test_session.rs` (which cannot exercise helper routing).
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_session_cancel_handle_cancels_helper_subprocess() {
    let user = require_target_user();
    let mut session = make_session(user);
    let canceller = spawn_cancel_with_retry(session.cancel_handle(), false);

    let started = std::time::Instant::now();
    let result = session
        .run_subprocess("sleep", Some(&["60".to_string()]), None, None, true, None)
        .await;
    let elapsed = started.elapsed();

    let delivered = canceller.await.expect("canceller task must not panic");
    match result {
        // With the cancel-request channel wired into the helper config, the
        // exit must classify as Canceled rather than Failed.
        Ok(r) => assert_eq!(
            r.state,
            ActionState::Canceled,
            "canceled helper subprocess must classify as Canceled; exit_code: {:?}, stdout: {}",
            r.exit_code,
            r.stdout
        ),
        // A canceled helper subprocess may surface as an error result — but
        // only when the cancel was actually delivered. Otherwise the
        // subprocess failed on its own: fail with the underlying error.
        Err(e) => assert!(
            delivered,
            "subprocess failed before any cancel was delivered: {e}"
        ),
    }
    assert!(
        delivered,
        "handle must find the in-flight helper action and deliver the cancel"
    );
    assert!(
        elapsed < Duration::from_secs(30),
        "cancel must interrupt the 60s sleep, took {elapsed:?}"
    );
    session.cleanup();
}

/// `mark_action_failed=true` must convert a canceled helper subprocess to
/// Failed, matching drive_action's conversion and the
/// `SessionCancelHandle::cancel` contract.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_session_cancel_handle_mark_failed_helper_subprocess() {
    let user = require_target_user();
    let mut session = make_session(user);
    let canceller = spawn_cancel_with_retry(session.cancel_handle(), true);

    let result = session
        .run_subprocess("sleep", Some(&["60".to_string()]), None, None, true, None)
        .await;

    let delivered = canceller.await.expect("canceller task must not panic");
    match result {
        Ok(r) => assert_eq!(
            r.state,
            ActionState::Failed,
            "mark_action_failed must report the canceled helper subprocess as Failed; exit_code: {:?}, stdout: {}",
            r.exit_code,
            r.stdout
        ),
        // Tolerated only when the cancel was actually delivered — otherwise
        // the subprocess failed on its own: fail with the underlying error.
        Err(e) => assert!(
            delivered,
            "subprocess failed before any cancel was delivered: {e}"
        ),
    }
    // Without this, a subprocess that fails on its own also yields Failed
    // and the test passes without exercising cancellation at all.
    assert!(
        delivered,
        "cancel must be delivered for this test to exercise mark_action_failed"
    );
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
    // The process user is not a member of the disjoint user's group, so the
    // chown to that group must fail. TempDir::new chowns before chmod and
    // propagates the failure (SessionError::PathPermissions) rather than
    // silently creating a directory with the wrong group — so this must be an
    // Err. Snapshot the parent so we can also assert the just-created directory
    // is removed on failure (the TempDir struct, and its Drop cleanup, never
    // exist when chown fails, so TempDir::new must remove it explicitly).
    let parent = openjd_sessions::tempdir::openjd_temp_dir(None).unwrap();
    let before: std::collections::HashSet<PathBuf> = std::fs::read_dir(&parent)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();

    let result = TempDir::new(None, None, Some(&*user));
    match result {
        Err(SessionError::PathPermissions { .. }) => {}
        Err(other) => panic!("Expected PathPermissions error, got {other:?}"),
        Ok(td) => panic!(
            "Expected chown to the disjoint group to fail, but a directory was created at {}",
            td.path().display()
        ),
    }

    // No orphaned directory: the set of entries under the parent is unchanged.
    let after: std::collections::HashSet<PathBuf> = std::fs::read_dir(&parent)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    assert_eq!(
        before,
        after,
        "failed cross-user TempDir::new must not leave a directory behind under {}",
        parent.display()
    );
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
