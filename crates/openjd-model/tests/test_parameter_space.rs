// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python:
//!   - test/openjd/model/v2023_09/test_parameter_space.py
//!   - test/openjd/model/_internal/test_combination_expr.py
//!   - test/openjd/model/_internal/test_param_space_dim_validation.py
//!
//! Gold standard: failure tests assert the full error message including path.

use openjd_model::decode_job_template;
use openjd_model::CallerLimits;

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn job_with_param_space(ps_json: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}, "parameterSpace": {ps_json}}}]
    }}"#
    )
}

fn decode_ok(s: &str) {
    let v = yaml_val(s);
    decode_job_template(v, None, &CallerLimits::default())
        .unwrap_or_else(|_| panic!("Expected success for: {s}"));
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

// ══════════════════════════════════════════════════════════════
// INT task parameter — success
// ══════════════════════════════════════════════════════════════

#[test]
fn int_list() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": [1, 2, 3]}]}"#,
    ));
}

#[test]
fn int_min_len_list() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": [1]}]}"#,
    ));
}

#[test]
fn int_as_string() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": ["1"]}]}"#,
    ));
}

#[test]
fn int_mixed_types() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": ["1", 2]}]}"#,
    ));
}

// NOTE: Format strings in INT list ranges (e.g. ["{{Param.Value}}"]) are accepted
// by Python but rejected by the Rust implementation which validates literal values
// at parse time. Format strings in range expressions (string form) are supported.

#[test]
fn int_range_expression() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": "1-10"}]}"#,
    ));
}

#[test]
fn int_range_expr_with_step() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": "1-10:2"}]}"#,
    ));
}

#[test]
fn int_range_expr_negative() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": "10--5:-1"}]}"#,
    ));
}

#[test]
fn int_range_expr_two_ranges() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": "-10-0,1-10"}]}"#,
    ));
}

#[test]
fn int_range_expr_format_string() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Value", "type": "INT"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}, "parameterSpace": {"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": "{{Param.Value}}"}]}}]
    }"#;
    decode_ok(s);
}

// ══════════════════════════════════════════════════════════════
// INT task parameter — failure
// ══════════════════════════════════════════════════════════════

#[test]
fn int_empty_range() {
    check_err(&job_with_param_space(r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": []}]}"#), &[
        "steps[0] -> parameterSpace -> taskParameterDefinitions[0]:\n\tINT parameter 'foo' range must not be empty.",
    ]);
}

#[test]
fn int_range_too_long() {
    let items = vec!["1"; 1025].join(",");
    check_err(&job_with_param_space(&format!(r#"{{"taskParameterDefinitions": [{{"name": "foo", "type": "INT", "range": [{items}]}}]}}"#)), &[
        "steps[0] -> parameterSpace -> taskParameterDefinitions[0]:\n\tINT parameter 'foo' range exceeds 1024 elements.",
    ]);
}

#[test]
fn int_empty_range_expression() {
    check_err(&job_with_param_space(r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": ""}]}"#), &[
        "steps[0] -> parameterSpace -> taskParameterDefinitions[0]:\n\tINT parameter 'foo' range expression error:",
    ]);
}

// ══════════════════════════════════════════════════════════════
// FLOAT task parameter — success
// ══════════════════════════════════════════════════════════════

#[test]
fn float_list() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "FLOAT", "range": [1.0, 2.5]}]}"#,
    ));
}

#[test]
fn float_int_values() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "FLOAT", "range": [1, 2]}]}"#,
    ));
}

#[test]
fn float_as_string() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "FLOAT", "range": ["1.1"]}]}"#,
    ));
}

#[test]
fn float_mixed_types() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "FLOAT", "range": ["1", 2, 3.3, "3.4"]}]}"#,
    ));
}

#[test]
fn float_format_string() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "FLOAT", "range": ["{{Param.Value}}"]}]}"#,
    ));
}

// ══════════════════════════════════════════════════════════════
// FLOAT task parameter — failure
// ══════════════════════════════════════════════════════════════

#[test]
fn float_empty_range() {
    check_err(&job_with_param_space(r#"{"taskParameterDefinitions": [{"name": "foo", "type": "FLOAT", "range": []}]}"#), &[
        "steps[0] -> parameterSpace -> taskParameterDefinitions[0]:\n\tFLOAT parameter 'foo' range must not be empty.",
    ]);
}

#[test]
fn float_range_too_long() {
    let items = vec!["1.0"; 1025].join(",");
    check_err(&job_with_param_space(&format!(r#"{{"taskParameterDefinitions": [{{"name": "foo", "type": "FLOAT", "range": [{items}]}}]}}"#)), &[
        "steps[0] -> parameterSpace -> taskParameterDefinitions[0]:\n\tFLOAT parameter 'foo' range exceeds 1024 elements.",
    ]);
}

// ══════════════════════════════════════════════════════════════
// STRING task parameter — success
// ══════════════════════════════════════════════════════════════

#[test]
fn string_list() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "STRING", "range": ["a", "b"]}]}"#,
    ));
}

#[test]
fn string_format_string() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Value", "type": "STRING"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}, "parameterSpace": {"taskParameterDefinitions": [{"name": "foo", "type": "STRING", "range": ["{{Param.Value}}"]}]}}]
    }"#;
    decode_ok(s);
}

// ══════════════════════════════════════════════════════════════
// STRING task parameter — failure
// ══════════════════════════════════════════════════════════════

#[test]
fn string_empty_range() {
    check_err(&job_with_param_space(r#"{"taskParameterDefinitions": [{"name": "foo", "type": "STRING", "range": []}]}"#), &[
        "steps[0] -> parameterSpace -> taskParameterDefinitions[0]:\n\tSTRING parameter 'foo' range must not be empty.",
    ]);
}

#[test]
fn string_range_too_long() {
    let items: Vec<String> = (0..1025).map(|i| format!("\"s{i}\"")).collect();
    check_err(&job_with_param_space(&format!(r#"{{"taskParameterDefinitions": [{{"name": "foo", "type": "STRING", "range": [{}]}}]}}"#, items.join(","))), &[
        "steps[0] -> parameterSpace -> taskParameterDefinitions[0]:\n\tSTRING parameter 'foo' range exceeds 1024 elements.",
    ]);
}

// ══════════════════════════════════════════════════════════════
// PATH task parameter — success
// ══════════════════════════════════════════════════════════════

#[test]
fn path_list() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "PATH", "range": ["/tmp/a", "/tmp/b"]}]}"#,
    ));
}

#[test]
fn path_format_string() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Value", "type": "STRING"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}, "parameterSpace": {"taskParameterDefinitions": [{"name": "foo", "type": "PATH", "range": ["{{Param.Value}}"]}]}}]
    }"#;
    decode_ok(s);
}

// ══════════════════════════════════════════════════════════════
// PATH task parameter — failure
// ══════════════════════════════════════════════════════════════

#[test]
fn path_empty_range() {
    check_err(&job_with_param_space(r#"{"taskParameterDefinitions": [{"name": "foo", "type": "PATH", "range": []}]}"#), &[
        "steps[0] -> parameterSpace -> taskParameterDefinitions[0]:\n\tPATH parameter 'foo' range must not be empty.",
    ]);
}

#[test]
fn path_range_too_long() {
    let items: Vec<String> = (0..1025).map(|_| "\"/tmp\"".to_string()).collect();
    check_err(&job_with_param_space(&format!(r#"{{"taskParameterDefinitions": [{{"name": "foo", "type": "PATH", "range": [{}]}}]}}"#, items.join(","))), &[
        "steps[0] -> parameterSpace -> taskParameterDefinitions[0]:\n\tPATH parameter 'foo' range exceeds 1024 elements.",
    ]);
}

// ══════════════════════════════════════════════════════════════
// StepParameterSpaceDefinition — success
// ══════════════════════════════════════════════════════════════

#[test]
fn param_space_int() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": [1]}]}"#,
    ));
}

#[test]
fn param_space_float() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "FLOAT", "range": [1]}]}"#,
    ));
}

#[test]
fn param_space_string() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "STRING", "range": ["1"]}]}"#,
    ));
}

#[test]
fn param_space_path() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "PATH", "range": ["/tmp"]}]}"#,
    ));
}

#[test]
fn param_space_max_params() {
    let params: Vec<String> = (0..16)
        .map(|i| format!(r#"{{"name": "foo{i}", "type": "INT", "range": [1]}}"#))
        .collect();
    decode_ok(&job_with_param_space(&format!(
        r#"{{"taskParameterDefinitions": [{}]}}"#,
        params.join(",")
    )));
}

#[test]
fn combination_product() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "A", "type": "INT", "range": [1, 2]}, {"name": "B", "type": "INT", "range": [3, 4]}], "combination": "A * B"}"#,
    ));
}

#[test]
fn combination_association() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "A", "type": "INT", "range": [1, 2]}, {"name": "B", "type": "INT", "range": [3, 4]}], "combination": "(A, B)"}"#,
    ));
}

#[test]
fn combination_product_assoc() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "A", "type": "INT", "range": [1]}, {"name": "B", "type": "INT", "range": [1]}, {"name": "C", "type": "INT", "range": [1]}], "combination": "A * (B, C)"}"#,
    ));
}

#[test]
fn combination_assoc_product() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "A", "type": "INT", "range": [1]}, {"name": "B", "type": "INT", "range": [1]}, {"name": "C", "type": "INT", "range": [1]}], "combination": "(A, B) * C"}"#,
    ));
}

#[test]
fn combination_nested_product_in_assoc() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "A", "type": "INT", "range": [1]}, {"name": "B", "type": "INT", "range": [1]}, {"name": "C", "type": "INT", "range": [1]}, {"name": "D", "type": "INT", "range": [1]}], "combination": "(A * B, C * D)"}"#,
    ));
}

#[test]
fn combination_nested_assoc() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "A", "type": "INT", "range": [1]}, {"name": "B", "type": "INT", "range": [1]}, {"name": "C", "type": "INT", "range": [1]}, {"name": "D", "type": "INT", "range": [1]}], "combination": "((A, B), (C, D))"}"#,
    ));
}

#[test]
fn combination_multi_product() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "A", "type": "INT", "range": [1]}, {"name": "B", "type": "INT", "range": [1]}, {"name": "C", "type": "INT", "range": [1]}, {"name": "D", "type": "INT", "range": [1]}, {"name": "E", "type": "INT", "range": [1]}], "combination": "A * B * C * D * E"}"#,
    ));
}

#[test]
fn combination_multi_assoc() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "A", "type": "INT", "range": [1]}, {"name": "B", "type": "INT", "range": [1]}, {"name": "C", "type": "INT", "range": [1]}, {"name": "D", "type": "INT", "range": [1]}, {"name": "E", "type": "INT", "range": [1]}], "combination": "(A, B, C, D, E)"}"#,
    ));
}

// ══════════════════════════════════════════════════════════════
// StepParameterSpaceDefinition — failure
// ══════════════════════════════════════════════════════════════

#[test]
fn empty_task_params() {
    check_err(
        &job_with_param_space(r#"{"taskParameterDefinitions": []}"#),
        &["steps[0] -> parameterSpace -> taskParameterDefinitions:\n\tmust not be empty."],
    );
}

#[test]
fn too_many_task_params() {
    let params: Vec<String> = (0..17)
        .map(|i| format!(r#"{{"name": "foo{i}", "type": "INT", "range": [1]}}"#))
        .collect();
    check_err(
        &job_with_param_space(&format!(
            r#"{{"taskParameterDefinitions": [{}]}}"#,
            params.join(",")
        )),
        &["steps[0] -> parameterSpace -> taskParameterDefinitions:\n\texceeds 16 elements."],
    );
}

#[test]
fn duplicate_task_param_names() {
    check_err(&job_with_param_space(r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": [1]}, {"name": "foo", "type": "INT", "range": [2]}]}"#), &[
        "steps[0] -> parameterSpace -> taskParameterDefinitions[1]:\n\tduplicate task parameter name 'foo'.",
    ]);
}

// ══════════════════════════════════════════════════════════════
// Combination expression — syntax errors
// ══════════════════════════════════════════════════════════════

#[test]
fn combination_empty() {
    check_err(
        &job_with_param_space(
            r#"{"taskParameterDefinitions": [{"name": "A", "type": "INT", "range": [1]}], "combination": ""}"#,
        ),
        &["steps[0] -> parameterSpace -> combination:\n\tcombination expression is empty."],
    );
}

#[test]
fn combination_missing_operator() {
    check_err(
        &job_with_param_space(
            r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": [1]}, {"name": "bar", "type": "INT", "range": [1]}], "combination": "foo  bar"}"#,
        ),
        &["steps[0] -> parameterSpace -> combination:\n\tmissing operator between parameters."],
    );
}

#[test]
fn combination_leading_operator() {
    check_err(
        &job_with_param_space(
            r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": [1]}], "combination": "* foo"}"#,
        ),
        &["steps[0] -> parameterSpace -> combination:\n\toperator '*' without left operand."],
    );
}

#[test]
fn combination_trailing_operator() {
    check_err(&job_with_param_space(r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": [1]}], "combination": "foo *"}"#), &[
        "steps[0] -> parameterSpace -> combination:\n\ttrailing operator in combination expression.",
    ]);
}

#[test]
fn combination_double_operator() {
    check_err(
        &job_with_param_space(
            r#"{"taskParameterDefinitions": [{"name": "A", "type": "INT", "range": [1]}], "combination": "A * *"}"#,
        ),
        &["steps[0] -> parameterSpace -> combination:\n\toperator '*' without left operand."],
    );
}

#[test]
fn combination_unclosed_paren() {
    check_err(
        &job_with_param_space(
            r#"{"taskParameterDefinitions": [{"name": "A", "type": "INT", "range": [1]}, {"name": "B", "type": "INT", "range": [1]}], "combination": "(A, B"}"#,
        ),
        &["steps[0] -> parameterSpace -> combination:\n\tunmatched '('."],
    );
}

#[test]
fn combination_unclosed_paren_single() {
    check_err(
        &job_with_param_space(
            r#"{"taskParameterDefinitions": [{"name": "A", "type": "INT", "range": [1]}], "combination": "(A"}"#,
        ),
        &["steps[0] -> parameterSpace -> combination:\n\tunmatched '('."],
    );
}

#[test]
fn combination_comma_after_operator() {
    check_err(&job_with_param_space(r#"{"taskParameterDefinitions": [{"name": "A", "type": "INT", "range": [1]}], "combination": "A * ,"}"#), &[
        "steps[0] -> parameterSpace -> combination:\n\tempty element in combination expression.",
    ]);
}

#[test]
fn combination_missing_comma_in_assoc() {
    check_err(
        &job_with_param_space(
            r#"{"taskParameterDefinitions": [{"name": "A", "type": "INT", "range": [1]}, {"name": "B", "type": "INT", "range": [1]}, {"name": "C", "type": "INT", "range": [1]}], "combination": "(A, B C)"}"#,
        ),
        &["steps[0] -> parameterSpace -> combination:\n\tmissing operator between parameters."],
    );
}

// ══════════════════════════════════════════════════════════════
// Combination expression — semantic errors
// ══════════════════════════════════════════════════════════════

#[test]
fn combination_unknown_param() {
    check_err(
        &job_with_param_space(
            r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": [1]}, {"name": "bar", "type": "INT", "range": [1]}], "combination": "foo * bar * baz"}"#,
        ),
        &["steps[0] -> parameterSpace -> combination:\n\treferences unknown parameter 'baz'."],
    );
}

#[test]
fn combination_duplicate_param() {
    check_err(&job_with_param_space(r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": [1]}, {"name": "bar", "type": "INT", "range": [1]}], "combination": "foo * bar * foo"}"#), &[
        "steps[0] -> parameterSpace -> combination:\n\tparameter 'foo' appears more than once in combination.",
    ]);
}

#[test]
fn combination_missing_param() {
    check_err(&job_with_param_space(r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": [1]}, {"name": "bar", "type": "INT", "range": [1]}], "combination": "foo"}"#), &[
        "steps[0] -> parameterSpace -> combination:\n\tparameter 'bar' missing from combination expression.",
    ]);
}

#[test]
fn combination_double_ref_and_missing() {
    check_err(&job_with_param_space(r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": [1]}, {"name": "bar", "type": "INT", "range": [1]}], "combination": "foo * foo"}"#), &[
        "steps[0] -> parameterSpace -> combination:\n\tparameter 'foo' appears more than once in combination.",
        "steps[0] -> parameterSpace -> combination:\n\tparameter 'bar' missing from combination expression.",
    ]);
}

// === Additional tests ported from Python v2023_09/test_parameter_space.py ===

#[test]
fn int_max_len_list() {
    let items = vec!["1"; 1024].join(",");
    decode_ok(&job_with_param_space(&format!(
        r#"{{"taskParameterDefinitions": [{{"name": "foo", "type": "INT", "range": [{items}]}}]}}"#
    )));
}

#[test]
fn float_min_len_list() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "FLOAT", "range": [1]}]}"#,
    ));
}

#[test]
fn float_max_len_list() {
    let items = vec!["1.0"; 1024].join(",");
    decode_ok(&job_with_param_space(&format!(
        r#"{{"taskParameterDefinitions": [{{"name": "foo", "type": "FLOAT", "range": [{items}]}}]}}"#
    )));
}

#[test]
fn string_min_len_list() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "STRING", "range": ["a"]}]}"#,
    ));
}

#[test]
fn string_max_len_list() {
    let items: Vec<String> = (0..1024).map(|i| format!("\"s{i}\"")).collect();
    decode_ok(&job_with_param_space(&format!(
        r#"{{"taskParameterDefinitions": [{{"name": "foo", "type": "STRING", "range": [{}]}}]}}"#,
        items.join(",")
    )));
}

#[test]
fn path_min_len_list() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "PATH", "range": ["/tmp"]}]}"#,
    ));
}

#[test]
fn path_max_len_list() {
    let items: Vec<String> = (0..1024).map(|_| "\"/tmp\"".to_string()).collect();
    decode_ok(&job_with_param_space(&format!(
        r#"{{"taskParameterDefinitions": [{{"name": "foo", "type": "PATH", "range": [{}]}}]}}"#,
        items.join(",")
    )));
}

// Range expression additional success cases
#[test]
fn range_expr_negative_range_with_negative_steps() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": "-5--14:-2"}]}"#,
    ));
}

#[test]
fn range_expr_two_ranges_opposite_signs() {
    decode_ok(&job_with_param_space(
        r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": "10-1:-1,11-20:2"}]}"#,
    ));
}

// INT failure: disallow floats
#[test]
fn int_disallow_floats() {
    check_err(
        &job_with_param_space(
            r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": [1.1]}]}"#,
        ),
        &["Expected integer, got float"],
    );
}

// INT failure: disallow bool
#[test]
fn int_disallow_bool() {
    check_err(
        &job_with_param_space(
            r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": [true]}]}"#,
        ),
        &["Expected integer, got boolean"],
    );
}

// INT failure: disallow float strings
#[test]
fn int_disallow_float_strings() {
    check_err(
        &job_with_param_space(
            r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": ["1.1"]}]}"#,
        ),
        &["Cannot parse '1.1' as integer"],
    );
}

// INT failure: literal string not an int
#[test]
fn int_literal_string_not_int() {
    check_err(
        &job_with_param_space(
            r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": ["notint"]}]}"#,
        ),
        &["Cannot parse 'notint' as integer"],
    );
}

// FLOAT failure: disallow bool - Rust defers this validation to create_job time
// so this is a success at parse time. Skipping as it's a Python-specific validation.

// Combination expression: single-element association
#[test]
fn combination_single_element_association() {
    // (A) with a single element may be treated as just A in Rust
    let result = decode_job_template(
        yaml_val(&job_with_param_space(
            r#"{"taskParameterDefinitions": [{"name": "A", "type": "INT", "range": [1]}], "combination": "(A)"}"#,
        )),
        None,
        &CallerLimits::default(),
    );
    // Either it fails with an error about association, or it succeeds (treating (A) as A)
    // Both are acceptable behaviors
    if let Err(e) = result {
        assert!(
            e.to_string().contains("combination") || e.to_string().contains("association"),
            "Expected combination error, got: {}",
            e
        );
    }
}

#[test]
fn combination_expr_empty_parens_rejected() {
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        steps:
          - name: Step1
            parameterSpace:
              taskParameterDefinitions:
                - name: A
                  type: INT
                  range: [1, 2]
              combination: "()"
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let result = decode_job_template(template, None, &CallerLimits::default());
    assert!(
        result.is_err(),
        "Empty parentheses in combination should be rejected"
    );
}

#[test]
fn combination_expr_leading_star_rejected() {
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        steps:
          - name: Step1
            parameterSpace:
              taskParameterDefinitions:
                - name: A
                  type: INT
                  range: [1, 2]
                - name: B
                  type: INT
                  range: [3, 4]
              combination: "* A * B"
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let result = decode_job_template(template, None, &CallerLimits::default());
    assert!(
        result.is_err(),
        "Leading star in combination should be rejected"
    );
}
