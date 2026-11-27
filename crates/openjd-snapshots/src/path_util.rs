// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use std::collections::HashSet;

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

// --- Directory rules for normalized manifest paths ---
//
// Manifest paths are normalized by `normalize_path` and use `/` on every
// platform, so these rules work on the strings. `std::path::Path` is not used
// for them because its parent and root rules depend on the host OS: on POSIX
// the parent of `C:/foo` is `C:`, not the drive root `C:/`.

/// Returns the parent directory of a (normalized) path, using `/` separators.
///
/// - `/foo/bar/baz` → `/foo/bar`
/// - `/foo` → `/` (immediate child of absolute root)
/// - `/` → `/` (root's parent is itself; defensive — shouldn't arise in a file path)
/// - `C:/foo/bar` → `C:/foo`
/// - `C:/foo` → `C:/` (immediate child of drive root — preserve the slash so
///   the caller can distinguish the drive root from a bare drive specifier)
/// - `foo/bar` → `foo` (relative)
/// - `foo` → `"."` (immediate child of the relative-path root)
/// - `"."` → `"."` (relative root's parent is itself; defensive)
/// - `""` → `""` (defensive)
pub(crate) fn parent_dir(path: &str) -> &str {
    if path.is_empty() || path == "/" || path == "." {
        return path;
    }
    match path.rfind('/') {
        None => ".",    // relative, no slash: parent is the relative root
        Some(0) => "/", // absolute, single slash at start: parent is root
        Some(2) if is_drive_letter_prefix(path) => &path[..3], // `C:/foo` → `C:/`
        Some(i) => &path[..i],
    }
}

/// Returns true if `path` starts with a Windows drive-letter prefix like
/// `C:/`. Used to recognize drive roots so their trailing `/` is preserved
/// (to distinguish the drive root `C:/` from the bare drive `C:`).
pub(crate) fn is_drive_letter_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/'
}

/// Returns true if the path is a root path whose trailing `/` must be kept:
/// either absolute root `/` or a Windows drive root like `C:/`.
pub(crate) fn is_root_path(path: &str) -> bool {
    path == "/" || (path.len() == 3 && is_drive_letter_prefix(path))
}

/// Returns the root that a normalized path hangs from: `/` for a POSIX
/// absolute path, the drive root (`C:/`) for a drive-letter path, and `.` for
/// a relative path. Paths under different volume roots share no directory.
pub(crate) fn volume_root(path: &str) -> &str {
    if path.starts_with('/') {
        "/"
    } else if is_drive_letter_prefix(path) {
        &path[..3]
    } else {
        "."
    }
}

/// Returns true if `path` is a volume root: `/`, a drive root, or `.`.
pub(crate) fn is_volume_root(path: &str) -> bool {
    path == "." || is_root_path(path)
}

/// The strict ancestor directories of a normalized path, nearest first,
/// ending with its volume root (see [`volume_root`]). A volume root has none.
///
/// `/a/b/c` → `/a/b`, `/a`, `/`; `C:/a/b` → `C:/a`, `C:/`; `a/b` → `a`, `.`.
pub(crate) fn ancestor_dirs(path: &str) -> impl Iterator<Item = &str> {
    let mut current = path;
    std::iter::from_fn(move || {
        if current.is_empty() || is_volume_root(current) {
            return None;
        }
        current = parent_dir(current);
        Some(current)
    })
}

/// The directories that the given paths imply: every strict ancestor of
/// each path, volume roots included. A directory entry that is not in this
/// set has no descendant among `paths`, so it is an empty directory.
pub(crate) fn implied_dirs<'a>(paths: impl IntoIterator<Item = &'a str>) -> HashSet<&'a str> {
    let mut implied = HashSet::new();
    for p in paths {
        for a in ancestor_dirs(p) {
            // Ancestors are visited nearest first, so once one is known, the
            // rest of the chain is too.
            if !implied.insert(a) {
                break;
            }
        }
    }
    implied
}

/// Returns the longest directory prefix that is an ancestor (or equal to)
/// every path in `paths`. `paths` must be non-empty.
///
/// The result has no trailing slash, except when the result is a root path
/// (POSIX `/` or a Windows drive root like `C:/`). If the computed boundary
/// is an empty byte range (no shared directory prefix among relative paths),
/// returns `"."` — the relative root — so the result is always a valid path.
/// Paths under different volume roots have no common directory; callers
/// that may mix volumes group the paths by [`volume_root`] first.
pub(crate) fn longest_common_dir_prefix(paths: &[&str]) -> String {
    debug_assert!(!paths.is_empty());

    // Step 1: find the longest common byte prefix `L` across all paths.
    let first = paths[0];
    let mut common_len = first.len();
    for &p in &paths[1..] {
        let a = first.as_bytes();
        let b = p.as_bytes();
        let upto = common_len.min(b.len());
        let mut i = 0;
        while i < upto && a[i] == b[i] {
            i += 1;
        }
        common_len = i;
        if common_len == 0 {
            break;
        }
    }

    // Step 2: determine whether byte `common_len` sits at a directory boundary.
    //
    // The common byte prefix is a valid directory iff every path that is
    // strictly longer than `common_len` has `/` at position `common_len`.
    // (If all paths have exactly length `common_len`, that's also a valid
    // boundary — they're all equal, so the common path IS the result.)
    //
    // Otherwise, the common byte prefix ends mid-component (e.g. `/app/ba`
    // from `["/app/bar", "/app/baz"]`, or `/a` from `["/a", "/ab"]`) and we
    // must walk back to the last `/`.
    let at_boundary = paths
        .iter()
        .filter(|p| p.len() > common_len)
        .all(|p| p.as_bytes()[common_len] == b'/');

    let boundary = if at_boundary {
        common_len
    } else {
        // Walk back to the last `/` at or before common_len. If there is no
        // slash at all, truncate to zero.
        first[..common_len]
            .rfind('/')
            .map(|i| i + 1) // include the slash itself
            .unwrap_or(0)
    };

    // Step 3: strip trailing `/` unless the result *is* a root path.
    // Root paths keep their trailing `/`:
    //   - "/" is the POSIX absolute root
    //   - "C:/" (for any drive letter) is the root of a Windows drive
    //
    // For relative inputs that don't share any common directory prefix, the
    // boundary is 0 and the result would be an empty string. Return "." in
    // that case so the result is always a valid path that can be passed to
    // operations like `subtree_snapshot` as the subtree root.
    let result = &first[..boundary];
    if is_root_path(result) {
        result.to_string()
    } else if result.is_empty() {
        ".".to_string()
    } else if let Some(stripped) = result.strip_suffix('/') {
        stripped.to_string()
    } else {
        result.to_string()
    }
}

/// Returns `path` relative to `root` if `path` is `root` or lies beneath it,
/// or `None` otherwise. `path` equal to `root` gives `""`.
///
/// A root of `.` or `""` contains every path. A root that ends in `/` (`/`,
/// `C:/`) is matched as a prefix as-is; any other root matches when followed
/// by `/`, so `/a` contains `/a/b` but not `/ab`.
pub(crate) fn strip_root<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    if root.is_empty() || root == "." {
        return Some(path);
    }
    if path == root {
        return Some("");
    }
    if root.ends_with('/') {
        path.strip_prefix(root)
    } else {
        path.strip_prefix(root)?.strip_prefix('/')
    }
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

    // ----- Additional cases moved from integration tests -----

    #[test]
    fn is_absolute_windows_unc() {
        assert!(is_absolute_path("//server/share/file.txt"));
    }

    #[test]
    fn is_absolute_rejects_relative_with_dotdot() {
        assert!(!is_absolute_path("../file.txt"));
        assert!(!is_absolute_path("./file.txt"));
        assert!(!is_absolute_path("assets/file.txt"));
        assert!(!is_absolute_path("file.txt"));
    }

    #[test]
    fn normalize_collapses_double_slash() {
        assert_eq!(normalize_path("assets//textures"), "assets/textures");
    }

    #[cfg(windows)]
    #[test]
    fn normalize_windows_converts_nested_backslashes() {
        assert_eq!(
            normalize_path("assets\\textures\\wood"),
            "assets/textures/wood"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn normalize_posix_preserves_nested_backslashes() {
        // On POSIX, backslashes are valid filename characters
        assert_eq!(
            normalize_path("assets\\textures\\wood"),
            "assets\\textures\\wood"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn normalize_posix_preserves_backslashes_after_drive_prefix() {
        // "C:\projects\scene" has drive prefix "C:", so byte[2] ('\') is skipped
        // as separator, and "projects\scene" becomes a single component (no '/' to
        // split on) with backslashes preserved as valid filename chars.
        assert_eq!(normalize_path("C:\\projects\\scene"), "C:/projects\\scene");
    }

    #[test]
    fn normalize_preserves_forward_slashes() {
        assert_eq!(normalize_path("dir/name"), "dir/name");
    }

    #[test]
    fn is_absolute_unc_variants() {
        assert!(is_absolute_path("//server/share"));
        assert!(is_absolute_path("\\\\server\\share"));
    }
}
