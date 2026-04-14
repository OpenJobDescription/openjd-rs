// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Type conversion function implementations.

use crate::error::ExpressionError;
use crate::function_library::EvalContext;
use crate::value::{ExprValue, Float64};

type R = Result<ExprValue, ExpressionError>;
type Ctx<'a> = &'a mut dyn EvalContext;

pub fn string_fn(_: Ctx, a: &[ExprValue]) -> R {
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
            if f.0.fract() != 0.0 {
                return Err(ExpressionError::new(format!(
                    "Cannot convert {f} to int: not a whole number"
                )));
            }
            if *f > i64::MAX as f64 || *f < i64::MIN as f64 {
                return Err(ExpressionError::integer_overflow());
            }
            Ok(ExprValue::Int(f.0 as i64))
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

pub fn int_from_string(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::String(s) => {
            let v: i64 = s
                .trim()
                .parse()
                .map_err(|_| ExpressionError::new(format!("Cannot convert '{s}' to int")))?;
            Ok(ExprValue::Int(v))
        }
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn float_from_float(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Float(f) => Ok(ExprValue::Float(Float64::new(f.0)?)),
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
            let v: f64 = s
                .trim()
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
