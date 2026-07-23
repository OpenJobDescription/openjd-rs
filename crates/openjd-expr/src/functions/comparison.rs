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
    // Membership by value equality, mirroring list containment (and
    // Python, where `1.0 in range(1, 4)` is True): integral floats can
    // match via the same exact int↔float equality rule the equality
    // operators use, and any non-integer item is simply not a member.
    Ok(match &a[1] {
        ExprValue::Int(i) => r.contains(*i),
        ExprValue::Float(f) => crate::value::float_as_exact_i64(f.value())
            .map(|i| r.contains(i))
            .unwrap_or(false),
        _ => false,
    })
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

fn collect_indices(start: i64, stop: i64, step: i64) -> Vec<usize> {
    let mut indices = Vec::new();
    let mut idx = start;
    if step > 0 {
        while idx < stop {
            if idx >= 0 {
                indices.push(idx as usize);
            }
            idx += step;
        }
    } else {
        while idx > stop {
            if idx >= 0 {
                indices.push(idx as usize);
            }
            idx += step;
        }
    }
    indices
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
    let result: Vec<ExprValue> = collect_indices(s, e, step)
        .into_iter()
        .filter_map(|i| a[0].list_get(i as i64))
        .collect();
    ctx.count_ops(result.len())?;
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
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len() as i64;
    let start = extract_int_or_none(&a[1]);
    let stop = extract_int_or_none(&a[2]);
    let (sv, ev) = compute_slice_indices(len, start, stop, step);
    let result: String = collect_indices(sv, ev, step)
        .into_iter()
        .filter(|&i| i < chars.len())
        .map(|i| chars[i])
        .collect();
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
}
