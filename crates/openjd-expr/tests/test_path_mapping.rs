// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_path_mapping.py

use openjd_expr::{ExprValue, PathFormat, PathMappingRule, SymbolTable};

fn eval_with_rules(expr: &str, rules: Vec<PathMappingRule>, st: &SymbolTable) -> ExprValue {
    eval_with_rules_fmt(expr, rules, st, PathFormat::host())
}

fn eval_with_rules_fmt(
    expr: &str,
    rules: Vec<PathMappingRule>,
    st: &SymbolTable,
    fmt: PathFormat,
) -> ExprValue {
    let parsed = openjd_expr::ParsedExpression::new(expr).unwrap();
    let symtabs = [st];
    let lib = openjd_expr::default_library::get_default_library()
        .clone()
        .with_host_context();
    let mut ev = parsed
        .evaluator(&symtabs)
        .with_library(&lib)
        .with_path_mapping_rules(&rules)
        .with_path_format(fmt);
    ev.evaluate(&parsed.ast).unwrap()
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
    assert_eq!(rule.apply("/src"), Some("/dst".into()));
    assert_eq!(rule.apply("/src/file.txt"), Some("/dst/file.txt".into()));
    // Trailing slash → trailing slash preserved
    assert_eq!(rule.apply("/src/"), Some("/dst/".into()));
    assert_eq!(rule.apply("/src/subdir/"), Some("/dst/subdir/".into()));
}

#[test]
fn uri_trailing_slash_exact_output() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Uri,
        source_path: "s3://bucket/prefix".into(),
        destination_path: "/local".into(),
    };
    // No trailing slash → no trailing slash
    assert_eq!(rule.apply("s3://bucket/prefix"), Some("/local".into()));
    assert_eq!(
        rule.apply("s3://bucket/prefix/file.txt"),
        Some("/local/file.txt".into())
    );
    // Trailing slash → trailing slash preserved
    assert_eq!(rule.apply("s3://bucket/prefix/"), Some("/local/".into()));
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
    use openjd_expr::path_mapping::PathFormat;
    use openjd_expr::PathMappingRule;
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
            posix_rule("/src", "/dst").apply("/src/a/b"),
            Some("/dst/a/b".into())
        );
    }
    #[test]
    fn posix_exact() {
        assert_eq!(
            posix_rule("/src", "/dst").apply("/src"),
            Some("/dst".into())
        );
    }
    #[test]
    fn posix_trailing() {
        assert_eq!(
            posix_rule("/src", "/dst").apply("/src/"),
            Some("/dst/".into())
        );
    }
    #[test]
    fn posix_subdir_trailing() {
        assert_eq!(
            posix_rule("/src", "/dst").apply("/src/a/b/"),
            Some("/dst/a/b/".into())
        );
    }
    #[test]
    fn posix_no_match() {
        assert_eq!(posix_rule("/src", "/dst").apply("/other"), None);
    }
    #[test]
    fn posix_prefix_overlap() {
        assert_eq!(posix_rule("/src", "/dst").apply("/srcextra/file"), None);
    }
    #[test]
    fn posix_case_sensitive() {
        assert_eq!(posix_rule("/Src", "/dst").apply("/src/file"), None);
    }

    // ── Windows ──

    #[test]
    fn windows_subpath() {
        assert_eq!(
            windows_rule("C:\\old", "/new").apply("C:\\old\\sub\\file.txt"),
            Some("/new/sub/file.txt".into())
        );
    }
    #[test]
    fn windows_exact() {
        assert_eq!(
            windows_rule("C:\\old", "/new").apply("C:\\old"),
            Some("/new".into())
        );
    }
    #[test]
    fn windows_trailing_backslash() {
        assert_eq!(
            windows_rule("C:\\old", "/new").apply("C:\\old\\"),
            Some("/new/".into())
        );
    }
    #[test]
    fn windows_trailing_fwdslash() {
        assert_eq!(
            windows_rule("C:\\old", "/new").apply("C:\\old/"),
            Some("/new/".into())
        );
    }
    #[test]
    fn windows_no_match() {
        assert_eq!(windows_rule("C:\\old", "/new").apply("D:\\old\\file"), None);
    }
    #[test]
    fn windows_prefix_overlap() {
        assert_eq!(
            windows_rule("Z:\\shared", "/new").apply("Z:\\sharedextra\\file"),
            None
        );
    }

    // Windows case insensitivity (matches Python PureWindowsPath.is_relative_to)
    #[test]
    fn windows_case_insensitive_lower() {
        assert_eq!(
            windows_rule("C:\\Users", "/home").apply("c:\\users\\bob"),
            Some("/home/bob".into())
        );
    }
    #[test]
    fn windows_case_insensitive_upper() {
        assert_eq!(
            windows_rule("C:\\Users", "/home").apply("C:\\USERS\\bob"),
            Some("/home/bob".into())
        );
    }

    // Windows forward slashes in input path (PureWindowsPath accepts both)
    #[test]
    fn windows_fwd_slash_input() {
        assert_eq!(
            windows_rule("C:\\data", "/data").apply("C:/data/file.txt"),
            Some("/data/file.txt".into())
        );
    }
    #[test]
    fn windows_mixed_slash_input() {
        assert_eq!(
            windows_rule("C:\\data", "/data").apply("C:\\data/sub/file.txt"),
            Some("/data/sub/file.txt".into())
        );
    }

    // Windows to posix destination
    #[test]
    fn windows_to_posix_deep() {
        assert_eq!(
            windows_rule("Z:\\shared", "/mnt/shared").apply("Z:\\shared\\project\\file.txt"),
            Some("/mnt/shared/project/file.txt".into())
        );
    }

    // ── URI ──

    #[test]
    fn uri_subpath() {
        assert_eq!(
            uri_rule("s3://bucket/prefix", "/local").apply("s3://bucket/prefix/file.txt"),
            Some("/local/file.txt".into())
        );
    }
    #[test]
    fn uri_exact() {
        assert_eq!(
            uri_rule("s3://bucket/prefix", "/local").apply("s3://bucket/prefix"),
            Some("/local".into())
        );
    }
    #[test]
    fn uri_trailing() {
        assert_eq!(
            uri_rule("s3://bucket/prefix", "/local").apply("s3://bucket/prefix/"),
            Some("/local/".into())
        );
    }
    #[test]
    fn uri_nested() {
        assert_eq!(
            uri_rule("s3://bucket/prefix", "/local").apply("s3://bucket/prefix/a/b/c"),
            Some("/local/a/b/c".into())
        );
    }
    #[test]
    fn uri_no_match_bucket() {
        assert_eq!(
            uri_rule("s3://bucket/prefix", "/local").apply("s3://other/prefix/file"),
            None
        );
    }
    #[test]
    fn uri_no_match_prefix() {
        assert_eq!(
            uri_rule("s3://bucket/prefix", "/local").apply("s3://bucket/prefixextra"),
            None
        );
    }
    #[test]
    fn uri_no_match_scheme() {
        assert_eq!(
            uri_rule("s3://bucket/prefix", "/local").apply("gs://bucket/prefix/file"),
            None
        );
    }
    #[test]
    fn uri_no_match_filesystem() {
        assert_eq!(uri_rule("s3://bucket", "/local").apply("/local/file"), None);
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

    // ── apply_rules ──

    #[test]
    fn apply_rules_first_match_wins() {
        let rules = vec![posix_rule("/a", "/first"), posix_rule("/a", "/second")];
        assert_eq!(
            openjd_expr::path_mapping::apply_rules(&rules, "/a/file"),
            "/first/file"
        );
    }

    #[test]
    fn apply_rules_no_match_returns_original() {
        let rules = vec![posix_rule("/a", "/b")];
        assert_eq!(
            openjd_expr::path_mapping::apply_rules(&rules, "/other/file"),
            "/other/file"
        );
    }

    #[test]
    fn apply_rules_empty_returns_original() {
        assert_eq!(
            openjd_expr::path_mapping::apply_rules(&[], "/any/path"),
            "/any/path"
        );
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
    st.set(
        "P",
        ExprValue::Path {
            value: "/src/file.txt".into(),
            format: PathFormat::Posix,
        },
    )
    .unwrap();
    let parsed = openjd_expr::ParsedExpression::new("P.apply_path_mapping()").unwrap();
    let symtabs = [&st];
    let lib = openjd_expr::default_library::get_default_library()
        .clone()
        .with_host_context();
    let rules = vec![rule];
    let mut ev = parsed
        .evaluator(&symtabs)
        .with_library(&lib)
        .with_path_mapping_rules(&rules);
    let result = ev.evaluate(&parsed.ast);
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
        let result = rule.apply("S3://bucket/prefix/file.txt");
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
        let result = rule.apply("s3://MyBucket/prefix/file.txt");
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
        let result = rule.apply("s3://bucket/prefix/file.txt");
        assert_eq!(result, None, "URI path comparison should be case-sensitive");
    }

    #[test]
    fn uri_exact_case_match_still_works() {
        let rule = uri_rule("s3://bucket/prefix", "s3://other/out");
        let result = rule.apply("s3://bucket/prefix/file.txt");
        assert_eq!(result, Some("s3://other/out/file.txt".to_string()));
    }
}

#[allow(dead_code)]
fn rule_vec(r: PathMappingRule) -> Vec<PathMappingRule> {
    vec![r]
}
