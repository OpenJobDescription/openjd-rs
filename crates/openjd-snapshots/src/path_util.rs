// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

/// Normalizes a path string, collapses `.` and `..` components, and strips
/// trailing slashes (except root `/`).
/// On Windows, backslashes are converted to forward slashes and `\\?\` prefix is stripped.
/// On POSIX, backslashes are valid filename characters and preserved as-is.
pub fn normalize_path(path: &str) -> String {
    // On Windows, convert backslashes to forward slashes and strip \\?\ prefix.
    // On POSIX, backslashes are valid filename characters and preserved as-is.
    #[cfg(windows)]
    let path = {
        let mut p = path.replace('\\', "/");
        if p.starts_with("//?/") {
            p = p[4..].to_string();
        }
        p
    };
    #[cfg(windows)]
    let path = path.as_str();

    if path.is_empty() {
        return String::new();
    }

    let drive_prefix = if path.len() >= 2
        && path.as_bytes()[0].is_ascii_alphabetic()
        && path.as_bytes()[1] == b':'
    {
        Some(&path[..2])
    } else {
        None
    };
    let is_abs = path.starts_with('/') || drive_prefix.is_some();

    // Skip the drive letter component when splitting
    let path_to_split = if let Some(d) = drive_prefix {
        if path.len() > d.len() {
            &path[d.len() + 1..]
        } else {
            ""
        }
    } else {
        path
    };

    let mut parts: Vec<&str> = Vec::new();
    for component in path_to_split.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if is_abs {
                    parts.pop();
                } else if parts.last().is_none_or(|&p| p == "..") {
                    parts.push("..");
                } else {
                    parts.pop();
                }
            }
            c => parts.push(c),
        }
    }

    if is_abs {
        if let Some(d) = drive_prefix {
            if parts.is_empty() {
                format!("{d}/")
            } else {
                format!("{d}/{}", parts.join("/"))
            }
        } else {
            format!("/{}", parts.join("/"))
        }
    } else if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

/// Returns true if path starts with `/` or `\\` (UNC), or matches a Windows drive letter pattern like `C:` or `C:/`.
pub fn is_absolute_path(path: &str) -> bool {
    #[cfg(windows)]
    let path = path.replace('\\', "/");
    #[cfg(windows)]
    let path = path.as_str();

    path.starts_with('/')
        || path.starts_with("\\\\")
        || (path.len() >= 2
            && path.as_bytes()[0].is_ascii_alphabetic()
            && path.as_bytes()[1] == b':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_simple() {
        assert_eq!(normalize_path("a/b/c"), "a/b/c");
    }

    #[test]
    fn normalize_dot() {
        assert_eq!(normalize_path("a/./b"), "a/b");
    }

    #[test]
    fn normalize_dotdot() {
        assert_eq!(normalize_path("a/b/../c"), "a/c");
    }

    #[test]
    fn normalize_trailing_slash() {
        assert_eq!(normalize_path("a/b/"), "a/b");
    }

    #[test]
    fn normalize_root() {
        assert_eq!(normalize_path("/"), "/");
    }

    #[test]
    fn normalize_absolute() {
        assert_eq!(normalize_path("/a/b/../c"), "/a/c");
    }

    #[test]
    fn normalize_empty() {
        assert_eq!(normalize_path(""), "");
    }

    #[test]
    fn normalize_only_dots() {
        assert_eq!(normalize_path("./././."), ".");
    }

    #[cfg(windows)]
    #[test]
    fn normalize_windows_backslash() {
        assert_eq!(normalize_path("a\\b\\c"), "a/b/c");
    }

    #[cfg(not(windows))]
    #[test]
    fn normalize_posix_preserves_backslash() {
        assert_eq!(normalize_path("a\\b\\c"), "a\\b\\c");
    }

    #[cfg(windows)]
    #[test]
    fn normalize_windows_drive() {
        assert_eq!(normalize_path("C:\\Users\\test"), "C:/Users/test");
    }

    #[cfg(windows)]
    #[test]
    fn normalize_windows_unc_prefix() {
        assert_eq!(normalize_path("\\\\?\\C:\\foo\\bar"), "C:/foo/bar");
    }

    #[test]
    fn normalize_dotdot_at_root() {
        assert_eq!(normalize_path("/../a"), "/a");
    }

    #[test]
    fn normalize_relative_dotdot_beyond() {
        assert_eq!(normalize_path("a/../../b"), "../b");
    }

    #[test]
    fn is_absolute_unix() {
        assert!(is_absolute_path("/foo/bar"));
    }

    #[test]
    fn is_absolute_windows() {
        assert!(is_absolute_path("C:/Users"));
        assert!(is_absolute_path("C:\\Users"));
        assert!(is_absolute_path("C:"));
    }

    #[test]
    fn is_not_absolute() {
        assert!(!is_absolute_path("foo/bar"));
        assert!(!is_absolute_path(""));
        assert!(!is_absolute_path("./foo"));
    }

    #[test]
    fn normalize_bare_drive_letter() {
        assert_eq!(normalize_path("C:"), "C:/");
    }

    #[cfg(not(windows))]
    #[test]
    fn normalize_posix_backslash_in_filename() {
        // On POSIX, backslash is a valid filename character
        assert_eq!(normalize_path("dir/file\\name.txt"), "dir/file\\name.txt");
    }
}
