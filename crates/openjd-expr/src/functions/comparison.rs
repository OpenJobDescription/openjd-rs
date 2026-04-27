// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Comparison, containment, and slice operator implementations.

use crate::error::ExpressionError;
use crate::function_library::EvalContext;
use crate::value::ExprValue;

type R = Result<ExprValue, ExpressionError>;
type Ctx<'a> = &'a mut dyn EvalContext;

// ── Equality ──

pub fn eq_generic(_: Ctx, a: &[ExprValue]) -> R {
    Ok(ExprValue::Bool(a[0].equals(&a[1])))
}

pub fn ne_generic(_: Ctx, a: &[ExprValue]) -> R {
    Ok(ExprValue::Bool(!a[0].equals(&a[1])))
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

pub fn contains_list(ctx: Ctx, a: &[ExprValue]) -> R {
    let len = a[0].list_len().unwrap_or(0);
    ctx.count_ops(len)?;
    let found = a[0]
        .list_iter()
        .map(|mut iter| iter.any(|e| a[1].equals(&e)))
        .unwrap_or(false);
    Ok(ExprValue::Bool(found))
}

pub fn not_contains_list(ctx: Ctx, a: &[ExprValue]) -> R {
    let len = a[0].list_len().unwrap_or(0);
    ctx.count_ops(len)?;
    let found = a[0]
        .list_iter()
        .map(|mut iter| iter.any(|e| a[1].equals(&e)))
        .unwrap_or(false);
    Ok(ExprValue::Bool(!found))
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

pub fn contains_range(_: Ctx, a: &[ExprValue]) -> R {
    match (&a[0], &a[1]) {
        (ExprValue::RangeExpr(r), ExprValue::Int(i)) => Ok(ExprValue::Bool(r.contains(*i))),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn not_contains_range(_: Ctx, a: &[ExprValue]) -> R {
    match (&a[0], &a[1]) {
        (ExprValue::RangeExpr(r), ExprValue::Int(i)) => Ok(ExprValue::Bool(!r.contains(*i))),
        _ => Err(ExpressionError::type_error("type error")),
    }
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
        let e = stop.map(|i| if i < 0 { len + i } else { i }).unwrap_or(-1);
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
    ExprValue::make_list(result, elem_type.clone())
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
    let len = r.len() as i64;
    let start = extract_int_or_none(&a[1]);
    let stop = extract_int_or_none(&a[2]);
    let (s, e) = compute_slice_indices(len, start, stop, step);
    if step > 0 {
        // Forward slice → return RangeExpr
        Ok(ExprValue::RangeExpr(r.slice(s, e, step)?))
    } else {
        // Reverse slice → return list (RangeExpr can't represent descending order)
        let result: Vec<ExprValue> = collect_indices(s, e, step)
            .into_iter()
            .filter_map(|i| r.get(i as i64).map(ExprValue::Int))
            .collect();
        ctx.count_ops(result.len())?;
        Ok(ExprValue::make_list(result, crate::types::ExprType::INT)?)
    }
}
