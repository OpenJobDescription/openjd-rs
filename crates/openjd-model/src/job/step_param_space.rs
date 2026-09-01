// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

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

use openjd_expr::{ExprValue, RangeExpr};

use crate::error::ModelError;
use crate::job;
use crate::template::RangeConstraint;
use crate::types::{TaskParameterSet, TaskParameterType, TaskParameterValue};

// ── Shared utilities ──

/// Compute the product of child node lengths with overflow checking.
fn checked_product_len(children: &[Box<dyn Node>]) -> Result<usize, ModelError> {
    children.iter().try_fold(1usize, |acc, c| {
        acc.checked_mul(c.len()).ok_or_else(|| {
            ModelError::DecodeValidation(
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
    if values.len() == 1 {
        return values[0].to_string();
    }

    // Detect runs with a constant step. A run needs 3+ values to use step notation;
    // with only 2 values, any step is trivially valid so we don't commit to it.
    let mut parts = Vec::new();
    let mut i = 0;
    while i < values.len() {
        if i + 2 < values.len() {
            let step = values[i + 1] - values[i];
            if step > 0 && values[i + 2] - values[i + 1] == step {
                // Found a run of at least 3 with constant step
                let mut end = i + 2;
                while end + 1 < values.len() && values[end + 1] - values[end] == step {
                    end += 1;
                }
                if step == 1 {
                    parts.push(format!("{}-{}", values[i], values[end]));
                } else {
                    parts.push(format!("{}-{}:{}", values[i], values[end], step));
                }
                i = end + 1;
                continue;
            }
        }
        parts.push(values[i].to_string());
        i += 1;
    }
    parts.join(",")
}

/// Build a `RangeExpr` for chunk `i` given the chunk layout parameters.
/// Used by `StaticChunkNode` and `StaticChunkIterator` for noncontiguous chunking.
///
/// - `range`: the full integer range being chunked
/// - `constraint`: whether chunks must be contiguous (`1-10`) or can be non-contiguous (`1,3,7-10`)
/// - `small`: base chunk size (`total_values / num_chunks`)
/// - `leftovers`: how many of the first chunks get one extra element (`total_values % num_chunks`)
/// - `i`: zero-based chunk index to build
fn build_chunk_range_expr(
    range: &job::TaskParamRange<i64>,
    constraint: &RangeConstraint,
    small: usize,
    leftovers: usize,
    i: usize,
) -> RangeExpr {
    let size = small + if i < leftovers { 1 } else { 0 };
    let offset = i * small + i.min(leftovers);
    let build = |vals: &[i64]| -> RangeExpr {
        let range_str = match constraint {
            RangeConstraint::Contiguous => {
                if vals.len() == 1 {
                    vals[0].to_string()
                } else {
                    format!("{}-{}", vals[0], vals[vals.len() - 1])
                }
            }
            RangeConstraint::Noncontiguous => compress_range_expr(vals),
        };
        let expr = range_str
            .parse::<RangeExpr>()
            .expect("range string built from valid integers");
        match constraint {
            RangeConstraint::Contiguous => expr.with_contiguous(true),
            RangeConstraint::Noncontiguous => expr,
        }
    };
    match range {
        job::TaskParamRange::RangeExpr(r) => {
            let vals: Vec<i64> = (offset..offset + size)
                .map(|j| r.get(j as i64).expect("chunk element within range bounds"))
                .collect();
            build(&vals)
        }
        job::TaskParamRange::List(values) => build(&values[offset..offset + size]),
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
trait NodeIterator: Send + Sync {
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
        build_chunk_range_expr(&self.range, &self.constraint, self.small, self.leftovers, i)
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

/// Contiguous chunking node: splits values into chunks that respect gaps.
/// Contiguous runs in the source range are identified, then each run is
/// chunked independently. Uses index-based access to avoid materializing values.
struct ContiguousChunkNode {
    name: String,
    range: job::TaskParamRange<i64>,
    default_task_count: usize,
    num_chunks: usize, // cached exact count
    total_len: usize,
}

/// Count contiguous chunks by walking the range's sub-ranges.
/// For `RangeExpr`, uses the internal `IntRange` structure for O(R) where R is the
/// number of sub-ranges (not the number of values). For `List`, scans values in O(N).
fn count_contiguous_chunks_for_range(
    range: &job::TaskParamRange<i64>,
    default_task_count: usize,
) -> usize {
    match range {
        job::TaskParamRange::List(v) => {
            if v.is_empty() {
                return 0;
            }
            let mut total = 0usize;
            let mut interval_start = 0usize;
            for i in 0..v.len() - 1 {
                if v[i + 1] != v[i] + 1 {
                    let len = i - interval_start + 1;
                    total += len.div_ceil(default_task_count);
                    interval_start = i + 1;
                }
            }
            total += (v.len() - interval_start).div_ceil(default_task_count);
            total
        }
        job::TaskParamRange::RangeExpr(r) => {
            count_contiguous_chunks_from_sub_ranges(r, default_task_count)
        }
    }
}

/// Count contiguous chunks by iterating the `IntRange` sub-ranges of a `RangeExpr`.
/// Merges adjacent intervals and computes chunk counts arithmetically per interval.
fn count_contiguous_chunks_from_sub_ranges(r: &RangeExpr, default_task_count: usize) -> usize {
    let sub_ranges = r.ranges();
    if sub_ranges.is_empty() {
        return 0;
    }

    let mut total_chunks = 0usize;
    // Track the current merged contiguous interval as (start_val, end_val)
    let mut interval: Option<(i64, i64)> = None;

    for sr in sub_ranges {
        if sr.step() == 1 {
            // This sub-range is contiguous: values from sr.start() to sr.end()
            match interval {
                Some((is, ie)) if sr.start() == ie + 1 => {
                    // Extends the current interval
                    interval = Some((is, sr.end()));
                }
                Some((is, ie)) => {
                    // Gap — flush the current interval
                    let len = (ie - is + 1) as usize;
                    total_chunks += len.div_ceil(default_task_count);
                    interval = Some((sr.start(), sr.end()));
                }
                None => {
                    interval = Some((sr.start(), sr.end()));
                }
            }
        } else {
            // Step > 1: each value is isolated (has gaps between them).
            // We need to check if the first value merges with the current interval,
            // then each subsequent value is its own interval.
            let count = sr.len();
            for idx in 0..count {
                // SAFETY: idx is bounded by sr.len(), so get() always returns Some.
                let val = sr.get(idx).expect("index within sub-range bounds");
                match interval {
                    Some((is, ie)) if val == ie + 1 => {
                        interval = Some((is, val));
                    }
                    Some((is, ie)) => {
                        let len = (ie - is + 1) as usize;
                        total_chunks += len.div_ceil(default_task_count);
                        interval = Some((val, val));
                    }
                    None => {
                        interval = Some((val, val));
                    }
                }
            }
        }
    }
    // Flush final interval
    if let Some((is, ie)) = interval {
        let len = (ie - is + 1) as usize;
        total_chunks += len.div_ceil(default_task_count);
    }
    total_chunks
}

impl ContiguousChunkNode {
    fn new(name: String, range: job::TaskParamRange<i64>, default_task_count: usize) -> Self {
        let total_len = match &range {
            job::TaskParamRange::List(v) => v.len(),
            job::TaskParamRange::RangeExpr(r) => r.len(),
        };
        let dtc = default_task_count.max(1);
        let num_chunks = count_contiguous_chunks_for_range(&range, dtc);
        Self {
            name,
            range,
            default_task_count: dtc,
            num_chunks,
            total_len,
        }
    }
}

impl ContiguousChunkNode {
    /// The chunk at `index`, without walking the chunks before it.
    ///
    /// Intervals still have to be walked, because which interval holds a given chunk is
    /// only known from the chunk counts of the intervals before it. The chunk *within*
    /// that interval is computed arithmetically, so one huge contiguous interval costs
    /// O(1) instead of O(index) — which is what keeps a 100-billion-value range usable.
    fn chunk_at(&self, index: usize) -> Option<RangeExpr> {
        // Borrows the range rather than cloning it: this runs per random access, and a
        // `List` range would otherwise copy every value on each call.
        let mut cursor = 0usize;
        let mut remaining = index;
        while cursor < self.total_len {
            let first = range_value_at(&self.range, cursor);
            let end_idx = interval_end_index(&self.range, cursor);
            let last = range_value_at(&self.range, end_idx);
            let interval_len = (last - first + 1) as usize;
            cursor = end_idx + 1;

            let (cc, small, lo) = interval_chunking(interval_len, self.default_task_count);
            if remaining < cc {
                let offset = remaining * small + larger_chunks_before(remaining, lo, cc);
                let size = small
                    + (larger_chunks_before(remaining + 1, lo, cc)
                        - larger_chunks_before(remaining, lo, cc));
                let start = first + offset as i64;
                let end = start + size as i64 - 1;
                return Some(
                    format!("{start}-{end}")
                        .parse::<RangeExpr>()
                        .expect("range string built from valid integers")
                        .with_contiguous(true),
                );
            }
            remaining -= cc;
        }
        None
    }
}

impl Node for ContiguousChunkNode {
    fn len(&self) -> usize {
        self.num_chunks
    }
    fn get(&self, index: usize, result: &mut TaskParameterSet) {
        if let Some(r) = self.chunk_at(index) {
            result.insert(
                self.name.clone(),
                TaskParameterValue {
                    param_type: TaskParameterType::ChunkInt,
                    value: ExprValue::RangeExpr(r),
                },
            );
        }
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
                // Check by iterating chunks
                for chunk in ContiguousChunkIterState::new(self) {
                    if chunk == *r {
                        return Ok(());
                    }
                }
                Err(format!(
                    "Parameter '{}' value '{}' is not a valid chunk in the parameter space.",
                    self.name, r
                ))
            }
            _ => Err(format!(
                "Parameter '{}' value '{}' is not in the parameter space range.",
                self.name,
                v.value.to_display_string()
            )),
        }
    }
    fn iter(&self) -> Box<dyn NodeIterator> {
        Box::new(ContiguousChunkNodeIterator {
            state: ContiguousChunkIterState::new(self),
            name: self.name.clone(),
        })
    }
}

/// Reusable state for iterating contiguous chunks from a range.
/// Finds contiguous intervals, then divides each interval evenly into chunks
/// matching the Python `divide_int_interval_into_chunks` algorithm.
struct ContiguousChunkIterState {
    range: job::TaskParamRange<i64>,
    default_task_count: usize,
    total_len: usize,
    cursor: usize,
    // Current interval chunking state
    interval_start_val: i64, // first value of current interval
    interval_chunks_remaining: usize,
    interval_pos: i64, // next value to emit within interval
    interval_small: usize,
    interval_leftovers: usize,
    interval_chunk_index: usize,
    interval_chunk_count: usize,
}

impl ContiguousChunkIterState {
    fn new(node: &ContiguousChunkNode) -> Self {
        Self {
            range: node.range.clone(),
            default_task_count: node.default_task_count,
            total_len: node.total_len,
            cursor: 0,
            interval_start_val: 0,
            interval_chunks_remaining: 0,
            interval_pos: 0,
            interval_small: 0,
            interval_leftovers: 0,
            interval_chunk_index: 0,
            interval_chunk_count: 0,
        }
    }

    fn get_value(&self, i: usize) -> i64 {
        range_value_at(&self.range, i)
    }

    fn find_interval_end(&self, start: usize) -> usize {
        interval_end_index(&self.range, start)
    }
}

/// Value at index `i` of a task parameter range.
///
/// `i` must be within the range length; every caller bounds it by `total_len` first.
fn range_value_at(range: &job::TaskParamRange<i64>, i: usize) -> i64 {
    match range {
        job::TaskParamRange::List(v) => v[i],
        job::TaskParamRange::RangeExpr(r) => r.get(i as i64).expect("index within range bounds"),
    }
}

/// Chunk count and even-distribution parameters for one contiguous interval, as
/// `(chunk_count, small, leftovers)`. Shared so sequential iteration and random access
/// cannot drift apart on how an interval is split.
fn interval_chunking(interval_len: usize, default_task_count: usize) -> (usize, usize, usize) {
    let chunk_count = interval_len.div_ceil(default_task_count);
    if chunk_count >= interval_len {
        (chunk_count, 1, 0)
    } else if chunk_count <= 1 {
        (chunk_count, interval_len, 0)
    } else {
        (
            chunk_count,
            interval_len / chunk_count,
            interval_len % chunk_count,
        )
    }
}

/// Number of size-`small + 1` chunks before chunk `j` of an interval.
///
/// A chunk takes one extra value when `ceil((j+1)*leftovers/chunk_count)` exceeds
/// `ceil(j*leftovers/chunk_count)`, so summing that over the chunks before `j`
/// telescopes to the single ceiling below. That is what lets a chunk's offset and size be
/// computed without walking the chunks before it.
fn larger_chunks_before(j: usize, leftovers: usize, chunk_count: usize) -> usize {
    if leftovers == 0 {
        return 0;
    }
    // Widened to u128 for the product only. `j` is bounded by `chunk_count` and
    // `leftovers` is `interval_len % chunk_count`, so `j * leftovers` approaches
    // `(interval_len/2)^2` and overflows usize once an interval passes roughly 8.6e9
    // values — well inside the range sizes this module is built to handle. The quotient
    // is bounded by `leftovers`, so narrowing back is lossless.
    let product = (j as u128) * (leftovers as u128);
    (product.div_ceil(chunk_count as u128)) as usize
}

/// Find the last index of the contiguous interval starting at `start`.
///
/// For `RangeExpr`, uses sub-range structure to skip step-1 ranges in O(R). For `List`,
/// scans values in O(interval_len). Note a step > 1 sub-range yields one interval per
/// value, so a stepped range has as many intervals as values.
fn interval_end_index(range: &job::TaskParamRange<i64>, start: usize) -> usize {
    {
        match range {
            job::TaskParamRange::List(v) => {
                let mut end = start;
                while end + 1 < v.len() && v[end + 1] == v[end] + 1 {
                    end += 1;
                }
                end
            }
            job::TaskParamRange::RangeExpr(r) => {
                // Use sub-ranges: find which sub-range contains `start`, then
                // walk forward through step-1 sub-ranges that are adjacent.
                // cumulative_lengths is u64 (exact per-chunk element counts);
                // task-parameter ranges are bounded well below usize on all
                // practical targets, so the conversions are lossless here.
                let cumulative = r.cumulative_lengths();
                let sub_ranges = r.ranges();

                // Binary search for the sub-range containing `start`
                let sr_idx = cumulative.partition_point(|&c| c <= start as u64);
                let sr_offset = if sr_idx == 0 {
                    0
                } else {
                    cumulative[sr_idx - 1] as usize
                };

                let sr = &sub_ranges[sr_idx];

                // Where this interval ends within the current sub-range, and the value it
                // ends on. A step > 1 sub-range has a gap between each of its own values,
                // so the interval is that single value — but if it is the sub-range's
                // *last* value it can still merge into an adjacent following sub-range,
                // which is what `count_contiguous_chunks_from_sub_ranges` does. Returning
                // early here instead left iteration and `len()` disagreeing: `1-5:2,6-10`
                // at defaultTaskCount 2 counted 5 chunks but yielded 6.
                let (mut end, mut last_val) = if sr.step() != 1 {
                    let is_last_of_sub_range = start == sr_offset + sr.len() - 1;
                    if !is_last_of_sub_range {
                        return start;
                    }
                    (start, range_value_at(range, start))
                } else {
                    // Step-1: the interval extends to the end of this sub-range.
                    (sr_offset + sr.len() - 1, sr.end())
                };
                for next_sr in &sub_ranges[sr_idx + 1..] {
                    if next_sr.start() == last_val + 1 && next_sr.step() == 1 {
                        end += next_sr.len();
                        last_val = next_sr.end();
                    } else if next_sr.start() == last_val + 1 && next_sr.step() > 1 {
                        // The first value is adjacent, so it joins the interval. For a
                        // multi-value stepped sub-range the *second* value has a gap
                        // before it, so the interval ends here. A length-1 stepped
                        // sub-range has no second value, so it bridges into whatever
                        // follows instead of terminating the interval -- which is what
                        // `count_contiguous_chunks_from_sub_ranges` does, and leaving it
                        // out made `len()` under-report for e.g. `1-2,3-3:2,4-8`.
                        end += 1;
                        if next_sr.len() > 1 {
                            break;
                        }
                        last_val = next_sr.end();
                    } else {
                        break;
                    }
                }
                end
            }
        }
    }
}

impl ContiguousChunkIterState {
    /// Advance cursor to find the next contiguous interval and set up chunking state.
    fn start_next_interval(&mut self) -> bool {
        if self.cursor >= self.total_len {
            return false;
        }
        let first = self.get_value(self.cursor);

        // Find end of contiguous interval efficiently
        let end_idx = self.find_interval_end(self.cursor);
        let last = self.get_value(end_idx);
        let interval_len = (last - first + 1) as usize;
        self.cursor = end_idx + 1;

        let (chunk_count, small, leftovers) =
            interval_chunking(interval_len, self.default_task_count);

        self.interval_start_val = first;
        self.interval_pos = first;
        self.interval_chunks_remaining = chunk_count;
        self.interval_small = small;
        self.interval_leftovers = leftovers;
        self.interval_chunk_index = 0;
        self.interval_chunk_count = chunk_count;
        true
    }

    fn next_chunk(&mut self) -> Option<RangeExpr> {
        // If no chunks remaining in current interval, find next interval
        while self.interval_chunks_remaining == 0 {
            if !self.start_next_interval() {
                return None;
            }
        }

        // Even distribution, matching the reference's
        // `chunk_sizes[(i * chunk_count) // leftovers] += 1`.
        let idx = self.interval_chunk_index;
        let cc = self.interval_chunk_count;
        let lo = self.interval_leftovers;
        let size = self.interval_small
            + (larger_chunks_before(idx + 1, lo, cc) - larger_chunks_before(idx, lo, cc));

        let start = self.interval_pos;
        let end = start + size as i64 - 1;
        self.interval_pos = end + 1;
        self.interval_chunks_remaining -= 1;
        self.interval_chunk_index += 1;

        let s = format!("{start}-{end}");
        Some(
            s.parse::<RangeExpr>()
                .expect("valid range")
                .with_contiguous(true),
        )
    }
}

impl Iterator for ContiguousChunkIterState {
    type Item = RangeExpr;
    fn next(&mut self) -> Option<RangeExpr> {
        self.next_chunk()
    }
}

/// NodeIterator wrapper for ContiguousChunkNode.
struct ContiguousChunkNodeIterator {
    state: ContiguousChunkIterState,
    name: String,
}

impl NodeIterator for ContiguousChunkNodeIterator {
    fn next(&mut self, result: &mut TaskParameterSet) -> bool {
        match self.state.next_chunk() {
            Some(expr) => {
                result.insert(
                    self.name.clone(),
                    TaskParameterValue {
                        param_type: TaskParameterType::ChunkInt,
                        value: ExprValue::RangeExpr(expr),
                    },
                );
                true
            }
            None => false,
        }
    }
    fn reset(&mut self) {
        self.state.cursor = 0;
        self.state.interval_chunks_remaining = 0;
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
    /// Build a RangeExpr for chunk `i` on the fly.
    fn chunk_range_expr(&self, i: usize) -> RangeExpr {
        build_chunk_range_expr(&self.range, &self.constraint, self.small, self.leftovers, i)
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
        // Project `params` onto just this association's keys, so that
        // when this association is nested inside a parent (e.g. as a
        // child of a Product) the comparison ignores keys belonging to
        // sibling branches of the parent expression. Without this
        // projection, `params_equal` rejects every candidate on the
        // very first length check, since `params` carries the full
        // parameter set while `candidate` carries only this
        // association's children's keys.
        let assoc_keys: std::collections::HashSet<String> = {
            let mut ks = std::collections::HashSet::new();
            for child in &self.children {
                let mut sample = TaskParameterSet::new();
                child.get(0, &mut sample);
                for k in sample.keys() {
                    ks.insert(k.clone());
                }
            }
            ks
        };
        let projected: TaskParameterSet = params
            .iter()
            .filter(|(k, _)| assoc_keys.contains(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        // Linear scan: at least one index must match all children simultaneously
        for i in 0..self.length {
            let mut candidate = TaskParameterSet::new();
            for child in &self.children {
                child.get(i, &mut candidate);
            }
            if params_equal(&candidate, &projected) {
                return Ok(());
            }
        }
        // Build a display of the mismatched values
        let values: Vec<String> = projected
            .iter()
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
                let valid: HashSet<i64> = self.values.iter().copied().collect();
                for val in r.iter() {
                    if !valid.contains(&val) {
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
    /// Effective chunk size for a non-adaptive chunked space. `None` when the space has
    /// no chunked parameter. Adaptive spaces read `adaptive_chunk_size` instead, since
    /// theirs is mutable.
    chunk_size: Option<usize>,
    /// True when iteration must be sequential (adaptive or contiguous chunking).
    sequential: bool,
}

impl StepParameterSpaceIterator {
    /// Construct from a resolved `StepParameterSpace`.
    pub fn new(space: &job::StepParameterSpace) -> Result<Self, ModelError> {
        Self::new_inner(space, None)
    }

    /// Create with an explicit chunk task count override.
    /// When `Some(1)`, disables adaptive chunking and counts individual tasks.
    pub fn new_with_chunk_override(
        space: &job::StepParameterSpace,
        override_count: Option<usize>,
    ) -> Result<Self, ModelError> {
        Self::new_inner(space, override_count)
    }

    fn new_inner(
        space: &job::StepParameterSpace,
        chunk_override: Option<usize>,
    ) -> Result<Self, ModelError> {
        // CHUNK[INT] regroups values into generated RangeExpr chunks,
        // which bound values to |v| < 2^62. create_job validates this,
        // but StepParameterSpace is publicly constructible and
        // deserializable, so re-validate here: an out-of-bound value
        // would otherwise panic when a chunk is built during iteration.
        for (name, param) in &space.task_parameter_definitions {
            if let job::TaskParameter::ChunkInt {
                range: job::TaskParamRange::List(values),
                ..
            } = param
            {
                if let Some(v) = values
                    .iter()
                    .find(|v| v.unsigned_abs() >= openjd_expr::MAX_RANGE_VALUE_MAGNITUDE as u64)
                {
                    return Err(ModelError::DecodeValidation(format!(
                        "Task parameter '{name}': value {v} exceeds the CHUNK[INT] \
                         range value bound (magnitude must be below 2^62)",
                    )));
                }
            }
        }
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
                chunk_size: None,
                sequential: false,
            });
        }

        let expr = space.combination.as_deref().unwrap_or("*");

        // The chunked parameter, its effective chunk size, and whether it is adaptive, all
        // taken from the *same* parameter: the first CHUNK[INT] in definition order, which
        // is what the Python reference reports too.
        //
        // Deriving them separately would let `chunks_parameter_name` name one parameter
        // while `chunks_adaptive` described another. Template validation rejects more than
        // one CHUNK[INT] per step, but `StepParameterSpace` is publicly constructible and
        // deserializable — the same reason this function re-validates the value bound
        // above — and openjd-cli feeds `chunks_parameter_name` back into adaptive
        // chunk-size adjustment, so a mismatch would corrupt that feedback.
        //
        // Name and size are reported for any chunked space; only adaptive gets the mutable
        // Arc, because only an adaptive size can change part-way through iteration.
        let mut chunks_param_name: Option<String> = None;
        let mut chunk_size: Option<usize> = None;
        let mut adaptive_info: Option<(String, Arc<AtomicUsize>)> = None;
        for (name, param) in &space.task_parameter_definitions {
            if let job::TaskParameter::ChunkInt { chunks, .. } = param {
                let effective = chunk_override.unwrap_or(chunks.default_task_count).max(1);
                chunks_param_name = Some(name.clone());
                chunk_size = Some(effective);
                // An override pins the size, which rules out adaptive.
                if chunk_override.is_none() && chunks.target_runtime_seconds.is_some_and(|t| t > 0)
                {
                    adaptive_info = Some((name.clone(), Arc::new(AtomicUsize::new(effective))));
                }
                break;
            }
        }

        let root = if expr.trim() == "*" {
            // Default: no explicit combination — product of all params in definition order
            let mut children: Vec<Box<dyn Node>> = Vec::new();
            let mut adaptive_idx = None;
            for (i, name) in space.task_parameter_definitions.keys().enumerate() {
                if adaptive_info.as_ref().is_some_and(|(n, _)| n == name) {
                    adaptive_idx = Some(i);
                }
                children.push(make_leaf_node(name, space, &adaptive_info, chunk_override)?);
            }
            // Move adaptive child to the end (innermost/fastest-varying) to match Python
            if let Some(idx) = adaptive_idx {
                let child = children.remove(idx);
                children.push(child);
            }
            if children.len() == 1 {
                // SAFETY: We just checked len() == 1, so into_iter().next() always
                // returns Some. Using into_iter avoids an unwrap on pop().
                children
                    .into_iter()
                    .next()
                    .expect("non-empty vec with len 1")
            } else {
                let length = checked_product_len(&children)?;
                Box::new(ProductNode { children, length })
            }
        } else {
            let tokens = tokenize(expr);
            parse_node_expr(&tokens, space, &adaptive_info, chunk_override)?
        };

        let adaptive = adaptive_info.is_some();
        let adaptive_chunk_size = adaptive_info.map(|(_, rc)| rc);

        // Which path `Iterator::next` takes. Contiguous chunking stays sequential here
        // even though it now supports random access: locating a chunk means walking the
        // intervals before it, so driving a full walk through `get` would be
        // O(chunks x intervals) — quadratic for a range whose values are mostly isolated,
        // where every value is its own interval. Random access is gated separately, on
        // `adaptive` alone.
        let needs_sequential = adaptive || has_contiguous_chunks(space);
        let node_iter = if needs_sequential {
            Some(root.iter())
        } else {
            None
        };

        Ok(Self {
            root,
            names,
            current_index: 0,
            adaptive,
            adaptive_chunk_size,
            node_iter,
            chunks_param_name,
            chunk_size,
            sequential: needs_sequential,
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
    ///
    /// Returns `None` for out-of-bounds, and for adaptive chunking, where the chunk at a
    /// given index is not a function of the index alone. Gated on `adaptive` rather than
    /// `sequential`: contiguous chunking prefers sequential *iteration* for cost reasons
    /// but can still answer for a single index.
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
        match &self.adaptive_chunk_size {
            // Adaptive: read the live value, which `set_chunks_default_task_count` may
            // have changed since construction.
            Some(a) => Some(a.load(Ordering::Relaxed)),
            // Non-adaptive: the template's size, or the override that replaced it.
            None => self.chunk_size,
        }
    }

    /// Update the chunk size for adaptive chunking.
    pub fn set_chunks_default_task_count(&mut self, value: usize) {
        if let Some(ref a) = self.adaptive_chunk_size {
            a.store(value, Ordering::Relaxed);
            // The Arc<AtomicUsize> propagates to the live iterator — no reset needed.
        }
    }

    /// Rewind the iterator to the beginning so a fresh `Iterator::next`
    /// walk yields the same elements again.
    ///
    /// For non-sequential (random-access) iterators this resets the
    /// internal cursor used by `Iterator::next` to 0. For sequential
    /// (adaptive or contiguous-with-gaps) iterators it delegates to the
    /// inner node iterator's `reset()`. The adaptive `Arc<AtomicUsize>`
    /// chunk size is preserved across resets — `reset()` does not undo
    /// `set_chunks_default_task_count`.
    pub fn reset(&mut self) {
        self.current_index = 0;
        if let Some(iter) = self.node_iter.as_mut() {
            iter.reset();
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
        if self.sequential {
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
    chunk_override: Option<usize>,
) -> Result<Box<dyn Node>, ModelError> {
    let mut pos = 0;
    let result = parse_node_product(tokens, &mut pos, space, adaptive_info, chunk_override)?;
    if pos < tokens.len() {
        return Err(ModelError::DecodeValidation(format!(
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
    chunk_override: Option<usize>,
) -> Result<Box<dyn Node>, ModelError> {
    let mut children = vec![parse_node_element(
        tokens,
        pos,
        space,
        adaptive_info,
        chunk_override,
    )?];
    while *pos < tokens.len() && tokens[*pos] == "*" {
        *pos += 1;
        children.push(parse_node_element(
            tokens,
            pos,
            space,
            adaptive_info,
            chunk_override,
        )?);
    }
    if children.len() == 1 {
        // SAFETY: We just checked len() == 1, so into_iter().next() always
        // returns Some. Using into_iter avoids an unwrap on pop().
        Ok(children
            .into_iter()
            .next()
            .expect("non-empty vec with len 1"))
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
    chunk_override: Option<usize>,
) -> Result<Box<dyn Node>, ModelError> {
    if *pos >= tokens.len() {
        return Err(ModelError::DecodeValidation(
            "Unexpected end of combination expression".into(),
        ));
    }
    if tokens[*pos] == "(" {
        *pos += 1;
        let mut children = vec![parse_node_product(
            tokens,
            pos,
            space,
            adaptive_info,
            chunk_override,
        )?];
        while *pos < tokens.len() && tokens[*pos] == "," {
            *pos += 1;
            children.push(parse_node_product(
                tokens,
                pos,
                space,
                adaptive_info,
                chunk_override,
            )?);
        }
        if *pos >= tokens.len() || tokens[*pos] != ")" {
            return Err(ModelError::DecodeValidation(
                "Missing closing parenthesis in combination".into(),
            ));
        }
        *pos += 1;
        let length = children[0].len();
        for child in children.iter().skip(1) {
            if child.len() != length {
                return Err(ModelError::DecodeValidation(format!(
                    "Associative combination: all members must have the same number of values, got {} and {}",
                    length, child.len()
                )));
            }
        }
        if children.len() == 1 {
            Err(ModelError::DecodeValidation(
                "Association expression must have more than one term.".into(),
            ))
        } else {
            Ok(Box::new(AssociationNode { children, length }))
        }
    } else {
        let name = &tokens[*pos];
        *pos += 1;
        make_leaf_node(name, space, adaptive_info, chunk_override)
    }
}

/// Create a leaf node for a parameter name from the resolved definitions.
fn make_leaf_node(
    name: &str,
    space: &job::StepParameterSpace,
    adaptive_info: &Option<(String, Arc<AtomicUsize>)>,
    chunk_override: Option<usize>,
) -> Result<Box<dyn Node>, ModelError> {
    let param = space.task_parameter_definitions.get(name).ok_or_else(|| {
        ModelError::DecodeValidation(format!(
            "Unknown parameter '{name}' in combination expression"
        ))
    })?;

    match param {
        job::TaskParameter::Int { range, chunks } => {
            if let Some(chunk_cfg) = chunks {
                return make_chunk_node(name, range, chunk_cfg, adaptive_info, chunk_override);
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
            // Cloned rather than rebuilt from the f64: the element already carries
            // the spelling §7.5 asks for, and `Float64::new` would drop it.
            values: range.iter().cloned().map(ExprValue::Float).collect(),
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
            make_chunk_node(name, range, chunks, adaptive_info, chunk_override)
        }
    }
}

/// Whether any chunk parameter uses the contiguous constraint.
///
/// Such a space prefers sequential iteration: `ContiguousChunkNode` can answer a single
/// index, but only by walking the intervals before it, so a full walk driven through
/// `get` would be quadratic where values are mostly isolated.
fn has_contiguous_chunks(space: &job::StepParameterSpace) -> bool {
    space.task_parameter_definitions.values().any(|p| {
        matches!(
            p,
            job::TaskParameter::ChunkInt { chunks, .. }
                if chunks.range_constraint == RangeConstraint::Contiguous
        )
    })
}

/// Build a chunk node from a range and chunk config. Creates `AdaptiveChunkNode` when
/// `target_runtime_seconds > 0`, `ContiguousChunkNode` for contiguous static chunking,
/// or `StaticChunkNode` for noncontiguous static chunking.
fn make_chunk_node(
    name: &str,
    range: &job::TaskParamRange<i64>,
    chunks: &job::ResolvedChunks,
    adaptive_info: &Option<(String, Arc<AtomicUsize>)>,
    chunk_override: Option<usize>,
) -> Result<Box<dyn Node>, ModelError> {
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

    // Use override if provided, otherwise use the template's default
    let default_task_count = chunk_override.unwrap_or(chunks.default_task_count).max(1);

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

    // Contiguous chunking must respect gaps in the source range
    if chunks.range_constraint == RangeConstraint::Contiguous {
        return Ok(Box::new(ContiguousChunkNode::new(
            name.to_string(),
            range.clone(),
            default_task_count,
        )));
    }

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
        // Iterate first few chunks
        let first: Vec<_> = iter.take(3).collect();
        assert_eq!(first.len(), 3);
        assert!(first[0].contains_key("C"));
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

    // ── F2: get_value with range_expr returns values without panic ──

    #[test]
    fn test_contiguous_chunk_stepped_range_iterates_without_panic() {
        // Stepped range (step=2) exercises the sr.get(idx) path in
        // count_contiguous_chunks_for_range and ContiguousChunkIterState::get_value
        let space = make_space(vec![("C", static_chunk_param("1-10:2", 2))], None);
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        let results: Vec<_> = iter.collect();
        assert!(!results.is_empty(), "should produce at least one chunk");
        for r in &results {
            assert!(r.contains_key("C"));
        }
    }

    #[test]
    fn test_range_expr_random_access_does_not_panic() {
        // Exercises RangeExprNode::get which calls r.get(i as i64)
        let space = make_space(vec![("X", range_expr_param("1-5"))], None);
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        for i in 0..5 {
            let set = iter.get(i).unwrap();
            assert_eq!(set["X"].value, ExprValue::Int(i as i64 + 1));
        }
    }

    // ── Chunk metadata for non-adaptive spaces ──
    // The reference implementation reports the chunked parameter's name and chunk size
    // for any CHUNK[INT] parameter. Both were previously derived from adaptive detection,
    // so both went silent the moment a space was not adaptive — including when a chunk
    // override made an adaptive space static.

    fn noncontiguous_static_chunk_param(
        expr: &str,
        default_task_count: usize,
    ) -> job::TaskParameter {
        job::TaskParameter::ChunkInt {
            range: job::TaskParamRange::RangeExpr(expr.parse::<RangeExpr>().unwrap()),
            chunks: job::ResolvedChunks {
                default_task_count,
                target_runtime_seconds: None,
                range_constraint: RangeConstraint::Noncontiguous,
            },
        }
    }

    #[test]
    fn test_static_chunk_metadata_is_reported() {
        for param in [
            static_chunk_param("1-10", 5),
            noncontiguous_static_chunk_param("1-10", 5),
        ] {
            let space = make_space(vec![("Frame", param)], None);
            let iter = StepParameterSpaceIterator::new(&space).unwrap();
            assert!(!iter.chunks_adaptive());
            assert_eq!(iter.chunks_parameter_name(), Some("Frame"));
            assert_eq!(iter.chunks_default_task_count(), Some(5));
        }
    }

    #[test]
    fn test_chunk_metadata_reports_the_override_not_the_template() {
        let space = make_space(vec![("Frame", static_chunk_param("1-10", 5))], None);
        let iter = StepParameterSpaceIterator::new_with_chunk_override(&space, Some(2)).unwrap();
        assert_eq!(iter.chunks_parameter_name(), Some("Frame"));
        assert_eq!(iter.chunks_default_task_count(), Some(2));
    }

    #[test]
    fn test_override_of_an_adaptive_space_reports_metadata_and_is_no_longer_adaptive() {
        let space = make_space(
            vec![("Frame", adaptive_chunk_param((1..=10).collect(), 5))],
            None,
        );
        let adaptive = StepParameterSpaceIterator::new(&space).unwrap();
        assert!(adaptive.chunks_adaptive());
        assert_eq!(adaptive.chunks_default_task_count(), Some(5));

        // The override suppresses adaptive chunking, which previously also dropped both
        // getters even though the caller had just supplied the size.
        let overridden =
            StepParameterSpaceIterator::new_with_chunk_override(&space, Some(1)).unwrap();
        assert!(!overridden.chunks_adaptive());
        assert_eq!(overridden.chunks_parameter_name(), Some("Frame"));
        assert_eq!(overridden.chunks_default_task_count(), Some(1));
    }

    #[test]
    fn test_no_chunk_metadata_without_a_chunked_parameter() {
        let space = make_space(vec![("X", int_param(vec![1, 2, 3]))], None);
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        assert_eq!(iter.chunks_parameter_name(), None);
        assert_eq!(iter.chunks_default_task_count(), None);
    }

    #[test]
    fn test_adaptive_size_getter_still_tracks_the_live_value() {
        // The mutable Arc must keep winning over the construction-time size, or the
        // setter would appear to do nothing.
        let space = make_space(
            vec![("Frame", adaptive_chunk_param((1..=10).collect(), 5))],
            None,
        );
        let mut iter = StepParameterSpaceIterator::new(&space).unwrap();
        iter.set_chunks_default_task_count(3);
        assert_eq!(iter.chunks_default_task_count(), Some(3));
    }

    // ── Random access for contiguous chunking ──
    // Contiguous chunking used to force sequential iteration, so `get` and `__getitem__`
    // declined for any contiguous chunked space while `len` still reported a count.

    /// The chunk string at `index`, via random access.
    fn chunk_via_get(iter: &StepParameterSpaceIterator, index: usize) -> String {
        match &iter.get(index).expect("index within bounds")["Frame"].value {
            ExprValue::RangeExpr(r) => r.to_string(),
            other => panic!("expected a RangeExpr chunk, got {other:?}"),
        }
    }

    /// Every chunk string, via iteration.
    fn chunks_via_iteration(space: &job::StepParameterSpace) -> Vec<String> {
        StepParameterSpaceIterator::new(space)
            .unwrap()
            .map(|s| match &s["Frame"].value {
                ExprValue::RangeExpr(r) => r.to_string(),
                other => panic!("expected a RangeExpr chunk, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn test_contiguous_random_access_agrees_with_iteration_and_the_reference() {
        // Expected values are the pure-Python reference's output for the same shapes
        // (`divide_int_list_into_contiguous_chunks`), so this pins the values themselves
        // rather than only get-versus-iterate self-consistency.
        //
        // Uneven splits, exact splits, single-chunk, chunk-per-value, and ranges with
        // gaps and steps, since each drives a different branch of the interval walk.
        let cases: Vec<(&str, usize, Vec<&str>)> = vec![
            ("1-10", 3, vec!["1-3", "4-5", "6-8", "9-10"]),
            ("1-10", 5, vec!["1-5", "6-10"]),
            ("1-12", 5, vec!["1-4", "5-8", "9-12"]),
            (
                "1-10",
                1,
                vec![
                    "1-1", "2-2", "3-3", "4-4", "5-5", "6-6", "7-7", "8-8", "9-9", "10-10",
                ],
            ),
            ("1-10", 20, vec!["1-10"]),
            ("1-7", 2, vec!["1-2", "3-4", "5-6", "7-7"]),
            ("1-5,8-12", 3, vec!["1-3", "4-5", "8-10", "11-12"]),
            (
                "1-5,8-12",
                2,
                vec!["1-2", "3-4", "5-5", "8-9", "10-11", "12-12"],
            ),
            (
                "1-20:2",
                3,
                vec![
                    "1-1", "3-3", "5-5", "7-7", "9-9", "11-11", "13-13", "15-15", "17-17", "19-19",
                ],
            ),
            (
                "1-3,7,11-15",
                2,
                vec!["1-2", "3-3", "7-7", "11-12", "13-14", "15-15"],
            ),
            // A step > 1 sub-range followed by an adjacent step-1 one: the last value of
            // the stepped sub-range merges into the run that follows it, so `5` and
            // `6-10` form one interval of six values.
            ("1-5:2,6-10", 2, vec!["1-1", "3-3", "5-6", "7-8", "9-10"]),
            ("1-5:2,6-10", 3, vec!["1-1", "3-3", "5-7", "8-10"]),
            ("1-3:2,4-6", 2, vec!["1-1", "3-4", "5-6"]),
            (
                "1-9:2,10-20",
                2,
                vec![
                    "1-1", "3-3", "5-5", "7-7", "9-10", "11-12", "13-14", "15-16", "17-18", "19-20",
                ],
            ),
            // A step-1 run followed by an adjacent stepped sub-range: the follower's
            // first value joins the run, the rest are isolated.
            ("1-3,4-8:2", 2, vec!["1-2", "3-4", "6-6", "8-8"]),
            ("1-3,4-8:2", 3, vec!["1-2", "3-4", "6-6", "8-8"]),
            // Two adjacent stepped sub-ranges.
            ("1-4:3,5-9:2", 2, vec!["1-1", "4-5", "7-7", "9-9"]),
            (
                "2-8:2,9-12",
                2,
                vec!["2-2", "4-4", "6-6", "8-9", "10-11", "12-12"],
            ),
            // A length-1 stepped sub-range between two step-1 runs. Its single value is
            // adjacent on both sides, so the whole range is one contiguous interval and
            // the stepped sub-range must not terminate it.
            ("1-2,3-3:2,4-8", 2, vec!["1-2", "3-4", "5-6", "7-8"]),
        ];
        for (expr, dtc, reference) in cases {
            let space = make_space(vec![("Frame", static_chunk_param(expr, dtc))], None);

            // Iteration matches the reference.
            assert_eq!(
                chunks_via_iteration(&space),
                reference,
                "iteration disagrees with the reference for {expr} dtc={dtc}"
            );

            // Random access matches it too, index for index.
            let iter = StepParameterSpaceIterator::new(&space).unwrap();
            assert_eq!(
                iter.len(),
                reference.len(),
                "len disagrees with the reference for {expr} dtc={dtc}"
            );
            let by_index: Vec<String> = (0..iter.len()).map(|i| chunk_via_get(&iter, i)).collect();
            assert_eq!(
                by_index, reference,
                "get disagrees with the reference for {expr} dtc={dtc}"
            );
        }
    }

    #[test]
    fn test_contiguous_random_access_is_lazy_on_a_huge_range() {
        // One interval of 100 billion values. Indexing near the end must not walk the
        // chunks before it, which is the whole reason the chunk is computed
        // arithmetically rather than by advancing an iterator.
        let space = make_space(vec![("Frame", static_chunk_param(HUGE_RANGE, 1000))], None);
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        assert_eq!(iter.len(), 100_000_000);
        assert_eq!(chunk_via_get(&iter, 0), "1-1000");
        assert_eq!(chunk_via_get(&iter, 1), "1001-2000");
        assert_eq!(chunk_via_get(&iter, 99_999_999), "99999999001-100000000000");
    }

    #[test]
    fn test_chunk_offsets_do_not_overflow_on_a_huge_range() {
        // `j * leftovers` approaches (interval_len/2)^2, which passes usize::MAX once an
        // interval is over roughly 8.6e9 values. A default_task_count that divides the
        // range evenly leaves `leftovers == 0` and never multiplies, so the arithmetic has
        // to be exercised with one that leaves a remainder.
        let space = make_space(vec![("Frame", static_chunk_param(HUGE_RANGE, 3))], None);
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        let n = iter.len();
        assert_eq!(n, 33_333_333_334);

        // Both ends, plus the last index, where the product is largest.
        assert_eq!(chunk_via_get(&iter, 0), "1-3");
        assert_eq!(chunk_via_get(&iter, n - 1), "99999999999-100000000000");

        // Chunks must tile the range without gaps or overlaps. Spot-check adjacency
        // deep into the space, where a wrapped product would show up as a wild offset.
        for i in [1usize, 2, 1_000_000, 30_000_000_000, n - 2] {
            let a = chunk_via_get(&iter, i);
            let b = chunk_via_get(&iter, i + 1);
            let a_end: i64 = a.split('-').next_back().unwrap().parse().unwrap();
            let b_start: i64 = b.split('-').next().unwrap().parse().unwrap();
            assert_eq!(b_start, a_end + 1, "chunks {i} and {} do not tile", i + 1);
        }
    }

    #[test]
    fn test_len_agrees_with_iteration_across_mixed_step_ranges() {
        // `len()` comes from `count_contiguous_chunks_for_range` while iteration walks
        // intervals via `interval_end_index`. The two used to disagree whenever an
        // interval began in a step > 1 sub-range and continued into an adjacent step-1
        // one, which made `len()` under-report and left the last chunk unreachable.
        for (expr, dtc) in [
            ("1-5:2,6-10", 2),
            ("1-5:2,6-10", 3),
            ("1-3:2,4-6", 2),
            ("1-9:2,10-20", 2),
            ("1-20:2", 3),
            ("1-5,8-12", 3),
            ("1-3,4-8:2", 2),
            ("1-4:3,5-9:2", 2),
            ("2-8:2,9-12", 2),
            ("1-2,3-3:2,4-8", 2),
        ] {
            let space = make_space(vec![("Frame", static_chunk_param(expr, dtc))], None);
            let iter = StepParameterSpaceIterator::new(&space).unwrap();
            let walked = chunks_via_iteration(&space);
            assert_eq!(
                iter.len(),
                walked.len(),
                "len disagrees with iteration for {expr} dtc={dtc}"
            );
            // The final chunk must be reachable by index, which it is not when len()
            // under-reports.
            assert!(
                iter.get(walked.len() - 1).is_some(),
                "last chunk unreachable for {expr} dtc={dtc}"
            );
        }
    }

    #[test]
    fn test_chunk_metadata_and_adaptive_come_from_the_same_parameter() {
        // Template validation rejects two CHUNK[INT] parameters per step, but
        // StepParameterSpace is publicly constructible, so the getters must not describe
        // different parameters. openjd-cli feeds chunks_parameter_name back into adaptive
        // chunk-size adjustment, so a mismatch would corrupt that feedback.
        let space = make_space(
            vec![
                ("StaticFrame", noncontiguous_static_chunk_param("1-10", 5)),
                ("AdaptiveFrame", adaptive_chunk_param((1..=10).collect(), 2)),
            ],
            None,
        );
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        // The first CHUNK[INT] in definition order decides everything, as in the
        // reference, which breaks out of its scan on the first one it finds.
        assert_eq!(iter.chunks_parameter_name(), Some("StaticFrame"));
        assert!(!iter.chunks_adaptive());
        assert_eq!(iter.chunks_default_task_count(), Some(5));
    }

    #[test]
    fn test_contiguous_random_access_out_of_range_is_none() {
        let space = make_space(vec![("Frame", static_chunk_param("1-10", 5))], None);
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        assert_eq!(iter.len(), 2);
        assert!(iter.get(2).is_none());
        assert!(iter.get(1000).is_none());
    }

    #[test]
    fn test_contiguous_chunking_keeps_sequential_iteration_but_gains_random_access() {
        // The two concerns are deliberately decoupled. Iteration stays on the node
        // iterator, because driving a full walk through `get` would re-walk the intervals
        // for every index. Random access is gated on `adaptive` alone, so `get` answers.
        let space = make_space(vec![("Frame", static_chunk_param("1-10", 3))], None);
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        assert!(
            iter.sequential,
            "iteration should stay on the node iterator"
        );
        assert!(iter.node_iter.is_some());
        assert!(!iter.adaptive);
        assert!(iter.get(0).is_some(), "random access should still work");
    }

    #[test]
    fn test_many_interval_iteration_stays_linear() {
        // A stepped range makes every value its own interval, which is the shape that
        // would go quadratic if `Iterator::next` were routed through `chunk_at`. 20k
        // values would be ~4e8 interval walks; this completes promptly because iteration
        // uses the sequential path.
        let space = make_space(vec![("Frame", static_chunk_param("1-40000:2", 1))], None);
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        assert_eq!(iter.len(), 20_000);
        let chunks: Vec<String> = StepParameterSpaceIterator::new(&space)
            .unwrap()
            .map(|s| match &s["Frame"].value {
                ExprValue::RangeExpr(r) => r.to_string(),
                other => panic!("expected a RangeExpr chunk, got {other:?}"),
            })
            .collect();
        assert_eq!(chunks.len(), 20_000);
        assert_eq!(chunks[0], "1-1");
        assert_eq!(chunks[19_999], "39999-39999");
        // Random access agrees at both ends of the same space.
        assert_eq!(chunk_via_get(&iter, 0), "1-1");
        assert_eq!(chunk_via_get(&iter, 19_999), "39999-39999");
    }

    #[test]
    fn test_contiguous_random_access_within_a_product() {
        // ProductNode::get divides the index across children, so a chunked child has to
        // answer for an index that is not the outer index.
        let space = make_space(
            vec![
                ("A", int_param(vec![1, 2])),
                ("Frame", static_chunk_param("1-10", 5)),
            ],
            None,
        );
        let iter = StepParameterSpaceIterator::new(&space).unwrap();
        assert_eq!(iter.len(), 4); // 2 values x 2 chunks
        let expected: Vec<(i64, String)> = StepParameterSpaceIterator::new(&space)
            .unwrap()
            .map(|s| {
                let a = match &s["A"].value {
                    ExprValue::Int(v) => *v,
                    other => panic!("expected Int, got {other:?}"),
                };
                let f = match &s["Frame"].value {
                    ExprValue::RangeExpr(r) => r.to_string(),
                    other => panic!("expected RangeExpr, got {other:?}"),
                };
                (a, f)
            })
            .collect();
        for (i, want) in expected.iter().enumerate() {
            let set = iter.get(i).unwrap();
            let a = match &set["A"].value {
                ExprValue::Int(v) => *v,
                other => panic!("expected Int, got {other:?}"),
            };
            assert_eq!(&(a, chunk_via_get(&iter, i)), want, "product index {i}");
        }
    }
}
