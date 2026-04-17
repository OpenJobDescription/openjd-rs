// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test/openjd/model/v2023_09/test_job_parameters.py
//!
//! Gold standard: failure tests assert the full error message including path.

use openjd_expr::path_mapping::PathFormat;
use openjd_model::decode_job_template;

fn yaml_val(s: &str) -> serde_yaml::Value {
    serde_yaml::from_str(s).unwrap()
}

fn job_with_param(param_json: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{param_json}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}]
    }}"#
    )
}

fn decode_ok(s: &str) {
    let v = yaml_val(s);
    decode_job_template(v, None).expect("Expected success");
}

fn check_err(s: &str, expected: &[&str]) {
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

// ══════════════════════════════════════════════════════════════
// STRING parameter — success cases
// ══════════════════════════════════════════════════════════════

#[test]
fn test_string_param_minimal() {
    decode_ok(&job_with_param(r#"{"name": "Foo", "type": "STRING"}"#));
}

#[test]
fn test_string_param_with_default() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "STRING", "default": "some value"}"#,
    ));
}

#[test]
fn test_string_param_with_min_max_length() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "STRING", "minLength": 1, "maxLength": 10}"#,
    ));
}

#[test]
fn test_string_param_min_eq_max() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "STRING", "minLength": 1, "maxLength": 1}"#,
    ));
}

#[test]
fn test_string_param_with_allowed_values() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "STRING", "allowedValues": ["a", "b"]}"#,
    ));
}

#[test]
fn test_string_param_default_in_allowed() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "STRING", "default": "a", "allowedValues": ["a", "b"]}"#,
    ));
}

#[test]
fn test_string_param_default_is_min_length() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "STRING", "default": "aa", "minLength": 2}"#,
    ));
}

#[test]
fn test_string_param_default_is_max_length() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "STRING", "default": "aa", "maxLength": 2}"#,
    ));
}

#[test]
fn test_string_param_allowed_is_min_length() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "STRING", "allowedValues": ["aa"], "minLength": 2}"#,
    ));
}

#[test]
fn test_string_param_allowed_is_max_length() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "STRING", "allowedValues": ["aa"], "maxLength": 2}"#,
    ));
}

#[test]
fn test_string_param_minlength_zero() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "STRING", "minLength": 0}"#,
    ));
}

// ══════════════════════════════════════════════════════════════
// STRING parameter — failure cases
// ══════════════════════════════════════════════════════════════

#[test]
fn test_string_param_empty_allowed_values() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "STRING", "allowedValues": []}"#),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': allowedValues must not be empty."],
    );
}

#[test]
fn test_string_param_min_greater_than_max() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "STRING", "minLength": 10, "maxLength": 1}"#),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': minLength (10) > maxLength (1)."],
    );
}

#[test]
fn test_string_param_default_not_in_allowed() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "STRING", "default": "c", "allowedValues": ["a", "b"]}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': default 'c' is not in allowedValues."],
    );
}

#[test]
fn test_string_param_default_too_short() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "STRING", "default": "a", "minLength": 5}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': default length 1 is less than minLength 5.",
    ]);
}

#[test]
fn test_string_param_default_too_long() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "STRING", "default": "abcdef", "maxLength": 3}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': default length 6 exceeds maxLength 3."],
    );
}

#[test]
fn test_string_param_maxlength_zero() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "STRING", "maxLength": 0}"#),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': maxLength must be > 0."],
    );
}

#[test]
fn test_string_param_negative_minlength() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "STRING", "minLength": -1}"#),
        &["invalid value: integer `-1`, expected usize"],
    );
}

#[test]
fn test_string_param_allowed_less_than_min_length() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "STRING", "allowedValues": ["aa"], "minLength": 3}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': allowedValues[0] length 2 is less than minLength 3.",
    ]);
}

#[test]
fn test_string_param_allowed_exceeds_max_length() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "STRING", "allowedValues": ["aa"], "maxLength": 1}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': allowedValues[0] length 2 exceeds maxLength 1.",
    ]);
}

// ══════════════════════════════════════════════════════════════
// INT parameter — success cases
// ══════════════════════════════════════════════════════════════

#[test]
fn test_int_param_minimal() {
    decode_ok(&job_with_param(r#"{"name": "Foo", "type": "INT"}"#));
}

#[test]
fn test_int_param_with_default() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "INT", "default": "5"}"#,
    ));
}

#[test]
fn test_int_param_with_default_int() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "INT", "default": 5}"#,
    ));
}

#[test]
fn test_int_param_with_min_max() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "INT", "minValue": 0, "maxValue": 100}"#,
    ));
}

#[test]
fn test_int_param_min_eq_max() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "INT", "minValue": 1, "maxValue": 1}"#,
    ));
}

#[test]
fn test_int_param_with_allowed_values() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "INT", "allowedValues": [1, 2, 3]}"#,
    ));
}

#[test]
fn test_int_param_default_is_min_value() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "INT", "default": 2, "minValue": 2}"#,
    ));
}

#[test]
fn test_int_param_default_is_max_value() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "INT", "default": 2, "maxValue": 2}"#,
    ));
}

#[test]
fn test_int_param_default_in_allowed() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "INT", "default": 2, "allowedValues": [1, "2"]}"#,
    ));
}

// ══════════════════════════════════════════════════════════════
// INT parameter — failure cases
// ══════════════════════════════════════════════════════════════

#[test]
fn test_int_param_empty_allowed_values() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "INT", "allowedValues": []}"#),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': allowedValues must not be empty."],
    );
}

#[test]
fn test_int_param_default_not_int() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "INT", "default": "nine"}"#),
        &["Cannot parse 'nine' as integer"],
    );
}

#[test]
fn test_int_param_min_greater_than_max() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "INT", "minValue": 100, "maxValue": 1}"#),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': minValue (100) > maxValue (1)."],
    );
}

#[test]
fn test_int_param_default_below_min() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "INT", "default": "0", "minValue": 5}"#),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': value 0 is less than minimum 5"],
    );
}

#[test]
fn test_int_param_default_above_max() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "INT", "default": "100", "maxValue": 50}"#),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': value 100 exceeds maximum 50"],
    );
}

#[test]
fn test_int_param_default_not_in_allowed() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "INT", "default": "5", "allowedValues": [1, 2, 3]}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': value 5 is not in allowed values"],
    );
}

// ══════════════════════════════════════════════════════════════
// FLOAT parameter — success cases
// ══════════════════════════════════════════════════════════════

#[test]
fn test_float_param_minimal() {
    decode_ok(&job_with_param(r#"{"name": "Foo", "type": "FLOAT"}"#));
}

#[test]
fn test_float_param_with_default() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "FLOAT", "default": "1.5"}"#,
    ));
}

#[test]
fn test_float_param_with_default_float() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "FLOAT", "default": 1.5}"#,
    ));
}

#[test]
fn test_float_param_with_min_max() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "FLOAT", "minValue": 0.0, "maxValue": 100.0}"#,
    ));
}

#[test]
fn test_float_param_min_eq_max() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "FLOAT", "minValue": 1, "maxValue": 1}"#,
    ));
}

#[test]
fn test_float_param_with_allowed_values() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "FLOAT", "allowedValues": [1.2]}"#,
    ));
}

#[test]
fn test_float_param_default_is_min_value() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "FLOAT", "default": 2, "minValue": 2}"#,
    ));
}

#[test]
fn test_float_param_default_is_max_value() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "FLOAT", "default": 2, "maxValue": 2}"#,
    ));
}

#[test]
fn test_float_param_default_in_allowed() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "FLOAT", "default": 2, "allowedValues": [1, "2"]}"#,
    ));
}

// ══════════════════════════════════════════════════════════════
// FLOAT parameter — failure cases
// ══════════════════════════════════════════════════════════════

#[test]
fn test_float_param_empty_allowed_values() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "FLOAT", "allowedValues": []}"#),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': allowedValues must not be empty."],
    );
}

#[test]
fn test_float_param_default_not_float() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "FLOAT", "default": "abc"}"#),
        &["Cannot parse 'abc' as float"],
    );
}

#[test]
fn test_float_param_min_greater_than_max() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "FLOAT", "minValue": 100.0, "maxValue": 1.0}"#),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': minValue (100) > maxValue (1)."],
    );
}

#[test]
fn test_float_param_default_below_min() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "FLOAT", "default": "0", "minValue": 5.0}"#),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': value 0 is less than minimum 5"],
    );
}

#[test]
fn test_float_param_default_above_max() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "FLOAT", "default": "100", "maxValue": 50.0}"#),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': value 100 exceeds maximum 50"],
    );
}

#[test]
fn test_float_param_default_not_in_allowed() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "FLOAT", "default": "5", "allowedValues": [1, 2, 3]}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': value 5 is not in allowed values"],
    );
}

#[test]
fn test_float_param_allowed_below_min() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "FLOAT", "allowedValues": [2], "minValue": 3}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': allowedValues[0] (2) is less than minValue (3).",
    ]);
}

#[test]
fn test_float_param_allowed_above_max() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "FLOAT", "allowedValues": [2], "maxValue": 1}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': allowedValues[0] (2) exceeds maxValue (1).",
    ]);
}

// ══════════════════════════════════════════════════════════════
// PATH parameter — success cases
// ══════════════════════════════════════════════════════════════

#[test]
fn test_path_param_minimal() {
    decode_ok(&job_with_param(r#"{"name": "Foo", "type": "PATH"}"#));
}

#[test]
fn test_path_param_with_default() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "PATH", "default": "/tmp/foo"}"#,
    ));
}

#[test]
fn test_path_param_with_object_type_file() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "PATH", "objectType": "FILE"}"#,
    ));
}

#[test]
fn test_path_param_with_object_type_directory() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "PATH", "objectType": "DIRECTORY"}"#,
    ));
}

#[test]
fn test_path_param_with_data_flow_in() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "PATH", "dataFlow": "IN"}"#,
    ));
}

#[test]
fn test_path_param_with_data_flow_out() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "PATH", "dataFlow": "OUT"}"#,
    ));
}

#[test]
fn test_path_param_with_data_flow_inout() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "PATH", "dataFlow": "INOUT"}"#,
    ));
}

#[test]
fn test_path_param_with_data_flow_none() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "PATH", "dataFlow": "NONE"}"#,
    ));
}

#[test]
fn test_path_param_with_allowed_values() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "PATH", "allowedValues": ["/a"]}"#,
    ));
}

#[test]
fn test_path_param_default_in_allowed() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "PATH", "default": "aa", "allowedValues": ["aa", "bb"]}"#,
    ));
}

#[test]
fn test_path_param_default_is_min_length() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "PATH", "default": "aa", "minLength": 2}"#,
    ));
}

#[test]
fn test_path_param_default_is_max_length() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "PATH", "default": "aa", "maxLength": 2}"#,
    ));
}

// ══════════════════════════════════════════════════════════════
// PATH parameter — failure cases
// ══════════════════════════════════════════════════════════════

#[test]
fn test_path_param_invalid_object_type() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "PATH", "objectType": "UNSUPPORTED"}"#),
        &["unknown variant `UNSUPPORTED`, expected `FILE` or `DIRECTORY`"],
    );
}

#[test]
fn test_path_param_invalid_data_flow() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "PATH", "dataFlow": "UNSUPPORTED"}"#),
        &["unknown variant `UNSUPPORTED`, expected one of `NONE`, `IN`, `OUT`, `INOUT`"],
    );
}

#[test]
fn test_path_param_empty_allowed_values() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "PATH", "allowedValues": []}"#),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': allowedValues must not be empty."],
    );
}

#[test]
fn test_path_param_min_greater_than_max() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "PATH", "minLength": 10, "maxLength": 1}"#),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': minLength (10) > maxLength (1)."],
    );
}

#[test]
fn test_path_param_default_not_in_allowed() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "PATH", "default": "c", "allowedValues": ["a", "b"]}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': default 'c' is not in allowedValues."],
    );
}

#[test]
fn test_path_param_default_too_short() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "PATH", "default": "a", "minLength": 5}"#),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': default length 1 < minLength 5."],
    );
}

#[test]
fn test_path_param_default_too_long() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "PATH", "default": "abcdef", "maxLength": 3}"#),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': default length 6 > maxLength 3."],
    );
}

#[test]
fn test_path_param_allowed_less_than_min_length() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "PATH", "allowedValues": ["aa"], "minLength": 3}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': allowedValues[0] length 2 < minLength 3."],
    );
}

#[test]
fn test_path_param_allowed_exceeds_max_length() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "PATH", "allowedValues": ["aa"], "maxLength": 1}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': allowedValues[0] length 2 > maxLength 1."],
    );
}

// ══════════════════════════════════════════════════════════════
// Parameter name validation
// ══════════════════════════════════════════════════════════════

#[test]
fn test_param_name_empty() {
    check_err(
        &job_with_param(r#"{"name": "", "type": "STRING"}"#),
        &["Identifier length must be 1..=512, got 0"],
    );
}

#[test]
fn test_param_name_too_long() {
    let long_name = "A".repeat(65);
    check_err(
        &job_with_param(&format!(r#"{{"name": "{long_name}", "type": "STRING"}}"#)),
        &["parameterDefinitions[0]:\n\tname exceeds 64 characters."],
    );
}

#[test]
fn test_param_name_with_spaces() {
    check_err(
        &job_with_param(r#"{"name": "Foo Bar", "type": "STRING"}"#),
        &["Identifier 'Foo Bar' does not match pattern"],
    );
}

// ══════════════════════════════════════════════════════════════
// Missing / unknown type discriminator
// ══════════════════════════════════════════════════════════════

#[test]
fn test_missing_type() {
    check_err(
        &job_with_param(r#"{"name": "Foo"}"#),
        &["missing 'type' field in parameter definition"],
    );
}

#[test]
fn test_unknown_type() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "UNKNOWN"}"#),
        &["unknown parameter type: 'UNKNOWN'"],
    );
}

// ══════════════════════════════════════════════════════════════
// STRING parameter — userInterface tests
// ══════════════════════════════════════════════════════════════

#[test]
fn test_string_param_ui_line_edit() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "STRING", "userInterface": {"control": "LINE_EDIT"}}"#,
    ));
}

#[test]
fn test_string_param_ui_hidden() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "STRING", "userInterface": {"control": "HIDDEN"}}"#,
    ));
}

#[test]
fn test_string_param_ui_dropdown_list() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "STRING", "userInterface": {"control": "DROPDOWN_LIST"}, "allowedValues": ["a"]}"#,
    ));
}

#[test]
fn test_string_param_description() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "STRING", "description": "some test"}"#,
    ));
}

#[test]
fn test_string_param_description_with_newlines() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "STRING", "description": "aa\nbb\ncc"}"#,
    ));
}

// ══════════════════════════════════════════════════════════════
// INT parameter — userInterface tests
// ══════════════════════════════════════════════════════════════

#[test]
fn test_int_param_ui_spin_box() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "INT", "userInterface": {"control": "SPIN_BOX"}}"#,
    ));
}

#[test]
fn test_int_param_ui_hidden() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "INT", "userInterface": {"control": "HIDDEN"}}"#,
    ));
}

#[test]
fn test_int_param_ui_dropdown_list() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "INT", "userInterface": {"control": "DROPDOWN_LIST"}, "allowedValues": [1]}"#,
    ));
}

#[test]
fn test_int_param_description() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "INT", "description": "some text"}"#,
    ));
}

#[test]
fn test_int_param_negative_default() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "INT", "default": -1}"#,
    ));
}

#[test]
fn test_int_param_min_value_as_string() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "INT", "minValue": "1"}"#,
    ));
}

#[test]
fn test_int_param_max_value_as_string() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "INT", "maxValue": "1"}"#,
    ));
}

#[test]
fn test_int_param_allowed_values_as_string() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "INT", "allowedValues": ["1"]}"#,
    ));
}

// ══════════════════════════════════════════════════════════════
// FLOAT parameter — userInterface tests
// ══════════════════════════════════════════════════════════════

#[test]
fn test_float_param_ui_spin_box() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "FLOAT", "userInterface": {"control": "SPIN_BOX"}}"#,
    ));
}

#[test]
fn test_float_param_ui_hidden() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "FLOAT", "userInterface": {"control": "HIDDEN"}}"#,
    ));
}

#[test]
fn test_float_param_ui_dropdown_list() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "FLOAT", "userInterface": {"control": "DROPDOWN_LIST"}, "allowedValues": [1]}"#,
    ));
}

#[test]
fn test_float_param_description() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "FLOAT", "description": "some text"}"#,
    ));
}

#[test]
fn test_float_param_min_value_as_string() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "FLOAT", "minValue": "1.2"}"#,
    ));
}

#[test]
fn test_float_param_max_value_as_string() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "FLOAT", "maxValue": "1.2"}"#,
    ));
}

#[test]
fn test_float_param_allowed_values_as_string() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "FLOAT", "allowedValues": ["1.2"]}"#,
    ));
}

// ══════════════════════════════════════════════════════════════
// PATH parameter — userInterface tests
// ══════════════════════════════════════════════════════════════

#[test]
fn test_path_param_ui_choose_input_file() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "PATH", "objectType": "FILE", "userInterface": {"control": "CHOOSE_INPUT_FILE"}}"#,
    ));
}

#[test]
fn test_path_param_ui_choose_output_file() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "PATH", "objectType": "FILE", "userInterface": {"control": "CHOOSE_OUTPUT_FILE"}}"#,
    ));
}

#[test]
fn test_path_param_ui_choose_directory() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "PATH", "userInterface": {"control": "CHOOSE_DIRECTORY"}}"#,
    ));
}

#[test]
fn test_path_param_ui_hidden() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "PATH", "userInterface": {"control": "HIDDEN"}}"#,
    ));
}

#[test]
fn test_path_param_ui_dropdown_list() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "PATH", "userInterface": {"control": "DROPDOWN_LIST"}, "allowedValues": ["/aa/bb"]}"#,
    ));
}

#[test]
fn test_path_param_description() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "PATH", "description": "some test"}"#,
    ));
}

#[test]
fn test_path_param_description_with_newlines() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "PATH", "description": "aa\nbb\ncc"}"#,
    ));
}

// Note: Python tests for INT allowedValues vs minValue/maxValue validation
// are not ported because this validation is not yet implemented in Rust.

// ══════════════════════════════════════════════════════════════
// PATH parameter — maxLength zero
// ══════════════════════════════════════════════════════════════

// Note: Python test for PATH maxLength=0 is not ported because this
// validation is not yet implemented in Rust.

// ══════════════════════════════════════════════════════════════
// STRING parameter — allowedValues exceeding 1024 char limit
// ══════════════════════════════════════════════════════════════

#[test]
fn test_string_param_allowed_value_exceeds_1024() {
    let long = "x".repeat(1025);
    check_err(&job_with_param(&format!(r#"{{"name": "Foo", "type": "STRING", "allowedValues": ["{long}"]}}"#)), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': allowedValues[0] exceeds 1024 characters.",
    ]);
}

#[test]
fn test_string_param_default_exceeds_1024() {
    let long = "x".repeat(1025);
    check_err(
        &job_with_param(&format!(
            r#"{{"name": "Foo", "type": "STRING", "default": "{long}"}}"#
        )),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': default exceeds 1024 characters."],
    );
}

// ══════════════════════════════════════════════════════════════
// STRING parameter — UI error branches
// ══════════════════════════════════════════════════════════════

#[test]
fn test_string_param_ui_line_edit_with_allowed_values() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "STRING", "allowedValues": ["a"], "userInterface": {"control": "LINE_EDIT"}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': control 'LINE_EDIT' cannot be used with allowedValues.",
    ]);
}

#[test]
fn test_string_param_ui_multiline_edit_with_allowed_values() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "STRING", "allowedValues": ["a"], "userInterface": {"control": "MULTILINE_EDIT"}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': control 'MULTILINE_EDIT' cannot be used with allowedValues.",
    ]);
}

#[test]
fn test_string_param_ui_dropdown_without_allowed() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "STRING", "userInterface": {"control": "DROPDOWN_LIST"}}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': DROPDOWN_LIST requires allowedValues."],
    );
}

#[test]
fn test_string_param_ui_checkbox_without_allowed() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "STRING", "userInterface": {"control": "CHECK_BOX"}}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': CHECK_BOX requires allowedValues."],
    );
}

#[test]
fn test_string_param_ui_checkbox_wrong_count() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "STRING", "allowedValues": ["a", "b", "c"], "userInterface": {"control": "CHECK_BOX"}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': CHECK_BOX requires exactly 2 allowedValues.",
    ]);
}

#[test]
fn test_string_param_ui_checkbox_invalid_pair() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "STRING", "allowedValues": ["cat", "dog"], "userInterface": {"control": "CHECK_BOX"}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': CHECK_BOX allowedValues must be a valid boolean pair.",
    ]);
}

#[test]
fn test_string_param_ui_checkbox_valid_pairs() {
    // true/false
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "STRING", "allowedValues": ["True", "False"], "userInterface": {"control": "CHECK_BOX"}}"#,
    ));
    // yes/no
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "STRING", "allowedValues": ["YES", "NO"], "userInterface": {"control": "CHECK_BOX"}}"#,
    ));
    // on/off
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "STRING", "allowedValues": ["on", "off"], "userInterface": {"control": "CHECK_BOX"}}"#,
    ));
    // 1/0
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "STRING", "allowedValues": ["0", "1"], "userInterface": {"control": "CHECK_BOX"}}"#,
    ));
}

#[test]
fn test_string_param_ui_unknown_control() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "STRING", "userInterface": {"control": "SLIDER"}}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': unknown control 'SLIDER'."],
    );
}

#[test]
fn test_string_param_ui_label_empty() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "STRING", "userInterface": {"label": ""}}"#),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': label must not be empty."],
    );
}

#[test]
fn test_string_param_ui_label_too_long() {
    let long_label = "x".repeat(65);
    check_err(
        &job_with_param(&format!(
            r#"{{"name": "Foo", "type": "STRING", "userInterface": {{"label": "{long_label}"}}}}"#
        )),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': label exceeds 64 characters."],
    );
}

#[test]
fn test_string_param_ui_label_control_chars() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "STRING", "userInterface": {"label": "ab\u0001cd"}}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': label contains control characters."],
    );
}

#[test]
fn test_string_param_ui_group_label_empty() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "STRING", "userInterface": {"groupLabel": ""}}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': groupLabel must not be empty."],
    );
}

// ══════════════════════════════════════════════════════════════
// INT parameter — UI error branches
// ══════════════════════════════════════════════════════════════

#[test]
fn test_int_param_ui_spin_box_with_allowed() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "INT", "allowedValues": [1], "userInterface": {"control": "SPIN_BOX"}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': SPIN_BOX cannot be used with allowedValues.",
    ]);
}

#[test]
fn test_int_param_ui_dropdown_without_allowed() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "INT", "userInterface": {"control": "DROPDOWN_LIST"}}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': DROPDOWN_LIST requires allowedValues."],
    );
}

#[test]
fn test_int_param_ui_dropdown_with_single_step_delta() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "INT", "allowedValues": [1], "userInterface": {"control": "DROPDOWN_LIST", "singleStepDelta": 1}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': singleStepDelta is only valid with SPIN_BOX.",
    ]);
}

#[test]
fn test_int_param_ui_hidden_with_single_step_delta() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "INT", "userInterface": {"control": "HIDDEN", "singleStepDelta": 1}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': singleStepDelta is not valid with HIDDEN.",
    ]);
}

#[test]
fn test_int_param_ui_unknown_control() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "INT", "userInterface": {"control": "SLIDER"}}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': unknown control 'SLIDER'."],
    );
}

#[test]
fn test_int_param_ui_single_step_delta_negative() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "INT", "userInterface": {"control": "SPIN_BOX", "singleStepDelta": -1}}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': singleStepDelta must be positive."],
    );
}

#[test]
fn test_int_param_ui_single_step_delta_zero() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "INT", "userInterface": {"control": "SPIN_BOX", "singleStepDelta": 0}}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': singleStepDelta must be positive."],
    );
}

#[test]
fn test_int_param_ui_label_empty() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "INT", "userInterface": {"label": ""}}"#),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': label must not be empty."],
    );
}

// ══════════════════════════════════════════════════════════════
// FLOAT parameter — UI error branches
// ══════════════════════════════════════════════════════════════

#[test]
fn test_float_param_ui_spin_box_with_allowed() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "FLOAT", "allowedValues": [1.0], "userInterface": {"control": "SPIN_BOX"}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': SPIN_BOX cannot be used with allowedValues.",
    ]);
}

#[test]
fn test_float_param_ui_dropdown_without_allowed() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "FLOAT", "userInterface": {"control": "DROPDOWN_LIST"}}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': DROPDOWN_LIST requires allowedValues."],
    );
}

#[test]
fn test_float_param_ui_dropdown_with_decimals() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "FLOAT", "allowedValues": [1.0], "userInterface": {"control": "DROPDOWN_LIST", "decimals": 2}}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': decimals is only valid with SPIN_BOX."],
    );
}

#[test]
fn test_float_param_ui_dropdown_with_single_step_delta() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "FLOAT", "allowedValues": [1.0], "userInterface": {"control": "DROPDOWN_LIST", "singleStepDelta": 0.1}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': singleStepDelta is only valid with SPIN_BOX.",
    ]);
}

#[test]
fn test_float_param_ui_hidden_with_decimals() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "FLOAT", "userInterface": {"control": "HIDDEN", "decimals": 2}}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': decimals is not valid with HIDDEN."],
    );
}

#[test]
fn test_float_param_ui_hidden_with_single_step_delta() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "FLOAT", "userInterface": {"control": "HIDDEN", "singleStepDelta": 0.1}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': singleStepDelta is not valid with HIDDEN.",
    ]);
}

#[test]
fn test_float_param_ui_unknown_control() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "FLOAT", "userInterface": {"control": "SLIDER"}}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': unknown control 'SLIDER'."],
    );
}

#[test]
fn test_float_param_ui_single_step_delta_negative() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "FLOAT", "userInterface": {"control": "SPIN_BOX", "singleStepDelta": -0.1}}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': singleStepDelta must be positive."],
    );
}

#[test]
fn test_float_param_ui_label_empty() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "FLOAT", "userInterface": {"label": ""}}"#),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': label must not be empty."],
    );
}

// ══════════════════════════════════════════════════════════════
// PATH parameter — UI error branches
// ══════════════════════════════════════════════════════════════

#[test]
fn test_path_param_ui_choose_input_file_with_allowed() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "PATH", "objectType": "FILE", "allowedValues": ["/a"], "userInterface": {"control": "CHOOSE_INPUT_FILE"}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': CHOOSE_INPUT_FILE cannot be used with allowedValues.",
    ]);
}

#[test]
fn test_path_param_ui_choose_input_file_with_directory() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "PATH", "objectType": "DIRECTORY", "userInterface": {"control": "CHOOSE_INPUT_FILE"}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': CHOOSE_INPUT_FILE requires objectType FILE.",
    ]);
}

#[test]
fn test_path_param_ui_choose_output_file_with_directory() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "PATH", "objectType": "DIRECTORY", "userInterface": {"control": "CHOOSE_OUTPUT_FILE"}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': CHOOSE_OUTPUT_FILE requires objectType FILE.",
    ]);
}

#[test]
fn test_path_param_ui_choose_directory_with_allowed() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "PATH", "allowedValues": ["/a"], "userInterface": {"control": "CHOOSE_DIRECTORY"}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': CHOOSE_DIRECTORY cannot be used with allowedValues.",
    ]);
}

#[test]
fn test_path_param_ui_choose_directory_with_file_type() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "PATH", "objectType": "FILE", "userInterface": {"control": "CHOOSE_DIRECTORY"}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': CHOOSE_DIRECTORY requires objectType DIRECTORY.",
    ]);
}

#[test]
fn test_path_param_ui_dropdown_without_allowed() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "PATH", "userInterface": {"control": "DROPDOWN_LIST"}}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': DROPDOWN_LIST requires allowedValues."],
    );
}

#[test]
fn test_path_param_ui_unknown_control() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "PATH", "userInterface": {"control": "SLIDER"}}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': unknown control 'SLIDER'."],
    );
}

#[test]
fn test_path_param_ui_label_empty() {
    check_err(
        &job_with_param(r#"{"name": "Foo", "type": "PATH", "userInterface": {"label": ""}}"#),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': label must not be empty."],
    );
}

// ══════════════════════════════════════════════════════════════
// PATH parameter — fileFilters validation
// ══════════════════════════════════════════════════════════════

#[test]
fn test_path_param_file_filters_on_non_file_chooser() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "PATH", "userInterface": {"control": "CHOOSE_DIRECTORY", "fileFilters": [{"label": "All", "patterns": ["*"]}]}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': fileFilters only valid with file chooser controls.",
    ]);
}

#[test]
fn test_path_param_file_filter_empty_patterns() {
    check_err(
        &job_with_param(
            r#"{"name": "Foo", "type": "PATH", "objectType": "FILE", "userInterface": {"control": "CHOOSE_INPUT_FILE", "fileFilters": [{"label": "All", "patterns": []}]}}"#,
        ),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': fileFilter patterns must not be empty."],
    );
}

#[test]
fn test_path_param_file_filter_invalid_pattern() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "PATH", "objectType": "FILE", "userInterface": {"control": "CHOOSE_INPUT_FILE", "fileFilters": [{"label": "All", "patterns": ["bad"]}]}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': file filter pattern 'bad' must be '*', '*.*', or '*.ext'.",
    ]);
}

#[test]
fn test_path_param_file_filter_empty_extension() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "PATH", "objectType": "FILE", "userInterface": {"control": "CHOOSE_INPUT_FILE", "fileFilters": [{"label": "All", "patterns": ["*."]}]}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': file filter pattern '*.' has empty extension.",
    ]);
}

#[test]
fn test_path_param_file_filter_disallowed_char() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "PATH", "objectType": "FILE", "userInterface": {"control": "CHOOSE_INPUT_FILE", "fileFilters": [{"label": "All", "patterns": ["*.t$t"]}]}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': file filter pattern '*.t$t' contains disallowed character '$'.",
    ]);
}

#[test]
fn test_path_param_file_filter_pattern_too_long() {
    let long_ext = "x".repeat(19);
    let pattern = format!("*.{long_ext}");
    check_err(&job_with_param(&format!(r#"{{"name": "Foo", "type": "PATH", "objectType": "FILE", "userInterface": {{"control": "CHOOSE_INPUT_FILE", "fileFilters": [{{"label": "All", "patterns": ["{pattern}"]}}]}}}}"#)), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': file filter pattern must be 1..=20 characters.",
    ]);
}

#[test]
fn test_path_param_file_filter_valid_patterns() {
    decode_ok(&job_with_param(
        r#"{"name": "Foo", "type": "PATH", "objectType": "FILE", "userInterface": {"control": "CHOOSE_INPUT_FILE", "fileFilters": [{"label": "All", "patterns": ["*", "*.*", "*.txt"]}]}}"#,
    ));
}

#[test]
fn test_path_param_file_filter_default_on_non_file_chooser() {
    check_err(&job_with_param(r#"{"name": "Foo", "type": "PATH", "userInterface": {"control": "CHOOSE_DIRECTORY", "fileFilterDefault": {"label": "All", "patterns": ["*"]}}}"#), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': fileFilterDefault only valid with file chooser controls.",
    ]);
}

// ══════════════════════════════════════════════════════════════
// PATH parameter — allowedValues exceeding 1024 char limit
// ══════════════════════════════════════════════════════════════

#[test]
fn test_path_param_allowed_value_exceeds_1024() {
    let long = "x".repeat(1025);
    check_err(&job_with_param(&format!(r#"{{"name": "Foo", "type": "PATH", "allowedValues": ["{long}"]}}"#)), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': allowedValues[0] exceeds 1024 characters.",
    ]);
}

#[test]
fn test_path_param_default_exceeds_1024() {
    let long = "x".repeat(1025);
    check_err(
        &job_with_param(&format!(
            r#"{{"name": "Foo", "type": "PATH", "default": "{long}"}}"#
        )),
        &["parameterDefinitions[0]:\n\tParameter 'Foo': default exceeds 1024 characters."],
    );
}

// ══════════════════════════════════════════════════════════════
// Large float value roundtrip
// ══════════════════════════════════════════════════════════════

#[test]
fn float_param_large_value_roundtrip() {
    let td = tempfile::TempDir::new().unwrap();
    let template = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "test",
        "parameterDefinitions": [{"name": "Big", "type": "FLOAT", "default": "1e20"}],
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}}}]
    }"#,
    );
    let jt = decode_job_template(template, None).unwrap();
    let result = openjd_model::preprocess_job_parameters(
        &jt,
        &openjd_model::JobParameterInputValues::new(),
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    match &result["Big"].value {
        openjd_expr::ExprValue::Float(f) => {
            let reparsed: f64 = f.to_string().parse().unwrap();
            assert_eq!(reparsed, 1e20);
        }
        other => panic!("Expected Float, got {other:?}"),
    }
}

#[test]
fn bug_string_param_allowed_values_byte_vs_char_length() {
    // "aéb" is 3 chars but 4 bytes. maxLength=3 should accept it.
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        parameterDefinitions:
          - name: Greeting
            type: STRING
            maxLength: 3
            allowedValues: ["aéb"]
            default: "aéb"
        steps:
          - name: Step1
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let result = decode_job_template(template, None);
    assert!(
        result.is_ok(),
        "3-char string with maxLength=3 should pass: {:?}",
        result.err()
    );
}

#[test]
fn string_param_minlength_uses_char_count() {
    // "éé" is 2 chars but 4 bytes. minLength=2 should accept it.
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        parameterDefinitions:
          - name: Val
            type: STRING
            minLength: 2
            default: "éé"
        steps:
          - name: Step1
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let result = decode_job_template(template, None);
    assert!(
        result.is_ok(),
        "2-char string with minLength=2 should pass: {:?}",
        result.err()
    );
}

#[test]
fn path_param_maxlength_uses_char_count() {
    // "/àb" is 3 chars but 4 bytes. maxLength=3 should accept it.
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        parameterDefinitions:
          - name: Dir
            type: PATH
            maxLength: 3
            default: "/àb"
        steps:
          - name: Step1
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let result = decode_job_template(template, None);
    assert!(
        result.is_ok(),
        "3-char path with maxLength=3 should pass: {:?}",
        result.err()
    );
}

#[test]
fn path_param_minlength_uses_char_count() {
    // "/é" is 2 chars but 3 bytes. minLength=2 should accept it.
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        parameterDefinitions:
          - name: Dir
            type: PATH
            minLength: 2
            default: "/é"
        steps:
          - name: Step1
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let result = decode_job_template(template, None);
    assert!(
        result.is_ok(),
        "2-char path with minLength=2 should pass: {:?}",
        result.err()
    );
}

#[test]
fn ui_label_maxlength_uses_char_count() {
    // 64 chars of "é" = 128 bytes. Should pass the 64-char label limit.
    let label = "é".repeat(64);
    let template = yaml_val(&format!(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        parameterDefinitions:
          - name: Val
            type: STRING
            userInterface:
              label: "{label}"
        steps:
          - name: Step1
            script:
              actions:
                onRun:
                  command: echo
    "#
    ));
    let result = decode_job_template(template, None);
    assert!(
        result.is_ok(),
        "64-char label should pass: {:?}",
        result.err()
    );
}

#[test]
fn float_param_nan_rejected_by_flexfloat() {
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        parameterDefinitions:
          - name: Val
            type: FLOAT
            default: .nan
        steps:
          - name: Step1
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let result = decode_job_template(template, None);
    assert!(result.is_err(), "NaN must be rejected");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("NaN is not a valid float value"),
        "Expected NaN rejection message, got: {msg}"
    );
}

#[test]
fn float_param_infinity_rejected_by_flexfloat() {
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        parameterDefinitions:
          - name: Val
            type: FLOAT
            default: .inf
        steps:
          - name: Step1
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let result = decode_job_template(template, None);
    assert!(result.is_err(), "Infinity must be rejected");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("Infinity is not a valid float value"),
        "Expected Infinity rejection message, got: {msg}"
    );
}

// ══════════════════════════════════════════════════════════════
// STRING/PATH parameter minLength/maxLength char vs byte semantics
// ══════════════════════════════════════════════════════════════

#[test]
fn string_param_maxlength_uses_chars_not_bytes() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{
            "name": "Msg",
            "type": "STRING",
            "default": "hello",
            "maxLength": 5
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#,
    );
    let jt = decode_job_template(v, None).unwrap();
    let param = &jt.parameter_definitions.as_ref().unwrap()[0];

    // "héllo" is 5 chars but 6 bytes — should be accepted with maxLength=5
    let test_value = openjd_expr::ExprValue::String("héllo".to_string());
    assert!(
        param.check_constraints(&test_value).is_ok(),
        "5-character string 'héllo' should pass maxLength=5 (char count, not byte count)"
    );
}

#[test]
fn string_param_minlength_uses_chars_not_bytes() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{
            "name": "Msg",
            "type": "STRING",
            "default": "hello world",
            "minLength": 6
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#,
    );
    let jt = decode_job_template(v, None).unwrap();
    let param = &jt.parameter_definitions.as_ref().unwrap()[0];

    // "ééé" is 3 chars but 6 bytes — should be rejected with minLength=6
    let test_value = openjd_expr::ExprValue::String("ééé".to_string());
    assert!(
        param.check_constraints(&test_value).is_err(),
        "3-character string 'ééé' should fail minLength=6 (char count, not byte count)"
    );
}

#[test]
fn path_param_maxlength_uses_chars_not_bytes() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{
            "name": "Dir",
            "type": "PATH",
            "default": "/tmp/hello",
            "maxLength": 10
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#,
    );
    let jt = decode_job_template(v, None).unwrap();
    let param = &jt.parameter_definitions.as_ref().unwrap()[0];

    // "/tmp/héllo" is 10 chars but 11 bytes — should be accepted with maxLength=10
    let test_value =
        openjd_expr::ExprValue::new_path("/tmp/héllo".to_string(), openjd_expr::PathFormat::Posix);
    assert!(
        param.check_constraints(&test_value).is_ok(),
        "10-character path '/tmp/héllo' should pass maxLength=10 (char count, not byte count)"
    );
}
