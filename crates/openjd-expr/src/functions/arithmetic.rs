// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Arithmetic operator implementations.

use crate::error::ExpressionError;
use crate::function_library::EvalContext;
use crate::types::ExprType;
use crate::value::{ExprValue, Float64};

type R = Result<ExprValue, ExpressionError>;
type Ctx<'a> = &'a mut dyn EvalContext;

// ── Integer arithmetic ──

pub fn add_int(_: Ctx, a: &[ExprValue]) -> R {
    match (&a[0], &a[1]) {
        (ExprValue::Int(l), ExprValue::Int(r)) => Ok(ExprValue::Int(
            l.checked_add(*r)
                .ok_or_else(ExpressionError::integer_overflow)?,
        )),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn sub_int(_: Ctx, a: &[ExprValue]) -> R {
    match (&a[0], &a[1]) {
        (ExprValue::Int(l), ExprValue::Int(r)) => Ok(ExprValue::Int(
            l.checked_sub(*r)
                .ok_or_else(ExpressionError::integer_overflow)?,
        )),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn mul_int(_: Ctx, a: &[ExprValue]) -> R {
    match (&a[0], &a[1]) {
        (ExprValue::Int(l), ExprValue::Int(r)) => Ok(ExprValue::Int(
            l.checked_mul(*r)
                .ok_or_else(ExpressionError::integer_overflow)?,
        )),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn truediv_int(_: Ctx, a: &[ExprValue]) -> R {
    match (&a[0], &a[1]) {
        (ExprValue::Int(l), ExprValue::Int(r)) => {
            if *r == 0 {
                return Err(ExpressionError::division_by_zero("Division"));
            }
            Ok(ExprValue::Float(Float64::new(*l as f64 / *r as f64)?))
        }
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn floordiv_int(_: Ctx, a: &[ExprValue]) -> R {
    match (&a[0], &a[1]) {
        (ExprValue::Int(l), ExprValue::Int(r)) => {
            if *r == 0 {
                return Err(ExpressionError::division_by_zero("Division"));
            }
            let d = l
                .checked_div(*r)
                .ok_or_else(ExpressionError::integer_overflow)?;
            // Python uses floored division (toward -∞), not truncated (toward 0).
            // Adjust when the remainder is nonzero and operands have different signs.
            let result = if (l ^ r) < 0 && d * r != *l { d - 1 } else { d };
            Ok(ExprValue::Int(result))
        }
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn mod_int(_: Ctx, a: &[ExprValue]) -> R {
    match (&a[0], &a[1]) {
        (ExprValue::Int(l), ExprValue::Int(r)) => {
            if *r == 0 {
                return Err(ExpressionError::division_by_zero("Modulo"));
            }
            if *r == 1 || *r == -1 {
                return Ok(ExprValue::Int(0));
            }
            let rem = l
                .checked_rem(*r)
                .ok_or_else(ExpressionError::integer_overflow)?;
            // Python uses floored modulo: result sign matches divisor sign.
            let result = if rem != 0 && (rem ^ r) < 0 {
                rem + r
            } else {
                rem
            };
            Ok(ExprValue::Int(result))
        }
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn pow_int(_: Ctx, a: &[ExprValue]) -> R {
    match (&a[0], &a[1]) {
        (ExprValue::Int(base), ExprValue::Int(exp)) => {
            if *exp < 0 {
                if *base == 0 {
                    return Err(ExpressionError::float_error(
                        "Cannot raise zero to a negative power",
                    ));
                }
                let exp32 = i32::try_from(*exp).unwrap_or(i32::MIN);
                return Ok(ExprValue::Float(Float64::new((*base as f64).powi(exp32))?));
            }
            // Guard: exponent > 63 with |base| > 1 always overflows i64
            if *exp > 63 && !matches!(*base, -1..=1) {
                return Err(ExpressionError::integer_overflow());
            }
            // Special-case base ∈ {-1, 0, 1} to avoid u32 truncation of large exponents
            if *exp > u32::MAX as i64 {
                return Ok(ExprValue::Int(match *base {
                    0 => 0,
                    1 => 1,
                    -1 => {
                        if *exp % 2 == 0 {
                            1
                        } else {
                            -1
                        }
                    }
                    _ => unreachable!(),
                }));
            }
            Ok(ExprValue::Int(
                base.checked_pow(*exp as u32)
                    .ok_or_else(ExpressionError::integer_overflow)?,
            ))
        }
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn neg_int(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Int(n) => Ok(ExprValue::Int(
            n.checked_neg()
                .ok_or_else(ExpressionError::integer_overflow)?,
        )),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn pos_int(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Int(n) => Ok(ExprValue::Int(*n)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

// ── Float arithmetic ──

pub fn add_float(_: Ctx, a: &[ExprValue]) -> R {
    let (l, r) = get_two_floats(a)?;
    Ok(ExprValue::Float(Float64::new(l + r)?))
}

pub fn sub_float(_: Ctx, a: &[ExprValue]) -> R {
    let (l, r) = get_two_floats(a)?;
    Ok(ExprValue::Float(Float64::new(l - r)?))
}

pub fn mul_float(_: Ctx, a: &[ExprValue]) -> R {
    let (l, r) = get_two_floats(a)?;
    Ok(ExprValue::Float(Float64::new(l * r)?))
}

pub fn truediv_float(_: Ctx, a: &[ExprValue]) -> R {
    let (l, r) = get_two_floats(a)?;
    if r == 0.0 {
        return Err(ExpressionError::division_by_zero("Division"));
    }
    Ok(ExprValue::Float(Float64::new(l / r)?))
}

pub fn floordiv_float(_: Ctx, a: &[ExprValue]) -> R {
    let (l, r) = get_two_floats(a)?;
    if r == 0.0 {
        return Err(ExpressionError::division_by_zero("Division"));
    }
    let v = (l / r).floor();
    if v.abs() > i64::MAX as f64 {
        return Err(ExpressionError::integer_overflow());
    }
    Ok(ExprValue::Int(v as i64))
}

pub fn mod_float(_: Ctx, a: &[ExprValue]) -> R {
    let (l, r) = get_two_floats(a)?;
    if r == 0.0 {
        return Err(ExpressionError::division_by_zero("Modulo"));
    }
    // Python uses floored modulo: l - r * floor(l / r)
    Ok(ExprValue::Float(Float64::new(l - r * (l / r).floor())?))
}

pub fn pow_float(_: Ctx, a: &[ExprValue]) -> R {
    let (l, r) = get_two_floats(a)?;
    if l == 0.0 && r < 0.0 {
        return Err(ExpressionError::float_error(
            "Cannot raise zero to a negative power",
        ));
    }
    if l < 0.0 && r.fract() != 0.0 {
        return Err(ExpressionError::float_error(format!(
            "Cannot compute {} ** {} (would produce complex number)",
            l, r
        )));
    }
    let result = l.powf(r);
    if result.is_infinite() {
        return Err(ExpressionError::float_error(format!(
            "Overflow computing {} ** {} (result too large for float)",
            l, r
        )));
    }
    Ok(ExprValue::Float(Float64::new(result)?))
}

pub fn neg_float(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Float(n) => Ok(ExprValue::Float(Float64::new(-n.value())?)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn pos_float(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Float(n) => Ok(ExprValue::Float(Float64::new(n.value())?)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

// ── String operators ──

pub fn add_string(ctx: Ctx, a: &[ExprValue]) -> R {
    match (&a[0], &a[1]) {
        (ExprValue::String(l), ExprValue::String(r)) => {
            ctx.count_string_ops(l.len() + r.len())?;
            Ok(ExprValue::String(format!("{l}{r}")))
        }
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn add_string_range(_: Ctx, a: &[ExprValue]) -> R {
    match (&a[0], &a[1]) {
        (ExprValue::String(l), ExprValue::RangeExpr(r)) => Ok(ExprValue::String(format!("{l}{r}"))),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn add_range_string(_: Ctx, a: &[ExprValue]) -> R {
    match (&a[0], &a[1]) {
        (ExprValue::RangeExpr(l), ExprValue::String(r)) => Ok(ExprValue::String(format!("{l}{r}"))),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn mul_string(ctx: Ctx, a: &[ExprValue]) -> R {
    match (&a[0], &a[1]) {
        (ExprValue::String(s), ExprValue::Int(n)) => {
            if *n < 0 {
                return Ok(ExprValue::String(String::new()));
            }
            let result_len = s.len() * (*n as usize);
            ctx.count_string_ops(result_len)?;
            ctx.check_memory(result_len)?;
            Ok(ExprValue::String(s.repeat(*n as usize)))
        }
        _ => Err(ExpressionError::type_error("type error")),
    }
}

// ── Path operators ──

pub fn path_div(ctx: Ctx, a: &[ExprValue]) -> R {
    let (l, format) = match &a[0] {
        ExprValue::Path { value, format } => (value.as_str(), *format),
        ExprValue::String(s) => (s.as_str(), ctx.path_format()),
        _ => return Err(ExpressionError::type_error("type error")),
    };
    let r = match &a[1] {
        ExprValue::Path { value, .. } | ExprValue::String(value) => value.as_str(),
        _ => return Err(ExpressionError::type_error("type error")),
    };
    ctx.count_string_ops(l.len() + r.len())?;
    Ok(ExprValue::new_path(super::path::join(l, r, format), format))
}

pub fn add_path_string(ctx: Ctx, a: &[ExprValue]) -> R {
    match (&a[0], &a[1]) {
        (ExprValue::Path { value: l, format }, ExprValue::String(r)) => {
            ctx.count_string_ops(l.len() + r.len())?;
            Ok(ExprValue::new_path(format!("{l}{r}"), *format))
        }
        _ => Err(ExpressionError::type_error("type error")),
    }
}

// ── List operators ──

pub fn add_range_range(ctx: Ctx, a: &[ExprValue]) -> R {
    match (&a[0], &a[1]) {
        (ExprValue::RangeExpr(l), ExprValue::RangeExpr(r)) => {
            ctx.count_ops(l.len() + r.len())?;
            let elements: Vec<ExprValue> = l.iter().chain(r.iter()).map(ExprValue::Int).collect();
            Ok(ExprValue::make_list(elements, ExprType::INT)?)
        }
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn mul_list(ctx: Ctx, a: &[ExprValue]) -> R {
    let (elements, elem_type) = a[0]
        .clone()
        .into_list()
        .ok_or_else(|| ExpressionError::type_error("type error"))?;
    let n = match &a[1] {
        ExprValue::Int(n) => *n,
        _ => return Err(ExpressionError::type_error("type error")),
    };
    if n <= 0 {
        return ExprValue::make_list(Vec::new(), elem_type);
    }
    let result_len = elements.len() * n as usize;
    for _ in 0..result_len {
        ctx.count_op()?;
    }
    let mut result = Vec::new();
    for _ in 0..n {
        result.extend(elements.iter().cloned());
    }
    ExprValue::make_list(result, elem_type)
}

pub fn add_list_list(ctx: Ctx, a: &[ExprValue]) -> R {
    let (l, lt) = a[0]
        .clone()
        .into_list()
        .ok_or_else(|| ExpressionError::type_error("type error"))?;
    let (r, rt) = a[1]
        .clone()
        .into_list()
        .ok_or_else(|| ExpressionError::type_error("type error"))?;
    ctx.count_ops(l.len() + r.len())?;
    if lt != rt
        && lt != ExprType::NULLTYPE
        && rt != ExprType::NULLTYPE
        && !((lt == ExprType::INT && rt == ExprType::FLOAT)
            || (lt == ExprType::FLOAT && rt == ExprType::INT))
        && !((lt == ExprType::PATH && rt == ExprType::STRING)
            || (lt == ExprType::STRING && rt == ExprType::PATH))
    {
        return Err(ExpressionError::type_error(format!(
            "Cannot concatenate list[{lt}] and list[{rt}]"
        )));
    }
    let mut combined = l;
    combined.extend(r);
    let result_type = if lt == ExprType::NULLTYPE { rt } else { lt };
    ExprValue::make_list(combined, result_type)
}

pub fn add_list_range(ctx: Ctx, a: &[ExprValue]) -> R {
    let (mut l, et) = a[0]
        .clone()
        .into_list()
        .ok_or_else(|| ExpressionError::type_error("type error"))?;
    let r = match &a[1] {
        ExprValue::RangeExpr(r) => r,
        _ => return Err(ExpressionError::type_error("type error")),
    };
    ctx.count_ops(l.len() + r.len())?;
    l.extend(r.iter().map(ExprValue::Int));
    ExprValue::make_list(l, et)
}

pub fn add_range_list(ctx: Ctx, a: &[ExprValue]) -> R {
    let r = match &a[0] {
        ExprValue::RangeExpr(r) => r,
        _ => return Err(ExpressionError::type_error("type error")),
    };
    let (l, et) = a[1]
        .clone()
        .into_list()
        .ok_or_else(|| ExpressionError::type_error("type error"))?;
    ctx.count_ops(r.len() + l.len())?;
    let mut combined: Vec<ExprValue> = r.iter().map(ExprValue::Int).collect();
    combined.extend(l);
    ExprValue::make_list(combined, et)
}

// ── Comparison operators ──

pub fn not_bool(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Bool(b) => Ok(ExprValue::Bool(!*b)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

// ── Helpers ──

fn get_two_floats(a: &[ExprValue]) -> Result<(f64, f64), ExpressionError> {
    let l = match &a[0] {
        ExprValue::Float(f) => f.value(),
        ExprValue::Int(i) => *i as f64,
        _ => return Err(ExpressionError::type_error("type error")),
    };
    let r = match &a[1] {
        ExprValue::Float(f) => f.value(),
        ExprValue::Int(i) => *i as f64,
        _ => return Err(ExpressionError::type_error("type error")),
    };
    Ok((l, r))
}
