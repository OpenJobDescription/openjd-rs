// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Path method implementations.
//!
//! Uses `path_parse` for format-aware path manipulation instead of
//! `std::path::Path` which only understands the host OS's path format.

use crate::error::ExpressionError;
use crate::function_library::EvalContext;
use crate::path_mapping::PathFormat;
use crate::value::ExprValue;

use super::path_parse as pp;

type R = Result<ExprValue, ExpressionError>;
type Ctx<'a> = &'a mut dyn EvalContext;

fn get_path(a: &ExprValue, ctx: &dyn EvalContext) -> Result<(String, PathFormat), ExpressionError> {
    match a {
        ExprValue::Path { value, format } => Ok((value.clone(), *format)),
        ExprValue::String(s) => Ok((s.clone(), ctx.path_format())),
        _ => Err(ExpressionError::new(format!(
            "Path method not supported on {}",
            a.expr_type()
        ))),
    }
}

fn get_str_arg(a: &[ExprValue], idx: usize) -> String {
    a.get(idx)
        .map(|v| match v {
            ExprValue::String(s) => s.clone(),
            ExprValue::Path { value, .. } => value.clone(),
            _ => String::new(),
        })
        .unwrap_or_default()
}

pub fn as_posix_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, _) = get_path(&a[0], ctx)?;
    Ok(ExprValue::String(path_str.replace('\\', "/")))
}

pub fn with_name_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, fmt) = get_path(&a[0], ctx)?;
    let new_name = get_str_arg(a, 1);
    if new_name.contains('/') || (fmt == PathFormat::Windows && new_name.contains('\\')) {
        return Err(ExpressionError::new(format!(
            "with_name: name must not contain path separators, got '{new_name}'"
        )));
    }
    let parent = pp::parent(&path_str, fmt);
    let sep = pp::sep(fmt);
    Ok(ExprValue::new_path(format!("{parent}{sep}{new_name}"), fmt))
}

pub fn with_stem_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, fmt) = get_path(&a[0], ctx)?;
    let new_stem = get_str_arg(a, 1);
    if new_stem.contains('/') || (fmt == PathFormat::Windows && new_stem.contains('\\')) {
        return Err(ExpressionError::new(format!(
            "with_stem: name must not contain path separators, got '{new_stem}'"
        )));
    }
    let ext = pp::extension(&path_str, fmt);
    let parent = pp::parent(&path_str, fmt);
    let sep = pp::sep(fmt);
    Ok(ExprValue::new_path(
        format!("{parent}{sep}{new_stem}{ext}"),
        fmt,
    ))
}

pub fn with_suffix_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, fmt) = get_path(&a[0], ctx)?;
    let new_suffix = get_str_arg(a, 1);
    ctx.count_string_ops(path_str.len())?;
    if crate::uri_path::is_uri(&path_str) {
        let stem = crate::uri_path::stem(&path_str);
        let parent = crate::uri_path::parent(&path_str);
        return Ok(ExprValue::new_path(
            format!("{parent}/{stem}{new_suffix}"),
            fmt,
        ));
    }
    let stem = pp::file_stem(&path_str, fmt);
    let parent = pp::parent(&path_str, fmt);
    let sep = pp::sep(fmt);
    Ok(ExprValue::new_path(
        format!("{parent}{sep}{stem}{new_suffix}"),
        fmt,
    ))
}

pub fn with_number_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, fmt) = get_path(&a[0], ctx)?;
    let num = match &a[1] {
        ExprValue::Int(n) => *n,
        _ => return Err(ExpressionError::new("with_number() requires int argument")),
    };
    let is_string = matches!(&a[0], ExprValue::String(_));
    let (dir_part, filename) = pp::split(&path_str, fmt);
    let prefix = if dir_part.is_empty() {
        String::new()
    } else {
        format!("{}{}", dir_part, pp::sep(fmt))
    };
    let (stem, suffix) = match filename.rfind('.') {
        Some(i) if i > 0 => (&filename[..i], &filename[i..]),
        _ => (filename, ""),
    };
    let new_stem = with_number_replace(stem, num)?;
    let result = format!("{prefix}{new_stem}{suffix}");
    if is_string {
        Ok(ExprValue::String(result))
    } else {
        Ok(ExprValue::new_path(result, fmt))
    }
}

pub fn is_absolute_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, fmt) = get_path(&a[0], ctx)?;
    Ok(ExprValue::Bool(is_absolute(&path_str, fmt)))
}

/// Cross-platform is_absolute that respects path_format regardless of host OS.
pub fn is_absolute(path_str: &str, fmt: PathFormat) -> bool {
    if crate::uri_path::is_uri(path_str) {
        return true;
    }
    let bytes = path_str.as_bytes();
    // UNC path: //server or \\server
    if bytes.len() >= 2
        && ((bytes[0] == b'/' && bytes[1] == b'/') || (bytes[0] == b'\\' && bytes[1] == b'\\'))
    {
        return true;
    }
    match fmt {
        PathFormat::Windows => {
            bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && (bytes[2] == b'\\' || bytes[2] == b'/')
        }
        PathFormat::Posix | PathFormat::Uri => bytes.first() == Some(&b'/'),
    }
}

/// Join two path strings using the separator and absoluteness rules for `fmt`.
///
/// If `right` is absolute (according to `fmt`), it replaces `left` entirely.
/// On Windows, if `right` starts with a single `/` or `\` (root-relative),
/// the drive letter from `left` is preserved (matching `ntpath.join` behavior).
/// Otherwise, `right` is appended to `left` with the appropriate separator.
pub fn join(left: &str, right: &str, fmt: PathFormat) -> String {
    if is_absolute(right, fmt) {
        return right.to_string();
    }
    // Windows root-relative: /foo or \foo (but not \\server) replaces the path
    // but keeps the root from left. Matches ntpath.join behavior.
    // For drive paths (C:\...), the root is "C:".
    // For UNC paths (\\server\share\...), the root is "\\server\share".
    if fmt == PathFormat::Windows {
        let rb = right.as_bytes();
        if rb.first() == Some(&b'/') || rb.first() == Some(&b'\\') {
            let lb = left.as_bytes();
            // Drive path: keep "C:" prefix
            if lb.len() >= 2 && lb[0].is_ascii_alphabetic() && lb[1] == b':' {
                return format!("{}{right}", &left[..2]);
            }
            // UNC path: keep "\\server\share" or "//server/share" prefix
            if let Some(unc_root) = extract_unc_root(left) {
                return format!("{unc_root}{right}");
            }
        }
    }
    let left_is_uri = crate::uri_path::is_uri(left);
    let (sep, trim_chars): (&str, &[char]) = if left_is_uri {
        ("/", &['/'])
    } else {
        match fmt {
            // On Windows, both / and \ are separators
            PathFormat::Windows => ("\\", &['/', '\\']),
            // On POSIX, only / is a separator (\ is a valid filename char)
            PathFormat::Posix | PathFormat::Uri => ("/", &['/']),
        }
    };
    let left = left.trim_end_matches(trim_chars);
    // When appending to a URI from a Windows context, normalize backslashes to forward slashes.
    // In POSIX context, backslashes are valid filename characters and must not be converted.
    let right = if left_is_uri && fmt == PathFormat::Windows {
        std::borrow::Cow::Owned(right.replace('\\', "/"))
    } else {
        std::borrow::Cow::Borrowed(right)
    };
    format!("{left}{sep}{right}")
}

/// Join two path strings without recognizing URIs as absolute.
///
/// Like [`join`], but does not check `is_absolute(right)`. Use when `right` has
/// already been determined to be non-absolute via a URI-unaware check (e.g.,
/// `is_absolute` without URI recognition). This prevents `scheme://...` strings
/// from being treated as absolute when URI support is disabled.
pub fn non_uri_join(left: &str, right: &str, fmt: PathFormat) -> String {
    // Windows root-relative: /foo or \foo keeps the root from left
    if fmt == PathFormat::Windows {
        let rb = right.as_bytes();
        if rb.first() == Some(&b'/') || rb.first() == Some(&b'\\') {
            let lb = left.as_bytes();
            if lb.len() >= 2 && lb[0].is_ascii_alphabetic() && lb[1] == b':' {
                return format!("{}{right}", &left[..2]);
            }
            if let Some(unc_root) = extract_unc_root(left) {
                return format!("{unc_root}{right}");
            }
        }
    }
    let (sep, trim_chars): (&str, &[char]) = match fmt {
        PathFormat::Windows => ("\\", &['/', '\\']),
        PathFormat::Posix | PathFormat::Uri => ("/", &['/']),
    };
    let left = left.trim_end_matches(trim_chars);
    format!("{left}{sep}{right}")
}

/// Extract the UNC root from a path: `\\server\share` or `//server/share`.
/// Returns the root portion (two components after the leading `\\` or `//`).
fn extract_unc_root(path: &str) -> Option<&str> {
    let bytes = path.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let prefix_char = bytes[0];
    if !((prefix_char == b'\\' && bytes[1] == b'\\') || (prefix_char == b'/' && bytes[1] == b'/')) {
        return None;
    }
    // Find the separator after "server"
    let rest = &path[2..];
    let sep_after_server = rest.find(['/', '\\'])?;
    let after_server = sep_after_server + 3; // 2 for prefix + 1 for separator
                                             // Find the separator after "share" (or end of string)
    let share_start = after_server;
    let sep_after_share = path[share_start..]
        .find(['/', '\\'])
        .map(|i| share_start + i)
        .unwrap_or(path.len());
    Some(&path[..sep_after_share])
}

pub fn is_relative_to_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, _) = get_path(&a[0], ctx)?;
    let base = get_str_arg(a, 1);
    let is_rel = path_str.starts_with(&base)
        && (path_str.len() == base.len()
            || base.ends_with('/')
            || base.ends_with('\\')
            || matches!(path_str.as_bytes().get(base.len()), Some(b'/' | b'\\')));
    Ok(ExprValue::Bool(is_rel))
}

pub fn relative_to_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, fmt) = get_path(&a[0], ctx)?;
    let base = get_str_arg(a, 1);
    let is_rel = path_str.starts_with(&base)
        && (path_str.len() == base.len()
            || base.ends_with('/')
            || base.ends_with('\\')
            || matches!(path_str.as_bytes().get(base.len()), Some(b'/' | b'\\')));
    if !is_rel {
        return Err(ExpressionError::new(format!(
            "relative_to failed: '{path_str}' is not relative to '{base}'"
        )));
    }
    let rel = path_str
        .strip_prefix(&base)
        .unwrap_or(&path_str)
        .trim_start_matches('/')
        .trim_start_matches('\\');
    Ok(ExprValue::new_path(
        if rel.is_empty() {
            ".".to_string()
        } else {
            rel.to_string()
        },
        fmt,
    ))
}

pub fn apply_path_mapping_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, fmt) = get_path(&a[0], ctx)?;
    let mapped = crate::path_mapping::apply_rules(ctx.path_mapping_rules(), &path_str);
    if mapped == path_str {
        // No rule matched — still normalize separators to the target format
        Ok(ExprValue::new_path(path_str, fmt))
    } else {
        Ok(ExprValue::new_path(mapped, fmt))
    }
}

fn format_padded(num: i64, width: usize) -> String {
    if num < 0 {
        format!("-{:0>width$}", -num, width = width.saturating_sub(1))
    } else {
        format!("{:0>width$}", num, width = width)
    }
}

const MAX_PADDING_WIDTH: usize = 32;

fn with_number_replace(stem: &str, num: i64) -> Result<String, ExpressionError> {
    // 1. Printf %0Nd or %d
    if let Some(pct) = stem.rfind('%') {
        let after = &stem[pct + 1..];
        if after == "d" {
            return Ok(format!("{}{}", &stem[..pct], num));
        }
        if after.starts_with('0') && after.ends_with('d') {
            let width: usize = after[1..after.len() - 1].parse().unwrap_or(1);
            if width > MAX_PADDING_WIDTH {
                return Err(ExpressionError::new(format!(
                    "with_number: padding width {width} exceeds maximum of {MAX_PADDING_WIDTH}"
                )));
            }
            return Ok(format!("{}{}", &stem[..pct], format_padded(num, width)));
        }
    }
    // 2. Hash pattern ####
    if let Some(start) = stem.rfind('#') {
        let hash_start = stem[..=start]
            .rfind(|c: char| c != '#')
            .map(|i| i + 1)
            .unwrap_or(0);
        let width = start - hash_start + 1;
        if width > MAX_PADDING_WIDTH {
            return Err(ExpressionError::new(format!(
                "with_number: padding width {width} exceeds maximum of {MAX_PADDING_WIDTH}"
            )));
        }
        return Ok(format!(
            "{}{}",
            &stem[..hash_start],
            format_padded(num, width)
        ));
    }
    // 3. Trailing digits
    let digit_start = stem.len()
        - stem
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .count();
    if digit_start < stem.len() {
        let width = stem.len() - digit_start;
        return Ok(format!(
            "{}{}",
            &stem[..digit_start],
            format_padded(num, width)
        ));
    }
    // 4. No pattern — append _NNNN
    Ok(format!("{}_{}", stem, format_padded(num, 4)))
}

// ── Path properties ──

pub fn prop_name(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, fmt) = get_path(&a[0], ctx)?;
    ctx.count_string_ops(path_str.len())?;
    if crate::uri_path::is_uri(&path_str) {
        return Ok(ExprValue::String(crate::uri_path::name(&path_str)));
    }
    Ok(ExprValue::String(pp::file_name(&path_str, fmt).to_string()))
}

pub fn prop_stem(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, fmt) = get_path(&a[0], ctx)?;
    ctx.count_string_ops(path_str.len())?;
    if crate::uri_path::is_uri(&path_str) {
        return Ok(ExprValue::String(crate::uri_path::stem(&path_str)));
    }
    Ok(ExprValue::String(pp::file_stem(&path_str, fmt).to_string()))
}

pub fn prop_suffix(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, fmt) = get_path(&a[0], ctx)?;
    ctx.count_string_ops(path_str.len())?;
    Ok(ExprValue::String(if crate::uri_path::is_uri(&path_str) {
        crate::uri_path::suffix(&path_str)
    } else {
        pp::extension(&path_str, fmt).to_string()
    }))
}

pub fn prop_suffixes(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, fmt) = get_path(&a[0], ctx)?;
    ctx.count_string_ops(path_str.len())?;
    if crate::uri_path::is_uri(&path_str) {
        let suffixes: Vec<ExprValue> = crate::uri_path::suffixes(&path_str)
            .into_iter()
            .map(ExprValue::String)
            .collect();
        return ExprValue::make_list(suffixes, crate::types::ExprType::STRING);
    }
    let suffixes: Vec<ExprValue> = pp::suffixes(&path_str, fmt)
        .into_iter()
        .map(ExprValue::String)
        .collect();
    ExprValue::make_list(suffixes, crate::types::ExprType::STRING)
}

pub fn prop_parent(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, fmt) = get_path(&a[0], ctx)?;
    ctx.count_string_ops(path_str.len())?;
    if crate::uri_path::is_uri(&path_str) {
        return Ok(ExprValue::new_path(crate::uri_path::parent(&path_str), fmt));
    }
    Ok(ExprValue::new_path(pp::parent(&path_str, fmt), fmt))
}

pub fn prop_parts(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, fmt) = get_path(&a[0], ctx)?;
    ctx.count_string_ops(path_str.len())?;
    if crate::uri_path::is_uri(&path_str) {
        let parts: Vec<ExprValue> = crate::uri_path::parts(&path_str)
            .into_iter()
            .map(ExprValue::String)
            .collect();
        return ExprValue::make_list(parts, crate::types::ExprType::STRING);
    }
    let parts: Vec<ExprValue> = pp::parts(&path_str, fmt)
        .into_iter()
        .map(ExprValue::String)
        .collect();
    ExprValue::make_list(parts, crate::types::ExprType::STRING)
}
