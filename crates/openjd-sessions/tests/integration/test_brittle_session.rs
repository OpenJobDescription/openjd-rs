// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Brittle sessions (specs/sessions/session.md § Brittle Sessions).
//!
//! Once an action ends `Failed`, `Canceled` or `Timeout`, the session is
//! ending-only: it accepts only `exit_environment`, `cleanup` and
//! `clear_ending_only`, and only `clear_ending_only` brings it back to
//! `Ready`. That must hold whatever the caller later passes as
//! `keep_session_running`, and whether or not the exited environment has an
//! `onExit` action.

use openjd_expr::format_string::FormatString;
use openjd_model::job::{
    Action, Environment, EnvironmentActions, EnvironmentScript, StepActions, StepScript,
};
use openjd_sessions::action::ActionState;
use openjd_sessions::error::SessionError;
use openjd_sessions::session::{Session, SessionState};
use std::collections::{BTreeMap, HashMap};
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

/// A runner error while resolving the action's command line (here a format
/// string that names an unknown symbol in the task's args) makes the
/// session ending-only.
#[tokio::test]
async fn unresolvable_arg_then_exit_keep_running() {
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

/// A failure while setting up an action, before the runner starts, makes
/// the session ending-only. Here a wrap environment's `onWrapTaskRun`
/// intercepts the task, and seeding `WrappedAction.Timeout` fails because
/// the task's `onRun.timeout` names an unknown symbol.
#[tokio::test]
async fn setup_failure_then_exit_keep_running() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let mut wrap = env("Wrap", None, None);
    wrap.script.as_mut().unwrap().actions.on_wrap_task_run = Some(action("echo wrapped"));
    let id = s.enter_environment(&wrap, None, None, None).await.unwrap();
    let script = step_action(Action {
        timeout: Some(fs("{{ Param.Missing }}")),
        ..action("echo inner")
    });
    let err = s
        .run_task("t", &script, None, None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, SessionError::FormatString { .. }), "{err:?}");
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

// ── clear_ending_only ───────────────────────────────────────────────────
//
// `clear_ending_only()` is the only way from `ReadyEnding` back to `Ready`.
// It is allowed only in `ReadyEnding`, clears ending-only mode whatever set
// it, and changes nothing else.

/// Assert that the session accepts new work after `clear_ending_only()`:
/// a task runs, and a new environment can be entered and exited. Leaves the
/// session `Ready` with the same environments entered as before.
async fn assert_accepts_new_actions(s: &mut Session) {
    let entered = s.environments_entered().to_vec();
    let r = s
        .run_task("t2", &step("echo again"), None, None, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    assert_eq!(s.state(), SessionState::Ready);

    let new_id = s
        .enter_environment(
            &env("New", Some(action("echo new")), None),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(s.state(), SessionState::Ready);
    let mut expected = entered.clone();
    expected.push(new_id.clone());
    assert_eq!(s.environments_entered(), expected.as_slice());

    s.exit_environment(&new_id, None, true, None).await.unwrap();
    assert_eq!(s.state(), SessionState::Ready);
    assert_eq!(s.environments_entered(), entered.as_slice());
}

/// An environment with no actions that defines `CLEAR_TEST_VAR=value`.
fn env_with_var(name: &str, value: &str) -> Environment {
    Environment {
        name: name.into(),
        description: None,
        script: None,
        variables: Some(HashMap::from([("CLEAR_TEST_VAR".into(), fs(value))])),
        resolved_symtab: None,
    }
}

/// Assert that a task sees `CLEAR_TEST_VAR=value`, so the variables of the
/// environments still entered reach new actions.
async fn assert_task_sees_var(s: &mut Session, value: &str) {
    let r = s
        .run_task(
            "t-var",
            &step(&format!("test \"$CLEAR_TEST_VAR\" = {value}")),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        r.state,
        ActionState::Success,
        "CLEAR_TEST_VAR must be {value}"
    );
    assert_eq!(s.state(), SessionState::Ready);
}

/// Call `clear_ending_only()` on a `ReadyEnding` session and check that it
/// moves to `Ready` without touching the entered environments, the last
/// action's status or the environment variables.
fn clear_and_check(s: &mut Session) {
    assert_eq!(s.state(), SessionState::ReadyEnding);
    let entered = s.environments_entered().to_vec();
    let env_vars: BTreeMap<_, _> = s.evaluate_env_vars(None).into_iter().collect();
    // ActionStatus has no PartialEq; its Debug form covers every field.
    let status = format!("{:?}", s.action_status());
    s.clear_ending_only().unwrap();
    assert_eq!(s.state(), SessionState::Ready);
    assert_eq!(s.environments_entered(), entered.as_slice());
    assert_eq!(format!("{:?}", s.action_status()), status);
    let env_vars_after: BTreeMap<_, _> = s.evaluate_env_vars(None).into_iter().collect();
    assert_eq!(env_vars_after, env_vars);
}

/// Enter two environments and run a task that ends in `ending`. If
/// `exit_inner_first`, exit the inner environment with
/// `keep_session_running = true` before clearing, which on its own must
/// leave the session `ReadyEnding`. Then `clear_ending_only()` must bring
/// the session back to `Ready` so it accepts new work.
async fn check_clear_after_task_ending(ending: Ending, exit_inner_first: bool) {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let mut outer_env = env("Outer", None, Some(action("echo outer-exited")));
    outer_env.variables = env_with_var("Outer", "outer").variables;
    let outer = s
        .enter_environment(&outer_env, None, None, None)
        .await
        .unwrap();
    let inner = s
        .enter_environment(&env_with_var("Inner", "inner"), None, None, None)
        .await
        .unwrap();
    run_ending_task(&mut s, ending).await;
    if exit_inner_first {
        s.exit_environment(&inner, None, true, None).await.unwrap();
        assert_eq!(
            s.state(),
            SessionState::ReadyEnding,
            "keep_session_running = true alone must not leave ending-only"
        );
        assert_refuses_new_actions(&mut s).await;
    }

    clear_and_check(&mut s);
    let expected = if exit_inner_first {
        vec![outer.clone()]
    } else {
        vec![outer.clone(), inner.clone()]
    };
    assert_eq!(s.environments_entered(), expected.as_slice());
    // The variables of exactly the environments still entered reach new
    // actions: the exited inner one's override is gone, a kept one stays.
    assert_task_sees_var(&mut s, if exit_inner_first { "outer" } else { "inner" }).await;
    assert_accepts_new_actions(&mut s).await;

    // An exited environment stays exited, and the rest exit in LIFO order.
    if exit_inner_first {
        match s.exit_environment(&inner, None, true, None).await {
            Err(SessionError::UnknownEnvironment { identifier }) => assert_eq!(identifier, inner),
            other => panic!("the exited environment must stay exited, got {other:?}"),
        }
    } else {
        s.exit_environment(&inner, None, true, None).await.unwrap();
    }
    let out = s.exit_environment(&outer, None, false, None).await.unwrap();
    assert!(out.contains("outer-exited"), "{out:?}");
    assert_eq!(s.state(), SessionState::ReadyEnding);
    assert!(s.environments_entered().is_empty());
}

macro_rules! clear_after_task_ending_tests {
    ($($name:ident: $ending:expr, $exit_inner_first:expr;)*) => {$(
        #[tokio::test(flavor = "multi_thread")]
        async fn $name() {
            check_clear_after_task_ending($ending, $exit_inner_first).await;
        }
    )*};
}

clear_after_task_ending_tests! {
    clear_ending_only_after_failed_task: Ending::Failed, false;
    clear_ending_only_after_failed_task_and_keep_running_exit: Ending::Failed, true;
    clear_ending_only_after_canceled_task: Ending::Canceled, false;
    clear_ending_only_after_canceled_task_and_keep_running_exit: Ending::Canceled, true;
    clear_ending_only_after_timeout_task: Ending::Timeout, false;
    clear_ending_only_after_timeout_task_and_keep_running_exit: Ending::Timeout, true;
}

/// `clear_ending_only()` also clears ending-only mode set by a default
/// `exit_environment` (`keep_session_running = false`) after successful
/// actions, with or without an `onExit`.
#[tokio::test]
async fn clear_ending_only_after_default_exit() {
    for on_exit in [None, Some(action("echo bye"))] {
        let tmp = TempDir::new().unwrap();
        let mut s = Session::new_for_test(tmp.path().to_path_buf());
        let outer = s
            .enter_environment(&env_with_var("Outer", "outer"), None, None, None)
            .await
            .unwrap();
        let mut inner_env = env("Inner", None, on_exit.clone());
        inner_env.variables = env_with_var("Inner", "inner").variables;
        let inner = s
            .enter_environment(&inner_env, None, None, None)
            .await
            .unwrap();
        s.exit_environment(&inner, None, false, None).await.unwrap();
        assert_eq!(s.state(), SessionState::ReadyEnding);
        assert_refuses_new_actions(&mut s).await;

        clear_and_check(&mut s);
        assert_eq!(s.environments_entered(), std::slice::from_ref(&outer));
        assert_task_sees_var(&mut s, "outer").await;
        assert_accepts_new_actions(&mut s).await;
    }
}

/// After `clear_ending_only()`, a later failure makes the session
/// ending-only again, and it can be cleared again.
#[tokio::test]
async fn clear_ending_only_then_fail_again() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let id = s
        .enter_environment(&env("Env", None, None), None, None, None)
        .await
        .unwrap();
    run_ending_task(&mut s, Ending::Failed).await;
    clear_and_check(&mut s);

    run_ending_task(&mut s, Ending::Failed).await;
    assert_refuses_new_actions(&mut s).await;
    s.exit_environment(&id, None, true, None).await.unwrap();
    assert_eq!(s.state(), SessionState::ReadyEnding);

    clear_and_check(&mut s);
    assert_accepts_new_actions(&mut s).await;
}

/// Assert that `clear_ending_only()` fails with `InvalidState` in the
/// session's current state and leaves that state unchanged.
fn assert_clear_refused(s: &mut Session) {
    let current = s.state();
    let entered = s.environments_entered().to_vec();
    let err = s.clear_ending_only().unwrap_err();
    assert_eq!(
        err.to_string(),
        format!("Session must be in READY_ENDING state, current: {current}")
    );
    match err {
        SessionError::InvalidState {
            expected,
            current: reported,
        } => {
            assert_eq!(expected, vec![SessionState::ReadyEnding]);
            assert_eq!(reported, current);
        }
        other => panic!("expected InvalidState, got {other:?}"),
    }
    assert_eq!(s.state(), current);
    assert_eq!(s.environments_entered(), entered.as_slice());
}

/// `clear_ending_only()` is refused in `Ready`, both on a new session and
/// after work has run.
#[tokio::test]
async fn clear_ending_only_refused_in_ready() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    assert_clear_refused(&mut s);

    let _id = s
        .enter_environment(&env("Env", None, None), None, None, None)
        .await
        .unwrap();
    let r = s
        .run_task("t", &step("echo ok"), None, None, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    assert_clear_refused(&mut s);
    assert_accepts_new_actions(&mut s).await;
}

/// `clear_ending_only()` is refused in `Running` and `Canceling`, and the
/// refusal leaves ending-only mode set: once the state is forced back, a
/// successful action still settles to `ReadyEnding`.
///
/// `run_task` holds `&mut Session` for the whole action, so these states
/// can't be observed from outside a real action. They are forced with
/// `set_state_for_test`.
#[tokio::test]
async fn clear_ending_only_refused_in_running_and_canceling() {
    for forced in [SessionState::Running, SessionState::Canceling] {
        let tmp = TempDir::new().unwrap();
        let mut s = Session::new_for_test(tmp.path().to_path_buf());
        run_ending_task(&mut s, Ending::Failed).await;

        s.set_state_for_test(forced);
        assert_clear_refused(&mut s);

        // Still ending-only: a successful action settles to ReadyEnding.
        s.set_state_for_test(SessionState::Ready);
        let r = s
            .run_task("t2", &step("echo ok"), None, None, None)
            .await
            .unwrap();
        assert_eq!(r.state, ActionState::Success);
        assert_eq!(
            s.state(),
            SessionState::ReadyEnding,
            "a refused clear_ending_only() in {forced:?} must not clear ending-only mode"
        );
    }
}

/// `clear_ending_only()` is refused in `Ended`, whether or not the session
/// was ending-only before `cleanup()`.
#[tokio::test]
async fn clear_ending_only_refused_in_ended() {
    for make_ending_only in [false, true] {
        let tmp = TempDir::new().unwrap();
        let mut s = Session::new_for_test(tmp.path().to_path_buf());
        if make_ending_only {
            run_ending_task(&mut s, Ending::Failed).await;
        }
        s.cleanup();
        assert_eq!(s.state(), SessionState::Ended);
        assert_clear_refused(&mut s);
    }
}

/// A `SessionConfig` for the tests that need `Session::with_config`.
fn config_for_test(tmp: &TempDir) -> openjd_sessions::session::SessionConfig {
    openjd_sessions::session::SessionConfig {
        limits: Default::default(),
        session_id: "clear-ending-only".into(),
        job_parameter_values: HashMap::new(),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: None,
        os_env_vars: None,
        session_root_directory: Some(tmp.path().to_path_buf()),
        user: None,
        profile: None,
        cancel_token: None,
        debug_collect_stdout: false,
        echo_openjd_directives: false,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    }
}

/// `clear_ending_only()` keeps the variables the session was constructed
/// with (`SessionConfig::os_env_vars`), and new actions still see them.
#[tokio::test]
async fn clear_ending_only_keeps_session_env_vars() {
    let tmp = TempDir::new().unwrap();
    let config = openjd_sessions::session::SessionConfig {
        os_env_vars: Some(HashMap::from([("CLEAR_TEST_VAR".into(), "ctor".into())])),
        ..config_for_test(&tmp)
    };
    let mut s = Session::with_config(config).unwrap();
    run_ending_task(&mut s, Ending::Failed).await;

    clear_and_check(&mut s);
    assert_task_sees_var(&mut s, "ctor").await;
    s.cleanup();
}

/// `clear_ending_only()` keeps every field of the failed action's status,
/// the values the session redacts, and the working directory that
/// `cleanup()` later removes. The failing task reports progress, a status
/// message, a fail message and a redacted value first, so none of the
/// before/after comparisons is between two `None`s.
#[tokio::test]
async fn clear_ending_only_keeps_action_status_and_redactions() {
    let tmp = TempDir::new().unwrap();
    let mut s = Session::new_for_test(tmp.path().to_path_buf());
    let id = s
        .enter_environment(
            &env(
                "Env",
                Some(action(
                    "echo 'openjd_redacted_env: CLEAR_TEST_SECRET=hunter2-clear'",
                )),
                None,
            ),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    let r = s
        .run_task(
            "t",
            &step(
                "echo 'openjd_progress: 50.0'; echo 'openjd_status: halfway'; \
                 echo 'openjd_fail: clear-test failure'; exit 1",
            ),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Failed);
    let status = s.action_status().unwrap();
    assert_eq!(status.progress, Some(50.0));
    assert_eq!(status.status_message.as_deref(), Some("halfway"));
    assert!(status.fail_message.is_some(), "{status:?}");
    assert!(status.exit_code.is_some(), "{status:?}");
    assert!(status.ended_at.is_some(), "{status:?}");
    assert_eq!(s.redact("x hunter2-clear y"), "x ******** y");

    clear_and_check(&mut s);
    assert_eq!(s.redact("x hunter2-clear y"), "x ******** y");

    s.exit_environment(&id, None, false, None).await.unwrap();
    let working_dir = s.working_directory().to_path_buf();
    assert!(working_dir.exists());
    s.cleanup();
    assert!(
        !working_dir.exists(),
        "cleanup() after a clear must remove {working_dir:?}"
    );
}

/// After the session-wide `SessionConfig::cancel_token` is canceled,
/// `clear_ending_only()` refuses with `SessionError::SessionCanceled` and
/// leaves the session `ReadyEnding`. `exit_environment` and `cleanup` still
/// work after that refusal.
#[tokio::test]
async fn clear_ending_only_refused_after_session_cancel_token() {
    let tmp = TempDir::new().unwrap();
    let token = tokio_util::sync::CancellationToken::new();
    let config = openjd_sessions::session::SessionConfig {
        cancel_token: Some(token.clone()),
        ..config_for_test(&tmp)
    };
    let mut s = Session::with_config(config).unwrap();
    let id = s
        .enter_environment(&env("Env", None, None), None, None, None)
        .await
        .unwrap();
    token.cancel();

    // The canceled session-wide token makes the next action end Canceled,
    // which leaves the session ending-only.
    let r = s
        .run_task("t", &step("sleep 30"), None, None, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Canceled);
    assert_eq!(s.state(), SessionState::ReadyEnding);

    // clear_ending_only() refuses and changes nothing.
    let err = s.clear_ending_only().unwrap_err();
    assert_eq!(
        err.to_string(),
        "The session was canceled through its session-wide cancel token and cannot be continued."
    );
    assert!(matches!(err, SessionError::SessionCanceled));
    assert_eq!(s.state(), SessionState::ReadyEnding);
    assert_refuses_new_actions(&mut s).await;

    // A second refusal is identical — the state is untouched.
    assert!(matches!(
        s.clear_ending_only().unwrap_err(),
        SessionError::SessionCanceled
    ));
    assert_eq!(s.state(), SessionState::ReadyEnding);

    // exit_environment and cleanup still work after the refusal.
    let out = s.exit_environment(&id, None, false, None).await.unwrap();
    assert!(out.is_empty(), "no onExit action to run: {out:?}");
    assert_eq!(s.state(), SessionState::ReadyEnding);
    let working_dir = s.working_directory().to_path_buf();
    assert!(working_dir.exists());
    s.cleanup();
    assert_eq!(s.state(), SessionState::Ended);
    assert!(
        !working_dir.exists(),
        "cleanup() after the refusal must remove {working_dir:?}"
    );
}

/// A session-wide `SessionConfig::cancel_token` that exists but was never
/// canceled doesn't block `clear_ending_only()`: a failure makes the
/// session ending-only, and clearing still returns it to `Ready`.
#[tokio::test]
async fn clear_ending_only_with_uncanceled_session_token() {
    let tmp = TempDir::new().unwrap();
    let token = tokio_util::sync::CancellationToken::new();
    let config = openjd_sessions::session::SessionConfig {
        cancel_token: Some(token.clone()),
        ..config_for_test(&tmp)
    };
    let mut s = Session::with_config(config).unwrap();
    let _id = s
        .enter_environment(&env("Env", None, None), None, None, None)
        .await
        .unwrap();
    run_ending_task(&mut s, Ending::Failed).await;

    // The token is still live, so clearing works.
    assert!(!token.is_cancelled());
    clear_and_check(&mut s);
    assert_accepts_new_actions(&mut s).await;
    s.cleanup();
}

/// A per-action cancel (`cancel_action` / `SessionCancelHandle`), unlike a
/// session-wide one, doesn't block `clear_ending_only()`. The canceled task
/// makes the session ending-only, and clearing returns it to `Ready` so it
/// accepts new work — even with a live (uncanceled) session-wide token in
/// place.
#[tokio::test(flavor = "multi_thread")]
async fn clear_ending_only_after_per_action_cancel_with_session_token() {
    let tmp = TempDir::new().unwrap();
    let token = tokio_util::sync::CancellationToken::new();
    let config = openjd_sessions::session::SessionConfig {
        cancel_token: Some(token.clone()),
        ..config_for_test(&tmp)
    };
    let mut s = Session::with_config(config).unwrap();
    let _id = s
        .enter_environment(&env("Env", None, None), None, None, None)
        .await
        .unwrap();
    // run_ending_task cancels through a per-action SessionCancelHandle.
    run_ending_task(&mut s, Ending::Canceled).await;

    // The session-wide token was never canceled, so clearing works.
    assert!(!token.is_cancelled());
    clear_and_check(&mut s);
    assert_accepts_new_actions(&mut s).await;
    s.cleanup();
}
