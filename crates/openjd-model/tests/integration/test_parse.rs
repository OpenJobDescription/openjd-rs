// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test/openjd/model/test_parse.py
//!
//! Gold standard: failure tests assert the full error message including path.

use openjd_model::template::parse::{document_string_to_object, DocumentType};
use openjd_model::CallerLimits;
use openjd_model::{decode_environment_template, decode_job_template};

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn check_parse_err(doc: &str, doc_type: DocumentType, expected: &[&str]) {
    let err = document_string_to_object(doc, doc_type, &CallerLimits::default())
        .expect_err(&format!("Expected error for: {doc}"));
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

fn check_job_err_with_ext(s: &str, supported: &[&str], expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_job_template(v, Some(supported), &CallerLimits::default())
        .expect_err(&format!("Expected error for: {s}"));
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
    let err = decode_environment_template(v, None, &CallerLimits::default())
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
// document_string_to_object — success
// ══════════════════════════════════════════════════════════════

#[test]
fn doc_string_to_object_json_success() {
    let result = document_string_to_object(
        r#"{"key": "value"}"#,
        DocumentType::Json,
        &CallerLimits::default(),
    )
    .unwrap();
    assert_eq!(result["key"].as_str().unwrap(), "value");
}

#[test]
fn doc_string_to_object_yaml_success() {
    let result =
        document_string_to_object("key: value\n", DocumentType::Yaml, &CallerLimits::default())
            .unwrap();
    assert_eq!(result["key"].as_str().unwrap(), "value");
}

#[test]
fn yaml_non_finite_floats_remain_available_for_model_validation() {
    let result = document_string_to_object(
        "nan: .nan\npositive_inf: .inf\nnegative_inf: -.inf\n",
        DocumentType::Yaml,
        &CallerLimits::default(),
    )
    .unwrap();

    assert_eq!(result["nan"].as_str(), Some(".nan"));
    assert_eq!(result["positive_inf"].as_str(), Some(".inf"));
    assert_eq!(result["negative_inf"].as_str(), Some("-.inf"));
}

/// YAML 1.2 Core schema resolves only the lowercase, capitalized and
/// uppercase spellings of `true`, `false`, `null` and the non-finite floats.
/// Any other casing is a plain string, which is also how the Python
/// implementation reads them.
#[test]
fn yaml_core_schema_scalar_spellings() {
    let result = document_string_to_object(
        "t1: true\nt2: True\nt3: TRUE\nf1: false\nf2: False\nf3: FALSE\n\
         n1: null\nn2: Null\nn3: NULL\nn4: ~\n\
         s1: tRUE\ns2: FAlse\ns3: NUlL\ns4: .NAn\ns5: yes\n",
        DocumentType::Yaml,
        &CallerLimits::default(),
    )
    .unwrap();

    for key in ["t1", "t2", "t3"] {
        assert_eq!(result[key], serde_json::Value::Bool(true), "{key}");
    }
    for key in ["f1", "f2", "f3"] {
        assert_eq!(result[key], serde_json::Value::Bool(false), "{key}");
    }
    for key in ["n1", "n2", "n3", "n4"] {
        assert_eq!(result[key], serde_json::Value::Null, "{key}");
    }
    for (key, text) in [
        ("s1", "tRUE"),
        ("s2", "FAlse"),
        ("s3", "NUlL"),
        ("s4", ".NAn"),
        ("s5", "yes"),
    ] {
        assert_eq!(result[key].as_str(), Some(text), "{key}");
    }
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

/// An explicit `!!bool` or `!!float` tag only accepts that type's YAML 1.2
/// Core spellings, so a wrong-case value under one is a parse error rather
/// than a string. The Python implementation accepts these.
#[test]
fn yaml_core_tag_with_wrong_case_value_is_a_parse_error() {
    for (doc, expected) in [
        ("a: !!bool True\n", Ok(serde_json::json!({"a": true}))),
        (
            "a: !!bool tRUE\n",
            Err(
                "Validation error: The document is not a valid YAML document consisting of \
                 key-value pairs. error: line 1 column 11: invalid boolean (strict mode expects \
                 true/false)\n --> <input>:1:11\n  |\n1 | a: !!bool tRUE\n  |           ^ \
                 invalid boolean (strict mode expects true/false)",
            ),
        ),
        (
            "a: !!float .NAn\n",
            Err(
                "Validation error: The document is not a valid YAML document consisting of \
                 key-value pairs. error: line 1 column 12: invalid floating point\n --> \
                 <input>:1:12\n  |\n1 | a: !!float .NAn\n  |            ^ invalid floating \
                 point",
            ),
        ),
    ] {
        let result = document_string_to_object(doc, DocumentType::Yaml, &CallerLimits::default())
            .map_err(|e| e.to_string());
        assert_eq!(result, expected.map_err(str::to_string), "{doc:?}");
    }
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
    decode_job_template(v, None, &CallerLimits::default()).unwrap();
}

// ══════════════════════════════════════════════════════════════
// decode_job_template — YAML Core schema scalar spellings
// ══════════════════════════════════════════════════════════════

/// A wrong-case boolean spelling is a string, so a BOOL-typed field rejects it.
#[test]
fn yaml_wrong_case_boolean_is_rejected_for_bool_field() {
    let doc = "specificationVersion: jobtemplate-2023-09\n\
               name: J\n\
               steps:\n\
               - name: S\n  \
                 script:\n    \
                   embeddedFiles:\n    \
                   - name: f\n      \
                     type: TEXT\n      \
                     runnable: tRUE\n      \
                     data: x\n    \
                   actions:\n      \
                     onRun:\n        \
                       command: echo\n";
    let v = document_string_to_object(doc, DocumentType::Yaml, &CallerLimits::default()).unwrap();
    let err = decode_job_template(v, None, &CallerLimits::default()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Validation error: 'jobtemplate-2023-09' failed checks: \
         invalid type: string \"tRUE\", expected a boolean"
    );
}

/// A wrong-case boolean spelling is a string, so a STRING parameter accepts it.
#[test]
fn yaml_wrong_case_boolean_is_accepted_as_string_default() {
    let doc = "specificationVersion: jobtemplate-2023-09\n\
               name: J\n\
               parameterDefinitions:\n\
               - name: P\n  \
                 type: STRING\n  \
                 default: FAlse\n\
               steps:\n\
               - name: S\n  \
                 script:\n    \
                   actions:\n      \
                     onRun:\n        \
                       command: echo\n";
    let v = document_string_to_object(doc, DocumentType::Yaml, &CallerLimits::default()).unwrap();
    let template = decode_job_template(v, None, &CallerLimits::default()).unwrap();
    match &template.parameter_definitions.as_deref().unwrap()[0] {
        openjd_model::template::JobParameterDefinition::STRING(p) => {
            assert_eq!(p.default.as_deref(), Some("FAlse"));
        }
        other => panic!("expected a STRING parameter, got {other:?}"),
    }
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
    decode_environment_template(v, None, &CallerLimits::default()).unwrap();
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
    let result = decode_job_template(v, None, &CallerLimits::default());
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
        &[
            "1 validation error for JobTemplate\n",
            "extensions:\n\tUnsupported extension names: FEATURE_BUNDLE_1",
        ],
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
        &[
            "1 validation error for JobTemplate\n",
            "extensions:\n\tUnsupported extension names: FEATURE_BUNDLE_1",
        ],
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
        &[
            "1 validation error for JobTemplate\n",
            "extensions:\n\tUnsupported extension names: FEATURE_BUNDLE_1",
        ],
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
    assert!(decode_job_template(v, None, &CallerLimits::default()).is_err());
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
    decode_job_template(v, Some(&["FEATURE_BUNDLE_1"]), &CallerLimits::default()).unwrap();
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
    decode_job_template(v, Some(&["UNSUPPORTED_NAME"]), &CallerLimits::default()).unwrap();
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
        decode_job_template(v, None, &CallerLimits::default()).is_err(),
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
        decode_environment_template(v2, None, &CallerLimits::default()).is_err(),
        "Env template with empty extensions should be rejected"
    );
}
