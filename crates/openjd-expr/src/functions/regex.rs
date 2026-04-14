// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Regex function implementations.

use crate::error::ExpressionError;
use crate::function_library::EvalContext;
use crate::types::ExprType;
use crate::value::ExprValue;

type R = Result<ExprValue, ExpressionError>;
type Ctx<'a> = &'a mut dyn EvalContext;

fn get_two_strings(a: &[ExprValue], name: &str) -> Result<(String, String), ExpressionError> {
    let s = match &a[0] { ExprValue::String(s) => s.clone(), _ => return Err(ExpressionError::new(format!("{name}() requires string arguments"))) };
    let p = match &a[1] { ExprValue::String(s) => s.clone(), _ => return Err(ExpressionError::new(format!("{name}() requires string arguments"))) };
    Ok((s, p))
}

fn validate_regex_pattern(pattern: &str) -> Result<(), ExpressionError> {
    if pattern.is_empty() { return Err(ExpressionError::new("Empty regex pattern is not allowed")); }
    // Reject non-portable features
    let unsupported: &[(&str, &str)] = &[
        ("(?=", "lookahead"), ("(?!", "negative lookahead"),
        ("(?<=", "lookbehind"), ("(?<!", "negative lookbehind"),
        ("(?(", "conditional patterns"), ("(?P=", "named backreferences"),
    ];
    for (seq, name) in unsupported {
        if pattern.contains(seq) {
            return Err(ExpressionError::new(format!("Unsupported regex feature: {name}")));
        }
    }
    // Reject backreferences and end-of-string anchors (check for unescaped backslash)
    for (seq, name) in &[
        ("\\1", "backreferences"), ("\\2", "backreferences"), ("\\3", "backreferences"),
        ("\\4", "backreferences"), ("\\5", "backreferences"),
        ("\\Z", "end-of-string anchor \\Z"), ("\\z", "end-of-string anchor \\z"),
    ] {
        let mut idx = 0;
        while let Some(pos) = pattern[idx..].find(seq) {
            let abs_pos = idx + pos;
            let num_preceding = pattern[..abs_pos].chars().rev().take_while(|&c| c == '\\').count();
            if num_preceding % 2 == 0 {
                return Err(ExpressionError::new(format!("Unsupported regex feature: {name}")));
            }
            idx = abs_pos + 1;
        }
    }
    Ok(())
}

pub fn re_escape_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let s = match &a[0] { ExprValue::String(s) => s.clone(), _ => return Err(ExpressionError::new("re_escape() requires string")) };
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
            let groups: Vec<ExprValue> = (0..caps.len()).map(|i| ExprValue::String(caps.get(i).map(|m| m.as_str().to_string()).unwrap_or_default())).collect();
            Ok(ExprValue::make_list(groups, ExprType::STRING)?)
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
            let groups: Vec<ExprValue> = (0..caps.len()).map(|i| ExprValue::String(caps.get(i).map(|m| m.as_str().to_string()).unwrap_or_default())).collect();
            Ok(ExprValue::make_list(groups, ExprType::STRING)?)
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
        let matches: Vec<ExprValue> = re.find_iter(&s).map(|m| ExprValue::String(m.as_str().to_string())).collect();
        Ok(ExprValue::make_list(matches, ExprType::STRING)?)
    } else if num_groups == 1 {
        let matches: Vec<ExprValue> = re.captures_iter(&s).map(|c| ExprValue::String(c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default())).collect();
        Ok(ExprValue::make_list(matches, ExprType::STRING)?)
    } else {
        let matches: Result<Vec<ExprValue>, _> = re.captures_iter(&s).map(|c| {
            let groups: Vec<ExprValue> = (1..=num_groups).map(|i| ExprValue::String(c.get(i).map(|m| m.as_str().to_string()).unwrap_or_default())).collect();
            ExprValue::make_list(groups, ExprType::STRING)
        }).collect();
        Ok(ExprValue::make_list(matches?, ExprType::list(ExprType::STRING))?)
    }
}

pub fn re_replace_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    if a.len() != 3 { return Err(ExpressionError::new("re_replace() takes 3 arguments")); }
    let s = match &a[0] { ExprValue::String(s) => s.clone(), _ => return Err(ExpressionError::new("re_replace() requires strings")) };
    ctx.count_string_ops(s.len())?;
    let pat = match &a[1] { ExprValue::String(s) => s.clone(), _ => return Err(ExpressionError::new("re_replace() requires strings")) };
    let repl = match &a[2] { ExprValue::String(s) => s.clone(), _ => return Err(ExpressionError::new("re_replace() requires strings")) };
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
            if bytes[i+1].is_ascii_digit() {
                return Err(ExpressionError::new("Group references in replacement strings are not supported"));
            }
            if bytes[i+1] == b'g' && i + 2 < bytes.len() && bytes[i+2] == b'<' {
                return Err(ExpressionError::new("Group references in replacement strings are not supported"));
            }
        }
        // Check for $1, $2, ${1}, ${name} references
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            if bytes[i+1].is_ascii_digit() || bytes[i+1] == b'{' {
                return Err(ExpressionError::new("Group references in replacement strings are not supported"));
            }
        }
    }
    Ok(())
}

pub fn re_split_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    if a.len() < 2 || a.len() > 3 { return Err(ExpressionError::new("re_split() takes 2-3 arguments")); }
    let (s, pat) = get_two_strings(a, "re_split")?;
    ctx.count_string_ops(s.len())?;
    validate_regex_pattern(&pat)?;
    let maxsplit = a.get(2).and_then(|v| match v { ExprValue::Int(n) => Some(*n as usize), _ => None });
    let re = ctx.get_or_compile_regex(&pat)?;
    let parts: Vec<ExprValue> = match maxsplit {
        Some(n) => re.splitn(&s, n + 1).map(|p| ExprValue::String(p.to_string())).collect(),
        None => re.split(&s).map(|p| ExprValue::String(p.to_string())).collect(),
    };
    Ok(ExprValue::make_list(parts, ExprType::STRING)?)
}
