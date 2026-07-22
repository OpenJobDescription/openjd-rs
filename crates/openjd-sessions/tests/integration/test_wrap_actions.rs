// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! End-to-end tests for RFC 0008 session runtime routing.
//!
//! Each test uses a temp-file marker pattern: actions append a tagged line
//! to a trace file, and we assert on the file contents to prove which
//! hook ran where. This mirrors the conformance tests described in the
//! RFC so implementations can be validated against identical expectations.
//!
//! All tests use `bash` + `echo` + file append so no external toolchain
//! (python, docker, etc.) is required.

#![cfg(unix)] // bash/sh availability

use openjd_expr::format_string::FormatString;
use openjd_model::job::{
    Action, Environment, EnvironmentActions, EnvironmentScript, StepActions, StepScript,
};
use openjd_sessions::action::ActionState;
use openjd_sessions::session::Session;
use std::path::PathBuf;
use tempfile::TempDir;

// ────────────────────────────────────────────────────────────────────
// Construction helpers
// ────────────────────────────────────────────────────────────────────

fn fs(s: &str) -> FormatString {
    FormatString::new(s).unwrap()
}

fn action_with_command(command: &str, args: Vec<&str>) -> Action {
    Action {
        command: fs(command),
        args: Some(args.iter().map(|a| fs(a)).collect()),
        timeout: None,
        cancelation: None,
    }
}

fn plain_env(name: &str, on_enter: Option<Action>, on_exit: Option<Action>) -> Environment {
    Environment {
        name: name.to_string(),
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

fn fs_map(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, FormatString> {
    pairs.iter().map(|(k, v)| (k.to_string(), fs(v))).collect()
}

/// Build a wrap environment with the three optional wrap hooks plus its
/// own `on_enter`. We always set `on_enter` so the session can track the
/// env through its lifecycle (a session ENTER with no action is allowed
/// but makes the tests harder to reason about).
fn wrap_env(
    name: &str,
    on_enter: Action,
    on_wrap_env_enter: Option<Action>,
    on_wrap_task_run: Option<Action>,
    on_wrap_env_exit: Option<Action>,
) -> Environment {
    Environment {
        name: name.to_string(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: Some(on_enter),
                on_wrap_env_enter,
                on_wrap_task_run,
                on_wrap_env_exit,
                on_exit: None,
            },
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    }
}

fn step(command: &str, args: Vec<&str>) -> StepScript {
    StepScript {
        let_bindings: None,
        actions: StepActions {
            on_run: action_with_command(command, args),
        },
        embedded_files: None,
    }
}

fn read_trace(path: &PathBuf) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

// ────────────────────────────────────────────────────────────────────
// onWrapTaskRun
// ────────────────────────────────────────────────────────────────────

/// Baseline: no wrap environment in the session → the task runs
/// unwrapped. The marker file contains only the task's line.
#[tokio::test]
async fn baseline_task_runs_unwrapped_when_no_wrap_env() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    let cmd = format!("echo task-direct >> '{}'", trace.display());
    let s = step("sh", vec!["-c", &cmd]);
    let result = session
        .run_task("test_step", &s, None, None, None)
        .await
        .unwrap();
    assert_eq!(result.state, ActionState::Success);

    let contents = read_trace(&trace);
    assert_eq!(contents.trim(), "task-direct");
}

/// When a wrap env with `onWrapTaskRun` is active, the task's `onRun`
/// is replaced. `WrappedAction.Command` / `WrappedAction.Args` carry the
/// original command forward so the wrap script can re-invoke it.
#[tokio::test]
async fn task_run_wrapped_by_active_wrap_env() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    // Wrap script:
    //   1. Write a [WRAPPED] marker line so we can prove interception.
    //   2. Re-invoke the wrapped command via WrappedAction.Command /
    //      WrappedAction.Args, which demonstrates the template variable
    //      registration works all the way through to subprocess spawn.
    let wrap_script = format!(
        r#"
        echo "[WRAPPED] cmd={{{{WrappedAction.Command}}}}" >> '{path}'
        {{{{WrappedAction.Command}}}} "${{@}}"
        "#,
        path = trace.display(),
    );
    // The wrap action uses `bash -c SCRIPT --` so positional args start
    // at $1 and can be forwarded via "$@". We pass the wrapped command's
    // args with WrappedAction.Args (a list), which the session expands
    // into separate argv entries when resolving `args:`.
    let wrap = Action {
        command: fs("bash"),
        args: Some(vec![
            fs("-c"),
            fs(&wrap_script),
            fs("--"),
            fs("{{WrappedAction.Args}}"),
        ]),
        timeout: None,
        cancelation: None,
    };
    let env = wrap_env(
        "Wrapper",
        action_with_command("true", vec![]),
        None,
        Some(wrap),
        None,
    );
    session
        .enter_environment(&env, None, None, None)
        .await
        .unwrap();

    let task_cmd = format!("echo task-inner >> '{}'", trace.display());
    let s = step("sh", vec!["-c", &task_cmd]);
    let result = session
        .run_task("test_step", &s, None, None, None)
        .await
        .unwrap();
    assert_eq!(result.state, ActionState::Success);

    let contents = read_trace(&trace);
    // The wrap line proves interception, the task line proves forwarding.
    assert!(
        contents.contains("[WRAPPED] cmd=sh"),
        "expected [WRAPPED] marker; got:\n{contents}"
    );
    assert!(
        contents.contains("task-inner"),
        "expected forwarded task output; got:\n{contents}"
    );
}

// ────────────────────────────────────────────────────────────────────
// onWrapEnvEnter / onWrapEnvExit
// ────────────────────────────────────────────────────────────────────

/// With a wrap env defining `onWrapEnvEnter`, an inner environment's
/// `onEnter` is intercepted. The wrap script sees `WrappedEnv.Name`
/// resolved to the inner env's name.
#[tokio::test]
async fn wrap_env_enter_intercepts_inner_on_enter() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    let wrap_env_enter_script = format!(
        r#"echo "[WRAPPED-ENTER] env={{{{WrappedEnv.Name}}}}" >> '{path}'"#,
        path = trace.display(),
    );
    let wrap_env_enter = Action {
        command: fs("bash"),
        args: Some(vec![fs("-c"), fs(&wrap_env_enter_script)]),
        timeout: None,
        cancelation: None,
    };
    let outer = wrap_env(
        "Outer",
        action_with_command("true", vec![]),
        Some(wrap_env_enter),
        None,
        None,
    );
    session
        .enter_environment(&outer, None, None, None)
        .await
        .unwrap();

    // Inner env's onEnter — should be intercepted by Outer.onWrapEnvEnter.
    let inner_cmd = format!("echo inner-enter-body >> '{}'", trace.display());
    let inner = plain_env(
        "Inner",
        Some(action_with_command("sh", vec!["-c", &inner_cmd])),
        None,
    );
    session
        .enter_environment(&inner, None, None, None)
        .await
        .unwrap();

    let contents = read_trace(&trace);
    assert!(
        contents.contains("[WRAPPED-ENTER] env=Inner"),
        "expected wrapped-enter marker with inner env's name; got:\n{contents}"
    );
    // The inner body does NOT run when wrapped — the wrap hook replaces
    // the action entirely (the RFC's semantics: the wrap script is
    // responsible for forwarding if it wants the wrapped body to run).
    assert!(
        !contents.contains("inner-enter-body"),
        "inner body should be replaced by wrap script; got:\n{contents}"
    );
}

/// An outer environment's own `onEnter` is never wrapped by its own
/// `onWrapEnvEnter`. Regression-style test — proves the dispatch excludes
/// the entering env from the wrap-env lookup.
#[tokio::test]
async fn wrap_env_enter_does_not_wrap_outer_env_itself() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    let outer_enter_cmd = format!("echo outer-enter-body >> '{}'", trace.display());
    let wrap_env_enter_script = format!(
        r#"echo "[WRAPPED-ENTER]" >> '{path}'"#,
        path = trace.display(),
    );
    let wrap_env_enter = Action {
        command: fs("bash"),
        args: Some(vec![fs("-c"), fs(&wrap_env_enter_script)]),
        timeout: None,
        cancelation: None,
    };
    let outer = wrap_env(
        "Outer",
        action_with_command("sh", vec!["-c", &outer_enter_cmd]),
        Some(wrap_env_enter),
        None,
        None,
    );
    session
        .enter_environment(&outer, None, None, None)
        .await
        .unwrap();

    let contents = read_trace(&trace);
    assert!(
        contents.contains("outer-enter-body"),
        "outer env's own onEnter must run; got:\n{contents}"
    );
    assert!(
        !contents.contains("[WRAPPED-ENTER]"),
        "outer's own onEnter must not be wrapped by its own onWrapEnvEnter; got:\n{contents}"
    );
}

/// `onWrapEnvExit` intercepts an inner environment's `onExit`. Verify both
/// the interception and that `WrappedEnv.Name` resolves to the inner
/// env being exited.
#[tokio::test]
async fn wrap_env_exit_intercepts_inner_on_exit() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    let wrap_env_exit_script = format!(
        r#"echo "[WRAPPED-EXIT] env={{{{WrappedEnv.Name}}}}" >> '{path}'"#,
        path = trace.display(),
    );
    let wrap_env_exit = Action {
        command: fs("bash"),
        args: Some(vec![fs("-c"), fs(&wrap_env_exit_script)]),
        timeout: None,
        cancelation: None,
    };
    let outer = wrap_env(
        "Outer",
        action_with_command("true", vec![]),
        None,
        None,
        Some(wrap_env_exit),
    );
    session
        .enter_environment(&outer, None, None, None)
        .await
        .unwrap();

    let inner_exit_cmd = format!("echo inner-exit-body >> '{}'", trace.display());
    let inner = plain_env(
        "Inner",
        Some(action_with_command("true", vec![])),
        Some(action_with_command("sh", vec!["-c", &inner_exit_cmd])),
    );
    let inner_id = session
        .enter_environment(&inner, None, None, None)
        .await
        .unwrap();

    session
        .exit_environment(&inner_id, None, true, None)
        .await
        .unwrap();

    let contents = read_trace(&trace);
    assert!(
        contents.contains("[WRAPPED-EXIT] env=Inner"),
        "expected wrapped-exit marker with inner env's name; got:\n{contents}"
    );
    assert!(
        !contents.contains("inner-exit-body"),
        "inner body should be replaced by wrap script; got:\n{contents}"
    );
}

/// Full-cycle integration: outer wrap env + one wrapped inner env + a
/// wrapped task. The final trace shows the expected routing for every
/// lifecycle phase under the all-three wrap hooks.
#[tokio::test]
async fn full_cycle_wraps_every_inner_action() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    let mark = |label: &str| -> String { format!(r#"echo "{label}" >> '{}'"#, trace.display()) };

    let wrap_env_enter = Action {
        command: fs("bash"),
        args: Some(vec![
            fs("-c"),
            fs(&mark("[wrap-enter] {{WrappedEnv.Name}}")),
        ]),
        timeout: None,
        cancelation: None,
    };
    let wrap_task_run = Action {
        command: fs("bash"),
        args: Some(vec![
            fs("-c"),
            fs(&mark("[wrap-task] cmd={{WrappedAction.Command}}")),
        ]),
        timeout: None,
        cancelation: None,
    };
    let wrap_env_exit = Action {
        command: fs("bash"),
        args: Some(vec![fs("-c"), fs(&mark("[wrap-exit] {{WrappedEnv.Name}}"))]),
        timeout: None,
        cancelation: None,
    };
    let outer = wrap_env(
        "Outer",
        Action {
            command: fs("bash"),
            args: Some(vec![fs("-c"), fs(&mark("outer-enter-host"))]),
            timeout: None,
            cancelation: None,
        },
        Some(wrap_env_enter),
        Some(wrap_task_run),
        Some(wrap_env_exit),
    );
    session
        .enter_environment(&outer, None, None, None)
        .await
        .unwrap();

    // Inner: wrapped by Outer through all three hooks.
    let inner = plain_env(
        "WrappedInner",
        Some(action_with_command(
            "bash",
            vec!["-c", &mark("WrappedInner-enter-body")],
        )),
        Some(action_with_command(
            "bash",
            vec!["-c", &mark("WrappedInner-exit-body")],
        )),
    );
    let id = session
        .enter_environment(&inner, None, None, None)
        .await
        .unwrap();

    // Run a wrapped task.
    let task_script = step("bash", vec!["-c", &mark("task-body")]);
    session
        .run_task("test_step", &task_script, None, None, None)
        .await
        .unwrap();

    session
        .exit_environment(&id, None, true, None)
        .await
        .unwrap();

    let contents = read_trace(&trace);
    // The outer env's own enter runs on the host (its own action was NOT
    // wrapped — it has no outer wrap env).
    assert!(contents.contains("outer-enter-host"), "trace:\n{contents}");
    // Inner.onEnter is wrapped — body does not appear, wrap does.
    assert!(
        contents.contains("[wrap-enter] WrappedInner"),
        "trace:\n{contents}"
    );
    assert!(
        !contents.contains("WrappedInner-enter-body"),
        "wrapped onEnter body must not run: {contents}"
    );
    // Task is wrapped — body does not appear, wrap-task does.
    assert!(
        contents.contains("[wrap-task] cmd=bash"),
        "trace:\n{contents}"
    );
    assert!(
        !contents.contains("task-body"),
        "wrapped task body must not run: {contents}"
    );
    // Inner.onExit wrapped.
    assert!(
        contents.contains("[wrap-exit] WrappedInner"),
        "trace:\n{contents}"
    );
    assert!(
        !contents.contains("WrappedInner-exit-body"),
        "wrapped onExit body must not run: {contents}"
    );
}

/// RFC 0008: `WrappedAction.*` is in scope inside all three wrap hooks
/// (not just `onWrapTaskRun`). Verify that `onWrapEnvEnter` and `onWrapEnvExit`
/// see the inner environment's `onEnter`/`onExit` command and args via
/// `WrappedAction.Command` and `WrappedAction.Args`.
#[tokio::test]
async fn wrapped_action_visible_in_wrap_env_enter_and_wrap_env_exit() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    let mark = |label: &str| -> String { format!(r#"echo "{label}" >> '{}'"#, trace.display()) };

    let wrap_env_enter = Action {
        command: fs("bash"),
        args: Some(vec![
            fs("-c"),
            fs(&mark("[wrap-enter] cmd={{WrappedAction.Command}}")),
        ]),
        timeout: None,
        cancelation: None,
    };
    let wrap_env_exit = Action {
        command: fs("bash"),
        args: Some(vec![
            fs("-c"),
            fs(&mark("[wrap-exit] cmd={{WrappedAction.Command}}")),
        ]),
        timeout: None,
        cancelation: None,
    };
    let outer = wrap_env(
        "Outer",
        action_with_command("true", vec![]),
        Some(wrap_env_enter),
        None,
        Some(wrap_env_exit),
    );
    session
        .enter_environment(&outer, None, None, None)
        .await
        .unwrap();

    // Inner env with distinct enter/exit commands so the trace can tell
    // them apart. The wrapped commands themselves never run; only their
    // names should appear in the trace via WrappedAction.Command.
    let inner = plain_env(
        "Inner",
        Some(action_with_command("inner-enter-cmd", vec![])),
        Some(action_with_command("inner-exit-cmd", vec![])),
    );
    let id = session
        .enter_environment(&inner, None, None, None)
        .await
        .unwrap();
    session
        .exit_environment(&id, None, true, None)
        .await
        .unwrap();

    let contents = read_trace(&trace);
    assert!(
        contents.contains("[wrap-enter] cmd=inner-enter-cmd"),
        "WrappedAction.Command should resolve in onWrapEnvEnter; got:\n{contents}"
    );
    assert!(
        contents.contains("[wrap-exit] cmd=inner-exit-cmd"),
        "WrappedAction.Command should resolve in onWrapEnvExit; got:\n{contents}"
    );
}

// ────────────────────────────────────────────────────────────────────
// WrappedAction.Cancelation.* (RFC 0008, PR #148)
// ────────────────────────────────────────────────────────────────────

/// Build a wrap `onWrapTaskRun` script that echoes `MODE=<...>`,
/// `MODE_OR=<...>` (null-coalesced so a null Mode is observable as
/// `UNDECLARED` rather than rendering empty), and `NP=<...>`
/// (delimiter-wrapped so empty/null values render as `<>` unambiguously)
/// into the given trace path.
fn cancel_probe_action(trace: &std::path::Path) -> Action {
    let script = format!(
        r#"
        echo "MODE=<{{{{WrappedAction.Cancelation.Mode}}}}>" >> '{path}'
        echo "MODE_OR=<{{{{ WrappedAction.Cancelation.Mode or 'UNDECLARED' }}}}>" >> '{path}'
        echo "NP=<{{{{WrappedAction.Cancelation.NotifyPeriodInSeconds}}}}>" >> '{path}'
        "#,
        path = trace.display(),
    );
    Action {
        command: fs("bash"),
        args: Some(vec![fs("-c"), fs(&script)]),
        timeout: None,
        cancelation: None,
    }
}

/// A task's `onRun` that carries `CancelationMode::Terminate` surfaces
/// as `WrappedAction.Cancelation.Mode = "TERMINATE"` with
/// `WrappedAction.Cancelation.NotifyPeriodInSeconds = null`.
#[tokio::test]
async fn wrap_task_run_forwards_cancelation_mode_terminate() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    let env = wrap_env(
        "Wrapper",
        action_with_command("true", vec![]),
        None,
        Some(cancel_probe_action(&trace)),
        None,
    );
    session
        .enter_environment(&env, None, None, None)
        .await
        .unwrap();

    let mut inner_step = step("true", vec![]);
    inner_step.actions.on_run.cancelation = Some(openjd_model::job::CancelationMode::Terminate);

    let result = session
        .run_task("test_step", &inner_step, None, None, None)
        .await
        .unwrap();
    assert_eq!(result.state, ActionState::Success);

    let contents = read_trace(&trace);
    assert!(
        contents.contains("MODE=<TERMINATE>"),
        "expected MODE=<TERMINATE>; got:\n{contents}"
    );
    assert!(
        contents.contains("NP=<>"),
        "expected NP=<> (null coerces to empty in format-string); got:\n{contents}"
    );
}

/// A task's `onRun` with `CancelationMode::NotifyThenTerminate` and an
/// explicit `notify_period_in_seconds` surfaces both fields verbatim.
#[tokio::test]
async fn wrap_task_run_forwards_cancelation_mode_notify_then_terminate_with_period() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    let env = wrap_env(
        "Wrapper",
        action_with_command("true", vec![]),
        None,
        Some(cancel_probe_action(&trace)),
        None,
    );
    session
        .enter_environment(&env, None, None, None)
        .await
        .unwrap();

    let mut inner_step = step("true", vec![]);
    inner_step.actions.on_run.cancelation =
        Some(openjd_model::job::CancelationMode::NotifyThenTerminate {
            notify_period_in_seconds: Some(fs("45")),
        });

    let result = session
        .run_task("test_step", &inner_step, None, None, None)
        .await
        .unwrap();
    assert_eq!(result.state, ActionState::Success);

    let contents = read_trace(&trace);
    assert!(
        contents.contains("MODE=<NOTIFY_THEN_TERMINATE>"),
        "expected MODE=<NOTIFY_THEN_TERMINATE>; got:\n{contents}"
    );
    assert!(
        contents.contains("NP=<45>"),
        "expected NP=<45>; got:\n{contents}"
    );
}

/// When a task's `onRun` declares no `<Cancelation>`, the wrap script sees
/// `Mode = null` and `NotifyPeriodInSeconds = null`. This differs from
/// `TERMINATE` mode even though the runtime treats both identically for
/// cancelation dispatch — the template variable preserves the "author
/// did not declare" distinction. Null renders empty in interpolation
/// (`MODE=<>`), and is observable via null-coalescing
/// (`MODE_OR=<UNDECLARED>`).
#[tokio::test]
async fn wrap_task_run_cancelation_null_when_no_cancelation() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    let env = wrap_env(
        "Wrapper",
        action_with_command("true", vec![]),
        None,
        Some(cancel_probe_action(&trace)),
        None,
    );
    session
        .enter_environment(&env, None, None, None)
        .await
        .unwrap();

    // Note: step() helper produces an Action with `cancelation: None`.
    let inner_step = step("true", vec![]);
    assert!(inner_step.actions.on_run.cancelation.is_none());

    let result = session
        .run_task("test_step", &inner_step, None, None, None)
        .await
        .unwrap();
    assert_eq!(result.state, ActionState::Success);

    let contents = read_trace(&trace);
    assert!(
        contents.contains("MODE=<>"),
        "expected MODE=<> for un-declared cancelation (null renders empty); got:\n{contents}"
    );
    assert!(
        contents.contains("MODE_OR=<UNDECLARED>"),
        "expected MODE_OR=<UNDECLARED> (null Mode coalesces); got:\n{contents}"
    );
    assert!(
        contents.contains("NP=<>"),
        "expected NP=<>; got:\n{contents}"
    );
}

/// `NOTIFY_THEN_TERMINATE` without an explicit `notifyPeriodInSeconds` on
/// a task's `onRun` surfaces the schema default of 120 seconds
/// (Template Schemas §5.3.2). This asserts the "runtime supplies the
/// value it would have enforced in the unwrapped case" contract from
/// RFC 0008.
#[tokio::test]
async fn wrap_task_run_notify_period_defaults_to_120_when_task_omits_period() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    let env = wrap_env(
        "Wrapper",
        action_with_command("true", vec![]),
        None,
        Some(cancel_probe_action(&trace)),
        None,
    );
    session
        .enter_environment(&env, None, None, None)
        .await
        .unwrap();

    let mut inner_step = step("true", vec![]);
    inner_step.actions.on_run.cancelation =
        Some(openjd_model::job::CancelationMode::NotifyThenTerminate {
            notify_period_in_seconds: None,
        });

    let result = session
        .run_task("test_step", &inner_step, None, None, None)
        .await
        .unwrap();
    assert_eq!(result.state, ActionState::Success);

    let contents = read_trace(&trace);
    assert!(
        contents.contains("MODE=<NOTIFY_THEN_TERMINATE>"),
        "expected MODE=<NOTIFY_THEN_TERMINATE>; got:\n{contents}"
    );
    assert!(
        contents.contains("NP=<120>"),
        "expected NP=<120> (task onRun default); got:\n{contents}"
    );
}

/// `NOTIFY_THEN_TERMINATE` without an explicit `notifyPeriodInSeconds` on
/// an inner environment's `onEnter` surfaces the "otherwise" default of
/// 30 seconds (Template Schemas §5.3.2). Complements the 120-second
/// task-onRun test above and locks in that the seed path picks the right
/// default based on the wrapped action's position.
#[tokio::test]
async fn wrap_env_enter_notify_period_defaults_to_30_when_env_omits_period() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    // Wrap env intercepts the inner env's onEnter via onWrapEnvEnter and
    // echoes the injected values, then keeps onWrapTaskRun as a no-op so
    // the task's onRun still runs (unwrapped is fine for this test).
    let env = wrap_env(
        "Wrapper",
        action_with_command("true", vec![]),
        Some(cancel_probe_action(&trace)),
        Some(action_with_command("true", vec![])),
        Some(action_with_command("true", vec![])),
    );
    session
        .enter_environment(&env, None, None, None)
        .await
        .unwrap();

    // Inner env's onEnter declares NOTIFY_THEN_TERMINATE with no period.
    let inner_on_enter = Action {
        command: fs("inner-enter-cmd"),
        args: Some(vec![fs("--flag")]),
        timeout: None,
        cancelation: Some(openjd_model::job::CancelationMode::NotifyThenTerminate {
            notify_period_in_seconds: None,
        }),
    };
    let inner_env = plain_env("InnerEnv", Some(inner_on_enter), None);

    session
        .enter_environment(&inner_env, None, None, None)
        .await
        .unwrap();

    let contents = read_trace(&trace);
    assert!(
        contents.contains("MODE=<NOTIFY_THEN_TERMINATE>"),
        "expected MODE=<NOTIFY_THEN_TERMINATE>; got:\n{contents}"
    );
    assert!(
        contents.contains("NP=<30>"),
        "expected NP=<30> (onEnter/otherwise default); got:\n{contents}"
    );
}

// ────────────────────────────────────────────────────────────────────
// RFC 0008 single-layer rule (session-level)
// ────────────────────────────────────────────────────────────────────

/// RFC 0008: "A session MUST have at most one wrapping environment.
/// Schedulers MUST reject sessions whose stack contains two or more
/// environments that define any wrap hook, before entering any
/// environment." The model validator enforces this within one job
/// template; environments supplied separately (external environment
/// templates) can only be caught at enter time. The rejection must
/// happen before any state is recorded for the second environment, so
/// the session remains fully usable with the first wrap env.
#[tokio::test]
async fn entering_second_wrap_env_rejected_and_session_stays_usable() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    let mark = |label: &str| -> String { format!(r#"echo "{label}" >> '{}'"#, trace.display()) };
    let wrap_task = |label: &str| -> Action {
        Action {
            command: fs("bash"),
            args: Some(vec![fs("-c"), fs(&mark(label))]),
            timeout: None,
            cancelation: None,
        }
    };

    let wrap_a = wrap_env(
        "WrapA",
        action_with_command("true", vec![]),
        Some(wrap_task("[a-enter]")),
        Some(wrap_task("[a-task]")),
        Some(wrap_task("[a-exit]")),
    );
    let a_id = session
        .enter_environment(&wrap_a, None, None, None)
        .await
        .unwrap();

    let wrap_b = wrap_env(
        "WrapB",
        action_with_command("true", vec![]),
        Some(wrap_task("[b-enter]")),
        Some(wrap_task("[b-task]")),
        Some(wrap_task("[b-exit]")),
    );
    let err = session
        .enter_environment(&wrap_b, None, None, None)
        .await
        .expect_err("entering a second wrap-defining environment must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("at most one Environment defining wrap hooks"),
        "unexpected error message: {msg}"
    );
    assert!(
        msg.contains("WrapA") && msg.contains("WrapB"),
        "error should name both environments: {msg}"
    );

    // The rejected environment must leave no trace: WrapB is not entered,
    // and the session is still Ready — a task runs (wrapped by WrapA) and
    // WrapA exits cleanly.
    assert_eq!(session.environments_entered().len(), 1);
    let task = step("bash", vec!["-c", &mark("task-body")]);
    let result = session
        .run_task("test_step", &task, None, None, None)
        .await
        .unwrap();
    assert_eq!(result.state, ActionState::Success);
    session
        .exit_environment(&a_id, None, true, None)
        .await
        .unwrap();

    let contents = read_trace(&trace);
    assert!(
        contents.contains("[a-task]"),
        "task must be wrapped by WrapA after the rejected enter; got:\n{contents}"
    );
    assert!(
        !contents.contains("[b-task]") && !contents.contains("[b-enter]"),
        "the rejected WrapB must never run any hook; got:\n{contents}"
    );
}

// ────────────────────────────────────────────────────────────────────
// Wrap env's own frozen symtab (environment template parameters)
// ────────────────────────────────────────────────────────────────────

/// A wrap environment converted from an environment template carries its
/// own `parameterDefinitions` values in its frozen `resolved_symtab`
/// (`convert_environment_with_symtab`). The wrap dispatch merges that
/// symtab before resolving the hook, so `{{Param.X}}` in a wrap hook
/// resolves even when the wrapped step's own symbol table does not
/// contain the parameter. Regression test for the wrap-env-parameters
/// gap found in the RFC 0008 review (audit finding F11).
#[tokio::test]
async fn wrap_env_resolved_symtab_params_available_in_wrap_hooks() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    // The wrap env's own parameter, frozen at conversion time — exactly
    // what openjd-cli now produces via convert_environment_with_symtab.
    let mut env_param_symtab = openjd_expr::SymbolTable::new();
    env_param_symtab
        .set(
            "Param.Prefix",
            openjd_expr::ExprValue::String("FROM_ENV_PARAM".into()),
        )
        .unwrap();

    let wrap_task_script = format!(
        r#"echo "[wrap-task] prefix={{{{Param.Prefix}}}}" >> '{}'"#,
        trace.display()
    );
    let wrap_task = Action {
        command: fs("bash"),
        args: Some(vec![fs("-c"), fs(&wrap_task_script)]),
        timeout: None,
        cancelation: None,
    };
    let mut env = wrap_env(
        "Wrapper",
        action_with_command("true", vec![]),
        None,
        Some(wrap_task),
        None,
    );
    env.resolved_symtab = Some(openjd_expr::SerializedSymbolTable::from_symtab(
        &env_param_symtab,
    ));

    session
        .enter_environment(&env, None, None, None)
        .await
        .unwrap();

    // The step knows nothing about Param.Prefix: run_task is called with
    // no task parameters and no resolved symtab, mirroring a plain job.
    let task = step("true", vec![]);
    let result = session
        .run_task("test_step", &task, None, None, None)
        .await
        .unwrap();
    assert_eq!(result.state, ActionState::Success);

    let contents = read_trace(&trace);
    assert!(
        contents.contains("[wrap-task] prefix=FROM_ENV_PARAM"),
        "wrap hook must resolve the wrap env's own Param.* from its frozen \
         resolved_symtab; got:\n{contents}"
    );
}

/// Template Schemas §4.2: an environment script's `let` bindings are
/// available in its actions — and the wrap hooks are actions of the wrap
/// environment's script. The wrapped step's symbol table knows nothing
/// about these names, so the dispatch must evaluate the wrap env's own
/// let scope when seeding the hook's symbol table.
#[tokio::test]
async fn wrap_env_let_bindings_available_in_wrap_hooks() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    let wrap_task_script = format!(
        r#"echo "[wrap-task] greeting={{{{greeting}}}}" >> '{}'"#,
        trace.display()
    );
    let wrap_task = Action {
        command: fs("bash"),
        args: Some(vec![fs("-c"), fs(&wrap_task_script)]),
        timeout: None,
        cancelation: None,
    };
    let mut env = wrap_env(
        "Wrapper",
        action_with_command("true", vec![]),
        None,
        Some(wrap_task),
        None,
    );
    env.script.as_mut().unwrap().let_bindings = Some(vec!["greeting = 'FROM_ENV_LET'".to_string()]);

    session
        .enter_environment(&env, None, None, None)
        .await
        .unwrap();

    let task = step("true", vec![]);
    let result = session
        .run_task("test_step", &task, None, None, None)
        .await
        .unwrap();
    assert_eq!(result.state, ActionState::Success);

    let contents = read_trace(&trace);
    assert!(
        contents.contains("[wrap-task] greeting=FROM_ENV_LET"),
        "wrap hook must resolve the wrap env's own let bindings; got:\n{contents}"
    );
}

/// Wrapper and wrapped `let` scopes must not be conflated. When the wrap
/// environment and the wrapped step both bind the same name, the hook's
/// own args resolve against the WRAPPER's value while `WrappedAction.*`
/// (the step's onRun resolved for forwarding) carries the STEP's value —
/// each scope sees exactly what it would have seen unwrapped.
#[tokio::test]
async fn wrapper_and_wrapped_let_scopes_are_separate() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    // The hook prints its own view of `who` and the forwarded command,
    // which embeds the step's view of `who`.
    let wrap_task_script = format!(
        r#"echo "[hook] who={{{{who}}}} wrapped-arg={{{{WrappedAction.Args[0]}}}}" >> '{}'"#,
        trace.display()
    );
    let wrap_task = Action {
        command: fs("bash"),
        args: Some(vec![fs("-c"), fs(&wrap_task_script)]),
        timeout: None,
        cancelation: None,
    };
    let mut env = wrap_env(
        "Wrapper",
        action_with_command("true", vec![]),
        None,
        Some(wrap_task),
        None,
    );
    env.script.as_mut().unwrap().let_bindings = Some(vec!["who = 'WRAPPER'".to_string()]);
    session
        .enter_environment(&env, None, None, None)
        .await
        .unwrap();

    // The step binds the SAME name and references it from its onRun args.
    let mut task = step("echo", vec!["{{who}}"]);
    task.let_bindings = Some(vec!["who = 'STEP'".to_string()]);
    let result = session
        .run_task("test_step", &task, None, None, None)
        .await
        .unwrap();
    assert_eq!(result.state, ActionState::Success);

    let contents = read_trace(&trace);
    assert!(
        contents.contains("[hook] who=WRAPPER wrapped-arg=STEP"),
        "hook must see the wrapper's binding and WrappedAction.* the step's; got:\n{contents}"
    );
}

// ────────────────────────────────────────────────────────────────────
// WrappedAction.Environment contents
// ────────────────────────────────────────────────────────────────────

/// Template Schemas §4.3.1: `WrappedAction.Environment` carries all
/// session-defined variables — `openjd_env` exports AND entered
/// environments' declarative `variables:` maps.
#[tokio::test]
async fn wrapped_action_environment_includes_variables_map_and_openjd_env() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    let wrap_task_script = format!(
        r#"for e in {{{{ repr_sh(WrappedAction.Environment) }}}}; do echo "ENVLINE=$e" >> '{}'; done"#,
        trace.display()
    );
    let wrap_task = Action {
        command: fs("bash"),
        args: Some(vec![fs("-c"), fs(&wrap_task_script)]),
        timeout: None,
        cancelation: None,
    };
    let outer = wrap_env(
        "Wrapper",
        action_with_command("true", vec![]),
        None,
        Some(wrap_task),
        None,
    );
    session
        .enter_environment(&outer, None, None, None)
        .await
        .unwrap();

    // Inner env: STATIC_VAR via `variables:`, DYNAMIC_VAR via openjd_env.
    let mut inner = plain_env(
        "InnerVars",
        Some(action_with_command(
            "bash",
            vec!["-c", "echo 'openjd_env: DYNAMIC_VAR=world'"],
        )),
        None,
    );
    inner.variables = Some(fs_map(&[("STATIC_VAR", "hello")]));
    session
        .enter_environment(&inner, None, None, None)
        .await
        .unwrap();

    let task = step("true", vec![]);
    let result = session
        .run_task("test_step", &task, None, None, None)
        .await
        .unwrap();
    assert_eq!(result.state, ActionState::Success);

    let contents = read_trace(&trace);
    assert!(
        contents.contains("ENVLINE=DYNAMIC_VAR=world"),
        "openjd_env exports must surface in WrappedAction.Environment; got:\n{contents}"
    );
    assert!(
        contents.contains("ENVLINE=STATIC_VAR=hello"),
        "declarative variables: entries must surface in WrappedAction.Environment; got:\n{contents}"
    );
}

// ────────────────────────────────────────────────────────────────────
// Wrap-hook default timeouts
// ────────────────────────────────────────────────────────────────────

/// `run_wrap_action` enforces the caller-supplied default timeout when the
/// wrap action declares none. The session's exit dispatch relies on this
/// to give a substituted `onWrapEnvExit` the same 300-second default as
/// the `onExit` it replaces (Template Schemas §5 timeout defaults table);
/// the constant itself is too long to observe in a test, so this pins the
/// mechanism with a short duration.
#[tokio::test]
async fn run_wrap_action_applies_default_timeout_when_action_has_none() {
    use openjd_sessions::runner::env_script::EnvironmentScriptRunner;

    let tmp = TempDir::new().unwrap();
    let files_dir = tmp.path().join("files");
    std::fs::create_dir_all(&files_dir).unwrap();

    let mut runner = EnvironmentScriptRunner::new(
        "wrap-timeout-test",
        tmp.path().to_path_buf(),
        files_dir,
        None,
    );

    // No timeout on the action itself: the default must bound it.
    let action = action_with_command("sh", vec!["-c", "sleep 30"]);
    let symtab = openjd_expr::SymbolTable::new();
    let env_vars = std::collections::HashMap::new();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    let started = std::time::Instant::now();
    let result = runner
        .run_wrap_action(
            &action,
            &symtab,
            None,
            &env_vars,
            tx,
            Some(std::time::Duration::from_millis(500)),
        )
        .await
        .unwrap();

    assert_eq!(
        result.state,
        ActionState::Timeout,
        "a wrap action without its own timeout must be bounded by the default"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "timeout must fire at the default, not wait for the subprocess"
    );
}
