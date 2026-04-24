// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use indexmap::IndexMap;

use openjd_expr::FormatString;
use openjd_model::decode_job_template;
use openjd_model::job::{Action, Job, Step, StepActions, StepDependency, StepScript};
use openjd_model::step_dependency_graph::StepDependencyGraph;
use openjd_model::CallerLimits;

fn make_job(steps: Vec<(&str, Vec<&str>)>) -> Job {
    Job {
        name: "TestJob".to_string(),
        description: None,
        extensions: None,
        parameters: IndexMap::new(),
        steps: steps
            .into_iter()
            .map(|(name, deps)| Step {
                name: name.to_string(),
                description: None,
                script: StepScript {
                    let_bindings: None,
                    actions: StepActions {
                        on_run: Action {
                            command: FormatString::new("echo").unwrap(),
                            args: None,
                            timeout: None,
                            cancelation: None,
                        },
                    },
                    embedded_files: None,
                },
                step_environments: None,
                parameter_space: None,
                host_requirements: None,
                dependencies: if deps.is_empty() {
                    None
                } else {
                    Some(
                        deps.into_iter()
                            .map(|d| StepDependency {
                                depends_on: d.to_string(),
                            })
                            .collect(),
                    )
                },
                resolved_symtab: None,
            })
            .collect(),
        job_environments: None,
    }
}

#[test]
fn test_no_dependencies() {
    let job = make_job(vec![("A", vec![]), ("B", vec![]), ("C", vec![])]);
    let graph = StepDependencyGraph::new(&job).unwrap();
    assert_eq!(graph.topo_sorted().unwrap(), vec![0, 1, 2]);
    assert_eq!(graph.max_indegree(), 0);
    assert_eq!(graph.max_outdegree(), 0);
}

#[test]
fn test_linear_chain() {
    // A → B → C (C depends on B, B depends on A)
    let job = make_job(vec![("A", vec![]), ("B", vec!["A"]), ("C", vec!["B"])]);
    let graph = StepDependencyGraph::new(&job).unwrap();
    assert_eq!(graph.topo_sorted().unwrap(), vec![0, 1, 2]);
}

#[test]
fn test_diamond() {
    // A, B depends on A, C depends on A, D depends on B and C
    let job = make_job(vec![
        ("A", vec![]),
        ("B", vec!["A"]),
        ("C", vec!["A"]),
        ("D", vec!["B", "C"]),
    ]);
    let graph = StepDependencyGraph::new(&job).unwrap();
    assert_eq!(graph.topo_sorted().unwrap(), vec![0, 1, 2, 3]);
}

#[test]
fn test_reverse_order_deps() {
    // Steps defined as [C, B, A] where C depends on B, B depends on A
    // topo_sorted should return ["A", "B", "C"] (A first, then B, then C)
    let job = make_job(vec![("C", vec!["B"]), ("B", vec!["A"]), ("A", vec![])]);
    let graph = StepDependencyGraph::new(&job).unwrap();
    assert_eq!(graph.topo_sorted().unwrap(), vec![2, 1, 0]);
}

#[test]
fn test_step_node_lookup() {
    let job = make_job(vec![("A", vec![]), ("B", vec!["A"]), ("C", vec!["B"])]);
    let graph = StepDependencyGraph::new(&job).unwrap();
    let node_b = graph.step_node("B").unwrap();
    assert_eq!(node_b.name, "B");
    assert_eq!(node_b.step_index, 1);
    // B has 1 in_edge (depends on A) and 1 out_edge (C depends on B)
    assert_eq!(node_b.in_edges.len(), 1);
    assert_eq!(node_b.out_edges.len(), 1);
    assert!(graph.step_node("Z").is_none());
}

#[test]
fn test_unknown_dependency_error() {
    let job = make_job(vec![("A", vec!["NonExistent"])]);
    let err = StepDependencyGraph::new(&job).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Validation error: Step 'A' depends on unknown step 'NonExistent'"
    );
}

#[test]
fn test_max_degrees() {
    // Diamond: A has out-degree 2, D has in-degree 2
    let job = make_job(vec![
        ("A", vec![]),
        ("B", vec!["A"]),
        ("C", vec!["A"]),
        ("D", vec!["B", "C"]),
    ]);
    let graph = StepDependencyGraph::new(&job).unwrap();
    assert_eq!(graph.max_indegree(), 2);
    assert_eq!(graph.max_outdegree(), 2);
}

#[test]
fn test_topo_sorted_names() {
    let job = make_job(vec![("C", vec!["B"]), ("B", vec!["A"]), ("A", vec![])]);
    let graph = StepDependencyGraph::new(&job).unwrap();
    assert_eq!(graph.topo_sorted_names().unwrap(), vec!["A", "B", "C"]);
}

// ══════════════════════════════════════════════════════════════
// Ported from Python test_topo_sort_dep_order
// ══════════════════════════════════════════════════════════════

/// Python test: S1 depends on [S2,S3,S5,S6,S7] in various orders.
/// The topo sort should always produce the same stable result.
#[test]
fn test_topo_sort_dep_order_sorted() {
    // S1 depends on S2,S3,S5,S6,S7 (sorted order)
    let job = make_job(vec![
        ("S1", vec!["S2", "S3", "S5", "S6", "S7"]),
        ("S2", vec![]),
        ("S3", vec![]),
        ("S4", vec![]),
        ("S5", vec![]),
        ("S6", vec![]),
        ("S7", vec![]),
    ]);
    let graph = StepDependencyGraph::new(&job).unwrap();
    let names = graph.topo_sorted_names().unwrap();
    assert_eq!(names, vec!["S2", "S3", "S5", "S6", "S7", "S1", "S4"]);
}

#[test]
fn test_topo_sort_dep_order_reverse() {
    // S1 depends on S7,S6,S5,S3,S2 (reverse sorted)
    let job = make_job(vec![
        ("S1", vec!["S7", "S6", "S5", "S3", "S2"]),
        ("S2", vec![]),
        ("S3", vec![]),
        ("S4", vec![]),
        ("S5", vec![]),
        ("S6", vec![]),
        ("S7", vec![]),
    ]);
    let graph = StepDependencyGraph::new(&job).unwrap();
    let names = graph.topo_sorted_names().unwrap();
    assert_eq!(names, vec!["S2", "S3", "S5", "S6", "S7", "S1", "S4"]);
}

#[test]
fn test_topo_sort_dep_order_random() {
    // S1 depends on S2,S6,S5,S7,S3 (random order)
    let job = make_job(vec![
        ("S1", vec!["S2", "S6", "S5", "S7", "S3"]),
        ("S2", vec![]),
        ("S3", vec![]),
        ("S4", vec![]),
        ("S5", vec![]),
        ("S6", vec![]),
        ("S7", vec![]),
    ]);
    let graph = StepDependencyGraph::new(&job).unwrap();
    let names = graph.topo_sorted_names().unwrap();
    assert_eq!(names, vec!["S2", "S3", "S5", "S6", "S7", "S1", "S4"]);
}

// ══════════════════════════════════════════════════════════════
// Ported from Python test_topo_sort_cycle_error
// ══════════════════════════════════════════════════════════════

#[test]
fn test_cycle_detection() {
    let job = make_job(vec![("A", vec!["B"]), ("B", vec!["A"])]);
    let graph = StepDependencyGraph::new(&job).unwrap();
    assert_eq!(
        graph.topo_sorted().unwrap_err().to_string(),
        "Validation error: A circular dependency was found in the step dependency graph:\nA -> B -> A"
    );
}

#[test]
fn test_long_cycle_detection() {
    let job = make_job(vec![
        ("S1", vec!["S2"]),
        ("S2", vec!["S3"]),
        ("S3", vec!["S4"]),
        ("S4", vec!["S5"]),
        ("S5", vec!["S6"]),
        ("S6", vec!["S7"]),
        ("S7", vec!["S1"]),
    ]);
    let graph = StepDependencyGraph::new(&job).unwrap();
    assert_eq!(
        graph.topo_sorted().unwrap_err().to_string(),
        "Validation error: A circular dependency was found in the step dependency graph:\n\
         S1 -> S2 -> S3 -> S4 -> S5 -> S6 -> S7 -> S1"
    );
}

fn yaml_val(s: &str) -> serde_yaml::Value {
    serde_yaml::from_str(s).unwrap()
}

#[test]
fn self_referencing_step_dependency_rejected() {
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        steps:
          - name: Step1
            dependencies:
              - dependsOn: Step1
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let result = decode_job_template(template, None, &CallerLimits::default());
    assert!(
        result.is_err(),
        "Self-referencing step dependency should be rejected"
    );
}
