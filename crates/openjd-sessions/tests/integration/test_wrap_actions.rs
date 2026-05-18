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
                on_wrap_enter: None,
                on_wrap_task_run: None,
                on_wrap_exit: None,
                on_exit,
            },
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    }
}

/// Build a wrap environment with the three optional wrap hooks plus its
/// own `on_enter`. We always set `on_enter` so the session can track the
/// env through its lifecycle (a session ENTER with no action is allowed
/// but makes the tests harder to reason about).
fn wrap_env(
    name: &str,
    on_enter: Action,
    on_wrap_enter: Option<Action>,
    on_wrap_task_run: Option<Action>,
    on_wrap_exit: Option<Action>,
) -> Environment {
    Environment {
        name: name.to_string(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: EnvironmentActions {
                on_enter: Some(on_enter),
                on_wrap_enter,
                on_wrap_task_run,
                on_wrap_exit,
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
    let result = session.run_task(&s, None, None, None).await.unwrap();
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
    let result = session.run_task(&s, None, None, None).await.unwrap();
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
// onWrapEnter / onWrapExit
// ────────────────────────────────────────────────────────────────────

/// With a wrap env defining `onWrapEnter`, an inner environment's
/// `onEnter` is intercepted. The wrap script sees `Env.Wrapped.Name`
/// resolved to the inner env's name.
#[tokio::test]
async fn wrap_enter_intercepts_inner_on_enter() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    let wrap_enter_script = format!(
        r#"echo "[WRAPPED-ENTER] env={{{{Env.Wrapped.Name}}}}" >> '{path}'"#,
        path = trace.display(),
    );
    let wrap_enter = Action {
        command: fs("bash"),
        args: Some(vec![fs("-c"), fs(&wrap_enter_script)]),
        timeout: None,
        cancelation: None,
    };
    let outer = wrap_env(
        "Outer",
        action_with_command("true", vec![]),
        Some(wrap_enter),
        None,
        None,
    );
    session
        .enter_environment(&outer, None, None, None)
        .await
        .unwrap();

    // Inner env's onEnter — should be intercepted by Outer.onWrapEnter.
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
/// `onWrapEnter`. Regression-style test — proves the dispatch excludes
/// the entering env from the wrap-env lookup.
#[tokio::test]
async fn wrap_enter_does_not_wrap_outer_env_itself() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    let outer_enter_cmd = format!("echo outer-enter-body >> '{}'", trace.display());
    let wrap_enter_script = format!(
        r#"echo "[WRAPPED-ENTER]" >> '{path}'"#,
        path = trace.display(),
    );
    let wrap_enter = Action {
        command: fs("bash"),
        args: Some(vec![fs("-c"), fs(&wrap_enter_script)]),
        timeout: None,
        cancelation: None,
    };
    let outer = wrap_env(
        "Outer",
        action_with_command("sh", vec!["-c", &outer_enter_cmd]),
        Some(wrap_enter),
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
        "outer's own onEnter must not be wrapped by its own onWrapEnter; got:\n{contents}"
    );
}

/// `onWrapExit` intercepts an inner environment's `onExit`. Verify both
/// the interception and that `Env.Wrapped.Name` resolves to the inner
/// env being exited.
#[tokio::test]
async fn wrap_exit_intercepts_inner_on_exit() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    let wrap_exit_script = format!(
        r#"echo "[WRAPPED-EXIT] env={{{{Env.Wrapped.Name}}}}" >> '{path}'"#,
        path = trace.display(),
    );
    let wrap_exit = Action {
        command: fs("bash"),
        args: Some(vec![fs("-c"), fs(&wrap_exit_script)]),
        timeout: None,
        cancelation: None,
    };
    let outer = wrap_env(
        "Outer",
        action_with_command("true", vec![]),
        None,
        None,
        Some(wrap_exit),
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

    let wrap_enter = Action {
        command: fs("bash"),
        args: Some(vec![
            fs("-c"),
            fs(&mark("[wrap-enter] {{Env.Wrapped.Name}}")),
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
    let wrap_exit = Action {
        command: fs("bash"),
        args: Some(vec![
            fs("-c"),
            fs(&mark("[wrap-exit] {{Env.Wrapped.Name}}")),
        ]),
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
        Some(wrap_enter),
        Some(wrap_task_run),
        Some(wrap_exit),
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
        .run_task(&task_script, None, None, None)
        .await
        .unwrap();

    session.exit_environment(&id, None, true, None).await.unwrap();

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
/// (not just `onWrapTaskRun`). Verify that `onWrapEnter` and `onWrapExit`
/// see the inner environment's `onEnter`/`onExit` command and args via
/// `WrappedAction.Command` and `WrappedAction.Args`.
#[tokio::test]
async fn wrapped_action_visible_in_wrap_enter_and_wrap_exit() {
    let tmp = TempDir::new().unwrap();
    let trace = tmp.path().join("trace.log");
    let mut session = Session::new_for_test(tmp.path().to_path_buf());

    let mark = |label: &str| -> String { format!(r#"echo "{label}" >> '{}'"#, trace.display()) };

    let wrap_enter = Action {
        command: fs("bash"),
        args: Some(vec![
            fs("-c"),
            fs(&mark("[wrap-enter] cmd={{WrappedAction.Command}}")),
        ]),
        timeout: None,
        cancelation: None,
    };
    let wrap_exit = Action {
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
        Some(wrap_enter),
        None,
        Some(wrap_exit),
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
    session.exit_environment(&id, None, true, None).await.unwrap();

    let contents = read_trace(&trace);
    assert!(
        contents.contains("[wrap-enter] cmd=inner-enter-cmd"),
        "WrappedAction.Command should resolve in onWrapEnter; got:\n{contents}"
    );
    assert!(
        contents.contains("[wrap-exit] cmd=inner-exit-cmd"),
        "WrappedAction.Command should resolve in onWrapExit; got:\n{contents}"
    );
}
