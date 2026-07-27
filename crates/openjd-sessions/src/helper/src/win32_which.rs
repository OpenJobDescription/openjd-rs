// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Windows command search mirroring Python's `shutil.which`.
//!
//! This file is shared by both Windows spawn paths so their search
//! semantics are identical by construction: the session crate includes it
//! via `#[path]` from `win32_locate.rs` (same-user actions), and the
//! embedded helper compiles it as a module (cross-user actions).
//!
//! It exists instead of the `which` crate because resolution here must
//! honor the *action's* PATHEXT, not the resolving process's: `which`
//! always reads PATHEXT from the process environment, and it accepts any
//! existing file when the command has an explicit extension — whereas
//! `shutil.which` (and cmd.exe) treat an extension outside PATHEXT as
//! not-runnable and report not-found.

use std::path::{Path, PathBuf};

/// Default PATHEXT when none is available, matching Python
/// `shutil._WIN_DEFAULT_PATHEXT`.
pub const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD;.VBS;.JS;.WS;.MSC";

/// Search for `command` with `shutil.which` semantics.
///
/// - `search_path` is a `;`-separated directory list, searched in order;
///   within each directory every candidate extension is tried before
///   moving to the next directory (earliest directory wins).
/// - `pathext` is a `;`-separated extension list. An empty or blank value
///   selects [`DEFAULT_PATHEXT`] (mirroring `shutil.which`, and cmd.exe's
///   own treatment of an unset PATHEXT).
/// - If `command` already ends with one of the PATHEXT extensions
///   (case-insensitive), it is looked up as-is; otherwise each extension
///   is appended in PATHEXT order. A command whose explicit extension is
///   *not* in PATHEXT is therefore never matched by its literal name —
///   `script.ps1` with a default PATHEXT returns `None`, exactly like
///   `shutil.which`.
/// - A command containing a path separator is resolved against `cwd`
///   only (no PATH search), with the same extension rules.
pub fn locate_in(command: &str, search_path: &str, pathext: &str, cwd: &Path) -> Option<PathBuf> {
    let pathext = if pathext.trim().is_empty() {
        DEFAULT_PATHEXT
    } else {
        pathext
    };
    let exts: Vec<&str> = pathext.split(';').filter(|e| !e.is_empty()).collect();
    let cmd_lower = command.to_lowercase();
    let has_listed_ext = exts.iter().any(|e| cmd_lower.ends_with(&e.to_lowercase()));
    let candidates: Vec<String> = if has_listed_ext {
        vec![command.to_string()]
    } else {
        exts.iter().map(|e| format!("{command}{e}")).collect()
    };

    if command.contains('\\') || command.contains('/') {
        for cand in &candidates {
            let p = cwd.join(cand);
            if p.is_file() {
                return Some(p);
            }
        }
        return None;
    }

    for dir in search_path.split(';').filter(|d| !d.is_empty()) {
        let dir = Path::new(dir);
        for cand in &candidates {
            let p = dir.join(cand);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compare two optional paths case-insensitively: the search returns
    /// the candidate's PATHEXT casing (e.g. `tool.BAT` for an on-disk
    /// `tool.bat`), exactly like Python's shutil.which; NTFS resolves it
    /// case-insensitively.
    fn assert_path_eq(actual: Option<PathBuf>, expected: Option<PathBuf>, msg: &str) {
        let norm = |p: Option<PathBuf>| p.map(|p| p.to_string_lossy().to_lowercase());
        assert_eq!(norm(actual), norm(expected), "{msg}");
    }

    fn touch(path: &Path) {
        std::fs::write(path, "").unwrap();
    }

    fn tempdir() -> std::path::PathBuf {
        // No tempfile dependency in the helper crate: use a unique dir under
        // the OS temp dir keyed by test name via a counter + PID.
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!(
            "openjd-win32-which-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// PATHEXT is honored per directory: earliest directory wins across
    /// all extensions.
    #[test]
    fn earliest_directory_wins_across_extensions() {
        let root = tempdir();
        let (a, b) = (root.join("a"), root.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        touch(&a.join("tool.bat"));
        touch(&b.join("tool.exe"));
        let sp = format!("{};{}", a.display(), b.display());
        let found = locate_in("tool", &sp, ".EXE;.BAT", &root);
        assert_path_eq(found, Some(a.join("tool.bat")), "earliest dir wins");
    }

    /// The caller's PATHEXT restricts candidates: with PATHEXT=.EXE a
    /// `.bat` earlier in PATH is NOT runnable and the later `.exe` wins.
    /// (The action's PATHEXT must be honored, not the resolving
    /// process's.)
    #[test]
    fn action_pathext_restricts_candidates() {
        let root = tempdir();
        let (a, b) = (root.join("a"), root.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        touch(&a.join("tool.bat"));
        touch(&b.join("tool.exe"));
        let sp = format!("{};{}", a.display(), b.display());
        let found = locate_in("tool", &sp, ".EXE", &root);
        assert_path_eq(
            found,
            Some(b.join("tool.exe")),
            "PATHEXT=.EXE excludes .bat",
        );
        assert_eq!(
            locate_in("tool", &a.to_string_lossy(), ".EXE", &root),
            None,
            "a .bat-only match is not runnable under PATHEXT=.EXE"
        );
    }

    /// An explicit extension outside PATHEXT is not runnable — not-found,
    /// matching Python's shutil.which (and cmd.exe). The file existing is
    /// not sufficient.
    #[test]
    fn explicit_extension_outside_pathext_is_not_found() {
        let root = tempdir();
        touch(&root.join("script.ps1"));
        let sp = root.to_string_lossy().into_owned();
        assert_eq!(
            locate_in("script.ps1", &sp, DEFAULT_PATHEXT, &root),
            None,
            ".ps1 is not in the default PATHEXT"
        );
        // ...but IS runnable when the action's PATHEXT includes .PS1.
        assert_path_eq(
            locate_in("script.ps1", &sp, ".EXE;.PS1", &root),
            Some(root.join("script.ps1")),
            "explicit .ps1 runnable when PATHEXT lists it",
        );
    }

    /// A command with a listed explicit extension is looked up as-is;
    /// extension matching is case-insensitive.
    #[test]
    fn explicit_listed_extension_matches_case_insensitively() {
        let root = tempdir();
        touch(&root.join("tool.BAT"));
        let sp = root.to_string_lossy().into_owned();
        assert_path_eq(
            locate_in("tool.bat", &sp, ".COM;.EXE;.BAT", &root),
            Some(root.join("tool.bat")),
            "explicit .bat is in PATHEXT and matches on disk case-insensitively",
        );
    }

    /// Empty/blank PATHEXT selects the shutil.which default list.
    #[test]
    fn empty_pathext_uses_default() {
        let root = tempdir();
        touch(&root.join("tool.exe"));
        let sp = root.to_string_lossy().into_owned();
        assert_path_eq(
            locate_in("tool", &sp, "", &root),
            Some(root.join("tool.exe")),
            "default PATHEXT finds .exe",
        );
        assert_eq!(
            locate_in("script.ps1", &sp, "", &root),
            None,
            ".ps1 is outside the default PATHEXT"
        );
    }

    /// A command containing a path separator resolves against cwd only —
    /// PATH directories are not searched.
    #[test]
    fn relative_path_resolves_against_cwd_only() {
        let root = tempdir();
        let (sub, elsewhere) = (root.join("sub"), root.join("elsewhere"));
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        touch(&sub.join("tool.bat"));
        touch(&elsewhere.join("sub-tool.bat"));
        assert_path_eq(
            locate_in(r"sub\tool", &elsewhere.to_string_lossy(), ".BAT", &root),
            Some(root.join(r"sub\tool.bat")),
            "pathy command resolves against cwd",
        );
        assert_eq!(
            locate_in(r"missing\tool", &elsewhere.to_string_lossy(), ".BAT", &root),
            None,
            "a pathy command is never searched on PATH"
        );
    }
}
