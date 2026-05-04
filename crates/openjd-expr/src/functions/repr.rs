// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Repr function implementations (repr_py, repr_json, repr_sh, repr_cmd, repr_pwsh).

use crate::error::ExpressionError;
use crate::function_library::EvalContext;
use crate::value::ExprValue;

type R = Result<ExprValue, ExpressionError>;
type Ctx<'a> = &'a mut dyn EvalContext;

fn repr_string_len(a: &ExprValue) -> usize {
    match a {
        ExprValue::String(s) => s.len(),
        ExprValue::Path { value, .. } => value.len(),
        _ => 0,
    }
}

pub fn repr_py_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    ctx.count_string_ops(repr_string_len(&a[0]))?;
    Ok(ExprValue::String(repr_py(&a[0])))
}

pub fn repr_json_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    ctx.count_string_ops(repr_string_len(&a[0]))?;
    Ok(ExprValue::String(repr_json(&a[0])))
}

pub fn repr_sh_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    if a[0].is_list() {
        ctx.count_ops(a[0].list_len().unwrap_or(0))?;
    }
    ctx.count_string_ops(repr_string_len(&a[0]))?;
    repr_sh(&a[0]).map(ExprValue::String)
}

pub fn repr_cmd_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    ctx.count_string_ops(repr_string_len(&a[0]))?;
    Ok(ExprValue::String(repr_cmd(&a[0])))
}

pub fn repr_pwsh_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    ctx.count_string_ops(repr_string_len(&a[0]))?;
    Ok(ExprValue::String(repr_pwsh(&a[0])))
}

fn repr_py(val: &ExprValue) -> String {
    match val {
        ExprValue::String(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
        ExprValue::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        ExprValue::Null => "None".to_string(),
        ExprValue::Int(i) => i.to_string(),
        ExprValue::Float(f) => {
            if f.value().fract() == 0.0 {
                format!("{:.1}", f)
            } else {
                f.to_string()
            }
        }
        val if val.is_list() => {
            let iter = val.list_iter().expect("is_list() was true");
            format!(
                "[{}]",
                iter.map(|e| repr_py(&e)).collect::<Vec<_>>().join(", ")
            )
        }
        ExprValue::Path { value, .. } => {
            format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
        }
        ExprValue::RangeExpr(r) => format!("'{r}'"),
        _ => val.to_display_string(),
    }
}

/// Escape a string for JSON output, matching Python's `json.dumps(ensure_ascii=True)`.
/// All non-ASCII characters are encoded as `\uXXXX` (with surrogate pairs for chars > U+FFFF).
fn json_escape(s: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c if c.is_ascii() => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    let _ = write!(out, "\\u{:04x}", unit);
                }
            }
        }
    }
    out
}

fn repr_json(val: &ExprValue) -> String {
    match val {
        ExprValue::String(s) => format!("\"{}\"", json_escape(s)),
        ExprValue::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        ExprValue::Null => "null".to_string(),
        ExprValue::Int(i) => i.to_string(),
        ExprValue::Float(f) => {
            if f.value().fract() == 0.0 {
                format!("{:.1}", f)
            } else {
                f.to_string()
            }
        }
        val if val.is_list() => {
            let iter = val.list_iter().expect("guard ensures list");
            format!(
                "[{}]",
                iter.map(|e| repr_json(&e)).collect::<Vec<_>>().join(", ")
            )
        }
        ExprValue::Path { value, .. } => format!("\"{}\"", json_escape(value)),
        ExprValue::RangeExpr(r) => format!("\"{r}\""),
        _ => val.to_display_string(),
    }
}

fn repr_sh(val: &ExprValue) -> Result<String, ExpressionError> {
    match val {
        ExprValue::String(s) => shlex_quote(s),
        ExprValue::Bool(b) => Ok(if *b { "true" } else { "false" }.to_string()),
        ExprValue::Null => Ok("''".to_string()),
        ExprValue::Int(i) => Ok(i.to_string()),
        ExprValue::Float(f) => Ok(if f.value().fract() == 0.0 {
            format!("{:.1}", f)
        } else {
            f.to_string()
        }),
        ExprValue::Path { value, .. } => shlex_quote(value),
        val if val.is_list() => {
            let strs: Vec<String> = val
                .list_iter()
                .expect("guard ensures list")
                .map(|e| match &e {
                    ExprValue::String(s) | ExprValue::Path { value: s, .. } => s.clone(),
                    other => other.to_display_string(),
                })
                .collect();
            shlex::try_join(strs.iter().map(|s| s.as_str()))
                .map_err(|e| ExpressionError::new(format!("Cannot shell-quote list: {e}")))
        }
        _ => Ok(val.to_display_string()),
    }
}

/// Shell-quote a single string, returning an error on null bytes.
fn shlex_quote(s: &str) -> Result<String, ExpressionError> {
    shlex::try_quote(s)
        .map(|c| c.into_owned())
        .map_err(|e| ExpressionError::new(format!("Cannot shell-quote string: {e}")))
}

fn repr_cmd(val: &ExprValue) -> String {
    match val {
        ExprValue::String(s) => cmd_quote(s),
        ExprValue::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        ExprValue::Null => "\"\"".to_string(),
        ExprValue::Int(i) => i.to_string(),
        ExprValue::Float(f) => {
            if f.value().fract() == 0.0 {
                format!("{:.1}", f)
            } else {
                f.to_string()
            }
        }
        ExprValue::Path { value, .. } => cmd_quote(value),
        val if val.is_list() => val
            .list_iter()
            .expect("guard ensures list")
            .map(|e| match &e {
                ExprValue::String(s) | ExprValue::Path { value: s, .. } => cmd_quote(s),
                other => other.to_display_string(),
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => val.to_display_string(),
    }
}

fn repr_pwsh(val: &ExprValue) -> String {
    match val {
        ExprValue::String(s) => format!("'{}'", s.replace('\'', "''")),
        ExprValue::Bool(b) => if *b { "$true" } else { "$false" }.to_string(),
        ExprValue::Null => "$null".to_string(),
        ExprValue::Int(i) => i.to_string(),
        ExprValue::Float(f) => {
            if f.value().fract() == 0.0 {
                format!("{:.1}", f)
            } else {
                f.to_string()
            }
        }
        ExprValue::Path { value, .. } => format!("'{}'", value.replace('\'', "''")),
        val if val.is_list() => {
            let items: Vec<String> = val
                .list_iter()
                .expect("guard ensures list")
                .map(|e| match &e {
                    ExprValue::String(s) | ExprValue::Path { value: s, .. } => {
                        format!("'{}'", s.replace('\'', "''"))
                    }
                    ExprValue::Bool(b) => if *b { "$true" } else { "$false" }.to_string(),
                    ExprValue::Null => "$null".to_string(),
                    ExprValue::Int(i) => i.to_string(),
                    other => other.to_display_string(),
                })
                .collect();
            format!("@({})", items.join(", "))
        }
        ExprValue::RangeExpr(r) => format!("'{r}'"),
        _ => val.to_display_string(),
    }
}

fn cmd_quote(s: &str) -> String {
    // Strip newlines: cmd.exe has no escape for a literal newline inside a quoted argument,
    // and an embedded newline would cause cmd.exe to treat the remainder as a new command
    // (a command injection vector). See EXPR specification §2.2.6.
    let stripped: String = s.chars().filter(|c| *c != '\n' && *c != '\r').collect();
    const NEEDS_QUOTING: &str = " \t&|<>^\"()%!";
    if !stripped.is_empty() && !stripped.chars().any(|c| NEEDS_QUOTING.contains(c)) {
        return stripped;
    }
    let mut escaped = String::with_capacity(stripped.len());
    for c in stripped.chars() {
        match c {
            '^' | '"' => {
                escaped.push('^');
                escaped.push(c);
            }
            '%' => escaped.push_str("%%"),
            // Escape ! as ^^! for EnableDelayedExpansion contexts: cmd.exe processes ^ escapes
            // before delayed expansion, so a single ^ is consumed and leaves a bare !.
            '!' => escaped.push_str("^^!"),
            c => escaped.push(c),
        }
    }
    format!("\"{escaped}\"")
}
