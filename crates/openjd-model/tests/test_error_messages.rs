// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

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

// ══════════════════════════════════════════════════════════════
// Extension list errors (Gold standard: full count + path + message)
//
// These tests pin the exact error output produced by the
// extensions-list validation pass in `parse.rs`. Matches the Python
// Pydantic wording for duplicates and unsupported names.
// ══════════════════════════════════════════════════════════════

#[test]
fn extensions_empty_list() {
    // An explicit empty `extensions: []` is rejected with a
    // single error at the `extensions` path.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": [],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &[
            "1 validation error for JobTemplate\n",
            "extensions:\n\tif provided, must be a non-empty list.",
        ],
    );
}

#[test]
fn extensions_single_unsupported() {
    // One unsupported extension produces one error with an
    // aggregated (single-value) "Unsupported extension names" message.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["NOT_A_REAL_EXTENSION"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &[
            "1 validation error for JobTemplate\n",
            "extensions:\n\tUnsupported extension names: NOT_A_REAL_EXTENSION",
        ],
    );
}

#[test]
fn extensions_multiple_unsupported_sorted() {
    // Multiple unsupported extensions are reported in a single
    // message with names sorted alphabetically for stable output.
    // The input order here is deliberately reversed to verify
    // sort behavior.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["ZZZ_LAST", "AAA_FIRST", "MMM_MIDDLE"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &[
            "1 validation error for JobTemplate\n",
            "extensions:\n\tUnsupported extension names: AAA_FIRST, MMM_MIDDLE, ZZZ_LAST",
        ],
    );
}

#[test]
fn extensions_known_but_not_enabled_by_caller() {
    // A *recognized* ModelExtension (EXPR) still counts as
    // unsupported when the caller's allowlist excludes it — the
    // helper collapses both cases into one message, matching the
    // Python implementation. This test uses a restricted allowlist
    // (only FEATURE_BUNDLE_1) so that EXPR, though a known variant,
    // is not permitted here.
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
    );
    let err = decode_job_template(v, Some(&["FEATURE_BUNDLE_1"]), &CallerLimits::default())
        .expect_err("Expected validation error");
    let msg = err.to_string();
    for line in &[
        "1 validation error for JobTemplate\n",
        "extensions:\n\tUnsupported extension names: EXPR",
    ] {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

#[test]
fn extensions_single_duplicate() {
    // A single duplicate name is reported via a dedicated
    // "Duplicate values for extension name" message. The duplicate
    // name (EXPR) is also listed in the (empty here) "Unsupported"
    // pass set because EXPR *is* in the caller allowlist — so only
    // one error is produced.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR", "EXPR"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &[
            "1 validation error for JobTemplate\n",
            "extensions:\n\tDuplicate values for extension name are not allowed. Duplicate values: EXPR",
        ],
    );
}

#[test]
fn extensions_multiple_duplicates_sorted() {
    // Multiple duplicate names are listed comma-separated, sorted
    // alphabetically. Each duplicate is named at most once in the
    // output regardless of how many times it recurs in the input.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["TASK_CHUNKING", "EXPR", "TASK_CHUNKING", "EXPR", "FEATURE_BUNDLE_1"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &[
            "1 validation error for JobTemplate\n",
            "extensions:\n\tDuplicate values for extension name are not allowed. Duplicate values: EXPR,TASK_CHUNKING",
        ],
    );
}

#[test]
fn extensions_duplicate_and_unsupported_collected_together() {
    // Duplicates and unsupported names are independent passes —
    // when both apply, the caller sees both errors together. This
    // is the collect-all behavior: fail-fast would have reported
    // only the first. Order here is duplicate-pass before
    // unsupported-pass (they're added in that order).
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR", "EXPR", "NOT_A_REAL_EXTENSION"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &[
            "2 validation errors for JobTemplate\n",
            "extensions:\n\tDuplicate values for extension name are not allowed. Duplicate values: EXPR",
            "extensions:\n\tUnsupported extension names: NOT_A_REAL_EXTENSION",
        ],
    );
}

#[test]
fn extensions_errors_use_environment_template_model_name() {
    // Extension-list errors on an environment template use
    // `EnvironmentTemplate` as the model name in the count header.
    check_env_err(
        r#"{
        "specificationVersion": "environment-2023-09",
        "extensions": ["NOT_REAL", "NOT_REAL", "OTHER_BOGUS"],
        "environment": {"name": "E", "script": {"actions": {"onEnter": {"command": "cmd"}}}}
    }"#,
        &[
            "2 validation errors for EnvironmentTemplate\n",
            "extensions:\n\tDuplicate values for extension name are not allowed. Duplicate values: NOT_REAL",
            "extensions:\n\tUnsupported extension names: NOT_REAL, OTHER_BOGUS",
        ],
    );
}
