// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Miscellaneous function implementations (fail).

use crate::error::ExpressionError;
use crate::function_library::EvalContext;
use crate::value::ExprValue;

type R = Result<ExprValue, ExpressionError>;
type Ctx<'a> = &'a mut dyn EvalContext;

pub fn fail_fn(_: Ctx, a: &[ExprValue]) -> R {
    let msg = if a.is_empty() {
        "fail() called".to_string()
    } else {
        a[0].to_display_string()
    };
    Err(ExpressionError::explicit_fail(msg))
}

pub fn zfill_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    if a.len() != 2 {
        return Err(ExpressionError::new("zfill() takes exactly 2 arguments"));
    }
    let s = match &a[0] {
        ExprValue::String(s) => s.clone(),
        ExprValue::Int(i) => i.to_string(),
        _ => a[0].to_display_string(),
    };
    ctx.count_string_ops(s.len())?;
    let width = match &a[1] {
        ExprValue::Int(w) => *w as usize,
        _ => return Err(ExpressionError::new("zfill() width must be int")),
    };
    let clen = s.chars().count();
    let result = if clen >= width {
        s
    } else {
        let (sign, num) = if s.starts_with('-') || s.starts_with('+') {
            (&s[..1], &s[1..])
        } else {
            ("", s.as_str())
        };
        let zeros = width - clen;
        format!("{}{}{}", sign, "0".repeat(zeros), num)
    };
    Ok(ExprValue::String(result))
}

pub fn any_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    if a.len() != 1 {
        return Err(ExpressionError::new("any() takes 1 argument"));
    }
    let iter = a[0]
        .list_iter()
        .ok_or_else(|| ExpressionError::new("any() argument must be a list"))?;
    for e in iter {
        ctx.count_op()?;
        // Signature restricts to list[bool]; match directly (no truthy semantics).
        if let ExprValue::Bool(true) = e {
            return Ok(ExprValue::Bool(true));
        }
    }
    Ok(ExprValue::Bool(false))
}

pub fn all_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    if a.len() != 1 {
        return Err(ExpressionError::new("all() takes 1 argument"));
    }
    let iter = a[0]
        .list_iter()
        .ok_or_else(|| ExpressionError::new("all() argument must be a list"))?;
    for e in iter {
        ctx.count_op()?;
        // Signature restricts to list[bool]; match directly (no truthy semantics).
        if let ExprValue::Bool(false) = e {
            return Ok(ExprValue::Bool(false));
        }
    }
    Ok(ExprValue::Bool(true))
}

pub fn abs_int(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Int(i) => Ok(ExprValue::Int(
            i.checked_abs()
                .ok_or_else(ExpressionError::integer_overflow)?,
        )),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn abs_float(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Float(f) => Ok(ExprValue::Float(crate::value::Float64::new(
            f.value().abs(),
        )?)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn len_string(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::String(s) => Ok(ExprValue::Int(s.chars().count() as i64)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn len_path(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Path { value, .. } => Ok(ExprValue::Int(value.chars().count() as i64)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn len_list(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        val if val.is_list() => Ok(ExprValue::Int(val.list_len().unwrap() as i64)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn len_range(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::RangeExpr(r) => Ok(ExprValue::Int(r.len() as i64)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

pub fn path_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = match &a[0] {
        ExprValue::String(s) => s.clone(),
        ExprValue::Path { value, .. } => value.clone(),
        val if val.is_list() => {
            let parts: Vec<String> = val
                .list_iter()
                .expect("guard ensures list")
                .map(|e| match &e {
                    ExprValue::String(s) | ExprValue::Path { value: s, .. } => s.clone(),
                    _ => e.to_display_string(),
                })
                .collect();
            if parts.is_empty() {
                String::new()
            } else if parts[0].contains("://") {
                // URI: join with "/" preserving empty components (no normalization)
                if parts.len() == 1 {
                    parts[0].clone()
                } else {
                    format!("{}/{}", parts[0], parts[1..].join("/"))
                }
            } else {
                super::path_parse::join_pathlib(&parts, ctx.path_format())
            }
        }
        _ => {
            return Err(ExpressionError::new(format!(
                "path() not supported for {}",
                a[0].expr_type()
            )))
        }
    };
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::new_path(s, ctx.path_format()))
}

pub fn path_join_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let parts: Vec<String> = a
        .iter()
        .map(|v| match v {
            ExprValue::String(s) | ExprValue::Path { value: s, .. } => s.clone(),
            _ => v.to_display_string(),
        })
        .collect();
    let sep = super::path_parse::sep(ctx.path_format());
    let mut result = parts[0].clone();
    for part in &parts[1..] {
        result.push(sep);
        result.push_str(part);
    }
    ctx.count_string_ops(result.len())?;
    Ok(ExprValue::new_path(result, ctx.path_format()))
}

// ── Getitem operators ──

pub fn getitem_list(_: Ctx, a: &[ExprValue]) -> R {
    let len = a[0]
        .list_len()
        .ok_or_else(|| ExpressionError::new("indexing requires list"))?;
    let idx = match &a[1] {
        ExprValue::Int(i) => *i,
        _ => return Err(ExpressionError::new("index must be int")),
    };
    a[0].list_get(idx).ok_or_else(|| {
        ExpressionError::index_out_of_bounds(format!(
            "Index {} out of bounds for list of length {}",
            a[1].to_display_string(),
            len
        ))
    })
}

pub fn getitem_string(_: Ctx, a: &[ExprValue]) -> R {
    let s = match &a[0] {
        ExprValue::String(s) => s.as_str(),
        _ => return Err(ExpressionError::new("indexing requires string")),
    };
    let idx = match &a[1] {
        ExprValue::Int(i) => *i,
        _ => return Err(ExpressionError::new("index must be int")),
    };
    let chars: Vec<char> = s.chars().collect();
    let idx = if idx < 0 {
        chars.len() as i64 + idx
    } else {
        idx
    } as usize;
    if idx >= chars.len() {
        return Err(ExpressionError::index_out_of_bounds(format!(
            "Index {} out of bounds for string of length {}",
            a[1].to_display_string(),
            chars.len()
        )));
    }
    Ok(ExprValue::String(chars[idx].to_string()))
}

pub fn getitem_range(_: Ctx, a: &[ExprValue]) -> R {
    let r = match &a[0] {
        ExprValue::RangeExpr(r) => r,
        _ => return Err(ExpressionError::new("indexing requires range_expr")),
    };
    let idx = match &a[1] {
        ExprValue::Int(i) => *i,
        _ => return Err(ExpressionError::new("index must be int")),
    };
    r.get(idx).map(ExprValue::Int).ok_or_else(|| {
        ExpressionError::index_out_of_bounds(format!(
            "Index {} out of bounds for range_expr of length {}",
            idx,
            r.len()
        ))
    })
}
