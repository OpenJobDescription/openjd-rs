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

/// Python whitespace for `strip`/`lstrip`/`rstrip` and no-separator
/// `split`/`rsplit`: CPython's `Py_UNICODE_ISSPACE`, via the generated
/// `SPACE` table. This is Unicode `White_Space` (what Rust's
/// `char::is_whitespace` tests) plus the information separators
/// U+001C..U+001F, so Rust's `str::trim`/`split_whitespace` must not be
/// used for these functions.
fn is_python_space(c: char) -> bool {
    use super::unicode_tables::{in_table, SPACE};
    in_table(SPACE, c)
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
        Ok(ExprValue::String(
            s.trim_matches(is_python_space).to_string(),
        ))
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
        Ok(ExprValue::String(
            s.trim_start_matches(is_python_space).to_string(),
        ))
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
        Ok(ExprValue::String(
            s.trim_end_matches(is_python_space).to_string(),
        ))
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

/// Extract the optional third `maxsplit` argument for split()/rsplit().
/// Negative values mean "no limit", matching Python's `str.split`.
/// (`re_split` deliberately differs: Python's `re.split` treats negative as
/// "no splits" and 0 as unlimited — see `functions/regex.rs`.)
///
/// The `i64 -> usize` conversion saturates rather than casting: on 32-bit
/// targets `usize` is narrower than `i64`, and a plain `as` cast would wrap a
/// large maxsplit down to a small one (or to 0) and silently split too few
/// times. Saturating to `usize::MAX` keeps "larger than the string" behaving
/// like "no limit" on every target width.
fn maxsplit_arg(a: &[ExprValue]) -> Option<usize> {
    a.get(2).and_then(|v| match v {
        ExprValue::Int(n) if *n >= 0 => Some(usize::try_from(*n).unwrap_or(usize::MAX)),
        _ => None,
    })
}

pub fn split_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    if a.len() == 1 {
        // Whitespace split
        ctx.count_string_ops(s.len())?;
        let parts: Vec<ExprValue> = s
            .split(is_python_space)
            .filter(|p| !p.is_empty())
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
            .splitn(n.saturating_add(1), sep)
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
            .split(is_python_space)
            .filter(|p| !p.is_empty())
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
                .rsplitn(n.saturating_add(1), sep)
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
    use super::unicode_tables::{in_table, DIGIT};
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::Bool(
        !s.is_empty() && s.chars().all(|c| in_table(DIGIT, c)),
    ))
}
pub fn isalpha_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    use super::unicode_tables::{in_table, ALPHA};
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::Bool(
        !s.is_empty() && s.chars().all(|c| in_table(ALPHA, c)),
    ))
}
pub fn isalnum_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    use super::unicode_tables::{in_table, ALNUM};
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::Bool(
        !s.is_empty() && s.chars().all(|c| in_table(ALNUM, c)),
    ))
}
pub fn isspace_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    use super::unicode_tables::{in_table, SPACE};
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::Bool(
        !s.is_empty() && s.chars().all(|c| in_table(SPACE, c)),
    ))
}
pub fn isupper_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    use super::unicode_tables::{in_table, CASED, UPPER};
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    // Python str.isupper: at least one cased character, and every cased
    // character is uppercase. Uncased characters (digits, CJK, punctuation)
    // are ignored. Titlecase (Lt) characters are cased but not uppercase,
    // so their presence makes the result false.
    Ok(ExprValue::Bool(
        s.chars().any(|c| in_table(CASED, c))
            && s.chars()
                .filter(|&c| in_table(CASED, c))
                .all(|c| in_table(UPPER, c)),
    ))
}
pub fn islower_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    use super::unicode_tables::{in_table, CASED, LOWER};
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    // Python str.islower: at least one cased character, and every cased
    // character is lowercase. See isupper_fn for the cased-character rule.
    Ok(ExprValue::Bool(
        s.chars().any(|c| in_table(CASED, c))
            && s.chars()
                .filter(|&c| in_table(CASED, c))
                .all(|c| in_table(LOWER, c)),
    ))
}
pub fn isascii_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::Bool(s.is_ascii()))
}

/// True per Unicode's Final_Sigma rule: `chars[i]` (a capital sigma) is
/// preceded by a cased character and not followed by one, skipping
/// case-ignorable characters in both directions. Mirrors CPython's
/// `handle_capital_sigma` so `title()`/`capitalize()` lower U+03A3 exactly
/// like Python's context-sensitive case mapping.
fn is_final_sigma(chars: &[char], i: usize) -> bool {
    use super::unicode_tables::{in_table, CASED, CASE_IGNORABLE};
    let preceded_by_cased = chars[..i]
        .iter()
        .rev()
        .find(|&&c| !in_table(CASE_IGNORABLE, c))
        .is_some_and(|&c| in_table(CASED, c));
    let followed_by_cased = chars[i + 1..]
        .iter()
        .find(|&&c| !in_table(CASE_IGNORABLE, c))
        .is_some_and(|&c| in_table(CASED, c));
    preceded_by_cased && !followed_by_cased
}

/// Append the Python full lowercase mapping of `chars[i]`, applying the
/// Final_Sigma context rule for U+03A3 (Rust's `char::to_lowercase` cannot
/// see the surrounding context).
fn push_lowered(chars: &[char], i: usize, out: &mut String) {
    const CAPITAL_SIGMA: char = '\u{03a3}';
    const FINAL_SIGMA: char = '\u{03c2}';
    const SMALL_SIGMA: char = '\u{03c3}';
    if chars[i] == CAPITAL_SIGMA {
        out.push(if is_final_sigma(chars, i) {
            FINAL_SIGMA
        } else {
            SMALL_SIGMA
        });
    } else {
        out.extend(chars[i].to_lowercase());
    }
}

/// Append the Python full titlecase mapping (ToTitleFull) of `c`.
fn push_titled(c: char, out: &mut String) {
    use super::unicode_tables::title_mapping;
    match title_mapping(c) {
        Some(mapped) => out.push_str(mapped),
        None => out.push(c),
    }
}

pub fn title_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    use super::unicode_tables::{in_table, CASED};
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    // Python str.title (CPython do_title): titlecase a character when the
    // previous character is not cased, lowercase it otherwise. Word
    // boundaries are uncased characters — digits and punctuation both
    // restart a word ('1st' -> '1St'), and uncased letters like CJK
    // ideographs do too.
    // Check before collecting so the Vec<char> is never allocated past
    // the budget; char count <= byte count, so s.len() is an upper bound.
    ctx.check_memory(s.len().saturating_mul(std::mem::size_of::<char>()))?;
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len());
    let mut prev_cased = false;
    for i in 0..chars.len() {
        if prev_cased {
            push_lowered(&chars, i, &mut result);
        } else {
            push_titled(chars[i], &mut result);
        }
        prev_cased = in_table(CASED, chars[i]);
    }
    Ok(ExprValue::String(result))
}

pub fn capitalize_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = get_str(&a[0])?;
    ctx.count_string_ops(s.len())?;
    // Python str.capitalize: titlecase the first character (ToTitleFull —
    // uppercasing is wrong for titlecase digraphs like U+01C6), lowercase
    // the rest with Final_Sigma context.
    // Check before collecting so the Vec<char> is never allocated past
    // the budget; char count <= byte count, so s.len() is an upper bound.
    ctx.check_memory(s.len().saturating_mul(std::mem::size_of::<char>()))?;
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len());
    if !chars.is_empty() {
        push_titled(chars[0], &mut result);
        for i in 1..chars.len() {
            push_lowered(&chars, i, &mut result);
        }
    }
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
