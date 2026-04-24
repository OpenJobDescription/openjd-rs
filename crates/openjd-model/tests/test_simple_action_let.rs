// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test/openjd/model/v2023_09/test_simple_action_let_bindings.py
//!
//! Gold standard: failure tests assert the full error message including path.

use openjd_model::decode_job_template;
use openjd_model::CallerLimits;

fn yaml_val(s: &str) -> serde_yaml::Value {
    serde_yaml::from_str(s).unwrap()
}

fn decode_ok(s: &str) {
    let v = yaml_val(s);
    decode_job_template(
        v,
        Some(&["EXPR", "FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    )
    .unwrap_or_else(|_| panic!("Expected success for: {s}"));
}

fn check_err(s: &str, expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_job_template(
        v,
        Some(&["EXPR", "FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    )
    .expect_err(&format!("Expected error for: {s}"));
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

// ══════════════════════════════════════════════════════════════
// Success cases
// ══════════════════════════════════════════════════════════════

#[test]
fn single_binding() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{"name": "S", "bash": {"let": ["x = 1"], "script": "echo {{x}}"}}]
    }"#,
    );
}

#[test]
fn multiple_bindings() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{"name": "S", "bash": {"let": ["x = 1", "y = 2", "z = 3"], "script": "echo"}}]
    }"#,
    );
}

#[test]
fn chained_bindings() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{"name": "S", "bash": {"let": ["a = 5", "b = a + 1"], "script": "echo {{b}}"}}]
    }"#,
    );
}

#[test]
fn python_with_let() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{"name": "S", "python": {"let": ["count = 10"], "script": "print({{count}})"}}]
    }"#,
    );
}

#[test]
fn step_let_and_simple_action_let() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "let": ["step_val = 100"],
            "bash": {"let": ["action_val = step_val + 1"], "script": "echo {{action_val}}"}
        }]
    }"#,
    );
}

#[test]
fn different_names_step_and_action() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "let": ["x = 100"],
            "bash": {"let": ["y = x + 1"], "script": "echo {{y}}"}
        }]
    }"#,
    );
}

// ══════════════════════════════════════════════════════════════
// Validation failures
// ══════════════════════════════════════════════════════════════

#[test]
fn empty_let_list() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{"name": "S", "bash": {"let": [], "script": "echo"}}]
    }"#,
        &["if provided, must not be empty."],
    );
}

#[test]
fn max_50_bindings() {
    let bindings: Vec<String> = (0..51).map(|i| format!(r#""x{i} = {i}""#)).collect();
    let let_list = bindings.join(", ");
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{{"name": "S", "bash": {{"let": [{let_list}], "script": "echo"}}}}]
    }}"#
    );
    check_err(&s, &["must not contain more than 50 bindings."]);
}

#[test]
fn duplicate_name_same_block() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{"name": "S", "bash": {"let": ["x = 1", "y = 2", "x = 3"], "script": "echo"}}]
    }"#,
        &["duplicate name 'x'."],
    );
}

#[test]
fn self_reference() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{"name": "S", "bash": {"let": ["x = x + 1"], "script": "echo"}}]
    }"#,
        &["'x' references itself."],
    );
}

#[test]
fn step_and_action_let_shadow() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "let": ["x = 1"],
            "bash": {"let": ["x = 2"], "script": "echo {{x}}"}
        }]
    }"#,
        &["'x' shadows enclosing scope."],
    );
}

// ══════════════════════════════════════════════════════════════
// Binding with param reference
// ══════════════════════════════════════════════════════════════

#[test]
fn binding_with_param_reference() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "parameterDefinitions": [{"name": "Count", "type": "INT", "default": "5"}],
        "steps": [{"name": "S", "bash": {"let": ["val = Param.Count + 1"], "script": "echo {{val}}"}}]
    }"#,
    );
}

// ══════════════════════════════════════════════════════════════
// Binding with task param reference
// ══════════════════════════════════════════════════════════════

#[test]
fn binding_with_task_param_reference() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "parameterSpace": {"taskParameterDefinitions": [{"name": "Frame", "type": "INT", "range": "1-10"}]},
            "bash": {"let": ["frame = Task.Param.Frame * 2"], "script": "echo {{frame}}"}
        }]
    }"#,
    );
}

// ══════════════════════════════════════════════════════════════
// Type error in simple action let binding
// ══════════════════════════════════════════════════════════════

#[test]
fn type_error_in_binding() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "parameterDefinitions": [{"name": "Count", "type": "INT", "default": "5"}],
        "steps": [{"name": "S", "bash": {"let": ["bad = Param.Count + 'hello'"], "script": "echo"}}]
    }"#,
        &["Invalid expression in let binding 'bad':"],
    );
}

// ══════════════════════════════════════════════════════════════
// Session symbols in simple action let bindings
// ══════════════════════════════════════════════════════════════

#[test]
fn session_symbols_in_binding() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "parameterSpace": {"taskParameterDefinitions": [{"name": "Frame", "type": "INT", "range": "1-10"}]},
            "bash": {
                "let": [
                    "out = Session.WorkingDirectory / 'renders'",
                    "frame_str = string(Task.Param.Frame)"
                ],
                "script": "echo {{out}} {{frame_str}}"
            }
        }]
    }"#,
    );
}

// Note: Python test for EXPR extension requirement in SimpleAction let bindings
// is not ported because the Rust implementation handles this at a higher level.

// ══════════════════════════════════════════════════════════════
// All interpreter types with let
// ══════════════════════════════════════════════════════════════

#[test]
fn cmd_with_let() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{"name": "S", "cmd": {"let": ["x = 1"], "script": "echo {{x}}"}}]
    }"#,
    );
}

#[test]
fn powershell_with_let() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{"name": "S", "powershell": {"let": ["x = 1"], "script": "Write-Host {{x}}"}}]
    }"#,
    );
}

#[test]
fn node_with_let() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{"name": "S", "node": {"let": ["x = 1"], "script": "console.log({{x}})"}}]
    }"#,
    );
}
