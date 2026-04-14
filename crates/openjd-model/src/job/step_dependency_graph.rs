// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Step dependency graph for instantiated jobs.

use std::collections::HashMap;

use crate::error::OpenJdError;
use crate::job;

type NodeIndex = usize;

/// A step-to-step dependency edge.
#[derive(Debug)]
pub struct StepDependencyEdge {
    /// Index of the step that is depended upon.
    pub origin: NodeIndex,
    /// Index of the step that depends on origin.
    pub dependent: NodeIndex,
}

/// A node in the dependency graph.
#[derive(Debug)]
pub struct StepDependencyNode {
    /// The step this node represents (index into job.steps).
    pub step_index: usize,
    /// The step name.
    pub name: String,
    /// Edges where this step depends on another (this step is the dependent).
    pub in_edges: Vec<usize>,
    /// Edges where another step depends on this one (this step is the origin).
    pub out_edges: Vec<usize>,
}

/// Dependency graph over the steps of an instantiated job.
#[derive(Debug)]
pub struct StepDependencyGraph {
    nodes: Vec<StepDependencyNode>,
    edges: Vec<StepDependencyEdge>,
    name_to_index: HashMap<String, NodeIndex>,
}

impl StepDependencyGraph {
    /// Build the dependency graph from a Job.
    pub fn new(job: &job::Job) -> Result<Self, OpenJdError> {
        let name_to_index: HashMap<String, NodeIndex> = job
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| (s.name.clone(), i))
            .collect();

        let mut nodes: Vec<StepDependencyNode> = job
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| StepDependencyNode {
                step_index: i,
                name: s.name.clone(),
                in_edges: Vec::new(),
                out_edges: Vec::new(),
            })
            .collect();

        let mut edges = Vec::new();

        for (dep_idx, step) in job.steps.iter().enumerate() {
            if let Some(deps) = &step.dependencies {
                for dep in deps {
                    let origin_idx = *name_to_index.get(&dep.depends_on).ok_or_else(|| {
                        OpenJdError::DecodeValidation(format!(
                            "Step '{}' depends on unknown step '{}'",
                            step.name, dep.depends_on
                        ))
                    })?;
                    let edge_idx = edges.len();
                    edges.push(StepDependencyEdge {
                        origin: origin_idx,
                        dependent: dep_idx,
                    });
                    nodes[dep_idx].in_edges.push(edge_idx);
                    nodes[origin_idx].out_edges.push(edge_idx);
                }
            }
        }

        Ok(Self {
            nodes,
            edges,
            name_to_index,
        })
    }

    /// Get a node by step name.
    pub fn step_node(&self, name: &str) -> Option<&StepDependencyNode> {
        self.name_to_index.get(name).map(|&i| &self.nodes[i])
    }

    /// Maximum in-degree (max number of dependencies any step has).
    pub fn max_indegree(&self) -> usize {
        self.nodes
            .iter()
            .map(|n| n.in_edges.len())
            .max()
            .unwrap_or(0)
    }

    /// Maximum out-degree (max number of steps that depend on any single step).
    pub fn max_outdegree(&self) -> usize {
        self.nodes
            .iter()
            .map(|n| n.out_edges.len())
            .max()
            .unwrap_or(0)
    }

    /// Stable topological sort matching the Python implementation.
    ///
    /// DFS-based: processes nodes in template order. For each unvisited node,
    /// pushes it onto a stack and explores dependencies (pushed in reverse
    /// template order so the earliest dependency is processed first).
    /// Returns step indices in dependency-respecting, template-stable order.
    pub fn topo_sorted(&self) -> Result<Vec<usize>, OpenJdError> {
        let n = self.nodes.len();
        // 0 = unvisited, 1 = started, 2 = completed
        let mut state = vec![0u8; n];
        let mut result = Vec::with_capacity(n);

        for i in 0..n {
            if state[i] != 0 {
                continue;
            }
            let mut stack: Vec<NodeIndex> = vec![i];
            while let Some(&top) = stack.last() {
                match state[top] {
                    2 => {
                        stack.pop();
                    }
                    1 => {
                        // Second visit — mark completed
                        state[top] = 2;
                        result.push(top);
                        stack.pop();
                    }
                    _ => {
                        // First visit — mark started, push unfinished deps
                        state[top] = 1;
                        // Collect dependency indices, sort by reverse template index
                        let mut dep_indices: Vec<NodeIndex> = self.nodes[top]
                            .in_edges
                            .iter()
                            .map(|&e| self.edges[e].origin)
                            .collect();
                        dep_indices.sort_unstable_by(|a, b| b.cmp(a));
                        for dep in dep_indices {
                            match state[dep] {
                                2 => {} // already done
                                1 => {
                                    // Build cycle path from the stack
                                    let cycle: Vec<&str> = stack
                                        .iter()
                                        .filter(|&&idx| state[idx] == 1)
                                        .map(|&idx| self.nodes[idx].name.as_str())
                                        .collect();
                                    let dep_name = &self.nodes[dep].name;
                                    return Err(OpenJdError::DecodeValidation(format!(
                                        "A circular dependency was found in the step dependency graph:\n{} -> {}",
                                        cycle.join(" -> "), dep_name
                                    )));
                                }
                                _ => stack.push(dep),
                            }
                        }
                    }
                }
            }
        }

        Ok(result)
    }

    /// Convenience: topological sort returning step names.
    pub fn topo_sorted_names(&self) -> Result<Vec<String>, OpenJdError> {
        self.topo_sorted().map(|indices| {
            indices
                .into_iter()
                .map(|i| self.nodes[i].name.clone())
                .collect()
        })
    }
}
