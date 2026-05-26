// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test_paths.py

use openjd_expr::{ExprValue, ParsedExpression, PathFormat, SymbolTable};

fn eval(expr: &str) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap()
}
#[allow(dead_code)]
fn eval_with(expr: &str, st: &SymbolTable) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(st))
        .unwrap()
}
#[allow(dead_code)]
fn eval_fails(expr: &str) -> bool {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_err()
}
fn assert_err(expr: &str, expected: &[&str]) {
    let e = ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}
#[allow(dead_code)]
fn assert_err_with(expr: &str, st: &SymbolTable, expected: &[&str]) {
    let e = ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(st))
        .unwrap_err()
        .to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

fn assert_err_posix(expr: &str, st: &SymbolTable, expected: &[&str]) {
    let parsed = openjd_expr::ParsedExpression::new(expr).unwrap();
    let symtabs = [st];
    let e = parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap_err()
        .to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

fn posix_st(key: &str, path: &str) -> SymbolTable {
    let mut st = SymbolTable::new();
    st.set(
        key,
        ExprValue::new_path(path.to_string(), PathFormat::Posix),
    )
    .unwrap();
    st
}

fn eval_with_fmt(expr: &str, st: &SymbolTable, fmt: PathFormat) -> ExprValue {
    let parsed = openjd_expr::ParsedExpression::new(expr).unwrap();
    let symtabs = [st];
    parsed.with_path_format(fmt).evaluate(&symtabs).unwrap()
}

/// Evaluate with a POSIX symtab — uses PathFormat::Posix so path format checks pass.
fn eval_posix(expr: &str, st: &SymbolTable) -> ExprValue {
    eval_with_fmt(expr, st, PathFormat::Posix)
}

fn windows_st(key: &str, path: &str) -> SymbolTable {
    let mut st = SymbolTable::new();
    st.set(
        key,
        ExprValue::new_path(path.to_string(), PathFormat::Windows),
    )
    .unwrap();
    st
}

fn eval_windows(expr: &str, st: &SymbolTable) -> ExprValue {
    eval_with_fmt(expr, st, PathFormat::Windows)
}

// === TestPaths ===
#[test]
fn path_name() {
    assert_eq!(
        eval_posix("P.name", &posix_st("P", "/a/b/file.txt")).to_display_string(),
        "file.txt"
    );
}
#[test]
fn path_stem() {
    assert_eq!(
        eval_posix("P.stem", &posix_st("P", "/a/b/file.txt")).to_display_string(),
        "file"
    );
}
#[test]
fn path_suffix() {
    assert_eq!(
        eval_posix("P.suffix", &posix_st("P", "/a/b/file.txt")).to_display_string(),
        ".txt"
    );
}
#[test]
fn path_parent() {
    assert_eq!(
        eval_posix("P.parent", &posix_st("P", "/a/b/file.txt")).to_display_string(),
        "/a/b"
    );
}
#[test]
fn path_parts() {
    assert!(eval_posix("P.parts", &posix_st("P", "/a/b/c")).is_list());
}

#[test]
fn path_suffixes() {
    let r = eval_posix("P.suffixes", &posix_st("P", "/a/b/file.tar.gz"));
    assert!(r.is_list());
}

#[test]
fn path_constructor() {
    assert!(matches!(
        eval("path('/tmp/file.txt')"),
        ExprValue::Path { .. }
    ));
}

// === TestIsAbsolute ===
#[test]
fn is_absolute_posix() {
    assert_eq!(
        eval_posix("P.is_absolute()", &posix_st("P", "/tmp")).to_display_string(),
        "true"
    );
}
#[test]
fn is_absolute_relative() {
    assert_eq!(
        eval_posix("P.is_absolute()", &posix_st("P", "relative/path")).to_display_string(),
        "false"
    );
}

// === TestIsRelativeTo ===
#[test]
fn is_relative_to_true() {
    assert_eq!(
        eval_posix("P.is_relative_to('/a/b')", &posix_st("P", "/a/b/c/d")).to_display_string(),
        "true"
    );
}
#[test]
fn is_relative_to_false() {
    assert_eq!(
        eval_posix("P.is_relative_to('/x/y')", &posix_st("P", "/a/b/c")).to_display_string(),
        "false"
    );
}

// === TestRelativeTo ===
#[test]
fn relative_to() {
    assert_eq!(
        eval_posix("P.relative_to('/a/b')", &posix_st("P", "/a/b/c/d")).to_display_string(),
        "c/d"
    );
}
#[test]
fn relative_to_error() {
    assert_err_posix(
        "P.relative_to('/x/y')",
        &posix_st("P", "/a/b"),
        &[
            "relative_to failed: '/a/b' is not relative to '/x/y'\n",
            "  P.relative_to('/x/y')\n",
            "  ~~^~~~~~~~~~~~~~~~~~~",
        ],
    );
}

// === TestWithNumber ===
#[test]
fn with_number() {
    assert_eq!(
        eval_posix("P.with_number(42)", &posix_st("P", "/a/b/file.####.exr")).to_display_string(),
        "/a/b/file.0042.exr"
    );
}

#[test]
fn uri_parent_div_string() {
    let st = posix_st("P", "s3://my-bucket/assets/teapot.obj");
    let parent = eval_posix("P.parent", &st);
    // parent should be path type, not string
    assert!(
        matches!(parent, ExprValue::Path { .. }),
        "parent type: {}",
        parent.expr_type()
    );
    assert_eq!(parent.to_display_string(), "s3://my-bucket/assets");
    // parent / "other.obj" should work
    let joined = eval_posix("P.parent / 'other.obj'", &st);
    assert_eq!(
        joined.to_display_string(),
        "s3://my-bucket/assets/other.obj"
    );
}

// === Additional path tests ported from Python ===

// Path properties
#[test]
fn path_stem_multi_ext() {
    assert_eq!(
        eval_posix("P.stem", &posix_st("P", "/a/b/file.tar.gz")).to_display_string(),
        "file.tar"
    );
}
#[test]
fn path_suffix_multi_ext() {
    assert_eq!(
        eval_posix("P.suffix", &posix_st("P", "/a/b/file.tar.gz")).to_display_string(),
        ".gz"
    );
}
#[test]
fn path_name_empty() {
    assert_eq!(
        eval_posix("P.name", &posix_st("P", "/")).to_display_string(),
        ""
    );
}
#[test]
fn chained_property() {
    assert_eq!(
        eval_posix("P.parent.name", &posix_st("P", "/a/b/c.txt")).to_display_string(),
        "b"
    );
}
#[test]
fn repeated_parent() {
    assert_eq!(
        eval_posix("P.parent.parent", &posix_st("P", "/a/b/c.txt")).to_display_string(),
        "/a"
    );
}
#[test]
fn parent_then_name() {
    assert_eq!(
        eval_posix("P.parent.name", &posix_st("P", "/a/b/file.txt")).to_display_string(),
        "b"
    );
}

// Path construction
#[test]
fn path_from_string() {
    assert!(matches!(
        eval("path('/tmp/file.txt')"),
        ExprValue::Path { .. }
    ));
}
#[test]
fn path_from_list() {
    let r = eval("path(['/', 'a', 'b', 'c'])");
    assert!(matches!(r, ExprValue::Path { .. }));
}
#[test]
fn path_concat() {
    assert_eq!(
        eval_posix("P + '.bak'", &posix_st("P", "/a/b/file")).to_display_string(),
        "/a/b/file.bak"
    );
}

// path_join removed — not in spec

// is_absolute
#[test]
fn posix_absolute() {
    assert_eq!(
        eval_posix("P.is_absolute()", &posix_st("P", "/tmp")).to_display_string(),
        "true"
    );
}
#[test]
fn posix_relative_not_absolute() {
    assert_eq!(
        eval_posix("P.is_absolute()", &posix_st("P", "relative")).to_display_string(),
        "false"
    );
}
#[test]
fn uri_always_absolute() {
    assert_eq!(
        eval_posix("P.is_absolute()", &posix_st("P", "s3://bucket/key")).to_display_string(),
        "true"
    );
}

// is_relative_to
#[test]
fn posix_relative_to_true() {
    assert_eq!(
        eval_posix("P.is_relative_to('/a/b')", &posix_st("P", "/a/b/c/d")).to_display_string(),
        "true"
    );
}
#[test]
fn posix_relative_to_false() {
    assert_eq!(
        eval_posix("P.is_relative_to('/x/y')", &posix_st("P", "/a/b")).to_display_string(),
        "false"
    );
}
#[test]
fn uri_relative_to() {
    assert_eq!(
        eval_posix(
            "P.is_relative_to('s3://bucket')",
            &posix_st("P", "s3://bucket/key")
        )
        .to_display_string(),
        "true"
    );
}
#[test]
fn uri_not_relative_to() {
    assert_eq!(
        eval_posix(
            "P.is_relative_to('s3://other')",
            &posix_st("P", "s3://bucket/key")
        )
        .to_display_string(),
        "false"
    );
}

// relative_to
#[test]
fn posix_relative_to_result() {
    assert_eq!(
        eval_posix("P.relative_to('/a/b')", &posix_st("P", "/a/b/c/d")).to_display_string(),
        "c/d"
    );
}
#[test]
fn posix_relative_to_same() {
    assert_eq!(
        eval_posix("P.relative_to('/a/b')", &posix_st("P", "/a/b")).to_display_string(),
        "."
    );
}
#[test]
fn relative_to_error_short() {
    assert_err_posix(
        "P.relative_to('/x')",
        &posix_st("P", "/a/b"),
        &[
            "relative_to failed: '/a/b' is not relative to '/x'\n",
            "  P.relative_to('/x')\n",
            "  ~~^~~~~~~~~~~~~~~~~",
        ],
    );
}

// with_suffix
#[test]
fn with_suffix_replace() {
    assert_eq!(
        eval_posix("P.with_suffix('.png')", &posix_st("P", "/a/b/file.txt")).to_display_string(),
        "/a/b/file.png"
    );
}

// with_name
#[test]
fn with_name_replace() {
    assert_eq!(
        eval_posix("P.with_name('other.txt')", &posix_st("P", "/a/b/file.txt")).to_display_string(),
        "/a/b/other.txt"
    );
}

// with_stem
#[test]
fn with_stem_replace() {
    assert_eq!(
        eval_posix("P.with_stem('other')", &posix_st("P", "/a/b/file.txt")).to_display_string(),
        "/a/b/other.txt"
    );
}

// as_posix
#[test]
fn as_posix_identity() {
    assert_eq!(
        eval_posix("P.as_posix()", &posix_st("P", "/a/b/c")).to_display_string(),
        "/a/b/c"
    );
}

// with_number patterns
#[test]
fn with_number_digits() {
    assert_eq!(
        eval_posix("P.with_number(72)", &posix_st("P", "/out/file_003.exr")).to_display_string(),
        "/out/file_072.exr"
    );
}
#[test]
fn with_number_printf_d() {
    assert_eq!(
        eval_posix("P.with_number(72)", &posix_st("P", "/out/file_%d.exr")).to_display_string(),
        "/out/file_72.exr"
    );
}
#[test]
fn with_number_printf_04d() {
    assert_eq!(
        eval_posix("P.with_number(72)", &posix_st("P", "/out/file_%04d.exr")).to_display_string(),
        "/out/file_0072.exr"
    );
}
#[test]
fn with_number_hash4() {
    assert_eq!(
        eval_posix("P.with_number(72)", &posix_st("P", "/out/file_####.exr")).to_display_string(),
        "/out/file_0072.exr"
    );
}
#[test]
fn with_number_hash6() {
    assert_eq!(
        eval_posix("P.with_number(72)", &posix_st("P", "/out/file_######.exr")).to_display_string(),
        "/out/file_000072.exr"
    );
}
#[test]
fn with_number_no_pattern() {
    assert_eq!(
        eval_posix("P.with_number(72)", &posix_st("P", "/out/render.exr")).to_display_string(),
        "/out/render_0072.exr"
    );
}
#[test]
fn with_number_multi_ext() {
    assert_eq!(
        eval_posix("P.with_number(72)", &posix_st("P", "/out/render.0001.exr")).to_display_string(),
        "/out/render.0072.exr"
    );
}
#[test]
fn with_number_negative() {
    assert_eq!(
        eval_posix("P.with_number(-1)", &posix_st("P", "/out/file_003.exr")).to_display_string(),
        "/out/file_-01.exr"
    );
}

// Property access on function results
#[test]
fn path_name_on_function_result() {
    assert_eq!(
        eval("path('/a/b/file.txt').name").to_display_string(),
        "file.txt"
    );
}
#[test]
fn path_stem_on_function_result() {
    assert_eq!(
        eval("path('/a/b/file.txt').stem").to_display_string(),
        "file"
    );
}
#[test]
fn path_parent_on_function_result() {
    assert_eq!(
        eval_with_fmt(
            "path('/a/b/file.txt').parent",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "/a/b"
    );
}

// Path / operator
#[test]
fn path_div_basic() {
    assert_eq!(
        eval_posix("P / 'child'", &posix_st("P", "/a/b")).to_display_string(),
        "/a/b/child"
    );
}
#[test]
fn path_div_absolute_replaces() {
    assert_eq!(
        eval_posix("P / '/new'", &posix_st("P", "/a/b")).to_display_string(),
        "/new"
    );
}

// === with_number edge cases ===
#[test]
fn with_number_shot_preserved_digits() {
    assert_eq!(
        eval_posix(
            "P.with_number(72)",
            &posix_st("P", "/renders/shot01_003.exr")
        )
        .to_display_string(),
        "/renders/shot01_072.exr"
    );
}
#[test]
fn with_number_shot_preserved_hash() {
    assert_eq!(
        eval_posix(
            "P.with_number(72)",
            &posix_st("P", "/renders/shot01_####.exr")
        )
        .to_display_string(),
        "/renders/shot01_0072.exr"
    );
}
#[test]
fn with_number_multiple_hash_uses_last() {
    assert_eq!(
        eval_posix(
            "P.with_number(72)",
            &posix_st("P", "/renders/##_shot_####.exr")
        )
        .to_display_string(),
        "/renders/##_shot_0072.exr"
    );
}
#[test]
fn with_number_multiple_printf_uses_last() {
    assert_eq!(
        eval_posix(
            "P.with_number(72)",
            &posix_st("P", "/renders/%02d_shot_%04d.exr")
        )
        .to_display_string(),
        "/renders/%02d_shot_0072.exr"
    );
}
#[test]
fn with_number_vfx_multi_ext() {
    assert_eq!(
        eval_posix(
            "P.with_number(72)",
            &posix_st("P", "/renders/render.0001.exr")
        )
        .to_display_string(),
        "/renders/render.0072.exr"
    );
}
#[test]
fn with_number_version_multi_ext() {
    assert_eq!(
        eval_posix(
            "P.with_number(72)",
            &posix_st("P", "/renders/file.v2.001.exr")
        )
        .to_display_string(),
        "/renders/file.v2.072.exr"
    );
}
#[test]
fn with_number_digits_as_extension() {
    assert_eq!(
        eval_posix(
            "P.with_number(72)",
            &posix_st("P", "/renders/file_0001.001")
        )
        .to_display_string(),
        "/renders/file_0072.001"
    );
}
#[test]
fn with_number_mixed_printf_hash_rightmost() {
    assert_eq!(
        eval_posix(
            "P.with_number(42)",
            &posix_st("P", "/renders/f_%d_abc_###.exr")
        )
        .to_display_string(),
        "/renders/f_%d_abc_042.exr"
    );
}
#[test]
fn with_number_mixed_printf_digits_rightmost() {
    assert_eq!(
        eval_posix(
            "P.with_number(72)",
            &posix_st("P", "/renders/file_%04d_001.exr")
        )
        .to_display_string(),
        "/renders/file_%04d_072.exr"
    );
}
#[test]
fn with_number_printf_padding_too_wide() {
    assert_err_posix(
        "P.with_number(1)",
        &posix_st("P", "/out/file_%099d.exr"),
        &[
            "with_number: padding width 99 exceeds maximum of 32\n",
            "  P.with_number(1)\n",
            "  ~~^~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn with_number_hash_padding_too_wide() {
    let st = posix_st("P", "/out/file_#####################################.exr");
    assert_err_posix("P.with_number(1)", &st, &["with_number: padding width"]);
}
#[test]
fn with_number_with_variable() {
    let mut st = SymbolTable::new();
    st.set(
        "P",
        ExprValue::new_path("/renders/shot_####.exr", PathFormat::Posix),
    )
    .unwrap();
    st.set("F", ExprValue::Int(42)).unwrap();
    assert_eq!(
        eval_posix("P.with_number(F)", &st).to_display_string(),
        "/renders/shot_0042.exr"
    );
}

// === is_relative_to edge cases ===
#[test]
fn posix_relative_to_basic() {
    assert_eq!(
        eval_posix("P.relative_to('/a')", &posix_st("P", "/a/b/c")).to_display_string(),
        "b/c"
    );
}
#[test]
fn posix_relative_to_nested() {
    assert_eq!(
        eval_posix("P.relative_to('/a/b')", &posix_st("P", "/a/b/c/d")).to_display_string(),
        "c/d"
    );
}
#[test]
fn uri_relative_to_basic() {
    assert_eq!(
        eval_posix(
            "P.relative_to('s3://bucket')",
            &posix_st("P", "s3://bucket/file.txt")
        )
        .to_display_string(),
        "file.txt"
    );
}
#[test]
fn uri_relative_to_nested() {
    assert_eq!(
        eval_posix(
            "P.relative_to('s3://bucket/a')",
            &posix_st("P", "s3://bucket/a/b/c")
        )
        .to_display_string(),
        "b/c"
    );
}
#[test]
fn uri_relative_to_same() {
    assert_eq!(
        eval_posix(
            "P.relative_to('s3://bucket/a')",
            &posix_st("P", "s3://bucket/a")
        )
        .to_display_string(),
        "."
    );
}
#[test]
fn uri_not_relative_to_error() {
    assert_err_posix(
        "P.relative_to('s3://other')",
        &posix_st("P", "s3://bucket/a"),
        &[
            "relative_to failed: 's3://bucket/a' is not relative to 's3://other'\n",
            "  P.relative_to('s3://other')\n",
            "  ~~^~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

// === empty path ===
#[test]
fn empty_path_name() {
    assert_eq!(
        eval_posix("P.name", &posix_st("P", "")).to_display_string(),
        ""
    );
}

// === filesystem vs URI ===
#[test]
fn filesystem_vs_uri() {
    let mut st = SymbolTable::new();
    st.set(
        "F",
        ExprValue::new_path("/local/file.txt", PathFormat::Posix),
    )
    .unwrap();
    st.set(
        "U",
        ExprValue::new_path("s3://bucket/file.txt", PathFormat::Posix),
    )
    .unwrap();
    assert_eq!(eval_posix("F.name", &st).to_display_string(), "file.txt");
    assert_eq!(eval_posix("U.name", &st).to_display_string(), "file.txt");
}

// === path from parts edge cases ===
#[test]
fn path_from_parts_roundtrip() {
    let st = posix_st("P", "/a/b/c.txt");
    let r = eval_posix("string(path(P.parts))", &st);
    assert_eq!(r.to_display_string(), "/a/b/c.txt");
}

// === Helper for Windows path format ===
fn eval_fmt(expr: &str, st: &SymbolTable, fmt: PathFormat) -> ExprValue {
    let parsed = openjd_expr::ParsedExpression::new(expr).unwrap();
    let symtabs = [st];
    parsed.with_path_format(fmt).evaluate(&symtabs).unwrap()
}
fn eval_fmt_fails(expr: &str, st: &SymbolTable, fmt: PathFormat) -> bool {
    let parsed = openjd_expr::ParsedExpression::new(expr).unwrap();
    let symtabs = [st];
    parsed.with_path_format(fmt).evaluate(&symtabs).is_err()
}

// === Exact Python name matches ===
#[test]
fn digit_sequence() {
    assert_eq!(
        eval_posix("P.with_number(72)", &posix_st("P", "/renders/shot_003.exr"))
            .to_display_string(),
        "/renders/shot_072.exr"
    );
}
#[test]
fn printf_d() {
    assert_eq!(
        eval_posix("P.with_number(72)", &posix_st("P", "/renders/shot_%d.exr")).to_display_string(),
        "/renders/shot_72.exr"
    );
}
#[test]
fn printf_04d() {
    assert_eq!(
        eval_posix(
            "P.with_number(72)",
            &posix_st("P", "/renders/shot_%04d.exr")
        )
        .to_display_string(),
        "/renders/shot_0072.exr"
    );
}
#[test]
fn hash_4() {
    assert_eq!(
        eval_posix(
            "P.with_number(72)",
            &posix_st("P", "/renders/shot_####.exr")
        )
        .to_display_string(),
        "/renders/shot_0072.exr"
    );
}
#[test]
fn hash_6() {
    assert_eq!(
        eval_posix(
            "P.with_number(72)",
            &posix_st("P", "/renders/shot_######.exr")
        )
        .to_display_string(),
        "/renders/shot_000072.exr"
    );
}
#[test]
fn no_pattern_appends_number() {
    assert_eq!(
        eval_posix("P.with_number(72)", &posix_st("P", "/renders/shot.exr")).to_display_string(),
        "/renders/shot_0072.exr"
    );
}
#[test]
fn multi_extension_vfx() {
    assert_eq!(
        eval_posix(
            "P.with_number(72)",
            &posix_st("P", "/renders/render.0001.exr")
        )
        .to_display_string(),
        "/renders/render.0072.exr"
    );
}
#[test]
fn multi_extension_version() {
    assert_eq!(
        eval_posix(
            "P.with_number(72)",
            &posix_st("P", "/renders/file.v2.003.exr")
        )
        .to_display_string(),
        "/renders/file.v2.072.exr"
    );
}
#[test]
fn digits_as_extension() {
    assert_eq!(
        eval_posix(
            "P.with_number(72)",
            &posix_st("P", "/renders/file_0001.001")
        )
        .to_display_string(),
        "/renders/file_0072.001"
    );
}
#[test]
fn shot_number_preserved_digit_sequence() {
    assert_eq!(
        eval_posix(
            "P.with_number(72)",
            &posix_st("P", "/renders/shot01_003.exr")
        )
        .to_display_string(),
        "/renders/shot01_072.exr"
    );
}
#[test]
fn shot_number_preserved_hash() {
    assert_eq!(
        eval_posix(
            "P.with_number(72)",
            &posix_st("P", "/renders/shot01_####.exr")
        )
        .to_display_string(),
        "/renders/shot01_0072.exr"
    );
}
#[test]
fn multiple_hash_patterns_uses_last() {
    assert_eq!(
        eval_posix(
            "P.with_number(72)",
            &posix_st("P", "/renders/##_shot_####.exr")
        )
        .to_display_string(),
        "/renders/##_shot_0072.exr"
    );
}
#[test]
fn multiple_printf_uses_last() {
    assert_eq!(
        eval_posix(
            "P.with_number(72)",
            &posix_st("P", "/renders/%02d_shot_%04d.exr")
        )
        .to_display_string(),
        "/renders/%02d_shot_0072.exr"
    );
}
#[test]
fn mixed_printf_and_hash_rightmost_wins() {
    assert_eq!(
        eval_posix(
            "P.with_number(42)",
            &posix_st("P", "/renders/f_%d_abc_###.exr")
        )
        .to_display_string(),
        "/renders/f_%d_abc_042.exr"
    );
}
#[test]
fn mixed_printf_and_digits_rightmost_wins() {
    assert_eq!(
        eval_posix(
            "P.with_number(72)",
            &posix_st("P", "/renders/file_%04d_003.exr")
        )
        .to_display_string(),
        "/renders/file_%04d_072.exr"
    );
}
#[test]
fn printf_padding_too_wide() {
    assert_err_posix(
        "path('/out/file_%099d.exr').with_number(1)",
        &SymbolTable::new(),
        &[
            "with_number: padding width 99 exceeds maximum of 32\n",
            "  path('/out/file_%099d.exr').with_number(1)\n",
            "  ~~~~~~~~~~~~~~~~~~~~~~~~~~~~^~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn hash_padding_too_wide() {
    let hashes = "#".repeat(33);
    let expr = format!("path('/out/file_{hashes}.exr').with_number(1)");
    assert_err_posix(
        &expr,
        &SymbolTable::new(),
        &[
            "with_number: padding width 33 exceeds maximum of 32\n",
            &format!("  path('/out/file_{hashes}.exr').with_number(1)\n"),
            "  ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~^~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn with_variable() {
    let mut st = SymbolTable::new();
    st.set(
        "P",
        ExprValue::new_path("/renders/shot_####.exr", PathFormat::Posix),
    )
    .unwrap();
    st.set("Frame", ExprValue::Int(42)).unwrap();
    assert_eq!(
        eval_posix("P.with_number(Frame)", &st).to_display_string(),
        "/renders/shot_0042.exr"
    );
}

// === is_relative_to with exact Python names ===
#[test]
fn posix_true() {
    assert_eq!(
        eval("path('/a/b/c').is_relative_to(path('/a/b'))").to_display_string(),
        "true"
    );
}
#[test]
fn posix_false() {
    assert_eq!(
        eval("path('/a/b/c').is_relative_to(path('/x/y'))").to_display_string(),
        "false"
    );
}
#[test]
fn uri_true() {
    assert_eq!(
        eval("path('s3://bucket/key/file').is_relative_to(path('s3://bucket/key'))")
            .to_display_string(),
        "true"
    );
}
#[test]
fn uri_false_different_bucket() {
    assert_eq!(
        eval("path('s3://bucket1/key').is_relative_to(path('s3://bucket2/key'))")
            .to_display_string(),
        "false"
    );
}
#[test]
fn uri_same_relative() {
    assert_eq!(
        eval("path('s3://bucket/key').relative_to(path('s3://bucket/key'))").to_display_string(),
        "."
    );
}
#[test]
fn uri_vs_filesystem() {
    assert_eq!(
        eval("path('s3://bucket/key').is_relative_to(path('/a/b'))").to_display_string(),
        "false"
    );
}

// === relative_to with exact Python names ===
#[test]
fn posix_basic() {
    assert_eq!(
        eval("path('/a/b/c').relative_to(path('/a/b'))").to_display_string(),
        "c"
    );
}
#[test]
fn posix_nested() {
    assert_eq!(
        eval_with_fmt(
            "path('/a/b/c/d').relative_to(path('/a'))",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "b/c/d"
    );
}
#[test]
fn posix_same_path() {
    assert_eq!(
        eval("path('/a/b').relative_to(path('/a/b'))").to_display_string(),
        "."
    );
}
#[test]
fn posix_not_relative() {
    assert_err_posix(
        "path('/a/b').relative_to(path('/x/y'))",
        &SymbolTable::new(),
        &[
            "relative_to failed: '/a/b' is not relative to '/x/y'\n",
            "  path('/a/b').relative_to(path('/x/y'))\n",
            "  ~~~~~~~~~~~~~^~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn uri_basic() {
    assert_eq!(
        eval("path('s3://bucket/key/file.txt').relative_to(path('s3://bucket/key'))")
            .to_display_string(),
        "file.txt"
    );
}
#[test]
fn uri_nested() {
    assert_eq!(
        eval_with_fmt(
            "path('s3://bucket/a/b/c').relative_to(path('s3://bucket'))",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "a/b/c"
    );
}
#[test]
fn uri_not_relative() {
    assert_err(
        "path('s3://bucket1/key').relative_to(path('s3://bucket2'))",
        &[
            "relative_to failed: 's3://bucket1/key' is not relative to 's3://bucket2'\n",
            "  path('s3://bucket1/key').relative_to(path('s3://bucket2'))\n",
            "  ~~~~~~~~~~~~~~~~~~~~~~~~~^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn uri_vs_filesystem_error() {
    assert_err_posix(
        "path('s3://bucket/key').relative_to(path('/a/b'))",
        &SymbolTable::new(),
        &[
            "relative_to failed: 's3://bucket/key' is not relative to '/a/b'\n",
            "  path('s3://bucket/key').relative_to(path('/a/b'))\n",
            "  ~~~~~~~~~~~~~~~~~~~~~~~~^~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn filesystem_vs_uri_error() {
    assert_err_posix(
        "path('/a/b').relative_to(path('s3://bucket'))",
        &SymbolTable::new(),
        &[
            "relative_to failed: '/a/b' is not relative to 's3://bucket'\n",
            "  path('/a/b').relative_to(path('s3://bucket'))\n",
            "  ~~~~~~~~~~~~~^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

// === is_absolute ===
#[test]
fn posix_relative() {
    assert_eq!(
        eval("path('a/b').is_absolute()").to_display_string(),
        "false"
    );
}
#[test]
fn empty_path() {
    assert_eq!(eval("path('').is_absolute()").to_display_string(), "false");
}

// === Windows paths ===
// === Windows paths ===
#[test]
fn windows_absolute() {
    assert_eq!(
        eval_fmt(
            "path('C:\\\\a\\\\b').is_absolute()",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        "true"
    );
}
#[test]
fn windows_relative() {
    assert_eq!(
        eval_fmt(
            "path('a/b').is_absolute()",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        "false"
    );
}
#[test]
fn windows_drive_on_posix_not_absolute() {
    assert_eq!(
        eval_with_fmt(
            "path('C:/a/b').is_absolute()",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "false"
    );
}

// === UNC paths ===
#[test]
fn unc_absolute() {
    assert_eq!(
        eval_fmt(
            "path('//server/share/dir').is_absolute()",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        "true"
    );
}
#[test]
fn unc_absolute_posix() {
    assert_eq!(
        eval("path('//server/share/dir').is_absolute()").to_display_string(),
        "true"
    );
}
#[test]
fn unc_true() {
    assert_eq!(
        eval_fmt(
            "path('//server/share/dir/file').is_relative_to(path('//server/share'))",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        "true"
    );
}
#[test]
fn unc_false() {
    assert_eq!(
        eval_fmt(
            "path('//server/share/dir').is_relative_to(path('//other/share'))",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        "false"
    );
}
#[test]
fn unc_not_relative() {
    assert!(eval_fmt_fails(
        "path('//server/share/dir').relative_to(path('//other/share'))",
        &SymbolTable::new(),
        PathFormat::Windows
    ));
}

// === path from parts edge cases ===
#[test]
fn path_from_parts_skip_root() {
    assert_eq!(
        eval_with_fmt(
            "path(path('/a/b/c').parts[1:])",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "a/b/c"
    );
}
#[test]
fn path_from_parts_last_two() {
    assert_eq!(
        eval_with_fmt(
            "path(path('/a/b/c/d').parts[-2:])",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "c/d"
    );
}
#[test]
fn path_from_parts_reverse() {
    assert_eq!(
        eval_with_fmt(
            "path(path('a/b/c').parts[::-1])",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "c/b/a"
    );
}
#[test]
fn path_from_sliced_parts() {
    assert_eq!(
        eval_posix("path(P.parts[:3])", &posix_st("P", "/a/b/c/d")).to_display_string(),
        "/a/b"
    );
}

// === chained/repeated ===
#[test]
fn chained_property_access() {
    assert_eq!(
        eval("path('/a/b/c.txt').parent.name").to_display_string(),
        "b"
    );
}
#[test]
fn repeated_parent_access() {
    let st = posix_st("P", "/a/b/c/d/file.txt");
    assert_eq!(eval_posix("P.parent", &st).to_display_string(), "/a/b/c/d");
    assert_eq!(
        eval_posix("P.parent.parent", &st).to_display_string(),
        "/a/b/c"
    );
    assert_eq!(
        eval_posix("P.parent.parent.parent", &st).to_display_string(),
        "/a/b"
    );
}

// === with_suffix function form ===
#[test]
fn with_suffix_function() {
    assert!(eval_posix(
        "with_suffix(P, '.png')",
        &posix_st("P", "/output/render.exr")
    )
    .to_display_string()
    .ends_with("render.png"));
}

// === path_stem/suffix multi extension ===
#[test]
fn path_stem_multi_extension() {
    assert_eq!(
        eval_posix("P.stem", &posix_st("P", "/data/archive.tar.gz")).to_display_string(),
        "archive.tar"
    );
}
#[test]
fn path_suffix_multi_extension() {
    assert_eq!(
        eval_posix("P.suffix", &posix_st("P", "/data/archive.tar.gz")).to_display_string(),
        ".gz"
    );
}

// === UNC basic (relative_to with Windows format) ===
#[test]
fn unc_basic() {
    let r = eval_fmt(
        "path('//server/share/dir/file').relative_to(path('//server/share'))",
        &SymbolTable::new(),
        PathFormat::Windows,
    );
    // On Windows format, separator is backslash
    assert!(r.to_display_string() == "dir\\file" || r.to_display_string() == "dir/file");
}

// === Missing Python tests ===

// is_relative_to: same path
#[test]
fn is_relative_to_posix_same_path() {
    assert_eq!(
        eval("path('/a/b').is_relative_to(path('/a/b'))").to_display_string(),
        "true"
    );
}
#[test]
fn is_relative_to_uri_same() {
    assert_eq!(
        eval("path('s3://bucket/key').is_relative_to(path('s3://bucket/key'))").to_display_string(),
        "true"
    );
}

// is_relative_to: filesystem vs URI
#[test]
fn is_relative_to_filesystem_vs_uri() {
    assert_eq!(
        eval("path('/a/b').is_relative_to(path('s3://bucket'))").to_display_string(),
        "false"
    );
}

// unc_not_relative with error message assertion
fn eval_fmt_err(expr: &str, st: &SymbolTable, fmt: PathFormat) -> String {
    let parsed = openjd_expr::ParsedExpression::new(expr).unwrap();
    let symtabs = [st];
    parsed
        .with_path_format(fmt)
        .evaluate(&symtabs)
        .unwrap_err()
        .to_string()
}

#[test]
fn unc_not_relative_error_message() {
    let e = eval_fmt_err(
        "path('//server/share/dir').relative_to(path('//other/share'))",
        &SymbolTable::new(),
        PathFormat::Windows,
    );
    assert!(e.contains("relative_to failed:"), "got:\n{e}");
    assert!(e.contains("is not relative to"), "got:\n{e}");
    assert!(
        e.contains("path('//server/share/dir').relative_to(path('//other/share'))"),
        "got:\n{e}"
    );
}

// digits_as_extension: file.001 (no digit pattern in stem)
#[test]
fn digits_as_extension_no_stem_digits() {
    assert_eq!(
        eval_posix("P.with_number(72)", &posix_st("P", "/renders/file.001")).to_display_string(),
        "/renders/file_0072.001"
    );
}

// path from list with relative parts (no root)
#[test]
fn path_from_list_relative() {
    assert_eq!(
        eval_with_fmt(
            "path(['a', 'b', 'c'])",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "a/b/c"
    );
}

// === Bug 5: is_relative_to path component boundary ===
#[test]
fn is_relative_to_component_boundary() {
    // '/foo/bar' is NOT relative to '/foo/b' — must check component boundaries
    assert_eq!(
        eval_posix("P.is_relative_to('/foo/b')", &posix_st("P", "/foo/bar")).to_display_string(),
        "false"
    );
}

// === Bug 6: relative_to path component boundary ===
#[test]
fn relative_to_component_boundary_error() {
    // '/foo/bar'.relative_to('/foo/b') should error, not return "ar"
    assert_err_posix(
        "P.relative_to('/foo/b')",
        &posix_st("P", "/foo/bar"),
        &["relative_to failed"],
    );
}

// ══════════════════════════════════════════════════════════════
// path::join — unit tests for the format-aware join function
// ══════════════════════════════════════════════════════════════

use openjd_expr::functions::path::join as path_join;

// --- POSIX format ---

#[test]
fn join_posix_basic() {
    assert_eq!(path_join("/a/b", "c", PathFormat::Posix), "/a/b/c");
}

#[test]
fn join_posix_trailing_slash_stripped() {
    assert_eq!(path_join("/a/b/", "c", PathFormat::Posix), "/a/b/c");
}

#[test]
fn join_posix_absolute_right_replaces() {
    assert_eq!(path_join("/a/b", "/new", PathFormat::Posix), "/new");
}

#[test]
fn join_posix_backslash_in_dirname_preserved() {
    // On POSIX, backslash is a valid filename character, not a separator
    assert_eq!(
        path_join("/a/dir\\name", "file", PathFormat::Posix),
        "/a/dir\\name/file"
    );
}

#[test]
fn join_posix_trailing_backslash_not_stripped() {
    // Trailing \ is part of the dirname on POSIX, not a separator
    assert_eq!(path_join("/a/b\\", "c", PathFormat::Posix), "/a/b\\/c");
}

#[test]
fn join_posix_empty_right() {
    assert_eq!(path_join("/a/b", "", PathFormat::Posix), "/a/b/");
}

#[test]
fn join_posix_root() {
    assert_eq!(path_join("/", "a", PathFormat::Posix), "/a");
}

// --- Windows format ---

#[test]
fn join_windows_basic() {
    assert_eq!(
        path_join("C:\\a\\b", "c", PathFormat::Windows),
        "C:\\a\\b\\c"
    );
}

#[test]
fn join_windows_trailing_backslash_stripped() {
    assert_eq!(
        path_join("C:\\a\\b\\", "c", PathFormat::Windows),
        "C:\\a\\b\\c"
    );
}

#[test]
fn join_windows_trailing_slash_stripped() {
    // Forward slashes are also separators on Windows
    assert_eq!(
        path_join("C:\\a\\b/", "c", PathFormat::Windows),
        "C:\\a\\b\\c"
    );
}

#[test]
fn join_windows_absolute_right_replaces() {
    assert_eq!(
        path_join("C:\\a\\b", "D:\\new", PathFormat::Windows),
        "D:\\new"
    );
}

#[test]
fn join_windows_unc_right_replaces() {
    assert_eq!(
        path_join("C:\\a\\b", "\\\\server\\share", PathFormat::Windows),
        "\\\\server\\share"
    );
}

#[test]
fn join_windows_drive_root() {
    assert_eq!(path_join("C:\\", "a", PathFormat::Windows), "C:\\a");
}

// --- URI left ---

#[test]
fn join_uri_left_uses_forward_slash() {
    assert_eq!(
        path_join("s3://bucket/prefix", "file.obj", PathFormat::Windows),
        "s3://bucket/prefix/file.obj"
    );
}

#[test]
fn join_uri_left_trailing_slash_stripped() {
    assert_eq!(
        path_join("s3://bucket/prefix/", "file.obj", PathFormat::Posix),
        "s3://bucket/prefix/file.obj"
    );
}

#[test]
fn join_uri_left_normalizes_backslashes_in_right() {
    // When left is a URI, backslashes in right are converted to forward slashes
    assert_eq!(
        path_join(
            "s3://bucket/prefix",
            "sub\\dir\\file.obj",
            PathFormat::Windows
        ),
        "s3://bucket/prefix/sub/dir/file.obj"
    );
}

#[test]
fn join_uri_left_normalizes_backslashes_posix_format() {
    // In POSIX context, backslashes are valid filename chars — NOT converted
    assert_eq!(
        path_join("s3://bucket", "a\\b\\c", PathFormat::Posix),
        "s3://bucket/a\\b\\c"
    );
}

// --- Absolute right (URI) ---

#[test]
fn join_uri_right_replaces_posix() {
    assert_eq!(
        path_join("/local/path", "s3://bucket/key", PathFormat::Posix),
        "s3://bucket/key"
    );
}

#[test]
fn join_uri_right_replaces_windows() {
    assert_eq!(
        path_join(
            "C:\\local",
            "https://cdn.example.com/file",
            PathFormat::Windows
        ),
        "https://cdn.example.com/file"
    );
}

// --- Cross-format edge cases ---

#[test]
fn join_posix_windows_path_as_relative() {
    // C:\foo is not absolute under POSIX — treated as a relative component
    assert_eq!(
        path_join("/base", "C:\\foo", PathFormat::Posix),
        "/base/C:\\foo"
    );
}

#[test]
fn join_windows_posix_path_as_relative() {
    // /foo on Windows is root-relative: keeps drive from left, replaces path
    // Matches ntpath.join('C:\\base', '/foo') → 'C:/foo'
    assert_eq!(path_join("C:\\base", "/foo", PathFormat::Windows), "C:/foo");
}

#[test]
fn join_windows_backslash_root_relative() {
    // \foo on Windows is also root-relative
    assert_eq!(
        path_join("C:\\base", "\\foo", PathFormat::Windows),
        "C:\\foo"
    );
}

#[test]
fn join_windows_root_relative_unc_backslash() {
    // UNC root \\server\share + root-relative /foo → keeps UNC root
    assert_eq!(
        path_join("\\\\server\\share\\deep\\path", "/foo", PathFormat::Windows),
        "\\\\server\\share/foo"
    );
}

#[test]
fn join_windows_root_relative_unc_backslash_bslash_right() {
    assert_eq!(
        path_join(
            "\\\\server\\share\\deep\\path",
            "\\foo",
            PathFormat::Windows
        ),
        "\\\\server\\share\\foo"
    );
}

#[test]
fn join_windows_root_relative_unc_forward_slash() {
    // UNC root //server/share + root-relative /foo → keeps UNC root
    assert_eq!(
        path_join("//server/share/deep/path", "/foo", PathFormat::Windows),
        "//server/share/foo"
    );
}

#[test]
fn join_windows_root_relative_unc_forward_slash_bslash_right() {
    assert_eq!(
        path_join("//server/share/deep/path", "\\foo", PathFormat::Windows),
        "//server/share\\foo"
    );
}

#[test]
fn join_windows_unc_root_only() {
    // UNC root with no deeper path
    assert_eq!(
        path_join("\\\\server\\share", "/foo", PathFormat::Windows),
        "\\\\server\\share/foo"
    );
}

#[test]
fn join_windows_unc_normal_relative() {
    // Normal relative append to UNC
    assert_eq!(
        path_join("\\\\server\\share", "relative", PathFormat::Windows),
        "\\\\server\\share\\relative"
    );
}

#[test]
fn join_windows_unc_forward_normal_relative() {
    assert_eq!(
        path_join("//server/share", "relative", PathFormat::Windows),
        "//server/share\\relative"
    );
}

// ── Cross-format tests ──
// Forward-slash paths must produce the same results for path properties
// regardless of whether the evaluator uses Posix or Windows format.
// This catches bugs where paths like "/input/scene.exr" work on Posix
// but break on Windows (e.g. the session scenario tests).

macro_rules! cross_format_test {
    ($name:ident, $expr:expr, $path:expr, $expected:expr) => {
        mod $name {
            use super::*;

            #[test]
            fn posix() {
                assert_eq!(
                    eval_posix($expr, &posix_st("P", $path)).to_display_string(),
                    $expected,
                    "Posix format failed for {} on {}",
                    $expr,
                    $path
                );
            }

            #[test]
            fn windows() {
                assert_eq!(
                    eval_windows($expr, &windows_st("P", $path)).to_display_string(),
                    $expected,
                    "Windows format failed for {} on {}",
                    $expr,
                    $path
                );
            }
        }
    };
}

cross_format_test!(cross_stem_basic, "P.stem", "/a/b/file.txt", "file");
cross_format_test!(
    cross_stem_multi_ext,
    "P.stem",
    "/a/b/file.tar.gz",
    "file.tar"
);
cross_format_test!(cross_stem_no_ext, "P.stem", "/a/b/Makefile", "Makefile");
cross_format_test!(cross_stem_hidden, "P.stem", "/a/b/.hidden", ".hidden");
cross_format_test!(cross_stem_deep, "P.stem", "/input/scene.exr", "scene");

cross_format_test!(cross_suffix_basic, "P.suffix", "/a/b/file.txt", ".txt");
cross_format_test!(cross_suffix_multi, "P.suffix", "/a/b/file.tar.gz", ".gz");
cross_format_test!(cross_suffix_none, "P.suffix", "/a/b/Makefile", "");

cross_format_test!(cross_name_basic, "P.name", "/a/b/file.txt", "file.txt");
cross_format_test!(cross_name_root_file, "P.name", "/file.txt", "file.txt");
cross_format_test!(cross_name_deep, "P.name", "/a/b/c/d.exr", "d.exr");

// parent normalizes separators, so it legitimately differs between formats.
// Tested separately below.

cross_format_test!(
    cross_stem_on_function,
    "path('/a/b/file.txt').stem",
    "/unused",
    "file"
);

// =============================================================================
// Bug 5.6: path(list[string]) must match PurePosixPath / PureWindowsPath
// =============================================================================

// ── Posix simple ──

#[test]
fn path_list_posix_simple_two() {
    assert_eq!(
        eval_with_fmt("path(['a', 'b'])", &SymbolTable::new(), PathFormat::Posix)
            .to_display_string(),
        "a/b"
    );
}

#[test]
fn path_list_posix_simple_three() {
    assert_eq!(
        eval_with_fmt(
            "path(['a', 'b', 'c'])",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "a/b/c"
    );
}

#[test]
fn path_list_posix_abs_then_rel() {
    assert_eq!(
        eval_with_fmt("path(['/a', 'b'])", &SymbolTable::new(), PathFormat::Posix)
            .to_display_string(),
        "/a/b"
    );
}

#[test]
fn path_list_posix_root_then_parts() {
    assert_eq!(
        eval_with_fmt(
            "path(['/', 'a', 'b', 'c'])",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "/a/b/c"
    );
}

// ── Posix absolute resets ──

#[test]
fn path_list_posix_abs_reset() {
    // PurePosixPath('a', '/b') == '/b'
    assert_eq!(
        eval_with_fmt("path(['a', '/b'])", &SymbolTable::new(), PathFormat::Posix)
            .to_display_string(),
        "/b"
    );
}

#[test]
fn path_list_posix_abs_reset_mid() {
    // PurePosixPath('a', 'b', '/c', 'd') == '/c/d'
    assert_eq!(
        eval_with_fmt(
            "path(['a', 'b', '/c', 'd'])",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "/c/d"
    );
}

#[test]
fn path_list_posix_abs_reset_both() {
    // PurePosixPath('/a', '/b') == '/b'
    assert_eq!(
        eval_with_fmt("path(['/a', '/b'])", &SymbolTable::new(), PathFormat::Posix)
            .to_display_string(),
        "/b"
    );
}

#[test]
fn path_list_posix_abs_reset_chain() {
    // PurePosixPath('a', 'b', '/c', 'd', '/e') == '/e'
    assert_eq!(
        eval_with_fmt(
            "path(['a', 'b', '/c', 'd', '/e'])",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "/e"
    );
}

// ── Posix edge cases ──

#[test]
fn path_list_posix_empty_string() {
    // PurePosixPath('') == '.'
    assert_eq!(
        eval_with_fmt("path([''])", &SymbolTable::new(), PathFormat::Posix).to_display_string(),
        "."
    );
}

#[test]
fn path_list_posix_a_then_empty() {
    // PurePosixPath('a', '') == 'a'
    assert_eq!(
        eval_with_fmt("path(['a', ''])", &SymbolTable::new(), PathFormat::Posix)
            .to_display_string(),
        "a"
    );
}

#[test]
fn path_list_posix_empty_then_a() {
    // PurePosixPath('', 'a') == 'a'
    assert_eq!(
        eval_with_fmt("path(['', 'a'])", &SymbolTable::new(), PathFormat::Posix)
            .to_display_string(),
        "a"
    );
}

#[test]
fn path_list_posix_trailing_slash() {
    // PurePosixPath('a/', 'b') == 'a/b'
    assert_eq!(
        eval_with_fmt("path(['a/', 'b'])", &SymbolTable::new(), PathFormat::Posix)
            .to_display_string(),
        "a/b"
    );
}

#[test]
fn path_list_posix_dot_then_a() {
    // PurePosixPath('.', 'a') == 'a'
    assert_eq!(
        eval_with_fmt("path(['.', 'a'])", &SymbolTable::new(), PathFormat::Posix)
            .to_display_string(),
        "a"
    );
}

#[test]
fn path_list_posix_dotdot_then_a() {
    // PurePosixPath('..', 'a') == '../a'
    assert_eq!(
        eval_with_fmt("path(['..', 'a'])", &SymbolTable::new(), PathFormat::Posix)
            .to_display_string(),
        "../a"
    );
}

#[test]
fn path_list_posix_a_dot_b() {
    // PurePosixPath('a', '.', 'b') == 'a/b'
    assert_eq!(
        eval_with_fmt(
            "path(['a', '.', 'b'])",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "a/b"
    );
}

#[test]
fn path_list_posix_a_dotdot_b() {
    // PurePosixPath('a', '..', 'b') == 'a/../b'
    assert_eq!(
        eval_with_fmt(
            "path(['a', '..', 'b'])",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "a/../b"
    );
}

#[test]
fn path_list_posix_empty_empty() {
    // PurePosixPath('', '') == '.'
    assert_eq!(
        eval_with_fmt("path(['', ''])", &SymbolTable::new(), PathFormat::Posix).to_display_string(),
        "."
    );
}

#[test]
fn path_list_posix_root_then_empty() {
    // PurePosixPath('/', '') == '/'
    assert_eq!(
        eval_with_fmt("path(['/', ''])", &SymbolTable::new(), PathFormat::Posix)
            .to_display_string(),
        "/"
    );
}

// ── Windows simple ──

#[test]
fn path_list_windows_simple_two() {
    assert_eq!(
        eval_with_fmt("path(['a', 'b'])", &SymbolTable::new(), PathFormat::Windows)
            .to_display_string(),
        r"a\b"
    );
}

#[test]
fn path_list_windows_drive_then_rel() {
    assert_eq!(
        eval_with_fmt(
            "path(['C:\\\\a', 'b'])",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        r"C:\a\b"
    );
}

// ── Windows drive/root resets ──

#[test]
fn path_list_windows_abs_drive_reset() {
    // PureWindowsPath('a', 'C:\\b') == 'C:\\b'
    assert_eq!(
        eval_with_fmt(
            "path(['a', 'C:\\\\b'])",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        r"C:\b"
    );
}

#[test]
fn path_list_windows_diff_drive_reset() {
    // PureWindowsPath('C:\\a', 'D:\\b') == 'D:\\b'
    assert_eq!(
        eval_with_fmt(
            "path(['C:\\\\a', 'D:\\\\b'])",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        r"D:\b"
    );
}

#[test]
fn path_list_windows_root_keeps_drive() {
    // PureWindowsPath('C:\\a', '\\b') == 'C:\\b'
    assert_eq!(
        eval_with_fmt(
            "path(['C:\\\\a', '\\\\b'])",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        r"C:\b"
    );
}

#[test]
fn path_list_windows_fwd_slash_root_keeps_drive() {
    // PureWindowsPath('C:\\a', '/b') == 'C:\\b'
    assert_eq!(
        eval_with_fmt(
            "path(['C:\\\\a', '/b'])",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        r"C:\b"
    );
}

#[test]
fn path_list_windows_drive_relative() {
    // PureWindowsPath('C:', 'a') == 'C:a'
    assert_eq!(
        eval_with_fmt(
            "path(['C:', 'a'])",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        "C:a"
    );
}

#[test]
fn path_list_windows_drive_root_then_rel() {
    // PureWindowsPath('C:/', 'a') == 'C:\\a'
    assert_eq!(
        eval_with_fmt(
            "path(['C:/', 'a'])",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        r"C:\a"
    );
}

#[test]
fn path_list_windows_diff_drive_relative_reset() {
    // PureWindowsPath('C:\\a', 'D:b') == 'D:b'
    assert_eq!(
        eval_with_fmt(
            "path(['C:\\\\a', 'D:b'])",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        "D:b"
    );
}

#[test]
fn path_list_windows_same_drive_no_root_appends() {
    // PureWindowsPath('C:\\a', 'C:b') == 'C:\\a\\b'
    assert_eq!(
        eval_with_fmt(
            "path(['C:\\\\a', 'C:b'])",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        r"C:\a\b"
    );
}

#[test]
fn path_list_windows_same_drive_empty_noop() {
    // PureWindowsPath('C:\\a', 'C:') == 'C:\\a'
    assert_eq!(
        eval_with_fmt(
            "path(['C:\\\\a', 'C:'])",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        r"C:\a"
    );
}

// ── Windows UNC ──

#[test]
fn path_list_windows_unc_then_rel() {
    assert_eq!(
        eval_with_fmt(
            "path(['\\\\\\\\server\\\\share', 'a'])",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        r"\\server\share\a"
    );
}

#[test]
fn path_list_windows_unc_root_reset() {
    // PureWindowsPath('\\\\server\\share\\a', '\\b') == '\\\\server\\share\\b'
    assert_eq!(
        eval_with_fmt(
            "path(['\\\\\\\\server\\\\share\\\\a', '\\\\b'])",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        r"\\server\share\b"
    );
}

// ── Windows edge cases ──

#[test]
fn path_list_windows_empty_string() {
    assert_eq!(
        eval_with_fmt("path([''])", &SymbolTable::new(), PathFormat::Windows).to_display_string(),
        "."
    );
}

#[test]
fn path_list_windows_dot_then_a() {
    assert_eq!(
        eval_with_fmt("path(['.', 'a'])", &SymbolTable::new(), PathFormat::Windows)
            .to_display_string(),
        "a"
    );
}

#[test]
fn path_list_windows_a_dot_b() {
    assert_eq!(
        eval_with_fmt(
            "path(['a', '.', 'b'])",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        r"a\b"
    );
}

#[test]
fn path_list_windows_a_dotdot_b() {
    assert_eq!(
        eval_with_fmt(
            "path(['a', '..', 'b'])",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        r"a\..\b"
    );
}

#[test]
fn path_list_windows_no_drive_root_reset() {
    // PureWindowsPath('a', '\\b') == '\\b'
    assert_eq!(
        eval_with_fmt(
            "path(['a', '\\\\b'])",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        r"\b"
    );
}

// === Bug P-4: Windows case-insensitive is_relative_to / relative_to ===

#[test]
fn windows_is_relative_to_case_insensitive() {
    let st = windows_st("P", "C:\\Users\\Bob");
    assert_eq!(
        eval_windows("P.is_relative_to('c:\\\\users')", &st).to_display_string(),
        "true"
    );
}

#[test]
fn windows_relative_to_case_insensitive() {
    // Should match case-insensitively but preserve original case in result
    let st = windows_st("P", "C:\\Users\\Bob");
    assert_eq!(
        eval_windows("P.relative_to('c:\\\\users')", &st).to_display_string(),
        "Bob"
    );
}

#[test]
fn windows_is_relative_to_case_sensitive_posix() {
    // POSIX must remain case-sensitive
    let st = posix_st("P", "/A/B");
    assert_eq!(
        eval_posix("P.is_relative_to('/a')", &st).to_display_string(),
        "false"
    );
}

#[test]
fn windows_relative_to_case_sensitive_posix() {
    // POSIX relative_to must remain case-sensitive — should error
    assert_err_posix(
        "P.relative_to('/a')",
        &posix_st("P", "/A/B"),
        &["relative_to failed"],
    );
}

// === is_relative_to / relative_to partial component boundary ===

#[test]
fn posix_is_relative_to_partial_component_false() {
    // "/mnt/data" does NOT start with "/mnt/dat" — must match at component boundary
    let st = posix_st("P", "/mnt/data");
    assert_eq!(
        eval_posix("P.is_relative_to('/mnt/dat')", &st).to_display_string(),
        "false"
    );
}

#[test]
fn posix_is_relative_to_full_component_true() {
    let st = posix_st("P", "/mnt/data");
    assert_eq!(
        eval_posix("P.is_relative_to('/mnt')", &st).to_display_string(),
        "true"
    );
}

#[test]
fn windows_is_relative_to_partial_component_false() {
    let st = windows_st("P", "C:\\Users\\Bob");
    assert_eq!(
        eval_windows("P.is_relative_to('C:\\\\Users\\\\Bo')", &st).to_display_string(),
        "false"
    );
}

#[test]
fn windows_is_relative_to_partial_component_case_insensitive_false() {
    // Case-insensitive match but still partial component — must be false
    let st = windows_st("P", "C:\\Users\\Bob");
    assert_eq!(
        eval_windows("P.is_relative_to('c:\\\\users\\\\bo')", &st).to_display_string(),
        "false"
    );
}

// ── Empty-name guard: filesystem root paths ──────────────────────
//
// `with_name`, `with_stem`, `with_suffix`, and `with_number` raise
// `ExpressionError` when the input has no final component to operate
// on. Without this guard the operators silently invent a filename
// against an empty stem and emit nonsensical results
// (e.g., `path("/").with_name("x")` → `"//x"`). Matches Python
// pathlib's behaviour:
// `PurePosixPath('/').with_name('x')` raises
// `ValueError: PurePosixPath('/') has an empty name`.
//
// The error message format is
// `with_<op>: '<path>' has an empty name` so the path that was
// rejected appears verbatim in the diagnostic.

fn assert_empty_name_err(expr: &str, st: &SymbolTable, fmt: PathFormat, op: &str, path: &str) {
    let parsed = openjd_expr::ParsedExpression::new(expr).unwrap();
    let symtabs = [st];
    let e = parsed
        .with_path_format(fmt)
        .evaluate(&symtabs)
        .unwrap_err()
        .to_string();
    let needle = format!("{op}: '{path}' has an empty name");
    assert!(
        e.contains(&needle),
        "expected substring {needle:?} in:\n{e}"
    );
}

#[test]
fn with_name_on_posix_root_errors() {
    assert_empty_name_err(
        "P.with_name('x.png')",
        &posix_st("P", "/"),
        PathFormat::Posix,
        "with_name",
        "/",
    );
}

#[test]
fn with_stem_on_posix_root_errors() {
    assert_empty_name_err(
        "P.with_stem('final')",
        &posix_st("P", "/"),
        PathFormat::Posix,
        "with_stem",
        "/",
    );
}

#[test]
fn with_suffix_on_posix_root_errors() {
    assert_empty_name_err(
        "P.with_suffix('.png')",
        &posix_st("P", "/"),
        PathFormat::Posix,
        "with_suffix",
        "/",
    );
}

#[test]
fn with_number_on_posix_root_errors() {
    assert_empty_name_err(
        "P.with_number(42)",
        &posix_st("P", "/"),
        PathFormat::Posix,
        "with_number",
        "/",
    );
}

#[test]
fn with_name_on_windows_drive_root_errors() {
    assert_empty_name_err(
        "P.with_name('x.png')",
        &windows_st("P", "C:\\"),
        PathFormat::Windows,
        "with_name",
        "C:\\",
    );
}

#[test]
fn with_stem_on_windows_drive_root_errors() {
    assert_empty_name_err(
        "P.with_stem('final')",
        &windows_st("P", "C:\\"),
        PathFormat::Windows,
        "with_stem",
        "C:\\",
    );
}

#[test]
fn with_suffix_on_windows_drive_root_errors() {
    assert_empty_name_err(
        "P.with_suffix('.png')",
        &windows_st("P", "C:\\"),
        PathFormat::Windows,
        "with_suffix",
        "C:\\",
    );
}

#[test]
fn with_number_on_windows_drive_root_errors() {
    assert_empty_name_err(
        "P.with_number(42)",
        &windows_st("P", "C:\\"),
        PathFormat::Windows,
        "with_number",
        "C:\\",
    );
}

// Sanity: a normal path passes through without raising.
#[test]
fn with_name_non_empty_does_not_error() {
    assert_eq!(
        eval_posix("P.with_name('y.png')", &posix_st("P", "/a/b/x.txt")).to_display_string(),
        "/a/b/y.png"
    );
}

// =====================================================================
// Pathlib-parity tests: with_name / with_stem / with_suffix / with_number
// =====================================================================
//
// These tests pin the contract for the four `with_*` operators in
// alignment with Python pathlib. Specifically they cover:
//
//   1. **Relative paths stay relative.** A single-component
//      relative path like `path("foo").with_name("x")` produces
//      `"x"`, not `"/x"`. Drive- and UNC-rooted paths produce the
//      correct anchor (`C:\x`, `\\srv\share\x`) without a doubled
//      separator.
//
//   2. **`Invalid name` validation.** `with_name`/`with_stem`
//      reject `""`, `"."`, and separator-containing strings.
//      `".."` is accepted (matches pathlib).
//
//   3. **`Invalid suffix` validation.** `with_suffix` rejects
//      strings that are non-empty and either don't start with `.`
//      or are just `.`. `.tar.gz` (multi-dot) is accepted.
//
//   4. **Empty stem.** `with_stem("")` raises with two distinct
//      messages depending on whether the path has a suffix
//      (`Invalid name ''` vs `'<path>' has a non-empty suffix`),
//      matching pathlib's behaviour.
//
// The empty-name guard for paths-with-no-final-component
// (filesystem roots, bare URI authorities) is covered by the
// existing `with_*_on_*_root_errors` tests above.

fn windows_eval(expr: &str, st: &SymbolTable) -> ExprValue {
    eval_with_fmt(expr, st, PathFormat::Windows)
}

fn assert_err_windows(expr: &str, st: &SymbolTable, expected: &[&str]) {
    let parsed = openjd_expr::ParsedExpression::new(expr).unwrap();
    let symtabs = [st];
    let e = parsed
        .with_path_format(PathFormat::Windows)
        .evaluate(&symtabs)
        .unwrap_err()
        .to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

// ── Relative paths stay relative (the parent-join fix) ─────────────

#[test]
fn with_name_relative_single_component_stays_relative() {
    // pathlib: PurePosixPath("foo").with_name("x") == PurePosixPath("x")
    assert_eq!(
        eval_posix("P.with_name('x')", &posix_st("P", "foo")).to_display_string(),
        "x"
    );
}

#[test]
fn with_name_relative_multi_component_stays_relative() {
    // pathlib: PurePosixPath("foo/bar").with_name("x") == PurePosixPath("foo/x")
    assert_eq!(
        eval_posix("P.with_name('x')", &posix_st("P", "foo/bar")).to_display_string(),
        "foo/x"
    );
}

#[test]
fn with_name_absolute_single_component_keeps_root() {
    // pathlib: PurePosixPath("/foo").with_name("x") == PurePosixPath("/x")
    assert_eq!(
        eval_posix("P.with_name('x')", &posix_st("P", "/foo")).to_display_string(),
        "/x"
    );
}

#[test]
fn with_name_absolute_multi_component_keeps_root() {
    assert_eq!(
        eval_posix("P.with_name('x')", &posix_st("P", "/a/b/c")).to_display_string(),
        "/a/b/x"
    );
}

#[test]
fn with_stem_relative_keeps_relative() {
    assert_eq!(
        eval_posix("P.with_stem('x')", &posix_st("P", "foo.txt")).to_display_string(),
        "x.txt"
    );
}

#[test]
fn with_suffix_relative_keeps_relative() {
    assert_eq!(
        eval_posix("P.with_suffix('.x')", &posix_st("P", "foo")).to_display_string(),
        "foo.x"
    );
}

#[test]
fn with_number_relative_keeps_relative() {
    assert_eq!(
        eval_posix("P.with_number(42)", &posix_st("P", "shot_####.exr")).to_display_string(),
        "shot_0042.exr"
    );
}

#[test]
fn with_number_absolute_keeps_root() {
    // Pre-fix this produced "//shot_0042.exr" (doubled leading
    // slash) because the prefix was constructed as
    // `dir_part + sep`.
    assert_eq!(
        eval_posix("P.with_number(42)", &posix_st("P", "/shot_####.exr")).to_display_string(),
        "/shot_0042.exr"
    );
}

#[test]
fn with_name_windows_drive_root_keeps_root() {
    // pathlib: PureWindowsPath("C:\\foo").with_name("x")
    // produces PureWindowsPath("C:/x") — single backslash, not
    // doubled.
    assert_eq!(
        windows_eval("P.with_name('x')", &windows_st("P", "C:\\foo")).to_display_string(),
        "C:\\x"
    );
}

#[test]
fn with_name_windows_unc_root_keeps_root() {
    // pathlib: PureWindowsPath("\\\\srv\\share\\foo").with_name("x")
    // produces PureWindowsPath("//srv/share/x").
    assert_eq!(
        windows_eval("P.with_name('x')", &windows_st("P", "\\\\srv\\share\\foo"),)
            .to_display_string(),
        "\\\\srv\\share\\x"
    );
}

#[test]
fn with_name_windows_relative_stays_relative() {
    assert_eq!(
        windows_eval("P.with_name('x')", &windows_st("P", "foo")).to_display_string(),
        "x"
    );
}

#[test]
fn with_name_windows_relative_multi_component() {
    assert_eq!(
        windows_eval("P.with_name('x')", &windows_st("P", "foo\\bar")).to_display_string(),
        "foo\\x"
    );
}

// ── `Invalid name` validation (with_name, with_stem) ───────────────

#[test]
fn with_name_empty_string_errors() {
    // pathlib: ValueError: Invalid name ''
    assert_err_posix(
        "P.with_name('')",
        &posix_st("P", "/a/b"),
        &["with_name: Invalid name ''"],
    );
}

#[test]
fn with_name_dot_errors() {
    // pathlib: ValueError: Invalid name '.'
    assert_err_posix(
        "P.with_name('.')",
        &posix_st("P", "/a/b"),
        &["with_name: Invalid name '.'"],
    );
}

#[test]
fn with_name_double_dot_accepted() {
    // pathlib accepts '..' as a name — it's a filename that
    // happens to look like the parent-dir indicator.
    assert_eq!(
        eval_posix("P.with_name('..')", &posix_st("P", "a/b")).to_display_string(),
        "a/.."
    );
}

#[test]
fn with_name_with_separator_errors() {
    // pathlib: ValueError: Invalid name 'a/b'
    assert_err_posix(
        "P.with_name('new/name')",
        &posix_st("P", "/a/b"),
        &["with_name: Invalid name 'new/name'"],
    );
}

#[test]
fn with_name_windows_with_backslash_errors() {
    // On Windows both '/' and '\\' are separators; pathlib
    // rejects either.
    assert_err_windows(
        "P.with_name('x\\\\y')",
        &windows_st("P", "C:\\a\\b"),
        &["with_name: Invalid name 'x\\y'"],
    );
}

#[test]
fn with_stem_empty_on_no_suffix_errors() {
    // pathlib: ValueError: Invalid name '' (the resulting
    // filename would be empty).
    assert_err_posix(
        "P.with_stem('')",
        &posix_st("P", "foo"),
        &["with_stem: Invalid name ''"],
    );
}

#[test]
fn with_stem_empty_on_path_with_suffix_errors() {
    // pathlib: ValueError: PurePosixPath('foo.txt') has a
    // non-empty suffix. The resulting filename `.txt` would
    // parse as a hidden-file name.
    assert_err_posix(
        "P.with_stem('')",
        &posix_st("P", "foo.txt"),
        &["with_stem: 'foo.txt' has a non-empty suffix"],
    );
}

#[test]
fn with_stem_with_separator_errors() {
    assert_err_posix(
        "P.with_stem('a/b')",
        &posix_st("P", "/file.txt"),
        &["with_stem: Invalid name 'a/b'"],
    );
}

// ── `Invalid suffix` validation (with_suffix) ──────────────────────

#[test]
fn with_suffix_empty_strips() {
    // pathlib: PurePosixPath("file.txt").with_suffix("")
    //   == PurePosixPath("file")
    assert_eq!(
        eval_posix("P.with_suffix('')", &posix_st("P", "file.txt")).to_display_string(),
        "file"
    );
}

#[test]
fn with_suffix_dot_alone_errors() {
    // pathlib: ValueError: Invalid suffix '.'
    assert_err_posix(
        "P.with_suffix('.')",
        &posix_st("P", "file"),
        &["with_suffix: Invalid suffix '.'"],
    );
}

#[test]
fn with_suffix_no_leading_dot_errors() {
    // pathlib: ValueError: Invalid suffix 'foo'
    assert_err_posix(
        "P.with_suffix('foo')",
        &posix_st("P", "file"),
        &["with_suffix: Invalid suffix 'foo'"],
    );
}

#[test]
fn with_suffix_with_separator_errors() {
    // pathlib raises 'Invalid name "<resulting filename>"' for
    // a suffix containing a separator. We keep the diagnostic
    // localised to the suffix argument since it's clearer about
    // what the caller did wrong.
    assert_err_posix(
        "P.with_suffix('.x/y')",
        &posix_st("P", "file"),
        &["with_suffix: Invalid suffix '.x/y'"],
    );
}

#[test]
fn with_suffix_multi_dot_accepted() {
    // pathlib: PurePosixPath("a").with_suffix(".tar.gz")
    //   == PurePosixPath("a.tar.gz")
    assert_eq!(
        eval_posix("P.with_suffix('.tar.gz')", &posix_st("P", "a")).to_display_string(),
        "a.tar.gz"
    );
}

#[test]
fn with_suffix_dot_space_accepted() {
    // pathlib: PurePosixPath("a").with_suffix(". ")
    //   == PurePosixPath("a. ") — only the leading dot is
    // checked; trailing whitespace inside the suffix is fine.
    assert_eq!(
        eval_posix("P.with_suffix('. ')", &posix_st("P", "a")).to_display_string(),
        "a. "
    );
}

// =====================================================================
// Pathlib-parity tests (round 2): trailing-dot suffix parsing,
// drive-relative joining, with_stem('.'), and post-construction
// filename validation.
// =====================================================================
//
// Generated by running an exhaustive 1120-case pathlib-vs-ours diff
// (see PR description) and pinning the corner cases that exposed
// real divergences. These cover four categories:
//
//   B. Trailing-dot suffix parsing. Per pathlib, `'foo.'.suffix
//      == ''` and `'foo.'.stem == 'foo.'`. The trailing dot is not
//      a real extension because there's no character after it.
//      Our `.stem`/`.suffix`/`.suffixes` (and the with_* operators
//      that derive from them) now follow the same rule.
//
//   D. Windows drive-relative anchor joining. Per pathlib,
//      `PureWindowsPath('C:foo').with_name('x')` produces `'C:x'`
//      (no separator inserted between drive and name) because
//      `C:` is itself an anchor. Our `join_parent_and_name` now
//      treats a length-2 `[a-z]:` parent as already-terminated.
//
//   C. Post-construction filename validation in with_suffix.
//      `'..foo'.with_suffix('')` per pathlib produces a filename
//      `'.'` (because `'..foo'.stem == '.'` and we strip the
//      suffix), then pathlib catches the `.` as `Invalid name`.
//      We now do the same.
//
//   E. with_stem('.') accepted when path has a non-empty suffix.
//      Per pathlib, `'foo.txt'.with_stem('.')` produces `'..txt'`
//      (a hidden-file with extension `.txt`). We previously
//      rejected `.` as the stem unconditionally; now we route
//      through the post-construction `is_valid_name` check, which
//      accepts `.{ext}` while still rejecting bare `.`.

// ── Category B: trailing-dot suffix parsing ────────────────────

#[test]
fn stem_of_trailing_dot_includes_dot() {
    assert_eq!(
        eval_posix("P.stem", &posix_st("P", "foo.")).to_display_string(),
        "foo."
    );
}
#[test]
fn suffix_of_trailing_dot_is_empty() {
    assert_eq!(
        eval_posix("P.suffix", &posix_st("P", "foo.")).to_display_string(),
        ""
    );
}
#[test]
fn stem_of_double_dot_includes_both_dots() {
    assert_eq!(
        eval_posix("P.stem", &posix_st("P", "..")).to_display_string(),
        ".."
    );
}
#[test]
fn suffix_of_double_dot_is_empty() {
    assert_eq!(
        eval_posix("P.suffix", &posix_st("P", "..")).to_display_string(),
        ""
    );
}
#[test]
fn stem_of_a_b_dot_includes_dot() {
    assert_eq!(
        eval_posix("P.stem", &posix_st("P", "a.b.")).to_display_string(),
        "a.b."
    );
}
#[test]
fn with_stem_on_trailing_dot_path_replaces_correctly() {
    // pathlib: `'foo.'.with_stem('x') == 'x'` (the trailing dot
    // is part of the stem, so replacing the stem replaces
    // everything).
    assert_eq!(
        eval_posix("P.with_stem('x')", &posix_st("P", "foo.")).to_display_string(),
        "x"
    );
}
#[test]
fn with_suffix_empty_on_trailing_dot_keeps_dot() {
    // pathlib: `'foo.'.with_suffix('')` returns `'foo.'` because
    // there's no real suffix to strip — the trailing dot is part
    // of the stem.
    assert_eq!(
        eval_posix("P.with_suffix('')", &posix_st("P", "foo.")).to_display_string(),
        "foo."
    );
}
#[test]
fn suffixes_of_dotted_basename_strips_leading_dots() {
    // pathlib: `'.tar.gz'.suffixes == ['.gz']` — leading dots
    // are stripped before splitting, so a name that LOOKS like
    // a multi-suffix is parsed differently when it leads with a
    // dot. Our `to_display_string` of a string list uses
    // double-quoted strings (`["..."`]); the underlying value
    // matches pathlib's `['.gz']`.
    assert_eq!(
        eval_posix("string(P.suffixes)", &posix_st("P", ".tar.gz")).to_display_string(),
        "[\".gz\"]"
    );
}
#[test]
fn suffixes_of_trailing_dot_is_empty_list() {
    assert_eq!(
        eval_posix("string(P.suffixes)", &posix_st("P", "foo.tar.")).to_display_string(),
        "[]"
    );
}

// ── Category D: Windows drive-relative anchor joining ──────────

#[test]
fn with_name_on_drive_relative_no_extra_separator() {
    // pathlib: `PureWindowsPath('C:foo').with_name('x') == 'C:x'`
    assert_eq!(
        windows_eval("P.with_name('x')", &windows_st("P", "C:foo")).to_display_string(),
        "C:x"
    );
}
#[test]
fn with_stem_on_drive_relative_no_extra_separator() {
    assert_eq!(
        windows_eval("P.with_stem('x')", &windows_st("P", "C:foo.txt")).to_display_string(),
        "C:x.txt"
    );
}
#[test]
fn with_suffix_on_drive_relative_no_extra_separator() {
    assert_eq!(
        windows_eval("P.with_suffix('.png')", &windows_st("P", "C:foo.txt")).to_display_string(),
        "C:foo.png"
    );
}

// ── Category C: post-construction filename validation in with_suffix ──

#[test]
fn with_suffix_empty_on_dot_dot_foo_errors() {
    // pathlib: `'..foo'.suffix == '.foo'`; `'..foo'.stem == '.'`
    //   `'..foo'.with_suffix('')` would produce filename '.'
    //   which pathlib rejects with `Invalid name '.'`.
    assert_err_posix(
        "P.with_suffix('')",
        &posix_st("P", "..foo"),
        &["with_suffix: Invalid name '.'"],
    );
}

// ── Category E: with_stem('.') with non-empty suffix is accepted ──

#[test]
fn with_stem_dot_on_path_with_suffix_accepted() {
    // pathlib: `'foo.txt'.with_stem('.')` produces `'..txt'`
    // (a hidden file with extension `.txt`). We accept this
    // matching pathlib — the resulting filename `..txt` is a
    // valid name.
    assert_eq!(
        eval_posix("P.with_stem('.')", &posix_st("P", "foo.txt")).to_display_string(),
        "..txt"
    );
}
#[test]
fn with_stem_dot_on_path_without_suffix_rejected() {
    // pathlib: `'foo'.with_stem('.')` raises `Invalid name '.'`
    // because the resulting filename would be just `.`.
    assert_err_posix(
        "P.with_stem('.')",
        &posix_st("P", "foo"),
        &["with_stem: Invalid name '.'"],
    );
}

// =====================================================================
// Pathlib-parity tests (round 3): path normalization on construction
// =====================================================================
//
// Category A from the pathlib parity probe: the `path()`
// constructor canonicalizes filesystem inputs the same way
// pathlib does. This means redundant components are eliminated
// at construction time and never leak into downstream operations
// (`string`, `parent`, `with_*`, etc.).
//
// URI inputs are NOT normalized — the spec preserves them
// verbatim because URI path components are opaque (an S3 key
// `a//b` may be a different resource than `a/b`).

// ── Stripping redundant components on construction ─────────────

// ── Stripping redundant components on construction ─────────────
//
// These tests pin POSIX-specific normalization rules so they're
// host-independent: `eval_with_fmt(..., PathFormat::Posix)`
// rather than the bare `eval` helper, which uses
// `PathFormat::host()` and would normalize paths to backslashes
// on Windows runners.

#[test]
fn path_strips_leading_dot_slash() {
    // pathlib: PurePosixPath("./foo") -> "foo"
    assert_eq!(
        eval_with_fmt(
            "string(path('./foo'))",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "foo"
    );
}

#[test]
fn path_strips_internal_dot_segments() {
    // pathlib: PurePosixPath("a/./b") -> "a/b"
    assert_eq!(
        eval_with_fmt(
            "string(path('a/./b'))",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "a/b"
    );
}

#[test]
fn path_strips_trailing_separator() {
    // pathlib: PurePosixPath("a/b/") -> "a/b"
    assert_eq!(
        eval_with_fmt(
            "string(path('a/b/'))",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "a/b"
    );
}

#[test]
fn path_collapses_runs_of_separators() {
    // pathlib: PurePosixPath("a//b") -> "a/b"
    assert_eq!(
        eval_with_fmt(
            "string(path('a//b'))",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "a/b"
    );
}

#[test]
fn path_collapses_three_or_more_leading_slashes() {
    // pathlib: PurePosixPath("///foo") -> "/foo"
    assert_eq!(
        eval_with_fmt(
            "string(path('///foo'))",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "/foo"
    );
}

#[test]
fn path_preserves_double_slash_root() {
    // pathlib special case: PurePosixPath("//foo") -> "//foo"
    // (POSIX double-slash root is preserved per IEEE Std
    // 1003.1-2017 §3.271).
    assert_eq!(
        eval_with_fmt(
            "string(path('//foo'))",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "//foo"
    );
}

#[test]
fn path_preserves_dot_dot_segments() {
    // pathlib: PurePosixPath("a/../b") -> "a/../b" (.. is NOT
    // resolved; pathlib doesn't touch the filesystem so it
    // can't safely simplify symbolic-link-bearing paths).
    assert_eq!(
        eval_with_fmt(
            "string(path('a/../b'))",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "a/../b"
    );
}

#[test]
fn path_empty_string_becomes_dot() {
    // pathlib: PurePosixPath("") -> "."
    assert_eq!(
        eval_with_fmt("string(path(''))", &SymbolTable::new(), PathFormat::Posix)
            .to_display_string(),
        "."
    );
}

// ── Windows normalization ──────────────────────────────────────

#[test]
fn path_windows_normalizes_forward_slash_to_backslash() {
    // pathlib: PureWindowsPath("foo/bar") -> "foo\\bar"
    assert_eq!(
        eval_with_fmt(
            "string(path('foo/bar'))",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        "foo\\bar"
    );
}

#[test]
fn path_windows_collapses_double_backslash_after_drive() {
    // pathlib: PureWindowsPath("C:\\\\foo") -> "C:\\foo"
    // (input is `C:\\foo` source-encoded as `C:\\\\foo`)
    assert_eq!(
        eval_with_fmt(
            "string(path('C:\\\\\\\\foo'))",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        "C:\\foo"
    );
}

// ── parent property reflects normalization ─────────────────────

#[test]
fn parent_of_dot_slash_path_is_dot() {
    // pathlib: PurePosixPath("./foo").parent -> "."
    assert_eq!(
        eval_with_fmt(
            "string(path('./foo').parent)",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "."
    );
}

#[test]
fn parent_of_three_slash_path_is_single_slash() {
    // pathlib: PurePosixPath("///foo").parent -> "/"
    assert_eq!(
        eval_with_fmt(
            "string(path('///foo').parent)",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "/"
    );
}

// ── URI paths are NOT normalized ───────────────────────────────
//
// URIs must short-circuit format-aware processing in `path_fn`,
// so the constructor preserves URI grammar (forward slashes,
// opaque authority, no normalization) regardless of the
// evaluator's `PathFormat`. Run each case under BOTH formats:
// passing under POSIX alone wouldn't catch a regression where
// URI inputs leak into host-format normalization on a
// Windows-format evaluator. Per spec wiki §1.2.1, URI components
// are opaque (S3 keys `a//b` and `a/b` may be different
// resources).

#[test]
fn uri_path_preserves_double_slash_segments_posix() {
    assert_eq!(
        eval_with_fmt(
            "string(path('s3://bucket/a//b'))",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "s3://bucket/a//b"
    );
}

#[test]
fn uri_path_preserves_double_slash_segments_windows() {
    assert_eq!(
        eval_with_fmt(
            "string(path('s3://bucket/a//b'))",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        "s3://bucket/a//b"
    );
}

#[test]
fn uri_path_preserves_dot_segments_posix() {
    assert_eq!(
        eval_with_fmt(
            "string(path('s3://bucket/a/./b'))",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "s3://bucket/a/./b"
    );
}

#[test]
fn uri_path_preserves_dot_segments_windows() {
    assert_eq!(
        eval_with_fmt(
            "string(path('s3://bucket/a/./b'))",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        "s3://bucket/a/./b"
    );
}

#[test]
fn uri_path_preserves_trailing_slash_posix() {
    assert_eq!(
        eval_with_fmt(
            "string(path('s3://bucket/dir/'))",
            &SymbolTable::new(),
            PathFormat::Posix
        )
        .to_display_string(),
        "s3://bucket/dir/"
    );
}

#[test]
fn uri_path_preserves_trailing_slash_windows() {
    assert_eq!(
        eval_with_fmt(
            "string(path('s3://bucket/dir/'))",
            &SymbolTable::new(),
            PathFormat::Windows
        )
        .to_display_string(),
        "s3://bucket/dir/"
    );
}

// ── Windows drive-relative `x:y` disambiguation in with_name ───

#[test]
fn with_name_drive_letter_pattern_prepended_with_dot_slash() {
    // pathlib: PureWindowsPath("foo").with_name("x:y") -> ".\\x:y"
    // (prepends `.\` to disambiguate from drive-relative path)
    assert_eq!(
        windows_eval("P.with_name('x:y')", &windows_st("P", "foo")).to_display_string(),
        ".\\x:y"
    );
}
