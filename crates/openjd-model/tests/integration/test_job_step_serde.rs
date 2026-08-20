// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Wire-format tests for the job-side `StepScript` and `EnvironmentScript`
//! `let` bindings (see `specs/model/job-types.md`).
//!
//! The OpenJD wire key is `"let"` (matching the template side). These tests
//! pin that the job-side deserializer accepts the `"let"` wire key, keeps the
//! legacy `"letBindings"` spelling working as an alias, emits `"let"` on
//! serialization, and rejects unknown fields instead of silently dropping
//! them. `EnvironmentScript` is exercised through a full `job::Step` document
//! because that is how the worker reaches it via `deserialize_step`.

use openjd_expr::FormatString;
use openjd_model::job::{
    Action, EnvironmentActions, EnvironmentScript, Step, StepActions, StepScript,
};

/// Asserts that a serialized script object uses the `let` wire key and does
/// not emit the legacy `letBindings` spelling.
fn assert_emits_let_wire_key(value: &serde_json::Value) {
    let obj = value.as_object().expect("script serializes to an object");
    assert!(
        obj.contains_key("let"),
        "expected the `let` wire key, got: {value}",
    );
    assert!(
        !obj.contains_key("letBindings"),
        "must not emit the legacy `letBindings` key, got: {value}",
    );
}

/// Asserts that `err` is an unknown-field error naming the offending key.
fn assert_unknown_field_error(err: &serde_json::Error, key: &str) {
    let msg = err.to_string();
    assert!(
        msg.contains(&format!("unknown field `{key}`")),
        "expected an unknown-field error naming `{key}`, got: {msg}",
    );
}

// ---------------------------------------------------------------------------
// Group A — accept/serialize the `let` wire key
// ---------------------------------------------------------------------------

#[test]
fn step_script_accepts_let_wire_key() {
    let step: Step = serde_json::from_str(
        r#"{"name":"S","script":{"let":["greeting = 'hello'"],"actions":{"onRun":{"command":"echo hello"}}}}"#,
    )
    .expect("step with the `let` wire key must deserialize");
    assert_eq!(
        step.script.let_bindings,
        Some(vec!["greeting = 'hello'".to_string()]),
    );
}

#[test]
fn step_script_accepts_legacy_letbindings_alias() {
    let step: Step = serde_json::from_str(
        r#"{"name":"S","script":{"letBindings":["greeting = 'hello'"],"actions":{"onRun":{"command":"echo hello"}}}}"#,
    )
    .expect("step with the legacy `letBindings` key must deserialize");
    assert_eq!(
        step.script.let_bindings,
        Some(vec!["greeting = 'hello'".to_string()]),
    );
}

#[test]
fn step_script_serializes_let_key() {
    let script = StepScript {
        let_bindings: Some(vec!["greeting = 'hello'".to_string()]),
        actions: StepActions {
            on_run: Action {
                command: FormatString::new("echo hello").unwrap(),
                args: None,
                timeout: None,
                cancelation: None,
            },
        },
        embedded_files: None,
    };
    let value = serde_json::to_value(&script).unwrap();
    assert_emits_let_wire_key(&value);
}

#[test]
fn step_script_rejects_duplicate_let_keys() {
    let err = serde_json::from_str::<Step>(
        r#"{"name":"S","script":{"let":["a = '1'"],"letBindings":["b = '2'"],"actions":{"onRun":{"command":"echo hello"}}}}"#,
    )
    .expect_err("supplying both `let` and `letBindings` must be a duplicate-field error");
    assert!(
        err.to_string().contains("duplicate field `let`"),
        "expected a duplicate-field error naming `let`, got: {err}",
    );
}

#[test]
fn environment_script_in_step_environments_accepts_let() {
    let step: Step = serde_json::from_str(
        r#"{"name":"S","script":{"actions":{"onRun":{"command":"echo hello"}}},"stepEnvironments":[{"name":"E","script":{"let":["x = '1'"],"actions":{"onEnter":{"command":"echo hello"}}}}]}"#,
    )
    .expect("step whose environment script uses the `let` wire key must deserialize");
    let env = &step
        .step_environments
        .as_ref()
        .expect("stepEnvironments present")[0];
    let env_script = env.script.as_ref().expect("environment script present");
    assert_eq!(env_script.let_bindings, Some(vec!["x = '1'".to_string()]));
}

#[test]
fn environment_script_serializes_let_key() {
    let script = EnvironmentScript {
        let_bindings: Some(vec!["x = '1'".to_string()]),
        actions: EnvironmentActions {
            on_enter: None,
            on_wrap_env_enter: None,
            on_wrap_task_run: None,
            on_wrap_env_exit: None,
            on_exit: None,
        },
        embedded_files: None,
    };
    let value = serde_json::to_value(&script).unwrap();
    assert_emits_let_wire_key(&value);
}

// ---------------------------------------------------------------------------
// Group B — reject unknown fields (deny_unknown_fields)
// ---------------------------------------------------------------------------

#[test]
fn step_script_rejects_unknown_field() {
    let err = serde_json::from_str::<Step>(
        r#"{"name":"S","script":{"letBindingz":[],"actions":{"onRun":{"command":"echo hello"}}}}"#,
    )
    .expect_err("an unknown script field must be rejected, not silently dropped");
    assert_unknown_field_error(&err, "letBindingz");
}

#[test]
fn step_script_alias_works_with_deny_unknown_fields() {
    // serde historically had a bug where `deny_unknown_fields` caused field
    // aliases to be ignored; this pins that the legacy `letBindings` alias
    // still deserializes once unknown fields are rejected.
    let step: Step = serde_json::from_str(
        r#"{"name":"S","script":{"letBindings":["greeting = 'hello'"],"actions":{"onRun":{"command":"echo hello"}}}}"#,
    )
    .expect("the `letBindings` alias must still deserialize under deny_unknown_fields");
    assert_eq!(
        step.script.let_bindings,
        Some(vec!["greeting = 'hello'".to_string()]),
    );
}

#[test]
fn environment_script_rejects_unknown_field() {
    let err = serde_json::from_str::<Step>(
        r#"{"name":"S","script":{"actions":{"onRun":{"command":"echo hello"}}},"stepEnvironments":[{"name":"E","script":{"letBindingz":[],"actions":{"onEnter":{"command":"echo hello"}}}}]}"#,
    )
    .expect_err("an unknown environment-script field must be rejected");
    assert_unknown_field_error(&err, "letBindingz");
}
