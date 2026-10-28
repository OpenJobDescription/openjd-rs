// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Integer range expressions.
//!
//! Mirrors Python `openjd.expr._range_expr`. Parses expressions like
//! `"1-10"`, `"1-10:2"`, `"1-5,10-15"` into a set of integers, stored as
//! the runs of its canonical form (Template Schemas §3.4.1.1.1), which is
//! also what `Display` writes.

use crate::error::ExpressionError;
use std::fmt;

/// Maximum number of comma-separated sub-ranges in a single range expression.
///
/// Parsing, [`RangeExpr::from_ranges`], and deserialization reject both an
/// input with more sub-ranges than this and a value whose canonical form
/// has more runs than this. The canonical form can have up to two runs per
/// input sub-range (`"0-1,3-4"` is `"0,1,3,4"`), so the second check is what
/// guarantees that the canonical form of every accepted value parses (and
/// deserializes) back. Values computed by [`RangeExpr::from_values`] and
/// [`RangeExpr::slice`] are not capped; their size is bounded by the
/// evaluator's memory limit instead.
/// Real-world range expressions (frame ranges, task chunks) contain at most
/// a few dozen sub-ranges; 10,000 is two orders of magnitude above any
/// plausible legitimate use. Rejecting larger inputs at parse time prevents
/// an attacker from forcing a multi-megabyte `Vec<IntRange>` allocation
/// through a parameter value before any downstream resource-bounding
/// (e.g. the evaluator's memory limit) applies.
///
/// This cap targets the source-text and heap dimensions of a `RangeExpr`.
/// It does **not** cap the logical element count of a single chunk (that is
/// [`MAX_RANGE_EXPR_LEN`]) —
/// `RangeExpr` stores chunks symbolically (`start`, `end`, `step`), so a
/// single-chunk range `"1-100000000000"` allocates only one `IntRange`
/// regardless of its logical length. Downstream materialization (e.g.
/// `list(range_expr)`) is already bounded by the evaluator's per-element
/// operation charge and memory limit.
///
/// See `specs/expr/range-expr.md` (Defensive caps) for rationale.
pub const MAX_RANGE_EXPR_CHUNKS: usize = 10_000;

/// Error raised when parsing a range expression fails.
#[derive(Debug, Clone)]
#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub struct RangeExprError {
    pub expr: String,
    pub message: String,
    pub position: Option<usize>,
}

impl RangeExprError {
    #[allow(
        missing_docs,
        reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
    )]
    pub fn new(expr: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            expr: expr.into(),
            message: message.into(),
            position: None,
        }
    }
    #[allow(
        missing_docs,
        reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
    )]
    pub fn at(expr: impl Into<String>, message: impl Into<String>, position: usize) -> Self {
        Self {
            expr: expr.into(),
            message: message.into(),
            position: Some(position),
        }
    }
}

impl fmt::Display for RangeExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(pos) = self.position {
            write!(
                f,
                "{} in '{}' after '{}'",
                self.message,
                self.expr,
                &self.expr[..pos]
            )
        } else {
            write!(f, "{}: '{}'", self.message, self.expr)
        }
    }
}

impl std::error::Error for RangeExprError {}

impl From<RangeExprError> for ExpressionError {
    /// Lift a range-expression parse error into an `ExpressionError`.
    ///
    /// If the error carries a `position`, attach the source string with a
    /// span covering the single character at that offset so the standard
    /// caret renderer points at the failure. Without a position, fall
    /// back to the stringified `Display` form.
    fn from(e: RangeExprError) -> Self {
        let msg = e.to_string();
        let err = ExpressionError::parse_error(msg);
        match e.position {
            Some(pos) if pos < e.expr.len() => err.with_span(&e.expr, pos, pos + 1),
            _ => err,
        }
    }
}

/// An arithmetic progression of integers: `start`, `start + step`, ...,
/// `end`.
///
/// Fields are private to protect the construction invariants that
/// `len_u64()`, iteration, and indexing arithmetic rely on: `start <= end`,
/// `step > 0`, `end - start` is a multiple of `step`, no value or step is
/// `i64::MIN`, and the progression has at most [`MAX_RANGE_EXPR_LEN`]
/// values. Deserialization re-validates through [`IntRange::new`]. Read
/// access is via the accessor methods.
///
/// Each `<Int>`, `<Range>`, or `<SkipRange>` element of a range expression
/// is one `IntRange`. A [`RangeExpr`] stores the runs of its canonical form
/// as `IntRange`s (see [`RangeExpr::ranges`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
pub struct IntRange {
    start: i64,
    end: i64,
    step: i64,
}

impl<'de> serde::Deserialize<'de> for IntRange {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct IntRangeHelper {
            start: i64,
            end: i64,
            step: i64,
        }
        let h = IntRangeHelper::deserialize(deserializer)?;
        IntRange::new(h.start, h.end, h.step).map_err(serde::de::Error::custom)
    }
}

/// Maximum number of values in a range expression: `i64::MAX` (2^63 - 1).
///
/// The expression language's `int` is a 64-bit signed integer, so the
/// count returned by `len()` and every index into a range expression must
/// fit one. A range expression (or a single [`IntRange`]) with more values
/// is rejected at construction. Keeping the count at or below `i64::MAX`
/// also leaves the high bit of the packed length field free for the
/// contiguous display flag.
///
/// Values themselves may be any `i64` except `i64::MIN` (see
/// `specs/expr/range-expr.md`, Value bounds), so a range like
/// `"-9223372036854775807,9223372036854775807"` is accepted while
/// `"-9223372036854775807-9223372036854775807"` (2^64 - 1 values) is not.
pub const MAX_RANGE_EXPR_LEN: u64 = i64::MAX as u64;

/// Validate one range endpoint, step, or value: everything except
/// `i64::MIN`, whose negation overflows, is accepted.
fn check_range_value(v: i64) -> Result<(), ExpressionError> {
    if v == i64::MIN {
        Err(ExpressionError::integer_overflow())
    } else {
        Ok(())
    }
}

/// The error for a range expression with more than [`MAX_RANGE_EXPR_LEN`]
/// values.
fn too_many_values() -> ExpressionError {
    ExpressionError::parse_error(format!(
        "Range expression has too many values; the maximum is {MAX_RANGE_EXPR_LEN}"
    ))
}

// Arithmetic note: with values in (i64::MIN, i64::MAX] the span `end -
// start` reaches 2^64 - 2, which overflows i64 but fits u64. Spans and
// offsets are therefore computed with `abs_diff` (u64) and applied with
// `wrapping_add_unsigned`/`wrapping_sub_unsigned`; each such result is a
// value of the progression, so it is in range and the wrapping is exact.
impl IntRange {
    /// Create a range, normalizing descending ranges to ascending form.
    /// After construction, `start <= end`, `step > 0`, and `end` is the
    /// last value of the progression (so `"1-10:4"` has `end` 9).
    ///
    /// Returns an error for a zero step, a direction/step mismatch, an
    /// endpoint or step of `i64::MIN` (integer overflow), or more than
    /// [`MAX_RANGE_EXPR_LEN`] values.
    pub fn new(start: i64, end: i64, step: i64) -> Result<Self, ExpressionError> {
        if step == 0 {
            return Err(ExpressionError::parse_error("Range: step must not be zero"));
        }
        check_range_value(start)?;
        check_range_value(end)?;
        check_range_value(step)?;
        if step > 0 && start > end {
            return Err(ExpressionError::parse_error(
                "Range: a descending range must have a negative step",
            ));
        }
        if step < 0 && start < end {
            return Err(ExpressionError::parse_error(
                "Range: an ascending range must have a positive step",
            ));
        }
        let magnitude = step.unsigned_abs();
        let steps = start.abs_diff(end) / magnitude;
        if steps >= MAX_RANGE_EXPR_LEN {
            return Err(too_many_values());
        }
        // `offset <= |end - start|`, so it fits u64 and lands on a value
        // between start and end.
        let offset = steps * magnitude;
        // `magnitude <= i64::MAX` because `step != i64::MIN`.
        let step = magnitude as i64;
        if start > end {
            // Normalize descending to ascending form (matching Python
            // _IntRange): the last value reached becomes the start.
            Ok(Self {
                start: start.wrapping_sub_unsigned(offset),
                end: start,
                step,
            })
        } else {
            Ok(Self {
                start,
                end: start.wrapping_add_unsigned(offset),
                step,
            })
        }
    }

    /// The one-value range `value`. The caller guarantees `value` is not
    /// `i64::MIN`.
    fn single(value: i64) -> Self {
        Self {
            start: value,
            end: value,
            step: 1,
        }
    }

    /// Number of integers in this range.
    ///
    /// Exact on 64-bit targets, where a count (at most
    /// [`MAX_RANGE_EXPR_LEN`]) always fits `usize`. On 32-bit targets a
    /// larger count clamps to `usize::MAX`.
    pub fn len(&self) -> usize {
        usize::try_from(self.len_u64()).unwrap_or(usize::MAX)
    }

    /// Number of integers in this range as an exact u64, at most
    /// [`MAX_RANGE_EXPR_LEN`].
    pub(crate) fn len_u64(&self) -> u64 {
        self.end.abs_diff(self.start) / self.step as u64 + 1
    }

    /// Returns `true` if the range contains no elements.
    pub fn is_empty(&self) -> bool {
        self.len_u64() == 0
    }

    /// Test whether `value` is a member of this range.
    pub fn contains(&self, value: i64) -> bool {
        if value < self.start || value > self.end {
            return false;
        }
        value.abs_diff(self.start).is_multiple_of(self.step as u64)
    }

    /// Value at zero-based index `k`. The caller is responsible for `k`
    /// being in bounds; then `k * step <= end - start` fits u64.
    fn nth(&self, k: u64) -> i64 {
        self.start.wrapping_add_unsigned(k * self.step as u64)
    }

    /// Iterate over all values in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = i64> + '_ {
        (0..self.len_u64()).map(move |i| self.nth(i))
    }

    /// First value in the range (`start <= end` after normalization).
    pub fn start(&self) -> i64 {
        self.start
    }

    /// Last value in the range, inclusive.
    pub fn end(&self) -> i64 {
        self.end
    }

    /// Step between values (always positive after normalization).
    pub fn step(&self) -> i64 {
        self.step
    }

    /// Get element by zero-based index, or `None` if out of bounds.
    pub fn get(&self, index: usize) -> Option<i64> {
        self.value_at(index as u64)
    }

    /// Get element by zero-based u64 index, or `None` if out of bounds.
    /// (u64 rather than usize so symbolic offsets beyond the pointer
    /// width still resolve on 32-bit targets.)
    pub(crate) fn value_at(&self, index: u64) -> Option<i64> {
        if index < self.len_u64() {
            Some(self.nth(index))
        } else {
            None
        }
    }
}

/// Writes the element form of this one progression: `x` for one value,
/// `x,y` for two, `x-y` for three or more with step 1, and `x-y:s`
/// otherwise. For a run of a [`RangeExpr`]'s canonical form (never two
/// values), this is the run as the canonical form writes it.
impl std::fmt::Display for IntRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self.len_u64();
        if len == 1 {
            write!(f, "{}", self.start)
        } else if len == 2 {
            write!(f, "{},{}", self.start, self.end)
        } else if self.step == 1 {
            write!(f, "{}-{}", self.start, self.end)
        } else {
            write!(f, "{}-{}:{}", self.start, self.end, self.step)
        }
    }
}

/// A range expression: a set of integers, stored as the runs of its
/// canonical form.
///
/// The value of a range expression is its list of values in increasing
/// order (Template Schemas §3.4.1.1.1); how it was written is not part of
/// the value. Every constructor normalizes the representation to the runs
/// of the canonical form, so equal values have equal representations:
/// `==` and `Hash` compare values, and `Display` writes the canonical form
/// (or `<N>-<M>` for a value flagged with [`RangeExpr::with_contiguous`]).
#[derive(Debug, Clone, Eq, serde::Serialize)]
pub struct RangeExpr {
    /// The runs of the canonical form, in increasing order: each is one
    /// value (`step` 1) or three or more values with a constant step.
    ranges: Vec<IntRange>,
    /// Exact cumulative length indices for O(log n) getitem (mirrors the
    /// Python reference's `_range_length_indices` bisect table). Entry i =
    /// total elements in ranges[0..=i], at most [`MAX_RANGE_EXPR_LEN`].
    cumulative_lengths: Vec<u64>,
    /// Packed: lower 63 bits = length, MSB = contiguous display flag.
    /// The length is at most [`MAX_RANGE_EXPR_LEN`] (2^63 - 1), so it never
    /// reaches the flag bit. The flag only affects Display; it is not
    /// preserved through constructors. u64 (not usize) so the packing is
    /// identical on 32-bit targets.
    #[serde(serialize_with = "serialize_length")]
    length: u64,
}

const CONTIGUOUS_BIT: u64 = 1 << 63;
const LENGTH_MASK: u64 = !CONTIGUOUS_BIT;

fn serialize_length<S: serde::Serializer>(length: &u64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u64(length & LENGTH_MASK)
}

impl<'de> serde::Deserialize<'de> for RangeExpr {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct RangeExprHelper {
            ranges: Vec<IntRange>,
        }
        let helper = RangeExprHelper::deserialize(deserializer)?;
        Self::from_ranges(helper.ranges).map_err(serde::de::Error::custom)
    }
}

/// Value equality: the representation is canonical, so comparing the runs
/// compares the lists of values (Expression Language §1.2.5). The
/// contiguous display flag is not part of the value.
impl PartialEq for RangeExpr {
    fn eq(&self, other: &Self) -> bool {
        self.ranges == other.ranges
    }
}

/// Consistent with `==`: hashes the canonical runs, not the display flag.
impl std::hash::Hash for RangeExpr {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.ranges.hash(state);
    }
}

impl std::str::FromStr for RangeExpr {
    type Err = ExpressionError;

    fn from_str(expr: &str) -> Result<Self, Self::Err> {
        parse_range_expr(expr)
    }
}

/// Rewrite a sequence of progressions whose concatenated values are
/// strictly increasing as the runs of the canonical form (Template Schemas
/// §3.4.1.1.1), without listing the values.
///
/// This is the informative algorithm of that section. A run starts at the
/// first value `x` not yet in a run, takes the difference `s` to the next
/// value, and extends while consecutive values differ by `s`: along a
/// progression with step `s` it jumps to that progression's end in one
/// move, and across a boundary it compares the next progression's first
/// value. A run of three or more values is kept; otherwise `x` is a run
/// alone and the next run starts at the value after `x`. Each progression
/// is visited a bounded number of times, so the work is O(number of
/// progressions), and the output has at most two runs per progression.
fn canonical_runs(progressions: &[IntRange]) -> Vec<IntRange> {
    let count = progressions.len();
    let mut runs = Vec::with_capacity(count);
    let last_offset = |p: usize| progressions[p].len_u64() - 1;
    let value = |p: usize, o: u64| progressions[p].nth(o);
    let next = |p: usize, o: u64| -> Option<(usize, u64)> {
        if o < last_offset(p) {
            Some((p, o + 1))
        } else if p + 1 < count {
            Some((p + 1, 0))
        } else {
            None
        }
    };
    let mut position = if count == 0 { None } else { Some((0, 0)) };
    while let Some((p, o)) = position {
        let first = value(p, o);
        let Some(second) = next(p, o) else {
            runs.push(IntRange::single(first));
            break;
        };
        // Values increase, so this is the difference itself. It can exceed
        // i64::MAX (for -i64::MAX and i64::MAX), but not u64::MAX.
        let step = value(second.0, second.1).abs_diff(first);
        let (mut end_p, mut end_o) = second;
        let mut length: u64 = 2;
        loop {
            if end_o < last_offset(end_p) {
                // Inside a progression the next difference is its step.
                if progressions[end_p].step as u64 != step {
                    break;
                }
                length += last_offset(end_p) - end_o;
                end_o = last_offset(end_p);
            } else if end_p + 1 < count
                && progressions[end_p + 1].start.abs_diff(value(end_p, end_o)) == step
            {
                end_p += 1;
                end_o = 0;
                length += 1;
            } else {
                break;
            }
        }
        if length >= 3 {
            // Three values span 2 * step <= 2^64 - 2, so the step of a run
            // of three or more values fits i64.
            runs.push(IntRange {
                start: first,
                end: value(end_p, end_o),
                step: step as i64,
            });
            position = next(end_p, end_o);
        } else {
            runs.push(IntRange::single(first));
            position = Some(second);
        }
    }
    // The capacity is one per progression; a value keeps only its runs.
    runs.shrink_to_fit();
    runs
}

impl RangeExpr {
    /// The empty range expression (from an empty list or an empty slice).
    fn empty() -> Self {
        Self {
            ranges: Vec::new(),
            cumulative_lengths: Vec::new(),
            length: 0,
        }
    }

    /// Build from progressions whose concatenated values are strictly
    /// increasing and number at most [`MAX_RANGE_EXPR_LEN`] in total.
    fn from_increasing(progressions: &[IntRange]) -> Self {
        let ranges = canonical_runs(progressions);
        let cumulative_lengths = build_cumulative(&ranges);
        let length = cumulative_lengths.last().copied().unwrap_or(0);
        Self {
            ranges,
            cumulative_lengths,
            length,
        }
    }

    /// Set contiguous display mode. When true and the values are a
    /// contiguous run of integers, Display writes `<N>-<M>` even for one or
    /// two values (e.g., `"5-5"`, `"5-6"`): the form of a `CONTIGUOUS`
    /// chunk of a `CHUNK[INT]` task parameter (Template Schemas §3.4.1.5).
    /// A value that is not contiguous is written in canonical form either
    /// way. The flag is not part of the value: it does not affect `==`,
    /// `Hash`, or iteration, and values computed from this one do not
    /// inherit it.
    #[must_use]
    pub fn with_contiguous(mut self, contiguous: bool) -> Self {
        if contiguous {
            self.length |= CONTIGUOUS_BIT;
        } else {
            self.length &= LENGTH_MASK;
        }
        self
    }

    /// Create a RangeExpr from a list of individual values, in any order
    /// and with duplicates allowed (the inputs Python `from_list` accepts).
    /// The result is written in canonical form, so `[1, 2, 4, 6]` displays
    /// as `"1,2-6:2"` where Python's `from_list` writes `"1,2,4,6"`.
    ///
    /// Runs in O(n log n) for the sort, then O(n) to find the runs of the
    /// canonical form. Returns an integer-overflow error if any value is
    /// `i64::MIN`.
    pub fn from_values(mut values: Vec<i64>) -> Result<Self, ExpressionError> {
        for v in &values {
            check_range_value(*v)?;
        }
        values.sort_unstable();
        values.dedup();
        // A Vec holds at most isize::MAX values, within MAX_RANGE_EXPR_LEN.
        let singles: Vec<IntRange> = values.into_iter().map(IntRange::single).collect();
        Ok(Self::from_increasing(&singles))
    }

    /// Create from pre-built `IntRange`s in any order. Sorts them,
    /// validates that they do not overlap, and normalizes to the runs of
    /// the canonical form.
    ///
    /// Returns an error if the number of input ranges, or of runs in the
    /// canonical form, exceeds [`MAX_RANGE_EXPR_CHUNKS`], if two ranges
    /// overlap (their intervals
    /// from first to last value intersect), if the total number of values
    /// exceeds [`MAX_RANGE_EXPR_LEN`], or if any range violates the
    /// `IntRange::new` invariants. Each input is re-normalized through
    /// `IntRange::new` as defense in depth. `IntRange`'s fields are private
    /// and its `Deserialize` implementation already re-validates, but
    /// `from_ranges` is public API, so it independently preserves the
    /// invariants required by indexing and length arithmetic.
    pub fn from_ranges(ranges: Vec<IntRange>) -> Result<Self, ExpressionError> {
        if ranges.is_empty() {
            return Err(ExpressionError::parse_error(
                "Range expression cannot be empty",
            ));
        }
        if ranges.len() > MAX_RANGE_EXPR_CHUNKS {
            return Err(ExpressionError::parse_error(format!(
                "Range expression has too many sub-ranges ({}); maximum is {}",
                ranges.len(),
                MAX_RANGE_EXPR_CHUNKS,
            )));
        }
        let mut ranges = ranges
            .into_iter()
            .map(|r| IntRange::new(r.start, r.end, r.step))
            .collect::<Result<Vec<_>, _>>()?;
        ranges.sort_by_key(|r| (r.start, r.end));
        for pair in ranges.windows(2) {
            if pair[1].start <= pair[0].end {
                return Err(ExpressionError::parse_error(format!(
                    "Range expression has overlapping ranges: {} and {}",
                    pair[0], pair[1]
                )));
            }
        }
        let mut total: u64 = 0;
        for r in &ranges {
            total = total
                .checked_add(r.len_u64())
                .filter(|&t| t <= MAX_RANGE_EXPR_LEN)
                .ok_or_else(too_many_values)?;
        }
        let result = Self::from_increasing(&ranges);
        // The canonical form can have up to two runs per input range, so
        // cap it as well: then the canonical form of every value accepted
        // here is itself accepted here, by `parse` and by `Deserialize`.
        if result.ranges.len() > MAX_RANGE_EXPR_CHUNKS {
            return Err(ExpressionError::parse_error(format!(
                "Range expression has too many sub-ranges in canonical form ({}); maximum is {}",
                result.ranges.len(),
                MAX_RANGE_EXPR_CHUNKS,
            )));
        }
        Ok(result)
    }

    /// Total number of integers across all sub-ranges.
    ///
    /// Exact on 64-bit targets. (On 32-bit targets a count above
    /// usize::MAX clamps; index arithmetic internally uses the exact
    /// u64 count via the crate-private `len_u64`.)
    pub fn len(&self) -> usize {
        usize::try_from(self.length & LENGTH_MASK).unwrap_or(usize::MAX)
    }

    /// Exact total number of integers across all sub-ranges as u64, at
    /// most [`MAX_RANGE_EXPR_LEN`], so it also converts to i64 exactly.
    pub(crate) fn len_u64(&self) -> u64 {
        self.length & LENGTH_MASK
    }

    /// Returns `true` if the range expression contains no elements.
    pub fn is_empty(&self) -> bool {
        self.length & LENGTH_MASK == 0
    }

    /// Test membership via binary search on sub-range endpoints. O(log m).
    pub fn contains(&self, value: i64) -> bool {
        // Binary search on range ends (mirrors Python's bisect_left on _ends)
        let idx = self.ranges.partition_point(|r| r.end < value);
        idx < self.ranges.len() && self.ranges[idx].contains(value)
    }

    /// Value at flattened index `idx`, or `None` if out of bounds.
    /// O(log m) via the cumulative table (mirrors Python's bisect on
    /// `_range_length_indices`).
    pub(crate) fn value_at(&self, idx: u64) -> Option<i64> {
        if idx >= self.len_u64() {
            return None;
        }
        let range_idx = self.cumulative_lengths.partition_point(|&cum| cum <= idx);
        let offset = if range_idx == 0 {
            idx
        } else {
            idx - self.cumulative_lengths[range_idx - 1]
        };
        self.ranges[range_idx].value_at(offset)
    }

    /// Get element by index (supports negative indexing like Python).
    pub fn get(&self, index: i64) -> Option<i64> {
        // Counts are at most i64::MAX, so len_u64 as i64 is exact.
        let idx = if index < 0 {
            index + self.len_u64() as i64
        } else {
            index
        };
        if idx < 0 {
            return None;
        }
        self.value_at(idx as u64)
    }

    /// The runs of the canonical form, in increasing order. Each run is
    /// one value (with `step` 1) or three or more values with a constant
    /// step, so the runs are determined by the list of values alone: for
    /// example `"1,3,5,7,9"` and `"1-9:2"` both have the one run 1 to 9
    /// step 2, and `"1-2"` has the two runs 1 and 2.
    pub fn ranges(&self) -> &[IntRange] {
        &self.ranges
    }

    /// Cumulative element counts per sub-range (for index mapping).
    pub fn cumulative_lengths(&self) -> &[u64] {
        &self.cumulative_lengths
    }

    /// Iterate over all values in ascending order across all sub-ranges.
    pub fn iter(&self) -> impl Iterator<Item = i64> + '_ {
        self.ranges.iter().flat_map(|r| r.iter())
    }

    /// Collect every value, in ascending order, into a vector. This
    /// materializes the range; prefer [`RangeExpr::iter`] or indexing for
    /// large ranges.
    pub fn to_vec(&self) -> Vec<i64> {
        self.iter().collect()
    }

    /// Slice this range expression with a positive step.
    ///
    /// `start`, `stop`, `step` are already-normalized indices into the
    /// flattened element sequence. `step` must be positive. Returns a new
    /// `RangeExpr`, in canonical form, without materializing any elements.
    ///
    /// Runs in O(m) where m is the number of sub-ranges, regardless of
    /// how many elements are selected. Each sub-range's intersection with
    /// the slice index sequence is computed as pure arithmetic.
    ///
    /// For negative step (reverse slices), callers should use `get()` to
    /// collect elements into a list, since `RangeExpr` cannot represent
    /// descending sequences.
    pub fn slice(&self, start: i64, stop: i64, step: i64) -> Result<RangeExpr, ExpressionError> {
        if step <= 0 {
            return Err(ExpressionError::parse_error(
                "RangeExpr::slice requires a positive step",
            ));
        }
        // Index-space arithmetic is u64: global indices are non-negative
        // and bounded by the total element count (<= i64::MAX), but sums
        // like `offset + step` can reach ~2^64 for a user-supplied step
        // near i64::MAX. A step beyond total_len selects only the start
        // index, so clamp it — that also keeps `k * step` bounded.
        let total_len = self.len_u64() as i64;
        let start = start.clamp(0, total_len) as u64;
        let stop = stop.clamp(0, total_len) as u64;
        let step = step.min(total_len.max(1)) as u64;
        if start >= stop {
            return Ok(Self::empty());
        }

        let mut selected = Vec::new();
        let mut cum_start: u64 = 0; // global index where current sub-range begins

        for r in &self.ranges {
            let r_len = r.len_u64();
            let cum_end = cum_start + r_len; // exclusive end of this sub-range's global indices

            // The slice selects global indices start, start+step, ...;
            // find the first one at or after cum_start, intersected with
            // [start, stop).
            let first_global = if start >= cum_start {
                start
            } else {
                // First multiple of step at or after cum_start
                let offset = cum_start - start;
                let k = offset.div_ceil(step);
                start + k * step
            };

            let range_stop = stop.min(cum_end);
            if first_global >= range_stop {
                cum_start = cum_end;
                continue;
            }

            debug_assert!((first_global - start).is_multiple_of(step));

            let first_local = first_global - cum_start;
            let count = (range_stop - first_global - 1) / step + 1;
            let last_local = first_local + (count - 1) * step;
            let new_start = r.nth(first_local);
            let new_end = r.nth(last_local);
            // The selected values' spacing is r.step * step. With three or
            // more of them it is at most half their span and fits i64; with
            // exactly two it can exceed i64::MAX, and then they are stored
            // as two single values (canonicalization below treats the two
            // forms alike).
            let new_step = (r.step as u64)
                .checked_mul(step)
                .filter(|&s| s <= i64::MAX as u64);

            match new_step {
                _ if count == 1 => selected.push(IntRange::single(new_start)),
                Some(ns) => selected.push(IntRange {
                    start: new_start,
                    end: new_end,
                    step: ns as i64,
                }),
                None => {
                    debug_assert_eq!(count, 2);
                    selected.push(IntRange::single(new_start));
                    selected.push(IntRange::single(new_end));
                }
            }

            cum_start = cum_end;
        }

        // The selected values are a subset of this range's, so they are
        // increasing and number at most MAX_RANGE_EXPR_LEN.
        Ok(Self::from_increasing(&selected))
    }

    /// Heap allocation size (for memory tracking).
    pub fn heap_size(&self) -> usize {
        use std::mem::size_of;
        self.ranges.capacity() * size_of::<IntRange>()
            + self.cumulative_lengths.capacity() * size_of::<u64>()
    }
}

fn build_cumulative(ranges: &[IntRange]) -> Vec<u64> {
    let mut cum = Vec::with_capacity(ranges.len());
    let mut total = 0u64;
    for r in ranges {
        total += r.len_u64();
        cum.push(total);
    }
    cum
}

/// Writes the canonical form (Template Schemas §3.4.1.1.1): the runs, each
/// as `x`, `x-y` (step 1), or `x-y:s`, joined by `,`. A value flagged with
/// [`RangeExpr::with_contiguous`] whose values are contiguous is written
/// `<N>-<M>` instead. O(number of runs).
impl std::fmt::Display for RangeExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self.len_u64();
        if self.length & CONTIGUOUS_BIT != 0 && len > 0 {
            let first = self.ranges[0].start;
            let last = self.ranges[self.ranges.len() - 1].end;
            if last.abs_diff(first) == len - 1 {
                return write!(f, "{first}-{last}");
            }
        }
        for (i, run) in self.ranges.iter().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            write!(f, "{run}")?;
        }
        Ok(())
    }
}

/// Parse a range expression string.
fn parse_range_expr(expr: &str) -> Result<RangeExpr, ExpressionError> {
    let expr = expr.trim();
    if expr.is_empty() {
        return Err(ExpressionError::parse_error("Empty expression"));
    }

    let mut ranges = Vec::new();
    let mut pos = 0;
    let bytes = expr.as_bytes();

    loop {
        // Bail out early if the input is producing more sub-ranges than
        // we are willing to handle. `from_ranges` would reject the same
        // input, but the early check lets us stop consuming source
        // characters as soon as the limit is hit.
        if ranges.len() > MAX_RANGE_EXPR_CHUNKS {
            return Err(ExpressionError::parse_error(format!(
                "Range expression has too many sub-ranges (> {MAX_RANGE_EXPR_CHUNKS}): '{expr}'",
            )));
        }
        // Skip whitespace
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            // If we got here after a comma, that's a trailing comma error
            if !ranges.is_empty() && pos > 0 {
                // Check if the last non-whitespace char before end was a comma
                let last_content = expr.trim_end();
                if last_content.ends_with(',') {
                    return Err(ExpressionError::parse_error(format!(
                        "Trailing comma in range expression: '{expr}'"
                    )));
                }
            }
            break;
        }

        // Parse integer (possibly negative)
        let start = parse_integer(expr, &mut pos)?;

        // Skip whitespace
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }

        if pos >= bytes.len() || bytes[pos] == b',' {
            // Single value
            ranges.push(IntRange::new(start, start, 1)?);
            if pos < bytes.len() {
                pos += 1;
            } // skip comma
            continue;
        }

        if bytes[pos] != b'-' {
            return Err(ExpressionError::parse_error(format!(
                "Unexpected '{}' in '{expr}'",
                bytes[pos] as char
            )));
        }
        pos += 1; // skip '-'

        // Skip whitespace
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }

        let end = parse_integer(expr, &mut pos)?;

        // Skip whitespace
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }

        if pos >= bytes.len() || bytes[pos] == b',' {
            // Range without step
            if start <= end {
                ranges.push(IntRange::new(start, end, 1)?);
            } else {
                // Descending range without step is invalid per spec
                return Err(ExpressionError::parse_error(format!(
                    "Descending range {start}-{end} requires a negative step"
                )));
            }
            if pos < bytes.len() {
                pos += 1;
            } // skip comma
            continue;
        }

        if bytes[pos] != b':' {
            return Err(ExpressionError::parse_error(format!(
                "Expected ':' or ',' in '{expr}'"
            )));
        }
        pos += 1; // skip ':'

        // Skip whitespace
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }

        let step = parse_integer(expr, &mut pos)?;
        if step == 0 {
            return Err(ExpressionError::parse_error("Step must not be zero"));
        }

        ranges.push(IntRange::new(start, end, step)?);

        // Skip whitespace
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }

        if pos < bytes.len() {
            if bytes[pos] == b',' {
                pos += 1;
            } else {
                return Err(ExpressionError::parse_error(format!(
                    "Unexpected '{}' in '{expr}'",
                    bytes[pos] as char
                )));
            }
        }
    }

    if ranges.is_empty() {
        return Err(ExpressionError::parse_error("Empty expression"));
    }

    RangeExpr::from_ranges(ranges)
}

fn parse_integer(expr: &str, pos: &mut usize) -> Result<i64, ExpressionError> {
    let bytes = expr.as_bytes();
    if *pos >= bytes.len() {
        return Err(ExpressionError::parse_error(format!(
            "Unexpected end of expression: '{expr}'"
        )));
    }

    let negative = bytes[*pos] == b'-';
    if negative {
        *pos += 1;
    }

    if *pos >= bytes.len() || !bytes[*pos].is_ascii_digit() {
        return Err(ExpressionError::parse_error(format!(
            "Expected integer in '{expr}'"
        )));
    }

    let start = *pos;
    while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
        *pos += 1;
    }

    let num_str = &expr[start..*pos];
    let value: i64 = num_str.parse().map_err(|_| {
        ExpressionError::parse_error(format!("Invalid integer '{num_str}' in '{expr}'"))
    })?;

    Ok(if negative { -value } else { value })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_range() {
        let r = "1-5".parse::<RangeExpr>().unwrap();
        assert_eq!(r.to_vec(), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn stepped_range() {
        let r = "1-10:2".parse::<RangeExpr>().unwrap();
        assert_eq!(r.to_vec(), vec![1, 3, 5, 7, 9]);
    }

    #[test]
    fn multiple_ranges() {
        let r = "1-3,10-12".parse::<RangeExpr>().unwrap();
        assert_eq!(r.to_vec(), vec![1, 2, 3, 10, 11, 12]);
    }

    #[test]
    fn single_value() {
        let r = "42".parse::<RangeExpr>().unwrap();
        assert_eq!(r.to_vec(), vec![42]);
    }

    #[test]
    fn negative_range() {
        let r = "-3 - 2".parse::<RangeExpr>().unwrap();
        assert_eq!(r.to_vec(), vec![-3, -2, -1, 0, 1, 2]);
    }

    #[test]
    fn overlap_error() {
        assert!("1-5,3-7".parse::<RangeExpr>().is_err());
    }

    #[test]
    fn zero_step_error() {
        assert!("1-5:0".parse::<RangeExpr>().is_err());
    }

    #[test]
    fn empty_error() {
        assert!("".parse::<RangeExpr>().is_err());
    }

    #[test]
    fn descending_without_step_error() {
        assert!("5-1".parse::<RangeExpr>().is_err());
    }

    // ── slice() tests ──

    #[test]
    fn slice_basic() {
        let r = "1-10".parse::<RangeExpr>().unwrap();
        assert_eq!(r.slice(2, 5, 1).unwrap().to_vec(), vec![3, 4, 5]);
    }

    #[test]
    fn slice_from_start() {
        let r = "1-10".parse::<RangeExpr>().unwrap();
        assert_eq!(r.slice(0, 3, 1).unwrap().to_vec(), vec![1, 2, 3]);
    }

    #[test]
    fn slice_to_end() {
        let r = "1-5".parse::<RangeExpr>().unwrap();
        assert_eq!(r.slice(3, 5, 1).unwrap().to_vec(), vec![4, 5]);
    }

    #[test]
    fn slice_with_step() {
        let r = "1-10".parse::<RangeExpr>().unwrap();
        assert_eq!(r.slice(0, 10, 2).unwrap().to_vec(), vec![1, 3, 5, 7, 9]);
    }

    #[test]
    fn slice_reverse_returns_error() {
        let r = "1-5".parse::<RangeExpr>().unwrap();
        assert!(r.slice(4, -1, -1).is_err());
    }

    #[test]
    fn slice_empty_result() {
        let r = "1-5".parse::<RangeExpr>().unwrap();
        assert!(r.slice(5, 10, 1).unwrap().is_empty());
    }

    #[test]
    fn slice_stepped_source() {
        // Source: 1,3,5,7,9 (step 2). Slice [1:4] → elements at indices 1,2,3 → 3,5,7
        let r = "1-10:2".parse::<RangeExpr>().unwrap();
        assert_eq!(r.slice(1, 4, 1).unwrap().to_vec(), vec![3, 5, 7]);
    }

    #[test]
    fn slice_stepped_source_with_step() {
        // Source: 1,3,5,7,9. Slice [::2] → indices 0,2,4 → 1,5,9
        let r = "1-10:2".parse::<RangeExpr>().unwrap();
        assert_eq!(r.slice(0, 5, 2).unwrap().to_vec(), vec![1, 5, 9]);
    }

    #[test]
    fn slice_multi_range() {
        // Source: 1,2,3,10,11,12. Slice [1:5] → indices 1,2,3,4 → 2,3,10,11
        let r = "1-3,10-12".parse::<RangeExpr>().unwrap();
        assert_eq!(r.slice(1, 5, 1).unwrap().to_vec(), vec![2, 3, 10, 11]);
    }

    #[test]
    fn slice_multi_range_reverse_returns_error() {
        let r = "1-3,10-12".parse::<RangeExpr>().unwrap();
        assert!(r.slice(5, -1, -1).is_err());
    }

    #[test]
    fn slice_large_range_no_materialization() {
        // 1 billion elements — should complete instantly
        let r = RangeExpr::from_ranges(vec![IntRange {
            start: 1,
            end: 1_000_000_000,
            step: 1,
        }])
        .unwrap();
        assert_eq!(r.slice(0, 3, 1).unwrap().to_vec(), vec![1, 2, 3]);
    }

    #[test]
    fn slice_large_range_tail() {
        let r = RangeExpr::from_ranges(vec![IntRange {
            start: 1,
            end: 1_000_000_000,
            step: 1,
        }])
        .unwrap();
        let len = r.len() as i64;
        assert_eq!(
            r.slice(len - 3, len, 1).unwrap().to_vec(),
            vec![999_999_998, 999_999_999, 1_000_000_000]
        );
    }

    #[test]
    fn slice_large_range_with_step() {
        let r = RangeExpr::from_ranges(vec![IntRange {
            start: 1,
            end: 1_000_000_000,
            step: 1,
        }])
        .unwrap();
        // Every 100 millionth element, first 3
        assert_eq!(
            r.slice(0, 1_000_000_000, 100_000_000).unwrap().to_vec(),
            vec![
                1,
                100_000_001,
                200_000_001,
                300_000_001,
                400_000_001,
                500_000_001,
                600_000_001,
                700_000_001,
                800_000_001,
                900_000_001
            ]
        );
    }

    #[test]
    fn slice_zero_step_error() {
        let r = "1-5".parse::<RangeExpr>().unwrap();
        assert!(r.slice(0, 5, 0).is_err());
    }

    #[test]
    fn slice_negative_step_error() {
        let r = "1-5".parse::<RangeExpr>().unwrap();
        assert!(r.slice(4, -1, -1).is_err());
    }

    #[test]
    fn slice_single_element() {
        let r = "1-10".parse::<RangeExpr>().unwrap();
        assert_eq!(r.slice(3, 4, 1).unwrap().to_vec(), vec![4]);
    }

    // ── Defensive caps (SEC-2026-4) ──

    #[test]
    fn from_ranges_revalidates_inputs_as_defense_in_depth() {
        let err = RangeExpr::from_ranges(vec![IntRange {
            start: 0,
            end: 1,
            step: 0,
        }])
        .unwrap_err();

        assert_eq!(err.to_string(), "Range: step must not be zero");
    }

    #[test]
    fn reject_too_many_chunks_from_str() {
        // MAX_RANGE_EXPR_CHUNKS + 1 single values: 0,1,2,...
        let expr = (0..=MAX_RANGE_EXPR_CHUNKS as i64)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let err = expr.parse::<RangeExpr>().unwrap_err().to_string();
        assert!(err.contains("too many sub-ranges"), "got: {err}");
    }

    #[test]
    fn accept_max_chunks_from_str() {
        // Exactly MAX_RANGE_EXPR_CHUNKS single values: the squares
        // 0,1,4,9,... No three consecutive squares are equally spaced, so
        // the canonical form has one run per value.
        let expr = (0..MAX_RANGE_EXPR_CHUNKS as i64)
            .map(|i| (i * i).to_string())
            .collect::<Vec<_>>()
            .join(",");
        let r = expr.parse::<RangeExpr>().unwrap();
        assert_eq!(r.ranges().len(), MAX_RANGE_EXPR_CHUNKS);
    }

    #[test]
    fn reject_too_many_chunks_from_ranges() {
        let ranges: Vec<IntRange> = (0..=MAX_RANGE_EXPR_CHUNKS as i64)
            .map(|i| IntRange::new(i * 2, i * 2, 1).unwrap())
            .collect();
        let err = RangeExpr::from_ranges(ranges).unwrap_err().to_string();
        assert!(err.contains("too many sub-ranges"), "got: {err}");
    }

    #[test]
    fn accept_single_huge_chunk() {
        // A single chunk with a very large logical length is allowed —
        // `RangeExpr` stores chunks symbolically, so the heap cost is O(1)
        // regardless of `end - start`. Only chunk count is capped.
        let expr = "1-100000000000";
        let r = expr.parse::<RangeExpr>().unwrap();
        assert_eq!(r.ranges().len(), 1);
        assert_eq!(r.len(), 100_000_000_000);
    }

    #[test]
    fn length_sums_the_ranges() {
        // `from_ranges` sums per-range lengths with `checked_add`, rejecting
        // a total above MAX_RANGE_EXPR_LEN (tested by
        // `values_at_i64_bounds_accepted`). This test confirms the
        // happy-path total.
        let ranges = vec![
            IntRange::new(0, 1_000_000, 1).unwrap(),
            IntRange::new(2_000_000, 3_000_000, 1).unwrap(),
        ];
        let r = RangeExpr::from_ranges(ranges).unwrap();
        assert_eq!(r.ranges().len(), 2);
        assert_eq!(r.len(), 1_000_001 + 1_000_001);
    }

    #[test]
    fn chunk_cap_parse_does_not_hang() {
        // A pathological input with 100,000 comma-separated values. The parser
        // should reject it in well under a second without building the full
        // vector. This guards against regressions that would remove the
        // in-loop cap check.
        let start = std::time::Instant::now();
        let expr = (0..100_000i64)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let err = expr.parse::<RangeExpr>().unwrap_err().to_string();
        assert!(err.contains("too many sub-ranges"), "got: {err}");
        // Generous budget (2 seconds) to avoid flakes on loaded CI machines.
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "parser took too long on 100k-chunk input: {:?}",
            start.elapsed(),
        );
    }

    #[test]
    fn i64_min_rejected() {
        // i64::MIN is the one excluded value: as an endpoint, a step, or a
        // list value.
        let overflow = "Integer overflow: result is outside the 64-bit signed range";
        assert_eq!(
            IntRange::new(i64::MIN, 0, 1).unwrap_err().to_string(),
            overflow
        );
        assert_eq!(
            IntRange::new(0, 0, i64::MIN).unwrap_err().to_string(),
            overflow
        );
        // An end of i64::MIN, even one the step would not reach.
        assert_eq!(
            IntRange::new(0, i64::MIN, -2).unwrap_err().to_string(),
            overflow
        );
        assert_eq!(
            IntRange::new(i64::MIN + 2, i64::MIN, -2)
                .unwrap_err()
                .to_string(),
            overflow
        );
        assert_eq!(
            RangeExpr::from_values(vec![1, i64::MIN])
                .unwrap_err()
                .to_string(),
            overflow
        );
    }

    #[test]
    fn values_at_i64_bounds_accepted() {
        let max = i64::MAX;
        let range: RangeExpr = format!("{}-{}", max - 2, max).parse().unwrap();
        assert_eq!(range.to_vec(), vec![max - 2, max - 1, max]);
        let range: RangeExpr = format!("{}-{}", -max, -max + 2).parse().unwrap();
        assert_eq!(range.to_vec(), vec![-max, -max + 1, -max + 2]);
        // The largest count (i64::MAX values) is tested in the integration
        // test largest_count_parses_and_indexes, under a deadline.
    }

    #[test]
    fn int_range_arithmetic_beyond_i64_span() {
        // Span 2^64 - 2, step i64::MAX: three values.
        let max = i64::MAX;
        let r = IntRange::new(-max, max, max).unwrap();
        assert_eq!(r.len_u64(), 3);
        assert_eq!(r.iter().collect::<Vec<_>>(), vec![-max, 0, max]);
        assert!(r.contains(0));
        assert!(!r.contains(1));
        assert!(r.contains(max));
        // Descending normalization over the same span.
        let r = IntRange::new(max, -max, -max).unwrap();
        assert_eq!((r.start(), r.end(), r.step()), (-max, max, max));
        // A step that does not reach `end` exactly.
        let r = IntRange::new(-max, max, max - 1).unwrap();
        assert_eq!(r.iter().collect::<Vec<_>>(), vec![-max, -1, max - 2]);
    }

    #[test]
    fn max_magnitude_step_iteration_and_slice() {
        let max = i64::MAX;
        let range: RangeExpr = format!("{}-{}:{}", -max, max, max).parse().unwrap();
        assert_eq!(range.iter().collect::<Vec<_>>(), vec![-max, 0, max]);
        let sliced = range.slice(1, 3, 1).unwrap();
        assert_eq!(sliced.iter().collect::<Vec<_>>(), vec![0, max]);
    }

    #[test]
    fn slice_step_product_beyond_i64_uses_single_value_runs() {
        // The selected values' spacing (run step * slice step) exceeds
        // i64::MAX, so the two values are stored as two runs.
        let max = i64::MAX;
        let range: RangeExpr = format!("{}-{}:{}", -max, max, max).parse().unwrap();
        let sliced = range.slice(0, 3, 2).unwrap();
        assert_eq!(sliced.iter().collect::<Vec<_>>(), vec![-max, max]);
        assert_eq!(sliced.ranges().len(), 2);
        assert_eq!(sliced.to_string(), format!("{},{}", -max, max));
    }
}
