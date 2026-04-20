// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Step parameter space iteration.
//!
//! Provides `StepParameterSpaceIterator` for lazily iterating over the
//! multidimensional space of task parameter values. Operates on resolved
//! `job::StepParameterSpace` types (no SymbolTable needed).
//!
//! Uses a tree of `Node` objects for lazy evaluation:
//! - `RangeExprNode`: computes values on demand via index arithmetic
//! - `ProductNode`: divmod indexing (rightmost moves fastest)
//! - `AssociationNode`: lockstep indexing
//! - `StaticChunkNode`: pre-computed chunk boundaries

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use openjd_expr::value::Float64;
use openjd_expr::{ExprValue, RangeExpr};

use crate::error::OpenJdError;
use crate::job;
use crate::template::RangeConstraint;
use crate::types::{TaskParameterSet, TaskParameterType, TaskParameterValue};

// ── Shared utilities ──

/// Compute the product of child node lengths with overflow checking.
fn checked_product_len(children: &[Box<dyn Node>]) -> Result<usize, OpenJdError> {
    children.iter().try_fold(1usize, |acc, c| {
        acc.checked_mul(c.len()).ok_or_else(|| {
            OpenJdError::DecodeValidation(
                "Total parameter space size overflow: the product of parameter dimensions is too large.".into(),
            )
        })
    })
}

/// Tokenize a combination expression into identifiers and operators.
fn tokenize(expr: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in expr.chars() {
        match ch {
            '*' | '(' | ')' | ',' => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                tokens.push(ch.to_string());
            }
            c if c.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Compress a slice of integers into a compact range expression string.
/// e.g., [1,2,3,5,7,8,9] → "1-3,5,7-9"
fn compress_range_expr(values: &[i64]) -> String {
    if values.is_empty() {
        return String::new();
    }
    let mut parts = Vec::new();
    let mut start = values[0];
    let mut end = values[0];
    for &v in &values[1..] {
        if v == end + 1 {
            end = v;
        } else {
            push_range(&mut parts, start, end);
            start = v;
            end = v;
        }
    }
    push_range(&mut parts, start, end);
    parts.join(",")
}

fn push_range(parts: &mut Vec<String>, start: i64, end: i64) {
    if start == end {
        parts.push(start.to_string());
    } else {
        parts.push(format!("{start}-{end}"));
    }
}

// ── Node trait and implementations ──

/// Internal trait for lazy parameter space tree nodes.
trait Node: Send + Sync {
    fn len(&self) -> usize;
    fn get(&self, index: usize, result: &mut TaskParameterSet);
    /// Validate containment with a detailed error message on failure.
    fn validate_containment(&self, params: &TaskParameterSet) -> Result<(), String>;
    /// Create an iterator over this node's elements.
    fn iter(&self) -> Box<dyn NodeIterator>;
}

/// Iterator trait for node-level iteration (supports adaptive chunking).
trait NodeIterator {
    fn next(&mut self, result: &mut TaskParameterSet) -> bool;
    fn reset(&mut self);
}

/// Simple index-based iterator for non-adaptive nodes.
/// Tracks only index and length — the caller (ProductIterator/AssociationIterator)
/// is responsible for calling `get()` on the original node to populate results.
struct IndexedNodeIterator {
    len: usize,
    index: usize,
}

impl NodeIterator for IndexedNodeIterator {
    fn next(&mut self, _result: &mut TaskParameterSet) -> bool {
        if self.index >= self.len {
            return false;
        }
        self.index += 1;
        true
    }
    fn reset(&mut self) {
        self.index = 0;
    }
}

/// Value-producing iterator for a single parameter with a list of values.
struct RangeListIterator {
    name: String,
    param_type: TaskParameterType,
    values: Vec<ExprValue>,
    index: usize,
}

impl NodeIterator for RangeListIterator {
    fn next(&mut self, result: &mut TaskParameterSet) -> bool {
        if self.index >= self.values.len() {
            return false;
        }
        result.insert(
            self.name.clone(),
            TaskParameterValue {
                param_type: self.param_type,
                value: self.values[self.index].clone(),
            },
        );
        self.index += 1;
        true
    }
    fn reset(&mut self) {
        self.index = 0;
    }
}

/// Value-producing iterator for a single parameter with a RangeExpr.
struct RangeExprIterator {
    name: String,
    range: RangeExpr,
    index: usize,
}

impl NodeIterator for RangeExprIterator {
    fn next(&mut self, result: &mut TaskParameterSet) -> bool {
        if self.index >= self.range.len() {
            return false;
        }
        result.insert(
            self.name.clone(),
            TaskParameterValue {
                param_type: TaskParameterType::Int,
                value: ExprValue::Int(
                    self.range
                        .get(self.index as i64)
                        .expect("index checked against range.len()"),
                ),
            },
        );
        self.index += 1;
        true
    }
    fn reset(&mut self) {
        self.index = 0;
    }
}

/// Value-producing iterator for static chunk nodes.
struct StaticChunkIterator {
    name: String,
    range: job::TaskParamRange<i64>,
    constraint: RangeConstraint,
    num_chunks: usize,
    small: usize,
    leftovers: usize,
    index: usize,
}

impl StaticChunkIterator {
    fn chunk_range_expr(&self, i: usize) -> RangeExpr {
        let size = self.small + if i < self.leftovers { 1 } else { 0 };
        let offset = i * self.small + i.min(self.leftovers);
        match &self.range {
            job::TaskParamRange::RangeExpr(r) => {
                let start = r
                    .get(offset as i64)
                    .expect("chunk offset within range bounds");
                let end = r
                    .get((offset + size - 1) as i64)
                    .expect("chunk end within range bounds");
                let range_str = match self.constraint {
                    RangeConstraint::Contiguous => {
                        if size == 1 {
                            start.to_string()
                        } else {
                            format!("{start}-{end}")
                        }
                    }
                    RangeConstraint::Noncontiguous => {
                        let vals: Vec<i64> = (offset..offset + size)
                            .map(|j| r.get(j as i64).expect("chunk element within range bounds"))
                            .collect();
                        compress_range_expr(&vals)
                    }
                };
                let expr = range_str
                    .parse::<RangeExpr>()
                    .expect("range string built from valid integers");
                match self.constraint {
                    RangeConstraint::Contiguous => expr.with_contiguous(true),
                    RangeConstraint::Noncontiguous => expr,
                }
            }
            job::TaskParamRange::List(values) => {
                let chunk = &values[offset..offset + size];
                let range_str = match self.constraint {
                    RangeConstraint::Contiguous => {
                        if chunk.len() == 1 {
                            chunk[0].to_string()
                        } else {
                            format!("{}-{}", chunk[0], chunk[chunk.len() - 1])
                        }
                    }
                    RangeConstraint::Noncontiguous => compress_range_expr(chunk),
                };
                let expr = range_str
                    .parse::<RangeExpr>()
                    .expect("range string built from valid integers");
                match self.constraint {
                    RangeConstraint::Contiguous => expr.with_contiguous(true),
                    RangeConstraint::Noncontiguous => expr,
                }
            }
        }
    }
}

impl NodeIterator for StaticChunkIterator {
    fn next(&mut self, result: &mut TaskParameterSet) -> bool {
        if self.index >= self.num_chunks {
            return false;
        }
        result.insert(
            self.name.clone(),
            TaskParameterValue {
                param_type: TaskParameterType::ChunkInt,
                value: ExprValue::RangeExpr(self.chunk_range_expr(self.index)),
            },
        );
        self.index += 1;
        true
    }
    fn reset(&mut self) {
        self.index = 0;
    }
}

/// Zero-dimensional space: produces one empty parameter set.
struct ZeroDimSpaceNode;

impl Node for ZeroDimSpaceNode {
    fn len(&self) -> usize {
        1
    }
    fn get(&self, _index: usize, _result: &mut TaskParameterSet) {}
    fn validate_containment(&self, _params: &TaskParameterSet) -> Result<(), String> {
        Ok(())
    }
    fn iter(&self) -> Box<dyn NodeIterator> {
        Box::new(IndexedNodeIterator { len: 1, index: 0 })
    }
}

/// Wraps a parameter name + pre-materialized list of values.
struct RangeListNode {
    name: String,
    param_type: TaskParameterType,
    values: Vec<ExprValue>,
}

impl Node for RangeListNode {
    fn len(&self) -> usize {
        self.values.len()
    }
    fn get(&self, index: usize, result: &mut TaskParameterSet) {
        result.insert(
            self.name.clone(),
            TaskParameterValue {
                param_type: self.param_type,
                value: self.values[index].clone(),
            },
        );
    }
    fn validate_containment(&self, params: &TaskParameterSet) -> Result<(), String> {
        let v = params.get(&self.name).ok_or_else(|| {
            format!(
                "Parameter '{}' not found in the provided parameters.",
                self.name
            )
        })?;
        if self.param_type == TaskParameterType::ChunkInt {
            // Chunk: value must be a RangeExpr whose elements are all in our range
            match &v.value {
                ExprValue::RangeExpr(r) => {
                    for val in r.iter() {
                        if !self
                            .values
                            .iter()
                            .any(|ev| matches!(ev, ExprValue::Int(i) if *i == val))
                        {
                            return Err(format!(
                                "Parameter '{}' value '{}' is not a subset of the range in the parameter space.",
                                self.name, r
                            ));
                        }
                    }
                    Ok(())
                }
                _ => Err(format!(
                    "Parameter '{}' value '{}' is not in the parameter space range.",
                    self.name,
                    v.value.to_display_string()
                )),
            }
        } else if !self.values.iter().any(|ev| expr_value_eq(ev, &v.value)) {
            Err(format!(
                "Parameter '{}' value '{}' is not in the parameter space range.",
                self.name,
                v.value.to_display_string()
            ))
        } else {
            Ok(())
        }
    }
    fn iter(&self) -> Box<dyn NodeIterator> {
        Box::new(RangeListIterator {
            name: self.name.clone(),
            param_type: self.param_type,
            values: self.values.clone(),
            index: 0,
        })
    }
}

/// Wraps a parameter name + `RangeExpr`; computes values on demand.
struct RangeExprNode {
    name: String,
    range: RangeExpr,
}

impl Node for RangeExprNode {
    fn len(&self) -> usize {
        self.range.len()
    }
    fn get(&self, index: usize, result: &mut TaskParameterSet) {
        let val = self
            .range
            .get(index as i64)
            .expect("caller must pass index < self.range.len()");
        result.insert(
            self.name.clone(),
            TaskParameterValue {
                param_type: TaskParameterType::Int,
                value: ExprValue::Int(val),
            },
        );
    }
    fn validate_containment(&self, params: &TaskParameterSet) -> Result<(), String> {
        let v = params.get(&self.name).ok_or_else(|| {
            format!(
                "Parameter '{}' not found in the provided parameters.",
                self.name
            )
        })?;
        match &v.value {
            ExprValue::Int(i) => {
                if self.range.contains(*i) {
                    Ok(())
                } else {
                    Err(format!(
                        "Parameter '{}' value '{}' is not in the parameter space range.",
                        self.name, i
                    ))
                }
            }
            _ => Err(format!(
                "Parameter '{}' value '{}' is not in the parameter space range.",
                self.name,
                v.value.to_display_string()
            )),
        }
    }
    fn iter(&self) -> Box<dyn NodeIterator> {
        Box::new(RangeExprIterator {
            name: self.name.clone(),
            range: self.range.clone(),
            index: 0,
        })
    }
}

/// Wraps a parameter name + pre-computed chunk `RangeExpr`s.
struct StaticChunkNode {
    name: String,
    range: job::TaskParamRange<i64>,
    constraint: RangeConstraint,
    num_chunks: usize,
    small: usize,     // base chunk size = total / num_chunks
    leftovers: usize, // first `leftovers` chunks get size small+1
}

impl StaticChunkNode {
    /// Compute offset and size of chunk `i`.
    fn chunk_bounds(&self, i: usize) -> (usize, usize) {
        let size = self.small + if i < self.leftovers { 1 } else { 0 };
        // offset = i * small + min(i, leftovers)
        let offset = i * self.small + i.min(self.leftovers);
        (offset, size)
    }

    /// Build a RangeExpr for chunk `i` on the fly.
    fn chunk_range_expr(&self, i: usize) -> RangeExpr {
        let (offset, size) = self.chunk_bounds(i);
        match &self.range {
            job::TaskParamRange::RangeExpr(r) => {
                let start = r
                    .get(offset as i64)
                    .expect("chunk offset within range bounds");
                let end = r
                    .get((offset + size - 1) as i64)
                    .expect("chunk end within range bounds");
                let range_str = match self.constraint {
                    RangeConstraint::Contiguous => {
                        if size == 1 {
                            start.to_string()
                        } else {
                            format!("{start}-{end}")
                        }
                    }
                    RangeConstraint::Noncontiguous => {
                        let vals: Vec<i64> = (offset..offset + size)
                            .map(|j| r.get(j as i64).expect("chunk element within range bounds"))
                            .collect();
                        compress_range_expr(&vals)
                    }
                };
                let expr = range_str
                    .parse::<RangeExpr>()
                    .expect("range string built from valid integers");
                match self.constraint {
                    RangeConstraint::Contiguous => expr.with_contiguous(true),
                    RangeConstraint::Noncontiguous => expr,
                }
            }
            job::TaskParamRange::List(values) => {
                let chunk = &values[offset..offset + size];
                let range_str = match self.constraint {
                    RangeConstraint::Contiguous => {
                        if chunk.len() == 1 {
                            chunk[0].to_string()
                        } else {
                            format!("{}-{}", chunk[0], chunk[chunk.len() - 1])
                        }
                    }
                    RangeConstraint::Noncontiguous => compress_range_expr(chunk),
                };
                let expr = range_str
                    .parse::<RangeExpr>()
                    .expect("range string built from valid integers");
                match self.constraint {
                    RangeConstraint::Contiguous => expr.with_contiguous(true),
                    RangeConstraint::Noncontiguous => expr,
                }
            }
        }
    }
}

impl Node for StaticChunkNode {
    fn len(&self) -> usize {
        self.num_chunks
    }
    fn get(&self, index: usize, result: &mut TaskParameterSet) {
        result.insert(
            self.name.clone(),
            TaskParameterValue {
                param_type: TaskParameterType::ChunkInt,
                value: ExprValue::RangeExpr(self.chunk_range_expr(index)),
            },
        );
    }
    fn validate_containment(&self, params: &TaskParameterSet) -> Result<(), String> {
        let v = params.get(&self.name).ok_or_else(|| {
            format!(
                "Parameter '{}' not found in the provided parameters.",
                self.name
            )
        })?;
        match &v.value {
            ExprValue::RangeExpr(r) => {
                if (0..self.num_chunks).any(|i| self.chunk_range_expr(i) == *r) {
                    Ok(())
                } else {
                    Err(format!(
                        "Parameter '{}' value '{}' is not a valid chunk in the parameter space.",
                        self.name, r
                    ))
                }
            }
            _ => Err(format!(
                "Parameter '{}' value '{}' is not in the parameter space range.",
                self.name,
                v.value.to_display_string()
            )),
        }
    }
    fn iter(&self) -> Box<dyn NodeIterator> {
        Box::new(StaticChunkIterator {
            name: self.name.clone(),
            range: self.range.clone(),
            constraint: self.constraint.clone(),
            num_chunks: self.num_chunks,
            small: self.small,
            leftovers: self.leftovers,
            index: 0,
        })
    }
}

/// Cartesian product of children (rightmost moves fastest).
struct ProductNode {
    children: Vec<Box<dyn Node>>,
    length: usize,
}

impl Node for ProductNode {
    fn len(&self) -> usize {
        self.length
    }
    fn get(&self, mut index: usize, result: &mut TaskParameterSet) {
        for child in self.children.iter().rev() {
            let child_len = child.len();
            child.get(index % child_len, result);
            index /= child_len;
        }
    }
    fn validate_containment(&self, params: &TaskParameterSet) -> Result<(), String> {
        for child in &self.children {
            child.validate_containment(params)?;
        }
        Ok(())
    }
    fn iter(&self) -> Box<dyn NodeIterator> {
        Box::new(ProductIterator::new(&self.children))
    }
}

/// Iterator for ProductNode that composes child iterators.
/// Non-adaptive children cycle through their values (rightmost fastest);
/// the adaptive child (if any) advances when non-adaptive children wrap.
struct ProductIterator {
    children: Vec<ChildIterator>,
    started: bool,
}

struct ChildIterator {
    iter: Box<dyn NodeIterator>,
    current: TaskParameterSet,
}

impl ProductIterator {
    fn new(children: &[Box<dyn Node>]) -> Self {
        let children = children
            .iter()
            .map(|child| ChildIterator {
                iter: child.iter(),
                current: TaskParameterSet::new(),
            })
            .collect();
        Self {
            children,
            started: false,
        }
    }

    /// Advance the first value from each child. Returns false if any child is empty.
    fn initialize(&mut self) -> bool {
        for child in &mut self.children {
            if !child.iter.next(&mut child.current) {
                return false;
            }
        }
        true
    }
}

impl NodeIterator for ProductIterator {
    fn next(&mut self, result: &mut TaskParameterSet) -> bool {
        if !self.started {
            self.started = true;
            if !self.initialize() {
                return false;
            }
        } else {
            // Advance rightmost, carry left
            let mut carry = true;
            for child in self.children.iter_mut().rev() {
                if !carry {
                    break;
                }
                child.current.clear();
                if child.iter.next(&mut child.current) {
                    carry = false;
                } else {
                    // Exhausted — reset and advance to first value, carry continues
                    child.iter.reset();
                    if !child.iter.next(&mut child.current) {
                        return false;
                    }
                }
            }
            if carry {
                return false;
            }
        }
        for child in &self.children {
            result.extend(child.current.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        true
    }
    fn reset(&mut self) {
        self.started = false;
        for child in &mut self.children {
            child.iter.reset();
            child.current.clear();
        }
    }
}

/// Association: all children have the same length, indexed in lockstep.
struct AssociationNode {
    children: Vec<Box<dyn Node>>,
    length: usize,
}

impl Node for AssociationNode {
    fn len(&self) -> usize {
        self.length
    }
    fn get(&self, index: usize, result: &mut TaskParameterSet) {
        for child in &self.children {
            child.get(index, result);
        }
    }
    fn validate_containment(&self, params: &TaskParameterSet) -> Result<(), String> {
        // Linear scan: at least one index must match all children simultaneously
        for i in 0..self.length {
            let mut candidate = TaskParameterSet::new();
            for child in &self.children {
                child.get(i, &mut candidate);
            }
            if params_equal(&candidate, params) {
                return Ok(());
            }
        }
        // Build a display of the mismatched values
        let values: Vec<String> = params
            .iter()
            .filter(|(k, _)| {
                self.children.iter().any(|c| {
                    let mut ps = TaskParameterSet::new();
                    c.get(0, &mut ps);
                    ps.contains_key(*k)
                })
            })
            .map(|(k, v)| format!("{}={}", k, v.value.to_display_string()))
            .collect();
        Err(format!(
            "The values {{{}}}, of an association expression in the combination expression, do not appear in the parameter space.",
            values.join(", ")
        ))
    }
    fn iter(&self) -> Box<dyn NodeIterator> {
        Box::new(AssociationIterator::new(&self.children))
    }
}

/// Iterator for AssociationNode: lockstep iteration of children.
struct AssociationIterator {
    children: Vec<ChildIterator>,
}

impl AssociationIterator {
    fn new(children: &[Box<dyn Node>]) -> Self {
        let children = children
            .iter()
            .map(|child| ChildIterator {
                iter: child.iter(),
                current: TaskParameterSet::new(),
            })
            .collect();
        Self { children }
    }
}

impl NodeIterator for AssociationIterator {
    fn next(&mut self, result: &mut TaskParameterSet) -> bool {
        for child in &mut self.children {
            child.current.clear();
            if !child.iter.next(&mut child.current) {
                return false;
            }
            result.extend(child.current.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        true
    }
    fn reset(&mut self) {
        for child in &mut self.children {
            child.iter.reset();
            child.current.clear();
        }
    }
}

/// Adaptive chunk node: produces chunks on the fly based on mutable `default_task_count`.
struct AdaptiveChunkNode {
    name: String,
    values: Vec<i64>,
    default_task_count: Arc<AtomicUsize>,
    range_constraint: RangeConstraint,
}

impl Node for AdaptiveChunkNode {
    fn len(&self) -> usize {
        // Upper bound: one chunk per value. Actual count depends on runtime chunk size.
        // Used only for association length validation during construction.
        let dtc = self.default_task_count.load(Ordering::Relaxed).max(1);
        self.values.len().div_ceil(dtc)
    }
    fn get(&self, _index: usize, _result: &mut TaskParameterSet) {
        // Random access not supported — use iter() instead.
    }
    fn validate_containment(&self, params: &TaskParameterSet) -> Result<(), String> {
        let v = params.get(&self.name).ok_or_else(|| {
            format!(
                "Parameter '{}' not found in the provided parameters.",
                self.name
            )
        })?;
        match &v.value {
            ExprValue::RangeExpr(r) => {
                for val in r.iter() {
                    if !self.values.contains(&val) {
                        return Err(format!(
                            "Parameter '{}' value '{}' is not a subset of the range in the parameter space.",
                            self.name, r
                        ));
                    }
                }
                Ok(())
            }
            _ => Err(format!(
                "Parameter '{}' value '{}' is not in the parameter space range.",
                self.name,
                v.value.to_display_string()
            )),
        }
    }
    fn iter(&self) -> Box<dyn NodeIterator> {
        Box::new(AdaptiveChunkIterator {
            name: self.name.clone(),
            values: self.values.clone(),
            default_task_count: self.default_task_count.clone(),
            range_constraint: self.range_constraint.clone(),
            cursor: 0,
        })
    }
}

/// Iterator for adaptive chunk nodes.
struct AdaptiveChunkIterator {
    name: String,
    values: Vec<i64>,
    default_task_count: Arc<AtomicUsize>,
    range_constraint: RangeConstraint,
    cursor: usize,
}

impl AdaptiveChunkIterator {
    fn make_chunk(&self, slice: &[i64]) -> RangeExpr {
        let range_str = match self.range_constraint {
            RangeConstraint::Contiguous => {
                if slice.len() == 1 {
                    slice[0].to_string()
                } else {
                    format!("{}-{}", slice[0], slice[slice.len() - 1])
                }
            }
            RangeConstraint::Noncontiguous => compress_range_expr(slice),
        };
        let expr = range_str
            .parse::<RangeExpr>()
            .expect("range string built from valid integers");
        match self.range_constraint {
            RangeConstraint::Contiguous => expr.with_contiguous(true),
            RangeConstraint::Noncontiguous => expr,
        }
    }
}

impl NodeIterator for AdaptiveChunkIterator {
    fn next(&mut self, result: &mut TaskParameterSet) -> bool {
        if self.cursor >= self.values.len() {
            return false;
        }
        let chunk_size = self.default_task_count.load(Ordering::Relaxed).max(1);
        let chunk = match self.range_constraint {
            RangeConstraint::Contiguous => {
                let start = self.cursor;
                let mut end = start + 1;
                while end < self.values.len()
                    && end - start < chunk_size
                    && self.values[end] == self.values[end - 1] + 1
                {
                    end += 1;
                }
                let slice = &self.values[start..end];
                self.cursor = end;
                self.make_chunk(slice)
            }
            RangeConstraint::Noncontiguous => {
                let end = (self.cursor + chunk_size).min(self.values.len());
                let slice = &self.values[self.cursor..end];
                self.cursor = end;
                self.make_chunk(slice)
            }
        };
        result.insert(
            self.name.clone(),
            TaskParameterValue {
                param_type: TaskParameterType::ChunkInt,
                value: ExprValue::RangeExpr(chunk),
            },
        );
        true
    }
    fn reset(&mut self) {
        self.cursor = 0;
    }
}

// ── Public API ──

/// Lazy iterator over a resolved step parameter space.
pub struct StepParameterSpaceIterator {
    root: Box<dyn Node>,
    names: HashSet<String>,
    current_index: usize,
    adaptive: bool,
    adaptive_chunk_size: Option<Arc<AtomicUsize>>,
    node_iter: Option<Box<dyn NodeIterator>>,
    chunks_param_name: Option<String>,
}

impl StepParameterSpaceIterator {
    /// Construct from a resolved `StepParameterSpace`.
    pub fn new(space: &job::StepParameterSpace) -> Result<Self, OpenJdError> {
        Self::new_inner(space, None)
    }

    /// Create with an explicit chunk task count override.
    /// When `Some(1)`, disables adaptive chunking and counts individual tasks.
    pub fn new_with_chunk_override(
        space: &job::StepParameterSpace,
        override_count: Option<usize>,
    ) -> Result<Self, OpenJdError> {
        Self::new_inner(space, override_count)
    }

    fn new_inner(
        space: &job::StepParameterSpace,
        chunk_override: Option<usize>,
    ) -> Result<Self, OpenJdError> {
        let names: HashSet<String> = space.task_parameter_definitions.keys().cloned().collect();

        if space.task_parameter_definitions.is_empty() {
            return Ok(Self {
                root: Box::new(ZeroDimSpaceNode),
                names,
                current_index: 0,
                adaptive: false,
                adaptive_chunk_size: None,
                node_iter: None,
                chunks_param_name: None,
            });
        }

        let expr = space.combination.as_deref().unwrap_or("*");

        // Check if any parameter needs adaptive chunking
        let mut adaptive_info: Option<(String, Arc<AtomicUsize>)> = None;
        if chunk_override.is_none() {
            for (name, param) in &space.task_parameter_definitions {
                if let job::TaskParameter::ChunkInt { chunks, .. } = param {
                    if chunks.target_runtime_seconds.is_some_and(|t| t > 0) {
                        let arc = Arc::new(AtomicUsize::new(chunks.default_task_count.max(1)));
                        adaptive_info = Some((name.clone(), arc));
                        break;
                    }
                }
            }
        }

        let root = if expr.trim() == "*" {
            // Default: no explicit combination — product of all params in definition order
            let mut children: Vec<Box<dyn Node>> = Vec::new();
            for name in space.task_parameter_definitions.keys() {
                children.push(make_leaf_node(name, space, &adaptive_info)?);
            }
            if children.len() == 1 {
                children.pop().unwrap()
            } else {
                let length = checked_product_len(&children)?;
                Box::new(ProductNode { children, length })
            }
        } else {
            let tokens = tokenize(expr);
            parse_node_expr(&tokens, space, &adaptive_info)?
        };

        let adaptive = adaptive_info.is_some();
        let chunks_param_name = adaptive_info.as_ref().map(|(n, _)| n.clone());
        let adaptive_chunk_size = adaptive_info.map(|(_, rc)| rc);
        let node_iter = if adaptive { Some(root.iter()) } else { None };

        Ok(Self {
            root,
            names,
            current_index: 0,
            adaptive,
            adaptive_chunk_size,
            node_iter,
            chunks_param_name,
        })
    }

    pub fn names(&self) -> &HashSet<String> {
        &self.names
    }

    pub fn len(&self) -> usize {
        if self.adaptive {
            0
        } else {
            self.root.len()
        }
    }

    pub fn is_empty(&self) -> bool {
        if self.adaptive {
            false
        } else {
            self.root.len() == 0
        }
    }

    /// Random access to a specific task parameter set by index.
    /// Returns `None` for out-of-bounds or when adaptive chunking is active.
    pub fn get(&self, index: usize) -> Option<TaskParameterSet> {
        if self.adaptive {
            return None;
        }
        if index >= self.root.len() {
            return None;
        }
        let mut result = TaskParameterSet::new();
        self.root.get(index, &mut result);
        Some(result)
    }

    /// Check if a parameter set is contained in this space.
    pub fn contains(&self, params: &TaskParameterSet) -> bool {
        self.validate_containment(params).is_ok()
    }

    /// Validate that a parameter set is contained in this space.
    /// Returns a detailed error message if not.
    pub fn validate_containment(&self, params: &TaskParameterSet) -> Result<(), String> {
        let mut params_keys: Vec<&str> = params.keys().map(|s| s.as_str()).collect();
        let mut space_keys: Vec<&str> = self.names.iter().map(|s| s.as_str()).collect();
        params_keys.sort();
        space_keys.sort();
        if params_keys != space_keys {
            return Err(format!(
                "Task parameter names {:?} do not match the parameter space names {:?}.",
                params_keys, space_keys
            ));
        }
        self.root.validate_containment(params)
    }

    /// Whether adaptive chunking is active.
    pub fn chunks_adaptive(&self) -> bool {
        self.adaptive
    }

    /// The parameter name used for chunking, if any.
    pub fn chunks_parameter_name(&self) -> Option<&str> {
        self.chunks_param_name.as_deref()
    }

    /// Current default_task_count for adaptive chunking.
    pub fn chunks_default_task_count(&self) -> Option<usize> {
        self.adaptive_chunk_size
            .as_ref()
            .map(|a| a.load(Ordering::Relaxed))
    }

    /// Update the chunk size for adaptive chunking.
    pub fn set_chunks_default_task_count(&mut self, value: usize) {
        if let Some(ref a) = self.adaptive_chunk_size {
            a.store(value, Ordering::Relaxed);
            // The Arc<AtomicUsize> propagates to the live iterator — no reset needed.
        }
    }
}

fn params_equal(a: &TaskParameterSet, b: &TaskParameterSet) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().all(|(k, v)| {
        b.get(k)
            .is_some_and(|bv| expr_value_eq(&v.value, &bv.value))
    })
}

fn expr_value_eq(a: &ExprValue, b: &ExprValue) -> bool {
    match (a, b) {
        (ExprValue::Int(x), ExprValue::Int(y)) => x == y,
        (ExprValue::Float(x), ExprValue::Float(y)) => x.value() == y.value(),
        (ExprValue::String(x), ExprValue::String(y)) => x == y,
        (ExprValue::RangeExpr(x), ExprValue::RangeExpr(y)) => x == y,
        (ExprValue::Path { value: x, .. }, ExprValue::Path { value: y, .. }) => x == y,
        (ExprValue::String(x), ExprValue::Path { value: y, .. }) => x == y,
        (ExprValue::Path { value: x, .. }, ExprValue::String(y)) => x == y,
        _ => false,
    }
}

impl Iterator for StepParameterSpaceIterator {
    type Item = TaskParameterSet;
    fn next(&mut self) -> Option<TaskParameterSet> {
        if self.adaptive {
            let iter = self.node_iter.as_mut()?;
            let mut result = TaskParameterSet::new();
            if iter.next(&mut result) {
                Some(result)
            } else {
                None
            }
        } else {
            let item = self.get(self.current_index)?;
            self.current_index += 1;
            Some(item)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.adaptive {
            (0, None)
        } else {
            let remaining = self.root.len().saturating_sub(self.current_index);
            (remaining, Some(remaining))
        }
    }
}

// ── Node construction from combination expression ──

fn parse_node_expr(
    tokens: &[String],
    space: &job::StepParameterSpace,
    adaptive_info: &Option<(String, Arc<AtomicUsize>)>,
) -> Result<Box<dyn Node>, OpenJdError> {
    let mut pos = 0;
    let result = parse_node_product(tokens, &mut pos, space, adaptive_info)?;
    if pos < tokens.len() {
        return Err(OpenJdError::DecodeValidation(format!(
            "Unexpected token '{}' in combination expression",
            tokens[pos]
        )));
    }
    Ok(result)
}

fn parse_node_product(
    tokens: &[String],
    pos: &mut usize,
    space: &job::StepParameterSpace,
    adaptive_info: &Option<(String, Arc<AtomicUsize>)>,
) -> Result<Box<dyn Node>, OpenJdError> {
    let mut children = vec![parse_node_element(tokens, pos, space, adaptive_info)?];
    while *pos < tokens.len() && tokens[*pos] == "*" {
        *pos += 1;
        children.push(parse_node_element(tokens, pos, space, adaptive_info)?);
    }
    if children.len() == 1 {
        Ok(children.pop().unwrap())
    } else {
        let length = checked_product_len(&children)?;
        Ok(Box::new(ProductNode { children, length }))
    }
}

fn parse_node_element(
    tokens: &[String],
    pos: &mut usize,
    space: &job::StepParameterSpace,
    adaptive_info: &Option<(String, Arc<AtomicUsize>)>,
) -> Result<Box<dyn Node>, OpenJdError> {
    if *pos >= tokens.len() {
        return Err(OpenJdError::DecodeValidation(
            "Unexpected end of combination expression".into(),
        ));
    }
    if tokens[*pos] == "(" {
        *pos += 1;
        let mut children = vec![parse_node_product(tokens, pos, space, adaptive_info)?];
        while *pos < tokens.len() && tokens[*pos] == "," {
            *pos += 1;
            children.push(parse_node_product(tokens, pos, space, adaptive_info)?);
        }
        if *pos >= tokens.len() || tokens[*pos] != ")" {
            return Err(OpenJdError::DecodeValidation(
                "Missing closing parenthesis in combination".into(),
            ));
        }
        *pos += 1;
        let length = children[0].len();
        for child in children.iter().skip(1) {
            if child.len() != length {
                return Err(OpenJdError::DecodeValidation(format!(
                    "Associative combination: all members must have the same number of values, got {} and {}",
                    length, child.len()
                )));
            }
        }
        if children.len() == 1 {
            Err(OpenJdError::DecodeValidation(
                "Association expression must have more than one term.".into(),
            ))
        } else {
            Ok(Box::new(AssociationNode { children, length }))
        }
    } else {
        let name = &tokens[*pos];
        *pos += 1;
        make_leaf_node(name, space, adaptive_info)
    }
}

/// Create a leaf node for a parameter name from the resolved definitions.
fn make_leaf_node(
    name: &str,
    space: &job::StepParameterSpace,
    adaptive_info: &Option<(String, Arc<AtomicUsize>)>,
) -> Result<Box<dyn Node>, OpenJdError> {
    let param = space.task_parameter_definitions.get(name).ok_or_else(|| {
        OpenJdError::DecodeValidation(format!(
            "Unknown parameter '{name}' in combination expression"
        ))
    })?;

    match param {
        job::TaskParameter::Int { range, chunks } => {
            if let Some(chunk_cfg) = chunks {
                return make_chunk_node(name, range, chunk_cfg, adaptive_info);
            }
            match range {
                job::TaskParamRange::List(v) => Ok(Box::new(RangeListNode {
                    name: name.to_string(),
                    param_type: TaskParameterType::Int,
                    values: v.iter().map(|&i| ExprValue::Int(i)).collect(),
                })),
                job::TaskParamRange::RangeExpr(r) => Ok(Box::new(RangeExprNode {
                    name: name.to_string(),
                    range: r.clone(),
                })),
            }
        }
        job::TaskParameter::Float { range } => Ok(Box::new(RangeListNode {
            name: name.to_string(),
            param_type: TaskParameterType::Float,
            values: range
                .iter()
                .map(|&f| ExprValue::Float(Float64::new(f).unwrap()))
                .collect(),
        })),
        job::TaskParameter::String { range } => Ok(Box::new(RangeListNode {
            name: name.to_string(),
            param_type: TaskParameterType::String,
            values: range.iter().map(|s| ExprValue::String(s.clone())).collect(),
        })),
        job::TaskParameter::Path { range } => Ok(Box::new(RangeListNode {
            name: name.to_string(),
            param_type: TaskParameterType::Path,
            values: range.iter().map(|s| ExprValue::String(s.clone())).collect(),
        })),
        job::TaskParameter::ChunkInt { range, chunks } => {
            make_chunk_node(name, range, chunks, adaptive_info)
        }
    }
}

/// Build a chunk node from a range and chunk config. Creates `AdaptiveChunkNode` when
/// `target_runtime_seconds > 0`, otherwise creates `StaticChunkNode`.
fn make_chunk_node(
    name: &str,
    range: &job::TaskParamRange<i64>,
    chunks: &job::ResolvedChunks,
    adaptive_info: &Option<(String, Arc<AtomicUsize>)>,
) -> Result<Box<dyn Node>, OpenJdError> {
    // Check if this parameter should use adaptive chunking
    if let Some((adaptive_name, rc)) = adaptive_info {
        if adaptive_name == name {
            let values: Vec<i64> = match range {
                job::TaskParamRange::List(v) => v.clone(),
                job::TaskParamRange::RangeExpr(r) => r.iter().collect(),
            };
            return Ok(Box::new(AdaptiveChunkNode {
                name: name.to_string(),
                values,
                default_task_count: rc.clone(),
                range_constraint: chunks.range_constraint.clone(),
            }));
        }
    }

    let total_len = match range {
        job::TaskParamRange::List(v) => v.len(),
        job::TaskParamRange::RangeExpr(r) => r.len(),
    };
    if total_len == 0 {
        return Ok(Box::new(RangeListNode {
            name: name.to_string(),
            param_type: TaskParameterType::ChunkInt,
            values: Vec::new(),
        }));
    }

    let default_task_count = chunks.default_task_count.max(1);
    let chunk_count = total_len.div_ceil(default_task_count);
    let small = total_len / chunk_count;
    let leftovers = total_len % chunk_count;

    Ok(Box::new(StaticChunkNode {
        name: name.to_string(),
        range: range.clone(),
        constraint: chunks.range_constraint.clone(),
        num_chunks: chunk_count,
        small,
        leftovers,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_range_expr() {
        assert_eq!(compress_range_expr(&[1, 2, 3]), "1-3");
        assert_eq!(compress_range_expr(&[1, 2, 3, 5, 7, 8, 9]), "1-3,5,7-9");
        assert_eq!(compress_range_expr(&[1]), "1");
        assert_eq!(compress_range_expr(&[1, 3]), "1,3");
        assert_eq!(compress_range_expr(&[]), "");
    }

    #[test]
    fn test_tokenize() {
        assert_eq!(tokenize("A * B"), vec!["A", "*", "B"]);
        assert_eq!(
            tokenize("(A, B) * C"),
            vec!["(", "A", ",", "B", ")", "*", "C"]
        );
        assert_eq!(tokenize("A"), vec!["A"]);
    }

    // ── Helper to build test spaces ──

    fn make_space(
        params: Vec<(&str, job::TaskParameter)>,
        combination: Option<&str>,
    ) -> job::StepParameterSpace {
        let mut defs = indexmap::IndexMap::new();
        for (name, param) in params {
            defs.insert(name.to_string(), param);
        }
        job::StepParameterSpace {
            task_parameter_definitions: defs,
            combination: combination.map(|s| s.to_string()),
        }
    }

    fn int_param(values: Vec<i64>) -> job::TaskParameter {
        job::TaskParameter::Int {
            range: job::TaskParamRange::List(values),
            chunks: None,
        }
    }

    fn adaptive_chunk_param(values: Vec<i64>, default_task_count: usize) -> job::TaskParameter {
        job::TaskParameter::ChunkInt {
            range: job::TaskParamRange::List(values),
            chunks: job::ResolvedChunks {
                default_task_count,
                target_runtime_seconds: Some(60), // >0 triggers adaptive
                range_constraint: RangeConstraint::Noncontiguous,
            },
        }
    }

    fn range_expr_param(expr: &str) -> job::TaskParameter {
        job::TaskParameter::Int {
            range: job::TaskParamRange::RangeExpr(expr.parse::<RangeExpr>().unwrap()),
            chunks: None,
        }
    }

    fn static_chunk_param(expr: &str, default_task_count: usize) -> job::TaskParameter {
        job::TaskParameter::ChunkInt {
            range: job::TaskParamRange::RangeExpr(expr.parse::<RangeExpr>().unwrap()),
            chunks: job::ResolvedChunks {
                default_task_count,
                target_runtime_seconds: None,
                range_constraint: RangeConstraint::Contiguous,
            },
        }
    }

    // ── Laziness tests ──
    // These use a 100-billion-element RangeExpr. If any code path eagerly
    // materializes the range, the test will OOM or hang — proving non-laziness.

    const HUGE_RANGE: &str = "1-100000000000";

    #[test]
    fn test_lazy_construction_range_expr() {
        let space = make_space(vec![("X", range_expr_param(HUGE_RANGE))], None);
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        assert_eq!(iter.len(), 100_000_000_000);
    }

    #[test]
    fn test_lazy_random_access_range_expr() {
        let space = make_space(vec![("X", range_expr_param(HUGE_RANGE))], None);
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        let first = iter.get(0).unwrap();
        assert_eq!(first["X"].value, ExprValue::Int(1));
        let last = iter.get(99_999_999_999).unwrap();
        assert_eq!(last["X"].value, ExprValue::Int(100_000_000_000));
    }

    #[test]
    fn test_lazy_product_with_huge_range() {
        let space = make_space(
            vec![
                ("A", int_param(vec![1, 2])),
                ("X", range_expr_param(HUGE_RANGE)),
            ],
            None,
        );
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        assert_eq!(iter.len(), 200_000_000_000);
        // Random access into the middle
        let mid = iter.get(50_000_000_000).unwrap();
        assert!(mid.contains_key("A"));
        assert!(mid.contains_key("X"));
    }

    #[test]
    fn test_lazy_iterate_first_few_of_huge_range() {
        let space = make_space(vec![("X", range_expr_param(HUGE_RANGE))], None);
        let mut iter = StepParameterSpaceIterator::new(&space).unwrap();
        let first = iter.next().unwrap();
        assert_eq!(first["X"].value, ExprValue::Int(1));
        let second = iter.next().unwrap();
        assert_eq!(second["X"].value, ExprValue::Int(2));
    }

    #[test]
    fn test_lazy_product_iterate_first_few() {
        let space = make_space(
            vec![
                ("A", int_param(vec![10, 20])),
                ("X", range_expr_param(HUGE_RANGE)),
            ],
            None,
        );
        let mut iter = StepParameterSpaceIterator::new(&space).unwrap();
        // First item: A=10, X=1 (or A=20, X=1 depending on HashMap order)
        let first = iter.next().unwrap();
        assert!(first.contains_key("A"));
        assert!(first.contains_key("X"));
        // Just verify we can get a few without hanging
        for _ in 0..10 {
            assert!(iter.next().is_some());
        }
    }

    #[test]
    fn test_lazy_static_chunk_with_huge_range() {
        // 100B items / 1000 per chunk = 100M chunks — construction must be lazy
        let space = make_space(vec![("C", static_chunk_param(HUGE_RANGE, 1000))], None);
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        assert_eq!(iter.len(), 100_000_000);
        // Random access to last chunk
        let last = iter.get(99_999_999).unwrap();
        assert!(last.contains_key("C"));
    }

    #[test]
    fn test_lazy_iter_of_product_with_huge_range() {
        // Tests that ProductNode::iter() doesn't materialize the huge child
        let space = make_space(
            vec![
                ("A", int_param(vec![1, 2])),
                ("X", range_expr_param(HUGE_RANGE)),
                ("Chunk", adaptive_chunk_param(vec![10, 20, 30, 40], 2)),
            ],
            None,
        );
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        assert!(iter.chunks_adaptive());
        // Iterate a few — must not OOM from materializing X's 100B values
        let mut count = 0;
        for params in iter {
            assert!(params.contains_key("A"));
            assert!(params.contains_key("X"));
            assert!(params.contains_key("Chunk"));
            count += 1;
            if count >= 5 {
                break;
            }
        }
        assert_eq!(count, 5);
    }

    // ── Adaptive chunking tests ──

    #[test]
    fn test_len_returns_zero_for_adaptive_chunking() {
        let space = make_space(
            vec![("Chunk", adaptive_chunk_param(vec![1, 2, 3, 4, 5, 6], 2))],
            None,
        );
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        assert!(iter.chunks_adaptive());
        assert_eq!(iter.len(), 0);
    }

    #[test]
    fn test_get_returns_none_for_adaptive_chunking() {
        let space = make_space(
            vec![("Chunk", adaptive_chunk_param(vec![1, 2, 3, 4, 5, 6], 2))],
            None,
        );
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        assert!(iter.chunks_adaptive());
        assert!(iter.get(0).is_none());
    }

    #[test]
    fn test_adaptive_chunking_with_multiple_params_iterates() {
        let space = make_space(
            vec![
                ("Frame", int_param(vec![1, 2])),
                ("Chunk", adaptive_chunk_param(vec![10, 20, 30, 40], 2)),
            ],
            None,
        );
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        assert!(iter.chunks_adaptive());
        let mut count = 0;
        for params in iter {
            assert!(params.contains_key("Frame"));
            assert!(params.contains_key("Chunk"));
            count += 1;
            if count > 100 {
                break;
            }
        }
        assert_eq!(count, 4);
    }

    #[test]
    fn test_adaptive_chunking_single_param_iterates() {
        let space = make_space(
            vec![("Chunk", adaptive_chunk_param(vec![1, 2, 3, 4, 5, 6], 3))],
            None,
        );
        let results: Vec<_> = StepParameterSpaceIterator::new(&space).unwrap().collect();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_adaptive_with_association_iterates() {
        let space = make_space(
            vec![
                ("Frame", int_param(vec![1, 2])),
                ("Chunk", adaptive_chunk_param(vec![10, 20], 1)),
            ],
            Some("(Frame, Chunk)"),
        );
        let results: Vec<_> = StepParameterSpaceIterator::new(&space).unwrap().collect();
        assert_eq!(results.len(), 2);
    }

    // ── validate_containment tests ──

    fn tpv(param_type: TaskParameterType, value: ExprValue) -> TaskParameterValue {
        TaskParameterValue { param_type, value }
    }

    #[test]
    fn test_validate_containment_name_mismatch() {
        let space = make_space(vec![("Frame", int_param(vec![1, 2, 3]))], None);
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        let mut params = TaskParameterSet::new();
        params.insert(
            "Wrong".into(),
            tpv(TaskParameterType::Int, ExprValue::Int(1)),
        );
        let err = iter.validate_containment(&params).unwrap_err();
        assert!(err.contains("do not match"), "got: {err}");
        assert!(err.contains("Wrong"), "got: {err}");
        assert!(err.contains("Frame"), "got: {err}");
    }

    #[test]
    fn test_validate_containment_value_not_in_range() {
        let space = make_space(vec![("Frame", int_param(vec![1, 2, 3]))], None);
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        let mut params = TaskParameterSet::new();
        params.insert(
            "Frame".into(),
            tpv(TaskParameterType::Int, ExprValue::Int(99)),
        );
        let err = iter.validate_containment(&params).unwrap_err();
        assert!(err.contains("Frame"), "got: {err}");
        assert!(err.contains("99"), "got: {err}");
        assert!(
            err.contains("not in the parameter space range"),
            "got: {err}"
        );
    }

    #[test]
    fn test_validate_containment_range_expr_value_not_in_range() {
        let space = make_space(vec![("X", range_expr_param("1-10"))], None);
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        let mut params = TaskParameterSet::new();
        params.insert("X".into(), tpv(TaskParameterType::Int, ExprValue::Int(99)));
        let err = iter.validate_containment(&params).unwrap_err();
        assert!(err.contains("X"), "got: {err}");
        assert!(err.contains("99"), "got: {err}");
        assert!(
            err.contains("not in the parameter space range"),
            "got: {err}"
        );
    }

    #[test]
    fn test_validate_containment_success() {
        let space = make_space(vec![("Frame", int_param(vec![1, 2, 3]))], None);
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        let mut params = TaskParameterSet::new();
        params.insert(
            "Frame".into(),
            tpv(TaskParameterType::Int, ExprValue::Int(2)),
        );
        assert!(iter.validate_containment(&params).is_ok());
    }

    #[test]
    fn test_validate_containment_association_not_found() {
        let space = make_space(
            vec![("A", int_param(vec![1, 2])), ("B", int_param(vec![10, 20]))],
            Some("(A, B)"),
        );
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        // A=1,B=20 is not a valid association pair (valid: A=1,B=10 and A=2,B=20)
        let mut params = TaskParameterSet::new();
        params.insert("A".into(), tpv(TaskParameterType::Int, ExprValue::Int(1)));
        params.insert("B".into(), tpv(TaskParameterType::Int, ExprValue::Int(20)));
        let err = iter.validate_containment(&params).unwrap_err();
        assert!(err.contains("association"), "got: {err}");
    }

    #[test]
    fn test_validate_containment_chunk_not_subset() {
        let space = make_space(vec![("C", static_chunk_param("1-10", 5))], None);
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        // Chunk "1-99" is not a subset of range 1-10
        let mut params = TaskParameterSet::new();
        params.insert(
            "C".into(),
            tpv(
                TaskParameterType::ChunkInt,
                ExprValue::RangeExpr("1-99".parse::<RangeExpr>().unwrap()),
            ),
        );
        let err = iter.validate_containment(&params).unwrap_err();
        assert!(err.contains("C"), "got: {err}");
        assert!(err.contains("not"), "got: {err}");
    }
}
