// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python v2023_09/test_redacted_env_vars.py
//!
//! Tests the REDACTED_ENV_VARS extension handling.
//! The Rust crate doesn't implement REDACTED_ENV_VARS yet, so these tests
//! verify that the extension is properly rejected when not supported and
//! accepted when listed as supported.

use openjd_model::decode_job_template;

fn yaml_val(s: &str) -> serde_yaml::Value {
    serde_yaml::from_str(s).unwrap()
}

fn redacted_env_vars_template() -> serde_yaml::Value {
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

#[test]
fn redacted_env_vars_extension_supported() {
    // When REDACTED_ENV_VARS is in the supported list, parsing succeeds
    let result = decode_job_template(redacted_env_vars_template(), Some(&["REDACTED_ENV_VARS"]));
    assert!(result.is_ok(), "expected success, got: {:?}", result.err());
}

#[test]
fn redacted_env_vars_extension_not_supported_default() {
    // By default (None), no extensions are supported → should fail
    let err = decode_job_template(redacted_env_vars_template(), None).unwrap_err();
    assert!(err.to_string().contains("REDACTED_ENV_VARS"), "got: {err}");
}

#[test]
fn redacted_env_vars_extension_not_supported_empty_list() {
    // Empty supported list → should fail
    let err = decode_job_template(redacted_env_vars_template(), Some(&[])).unwrap_err();
    assert!(err.to_string().contains("REDACTED_ENV_VARS"), "got: {err}");
}

#[test]
fn redacted_env_vars_extension_not_supported_wrong_list() {
    // Only EXPR supported, not REDACTED_ENV_VARS → should fail
    let err = decode_job_template(redacted_env_vars_template(), Some(&["EXPR"])).unwrap_err();
    assert!(err.to_string().contains("REDACTED_ENV_VARS"), "got: {err}");
}
