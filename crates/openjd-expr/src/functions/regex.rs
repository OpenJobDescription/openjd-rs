// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Regex function implementations.

use crate::error::ExpressionError;
use crate::function_library::EvalContext;
use crate::types::ExprType;
use crate::value::ExprValue;

type R = Result<ExprValue, ExpressionError>;
type Ctx<'a> = &'a mut dyn EvalContext;

fn get_two_strings(a: &[ExprValue], name: &str) -> Result<(String, String), ExpressionError> {
    let s = match &a[0] {
        ExprValue::String(s) => s.clone(),
        _ => {
            return Err(ExpressionError::new(format!(
                "{name}() requires string arguments"
            )))
        }
    };
    let p = match &a[1] {
        ExprValue::String(s) => s.clone(),
        _ => {
            return Err(ExpressionError::new(format!(
                "{name}() requires string arguments"
            )))
        }
    };
    Ok((s, p))
}

fn validate_regex_pattern(pattern: &str) -> Result<(), ExpressionError> {
    if pattern.is_empty() {
        return Err(ExpressionError::new("Empty regex pattern is not allowed"));
    }

    // Reject Rust-only constructs that `regex_syntax` happily parses but
    // that the spec (§2.2.5) forbids for Python/Rust cross-engine
    // compatibility:
    //   - `\z`           — Rust-only end-of-string anchor
    //   - `\x{HHHH}`     — Rust-only Unicode brace syntax
    //   - `\u{HHHH}`     — Rust-only Unicode brace syntax
    //   - `\U{HHHH}`     — Rust-only Unicode brace syntax
    //
    // The source scan walks the pattern character by character, tracking
    // whether a backslash is active (i.e., the previous char was an
    // unescaped `\`). This correctly handles escape parity — e.g.,
    // `\\z` contains a literal backslash followed by a `z`, not the `\z`
    // anchor — without re-implementing the full regex grammar.
    reject_rust_only_features(pattern)?;

    // Parse the pattern into its HIR. Using `regex_syntax` (instead of a
    // pure substring scan) means we correctly ignore lookaround-shaped
    // sequences inside character classes, escapes, or regex comments.
    // `regex_syntax` rejects lookaround, backreferences, and `\Z` at
    // parse time; the HIR walker below is a belt-and-braces guard.
    let hir = match regex_syntax::Parser::new().parse(pattern) {
        Ok(h) => h,
        Err(e) => return Err(translate_parse_error(e)),
    };
    check_hir_portability(&hir)
}

/// Scan the pattern source for Rust-only escape sequences that
/// `regex_syntax` accepts but the spec forbids. Respects backslash-escape
/// parity so `\\z` (literal backslash + `z`) isn't mistaken for the `\z`
/// anchor.
fn reject_rust_only_features(pattern: &str) -> Result<(), ExpressionError> {
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            // Consume the backslash; the next byte (if any) is escaped.
            let next = match bytes.get(i + 1) {
                Some(&b) => b,
                None => break, // trailing backslash — let regex_syntax diagnose
            };
            match next {
                b'z' => {
                    return Err(ExpressionError::new(
                        "Unsupported regex feature: end-of-string anchor \\z",
                    ));
                }
                // `\x{`, `\u{`, `\U{` are the Rust-only Unicode brace
                // escapes. Plain `\xHH`, `\uHHHH`, `\UHHHHHHHH` without
                // braces are portable and must still pass.
                b'x' | b'u' | b'U' => {
                    if matches!(bytes.get(i + 2), Some(b'{')) {
                        return Err(ExpressionError::new(format!(
                            "Unsupported regex feature: Unicode brace syntax \\{}{{...}}",
                            next as char
                        )));
                    }
                }
                _ => {}
            }
            // Skip the backslash *and* the escaped character so a later
            // `\` in this iteration's tail isn't mistaken for a fresh
            // escape introducer.
            i += 2;
        } else {
            i += 1;
        }
    }
    Ok(())
}

/// Translate a `regex_syntax` parse error into a stable user-facing message
/// that names the offending feature where possible.
fn translate_parse_error(err: regex_syntax::Error) -> ExpressionError {
    let msg = err.to_string();
    let lower = msg.to_lowercase();
    // Detect specific unsupported features and use canonical names. The
    // substring checks are on the formatted error message; message text is
    // stable given the regex-syntax version pin in Cargo.toml.
    let feature = if lower.contains("look-around") || lower.contains("lookaround") {
        // regex-syntax reports all four lookaround variants as "look-around";
        // distinguish via the pattern flavor surfaced in the message.
        if lower.contains("negative lookahead") || lower.contains("(?!") {
            "negative lookahead"
        } else if lower.contains("positive lookahead") || lower.contains("(?=") {
            "lookahead"
        } else if lower.contains("negative lookbehind") || lower.contains("(?<!") {
            "negative lookbehind"
        } else if lower.contains("positive lookbehind") || lower.contains("(?<=") {
            "lookbehind"
        } else {
            "look-around"
        }
    } else if lower.contains("backreference") || lower.contains("back reference") {
        "backreferences"
    } else if lower.contains("unrecognized escape") && msg.contains("\\Z") {
        // `\Z` is a Python-style end-of-string anchor. The Rust regex
        // grammar does not support it and reports it as an unknown escape.
        // (The complementary `\z` case is rejected earlier by
        // `reject_rust_only_features`.)
        "end-of-string anchor \\Z"
    } else {
        return ExpressionError::new(format!("Invalid regex pattern: {msg}"));
    };
    ExpressionError::new(format!("Unsupported regex feature: {feature}"))
}

/// Walk the parsed HIR and reject forbidden constructs that `regex_syntax`
/// itself accepts.
///
/// `regex_syntax` already rejects lookaround, backreferences, and `\Z`/`\z`
/// at parse time, so in practice this walker is a belt-and-braces guard for
/// any future grammar additions that expose these constructs via HIR.
fn check_hir_portability(hir: &regex_syntax::hir::Hir) -> Result<(), ExpressionError> {
    use regex_syntax::hir::{HirKind, Look};
    match hir.kind() {
        HirKind::Look(l) => match l {
            // These anchors are in the spec's allowed subset.
            Look::Start
            | Look::End
            | Look::StartLF
            | Look::EndLF
            | Look::StartCRLF
            | Look::EndCRLF
            | Look::WordAscii
            | Look::WordAsciiNegate
            | Look::WordUnicode
            | Look::WordUnicodeNegate
            | Look::WordStartAscii
            | Look::WordEndAscii
            | Look::WordStartUnicode
            | Look::WordEndUnicode
            | Look::WordStartHalfAscii
            | Look::WordEndHalfAscii
            | Look::WordStartHalfUnicode
            | Look::WordEndHalfUnicode => Ok(()),
        },
        HirKind::Capture(c) => check_hir_portability(&c.sub),
        HirKind::Repetition(r) => check_hir_portability(&r.sub),
        HirKind::Concat(parts) | HirKind::Alternation(parts) => {
            for p in parts {
                check_hir_portability(p)?;
            }
            Ok(())
        }
        HirKind::Empty | HirKind::Literal(_) | HirKind::Class(_) => Ok(()),
    }
}

pub fn re_escape_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = match &a[0] {
        ExprValue::String(s) => s.clone(),
        _ => return Err(ExpressionError::new("re_escape() requires string")),
    };
    ctx.count_string_ops(s.len())?;
    Ok(ExprValue::String(regex::escape(&s)))
}

pub fn re_match_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (s, pat) = get_two_strings(a, "re_match")?;
    ctx.count_string_ops(s.len())?;
    validate_regex_pattern(&pat)?;
    let re = ctx.get_or_compile_regex(&format!("^(?:{})", pat))?;
    match re.captures(&s) {
        None => Ok(ExprValue::Null),
        Some(caps) => {
            let groups: Vec<ExprValue> = (0..caps.len())
                .map(|i| {
                    ExprValue::String(
                        caps.get(i)
                            .map(|m| m.as_str().to_string())
                            .unwrap_or_default(),
                    )
                })
                .collect();
            Ok(ExprValue::make_list_checked(ctx, groups, ExprType::STRING)?)
        }
    }
}

pub fn re_search_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (s, pat) = get_two_strings(a, "re_search")?;
    ctx.count_string_ops(s.len())?;
    validate_regex_pattern(&pat)?;
    let re = ctx.get_or_compile_regex(&pat)?;
    match re.captures(&s) {
        None => Ok(ExprValue::Null),
        Some(caps) => {
            let groups: Vec<ExprValue> = (0..caps.len())
                .map(|i| {
                    ExprValue::String(
                        caps.get(i)
                            .map(|m| m.as_str().to_string())
                            .unwrap_or_default(),
                    )
                })
                .collect();
            Ok(ExprValue::make_list_checked(ctx, groups, ExprType::STRING)?)
        }
    }
}

pub fn re_findall_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (s, pat) = get_two_strings(a, "re_findall")?;
    ctx.count_string_ops(s.len())?;
    validate_regex_pattern(&pat)?;
    let re = ctx.get_or_compile_regex(&pat)?;
    let num_groups = re.captures_len() - 1;
    if num_groups == 0 {
        let matches: Vec<ExprValue> = re
            .find_iter(&s)
            .map(|m| ExprValue::String(m.as_str().to_string()))
            .collect();
        Ok(ExprValue::make_list_checked(
            ctx,
            matches,
            ExprType::STRING,
        )?)
    } else if num_groups == 1 {
        let matches: Vec<ExprValue> = re
            .captures_iter(&s)
            .map(|c| {
                ExprValue::String(c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default())
            })
            .collect();
        Ok(ExprValue::make_list_checked(
            ctx,
            matches,
            ExprType::STRING,
        )?)
    } else {
        // Build inner lists without per-element ctx-checking: each captures
        // row has a bounded number of groups (num_groups, fixed by the
        // pattern), so the inner allocations are small. The outer list is
        // the one that scales with the input; check memory on it.
        let matches: Result<Vec<ExprValue>, _> = re
            .captures_iter(&s)
            .map(|c| {
                let groups: Vec<ExprValue> = (1..=num_groups)
                    .map(|i| {
                        ExprValue::String(
                            c.get(i).map(|m| m.as_str().to_string()).unwrap_or_default(),
                        )
                    })
                    .collect();
                ExprValue::make_list(groups, ExprType::STRING)
            })
            .collect();
        Ok(ExprValue::make_list_checked(
            ctx,
            matches?,
            ExprType::list(ExprType::STRING),
        )?)
    }
}

pub fn re_replace_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    if a.len() != 3 {
        return Err(ExpressionError::new("re_replace() takes 3 arguments"));
    }
    let s = match &a[0] {
        ExprValue::String(s) => s.clone(),
        _ => return Err(ExpressionError::new("re_replace() requires strings")),
    };
    ctx.count_string_ops(s.len())?;
    let pat = match &a[1] {
        ExprValue::String(s) => s.clone(),
        _ => return Err(ExpressionError::new("re_replace() requires strings")),
    };
    let repl = match &a[2] {
        ExprValue::String(s) => s.clone(),
        _ => return Err(ExpressionError::new("re_replace() requires strings")),
    };
    validate_regex_pattern(&pat)?;
    validate_regex_replacement(&repl)?;
    let re = ctx.get_or_compile_regex(&pat)?;
    let result = re.replace_all(&s, regex::NoExpand(&repl));
    Ok(ExprValue::String(result.into_owned()))
}

fn validate_regex_replacement(repl: &str) -> Result<(), ExpressionError> {
    let bytes = repl.as_bytes();
    for i in 0..bytes.len() {
        // Check for \1-\9 and \g<...> backreferences
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            if bytes[i + 1].is_ascii_digit() {
                return Err(ExpressionError::new(
                    "Group references in replacement strings are not supported",
                ));
            }
            if bytes[i + 1] == b'g' && i + 2 < bytes.len() && bytes[i + 2] == b'<' {
                return Err(ExpressionError::new(
                    "Group references in replacement strings are not supported",
                ));
            }
        }
        // Check for $1, $2, ${1}, ${name} references
        if bytes[i] == b'$'
            && i + 1 < bytes.len()
            && (bytes[i + 1].is_ascii_digit() || bytes[i + 1] == b'{')
        {
            return Err(ExpressionError::new(
                "Group references in replacement strings are not supported",
            ));
        }
    }
    Ok(())
}

pub fn re_split_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    if a.len() < 2 || a.len() > 3 {
        return Err(ExpressionError::new("re_split() takes 2-3 arguments"));
    }
    let (s, pat) = get_two_strings(a, "re_split")?;
    ctx.count_string_ops(s.len())?;
    validate_regex_pattern(&pat)?;
    let maxsplit = a.get(2).and_then(|v| match v {
        ExprValue::Int(n) => Some(*n as usize),
        _ => None,
    });
    let re = ctx.get_or_compile_regex(&pat)?;
    let parts: Vec<ExprValue> = match maxsplit {
        Some(n) => re
            .splitn(&s, n + 1)
            .map(|p| ExprValue::String(p.to_string()))
            .collect(),
        None => re
            .split(&s)
            .map(|p| ExprValue::String(p.to_string()))
            .collect(),
    };
    ExprValue::make_list(parts, ExprType::STRING)
}
