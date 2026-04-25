// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

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
    let job = create_job(&jt, &processed, &CallerLimits::default()).unwrap();
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
    let job = create_job(&jt, &processed, &CallerLimits::default()).unwrap();
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
    let job = create_job(&jt, &processed, &CallerLimits::default()).unwrap();
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

    // Build one that should NOT be in the space
    let mut not_in_set = openjd_model::types::TaskParameterSet::new();
    not_in_set.insert(
        "Param1".to_string(),
        openjd_model::types::TaskParameterValue {
            param_type: openjd_model::types::TaskParameterType::Int,
            value: openjd_expr::ExprValue::Int(9),
        },
    );
    assert!(!iter.contains(&not_in_set));
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
    let job = create_job(&jt, &processed, &CallerLimits::default()).unwrap();
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
            let job = create_job(&jt, &processed, &CallerLimits::default()).unwrap();
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
    let job = create_job(&jt, &params, &CallerLimits::default()).unwrap();
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
    let msg = create_job(&jt, &params, &CallerLimits::default())
        .unwrap_err()
        .to_string();
    assert!(
        msg.contains("parameter space") || msg.contains("overflow"),
        "Expected overflow error message, got: {msg}"
    );
}
