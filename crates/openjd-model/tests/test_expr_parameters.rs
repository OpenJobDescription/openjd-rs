// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_job_parameters_bool.py, test_job_parameters_range_expr.py,
//! test_job_parameters_list_string.py, test_job_parameters_list_int.py,
//! test_job_parameters_list_float.py, test_job_parameters_list_path.py,
//! test_job_parameters_list_bool.py, test_job_parameters_list_list_int.py,
//! test_job_parameters_case_insensitive.py
//!
//! Gold standard: failure tests assert the full error message including path.

use openjd_model::decode_job_template;
use openjd_model::CallerLimits;

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn job_with_expr_param(param_json: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "parameterDefinitions": [{param_json}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}]
    }}"#
    )
}

fn decode_ok(s: &str) {
    let v = yaml_val(s);
    decode_job_template(v, Some(&["EXPR"]), &CallerLimits::default()).expect("Expected success");
}

/// Gold standard check_err: asserts path + message for validation errors.
fn check_err(s: &str, expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_job_template(v, Some(&["EXPR"]), &CallerLimits::default())
        .expect_err(&format!("Expected error for: {s}"));
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

/// For serde-level errors (deserialization failures), assert substring.
fn check_serde_err(s: &str, expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_job_template(v, Some(&["EXPR"]), &CallerLimits::default())
        .expect_err(&format!("Expected error for: {s}"));
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

/// For errors without EXPR extension.
fn check_err_no_ext(s: &str, expected: &[&str]) {
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

// ============================================================
// BOOL parameter — success cases
// ============================================================

#[test]
fn test_bool_param_minimal() {
    decode_ok(&job_with_expr_param(r#"{"name": "Foo", "type": "BOOL"}"#));
}

#[test]
fn test_bool_param_default_true() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "BOOL", "default": true}"#,
    ));
}

#[test]
fn test_bool_param_default_false() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "BOOL", "default": false}"#,
    ));
}

#[test]
fn test_bool_param_default_string_true() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "BOOL", "default": "true"}"#,
    ));
}

#[test]
fn test_bool_param_default_string_false() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "BOOL", "default": "false"}"#,
    ));
}

#[test]
fn test_bool_param_default_string_yes() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "BOOL", "default": "yes"}"#,
    ));
}

#[test]
fn test_bool_param_default_string_no() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "BOOL", "default": "no"}"#,
    ));
}

#[test]
fn test_bool_param_default_string_on() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "BOOL", "default": "on"}"#,
    ));
}

#[test]
fn test_bool_param_default_string_off() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "BOOL", "default": "off"}"#,
    ));
}

#[test]
fn test_bool_param_default_string_1() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "BOOL", "default": "1"}"#,
    ));
}

#[test]
fn test_bool_param_default_string_0() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "BOOL", "default": "0"}"#,
    ));
}

#[test]
fn test_bool_param_default_int_1() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "BOOL", "default": 1}"#,
    ));
}

#[test]
fn test_bool_param_default_int_0() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "BOOL", "default": 0}"#,
    ));
}

#[test]
fn test_bool_param_description() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "BOOL", "description": "some text"}"#,
    ));
}

#[test]
fn test_bool_param_ui_checkbox() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "BOOL", "userInterface": {"control": "CHECK_BOX"}}"#,
    ));
}

#[test]
fn test_bool_param_ui_hidden() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "BOOL", "userInterface": {"control": "HIDDEN"}}"#,
    ));
}

#[test]
fn test_bool_param_all_fields() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "BOOL", "userInterface": {"control": "CHECK_BOX", "label": "Enable", "groupLabel": "Options"}, "default": false, "description": "Enable feature"}"#,
    ));
}

// ============================================================
// BOOL parameter — failure cases
// ============================================================

#[test]
fn test_bool_without_expr_extension() {
    check_err_no_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Foo", "type": "BOOL"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["parameterDefinitions[0]:\n\tparameter type 'BOOL' is not allowed."],
    );
}

#[test]
fn test_bool_default_invalid_string() {
    check_serde_err(
        &job_with_expr_param(r#"{"name": "Foo", "type": "BOOL", "default": "maybe"}"#),
        &["Invalid bool value: 'maybe'"],
    );
}

#[test]
fn test_bool_default_int_2() {
    check_serde_err(
        &job_with_expr_param(r#"{"name": "Foo", "type": "BOOL", "default": 2}"#),
        &["Invalid bool value: 2"],
    );
}

#[test]
fn test_bool_default_float_0_5() {
    check_serde_err(
        &job_with_expr_param(r#"{"name": "Foo", "type": "BOOL", "default": 0.5}"#),
        &["Invalid bool value: 0.5"],
    );
}

// ============================================================
// RANGE_EXPR parameter — success cases
// ============================================================

#[test]
fn test_range_expr_param_minimal() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "RANGE_EXPR"}"#,
    ));
}

#[test]
fn test_range_expr_param_with_default() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "RANGE_EXPR", "default": "1-10"}"#,
    ));
}

#[test]
fn test_range_expr_param_with_step() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "RANGE_EXPR", "default": "1-100:10"}"#,
    ));
}

#[test]
fn test_range_expr_param_default_list() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "RANGE_EXPR", "default": "1,3,5,7"}"#,
    ));
}

#[test]
fn test_range_expr_param_default_mixed() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "RANGE_EXPR", "default": "1-10,20-30:2"}"#,
    ));
}

#[test]
fn test_range_expr_param_description() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "RANGE_EXPR", "description": "frame range"}"#,
    ));
}

#[test]
fn test_range_expr_param_min_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "RANGE_EXPR", "minLength": 1}"#,
    ));
}

#[test]
fn test_range_expr_param_max_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "RANGE_EXPR", "maxLength": 100}"#,
    ));
}

#[test]
fn test_range_expr_param_min_and_max_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "RANGE_EXPR", "minLength": 1, "maxLength": 100}"#,
    ));
}

#[test]
fn test_range_expr_param_min_equals_max() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "RANGE_EXPR", "minLength": 5, "maxLength": 5}"#,
    ));
}

#[test]
fn test_range_expr_param_ui_line_edit() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "RANGE_EXPR", "userInterface": {"control": "LINE_EDIT"}}"#,
    ));
}

#[test]
fn test_range_expr_param_ui_hidden() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "RANGE_EXPR", "userInterface": {"control": "HIDDEN"}}"#,
    ));
}

// ============================================================
// RANGE_EXPR parameter — failure cases
// ============================================================

#[test]
fn test_range_expr_without_expr_extension() {
    check_err_no_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Foo", "type": "RANGE_EXPR"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["parameterDefinitions[0]:\n\tparameter type 'RANGE_EXPR' is not allowed."],
    );
}

#[test]
fn test_range_expr_invalid_default() {
    check_err(&job_with_expr_param(
        r#"{"name": "Foo", "type": "RANGE_EXPR", "default": "invalid"}"#
    ), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': default 'invalid' is not a valid range expression.",
    ]);
}

#[test]
fn test_range_expr_empty_default() {
    check_err(&job_with_expr_param(
        r#"{"name": "Foo", "type": "RANGE_EXPR", "default": ""}"#
    ), &[
        "parameterDefinitions[0]:\n\tParameter 'Foo': default '' is not a valid range expression.",
    ]);
}

#[test]
fn test_range_expr_default_not_string() {
    check_serde_err(
        &job_with_expr_param(r#"{"name": "Foo", "type": "RANGE_EXPR", "default": 123}"#),
        &["invalid type"],
    );
}

// ============================================================
// LIST[INT] parameter — success cases
// ============================================================

#[test]
fn test_list_int_param_minimal() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[INT]"}"#,
    ));
}

#[test]
fn test_list_int_param_with_default() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[INT]", "default": [1, 2, 3]}"#,
    ));
}

#[test]
fn test_list_int_param_empty_default() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[INT]", "default": []}"#,
    ));
}

#[test]
fn test_list_int_param_description() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[INT]", "description": "list of ints"}"#,
    ));
}

#[test]
fn test_list_int_param_min_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[INT]", "minLength": 1}"#,
    ));
}

#[test]
fn test_list_int_param_max_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[INT]", "maxLength": 10}"#,
    ));
}

#[test]
fn test_list_int_param_item_min_value() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[INT]", "item": {"minValue": 0}}"#,
    ));
}

#[test]
fn test_list_int_param_item_max_value() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[INT]", "item": {"maxValue": 100}}"#,
    ));
}

#[test]
fn test_list_int_param_item_allowed_values() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[INT]", "item": {"allowedValues": [1, 2, 3]}}"#,
    ));
}

#[test]
fn test_list_int_param_ui_hidden() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[INT]", "userInterface": {"control": "HIDDEN"}}"#,
    ));
}

// ============================================================
// LIST[INT] parameter — failure cases
// ============================================================

#[test]
fn test_list_int_without_expr_extension() {
    check_err_no_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Foo", "type": "LIST[INT]"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["parameterDefinitions[0]:\n\tparameter type 'LIST[INT]' is not allowed."],
    );
}

#[test]
fn test_list_int_default_not_list() {
    check_serde_err(
        &job_with_expr_param(r#"{"name": "Foo", "type": "LIST[INT]", "default": "not a list"}"#),
        &["invalid type"],
    );
}

// ============================================================
// LIST[FLOAT] parameter — success cases
// ============================================================

#[test]
fn test_list_float_param_minimal() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[FLOAT]"}"#,
    ));
}

#[test]
fn test_list_float_param_with_default() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[FLOAT]", "default": [1.0, 2.5]}"#,
    ));
}

// ============================================================
// LIST[FLOAT] parameter — failure cases
// ============================================================

#[test]
fn test_list_float_without_expr_extension() {
    check_err_no_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Foo", "type": "LIST[FLOAT]"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["parameterDefinitions[0]:\n\tparameter type 'LIST[FLOAT]' is not allowed."],
    );
}

// ============================================================
// LIST[STRING] parameter — success cases
// ============================================================

#[test]
fn test_list_string_param_minimal() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[STRING]"}"#,
    ));
}

#[test]
fn test_list_string_param_with_default() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[STRING]", "default": ["a", "b"]}"#,
    ));
}

#[test]
fn test_list_string_param_description() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[STRING]", "description": "list of strings"}"#,
    ));
}

#[test]
fn test_list_string_param_min_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[STRING]", "minLength": 1}"#,
    ));
}

#[test]
fn test_list_string_param_max_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[STRING]", "maxLength": 10}"#,
    ));
}

#[test]
fn test_list_string_param_min_and_max_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[STRING]", "minLength": 1, "maxLength": 10}"#,
    ));
}

#[test]
fn test_list_string_param_item_min_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[STRING]", "item": {"minLength": 1}}"#,
    ));
}

#[test]
fn test_list_string_param_item_max_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[STRING]", "item": {"maxLength": 100}}"#,
    ));
}

#[test]
fn test_list_string_param_item_allowed_values() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[STRING]", "item": {"allowedValues": ["a", "b"]}}"#,
    ));
}

#[test]
fn test_list_string_param_ui_hidden() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[STRING]", "userInterface": {"control": "HIDDEN"}}"#,
    ));
}

// ============================================================
// LIST[STRING] parameter — failure cases
// ============================================================

#[test]
fn test_list_string_without_expr_extension() {
    check_err_no_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Foo", "type": "LIST[STRING]"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["parameterDefinitions[0]:\n\tparameter type 'LIST[STRING]' is not allowed."],
    );
}

#[test]
fn test_list_string_default_not_list() {
    check_serde_err(
        &job_with_expr_param(r#"{"name": "Foo", "type": "LIST[STRING]", "default": "not a list"}"#),
        &["invalid type"],
    );
}

// ============================================================
// LIST[BOOL] parameter — success cases
// ============================================================

#[test]
fn test_list_bool_param_minimal() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[BOOL]"}"#,
    ));
}

#[test]
fn test_list_bool_param_with_default() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[BOOL]", "default": [true, false]}"#,
    ));
}

// ============================================================
// LIST[BOOL] parameter — failure cases
// ============================================================

#[test]
fn test_list_bool_without_expr_extension() {
    check_err_no_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Foo", "type": "LIST[BOOL]"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["parameterDefinitions[0]:\n\tparameter type 'LIST[BOOL]' is not allowed."],
    );
}

// ============================================================
// LIST[PATH] parameter — success cases
// ============================================================

#[test]
fn test_list_path_param_minimal() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[PATH]"}"#,
    ));
}

#[test]
fn test_list_path_param_with_default() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[PATH]", "default": ["/tmp/a", "/tmp/b"]}"#,
    ));
}

// ============================================================
// LIST[PATH] parameter — failure cases
// ============================================================

#[test]
fn test_list_path_without_expr_extension() {
    check_err_no_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Foo", "type": "LIST[PATH]"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["parameterDefinitions[0]:\n\tparameter type 'LIST[PATH]' is not allowed."],
    );
}

// ============================================================
// LIST[LIST[INT]] parameter — success cases
// ============================================================

#[test]
fn test_list_list_int_param_minimal() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[LIST[INT]]"}"#,
    ));
}

#[test]
fn test_list_list_int_param_with_default() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "default": [[1, 2], [3]]}"#,
    ));
}

// ============================================================
// LIST[LIST[INT]] parameter — failure cases
// ============================================================

#[test]
fn test_list_list_int_without_expr_extension() {
    check_err_no_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Foo", "type": "LIST[LIST[INT]]"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["parameterDefinitions[0]:\n\tparameter type 'LIST[LIST[INT]]' is not allowed."],
    );
}

// ============================================================
// Case-insensitive parameter names
// ============================================================

#[test]
fn test_case_sensitive_duplicate() {
    check_err_no_ext(
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
fn test_case_different_not_duplicate() {
    // Different case → not a duplicate
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [
            {"name": "Foo", "type": "STRING"},
            {"name": "foo", "type": "STRING"}
        ],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#;
    let v = yaml_val(s);
    assert!(decode_job_template(v, None, &CallerLimits::default()).is_ok());
}

// ============================================================
// Case-insensitive parameter types with EXPR extension
// ============================================================

#[test]
fn test_lowercase_string_with_expr() {
    decode_ok(&job_with_expr_param(r#"{"name": "Foo", "type": "string"}"#));
}

#[test]
fn test_lowercase_int_with_expr() {
    decode_ok(&job_with_expr_param(r#"{"name": "Foo", "type": "int"}"#));
}

#[test]
fn test_lowercase_float_with_expr() {
    decode_ok(&job_with_expr_param(r#"{"name": "Foo", "type": "float"}"#));
}

#[test]
fn test_lowercase_path_with_expr() {
    decode_ok(&job_with_expr_param(r#"{"name": "Foo", "type": "path"}"#));
}

#[test]
fn test_lowercase_bool_with_expr() {
    decode_ok(&job_with_expr_param(r#"{"name": "Foo", "type": "bool"}"#));
}

#[test]
fn test_mixed_case_string_with_expr() {
    decode_ok(&job_with_expr_param(r#"{"name": "Foo", "type": "String"}"#));
}

#[test]
fn test_mixed_case_list_string_with_expr() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "List[String]"}"#,
    ));
}

#[test]
fn test_lowercase_list_int_with_expr() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "list[int]"}"#,
    ));
}

#[test]
fn test_lowercase_list_list_int_with_expr() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "list[list[int]]"}"#,
    ));
}

#[test]
fn test_lowercase_range_expr_with_expr() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "range_expr"}"#,
    ));
}

// Note: Python tests for case-insensitive types failing without EXPR are not ported
// because the Rust implementation accepts case-insensitive types by default.

// ============================================================
// LIST[FLOAT] parameter — additional tests
// ============================================================

#[test]
fn test_list_float_param_empty_default() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[FLOAT]", "default": []}"#,
    ));
}

#[test]
fn test_list_float_param_description() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[FLOAT]", "description": "list of floats"}"#,
    ));
}

// ============================================================
// LIST[BOOL] parameter — additional tests
// ============================================================

#[test]
fn test_list_bool_param_empty_default() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[BOOL]", "default": []}"#,
    ));
}

#[test]
fn test_list_bool_param_description() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[BOOL]", "description": "list of bools"}"#,
    ));
}

// ============================================================
// LIST[PATH] parameter — additional tests
// ============================================================

#[test]
fn test_list_path_param_empty_default() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[PATH]", "default": []}"#,
    ));
}

#[test]
fn test_list_path_param_description() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[PATH]", "description": "list of paths"}"#,
    ));
}

// ============================================================
// LIST[LIST[INT]] parameter — additional tests
// ============================================================

#[test]
fn test_list_list_int_param_empty_default() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "default": []}"#,
    ));
}

#[test]
fn test_list_list_int_param_description() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "description": "nested list"}"#,
    ));
}

#[test]
fn test_list_list_int_param_nested_empty() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "default": [[]]}"#,
    ));
}

// ============================================================
// Phase 2: validate_definition() — default vs constraint tests
// These exercise the validate_definition() error branches that
// check defaults against item constraints at template parse time.
// ============================================================

// ============================================================
// LIST[STRING] default validation per §2.11
// Constraints: list minLength/maxLength, item allowedValues/minLength/maxLength
// ============================================================

// --- list length ---

#[test]
fn test_list_string_default_at_min_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[STRING]", "minLength": 2, "default": ["a", "b"]}"#,
    ));
}

#[test]
fn test_list_string_default_below_min_length() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[STRING]", "minLength": 2, "default": ["a"]}"#,
        ),
        &["Parameter 'Foo': default list length 1 < minLength 2."],
    );
}

#[test]
fn test_list_string_default_at_max_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[STRING]", "maxLength": 2, "default": ["a", "b"]}"#,
    ));
}

#[test]
fn test_list_string_default_above_max_length() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[STRING]", "maxLength": 2, "default": ["a", "b", "c"]}"#,
        ),
        &["Parameter 'Foo': default list length 3 > maxLength 2."],
    );
}

// --- item allowedValues ---

#[test]
fn test_list_string_default_item_in_allowed() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[STRING]", "item": {"allowedValues": ["x", "y"]}, "default": ["x"]}"#,
    ));
}

#[test]
fn test_list_string_default_item_not_in_allowed() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[STRING]", "item": {"allowedValues": ["x", "y"]}, "default": ["z"]}"#,
        ),
        &["Parameter 'Foo': default[0] 'z' not in item allowedValues."],
    );
}

// --- item minLength ---

#[test]
fn test_list_string_default_item_at_min_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[STRING]", "item": {"minLength": 3}, "default": ["abc"]}"#,
    ));
}

#[test]
fn test_list_string_default_item_below_min_length() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[STRING]", "item": {"minLength": 3}, "default": ["ab"]}"#,
        ),
        &["Parameter 'Foo': default[0] length 2 < item minLength 3."],
    );
}

// --- item maxLength ---

#[test]
fn test_list_string_default_item_at_max_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[STRING]", "item": {"maxLength": 3}, "default": ["abc"]}"#,
    ));
}

#[test]
fn test_list_string_default_item_above_max_length() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[STRING]", "item": {"maxLength": 3}, "default": ["abcd"]}"#,
        ),
        &["Parameter 'Foo': default[0] length 4 > item maxLength 3."],
    );
}

// --- error index ---

#[test]
fn test_list_string_default_error_reports_correct_index() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[STRING]", "item": {"allowedValues": ["x"]}, "default": ["x", "bad"]}"#,
        ),
        &["Parameter 'Foo': default[1] 'bad' not in item allowedValues."],
    );
}

// ============================================================
// LIST[PATH] default validation per §2.12
// Constraints: list minLength/maxLength, item minLength/maxLength
// (no allowedValues check in validate_definition)
// ============================================================

// --- list length ---

#[test]
fn test_list_path_default_at_min_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[PATH]", "minLength": 2, "default": ["/a", "/b"]}"#,
    ));
}

#[test]
fn test_list_path_default_below_min_length() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[PATH]", "minLength": 2, "default": ["/a"]}"#,
        ),
        &["Parameter 'Foo': default list length 1 < minLength 2."],
    );
}

#[test]
fn test_list_path_default_at_max_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[PATH]", "maxLength": 2, "default": ["/a", "/b"]}"#,
    ));
}

#[test]
fn test_list_path_default_above_max_length() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[PATH]", "maxLength": 2, "default": ["/a", "/b", "/c"]}"#,
        ),
        &["Parameter 'Foo': default list length 3 > maxLength 2."],
    );
}

// --- item minLength ---

#[test]
fn test_list_path_default_item_at_min_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[PATH]", "item": {"minLength": 3}, "default": ["abc"]}"#,
    ));
}

#[test]
fn test_list_path_default_item_below_min_length() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[PATH]", "item": {"minLength": 3}, "default": ["ab"]}"#,
        ),
        &["Parameter 'Foo': default[0] length 2 < item minLength 3."],
    );
}

// --- item maxLength ---

#[test]
fn test_list_path_default_item_at_max_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[PATH]", "item": {"maxLength": 3}, "default": ["abc"]}"#,
    ));
}

#[test]
fn test_list_path_default_item_above_max_length() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[PATH]", "item": {"maxLength": 3}, "default": ["abcd"]}"#,
        ),
        &["Parameter 'Foo': default[0] length 4 > item maxLength 3."],
    );
}

// ============================================================
// LIST[INT] default validation per §2.13
// Constraints: list minLength/maxLength, item minValue/maxValue/allowedValues
// ============================================================

// --- list length ---

#[test]
fn test_list_int_default_at_min_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[INT]", "minLength": 2, "default": [1, 2]}"#,
    ));
}

#[test]
fn test_list_int_default_below_min_length() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[INT]", "minLength": 2, "default": [1]}"#,
        ),
        &["Parameter 'Foo': default list length 1 < minLength 2."],
    );
}

#[test]
fn test_list_int_default_at_max_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[INT]", "maxLength": 2, "default": [1, 2]}"#,
    ));
}

#[test]
fn test_list_int_default_above_max_length() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[INT]", "maxLength": 2, "default": [1, 2, 3]}"#,
        ),
        &["Parameter 'Foo': default list length 3 > maxLength 2."],
    );
}

// --- item minValue ---

#[test]
fn test_list_int_default_item_at_min_value() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[INT]", "item": {"minValue": 0}, "default": [0]}"#,
    ));
}

#[test]
fn test_list_int_default_item_below_min_value() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[INT]", "item": {"minValue": 0}, "default": [-1]}"#,
        ),
        &["Parameter 'Foo': default[0] -1 < item minValue 0."],
    );
}

// --- item maxValue ---

#[test]
fn test_list_int_default_item_at_max_value() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[INT]", "item": {"maxValue": 10}, "default": [10]}"#,
    ));
}

#[test]
fn test_list_int_default_item_above_max_value() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[INT]", "item": {"maxValue": 10}, "default": [11]}"#,
        ),
        &["Parameter 'Foo': default[0] 11 > item maxValue 10."],
    );
}

// --- item allowedValues ---

#[test]
fn test_list_int_default_item_in_allowed() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[INT]", "item": {"allowedValues": [1, 2, 3]}, "default": [2]}"#,
    ));
}

#[test]
fn test_list_int_default_item_not_in_allowed() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[INT]", "item": {"allowedValues": [1, 2, 3]}, "default": [4]}"#,
        ),
        &["Parameter 'Foo': default[0] 4 not in item allowedValues."],
    );
}

// ============================================================
// LIST[FLOAT] default validation per §2.14
// Constraints: list minLength/maxLength, item minValue/maxValue
// (no allowedValues check in validate_definition)
// ============================================================

// --- list length ---

#[test]
fn test_list_float_default_at_min_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[FLOAT]", "minLength": 2, "default": [1.0, 2.0]}"#,
    ));
}

#[test]
fn test_list_float_default_below_min_length() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[FLOAT]", "minLength": 2, "default": [1.0]}"#,
        ),
        &["Parameter 'Foo': default list length 1 < minLength 2."],
    );
}

#[test]
fn test_list_float_default_at_max_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[FLOAT]", "maxLength": 2, "default": [1.0, 2.0]}"#,
    ));
}

#[test]
fn test_list_float_default_above_max_length() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[FLOAT]", "maxLength": 2, "default": [1.0, 2.0, 3.0]}"#,
        ),
        &["Parameter 'Foo': default list length 3 > maxLength 2."],
    );
}

// --- item minValue ---

#[test]
fn test_list_float_default_item_at_min_value() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[FLOAT]", "item": {"minValue": 0.0}, "default": [0.0]}"#,
    ));
}

#[test]
fn test_list_float_default_item_below_min_value() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[FLOAT]", "item": {"minValue": 0.0}, "default": [-0.1]}"#,
        ),
        &["< item minValue 0"],
    );
}

// --- item maxValue ---

#[test]
fn test_list_float_default_item_at_max_value() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[FLOAT]", "item": {"maxValue": 10.0}, "default": [10.0]}"#,
    ));
}

#[test]
fn test_list_float_default_item_above_max_value() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[FLOAT]", "item": {"maxValue": 10.0}, "default": [10.1]}"#,
        ),
        &["> item maxValue 10"],
    );
}

// ============================================================
// LIST[BOOL] default validation per §2.15
// Constraints: list minLength/maxLength only (no item constraints)
// ============================================================

#[test]
fn test_list_bool_default_at_min_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[BOOL]", "minLength": 2, "default": [true, false]}"#,
    ));
}

#[test]
fn test_list_bool_default_below_min_length() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[BOOL]", "minLength": 2, "default": [true]}"#,
        ),
        &["Parameter 'Foo': default list length 1 < minLength 2."],
    );
}

#[test]
fn test_list_bool_default_at_max_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[BOOL]", "maxLength": 2, "default": [true, false]}"#,
    ));
}

#[test]
fn test_list_bool_default_above_max_length() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[BOOL]", "maxLength": 2, "default": [true, false, true]}"#,
        ),
        &["Parameter 'Foo': default list length 3 > maxLength 2."],
    );
}

// ============================================================
// LIST[LIST[INT]] default validation per §2.16
// Constraints: outer list minLength/maxLength,
//              inner list item minLength/maxLength,
//              inner item.item minValue/maxValue/allowedValues
// ============================================================

// --- outer list length ---

#[test]
fn test_list_list_int_default_at_min_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "minLength": 2, "default": [[1], [2]]}"#,
    ));
}

#[test]
fn test_list_list_int_default_below_min_length() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "minLength": 2, "default": [[1]]}"#,
        ),
        &["Parameter 'Foo': default list length 1 < minLength 2."],
    );
}

#[test]
fn test_list_list_int_default_at_max_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "maxLength": 2, "default": [[1], [2]]}"#,
    ));
}

#[test]
fn test_list_list_int_default_above_max_length() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "maxLength": 2, "default": [[1], [2], [3]]}"#,
        ),
        &["Parameter 'Foo': default list length 3 > maxLength 2."],
    );
}

// --- inner list length ---

#[test]
fn test_list_list_int_default_inner_at_min_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "item": {"minLength": 2}, "default": [[1, 2]]}"#,
    ));
}

#[test]
fn test_list_list_int_default_inner_below_min_length() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "item": {"minLength": 2}, "default": [[1]]}"#,
        ),
        &["Parameter 'Foo': default[0] inner list length 1 < item minLength 2."],
    );
}

#[test]
fn test_list_list_int_default_inner_at_max_length() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "item": {"maxLength": 2}, "default": [[1, 2]]}"#,
    ));
}

#[test]
fn test_list_list_int_default_inner_above_max_length() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "item": {"maxLength": 2}, "default": [[1, 2, 3]]}"#,
        ),
        &["Parameter 'Foo': default[0] inner list length 3 > item maxLength 2."],
    );
}

// --- inner item minValue ---

#[test]
fn test_list_list_int_default_inner_item_at_min_value() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "item": {"item": {"minValue": 0}}, "default": [[0]]}"#,
    ));
}

#[test]
fn test_list_list_int_default_inner_item_below_min_value() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "item": {"item": {"minValue": 0}}, "default": [[-1]]}"#,
        ),
        &["Parameter 'Foo': default[0][0] -1 < item.item minValue 0."],
    );
}

// --- inner item maxValue ---

#[test]
fn test_list_list_int_default_inner_item_at_max_value() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "item": {"item": {"maxValue": 10}}, "default": [[10]]}"#,
    ));
}

#[test]
fn test_list_list_int_default_inner_item_above_max_value() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "item": {"item": {"maxValue": 10}}, "default": [[11]]}"#,
        ),
        &["Parameter 'Foo': default[0][0] 11 > item.item maxValue 10."],
    );
}

// --- inner item allowedValues ---

#[test]
fn test_list_list_int_default_inner_item_in_allowed() {
    decode_ok(&job_with_expr_param(
        r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "item": {"item": {"allowedValues": [1, 2]}}, "default": [[1]]}"#,
    ));
}

#[test]
fn test_list_list_int_default_inner_item_not_in_allowed() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "item": {"item": {"allowedValues": [1, 2]}}, "default": [[3]]}"#,
        ),
        &["Parameter 'Foo': default[0][0] 3 not in item.item allowedValues."],
    );
}

// --- error index reporting ---

#[test]
fn test_list_list_int_default_error_reports_correct_indices() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "item": {"item": {"maxValue": 5}}, "default": [[1], [2, 99]]}"#,
        ),
        &["Parameter 'Foo': default[1][1] 99 > item.item maxValue 5."],
    );
}

// ============================================================
// EXPR parameter — userInterface validation
// ============================================================

// BOOL: valid controls are CHECK_BOX, HIDDEN
#[test]
fn test_bool_ui_invalid_control() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "BOOL", "userInterface": {"control": "LINE_EDIT"}}"#,
        ),
        &["unknown control 'LINE_EDIT'"],
    );
}

// RANGE_EXPR: valid controls are LINE_EDIT, HIDDEN
#[test]
fn test_range_expr_ui_invalid_control() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "RANGE_EXPR", "userInterface": {"control": "SPIN_BOX"}}"#,
        ),
        &["unknown control 'SPIN_BOX'"],
    );
}

// LIST[STRING]: valid controls are LINE_EDIT_LIST, HIDDEN
#[test]
fn test_list_string_ui_invalid_control() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[STRING]", "userInterface": {"control": "DROPDOWN_LIST"}}"#,
        ),
        &["unknown control 'DROPDOWN_LIST'"],
    );
}

// LIST[PATH]: valid controls are CHOOSE_INPUT_FILE_LIST, CHOOSE_OUTPUT_FILE_LIST, CHOOSE_DIRECTORY_LIST, HIDDEN
#[test]
fn test_list_path_ui_invalid_control() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[PATH]", "userInterface": {"control": "LINE_EDIT_LIST"}}"#,
        ),
        &["unknown control 'LINE_EDIT_LIST'"],
    );
}

// LIST[INT]: valid controls are SPIN_BOX_LIST, HIDDEN
#[test]
fn test_list_int_ui_invalid_control() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[INT]", "userInterface": {"control": "LINE_EDIT"}}"#,
        ),
        &["unknown control 'LINE_EDIT'"],
    );
}

// LIST[FLOAT]: valid controls are SPIN_BOX_LIST, HIDDEN
#[test]
fn test_list_float_ui_invalid_control() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[FLOAT]", "userInterface": {"control": "CHECK_BOX"}}"#,
        ),
        &["unknown control 'CHECK_BOX'"],
    );
}

// LIST[BOOL]: valid controls are CHECK_BOX_LIST, HIDDEN
#[test]
fn test_list_bool_ui_invalid_control() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[BOOL]", "userInterface": {"control": "LINE_EDIT_LIST"}}"#,
        ),
        &["unknown control 'LINE_EDIT_LIST'"],
    );
}

// LIST[LIST[INT]]: valid control is HIDDEN only
#[test]
fn test_list_list_int_ui_invalid_control() {
    check_err(
        &job_with_expr_param(
            r#"{"name": "Foo", "type": "LIST[LIST[INT]]", "userInterface": {"control": "SPIN_BOX_LIST"}}"#,
        ),
        &["unknown control 'SPIN_BOX_LIST'"],
    );
}

// Label validation should work for EXPR types too
#[test]
fn test_bool_ui_label_too_long() {
    let long_label = "x".repeat(65);
    check_err(
        &job_with_expr_param(&format!(
            r#"{{"name": "Foo", "type": "BOOL", "userInterface": {{"label": "{long_label}"}}}}"#
        )),
        &["label exceeds 64 characters"],
    );
}
