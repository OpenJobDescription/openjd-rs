// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Platform-specific path mapping tests.
//!
//! Tests `apply()` and `apply_rules()` which use `PathFormat::host()` to pick
//! the output separator. These tests verify that the host-native behavior is
//! correct on each platform.

use openjd_expr::path_mapping::{
    apply_rules, apply_rules_with_format, PathFormat, PathMappingRule,
};

fn posix_rule(src: &str, dst: &str) -> PathMappingRule {
    PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: src.to_string(),
        destination_path: dst.to_string(),
    }
}

fn windows_rule(src: &str, dst: &str) -> PathMappingRule {
    PathMappingRule {
        source_path_format: PathFormat::Windows,
        source_path: src.to_string(),
        destination_path: dst.to_string(),
    }
}

fn uri_rule(src: &str, dst: &str) -> PathMappingRule {
    PathMappingRule {
        source_path_format: PathFormat::Uri,
        source_path: src.to_string(),
        destination_path: dst.to_string(),
    }
}

// =========================================================================
// PathFormat::host() sanity
// =========================================================================

#[cfg(unix)]
#[test]
fn host_format_is_posix() {
    assert_eq!(PathFormat::host(), PathFormat::Posix);
}

#[cfg(windows)]
#[test]
fn host_format_is_windows() {
    assert_eq!(PathFormat::host(), PathFormat::Windows);
}

// =========================================================================
// apply() — host-native output separator
// =========================================================================

// --- POSIX host ---

#[cfg(unix)]
mod posix_host {
    use super::*;

    #[test]
    fn apply_posix_source_exact_match() {
        let rule = posix_rule("/mnt/shared", "/local/shared");
        assert_eq!(rule.apply("/mnt/shared"), Some("/local/shared".into()));
    }

    #[test]
    fn apply_posix_source_with_child() {
        let rule = posix_rule("/mnt/shared", "/local/shared");
        // On POSIX host, child parts joined with /
        assert_eq!(
            rule.apply("/mnt/shared/dir/file.txt"),
            Some("/local/shared/dir/file.txt".into())
        );
    }

    #[test]
    fn apply_posix_source_trailing_slash_preserved() {
        let rule = posix_rule("/mnt/shared", "/local/shared");
        assert_eq!(
            rule.apply("/mnt/shared/dir/"),
            Some("/local/shared/dir/".into())
        );
    }

    #[test]
    fn apply_windows_source_on_posix_host() {
        // A Windows source rule should still match Windows-formatted input,
        // but output uses POSIX separators on a POSIX host.
        let rule = windows_rule(r"C:\projects", "/local/projects");
        assert_eq!(
            rule.apply(r"C:\projects\scene\file.ma"),
            Some("/local/projects/scene/file.ma".into())
        );
    }

    #[test]
    fn apply_windows_source_case_insensitive_on_posix_host() {
        let rule = windows_rule(r"C:\Projects", "/local/projects");
        assert_eq!(
            rule.apply(r"c:\projects\file.txt"),
            Some("/local/projects/file.txt".into())
        );
    }

    #[test]
    fn apply_uri_source_on_posix_host() {
        let rule = uri_rule("s3://bucket/assets", "/local/assets");
        assert_eq!(
            rule.apply("s3://bucket/assets/teapot.obj"),
            Some("/local/assets/teapot.obj".into())
        );
    }

    #[test]
    fn apply_no_match_returns_none() {
        let rule = posix_rule("/mnt/shared", "/local/shared");
        assert_eq!(rule.apply("/other/path"), None);
    }

    #[test]
    fn apply_rules_first_match_wins() {
        let rules = vec![
            posix_rule("/mnt/a", "/local/a"),
            posix_rule("/mnt", "/local"),
        ];
        // /mnt/a/file matches the first rule
        assert_eq!(apply_rules(&rules, "/mnt/a/file"), "/local/a/file");
    }

    #[test]
    fn apply_rules_no_match_returns_original() {
        let rules = vec![posix_rule("/mnt/shared", "/local/shared")];
        assert_eq!(apply_rules(&rules, "/other/path"), "/other/path");
    }

    #[test]
    fn apply_rules_empty_rules_returns_original() {
        assert_eq!(apply_rules(&[], "/any/path"), "/any/path");
    }
}

// --- Windows host ---

#[cfg(windows)]
mod windows_host {
    use super::*;

    #[test]
    fn apply_windows_source_exact_match() {
        let rule = windows_rule(r"C:\projects", r"D:\local\projects");
        assert_eq!(
            rule.apply(r"C:\projects"),
            Some(r"D:\local\projects".into())
        );
    }

    #[test]
    fn apply_windows_source_with_child() {
        let rule = windows_rule(r"C:\projects", r"D:\local\projects");
        // On Windows host, child parts joined with backslash
        assert_eq!(
            rule.apply(r"C:\projects\scene\file.ma"),
            Some(r"D:\local\projects\scene\file.ma".into())
        );
    }

    #[test]
    fn apply_windows_source_trailing_backslash_preserved() {
        let rule = windows_rule(r"C:\projects", r"D:\local\projects");
        assert_eq!(
            rule.apply(r"C:\projects\dir\"),
            Some(r"D:\local\projects\dir\".into())
        );
    }

    #[test]
    fn apply_windows_source_trailing_forward_slash_preserved() {
        let rule = windows_rule(r"C:\projects", r"D:\local\projects");
        assert_eq!(
            rule.apply(r"C:\projects\dir/"),
            Some(r"D:\local\projects\dir\".into())
        );
    }

    #[test]
    fn apply_windows_source_case_insensitive() {
        let rule = windows_rule(r"C:\Projects", r"D:\local");
        assert_eq!(
            rule.apply(r"c:\projects\file.txt"),
            Some(r"D:\local\file.txt".into())
        );
    }

    #[test]
    fn apply_windows_source_mixed_separators_in_input() {
        let rule = windows_rule(r"C:\projects", r"D:\local");
        // Input uses forward slashes — split_path_parts handles both
        assert_eq!(
            rule.apply("C:/projects/scene/file.ma"),
            Some(r"D:\local\scene\file.ma".into())
        );
    }

    #[test]
    fn apply_posix_source_on_windows_host() {
        // A POSIX source rule matching POSIX-formatted input,
        // but output uses Windows separators on a Windows host.
        let rule = posix_rule("/mnt/shared", r"D:\local\shared");
        assert_eq!(
            rule.apply("/mnt/shared/dir/file.txt"),
            Some(r"D:\local\shared\dir\file.txt".into())
        );
    }

    #[test]
    fn apply_posix_source_case_sensitive_on_windows_host() {
        // POSIX source matching is always case-sensitive, even on Windows host
        let rule = posix_rule("/mnt/Shared", r"D:\local");
        assert_eq!(rule.apply("/mnt/shared/file.txt"), None);
        assert_eq!(
            rule.apply("/mnt/Shared/file.txt"),
            Some(r"D:\local\file.txt".into())
        );
    }

    #[test]
    fn apply_uri_source_on_windows_host() {
        let rule = uri_rule("s3://bucket/assets", r"D:\local\assets");
        assert_eq!(
            rule.apply("s3://bucket/assets/teapot.obj"),
            Some(r"D:\local\assets\teapot.obj".into())
        );
    }

    #[test]
    fn apply_uri_trailing_slash_uses_backslash_on_windows() {
        let rule = uri_rule("s3://bucket", r"D:\local");
        assert_eq!(
            rule.apply("s3://bucket/dir/"),
            Some(r"D:\local\dir\".into())
        );
    }

    #[test]
    fn apply_no_match_returns_none() {
        let rule = windows_rule(r"C:\projects", r"D:\local");
        assert_eq!(rule.apply(r"D:\other\path"), None);
    }

    #[test]
    fn apply_unc_path() {
        let rule = windows_rule(r"\\server\share\assets", r"Z:\assets");
        assert_eq!(
            rule.apply(r"\\server\share\assets\file.exr"),
            Some(r"Z:\assets\file.exr".into())
        );
    }

    #[test]
    fn apply_rules_first_match_wins() {
        let rules = vec![
            windows_rule(r"C:\projects\a", r"D:\a"),
            windows_rule(r"C:\projects", r"D:\all"),
        ];
        assert_eq!(
            apply_rules(&rules, r"C:\projects\a\file.txt"),
            r"D:\a\file.txt"
        );
    }

    #[test]
    fn apply_rules_no_match_returns_original() {
        let rules = vec![windows_rule(r"C:\projects", r"D:\local")];
        assert_eq!(apply_rules(&rules, r"E:\other"), r"E:\other");
    }

    #[test]
    fn apply_rules_empty_rules_returns_original() {
        assert_eq!(apply_rules(&[], r"C:\any\path"), r"C:\any\path");
    }
}

// =========================================================================
// apply_rules_with_format — cross-platform (runs on both)
// =========================================================================

#[test]
fn apply_rules_with_format_posix_output() {
    let rules = vec![windows_rule(r"C:\projects", "/mnt/projects")];
    assert_eq!(
        apply_rules_with_format(&rules, r"C:\projects\scene\file.ma", PathFormat::Posix),
        "/mnt/projects/scene/file.ma"
    );
}

#[test]
fn apply_rules_with_format_windows_output() {
    let rules = vec![posix_rule("/mnt/shared", r"C:\local\shared")];
    assert_eq!(
        apply_rules_with_format(&rules, "/mnt/shared/dir/file.txt", PathFormat::Windows),
        r"C:\local\shared\dir\file.txt"
    );
}

#[test]
fn apply_rules_with_format_uri_to_posix() {
    let rules = vec![uri_rule("s3://bucket/assets", "/local/assets")];
    assert_eq!(
        apply_rules_with_format(&rules, "s3://bucket/assets/file.obj", PathFormat::Posix),
        "/local/assets/file.obj"
    );
}

#[test]
fn apply_rules_with_format_uri_to_windows() {
    let rules = vec![uri_rule("s3://bucket/assets", r"D:\cache\assets")];
    assert_eq!(
        apply_rules_with_format(&rules, "s3://bucket/assets/file.obj", PathFormat::Windows),
        r"D:\cache\assets\file.obj"
    );
}

#[test]
fn apply_rules_with_format_no_match() {
    let rules = vec![posix_rule("/mnt/a", "/local/a")];
    assert_eq!(
        apply_rules_with_format(&rules, "/other/path", PathFormat::Posix),
        "/other/path"
    );
}
