// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! An integer a template writes where the schema expects `<integer>` or
//! `<intstring>` is stored as an `i64`. A number outside the 64-bit signed
//! range is refused when the template is decoded, in every field and in every
//! form: a YAML integer, a YAML float with no fractional part, and a numeric
//! string. It is never rounded or saturated to a different number.

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

const PREFIX: &str = "Validation error: 'jobtemplate-2023-09' failed checks: ";
const TOO_SMALL_FLOAT: &str = "Cannot convert -9.223372036854776e+18 to int: a YAML integer \
                               below -9223372036854775808 is read as this float; write \
                               -9223372036854775808 as an integer";

/// Every spelling of a number just outside the range, and the whole message
/// each gets. The YAML parser reads 2^63 as a `u64`, and an integer past
/// `u64::MAX` or below `i64::MIN` as the nearest `f64`, which names it in the
/// message; an integer just below `i64::MIN` rounds to exactly -2^63.
const OUTSIDE: &[(&str, &str)] = &[
    (
        "9223372036854775808",
        "Cannot convert 9223372036854775808 to int: number too large to fit in target type",
    ),
    ("-9223372036854775809", TOO_SMALL_FLOAT),
    ("-9223372036854775808.0", TOO_SMALL_FLOAT),
    (
        "'9223372036854775808'",
        "Cannot convert '9223372036854775808' to int: number too large to fit in target type",
    ),
    (
        "'-9223372036854775809'",
        "Cannot convert '-9223372036854775809' to int: number too small to fit in target type",
    ),
    (
        "99999999999999999999",
        "Cannot convert 1e+20 to int: number too large to fit in target type",
    ),
    (
        "1.0e19",
        "Cannot convert 1e+19 to int: number too large to fit in target type",
    ),
];

fn assert_refused(template: &str, message: &str) {
    let err = decode(template).expect_err(&format!("expected a refusal for:\n{template}"));
    assert_eq!(err, format!("{PREFIX}{message}"), "for:\n{template}");
}

#[test]
fn int_default_outside_the_range_is_refused() {
    for (v, m) in OUTSIDE {
        assert_refused(&int_param(&format!("default: {v}")), m);
    }
}

#[test]
fn int_min_max_and_allowed_values_outside_the_range_are_refused() {
    for (v, m) in OUTSIDE {
        assert_refused(&int_param(&format!("minValue: {v}")), m);
        assert_refused(&int_param(&format!("maxValue: {v}")), m);
        assert_refused(&int_param(&format!("allowedValues: [1, {v}]")), m);
    }
}

#[test]
fn int_user_interface_step_outside_the_range_is_refused() {
    for (v, m) in OUTSIDE {
        assert_refused(
            &int_param(&format!(
                "userInterface: {{control: SPIN_BOX, singleStepDelta: {v}}}"
            )),
            m,
        );
    }
}

#[test]
fn list_int_values_outside_the_range_are_refused() {
    for (v, m) in OUTSIDE {
        assert_refused(&expr_param("LIST[INT]", &format!("default: [1, {v}]")), m);
        assert_refused(
            &expr_param("LIST[INT]", &format!("item: {{minValue: {v}}}")),
            m,
        );
        assert_refused(
            &expr_param("LIST[LIST[INT]]", &format!("default: [[1, {v}]]")),
            m,
        );
    }
}

#[test]
fn task_range_element_outside_the_range_is_refused() {
    for (v, m) in OUTSIDE {
        assert_refused(&int_range(&format!("[1, {v}]")), m);
    }
}

#[test]
fn chunk_count_outside_the_range_is_refused() {
    for (v, m) in OUTSIDE {
        assert_refused(&chunk_count(v), m);
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
        // The f64 next above -2^63, an integral float in the range.
        "-9.223372036854774784e18",
    ] {
        decode(&int_param(&format!("default: {v}")))
            .unwrap_or_else(|e| panic!("default {v} should decode: {e}"));
        decode(&int_range(&format!("[{v}]")))
            .unwrap_or_else(|e| panic!("range element {v} should decode: {e}"));
    }
}

#[test]
fn a_fractional_number_keeps_its_own_message() {
    for template in [int_param("default: 2.5"), int_range("[2.5]")] {
        assert_eq!(
            decode(&template).unwrap_err(),
            format!("{PREFIX}Expected integer, got float: 2.5")
        );
    }
}

/// A format string element of an `<IntRangeList>` is an `<intstring>` once it
/// resolves, read by the same rule as a template's numeric string: digits
/// outside the range are out of range, and anything else that is not an
/// integer, including digits that overflow before a letter, is not one.
#[test]
fn a_resolved_range_element_is_read_as_an_intstring() {
    use openjd_model::{create_job, preprocess_job_parameters, JobParameterInputValues};
    let template = "specificationVersion: jobtemplate-2023-09
name: T
parameterDefinitions:
- {name: V, type: STRING}
steps:
- name: S
  parameterSpace:
    taskParameterDefinitions:
    - {name: Frame, type: INT, range: ['{{Param.V}}']}
  script:
    actions:
      onRun: {command: foo}
";
    let jt = decode_job_template(yaml_val(template), None, &CallerLimits::default()).unwrap();
    let options = openjd_model::PathParameterOptions {
        job_template_dir: "/tmp/template",
        current_working_dir: "/tmp/cwd",
        allow_template_dir_walk_up: true,
        path_format: openjd_expr::PathFormat::Posix,
        allow_uri_path_values: true,
    };
    for (value, problem) in [
        (" 9223372036854775807 ", None),
        (
            "9223372036854775808",
            Some("is out of range for a 64-bit integer"),
        ),
        ("99999999999999999999abc", Some("is not an integer")),
    ] {
        let mut input = JobParameterInputValues::new();
        input.insert("V".into(), openjd_expr::ExprValue::String(value.into()));
        let params = preprocess_job_parameters(&jt, &input, &[], &options).unwrap();
        let result = create_job(&jt, &params, &jt.default_validation_context());
        match problem {
            None => {
                result.unwrap_or_else(|e| panic!("{value:?}: {e}"));
            }
            Some(problem) => {
                let err = result.err().unwrap().to_string();
                assert!(
                    err.contains(&format!("range[0]: value '{value}' {problem}")),
                    "{value:?}: {err}"
                );
            }
        }
    }
}
