// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Math function implementations (min, max, floor, ceil, round, sum).

use crate::error::ExpressionError;
use crate::function_library::EvalContext;
use crate::value::{ExprValue, Float64};

type R = Result<ExprValue, ExpressionError>;
type Ctx<'a> = &'a mut dyn EvalContext;

fn min_max_items(a: &[ExprValue], name: &str) -> Result<Vec<ExprValue>, ExpressionError> {
    if a.is_empty() {
        return Err(ExpressionError::new(format!(
            "{name}() requires at least 1 argument"
        )));
    }
    if a.len() == 1 {
        match &a[0] {
            val if val.is_list() => {
                let elements: Vec<ExprValue> =
                    val.list_iter().expect("guard ensures list").collect();
                if elements.is_empty() {
                    return Err(ExpressionError::new(format!(
                        "{name}() requires a non-empty list"
                    )));
                }
                Ok(elements)
            }
            ExprValue::RangeExpr(r) => {
                if r.is_empty() {
                    return Err(ExpressionError::new(format!(
                        "{name}() requires a non-empty list"
                    )));
                }
                if name == "min" {
                    return Ok(vec![ExprValue::Int(r.iter().next().unwrap())]);
                } else {
                    Ok(vec![ExprValue::Int(r.iter().last().unwrap())])
                }
            }
            _ => Ok(a.to_vec()),
        }
    } else {
        Ok(a.to_vec())
    }
}

pub fn min_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let items = min_max_items(a, "min")?;
    ctx.count_ops(items.len())?;
    let mut result = items[0].clone();
    for item in &items[1..] {
        if result.compare(item)?.is_gt() {
            result = item.clone();
        }
    }
    if items.iter().any(|i| matches!(i, ExprValue::Float(_))) {
        if let ExprValue::Int(i) = &result {
            return Ok(ExprValue::Float(Float64::new(*i as f64)?));
        }
    }
    Ok(result)
}

pub fn max_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let items = min_max_items(a, "max")?;
    ctx.count_ops(items.len())?;
    let mut result = items[0].clone();
    for item in &items[1..] {
        if result.compare(item)?.is_lt() {
            result = item.clone();
        }
    }
    if items.iter().any(|i| matches!(i, ExprValue::Float(_))) {
        if let ExprValue::Int(i) = &result {
            return Ok(ExprValue::Float(Float64::new(*i as f64)?));
        }
    }
    Ok(result)
}

fn round_half_even(x: f64) -> f64 {
    let rounded = x.round();
    if (x - rounded).abs() == 0.5 {
        if rounded as i64 % 2 != 0 {
            rounded - x.signum()
        } else {
            rounded
        }
    } else {
        rounded
    }
}

pub fn floor_float(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Float(f) => {
            let v = f.floor();
            if v.abs() > i64::MAX as f64 {
                return Err(ExpressionError::integer_overflow());
            }
            Ok(ExprValue::Int(v as i64))
        }
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn floor_int(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Int(i) => Ok(ExprValue::Int(*i)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn ceil_float(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Float(f) => {
            let v = f.ceil();
            if v.abs() > i64::MAX as f64 {
                return Err(ExpressionError::integer_overflow());
            }
            Ok(ExprValue::Int(v as i64))
        }
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn ceil_int(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Int(i) => Ok(ExprValue::Int(*i)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn round_fn(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Float(f) => {
            let has_ndigits = a.len() > 1;
            let ndigits = a
                .get(1)
                .and_then(|v| match v {
                    ExprValue::Int(n) => Some(*n),
                    _ => None,
                })
                .unwrap_or(0);
            if !has_ndigits {
                let v = round_half_even(f.value());
                if v.abs() > i64::MAX as f64 {
                    return Err(ExpressionError::integer_overflow());
                }
                Ok(ExprValue::Int(v as i64))
            } else if ndigits >= 0 {
                let factor = 10f64.powi(ndigits as i32);
                let rounded = round_half_even(f.value() * factor) / factor;
                if ndigits == 0 {
                    Ok(ExprValue::Float(Float64::with_str(
                        rounded,
                        format!("{}.0", rounded as i64),
                    )?))
                } else {
                    Ok(ExprValue::Float(Float64::with_str(
                        rounded,
                        format!("{:.prec$}", rounded, prec = ndigits as usize),
                    )?))
                }
            } else {
                let factor = 10f64.powi((-ndigits) as i32);
                Ok(ExprValue::Float(Float64::new(
                    round_half_even(f.value() / factor) * factor,
                )?))
            }
        }
        ExprValue::Int(i) => {
            let ndigits = a
                .get(1)
                .and_then(|v| match v {
                    ExprValue::Int(n) => Some(*n),
                    _ => None,
                })
                .unwrap_or(0);
            if ndigits >= 0 {
                Ok(ExprValue::Int(*i))
            } else {
                let factor = 10f64.powi((-ndigits) as i32);
                let v = round_half_even(*i as f64 / factor) * factor;
                if v.abs() > i64::MAX as f64 {
                    return Err(ExpressionError::integer_overflow());
                }
                Ok(ExprValue::Int(v as i64))
            }
        }
        _ => Err(ExpressionError::new("round() requires numeric argument")),
    }
}

pub fn sum_list(ctx: Ctx, a: &[ExprValue]) -> R {
    if let Some(iter) = a[0].list_iter() {
        let mut int_sum: i64 = 0;
        let mut is_float = false;
        let mut float_sum: f64 = 0.0;
        for e in iter {
            ctx.count_op()?;
            match e {
                ExprValue::Int(i) => {
                    int_sum = int_sum
                        .checked_add(i)
                        .ok_or_else(ExpressionError::integer_overflow)?;
                    float_sum += i as f64;
                }
                ExprValue::Float(f) => {
                    is_float = true;
                    float_sum += f.value();
                }
                _ => return Err(ExpressionError::new("sum() elements must be numeric")),
            }
        }
        if is_float {
            Ok(ExprValue::Float(Float64::new(float_sum)?))
        } else {
            Ok(ExprValue::Int(int_sum))
        }
    } else if let ExprValue::RangeExpr(r) = &a[0] {
        for _ in r.iter() {
            ctx.count_op()?;
        }
        Ok(ExprValue::Int(r.iter().sum()))
    } else {
        Err(ExpressionError::new("sum() requires list or range_expr"))
    }
}
