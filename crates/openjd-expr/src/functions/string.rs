// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! String function implementations.

use super::StringOutputBudget;
use crate::error::ExpressionError;
use crate::function_library::EvalContext;
use crate::types::ExprType;
use crate::value::ExprValue;

type R = Result<ExprValue, ExpressionError>;
type Ctx<'a> = &'a mut dyn EvalContext;

fn get_str(a: &ExprValue) -> Result<&str, ExpressionError> {
    match a {
        ExprValue::String(s) => Ok(s),
        ExprValue::Path { value, .. } => Ok(value),
        _ => Err(ExpressionError::new(format!(
            "String method not supported on {}",
            a.expr_type()
        ))),
    }
}

/// Convert a byte offset (from str::find) to a codepoint offset (matching Python).
fn byte_to_char_offset(s: &str, byte_offset: usize) -> usize {
    s[..byte_offset].chars().count()
}

pub fn upper_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::String(s.to_uppercase()))
}
pub fn lower_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::String(s.to_lowercase()))
}

pub fn strip_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    if a.len() > 1 {
        let chars: Vec<char> = get_str(&a[1])?.chars().collect();
        Ok(ExprValue::String(
            s.trim_matches(|c| chars.contains(&c)).to_string(),
        ))
    } else {
        Ok(ExprValue::String(s.trim().to_string()))
    }
}

pub fn lstrip_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    if a.len() > 1 {
        let chars: Vec<char> = get_str(&a[1])?.chars().collect();
        Ok(ExprValue::String(
            s.trim_start_matches(|c| chars.contains(&c)).to_string(),
        ))
    } else {
        Ok(ExprValue::String(s.trim_start().to_string()))
    }
}

pub fn rstrip_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    if a.len() > 1 {
        let chars: Vec<char> = get_str(&a[1])?.chars().collect();
        Ok(ExprValue::String(
            s.trim_end_matches(|c| chars.contains(&c)).to_string(),
        ))
    } else {
        Ok(ExprValue::String(s.trim_end().to_string()))
    }
}

pub fn removeprefix_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    let prefix = get_str(&a[1])?;
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::String(
        s.strip_prefix(prefix).unwrap_or(s).to_string(),
    ))
}

pub fn removesuffix_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    let suffix = get_str(&a[1])?;
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::String(
        s.strip_suffix(suffix).unwrap_or(s).to_string(),
    ))
}

pub fn replace_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    let old = get_str(&a[1])?;
    let new = get_str(&a[2])?;
    if old.is_empty() {
        return Err(ExpressionError::new("replace failed: empty old string"));
    }
    let replacements = s.len() / old.len();
    let growth_per_replacement = new.len().saturating_sub(old.len());
    let output_bytes = s
        .len()
        .saturating_add(replacements.saturating_mul(growth_per_replacement));
    let budget = StringOutputBudget::reserve(ctx, s.len().max(output_bytes), output_bytes)?;
    Ok(budget.finish(s.replace(old, new)))
}

pub fn startswith_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::Bool(s.starts_with(get_str(&a[1])?)))
}

pub fn endswith_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::Bool(s.ends_with(get_str(&a[1])?)))
}

pub fn find_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    let sub = get_str(&a[1])?;
    if sub.is_empty() {
        return Err(ExpressionError::new("find failed: empty substring"));
    }
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::Int(
        s.find(sub)
            .map(|p| byte_to_char_offset(s, p) as i64)
            .unwrap_or(-1),
    ))
}

pub fn rfind_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    let sub = get_str(&a[1])?;
    if sub.is_empty() {
        return Err(ExpressionError::new("rfind failed: empty substring"));
    }
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::Int(
        s.rfind(sub)
            .map(|p| byte_to_char_offset(s, p) as i64)
            .unwrap_or(-1),
    ))
}

pub fn index_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    let sub = get_str(&a[1])?;
    if sub.is_empty() {
        return Err(ExpressionError::new("index failed: empty substring"));
    }
    ctx.count_string_ops(s.len())?;
    match s.find(sub) {
        Some(p) => Ok(ExprValue::Int(byte_to_char_offset(s, p) as i64)),
        None => Err(ExpressionError::new(format!(
            "index failed: substring '{sub}' not found"
        ))),
    }
}

pub fn rindex_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    let sub = get_str(&a[1])?;
    if sub.is_empty() {
        return Err(ExpressionError::new("rindex failed: empty substring"));
    }
    ctx.count_string_ops(s.len())?;
    match s.rfind(sub) {
        Some(p) => Ok(ExprValue::Int(byte_to_char_offset(s, p) as i64)),
        None => Err(ExpressionError::new(format!(
            "rindex failed: substring '{sub}' not found"
        ))),
    }
}

pub fn count_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    let sub = get_str(&a[1])?;
    if sub.is_empty() {
        return Err(ExpressionError::new("count failed: empty substring"));
    }
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::Int(s.matches(sub).count() as i64))
}

/// Extract the optional third `maxsplit` argument for split-family functions.
/// Negative values mean "no limit", matching Python's `str.split`/`re.split`.
pub(crate) fn maxsplit_arg(a: &[ExprValue]) -> Option<usize> {
    a.get(2).and_then(|v| match v {
        ExprValue::Int(n) if *n >= 0 => Some(*n as usize),
        _ => None,
    })
}

pub fn split_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    if a.len() == 1 {
        // Whitespace split
        ctx.count_string_ops(s.len())?;
        let parts: Vec<ExprValue> = s
            .split_whitespace()
            .map(|p| ExprValue::String(p.to_string()))
            .collect();
        return ExprValue::make_list_checked(ctx, parts, ExprType::STRING);
    }
    let sep = get_str(&a[1])?;
    if sep.is_empty() {
        return Err(ExpressionError::new("split failed: empty separator"));
    }
    ctx.count_string_ops(s.len())?;
    let maxsplit = maxsplit_arg(a);
    let parts: Vec<ExprValue> = match maxsplit {
        Some(n) => s
            .splitn(n + 1, sep)
            .map(|p| ExprValue::String(p.to_string()))
            .collect(),
        None => s
            .split(sep)
            .map(|p| ExprValue::String(p.to_string()))
            .collect(),
    };
    ExprValue::make_list_checked(ctx, parts, ExprType::STRING)
}

pub fn rsplit_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    if a.len() == 1 {
        ctx.count_string_ops(s.len())?;
        let parts: Vec<ExprValue> = s
            .split_whitespace()
            .map(|p| ExprValue::String(p.to_string()))
            .collect();
        return ExprValue::make_list_checked(ctx, parts, ExprType::STRING);
    }
    let sep = get_str(&a[1])?;
    if sep.is_empty() {
        return Err(ExpressionError::new("split failed: empty separator"));
    }
    ctx.count_string_ops(s.len())?;
    let maxsplit = maxsplit_arg(a);
    let parts: Vec<ExprValue> = match maxsplit {
        Some(n) => {
            let mut v: Vec<_> = s
                .rsplitn(n + 1, sep)
                .map(|p| ExprValue::String(p.to_string()))
                .collect();
            v.reverse();
            v
        }
        None => s
            .split(sep)
            .map(|p| ExprValue::String(p.to_string()))
            .collect(),
    };
    ExprValue::make_list_checked(ctx, parts, ExprType::STRING)
}

pub fn isdigit_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::Bool(
        !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()),
    ))
}
pub fn isalpha_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::Bool(
        !s.is_empty() && s.chars().all(|c| c.is_alphabetic()),
    ))
}
pub fn isalnum_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::Bool(
        !s.is_empty() && s.chars().all(|c| c.is_alphanumeric()),
    ))
}
pub fn isspace_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::Bool(
        !s.is_empty() && s.chars().all(|c| c.is_whitespace()),
    ))
}
pub fn isupper_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::Bool(
        s.chars().any(|c| c.is_alphabetic())
            && s.chars()
                .filter(|c| c.is_alphabetic())
                .all(|c| c.is_uppercase()),
    ))
}
pub fn islower_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::Bool(
        s.chars().any(|c| c.is_alphabetic())
            && s.chars()
                .filter(|c| c.is_alphabetic())
                .all(|c| c.is_lowercase()),
    ))
}
pub fn isascii_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::Bool(s.is_ascii()))
}

pub fn title_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if capitalize_next {
                result.extend(c.to_uppercase());
                capitalize_next = false;
            } else {
                result.extend(c.to_lowercase());
            }
        } else {
            result.push(c);
            capitalize_next = true;
        }
    }
    Ok(ExprValue::String(result))
}

pub fn capitalize_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    let mut chars = s.chars();
    let result = match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + &chars.as_str().to_lowercase(),
    };
    Ok(ExprValue::String(result))
}

pub(super) fn preflight_padding(
    ctx: Ctx,
    s: &str,
    requested_width: i64,
) -> Result<(StringOutputBudget, usize, usize, usize), ExpressionError> {
    // Charge the full input traversal before counting Unicode scalar values.
    ctx.count_string_ops(s.len())?;
    let width = usize::try_from(requested_width.max(0)).unwrap_or(usize::MAX);
    let char_len = s.chars().count();
    let padding_bytes = width.saturating_sub(char_len);
    let output_bytes = s.len().saturating_add(padding_bytes);
    let budget = StringOutputBudget::reserve(ctx, padding_bytes, output_bytes)?;
    Ok((budget, width, char_len, output_bytes))
}

pub fn center_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    let requested_width = match &a[1] {
        ExprValue::Int(w) => *w,
        _ => return Err(ExpressionError::new("center() width must be int")),
    };
    let (budget, width, char_len, output_bytes) = preflight_padding(ctx, s, requested_width)?;
    if char_len >= width {
        return Ok(budget.finish(s.to_string()));
    }
    let pad = width - char_len;
    let left = pad / 2 + (pad & width & 1);
    let right = pad - left;
    let mut result = String::with_capacity(output_bytes);
    for _ in 0..left {
        result.push(' ');
    }
    result.push_str(s);
    for _ in 0..right {
        result.push(' ');
    }
    Ok(budget.finish(result))
}

pub fn ljust_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    let requested_width = match &a[1] {
        ExprValue::Int(w) => *w,
        _ => return Err(ExpressionError::new("ljust() width must be int")),
    };
    let (budget, width, char_len, output_bytes) = preflight_padding(ctx, s, requested_width)?;
    let mut result = String::with_capacity(output_bytes);
    result.push_str(s);
    for _ in char_len..width {
        result.push(' ');
    }
    Ok(budget.finish(result))
}

pub fn rjust_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    let requested_width = match &a[1] {
        ExprValue::Int(w) => *w,
        _ => return Err(ExpressionError::new("rjust() width must be int")),
    };
    let (budget, width, char_len, output_bytes) = preflight_padding(ctx, s, requested_width)?;
    let mut result = String::with_capacity(output_bytes);
    for _ in char_len..width {
        result.push(' ');
    }
    result.push_str(s);
    Ok(budget.finish(result))
}
