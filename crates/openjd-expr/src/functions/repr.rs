// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Repr function implementations (repr_py, repr_json, repr_sh, repr_cmd, repr_pwsh).

use super::StringOutputBudget;
use crate::error::ExpressionError;
use crate::function_library::EvalContext;
use crate::value::ExprValue;

type R = Result<ExprValue, ExpressionError>;
type Ctx<'a> = &'a mut dyn EvalContext;

#[derive(Clone, Copy)]
enum ReprStyle {
    Py,
    Json,
    Sh,
    Cmd,
    Pwsh,
}

/// Every current repr escaping scheme expands one input byte to at most six
/// output bytes. Keep this deliberately broad: the budget model must not
/// duplicate the renderers' escape tables.
const MAX_ESCAPE_EXPANSION: usize = 6;

#[derive(Clone, Copy)]
enum ValueRef<'a> {
    Bool(bool),
    Int(i64),
    Float(&'a crate::value::Float64),
    String(&'a str),
    Path(&'a str),
    List(&'a ExprValue),
}

fn for_each_list_item<'a>(value: &'a ExprValue, mut f: impl FnMut(ValueRef<'a>)) {
    match value {
        ExprValue::ListBool(values) => {
            for value in values {
                f(ValueRef::Bool(*value));
            }
        }
        ExprValue::ListInt(values) => {
            for value in values {
                f(ValueRef::Int(*value));
            }
        }
        ExprValue::ListFloat(values) => {
            for value in values {
                f(ValueRef::Float(value));
            }
        }
        ExprValue::ListString(values, _) => {
            for value in values {
                f(ValueRef::String(value));
            }
        }
        ExprValue::ListPath(values, _, _) => {
            for value in values {
                f(ValueRef::Path(value));
            }
        }
        ExprValue::ListList(values, _, _) => {
            for value in values {
                f(ValueRef::List(value));
            }
        }
        _ => unreachable!("called with a non-list value"),
    }
}

// Phase 1: count list items without scanning string contents.

/// Count the total number of list items recursively. This is O(1) per list
/// node (reads `.len()` from the Vec), so it completes quickly even for large
/// nested structures. The result is charged to `count_ops` *before* any
/// character-scanning work begins.
fn count_list_items(value: &ExprValue) -> usize {
    match value {
        ExprValue::ListBool(v) => v.len(),
        ExprValue::ListInt(v) => v.len(),
        ExprValue::ListFloat(v) => v.len(),
        ExprValue::ListString(v, _) | ExprValue::ListPath(v, _, _) => v.len(),
        ExprValue::ListList(v, _, _) => v
            .len()
            .saturating_add(v.iter().map(count_list_items).sum::<usize>()),
        _ => 0,
    }
}

// Phase 2: compute deliberately conservative output bounds.

fn escaped_bound(input_bytes: usize) -> usize {
    input_bytes
        .saturating_mul(MAX_ESCAPE_EXPANSION)
        .saturating_add(2)
}

fn decimal_len(value: i64) -> usize {
    if value == 0 {
        1
    } else {
        let sign = usize::from(value < 0);
        sign + value.unsigned_abs().ilog10() as usize + 1
    }
}

fn display_bound_ref(value: ValueRef<'_>) -> usize {
    match value {
        ValueRef::Bool(true) => 4,
        ValueRef::Bool(false) => 5,
        ValueRef::Int(value) => decimal_len(value),
        ValueRef::Float(value) => value.display_len(),
        ValueRef::String(value) | ValueRef::Path(value) => value.len(),
        ValueRef::List(value) => display_bound(value),
    }
}

fn display_bound(value: &ExprValue) -> usize {
    match value {
        ExprValue::Null => 4,
        ExprValue::Bool(value) => {
            if *value {
                4
            } else {
                5
            }
        }
        ExprValue::Int(value) => decimal_len(*value),
        ExprValue::Float(value) => value.display_len(),
        ExprValue::String(value) | ExprValue::Path { value, .. } => value.len(),
        ExprValue::ListBool(_)
        | ExprValue::ListInt(_)
        | ExprValue::ListFloat(_)
        | ExprValue::ListString(_, _)
        | ExprValue::ListPath(_, _, _)
        | ExprValue::ListList(_, _, _) => {
            let len = value.list_len().unwrap_or(0);
            let mut total = 2usize.saturating_add(len.saturating_sub(1).saturating_mul(2));
            for_each_list_item(value, |item| {
                let item_bound = match item {
                    ValueRef::String(value) | ValueRef::Path(value) => {
                        value.len().saturating_add(2)
                    }
                    _ => display_bound_ref(item),
                };
                total = total.saturating_add(item_bound);
            });
            total
        }
        ExprValue::RangeExpr(range) => range.ranges().len().saturating_mul(64).saturating_add(2),
        ExprValue::Unresolved(_) => 256,
    }
}

fn output_bound_ref(value: ValueRef<'_>, style: ReprStyle) -> usize {
    match style {
        ReprStyle::Py | ReprStyle::Json => match value {
            ValueRef::String(value) | ValueRef::Path(value) => escaped_bound(value.len()),
            ValueRef::List(value) => output_bound(value, style),
            _ => display_bound_ref(value).saturating_add(2),
        },
        ReprStyle::Sh => match value {
            ValueRef::String(value) | ValueRef::Path(value) => escaped_bound(value.len()),
            _ => escaped_bound(display_bound_ref(value)),
        },
        ReprStyle::Cmd => match value {
            ValueRef::String(value) | ValueRef::Path(value) => escaped_bound(value.len()),
            _ => display_bound_ref(value),
        },
        ReprStyle::Pwsh => match value {
            ValueRef::String(value) | ValueRef::Path(value) => escaped_bound(value.len()),
            ValueRef::Bool(_) => 6,
            ValueRef::Int(value) => decimal_len(value),
            ValueRef::Float(value) => value.display_len(),
            ValueRef::List(value) => display_bound(value),
        },
    }
}

/// Compute an upper bound on output bytes. Called only after `count_ops` has
/// passed, so walking the strings and nested lists is budget-bounded.
fn output_bound(value: &ExprValue, style: ReprStyle) -> usize {
    match value {
        ExprValue::String(value) | ExprValue::Path { value, .. } => escaped_bound(value.len()),
        ExprValue::ListBool(_)
        | ExprValue::ListInt(_)
        | ExprValue::ListFloat(_)
        | ExprValue::ListString(_, _)
        | ExprValue::ListPath(_, _, _)
        | ExprValue::ListList(_, _, _) => {
            let len = value.list_len().unwrap_or(0);
            let (wrapper, separator) = match style {
                ReprStyle::Py | ReprStyle::Json => (2usize, 2usize),
                ReprStyle::Sh | ReprStyle::Cmd => (0, 1),
                ReprStyle::Pwsh => (3, 2),
            };
            let mut total = wrapper.saturating_add(len.saturating_sub(1).saturating_mul(separator));
            for_each_list_item(value, |item| {
                total = total.saturating_add(output_bound_ref(item, style));
            });
            total
        }
        ExprValue::Null => match style {
            ReprStyle::Py | ReprStyle::Json => 4,
            ReprStyle::Sh | ReprStyle::Cmd => 2,
            ReprStyle::Pwsh => 5,
        },
        ExprValue::Bool(value) => match (style, value) {
            (ReprStyle::Py, true) => 4,
            (ReprStyle::Py, false) => 5,
            (ReprStyle::Pwsh, true) => 5,
            (ReprStyle::Pwsh, false) => 6,
            (_, true) => 4,
            (_, false) => 5,
        },
        ExprValue::Int(value) => decimal_len(*value),
        ExprValue::Float(value) => value.display_len().saturating_add(2),
        ExprValue::RangeExpr(range) => range.ranges().len().saturating_mul(64).saturating_add(2),
        ExprValue::Unresolved(_) => 256,
    }
}

// Preflight: charge budgets in order.

/// Two-phase preflight:
/// 1. Count list items (O(1) per list node) and charge `count_ops` — this
///    rejects inputs with too many elements before any string scanning.
/// 2. Scan strings to compute output bound, charge `count_string_ops` and
///    `check_memory`.
fn preflight_repr(
    ctx: Ctx,
    value: &ExprValue,
    style: ReprStyle,
) -> Result<StringOutputBudget, ExpressionError> {
    let items = count_list_items(value);
    ctx.count_ops(items)?;
    let bound = output_bound(value, style);
    StringOutputBudget::reserve(ctx, bound, bound)
}

// ─── Public entry points ───

pub fn repr_py_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let budget = preflight_repr(ctx, &a[0], ReprStyle::Py)?;
    let mut buf = String::new();
    write_repr_py(&a[0], &mut buf);
    Ok(budget.finish(buf))
}

pub fn repr_json_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let budget = preflight_repr(ctx, &a[0], ReprStyle::Json)?;
    let mut buf = String::new();
    write_repr_json(&a[0], &mut buf);
    Ok(budget.finish(buf))
}

pub fn repr_sh_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let budget = preflight_repr(ctx, &a[0], ReprStyle::Sh)?;
    let result = repr_sh(&a[0])?;
    Ok(budget.finish(result))
}

pub fn repr_cmd_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let budget = preflight_repr(ctx, &a[0], ReprStyle::Cmd)?;
    let mut buf = String::new();
    write_repr_cmd(&a[0], &mut buf);
    Ok(budget.finish(buf))
}

pub fn repr_pwsh_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let budget = preflight_repr(ctx, &a[0], ReprStyle::Pwsh)?;
    let mut buf = String::new();
    write_repr_pwsh(&a[0], &mut buf);
    Ok(budget.finish(buf))
}

// Renderers write directly into a single buffer.

fn write_delimited_list(
    value: &ExprValue,
    buf: &mut String,
    prefix: &str,
    separator: &str,
    suffix: &str,
    write_item: fn(ValueRef<'_>, &mut String),
) {
    buf.push_str(prefix);
    let mut first = true;
    for_each_list_item(value, |item| {
        if !first {
            buf.push_str(separator);
        }
        first = false;
        write_item(item, buf);
    });
    buf.push_str(suffix);
}

fn write_float(value: &crate::value::Float64, buf: &mut String) {
    use std::fmt::Write;
    let _ = write!(buf, "{value}");
}

fn write_display_ref(value: ValueRef<'_>, buf: &mut String) {
    use std::fmt::Write;
    match value {
        ValueRef::Bool(value) => buf.push_str(if value { "true" } else { "false" }),
        ValueRef::Int(value) => {
            let _ = write!(buf, "{value}");
        }
        ValueRef::Float(value) => write_float(value, buf),
        ValueRef::String(value) | ValueRef::Path(value) => buf.push_str(value),
        ValueRef::List(value) => write_display(value, buf),
    }
}

fn write_display(value: &ExprValue, buf: &mut String) {
    use std::fmt::Write;
    match value {
        ExprValue::Null => buf.push_str("null"),
        ExprValue::Bool(value) => buf.push_str(if *value { "true" } else { "false" }),
        ExprValue::Int(value) => {
            let _ = write!(buf, "{value}");
        }
        ExprValue::Float(value) => write_float(value, buf),
        ExprValue::String(value) | ExprValue::Path { value, .. } => buf.push_str(value),
        ExprValue::ListString(_, _) | ExprValue::ListPath(_, _, _) => {
            write_delimited_list(value, buf, "[", ", ", "]", write_quoted_display_string)
        }
        ExprValue::ListBool(_)
        | ExprValue::ListInt(_)
        | ExprValue::ListFloat(_)
        | ExprValue::ListList(_, _, _) => {
            write_delimited_list(value, buf, "[", ", ", "]", write_display_ref)
        }
        ExprValue::RangeExpr(value) => {
            let _ = write!(buf, "{value}");
        }
        ExprValue::Unresolved(value) => {
            let _ = write!(buf, "<unresolved[{value}]>");
        }
    }
}

fn write_quoted_display_string(value: ValueRef<'_>, buf: &mut String) {
    match value {
        ValueRef::String(value) | ValueRef::Path(value) => {
            buf.push('"');
            buf.push_str(value);
            buf.push('"');
        }
        _ => unreachable!("called with a non-string list item"),
    }
}

fn write_repr_py(val: &ExprValue, buf: &mut String) {
    use std::fmt::Write;
    match val {
        ExprValue::String(value) | ExprValue::Path { value, .. } => {
            write_repr_py_string(value, buf)
        }
        ExprValue::Bool(value) => buf.push_str(if *value { "True" } else { "False" }),
        ExprValue::Null => buf.push_str("None"),
        ExprValue::Int(value) => {
            let _ = write!(buf, "{value}");
        }
        ExprValue::Float(value) => write_float(value, buf),
        ExprValue::ListBool(_)
        | ExprValue::ListInt(_)
        | ExprValue::ListFloat(_)
        | ExprValue::ListString(_, _)
        | ExprValue::ListPath(_, _, _)
        | ExprValue::ListList(_, _, _) => {
            write_delimited_list(val, buf, "[", ", ", "]", write_repr_py_ref);
        }
        ExprValue::RangeExpr(value) => {
            let _ = write!(buf, "'{value}'");
        }
        _ => buf.push_str(&val.to_display_string()),
    }
}

fn write_repr_py_string(value: &str, buf: &mut String) {
    buf.push('\'');
    for c in value.chars() {
        match c {
            '\\' => buf.push_str("\\\\"),
            '\'' => buf.push_str("\\'"),
            _ => buf.push(c),
        }
    }
    buf.push('\'');
}

fn write_repr_py_ref(value: ValueRef<'_>, buf: &mut String) {
    use std::fmt::Write;
    match value {
        ValueRef::Bool(value) => buf.push_str(if value { "True" } else { "False" }),
        ValueRef::Int(value) => {
            let _ = write!(buf, "{value}");
        }
        ValueRef::Float(value) => write_float(value, buf),
        ValueRef::String(value) | ValueRef::Path(value) => write_repr_py_string(value, buf),
        ValueRef::List(value) => write_repr_py(value, buf),
    }
}

fn write_repr_json(val: &ExprValue, buf: &mut String) {
    use std::fmt::Write;
    match val {
        ExprValue::String(value) | ExprValue::Path { value, .. } => {
            write_repr_json_string(value, buf)
        }
        ExprValue::Bool(value) => buf.push_str(if *value { "true" } else { "false" }),
        ExprValue::Null => buf.push_str("null"),
        ExprValue::Int(value) => {
            let _ = write!(buf, "{value}");
        }
        ExprValue::Float(value) => write_float(value, buf),
        ExprValue::ListBool(_)
        | ExprValue::ListInt(_)
        | ExprValue::ListFloat(_)
        | ExprValue::ListString(_, _)
        | ExprValue::ListPath(_, _, _)
        | ExprValue::ListList(_, _, _) => {
            write_delimited_list(val, buf, "[", ", ", "]", write_repr_json_ref);
        }
        ExprValue::RangeExpr(value) => {
            let _ = write!(buf, "\"{value}\"");
        }
        _ => buf.push_str(&val.to_display_string()),
    }
}

fn write_repr_json_string(value: &str, buf: &mut String) {
    buf.push('"');
    write_json_escape(value, buf);
    buf.push('"');
}

fn write_repr_json_ref(value: ValueRef<'_>, buf: &mut String) {
    use std::fmt::Write;
    match value {
        ValueRef::Bool(value) => buf.push_str(if value { "true" } else { "false" }),
        ValueRef::Int(value) => {
            let _ = write!(buf, "{value}");
        }
        ValueRef::Float(value) => write_float(value, buf),
        ValueRef::String(value) | ValueRef::Path(value) => write_repr_json_string(value, buf),
        ValueRef::List(value) => write_repr_json(value, buf),
    }
}

/// Escape a string for JSON output, matching Python's `json.dumps(ensure_ascii=True)`.
/// All non-ASCII characters are encoded as `\uXXXX` (with surrogate pairs for chars > U+FFFF).
fn write_json_escape(s: &str, buf: &mut String) {
    use std::fmt::Write;
    for c in s.chars() {
        match c {
            '"' => buf.push_str("\\\""),
            '\\' => buf.push_str("\\\\"),
            '\x08' => buf.push_str("\\b"),
            '\x0c' => buf.push_str("\\f"),
            '\n' => buf.push_str("\\n"),
            '\r' => buf.push_str("\\r"),
            '\t' => buf.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(buf, "\\u{:04x}", c as u32);
            }
            c if c.is_ascii() => buf.push(c),
            c => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    let _ = write!(buf, "\\u{:04x}", unit);
                }
            }
        }
    }
}

fn repr_sh(val: &ExprValue) -> Result<String, ExpressionError> {
    match val {
        ExprValue::String(value) | ExprValue::Path { value, .. } => shlex_quote(value),
        ExprValue::Bool(value) => Ok(if *value { "true" } else { "false" }.to_string()),
        ExprValue::Null => Ok("''".to_string()),
        ExprValue::Int(value) => Ok(value.to_string()),
        ExprValue::Float(value) => Ok(value.to_display_string()),
        ExprValue::ListBool(_)
        | ExprValue::ListInt(_)
        | ExprValue::ListFloat(_)
        | ExprValue::ListString(_, _)
        | ExprValue::ListPath(_, _, _)
        | ExprValue::ListList(_, _, _) => {
            let mut buf = String::new();
            write_repr_sh_list(val, &mut buf)?;
            Ok(buf)
        }
        _ => Ok(val.to_display_string()),
    }
}

fn write_repr_sh_list(value: &ExprValue, buf: &mut String) -> Result<(), ExpressionError> {
    let mut first = true;
    let mut result = Ok(());
    for_each_list_item(value, |item| {
        if result.is_ok() {
            if !first {
                buf.push(' ');
            }
            first = false;
            result = write_repr_sh_ref(item, buf);
        }
    });
    result
}

fn write_repr_sh_ref(value: ValueRef<'_>, buf: &mut String) -> Result<(), ExpressionError> {
    match value {
        ValueRef::String(value) | ValueRef::Path(value) => shlex_quote_into(value, buf),
        ValueRef::List(value) => {
            let mut display = String::new();
            write_display(value, &mut display);
            let quoted = shlex::try_quote(&display)
                .map_err(|error| ExpressionError::new(format!("Cannot shell-quote list: {error}")))?
                .into_owned();
            drop(display);
            buf.push_str(&quoted);
            Ok(())
        }
        _ => {
            write_display_ref(value, buf);
            Ok(())
        }
    }
}

/// Shell-quote a single string, returning an error on null bytes.
fn shlex_quote(value: &str) -> Result<String, ExpressionError> {
    shlex::try_quote(value)
        .map(|quoted| quoted.into_owned())
        .map_err(|error| ExpressionError::new(format!("Cannot shell-quote string: {error}")))
}

fn shlex_quote_into(value: &str, buf: &mut String) -> Result<(), ExpressionError> {
    let quoted = shlex::try_quote(value)
        .map_err(|error| ExpressionError::new(format!("Cannot shell-quote list: {error}")))?;
    buf.push_str(&quoted);
    Ok(())
}

fn write_repr_cmd(val: &ExprValue, buf: &mut String) {
    use std::fmt::Write;
    match val {
        ExprValue::String(value) | ExprValue::Path { value, .. } => cmd_quote_into(value, buf),
        ExprValue::Bool(value) => buf.push_str(if *value { "true" } else { "false" }),
        ExprValue::Null => buf.push_str("\"\""),
        ExprValue::Int(value) => {
            let _ = write!(buf, "{value}");
        }
        ExprValue::Float(value) => write_float(value, buf),
        ExprValue::ListBool(_)
        | ExprValue::ListInt(_)
        | ExprValue::ListFloat(_)
        | ExprValue::ListString(_, _)
        | ExprValue::ListPath(_, _, _)
        | ExprValue::ListList(_, _, _) => {
            write_delimited_list(val, buf, "", " ", "", write_repr_cmd_ref);
        }
        _ => write_display(val, buf),
    }
}

fn write_repr_cmd_ref(value: ValueRef<'_>, buf: &mut String) {
    match value {
        ValueRef::String(value) | ValueRef::Path(value) => cmd_quote_into(value, buf),
        _ => write_display_ref(value, buf),
    }
}

fn write_repr_pwsh(val: &ExprValue, buf: &mut String) {
    use std::fmt::Write;
    match val {
        ExprValue::String(value) | ExprValue::Path { value, .. } => {
            write_repr_pwsh_string(value, buf)
        }
        ExprValue::Bool(value) => buf.push_str(if *value { "$true" } else { "$false" }),
        ExprValue::Null => buf.push_str("$null"),
        ExprValue::Int(value) => {
            let _ = write!(buf, "{value}");
        }
        ExprValue::Float(value) => write_float(value, buf),
        ExprValue::ListBool(_)
        | ExprValue::ListInt(_)
        | ExprValue::ListFloat(_)
        | ExprValue::ListString(_, _)
        | ExprValue::ListPath(_, _, _)
        | ExprValue::ListList(_, _, _) => {
            write_delimited_list(val, buf, "@(", ", ", ")", write_repr_pwsh_ref);
        }
        ExprValue::RangeExpr(value) => {
            let _ = write!(buf, "'{value}'");
        }
        _ => write_display(val, buf),
    }
}

fn write_repr_pwsh_string(value: &str, buf: &mut String) {
    buf.push('\'');
    for c in value.chars() {
        if c == '\'' {
            buf.push_str("''");
        } else {
            buf.push(c);
        }
    }
    buf.push('\'');
}

fn write_repr_pwsh_ref(value: ValueRef<'_>, buf: &mut String) {
    use std::fmt::Write;
    match value {
        ValueRef::String(value) | ValueRef::Path(value) => write_repr_pwsh_string(value, buf),
        ValueRef::Bool(value) => buf.push_str(if value { "$true" } else { "$false" }),
        ValueRef::Int(value) => {
            let _ = write!(buf, "{value}");
        }
        ValueRef::Float(value) => write_float(value, buf),
        ValueRef::List(value) => write_display(value, buf),
    }
}

/// Quote a string for cmd.exe, writing directly into the buffer.
fn cmd_quote_into(s: &str, buf: &mut String) {
    const NEEDS_QUOTING: &str = " \t&|<>^\"()%!";
    // Strip newlines first (cmd.exe has no escape for literal newlines),
    // then decide quoting based on the stripped content.
    let has_newlines = s.chars().any(|c| c == '\n' || c == '\r');
    let has_special = s
        .chars()
        .any(|c| c != '\n' && c != '\r' && NEEDS_QUOTING.contains(c));
    let stripped_empty = !s.chars().any(|c| c != '\n' && c != '\r');
    // An empty result (original was empty or all-newlines) gets quoted.
    if (s.is_empty() || stripped_empty) || has_special {
        buf.push('"');
        for c in s.chars() {
            match c {
                '\n' | '\r' => {}
                '^' | '"' => {
                    buf.push('^');
                    buf.push(c);
                }
                '%' => buf.push_str("%%"),
                '!' => buf.push_str("^^!"),
                c => buf.push(c),
            }
        }
        buf.push('"');
    } else if has_newlines {
        // Has non-special content plus newlines — strip newlines, no quoting.
        for c in s.chars() {
            if c != '\n' && c != '\r' {
                buf.push(c);
            }
        }
    } else {
        // No newlines, no special chars — pass through.
        buf.push_str(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ExprType;

    /// Helper that computes output_bound (the value charged to check_memory)
    /// and verifies it covers the actual rendered output.
    fn check_bound(value: &ExprValue) {
        let styles = [
            (ReprStyle::Py, "py"),
            (ReprStyle::Json, "json"),
            (ReprStyle::Cmd, "cmd"),
            (ReprStyle::Pwsh, "pwsh"),
        ];
        for (style, name) in styles {
            let bound = output_bound(value, style);
            let mut buf = String::new();
            match style {
                ReprStyle::Py => write_repr_py(value, &mut buf),
                ReprStyle::Json => write_repr_json(value, &mut buf),
                ReprStyle::Cmd => write_repr_cmd(value, &mut buf),
                ReprStyle::Pwsh => write_repr_pwsh(value, &mut buf),
                ReprStyle::Sh => {}
            }
            assert!(
                bound >= buf.len(),
                "repr_{name}: bound {bound} < rendered {} for {buf:?}",
                buf.len()
            );
        }
        // sh is fallible, check separately
        let sh_bound = output_bound(value, ReprStyle::Sh);
        if let Ok(sh_out) = repr_sh(value) {
            assert!(
                sh_bound >= sh_out.len(),
                "repr_sh: bound {sh_bound} < rendered {} for {sh_out:?}",
                sh_out.len()
            );
        }
    }

    #[test]
    fn preflight_output_bounds_cover_rendered_representations() {
        let inner = ExprValue::make_list(
            vec![
                ExprValue::String("plain".into()),
                ExprValue::String("quote'\\\n\u{1f600}".into()),
            ],
            ExprType::STRING,
        )
        .unwrap();
        let value = ExprValue::make_list(
            vec![inner, ExprValue::String("!%^\"".into())],
            ExprType::list(ExprType::STRING),
        )
        .unwrap();
        check_bound(&value);
        check_bound(&ExprValue::String("^".repeat(1000)));
        check_bound(&ExprValue::String("\\".repeat(1000)));

        let nested_carets = ExprValue::make_list(
            vec![
                ExprValue::make_list(vec![ExprValue::String("^".repeat(1000))], ExprType::STRING)
                    .unwrap(),
            ],
            ExprType::list(ExprType::STRING),
        )
        .unwrap();
        check_bound(&nested_carets);
    }

    #[test]
    fn scalar_bounds() {
        check_bound(&ExprValue::Int(0));
        check_bound(&ExprValue::Int(-1234567890));
        check_bound(&ExprValue::Bool(true));
        check_bound(&ExprValue::Null);
    }
}
