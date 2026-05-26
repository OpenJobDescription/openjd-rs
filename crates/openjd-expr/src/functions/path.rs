// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

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
    // `replace` walks the input and allocates a new string
    // proportional to it. Budget the work so a sufficiently
    // long input can't force unbounded compute or allocation.
    ctx.count_string_ops(path_str.len())?;
    Ok(ExprValue::String(path_str.replace('\\', "/")))
}

/// Return `true` if the input has no final component to operate
/// on with `with_name` / `with_stem` / `with_suffix` / `with_number`.
///
/// For URI inputs, that's a bare authority (`s3://bucket`) or an
/// authority with a trailing slash (`s3://bucket/`) — in both cases
/// `uri_path::name` returns the empty string. For filesystem paths
/// it's the same: a path whose `pp::file_name` is empty (the root
/// `/` on POSIX, `C:\` or `\\server\share` on Windows). Without
/// this guard, the `with_*` operators silently invent a filename
/// against an empty stem and emit nonsensical results like
/// `s3://bucket/.png` (a URI for a hidden file in the bucket root)
/// or `//x` (a UNC-shaped path) — see `with_name_fn` etc. for the
/// per-operator details.
///
/// Matches Python pathlib's behaviour:
/// `PurePosixPath('/').with_name('x')` raises
/// `ValueError: PurePosixPath('/') has an empty name`.
fn has_empty_name(path_str: &str, fmt: PathFormat) -> bool {
    if crate::uri_path::is_uri(path_str) {
        crate::uri_path::name(path_str).is_empty()
    } else {
        pp::file_name(path_str, fmt).is_empty()
    }
}

/// Return `true` if `name` is a valid replacement filename for
/// `with_name` / `with_stem`.
///
/// Matches Python pathlib's `with_name` validation: rejects the
/// empty string, the `.` sentinel, and any string containing a
/// path separator. `..` is *accepted* — pathlib treats it as a
/// valid name (it's a filename that happens to look like the
/// parent-dir indicator). On Windows both `/` and `\` are
/// separators; on POSIX only `/`.
fn is_valid_name(name: &str, fmt: PathFormat) -> bool {
    if name.is_empty() || name == "." {
        return false;
    }
    if name.contains('/') {
        return false;
    }
    if fmt == PathFormat::Windows && name.contains('\\') {
        return false;
    }
    true
}

/// Return `true` if `suffix` is a valid replacement suffix for
/// `with_suffix`.
///
/// Matches Python pathlib's `with_suffix` validation:
/// - Empty string is OK (strips the suffix).
/// - Otherwise the suffix MUST start with `.` AND not be just
///   `.` (so `.x` is OK, `.` alone is rejected).
/// - The suffix MUST NOT contain a separator (`/` always; `\\`
///   on Windows).
fn is_valid_suffix(suffix: &str, fmt: PathFormat) -> bool {
    if suffix.is_empty() {
        return true;
    }
    if !suffix.starts_with('.') || suffix == "." {
        return false;
    }
    if suffix.contains('/') {
        return false;
    }
    if fmt == PathFormat::Windows && suffix.contains('\\') {
        return false;
    }
    true
}

/// Join `parent` and `name` with the format's separator, but
/// elide the separator when `parent` is empty (relative
/// single-component path) or already ends with a separator
/// (filesystem anchor like `/`, `C:\`, or `\\server\share\`).
/// Without this, a single-component relative path
/// (`pp::parent("foo") == ""`) would produce `"/x"` instead of
/// `"x"`, and a drive-rooted path (`pp::parent("C:\\foo") ==
/// "C:\\"`) would produce `"C:\\\\x"` instead of `"C:\\x"`.
///
/// Matches Python pathlib: `PurePosixPath("foo").with_name("x")
/// == PurePosixPath("x")`, `PurePosixPath("/foo").with_name("x")
/// == PurePosixPath("/x")`.
fn join_parent_and_name(parent: &str, name: &str, fmt: PathFormat) -> String {
    if parent.is_empty() || parent == "." {
        // Pathlib's `with_*` operators treat a relative
        // single-component parent (rendered as `.`) as having
        // no parent for join purposes:
        //   PurePosixPath("foo").with_name("x") == "x", not "./x".
        // The `parent` *property* still renders as `.` per
        // pathlib (see `pp::parent`), but the join here drops
        // the leading dot to match pathlib's `_from_parsed_parts`
        // semantics.
        //
        // Pathlib prepends `.\` on Windows when the resulting
        // name would otherwise parse as a drive-relative anchor
        // (`X:y`, `X:`). The pattern: at least 2 chars, first
        // char ASCII alphabetic, second char `:`. Without this
        // adjustment, `with_name('x:y')` on a relative path
        // would produce `'x:y'`, which when re-parsed as a
        // Windows path looks like the drive-relative path
        // `x:` + `y`. Pathlib disambiguates by emitting
        // `.\x:y`. POSIX has no such ambiguity.
        if fmt == PathFormat::Windows {
            let nb = name.as_bytes();
            if nb.len() >= 2 && nb[0].is_ascii_alphabetic() && nb[1] == b':' {
                return format!(".\\{name}");
            }
        }
        return name.to_string();
    }
    let last = parent.as_bytes().last().copied();
    let already_terminated = match fmt {
        PathFormat::Windows => {
            // Trailing separator (`C:\`, `\\srv\share\`, `\foo\`,
            // etc.) — no extra separator needed.
            if last == Some(b'/') || last == Some(b'\\') {
                true
            } else {
                // Drive-relative anchor (`C:`, `D:`) with no
                // following separator. Pathlib joins this without
                // a separator: `PureWindowsPath('C:foo').with_name('x')`
                // produces `'C:x'`, not `'C:\\x'`. Detect by:
                // length == 2, byte 0 alphabetic, byte 1 == ':'.
                let bytes = parent.as_bytes();
                bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
            }
        }
        PathFormat::Posix | PathFormat::Uri => last == Some(b'/'),
    };
    if already_terminated {
        format!("{parent}{name}")
    } else {
        format!("{parent}{sep}{name}", sep = pp::sep(fmt))
    }
}

pub fn with_name_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, fmt) = get_path(&a[0], ctx)?;
    let new_name = get_str_arg(a, 1);
    if !is_valid_name(&new_name, fmt) {
        return Err(ExpressionError::new(format!(
            "with_name: Invalid name '{new_name}'"
        )));
    }
    // Charge an operation budget proportional to the input
    // length so an attacker cannot construct an input that
    // forces unbounded string work. Matches the convention used
    // by `with_suffix_fn` and the path-property helpers
    // (`prop_name`, `prop_stem`, etc.) — see
    // `EvalContext::count_string_ops`.
    ctx.count_string_ops(path_str.len())?;
    if has_empty_name(&path_str, fmt) {
        return Err(ExpressionError::new(format!(
            "with_name: '{path_str}' has an empty name"
        )));
    }
    // URI paths have their own segment grammar that's independent
    // of the host's path format. Without this branch, on a Windows
    // host `with_name(...)` on a URI value would emit
    // `s3:\\bucket\dir\file.ext` because `pp::parent` /
    // `pp::sep` use the host separator. See `with_suffix_fn`,
    // which already handled this; this branch is the same idea
    // for `with_name`.
    if crate::uri_path::is_uri(&path_str) {
        let parent = crate::uri_path::parent(&path_str);
        return Ok(ExprValue::new_path(format!("{parent}/{new_name}"), fmt));
    }
    let parent = pp::parent(&path_str, fmt);
    Ok(ExprValue::new_path(
        join_parent_and_name(&parent, &new_name, fmt),
        fmt,
    ))
}

pub fn with_stem_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, fmt) = get_path(&a[0], ctx)?;
    let new_stem = get_str_arg(a, 1);
    // pathlib's `with_stem` is implemented as
    // `self.with_name(stem + self.suffix)`. We mirror that — the
    // separator-containing and empty-stem-with-no-suffix cases
    // turn into `Invalid name` errors after the filename is
    // constructed, and the empty-stem-on-path-with-suffix case
    // becomes `has a non-empty suffix`. Special-case the latter
    // because pathlib uses a more specific diagnostic for it.
    //
    // Note: a stem of `.` is NOT rejected up front. When the
    // path has a non-empty suffix, the resulting filename is
    // `.{suffix}` (e.g., `..txt` for `'foo.txt'.with_stem('.')`),
    // which is a valid name. When there's no suffix, the
    // resulting filename is just `.` and the post-construction
    // `is_valid_name` check catches it as `Invalid name '.'`.
    // Only reject up front the cases pathlib's `with_name`
    // rejects on the stem alone — separator-containing names.
    if !new_stem.is_empty() && new_stem != "." {
        // Cheap up-front separator check; the post-construction
        // check still catches the stem-equals-dot case.
        if new_stem.contains('/') || (fmt == PathFormat::Windows && new_stem.contains('\\')) {
            return Err(ExpressionError::new(format!(
                "with_stem: Invalid name '{new_stem}'"
            )));
        }
    }
    ctx.count_string_ops(path_str.len())?;
    if has_empty_name(&path_str, fmt) {
        return Err(ExpressionError::new(format!(
            "with_stem: '{path_str}' has an empty name"
        )));
    }
    // URI paths use `/` regardless of host. See `with_name_fn`
    // for the rationale; this branch is the same idea for
    // `with_stem`. Suffix is taken from the URI grammar
    // (`uri_path::suffix`), not from `pp::extension`, so the
    // result is host-format-independent end-to-end.
    let is_uri = crate::uri_path::is_uri(&path_str);
    let (parent, ext) = if is_uri {
        (
            crate::uri_path::parent(&path_str),
            crate::uri_path::suffix(&path_str),
        )
    } else {
        (
            pp::parent(&path_str, fmt),
            pp::extension(&path_str, fmt).to_string(),
        )
    };
    if new_stem.is_empty() && !ext.is_empty() {
        // Specific pathlib diagnostic for this case.
        return Err(ExpressionError::new(format!(
            "with_stem: '{path_str}' has a non-empty suffix"
        )));
    }
    let new_filename = format!("{new_stem}{ext}");
    if !is_valid_name(&new_filename, fmt) {
        return Err(ExpressionError::new(format!(
            "with_stem: Invalid name '{new_filename}'"
        )));
    }
    let result = if is_uri {
        format!("{parent}/{new_filename}")
    } else {
        join_parent_and_name(&parent, &new_filename, fmt)
    };
    Ok(ExprValue::new_path(result, fmt))
}

pub fn with_suffix_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, fmt) = get_path(&a[0], ctx)?;
    let new_suffix = get_str_arg(a, 1);
    if !is_valid_suffix(&new_suffix, fmt) {
        return Err(ExpressionError::new(format!(
            "with_suffix: Invalid suffix '{new_suffix}'"
        )));
    }
    ctx.count_string_ops(path_str.len())?;
    if has_empty_name(&path_str, fmt) {
        return Err(ExpressionError::new(format!(
            "with_suffix: '{path_str}' has an empty name"
        )));
    }
    let is_uri = crate::uri_path::is_uri(&path_str);
    let (parent, stem) = if is_uri {
        (
            crate::uri_path::parent(&path_str),
            crate::uri_path::stem(&path_str),
        )
    } else {
        (
            pp::parent(&path_str, fmt),
            pp::file_stem(&path_str, fmt).to_string(),
        )
    };
    let new_filename = format!("{stem}{new_suffix}");
    // Pathlib also validates the constructed filename and raises
    // if it's invalid. Concretely: `'..foo'.with_suffix('')`
    // would produce the filename `.` (because `'..foo'.stem` is
    // `.` per pathlib's rule), and pathlib catches that with
    // `Invalid name '.'`. Re-running our `is_valid_name` against
    // the constructed filename catches the same case.
    if !is_valid_name(&new_filename, fmt) {
        return Err(ExpressionError::new(format!(
            "with_suffix: Invalid name '{new_filename}'"
        )));
    }
    let result = if is_uri {
        format!("{parent}/{new_filename}")
    } else {
        join_parent_and_name(&parent, &new_filename, fmt)
    };
    Ok(ExprValue::new_path(result, fmt))
}

/// Split a filename into `(stem, suffix)` per Python pathlib's
/// rule: the suffix exists only when the rightmost `.` is neither
/// at the start of the name nor at the end. Used by
/// `with_number_fn` to preserve the suffix verbatim while
/// substituting the stem.
fn split_name_at_suffix(filename: &str) -> (&str, &str) {
    match filename.rfind('.') {
        Some(i) if i > 0 && i + 1 < filename.len() => (&filename[..i], &filename[i..]),
        _ => (filename, ""),
    }
}

pub fn with_number_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, fmt) = get_path(&a[0], ctx)?;
    let num = match &a[1] {
        ExprValue::Int(n) => *n,
        _ => return Err(ExpressionError::new("with_number() requires int argument")),
    };
    ctx.count_string_ops(path_str.len())?;
    if has_empty_name(&path_str, fmt) {
        return Err(ExpressionError::new(format!(
            "with_number: '{path_str}' has an empty name"
        )));
    }
    let is_string = matches!(&a[0], ExprValue::String(_));
    // URI paths use `/` regardless of host. Without this branch,
    // on a Windows host `with_number(...)` on a URI value would
    // emit `s3://bucket/renders\shot_0042.exr` because
    // `pp::split` and `pp::sep` use the host separator. See
    // `with_name_fn` / `with_stem_fn` for the analogous fix.
    let result = if crate::uri_path::is_uri(&path_str) {
        let parent = crate::uri_path::parent(&path_str);
        let filename = crate::uri_path::name(&path_str);
        let (stem, suffix) = split_name_at_suffix(&filename);
        let new_stem = with_number_replace(stem, num)?;
        // `parent` is the URI authority + leading path segments.
        format!("{parent}/{new_stem}{suffix}")
    } else {
        let (dir_part, filename) = pp::split(&path_str, fmt);
        let (stem, suffix) = split_name_at_suffix(filename);
        let new_stem = with_number_replace(stem, num)?;
        let new_filename = format!("{new_stem}{suffix}");
        join_parent_and_name(dir_part, &new_filename, fmt)
    };
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

fn path_starts_with(path: &str, base: &str, fmt: PathFormat) -> bool {
    if fmt == PathFormat::Windows {
        path.len() >= base.len() && path[..base.len()].eq_ignore_ascii_case(base)
    } else {
        path.starts_with(base)
    }
}

pub fn is_relative_to_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, fmt) = get_path(&a[0], ctx)?;
    let base = get_str_arg(a, 1);
    // The prefix comparison below is `O(min(path_str.len(),
    // base.len()))` in the worst case (especially on Windows where
    // it does a case-insensitive compare). Budget the work
    // proportionally to the larger input so a long pair can't
    // force unbounded compute.
    ctx.count_string_ops(path_str.len().max(base.len()))?;
    let is_rel = path_starts_with(&path_str, &base, fmt)
        && (path_str.len() == base.len()
            || base.ends_with('/')
            || base.ends_with('\\')
            || matches!(path_str.as_bytes().get(base.len()), Some(b'/' | b'\\')));
    Ok(ExprValue::Bool(is_rel))
}

pub fn relative_to_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let (path_str, fmt) = get_path(&a[0], ctx)?;
    let base = get_str_arg(a, 1);
    // Same rationale as `is_relative_to_fn`. This function also
    // clones the tail of `path_str` into a fresh String, so the
    // allocation is `O(path_str.len() - base.len())`, but the
    // input-length bound here is the safer upper bound.
    ctx.count_string_ops(path_str.len().max(base.len()))?;
    let is_rel = path_starts_with(&path_str, &base, fmt)
        && (path_str.len() == base.len()
            || base.ends_with('/')
            || base.ends_with('\\')
            || matches!(path_str.as_bytes().get(base.len()), Some(b'/' | b'\\')));
    if !is_rel {
        return Err(ExpressionError::new(format!(
            "relative_to failed: '{path_str}' is not relative to '{base}'"
        )));
    }
    let rel = path_str[base.len()..]
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

/// Build a closure for `apply_path_mapping` that captures the given rules.
///
/// The rules are stored in an `Arc` so many `FunctionLibrary` clones can
/// share them cheaply. This factory is the only way to produce an
/// `apply_path_mapping` implementation; the host crate passes its rules
/// at library-construction time rather than plumbing them through the
/// evaluator on every call.
pub fn make_apply_path_mapping_fn(
    rules: std::sync::Arc<Vec<crate::path_mapping::PathMappingRule>>,
) -> impl Fn(&mut dyn EvalContext, &[ExprValue]) -> R + Send + Sync + 'static {
    move |ctx, a| {
        let (path_str, fmt) = get_path(&a[0], ctx)?;
        // `apply_rules_with_format` walks the rules and does an
        // `O(rules × path_len)` prefix comparison + a string
        // substitution. Budget the work proportionally to the
        // input length — the rule count is bounded by the
        // session profile, but the path length is caller-
        // controlled.
        ctx.count_string_ops(path_str.len())?;
        let mapped =
            crate::path_mapping::apply_rules_with_format(&rules, &path_str, ctx.path_format());
        if mapped == path_str {
            Ok(ExprValue::new_path(path_str, fmt))
        } else {
            Ok(ExprValue::new_path(mapped, fmt))
        }
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
        return ExprValue::make_list_checked(ctx, suffixes, crate::types::ExprType::STRING);
    }
    let suffixes: Vec<ExprValue> = pp::suffixes(&path_str, fmt)
        .into_iter()
        .map(ExprValue::String)
        .collect();
    ExprValue::make_list_checked(ctx, suffixes, crate::types::ExprType::STRING)
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
        return ExprValue::make_list_checked(ctx, parts, crate::types::ExprType::STRING);
    }
    let parts: Vec<ExprValue> = pp::parts(&path_str, fmt)
        .into_iter()
        .map(ExprValue::String)
        .collect();
    ExprValue::make_list_checked(ctx, parts, crate::types::ExprType::STRING)
}
