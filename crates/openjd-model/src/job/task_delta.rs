// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Task delta between two step parameter spaces, without enumerating tasks.
//!
//! **Unstable.** This API may change or be removed in any release without
//! notice. Don't depend on it outside openjd-rs.
//!
//! When both spaces are a plain Cartesian product over the same parameters
//! (no association, no chunking), the change is exact box algebra. With
//! per-parameter value sets `O_k` (old) and `N_k` (new), and `I_k = O_k ∩ N_k`:
//!
//! ```text
//! kept      = I_0 × I_1 × … × I_{D-1}
//! added_k   = I_0 × … × I_{k-1} × (N_k \ O_k) × N_{k+1} × … × N_{D-1}
//! removed_k = I_0 × … × I_{k-1} × (O_k \ N_k) × O_{k+1} × … × O_{D-1}
//! ```
//!
//! The `added_k` are pairwise disjoint and their union is exactly `new \ kept`
//! (likewise `removed_k` and `old \ kept`), so the delta is at most `2D + 1`
//! boxes. Each [`TaskBox`] is itself a [`StepParameterSpace`], so its tasks are
//! iterated with [`StepParameterSpaceIterator`] and counted without iteration.
//! Cost is linear in the per-parameter range sizes, not in their product.
//!
//! Anything else (association, chunking, a different dimension order or
//! order of shared values in a range, a
//! range that repeats a value, a
//! task count that overflows, a parameter added, removed or
//! retyped) returns [`TaskDelta::Fallback`]: there the caller enumerates both
//! spaces and diffs tasks by value.

use std::collections::HashSet;

use indexmap::IndexMap;
use openjd_expr::value::Float64;
use openjd_expr::ExprValue;

use crate::error::ModelError;
use crate::job::{StepParameterSpace, TaskParamRange, TaskParameter};
use crate::step_param_space::StepParameterSpaceIterator;
use crate::types::TaskParameterSet;

/// A Cartesian product of per-parameter value lists: a set of tasks.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskBox {
    space: StepParameterSpace,
}

impl TaskBox {
    /// The box as a parameter space: every range is a list, combination is
    /// the product in definition order.
    #[must_use]
    pub fn space(&self) -> &StepParameterSpace {
        &self.space
    }

    /// Number of tasks: the product of the range lengths. Saturates on overflow.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.space
            .task_parameter_definitions
            .values()
            .map(range_len)
            .fold(1u64, u64::saturating_mul)
    }

    /// Whether the box has no tasks.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterate the box's tasks, rightmost parameter fastest.
    pub fn tasks(&self) -> Result<StepParameterSpaceIterator, ModelError> {
        StepParameterSpaceIterator::new(&self.space)
    }

    /// Row-major position of `task` in this box (rightmost parameter
    /// fastest, matching [`StepParameterSpaceIterator`]), or `None` when the
    /// task is not in the box. For the old space's full box this is the
    /// task's index at job creation. O(sum of range lengths) per call.
    #[must_use]
    pub fn index_of(&self, task: &TaskParameterSet) -> Option<u64> {
        let mut index = 0u64;
        for (name, param) in &self.space.task_parameter_definitions {
            let key = expr_key(&task.get(name)?.value)?;
            let keys = range_keys(param);
            let pos = count(keys.iter().position(|k| *k == key)?);
            index = index.checked_mul(count(keys.len()))?.checked_add(pos)?;
        }
        Some(index)
    }
}

/// How the tasks of a step change between two parameter spaces.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskDelta {
    /// Same task set (the spaces are equal).
    Unchanged,
    /// Exact product decomposition.
    Product(Box<ProductDelta>),
    /// Not a plain product on both sides; enumerate and diff by value.
    Fallback {
        /// Why the product decomposition does not apply.
        reason: String,
    },
}

/// The exact decomposition of a product-to-product change. Boxes are
/// disjoint; empty boxes are omitted.
#[derive(Debug, Clone, PartialEq)]
pub struct ProductDelta {
    /// The whole old space, for [`TaskBox::index_of`].
    pub old: TaskBox,
    /// Tasks in both spaces.
    pub kept: TaskBox,
    /// Tasks only in the new space, in new iteration order per box.
    pub added: Vec<TaskBox>,
    /// Tasks only in the old space.
    pub removed: Vec<TaskBox>,
}

/// Whether definition order decides task order: no `combination`, or `*`,
/// which is the plain product in definition order. An explicit combination
/// decides task order itself.
pub(crate) fn definitions_fix_task_order(space: &StepParameterSpace) -> bool {
    matches!(
        space.combination.as_deref().map(str::trim),
        None | Some("*")
    )
}

/// Compute the task delta between two steps' parameter spaces. `None` is a
/// step without a parameter space (one task with no parameters).
pub fn task_delta(old: Option<&StepParameterSpace>, new: Option<&StepParameterSpace>) -> TaskDelta {
    if old == new {
        // `IndexMap ==` ignores definition order, but without an explicit
        // combination that order is the task order, so every task index moves.
        let order_changed = matches!((old, new), (Some(a), Some(b))
            if definitions_fix_task_order(a)
                && !a.task_parameter_definitions.keys().eq(b.task_parameter_definitions.keys()));
        if order_changed {
            return TaskDelta::Fallback {
                reason: "task parameter definition order changed, so task order changed".into(),
            };
        }
        return TaskDelta::Unchanged;
    }
    let fallback = |reason: &str| TaskDelta::Fallback {
        reason: reason.to_string(),
    };
    let (Some(old), Some(new)) = (old, new) else {
        return fallback("parameter space added or removed");
    };
    let (Some(old_dims), Some(new_dims)) = (product_dims(old), product_dims(new)) else {
        return fallback(
            "not an exact plain product: association, chunking, a repeated range value or name, or a task count overflow",
        );
    };
    let old_names: HashSet<&str> = old_dims.iter().map(|(n, _)| n.as_str()).collect();
    let new_names: HashSet<&str> = new_dims.iter().map(|(n, _)| n.as_str()).collect();
    if old_names != new_names {
        return fallback("task parameter added or removed");
    }
    // A different dimension order changes the iteration order of every kept
    // task, which boxes cannot express.
    if !old_dims
        .iter()
        .map(|(n, _)| n)
        .eq(new_dims.iter().map(|(n, _)| n))
    {
        return fallback("task parameter iteration order changed, so task order changed");
    }
    // Decompose in the new side's dimension order.
    let mut dims = Vec::with_capacity(new_dims.len());
    for (name, n) in &new_dims {
        let Some((_, o)) = old_dims.iter().find(|(m, _)| m == name) else {
            return fallback("task parameter added or removed");
        };
        if std::mem::discriminant(*o) != std::mem::discriminant(*n) {
            return fallback("task parameter type changed");
        }
        dims.push((name.as_str(), *o, *n));
    }
    // The values both sides share must keep their relative order in every
    // dimension, or kept tasks are reordered, which boxes cannot express.
    for &(_, o, n) in &dims {
        let (ok, nk) = (range_keys(o), range_keys(n));
        let (os, ns): (HashSet<&String>, HashSet<&String>) =
            (ok.iter().collect(), nk.iter().collect());
        if !ok
            .iter()
            .filter(|k| ns.contains(k))
            .eq(nk.iter().filter(|k| os.contains(k)))
        {
            return fallback("a range's shared values changed order, so task order changed");
        }
    }

    let kept: Vec<(&str, TaskParameter)> = dims
        .iter()
        .map(|&(name, o, n)| {
            let old_keys: HashSet<String> = range_keys(o).into_iter().collect();
            (name, filter(n, |k| old_keys.contains(k)))
        })
        .collect();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    for k in 0..dims.len() {
        let make = |side_is_new: bool| -> TaskBox {
            let defs = dims.iter().enumerate().map(|(j, &(name, o, n))| {
                let p = if j < k {
                    kept[j].1.clone()
                } else if j == k {
                    let (this, other) = if side_is_new { (n, o) } else { (o, n) };
                    let other_keys: HashSet<String> = range_keys(other).into_iter().collect();
                    filter(this, |key| !other_keys.contains(key))
                } else if side_is_new {
                    as_list(n)
                } else {
                    as_list(o)
                };
                (name.to_string(), p)
            });
            make_box(defs)
        };
        let a = make(true);
        if !a.is_empty() {
            added.push(a);
        }
        let r = make(false);
        if !r.is_empty() {
            removed.push(r);
        }
    }
    TaskDelta::Product(Box::new(ProductDelta {
        old: make_box(old_dims.iter().map(|(n, p)| (n.clone(), as_list(p)))),
        kept: make_box(kept.into_iter().map(|(n, p)| (n.to_string(), p))),
        added,
        removed,
    }))
}

fn make_box(defs: impl Iterator<Item = (String, TaskParameter)>) -> TaskBox {
    TaskBox {
        space: StepParameterSpace {
            task_parameter_definitions: defs.collect::<IndexMap<_, _>>(),
            combination: None,
        },
    }
}

/// The dimensions of a plain product, in iteration order (outermost first),
/// or `None` when the decomposition would not be exact: association or any
/// other non-`*` syntax, a repeated or unknown name, chunking, a range that
/// repeats a value (the algebra is over sets), or a task count that
/// overflows `u64` (the iterator rejects those).
fn product_dims(space: &StepParameterSpace) -> Option<Vec<(String, &TaskParameter)>> {
    let defs = &space.task_parameter_definitions;
    let names: Vec<String> = match space.combination.as_deref().map(str::trim) {
        None | Some("*") => defs.keys().cloned().collect(),
        Some(c) => c.split('*').map(|s| s.trim().to_string()).collect(),
    };
    let unique: HashSet<&str> = names.iter().map(String::as_str).collect();
    if names.len() != defs.len() || unique.len() != names.len() {
        return None;
    }
    let mut total = 1u64;
    names
        .into_iter()
        .map(|n| {
            // An unknown name also covers "(A, B)" and "" here.
            let p = defs.get(&n)?;
            if matches!(
                p,
                TaskParameter::Int {
                    chunks: Some(_),
                    ..
                } | TaskParameter::ChunkInt { .. }
            ) {
                return None;
            }
            let keys = range_keys(p);
            if keys.iter().collect::<HashSet<_>>().len() != keys.len() {
                return None;
            }
            total = total.checked_mul(count(keys.len()))?;
            Some((n, p))
        })
        .collect()
}

fn expr_key(v: &ExprValue) -> Option<String> {
    Some(match v {
        ExprValue::Int(i) => format!("i:{i}"),
        ExprValue::Float(f) => format!("f:{}", f.to_display_string()),
        ExprValue::String(s) => format!("s:{s}"),
        ExprValue::Path { value, .. } => format!("s:{value}"),
        _ => return None,
    })
}

fn range_keys(p: &TaskParameter) -> Vec<String> {
    match p {
        TaskParameter::Int { range, .. } | TaskParameter::ChunkInt { range, .. } => {
            int_values(range).map(|i| format!("i:{i}")).collect()
        }
        TaskParameter::Float { range } => range
            .iter()
            .map(|f| format!("f:{}", f.to_display_string()))
            .collect(),
        TaskParameter::String { range } | TaskParameter::Path { range } => {
            range.iter().map(|s| format!("s:{s}")).collect()
        }
    }
}

fn int_values(r: &TaskParamRange<i64>) -> Box<dyn Iterator<Item = i64> + '_> {
    match r {
        TaskParamRange::List(v) => Box::new(v.iter().copied()),
        TaskParamRange::RangeExpr(e) => Box::new(e.iter()),
    }
}

fn range_len(p: &TaskParameter) -> u64 {
    match p {
        TaskParameter::Int { range, .. } | TaskParameter::ChunkInt { range, .. } => match range {
            TaskParamRange::List(v) => count(v.len()),
            TaskParamRange::RangeExpr(e) => e.len(),
        },
        TaskParameter::Float { range } => count(range.len()),
        TaskParameter::String { range } | TaskParameter::Path { range } => count(range.len()),
    }
}

/// The length of a list held in memory as a `u64` count. Saturates, though
/// a `usize` fits `u64` on every target Rust supports.
fn count(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

fn as_list(p: &TaskParameter) -> TaskParameter {
    filter(p, |_| true)
}

/// The same parameter with only the values whose key satisfies `keep`, as a
/// list range, preserving order.
fn filter(p: &TaskParameter, keep: impl Fn(&String) -> bool) -> TaskParameter {
    match p {
        TaskParameter::Int { range, .. } | TaskParameter::ChunkInt { range, .. } => {
            TaskParameter::Int {
                range: TaskParamRange::List(
                    int_values(range)
                        .filter(|i| keep(&format!("i:{i}")))
                        .collect(),
                ),
                chunks: None,
            }
        }
        TaskParameter::Float { range } => TaskParameter::Float {
            range: range
                .iter()
                .filter(|f| keep(&format!("f:{}", f.to_display_string())))
                .cloned()
                .collect::<Vec<Float64>>(),
        },
        TaskParameter::String { range } => TaskParameter::String {
            range: range
                .iter()
                .filter(|s| keep(&format!("s:{s}")))
                .cloned()
                .collect(),
        },
        TaskParameter::Path { range } => TaskParameter::Path {
            range: range
                .iter()
                .filter(|s| keep(&format!("s:{s}")))
                .cloned()
                .collect(),
        },
    }
}
