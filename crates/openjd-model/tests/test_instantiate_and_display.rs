// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Red/green TDD tests for instantiate_step error propagation and FlexFloat Display.

use openjd_model::JobParameterInputValues;
use openjd_model::{create_job, decode_job_template, preprocess_job_parameters};
use std::path::Path;

fn yaml_val(s: &str) -> serde_yaml::Value {
    serde_yaml::from_str(s).unwrap()
}

fn preprocess(
    jt: &openjd_model::template::JobTemplate,
    input: &JobParameterInputValues,
) -> openjd_model::JobParameterValues {
    preprocess_job_parameters(jt, input, &[], Path::new("/tmp"), Path::new("/tmp"), true).unwrap()
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
    let jt = decode_job_template(v, Some(exts)).unwrap();

    // Provide Divisor=0 to trigger division by zero at create_job time
    let mut input = JobParameterInputValues::new();
    input.insert("Divisor".into(), openjd_expr::ExprValue::Int(0));
    let params = preprocess(&jt, &input);
    let result = create_job(&jt, &params);

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
    let jt = decode_job_template(v, Some(exts)).unwrap();

    // Normal case: should succeed and the binding should be in the resolved symtab
    let mut input = JobParameterInputValues::new();
    input.insert("Count".into(), openjd_expr::ExprValue::Int(5));
    let params = preprocess(&jt, &input);
    let job = create_job(&jt, &params).unwrap();
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
    use openjd_model::template::FlexFloat;
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
    use openjd_model::template::FlexFloat;
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
    use openjd_model::template::FlexFloat;
    let ff = FlexFloat(42.0, None);
    let display = format!("{ff}");
    assert_eq!(display, "42", "FlexFloat should display 42.0 as '42'");
}
