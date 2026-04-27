// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python v2023_09/test_redacted_env_vars.py and test_parse.py.
//!
//! Tests the REDACTED_ENV_VARS extension handling for both job templates and
//! environment templates.

use openjd_model::CallerLimits;
use openjd_model::{decode_environment_template, decode_job_template};

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn redacted_env_vars_template() -> serde_json::Value {
    yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["REDACTED_ENV_VARS"],
        "name": "Test Job",
        "steps": [{
            "name": "step1",
            "script": {
                "actions": {"onRun": {"command": "python", "args": ["{{Task.File.Run}}"]}},
                "embeddedFiles": [{
                    "name": "Run",
                    "type": "TEXT",
                    "data": "print(\"openjd_redacted_env: SECRETVAR=SECRETVAL\")"
                }]
            }
        }]
    }"#,
    )
}

// ══════════════════════════════════════════════════════════════
// Job template — REDACTED_ENV_VARS
// ══════════════════════════════════════════════════════════════

#[test]
fn redacted_env_vars_extension_supported() {
    // When REDACTED_ENV_VARS is in the supported list, parsing succeeds
    let result = decode_job_template(
        redacted_env_vars_template(),
        Some(&["REDACTED_ENV_VARS"]),
        &CallerLimits::default(),
    );
    assert!(result.is_ok(), "expected success, got: {:?}", result.err());
}

#[test]
fn redacted_env_vars_extension_not_supported_default() {
    // By default (None), no extensions are supported → should fail
    let err = decode_job_template(redacted_env_vars_template(), None, &CallerLimits::default())
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Unknown or unsupported extension: REDACTED_ENV_VARS"),
        "Expected 'Unknown or unsupported extension: REDACTED_ENV_VARS', got: {msg}"
    );
}

#[test]
fn redacted_env_vars_extension_not_supported_empty_list() {
    // Empty supported list → should fail
    let err = decode_job_template(
        redacted_env_vars_template(),
        Some(&[]),
        &CallerLimits::default(),
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Unknown or unsupported extension: REDACTED_ENV_VARS"),
        "Expected 'Unknown or unsupported extension: REDACTED_ENV_VARS', got: {msg}"
    );
}

#[test]
fn redacted_env_vars_extension_not_supported_wrong_list() {
    // Only EXPR supported, not REDACTED_ENV_VARS → should fail
    let err = decode_job_template(
        redacted_env_vars_template(),
        Some(&["EXPR"]),
        &CallerLimits::default(),
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Unknown or unsupported extension: REDACTED_ENV_VARS"),
        "Expected 'Unknown or unsupported extension: REDACTED_ENV_VARS', got: {msg}"
    );
}

// ══════════════════════════════════════════════════════════════
// Job template — REDACTED_ENV_VARS with other extensions
// Ported from Python test_template_extensions_list
// ══════════════════════════════════════════════════════════════

#[test]
fn redacted_env_vars_with_other_extensions() {
    // REDACTED_ENV_VARS alongside FEATURE_BUNDLE_1 — both supported
    let template = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["REDACTED_ENV_VARS", "FEATURE_BUNDLE_1"],
        "name": "Test Job",
        "steps": [{"name": "step1", "script": {"actions": {"onRun": {"command": "echo"}}}}]
    }"#,
    );
    let result = decode_job_template(
        template,
        Some(&["REDACTED_ENV_VARS", "FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    );
    assert!(result.is_ok(), "expected success, got: {:?}", result.err());
}

#[test]
fn redacted_env_vars_only_one_of_two_supported() {
    // Template requests REDACTED_ENV_VARS + FEATURE_BUNDLE_1, but only FB1 is supported
    let template = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["REDACTED_ENV_VARS", "FEATURE_BUNDLE_1"],
        "name": "Test Job",
        "steps": [{"name": "step1", "script": {"actions": {"onRun": {"command": "echo"}}}}]
    }"#,
    );
    let err = decode_job_template(
        template,
        Some(&["FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Unknown or unsupported extension: REDACTED_ENV_VARS"),
        "Expected error about REDACTED_ENV_VARS, got: {msg}"
    );
}

// ══════════════════════════════════════════════════════════════
// Environment template — REDACTED_ENV_VARS
// Ported from Python test_template_extensions_list (parametrized for env templates)
// ══════════════════════════════════════════════════════════════

fn redacted_env_vars_env_template() -> serde_json::Value {
    yaml_val(
        r#"{
        "specificationVersion": "environment-2023-09",
        "extensions": ["REDACTED_ENV_VARS"],
        "environment": {
            "name": "TestEnv",
            "script": {"actions": {"onEnter": {"command": "echo"}}}
        }
    }"#,
    )
}

#[test]
fn redacted_env_vars_env_template_supported() {
    let result = decode_environment_template(
        redacted_env_vars_env_template(),
        Some(&["REDACTED_ENV_VARS"]),
    );
    assert!(result.is_ok(), "expected success, got: {:?}", result.err());
}

#[test]
fn redacted_env_vars_env_template_not_supported() {
    let err = decode_environment_template(redacted_env_vars_env_template(), None).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Unknown or unsupported extension: REDACTED_ENV_VARS"),
        "Expected 'Unknown or unsupported extension: REDACTED_ENV_VARS', got: {msg}"
    );
}

#[test]
fn redacted_env_vars_env_template_wrong_list() {
    let err = decode_environment_template(
        redacted_env_vars_env_template(),
        Some(&["FEATURE_BUNDLE_1"]),
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Unknown or unsupported extension: REDACTED_ENV_VARS"),
        "Expected error about REDACTED_ENV_VARS, got: {msg}"
    );
}
