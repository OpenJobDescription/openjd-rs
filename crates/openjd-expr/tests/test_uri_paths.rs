// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_uri_paths.py

use openjd_expr::{ExprValue, ParsedExpression, PathFormat, SymbolTable};

#[allow(dead_code)]
fn eval(expr: &str) -> ExprValue {
    let parsed = ParsedExpression::new(expr).unwrap();
    let st = SymbolTable::new();
    let symtabs = [&st];
    parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap()
}
fn eval_with(expr: &str, st: &SymbolTable) -> ExprValue {
    let parsed = ParsedExpression::new(expr).unwrap();
    let symtabs = [st];
    parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap()
}

fn uri_st(key: &str, uri: &str) -> SymbolTable {
    let mut st = SymbolTable::new();
    st.set(key, ExprValue::new_path(uri.to_string(), PathFormat::Posix))
        .unwrap();
    st
}

// === TestUriProperties ===
#[test]
fn uri_name() {
    assert_eq!(
        eval_with("P.name", &uri_st("P", "s3://bucket/path/file.txt")).to_display_string(),
        "file.txt"
    );
}
#[test]
fn uri_stem() {
    assert_eq!(
        eval_with("P.stem", &uri_st("P", "s3://bucket/path/file.txt")).to_display_string(),
        "file"
    );
}
#[test]
fn uri_suffix() {
    assert_eq!(
        eval_with("P.suffix", &uri_st("P", "s3://bucket/path/file.txt")).to_display_string(),
        ".txt"
    );
}
#[test]
fn uri_parent() {
    assert_eq!(
        eval_with("P.parent", &uri_st("P", "s3://bucket/path/file.txt")).to_display_string(),
        "s3://bucket/path"
    );
}

// === TestUriPathExpressions ===
#[test]
fn uri_is_absolute() {
    assert_eq!(
        eval_with("P.is_absolute()", &uri_st("P", "s3://bucket/file")).to_display_string(),
        "true"
    );
}

// === TestUriPathSchemeVariety ===
#[test]
fn uri_s3() {
    assert_eq!(
        eval_with("P.name", &uri_st("P", "s3://bucket/file.txt")).to_display_string(),
        "file.txt"
    );
}
#[test]
fn uri_gs() {
    assert_eq!(
        eval_with("P.name", &uri_st("P", "gs://bucket/file.txt")).to_display_string(),
        "file.txt"
    );
}
#[test]
fn uri_http() {
    assert_eq!(
        eval_with("P.name", &uri_st("P", "http://host/path/file.txt")).to_display_string(),
        "file.txt"
    );
}

// === TestUriPathInSymbolTable ===
#[test]
fn uri_in_symtab() {
    let st = uri_st("P", "s3://my-bucket/renders/output.exr");
    assert_eq!(eval_with("P.name", &st).to_display_string(), "output.exr");
    assert_eq!(eval_with("P.stem", &st).to_display_string(), "output");
    assert_eq!(eval_with("P.suffix", &st).to_display_string(), ".exr");
}

// === Additional URI path tests ported from Python ===

// URI detection
#[test]
fn uri_not_uri() {
    assert_eq!(
        eval_with("P.is_absolute()", &uri_st("P", "/local/path")).to_display_string(),
        "true"
    );
}
#[test]
fn uri_posix_not_uri() {
    assert_eq!(
        eval_with("P.name", &uri_st("P", "/local/file.txt")).to_display_string(),
        "file.txt"
    );
}
#[test]
fn uri_custom_scheme() {
    assert_eq!(
        eval_with("P.name", &uri_st("P", "myscheme://host/file.txt")).to_display_string(),
        "file.txt"
    );
}

// URI properties
#[test]
fn uri_stem_compound() {
    assert_eq!(
        eval_with("P.stem", &uri_st("P", "s3://bucket/file.tar.gz")).to_display_string(),
        "file.tar"
    );
}
#[test]
fn uri_suffix_compound() {
    assert_eq!(
        eval_with("P.suffix", &uri_st("P", "s3://bucket/file.tar.gz")).to_display_string(),
        ".gz"
    );
}
#[test]
fn uri_suffix_none() {
    assert_eq!(
        eval_with("P.suffix", &uri_st("P", "s3://bucket/file")).to_display_string(),
        ""
    );
}
#[test]
fn uri_suffixes() {
    let r = eval_with("P.suffixes", &uri_st("P", "s3://bucket/file.tar.gz"));
    assert!(r.is_list());
}
#[test]
fn uri_suffixes_none() {
    let r = eval_with("P.suffixes", &uri_st("P", "s3://bucket/file"));
    assert!(r.is_list());
    assert_eq!(r.list_len(), Some(0));
}
#[test]
fn uri_suffixes_single() {
    let r = eval_with("P.suffixes", &uri_st("P", "s3://bucket/file.txt"));
    assert_eq!(r.list_len(), Some(1));
}
#[test]
fn uri_parts() {
    let r = eval_with("P.parts", &uri_st("P", "s3://bucket/a/b"));
    assert!(r.is_list());
}
#[test]
fn uri_parent_at_root() {
    assert_eq!(
        eval_with("P.parent", &uri_st("P", "s3://bucket")).to_display_string(),
        "s3://bucket"
    );
}
#[test]
fn uri_parent_single_component() {
    assert_eq!(
        eval_with("P.parent", &uri_st("P", "s3://bucket/file")).to_display_string(),
        "s3://bucket"
    );
}
#[test]
fn uri_parent_chain() {
    assert_eq!(
        eval_with("P.parent.parent", &uri_st("P", "s3://bucket/a/b/c")).to_display_string(),
        "s3://bucket/a"
    );
}
#[test]
fn uri_name_bare() {
    assert_eq!(
        eval_with("P.name", &uri_st("P", "s3://bucket")).to_display_string(),
        ""
    );
}

// URI path operations
#[test]
fn uri_with_suffix() {
    assert_eq!(
        eval_with(
            "P.with_suffix('.png')",
            &uri_st("P", "s3://bucket/file.txt")
        )
        .to_display_string(),
        "s3://bucket/file.png"
    );
}
#[test]
fn uri_with_name() {
    assert_eq!(
        eval_with(
            "P.with_name('other.txt')",
            &uri_st("P", "s3://bucket/file.txt")
        )
        .to_display_string(),
        "s3://bucket/other.txt"
    );
}
#[test]
fn uri_with_stem() {
    assert_eq!(
        eval_with("P.with_stem('other')", &uri_st("P", "s3://bucket/file.txt")).to_display_string(),
        "s3://bucket/other.txt"
    );
}
#[test]
fn uri_with_number() {
    assert_eq!(
        eval_with(
            "P.with_number(42)",
            &uri_st("P", "s3://bucket/shot_####.exr")
        )
        .to_display_string(),
        "s3://bucket/shot_0042.exr"
    );
}
#[test]
fn uri_as_posix() {
    assert_eq!(
        eval_with("P.as_posix()", &uri_st("P", "s3://bucket/a/b")).to_display_string(),
        "s3://bucket/a/b"
    );
}

// URI join
#[test]
fn uri_join() {
    assert_eq!(
        eval_with("P / 'child'", &uri_st("P", "s3://bucket/dir")).to_display_string(),
        "s3://bucket/dir/child"
    );
}
#[test]
fn uri_join_multi() {
    assert_eq!(
        eval_with("P / 'a' / 'b'", &uri_st("P", "s3://bucket")).to_display_string(),
        "s3://bucket/a/b"
    );
}
#[test]
fn uri_join_trailing_slash() {
    assert_eq!(
        eval_with("P / 'file'", &uri_st("P", "s3://bucket/dir/")).to_display_string(),
        "s3://bucket/dir/file"
    );
}

// URI no normalization
#[test]
fn uri_double_slash_preserved() {
    assert_eq!(
        eval_with("P.name", &uri_st("P", "s3://bucket/a//b/file.txt")).to_display_string(),
        "file.txt"
    );
}
#[test]
fn uri_dot_segments_preserved() {
    assert!(eval_with("P.parts", &uri_st("P", "s3://bucket/a/./b/../c")).is_list());
}
#[test]
fn uri_trailing_slash() {
    assert_eq!(
        eval_with("P.name", &uri_st("P", "s3://bucket/prefix/")).to_display_string(),
        ""
    );
}

// URI construction
#[test]
fn uri_from_string() {
    assert!(matches!(
        eval("path('s3://bucket/file')"),
        ExprValue::Path { .. }
    ));
}
#[test]
fn uri_from_parts() {
    let r = eval("path(['s3://bucket', 'dir', 'file.txt'])");
    assert_eq!(r.to_display_string(), "s3://bucket/dir/file.txt");
}
#[test]
fn uri_from_parts_double_slash() {
    let r = eval("path(['s3://bucket', 'a', '', 'b'])");
    assert_eq!(r.to_display_string(), "s3://bucket/a//b");
}

// URI scheme variety
#[test]
fn uri_https() {
    assert_eq!(
        eval_with("P.name", &uri_st("P", "https://host/path/file.txt")).to_display_string(),
        "file.txt"
    );
}
#[test]
fn uri_fsx() {
    assert_eq!(
        eval_with("P.name", &uri_st("P", "fsx://vol/path/file.txt")).to_display_string(),
        "file.txt"
    );
}

// URI in symbol table
#[test]
fn uri_join_in_symtab() {
    let st = uri_st("P", "s3://bucket/renders");
    let r = eval_with("P / 'output.exr'", &st);
    assert_eq!(r.to_display_string(), "s3://bucket/renders/output.exr");
}
#[test]
fn uri_with_suffix_in_symtab() {
    let st = uri_st("P", "s3://bucket/renders/scene.exr");
    let r = eval_with("P.with_suffix('.png')", &st);
    assert_eq!(r.to_display_string(), "s3://bucket/renders/scene.png");
}

// URI roundtrip via parts
#[test]
fn uri_roundtrip_via_parts() {
    let st = uri_st("P", "s3://bucket/a/b/file.txt");
    let r = eval_with("string(path(P.parts))", &st);
    assert_eq!(r.to_display_string(), "s3://bucket/a/b/file.txt");
}

// URI concat
#[test]
fn uri_concat() {
    let st = uri_st("P", "s3://bucket/file");
    let r = eval_with("P + '.txt'", &st);
    assert_eq!(r.to_display_string(), "s3://bucket/file.txt");
}

// === URI edge cases ===
#[test]
fn uri_bare_authority() {
    assert_eq!(
        eval_with("P.name", &uri_st("P", "s3://bucket")).to_display_string(),
        ""
    );
}
#[test]
fn uri_bare_scheme_not_uri() {
    assert_eq!(
        eval_with("P.is_absolute()", &uri_st("P", "notascheme")).to_display_string(),
        "false"
    );
}
#[test]
fn uri_colon_in_path_not_uri() {
    assert_eq!(
        eval_with("P.name", &uri_st("P", "/path/with:colon")).to_display_string(),
        "with:colon"
    );
}
#[test]
fn uri_windows_drive_not_uri() {
    assert_eq!(
        eval_with("P.name", &uri_st("P", "C:/Users/test")).to_display_string(),
        "test"
    );
}
#[test]
fn uri_single_component() {
    assert_eq!(
        eval_with("P.name", &uri_st("P", "s3://bucket/file")).to_display_string(),
        "file"
    );
}
#[test]
fn uri_triple_slash_preserved() {
    let r = eval_with("P.parts", &uri_st("P", "s3://bucket/a///b"));
    assert!(r.is_list());
}
#[test]
fn uri_double_slash_parts() {
    let r = eval_with("P.parts", &uri_st("P", "s3://bucket/a//b"));
    assert!(r.is_list());
    // Should have empty string component for the double slash
    let elems = r.list_elements().unwrap();
    assert!(
        elems.iter().any(|e| e.to_display_string().is_empty()),
        "should have empty component"
    );
}
#[test]
fn uri_dot_segments_parts() {
    let r = eval_with("P.parts", &uri_st("P", "s3://bucket/a/./b/../c"));
    let elems = r.list_elements().unwrap();
    // Dots should be preserved (no normalization)
    assert!(
        elems.iter().any(|e| e.to_display_string() == "."),
        "should have . component"
    );
    assert!(
        elems.iter().any(|e| e.to_display_string() == ".."),
        "should have .. component"
    );
}
#[test]
fn uri_from_parts_bare() {
    assert_eq!(
        eval("path(['s3://bucket'])").to_display_string(),
        "s3://bucket"
    );
}
#[test]
fn uri_join_absolute_replaces() {
    // Joining an absolute path replaces the left side
    assert_eq!(
        eval_with("P / '/new/path'", &uri_st("P", "s3://bucket/old")).to_display_string(),
        "/new/path"
    );
}
#[test]
fn uri_join_string() {
    assert_eq!(
        eval_with("P / 'subdir/file.txt'", &uri_st("P", "s3://bucket")).to_display_string(),
        "s3://bucket/subdir/file.txt"
    );
}

// === Exact Python name matches for URI tests ===

// TestUriDetection
#[test]
fn s3_uri() {
    assert!(eval("path('s3://bucket/key').is_absolute()").to_display_string() == "true");
}
#[test]
fn https_uri() {
    assert!(eval("path('https://host/path').is_absolute()").to_display_string() == "true");
}
#[test]
fn fsx_uri() {
    assert!(eval("path('fsx://vol/path').is_absolute()").to_display_string() == "true");
}
#[test]
fn posix_absolute_not_uri() {
    assert_eq!(
        eval("path('/local/path').is_absolute()").to_display_string(),
        "true"
    );
}
#[test]
fn relative_not_uri() {
    assert_eq!(
        eval("path('relative/path').is_absolute()").to_display_string(),
        "false"
    );
}
#[test]
fn bare_scheme_no_slashes_not_uri() {
    assert_eq!(
        eval("path('mailto:user@host').is_absolute()").to_display_string(),
        "false"
    );
}

// TestUriParts
#[test]
fn basic_parts() {
    let r = eval_with("P.parts", &uri_st("P", "s3://bucket/a/b"));
    assert!(r.is_list());
}
#[test]
fn single_component_parts() {
    let r = eval_with("P.parts", &uri_st("P", "s3://bucket/file"));
    assert!(r.is_list());
}
#[test]
fn trailing_slash_parts() {
    let r = eval_with("P.parts", &uri_st("P", "s3://bucket/prefix/"));
    assert!(r.is_list());
}

// TestUriProperties
#[test]
fn name() {
    assert_eq!(
        eval_with("P.name", &uri_st("P", "s3://bucket/dir/file.txt")).to_display_string(),
        "file.txt"
    );
}
#[test]
fn stem() {
    assert_eq!(
        eval_with("P.stem", &uri_st("P", "s3://bucket/dir/file.txt")).to_display_string(),
        "file"
    );
}
#[test]
fn suffix() {
    assert_eq!(
        eval_with("P.suffix", &uri_st("P", "s3://bucket/dir/file.txt")).to_display_string(),
        ".txt"
    );
}
#[test]
fn parent() {
    assert_eq!(
        eval_with("P.parent", &uri_st("P", "s3://bucket/dir/file.txt")).to_display_string(),
        "s3://bucket/dir"
    );
}
#[test]
fn suffixes() {
    let r = eval_with("P.suffixes", &uri_st("P", "s3://bucket/file.tar.gz"));
    assert_eq!(r.list_len(), Some(2));
}

// TestUriFromParts
#[test]
fn from_parts() {
    assert_eq!(
        eval("path(['s3://bucket', 'dir', 'file.txt'])").to_display_string(),
        "s3://bucket/dir/file.txt"
    );
}
#[test]
fn from_parts_bare() {
    assert_eq!(
        eval("path(['s3://bucket'])").to_display_string(),
        "s3://bucket"
    );
}
#[test]
fn from_parts_double_slash() {
    assert_eq!(
        eval("path(['s3://bucket', 'a', '', 'b'])").to_display_string(),
        "s3://bucket/a//b"
    );
}
#[test]
fn roundtrip() {
    let st = uri_st("P", "s3://bucket/a/b/file.txt");
    assert_eq!(
        eval_with("string(path(P.parts))", &st).to_display_string(),
        "s3://bucket/a/b/file.txt"
    );
}

// TestUriPathOperators
#[test]
fn join_string() {
    assert_eq!(
        eval_with("P / 'subdir/file.txt'", &uri_st("P", "s3://bucket")).to_display_string(),
        "s3://bucket/subdir/file.txt"
    );
}
#[test]
fn join_multi() {
    assert_eq!(
        eval_with("P / 'a' / 'b'", &uri_st("P", "s3://bucket")).to_display_string(),
        "s3://bucket/a/b"
    );
}
#[test]
fn join_absolute_replaces() {
    assert_eq!(
        eval_with("P / '/new/path'", &uri_st("P", "s3://bucket/old")).to_display_string(),
        "/new/path"
    );
}
#[test]
fn join_trailing_slash_no_double() {
    assert_eq!(
        eval_with("P / 'file'", &uri_st("P", "s3://bucket/dir/")).to_display_string(),
        "s3://bucket/dir/file"
    );
}
#[test]
fn concat() {
    assert_eq!(
        eval_with("P + '.txt'", &uri_st("P", "s3://bucket/file")).to_display_string(),
        "s3://bucket/file.txt"
    );
}
#[test]
fn with_suffix() {
    assert_eq!(
        eval_with(
            "P.with_suffix('.png')",
            &uri_st("P", "s3://bucket/file.txt")
        )
        .to_display_string(),
        "s3://bucket/file.png"
    );
}
#[test]
fn with_name() {
    assert_eq!(
        eval_with(
            "P.with_name('other.txt')",
            &uri_st("P", "s3://bucket/file.txt")
        )
        .to_display_string(),
        "s3://bucket/other.txt"
    );
}
#[test]
fn with_stem() {
    assert_eq!(
        eval_with("P.with_stem('other')", &uri_st("P", "s3://bucket/file.txt")).to_display_string(),
        "s3://bucket/other.txt"
    );
}
#[test]
fn as_posix_identity() {
    assert_eq!(
        eval_with("P.as_posix()", &uri_st("P", "s3://bucket/a/b")).to_display_string(),
        "s3://bucket/a/b"
    );
}
#[test]
fn with_number() {
    assert_eq!(
        eval_with(
            "P.with_number(42)",
            &uri_st("P", "s3://bucket/shot_####.exr")
        )
        .to_display_string(),
        "s3://bucket/shot_0042.exr"
    );
}

// TestUriPathConstruction
#[test]
fn from_string() {
    assert!(matches!(
        eval("path('s3://bucket/file')"),
        ExprValue::Path { .. }
    ));
}
#[test]
fn from_parts_with_empty_preserves_double_slash() {
    assert_eq!(
        eval("path(['s3://bucket', 'a', '', 'b'])").to_display_string(),
        "s3://bucket/a//b"
    );
}

// TestUriPathSchemeVariety
#[test]
fn https() {
    assert_eq!(
        eval_with("P.name", &uri_st("P", "https://host/path/file.txt")).to_display_string(),
        "file.txt"
    );
}
#[test]
fn fsx() {
    assert_eq!(
        eval_with("P.name", &uri_st("P", "fsx://vol/path/file.txt")).to_display_string(),
        "file.txt"
    );
}
#[test]
fn custom_scheme() {
    assert_eq!(
        eval_with("P.name", &uri_st("P", "myscheme://host/file.txt")).to_display_string(),
        "file.txt"
    );
}

// TestUriPathInSymbolTable
#[test]
fn uri_in_symtab_name() {
    let st = uri_st("P", "s3://bucket/renders/scene.exr");
    assert_eq!(eval_with("P.name", &st).to_display_string(), "scene.exr");
}

// === Missing Python tests ported below ===

fn list_strings(v: &ExprValue) -> Vec<String> {
    v.list_elements()
        .unwrap()
        .iter()
        .map(|e| e.to_display_string())
        .collect()
}

// TestUriDetection: custom scheme with special chars (my-scheme+2)
#[test]
fn uri_custom_scheme_with_special_chars() {
    assert_eq!(
        eval("path('my-scheme+2://server/path/file.txt').name").to_display_string(),
        "file.txt"
    );
}

// TestUriDetection: s3:bucket/key (no double slash) is not a URI
#[test]
fn bare_scheme_no_double_slash_not_uri() {
    assert_eq!(
        eval("path('s3:bucket/key').is_absolute()").to_display_string(),
        "false"
    );
}

// TestUriParts: exact value checks
#[test]
fn parts_exact_basic() {
    assert_eq!(
        list_strings(&eval_with(
            "P.parts",
            &uri_st("P", "s3://bucket/dir/file.obj")
        )),
        vec!["s3://bucket", "dir", "file.obj"]
    );
}
#[test]
fn parts_exact_single_component() {
    assert_eq!(
        list_strings(&eval_with("P.parts", &uri_st("P", "s3://bucket/key"))),
        vec!["s3://bucket", "key"]
    );
}
#[test]
fn parts_exact_bare_authority() {
    assert_eq!(
        list_strings(&eval_with("P.parts", &uri_st("P", "s3://bucket"))),
        vec!["s3://bucket"]
    );
}
#[test]
fn parts_exact_double_slash() {
    assert_eq!(
        list_strings(&eval_with("P.parts", &uri_st("P", "s3://bucket/a//b/c"))),
        vec!["s3://bucket", "a", "", "b", "c"]
    );
}
#[test]
fn parts_exact_triple_slash() {
    assert_eq!(
        list_strings(&eval_with("P.parts", &uri_st("P", "s3://bucket/a///b"))),
        vec!["s3://bucket", "a", "", "", "b"]
    );
}
#[test]
fn parts_exact_dot_segments() {
    assert_eq!(
        list_strings(&eval_with(
            "P.parts",
            &uri_st("P", "s3://bucket/a/./b/../c")
        )),
        vec!["s3://bucket", "a", ".", "b", "..", "c"]
    );
}
#[test]
fn parts_exact_trailing_slash() {
    assert_eq!(
        list_strings(&eval_with("P.parts", &uri_st("P", "s3://bucket/prefix/"))),
        vec!["s3://bucket", "prefix", ""]
    );
}

// TestUriPathExpressions: exact suffixes values
#[test]
fn expr_suffixes_exact() {
    assert_eq!(
        list_strings(&eval("path('https://host/archive.tar.gz').suffixes")),
        vec![".tar", ".gz"]
    );
}

// TestUriPathExpressions: exact parts values
#[test]
fn expr_parts_exact() {
    assert_eq!(
        list_strings(&eval("path('s3://bucket/dir/file.obj').parts")),
        vec!["s3://bucket", "dir", "file.obj"]
    );
}

// TestUriPathExpressions: parent chain to root and beyond
#[test]
fn expr_parent_chain_to_root() {
    assert_eq!(
        eval("path('s3://bucket/a/b').parent.parent.parent").to_display_string(),
        "s3://bucket"
    );
}

// TestUriPathExpressions: bare authority parts via expression
#[test]
fn expr_bare_authority_parts() {
    assert_eq!(
        list_strings(&eval("path('s3://bucket').parts")),
        vec!["s3://bucket"]
    );
}

// TestUriPathNoNormalization: to_string preservation checks
#[test]
fn no_norm_double_slash_to_string() {
    assert_eq!(
        eval("path('s3://bucket/a//b/file.txt')").to_display_string(),
        "s3://bucket/a//b/file.txt"
    );
}
#[test]
fn no_norm_double_slash_parts_exact() {
    assert_eq!(
        list_strings(&eval("path('s3://bucket/a//b/file.txt').parts")),
        vec!["s3://bucket", "a", "", "b", "file.txt"]
    );
}
#[test]
fn no_norm_triple_slash_to_string() {
    assert_eq!(
        eval("path('s3://bucket/a///b')").to_display_string(),
        "s3://bucket/a///b"
    );
}
#[test]
fn no_norm_dot_segments_to_string() {
    assert_eq!(
        eval("path('s3://bucket/a/./b/../c')").to_display_string(),
        "s3://bucket/a/./b/../c"
    );
}
#[test]
fn no_norm_dot_segments_parts_exact() {
    assert_eq!(
        list_strings(&eval("path('s3://bucket/a/./b/../c').parts")),
        vec!["s3://bucket", "a", ".", "b", "..", "c"]
    );
}
#[test]
fn no_norm_trailing_slash_to_string() {
    assert_eq!(
        eval("path('s3://bucket/prefix/')").to_display_string(),
        "s3://bucket/prefix/"
    );
}
#[test]
fn no_norm_roundtrip_equality() {
    assert_eq!(
        eval("path(path('s3://bucket/a//b/file.txt').parts) == path('s3://bucket/a//b/file.txt')")
            .to_display_string(),
        "true"
    );
}

// TestUriPathOperators: join with 4 segments (Python test_join_multi)
#[test]
fn join_multi_four_segments() {
    assert_eq!(
        eval("path('s3://bucket') / 'a' / 'b' / 'c.txt'").to_display_string(),
        "s3://bucket/a/b/c.txt"
    );
}

// TestUriPathOperators: join with path() object (Python test_join_absolute_replaces)
#[test]
fn join_absolute_path_object_replaces() {
    let r = eval("path('s3://bucket/dir') / path('/local/path')");
    assert!(r.to_display_string().ends_with("/local/path"));
}

// TestUriPathOperators: join_string with sub/file.obj (Python exact)
#[test]
fn join_string_sub_file() {
    assert_eq!(
        eval("path('s3://bucket/dir') / 'sub/file.obj'").to_display_string(),
        "s3://bucket/dir/sub/file.obj"
    );
}

// TestUriPathConstruction: from_string checks to_string value
#[test]
fn from_string_value() {
    assert_eq!(
        eval("path('s3://bucket/dir/file.obj')").to_display_string(),
        "s3://bucket/dir/file.obj"
    );
}

// TestUriPathSchemeVariety: https full to_string check
#[test]
fn https_full_to_string() {
    assert_eq!(
        eval("path('https://example.com/models/scene.obj')").to_display_string(),
        "https://example.com/models/scene.obj"
    );
    assert_eq!(
        eval("path('https://example.com/models/scene.obj').name").to_display_string(),
        "scene.obj"
    );
}

// TestUriPathSchemeVariety: fsx exact parts
#[test]
fn fsx_exact_parts() {
    assert_eq!(
        list_strings(&eval("path('fsx://vol-123/data/file.bin').parts")),
        vec!["fsx://vol-123", "data", "file.bin"]
    );
}

// TestUriPathSchemeVariety: custom scheme parent
#[test]
fn custom_scheme_parent() {
    assert_eq!(
        eval("path('my-scheme+2://server/path/file.txt').parent").to_display_string(),
        "my-scheme+2://server/path"
    );
}

// TestUriPathInSymbolTable: parent check
#[test]
fn uri_in_symtab_parent() {
    let st = uri_st("P", "s3://bucket/dir/file.obj");
    assert_eq!(
        eval_with("P.parent", &st).to_display_string(),
        "s3://bucket/dir"
    );
}

// TestUriPathInSymbolTable: multi-segment join
#[test]
fn uri_join_in_symtab_multi() {
    let mut st = SymbolTable::new();
    st.set(
        "Dir",
        ExprValue::new_path("s3://bucket/assets".to_string(), PathFormat::Posix),
    )
    .unwrap();
    assert_eq!(
        eval_with("Dir / 'sub' / 'file.obj'", &st).to_display_string(),
        "s3://bucket/assets/sub/file.obj"
    );
}

// TestUriFromParts: join helper (Python uri_join with multiple children)
#[test]
fn uri_join_multi_children() {
    // Python: uri_join("s3://bucket/dir", ["sub", "file.obj"]) == "s3://bucket/dir/sub/file.obj"
    assert_eq!(
        eval_with("P / 'sub' / 'file.obj'", &uri_st("P", "s3://bucket/dir")).to_display_string(),
        "s3://bucket/dir/sub/file.obj"
    );
}

// TestUriFromParts: roundtrip with double slash
#[test]
fn roundtrip_double_slash() {
    let st = uri_st("P", "s3://bucket/a//b/file.txt");
    assert_eq!(
        eval_with("string(path(P.parts))", &st).to_display_string(),
        "s3://bucket/a//b/file.txt"
    );
}
