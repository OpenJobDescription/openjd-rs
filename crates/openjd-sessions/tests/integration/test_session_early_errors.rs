// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Errors after a session call is accepted but before its action starts
//! (specs/sessions/session.md § Failures before an action starts;
//! error-handling.md § Rejections and failed actions).
//!
//! Once `enter_environment`, `exit_environment` or `run_task` has passed its
//! up-front checks, a failure to prepare the action (building the symbol
//! table, resolving an environment's variables) fails the action, the same
//! as the pure-Python `openjd.sessions` runtime does: the action status is
//! `Failed` with the error as its message, and the session is ending-only.
//! The call still returns the error.
//!
//! The up-front checks (wrong state, unknown or duplicate environment, LIFO
//! order, wrap-layer rule, `run_subprocess` argument checks) stay plain
//! errors that change nothing, as Python's do.

use openjd_expr::format_string::FormatString;
use openjd_expr::{ExprValue, SerializedSymbolTable};
use openjd_model::job::{
    Action, Environment, EnvironmentActions, EnvironmentScript, StepActions, StepScript,
};
use openjd_model::types::{
    JobParameterType, JobParameterValue, JobParameterValues, TaskParameterSet, TaskParameterType,
    TaskParameterValue,
};
use openjd_sessions::action::ActionState;
use openjd_sessions::error::SessionError;
use openjd_sessions::session::{Session, SessionConfig, SessionState};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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

fn step(script: &str) -> StepScript {
    StepScript {
        let_bindings: None,
        actions: StepActions {
            on_run: action(script),
        },
        embedded_files: None,
    }
}

fn env_actions(on_enter: Option<Action>, on_exit: Option<Action>) -> EnvironmentActions {
    EnvironmentActions {
        on_enter,
        on_wrap_env_enter: None,
        on_wrap_task_run: None,
        on_wrap_env_exit: None,
        on_exit,
    }
}

fn env(name: &str, on_enter: Option<Action>, on_exit: Option<Action>) -> Environment {
    Environment {
        name: name.into(),
        description: None,
        script: Some(EnvironmentScript {
            let_bindings: None,
            actions: env_actions(on_enter, on_exit),
            embedded_files: None,
        }),
        variables: None,
        resolved_symtab: None,
    }
}

fn env_with_variables(name: &str, on_exit: Option<Action>, vars: &[(&str, &str)]) -> Environment {
    Environment {
        variables: Some(
            vars.iter()
                .map(|(k, v)| (k.to_string(), fs(v)))
                .collect::<HashMap<_, _>>(),
        ),
        ..env(name, None, on_exit)
    }
}

/// An environment that defines a wrap hook (RFC 0008).
fn wrap_env(name: &str) -> Environment {
    let mut e = env(name, None, None);
    e.script.as_mut().unwrap().actions.on_wrap_task_run = Some(action("echo wrapped"));
    e
}

/// A resolved symbol table that can't be deserialized: the transport form
/// must be a JSON array.
fn bad_symtab() -> SerializedSymbolTable {
    SerializedSymbolTable::from_json_str("{}").unwrap()
}

const BAD_SYMTAB_MESSAGE: &str =
    "Failed to deserialize resolved_symtab: SerializedSymbolTable: expected JSON array";

/// Every status the session reported through its callback, in order.
type Seen = Arc<Mutex<Vec<ActionState>>>;

/// A session built through the public constructor, with `params` as its job
/// parameter values and a callback that records each reported action state.
fn session(root: &TempDir, params: JobParameterValues) -> (Session, Seen) {
    let seen: Seen = Arc::default();
    let seen_cb = seen.clone();
    let config = SessionConfig {
        limits: Default::default(),
        session_id: "early-errors".into(),
        job_parameter_values: params,
        session_root_directory: Some(root.path().to_path_buf()),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: Some(Box::new(move |_, status| {
            seen_cb.lock().unwrap().push(status.state)
        })),
        os_env_vars: None,
        user: None,
        profile: None,
        cancel_token: None,
        debug_collect_stdout: true,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    };
    (Session::with_config(config).unwrap(), seen)
}

fn job_param(name: &str, param_type: JobParameterType, value: &str) -> JobParameterValues {
    HashMap::from([(
        name.to_string(),
        JobParameterValue {
            param_type,
            value: ExprValue::String(value.into()),
        },
    )])
}

/// Check the outcome every pre-start failure must have: the call returned
/// `expected` as its error, the action is `Failed` with that error as its
/// message, the callback saw it, and the session is ending-only.
fn assert_failed_action<T: std::fmt::Debug>(
    s: &Session,
    seen: &Seen,
    result: Result<T, SessionError>,
    expected: &str,
) {
    assert_failed_action_seq(s, seen, result, expected, None);
}

/// As [`assert_failed_action`], and additionally pin the exact callback
/// sequence when `callbacks` is `Some`. A failure before the action is
/// reported `Running` fires only `[Failed]`; one after it (path mapping,
/// wrap-hook setup, and everything in `run_task`) fires `[Running, Failed]`.
fn assert_failed_action_seq<T: std::fmt::Debug>(
    s: &Session,
    seen: &Seen,
    result: Result<T, SessionError>,
    expected: &str,
    callbacks: Option<&[ActionState]>,
) {
    let err = result.expect_err("the call must return the error");
    assert_eq!(err.to_string(), expected);
    let status = s.action_status().expect("the failed action has a status");
    assert_eq!(status.state, ActionState::Failed);
    assert_eq!(status.fail_message.as_deref(), Some(expected));
    assert_eq!(status.exit_code, None);
    assert!(status.started_at.is_some());
    assert!(status.ended_at.is_some());
    let seen = seen.lock().unwrap();
    assert_eq!(
        seen.last(),
        Some(&ActionState::Failed),
        "the callback must report the failed action"
    );
    if let Some(expected_seq) = callbacks {
        assert_eq!(seen.as_slice(), expected_seq, "callback sequence");
    }
    assert_eq!(s.state(), SessionState::ReadyEnding);
}

/// An ending-only session refuses `run_task`, and `clear_ending_only()`
/// brings it back so that `run_task` runs again.
async fn assert_refused_until_cleared(s: &mut Session) {
    match s
        .run_task("t", &step("echo refused"), None, None, None)
        .await
    {
        Err(SessionError::InvalidState { expected, current }) => {
            assert_eq!(expected, vec![SessionState::Ready]);
            assert_eq!(current, SessionState::ReadyEnding);
        }
        other => panic!("run_task must be refused while ending-only, got {other:?}"),
    }
    s.clear_ending_only().unwrap();
    assert_eq!(s.state(), SessionState::Ready);
    let r = s
        .run_task("t", &step("echo again"), None, None, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    assert_eq!(s.state(), SessionState::Ready);
}

// ── enter_environment ────────────────────────────────────────────────

/// Python returns the identifier and leaves the environment entered, so the
/// caller exits it. Rust returns the error; the environment is still the
/// last entry of `environments_entered()`, and exiting it runs its onExit.
async fn check_enter_failure(
    s: &mut Session,
    seen: &Seen,
    e: &Environment,
    symtab: Option<&SerializedSymbolTable>,
    expected: &str,
) {
    let result = s.enter_environment(e, symtab, Some("env-1"), None).await;
    // Building the symbol table or resolving variables fails before the
    // action is reported Running, so the only callback is Failed.
    assert_failed_action_seq(s, seen, result, expected, Some(&[ActionState::Failed]));
    assert_eq!(s.environments_entered(), ["env-1".to_string()]);
    assert_refused_until_cleared(s).await;

    let out = s
        .exit_environment(&"env-1".to_string(), None, true, None)
        .await
        .unwrap();
    assert!(out.contains("exited"), "onExit must run: {out:?}");
    assert!(s.environments_entered().is_empty());
}

/// A bad job parameter value fails every call that builds a symbol table,
/// not only the first: after `clear_ending_only()`, `run_task` fails the
/// same way, and so does the exit, which still removes the environment.
async fn check_job_parameter_failure(s: &mut Session, seen: &Seen, expected: &str) {
    let e = env(
        "Env",
        Some(action("echo entered")),
        Some(action("echo exited")),
    );
    let result = s.enter_environment(&e, None, Some("env-1"), None).await;
    assert_failed_action(s, seen, result, expected);
    assert_eq!(s.environments_entered(), ["env-1".to_string()]);
    assert!(matches!(
        s.run_task("t", &step("echo refused"), None, None, None)
            .await,
        Err(SessionError::InvalidState { .. })
    ));

    s.clear_ending_only().unwrap();
    let result = s.run_task("t", &step("echo ran"), None, None, None).await;
    assert_failed_action(s, seen, result, expected);

    let result = s
        .exit_environment(&"env-1".to_string(), None, true, None)
        .await;
    assert_failed_action(s, seen, result, expected);
    assert!(s.environments_entered().is_empty());
}

#[tokio::test]
async fn enter_with_undeserializable_resolved_symtab_fails_the_action() {
    let root = TempDir::new().unwrap();
    let (mut s, seen) = session(&root, JobParameterValues::new());
    let e = env(
        "Env",
        Some(action("echo entered")),
        Some(action("echo exited")),
    );
    check_enter_failure(&mut s, &seen, &e, Some(&bad_symtab()), BAD_SYMTAB_MESSAGE).await;
}

/// The same failure without an onEnter: no action would have run, but the
/// call was accepted, so it still fails an action.
#[tokio::test]
async fn enter_without_on_enter_with_undeserializable_resolved_symtab_fails_the_action() {
    let root = TempDir::new().unwrap();
    let (mut s, seen) = session(&root, JobParameterValues::new());
    let e = env("Env", None, Some(action("echo exited")));
    check_enter_failure(&mut s, &seen, &e, Some(&bad_symtab()), BAD_SYMTAB_MESSAGE).await;
}

/// A job parameter value that can't be coerced to its declared type fails
/// while building the symbol table. (Python doesn't coerce, so it reports
/// no error for this input.)
#[tokio::test]
async fn enter_with_uncoercible_job_parameter_fails_the_action() {
    let root = TempDir::new().unwrap();
    let (mut s, seen) = session(&root, job_param("N", JobParameterType::Int, "abc"));
    check_job_parameter_failure(
        &mut s,
        &seen,
        "Failed to coerce parameter 'RawParam.N': Cannot convert 'abc' to int: invalid digit found in string",
    )
    .await;
}

/// A PATH job parameter longer than the path mapping limit fails while
/// building the symbol table.
#[tokio::test]
async fn enter_with_overlong_path_parameter_fails_the_action() {
    let root = TempDir::new().unwrap();
    let max = openjd_expr::path_mapping::MAX_PATH_LEN;
    let long = "a".repeat(max + 1);
    let (mut s, seen) = session(&root, job_param("P", JobParameterType::Path, &long));
    let expected = format!(
        "Failed to apply path mapping to Param.P: the path is {} bytes long, \
         more than the limit of {max} bytes",
        max + 1
    );
    check_job_parameter_failure(&mut s, &seen, &expected).await;
}

#[tokio::test]
async fn enter_with_unresolvable_variable_fails_the_action() {
    let root = TempDir::new().unwrap();
    let (mut s, seen) = session(&root, JobParameterValues::new());
    let e = env_with_variables(
        "Env",
        Some(action("echo exited")),
        &[("V", "{{Param.Missing}}")],
    );
    let err = s
        .enter_environment(&e, None, Some("env-1"), None)
        .await
        .unwrap_err();
    // The format-string message itself belongs to openjd-expr; pin the
    // session's framing of it and that the action carries the same text.
    let msg = err.to_string();
    assert!(
        msg.starts_with("Failed to resolve env var 'V': "),
        "unexpected error: {msg}"
    );
    assert!(msg.contains("Param.Missing"), "unexpected error: {msg}");
    assert_failed_action(&s, &seen, Err::<(), _>(err), &msg);
    assert_eq!(s.environments_entered(), ["env-1".to_string()]);
    assert_refused_until_cleared(&mut s).await;
}

#[tokio::test]
async fn enter_with_nul_in_variable_fails_the_action() {
    let root = TempDir::new().unwrap();
    let (mut s, seen) = session(&root, job_param("S", JobParameterType::String, "a\0b"));
    let e = env_with_variables("Env", Some(action("echo exited")), &[("V", "{{Param.S}}")]);
    check_enter_failure(
        &mut s,
        &seen,
        &e,
        None,
        "Failed to resolve env var 'V': resolved value contains a NUL byte, which cannot \
         be represented in a process environment.",
    )
    .await;
}

#[tokio::test]
async fn enter_with_overlong_variable_fails_the_action() {
    let root = TempDir::new().unwrap();
    let (mut s, seen) = session(&root, JobParameterValues::new());
    let long = "x".repeat(2049);
    let e = env_with_variables("Env", Some(action("echo exited")), &[("V", &long)]);
    check_enter_failure(
        &mut s,
        &seen,
        &e,
        None,
        "Failed to resolve env var 'V': resolved value is 2049 characters, exceeding the \
         maximum of 2048.",
    )
    .await;
}

/// When more than one variable is invalid, the error reported is
/// deterministic: variables resolve in sorted key order, so the
/// alphabetically-first invalid one is reported regardless of the
/// `variables` HashMap's iteration order. Pins the sort (a mutant that
/// drops it reports whichever the map happens to visit first).
#[tokio::test]
async fn enter_with_several_bad_variables_reports_the_sorted_first() {
    let root = TempDir::new().unwrap();
    let (mut s, _seen) = session(&root, JobParameterValues::new());
    // Several invalid variables; AAA_BAD sorts first. On correct (sorted)
    // code AAA_BAD is always reported; a mutant that drops the sort reports
    // whichever the HashMap visits first, so more invalid keys raise the
    // chance it is caught.
    let e = env_with_variables(
        "Env",
        Some(action("echo exited")),
        &[
            ("MMM_GOOD", "set"),
            ("AAA_BAD", "{{Param.Missing}}"),
            ("BBB_BAD", "{{Param.Missing}}"),
            ("CCC_BAD", "{{Param.Missing}}"),
            ("DDD_BAD", "{{Param.Missing}}"),
            ("EEE_BAD", "{{Param.Missing}}"),
            ("FFF_BAD", "{{Param.Missing}}"),
            ("GGG_BAD", "{{Param.Missing}}"),
            ("HHH_BAD", "{{Param.Missing}}"),
            ("ZZZ_BAD", "{{Param.Missing}}"),
        ],
    );
    let err = s
        .enter_environment(&e, None, Some("env-1"), None)
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .starts_with("Failed to resolve env var 'AAA_BAD':"),
        "the alphabetically-first invalid variable must be the one reported, got: {err}"
    );
    assert_eq!(s.state(), SessionState::ReadyEnding);
}

/// A failed variable resolution sets none of the environment's variables,
/// as in Python, so its onExit doesn't see a partial set. `variables` is a
/// HashMap, so without this the outcome would depend on iteration order.
#[tokio::test]
async fn enter_with_one_bad_variable_sets_none_of_them() {
    let root = TempDir::new().unwrap();
    let (mut s, _seen) = session(&root, JobParameterValues::new());
    // Variables resolve in sorted key order, so a bad variable that sorts
    // last still leaves none set: this kills a mutant that sets each as it
    // resolves regardless of HashMap iteration order.
    let vars: Vec<(String, String)> = (0..8)
        .map(|i| (format!("GOOD{i}"), "set".to_string()))
        .chain([("ZZZ_BAD".to_string(), "{{Param.Missing}}".to_string())])
        .collect();
    let vars: Vec<(&str, &str)> = vars.iter().map(|(k, v)| (&**k, &**v)).collect();
    let e = env_with_variables(
        "Env",
        Some(action(
            "echo \"seen=[${GOOD0}${GOOD1}${GOOD2}${GOOD3}${GOOD4}${GOOD5}${GOOD6}${GOOD7}]\"",
        )),
        &vars,
    );
    s.enter_environment(&e, None, Some("env-1"), None)
        .await
        .unwrap_err();
    assert_eq!(s.state(), SessionState::ReadyEnding);
    let env_vars = s.evaluate_env_vars(None);
    for i in 0..8 {
        assert!(
            !env_vars.contains_key(&format!("GOOD{i}")),
            "GOOD{i} must not be set"
        );
    }
    let out = s
        .exit_environment(&"env-1".to_string(), None, true, None)
        .await
        .unwrap();
    assert!(out.contains("seen=[]"), "onExit saw a variable: {out:?}");
}

// ── exit_environment ─────────────────────────────────────────────────

/// Python removes the environment from tracking before it builds the
/// symbol table, so the failed exit still counts as an exit: the
/// environment is gone and its onExit never runs.
#[tokio::test]
async fn exit_with_undeserializable_resolved_symtab_fails_the_action() {
    for keep_session_running in [true, false] {
        let root = TempDir::new().unwrap();
        let (mut s, seen) = session(&root, JobParameterValues::new());
        let id = s
            .enter_environment(
                &env("Env", None, Some(action("echo exited"))),
                None,
                None,
                None,
            )
            .await
            .unwrap();
        let before = seen.lock().unwrap().len();
        let result = s
            .exit_environment(&id, Some(&bad_symtab()), keep_session_running, None)
            .await;
        assert_failed_action(&s, &seen, result, BAD_SYMTAB_MESSAGE);
        // Exit builds the symbol table after removing the environment and
        // before reporting Running, so the only callback it adds is Failed.
        assert_eq!(
            &seen.lock().unwrap()[before..],
            &[ActionState::Failed],
            "exit must report only Failed, no Running"
        );
        assert!(s.environments_entered().is_empty());
        match s.exit_environment(&id, None, true, None).await {
            Err(SessionError::UnknownEnvironment { identifier }) => assert_eq!(identifier, id),
            other => panic!("the environment must already be exited, got {other:?}"),
        }
        assert_refused_until_cleared(&mut s).await;
    }
}

/// Exit's pre-start failure goes through `fail_action_before_start`, which
/// starts a fresh action status and logs the "Exiting Environment" banner.
/// A previous task's progress/status must not leak into the failed exit's
/// status, and the banner must be logged. (Pairs with the enter banner test;
/// pins the exit call site, which a mutant could otherwise route through
/// `fail_action_setup` undetected.)
#[tokio::test]
async fn exit_pre_start_failure_resets_status_and_logs_banner() {
    testing_logger::setup();
    let root = TempDir::new().unwrap();
    let (mut s, _seen) = session(&root, JobParameterValues::new());
    let id = s
        .enter_environment(
            &env("MyEnv", None, Some(action("echo exited"))),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    // A task that reports progress and status, so a leak would be visible.
    let r = s
        .run_task(
            "t",
            &step("echo openjd_progress: 50.0; echo openjd_status: halfway; exit 0"),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    // The task succeeded, so the session is Ready and the exit is accepted.
    let result = s
        .exit_environment(&id, Some(&bad_symtab()), true, None)
        .await;
    let err = result.expect_err("the exit must fail");
    assert_eq!(err.to_string(), BAD_SYMTAB_MESSAGE);
    let status = s.action_status().unwrap();
    assert_eq!(status.state, ActionState::Failed);
    assert_eq!(
        status.progress, None,
        "previous task progress must not leak"
    );
    assert_eq!(status.status_message, None, "previous status must not leak");
    assert_eq!(status.fail_message.as_deref(), Some(BAD_SYMTAB_MESSAGE));
    testing_logger::validate(|logs| {
        let bodies: Vec<&str> = logs.iter().map(|l| l.body.as_str()).collect();
        assert!(
            bodies
                .iter()
                .any(|b| b.contains("Exiting Environment: MyEnv")),
            "exit banner must be logged: {bodies:?}"
        );
    });
}

/// A failed exit leaves the outer environment on the stack, and it can
/// still be exited in order.
#[tokio::test]
async fn exit_failure_leaves_the_outer_environment_exitable() {
    let root = TempDir::new().unwrap();
    let (mut s, seen) = session(&root, JobParameterValues::new());
    let outer = s
        .enter_environment(
            &env("Outer", None, Some(action("echo outer-exited"))),
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
    let result = s
        .exit_environment(&inner, Some(&bad_symtab()), true, None)
        .await;
    assert_failed_action(&s, &seen, result, BAD_SYMTAB_MESSAGE);
    assert_eq!(s.environments_entered(), std::slice::from_ref(&outer));
    let out = s.exit_environment(&outer, None, true, None).await.unwrap();
    assert!(out.contains("outer-exited"), "{out:?}");
    assert_eq!(s.state(), SessionState::ReadyEnding);
}

// ── run_task ─────────────────────────────────────────────────────────

#[tokio::test]
async fn run_task_with_undeserializable_resolved_symtab_fails_the_action() {
    let root = TempDir::new().unwrap();
    let (mut s, seen) = session(&root, JobParameterValues::new());
    let result = s
        .run_task("t", &step("echo ran"), None, Some(&bad_symtab()), None)
        .await;
    // run_task reports Running before it builds the symbol table.
    assert_failed_action_seq(
        &s,
        &seen,
        result,
        BAD_SYMTAB_MESSAGE,
        Some(&[ActionState::Running, ActionState::Failed]),
    );
    assert_refused_until_cleared(&mut s).await;
}

#[tokio::test]
async fn run_task_with_uncoercible_task_parameter_fails_the_action() {
    let root = TempDir::new().unwrap();
    let (mut s, seen) = session(&root, JobParameterValues::new());
    let params: TaskParameterSet = [(
        "Frame".to_string(),
        TaskParameterValue {
            param_type: TaskParameterType::Int,
            value: ExprValue::String("abc".into()),
        },
    )]
    .into_iter()
    .collect();
    let result = s
        .run_task("t", &step("echo ran"), Some(&params), None, None)
        .await;
    assert_failed_action(
        &s,
        &seen,
        result,
        "Failed to coerce parameter 'Task.RawParam.Frame': Cannot convert 'abc' to int: invalid digit found in string",
    );
    assert_refused_until_cleared(&mut s).await;
}

// ── Up-front checks stay plain errors ────────────────────────────────

/// A rejected call changes nothing: the state, the entered environments and
/// the previous action's status are as they were, and no callback fires.
struct Snapshot {
    state: SessionState,
    entered: Vec<String>,
    status: Option<(ActionState, Option<String>)>,
    callbacks: usize,
}

fn snapshot(s: &Session, seen: &Seen) -> Snapshot {
    Snapshot {
        state: s.state(),
        entered: s.environments_entered().to_vec(),
        status: s
            .action_status()
            .map(|st| (st.state, st.fail_message.clone())),
        callbacks: seen.lock().unwrap().len(),
    }
}

fn assert_unchanged(s: &Session, seen: &Seen, before: &Snapshot) {
    let after = snapshot(s, seen);
    assert_eq!(after.state, before.state);
    assert_eq!(after.entered, before.entered);
    assert_eq!(after.status, before.status);
    assert_eq!(after.callbacks, before.callbacks, "no callback may fire");
}

/// These checks run before anything that could fail an action, so they win
/// even when the call also carries input that would fail one.
#[tokio::test]
async fn enter_rejections_change_nothing() {
    let root = TempDir::new().unwrap();
    let (mut s, seen) = session(&root, JobParameterValues::new());
    s.enter_environment(&wrap_env("Wrap"), None, Some("env-1"), None)
        .await
        .unwrap();
    let before = snapshot(&s, &seen);

    match s
        .enter_environment(
            &env("Dup", None, None),
            Some(&bad_symtab()),
            Some("env-1"),
            None,
        )
        .await
    {
        Err(SessionError::DuplicateEnvironment { id }) => assert_eq!(id, "env-1"),
        other => panic!("expected DuplicateEnvironment, got {other:?}"),
    }
    assert_unchanged(&s, &seen, &before);

    match s
        .enter_environment(&wrap_env("Wrap2"), Some(&bad_symtab()), None, None)
        .await
    {
        Err(SessionError::MultipleWrapEnvironments { existing, entering }) => {
            assert_eq!((existing.as_str(), entering.as_str()), ("Wrap", "Wrap2"));
        }
        other => panic!("expected MultipleWrapEnvironments, got {other:?}"),
    }
    assert_unchanged(&s, &seen, &before);
}

/// The duplicate-identifier check runs before the wrap-layer check, as in
/// Python: re-entering a wrap environment under its own identifier reports
/// the duplicate, not the wrap-layer conflict. Both are rejections that
/// change nothing, so only the variant is pinned here.
#[tokio::test]
async fn enter_duplicate_identifier_is_checked_before_wrap_layer() {
    let root = TempDir::new().unwrap();
    let (mut s, seen) = session(&root, JobParameterValues::new());
    s.enter_environment(&wrap_env("Wrap"), None, Some("env-1"), None)
        .await
        .unwrap();
    let before = snapshot(&s, &seen);
    match s
        .enter_environment(&wrap_env("Wrap"), None, Some("env-1"), None)
        .await
    {
        Err(SessionError::DuplicateEnvironment { id }) => assert_eq!(id, "env-1"),
        other => panic!("expected DuplicateEnvironment, got {other:?}"),
    }
    assert_unchanged(&s, &seen, &before);
}

#[tokio::test]
async fn exit_rejections_change_nothing() {
    let root = TempDir::new().unwrap();
    let (mut s, seen) = session(&root, JobParameterValues::new());
    let outer = s
        .enter_environment(&env("Outer", None, None), None, None, None)
        .await
        .unwrap();
    let _inner = s
        .enter_environment(&env("Inner", None, None), None, None, None)
        .await
        .unwrap();
    let before = snapshot(&s, &seen);

    match s
        .exit_environment(&"nope".to_string(), Some(&bad_symtab()), false, None)
        .await
    {
        Err(SessionError::UnknownEnvironment { identifier }) => assert_eq!(identifier, "nope"),
        other => panic!("expected UnknownEnvironment, got {other:?}"),
    }
    assert_unchanged(&s, &seen, &before);

    match s
        .exit_environment(&outer, Some(&bad_symtab()), false, None)
        .await
    {
        Err(SessionError::LifoViolation { got, .. }) => assert_eq!(got, outer),
        other => panic!("expected LifoViolation, got {other:?}"),
    }
    assert_unchanged(&s, &seen, &before);
}

/// In the wrong state every call is refused before it looks at its input.
#[tokio::test]
async fn wrong_state_rejections_change_nothing() {
    let root = TempDir::new().unwrap();
    let (mut s, seen) = session(&root, JobParameterValues::new());
    let r = s
        .run_task("t", &step("exit 1"), None, None, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Failed);
    let before = snapshot(&s, &seen);
    assert_eq!(before.state, SessionState::ReadyEnding);

    let bad = bad_symtab();
    assert!(matches!(
        s.enter_environment(&env("E", None, None), Some(&bad), None, None)
            .await,
        Err(SessionError::InvalidState { .. })
    ));
    assert_unchanged(&s, &seen, &before);
    assert!(matches!(
        s.run_task("t", &step("echo"), None, Some(&bad), None).await,
        Err(SessionError::InvalidState { .. })
    ));
    assert_unchanged(&s, &seen, &before);
    assert!(matches!(
        s.run_subprocess("echo", None, None, None, true, None).await,
        Err(SessionError::InvalidState { .. })
    ));
    assert_unchanged(&s, &seen, &before);

    s.cleanup();
    let before = snapshot(&s, &seen);
    assert!(matches!(
        s.exit_environment(&"x".to_string(), Some(&bad), true, None)
            .await,
        Err(SessionError::InvalidState { .. })
    ));
    assert_unchanged(&s, &seen, &before);
}

/// `run_subprocess` has no failable step between its argument checks and
/// the action; its argument checks stay plain errors.
#[tokio::test]
async fn run_subprocess_argument_rejections_change_nothing() {
    let root = TempDir::new().unwrap();
    let (mut s, seen) = session(&root, JobParameterValues::new());
    let before = snapshot(&s, &seen);
    for (command, timeout, message) in [
        ("", None, "command must be a non-empty string"),
        ("  ", None, "command must be a non-empty string"),
        (
            "echo",
            Some(std::time::Duration::ZERO),
            "timeout must be positive",
        ),
    ] {
        let err = s
            .run_subprocess(command, None, timeout, None, true, None)
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), message);
        assert_unchanged(&s, &seen, &before);
    }
    assert_eq!(s.state(), SessionState::Ready);
}

/// A pre-start failure logs the call's section banner first, so the failure
/// appears under it in the session log (as `fail_action_before_start`
/// documents). Uses the public constructor's own session id via with_config.
#[tokio::test]
async fn enter_pre_start_failure_logs_the_banner() {
    testing_logger::setup();
    let root = TempDir::new().unwrap();
    let (mut s, _seen) = session(&root, JobParameterValues::new());
    s.enter_environment(
        &env("MyEnv", None, Some(action("echo exited"))),
        Some(&bad_symtab()),
        Some("env-1"),
        None,
    )
    .await
    .unwrap_err();
    testing_logger::validate(|logs| {
        let bodies: Vec<&str> = logs.iter().map(|l| l.body.as_str()).collect();
        assert!(
            bodies
                .iter()
                .any(|b| b.contains("Entering Environment: MyEnv")),
            "banner must be logged: {bodies:?}"
        );
        assert!(
            bodies.iter().any(|b| b.contains("Action failed to run")),
            "failure must be logged: {bodies:?}"
        );
    });
}

// ── Pre-start failures after the Running notification ────────────────
//
// Path mapping and RFC 0008 wrap-hook setup run inside the onEnter/onExit
// action branch, after the session reports Running, so they settle through
// fail_action_setup and fire [Running, Failed] (unlike the symbol-table and
// variables failures above, which fire [Failed] only). These pin that
// sequence for enter and exit; a mutant that drops the fail_action_setup at
// those sites, or moves the Running notification after them, changes it.

/// Make the session working directory unwritable so the path mapping rules
/// file (written whenever the action runs, with or without rules) can't be
/// created.
#[cfg(unix)]
fn make_working_dir_readonly(s: &Session) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        s.working_directory(),
        std::fs::Permissions::from_mode(0o500),
    )
    .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn enter_path_mapping_write_failure_reports_running_then_failed() {
    let root = TempDir::new().unwrap();
    let (mut s, seen) = session(&root, JobParameterValues::new());
    make_working_dir_readonly(&s);
    // onEnter present, so the action is reported Running before path mapping.
    let result = s
        .enter_environment(
            &env(
                "Env",
                Some(action("echo entered")),
                Some(action("echo exited")),
            ),
            None,
            Some("env-1"),
            None,
        )
        .await;
    let err = result.expect_err("the path mapping write must fail");
    assert!(
        matches!(err, SessionError::WorkingDirectory { .. }),
        "expected WorkingDirectory, got {err:?}"
    );
    let status = s.action_status().unwrap();
    assert_eq!(status.state, ActionState::Failed);
    assert_eq!(
        status.fail_message.as_deref(),
        Some(err.to_string().as_str())
    );
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        &[ActionState::Running, ActionState::Failed],
        "a failure after Running reports Running then Failed"
    );
    assert_eq!(s.state(), SessionState::ReadyEnding);
    assert_eq!(s.environments_entered(), ["env-1".to_string()]);
}

#[cfg(unix)]
#[tokio::test]
async fn exit_path_mapping_write_failure_reports_running_then_failed() {
    let root = TempDir::new().unwrap();
    let (mut s, seen) = session(&root, JobParameterValues::new());
    let id = s
        .enter_environment(
            &env("Env", None, Some(action("echo exited"))),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    let before = seen.lock().unwrap().len();
    make_working_dir_readonly(&s);
    // onExit present, so the exit action is reported Running before path mapping.
    let result = s.exit_environment(&id, None, true, None).await;
    let err = result.expect_err("the path mapping write must fail");
    assert!(
        matches!(err, SessionError::WorkingDirectory { .. }),
        "expected WorkingDirectory, got {err:?}"
    );
    assert_eq!(s.action_status().unwrap().state, ActionState::Failed);
    assert_eq!(
        &seen.lock().unwrap()[before..],
        &[ActionState::Running, ActionState::Failed],
        "a failure after Running reports Running then Failed"
    );
    assert_eq!(s.state(), SessionState::ReadyEnding);
    assert!(s.environments_entered().is_empty());
}
