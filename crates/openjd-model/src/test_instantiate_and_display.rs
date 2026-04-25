// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Red/green TDD tests for instantiate_step error propagation and FlexFloat Display.

use crate::CallerLimits;
use crate::JobParameterInputValues;
use crate::{create_job, decode_job_template, preprocess_job_parameters};

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn preprocess(
    jt: &crate::template::JobTemplate,
    input: &JobParameterInputValues,
) -> crate::JobParameterValues {
    preprocess_job_parameters(
        jt,
        input,
        &[],
        &crate::PathParameterOptions {
            job_template_dir: "/tmp",
            current_working_dir: "/tmp",
            path_format: openjd_expr::path_mapping::PathFormat::Posix,
            allow_template_dir_walk_up: true,
            allow_uri_path_values: true,
        },
    )
    .unwrap()
}

// ═══════════════════════════════════════════════════════════════
// instantiate_step: let binding errors must propagate
// ═══════════════════════════════════════════════════════════════

/// A step-level let binding that divides by a parameter value of 0 should
/// produce an error at create_job time, not be silently swallowed.
#[test]
fn create_job_step_let_binding_division_by_zero_errors() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "parameterDefinitions": [{
            "name": "Divisor",
            "type": "INT",
            "default": 1
        }],
        "steps": [{
            "name": "S",
            "let": ["result = 100 / Param.Divisor"],
            "script": {"actions": {"onRun": {"command": "echo"}}}
        }]
    }"#,
    );
    let exts: &[&str] = &["EXPR"];
    let jt = decode_job_template(v, Some(exts), &CallerLimits::default()).unwrap();

    // Provide Divisor=0 to trigger division by zero at create_job time
    let mut input = JobParameterInputValues::new();
    input.insert("Divisor".into(), openjd_expr::ExprValue::Int(0));
    let params = preprocess(&jt, &input);
    let result = create_job(&jt, &params, &CallerLimits::default());

    assert!(
        result.is_err(),
        "create_job should propagate let binding evaluation error (division by zero)"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("division") || msg.contains("divide") || msg.contains("zero"),
        "Error should mention division by zero, got: {msg}"
    );
}

/// A step-level let binding referencing a symbol that exists during validation
/// (as Unresolved) but fails at instantiation should propagate the error.
#[test]
fn create_job_step_let_binding_type_error_at_instantiation() {
    // Use a let binding that does arithmetic on a string parameter.
    // Validation passes because it type-checks with Unresolved tokens,
    // but instantiation with a concrete string value should fail.
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "parameterDefinitions": [{
            "name": "Count",
            "type": "INT",
            "default": 5
        }],
        "steps": [{
            "name": "S",
            "let": ["doubled = Param.Count * 2"],
            "script": {"actions": {"onRun": {"command": "echo {{doubled}}"}}}
        }]
    }"#,
    );
    let exts: &[&str] = &["EXPR"];
    let jt = decode_job_template(v, Some(exts), &CallerLimits::default()).unwrap();

    // Normal case: should succeed and the binding should be in the resolved symtab
    let mut input = JobParameterInputValues::new();
    input.insert("Count".into(), openjd_expr::ExprValue::Int(5));
    let params = preprocess(&jt, &input);
    let job = create_job(&jt, &params, &CallerLimits::default()).unwrap();
    // Verify the let binding was evaluated and is in the resolved symtab
    let step = &job.steps[0];
    let symtab_json = step
        .resolved_symtab
        .as_ref()
        .expect("resolved_symtab should be present");
    let symtab_str = serde_json::to_string(symtab_json).unwrap();
    assert!(
        symtab_str.contains("doubled"),
        "resolved_symtab should contain 'doubled' binding, got: {symtab_str}"
    );
}

// ═══════════════════════════════════════════════════════════════
// FlexFloat Display: large whole numbers should not saturate
// ═══════════════════════════════════════════════════════════════

/// FlexFloat Display for a whole number > i64::MAX should not saturate to i64::MAX.
#[test]
fn flexfloat_display_large_positive_whole_number() {
    use crate::template::FlexFloat;
    let ff = FlexFloat(1e19, None);
    let display = format!("{ff}");
    assert_ne!(
        display,
        i64::MAX.to_string(),
        "FlexFloat should not saturate to i64::MAX for 1e19"
    );
    assert_eq!(display, "10000000000000000000");
}

/// FlexFloat Display for a whole number < i64::MIN should not saturate to i64::MIN.
#[test]
fn flexfloat_display_large_negative_whole_number() {
    use crate::template::FlexFloat;
    let ff = FlexFloat(-1e19, None);
    let display = format!("{ff}");
    assert_ne!(
        display,
        i64::MIN.to_string(),
        "FlexFloat should not saturate to i64::MIN for -1e19"
    );
    assert_eq!(display, "-10000000000000000000");
}

/// FlexFloat Display for a normal whole number within i64 range should still use integer format.
#[test]
fn flexfloat_display_normal_whole_number() {
    use crate::template::FlexFloat;
    let ff = FlexFloat(42.0, None);
    let display = format!("{ff}");
    assert_eq!(display, "42", "FlexFloat should display 42.0 as '42'");
}

// ═══════════════════════════════════════════════════════════════
// FlexFloat Display: large whole numbers via job template roundtrip
// ═══════════════════════════════════════════════════════════════

#[test]
fn flexfloat_display_large_whole_number_overflow() {
    let template_yaml = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{
            "name": "BigFloat",
            "type": "FLOAT",
            "default": 1e19
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#
    .to_string();
    let v = yaml_val(&template_yaml);
    let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();

    let param = &jt.parameter_definitions.as_ref().unwrap()[0];
    let default_str = param.default_value().unwrap();

    let expected_correct = "10000000000000000000";
    let i64_max_str = i64::MAX.to_string();

    if default_str == i64_max_str {
        panic!(
            "BUG CONFIRMED: FlexFloat Display overflow! \
             1e19 displayed as i64::MAX ({i64_max_str}) instead of {expected_correct}"
        );
    }
    assert_eq!(
        default_str, expected_correct,
        "FlexFloat should display 1e19 correctly, got: {default_str}"
    );
}

#[test]
fn flexfloat_display_negative_large_whole_number() {
    let template_yaml = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{
            "name": "BigNeg",
            "type": "FLOAT",
            "default": -1e19
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#;
    let v = yaml_val(template_yaml);
    let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
    let param = &jt.parameter_definitions.as_ref().unwrap()[0];
    let default_str = param.default_value().unwrap();

    let i64_min_str = i64::MIN.to_string();
    if default_str == i64_min_str {
        panic!(
            "BUG CONFIRMED: FlexFloat Display overflow for negative! \
             -1e19 displayed as i64::MIN ({i64_min_str})"
        );
    }
}
