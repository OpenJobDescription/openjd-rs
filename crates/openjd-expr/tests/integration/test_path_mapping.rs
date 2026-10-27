// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test_path_mapping.py

// These tests exercise host-context registration for apply_path_mapping.
// All call sites go through the non-deprecated FunctionLibrary::for_profile.

use openjd_expr::{
    ExprProfile, ExprValue, FunctionLibrary, HostContext, PathFormat, PathMapper, PathMappingRule,
    SymbolTable,
};

/// Apply one rule through a [`PathMapper`], the only way rules are applied.
trait ApplyOne {
    fn apply_with_format(&self, path: &str, output_format: PathFormat) -> Option<String>;
}

impl ApplyOne for PathMappingRule {
    fn apply_with_format(&self, path: &str, output_format: PathFormat) -> Option<String> {
        PathMapper::new(vec![self.clone()])
            .expect("valid rule")
            .apply_with_format(path, output_format)
    }
}

fn eval_with_rules(expr: &str, rules: Vec<PathMappingRule>, st: &SymbolTable) -> ExprValue {
    eval_with_rules_fmt(expr, rules, st, PathFormat::Posix)
}

fn eval_with_rules_fmt(
    expr: &str,
    rules: Vec<PathMappingRule>,
    st: &SymbolTable,
    fmt: PathFormat,
) -> ExprValue {
    let parsed = openjd_expr::ParsedExpression::new(expr).unwrap();
    let symtabs = [st];
    let lib = FunctionLibrary::for_profile(
        &ExprProfile::current().with_host_context(HostContext::with_rules(rules).unwrap()),
    );
    parsed
        .with_library(&lib)
        .with_path_format(fmt)
        .evaluate(&symtabs)
        .unwrap()
}

// === TestPathMappingRuleFromPosix ===
#[test]
fn posix_to_windows_basic() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/mnt/shared".to_string(),
        destination_path: "Z:\\shared".to_string(),
    };
    let mut st = SymbolTable::new();
    st.set(
        "P",
        ExprValue::String("/mnt/shared/project/file.txt".to_string()),
    )
    .unwrap();
    let r = eval_with_rules("P.apply_path_mapping()", vec![rule], &st);
    assert!(r.to_display_string().contains("shared") && r.to_display_string().contains("project"));
}

// Regression: split_path_parts drops the leading separator, so a relative
// input matched an absolute rule (and vice versa). Python's pathlib
// is_relative_to never matches across the absolute/relative boundary.
#[test]
fn relative_path_does_not_match_absolute_rule() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/home/user".to_string(),
        destination_path: "/mnt/x".to_string(),
    };
    assert_eq!(
        rule.apply_with_format("home/user/f", PathFormat::Posix),
        None
    );
}

// A rule's source path must be absolute for its format, so a relative
// source path, which could only ever match relative inputs, is rejected
// when the mapper is built.
fn rejection(rule: PathMappingRule) -> String {
    PathMapper::new(vec![rule]).unwrap_err().to_string()
}

#[test]
fn relative_posix_source_is_rejected() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "home/user".to_string(),
        destination_path: "mapped".to_string(),
    };
    assert_eq!(
        rejection(rule),
        "path mapping rule 0: source_path 'home/user' must be an absolute POSIX path, starting with '/'"
    );
}

#[test]
fn rooted_windows_source_without_drive_is_rejected() {
    // `\projects` is relative to the current drive, so it is not absolute.
    let rule = PathMappingRule {
        source_path_format: PathFormat::Windows,
        source_path: "\\projects".to_string(),
        destination_path: "D:\\work".to_string(),
    };
    assert_eq!(
        rejection(rule),
        "path mapping rule 0: source_path '\\projects' must be an absolute WINDOWS path, \
         starting with a drive such as 'C:\\' or a UNC share such as '\\\\server\\share'"
    );
}

#[test]
fn relative_path_does_not_match_windows_drive_rule() {
    // Neither a relative path nor a drive-relative one is under C:\projects
    // (matches PureWindowsPath.is_relative_to).
    let rule = PathMappingRule {
        source_path_format: PathFormat::Windows,
        source_path: "C:\\projects".to_string(),
        destination_path: "D:\\work".to_string(),
    };
    assert_eq!(
        rule.apply_with_format("projects\\file.txt", PathFormat::Windows),
        None
    );
    assert_eq!(
        rule.apply_with_format("C:projects\\file.txt", PathFormat::Windows),
        None
    );
}

#[test]
fn drive_relative_windows_source_is_rejected() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Windows,
        source_path: "C:projects".to_string(),
        destination_path: "D:\\work".to_string(),
    };
    assert_eq!(
        rejection(rule),
        "path mapping rule 0: source_path 'C:projects' must be an absolute WINDOWS path, \
         starting with a drive such as 'C:\\' or a UNC share such as '\\\\server\\share'"
    );
}

// Regression: split_path_parts split on `\` for every format, but under POSIX a
// backslash is an ordinary filename character. A POSIX rule for "/home/user"
// therefore matched the input "/home\user/f", remapping a path it has no claim
// on. Python agrees it must not match:
//   PurePosixPath("/home\\user/f").parts        == ('/', 'home\\user', 'f')
//   ....is_relative_to("/home/user")            == False
#[test]
fn posix_rule_does_not_match_backslash_as_separator() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/home/user".to_string(),
        destination_path: "/mnt/x".to_string(),
    };
    assert_eq!(
        rule.apply_with_format("/home\\user/f", PathFormat::Posix),
        None
    );
    // The whole tail as one component: "home\user\f" is a single segment.
    assert_eq!(
        rule.apply_with_format("/home\\user\\f", PathFormat::Posix),
        None
    );
}

#[test]
fn posix_rule_with_backslash_in_source_needs_literal_match() {
    // The same rule in reverse: a POSIX rule whose own source_path contains a
    // backslash treats it as part of the component name, so only a literally
    // matching input maps. Mirrors PurePosixPath("/home\\user/f")
    // .is_relative_to("/home\\user") == True.
    let rule = PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/home\\user".to_string(),
        destination_path: "/mnt/x".to_string(),
    };
    assert_eq!(
        rule.apply_with_format("/home\\user/f", PathFormat::Posix)
            .as_deref(),
        Some("/mnt/x/f")
    );
    // ...and the slash-separated spelling is now a different path entirely.
    assert_eq!(
        rule.apply_with_format("/home/user/f", PathFormat::Posix),
        None
    );
}

#[test]
fn windows_rule_still_treats_backslash_as_separator() {
    // Negative control for the separator split: Windows must keep accepting
    // both separators, interchangeably, in rule and input.
    let rule = PathMappingRule {
        source_path_format: PathFormat::Windows,
        source_path: "C:\\Users\\bob".to_string(),
        destination_path: "D:\\mapped".to_string(),
    };
    assert_eq!(
        rule.apply_with_format("C:\\Users\\bob\\f.txt", PathFormat::Windows)
            .as_deref(),
        Some("D:\\mapped\\f.txt")
    );
    assert_eq!(
        rule.apply_with_format("C:/Users/bob/f.txt", PathFormat::Windows)
            .as_deref(),
        Some("D:\\mapped\\f.txt")
    );
    // And a forward-slash rule must still match a backslash input.
    let fwd_rule = PathMappingRule {
        source_path_format: PathFormat::Windows,
        source_path: "C:/Users/bob".to_string(),
        destination_path: "D:\\mapped".to_string(),
    };
    assert_eq!(
        fwd_rule
            .apply_with_format("C:\\Users\\bob\\f.txt", PathFormat::Windows)
            .as_deref(),
        Some("D:\\mapped\\f.txt")
    );
}

#[test]
fn posix_rule_still_matches_ordinary_paths() {
    // Negative control: the common case must be untouched by the split change.
    let rule = PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/home/user".to_string(),
        destination_path: "/mnt/x".to_string(),
    };
    assert_eq!(
        rule.apply_with_format("/home/user/f", PathFormat::Posix)
            .as_deref(),
        Some("/mnt/x/f")
    );
    assert_eq!(
        rule.apply_with_format("/home/user", PathFormat::Posix)
            .as_deref(),
        Some("/mnt/x")
    );
    assert_eq!(
        rule.apply_with_format("/home/other/f", PathFormat::Posix),
        None
    );
}

// === TestPathMappingRuleValidation ===
#[test]
fn path_mapping_preserves_type() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/old".to_string(),
        destination_path: "/new".to_string(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("/old/file.txt".to_string()))
        .unwrap();
    let r = eval_with_rules("P.apply_path_mapping()", vec![rule], &st);
    assert!(matches!(r, ExprValue::Path { .. }));
}

// === TestPathMappingRuleFromPosix ===
#[test]
fn posix_exact_match() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/mnt/shared".into(),
        destination_path: "/new/shared".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("/mnt/shared".into()))
        .unwrap();
    let r = eval_with_rules("P.apply_path_mapping()", vec![rule], &st);
    assert_eq!(r.to_display_string(), "/new/shared");
}
#[test]
fn posix_trailing_slash_preserved() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/mnt/shared".into(),
        destination_path: "/new/shared".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("/mnt/shared/".into()))
        .unwrap();
    let r = eval_with_rules("P.apply_path_mapping()", vec![rule], &st);
    assert!(r.to_display_string().ends_with('/'));
}
#[test]
fn posix_no_match_different_path() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/mnt/shared".into(),
        destination_path: "/new/shared".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("/other/path".into()))
        .unwrap();
    let r = eval_with_rules("P.apply_path_mapping()", vec![rule], &st);
    assert_eq!(r.to_display_string(), "/other/path");
}

#[test]
fn unmapped_posix_path_normalized_to_windows_format() {
    // When no rule matches and format is Windows, separators should be normalized
    let rule = PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/mnt/shared".into(),
        destination_path: "/new/shared".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("/other/path/file.txt".into()))
        .unwrap();
    let r = eval_with_rules_fmt(
        "P.apply_path_mapping()",
        vec![rule],
        &st,
        PathFormat::Windows,
    );
    assert_eq!(r.to_display_string(), "\\other\\path\\file.txt");
}

#[test]
fn posix_no_match_same_prefix() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/mnt/shared".into(),
        destination_path: "/new/shared".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("/mnt/sharedextra/file".into()))
        .unwrap();
    let r = eval_with_rules("P.apply_path_mapping()", vec![rule], &st);
    assert_eq!(r.to_display_string(), "/mnt/sharedextra/file");
}
#[test]
fn posix_with_subpath() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/mnt/shared".into(),
        destination_path: "/new/shared".into(),
    };
    let mut st = SymbolTable::new();
    st.set(
        "P",
        ExprValue::String("/mnt/shared/project/file.txt".into()),
    )
    .unwrap();
    let r = eval_with_rules("P.apply_path_mapping()", vec![rule], &st);
    assert_eq!(r.to_display_string(), "/new/shared/project/file.txt");
}

// === TestPathMappingRuleFromWindows ===
#[test]
fn windows_with_subpath() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Windows,
        source_path: "Z:\\shared".into(),
        destination_path: "/mnt/shared".into(),
    };
    let mut st = SymbolTable::new();
    st.set(
        "P",
        ExprValue::String("Z:\\shared\\project\\file.txt".into()),
    )
    .unwrap();
    let r = eval_with_rules_fmt(
        "P.apply_path_mapping()",
        vec![rule],
        &st,
        PathFormat::Windows,
    );
    assert!(r.to_display_string().contains("shared") && r.to_display_string().contains("project"));
}
#[test]
fn windows_exact_match() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Windows,
        source_path: "Z:\\shared".into(),
        destination_path: "/mnt/shared".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("Z:\\shared".into())).unwrap();
    let r = eval_with_rules_fmt(
        "P.apply_path_mapping()",
        vec![rule],
        &st,
        PathFormat::Windows,
    );
    assert_eq!(r.to_display_string(), "\\mnt\\shared");
}
#[test]
fn windows_no_match() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Windows,
        source_path: "Z:\\shared".into(),
        destination_path: "/mnt/shared".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("C:\\other".into())).unwrap();
    let r = eval_with_rules_fmt(
        "P.apply_path_mapping()",
        vec![rule],
        &st,
        PathFormat::Windows,
    );
    assert_eq!(r.to_display_string(), "C:\\other");
}

// === TestPathMappingRuleFromUri ===
#[test]
fn uri_with_subpath() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Uri,
        source_path: "s3://bucket/prefix".into(),
        destination_path: "/local/data".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("s3://bucket/prefix/file.txt".into()))
        .unwrap();
    let r = eval_with_rules("P.apply_path_mapping()", vec![rule], &st);
    assert_eq!(r.to_display_string(), "/local/data/file.txt");
}
#[test]
fn uri_nested_subpath() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Uri,
        source_path: "s3://bucket/prefix".into(),
        destination_path: "/local/data".into(),
    };
    let mut st = SymbolTable::new();
    st.set(
        "P",
        ExprValue::String("s3://bucket/prefix/a/b/c.txt".into()),
    )
    .unwrap();
    let r = eval_with_rules("P.apply_path_mapping()", vec![rule], &st);
    assert_eq!(r.to_display_string(), "/local/data/a/b/c.txt");
}
#[test]
fn uri_exact_match() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Uri,
        source_path: "s3://bucket/prefix".into(),
        destination_path: "/local/data".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("s3://bucket/prefix".into()))
        .unwrap();
    let r = eval_with_rules("P.apply_path_mapping()", vec![rule], &st);
    assert_eq!(r.to_display_string(), "/local/data");
}
#[test]
fn uri_no_match_different_bucket() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Uri,
        source_path: "s3://bucket/prefix".into(),
        destination_path: "/local/data".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("s3://other/prefix/file.txt".into()))
        .unwrap();
    let r = eval_with_rules("P.apply_path_mapping()", vec![rule], &st);
    assert_eq!(r.to_display_string(), "s3://other/prefix/file.txt");
}
#[test]
fn uri_no_match_prefix_overlap() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Uri,
        source_path: "s3://bucket/prefix".into(),
        destination_path: "/local/data".into(),
    };
    let mut st = SymbolTable::new();
    st.set(
        "P",
        ExprValue::String("s3://bucket/prefixextra/file.txt".into()),
    )
    .unwrap();
    let r = eval_with_rules("P.apply_path_mapping()", vec![rule], &st);
    assert_eq!(r.to_display_string(), "s3://bucket/prefixextra/file.txt");
}
#[test]
fn uri_no_match_different_scheme() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Uri,
        source_path: "s3://bucket/prefix".into(),
        destination_path: "/local/data".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("gs://bucket/prefix/file.txt".into()))
        .unwrap();
    let r = eval_with_rules("P.apply_path_mapping()", vec![rule], &st);
    assert_eq!(r.to_display_string(), "gs://bucket/prefix/file.txt");
}
#[test]
fn uri_no_match_filesystem() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Uri,
        source_path: "s3://bucket/prefix".into(),
        destination_path: "/local/data".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("/local/file.txt".into()))
        .unwrap();
    let r = eval_with_rules("P.apply_path_mapping()", vec![rule], &st);
    assert_eq!(r.to_display_string(), "/local/file.txt");
}
#[test]
fn uri_https_scheme() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Uri,
        source_path: "https://host/path".into(),
        destination_path: "/local/data".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("https://host/path/file.txt".into()))
        .unwrap();
    let r = eval_with_rules("P.apply_path_mapping()", vec![rule], &st);
    assert_eq!(r.to_display_string(), "/local/data/file.txt");
}
#[test]
fn uri_custom_scheme() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Uri,
        source_path: "fsx://vol/path".into(),
        destination_path: "/local/data".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("fsx://vol/path/file.txt".into()))
        .unwrap();
    let r = eval_with_rules("P.apply_path_mapping()", vec![rule], &st);
    assert_eq!(r.to_display_string(), "/local/data/file.txt");
}
#[test]
fn uri_trailing_slash() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Uri,
        source_path: "s3://bucket/prefix".into(),
        destination_path: "/local/data".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("s3://bucket/prefix/".into()))
        .unwrap();
    let r = eval_with_rules("P.apply_path_mapping()", vec![rule], &st);
    assert!(r.to_display_string().ends_with('/'));
}

// === Trailing slash preservation (matches Python behavior exactly) ===

#[test]
fn posix_trailing_slash_exact_output() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/src".into(),
        destination_path: "/dst".into(),
    };
    // No trailing slash → no trailing slash
    assert_eq!(
        rule.apply_with_format("/src", PathFormat::Posix),
        Some("/dst".into())
    );
    assert_eq!(
        rule.apply_with_format("/src/file.txt", PathFormat::Posix),
        Some("/dst/file.txt".into())
    );
    // Trailing slash → trailing slash preserved
    assert_eq!(
        rule.apply_with_format("/src/", PathFormat::Posix),
        Some("/dst/".into())
    );
    assert_eq!(
        rule.apply_with_format("/src/subdir/", PathFormat::Posix),
        Some("/dst/subdir/".into())
    );
}

#[test]
fn uri_trailing_slash_exact_output() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Uri,
        source_path: "s3://bucket/prefix".into(),
        destination_path: "/local".into(),
    };
    // No trailing slash → no trailing slash
    assert_eq!(
        rule.apply_with_format("s3://bucket/prefix", PathFormat::Posix),
        Some("/local".into())
    );
    assert_eq!(
        rule.apply_with_format("s3://bucket/prefix/file.txt", PathFormat::Posix),
        Some("/local/file.txt".into())
    );
    // Trailing slash → trailing slash preserved
    assert_eq!(
        rule.apply_with_format("s3://bucket/prefix/", PathFormat::Posix),
        Some("/local/".into())
    );
}

// === No rules returns unchanged ===
#[test]
fn no_rules_unchanged() {
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("/some/path".into())).unwrap();
    let r = eval_with_rules("P.apply_path_mapping()", vec![], &st);
    assert_eq!(r.to_display_string(), "/some/path");
}

// ══════════════════════════════════════════════════════════════
// PathMappingRule::apply() unit tests (no evaluator needed)
// ══════════════════════════════════════════════════════════════

mod apply_unit {
    use super::ApplyOne;
    use openjd_expr::path_mapping::PathFormat;
    use openjd_expr::{PathMapper, PathMappingRule};
    fn posix_rule(src: &str, dst: &str) -> PathMappingRule {
        PathMappingRule {
            source_path_format: PathFormat::Posix,
            source_path: src.into(),
            destination_path: dst.into(),
        }
    }
    fn windows_rule(src: &str, dst: &str) -> PathMappingRule {
        PathMappingRule {
            source_path_format: PathFormat::Windows,
            source_path: src.into(),
            destination_path: dst.into(),
        }
    }
    fn uri_rule(src: &str, dst: &str) -> PathMappingRule {
        PathMappingRule {
            source_path_format: PathFormat::Uri,
            source_path: src.into(),
            destination_path: dst.into(),
        }
    }

    // ── POSIX ──

    #[test]
    fn posix_subpath() {
        assert_eq!(
            posix_rule("/src", "/dst").apply_with_format("/src/a/b", PathFormat::Posix),
            Some("/dst/a/b".into())
        );
    }
    #[test]
    fn posix_exact() {
        assert_eq!(
            posix_rule("/src", "/dst").apply_with_format("/src", PathFormat::Posix),
            Some("/dst".into())
        );
    }
    #[test]
    fn posix_trailing() {
        assert_eq!(
            posix_rule("/src", "/dst").apply_with_format("/src/", PathFormat::Posix),
            Some("/dst/".into())
        );
    }
    #[test]
    fn posix_subdir_trailing() {
        assert_eq!(
            posix_rule("/src", "/dst").apply_with_format("/src/a/b/", PathFormat::Posix),
            Some("/dst/a/b/".into())
        );
    }
    #[test]
    fn posix_no_match() {
        assert_eq!(
            posix_rule("/src", "/dst").apply_with_format("/other", PathFormat::Posix),
            None
        );
    }
    #[test]
    fn posix_prefix_overlap() {
        assert_eq!(
            posix_rule("/src", "/dst").apply_with_format("/srcextra/file", PathFormat::Posix),
            None
        );
    }
    #[test]
    fn posix_case_sensitive() {
        assert_eq!(
            posix_rule("/Src", "/dst").apply_with_format("/src/file", PathFormat::Posix),
            None
        );
    }

    // ── Windows ──

    #[test]
    fn windows_subpath() {
        assert_eq!(
            windows_rule("C:\\old", "/new")
                .apply_with_format("C:\\old\\sub\\file.txt", PathFormat::Posix),
            Some("/new/sub/file.txt".into())
        );
    }
    #[test]
    fn windows_exact() {
        assert_eq!(
            windows_rule("C:\\old", "/new").apply_with_format("C:\\old", PathFormat::Posix),
            Some("/new".into())
        );
    }
    #[test]
    fn windows_trailing_backslash() {
        assert_eq!(
            windows_rule("C:\\old", "/new").apply_with_format("C:\\old\\", PathFormat::Posix),
            Some("/new/".into())
        );
    }
    #[test]
    fn windows_trailing_fwdslash() {
        assert_eq!(
            windows_rule("C:\\old", "/new").apply_with_format("C:\\old/", PathFormat::Posix),
            Some("/new/".into())
        );
    }
    #[test]
    fn windows_no_match() {
        assert_eq!(
            windows_rule("C:\\old", "/new").apply_with_format("D:\\old\\file", PathFormat::Posix),
            None
        );
    }
    #[test]
    fn windows_prefix_overlap() {
        assert_eq!(
            windows_rule("Z:\\shared", "/new")
                .apply_with_format("Z:\\sharedextra\\file", PathFormat::Posix),
            None
        );
    }

    // Windows case insensitivity (matches Python PureWindowsPath.is_relative_to)
    #[test]
    fn windows_case_insensitive_lower() {
        assert_eq!(
            windows_rule("C:\\Users", "/home")
                .apply_with_format("c:\\users\\bob", PathFormat::Posix),
            Some("/home/bob".into())
        );
    }
    #[test]
    fn windows_case_insensitive_upper() {
        assert_eq!(
            windows_rule("C:\\Users", "/home")
                .apply_with_format("C:\\USERS\\bob", PathFormat::Posix),
            Some("/home/bob".into())
        );
    }

    // Windows forward slashes in input path (PureWindowsPath accepts both)
    #[test]
    fn windows_fwd_slash_input() {
        assert_eq!(
            windows_rule("C:\\data", "/data")
                .apply_with_format("C:/data/file.txt", PathFormat::Posix),
            Some("/data/file.txt".into())
        );
    }
    #[test]
    fn windows_mixed_slash_input() {
        assert_eq!(
            windows_rule("C:\\data", "/data")
                .apply_with_format("C:\\data/sub/file.txt", PathFormat::Posix),
            Some("/data/sub/file.txt".into())
        );
    }

    // Windows to posix destination
    #[test]
    fn windows_to_posix_deep() {
        assert_eq!(
            windows_rule("Z:\\shared", "/mnt/shared")
                .apply_with_format("Z:\\shared\\project\\file.txt", PathFormat::Posix),
            Some("/mnt/shared/project/file.txt".into())
        );
    }

    // ── URI ──

    #[test]
    fn uri_subpath() {
        assert_eq!(
            uri_rule("s3://bucket/prefix", "/local")
                .apply_with_format("s3://bucket/prefix/file.txt", PathFormat::Posix),
            Some("/local/file.txt".into())
        );
    }
    #[test]
    fn uri_exact() {
        assert_eq!(
            uri_rule("s3://bucket/prefix", "/local")
                .apply_with_format("s3://bucket/prefix", PathFormat::Posix),
            Some("/local".into())
        );
    }
    #[test]
    fn uri_trailing() {
        assert_eq!(
            uri_rule("s3://bucket/prefix", "/local")
                .apply_with_format("s3://bucket/prefix/", PathFormat::Posix),
            Some("/local/".into())
        );
    }
    #[test]
    fn uri_nested() {
        assert_eq!(
            uri_rule("s3://bucket/prefix", "/local")
                .apply_with_format("s3://bucket/prefix/a/b/c", PathFormat::Posix),
            Some("/local/a/b/c".into())
        );
    }
    #[test]
    fn uri_no_match_bucket() {
        assert_eq!(
            uri_rule("s3://bucket/prefix", "/local")
                .apply_with_format("s3://other/prefix/file", PathFormat::Posix),
            None
        );
    }
    #[test]
    fn uri_no_match_prefix() {
        assert_eq!(
            uri_rule("s3://bucket/prefix", "/local")
                .apply_with_format("s3://bucket/prefixextra", PathFormat::Posix),
            None
        );
    }
    #[test]
    fn uri_no_match_scheme() {
        assert_eq!(
            uri_rule("s3://bucket/prefix", "/local")
                .apply_with_format("gs://bucket/prefix/file", PathFormat::Posix),
            None
        );
    }
    #[test]
    fn uri_no_match_filesystem() {
        assert_eq!(
            uri_rule("s3://bucket", "/local").apply_with_format("/local/file", PathFormat::Posix),
            None
        );
    }

    // ── apply_with_format: Posix output ──

    #[test]
    fn posix_output_subpath() {
        assert_eq!(
            posix_rule("/src", "/dst").apply_with_format("/src/a/b", PathFormat::Posix),
            Some("/dst/a/b".into())
        );
    }
    #[test]
    fn posix_output_trailing() {
        assert_eq!(
            posix_rule("/src", "/dst").apply_with_format("/src/", PathFormat::Posix),
            Some("/dst/".into())
        );
    }
    #[test]
    fn windows_source_posix_output() {
        assert_eq!(
            windows_rule("C:\\old", "/new")
                .apply_with_format("C:\\old\\sub\\file.txt", PathFormat::Posix),
            Some("/new/sub/file.txt".into())
        );
    }
    #[test]
    fn uri_source_posix_output() {
        assert_eq!(
            uri_rule("s3://b/p", "/local").apply_with_format("s3://b/p/file", PathFormat::Posix),
            Some("/local/file".into())
        );
    }

    // ── apply_with_format: Windows output ──

    #[test]
    fn posix_source_windows_output() {
        assert_eq!(
            posix_rule("/mnt/data", "D:\\data")
                .apply_with_format("/mnt/data/sub/file.txt", PathFormat::Windows),
            Some("D:\\data\\sub\\file.txt".into())
        );
    }
    #[test]
    fn posix_source_windows_output_exact() {
        assert_eq!(
            posix_rule("/mnt/data", "D:\\data").apply_with_format("/mnt/data", PathFormat::Windows),
            Some("D:\\data".into())
        );
    }
    #[test]
    fn posix_source_windows_output_trailing() {
        assert_eq!(
            posix_rule("/mnt/data", "D:\\data")
                .apply_with_format("/mnt/data/", PathFormat::Windows),
            Some("D:\\data\\".into())
        );
    }
    #[test]
    fn windows_source_windows_output() {
        assert_eq!(
            windows_rule("C:\\old", "D:\\new")
                .apply_with_format("C:\\old\\sub\\file.txt", PathFormat::Windows),
            Some("D:\\new\\sub\\file.txt".into())
        );
    }
    #[test]
    fn windows_source_windows_output_trailing() {
        assert_eq!(
            windows_rule("C:\\old", "D:\\new").apply_with_format("C:\\old\\", PathFormat::Windows),
            Some("D:\\new\\".into())
        );
    }
    #[test]
    fn uri_source_windows_output() {
        assert_eq!(
            uri_rule("s3://b/p", "D:\\local")
                .apply_with_format("s3://b/p/a/b", PathFormat::Windows),
            Some("D:\\local\\a\\b".into())
        );
    }
    #[test]
    fn uri_source_windows_output_trailing() {
        assert_eq!(
            uri_rule("s3://b/p", "D:\\local").apply_with_format("s3://b/p/", PathFormat::Windows),
            Some("D:\\local\\".into())
        );
    }

    // ── apply_with_format: Windows case insensitivity still works ──

    #[test]
    fn windows_case_insensitive_windows_output() {
        assert_eq!(
            windows_rule("C:\\Users", "D:\\home")
                .apply_with_format("c:\\users\\bob", PathFormat::Windows),
            Some("D:\\home\\bob".into())
        );
    }

    // ── PathMapper: choosing between rules ──

    fn mapper(rules: Vec<PathMappingRule>) -> PathMapper {
        PathMapper::new(rules).expect("valid rules")
    }

    #[test]
    fn same_location_last_rule_wins() {
        // Rules naming the same location: the one given last wins.
        let m = mapper(vec![
            posix_rule("/a", "/first"),
            posix_rule("/a", "/second"),
        ]);
        assert_eq!(
            m.map_with_format("/a/file", PathFormat::Posix),
            "/second/file"
        );
    }

    #[test]
    fn same_location_spelled_differently_last_rule_wins() {
        // `/mnt/p/` and `/mnt//p` are the same location as `/mnt/p`, so
        // these rules tie and the one given last wins, whatever the
        // string lengths.
        let m = mapper(vec![
            posix_rule("/mnt/p/", "/long"),
            posix_rule("/mnt//p", "/double"),
            posix_rule("/mnt/p", "/short"),
        ]);
        assert_eq!(m.map_with_format("/mnt/p/f", PathFormat::Posix), "/short/f");
        let m = mapper(vec![
            posix_rule("/mnt/p", "/short"),
            posix_rule("/mnt/p/", "/long"),
        ]);
        assert_eq!(m.map_with_format("/mnt/p/f", PathFormat::Posix), "/long/f");
        let m = mapper(vec![
            windows_rule("C:\\Data", "/upper"),
            windows_rule("c:/data/", "/lower"),
        ]);
        assert_eq!(
            m.map_with_format("C:\\DATA\\f", PathFormat::Posix),
            "/lower/f"
        );
    }

    #[test]
    fn deepest_rule_wins_in_any_order() {
        for rules in [
            vec![posix_rule("/a", "/shallow"), posix_rule("/a/b", "/deep")],
            vec![posix_rule("/a/b", "/deep"), posix_rule("/a", "/shallow")],
        ] {
            let m = mapper(rules);
            assert_eq!(m.map_with_format("/a/b/f", PathFormat::Posix), "/deep/f");
            assert_eq!(
                m.map_with_format("/a/c/f", PathFormat::Posix),
                "/shallow/c/f"
            );
        }
    }

    #[test]
    fn deepest_rule_wins_over_longer_string() {
        // `/aaaaaaaa///` is the longer string but has fewer components.
        let m = mapper(vec![
            posix_rule("/aaaaaaaa///", "/shallow"),
            posix_rule("/aaaaaaaa/b", "/deep"),
        ]);
        assert_eq!(
            m.map_with_format("/aaaaaaaa/b/f", PathFormat::Posix),
            "/deep/f"
        );
    }

    #[test]
    fn cross_format_deepest_then_last_wins() {
        // `//srv/share/x` is a POSIX path and a Windows UNC path.
        let posix = posix_rule("/srv", "/posix");
        let unc_deep = windows_rule("\\\\srv\\share\\x", "/unc");
        let unc_shallow = windows_rule("\\\\srv\\share", "/unc");
        // POSIX /srv has two components, \\srv\share has one: POSIX wins.
        let m = mapper(vec![posix.clone(), unc_shallow]);
        assert_eq!(
            m.map_with_format("//srv/share/x/f", PathFormat::Posix),
            "/posix/share/x/f"
        );
        // Two components each: the rule given last wins.
        let m = mapper(vec![posix.clone(), unc_deep.clone()]);
        assert_eq!(
            m.map_with_format("//srv/share/x/f", PathFormat::Posix),
            "/unc/f"
        );
        let m = mapper(vec![unc_deep, posix]);
        assert_eq!(
            m.map_with_format("//srv/share/x/f", PathFormat::Posix),
            "/posix/share/x/f"
        );
    }

    #[test]
    fn rules_by_precedence_order() {
        let m = mapper(vec![
            posix_rule("/a", "/1"),
            posix_rule("/a/b/c", "/2"),
            uri_rule("s3://b/x", "/3"),
            posix_rule("/a", "/4"),
            windows_rule("C:\\a\\b", "/5"),
        ]);
        let order: Vec<&str> = m
            .rules_by_precedence()
            .map(|r| r.destination_path.as_str())
            .collect();
        // Depth 4, then depth 3 (later first), then depth 2 (later first).
        assert_eq!(order, ["/2", "/5", "/4", "/3", "/1"]);
        let given: Vec<&str> = m
            .rules()
            .iter()
            .map(|r| r.destination_path.as_str())
            .collect();
        assert_eq!(given, ["/1", "/2", "/3", "/4", "/5"]);
    }

    #[test]
    fn no_match_returns_original() {
        let m = mapper(vec![posix_rule("/a", "/b")]);
        assert_eq!(
            m.map_with_format("/other/file", PathFormat::Posix),
            "/other/file"
        );
        assert_eq!(m.apply_with_format("/other/file", PathFormat::Posix), None);
    }

    #[test]
    fn empty_mapper_returns_original() {
        let m = mapper(vec![]);
        assert!(m.is_empty());
        assert_eq!(m.map("/any/path"), "/any/path");
        assert_eq!(PathMapper::default().map("/any/path"), "/any/path");
    }

    #[test]
    fn map_uses_host_separator() {
        let m = mapper(vec![posix_rule("/a", "/b")]);
        let expected = if cfg!(windows) { "/b\\c\\d" } else { "/b/c/d" };
        assert_eq!(m.map("/a/c/d"), expected);
        assert_eq!(m.apply("/a/c/d").as_deref(), Some(expected));
    }

    // ── Components: `.` and empty components ──

    #[test]
    fn dot_and_empty_components_are_ignored() {
        // As in pathlib, `.` and repeated separators do not count.
        let m = mapper(vec![posix_rule("/a/./b", "/x")]);
        assert_eq!(m.map_with_format("/a//b/./c", PathFormat::Posix), "/x/c");
        let m = mapper(vec![windows_rule("C:\\a\\.\\b", "/x")]);
        assert_eq!(
            m.map_with_format("C:\\a\\\\b\\.\\c", PathFormat::Posix),
            "/x/c"
        );
    }

    #[test]
    fn dot_dot_is_a_component() {
        // `..` is not collapsed: symbolic links make that unsafe.
        let m = mapper(vec![posix_rule("/a", "/x")]);
        assert_eq!(m.map_with_format("/a/../b", PathFormat::Posix), "/x/../b");
        assert_eq!(m.apply_with_format("/b/../a/f", PathFormat::Posix), None);
    }

    #[test]
    fn uri_empty_segments_are_kept() {
        // URI paths are opaque: an empty segment is a segment.
        let m = mapper(vec![uri_rule("s3://b/a", "/x")]);
        assert_eq!(m.map_with_format("s3://b/a//f", PathFormat::Posix), "/x//f");
        let m = mapper(vec![uri_rule("s3://b/a/", "/x")]);
        assert_eq!(m.apply_with_format("s3://b/a/f", PathFormat::Posix), None);
        assert_eq!(m.map_with_format("s3://b/a/", PathFormat::Posix), "/x/");
    }

    // ── Windows anchors: drives and UNC shares ──

    #[test]
    fn unc_share_is_one_component() {
        let m = mapper(vec![windows_rule("\\\\srv\\share", "/mnt/share")]);
        assert_eq!(
            m.map_with_format("\\\\srv\\share\\x", PathFormat::Posix),
            "/mnt/share/x"
        );
        assert_eq!(
            m.map_with_format("//SRV/Share/x", PathFormat::Posix),
            "/mnt/share/x"
        );
        assert_eq!(
            m.map_with_format("\\\\srv\\share", PathFormat::Posix),
            "/mnt/share"
        );
        // A different share on the same server is a different anchor.
        assert_eq!(
            m.apply_with_format("\\\\srv\\other\\x", PathFormat::Posix),
            None
        );
    }

    #[test]
    fn single_backslash_path_does_not_match_unc_share() {
        // `\srv\share\x` is a path on the current drive, not on \\srv\share
        // (PureWindowsPath agrees: it is not relative to \\srv\share).
        let m = mapper(vec![windows_rule("\\\\srv\\share", "/mnt/share")]);
        assert_eq!(
            m.apply_with_format("\\srv\\share\\x", PathFormat::Posix),
            None
        );
        assert_eq!(
            m.apply_with_format("\\\\\\srv\\share\\x", PathFormat::Posix),
            None
        );
    }

    #[test]
    fn unc_server_alone_is_not_a_share() {
        let m = mapper(vec![windows_rule("\\\\srv\\share", "/mnt/share")]);
        assert_eq!(m.apply_with_format("\\\\srv", PathFormat::Posix), None);
        assert_eq!(m.apply_with_format("\\\\srv\\", PathFormat::Posix), None);
        for source in [
            "\\\\srv",
            "\\\\srv\\",
            "\\\\\\srv\\share",
            "\\\\srv\\\\share",
        ] {
            assert!(
                PathMapper::new(vec![windows_rule(source, "/x")]).is_err(),
                "{source:?}"
            );
        }
    }

    #[test]
    fn drive_letter_compares_case_insensitively() {
        let m = mapper(vec![windows_rule("c:\\data", "/d")]);
        assert_eq!(m.map_with_format("C:/DATA/f", PathFormat::Posix), "/d/f");
        assert_eq!(m.apply_with_format("D:\\data\\f", PathFormat::Posix), None);
    }

    #[test]
    fn drive_root_rule() {
        let m = mapper(vec![windows_rule("C:\\", "/c")]);
        assert_eq!(m.map_with_format("c:\\a\\b", PathFormat::Posix), "/c/a/b");
        assert_eq!(m.apply_with_format("C:", PathFormat::Posix), None);
    }

    // ── Windows case: the Windows uppercase table, not Unicode case folding ──

    #[test]
    fn windows_case_folds_non_ascii_like_windows() {
        for (rule_part, input_part) in [
            ("\u{c9}t\u{e9}", "\u{e9}T\u{c9}"),   // été / ÉTÉ
            ("\u{3a3}\u{3b1}", "\u{3c3}\u{391}"), // Σα / σΑ
            ("\u{416}", "\u{436}"),               // Cyrillic Zhe
            ("\u{ff21}", "\u{ff41}"),             // fullwidth A / a
        ] {
            let m = mapper(vec![windows_rule(&format!("C:\\{rule_part}"), "/x")]);
            assert_eq!(
                m.map_with_format(&format!("C:\\{input_part}\\f"), PathFormat::Posix),
                "/x/f",
                "{rule_part} vs {input_part}"
            );
        }
    }

    #[test]
    fn windows_case_does_not_fold_what_windows_keeps_apart() {
        for (rule_part, input_part) in [
            ("k", "\u{212a}"),          // KELVIN SIGN is not k
            ("K", "\u{212a}"),          // ...nor K
            ("I", "\u{131}"),           // dotless i
            ("i", "\u{130}"),           // I with dot above
            ("S", "\u{17f}"),           // long s
            ("\u{3a3}", "\u{3c2}"),     // final sigma
            ("\u{df}", "\u{1e9e}"),     // sharp s / capital sharp s
            ("SS", "\u{df}"),           // no multi-character folding
            ("\u{13a0}", "\u{ab70}"),   // Cherokee: newer than the Windows table
            ("\u{10400}", "\u{10428}"), // Deseret, outside the BMP
        ] {
            let m = mapper(vec![windows_rule(&format!("C:\\{rule_part}"), "/x")]);
            assert_eq!(
                m.apply_with_format(&format!("C:\\{input_part}\\f"), PathFormat::Posix),
                None,
                "{rule_part:?} vs {input_part:?}"
            );
        }
    }

    #[test]
    fn windows_unc_server_and_share_fold_like_windows() {
        let m = mapper(vec![windows_rule("\\\\\u{c9}t\u{e9}\\\u{3a3}", "/x")]);
        assert_eq!(
            m.map_with_format("\\\\\u{e9}T\u{c9}\\\u{3c3}\\f", PathFormat::Posix),
            "/x/f"
        );
        assert_eq!(
            m.apply_with_format("\\\\\u{e9}T\u{c9}\\\u{3c2}\\f", PathFormat::Posix),
            None
        );
    }

    #[test]
    fn windows_output_keeps_input_case() {
        let m = mapper(vec![windows_rule("C:\\Data", "D:\\Data")]);
        assert_eq!(
            m.map_with_format("c:\\data\\\u{e9}T\u{e9}\\File.TXT", PathFormat::Windows),
            "D:\\Data\\\u{e9}T\u{e9}\\File.TXT"
        );
    }

    // ── Validation ──

    #[test]
    fn invalid_rule_reports_its_index() {
        let err = PathMapper::new(vec![
            posix_rule("/ok", "/a"),
            uri_rule("bucket/prefix", "/b"),
        ])
        .unwrap_err();
        assert_eq!(err.index, Some(1));
        assert_eq!(
            err.to_string(),
            "path mapping rule 1: source_path 'bucket/prefix' must be a URI, starting with '<scheme>://'"
        );
    }

    #[test]
    fn validate_reports_no_index() {
        let err = posix_rule("relative", "/a").validate().unwrap_err();
        assert_eq!(err.index, None);
        assert_eq!(
            err.to_string(),
            "source_path 'relative' must be an absolute POSIX path, starting with '/'"
        );
        assert_eq!(posix_rule("/abs", "/a").validate(), Ok(()));
    }

    #[test]
    fn single_letter_uri_scheme_is_rejected() {
        // `C://data` is a Windows drive path, not a URI.
        assert!(uri_rule("C://data", "/x").validate().is_err());
        assert!(uri_rule("s3://data", "/x").validate().is_ok());
        assert!(uri_rule("", "/x").validate().is_err());
        assert!(posix_rule("", "/x").validate().is_err());
        assert!(windows_rule("", "/x").validate().is_err());
    }

    #[test]
    fn many_rules_map_correctly() {
        // The trie keeps lookup independent of the rule count.
        let rules: Vec<PathMappingRule> = (0..5000)
            .map(|i| posix_rule(&format!("/r{i}"), &format!("/d{i}")))
            .collect();
        let m = mapper(rules);
        assert_eq!(m.map_with_format("/r4999/f", PathFormat::Posix), "/d4999/f");
        assert_eq!(m.map_with_format("/r0/f", PathFormat::Posix), "/d0/f");
        assert_eq!(m.apply_with_format("/r5000/f", PathFormat::Posix), None);
    }
}

// ══════════════════════════════════════════════════════════════
// Serde tests (from_dict / to_dict equivalents from Python)
// ══════════════════════════════════════════════════════════════

mod serde_tests {
    use openjd_expr::{PathFormat, PathMappingRule};

    // ── POSIX from_dict / to_dict ──

    #[test]
    fn posix_from_dict() {
        let json = r#"{"source_path_format":"POSIX","source_path":"/mnt/shared","destination_path":"/new/prefix"}"#;
        let rule: PathMappingRule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.source_path_format, PathFormat::Posix);
        assert_eq!(rule.source_path, "/mnt/shared");
        assert_eq!(rule.destination_path, "/new/prefix");
    }

    #[test]
    fn posix_to_dict() {
        let rule = PathMappingRule {
            source_path_format: PathFormat::Posix,
            source_path: "/mnt/shared".into(),
            destination_path: "/new/prefix".into(),
        };
        let val = serde_json::to_value(&rule).unwrap();
        assert_eq!(val["source_path_format"], "POSIX");
        assert_eq!(val["source_path"], "/mnt/shared");
        assert_eq!(val["destination_path"], "/new/prefix");
    }

    // ── Windows from_dict / to_dict ──

    #[test]
    fn windows_from_dict() {
        let json = r#"{"source_path_format":"WINDOWS","source_path":"C:\\projects","destination_path":"/mnt/projects"}"#;
        let rule: PathMappingRule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.source_path_format, PathFormat::Windows);
        assert_eq!(rule.source_path, "C:\\projects");
        assert_eq!(rule.destination_path, "/mnt/projects");
    }

    #[test]
    fn windows_to_dict() {
        let rule = PathMappingRule {
            source_path_format: PathFormat::Windows,
            source_path: "C:\\projects".into(),
            destination_path: "/mnt/projects".into(),
        };
        let val = serde_json::to_value(&rule).unwrap();
        assert_eq!(val["source_path_format"], "WINDOWS");
        assert_eq!(val["source_path"], "C:\\projects");
        assert_eq!(val["destination_path"], "/mnt/projects");
    }

    // ── URI from_dict / to_dict / roundtrip ──

    #[test]
    fn uri_from_dict() {
        let json = r#"{"source_path_format":"URI","source_path":"s3://bucket/assets","destination_path":"/local"}"#;
        let rule: PathMappingRule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.source_path_format, PathFormat::Uri);
        assert_eq!(rule.source_path, "s3://bucket/assets");
        assert_eq!(rule.destination_path, "/local");
    }

    #[test]
    fn uri_to_dict() {
        let rule = PathMappingRule {
            source_path_format: PathFormat::Uri,
            source_path: "s3://bucket/assets".into(),
            destination_path: "/local".into(),
        };
        let val = serde_json::to_value(&rule).unwrap();
        assert_eq!(val["source_path_format"], "URI");
        assert_eq!(val["source_path"], "s3://bucket/assets");
        assert_eq!(val["destination_path"], "/local");
    }

    #[test]
    fn uri_roundtrip_dict() {
        let original = PathMappingRule {
            source_path_format: PathFormat::Uri,
            source_path: "s3://bucket/assets".into(),
            destination_path: "/local".into(),
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: PathMappingRule = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.source_path_format, original.source_path_format);
        assert_eq!(restored.source_path, original.source_path);
        assert_eq!(restored.destination_path, original.destination_path);
    }

    // ── Case-insensitive format parsing (Python from_dict accepts "posix", "uri", etc.) ──

    #[test]
    fn from_dict_case_insensitive_posix() {
        let json = r#"{"source_path_format":"posix","source_path":"/mnt/shared","destination_path":"/new"}"#;
        let rule: PathMappingRule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.source_path_format, PathFormat::Posix);
    }

    #[test]
    fn from_dict_case_insensitive_uri() {
        let json = r#"{"source_path_format":"uri","source_path":"s3://bucket","destination_path":"/local"}"#;
        let rule: PathMappingRule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.source_path_format, PathFormat::Uri);
    }

    #[test]
    fn from_dict_case_insensitive_windows() {
        let json = r#"{"source_path_format":"windows","source_path":"C:\\path","destination_path":"/new"}"#;
        let rule: PathMappingRule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.source_path_format, PathFormat::Windows);
    }

    // ── Validation: empty, missing field, extra field ──

    #[test]
    fn from_dict_empty_fails() {
        let result = serde_json::from_str::<PathMappingRule>("{}");
        assert!(result.is_err());
    }

    #[test]
    fn from_dict_missing_field_fails() {
        let json = r#"{"source_path_format":"POSIX","source_path":"/mnt/shared"}"#;
        let result = serde_json::from_str::<PathMappingRule>(json);
        assert!(result.is_err());
    }

    #[test]
    fn from_dict_extra_field_fails() {
        let json = r#"{"source_path_format":"POSIX","source_path":"/mnt/shared","destination_path":"/new","extra":"field"}"#;
        let result = serde_json::from_str::<PathMappingRule>(json);
        assert!(result.is_err());
    }
}

// ══════════════════════════════════════════════════════════════
// Additional evaluator-level tests for Windows (matching Python)
// ══════════════════════════════════════════════════════════════

#[test]
fn windows_no_match_same_prefix_eval() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Windows,
        source_path: "C:\\projects".into(),
        destination_path: "/mnt/projects".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("C:\\projects2\\file.txt".into()))
        .unwrap();
    let r = eval_with_rules_fmt(
        "P.apply_path_mapping()",
        vec![rule],
        &st,
        PathFormat::Windows,
    );
    assert_eq!(r.to_display_string(), "C:\\projects2\\file.txt");
}

#[test]
fn windows_trailing_backslash_preserved_eval() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Windows,
        source_path: "C:\\projects".into(),
        destination_path: "/mnt/projects".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("C:\\projects\\subdir\\".into()))
        .unwrap();
    let r = eval_with_rules_fmt(
        "P.apply_path_mapping()",
        vec![rule],
        &st,
        PathFormat::Windows,
    );
    assert!(r.to_display_string().ends_with('/') || r.to_display_string().ends_with('\\'));
}

#[test]
fn windows_trailing_forward_slash_preserved_eval() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Windows,
        source_path: "C:\\projects".into(),
        destination_path: "/mnt/projects".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("C:\\projects\\subdir/".into()))
        .unwrap();
    let r = eval_with_rules_fmt(
        "P.apply_path_mapping()",
        vec![rule],
        &st,
        PathFormat::Windows,
    );
    assert!(r.to_display_string().ends_with('/') || r.to_display_string().ends_with('\\'));
}

// === apply_path_mapping only accepts string, not path (spec §2.2.6) ===
#[test]
fn apply_path_mapping_rejects_path_input() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/src".into(),
        destination_path: "/dst".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::new_path("/src/file.txt", PathFormat::Posix))
        .unwrap();
    let parsed = openjd_expr::ParsedExpression::new("P.apply_path_mapping()").unwrap();
    let lib = FunctionLibrary::for_profile(
        &ExprProfile::current().with_host_context(HostContext::with_rules(vec![rule]).unwrap()),
    );
    let result = parsed.with_library(&lib).evaluate(&[&st]);
    assert!(
        result.is_err(),
        "apply_path_mapping should reject path input, only string is allowed"
    );
}

// ══════════════════════════════════════════════════════════════
// BUG-3: URI scheme+authority case-insensitive, path case-sensitive
// Per RFC 3986 §3.1 and §3.2
// ══════════════════════════════════════════════════════════════

mod uri_case_sensitivity {
    use super::ApplyOne;
    use openjd_expr::{PathFormat, PathMappingRule};

    fn uri_rule(src: &str, dst: &str) -> PathMappingRule {
        PathMappingRule {
            source_path_format: PathFormat::Uri,
            source_path: src.into(),
            destination_path: dst.into(),
        }
    }

    #[test]
    fn uri_scheme_case_insensitive_match() {
        let rule = uri_rule("s3://bucket/prefix", "s3://other-bucket/out");
        // Uppercase scheme should still match
        let result = rule.apply_with_format("S3://bucket/prefix/file.txt", PathFormat::Posix);
        assert_eq!(
            result,
            Some("s3://other-bucket/out/file.txt".to_string()),
            "URI scheme comparison should be case-insensitive"
        );
    }

    #[test]
    fn uri_authority_case_insensitive_match() {
        let rule = uri_rule("s3://mybucket/prefix", "s3://other/out");
        // Uppercase authority should still match
        let result = rule.apply_with_format("s3://MyBucket/prefix/file.txt", PathFormat::Posix);
        assert_eq!(
            result,
            Some("s3://other/out/file.txt".to_string()),
            "URI authority comparison should be case-insensitive"
        );
    }

    #[test]
    fn uri_path_case_sensitive() {
        let rule = uri_rule("s3://bucket/Prefix", "s3://other/out");
        // Path component is case-sensitive, so lowercase 'prefix' should NOT match 'Prefix'
        let result = rule.apply_with_format("s3://bucket/prefix/file.txt", PathFormat::Posix);
        assert_eq!(result, None, "URI path comparison should be case-sensitive");
    }

    #[test]
    fn uri_exact_case_match_still_works() {
        let rule = uri_rule("s3://bucket/prefix", "s3://other/out");
        let result = rule.apply_with_format("s3://bucket/prefix/file.txt", PathFormat::Posix);
        assert_eq!(result, Some("s3://other/out/file.txt".to_string()));
    }
}

#[allow(dead_code)]
fn rule_vec(r: PathMappingRule) -> Vec<PathMappingRule> {
    vec![r]
}
