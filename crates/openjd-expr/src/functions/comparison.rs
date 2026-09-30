// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Comparison, containment, and slice operator implementations.

use crate::budgeted_vec::BudgetedVec;
use crate::error::ExpressionError;
use crate::function_library::EvalContext;
use crate::value::ExprValue;

type R = Result<ExprValue, ExpressionError>;
type Ctx<'a> = &'a mut dyn EvalContext;

// ── Equality ──

/// Budget-aware value equality shared by `==`, `!=`, and list containment.
///
/// Delegates to [`ExprValue::equals_charged`] — the single definition of
/// value equality — passing the operation budget as the charge callback.
/// Comparisons decidable by length alone (including a materialized list
/// against a huge symbolic range) are decided in O(1) with no charge;
/// element comparisons actually performed are charged as they happen.
fn values_equal(ctx: Ctx, left: &ExprValue, right: &ExprValue) -> Result<bool, ExpressionError> {
    left.equals_charged(right, &mut |n| ctx.count_ops(n))
}

pub fn eq_generic(ctx: Ctx, a: &[ExprValue]) -> R {
    Ok(ExprValue::Bool(values_equal(ctx, &a[0], &a[1])?))
}

pub fn ne_generic(ctx: Ctx, a: &[ExprValue]) -> R {
    Ok(ExprValue::Bool(!values_equal(ctx, &a[0], &a[1])?))
}

// ── Ordering ──

fn do_compare(op_str: &str, a: &[ExprValue]) -> Result<std::cmp::Ordering, ExpressionError> {
    a[0].compare(&a[1]).map_err(|_| {
        ExpressionError::type_error(format!(
            "Cannot use '{}' operator with {} and {}",
            op_str,
            a[0].expr_type(),
            a[1].expr_type()
        ))
    })
}

pub fn lt_generic(_: Ctx, a: &[ExprValue]) -> R {
    Ok(ExprValue::Bool(do_compare("<", a)?.is_lt()))
}

pub fn le_generic(_: Ctx, a: &[ExprValue]) -> R {
    Ok(ExprValue::Bool(do_compare("<=", a)?.is_le()))
}

pub fn gt_generic(_: Ctx, a: &[ExprValue]) -> R {
    Ok(ExprValue::Bool(do_compare(">", a)?.is_gt()))
}

pub fn ge_generic(_: Ctx, a: &[ExprValue]) -> R {
    Ok(ExprValue::Bool(do_compare(">=", a)?.is_ge()))
}

// ── Containment ──

fn list_contains(ctx: Ctx, a: &[ExprValue]) -> Result<bool, ExpressionError> {
    let len = a[0].list_len().unwrap_or(0);
    ctx.count_ops(len)?;
    if let Some(iter) = a[0].list_iter() {
        for element in iter {
            if values_equal(ctx, &a[1], &element)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn contains_list(ctx: Ctx, a: &[ExprValue]) -> R {
    Ok(ExprValue::Bool(list_contains(ctx, a)?))
}

pub fn not_contains_list(ctx: Ctx, a: &[ExprValue]) -> R {
    Ok(ExprValue::Bool(!list_contains(ctx, a)?))
}

pub fn contains_string(ctx: Ctx, a: &[ExprValue]) -> R {
    match (&a[0], &a[1]) {
        (ExprValue::String(haystack), ExprValue::String(needle)) => {
            ctx.count_string_ops(haystack.len() + needle.len())?;
            Ok(ExprValue::Bool(haystack.contains(needle.as_str())))
        }
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn not_contains_string(ctx: Ctx, a: &[ExprValue]) -> R {
    match (&a[0], &a[1]) {
        (ExprValue::String(haystack), ExprValue::String(needle)) => {
            ctx.count_string_ops(haystack.len() + needle.len())?;
            Ok(ExprValue::Bool(!haystack.contains(needle.as_str())))
        }
        _ => Err(ExpressionError::type_error("type error")),
    }
}

fn range_contains(a: &[ExprValue]) -> Result<bool, ExpressionError> {
    let r = match &a[0] {
        ExprValue::RangeExpr(r) => r,
        _ => {
            return Err(ExpressionError::type_error(
                "__contains__ requires a range_expr container",
            ))
        }
    };
    // The signature is `(range_expr, int | float)`, so dispatch has already
    // refused every other item type; the error arm guards a direct call.
    // A float item is a member when it is exactly an integer in the range,
    // the same exact int↔float rule the equality operators use (and
    // Python's `1.0 in range(1, 4)`), so `1.0 in range_expr('1-3')` agrees
    // with `1.0 in [1, 2, 3]`.
    match &a[1] {
        ExprValue::Int(i) => Ok(r.contains(*i)),
        ExprValue::Float(f) => Ok(crate::value::float_as_exact_i64(f.value())
            .map(|i| r.contains(i))
            .unwrap_or(false)),
        _ => Err(ExpressionError::type_error(
            "__contains__ on a range_expr requires an int or float item",
        )),
    }
}
pub fn contains_range(_: Ctx, a: &[ExprValue]) -> R {
    Ok(ExprValue::Bool(range_contains(a)?))
}

pub fn not_contains_range(_: Ctx, a: &[ExprValue]) -> R {
    Ok(ExprValue::Bool(!range_contains(a)?))
}

// ── Slicing (4-arg __getitem__) ──

fn extract_int_or_none(v: &ExprValue) -> Option<i64> {
    match v {
        ExprValue::Int(i) => Some(*i),
        _ => None, // Null → None
    }
}

fn compute_slice_indices(len: i64, start: Option<i64>, stop: Option<i64>, step: i64) -> (i64, i64) {
    if step > 0 {
        let s = start
            .map(|i| if i < 0 { (len + i).max(0) } else { i.min(len) })
            .unwrap_or(0);
        let e = stop
            .map(|i| if i < 0 { (len + i).max(0) } else { i.min(len) })
            .unwrap_or(len);
        (s, e)
    } else {
        let s = start
            .map(|i| if i < 0 { len + i } else { i.min(len - 1) })
            .unwrap_or(len - 1);
        // Clamp the resolved stop to the -1 sentinel ("walk to index 0",
        // matching CPython's PySlice_AdjustIndices): a stop far below
        // -len would otherwise leave the reverse walk spinning through
        // ~i64::MAX below-zero indices that are never charged.
        let e = stop
            .map(|i| if i < 0 { (len + i).max(-1) } else { i })
            .unwrap_or(-1);
        (s, e)
    }
}

/// Indices a slice visits, in order. The step is added with saturation so
/// a step near `i64::MAX` or `i64::MIN` cannot overflow; a saturated index
/// is past every bound `compute_slice_indices` can return, so the loop
/// ends.
fn collect_indices(start: i64, stop: i64, step: i64) -> Vec<usize> {
    let mut indices = Vec::new();
    let mut idx = start;
    if step > 0 {
        while idx < stop {
            if idx >= 0 {
                indices.push(idx as usize);
            }
            idx = idx.saturating_add(step);
        }
    } else {
        while idx > stop {
            if idx >= 0 {
                indices.push(idx as usize);
            }
            idx = idx.saturating_add(step);
        }
    }
    indices
}

/// Number of indices [`collect_indices`] yields for the same arguments.
/// Computed arithmetically so callers can check the memory budget before
/// building the result. Expects `(start, stop)` from
/// [`compute_slice_indices`]: for a forward step `start >= 0`, and for a
/// backward step `stop >= -1`, so a negative `start` yields nothing (as
/// `collect_indices`'s `idx >= 0` filter would).
fn slice_len(start: i64, stop: i64, step: i64) -> usize {
    let span = if step > 0 {
        stop.saturating_sub(start)
    } else {
        start.saturating_sub(stop)
    };
    if span <= 0 {
        return 0;
    }
    // ceil(span / |step|), in u64 so `|i64::MIN|` cannot overflow.
    ((span as u64 - 1) / step.unsigned_abs() + 1) as usize
}

pub fn slice_list(ctx: Ctx, a: &[ExprValue]) -> R {
    let step = extract_int_or_none(&a[3]).unwrap_or(1);
    if step == 0 {
        return Err(ExpressionError::new("Slice step cannot be zero"));
    }
    let elem_type = a[0].list_elem_type().unwrap();
    let len = a[0].list_len().unwrap() as i64;
    let start = extract_int_or_none(&a[1]);
    let stop = extract_int_or_none(&a[2]);
    let (s, e) = compute_slice_indices(len, start, stop, step);
    // Check the result's slot count before allocating the index vector
    // or the element vector. `make_list_checked` checks again with the
    // elements' heap sizes once they are known.
    let count = slice_len(s, e, step);
    ctx.count_ops(count)?;
    ctx.check_memory(count.saturating_mul(std::mem::size_of::<ExprValue>()))?;
    let result: Vec<ExprValue> = collect_indices(s, e, step)
        .into_iter()
        .filter_map(|i| a[0].list_get(i as i64))
        .collect();
    ExprValue::make_list_checked(ctx, result, elem_type.clone())
}

pub fn slice_string(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = match &a[0] {
        ExprValue::String(s) => s.as_str(),
        _ => return Err(ExpressionError::type_error("type error")),
    };
    ctx.count_string_ops(s.len())?;
    let step = extract_int_or_none(&a[3]).unwrap_or(1);
    if step == 0 {
        return Err(ExpressionError::new("Slice step cannot be zero"));
    }
    let len = s.chars().count() as i64;
    let start = extract_int_or_none(&a[1]);
    let stop = extract_int_or_none(&a[2]);
    let (sv, ev) = compute_slice_indices(len, start, stop, step);
    let count = slice_len(sv, ev, step);
    if count == 0 {
        return Ok(ExprValue::String(String::new()));
    }
    // A non-zero step never visits an index twice, so each selected
    // character is a distinct character of `s`. The result is therefore
    // at most `s.len()` bytes and at most 4 bytes per selected character.
    // Check that bound before allocating, then copy the characters
    // directly from `s` (no index vector, no `Vec<char>`) and shrink the
    // buffer so the tracked size equals the actual size.
    let max_bytes = s.len().min(count.saturating_mul(4));
    ctx.check_memory(max_bytes)?;
    let mut result = String::with_capacity(max_bytes);
    let stride = step.unsigned_abs() as usize;
    if step > 0 {
        result.extend(s.chars().skip(sv as usize).step_by(stride).take(count));
    } else {
        // Walking from the end, the k-th character has index `len - 1 - k`.
        // `count > 0` guarantees `0 <= sv < len`.
        result.extend(
            s.chars()
                .rev()
                .skip((len - 1 - sv) as usize)
                .step_by(stride)
                .take(count),
        );
    }
    result.shrink_to_fit();
    Ok(ExprValue::String(result))
}

pub fn slice_range(ctx: Ctx, a: &[ExprValue]) -> R {
    let r = match &a[0] {
        ExprValue::RangeExpr(r) => r,
        _ => return Err(ExpressionError::type_error("type error")),
    };
    let step = extract_int_or_none(&a[3]).unwrap_or(1);
    if step == 0 {
        return Err(ExpressionError::new("Slice step cannot be zero"));
    }
    // Range lengths are exact and below 2^63 (values are bounded to
    // |v| < 2^62 at construction), so index resolution fits i64. The
    // `len + i` sums for negative indices stay in i64 because language
    // ints are i64 and len < 2^63.
    let len = r.len_u64() as i64;
    let start = extract_int_or_none(&a[1]);
    let stop = extract_int_or_none(&a[2]);
    if step > 0 {
        let s = start
            .map(|i| if i < 0 { (len + i).max(0) } else { i.min(len) })
            .unwrap_or(0);
        let e = stop
            .map(|i| if i < 0 { (len + i).max(0) } else { i.min(len) })
            .unwrap_or(len);
        // Forward slice → return RangeExpr
        Ok(ExprValue::RangeExpr(r.slice(s, e, step)?))
    } else {
        let s = start
            .map(|i| {
                if i < 0 {
                    (len + i).max(-1)
                } else {
                    i.min(len - 1)
                }
            })
            .unwrap_or(len - 1);
        // Clamp the resolved stop to the -1 sentinel ("walk to index 0",
        // matching CPython's PySlice_AdjustIndices): a stop far below
        // -len would otherwise leave the reverse walk spinning through
        // ~i64::MAX below-zero indices that are never charged — an
        // unbudgeted hang.
        let e = stop
            .map(|i| if i < 0 { (len + i).max(-1) } else { i })
            .unwrap_or(-1);
        // Reverse slice → return list (RangeExpr can't represent
        // descending order). Pre-check the result's memory against the
        // limit before building — op charges alone admit up to the
        // operation limit in elements (~640 MB of ExprValues under
        // default limits) before make_list_checked's post-hoc check —
        // then charge per element as we walk.
        // s, e are clamped to [-1, len-1] and step < 0, so all walk
        // arithmetic stays in i64. `-step` is safe: an i64::MIN step
        // would negate-overflow, so clamp it first — steps of magnitude
        // >= len visit at most one element anyway.
        let step = step.max(-(len.max(1)));
        let result_len = if s > e {
            // Walk visits s, s-|step|, ... down to (exclusive) e.
            usize::try_from((s - e - 1) / (-step) + 1).unwrap_or(usize::MAX)
        } else {
            0
        };
        let mut result = BudgetedVec::with_capacity(ctx, result_len)?;
        let mut idx = s;
        while idx > e {
            if idx >= 0 {
                ctx.count_op()?;
                if let Some(v) = r.value_at(idx as u64) {
                    result.push(ctx, ExprValue::Int(v))?;
                }
            }
            idx += step;
        }
        Ok(ExprValue::make_list_checked(
            ctx,
            result.into_vec(),
            crate::types::ExprType::INT,
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path_mapping::PathFormat;

    struct TestContext;

    impl EvalContext for TestContext {
        fn path_format(&self) -> PathFormat {
            PathFormat::host()
        }

        fn count_op(&mut self) -> Result<(), ExpressionError> {
            Ok(())
        }

        fn count_ops(&mut self, _n: usize) -> Result<(), ExpressionError> {
            Ok(())
        }

        fn count_string_ops(&mut self, _len: usize) -> Result<(), ExpressionError> {
            Ok(())
        }

        fn check_memory(&self, _bytes: usize) -> Result<(), ExpressionError> {
            Ok(())
        }
    }

    #[test]
    fn range_contains_direct_call_rejects_non_range_container() {
        let err =
            contains_range(&mut TestContext, &[ExprValue::Int(1), ExprValue::Int(1)]).unwrap_err();

        assert_eq!(
            err.to_string(),
            "__contains__ requires a range_expr container"
        );
    }
    #[test]
    fn range_contains_direct_call_rejects_non_int_item() {
        let r = ExprValue::RangeExpr("1-5".parse().unwrap());
        let err =
            contains_range(&mut TestContext, &[r, ExprValue::String("3".into())]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "__contains__ on a range_expr requires an int or float item"
        );
    }

    /// `slice_len` must agree with `collect_indices` for every argument
    /// combination `compute_slice_indices` can produce, since it is used
    /// to check the budget for the result `collect_indices` then builds.
    #[test]
    fn slice_len_matches_collect_indices() {
        let bounds: Vec<Option<i64>> = std::iter::once(None)
            .chain((-9..=9).map(Some))
            .chain([i64::MIN, i64::MAX, -1_000_000, 1_000_000].map(Some))
            .collect();
        let steps = [1, 2, 3, 7, -1, -2, -3, -7, i64::MAX, i64::MIN];
        for len in 0..=7 {
            for &start in &bounds {
                for &stop in &bounds {
                    for &step in &steps {
                        let (s, e) = compute_slice_indices(len, start, stop, step);
                        assert_eq!(
                            slice_len(s, e, step),
                            collect_indices(s, e, step).len(),
                            "len={len} start={start:?} stop={stop:?} step={step}"
                        );
                    }
                }
            }
        }
    }

    /// `slice_string` must select the same characters as indexing into a
    /// collected `Vec<char>`, including multi-byte characters and
    /// backward steps.
    #[test]
    fn slice_string_matches_char_indexing() {
        let text = "aé漢😀bçdz";
        let chars: Vec<char> = text.chars().collect();
        let len = chars.len() as i64;
        let bounds: Vec<Option<i64>> = std::iter::once(None)
            .chain((-(len + 2)..=(len + 2)).map(Some))
            .collect();
        for &start in &bounds {
            for &stop in &bounds {
                for step in [1, 2, 3, -1, -2, -3] {
                    let (s, e) = compute_slice_indices(len, start, stop, step);
                    let expected: String = collect_indices(s, e, step)
                        .into_iter()
                        .filter(|&i| i < chars.len())
                        .map(|i| chars[i])
                        .collect();
                    let to_val = |b: Option<i64>| b.map_or(ExprValue::Null, ExprValue::Int);
                    let got = slice_string(
                        &mut TestContext,
                        &[
                            ExprValue::String(text.to_string()),
                            to_val(start),
                            to_val(stop),
                            ExprValue::Int(step),
                        ],
                    )
                    .unwrap();
                    let ExprValue::String(got) = got else {
                        panic!("slice_string returned a non-string");
                    };
                    assert_eq!(got, expected, "start={start:?} stop={stop:?} step={step}");
                    // The buffer is shrunk, so capacity equals length.
                    assert_eq!(got.capacity(), got.len());
                }
            }
        }
    }
}
