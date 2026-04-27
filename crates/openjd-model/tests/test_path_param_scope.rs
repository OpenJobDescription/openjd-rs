// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test/openjd/model/v2023_09/test_path_param_scope.py
//!
//! PATH and LIST[PATH] parameters define two variables:
//! - Param.<name>: Only accessible in host contexts (SESSION and TASK scopes)
//! - RawParam.<name>: Accessible in all contexts (TEMPLATE, SESSION, and TASK scopes)

use openjd_model::decode_job_template;
use openjd_model::CallerLimits;

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn check_ok(s: &str) {
    let v = yaml_val(s);
    decode_job_template(v, None, &CallerLimits::default()).unwrap();
}

fn check_ok_ext(s: &str, ext: &[&str]) {
    let v = yaml_val(s);
    decode_job_template(v, Some(ext), &CallerLimits::default()).unwrap();
}

fn check_err(s: &str, expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_job_template(v, None, &CallerLimits::default())
        .expect_err(&format!("Expected error for: {s}"));
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

fn check_err_ext(s: &str, ext: &[&str], expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_job_template(v, Some(ext), &CallerLimits::default())
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
// TestPathParameterScope — TEMPLATE scope: Param.* NOT accessible
// ══════════════════════════════════════════════════════════════

#[test]
fn path_param_not_in_job_name() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo {{Param.Foo}}",
        "parameterDefinitions": [{"name": "Foo", "type": "PATH"}],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}}}]
    }"#,
        &["name:", "Param.Foo"],
    );
}

#[test]
fn path_param_not_in_parameter_space_range() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "parameterDefinitions": [{"name": "Foo", "type": "PATH"}],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {"taskParameterDefinitions": [
                {"name": "Bar", "type": "STRING", "range": ["{{Param.Foo}}"]}
            ]}
        }]
    }"#,
        &["Param.Foo"],
    );
}

#[test]
fn path_param_not_in_int_range_start() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "parameterDefinitions": [{"name": "Foo", "type": "PATH"}],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {"taskParameterDefinitions": [
                {"name": "Bar", "type": "INT", "range": "{{Param.Foo}}"}
            ]}
        }]
    }"#,
        &["Param.Foo"],
    );
}

// ══════════════════════════════════════════════════════════════
// TestPathParameterScope — TEMPLATE scope: RawParam.* SHOULD be accessible
// ══════════════════════════════════════════════════════════════

#[test]
fn path_rawparam_in_job_name() {
    check_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo {{RawParam.Foo}}",
        "parameterDefinitions": [{"name": "Foo", "type": "PATH"}],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}}}]
    }"#,
    );
}

#[test]
fn path_rawparam_in_parameter_space_range() {
    check_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "parameterDefinitions": [{"name": "Foo", "type": "PATH"}],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {"taskParameterDefinitions": [
                {"name": "Bar", "type": "STRING", "range": ["{{RawParam.Foo}}"]}
            ]}
        }]
    }"#,
    );
}

// ══════════════════════════════════════════════════════════════
// TestPathParameterScope — SESSION scope: Param.* SHOULD be accessible
// ══════════════════════════════════════════════════════════════

#[test]
fn path_param_in_environment_script() {
    check_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "parameterDefinitions": [{"name": "Foo", "type": "PATH"}],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}}}],
        "jobEnvironments": [{"name": "Env", "script": {"actions": {"onEnter": {"command": "echo {{Param.Foo}}"}}}}]
    }"#,
    );
}

#[test]
fn path_param_in_step_environment_script() {
    check_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "parameterDefinitions": [{"name": "Foo", "type": "PATH"}],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "stepEnvironments": [{"name": "StepEnv", "script": {"actions": {"onEnter": {"command": "echo {{Param.Foo}}"}}}}]
        }]
    }"#,
    );
}

// ══════════════════════════════════════════════════════════════
// TestPathParameterScope — TASK scope: Param.* SHOULD be accessible
// ══════════════════════════════════════════════════════════════

#[test]
fn path_param_in_step_script() {
    check_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "parameterDefinitions": [{"name": "Foo", "type": "PATH"}],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo {{Param.Foo}}"}}}}]
    }"#,
    );
}

#[test]
fn path_param_in_step_script_args() {
    check_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "parameterDefinitions": [{"name": "Foo", "type": "PATH"}],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo", "args": ["{{Param.Foo}}"]}}}}]
    }"#,
    );
}

// ══════════════════════════════════════════════════════════════
// TestListPathParameterScope — TEMPLATE scope: Param.* NOT accessible
// ══════════════════════════════════════════════════════════════

#[test]
fn list_path_param_not_in_job_name() {
    check_err_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Foo {{Param.Foo}}",
        "parameterDefinitions": [{"name": "Foo", "type": "LIST[PATH]", "default": ["/tmp"]}],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}}}]
    }"#,
        &["EXPR"],
        &["name:", "Param.Foo"],
    );
}

#[test]
fn list_path_param_not_in_parameter_space_range() {
    check_err_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Foo",
        "parameterDefinitions": [{"name": "Foo", "type": "LIST[PATH]", "default": ["/tmp"]}],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {"taskParameterDefinitions": [
                {"name": "Bar", "type": "STRING", "range": ["{{Param.Foo}}"]}
            ]}
        }]
    }"#,
        &["EXPR"],
        &["Param.Foo"],
    );
}

// ══════════════════════════════════════════════════════════════
// TestListPathParameterScope — TEMPLATE scope: RawParam.* SHOULD be accessible
// ══════════════════════════════════════════════════════════════

#[test]
fn list_path_rawparam_in_job_name() {
    check_ok_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Foo {{RawParam.Foo}}",
        "parameterDefinitions": [{"name": "Foo", "type": "LIST[PATH]", "default": ["/tmp"]}],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}}}]
    }"#,
        &["EXPR"],
    );
}

#[test]
fn list_path_rawparam_in_parameter_space_range() {
    check_ok_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Foo",
        "parameterDefinitions": [{"name": "Foo", "type": "LIST[PATH]", "default": ["/tmp"]}],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {"taskParameterDefinitions": [
                {"name": "Bar", "type": "STRING", "range": ["{{RawParam.Foo}}"]}
            ]}
        }]
    }"#,
        &["EXPR"],
    );
}

// ══════════════════════════════════════════════════════════════
// TestListPathParameterScope — SESSION scope: Param.* SHOULD be accessible
// ══════════════════════════════════════════════════════════════

#[test]
fn list_path_param_in_environment_script() {
    check_ok_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Foo",
        "parameterDefinitions": [{"name": "Foo", "type": "LIST[PATH]", "default": ["/tmp"]}],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}}}],
        "jobEnvironments": [{"name": "Env", "script": {"actions": {"onEnter": {"command": "echo {{Param.Foo}}"}}}}]
    }"#,
        &["EXPR"],
    );
}

#[test]
fn list_path_param_in_step_environment_script() {
    check_ok_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Foo",
        "parameterDefinitions": [{"name": "Foo", "type": "LIST[PATH]", "default": ["/tmp"]}],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "stepEnvironments": [{"name": "StepEnv", "script": {"actions": {"onEnter": {"command": "echo {{Param.Foo}}"}}}}]
        }]
    }"#,
        &["EXPR"],
    );
}

// ══════════════════════════════════════════════════════════════
// TestListPathParameterScope — TEMPLATE scope: RawParam.* type correctness
// ══════════════════════════════════════════════════════════════

#[test]
fn list_path_rawparam_has_list_type_in_template_scope() {
    // RawParam.Foo for LIST[PATH] should be list[string], not string.
    // len() on list[string] returns int; len() on string also returns int,
    // but sorted() only accepts lists — so this validates the type is list.
    check_ok_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Job {{sorted(RawParam.Foo)}}",
        "parameterDefinitions": [{"name": "Foo", "type": "LIST[PATH]", "default": ["/tmp"]}],
        "steps": [{"name": "Step",
            "script": {"actions": {"onRun": {"command": "echo"}}}
        }]
    }"#,
        &["EXPR"],
    );
}

// ══════════════════════════════════════════════════════════════
// TestListPathParameterScope — TASK scope: Param.* SHOULD be accessible
// ══════════════════════════════════════════════════════════════

#[test]
fn list_path_param_in_step_script() {
    check_ok_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Foo",
        "parameterDefinitions": [{"name": "Foo", "type": "LIST[PATH]", "default": ["/tmp"]}],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo {{Param.Foo}}"}}}}]
    }"#,
        &["EXPR"],
    );
}

#[test]
fn list_path_param_in_step_script_args() {
    check_ok_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Foo",
        "parameterDefinitions": [{"name": "Foo", "type": "LIST[PATH]", "default": ["/tmp"]}],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo", "args": ["{{Param.Foo}}"]}}}}]
    }"#,
        &["EXPR"],
    );
}

// ══════════════════════════════════════════════════════════════
// Env.File.* must NOT be available in step scripts (§7.3)
// ══════════════════════════════════════════════════════════════

#[test]
fn env_file_not_available_in_step_script_with_expr() {
    // Env.File.* is only available within environment scripts, not step scripts.
    check_err_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "stepEnvironments": [{
                "name": "E",
                "script": {
                    "embeddedFiles": [{"name": "cfg", "type": "TEXT", "data": "hello"}],
                    "actions": {"onEnter": {"command": "echo"}}
                }
            }],
            "script": {"actions": {"onRun": {"command": "echo", "args": ["{{Env.File.cfg}}"]}}}
        }]
    }"#,
        &["EXPR"],
        &["Env.File.cfg"],
    );
}
