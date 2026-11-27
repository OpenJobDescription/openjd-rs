// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! An integer a template writes where the schema expects `<integer>` or
//! `<intstring>` is stored as an `i64`. A number outside the 64-bit signed
//! range is refused when the template is decoded, in every field and in both
//! forms: a YAML integer, a YAML float with no fractional part, and a numeric
//! string. Before, a YAML number outside the range was cast with `as i64`,
//! which saturates, so `9223372036854775808` became `9223372036854775807`.

use openjd_model::{decode_job_template, CallerLimits};

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn decode(yaml: &str) -> Result<(), String> {
    decode_job_template(
        yaml_val(yaml),
        Some(&["EXPR", "TASK_CHUNKING"]),
        &CallerLimits::default(),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// A template with one INT parameter whose definition carries `fields`.
fn int_param(fields: &str) -> String {
    format!(
        "specificationVersion: jobtemplate-2023-09
name: T
parameterDefinitions:
- {{name: P, type: INT, {fields}}}
steps:
- name: S
  script:
    actions:
      onRun: {{command: foo}}
"
    )
}

/// A template with the EXPR extension and one parameter of type `ty`.
fn expr_param(ty: &str, fields: &str) -> String {
    format!(
        "specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: T
parameterDefinitions:
- {{name: P, type: '{ty}', {fields}}}
steps:
- name: S
  script:
    actions:
      onRun: {{command: foo}}
"
    )
}

/// A template whose one step has an INT task parameter with range `range`.
fn int_range(range: &str) -> String {
    format!(
        "specificationVersion: jobtemplate-2023-09
name: T
steps:
- name: S
  parameterSpace:
    taskParameterDefinitions:
    - {{name: Frame, type: INT, range: {range}}}
  script:
    actions:
      onRun: {{command: foo}}
"
    )
}

/// A TASK_CHUNKING template whose CHUNK[INT] parameter has
/// `defaultTaskCount: {count}`.
fn chunk_count(count: &str) -> String {
    format!(
        "specificationVersion: jobtemplate-2023-09
extensions: [TASK_CHUNKING]
name: T
steps:
- name: S
  parameterSpace:
    taskParameterDefinitions:
    - name: Frame
      type: CHUNK[INT]
      range: '1-10'
      chunks: {{defaultTaskCount: {count}, rangeConstraint: CONTIGUOUS}}
  script:
    actions:
      onRun: {{command: foo}}
"
    )
}

const OUT_OF_RANGE: &str = "is outside the 64-bit signed range";

/// Every spelling of a number just outside the range: 2^63 as a YAML integer,
/// 2^63 and -2^63-1 as numeric strings, a YAML integer beyond u64, a YAML
/// integer below i64::MIN, and a YAML float with no fractional part.
///
/// The YAML parser reads an integer below `i64::MIN` as an `f64`, so the
/// negative YAML integer here is -2^63-2048, the first one that `f64` holds
/// apart from -2^63. One closer to `i64::MIN`, such as -2^63-1, rounds to
/// -2^63 before this crate sees it (see specs/model/parameters.md).
const OUTSIDE: &[&str] = &[
    "9223372036854775808",
    "-9223372036854777856",
    "'9223372036854775808'",
    "'-9223372036854775809'",
    "99999999999999999999",
    "1.0e19",
];

fn assert_refused(template: &str) {
    let err = decode(template).expect_err(&format!("expected a refusal for:\n{template}"));
    assert!(
        err.contains(OUT_OF_RANGE),
        "expected {OUT_OF_RANGE:?} in the error, got:\n{err}\nfor:\n{template}"
    );
}

#[test]
fn int_default_outside_the_range_is_refused() {
    for v in OUTSIDE {
        assert_refused(&int_param(&format!("default: {v}")));
    }
}

#[test]
fn int_min_max_and_allowed_values_outside_the_range_are_refused() {
    for v in OUTSIDE {
        assert_refused(&int_param(&format!("minValue: {v}")));
        assert_refused(&int_param(&format!("maxValue: {v}")));
        assert_refused(&int_param(&format!("allowedValues: [1, {v}]")));
    }
}

#[test]
fn int_user_interface_step_outside_the_range_is_refused() {
    for v in OUTSIDE {
        assert_refused(&int_param(&format!(
            "userInterface: {{control: SPIN_BOX, singleStepDelta: {v}}}"
        )));
    }
}

#[test]
fn list_int_values_outside_the_range_are_refused() {
    for v in OUTSIDE {
        assert_refused(&expr_param("LIST[INT]", &format!("default: [1, {v}]")));
        assert_refused(&expr_param(
            "LIST[INT]",
            &format!("item: {{minValue: {v}}}"),
        ));
        assert_refused(&expr_param(
            "LIST[LIST[INT]]",
            &format!("default: [[1, {v}]]"),
        ));
    }
}

#[test]
fn task_range_element_outside_the_range_is_refused() {
    for v in OUTSIDE {
        assert_refused(&int_range(&format!("[1, {v}]")));
    }
}

#[test]
fn chunk_count_outside_the_range_is_refused() {
    for v in OUTSIDE {
        assert_refused(&chunk_count(v));
    }
}

#[test]
fn the_range_ends_and_integral_floats_are_accepted() {
    for v in [
        "9223372036854775807",
        "-9223372036854775808",
        "'9223372036854775807'",
        "' -9223372036854775808 '",
        "4.0",
        "-9.223372036854775808e18",
    ] {
        decode(&int_param(&format!("default: {v}")))
            .unwrap_or_else(|e| panic!("default {v} should decode: {e}"));
        decode(&int_range(&format!("[{v}]")))
            .unwrap_or_else(|e| panic!("range element {v} should decode: {e}"));
    }
}

#[test]
fn a_fractional_number_keeps_its_own_message() {
    let err = decode(&int_param("default: 2.5")).unwrap_err();
    assert!(err.contains("Expected integer, got float: 2.5"), "{err}");
    let err = decode(&int_range("[2.5]")).unwrap_err();
    assert!(err.contains("Expected integer, got float: 2.5"), "{err}");
}
