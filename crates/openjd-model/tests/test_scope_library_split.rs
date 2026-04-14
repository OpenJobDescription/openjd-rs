// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests that the function library split between TEMPLATE and SESSION/TASK scopes
//! is correctly enforced during validation.
//!
//! `apply_path_mapping` is only available in host context (SESSION/TASK scope).
//! Using it in TEMPLATE-scope fields must fail validation.
//! Using it in SESSION/TASK-scope fields must pass validation.

use openjd_model::decode_job_template;

fn yaml_val(s: &str) -> serde_yaml::Value {
    serde_yaml::from_str(s).unwrap()
}

fn decode_ok(s: &str) {
    let v = yaml_val(s);
    decode_job_template(v, Some(&["EXPR", "FEATURE_BUNDLE_1"]))
        .unwrap_or_else(|_| panic!("Expected success for: {s}"));
}

fn check_err(s: &str, expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_job_template(v, Some(&["EXPR", "FEATURE_BUNDLE_1"]))
        .expect_err(&format!("Expected error for: {s}"));
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

/// Helper: a job template with a PATH param and an expression in the given field.
fn job_with_path_param(name_expr: &str, step_body: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR", "FEATURE_BUNDLE_1"],
        "name": {name_expr},
        "parameterDefinitions": [{{"name": "Val", "type": "PATH"}}],
        "steps": [{step_body}]
    }}"#
    )
}

fn simple_step(script_body: &str) -> String {
    format!(r#"{{"name": "S", "script": {script_body}}}"#)
}

fn step_with_hr(hr: &str) -> String {
    format!(
        r#"{{
        "name": "S",
        "hostRequirements": {hr},
        "script": {{"actions": {{"onRun": {{"command": "run"}}}}}}
    }}"#
    )
}

// ═══════════════════════════════════════════════════════════════
// TEMPLATE scope — apply_path_mapping must FAIL
// ═══════════════════════════════════════════════════════════════

#[test]
fn template_scope_job_name_rejects_apply_path_mapping() {
    let t = job_with_path_param(
        r#""{{apply_path_mapping(Param.Val)}}""#,
        &simple_step(r#"{"actions": {"onRun": {"command": "run"}}}"#),
    );
    check_err(&t, &["apply_path_mapping"]);
}

#[test]
fn template_scope_host_req_amount_rejects_apply_path_mapping() {
    let t = job_with_path_param(
        r#""test""#,
        &step_with_hr(
            r#"{"amounts": [{"name": "amount.worker.vcpu", "min": "{{len(apply_path_mapping(Param.Val))}}"}]}"#,
        ),
    );
    check_err(&t, &["apply_path_mapping"]);
}

#[test]
fn template_scope_step_let_binding_rejects_apply_path_mapping() {
    let t = job_with_path_param(
        r#""test""#,
        r#"{
            "name": "S",
            "let": ["mapped = apply_path_mapping(Param.Val)"],
            "script": {"actions": {"onRun": {"command": "run"}}}
        }"#,
    );
    check_err(&t, &["apply_path_mapping"]);
}

// ═══════════════════════════════════════════════════════════════
// SESSION/TASK scope — apply_path_mapping must SUCCEED
// ═══════════════════════════════════════════════════════════════

#[test]
fn task_scope_action_command_accepts_apply_path_mapping() {
    let t = job_with_path_param(
        r#""test""#,
        &simple_step(r#"{"actions": {"onRun": {"command": "{{apply_path_mapping(Param.Val)}}"}}}"#),
    );
    decode_ok(&t);
}

#[test]
fn task_scope_action_args_accepts_apply_path_mapping() {
    let t = job_with_path_param(
        r#""test""#,
        &simple_step(
            r#"{"actions": {"onRun": {"command": "run", "args": ["{{apply_path_mapping(Param.Val)}}"]}}}"#,
        ),
    );
    decode_ok(&t);
}

#[test]
fn task_scope_action_timeout_accepts_apply_path_mapping() {
    let t = job_with_path_param(
        r#""test""#,
        &simple_step(
            r#"{"actions": {"onRun": {"command": "run", "timeout": "{{len(apply_path_mapping(Param.Val))}}"}}}"#,
        ),
    );
    decode_ok(&t);
}

#[test]
fn task_scope_cancelation_notify_accepts_apply_path_mapping() {
    let t = job_with_path_param(
        r#""test""#,
        &simple_step(
            r#"{"actions": {"onRun": {"command": "run", "cancelation": {"mode": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": "{{len(apply_path_mapping(Param.Val))}}"}}}}"#,
        ),
    );
    decode_ok(&t);
}

#[test]
fn task_scope_embedded_file_data_accepts_apply_path_mapping() {
    let t = job_with_path_param(
        r#""test""#,
        &simple_step(
            r#"{
            "embeddedFiles": [{"name": "f", "type": "TEXT", "data": "{{apply_path_mapping(Param.Val)}}"}],
            "actions": {"onRun": {"command": "run"}}
        }"#,
        ),
    );
    decode_ok(&t);
}

#[test]
fn task_scope_embedded_file_filename_accepts_apply_path_mapping() {
    let t = job_with_path_param(
        r#""test""#,
        &simple_step(
            r#"{
            "embeddedFiles": [{"name": "f", "type": "TEXT", "filename": "{{apply_path_mapping(Param.Val)}}", "data": "x"}],
            "actions": {"onRun": {"command": "run"}}
        }"#,
        ),
    );
    decode_ok(&t);
}

#[test]
fn task_scope_script_let_binding_accepts_apply_path_mapping() {
    let t = job_with_path_param(
        r#""test""#,
        &simple_step(
            r#"{
            "let": ["mapped = apply_path_mapping(Param.Val)"],
            "actions": {"onRun": {"command": "echo", "args": ["{{mapped}}"]}}
        }"#,
        ),
    );
    decode_ok(&t);
}

#[test]
fn session_scope_job_env_variable_accepts_apply_path_mapping() {
    let t = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR", "FEATURE_BUNDLE_1"],
        "name": "test",
        "parameterDefinitions": [{"name": "Val", "type": "PATH"}],
        "jobEnvironments": [{
            "name": "E",
            "variables": {"MAPPED": "{{apply_path_mapping(Param.Val)}}"}
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#
    .to_string();
    decode_ok(&t);
}

#[test]
fn session_scope_job_env_action_accepts_apply_path_mapping() {
    let t = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR", "FEATURE_BUNDLE_1"],
        "name": "test",
        "parameterDefinitions": [{"name": "Val", "type": "PATH"}],
        "jobEnvironments": [{
            "name": "E",
            "script": {"actions": {"onEnter": {"command": "{{apply_path_mapping(Param.Val)}}"}}}
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#
    .to_string();
    decode_ok(&t);
}

#[test]
fn session_scope_step_env_action_accepts_apply_path_mapping() {
    let t = job_with_path_param(
        r#""test""#,
        r#"{
            "name": "S",
            "stepEnvironments": [{
                "name": "SE",
                "script": {"actions": {"onEnter": {"command": "{{apply_path_mapping(Param.Val)}}"}}}
            }],
            "script": {"actions": {"onRun": {"command": "run"}}}
        }"#,
    );
    decode_ok(&t);
}

#[test]
fn session_scope_env_embedded_file_accepts_apply_path_mapping() {
    let t = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR", "FEATURE_BUNDLE_1"],
        "name": "test",
        "parameterDefinitions": [{"name": "Val", "type": "PATH"}],
        "jobEnvironments": [{
            "name": "E",
            "script": {
                "embeddedFiles": [{"name": "f", "type": "TEXT", "data": "{{apply_path_mapping(Param.Val)}}"}],
                "actions": {"onEnter": {"command": "bash"}}
            }
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#.to_string();
    decode_ok(&t);
}
