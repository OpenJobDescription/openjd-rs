// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Brittle sessions (specs/sessions/session.md § Brittle Sessions).
//!
//! Once an action ends `Failed`, `Canceled` or `Timeout`, the session is
//! ending-only: it accepts only `exit_environment` and `cleanup`, and it
//! never returns to `Ready`. That must hold whatever the caller later passes
//! as `keep_session_running`, and whether or not the exited environment has
//! an `onExit` action.

use openjd_expr::format_string::FormatString;
use openjd_model::job::{
    Action, Environment, EnvironmentActions, EnvironmentScript, StepActions, StepScript,
};
use openjd_sessions::action::ActionState;
use openjd_sessions::error::SessionError;
use openjd_sessions::session::{Session, SessionState};
use std::time::Duration;
use tempfile::TempDir;

fn fs(s: &str) -> FormatString {
    FormatString::new(s).unwrap()
}

fn action(script: &str) -> Action {
    Action {
        command: fs("sh"),
        args: Some(vec![fs("-c"), fs(script)]),
        timeout: None,
        cancelation: None,
    }
}

fn step_action(on_run: Action) -> StepScript {
    StepScript {
        let_bindings: None,
        actions: StepActions { on_run },
        embedded_files: None,
    }
}

fn step(script: &str) -> StepScript {
    step_action(action(script))
}

fn env(name: &str, on_enter: Option<Action>, on_exit: Option<Action>) -> Environment {
    Environment {
        name: name.into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter,
                on_wrap_env_enter: None,
                on_wrap_task_run: None,
                on_wrap_env_exit: None,
                on_exit,
            },
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    }
}

/// How the action that makes the session ending-only ends.
#[derive(Clone, Copy, Debug)]
enum Ending {
    Failed,
    Canceled,
    Timeout,
}

impl Ending {
    fn action_state(self) -> ActionState {
        match self {
            Ending::Failed => ActionState::Failed,
            Ending::Canceled => ActionState::Canceled,
            Ending::Timeout => ActionState::Timeout,
        }
    }
}

/// Run a task that ends in `ending`, and check the session reports it.
async fn run_ending_task(s: &mut Session, ending: Ending) {
    let script = match ending {
        Ending::Failed => step("exit 1"),
        Ending::Canceled => {
            let handle = s.cancel_handle();
            tokio::spawn(async move {
                // Retry until the action is registered, so the cancel can't
                // land before the subprocess starts.
                for _ in 0..200 {
                    if handle.cancel(Some(Duration::from_secs(0)), false) {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                panic!("the task never became cancelable");
            });
            step("sleep 30")
        }
        Ending::Timeout => step_action(Action {
            timeout: Some(fs("1")),
            ..action("sleep 30")
        }),
    };
    let r = s.run_task("t", &script, None, None, None).await.unwrap();
    assert_eq!(r.state, ending.action_state());
    assert_eq!(s.state(), SessionState::ReadyEnding);
}

/// Assert that the session refuses every action that needs `Ready`.
async fn assert_refuses_new_actions(s: &mut Session) {
    let current = s.state();
    match s
        .run_task("t2", &step("echo again"), None, None, None)
        .await
    {
        Err(SessionError::InvalidState { .. }) => {}
        other => panic!("run_task in {current:?} must be refused, got {other:?}"),
    }
    match s
        .enter_environment(
            &env("New", Some(action("echo new")), None),
            None,
            None,
            None,
        )
        .await
    {
        Err(SessionError::InvalidState { .. }) => {}
        other => panic!("enter_environment in {current:?} must be refused, got {other:?}"),
    }
    match s
        .enter_environment(&env("NoAction", None, None), None, None, None)
        .await
    {
        Err(SessionError::InvalidState { .. }) => {}
        other => panic!("enter_environment (no onEnter) must be refused, got {other:?}"),
    }
    match s
        .run_subprocess("echo", Some(&["again".into()]), None, None, true, None)
        .await
    {
        Err(SessionError::InvalidState { .. }) => {}
        other => panic!("run_subprocess in {current:?} must be refused, got {other:?}"),
    }
}

/// Enter one environment, run a task that ends in `ending`, then exit the
/// environment with `keep_session_running`. The session must stay
/// ending-only throughout.
async fn check_task_ending_then_exit(ending: Ending, keep_session_running: bool, on_exit: bool) {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let on_exit = on_exit.then(|| action("echo exited"));
    let id = s
        .enter_environment(&env("Env", None, on_exit.clone()), None, None, None)
        .await
        .unwrap();
    assert_eq!(s.state(), SessionState::Ready);

    run_ending_task(&mut s, ending).await;
    assert_refuses_new_actions(&mut s).await;

    let out = s
        .exit_environment(&id, None, keep_session_running, None)
        .await
        .unwrap();
    if on_exit.is_some() {
        assert!(out.contains("exited"), "onExit must still run: {out:?}");
    }
    assert_eq!(
        s.state(),
        SessionState::ReadyEnding,
        "a {ending:?} action must leave the session ending-only after \
         exit_environment(keep_session_running = {keep_session_running})"
    );
    assert_eq!(s.action_status().unwrap().state, ActionState::Success);
    assert_refuses_new_actions(&mut s).await;

    s.cleanup();
    assert_eq!(s.state(), SessionState::Ended);
}

macro_rules! task_ending_tests {
    ($($name:ident: $ending:expr, $keep:expr, $on_exit:expr;)*) => {$(
        #[tokio::test(flavor = "multi_thread")]
        async fn $name() {
            check_task_ending_then_exit($ending, $keep, $on_exit).await;
        }
    )*};
}

task_ending_tests! {
    failed_task_then_exit_keep_running_without_on_exit: Ending::Failed, true, false;
    failed_task_then_exit_keep_running_with_on_exit: Ending::Failed, true, true;
    failed_task_then_exit_ending_without_on_exit: Ending::Failed, false, false;
    failed_task_then_exit_ending_with_on_exit: Ending::Failed, false, true;
    canceled_task_then_exit_keep_running_without_on_exit: Ending::Canceled, true, false;
    canceled_task_then_exit_keep_running_with_on_exit: Ending::Canceled, true, true;
    canceled_task_then_exit_ending_without_on_exit: Ending::Canceled, false, false;
    canceled_task_then_exit_ending_with_on_exit: Ending::Canceled, false, true;
    timeout_task_then_exit_keep_running_without_on_exit: Ending::Timeout, true, false;
    timeout_task_then_exit_keep_running_with_on_exit: Ending::Timeout, true, true;
    timeout_task_then_exit_ending_without_on_exit: Ending::Timeout, false, false;
    timeout_task_then_exit_ending_with_on_exit: Ending::Timeout, false, true;
}

/// With two environments entered, exiting both with
/// `keep_session_running = true` after a failure must not reopen the
/// session at either step.
#[tokio::test]
async fn failed_task_then_exit_nested_environments_keep_running() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let outer = s
        .enter_environment(
            &env("Outer", None, Some(action("echo outer"))),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    let inner = s
        .enter_environment(&env("Inner", None, None), None, None, None)
        .await
        .unwrap();
    run_ending_task(&mut s, Ending::Failed).await;

    s.exit_environment(&inner, None, true, None).await.unwrap();
    assert_eq!(s.state(), SessionState::ReadyEnding);
    assert_refuses_new_actions(&mut s).await;

    s.exit_environment(&outer, None, true, None).await.unwrap();
    assert_eq!(s.state(), SessionState::ReadyEnding);
    assert_refuses_new_actions(&mut s).await;
}

/// A failed `onEnter` makes the session ending-only too, so exiting that
/// environment with `keep_session_running = true` must not reopen it.
#[tokio::test]
async fn failed_on_enter_then_exit_keep_running() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let failing = env("Env", Some(action("exit 3")), None);
    let id = "env-id";
    let err = s
        .enter_environment(&failing, None, Some(id), None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, SessionError::EnvironmentScriptFailed { .. }),
        "{err:?}"
    );
    assert_eq!(s.state(), SessionState::ReadyEnding);

    s.exit_environment(&id.to_string(), None, true, None)
        .await
        .unwrap();
    assert_eq!(s.state(), SessionState::ReadyEnding);
    assert_refuses_new_actions(&mut s).await;
}

/// A failed `onExit` with `keep_session_running = true` makes the session
/// ending-only, so exiting the next environment must not reopen it.
#[tokio::test]
async fn failed_on_exit_keep_running_then_exit_outer_keep_running() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let outer = s
        .enter_environment(&env("Outer", None, None), None, None, None)
        .await
        .unwrap();
    let inner = s
        .enter_environment(
            &env("Inner", None, Some(action("exit 4"))),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(s.exit_environment(&inner, None, true, None).await.is_err());
    assert_eq!(s.state(), SessionState::ReadyEnding);

    s.exit_environment(&outer, None, true, None).await.unwrap();
    assert_eq!(s.state(), SessionState::ReadyEnding);
    assert_refuses_new_actions(&mut s).await;
}

/// An action that fails to run at all (the runner returns an error, here a
/// command that does not exist) makes the session ending-only.
#[tokio::test]
async fn runner_error_then_exit_keep_running() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let id = s
        .enter_environment(&env("Env", None, None), None, None, None)
        .await
        .unwrap();
    let script = step_action(Action {
        command: fs("nonexistent-command-xyz-brittle"),
        args: None,
        timeout: None,
        cancelation: None,
    });
    let result = s.run_task("t", &script, None, None, None).await;
    let failed = match result {
        Err(_) => true,
        Ok(r) => r.state == ActionState::Failed,
    };
    assert!(failed, "a missing command must fail the action");
    assert_eq!(s.state(), SessionState::ReadyEnding);

    s.exit_environment(&id, None, true, None).await.unwrap();
    assert_eq!(s.state(), SessionState::ReadyEnding);
    assert_refuses_new_actions(&mut s).await;
}

/// A failure while setting up an action, before any subprocess starts (here
/// a format string that names an unknown symbol in the task's args), makes
/// the session ending-only.
#[tokio::test]
async fn setup_failure_then_exit_keep_running() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let id = s
        .enter_environment(&env("Env", None, None), None, None, None)
        .await
        .unwrap();
    let result = s
        .run_task("t", &step("echo {{ No.Such.Symbol }}"), None, None, None)
        .await;
    assert!(result.is_err(), "an unresolvable arg must fail the action");
    assert_eq!(s.state(), SessionState::ReadyEnding);
    assert_eq!(s.action_status().unwrap().state, ActionState::Failed);

    s.exit_environment(&id, None, true, None).await.unwrap();
    assert_eq!(s.state(), SessionState::ReadyEnding);
    assert_refuses_new_actions(&mut s).await;
}

/// A failed ad-hoc subprocess makes the session ending-only.
#[tokio::test]
async fn failed_subprocess_then_exit_keep_running() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let id = s
        .enter_environment(&env("Env", None, None), None, None, None)
        .await
        .unwrap();
    let r = s
        .run_subprocess(
            "sh",
            Some(&["-c".into(), "exit 5".into()]),
            None,
            None,
            true,
            None,
        )
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Failed);
    assert_eq!(s.state(), SessionState::ReadyEnding);

    s.exit_environment(&id, None, true, None).await.unwrap();
    assert_eq!(s.state(), SessionState::ReadyEnding);
    assert_refuses_new_actions(&mut s).await;
}

/// A cancel with `mark_action_failed = true` reports the action `Failed`,
/// and that also makes the session ending-only.
#[tokio::test(flavor = "multi_thread")]
async fn cancel_marked_failed_then_exit_keep_running() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let id = s
        .enter_environment(&env("Env", None, None), None, None, None)
        .await
        .unwrap();
    let handle = s.cancel_handle();
    tokio::spawn(async move {
        for _ in 0..200 {
            if handle.cancel(Some(Duration::from_secs(0)), true) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the task never became cancelable");
    });
    let r = s
        .run_task("t", &step("sleep 30"), None, None, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Failed);
    assert_eq!(s.state(), SessionState::ReadyEnding);

    s.exit_environment(&id, None, true, None).await.unwrap();
    assert_eq!(s.state(), SessionState::ReadyEnding);
    assert_refuses_new_actions(&mut s).await;
}

/// Control: when every action succeeds, `keep_session_running = true` keeps
/// the session usable after an exit, and `false` makes it ending-only.
#[tokio::test]
async fn successful_actions_keep_running_controls_reopening() {
    for (keep, expected) in [
        (true, SessionState::Ready),
        (false, SessionState::ReadyEnding),
    ] {
        let tmp = TempDir::new().unwrap();
        let mut s = Session::new_for_test(tmp.path().to_path_buf());
        let id = s
            .enter_environment(
                &env("Env", None, Some(action("echo bye"))),
                None,
                None,
                None,
            )
            .await
            .unwrap();
        let r = s
            .run_task("t", &step("echo ok"), None, None, None)
            .await
            .unwrap();
        assert_eq!(r.state, ActionState::Success);
        assert_eq!(s.state(), SessionState::Ready);
        s.exit_environment(&id, None, keep, None).await.unwrap();
        assert_eq!(s.state(), expected, "keep_session_running = {keep}");
        if expected == SessionState::Ready {
            let r = s
                .run_task("t2", &step("echo again"), None, None, None)
                .await
                .unwrap();
            assert_eq!(r.state, ActionState::Success);
            assert_eq!(s.state(), SessionState::Ready);
        } else {
            assert_refuses_new_actions(&mut s).await;
        }
    }
}
