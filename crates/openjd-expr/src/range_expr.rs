// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Integer range expression parsing.
//!
//! Mirrors Python `openjd.expr._range_expr`. Parses expressions like
//! `"1-10"`, `"1-10:2"`, `"1-5,10-15"` into sorted, non-overlapping ranges.

use crate::error::ExpressionError;
use std::fmt;

/// Maximum number of comma-separated sub-ranges in a single range expression.
///
/// Each sub-range becomes one `IntRange` entry in the parsed `RangeExpr`.
/// Real-world range expressions (frame ranges, task chunks) contain at most
/// a few dozen sub-ranges; 10,000 is two orders of magnitude above any
/// plausible legitimate use. Rejecting larger inputs at parse time prevents
/// an attacker from forcing a multi-megabyte `Vec<IntRange>` allocation
/// through a parameter value before any downstream resource-bounding
/// (e.g. the evaluator's memory limit) applies.
///
/// This cap targets the source-text and heap dimensions of a `RangeExpr`.
/// It does **not** cap the logical element count of a single chunk —
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
pub struct RangeExprError {
    pub expr: String,
    pub message: String,
    pub position: Option<usize>,
}

impl RangeExprError {
    pub fn new(expr: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            expr: expr.into(),
            message: message.into(),
            position: None,
        }
    }
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

/// A single contiguous range of integers with a step.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct IntRange {
    pub start: i64,
    pub end: i64,
    pub step: i64,
}

impl IntRange {
    /// Create a range, normalizing descending ranges to ascending form.
    /// After construction, `start <= end` and `step > 0` always hold.
    pub fn new(start: i64, end: i64, step: i64) -> Result<Self, ExpressionError> {
        if step == 0 {
            return Err(ExpressionError::parse_error("Range: step must not be zero"));
        }
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
        if step < 0 {
            // Normalize descending to ascending form (matching Python _IntRange)
            let count = ((start - end) / (-step)) + 1;
            let last = start + (count - 1) * step; // smallest value
            Ok(Self {
                start: last,
                end: start,
                step: -step,
            })
        } else {
            // Normalize end to actual last value in the range
            let count = (end - start) / step + 1;
            let actual_end = start + (count - 1) * step;
            Ok(Self {
                start,
                end: actual_end,
                step,
            })
        }
    }

    /// Number of integers in this range.
    pub fn len(&self) -> usize {
        // After normalization, start <= end and step > 0 always
        ((self.end - self.start) / self.step + 1) as usize
    }

    /// Returns `true` if the range contains no elements.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Test whether `value` is a member of this range.
    pub fn contains(&self, value: i64) -> bool {
        if value < self.start || value > self.end {
            return false;
        }
        (value - self.start) % self.step == 0
    }

    /// Iterate over all values in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = i64> + '_ {
        (0..self.len() as i64).map(move |i| self.start + i * self.step)
    }

    /// Get element by zero-based index, or `None` if out of bounds.
    pub fn get(&self, index: usize) -> Option<i64> {
        if index < self.len() {
            Some(self.start + index as i64 * self.step)
        } else {
            None
        }
    }
}

impl std::fmt::Display for IntRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self.len();
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

/// A range expression: a sorted set of non-overlapping integer ranges.
#[derive(Debug, Clone, Eq, serde::Serialize)]
pub struct RangeExpr {
    ranges: Vec<IntRange>,
    /// Cumulative length indices for O(log n) getitem. Entry i = total elements in ranges[0..=i].
    cumulative_lengths: Vec<usize>,
    /// Packed: lower 63 bits = length, MSB = contiguous display flag.
    /// Contiguous flag only affects Display; not preserved through constructors.
    #[serde(serialize_with = "serialize_length")]
    length: usize,
}

const CONTIGUOUS_BIT: usize = 1 << (usize::BITS - 1);
const LENGTH_MASK: usize = !CONTIGUOUS_BIT;

fn serialize_length<S: serde::Serializer>(length: &usize, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u64((length & LENGTH_MASK) as u64)
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

impl PartialEq for RangeExpr {
    fn eq(&self, other: &Self) -> bool {
        self.ranges == other.ranges
    }
}

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

impl RangeExpr {
    /// Set contiguous display mode. When true, Display uses "{start}-{end}" format
    /// even for single values (e.g., "5-5"). Only meaningful for contiguous chunks.
    #[must_use]
    pub fn with_contiguous(mut self, contiguous: bool) -> Self {
        if contiguous {
            self.length |= CONTIGUOUS_BIT;
        } else {
            self.length &= LENGTH_MASK;
        }
        self
    }

    /// Create a RangeExpr from a list of individual values.
    pub fn from_values(mut values: Vec<i64>) -> Self {
        if values.is_empty() {
            return Self {
                ranges: Vec::new(),
                cumulative_lengths: Vec::new(),
                length: 0,
            };
        }
        // Sort and deduplicate (matching Python from_list)
        values.sort();
        values.dedup();
        let length = values.len();
        let mut ranges = Vec::new();
        let mut i = 0;
        while i < values.len() {
            let start = values[i];
            // Detect step: check if next values form an arithmetic sequence
            if i + 1 < values.len() {
                let step = values[i + 1] - values[i];
                if step != 0 {
                    let mut j = i + 1;
                    while j < values.len() && values[j] == start + (j - i) as i64 * step {
                        j += 1;
                    }
                    if j > i + 1 {
                        let end = values[j - 1];
                        ranges.push(IntRange { start, end, step });
                        i = j;
                        continue;
                    }
                }
            }
            ranges.push(IntRange {
                start,
                end: start,
                step: 1,
            });
            i += 1;
        }
        let cumulative_lengths = build_cumulative(&ranges);
        Self {
            ranges,
            cumulative_lengths,
            length,
        }
    }

    /// Create from pre-built `IntRange`s. Sorts, merges adjacent ranges, and validates no overlaps.
    ///
    /// Returns an error if the number of input ranges exceeds
    /// [`MAX_RANGE_EXPR_CHUNKS`].
    pub fn from_ranges(mut ranges: Vec<IntRange>) -> Result<Self, ExpressionError> {
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
        // Sort by start
        ranges.sort_by_key(|r| (r.start, r.end));
        // Merge adjacent ranges with same step
        let mut merged = vec![ranges[0].clone()];
        for r in &ranges[1..] {
            let last = merged.last().unwrap();
            if last.step == r.step && last.end + r.step == r.start {
                let new_end = r.end;
                let last_start = last.start;
                let step = last.step;
                *merged.last_mut().unwrap() = IntRange {
                    start: last_start,
                    end: new_end,
                    step,
                };
            } else {
                merged.push(r.clone());
            }
        }
        // Validate no overlaps
        for i in 1..merged.len() {
            if merged[i].start <= merged[i - 1].end {
                return Err(ExpressionError::parse_error(format!(
                    "Range expression has overlapping ranges: {} and {}",
                    merged[i - 1],
                    merged[i]
                )));
            }
        }
        // Use saturating arithmetic when summing per-chunk lengths so that a
        // multi-chunk range whose combined logical length exceeds `usize`
        // capacity saturates at `usize::MAX` rather than wrapping to a small
        // value that would corrupt `len()`/`is_empty()`. `RangeExpr` stores
        // chunks symbolically, so an enormous logical length is not itself a
        // DoS vector — only the chunk count is capped via
        // [`MAX_RANGE_EXPR_CHUNKS`].
        let length: usize = merged
            .iter()
            .fold(0usize, |acc, r| acc.saturating_add(r.len()));
        let cumulative_lengths = build_cumulative(&merged);
        Ok(Self {
            ranges: merged,
            cumulative_lengths,
            length,
        })
    }

    /// Total number of integers across all sub-ranges.
    pub fn len(&self) -> usize {
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

    /// Get element by index (supports negative indexing like Python).
    pub fn get(&self, index: i64) -> Option<i64> {
        let len = self.length & LENGTH_MASK;
        let idx = if index < 0 { len as i64 + index } else { index } as usize;
        if idx >= len {
            return None;
        }
        // Binary search on cumulative lengths (mirrors Python's bisect on _range_length_indices)
        let range_idx = self.cumulative_lengths.partition_point(|&cum| cum <= idx);
        let offset = if range_idx == 0 {
            idx
        } else {
            idx - self.cumulative_lengths[range_idx - 1]
        };
        self.ranges[range_idx].get(offset)
    }

    /// The underlying sub-ranges (sorted, non-overlapping).
    pub fn ranges(&self) -> &[IntRange] {
        &self.ranges
    }

    /// Cumulative element counts per sub-range (for index mapping).
    pub fn cumulative_lengths(&self) -> &[usize] {
        &self.cumulative_lengths
    }

    /// Iterate over all values in ascending order across all sub-ranges.
    pub fn iter(&self) -> impl Iterator<Item = i64> + '_ {
        self.ranges.iter().flat_map(|r| r.iter())
    }

    pub fn to_vec(&self) -> Vec<i64> {
        self.iter().collect()
    }

    /// Slice this range expression with a positive step.
    ///
    /// `start`, `stop`, `step` are already-normalized indices into the
    /// flattened element sequence. `step` must be positive. Returns a new
    /// `RangeExpr` without materializing any elements.
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
        let total_len = self.len() as i64;
        // Clamp to valid range
        let start = start.max(0).min(total_len);
        let stop = stop.max(0).min(total_len);
        if start >= stop {
            return Ok(RangeExpr {
                ranges: Vec::new(),
                cumulative_lengths: Vec::new(),
                length: 0,
            });
        }

        let mut result_ranges = Vec::new();
        let mut cum_start: i64 = 0; // global index where current sub-range begins

        for r in &self.ranges {
            let r_len = r.len() as i64;
            let cum_end = cum_start + r_len; // exclusive end of this sub-range's global indices

            // The slice selects global indices: start, start+step, start+2*step, ...
            // Find the first slice index >= cum_start and the last < cum_end,
            // intersected with [start, stop).

            // First slice index that falls within this sub-range:
            // We need the smallest k such that start + k*step >= cum_start and start + k*step < cum_end
            let first_global = if start >= cum_start {
                start
            } else {
                // First multiple of step at or after cum_start
                let offset = cum_start - start;
                let k = (offset + step - 1) / step; // ceil division
                start + k * step
            };

            // Must also be < stop and < cum_end
            let range_stop = stop.min(cum_end);
            if first_global >= range_stop {
                cum_start = cum_end;
                continue;
            }

            // Verify it's aligned to the slice stride
            debug_assert!((first_global - start) % step == 0);

            // Local offset within this IntRange
            let first_local = (first_global - cum_start) as usize;
            // How many slice-selected indices fall in this sub-range?
            let count = (range_stop - first_global - 1) / step + 1;
            let last_local = first_local + (count as usize - 1) * step as usize;

            // Map local indices to values: value = r.start + local * r.step
            let new_start = r.start + first_local as i64 * r.step;
            let new_end = r.start + last_local as i64 * r.step;
            let new_step = r.step * step;

            if count == 1 {
                result_ranges.push(IntRange {
                    start: new_start,
                    end: new_start,
                    step: 1,
                });
            } else {
                result_ranges.push(IntRange {
                    start: new_start,
                    end: new_end,
                    step: new_step,
                });
            }

            cum_start = cum_end;
        }

        if result_ranges.is_empty() {
            return Ok(RangeExpr {
                ranges: Vec::new(),
                cumulative_lengths: Vec::new(),
                length: 0,
            });
        }
        let length = result_ranges.iter().map(|r| r.len()).sum();
        let cumulative_lengths = build_cumulative(&result_ranges);
        Ok(RangeExpr {
            ranges: result_ranges,
            cumulative_lengths,
            length,
        })
    }

    /// Heap allocation size (for memory tracking).
    pub fn heap_size(&self) -> usize {
        use std::mem::size_of;
        self.ranges.capacity() * size_of::<IntRange>()
            + self.cumulative_lengths.capacity() * size_of::<usize>()
    }
}

fn build_cumulative(ranges: &[IntRange]) -> Vec<usize> {
    let mut cum = Vec::with_capacity(ranges.len());
    let mut total = 0;
    for r in ranges {
        total += r.len();
        cum.push(total);
    }
    cum
}

impl std::fmt::Display for RangeExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.length & CONTIGUOUS_BIT != 0 {
            // Contiguous chunk format: always "{start}-{end}", even for single values
            if self.ranges.len() == 1 && self.ranges[0].step == 1 {
                return write!(f, "{}-{}", self.ranges[0].start, self.ranges[0].end);
            }
            let len = self.length & LENGTH_MASK;
            if len == 1 {
                let val = self.ranges[0].start;
                return write!(f, "{val}-{val}");
            }
            // Multiple ranges: fall through to normal display
        }
        let parts: Vec<String> = self.ranges.iter().map(|r| r.to_string()).collect();
        write!(f, "{}", parts.join(","))
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
        // Exactly MAX_RANGE_EXPR_CHUNKS non-contiguous single values:
        // 0,2,4,...,(2*N-2). Stride of 2 prevents adjacent-range merging,
        // so we end up with exactly N sub-ranges after from_ranges.
        let expr = (0..MAX_RANGE_EXPR_CHUNKS as i64)
            .map(|i| (i * 2).to_string())
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
    fn length_uses_saturating_sum() {
        // `from_ranges` uses `saturating_add` when summing per-chunk lengths,
        // so a multi-chunk range whose combined logical length would exceed
        // `usize::MAX` cannot wrap to a small value that would corrupt
        // `len()` / `is_empty()`. This test confirms the happy-path summation
        // matches the expected total; the saturating behavior is exercised
        // only in the arithmetic failure mode and is a defensive guard.
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
}
