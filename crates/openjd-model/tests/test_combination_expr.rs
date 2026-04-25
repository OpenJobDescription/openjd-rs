// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Combination expression parser edge case tests.
//! Ported from Python test_combination_expr.py and test_param_space_dim_validation.py.

use openjd_model::step_param_space::StepParameterSpaceIterator;
use openjd_model::JobParameterInputValues;
use openjd_model::{create_job, decode_job_template, preprocess_job_parameters, CallerLimits};

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn all_exts() -> Vec<&'static str> {
    vec!["EXPR", "FEATURE_BUNDLE_1", "TASK_CHUNKING"]
}

/// Build a job template JSON string with the given parameter definitions and combination.
/// Each param is (name, type, range_json).
fn make_template(params: &[(&str, &str, &str)], combination: &str) -> String {
    let defs: Vec<String> = params
        .iter()
        .map(|(name, typ, range)| {
            format!(r#"{{"name": "{name}", "type": "{typ}", "range": {range}}}"#)
        })
        .collect();
    let combo = if combination.is_empty() {
        String::new()
    } else {
        format!(r#", "combination": "{combination}""#)
    };
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{{"name": "step", "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}},
            "parameterSpace": {{
                "taskParameterDefinitions": [{defs}]{combo}
            }}
        }}]
    }}"#,
        defs = defs.join(", ")
    )
}

fn iterate(template: &str) -> Result<Vec<openjd_model::types::TaskParameterSet>, String> {
    let v = yaml_val(template);
    let exts = all_exts();
    let jt =
        decode_job_template(v, Some(&exts), &CallerLimits::default()).map_err(|e| e.to_string())?;
    let processed = preprocess_job_parameters(
        &jt,
        &JobParameterInputValues::new(),
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "/tmp",
            current_working_dir: "/tmp",
            allow_template_dir_walk_up: true,
            path_format: openjd_expr::path_mapping::PathFormat::Posix,
            allow_uri_path_values: true,
        },
    )
    .map_err(|e| e.to_string())?;
    let job = create_job(&jt, &processed, &CallerLimits::default()).map_err(|e| e.to_string())?;
    let step = &job.steps[0];
    match &step.parameter_space {
        Some(ps) => {
            let iter = StepParameterSpaceIterator::new(ps).map_err(|e| e.to_string())?;
            Ok(iter.collect())
        }
        None => Ok(Vec::new()),
    }
}

fn task_val(tasks: &[openjd_model::types::TaskParameterSet], idx: usize, name: &str) -> String {
    let tv = &tasks[idx][name];
    match &tv.value {
        openjd_expr::ExprValue::Int(i) => i.to_string(),
        openjd_expr::ExprValue::Float(f) => f.to_string(),
        openjd_expr::ExprValue::String(s) => s.clone(),
        other => format!("{:?}", other),
    }
}

// ── Successful parse cases (from Python test_combination_expr.py) ──

#[test]
fn single_identifier() {
    let t = make_template(&[("A", "INT", "[1,2,3]")], "");
    let tasks = iterate(&t).unwrap();
    assert_eq!(tasks.len(), 3);
}

#[test]
fn product_two_params() {
    let t = make_template(
        &[("A", "INT", "[1,2]"), ("B", "INT", "[10,20,30]")],
        "A * B",
    );
    let tasks = iterate(&t).unwrap();
    assert_eq!(tasks.len(), 6); // 2 * 3
                                // Rightmost moves fastest
    assert_eq!(task_val(&tasks, 0, "A"), "1");
    assert_eq!(task_val(&tasks, 0, "B"), "10");
    assert_eq!(task_val(&tasks, 1, "A"), "1");
    assert_eq!(task_val(&tasks, 1, "B"), "20");
    assert_eq!(task_val(&tasks, 3, "A"), "2");
    assert_eq!(task_val(&tasks, 3, "B"), "10");
}

#[test]
fn product_five_params() {
    let t = make_template(
        &[
            ("A", "INT", "[1,2]"),
            ("B", "INT", "[1,2]"),
            ("C", "INT", "[1,2]"),
            ("D", "INT", "[1,2]"),
            ("E", "INT", "[1,2]"),
        ],
        "A * B * C * D * E",
    );
    let tasks = iterate(&t).unwrap();
    assert_eq!(tasks.len(), 32); // 2^5
}

#[test]
fn association_two_params() {
    let t = make_template(
        &[("A", "INT", "[1,2,3]"), ("B", "STRING", r#"["x","y","z"]"#)],
        "(A, B)",
    );
    let tasks = iterate(&t).unwrap();
    assert_eq!(tasks.len(), 3);
    assert_eq!(task_val(&tasks, 0, "A"), "1");
    assert_eq!(task_val(&tasks, 0, "B"), "x");
    assert_eq!(task_val(&tasks, 2, "A"), "3");
    assert_eq!(task_val(&tasks, 2, "B"), "z");
}

#[test]
fn association_five_params() {
    let t = make_template(
        &[
            ("A", "INT", "[1,2]"),
            ("B", "INT", "[3,4]"),
            ("C", "INT", "[5,6]"),
            ("D", "INT", "[7,8]"),
            ("E", "INT", "[9,10]"),
        ],
        "(A, B, C, D, E)",
    );
    let tasks = iterate(&t).unwrap();
    assert_eq!(tasks.len(), 2);
}

// ── Nested combinations ──

#[test]
fn product_then_association() {
    // A * (B, C) — product of A with association of B,C
    let t = make_template(
        &[
            ("A", "INT", "[1,2,3]"),
            ("B", "INT", "[10,20]"),
            ("C", "INT", "[100,200]"),
        ],
        "A * (B, C)",
    );
    let tasks = iterate(&t).unwrap();
    assert_eq!(tasks.len(), 6); // 3 * 2
    assert_eq!(task_val(&tasks, 0, "A"), "1");
    assert_eq!(task_val(&tasks, 0, "B"), "10");
    assert_eq!(task_val(&tasks, 0, "C"), "100");
    assert_eq!(task_val(&tasks, 1, "A"), "1");
    assert_eq!(task_val(&tasks, 1, "B"), "20");
    assert_eq!(task_val(&tasks, 1, "C"), "200");
}

#[test]
fn association_then_product() {
    // (B, C) * A
    let t = make_template(
        &[
            ("A", "INT", "[1,2,3]"),
            ("B", "INT", "[10,20]"),
            ("C", "INT", "[100,200]"),
        ],
        "(B, C) * A",
    );
    let tasks = iterate(&t).unwrap();
    assert_eq!(tasks.len(), 6); // 2 * 3
                                // Rightmost (A) moves fastest
    assert_eq!(task_val(&tasks, 0, "B"), "10");
    assert_eq!(task_val(&tasks, 0, "C"), "100");
    assert_eq!(task_val(&tasks, 0, "A"), "1");
    assert_eq!(task_val(&tasks, 1, "B"), "10");
    assert_eq!(task_val(&tasks, 1, "C"), "100");
    assert_eq!(task_val(&tasks, 1, "A"), "2");
}

#[test]
fn nested_product_in_association() {
    // (A * B, C * D)
    let t = make_template(
        &[
            ("A", "INT", "[1,2]"),
            ("B", "INT", "[10,20]"),
            ("C", "INT", "[100,200]"),
            ("D", "INT", "[1000,2000]"),
        ],
        "(A * B, C * D)",
    );
    let tasks = iterate(&t).unwrap();
    assert_eq!(tasks.len(), 4); // assoc of two products each of size 4
}

#[test]
fn nested_association_in_association() {
    // ((A, B), (C, D))
    let t = make_template(
        &[
            ("A", "INT", "[1,2]"),
            ("B", "INT", "[3,4]"),
            ("C", "INT", "[5,6]"),
            ("D", "INT", "[7,8]"),
        ],
        "((A, B), (C, D))",
    );
    let tasks = iterate(&t).unwrap();
    assert_eq!(tasks.len(), 2);
    assert_eq!(task_val(&tasks, 0, "A"), "1");
    assert_eq!(task_val(&tasks, 0, "B"), "3");
    assert_eq!(task_val(&tasks, 0, "C"), "5");
    assert_eq!(task_val(&tasks, 0, "D"), "7");
}

#[test]
fn nested_association_left() {
    // ((A, C), B) — from Python test_param_space_dim_validation
    let t = make_template(
        &[
            ("A", "INT", "[1,2]"),
            ("B", "INT", "[3,4]"),
            ("C", "INT", "[5,6]"),
        ],
        "((A, C), B)",
    );
    let tasks = iterate(&t).unwrap();
    assert_eq!(tasks.len(), 2);
}

#[test]
fn nested_association_right() {
    // (A, (B, C)) — from Python test_param_space_dim_validation
    let t = make_template(
        &[
            ("A", "INT", "[1,2]"),
            ("B", "INT", "[3,4]"),
            ("C", "INT", "[5,6]"),
        ],
        "(A, (B, C))",
    );
    let tasks = iterate(&t).unwrap();
    assert_eq!(tasks.len(), 2);
}

#[test]
fn nested_product_left_in_association() {
    // (A*C, B) where A=5, B=5, C=1 — from Python test
    let t = make_template(
        &[
            ("A", "INT", "[1,2,3,4,5]"),
            ("B", "INT", "[6,7,8,9,10]"),
            ("C", "INT", "[100]"),
        ],
        "(A * C, B)",
    );
    let tasks = iterate(&t).unwrap();
    assert_eq!(tasks.len(), 5); // assoc: both sides have 5 elements
}

#[test]
fn nested_product_right_in_association() {
    // (A, B*C) where A=5, B=5, C=1
    let t = make_template(
        &[
            ("A", "INT", "[1,2,3,4,5]"),
            ("B", "INT", "[6,7,8,9,10]"),
            ("C", "INT", "[100]"),
        ],
        "(A, B * C)",
    );
    let tasks = iterate(&t).unwrap();
    assert_eq!(tasks.len(), 5);
}

#[test]
fn complex_nested_product_association() {
    // Param1 * (Param2, Param3 * Param4) — from Python test_step_param_space_iter.py
    let t = make_template(
        &[
            ("Param1", "INT", "[1,2]"),
            ("Param2", "STRING", r#"["a","b","c","d"]"#),
            ("Param3", "INT", "[10,11]"),
            ("Param4", "INT", "[20,21]"),
        ],
        "Param1 * (Param2, Param3 * Param4)",
    );
    let tasks = iterate(&t).unwrap();
    assert_eq!(tasks.len(), 8); // 2 * 4
                                // First element: Param1=1, Param2=a, Param3=10, Param4=20
    assert_eq!(task_val(&tasks, 0, "Param1"), "1");
    assert_eq!(task_val(&tasks, 0, "Param2"), "a");
    assert_eq!(task_val(&tasks, 0, "Param3"), "10");
    assert_eq!(task_val(&tasks, 0, "Param4"), "20");
}

// ── Whitespace variations ──

#[test]
fn no_spaces() {
    let t = make_template(
        &[
            ("A", "INT", "[1,2]"),
            ("B", "INT", "[3,4]"),
            ("C", "INT", "[5,6]"),
        ],
        "(A,B)*C",
    );
    let tasks = iterate(&t).unwrap();
    assert_eq!(tasks.len(), 4);
}

#[test]
fn extra_spaces() {
    let t = make_template(
        &[
            ("A", "INT", "[1,2]"),
            ("B", "INT", "[3,4]"),
            ("C", "INT", "[5,6]"),
        ],
        "  A  *  ( B , C )  ",
    );
    let tasks = iterate(&t).unwrap();
    assert_eq!(tasks.len(), 4);
}

#[test]
fn compact_association_product() {
    let t = make_template(
        &[
            ("A", "INT", "[1,2]"),
            ("B", "INT", "[3,4]"),
            ("C", "INT", "[5,6]"),
        ],
        "C*(A,B)",
    );
    let tasks = iterate(&t).unwrap();
    assert_eq!(tasks.len(), 4);
}

// ── Error cases ──

#[test]
fn error_mismatched_association_lengths() {
    let t = make_template(
        &[("A", "INT", "[1,2,3]"), ("B", "INT", "[10,20]")],
        "(A, B)",
    );
    let err = iterate(&t).unwrap_err();
    assert!(
        err.contains("same number of values") || err.contains("same length"),
        "Expected association length mismatch error, got: {err}"
    );
}

#[test]
fn error_nested_mismatched_association() {
    // (A, (B, C)) where A has different length than (B,C)
    let t = make_template(
        &[
            ("A", "INT", "[1,2,3]"),
            ("B", "INT", "[10,20]"),
            ("C", "INT", "[100,200]"),
        ],
        "(A, (B, C))",
    );
    let err = iterate(&t).unwrap_err();
    assert!(
        err.contains("same number of values") || err.contains("same length"),
        "Expected association length mismatch error, got: {err}"
    );
}

#[test]
fn error_single_element_association() {
    // (A) should be rejected — association must have more than one term
    let t = make_template(&[("A", "INT", "[1,2,3]")], "(A)");
    let err = iterate(&t).unwrap_err();
    assert!(
        err.contains("more than one term") || err.contains("Association"),
        "Expected single-element association error, got: {err}"
    );
}

#[test]
fn error_unknown_parameter_in_combination() {
    let t = make_template(&[("A", "INT", "[1,2,3]")], "A * B");
    let err = iterate(&t).unwrap_err();
    assert!(
        err.contains("B"),
        "Expected unknown parameter error mentioning B, got: {err}"
    );
}

#[test]
fn whitespace_only_combination_rejected() {
    let t = make_template(&[("A", "INT", "[1,2]")], "   ");
    assert!(
        iterate(&t).is_err(),
        "Whitespace-only combination should be rejected"
    );
}

// ══════════════════════════════════════════════════════════════
// BUG-1: Combination expression parser must reject malformed input
// ══════════════════════════════════════════════════════════════

#[test]
fn error_double_comma_rejected() {
    let t = make_template(&[("A", "INT", "[1,2]"), ("B", "INT", "[3,4]")], "(A,,B)");
    let v = yaml_val(&t);
    let err = decode_job_template(v, None, &CallerLimits::default())
        .expect_err("(A,,B) should be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("empty element in combination expression."),
        "Expected empty element error, got: {msg}"
    );
}

#[test]
fn error_leading_comma_rejected() {
    let t = make_template(&[("A", "INT", "[1,2]"), ("B", "INT", "[3,4]")], "(,A,B)");
    let v = yaml_val(&t);
    let err = decode_job_template(v, None, &CallerLimits::default())
        .expect_err("(,A,B) should be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("empty element in combination expression."),
        "Expected empty element error, got: {msg}"
    );
}

#[test]
fn error_trailing_comma_rejected() {
    let t = make_template(&[("A", "INT", "[1,2]"), ("B", "INT", "[3,4]")], "(A,B,)");
    let v = yaml_val(&t);
    let err = decode_job_template(v, None, &CallerLimits::default())
        .expect_err("(A,B,) should be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("empty group in combination expression.")
            || msg.contains("empty element in combination expression."),
        "Expected empty group/element error, got: {msg}"
    );
}
