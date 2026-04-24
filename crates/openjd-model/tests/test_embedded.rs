// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python v2023_09/test_embedded.py
//!
//! Additional embedded file parsing tests not already covered in
//! test_environment_template.rs and test_actions_and_steps.rs.
//! Uses job templates for limit enforcement (filename length) since
//! the Rust crate enforces limits in the job template validation path.

use openjd_model::CallerLimits;
use openjd_model::{decode_environment_template, decode_job_template};

fn yaml_val(s: &str) -> serde_yaml::Value {
    serde_yaml::from_str(s).unwrap()
}

fn job_with_embedded(embedded_json: &str) -> serde_yaml::Value {
    yaml_val(&format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{
            "name": "S",
            "script": {{
                "embeddedFiles": [{embedded_json}],
                "actions": {{"onRun": {{"command": "foo", "args": ["{{{{Task.File.Foo}}}}"]}}}}
            }}
        }}]
    }}"#
    ))
}

fn env_with_embedded(embedded_json: &str) -> serde_yaml::Value {
    yaml_val(&format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "environment": {{"name": "Foo", "script": {{
            "embeddedFiles": [{embedded_json}],
            "actions": {{"onEnter": {{"command": "foo"}}}}
        }}}}
    }}"#
    ))
}

fn job_ok(embedded_json: &str) {
    decode_job_template(
        job_with_embedded(embedded_json),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
}

fn job_err(embedded_json: &str) {
    let err = decode_job_template(
        job_with_embedded(embedded_json),
        None,
        &CallerLimits::default(),
    )
    .expect_err(&format!("expected error for embedded: {embedded_json}"));
    let msg = err.to_string();
    assert!(
        msg.contains("embeddedFiles"),
        "Expected embeddedFiles error path, got: {msg}"
    );
}

fn env_ok(embedded_json: &str) {
    decode_environment_template(env_with_embedded(embedded_json), None).unwrap();
}

fn env_err(embedded_json: &str) {
    let err = decode_environment_template(env_with_embedded(embedded_json), None)
        .expect_err(&format!("expected error for embedded: {embedded_json}"));
    let msg = err.to_string();
    assert!(
        !msg.is_empty(),
        "Expected non-empty error message, got: {msg}"
    );
}

// ══════════════════════════════════════════════════════════════
// Success cases — via environment template
// ══════════════════════════════════════════════════════════════

#[test]
fn data_min_length() {
    env_ok(r#"{"name": "Foo", "type": "TEXT", "data": "1"}"#);
}

#[test]
fn data_long_length() {
    let data = "x".repeat(32 * 1024);
    env_ok(&format!(
        r#"{{"name": "Foo", "type": "TEXT", "data": "{data}"}}"#
    ));
}

#[test]
fn filename_min_length() {
    env_ok(r#"{"name": "Foo", "type": "TEXT", "data": "hello", "filename": "x"}"#);
}

#[test]
fn filename_max_length_env() {
    let name = "x".repeat(64);
    env_ok(&format!(
        r#"{{"name": "Foo", "type": "TEXT", "data": "hello", "filename": "{name}"}}"#
    ));
}

#[test]
fn runnable_true() {
    env_ok(r#"{"name": "Foo", "type": "TEXT", "data": "hello", "runnable": true}"#);
}

#[test]
fn runnable_false() {
    env_ok(r#"{"name": "Foo", "type": "TEXT", "data": "hello", "runnable": false}"#);
}

// ══════════════════════════════════════════════════════════════
// Failure cases — via environment template (serde/structural)
// ══════════════════════════════════════════════════════════════

#[test]
fn runnable_must_be_bool() {
    let v = env_with_embedded(
        r#"{"name": "Foo", "type": "TEXT", "data": "hello", "runnable": "True"}"#,
    );
    let err = decode_environment_template(v, None).expect_err("runnable must be bool");
    let msg = err.to_string();
    assert!(
        msg.contains("expected a boolean"),
        "Expected boolean type error, got: {msg}"
    );
}

#[test]
fn type_case_sensitive() {
    let v = env_with_embedded(r#"{"name": "Foo", "type": "text", "data": "hello"}"#);
    let err = decode_environment_template(v, None).expect_err("type is case-sensitive");
    let msg = err.to_string();
    assert!(
        msg.contains("unknown variant `text`, expected `TEXT`"),
        "Expected unknown variant error, got: {msg}"
    );
}

#[test]
fn data_empty() {
    env_err(r#"{"name": "Foo", "type": "TEXT", "data": ""}"#);
}

#[test]
fn filename_empty() {
    env_err(r#"{"name": "Foo", "type": "TEXT", "data": "hello", "filename": ""}"#);
}

#[test]
fn filename_with_forward_slash() {
    env_err(r#"{"name": "Foo", "type": "TEXT", "data": "hello", "filename": "dir/file.txt"}"#);
}

#[test]
fn filename_with_backslash() {
    env_err(r#"{"name": "Foo", "type": "TEXT", "data": "hello", "filename": "dir\\file.txt"}"#);
}

// ══════════════════════════════════════════════════════════════
// Filename length limits — via job template (where limits are enforced)
// ══════════════════════════════════════════════════════════════

#[test]
fn filename_max_length_job() {
    let name = "x".repeat(64);
    job_ok(&format!(
        r#"{{"name": "Foo", "type": "TEXT", "data": "hello", "filename": "{name}"}}"#
    ));
}

#[test]
fn filename_too_long_job() {
    let name = "x".repeat(65);
    job_err(&format!(
        r#"{{"name": "Foo", "type": "TEXT", "data": "hello", "filename": "{name}"}}"#
    ));
}
