// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! List function implementations (sorted, reversed, unique, flatten, range).

use crate::error::ExpressionError;
use crate::function_library::EvalContext;
use crate::types::ExprType;
use crate::value::ExprValue;

type R = Result<ExprValue, ExpressionError>;
type Ctx<'a> = &'a mut dyn EvalContext;

pub fn sorted_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (elements, elem_type) = a[0].clone().into_list().ok_or_else(|| ExpressionError::new("sorted() argument must be a list"))?;
    ctx.count_ops(elements.len())?;
    let mut sorted = elements;
    sorted.sort_by(|a, b| a.compare(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(ExprValue::make_list(sorted, elem_type)?)
}

pub fn reversed_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (mut elements, elem_type) = a[0].clone().into_list().ok_or_else(|| ExpressionError::new("reversed() argument must be a list"))?;
    ctx.count_ops(elements.len())?;
    elements.reverse();
    Ok(ExprValue::make_list(elements, elem_type)?)
}

pub fn unique_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (elements, elem_type) = a[0].clone().into_list().ok_or_else(|| ExpressionError::new("unique() argument must be a list"))?;
    ctx.count_ops(elements.len())?;
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for e in elements {
        if seen.insert(e.clone()) { result.push(e); }
    }
    Ok(ExprValue::make_list(result, elem_type)?)
}

pub fn flatten_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let iter = a[0].list_iter().ok_or_else(|| ExpressionError::new("flatten() argument must be a list"))?;
    let mut result = Vec::new();
    for e in iter {
        ctx.count_op()?;
        if e.is_list() {
            let inner_len = e.list_len().unwrap_or(0);
            ctx.count_ops(inner_len)?;
            result.extend(e.list_iter().expect("is_list() was true"));
        } else { result.push(e); }
    }
    let et = if result.is_empty() { ExprType::INT } else { result[0].expr_type() };
    Ok(ExprValue::make_list(result, et)?)
}

pub fn range_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (start, stop, step) = match a.len() {
        1 => (0i64, match &a[0] { ExprValue::Int(n) => *n, _ => return Err(ExpressionError::new("range() argument must be int")) }, 1i64),
        2 => (match &a[0] { ExprValue::Int(n) => *n, _ => return Err(ExpressionError::new("range() arguments must be int")) },
              match &a[1] { ExprValue::Int(n) => *n, _ => return Err(ExpressionError::new("range() arguments must be int")) }, 1),
        3 => (match &a[0] { ExprValue::Int(n) => *n, _ => return Err(ExpressionError::new("range() arguments must be int")) },
              match &a[1] { ExprValue::Int(n) => *n, _ => return Err(ExpressionError::new("range() arguments must be int")) },
              match &a[2] { ExprValue::Int(n) => *n, _ => return Err(ExpressionError::new("range() arguments must be int")) }),
        _ => return Err(ExpressionError::new("range() takes 1-3 arguments")),
    };
    if step == 0 { return Err(ExpressionError::new("range() step cannot be zero")); }
    let mut elements = Vec::new();
    let mut v = start;
    if step > 0 { while v < stop { elements.push(ExprValue::Int(v)); v += step; ctx.count_op()?; } }
    else { while v > stop { elements.push(ExprValue::Int(v)); v += step; ctx.count_op()?; } }
    Ok(ExprValue::make_list(elements, ExprType::INT)?)
}

pub fn join_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let iter = a[0].list_iter().ok_or_else(|| ExpressionError::new("join() argument must be a list"))?;
    let sep = match a.get(1) { Some(ExprValue::String(s)) => s.as_str(), _ => "" };
    let mut parts = Vec::new();
    for e in iter { ctx.count_op()?; parts.push(e.to_display_string()); }
    Ok(ExprValue::String(parts.join(sep)))
}

pub fn list_from_range(ctx: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::RangeExpr(r) => {
            ctx.count_ops(r.len())?;
            let elements: Vec<ExprValue> = r.iter().map(ExprValue::Int).collect();
            Ok(ExprValue::make_list(elements, ExprType::INT)?)
        }
        _ => Err(ExpressionError::new("list() argument must be range_expr")),
    }
}

pub fn range_expr_from_string(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::String(s) => {
            let r: crate::range_expr::RangeExpr = s.parse()?;
            Ok(ExprValue::RangeExpr(r))
        }
        _ => Err(ExpressionError::new("range_expr() argument must be string")),
    }
}

pub fn range_expr_from_list(ctx: Ctx, a: &[ExprValue]) -> R {
    if let Some(iter) = a[0].list_iter() {
        if a[0].list_len() == Some(0) { return Err(ExpressionError::new("range_expr() requires at least one value")); }
        ctx.count_ops(a[0].list_len().unwrap_or(0))?;
        let mut ints: Vec<i64> = Vec::new();
        for e in iter {
            match e { ExprValue::Int(i) => ints.push(i), _ => return Err(ExpressionError::new("range_expr() list must contain ints")) }
        }
        ints.sort();
        ints.dedup();
        let r = crate::range_expr::RangeExpr::from_values(ints);
        Ok(ExprValue::RangeExpr(r))
    } else {
        Err(ExpressionError::new("range_expr() argument must be list"))
    }
}

pub fn range_expr_from_empty_list(_: Ctx, _a: &[ExprValue]) -> R {
    Err(ExpressionError::new("range_expr() requires at least one value"))
}

/// join with reversed args (method syntax: "sep".join(list))
pub fn join_method_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let sep = match &a[0] { ExprValue::String(s) => s.as_str(), _ => return Err(ExpressionError::new("join() separator must be string")) };
    let iter = a[1].list_iter().ok_or_else(|| ExpressionError::new("join() argument must be a list"))?;
    let mut parts = Vec::new();
    for e in iter { ctx.count_op()?; parts.push(e.to_display_string()); }
    Ok(ExprValue::String(parts.join(sep)))
}
