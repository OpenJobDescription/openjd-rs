// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Format-aware path parsing — replaces `std::path::Path` for cross-platform correctness.
//!
//! `std::path::Path` uses the host OS's path rules, which gives wrong results when
//! parsing POSIX paths on Windows or vice versa. These functions use the path's
//! `PathFormat` to determine separator handling.

use crate::path_mapping::PathFormat;

/// Check if a character is a path separator for the given format.
fn is_sep(c: char, fmt: PathFormat) -> bool {
    match fmt {
        PathFormat::Windows => c == '\\' || c == '/',
        PathFormat::Posix | PathFormat::Uri => c == '/',
    }
}

/// Get the canonical separator for the given format.
pub fn sep(fmt: PathFormat) -> char {
    match fmt {
        PathFormat::Windows => '\\',
        _ => '/',
    }
}

/// Strip trailing separators, but not if they are part of the root/anchor.
/// Returns the normalized path and the anchor length (bytes that must not be stripped).
fn normalize(path: &str, fmt: PathFormat) -> &str {
    if path.is_empty() || path == "." {
        return path;
    }
    let anchor_len = anchor_len(path, fmt);
    let mut end = path.len();
    while end > anchor_len && is_sep(path.as_bytes()[end - 1] as char, fmt) {
        end -= 1;
    }
    &path[..end]
}

/// Return the byte length of the anchor (root/drive/UNC prefix) in a path.
fn anchor_len(path: &str, fmt: PathFormat) -> usize {
    let bytes = path.as_bytes();
    match fmt {
        PathFormat::Windows => {
            if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
                // C:\ or C:
                if bytes.len() > 2 && is_sep(bytes[2] as char, fmt) {
                    3
                } else {
                    2
                }
            } else if bytes.len() >= 2
                && is_sep(bytes[0] as char, fmt)
                && is_sep(bytes[1] as char, fmt)
            {
                // UNC: \\server\share\ is the full anchor
                let rest = &path[2..];
                let server_end = rest.find(|c: char| is_sep(c, fmt)).unwrap_or(rest.len());
                let after_server = 2 + server_end;
                if after_server < path.len() {
                    // Have share part
                    let share_rest = &path[after_server + 1..];
                    let share_end = share_rest
                        .find(|c: char| is_sep(c, fmt))
                        .unwrap_or(share_rest.len());
                    let end = after_server + 1 + share_end;
                    // anchor = \\server\share\ — always include trailing sep conceptually
                    // If there's a sep after share, include it; otherwise anchor extends to end
                    // (the trailing sep will be added by parts/parent as needed)
                    if end < path.len() {
                        end + 1
                    } else {
                        end
                    }
                } else {
                    // Just \\server, no share
                    after_server
                }
            } else if !bytes.is_empty() && is_sep(bytes[0] as char, fmt) {
                1
            } else {
                0
            }
        }
        PathFormat::Posix | PathFormat::Uri => {
            if bytes.len() >= 2
                && bytes[0] == b'/'
                && bytes[1] == b'/'
                && (bytes.len() < 3 || bytes[2] != b'/')
            {
                2 // POSIX // is special
            } else if !bytes.is_empty() && bytes[0] == b'/' {
                1
            } else {
                0
            }
        }
    }
}

/// Split a path into (parent, file_name), matching pathlib behavior.
pub fn split(path: &str, fmt: PathFormat) -> (&str, &str) {
    let path = normalize(path, fmt);
    if path == "." {
        return (".", "");
    }
    let anchor = anchor_len(path, fmt);
    // For Windows UNC, the anchor includes the trailing sep; clamp to path length
    let anchor = anchor.min(path.len());

    // If path is entirely the anchor, name is empty
    if path.len() <= anchor {
        return (path, "");
    }

    let last_sep = path[anchor..].rfind(|c: char| is_sep(c, fmt));
    match last_sep {
        Some(i) => {
            let sep_pos = anchor + i;
            let parent = &path[..sep_pos];
            let name = &path[sep_pos + 1..];
            // If parent would be empty or shorter than anchor, use anchor
            if parent.len() < anchor {
                (&path[..anchor], name)
            } else {
                (parent, name)
            }
        }
        None => {
            // No separator after anchor — name is everything after anchor
            (&path[..anchor], &path[anchor..])
        }
    }
}

/// Get the file name (last component after the last separator).
pub fn file_name(path: &str, fmt: PathFormat) -> &str {
    split(path, fmt).1
}

/// Get the parent (everything before the last separator).
/// Returns the path itself if there's no parent (like "/" returns "/").
pub fn parent(path: &str, fmt: PathFormat) -> String {
    let (p, _name) = split(path, fmt);
    let base = p;
    // For Windows UNC with share, ensure trailing backslash
    if fmt == PathFormat::Windows {
        let s = base.replace('/', "\\");
        if s.starts_with("\\\\") && s.matches('\\').count() >= 3 && !s.ends_with('\\') {
            return format!("{}\\", s);
        }
        return s;
    }
    base.to_string()
}

/// Get the file stem (file_name without the last extension).
pub fn file_stem(path: &str, fmt: PathFormat) -> &str {
    let name = file_name(path, fmt);
    match name.rfind('.') {
        Some(0) | None => name, // ".hidden" or no dot → whole name is stem
        Some(i) => &name[..i],
    }
}

/// Get the extension (last ".ext" including the dot).
pub fn extension(path: &str, fmt: PathFormat) -> &str {
    let name = file_name(path, fmt);
    match name.rfind('.') {
        Some(0) | None => "", // ".hidden" or no dot → no extension
        Some(i) => &name[i..],
    }
}

/// Get the extension without the dot (for compatibility with std::path).
pub fn extension_no_dot(path: &str, fmt: PathFormat) -> &str {
    let ext = extension(path, fmt);
    ext.strip_prefix('.').unwrap_or("")
}

/// Split a path into its components (like Python's PurePath.parts).
///
/// For POSIX: "/mnt/renders/scene.exr" → ["/", "mnt", "renders", "scene.exr"]
/// For Windows: "C:\mnt\file" → ["C:\", "mnt", "file"]
/// For Windows: "\mnt\file" → ["\", "mnt", "file"]
pub fn parts(path: &str, fmt: PathFormat) -> Vec<String> {
    let path = normalize(path, fmt);
    if path.is_empty() || path == "." {
        return Vec::new();
    }

    let mut result = Vec::new();
    let anchor = anchor_len(path, fmt).min(path.len());

    if anchor > 0 {
        let mut anchor_str = path[..anchor].to_string();
        // Normalize separators in anchor for Windows
        if fmt == PathFormat::Windows {
            anchor_str = anchor_str.replace('/', "\\");
            // UNC root (\\server\share) must have trailing backslash
            if anchor_str.starts_with("\\\\")
                && anchor_str.matches('\\').count() >= 3
                && !anchor_str.ends_with('\\')
            {
                anchor_str.push('\\');
            }
        }
        result.push(anchor_str);
    }

    let remaining = &path[anchor..];
    for part in remaining.split(|c: char| is_sep(c, fmt)) {
        if !part.is_empty() {
            result.push(part.to_string());
        }
    }

    result
}

/// Get all suffixes (like Python's PurePath.suffixes).
/// "file.tar.gz" → [".tar", ".gz"]
pub fn suffixes(path: &str, fmt: PathFormat) -> Vec<String> {
    let name = file_name(path, fmt);
    let mut result = Vec::new();
    if let Some(first_dot) = name.find('.') {
        if first_dot == 0 {
            // Dotfile like ".hidden" — check for more dots
            if let Some(second_dot) = name[1..].find('.') {
                let mut remaining = &name[1 + second_dot..];
                while let Some(dot_pos) = remaining[1..].find('.') {
                    result.push(remaining[..dot_pos + 1].to_string());
                    remaining = &remaining[dot_pos + 1..];
                }
                result.push(remaining.to_string());
            }
            // else: just ".hidden" with no further dots → no suffixes
        } else {
            let mut remaining = &name[first_dot..];
            while let Some(dot_pos) = remaining[1..].find('.') {
                result.push(remaining[..dot_pos + 1].to_string());
                remaining = &remaining[dot_pos + 1..];
            }
            result.push(remaining.to_string());
        }
    }
    result
}

/// Join path parts using Python pathlib constructor semantics.
///
/// Matches `PurePosixPath(*parts)` or `PureWindowsPath(*parts)` behavior:
/// - Absolute components reset the accumulator
/// - Empty strings and `.` segments are removed
/// - Duplicate separators are collapsed
/// - `..` is preserved (not resolved)
pub fn join_pathlib(parts: &[String], fmt: PathFormat) -> String {
    match fmt {
        PathFormat::Posix | PathFormat::Uri => join_pathlib_posix(parts),
        PathFormat::Windows => join_pathlib_windows(parts),
    }
}

fn join_pathlib_posix(parts: &[String]) -> String {
    let mut segments: Vec<&str> = Vec::new();
    let mut is_absolute = false;

    for part in parts {
        if part.is_empty() {
            continue;
        }
        let sub_components: Vec<&str> = part.split('/').collect();
        for (i, c) in sub_components.iter().enumerate() {
            if c.is_empty() && i == 0 {
                // Leading empty = this part starts with '/'
                is_absolute = true;
                segments.clear();
            } else if *c == "." {
                // Skip '.' segments
            } else if !c.is_empty() {
                segments.push(c);
            }
        }
    }

    if is_absolute {
        if segments.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", segments.join("/"))
        }
    } else if segments.is_empty() {
        ".".to_string()
    } else {
        segments.join("/")
    }
}

/// Parse Windows drive from a path string. Returns (drive, rest).
/// Drive can be "C:" or "\\\\server\\share" (UNC).
fn win_parse_drive(s: &str) -> (&str, &str) {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        (&s[..2], &s[2..])
    } else if bytes.len() >= 2
        && is_sep(bytes[0] as char, PathFormat::Windows)
        && is_sep(bytes[1] as char, PathFormat::Windows)
    {
        // UNC: \\server\share
        let rest = &s[2..];
        let server_end = rest
            .find(|c: char| is_sep(c, PathFormat::Windows))
            .unwrap_or(rest.len());
        let after_server = 2 + server_end;
        if after_server < s.len() {
            let share_rest = &s[after_server + 1..];
            let share_end = share_rest
                .find(|c: char| is_sep(c, PathFormat::Windows))
                .unwrap_or(share_rest.len());
            let end = after_server + 1 + share_end;
            (&s[..end], &s[end..])
        } else {
            (s, "")
        }
    } else {
        ("", s)
    }
}

fn join_pathlib_windows(parts: &[String]) -> String {
    // Track accumulated drive, root, and relative parts separately.
    // For each new part, parse its drive and root, then apply pathlib rules.
    let mut drive = String::new();
    let mut root = String::new();
    let mut segments: Vec<String> = Vec::new();

    for part in parts {
        let (new_drive, after_drive) = win_parse_drive(part);
        let has_root = !after_drive.is_empty()
            && is_sep(after_drive.as_bytes()[0] as char, PathFormat::Windows);
        let new_root = if has_root { "\\" } else { "" };
        let rel = if has_root {
            &after_drive[1..]
        } else {
            after_drive
        };

        if !new_drive.is_empty() {
            if !new_drive.is_empty() && !drive.is_empty() && !new_drive.eq_ignore_ascii_case(&drive)
            {
                // Different drive → replace everything
                drive = new_drive.to_string();
                root = new_root.to_string();
                segments.clear();
            } else {
                // Same drive (or first drive)
                drive = new_drive.to_string();
                if !new_root.is_empty() {
                    root = new_root.to_string();
                    segments.clear();
                }
            }
        } else if !new_root.is_empty() {
            // Root without drive → keep existing drive, replace from root
            root = new_root.to_string();
            segments.clear();
        }
        // Append relative components, filtering empty and '.'
        for c in rel.split(|c: char| is_sep(c, PathFormat::Windows)) {
            if c == "." || c.is_empty() {
                continue;
            }
            segments.push(c.to_string());
        }
    }

    // Reconstruct
    let mut result = format!("{}{}", drive, root);
    if !segments.is_empty() {
        if !result.is_empty() && !result.ends_with('\\') && !result.ends_with(':') {
            result.push('\\');
        }
        result.push_str(&segments.join("\\"));
    }

    if result.is_empty() {
        ".".to_string()
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Existing tests ──

    #[test]
    fn posix_parts_absolute() {
        assert_eq!(
            parts("/mnt/renders/scene.exr", PathFormat::Posix),
            vec!["/", "mnt", "renders", "scene.exr"]
        );
    }

    #[test]
    fn posix_parts_relative() {
        assert_eq!(
            parts("sub/file.exr", PathFormat::Posix),
            vec!["sub", "file.exr"]
        );
    }

    #[test]
    fn posix_parts_root() {
        assert_eq!(parts("/", PathFormat::Posix), vec!["/"]);
    }

    #[test]
    fn windows_parts_drive() {
        assert_eq!(
            parts(r"C:\mnt\file.txt", PathFormat::Windows),
            vec![r"C:\", "mnt", "file.txt"]
        );
    }

    #[test]
    fn windows_parts_root_backslash() {
        assert_eq!(
            parts(r"\mnt\data\file.txt", PathFormat::Windows),
            vec![r"\", "mnt", "data", "file.txt"]
        );
    }

    #[test]
    fn windows_parts_unc() {
        assert_eq!(
            parts(r"\\server\share\dir", PathFormat::Windows),
            vec![r"\\server\share\", "dir"]
        );
    }

    #[test]
    fn posix_file_name() {
        assert_eq!(
            file_name("/mnt/renders/scene.exr", PathFormat::Posix),
            "scene.exr"
        );
    }

    #[test]
    fn posix_parent() {
        assert_eq!(
            parent("/mnt/renders/scene.exr", PathFormat::Posix),
            "/mnt/renders"
        );
    }

    #[test]
    fn posix_parent_root() {
        assert_eq!(parent("/", PathFormat::Posix), "/");
    }

    #[test]
    fn posix_file_stem() {
        assert_eq!(
            file_stem("/mnt/renders/scene.exr", PathFormat::Posix),
            "scene"
        );
    }

    #[test]
    fn posix_extension() {
        assert_eq!(
            extension("/mnt/renders/scene.exr", PathFormat::Posix),
            ".exr"
        );
    }

    #[test]
    fn no_extension() {
        assert_eq!(extension("/mnt/renders/Makefile", PathFormat::Posix), "");
    }

    #[test]
    fn posix_suffixes_single() {
        assert_eq!(suffixes("scene.exr", PathFormat::Posix), vec![".exr"]);
    }

    #[test]
    fn posix_suffixes_compound() {
        assert_eq!(
            suffixes("archive.tar.gz", PathFormat::Posix),
            vec![".tar", ".gz"]
        );
    }

    #[test]
    fn posix_suffixes_none() {
        assert_eq!(
            suffixes("Makefile", PathFormat::Posix),
            Vec::<String>::new()
        );
    }

    #[test]
    fn windows_parent_backslash() {
        assert_eq!(
            parent(r"\mnt\renders\scene.exr", PathFormat::Windows),
            r"\mnt\renders"
        );
    }

    #[test]
    fn windows_file_name_mixed_sep() {
        assert_eq!(
            file_name(r"C:\mnt/renders\scene.exr", PathFormat::Windows),
            "scene.exr"
        );
    }

    // ── POSIX parts: pathlib ground truth ──

    #[test]
    fn posix_parts_single_component() {
        assert_eq!(parts("/mnt", PathFormat::Posix), vec!["/", "mnt"]);
    }

    #[test]
    fn posix_parts_dot() {
        // PurePosixPath('.').parts == ()
        let empty: Vec<String> = vec![];
        assert_eq!(parts(".", PathFormat::Posix), empty);
    }

    #[test]
    fn posix_parts_dotdot() {
        assert_eq!(parts("..", PathFormat::Posix), vec![".."]);
    }

    #[test]
    fn posix_parts_dotdot_foo() {
        assert_eq!(parts("../foo", PathFormat::Posix), vec!["..", "foo"]);
    }

    #[test]
    fn posix_parts_repeated_separators() {
        // Collapses repeated /
        assert_eq!(
            parts("/mnt//renders///scene.exr", PathFormat::Posix),
            vec!["/", "mnt", "renders", "scene.exr"]
        );
    }

    #[test]
    fn posix_parts_double_slash_root() {
        // pathlib treats // as a special root
        assert_eq!(
            parts("//mnt/file", PathFormat::Posix),
            vec!["//", "mnt", "file"]
        );
    }

    #[test]
    fn posix_parts_trailing_slash() {
        // Trailing slash stripped
        assert_eq!(
            parts("/mnt/renders/", PathFormat::Posix),
            vec!["/", "mnt", "renders"]
        );
    }

    #[test]
    fn posix_parts_deep() {
        assert_eq!(
            parts("/a/b/c/d/e", PathFormat::Posix),
            vec!["/", "a", "b", "c", "d", "e"]
        );
    }

    #[test]
    fn posix_parts_bare_file() {
        assert_eq!(parts("file.txt", PathFormat::Posix), vec!["file.txt"]);
    }

    #[test]
    fn posix_parts_empty() {
        let empty: Vec<String> = vec![];
        assert_eq!(parts("", PathFormat::Posix), empty);
    }

    // ── POSIX properties: pathlib ground truth ──

    #[test]
    fn posix_dot_name() {
        assert_eq!(file_name(".", PathFormat::Posix), "");
    }

    #[test]
    fn posix_dot_stem() {
        assert_eq!(file_stem(".", PathFormat::Posix), "");
    }

    #[test]
    fn posix_dot_suffix() {
        assert_eq!(extension(".", PathFormat::Posix), "");
    }

    #[test]
    fn posix_dot_parent() {
        assert_eq!(parent(".", PathFormat::Posix), ".");
    }

    #[test]
    fn posix_trailing_slash_name() {
        // PurePosixPath('/mnt/renders/').name == 'renders'
        assert_eq!(file_name("/mnt/renders/", PathFormat::Posix), "renders");
    }

    #[test]
    fn posix_trailing_slash_stem() {
        assert_eq!(file_stem("/mnt/renders/", PathFormat::Posix), "renders");
    }

    #[test]
    fn posix_trailing_slash_suffix() {
        assert_eq!(extension("/mnt/renders/", PathFormat::Posix), "");
    }

    #[test]
    fn posix_trailing_slash_parent() {
        // PurePosixPath('/mnt/renders/').parent == PurePosixPath('/mnt')
        assert_eq!(parent("/mnt/renders/", PathFormat::Posix), "/mnt");
    }

    #[test]
    fn posix_hidden_tar_gz_suffixes() {
        assert_eq!(
            suffixes(".hidden.tar.gz", PathFormat::Posix),
            vec![".tar", ".gz"]
        );
    }

    #[test]
    fn posix_hidden_tar_gz_stem() {
        assert_eq!(
            file_stem(".hidden.tar.gz", PathFormat::Posix),
            ".hidden.tar"
        );
    }

    #[test]
    fn posix_hidden_tar_gz_suffix() {
        assert_eq!(extension(".hidden.tar.gz", PathFormat::Posix), ".gz");
    }

    // ── Windows parts: pathlib ground truth ──

    #[test]
    fn windows_parts_drive_root() {
        // PureWindowsPath('C:\\').parts == ('C:\\',)
        assert_eq!(parts(r"C:\", PathFormat::Windows), vec![r"C:\"]);
    }

    #[test]
    fn windows_parts_drive_file() {
        assert_eq!(
            parts(r"C:\mnt\file.txt", PathFormat::Windows),
            vec![r"C:\", "mnt", "file.txt"]
        );
    }

    #[test]
    fn windows_parts_forward_slash() {
        // Forward slashes accepted, normalized to backslash in root
        assert_eq!(
            parts("C:/path/to/file", PathFormat::Windows),
            vec![r"C:\", "path", "to", "file"]
        );
    }

    #[test]
    fn windows_parts_repeated_separators() {
        assert_eq!(
            parts("C:/path//to///file", PathFormat::Windows),
            vec![r"C:\", "path", "to", "file"]
        );
    }

    #[test]
    fn windows_parts_unc_root() {
        // PureWindowsPath('\\\\server\\share').parts == ('\\\\server\\share\\',)
        assert_eq!(
            parts(r"\\server\share", PathFormat::Windows),
            vec![r"\\server\share\"]
        );
    }

    #[test]
    fn windows_parts_unc_dir() {
        assert_eq!(
            parts(r"\\server\share\dir", PathFormat::Windows),
            vec![r"\\server\share\", "dir"]
        );
    }

    #[test]
    fn windows_parts_unc_dir_file() {
        assert_eq!(
            parts(r"\\server\share\dir\file.txt", PathFormat::Windows),
            vec![r"\\server\share\", "dir", "file.txt"]
        );
    }

    #[test]
    fn windows_parts_root_only() {
        // PureWindowsPath('\\mnt\\data\\file.txt').parts == ('\\', 'mnt', 'data', 'file.txt')
        assert_eq!(
            parts(r"\mnt\data\file.txt", PathFormat::Windows),
            vec![r"\", "mnt", "data", "file.txt"]
        );
    }

    #[test]
    fn windows_parts_unc_no_share() {
        // PureWindowsPath('\\\\server').parts == ('\\\\server',)
        // Note: no trailing backslash when there's no share
        assert_eq!(parts(r"\\server", PathFormat::Windows), vec![r"\\server"]);
    }

    #[test]
    fn windows_parts_relative_drive() {
        // PureWindowsPath('C:').parts == ('C:',)
        assert_eq!(parts("C:", PathFormat::Windows), vec!["C:"]);
    }

    #[test]
    fn windows_parts_trailing_slash() {
        // PureWindowsPath('C:\\mnt\\').parts == ('C:\\', 'mnt')
        assert_eq!(parts(r"C:\mnt\", PathFormat::Windows), vec![r"C:\", "mnt"]);
    }

    #[test]
    fn windows_parts_unc_trailing_slash() {
        // PureWindowsPath('\\\\server\\share\\').parts == ('\\\\server\\share\\',)
        assert_eq!(
            parts(r"\\server\share\", PathFormat::Windows),
            vec![r"\\server\share\"]
        );
    }

    // ── Windows properties: pathlib ground truth ──

    #[test]
    fn windows_drive_root_name() {
        // PureWindowsPath('C:\\').name == ''
        assert_eq!(file_name(r"C:\", PathFormat::Windows), "");
    }

    #[test]
    fn windows_drive_root_parent() {
        // PureWindowsPath('C:\\').parent == PureWindowsPath('C:\\')
        assert_eq!(parent(r"C:\", PathFormat::Windows), r"C:\");
    }

    #[test]
    fn windows_unc_name() {
        // PureWindowsPath('\\\\server\\share').name == ''
        assert_eq!(file_name(r"\\server\share", PathFormat::Windows), "");
    }

    #[test]
    fn windows_unc_parent() {
        // PureWindowsPath('\\\\server\\share').parent == PureWindowsPath('\\\\server\\share\\')
        // The parent of UNC root is itself (with trailing backslash)
        assert_eq!(
            parent(r"\\server\share", PathFormat::Windows),
            r"\\server\share\"
        );
    }

    #[test]
    fn windows_unc_dir_name() {
        // PureWindowsPath('\\\\server\\share\\dir').name == 'dir'
        assert_eq!(file_name(r"\\server\share\dir", PathFormat::Windows), "dir");
    }

    #[test]
    fn windows_unc_dir_parent() {
        // PureWindowsPath('\\\\server\\share\\dir').parent == PureWindowsPath('\\\\server\\share\\')
        assert_eq!(
            parent(r"\\server\share\dir", PathFormat::Windows),
            r"\\server\share\"
        );
    }

    // ── Forward-slash paths with Windows format ──
    // Forward slashes are valid separators on Windows. These paths must
    // parse identically to their backslash equivalents.

    #[test]
    fn windows_forward_slash_file_name() {
        assert_eq!(
            file_name("/input/scene.exr", PathFormat::Windows),
            "scene.exr"
        );
    }

    #[test]
    fn windows_forward_slash_file_stem() {
        assert_eq!(file_stem("/input/scene.exr", PathFormat::Windows), "scene");
    }

    #[test]
    fn windows_forward_slash_extension() {
        assert_eq!(extension("/input/scene.exr", PathFormat::Windows), ".exr");
    }

    #[test]
    fn windows_forward_slash_parent() {
        assert_eq!(parent("/input/scene.exr", PathFormat::Windows), r"\input");
    }

    #[test]
    fn windows_forward_slash_parts() {
        assert_eq!(
            parts("/input/scene.exr", PathFormat::Windows),
            vec![r"\", "input", "scene.exr"]
        );
    }
}
