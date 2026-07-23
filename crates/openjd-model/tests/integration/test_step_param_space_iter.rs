// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Integration tests: template → create_job → StepParameterSpaceIterator.
//! Ported from Python test_step_param_space_iter.py.

use std::collections::HashMap;

use openjd_expr::path_mapping::PathFormat;
use openjd_model::step_param_space::StepParameterSpaceIterator;
use openjd_model::CallerLimits;
use openjd_model::JobParameterInputValues;
use openjd_model::{create_job, decode_job_template, preprocess_job_parameters};

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn create_and_iterate(
    template_json: &str,
    params: &[(&str, &str)],
) -> Vec<openjd_model::types::TaskParameterSet> {
    let v = yaml_val(template_json);
    let supported = ["EXPR", "FEATURE_BUNDLE_1", "TASK_CHUNKING"];
    let supported_refs: Vec<&str> = supported.to_vec();
    let jt = decode_job_template(v, Some(&supported_refs), &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    for (k, val) in params {
        input.insert(
            k.to_string(),
            openjd_expr::ExprValue::String(val.to_string()),
        );
    }
    let processed = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "/tmp/template",
            current_working_dir: "/tmp/cwd",
            allow_template_dir_walk_up: true,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    let job = create_job(&jt, &processed, &jt.default_validation_context()).unwrap();
    let step = &job.steps[0];
    if let Some(ps) = &step.parameter_space {
        let iter = StepParameterSpaceIterator::new(ps).unwrap();
        iter.collect()
    } else {
        Vec::new()
    }
}

fn task_val_str(
    tasks: &[openjd_model::types::TaskParameterSet],
    task_idx: usize,
    param_name: &str,
) -> String {
    let tv = &tasks[task_idx][param_name];
    match &tv.value {
        openjd_expr::ExprValue::Int(i) => i.to_string(),
        openjd_expr::ExprValue::Float(f) => f.to_string(),
        openjd_expr::ExprValue::String(s) => s.clone(),
        openjd_expr::ExprValue::Bool(b) => b.to_string(),
        other => format!("{:?}", other),
    }
}

#[test]
fn test_no_param_space() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "do thing"}}}}]
    }"#,
    );
    let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
    let processed = preprocess_job_parameters(
        &jt,
        &JobParameterInputValues::new(),
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "/tmp",
            current_working_dir: "/tmp",
            allow_template_dir_walk_up: true,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    let job = create_job(&jt, &processed, &jt.default_validation_context()).unwrap();
    assert!(job.steps[0].parameter_space.is_none());
}

#[test]
fn test_single_int_range_list() {
    let tasks = create_and_iterate(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {"taskParameterDefinitions": [{"name": "Frame", "type": "INT", "range": [1, 2, 3]}]}
        }]
    }"#,
        &[],
    );
    assert_eq!(tasks.len(), 3);
    assert_eq!(task_val_str(&tasks, 0, "Frame"), "1");
    assert_eq!(task_val_str(&tasks, 1, "Frame"), "2");
    assert_eq!(task_val_str(&tasks, 2, "Frame"), "3");
}

#[test]
fn test_single_int_range_expression() {
    let tasks = create_and_iterate(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {"taskParameterDefinitions": [{"name": "Frame", "type": "INT", "range": "1-5"}]}
        }]
    }"#,
        &[],
    );
    assert_eq!(tasks.len(), 5);
    assert_eq!(task_val_str(&tasks, 0, "Frame"), "1");
    assert_eq!(task_val_str(&tasks, 4, "Frame"), "5");
}

#[test]
fn test_string_range() {
    let tasks = create_and_iterate(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {"taskParameterDefinitions": [{"name": "Color", "type": "STRING", "range": ["red", "green", "blue"]}]}
        }]
    }"#,
        &[],
    );
    assert_eq!(tasks.len(), 3);
    assert_eq!(task_val_str(&tasks, 0, "Color"), "red");
    assert_eq!(task_val_str(&tasks, 1, "Color"), "green");
    assert_eq!(task_val_str(&tasks, 2, "Color"), "blue");
}

#[test]
fn test_float_range() {
    let tasks = create_and_iterate(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {"taskParameterDefinitions": [{"name": "Scale", "type": "FLOAT", "range": [1.0, 2.5, 3.0]}]}
        }]
    }"#,
        &[],
    );
    assert_eq!(tasks.len(), 3);
}

#[test]
fn test_product_combination() {
    let tasks = create_and_iterate(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "A", "type": "INT", "range": [1, 2]},
                    {"name": "B", "type": "STRING", "range": ["x", "y", "z"]}
                ],
                "combination": "A * B"
            }
        }]
    }"#,
        &[],
    );
    assert_eq!(tasks.len(), 6);
}

#[test]
fn test_association_combination() {
    let tasks = create_and_iterate(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "A", "type": "INT", "range": [1, 2, 3]},
                    {"name": "B", "type": "STRING", "range": ["x", "y", "z"]}
                ],
                "combination": "(A, B)"
            }
        }]
    }"#,
        &[],
    );
    assert_eq!(tasks.len(), 3);
    assert_eq!(task_val_str(&tasks, 0, "A"), "1");
    assert_eq!(task_val_str(&tasks, 0, "B"), "x");
    assert_eq!(task_val_str(&tasks, 2, "A"), "3");
    assert_eq!(task_val_str(&tasks, 2, "B"), "z");
}

#[test]
fn test_format_string_in_range() {
    let tasks = create_and_iterate(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "parameterDefinitions": [{"name": "Prefix", "type": "STRING"}],
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {"taskParameterDefinitions": [{"name": "File", "type": "STRING", "range": ["{{Param.Prefix}}_a", "{{Param.Prefix}}_b"]}]}
        }]
    }"#,
        &[("Prefix", "test")],
    );
    assert_eq!(tasks.len(), 2);
    assert_eq!(task_val_str(&tasks, 0, "File"), "test_a");
    assert_eq!(task_val_str(&tasks, 1, "File"), "test_b");
}

#[test]
fn test_path_range() {
    let tasks = create_and_iterate(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {"taskParameterDefinitions": [{"name": "Dir", "type": "PATH", "range": ["/tmp/a", "/tmp/b"]}]}
        }]
    }"#,
        &[],
    );
    assert_eq!(tasks.len(), 2);
    assert_eq!(task_val_str(&tasks, 0, "Dir"), "/tmp/a");
    assert_eq!(task_val_str(&tasks, 1, "Dir"), "/tmp/b");
}

// === Tests ported from Python test_step_param_space_iter.py ===

#[test]
fn test_iterator_names() {
    let tasks = create_and_iterate(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "Param1", "type": "INT", "range": [1, 2]},
                    {"name": "Param2", "type": "STRING", "range": ["a", "b", "c"]}
                ]
            }
        }]
    }"#,
        &[],
    );
    // Verify we got the right number of tasks (product: 2*3=6)
    assert_eq!(tasks.len(), 6);
}

#[test]
fn test_single_param_len() {
    // Test len for various range types
    for (range, expected_len) in [
        ("[1, 2, 3]", 3usize),
        (r#""1-5""#, 5),
        ("[0, 10, 20, 40]", 4),
    ] {
        let template = format!(
            r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Job",
            "steps": [{{"name": "step", "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}},
                "parameterSpace": {{"taskParameterDefinitions": [{{"name": "Param1", "type": "INT", "range": {range}}}]}}
            }}]
        }}"#
        );
        let tasks = create_and_iterate(&template, &[]);
        assert_eq!(tasks.len(), expected_len, "Failed for range {range}");
    }
}

#[test]
fn test_product_iteration_three_params() {
    let tasks = create_and_iterate(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "Param1", "type": "INT", "range": [1, 2]},
                    {"name": "Param2", "type": "STRING", "range": ["a", "b", "c"]},
                    {"name": "Param3", "type": "INT", "range": [-1, -2]}
                ],
                "combination": "Param1 * Param2 * Param3"
            }
        }]
    }"#,
        &[],
    );
    assert_eq!(tasks.len(), 12); // 2 * 3 * 2
                                 // First element
    assert_eq!(task_val_str(&tasks, 0, "Param1"), "1");
    assert_eq!(task_val_str(&tasks, 0, "Param2"), "a");
    assert_eq!(task_val_str(&tasks, 0, "Param3"), "-1");
    // Second element (rightmost moves fastest)
    assert_eq!(task_val_str(&tasks, 1, "Param1"), "1");
    assert_eq!(task_val_str(&tasks, 1, "Param2"), "a");
    assert_eq!(task_val_str(&tasks, 1, "Param3"), "-2");
    // Last element
    assert_eq!(task_val_str(&tasks, 11, "Param1"), "2");
    assert_eq!(task_val_str(&tasks, 11, "Param2"), "c");
    assert_eq!(task_val_str(&tasks, 11, "Param3"), "-2");
}

#[test]
fn test_product_len() {
    let tasks = create_and_iterate(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "Param1", "type": "INT", "range": "1-2"},
                    {"name": "Param2", "type": "STRING", "range": ["a", "b", "c"]},
                    {"name": "Param3", "type": "INT", "range": [-1, -2]}
                ],
                "combination": "Param1 * Param2 * Param3"
            }
        }]
    }"#,
        &[],
    );
    assert_eq!(tasks.len(), 2 * 3 * 2);
}

#[test]
fn test_associate_iteration() {
    let tasks = create_and_iterate(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "Param1", "type": "INT", "range": "1-4"},
                    {"name": "Param2", "type": "STRING", "range": ["a", "b", "c", "d"]},
                    {"name": "Param3", "type": "INT", "range": [-1, -2, -3, -4]}
                ],
                "combination": "(Param1, Param2, Param3)"
            }
        }]
    }"#,
        &[],
    );
    assert_eq!(tasks.len(), 4);
    assert_eq!(task_val_str(&tasks, 0, "Param1"), "1");
    assert_eq!(task_val_str(&tasks, 0, "Param2"), "a");
    assert_eq!(task_val_str(&tasks, 0, "Param3"), "-1");
    assert_eq!(task_val_str(&tasks, 3, "Param1"), "4");
    assert_eq!(task_val_str(&tasks, 3, "Param2"), "d");
    assert_eq!(task_val_str(&tasks, 3, "Param3"), "-4");
}

#[test]
fn test_associate_len() {
    let tasks = create_and_iterate(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "Param1", "type": "INT", "range": [1, 2, 3, 4]},
                    {"name": "Param2", "type": "STRING", "range": ["a", "b", "c", "d"]},
                    {"name": "Param3", "type": "INT", "range": "-1--4:-1"}
                ],
                "combination": "(Param1, Param2, Param3)"
            }
        }]
    }"#,
        &[],
    );
    assert_eq!(tasks.len(), 4);
}

#[test]
fn test_nested_expr_iteration() {
    // Param1 * (Param2, Param3 * Param4)
    let tasks = create_and_iterate(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "Param1", "type": "INT", "range": [1, 2]},
                    {"name": "Param2", "type": "STRING", "range": ["a", "b", "c", "d"]},
                    {"name": "Param3", "type": "INT", "range": "10-11"},
                    {"name": "Param4", "type": "INT", "range": [20, 21]}
                ],
                "combination": "Param1 * (Param2, Param3 * Param4)"
            }
        }]
    }"#,
        &[],
    );
    assert_eq!(tasks.len(), 8); // 2 * 4
    assert_eq!(task_val_str(&tasks, 0, "Param1"), "1");
    assert_eq!(task_val_str(&tasks, 0, "Param2"), "a");
    assert_eq!(task_val_str(&tasks, 0, "Param3"), "10");
    assert_eq!(task_val_str(&tasks, 0, "Param4"), "20");
    assert_eq!(task_val_str(&tasks, 1, "Param2"), "b");
    assert_eq!(task_val_str(&tasks, 1, "Param3"), "10");
    assert_eq!(task_val_str(&tasks, 1, "Param4"), "21");
}

#[test]
fn test_defaults_product_combination() {
    // When no combination is specified, default is product of all params
    let tasks = create_and_iterate(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "Param1", "type": "INT", "range": [1, 2]},
                    {"name": "Param2", "type": "STRING", "range": ["a", "b"]}
                ]
            }
        }]
    }"#,
        &[],
    );
    assert_eq!(tasks.len(), 4); // 2 * 2
                                // Verify all expected combinations exist (order may vary)
    let mut found = std::collections::HashSet::new();
    for t in &tasks {
        let p1 = task_val_str(std::slice::from_ref(t), 0, "Param1");
        let p2 = task_val_str(std::slice::from_ref(t), 0, "Param2");
        found.insert((p1, p2));
    }
    assert!(found.contains(&("1".to_string(), "a".to_string())));
    assert!(found.contains(&("1".to_string(), "b".to_string())));
    assert!(found.contains(&("2".to_string(), "a".to_string())));
    assert!(found.contains(&("2".to_string(), "b".to_string())));
}

#[test]
fn test_contains_check() {
    // Verify contains works for single param
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {"taskParameterDefinitions": [{"name": "Param1", "type": "INT", "range": [10, 11, 12]}]}
        }]
    }"#,
    );
    let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
    let processed = preprocess_job_parameters(
        &jt,
        &JobParameterInputValues::new(),
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "/tmp",
            current_working_dir: "/tmp",
            allow_template_dir_walk_up: true,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    let job = create_job(&jt, &processed, &jt.default_validation_context()).unwrap();
    let ps = job.steps[0].parameter_space.as_ref().unwrap();
    let iter = StepParameterSpaceIterator::new(ps).unwrap();

    // Build a task parameter set that should be in the space
    let mut in_set = openjd_model::types::TaskParameterSet::new();
    in_set.insert(
        "Param1".to_string(),
        openjd_model::types::TaskParameterValue {
            param_type: openjd_model::types::TaskParameterType::Int,
            value: openjd_expr::ExprValue::Int(10),
        },
    );
    assert!(iter.contains(&in_set));

    // Build one that should NOT be in the space — value out of range.
    let mut not_in_set = openjd_model::types::TaskParameterSet::new();
    not_in_set.insert(
        "Param1".to_string(),
        openjd_model::types::TaskParameterValue {
            param_type: openjd_model::types::TaskParameterType::Int,
            value: openjd_expr::ExprValue::Int(9),
        },
    );
    assert!(!iter.contains(&not_in_set));
    let err = iter.validate_containment(&not_in_set).unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Param1' value '9' is not in the parameter space range."
    );

    // Wrong type for the parameter — INT space, but we pass a String.
    // The leaf node compares values structurally, so a String value is
    // not equal to any Int value in the space and gets rejected with
    // the same out-of-range message (using the value's display form).
    let mut wrong_type = openjd_model::types::TaskParameterSet::new();
    wrong_type.insert(
        "Param1".to_string(),
        openjd_model::types::TaskParameterValue {
            param_type: openjd_model::types::TaskParameterType::String,
            value: openjd_expr::ExprValue::String("hello".to_string()),
        },
    );
    assert!(!iter.contains(&wrong_type));
    let err = iter.validate_containment(&wrong_type).unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Param1' value 'hello' is not in the parameter space range."
    );

    // Missing key — the param set is empty. The top-level
    // `validate_containment` rejects on a name-set mismatch before any
    // node-level traversal happens.
    let empty = openjd_model::types::TaskParameterSet::new();
    assert!(!iter.contains(&empty));
    let err = iter.validate_containment(&empty).unwrap_err();
    assert_eq!(
        err,
        "Task parameter names [] do not match the parameter space names [\"Param1\"]."
    );

    // Extra key — the param set has Param1 plus a key not in the
    // space. Same name-set mismatch rejection.
    let mut extra_key = openjd_model::types::TaskParameterSet::new();
    extra_key.insert(
        "Param1".to_string(),
        openjd_model::types::TaskParameterValue {
            param_type: openjd_model::types::TaskParameterType::Int,
            value: openjd_expr::ExprValue::Int(10),
        },
    );
    extra_key.insert(
        "Bogus".to_string(),
        openjd_model::types::TaskParameterValue {
            param_type: openjd_model::types::TaskParameterType::Int,
            value: openjd_expr::ExprValue::Int(0),
        },
    );
    assert!(!iter.contains(&extra_key));
    let err = iter.validate_containment(&extra_key).unwrap_err();
    assert_eq!(
        err,
        "Task parameter names [\"Bogus\", \"Param1\"] do not match the parameter space names [\"Param1\"]."
    );

    // Same key count as the space (1) but a different name. The
    // top-level name-set check rejects this before any node-level
    // value check sees it — even though `Param1` would be a valid
    // value, replacing the key with `Bogus` makes the keyset
    // mismatch.
    let mut wrong_name_same_count = openjd_model::types::TaskParameterSet::new();
    wrong_name_same_count.insert(
        "Bogus".to_string(),
        openjd_model::types::TaskParameterValue {
            param_type: openjd_model::types::TaskParameterType::Int,
            value: openjd_expr::ExprValue::Int(10),
        },
    );
    assert!(!iter.contains(&wrong_name_same_count));
    let err = iter
        .validate_containment(&wrong_name_same_count)
        .unwrap_err();
    assert_eq!(
        err,
        "Task parameter names [\"Bogus\"] do not match the parameter space names [\"Param1\"]."
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Default product combination — iteration order determinism
// ═══════════════════════════════════════════════════════════════════════════

/// When no explicit combination expression is provided, the default product
/// must iterate parameters in template definition order (leftmost moves
/// slowest, rightmost moves fastest). This must be deterministic.
#[test]
fn test_default_product_preserves_definition_order() {
    let template = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {"taskParameterDefinitions": [
                {"name": "A", "type": "INT", "range": [1, 2]},
                {"name": "B", "type": "STRING", "range": ["x", "y"]},
                {"name": "C", "type": "INT", "range": [10, 20]}
            ]}
        }]
    }"#;
    let tasks = create_and_iterate(template, &[]);
    // 2 * 2 * 2 = 8 tasks. A is slowest, C is fastest.
    assert_eq!(tasks.len(), 8);
    // First 4 tasks have A=1, last 4 have A=2
    assert_eq!(task_val_str(&tasks, 0, "A"), "1");
    assert_eq!(task_val_str(&tasks, 4, "A"), "2");
    // Within each A group, first 2 have B=x, next 2 have B=y
    assert_eq!(task_val_str(&tasks, 0, "B"), "x");
    assert_eq!(task_val_str(&tasks, 2, "B"), "y");
    // C alternates fastest
    assert_eq!(task_val_str(&tasks, 0, "C"), "10");
    assert_eq!(task_val_str(&tasks, 1, "C"), "20");

    // Verify full sequence is deterministic across repeated constructions
    let expected: Vec<String> = tasks
        .iter()
        .enumerate()
        .map(|(i, _)| {
            format!(
                "{},{},{}",
                task_val_str(&tasks, i, "A"),
                task_val_str(&tasks, i, "B"),
                task_val_str(&tasks, i, "C")
            )
        })
        .collect();
    for _ in 0..20 {
        let run = create_and_iterate(template, &[]);
        let actual: Vec<String> = run
            .iter()
            .enumerate()
            .map(|(i, _)| {
                format!(
                    "{},{},{}",
                    task_val_str(&run, i, "A"),
                    task_val_str(&run, i, "B"),
                    task_val_str(&run, i, "C")
                )
            })
            .collect();
        assert_eq!(expected, actual, "Iteration order must be deterministic");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// get() / random access with complex combinations
// Ported from Python test_product_getitem, test_associate_getitem,
// test_nested_expr_getitem (gap in Python), test_no_param_getelem,
// test_single_param_getelem
// ═══════════════════════════════════════════════════════════════════════════

fn create_iterator(template_json: &str) -> StepParameterSpaceIterator {
    let v = yaml_val(template_json);
    let supported = ["EXPR", "FEATURE_BUNDLE_1", "TASK_CHUNKING"];
    let supported_refs: Vec<&str> = supported.to_vec();
    let jt = decode_job_template(v, Some(&supported_refs), &CallerLimits::default()).unwrap();
    let processed = preprocess_job_parameters(
        &jt,
        &JobParameterInputValues::new(),
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "/tmp/template",
            current_working_dir: "/tmp/cwd",
            allow_template_dir_walk_up: true,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    let job = create_job(&jt, &processed, &jt.default_validation_context()).unwrap();
    let step = &job.steps[0];
    let ps = step.parameter_space.as_ref().unwrap();
    StepParameterSpaceIterator::new(ps).unwrap()
}

fn iter_val_str(set: &openjd_model::types::TaskParameterSet, name: &str) -> String {
    match &set[name].value {
        openjd_expr::ExprValue::Int(i) => i.to_string(),
        openjd_expr::ExprValue::Float(f) => f.to_string(),
        openjd_expr::ExprValue::String(s) => s.clone(),
        other => format!("{:?}", other),
    }
}

#[test]
fn test_product_getitem() {
    // Ported from Python test_product_getitem: 3-param product, forward/reverse indexing
    let iter = create_iterator(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "Param1", "type": "INT", "range": [1, 2]},
                    {"name": "Param2", "type": "STRING", "range": ["a", "b", "c"]},
                    {"name": "Param3", "type": "INT", "range": [-1, -2]}
                ],
                "combination": "Param1 * Param2 * Param3"
            }
        }]
    }"#,
    );
    assert_eq!(iter.len(), 12);

    // Forward indexing — verify all 12 elements match iteration order
    let collected: Vec<_> = StepParameterSpaceIterator::new(
        // Re-create to get a fresh iterator for collection
        &{
            let v = yaml_val(
                r#"{
                "specificationVersion": "jobtemplate-2023-09",
                "name": "Job",
                "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
                    "parameterSpace": {
                        "taskParameterDefinitions": [
                            {"name": "Param1", "type": "INT", "range": [1, 2]},
                            {"name": "Param2", "type": "STRING", "range": ["a", "b", "c"]},
                            {"name": "Param3", "type": "INT", "range": [-1, -2]}
                        ],
                        "combination": "Param1 * Param2 * Param3"
                    }
                }]
            }"#,
            );
            let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
            let processed = preprocess_job_parameters(
                &jt,
                &JobParameterInputValues::new(),
                &[],
                &openjd_model::PathParameterOptions {
                    job_template_dir: "/tmp",
                    current_working_dir: "/tmp",
                    allow_template_dir_walk_up: true,
                    path_format: PathFormat::host(),
                    allow_uri_path_values: true,
                },
            )
            .unwrap();
            let job = create_job(&jt, &processed, &jt.default_validation_context()).unwrap();
            job.steps[0].parameter_space.clone().unwrap()
        },
    )
    .unwrap()
    .collect::<Vec<_>>();

    for i in 0..12 {
        let got = iter.get(i).unwrap();
        assert_eq!(
            iter_val_str(&got, "Param1"),
            task_val_str(&collected, i, "Param1"),
            "Mismatch at index {i} for Param1"
        );
        assert_eq!(
            iter_val_str(&got, "Param2"),
            task_val_str(&collected, i, "Param2"),
            "Mismatch at index {i} for Param2"
        );
        assert_eq!(
            iter_val_str(&got, "Param3"),
            task_val_str(&collected, i, "Param3"),
            "Mismatch at index {i} for Param3"
        );
    }

    // Out of bounds
    assert!(iter.get(12).is_none());
    assert!(iter.get(100).is_none());
}

#[test]
fn test_associate_getitem() {
    // Ported from Python test_associate_getitem: 3-param association, forward indexing
    let iter = create_iterator(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "Param1", "type": "INT", "range": "1-4"},
                    {"name": "Param2", "type": "STRING", "range": ["a", "b", "c", "d"]},
                    {"name": "Param3", "type": "INT", "range": [-1, -2, -3, -4]}
                ],
                "combination": "(Param1, Param2, Param3)"
            }
        }]
    }"#,
    );
    assert_eq!(iter.len(), 4);

    // Verify each element via get()
    let expected = [
        ("1", "a", "-1"),
        ("2", "b", "-2"),
        ("3", "c", "-3"),
        ("4", "d", "-4"),
    ];
    for (i, (p1, p2, p3)) in expected.iter().enumerate() {
        let set = iter.get(i).unwrap();
        assert_eq!(iter_val_str(&set, "Param1"), *p1, "index {i} Param1");
        assert_eq!(iter_val_str(&set, "Param2"), *p2, "index {i} Param2");
        assert_eq!(iter_val_str(&set, "Param3"), *p3, "index {i} Param3");
    }

    // Out of bounds
    assert!(iter.get(4).is_none());
}

#[test]
fn test_nested_expr_getitem() {
    // Fills the gap in Python: no test_nested_expr_getitem exists there.
    // Combination: Param1 * (Param2, Param3 * Param4)
    // Param1=[1,2], Param2=["a","b","c","d"], Param3=10-11, Param4=[20,21]
    // (Param2, Param3*Param4) is association of len 4
    // Total = 2 * 4 = 8
    let iter = create_iterator(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "Param1", "type": "INT", "range": [1, 2]},
                    {"name": "Param2", "type": "STRING", "range": ["a", "b", "c", "d"]},
                    {"name": "Param3", "type": "INT", "range": "10-11"},
                    {"name": "Param4", "type": "INT", "range": [20, 21]}
                ],
                "combination": "Param1 * (Param2, Param3 * Param4)"
            }
        }]
    }"#,
    );
    assert_eq!(iter.len(), 8);

    // Verify get() matches iteration for all 8 elements
    let tasks = create_and_iterate(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "Param1", "type": "INT", "range": [1, 2]},
                    {"name": "Param2", "type": "STRING", "range": ["a", "b", "c", "d"]},
                    {"name": "Param3", "type": "INT", "range": "10-11"},
                    {"name": "Param4", "type": "INT", "range": [20, 21]}
                ],
                "combination": "Param1 * (Param2, Param3 * Param4)"
            }
        }]
    }"#,
        &[],
    );

    for i in 0..8 {
        let got = iter.get(i).unwrap();
        for name in &["Param1", "Param2", "Param3", "Param4"] {
            assert_eq!(
                iter_val_str(&got, name),
                task_val_str(&tasks, i, name),
                "get({i}) mismatch for {name}"
            );
        }
    }

    // Out of bounds
    assert!(iter.get(8).is_none());
}

#[test]
fn test_single_param_getitem() {
    // Ported from Python test_single_param_getelem
    let iter = create_iterator(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {"taskParameterDefinitions": [{"name": "Param1", "type": "INT", "range": [10, 11, 12]}]}
        }]
    }"#,
    );
    assert_eq!(iter.len(), 3);
    assert_eq!(iter_val_str(&iter.get(0).unwrap(), "Param1"), "10");
    assert_eq!(iter_val_str(&iter.get(1).unwrap(), "Param1"), "11");
    assert_eq!(iter_val_str(&iter.get(2).unwrap(), "Param1"), "12");
    assert!(iter.get(3).is_none());
}

#[test]
fn lazy_param_space_range_expr_within_limit() {
    // max_task_param_range_len is 1024 for all configs (not raised by FB1)
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        steps:
          - name: Step1
            parameterSpace:
              taskParameterDefinitions:
                - name: Frame
                  type: INT
                  range: "1-1024"
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let jt = decode_job_template(template, None, &CallerLimits::default()).unwrap();
    let params: HashMap<String, openjd_model::JobParameterValue> = HashMap::new();
    let job = create_job(&jt, &params, &jt.default_validation_context()).unwrap();
    let space = job.steps[0].parameter_space.as_ref().unwrap();
    let iter = openjd_model::StepParameterSpaceIterator::new(space).unwrap();
    assert_eq!(iter.len(), 1024);
    let task = iter.get(1023).unwrap();
    assert_eq!(task.len(), 1);
}

// ══════════════════════════════════════════════════════════════
// Issue 1.2: ProductNode length overflow must error, not silently wrap
// ══════════════════════════════════════════════════════════════

#[test]
fn product_node_overflow_is_rejected() {
    // 7 INT list params each with 1024 values: 1024^7 = 2^70 > u64::MAX
    let mut params = String::new();
    for i in 0..7 {
        if i > 0 {
            params.push(',');
        }
        // Build a list of 1024 values
        let values: Vec<String> = (0..1024).map(|v| v.to_string()).collect();
        params.push_str(&format!(
            r#"{{"name": "P{i}", "type": "INT", "range": [{}]}}"#,
            values.join(",")
        ));
    }
    let template_str = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{{
            "name": "step",
            "parameterSpace": {{
                "taskParameterDefinitions": [{params}]
            }},
            "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}
        }}]
    }}"#
    );
    let template = yaml_val(&template_str);
    let jt = decode_job_template(template, None, &CallerLimits::default()).unwrap();
    let params: HashMap<String, openjd_model::JobParameterValue> = HashMap::new();
    // Overflow is caught at create_job time (parameter space iterator validation)
    let msg = create_job(&jt, &params, &jt.default_validation_context())
        .unwrap_err()
        .to_string();
    assert!(
        msg.contains("parameter space") || msg.contains("overflow"),
        "Expected overflow error message, got: {msg}"
    );
}

// ── reset() ───────────────────────────────────────────────────────────────
//
// `StepParameterSpaceIterator::reset()` rewinds the iterator without
// rebuilding it, preserving any adaptive chunk-size override set via
// `set_chunks_default_task_count`. The three observable behaviors below
// each correspond to a distinct branch of the implementation.

/// Build an iterator from a job template (allowing TASK_CHUNKING) so the
/// reset tests can exercise both random-access and sequential paths.
fn iterator_from_template(
    template_json: &str,
    supported_extensions: &[&str],
) -> StepParameterSpaceIterator {
    let v = yaml_val(template_json);
    let jt = decode_job_template(v, Some(supported_extensions), &CallerLimits::default()).unwrap();
    let processed = preprocess_job_parameters(
        &jt,
        &JobParameterInputValues::new(),
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "/tmp/template",
            current_working_dir: "/tmp/cwd",
            allow_template_dir_walk_up: true,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    let job = create_job(&jt, &processed, &jt.default_validation_context()).unwrap();
    let ps = job.steps[0].parameter_space.as_ref().unwrap();
    StepParameterSpaceIterator::new(ps).unwrap()
}

/// Stringify a task as `"Name1=value1,Name2=value2"` so two walks of an
/// iterator can be compared with `assert_eq!`. Sorted by parameter name
/// for determinism since `TaskParameterSet` is an `IndexMap`.
fn task_to_str(task: &openjd_model::types::TaskParameterSet) -> String {
    let mut entries: Vec<(String, String)> = task
        .iter()
        .map(|(k, v)| (k.clone(), v.value.to_display_string()))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn collect_walk(iter: &mut StepParameterSpaceIterator) -> Vec<String> {
    let mut out = Vec::new();
    for task in iter.by_ref() {
        out.push(task_to_str(&task));
    }
    out
}

#[test]
fn test_reset_non_sequential_iterator() {
    // Pure product space — no chunking, no contiguous-with-gaps — so the
    // iterator takes the random-access path (`current_index` cursor).
    // `reset()` must zero that cursor so a second walk yields the same
    // sequence as the first.
    let mut iter = iterator_from_template(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "Frame", "type": "INT", "range": [1, 2, 3]},
                    {"name": "Camera", "type": "STRING", "range": ["main", "side"]}
                ],
                "combination": "Frame * Camera"
            }
        }]
    }"#,
        &[],
    );

    assert!(!iter.chunks_adaptive());
    assert_eq!(iter.len(), 6);

    let first_walk = collect_walk(&mut iter);
    assert_eq!(first_walk.len(), 6);
    // Confirm we actually drained the iterator.
    assert!(iter.next().is_none());

    iter.reset();

    let second_walk = collect_walk(&mut iter);
    assert_eq!(first_walk, second_walk);
    // And reset is idempotent — calling it on an already-rewound iterator
    // mid-walk also restarts cleanly.
    let _ = iter.next();
    let _ = iter.next();
    iter.reset();
    let third_walk = collect_walk(&mut iter);
    assert_eq!(first_walk, third_walk);
}

#[test]
fn test_reset_sequential_iterator_contiguous_chunks() {
    // CHUNK[INT] with rangeConstraint=CONTIGUOUS forces the sequential
    // path (`needs_sequential = adaptive || has_contiguous_chunks`).
    // `reset()` here delegates to the inner `node_iter.reset()`. No
    // adaptive chunking — `targetRuntimeSeconds` is omitted — so the
    // chunk size is fixed at `defaultTaskCount = 10` and a re-walk
    // produces the same chunks deterministically.
    let mut iter = iterator_from_template(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["TASK_CHUNKING"],
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {"taskParameterDefinitions": [
                {"name": "Frame", "type": "CHUNK[INT]", "range": "1-35",
                 "chunks": {"defaultTaskCount": 10, "rangeConstraint": "CONTIGUOUS"}}
            ]}
        }]
    }"#,
        &["TASK_CHUNKING"],
    );

    assert!(!iter.chunks_adaptive());

    let first_walk = collect_walk(&mut iter);
    // 35 frames divided into 10-frame chunks, evenly spread:
    // four chunks of {9, 9, 9, 8} or similar — exact split is implementation-
    // defined, but the count is what we lock in.
    assert_eq!(first_walk.len(), 4);
    assert!(iter.next().is_none());

    iter.reset();

    let second_walk = collect_walk(&mut iter);
    assert_eq!(first_walk, second_walk);
}

#[test]
fn test_reset_preserves_adaptive_chunk_size_override() {
    // Adaptive iterator: `targetRuntimeSeconds > 0` means the chunk size
    // is mutable at runtime via `set_chunks_default_task_count`. `reset()`
    // must not undo that override — long-lived owners (e.g. a binding
    // wrapper that exposes the iterator across multiple FFI calls) rely
    // on tuned chunk sizes surviving rewind.
    let mut iter = iterator_from_template(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["TASK_CHUNKING"],
        "name": "Job",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {"taskParameterDefinitions": [
                {"name": "Frame", "type": "CHUNK[INT]", "range": "1-35",
                 "chunks": {"defaultTaskCount": 10, "targetRuntimeSeconds": 20,
                            "rangeConstraint": "CONTIGUOUS"}}
            ]}
        }]
    }"#,
        &["TASK_CHUNKING"],
    );

    assert!(iter.chunks_adaptive());
    assert_eq!(iter.chunks_default_task_count(), Some(10));

    // Initial walk at chunk size 10: ceil(35/10) = 4 chunks.
    let walk_at_10 = collect_walk(&mut iter);
    assert_eq!(walk_at_10.len(), 4);

    // Shrink the chunk size *before* resetting.
    iter.set_chunks_default_task_count(5);
    assert_eq!(iter.chunks_default_task_count(), Some(5));

    iter.reset();

    // The override survives reset — reading the property after reset
    // returns the new value, not the template's original 10.
    assert_eq!(iter.chunks_default_task_count(), Some(5));

    // And it actually drives chunking on the next walk: ceil(35/5) = 7.
    let walk_at_5 = collect_walk(&mut iter);
    assert_eq!(walk_at_5.len(), 7);

    // Walking a third time after another reset still uses chunk size 5.
    iter.reset();
    let walk_at_5_again = collect_walk(&mut iter);
    assert_eq!(walk_at_5, walk_at_5_again);
}

// ═══════════════════════════════════════════════════════════════════════════
// Regression: contains() on nested combination expressions
// ═══════════════════════════════════════════════════════════════════════════

/// `contains()` must accept values yielded by an iterator over a nested
/// combination expression — e.g. `A * (B, C * D)` — where an
/// association sits inside a product. The recursive
/// `validate_containment` traversal must correctly project the input
/// `params` onto each association's own keys before testing
/// containment, otherwise the association sees keys from its product
/// peers and reports them as a length mismatch.
///
/// Regression test for the bug noted in
/// `openjd-model-for-python/reports/model-bindings-quality-evaluation-report.md`
/// finding #2 ("Remaining issue (nested combination expressions)").
#[test]
fn test_contains_nested_association_in_product() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{
            "name": "step",
            "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "Param1", "type": "INT", "range": [1, 2]},
                    {"name": "Param2", "type": "STRING", "range": ["a", "b", "c", "d"]},
                    {"name": "Param3", "type": "INT", "range": [10, 11]},
                    {"name": "Param4", "type": "INT", "range": [20, 21]}
                ],
                "combination": "Param1 * ( Param2, Param3 * Param4 )"
            }
        }]
    }"#,
    );
    let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
    let processed = preprocess_job_parameters(
        &jt,
        &JobParameterInputValues::new(),
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "/tmp",
            current_working_dir: "/tmp",
            allow_template_dir_walk_up: true,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    let job = create_job(&jt, &processed, &jt.default_validation_context()).unwrap();
    let ps = job.steps[0].parameter_space.as_ref().unwrap();
    let mut iter = StepParameterSpaceIterator::new(ps).unwrap();

    // Collect every yielded value, then assert each is recognized by
    // `contains`. The association `(Param2, Param3 * Param4)` pairs
    // its iteration index against `(Param2[i], Param3*Param4[i])`, so
    // the yielded values exercise both the outer product and the
    // nested association branches of the validate_containment
    // traversal.
    let all_yielded: Vec<openjd_model::types::TaskParameterSet> = iter.by_ref().collect();

    // Sanity: nesting expands to 2 (Param1) * 4 (the association: 4
    // entries because Param2 has 4 elements and the inner Product
    // Param3*Param4 has 2*2=4 elements).
    assert_eq!(all_yielded.len(), 8, "yielded wrong number of values");

    for (i, value) in all_yielded.iter().enumerate() {
        assert!(
            iter.contains(value),
            "iter.contains() returned false for yielded value #{i}: {value:?}\n\
             validate_containment: {:?}",
            iter.validate_containment(value),
        );
    }
}

/// Companion of [`test_contains_nested_association_in_product`] for
/// the rejection direction — values that should NOT be in the space
/// are correctly rejected.
#[test]
fn test_contains_rejects_invalid_nested_values() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{
            "name": "step",
            "script": {"actions": {"onRun": {"command": "echo"}}},
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "Param1", "type": "INT", "range": [1, 2]},
                    {"name": "Param2", "type": "STRING", "range": ["a", "b"]},
                    {"name": "Param3", "type": "INT", "range": [10, 11]}
                ],
                "combination": "Param1 * ( Param2, Param3 )"
            }
        }]
    }"#,
    );
    let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
    let processed = preprocess_job_parameters(
        &jt,
        &JobParameterInputValues::new(),
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "/tmp",
            current_working_dir: "/tmp",
            allow_template_dir_walk_up: true,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    let job = create_job(&jt, &processed, &jt.default_validation_context()).unwrap();
    let ps = job.steps[0].parameter_space.as_ref().unwrap();
    let iter = StepParameterSpaceIterator::new(ps).unwrap();

    // Values in the space (the association pairs ("a", 10) and ("b", 11)):
    //   Param1=1 ⨯ ("a", 10), Param1=1 ⨯ ("b", 11),
    //   Param1=2 ⨯ ("a", 10), Param1=2 ⨯ ("b", 11)
    let make_set = |p1: i64, p2: &str, p3: i64| {
        let mut s = openjd_model::types::TaskParameterSet::new();
        s.insert(
            "Param1".to_string(),
            openjd_model::types::TaskParameterValue {
                param_type: openjd_model::types::TaskParameterType::Int,
                value: openjd_expr::ExprValue::Int(p1),
            },
        );
        s.insert(
            "Param2".to_string(),
            openjd_model::types::TaskParameterValue {
                param_type: openjd_model::types::TaskParameterType::String,
                value: openjd_expr::ExprValue::String(p2.to_string()),
            },
        );
        s.insert(
            "Param3".to_string(),
            openjd_model::types::TaskParameterValue {
                param_type: openjd_model::types::TaskParameterType::Int,
                value: openjd_expr::ExprValue::Int(p3),
            },
        );
        s
    };

    // In the space.
    assert!(iter.contains(&make_set(1, "a", 10)));
    assert!(iter.contains(&make_set(2, "b", 11)));

    // The association pair ("a", 11) is NOT in the space — Param2/Param3
    // are tied lockstep to ("a", 10) or ("b", 11). Even though Param1=1
    // is valid, the cross is rejected. Critically, the diagnostic
    // message reports only the failing association's keys
    // (`Param2`/`Param3`), not the sibling-product key (`Param1`).
    assert!(!iter.contains(&make_set(1, "a", 11)));
    let err = iter
        .validate_containment(&make_set(1, "a", 11))
        .unwrap_err();
    assert_eq!(
        err,
        "The values {Param2=a, Param3=11}, of an association expression in the combination expression, do not appear in the parameter space.",
        "diagnostic must report only the association's own keys"
    );

    assert!(!iter.contains(&make_set(2, "b", 10)));
    let err = iter
        .validate_containment(&make_set(2, "b", 10))
        .unwrap_err();
    assert_eq!(
        err,
        "The values {Param2=b, Param3=10}, of an association expression in the combination expression, do not appear in the parameter space."
    );

    // Param1 outside its range — still rejected. The diagnostic comes
    // from the leaf node's `validate_containment` (not the
    // association's), and reports the offending parameter and value.
    assert!(!iter.contains(&make_set(3, "a", 10)));
    let err = iter
        .validate_containment(&make_set(3, "a", 10))
        .unwrap_err();
    assert_eq!(
        err,
        "Parameter 'Param1' value '3' is not in the parameter space range."
    );
}

#[test]
fn directly_constructed_space_with_out_of_bound_chunk_values_rejected() {
    // StepParameterSpace is publicly constructible (and deserializable),
    // bypassing create_job's validation. The iterator constructor must
    // re-validate CHUNK[INT] list values against the RangeExpr value
    // bound rather than panicking when a chunk is built mid-iteration.
    use indexmap::IndexMap;
    use openjd_model::job;
    use openjd_model::template::RangeConstraint;

    let mut defs = IndexMap::new();
    defs.insert(
        "Frame".to_string(),
        job::TaskParameter::ChunkInt {
            range: job::TaskParamRange::List(vec![1, 2, 1i64 << 62]),
            chunks: job::ResolvedChunks {
                default_task_count: 2,
                target_runtime_seconds: None,
                range_constraint: RangeConstraint::Noncontiguous,
            },
        },
    );
    let space = job::StepParameterSpace {
        task_parameter_definitions: defs,
        combination: None,
    };
    let err = match StepParameterSpaceIterator::new(&space) {
        Err(e) => e,
        Ok(_) => panic!("expected out-of-bound chunk value to be rejected"),
    };
    assert_eq!(
        err.to_string(),
        "Validation error: Task parameter 'Frame': value 4611686018427387904 \
         exceeds the CHUNK[INT] range value bound (magnitude must be below 2^62)"
    );
}
