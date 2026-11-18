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
        limits: Default::default(),
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

/// Like [`make_session`], but installs a session-wide `cancel_token` so a
/// bare `token.cancel()` (the external cancel path a worker agent uses on a
/// service cancel) can be exercised against the cross-user helper.
fn make_session_with_token(
    user: Arc<PosixSessionUser>,
) -> (Session, tokio_util::sync::CancellationToken) {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    // Leak so the dir outlives the session constructor
    std::mem::forget(tmp);
    let token = tokio_util::sync::CancellationToken::new();
    let config = SessionConfig {
        limits: Default::default(),
        session_id: "cross-user-test".into(),
        job_parameter_values: HashMap::new(),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: None,
        os_env_vars: None,
        session_root_directory: Some(root),
        user: Some(user),
        profile: None,
        cancel_token: Some(token.clone()),
        debug_collect_stdout: true,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    };
    (Session::with_config(config).unwrap(), token)
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

// === Cross-user external-token cancel tests (issue #288) ===

/// A bare external `SessionConfig.cancel_token.cancel()` (not
/// `cancel_action`/`SessionCancelHandle`) must cancel a cross-user helper
/// subprocess: the token is a parent of every action token, so
/// `run_via_helper` has to observe it and signal the helper. Before the fix,
/// `run_via_helper` watched only the watch channel, so a bare token cancel
/// let the child run to completion and the action reported Success.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_external_token_cancels_helper_subprocess() {
    use openjd_sessions::session::SessionState;

    let user = require_target_user();
    let (mut session, token) = make_session_with_token(user);

    // Cancel the session-wide token mid-action, from another task.
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        token.cancel();
    });

    let started = std::time::Instant::now();
    let r = session
        .run_subprocess("sleep", Some(&["60".to_string()]), None, None, true, None)
        .await
        .expect("run_subprocess must return a result");
    let elapsed = started.elapsed();

    assert_eq!(
        r.state,
        ActionState::Canceled,
        "a bare session-wide token cancel must classify the helper subprocess \
         as Canceled; exit_code: {:?}, stdout: {}",
        r.exit_code,
        r.stdout
    );
    assert!(
        elapsed < Duration::from_secs(30),
        "the external token cancel must interrupt the 60s sleep, took {elapsed:?}"
    );
    // A Canceled action makes the session ending-only (brittle sessions).
    assert_eq!(session.state(), SessionState::ReadyEnding);
    session.cleanup();
}

/// The same external-token cancel through `run_task` (the environment/step
/// runner helper path, distinct from `run_subprocess`).
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_external_token_cancels_helper_task() {
    use openjd_expr::format_string::FormatString;
    use openjd_model::job::{Action, StepActions, StepScript};
    use openjd_sessions::session::SessionState;

    let user = require_target_user();
    let (mut session, token) = make_session_with_token(user);

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        token.cancel();
    });

    let script = StepScript {
        let_bindings: None,
        actions: StepActions {
            on_run: Action {
                command: FormatString::new("sleep").unwrap(),
                args: Some(vec![FormatString::new("60").unwrap()]),
                timeout: None,
                cancelation: None,
            },
        },
        embedded_files: None,
    };

    let started = std::time::Instant::now();
    let r = session
        .run_task("t", &script, None, None, None)
        .await
        .expect("run_task must return a result");
    let elapsed = started.elapsed();

    assert_eq!(
        r.state,
        ActionState::Canceled,
        "a bare session-wide token cancel must classify the helper task as \
         Canceled; exit_code: {:?}, stdout: {}",
        r.exit_code,
        r.stdout
    );
    assert!(
        elapsed < Duration::from_secs(30),
        "the external token cancel must interrupt the 60s sleep, took {elapsed:?}"
    );
    assert_eq!(session.state(), SessionState::ReadyEnding);
    session.cleanup();
}

/// A task that traps SIGTERM: on TERM it prints `GOT_TERM`, sleeps
/// `trap_sleep` seconds, then exits 0. With `ntt = Some(n)` its cancelation
/// is NOTIFY_THEN_TERMINATE with an `n`-second grace period.
fn trap_task(
    trap_sleep: u32,
    ntt: Option<u32>,
    timeout: Option<u32>,
) -> openjd_model::job::StepScript {
    use openjd_expr::format_string::FormatString;
    use openjd_model::job::{Action, CancelationMode, StepActions, StepScript};
    let script = format!(
        "trap 'echo GOT_TERM; sleep {trap_sleep}; exit 0' TERM; echo STARTED; sleep 60 & wait"
    );
    StepScript {
        let_bindings: None,
        actions: StepActions {
            on_run: Action {
                command: FormatString::new("bash").unwrap(),
                args: Some(vec![
                    FormatString::new("-c").unwrap(),
                    FormatString::new(&script).unwrap(),
                ]),
                timeout: timeout.map(|t| FormatString::new(&t.to_string()).unwrap()),
                cancelation: ntt.map(|n| CancelationMode::NotifyThenTerminate {
                    notify_period_in_seconds: Some(FormatString::new(&n.to_string()).unwrap()),
                }),
            },
        },
        embedded_files: None,
    }
}

/// A bare token cancel on a NOTIFY_THEN_TERMINATE task notifies first and
/// honours the grace period, as the same-user loop does: the TERM trap runs
/// (`GOT_TERM`) and finishes its 3s sleep inside the 10s grace.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_external_token_notify_then_terminate() {
    use openjd_sessions::session::SessionState;

    let user = require_target_user();
    let (mut session, token) = make_session_with_token(user);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        token.cancel();
    });

    let started = std::time::Instant::now();
    let r = session
        .run_task("t", &trap_task(3, Some(10), None), None, None, None)
        .await
        .expect("run_task must return a result");
    let elapsed = started.elapsed();

    assert_eq!(r.state, ActionState::Canceled, "stdout: {}", r.stdout);
    assert!(
        r.stdout.contains("GOT_TERM"),
        "a NOTIFY_THEN_TERMINATE token cancel must send the notify signal first; stdout: {}",
        r.stdout
    );
    assert!(
        elapsed >= Duration::from_secs(4) && elapsed < Duration::from_secs(12),
        "the TERM trap's 3s must run inside the 10s grace, took {elapsed:?}"
    );
    assert_eq!(session.state(), SessionState::ReadyEnding);
    assert!(matches!(
        session.clear_ending_only(),
        Err(openjd_sessions::SessionError::SessionCanceled)
    ));
    session.cleanup();
}

/// A `SessionCancelHandle` cancel with a 2s time limit on a
/// NOTIFY_THEN_TERMINATE(30s) task must stay bounded by the 2s limit: the
/// handle writes the pipe command itself, so `run_via_helper`'s token branch
/// must not send a second command with the declared 30s grace (which would
/// reset the helper's escalation deadline).
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_cancel_handle_time_limit_not_extended_by_token_branch() {
    let user = require_target_user();
    let mut session = make_session(user);
    let handle = session.cancel_handle();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        handle.cancel(Some(Duration::from_secs(2)), false)
    });

    let started = std::time::Instant::now();
    let r = session
        .run_task("t", &trap_task(20, Some(30), None), None, None, None)
        .await
        .expect("run_task must return a result");
    let elapsed = started.elapsed();

    assert!(
        canceller.await.unwrap(),
        "the handle must find the running task"
    );
    assert_eq!(r.state, ActionState::Canceled, "stdout: {}", r.stdout);
    assert!(
        elapsed < Duration::from_secs(8),
        "the 2s time limit must bound the cancel, took {elapsed:?}"
    );
    session.cleanup();
}

/// A bare token cancel during `onEnter` cancels the helper-run action.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_external_token_cancels_on_enter() {
    use openjd_expr::format_string::FormatString;
    use openjd_model::job::{Action, Environment, EnvironmentActions, EnvironmentScript};

    let user = require_target_user();
    let (mut session, token) = make_session_with_token(user);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        token.cancel();
    });
    let env = Environment {
        name: "Env".into(),
        description: None,
        variables: None,
        resolved_symtab: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            embedded_files: None,
            actions: EnvironmentActions {
                on_enter: Some(Action {
                    command: FormatString::new("sleep").unwrap(),
                    args: Some(vec![FormatString::new("60").unwrap()]),
                    timeout: None,
                    cancelation: None,
                }),
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit: None,
            },
        }),
    };

    let started = std::time::Instant::now();
    let _ = session.enter_environment(&env, None, None, None).await;
    let elapsed = started.elapsed();

    assert_eq!(
        session.action_status().map(|a| a.state),
        Some(ActionState::Canceled)
    );
    assert!(
        elapsed < Duration::from_secs(30),
        "the token cancel must interrupt the 60s onEnter, took {elapsed:?}"
    );
    session.cleanup();
}

/// An action that timed out stays `Timeout` when the session token fires
/// while it is still exiting, matching the same-user loop, which checks the
/// timeout before the cancel. The token branch must also not send a second
/// NOTIFY_THEN_TERMINATE: that would reset the helper's escalation deadline,
/// so the TERM-ignoring task would be killed at 3 + 4 = 7s instead of 1 + 4 =
/// 5s.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_timeout_then_token_stays_timeout() {
    let user = require_target_user();
    let (mut session, token) = make_session_with_token(user);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(3)).await;
        token.cancel();
    });

    let started = std::time::Instant::now();
    let r = session
        .run_task("t", &trap_task(20, Some(4), Some(1)), None, None, None)
        .await
        .expect("run_task must return a result");
    let elapsed = started.elapsed();
    assert_eq!(r.state, ActionState::Timeout, "stdout: {}", r.stdout);
    assert!(
        elapsed >= Duration::from_millis(4500) && elapsed < Duration::from_millis(6500),
        "the timeout's 4s grace must end the task at about 5s, took {elapsed:?}"
    );
    session.cleanup();
}

/// Run a NOTIFY_THEN_TERMINATE(4s) task with a 3s timeout whose TERM trap
/// never exits, cancel it at 1s with `cancel`, and check it ends `Canceled`
/// at about 1 + 4 = 5s: once a cancel is delivered the timeout must neither
/// relabel the action `Timeout` nor send a second command that resets the
/// escalation deadline (which would end it at 3 + 4 = 7s), as in the
/// same-user loop.
async fn check_cancel_then_timeout_stays_canceled(
    mut session: Session,
    cancel: impl FnOnce() + Send + 'static,
) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        cancel();
    });

    let started = std::time::Instant::now();
    let r = session
        .run_task("t", &trap_task(20, Some(4), Some(3)), None, None, None)
        .await
        .expect("run_task must return a result");
    let elapsed = started.elapsed();
    assert_eq!(r.state, ActionState::Canceled, "stdout: {}", r.stdout);
    assert!(
        elapsed >= Duration::from_millis(4500) && elapsed < Duration::from_millis(6500),
        "the cancel's 4s grace must end the task at about 5s, took {elapsed:?}"
    );
    session.cleanup();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_token_then_timeout_stays_canceled() {
    let (session, token) = make_session_with_token(require_target_user());
    check_cancel_then_timeout_stays_canceled(session, move || token.cancel()).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_cancel_handle_then_timeout_stays_canceled() {
    let session = make_session(require_target_user());
    let handle = session.cancel_handle();
    check_cancel_then_timeout_stays_canceled(session, move || {
        assert!(
            handle.cancel(None, false),
            "the handle must find the running task"
        );
    })
    .await;
}

// === One cancel method per action, whatever the entry point ===

/// How a cross-user test cancels its task.
#[derive(Debug, Clone, Copy)]
enum CancelVia {
    /// `SessionCancelHandle::cancel(time_limit, false)`, the same command
    /// builder `Session::cancel_action` uses.
    Handle(Option<Duration>),
    /// A bare `SessionConfig.cancel_token.cancel()`.
    Token,
    /// The action's own timeout, in seconds.
    Timeout(u32),
}

/// Run `trap_task(20, ntt, timeout)` cross-user and cancel it at 1.5s
/// through `via`. Returns the result and the time the run took.
async fn run_trap_task_and_cancel(
    ntt: Option<u32>,
    via: CancelVia,
) -> (openjd_sessions::action::ActionResult, Duration) {
    let user = require_target_user();
    let (mut session, token) = make_session_with_token(user);
    let timeout = match via {
        CancelVia::Timeout(t) => Some(t),
        _ => None,
    };
    let handle = session.cancel_handle();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        match via {
            CancelVia::Handle(limit) => handle.cancel(limit, false),
            CancelVia::Token => {
                token.cancel();
                true
            }
            CancelVia::Timeout(_) => true,
        }
    });

    let started = std::time::Instant::now();
    let r = session
        .run_task("t", &trap_task(20, ntt, timeout), None, None, None)
        .await
        .expect("run_task must return a result");
    let elapsed = started.elapsed();
    assert!(
        canceller.await.unwrap(),
        "{via:?}: the handle must find the running task"
    );
    session.cleanup();
    (r, elapsed)
}

/// A task that declares no cancelation is a TERMINATE action. The same-user
/// loop kills it at once on any cancel (`subprocess.rs`), so cross-user must
/// send the helper TERMINATE whichever way the cancel arrives: no TERM trap
/// (`GOT_TERM`), and the task ends right after the 1.5s cancel. Before the
/// shared command builder, the handle (and `cancel_action`) sent
/// NOTIFY_THEN_TERMINATE with a 5s default grace, or with the time limit.
async fn check_terminate_action_killed_at_once(via: CancelVia) {
    let (r, elapsed) = run_trap_task_and_cancel(None, via).await;
    let expected = match via {
        CancelVia::Timeout(_) => ActionState::Timeout,
        _ => ActionState::Canceled,
    };
    assert_eq!(r.state, expected, "{via:?}: stdout: {}", r.stdout);
    assert!(
        !r.stdout.contains("GOT_TERM"),
        "{via:?}: a TERMINATE action must not get the notify signal; stdout: {}",
        r.stdout
    );
    assert!(
        elapsed < Duration::from_millis(4000),
        "{via:?}: a TERMINATE action must be killed at once, took {elapsed:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_cancel_handle_terminate_action_killed_at_once() {
    check_terminate_action_killed_at_once(CancelVia::Handle(None)).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_cancel_handle_time_limit_terminate_action_killed_at_once() {
    check_terminate_action_killed_at_once(CancelVia::Handle(Some(Duration::from_secs(10)))).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_token_terminate_action_killed_at_once() {
    check_terminate_action_killed_at_once(CancelVia::Token).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_timeout_terminate_action_killed_at_once() {
    check_terminate_action_killed_at_once(CancelVia::Timeout(1)).await;
}

/// `run_subprocess` through the helper is a TERMINATE action too: a handle
/// cancel kills it at once, as on the same-user path.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_cancel_handle_run_subprocess_killed_at_once() {
    let mut session = make_session(require_target_user());
    let handle = session.cancel_handle();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        handle.cancel(None, false)
    });
    let args = [
        "-c".to_string(),
        "trap 'echo GOT_TERM; sleep 20; exit 0' TERM; echo STARTED; sleep 60 & wait".to_string(),
    ];
    let started = std::time::Instant::now();
    let r = session
        .run_subprocess("bash", Some(&args), None, None, true, None)
        .await
        .expect("run_subprocess must return a result");
    let elapsed = started.elapsed();
    assert!(
        canceller.await.unwrap(),
        "the handle must find the subprocess"
    );
    assert_eq!(r.state, ActionState::Canceled, "stdout: {}", r.stdout);
    assert!(
        !r.stdout.contains("GOT_TERM"),
        "run_subprocess must not get the notify signal; stdout: {}",
        r.stdout
    );
    assert!(
        elapsed < Duration::from_millis(4000),
        "run_subprocess must be killed at once, took {elapsed:?}"
    );
    session.cleanup();
}

/// A NOTIFY_THEN_TERMINATE(8s) task whose TERM trap never exits gets the
/// notify signal and is killed when the grace ends, about 1.5 + `grace`
/// seconds after the start, whichever way the cancel arrives. 8 s is far
/// enough from the 5 s fallback grace that a command sent with the
/// fallback lands outside the window.
async fn check_notify_then_terminate_grace(via: CancelVia, grace: Duration) {
    let (r, elapsed) = run_trap_task_and_cancel(Some(8), via).await;
    assert_eq!(
        r.state,
        ActionState::Canceled,
        "{via:?}: stdout: {}",
        r.stdout
    );
    assert!(
        r.stdout.contains("GOT_TERM"),
        "{via:?}: a NOTIFY_THEN_TERMINATE cancel must notify first; stdout: {}",
        r.stdout
    );
    let end = Duration::from_millis(1500) + grace;
    assert!(
        elapsed >= end - Duration::from_millis(500) && elapsed < end + Duration::from_millis(1500),
        "{via:?}: the task must be killed when the {grace:?} grace ends (about {end:?}), \
         took {elapsed:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_cancel_handle_notify_then_terminate_uses_declared_grace() {
    check_notify_then_terminate_grace(CancelVia::Handle(None), Duration::from_secs(8)).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_cancel_handle_notify_then_terminate_capped_by_time_limit() {
    check_notify_then_terminate_grace(
        CancelVia::Handle(Some(Duration::from_secs(2))),
        Duration::from_secs(2),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_token_notify_then_terminate_uses_declared_grace() {
    check_notify_then_terminate_grace(CancelVia::Token, Duration::from_secs(8)).await;
}

// === Cancel before the helper's Run command ===
//
// A handle cancel finds an action once it is registered, which is before
// the helper is sent its Run command, and a helper with no command running
// drops a cancel. These tests cancel in that window: deterministically from
// the session's before-Run test hook, and with a canceller that spins on the
// handle (the auditor's probe). Before the fix the action ran to completion
// (6 s, prints FINISHED) and was still reported Canceled.

/// A TERMINATE task that sleeps 6 s and then prints FINISHED.
fn sleep_then_finish_task() -> openjd_model::job::StepScript {
    use openjd_expr::format_string::FormatString;
    use openjd_model::job::{Action, StepActions, StepScript};
    StepScript {
        let_bindings: None,
        actions: StepActions {
            on_run: Action {
                command: FormatString::new("bash").unwrap(),
                args: Some(vec![
                    FormatString::new("-c").unwrap(),
                    FormatString::new("sleep 6; echo FINISHED").unwrap(),
                ]),
                timeout: None,
                cancelation: None,
            },
        },
        embedded_files: None,
    }
}

/// Install a before-Run hook that cancels through a handle with
/// `time_limit`, once. Returns a flag the hook sets when the handle found
/// the action.
fn cancel_before_run_once(
    session: &mut Session,
    time_limit: Option<Duration>,
) -> Arc<std::sync::atomic::AtomicBool> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let handle = session.cancel_handle();
    let fired = Arc::new(AtomicBool::new(false));
    let delivered = Arc::new(AtomicBool::new(false));
    let delivered_in_hook = delivered.clone();
    session.set_before_helper_run_hook_for_test(Arc::new(move || {
        if !fired.swap(true, Ordering::SeqCst) {
            delivered_in_hook.store(handle.cancel(time_limit, false), Ordering::SeqCst);
        }
    }));
    delivered
}

/// A handle cancel before Run must still cancel a task: the TERMINATE task
/// is killed at once instead of running its 6 s and printing FINISHED.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_cancel_before_run_task_killed_at_once() {
    let mut session = make_session(require_target_user());
    let delivered = cancel_before_run_once(&mut session, None);
    let started = std::time::Instant::now();
    let r = session
        .run_task("t", &sleep_then_finish_task(), None, None, None)
        .await
        .expect("run_task must return a result");
    let elapsed = started.elapsed();
    assert!(
        delivered.load(std::sync::atomic::Ordering::SeqCst),
        "the hook's handle must find the registered task"
    );
    assert_eq!(r.state, ActionState::Canceled, "stdout: {}", r.stdout);
    assert!(
        !r.stdout.contains("FINISHED"),
        "a cancel before Run must stop the task; stdout: {}",
        r.stdout
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "the task must be killed at once, took {elapsed:?}"
    );
    session.cleanup();
}

/// `run_subprocess` through the helper, cancelled before its Run command.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_cancel_before_run_subprocess_killed_at_once() {
    let mut session = make_session(require_target_user());
    let delivered = cancel_before_run_once(&mut session, None);
    let args = ["-c".to_string(), "sleep 6; echo FINISHED".to_string()];
    let started = std::time::Instant::now();
    let r = session
        .run_subprocess("bash", Some(&args), None, None, true, None)
        .await
        .expect("run_subprocess must return a result");
    let elapsed = started.elapsed();
    assert!(delivered.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(r.state, ActionState::Canceled, "stdout: {}", r.stdout);
    assert!(!r.stdout.contains("FINISHED"), "stdout: {}", r.stdout);
    assert!(
        elapsed < Duration::from_secs(3),
        "the subprocess must be killed at once, took {elapsed:?}"
    );
    session.cleanup();
}

/// `onEnter` and `onExit` take the same gate as a task: a 6 s action
/// canceled before its Run command ends at once with Canceled.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_cancel_before_run_on_enter_and_on_exit() {
    use openjd_expr::format_string::FormatString;
    use openjd_model::job::{Action, Environment, EnvironmentActions, EnvironmentScript};

    let sleep_action = || Action {
        command: FormatString::new("bash").unwrap(),
        args: Some(vec![
            FormatString::new("-c").unwrap(),
            FormatString::new("sleep 6; echo FINISHED").unwrap(),
        ]),
        timeout: None,
        cancelation: None,
    };
    let env = |on_enter: bool| Environment {
        name: "Env".into(),
        description: None,
        variables: None,
        resolved_symtab: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            embedded_files: None,
            actions: EnvironmentActions {
                on_enter: on_enter.then(sleep_action),
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit: (!on_enter).then(sleep_action),
            },
        }),
    };

    // onEnter
    let mut session = make_session(require_target_user());
    let delivered = cancel_before_run_once(&mut session, None);
    let started = std::time::Instant::now();
    let _ = session
        .enter_environment(&env(true), None, None, None)
        .await;
    let elapsed = started.elapsed();
    assert!(delivered.load(std::sync::atomic::Ordering::SeqCst));
    let status = session.action_status();
    assert_eq!(
        status.as_ref().map(|a| a.state),
        Some(ActionState::Canceled),
        "{status:?}"
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "onEnter must be killed at once, took {elapsed:?}"
    );
    session.cleanup();

    // onExit: enter with no onEnter, then cancel the exit before its Run.
    let mut session = make_session(require_target_user());
    let id = session
        .enter_environment(&env(false), None, None, None)
        .await
        .unwrap();
    let delivered = cancel_before_run_once(&mut session, None);
    let started = std::time::Instant::now();
    let _ = session.exit_environment(&id, None, false, None).await;
    let elapsed = started.elapsed();
    assert!(delivered.load(std::sync::atomic::Ordering::SeqCst));
    let status = session.action_status();
    assert_eq!(
        status.as_ref().map(|a| a.state),
        Some(ActionState::Canceled),
        "{status:?}"
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "onExit must be killed at once, took {elapsed:?}"
    );
    session.cleanup();
}

/// After a task canceled before Run, the same helper runs the session's
/// next action to Success. (On Unix a cancel can't carry over anyway, since
/// the helper drops one with no command running; this checks the helper is
/// still usable.) (These Docker tests don't check a NOTIFY_THEN_TERMINATE grace:
/// a held cancel signals the process right after it starts, before a script
/// could install a TERM trap. `cross_user_helper.rs`'s
/// `run_via_helper_gate_tests` check the held command exactly.)
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_cancel_before_run_leaves_helper_ready_for_next_action() {
    let mut session = make_session(require_target_user());
    let _delivered = cancel_before_run_once(&mut session, None);
    let r = session
        .run_task("t", &sleep_then_finish_task(), None, None, None)
        .await
        .expect("run_task must return a result");
    assert_eq!(r.state, ActionState::Canceled, "stdout: {}", r.stdout);
    // A canceled action makes the session ending-only (session.md § Brittle
    // Sessions); clear it to run the next action.
    session.clear_ending_only().unwrap();
    let args = ["-c".to_string(), "sleep 1; echo FINISHED".to_string()];
    let next = session
        .run_subprocess("bash", Some(&args), None, None, true, None)
        .await
        .expect("run_subprocess must return a result");
    assert_eq!(next.state, ActionState::Success, "stdout: {}", next.stdout);
    assert!(next.stdout.contains("FINISHED"), "stdout: {}", next.stdout);
    session.cleanup();
}

/// If a runner's dup of the cancel pipe fails, a cancel before Run still
/// reaches a task, onEnter and onExit (the held cancel goes through the
/// helper's own stdin), and so does a handle cancel after Run (the handle
/// has its own dup).
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_cancel_without_runner_cancel_pipe() {
    // Before Run.
    let mut session = make_session(require_target_user());
    session.fail_runner_cancel_writer_for_test();
    let delivered = cancel_before_run_once(&mut session, None);
    let started = std::time::Instant::now();
    let r = session
        .run_task("t", &sleep_then_finish_task(), None, None, None)
        .await
        .expect("run_task must return a result");
    let elapsed = started.elapsed();
    assert!(delivered.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(r.state, ActionState::Canceled, "stdout: {}", r.stdout);
    assert!(!r.stdout.contains("FINISHED"), "stdout: {}", r.stdout);
    assert!(elapsed < Duration::from_secs(3), "took {elapsed:?}");
    session.cleanup();

    // onEnter and onExit, before Run.
    let env = |on_enter: bool| {
        use openjd_expr::format_string::FormatString;
        use openjd_model::job::{Action, Environment, EnvironmentActions, EnvironmentScript};
        let action = || Action {
            command: FormatString::new("bash").unwrap(),
            args: Some(vec![
                FormatString::new("-c").unwrap(),
                FormatString::new("sleep 6; echo FINISHED").unwrap(),
            ]),
            timeout: None,
            cancelation: None,
        };
        Environment {
            name: "Env".into(),
            description: None,
            variables: None,
            resolved_symtab: None,
            script: Some(EnvironmentScript {
                let_bindings: None,
                embedded_files: None,
                actions: EnvironmentActions {
                    on_enter: on_enter.then(action),
                    on_wrap_env_enter: None,
                    on_wrap_task_run: None,
                    on_wrap_env_exit: None,
                    on_exit: (!on_enter).then(action),
                },
            }),
        }
    };
    let mut session = make_session(require_target_user());
    session.fail_runner_cancel_writer_for_test();
    let delivered = cancel_before_run_once(&mut session, None);
    let started = std::time::Instant::now();
    let _ = session
        .enter_environment(&env(true), None, None, None)
        .await;
    assert!(delivered.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        session.action_status().map(|a| a.state),
        Some(ActionState::Canceled)
    );
    assert!(started.elapsed() < Duration::from_secs(3), "onEnter");
    session.cleanup();

    let mut session = make_session(require_target_user());
    session.fail_runner_cancel_writer_for_test();
    let id = session
        .enter_environment(&env(false), None, None, None)
        .await
        .unwrap();
    let delivered = cancel_before_run_once(&mut session, None);
    let started = std::time::Instant::now();
    let _ = session.exit_environment(&id, None, false, None).await;
    assert!(delivered.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        session.action_status().map(|a| a.state),
        Some(ActionState::Canceled)
    );
    assert!(started.elapsed() < Duration::from_secs(3), "onExit");
    session.cleanup();

    // After Run.
    let mut session = make_session(require_target_user());
    session.fail_runner_cancel_writer_for_test();
    let handle = session.cancel_handle();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        handle.cancel(None, false)
    });
    let started = std::time::Instant::now();
    let r = session
        .run_task("t", &sleep_then_finish_task(), None, None, None)
        .await
        .expect("run_task must return a result");
    let elapsed = started.elapsed();
    assert!(canceller.await.unwrap());
    assert_eq!(r.state, ActionState::Canceled, "stdout: {}", r.stdout);
    assert!(!r.stdout.contains("FINISHED"), "stdout: {}", r.stdout);
    assert!(elapsed < Duration::from_secs(4), "took {elapsed:?}");
    session.cleanup();
}

/// The auditor's probe, without the hook: a canceller thread spins on the
/// handle from before the task starts, so its cancel lands right after the
/// task registers. Three runs, each must end at once.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_spinning_cancel_handle_stops_task() {
    use std::sync::atomic::{AtomicBool, Ordering};
    for run in 0..3 {
        let mut session = make_session(require_target_user());
        let handle = session.cancel_handle();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_spinner = stop.clone();
        let spinner = std::thread::spawn(move || {
            while !stop_spinner.load(Ordering::Relaxed) {
                if handle.cancel(None, false) {
                    return true;
                }
                std::hint::spin_loop();
            }
            false
        });
        let started = std::time::Instant::now();
        let r = session
            .run_task("t", &sleep_then_finish_task(), None, None, None)
            .await
            .expect("run_task must return a result");
        let elapsed = started.elapsed();
        stop.store(true, Ordering::Relaxed);
        assert!(
            spinner.join().unwrap(),
            "run {run}: the handle must find the task"
        );
        assert_eq!(
            r.state,
            ActionState::Canceled,
            "run {run}: stdout: {}",
            r.stdout
        );
        assert!(
            !r.stdout.contains("FINISHED"),
            "run {run}: stdout: {}",
            r.stdout
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "run {run}: the task must be killed at once, took {elapsed:?}"
        );
        session.cleanup();
    }
}

// === Cross-user brittle-session tests ===

/// A subprocess that fails through the cross-user helper makes the session
/// ending-only (specs/sessions/session.md § Brittle Sessions), so exiting an
/// environment with `keep_session_running = true` must not bring it back to
/// Ready. A succeeding helper subprocess leaves the session Ready, and
/// `clear_ending_only()` makes helper subprocesses run again.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_failed_helper_subprocess_makes_session_ending_only() {
    use openjd_model::job::Environment;
    use openjd_sessions::session::SessionState;

    let user = require_target_user();
    let mut session = make_session(user);
    let env = Environment {
        name: "Env".into(),
        description: None,
        script: None,
        variables: None,
        resolved_symtab: None,
    };
    let id = session
        .enter_environment(&env, None, None, None)
        .await
        .unwrap();

    let ok = session
        .run_subprocess("true", None, None, None, true, None)
        .await
        .unwrap();
    assert_eq!(ok.state, ActionState::Success);
    assert_eq!(session.state(), SessionState::Ready);

    let args = ["-c".to_string(), "exit 5".to_string()];
    let failed = session
        .run_subprocess("sh", Some(&args), None, None, true, None)
        .await
        .unwrap();
    assert_eq!(failed.state, ActionState::Failed);
    assert_eq!(session.state(), SessionState::ReadyEnding);

    session
        .exit_environment(&id, None, true, None)
        .await
        .unwrap();
    assert_eq!(session.state(), SessionState::ReadyEnding);
    assert!(matches!(
        session
            .run_subprocess("true", None, None, None, true, None)
            .await,
        Err(openjd_sessions::SessionError::InvalidState { .. })
    ));

    session.clear_ending_only().unwrap();
    assert_eq!(session.state(), SessionState::Ready);
    let ok = session
        .run_subprocess("true", None, None, None, true, None)
        .await
        .unwrap();
    assert_eq!(ok.state, ActionState::Success);
    assert_eq!(session.state(), SessionState::Ready);
    let failed = session
        .run_subprocess("sh", Some(&args), None, None, true, None)
        .await
        .unwrap();
    assert_eq!(failed.state, ActionState::Failed);
    assert_eq!(session.state(), SessionState::ReadyEnding);
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

// === Cross-user helper stdout framing regression tests ===
//
// Exercise the bounded, non-blocking stdout reader in `helper/src/runner.rs`.
// `#[ignore]`; need the Docker + sudo fixtures like `test_cross_user_subprocess_basic`.

/// Invalid UTF-8 (0x97) between valid lines must not abort the run: it streams
/// on, backslash-escaped. Pre-fix `read_line` returned InvalidData.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_invalid_utf8_output_continues() {
    let user = require_target_user();
    let mut session = make_session(user);
    // \227 is octal for 0x97 (POSIX-portable printf escape).
    let args = [
        "-c".to_string(),
        "printf 'before\\n\\227\\nafter\\n'".to_string(),
    ];
    let r = session
        .run_subprocess("sh", Some(&args), None, None, true, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success, "stdout: {}", r.stdout);
    assert!(r.stdout.contains("before"), "stdout: {}", r.stdout);
    assert!(
        r.stdout.contains(r"\x97"),
        "escaped byte missing: {}",
        r.stdout
    );
    assert!(r.stdout.contains("after"), "stdout: {}", r.stdout);
    session.cleanup();
}

/// Per-line memory stays bounded: a 1 MiB newline-less line yields one
/// truncated Out (<= 64 KiB) and "END" still streams. Pre-fix `read_line`
/// buffered the whole line and overflowed the response limit.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_bounded_single_out_long_line() {
    let user = require_target_user();
    let mut session = make_session(user);
    let args = [
        "-c".to_string(),
        "head -c 1048576 /dev/zero | tr '\\0' 'a'; printf '\\nEND\\n'".to_string(),
    ];
    let r = session
        .run_subprocess("sh", Some(&args), None, None, true, None)
        .await
        .unwrap();
    assert_eq!(
        r.state,
        ActionState::Success,
        "stdout len: {}",
        r.stdout.len()
    );
    // The unbounded 'a' run must be truncated to exactly the 64 KiB prefix (one Out).
    let a_count = r.stdout.chars().filter(|&c| c == 'a').count();
    assert_eq!(
        a_count,
        64 * 1024,
        "long line must preserve exactly the bounded 64 KiB prefix, got {a_count}"
    );
    assert!(r.stdout.contains("END"), "trailing line missing");
    session.cleanup();
}

/// Stress the bounded reader with 10 MiB on one newline-less line, written in
/// 10240 separate 1 KiB writes so the reader wakes thousands of times mid-line.
/// Still exactly one truncated Out (the 64 KiB prefix) and "END" still streams:
/// the discard path holds across many `fill_buf` wakeups without stalling the
/// pipe. Pre-fix `read_line` buffered all 10 MiB and overflowed the response
/// limit.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_bounded_single_out_10mib_repeated_flushes() {
    let user = require_target_user();
    let mut session = make_session(user);
    // One 1 KiB chunk emitted 10240 times = 10 MiB, each `printf` a separate
    // write(2) with no newline, so every wakeup lands mid-line.
    let args = [
        "-c".to_string(),
        "chunk=$(head -c 1024 /dev/zero | tr '\\0' 'a'); i=0; while [ $i -lt 10240 ]; do printf '%s' \"$chunk\"; i=$((i+1)); done; printf '\\nEND\\n'".to_string(),
    ];
    let r = session
        .run_subprocess("sh", Some(&args), None, None, true, None)
        .await
        .unwrap();
    assert_eq!(
        r.state,
        ActionState::Success,
        "stdout len: {}",
        r.stdout.len()
    );
    // Exactly the 64 KiB prefix survives. More would mean the cap leaked; a
    // multiple would mean the line was emitted more than once.
    let a_count = r.stdout.chars().filter(|&c| c == 'a').count();
    assert_eq!(
        a_count,
        64 * 1024,
        "10 MiB line must yield exactly one bounded 64 KiB prefix, got {a_count}"
    );
    assert!(r.stdout.contains("END"), "trailing line missing");
    session.cleanup();
}

/// Cancel stays responsive during a partial line: a newline-less line then
/// sleep must still cancel promptly. Pre-fix `read_line` blocked on the
/// missing newline until the sleep ended.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_cancel_during_partial_line() {
    let user = require_target_user();
    let mut session = make_session(user);
    let canceller = spawn_cancel_with_retry(session.cancel_handle(), false);

    let args = ["-c".to_string(), "printf x; sleep 300".to_string()];
    let started = std::time::Instant::now();
    let result = session
        .run_subprocess("sh", Some(&args), None, None, true, None)
        .await
        .expect("cross-user subprocess must return a cancellation result");
    let elapsed = started.elapsed();

    let delivered = canceller.await.expect("canceller task must not panic");
    assert_eq!(
        result.state,
        ActionState::Canceled,
        "cancel must end the action as Canceled; exit_code: {:?}, stdout: {}",
        result.exit_code,
        result.stdout
    );
    assert!(
        delivered,
        "handle must find the in-flight helper action and deliver the cancel"
    );
    assert!(
        elapsed < Duration::from_secs(30),
        "cancel must interrupt the partial-line sleep, took {elapsed:?}"
    );
    session.cleanup();
}

/// JSON-expansion guard end to end: 100 KiB of 0x01 expands 6x (`\u0001`) to
/// ~600 KiB, past the 128 KiB limit. Must complete with a truncated Out, not a
/// per-line HelperCommunication error.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_control_char_line_within_response_limit() {
    let user = require_target_user();
    let mut session = make_session(user);
    let args = [
        "-c".to_string(),
        "head -c 102400 /dev/zero | tr '\\0' '\\001'; printf '\\n'".to_string(),
    ];
    let r = session
        .run_subprocess("sh", Some(&args), None, None, true, None)
        .await
        .unwrap();
    assert_eq!(
        r.state,
        ActionState::Success,
        "control-char line must not trip the response limit; stdout len: {}",
        r.stdout.len()
    );
    let control_count = r.stdout.chars().filter(|&c| c == '\u{1}').count();
    assert!(
        control_count > 0,
        "bounded control-character output must be delivered, not silently dropped"
    );
    assert!(
        control_count <= 64 * 1024,
        "control-character output must remain bounded"
    );
    session.cleanup();
}

/// EOF flush: a trailing newline-less line is still delivered on exit.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_cross_user_eof_partial_line_preserved() {
    let user = require_target_user();
    let mut session = make_session(user);
    let args = ["-c".to_string(), "printf 'tail-no-newline'".to_string()];
    let r = session
        .run_subprocess("sh", Some(&args), None, None, true, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success, "stdout: {}", r.stdout);
    assert!(
        r.stdout.contains("tail-no-newline"),
        "EOF partial line dropped: {}",
        r.stdout
    );
    session.cleanup();
}
