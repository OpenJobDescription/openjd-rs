// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Gold standard error message tests.
//!
//! Every failure test asserts the full error output including:
//! - Error count and model name
//! - Field path (matching Python Pydantic format)
//! - Error message
//!
//! This ensures error messages are stable and match the Python implementation.

use openjd_model::CallerLimits;
use openjd_model::{decode_environment_template, decode_job_template};

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn check_err(s: &str, expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_job_template(
        v,
        Some(&["EXPR", "FEATURE_BUNDLE_1", "TASK_CHUNKING"]),
        &CallerLimits::default(),
    )
    .expect_err("Expected validation error");
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

fn check_env_err(s: &str, expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_environment_template(v, Some(&["EXPR", "FEATURE_BUNDLE_1"]))
        .expect_err("Expected validation error");
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

// ══════════════════════════════════════════════════════════════
// Template-level errors
// ══════════════════════════════════════════════════════════════

#[test]
fn empty_steps() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": []
    }"#,
        &[
            "1 validation error for JobTemplate\n",
            "JobTemplate: must have at least one step.",
        ],
    );
}

#[test]
fn empty_name() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["name:\n\tmust not be empty."],
    );
}

#[test]
fn empty_parameter_definitions() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["parameterDefinitions:\n\tif provided, must contain at least one element."],
    );
}

// ══════════════════════════════════════════════════════════════
// Parameter definition errors (with path)
// ══════════════════════════════════════════════════════════════

#[test]
fn duplicate_parameter_name() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [
            {"name": "Foo", "type": "STRING"},
            {"name": "Foo", "type": "STRING"}
        ],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["parameterDefinitions[1]:\n\tduplicate parameter name: 'Foo'"],
    );
}

#[test]
fn int_default_above_max() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [
            {"name": "X", "type": "INT", "default": 100, "maxValue": 50}
        ],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["parameterDefinitions[0]:\n\t"],
    );
}

// ══════════════════════════════════════════════════════════════
// Step errors (with indexed path)
// ══════════════════════════════════════════════════════════════

#[test]
fn missing_script() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S"}]
    }"#,
        &["steps[0]:\n\tmust have 'script' or a simple action field."],
    );
}

#[test]
fn duplicate_step_name() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [
            {"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}},
            {"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}
        ]
    }"#,
        &["steps[1] -> name:\n\tduplicate step name: 'S'"],
    );
}

#[test]
fn empty_command() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": ""}}}}]
    }"#,
        &["steps[0] -> script -> actions -> onRun -> command:\n\tmust not be empty."],
    );
}

// ══════════════════════════════════════════════════════════════
// Host requirements errors (deeply nested path)
// ══════════════════════════════════════════════════════════════

#[test]
fn host_req_os_family_invalid() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S",
            "hostRequirements": {"attributes": [{"name": "attr.worker.os.family", "anyOf": ["ubuntu"]}]},
            "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &[
            "steps[0] -> hostRequirements -> attributes[0] -> anyOf:\n\t",
            "not valid for attr.worker.os.family",
        ],
    );
}

// ══════════════════════════════════════════════════════════════
// Combination expression errors
// ══════════════════════════════════════════════════════════════

#[test]
fn combination_double_operator() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "A", "type": "INT", "range": [1]},
                    {"name": "B", "type": "INT", "range": [1]}
                ],
                "combination": "A ** B"
            },
            "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["steps[0] -> parameterSpace -> combination:\n\t"],
    );
}

#[test]
fn combination_duplicate_param() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "A", "type": "INT", "range": [1]}
                ],
                "combination": "A * A"
            },
            "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["steps[0] -> parameterSpace -> combination:\n\tparameter 'A' appears more than once"],
    );
}

// ══════════════════════════════════════════════════════════════
// Limit errors
// ══════════════════════════════════════════════════════════════

#[test]
fn job_name_too_long() {
    let long_name = "A".repeat(129);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "{long_name}",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "cmd"}}}}}}}}]
    }}"#
    );
    check_err(&s, &["name:\n\texceeds 128 characters."]);
}

// ══════════════════════════════════════════════════════════════
// Environment template errors
// ══════════════════════════════════════════════════════════════

#[test]
fn env_name_too_long() {
    let long_name = "A".repeat(65);
    let s = format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "environment": {{
            "name": "{long_name}",
            "variables": {{"X": "1"}}
        }}
    }}"#
    );
    check_env_err(
        &s,
        &[
            "1 validation error for EnvironmentTemplate\n",
            "environment -> name:\n\texceeds 64 characters.",
        ],
    );
}

// ══════════════════════════════════════════════════════════════
// EXPR extension errors
// ══════════════════════════════════════════════════════════════

#[test]
fn let_without_expr() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S",
            "let": ["x = 1"],
            "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["steps[0] -> let:\n\t'let' requires the EXPR extension."],
    );
}

#[test]
fn complex_expr_without_expr() {
    check_err(r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "X", "type": "INT", "default": 1}],
        "steps": [{"name": "S",
            "script": {"actions": {"onRun": {"command": "echo", "args": ["{{Param.X + 1}}"]}}}}]
    }"#, &[
        "steps[0] -> script -> actions -> onRun -> args[0]:\n\tcomplex expressions require the EXPR extension.",
    ]);
}

// ══════════════════════════════════════════════════════════════
// Multiple errors in one template
// ══════════════════════════════════════════════════════════════

#[test]
fn multiple_errors() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "",
        "steps": []
    }"#,
        &[
            "2 validation errors for JobTemplate\n",
            "name:\n\tmust not be empty.",
            "JobTemplate: must have at least one step.",
        ],
    );
}
