// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! URI-aware path operations.
//!
//! Mirrors Python `openjd.expr._uri_path`. When a path value contains a URI
//! (scheme://authority/path), these functions handle path operations instead of
//! `std::path`. The scheme+authority prefix is preserved as an opaque root.

/// Parsed URI: authority (`scheme://host`) and path segments.
#[derive(Debug, Clone)]
pub struct UriParts {
    pub authority: String,
    pub path_parts: Vec<String>,
}

/// Return `true` if `path` has a `scheme://` prefix.
pub fn is_uri(path: &str) -> bool {
    parse(path).is_some()
}

/// Parse a URI into authority + path parts, or `None` if not a URI.
pub fn parse(path: &str) -> Option<UriParts> {
    let scheme_end = path.find("://")?;
    let scheme = &path[..scheme_end];
    if scheme.is_empty() || !scheme.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }
    if !scheme
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '.' || c == '-')
    {
        return None;
    }
    let after_scheme = &path[scheme_end + 3..];
    let (authority_part, path_part) = match after_scheme.find('/') {
        Some(i) => (&after_scheme[..i], &after_scheme[i + 1..]),
        None => (after_scheme, ""),
    };
    let authority = format!("{}://{}", scheme, authority_part);
    let path_parts = if path_part.is_empty() {
        Vec::new()
    } else {
        path_part.split('/').map(|s| s.to_string()).collect()
    };
    Some(UriParts {
        authority,
        path_parts,
    })
}

/// Final component of a URI path (empty string if no path segments).
pub fn name(path: &str) -> String {
    parse(path)
        .and_then(|u| u.path_parts.last().cloned())
        .unwrap_or_default()
}

/// Parent of a URI path.
pub fn parent(path: &str) -> String {
    let Some(uri) = parse(path) else {
        return path.to_string();
    };
    if uri.path_parts.is_empty() {
        return uri.authority;
    }
    let parent_parts = &uri.path_parts[..uri.path_parts.len() - 1];
    if parent_parts.is_empty() {
        uri.authority
    } else {
        format!("{}/{}", uri.authority, parent_parts.join("/"))
    }
}

/// File extension of the final component (including the dot), or empty string.
///
/// Matches Python pathlib's rule (see
/// `crate::functions::path_parse::extension`): the suffix is empty
/// when the rightmost `.` is at the start of the name (`.hidden`)
/// or at the end (`foo.`).
pub fn suffix(path: &str) -> String {
    let n = name(path);
    n.rfind('.')
        .filter(|&i| i > 0 && i + 1 < n.len())
        .map(|i| n[i..].to_string())
        .unwrap_or_default()
}

/// All file extensions of the final component.
///
/// Matches Python pathlib's algorithm exactly — see the doc on
/// `crate::functions::path_parse::suffixes` for the algorithm and
/// its corollaries (trailing-dot names, leading-dot names, the
/// `..foo` quirk).
pub fn suffixes(path: &str) -> Vec<String> {
    let n = name(path);
    if n.ends_with('.') {
        return Vec::new();
    }
    let trimmed = n.trim_start_matches('.');
    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.len() <= 1 {
        return Vec::new();
    }
    parts[1..].iter().map(|p| format!(".{p}")).collect()
}

/// Final component without the last extension.
///
/// Same trailing-dot rule as `suffix` — see that function.
pub fn stem(path: &str) -> String {
    let n = name(path);
    n.rfind('.')
        .filter(|&i| i > 0 && i + 1 < n.len())
        .map(|i| n[..i].to_string())
        .unwrap_or(n)
}

/// Split into parts: first element is `scheme://authority`, rest are path segments.
pub fn parts(path: &str) -> Vec<String> {
    let Some(uri) = parse(path) else {
        return vec![path.to_string()];
    };
    let mut result = vec![uri.authority];
    result.extend(uri.path_parts);
    result
}

/// Join a URI path with child segments.
pub fn join(path: &str, child: &str) -> String {
    let Some(uri) = parse(path) else {
        return format!("{path}/{child}");
    };
    let mut p = uri.path_parts;
    // Remove trailing empty part (from trailing slash) before appending
    if p.last().is_some_and(|s| s.is_empty()) {
        p.pop();
    }
    format!("{}/{}/{child}", uri.authority, p.join("/"))
}

/// Reconstruct a URI from parts (first element is `scheme://authority`).
pub fn from_parts(parts: &[String]) -> String {
    if parts.is_empty() {
        return String::new();
    }
    if parts.len() == 1 {
        return parts[0].clone();
    }
    format!("{}/{}", parts[0], parts[1..].join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_uri() {
        assert!(!is_uri("/local/path"));
    }
    #[test]
    fn not_uri_windows() {
        assert!(!is_uri("C:\\path"));
    }
    #[test]
    fn s3_is_uri() {
        assert!(is_uri("s3://bucket/key"));
    }
    #[test]
    fn https_is_uri() {
        assert!(is_uri("https://host/path"));
    }

    #[test]
    fn parse_s3() {
        let u = parse("s3://bucket/dir/file.txt").unwrap();
        assert_eq!(u.authority, "s3://bucket");
        assert_eq!(u.path_parts, vec!["dir", "file.txt"]);
    }
    #[test]
    fn parse_bare() {
        let u = parse("s3://bucket").unwrap();
        assert_eq!(u.authority, "s3://bucket");
        assert!(u.path_parts.is_empty());
    }

    #[test]
    fn name_basic() {
        assert_eq!(name("s3://bucket/dir/file.txt"), "file.txt");
    }
    #[test]
    fn name_bare() {
        assert_eq!(name("s3://bucket"), "");
    }
    #[test]
    fn name_trailing_slash() {
        assert_eq!(name("s3://bucket/dir/"), "");
    }

    #[test]
    fn parent_basic() {
        assert_eq!(parent("s3://bucket/dir/file.txt"), "s3://bucket/dir");
    }
    #[test]
    fn parent_single() {
        assert_eq!(parent("s3://bucket/file.txt"), "s3://bucket");
    }
    #[test]
    fn parent_bare() {
        assert_eq!(parent("s3://bucket"), "s3://bucket");
    }

    #[test]
    fn suffix_basic() {
        assert_eq!(suffix("s3://bucket/file.tar.gz"), ".gz");
    }
    #[test]
    fn suffix_none() {
        assert_eq!(suffix("s3://bucket/file"), "");
    }

    #[test]
    fn suffixes_compound() {
        assert_eq!(suffixes("s3://bucket/file.tar.gz"), vec![".tar", ".gz"]);
    }
    #[test]
    fn suffixes_none() {
        assert_eq!(suffixes("s3://bucket/file"), Vec::<String>::new());
    }

    #[test]
    fn stem_basic() {
        assert_eq!(stem("s3://bucket/file.tar.gz"), "file.tar");
    }
    #[test]
    fn stem_no_ext() {
        assert_eq!(stem("s3://bucket/file"), "file");
    }

    #[test]
    fn parts_basic() {
        assert_eq!(
            parts("s3://bucket/dir/file"),
            vec!["s3://bucket", "dir", "file"]
        );
    }
    #[test]
    fn parts_bare() {
        assert_eq!(parts("s3://bucket"), vec!["s3://bucket"]);
    }

    #[test]
    fn from_parts_basic() {
        assert_eq!(
            from_parts(&["s3://bucket".into(), "dir".into(), "file".into()]),
            "s3://bucket/dir/file"
        );
    }
    #[test]
    fn from_parts_bare() {
        assert_eq!(from_parts(&["s3://bucket".into()]), "s3://bucket");
    }
}
