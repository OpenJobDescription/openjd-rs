// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test/openjd/model/test_parse.py
//!
//! Gold standard: failure tests assert the full error message including path.

use openjd_model::parse::{document_string_to_object, DocumentType};
use openjd_model::{decode_environment_template, decode_job_template};

fn yaml_val(s: &str) -> serde_yaml::Value {
    serde_yaml::from_str(s).unwrap()
}

fn check_parse_err(doc: &str, doc_type: DocumentType, expected: &[&str]) {
    let err =
        document_string_to_object(doc, doc_type).expect_err(&format!("Expected error for: {doc}"));
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

fn check_job_err(s: &str, expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_job_template(v, None).expect_err(&format!("Expected error for: {s}"));
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

fn check_job_err_with_ext(s: &str, supported: &[&str], expected: &[&str]) {
    let v = yaml_val(s);
    let err =
        decode_job_template(v, Some(supported)).expect_err(&format!("Expected error for: {s}"));
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
    let err = decode_environment_template(v, None).expect_err(&format!("Expected error for: {s}"));
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

// ══════════════════════════════════════════════════════════════
// document_string_to_object — success
// ══════════════════════════════════════════════════════════════

#[test]
fn doc_string_to_object_json_success() {
    let result = document_string_to_object(r#"{"key": "value"}"#, DocumentType::Json).unwrap();
    assert_eq!(result["key"].as_str().unwrap(), "value");
}

#[test]
fn doc_string_to_object_yaml_success() {
    let result = document_string_to_object("key: value\n", DocumentType::Yaml).unwrap();
    assert_eq!(result["key"].as_str().unwrap(), "value");
}

// ══════════════════════════════════════════════════════════════
// document_string_to_object — not a dict
// ══════════════════════════════════════════════════════════════

#[test]
fn not_a_dict_json() {
    check_parse_err(
        "[1, 2, 3]",
        DocumentType::Json,
        &["not a valid Json document consisting of key-value pairs"],
    );
}

#[test]
fn not_a_dict_yaml() {
    check_parse_err(
        "- 1\n- 2\n- 3\n",
        DocumentType::Yaml,
        &["not a valid Yaml document consisting of key-value pairs"],
    );
}

// ══════════════════════════════════════════════════════════════
// document_string_to_object — bad parse
// ══════════════════════════════════════════════════════════════

#[test]
fn bad_parse_json() {
    check_parse_err(
        "{",
        DocumentType::Json,
        &["not a valid JSON document consisting of key-value pairs"],
    );
}

#[test]
fn bad_parse_yaml() {
    check_parse_err(
        "-",
        DocumentType::Yaml,
        &["not a valid Yaml document consisting of key-value pairs"],
    );
}

// ══════════════════════════════════════════════════════════════
// decode_job_template — version errors
// ══════════════════════════════════════════════════════════════

#[test]
fn job_missing_specification_version() {
    check_job_err(
        r#"{"notspecversion": "badvalue"}"#,
        &["Template is missing Open Job Description schema version key: specificationVersion"],
    );
}

#[test]
fn job_unknown_version() {
    check_job_err(
        r#"{"specificationVersion": "badvalue"}"#,
        &[
            "Unknown template version: badvalue",
            "Values allowed for 'specificationVersion' in Job Templates are: jobtemplate-2023-09",
        ],
    );
}

#[test]
fn job_not_a_job_template_version() {
    check_job_err(
        r#"{"specificationVersion": "environment-2023-09"}"#,
        &[
            "Specification version 'environment-2023-09' is not a Job Template version",
            "Values allowed for 'specificationVersion' in Job Templates are: jobtemplate-2023-09",
        ],
    );
}

// ══════════════════════════════════════════════════════════════
// decode_job_template — success
// ══════════════════════════════════════════════════════════════

#[test]
fn job_decode_success() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "name",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "do thing"}}}}]
    }"#,
    );
    decode_job_template(v, None).unwrap();
}

// ══════════════════════════════════════════════════════════════
// decode_environment_template — version errors
// ══════════════════════════════════════════════════════════════

#[test]
fn env_missing_specification_version() {
    check_env_err(
        r#"{"notspecversion": "badvalue"}"#,
        &["Template is missing Open Job Description schema version key: specificationVersion"],
    );
}

#[test]
fn env_unknown_version() {
    check_env_err(
        r#"{"specificationVersion": "badvalue"}"#,
        &["Unknown template version: badvalue"],
    );
}

#[test]
fn env_not_an_environment_template_version() {
    check_env_err(
        r#"{"specificationVersion": "jobtemplate-2023-09"}"#,
        &["Specification version 'jobtemplate-2023-09' is not an Environment Template version"],
    );
}

// ══════════════════════════════════════════════════════════════
// decode_environment_template — success
// ══════════════════════════════════════════════════════════════

#[test]
fn env_decode_success() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {
            "name": "FooEnv",
            "description": "A description",
            "script": {"actions": {"onEnter": {"command": "echo", "args": ["Hello", "World"]}}}
        }
    }"#,
    );
    decode_environment_template(v, None).unwrap();
}

// ══════════════════════════════════════════════════════════════
// Extension list validation — job template
// ══════════════════════════════════════════════════════════════

#[test]
fn job_extensions_empty_list() {
    // Empty extensions list is rejected early in parsing.
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": [],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
    }"#,
    );
    let result = decode_job_template(v, None);
    assert!(result.is_err(), "empty extensions list should be rejected");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("non-empty"),
        "error should mention non-empty, got: {msg}"
    );
}

#[test]
fn job_extensions_unsupported_no_supported_list() {
    // By default (None) no extensions are supported
    check_job_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["FEATURE_BUNDLE_1"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
    }"#,
        &["Unknown or unsupported extension: FEATURE_BUNDLE_1"],
    );
}

#[test]
fn job_extensions_unsupported_wrong_supported_list() {
    // Template requests FEATURE_BUNDLE_1 but only EXPR is supported
    check_job_err_with_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["FEATURE_BUNDLE_1"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
    }"#,
        &["EXPR"],
        &["Unknown or unsupported extension: FEATURE_BUNDLE_1"],
    );
}

#[test]
fn job_extensions_unsupported_empty_supported_list() {
    check_job_err_with_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["FEATURE_BUNDLE_1"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
    }"#,
        &[],
        &["Unknown or unsupported extension: FEATURE_BUNDLE_1"],
    );
}

#[test]
fn job_extensions_invalid_name_format() {
    // Extension names must match [A-Z_0-9]{3,128} — lowercase fails at serde level
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["bad_name"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
    }"#,
    );
    assert!(decode_job_template(v, None).is_err());
}

#[test]
fn job_extensions_supported_succeeds() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["FEATURE_BUNDLE_1"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
    }"#,
    );
    decode_job_template(v, Some(&["FEATURE_BUNDLE_1"])).unwrap();
}

#[test]
fn job_no_extensions_with_unsupported_in_supported_list() {
    // If template doesn't request extensions, passing unsupported names is fine
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
    }"#,
    );
    decode_job_template(v, Some(&["UNSUPPORTED_NAME"])).unwrap();
}

// ══════════════════════════════════════════════════════════════
// Empty extensions list — both template types reject consistently
// (caught early in parse.rs pass 4 for both)
// ══════════════════════════════════════════════════════════════

#[test]
fn empty_extensions_job_template() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": [],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#,
    );
    assert!(
        decode_job_template(v, None).is_err(),
        "Job template with empty extensions should be rejected"
    );

    let v2 = yaml_val(
        r#"{
        "specificationVersion": "environment-2023-09",
        "extensions": [],
        "environment": {
            "name": "E",
            "script": {"actions": {"onEnter": {"command": "echo"}}}
        }
    }"#,
    );
    assert!(
        decode_environment_template(v2, None).is_err(),
        "Env template with empty extensions should be rejected"
    );
}
