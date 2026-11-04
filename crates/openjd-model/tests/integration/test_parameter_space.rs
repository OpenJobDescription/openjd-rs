// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

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

/// Decode `s` and assert that it fails with exactly `expected`.
fn check_err_exact(s: &str, expected: &str) {
    let err = decode_job_template(yaml_val(s), None, &CallerLimits::default())
        .expect_err(&format!("Expected error for: {s}"));
    assert_eq!(err.to_string(), expected);
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

/// A step with one task parameter, under a job that defines the INT job
/// parameter `Value`, so a range element can reference `{{Param.Value}}`.
fn job_with_int_value_param(task_param_json: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{{"name": "Value", "type": "INT", "default": 10}}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}, "parameterSpace": {{"taskParameterDefinitions": [{task_param_json}]}}}}]
    }}"#
    )
}

// Template Schemas §3.4.1.1: an `<IntRangeList>` element may be a
// `<TaskParameterStringValue>`, a format string that resolves to an integer.

#[test]
fn int_format_string() {
    decode_ok(&job_with_int_value_param(
        r#"{"name": "foo", "type": "INT", "range": ["{{Param.Value}}"]}"#,
    ));
}

#[test]
fn int_mixed_item_types() {
    decode_ok(&job_with_int_value_param(
        r#"{"name": "foo", "type": "INT", "range": [1, "2", "{{Param.Value}}"]}"#,
    ));
}

#[test]
fn int_format_string_surrounding_whitespace() {
    decode_ok(&job_with_int_value_param(
        r#"{"name": "foo", "type": "INT", "range": [" {{Param.Value}} ", "{{ Param.Value }}"]}"#,
    ));
}

#[test]
fn int_format_string_with_surrounding_text() {
    decode_ok(&job_with_int_value_param(
        r#"{"name": "foo", "type": "INT", "range": ["1{{Param.Value}}"]}"#,
    ));
}

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
fn int_format_string_undefined_variable() {
    check_err_exact(
        &job_with_int_value_param(
            r#"{"name": "foo", "type": "INT", "range": [1, "{{Param.Nope}}"]}"#,
        ),
        "Model validation error: 1 validation error for JobTemplate\nsteps[0] -> parameterSpace -> taskParameterDefinitions[0] -> range[1]:\n\tFailed to parse interpolation expression at [0, 14]. Undefined variable: 'Param.Nope'. Did you mean: Param.Value\n  Param.Nope\n  ~~~~~~^~~~",
    );
}

/// Template validation types a format string element as a list item
/// (Expression Language §1.3.2), with the same target job creation uses, so a
/// value that can never coerce to the element type fails `check`.
#[test]
fn numeric_range_list_element_static_type_error() {
    for (param_type, target) in [
        ("INT", "int | list[int] | nulltype"),
        ("CHUNK[INT]", "int | list[int] | nulltype"),
        ("FLOAT", "float | int | list[float] | nulltype"),
    ] {
        let (extensions, chunks) = if param_type == "CHUNK[INT]" {
            (
                r#""EXPR", "TASK_CHUNKING""#,
                r#", "chunks": {"defaultTaskCount": 1, "rangeConstraint": "NONCONTIGUOUS"}"#,
            )
        } else {
            (r#""EXPR""#, "")
        };
        let template = format!(
            r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "extensions": [{extensions}],
            "name": "Test",
            "parameterDefinitions": [{{"name": "B", "type": "BOOL", "default": true}}],
            "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}, "parameterSpace": {{"taskParameterDefinitions": [
                {{"name": "foo", "type": "{param_type}", "range": [1, "{{{{ null }}}}", "{{{{ [2] }}}}", "{{{{ Param.B }}}}", "{{{{ ['a'] }}}}"]{chunks}}}
            ]}}}}]
        }}"#
        );
        let err = decode_job_template(
            yaml_val(&template),
            Some(&["EXPR", "TASK_CHUNKING"]),
            &CallerLimits::default(),
        )
        .expect_err(param_type)
        .to_string();
        assert_eq!(
            err,
            format!(
                "Model validation error: 2 validation errors for JobTemplate\n\
                 steps[0] -> parameterSpace -> taskParameterDefinitions[0] -> range[3]:\n\
                 \tFailed to parse interpolation expression at [0, 13]. Cannot coerce bool to {target}\n  Param.B\n  ~~~~~~^\n\
                 steps[0] -> parameterSpace -> taskParameterDefinitions[0] -> range[4]:\n\
                 \tFailed to parse interpolation expression at [0, 11]. Cannot coerce list[string] to {target}\n  ['a']\n  ^~~~~"
            ),
            "{param_type}"
        );
    }
}

/// Element 0 is checked like any other, for INT, CHUNK[INT] and FLOAT.
#[test]
fn int_range_list_first_element_validated() {
    for (param_type, extensions, chunks) in [
        ("INT", "", ""),
        ("FLOAT", "", ""),
        (
            "CHUNK[INT]",
            r#""extensions": ["TASK_CHUNKING"],"#,
            r#", "chunks": {"defaultTaskCount": 1, "rangeConstraint": "NONCONTIGUOUS"}"#,
        ),
    ] {
        let template = format!(
            r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            {extensions}
            "name": "Test",
            "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}, "parameterSpace": {{"taskParameterDefinitions": [
                {{"name": "foo", "type": "{param_type}", "range": ["{{{{Param.Nope}}}}"]{chunks}}}
            ]}}}}]
        }}"#
        );
        let err = decode_job_template(
            yaml_val(&template),
            Some(&["TASK_CHUNKING"]),
            &CallerLimits::default(),
        )
        .expect_err(param_type)
        .to_string();
        assert_eq!(
            err,
            "Model validation error: 1 validation error for JobTemplate\n\
             steps[0] -> parameterSpace -> taskParameterDefinitions[0] -> range[0]:\n\
             \tFailed to parse interpolation expression at [0, 14]. Undefined variable: 'Param.Nope'.\n  Param.Nope\n  ~~~~~~^~~~",
            "{param_type}"
        );
    }
}

/// Range elements are validated in template scope (specs/model/validation.md
/// "Template scope"), where host functions such as `apply_path_mapping` are
/// not available.
#[test]
fn numeric_range_format_strings_use_template_scope() {
    for (param_type, range, path, at) in [
        (
            "INT",
            r#"[1, "1{{ apply_path_mapping('/a') }}"]"#,
            "range[1]",
            "[1, 31]",
        ),
        (
            "CHUNK[INT]",
            r#"[1, "1{{ apply_path_mapping('/a') }}"]"#,
            "range[1]",
            "[1, 31]",
        ),
        (
            "FLOAT",
            r#"[1, "1{{ apply_path_mapping('/a') }}"]"#,
            "range[1]",
            "[1, 31]",
        ),
        (
            "INT",
            r#""1-{{ apply_path_mapping('/a') }}""#,
            "range",
            "[2, 32]",
        ),
    ] {
        let (extensions, chunks) = if param_type == "CHUNK[INT]" {
            (
                r#""EXPR", "TASK_CHUNKING""#,
                r#", "chunks": {"defaultTaskCount": 1, "rangeConstraint": "NONCONTIGUOUS"}"#,
            )
        } else {
            (r#""EXPR""#, "")
        };
        let template = format!(
            r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "extensions": [{extensions}],
            "name": "Test",
            "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}, "parameterSpace": {{"taskParameterDefinitions": [
                {{"name": "foo", "type": "{param_type}", "range": {range}{chunks}}}
            ]}}}}]
        }}"#
        );
        let err = decode_job_template(
            yaml_val(&template),
            Some(&["EXPR", "TASK_CHUNKING"]),
            &CallerLimits::default(),
        )
        .expect_err(param_type)
        .to_string();
        assert_eq!(
            err,
            format!(
                "Model validation error: 1 validation error for JobTemplate\n\
                 steps[0] -> parameterSpace -> taskParameterDefinitions[0] -> {path}:\n\
                 \tFailed to parse interpolation expression at {at}. Unknown function: 'apply_path_mapping'\n  \
                 apply_path_mapping('/a')\n  ^~~~~~~~~~~~~~~~~~~~~~~~"
            ),
            "{param_type} {range}");
    }
}

#[test]
fn int_format_string_unterminated() {
    check_err_exact(
        &job_with_int_value_param(r#"{"name": "foo", "type": "INT", "range": ["{{Param.Value"]}"#),
        "Validation error: 'jobtemplate-2023-09' failed checks: Failed to parse interpolation expression at [0, 13]. Reason: Braces mismatch.\n  {{Param.Value\n  ^~",
    );
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
    decode_ok(&job_with_int_value_param(
        r#"{"name": "foo", "type": "FLOAT", "range": ["{{Param.Value}}"]}"#,
    ));
}

#[test]
fn float_mixed_item_types() {
    decode_ok(&job_with_int_value_param(
        r#"{"name": "foo", "type": "FLOAT", "range": [1, "2", 3.3, "3.4", " {{Param.Value}} "]}"#,
    ));
}

#[test]
fn float_format_string_undefined_variable() {
    check_err_exact(
        &job_with_int_value_param(
            r#"{"name": "foo", "type": "FLOAT", "range": [1.5, "{{Param.Nope}}"]}"#,
        ),
        "Model validation error: 1 validation error for JobTemplate\nsteps[0] -> parameterSpace -> taskParameterDefinitions[0] -> range[1]:\n\tFailed to parse interpolation expression at [0, 14]. Undefined variable: 'Param.Nope'. Did you mean: Param.Value\n  Param.Nope\n  ~~~~~~^~~~",
    );
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

// INT failure: disallow floats, positive or negative
#[test]
fn int_disallow_floats() {
    for (value, shown) in [("1.1", "1.1"), ("-4.5", "-4.5"), ("-0.5", "-0.5")] {
        check_err_exact(
            &job_with_param_space(&format!(
                r#"{{"taskParameterDefinitions": [{{"name": "foo", "type": "INT", "range": [1, {value}]}}]}}"#
            )),
            &format!(
                "Validation error: 'jobtemplate-2023-09' failed checks: Expected integer, got float: {shown}"
            ),
        );
    }
}

/// An `<IntRangeList>` element that is neither a number nor a string fails
/// to decode, with a message for its kind.
#[test]
fn int_range_list_element_of_wrong_kind() {
    for (element, message) in [
        ("null", "Expected integer, got null"),
        ("true", "Expected integer, got boolean"),
        ("[1]", "Expected integer or string"),
        (r#"{"a": 1}"#, "Expected integer or string"),
    ] {
        check_err_exact(
            &job_with_param_space(&format!(
                r#"{{"taskParameterDefinitions": [{{"name": "foo", "type": "INT", "range": [1, {element}]}}]}}"#
            )),
            &format!("Validation error: 'jobtemplate-2023-09' failed checks: {message}"),
        );
    }
}

// INT failure: disallow bool
#[test]
fn int_disallow_bool() {
    check_err_exact(
        &job_with_param_space(
            r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": [true]}]}"#,
        ),
        "Validation error: 'jobtemplate-2023-09' failed checks: Expected integer, got boolean",
    );
}

// INT failure: disallow float strings
#[test]
fn int_disallow_float_strings() {
    check_err_exact(
        &job_with_param_space(
            r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": ["1.1"]}]}"#,
        ),
        "Validation error: 'jobtemplate-2023-09' failed checks: Cannot parse '1.1' as integer",
    );
}

// INT failure: only `{{` starts a format string, so a string with single
// braces is a literal and must parse as an integer.
#[test]
fn int_literal_string_with_single_braces_not_int() {
    for literal in ["{5}", "1}}"] {
        check_err_exact(
            &job_with_param_space(&format!(
                r#"{{"taskParameterDefinitions": [{{"name": "foo", "type": "INT", "range": ["{literal}"]}}]}}"#
            )),
            &format!(
                "Validation error: 'jobtemplate-2023-09' failed checks: Cannot parse '{literal}' as integer"
            ),
        );
    }
}

// INT failure: literal string not an int
#[test]
fn int_literal_string_not_int() {
    check_err_exact(
        &job_with_param_space(
            r#"{"taskParameterDefinitions": [{"name": "foo", "type": "INT", "range": ["notint"]}]}"#,
        ),
        "Validation error: 'jobtemplate-2023-09' failed checks: Cannot parse 'notint' as integer",
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

// ══════════════════════════════════════════════════════════════
// Task parameter range length — §3.4 caps the list forms only
// ══════════════════════════════════════════════════════════════

/// §3.4.1.1.1 `<IntRangeExpr>` states no element cap, so the expansion of a
/// range expression must not be bounded by `max_task_param_range_len`. The
/// form exists to express frame ranges, which routinely exceed 1024 values.
#[test]
fn int_range_expression_expansion_is_not_capped() {
    for range in ["1-1024", "1-1025", "1-5000", "1-100000:2"] {
        decode_ok(&job_with_param_space(&format!(
            r#"{{"taskParameterDefinitions": [{{"name": "Frame", "type": "INT", "range": "{range}"}}]}}"#
        )));
    }
}

/// A malformed range expression is still rejected — dropping the length cap
/// must not drop grammar validation.
#[test]
fn int_range_expression_grammar_is_still_validated() {
    check_err(
        &job_with_param_space(
            r#"{"taskParameterDefinitions": [{"name": "Frame", "type": "INT", "range": "1-10,5-15"}]}"#,
        ),
        &["range expression error"],
    );
}

/// §3.4.1.1 item 4 caps `<IntRangeList>` at 1024 elements. That cap stays.
#[test]
fn int_range_list_is_still_capped() {
    let values: Vec<String> = (0..1025).map(|v| v.to_string()).collect();
    check_err(
        &job_with_param_space(&format!(
            r#"{{"taskParameterDefinitions": [{{"name": "Frame", "type": "INT", "range": [{}]}}]}}"#,
            values.join(",")
        )),
        &["range exceeds 1024 elements"],
    );
}

/// §3.4.1.3 caps `<StringRangeList>` too.
#[test]
fn string_range_list_is_still_capped() {
    let values: Vec<String> = (0..1025).map(|v| format!("\"v{v}\"")).collect();
    check_err(
        &job_with_param_space(&format!(
            r#"{{"taskParameterDefinitions": [{{"name": "S", "type": "STRING", "range": [{}]}}]}}"#,
            values.join(",")
        )),
        &["range exceeds 1024 elements"],
    );
}

#[test]
fn chunk_int_literal_range_expr_value_bound() {
    // A literal CHUNK[INT] range is checked against the 2^62 value bound
    // during template validation, with the template path, at either end.
    let template = |range: &str| {
        format!(
            r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["TASK_CHUNKING"],
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}},
            "parameterSpace": {{"taskParameterDefinitions": [{{"name": "Frame",
                "type": "CHUNK[INT]", "range": "{range}",
                "chunks": {{"defaultTaskCount": 2, "rangeConstraint": "CONTIGUOUS"}}}}]}}}}]
    }}"#
        )
    };
    let decode = |range: &str| {
        decode_job_template(
            yaml_val(&template(range)),
            Some(&["TASK_CHUNKING"]),
            &CallerLimits::default(),
        )
    };
    for (range, value) in [
        ("1-3,4611686018427387904", "4611686018427387904"),
        ("-4611686018427387904,1-3", "-4611686018427387904"),
    ] {
        assert_eq!(
            decode(range).unwrap_err().to_string(),
            format!(
                "Model validation error: 1 validation error for JobTemplate\n\
                 steps[0] -> parameterSpace -> taskParameterDefinitions[0]:\n\
                 \tCHUNK[INT] parameter 'Frame' range expression error: value {value} \
                 exceeds the CHUNK[INT] range value bound (magnitude must be below 2^62)"
            )
        );
    }
    // Magnitudes up to 2^62 - 1 are accepted.
    decode("-4611686018427387903,4611686018427387903").unwrap();
}
