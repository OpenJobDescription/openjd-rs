// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! List function implementations (sorted, reversed, unique, flatten, range).

use super::StringOutputBudget;
use crate::budgeted_vec::BudgetedVec;
use crate::error::ExpressionError;
use crate::function_library::EvalContext;
use crate::types::ExprType;
use crate::value::ExprValue;

type R = Result<ExprValue, ExpressionError>;
type Ctx<'a> = &'a mut dyn EvalContext;

pub fn sorted_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (elements, elem_type) = a[0]
        .clone()
        .into_list()
        .ok_or_else(|| ExpressionError::new("sorted() argument must be a list"))?;
    ctx.count_ops(elements.len())?;
    let mut sorted = elements;
    sorted.sort_by(|a, b| a.compare(b).unwrap_or(std::cmp::Ordering::Equal));
    ExprValue::make_list_checked(ctx, sorted, elem_type)
}

pub fn reversed_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (mut elements, elem_type) = a[0]
        .clone()
        .into_list()
        .ok_or_else(|| ExpressionError::new("reversed() argument must be a list"))?;
    ctx.count_ops(elements.len())?;
    elements.reverse();
    ExprValue::make_list_checked(ctx, elements, elem_type)
}

pub fn unique_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (elements, elem_type) = a[0]
        .clone()
        .into_list()
        .ok_or_else(|| ExpressionError::new("unique() argument must be a list"))?;
    ctx.count_ops(elements.len())?;
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for e in elements {
        if seen.insert(e.clone()) {
            result.push(e);
        }
    }
    ExprValue::make_list_checked(ctx, result, elem_type)
}

pub fn flatten_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let iter = a[0]
        .list_iter()
        .ok_or_else(|| ExpressionError::new("flatten() argument must be a list"))?;
    let mut result = Vec::new();
    for e in iter {
        ctx.count_op()?;
        if e.is_list() {
            let inner_len = e.list_len().unwrap_or(0);
            ctx.count_ops(inner_len)?;
            result.extend(e.list_iter().expect("is_list() was true"));
        } else {
            result.push(e);
        }
    }
    let et = if result.is_empty() {
        // An empty flatten result has no elements to infer a type from.
        // Use NULLTYPE (the bottom type for lists) so the result is accepted by
        // any function that declares a list[nulltype] overload (e.g. repr_sh).
        ExprType::NULLTYPE
    } else {
        result[0].expr_type()
    };
    ExprValue::make_list_checked(ctx, result, et)
}

pub fn range_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (start, stop, step) = match a.len() {
        1 => (
            0i64,
            match &a[0] {
                ExprValue::Int(n) => *n,
                _ => return Err(ExpressionError::new("range() argument must be int")),
            },
            1i64,
        ),
        2 => (
            match &a[0] {
                ExprValue::Int(n) => *n,
                _ => return Err(ExpressionError::new("range() arguments must be int")),
            },
            match &a[1] {
                ExprValue::Int(n) => *n,
                _ => return Err(ExpressionError::new("range() arguments must be int")),
            },
            1,
        ),
        3 => (
            match &a[0] {
                ExprValue::Int(n) => *n,
                _ => return Err(ExpressionError::new("range() arguments must be int")),
            },
            match &a[1] {
                ExprValue::Int(n) => *n,
                _ => return Err(ExpressionError::new("range() arguments must be int")),
            },
            match &a[2] {
                ExprValue::Int(n) => *n,
                _ => return Err(ExpressionError::new("range() arguments must be int")),
            },
        ),
        _ => return Err(ExpressionError::new("range() takes 1-3 arguments")),
    };
    if step == 0 {
        return Err(ExpressionError::new("range() step cannot be zero"));
    }
    let mut elements = Vec::new();
    let mut v = start;
    if step > 0 {
        while v < stop {
            elements.push(ExprValue::Int(v));
            ctx.count_op()?;
            // Overflow means the next value would exceed i64::MAX >= stop, so the range ends.
            match v.checked_add(step) {
                Some(next) => v = next,
                None => break,
            }
        }
    } else {
        while v > stop {
            elements.push(ExprValue::Int(v));
            ctx.count_op()?;
            // Underflow means the next value would be below i64::MIN <= stop, so the range ends.
            match v.checked_add(step) {
                Some(next) => v = next,
                None => break,
            }
        }
    }
    ExprValue::make_list_checked(ctx, elements, ExprType::INT)
}

pub fn join_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let len = a[0]
        .list_len()
        .ok_or_else(|| ExpressionError::new("join() argument must be a list"))?;
    let sep = match a.get(1) {
        Some(ExprValue::String(s)) => s.as_str(),
        _ => "",
    };
    for _ in 0..len {
        ctx.count_op()?;
    }

    let values = match &a[0] {
        ExprValue::ListString(values, _) | ExprValue::ListPath(values, _, _) => values,
        _ if len == 0 => {
            let budget = StringOutputBudget::reserve(ctx, 0, 0)?;
            return Ok(budget.finish(String::new()));
        }
        _ => return Err(ExpressionError::new("join() argument must contain strings")),
    };
    let output_bytes = values
        .iter()
        .map(String::len)
        .fold(0usize, usize::saturating_add)
        .saturating_add(sep.len().saturating_mul(len.saturating_sub(1)));
    let budget = StringOutputBudget::reserve(ctx, output_bytes, output_bytes)?;

    let mut result = String::with_capacity(output_bytes);
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            result.push_str(sep);
        }
        result.push_str(value);
    }
    Ok(budget.finish(result))
}

pub fn list_from_range(ctx: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::RangeExpr(r) => {
            ctx.count_ops(r.len())?;
            // Pre-check the projected allocation before collecting: the
            // op charge alone admits up to the operation limit in
            // elements (~640 MB of ExprValues under default limits)
            // before make_list_checked's post-hoc memory check.
            let mut elements = BudgetedVec::with_capacity(ctx, r.len())?;
            for value in r.iter() {
                elements.push(ctx, ExprValue::Int(value))?;
            }
            Ok(ExprValue::make_list_checked(
                ctx,
                elements.into_vec(),
                ExprType::INT,
            )?)
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
        if a[0].list_len() == Some(0) {
            return Err(ExpressionError::new(
                "range_expr() requires at least one value",
            ));
        }
        ctx.count_ops(a[0].list_len().unwrap_or(0))?;
        let mut ints: Vec<i64> = Vec::new();
        for e in iter {
            match e {
                ExprValue::Int(i) => ints.push(i),
                _ => return Err(ExpressionError::new("range_expr() list must contain ints")),
            }
        }
        ints.sort();
        ints.dedup();
        let r = crate::range_expr::RangeExpr::from_values(ints)?;
        Ok(ExprValue::RangeExpr(r))
    } else {
        Err(ExpressionError::new("range_expr() argument must be list"))
    }
}

pub fn range_expr_from_empty_list(_: Ctx, _a: &[ExprValue]) -> R {
    Err(ExpressionError::new(
        "range_expr() requires at least one value",
    ))
}
