// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test/openjd/model/v2023_09/test_template_variables.py
//!
//! Tests that template variable references are correctly validated:
//! - Success: valid references resolve without error
//! - Failure: invalid references produce the expected number of errors

use openjd_model::decode_job_template;
use openjd_model::CallerLimits;

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn decode_ok(s: &str) {
    let v = yaml_val(s);
    decode_job_template(v, None, &CallerLimits::default())
        .unwrap_or_else(|_| panic!("Expected success for: {s}"));
}

#[allow(dead_code)]
fn check_err_count(s: &str, expected_count: usize) {
    let v = yaml_val(s);
    let err = decode_job_template(v, None, &CallerLimits::default())
        .expect_err(&format!("Expected error for: {s}"));
    let msg = err.to_string();
    // Count error lines (each error starts with a path followed by \n\t)
    let actual = msg.matches("\n\t").count();
    assert_eq!(
        actual, expected_count,
        "Expected {expected_count} errors, got {actual}.\nFull error:\n{msg}"
    );
}

// ══════════════════════════════════════════════════════════════
// Success cases
// ══════════════════════════════════════════════════════════════

#[test]
fn minimum_int_parameter() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo {{Param.Foo}}",
        "parameterDefinitions": [{"name": "Foo", "type": "INT"}],
        "steps": [{"name": "StepName", "script": {"actions": {"onRun": {
            "command": "foo {{Param.Foo}} {{RawParam.Foo}} {{Session.WorkingDirectory}}",
            "args": ["foo {{Param.Foo}} {{RawParam.Foo}} {{Session.WorkingDirectory}}"]
        }}}}]
    }"#,
    );
}

#[test]
fn minimum_path_parameter() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo {{RawParam.Foo}}",
        "parameterDefinitions": [{"name": "Foo", "type": "PATH"}],
        "steps": [{"name": "StepName", "script": {"actions": {"onRun": {
            "command": "foo {{Param.Foo}} {{RawParam.Foo}} {{Session.WorkingDirectory}}",
            "args": ["foo {{Param.Foo}} {{RawParam.Foo}} {{Session.WorkingDirectory}}"]
        }}}}]
    }"#,
    );
}

// ══════════════════════════════════════════════════════════════
// Failure cases
// ══════════════════════════════════════════════════════════════

#[test]
fn session_working_directory_not_in_name_scope() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo {{Session.WorkingDirectory}}",
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "foo"}}}}]
    }"#,
    );
    let err = decode_job_template(v, None, &CallerLimits::default())
        .expect_err("Session.WorkingDirectory not in name scope");
    let msg = err.to_string();
    let expected = "\
Model validation error: 1 validation error for JobTemplate
name:
\tFailed to parse interpolation expression at [4, 32]. Undefined variable: 'Session.WorkingDirectory'.
  Session.WorkingDirectory
  ~~~~~~~~^~~~~~~~~~~~~~~~";
    assert_eq!(msg, expected);
}

#[test]
fn path_parameter_not_in_name_scope() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo {{Param.Foo}}",
        "parameterDefinitions": [{"name": "Foo", "type": "PATH"}],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "foo"}}}}]
    }"#,
    );
    let err = decode_job_template(v, None, &CallerLimits::default())
        .expect_err("PATH param not in name scope");
    let msg = err.to_string();
    let expected = "\
Model validation error: 1 validation error for JobTemplate
name:
\tFailed to parse interpolation expression at [4, 17]. Undefined variable: 'Param.Foo'. Did you mean: RawParam.Foo
  Param.Foo
  ~~~~~~^~~";
    assert_eq!(msg, expected);
}

// ══════════════════════════════════════════════════════════════
// Job.Name availability (§7.3): available everywhere except job name field
// ══════════════════════════════════════════════════════════════

fn decode_ok_expr(s: &str) {
    let v = yaml_val(s);
    decode_job_template(
        v,
        Some(&["EXPR", "FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    )
    .unwrap_or_else(|_| panic!("Expected success for: {s}"));
}

#[test]
fn job_name_in_job_environment_variable() {
    decode_ok_expr(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "MyJob",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}}],
        "jobEnvironments": [{"name": "E", "variables": {"JOB": "{{Job.Name}}"}}]
    }"#,
    );
}

#[test]
fn job_name_in_job_environment_script() {
    decode_ok_expr(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "MyJob",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}}],
        "jobEnvironments": [{"name": "E", "script": {"actions": {"onEnter": {"command": "echo", "args": ["{{Job.Name}}"]}}}}]
    }"#,
    );
}

#[test]
fn job_name_in_host_requirements() {
    decode_ok_expr(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "MyJob",
        "steps": [{"name": "S",
            "let": ["jn = Job.Name"],
            "hostRequirements": {"attributes": [{"name": "attr.worker.os.family", "allOf": ["{{jn}}"]}]},
            "script": {"actions": {"onRun": {"command": "echo"}}}
        }]
    }"#,
    );
}

// ══════════════════════════════════════════════════════════════
// Step.Name availability (§7.3): available in step scope including
// hostRequirements and parameterSpace
// ══════════════════════════════════════════════════════════════

#[test]
fn step_name_in_host_requirements() {
    decode_ok_expr(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "MyJob",
        "steps": [{"name": "S",
            "let": ["sn = Step.Name"],
            "hostRequirements": {"attributes": [{"name": "attr.worker.os.family", "allOf": ["{{sn}}"]}]},
            "script": {"actions": {"onRun": {"command": "echo"}}}
        }]
    }"#,
    );
}

#[test]
fn step_name_in_parameter_space_range() {
    decode_ok_expr(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "MyJob",
        "steps": [{"name": "S",
            "parameterSpace": {"taskParameterDefinitions": [
                {"name": "X", "type": "STRING", "range": ["{{Step.Name}}"]}
            ]},
            "script": {"actions": {"onRun": {"command": "echo"}}}
        }]
    }"#,
    );
}
