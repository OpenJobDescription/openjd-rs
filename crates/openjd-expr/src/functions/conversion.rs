// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Type conversion function implementations.

use crate::error::ExpressionError;
use crate::function_library::EvalContext;
use crate::value::{ExprValue, Float64};

type R = Result<ExprValue, ExpressionError>;
type Ctx<'a> = &'a mut dyn EvalContext;

pub fn string_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    // A list renders as a JSON array whose element strings are escaped, so the
    // output can grow several times larger than the input. Charge the same
    // budget `repr_json` does before building it. Scalars render as themselves.
    if a[0].is_list() {
        let budget = super::repr::preflight_display_list(ctx, &a[0])?;
        return Ok(budget.finish(a[0].to_display_string()));
    }
    Ok(ExprValue::String(a[0].to_display_string()))
}

pub fn int_from_int(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Int(i) => Ok(ExprValue::Int(*i)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn int_from_float(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Float(f) => {
            if f.value().fract() != 0.0 {
                return Err(ExpressionError::new(format!(
                    "Cannot convert {f} to int: not a whole number"
                )));
            }
            if f.value() >= i64::MAX as f64 || f.value() < i64::MIN as f64 {
                return Err(ExpressionError::integer_overflow());
            }
            Ok(ExprValue::Int(f.value() as i64))
        }
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn int_from_bool(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Bool(b) => Ok(ExprValue::Int(if *b { 1 } else { 0 })),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

/// Replace every Unicode decimal digit (Numeric_Type=Decimal, category Nd)
/// with its ASCII equivalent, matching CPython's transformation of `int()`
/// and `float()` string arguments. All other characters pass through
/// unchanged and are left for the numeric parser to accept or reject —
/// notably `Numeric_Type=Digit` characters like `'²'`, which `isdigit()`
/// accepts but CPython's `int()` rejects.
fn normalize_decimal_digits(s: &str) -> std::borrow::Cow<'_, str> {
    use super::unicode_tables::decimal_digit_value;
    if s.is_ascii() {
        return std::borrow::Cow::Borrowed(s);
    }
    std::borrow::Cow::Owned(
        s.chars()
            .map(|c| match decimal_digit_value(c) {
                Some(d) => char::from(b'0' + d as u8),
                None => c,
            })
            .collect(),
    )
}

pub fn int_from_string(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::String(s) => {
            let v: i64 = normalize_decimal_digits(s.trim())
                .parse()
                .map_err(|_| ExpressionError::new(format!("Cannot convert '{s}' to int")))?;
            Ok(ExprValue::Int(v))
        }
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn float_from_float(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Float(f) => Ok(ExprValue::Float(Float64::new(f.value())?)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn float_from_int(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Int(i) => Ok(ExprValue::Float(Float64::new(*i as f64)?)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn float_from_string(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::String(s) => {
            let lower = s.trim().to_lowercase();
            if lower == "inf" || lower == "infinity" || lower == "-inf" || lower == "-infinity" {
                return Err(ExpressionError::float_error(
                    "Cannot convert to float: infinity",
                ));
            }
            if lower == "nan" {
                return Err(ExpressionError::float_error("Cannot convert to float: NaN"));
            }
            let v: f64 = normalize_decimal_digits(s.trim())
                .parse()
                .map_err(|_| ExpressionError::new(format!("Cannot convert '{s}' to float")))?;
            Ok(ExprValue::Float(Float64::new(v)?))
        }
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn bool_from_bool(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Bool(b) => Ok(ExprValue::Bool(*b)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn bool_from_int(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Int(i) => Ok(ExprValue::Bool(*i != 0)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn bool_from_float(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Float(f) => Ok(ExprValue::Bool(*f != 0.0)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn bool_from_null(_: Ctx, _a: &[ExprValue]) -> R {
    Ok(ExprValue::Bool(false))
}

pub fn bool_from_string(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::String(s) => match s.to_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Ok(ExprValue::Bool(true)),
            "false" | "no" | "off" | "0" => Ok(ExprValue::Bool(false)),
            _ => Err(ExpressionError::new(format!(
                "Cannot convert '{s}' to bool. Expected one of: 1, true, on, yes, 0, false, off, no"
            ))),
        },
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn bool_from_path(_: Ctx, _a: &[ExprValue]) -> R {
    Err(ExpressionError::new("Cannot convert path to bool"))
}

pub fn bool_from_list(_: Ctx, _a: &[ExprValue]) -> R {
    Err(ExpressionError::new("Cannot convert list to bool"))
}
