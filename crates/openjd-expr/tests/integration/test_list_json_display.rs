// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! `string(list)` and the display form of lists must produce valid JSON.
//!
//! Regression tests for openjd-rs#312: element strings were quoted but not
//! escaped, so a quote, backslash, newline, or control character in an element
//! produced output no JSON parser accepts. See `specs/expr/values.md`
//! ("List display strings").

use openjd_expr::{
    ExprValue, FormatString, FormatStringOptions, ParsedExpression, PathFormat, SymbolTable,
};

fn eval(expr: &str) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap()
}

fn eval_windows(expr: &str) -> ExprValue {
    let parsed = ParsedExpression::new(expr).unwrap();
    let st = SymbolTable::new();
    let symtabs = [&st];
    parsed
        .with_path_format(PathFormat::Windows)
        .evaluate(&symtabs)
        .unwrap()
}

/// Assert the exact rendering *and* that it parses as JSON with the expected
/// elements — the exact string alone would not catch a mismatched escape that
/// happens to look plausible.
#[track_caller]
fn assert_json(expr: &str, expected: &str, elements: &[&str]) {
    let rendered = eval(expr).to_display_string();
    assert_eq!(rendered, expected, "rendering of {expr}");
    let parsed: Vec<String> = serde_json::from_str(&rendered)
        .unwrap_or_else(|e| panic!("{expr} produced invalid JSON {rendered:?}: {e}"));
    assert_eq!(parsed, elements, "round-tripped elements of {expr}");
}

#[test]
fn double_quote_is_escaped() {
    assert_json(r#"string(['a"b'])"#, r#"["a\"b"]"#, &["a\"b"]);
}

#[test]
fn backslash_is_escaped() {
    assert_json(r"string(['a\\b'])", r#"["a\\b"]"#, &[r"a\b"]);
}

#[test]
fn newline_tab_and_carriage_return_are_escaped() {
    assert_json(
        r"string(['a\nb\tc\rd'])",
        r#"["a\nb\tc\rd"]"#,
        &["a\nb\tc\rd"],
    );
}

#[test]
fn control_characters_are_escaped() {
    assert_json(
        r"string(['\x00\x1f\x08\x0c'])",
        r#"["\u0000\u001f\b\f"]"#,
        &["\u{0}\u{1f}\u{8}\u{c}"],
    );
}

#[test]
fn non_ascii_is_preserved_verbatim() {
    // Pins our local choice, not a specification requirement: the spec leaves
    // non-ASCII handling to the implementation and only requires parseable
    // JSON. Display strings favour readability; `repr_json` escapes for
    // transport.
    assert_json(
        "string(['café', '😀'])",
        "[\"café\", \"😀\"]",
        &["café", "😀"],
    );
}

#[test]
fn nested_lists_escape_their_elements() {
    let rendered = eval(r#"string([['a"b'], ['c\\d']])"#).to_display_string();
    assert_eq!(rendered, r#"[["a\"b"], ["c\\d"]]"#);
    let parsed: Vec<Vec<String>> = serde_json::from_str(&rendered).unwrap();
    assert_eq!(
        parsed,
        vec![vec!["a\"b".to_string()], vec![r"c\d".to_string()]]
    );
}

#[test]
fn windows_path_list_is_valid_json() {
    // Every Windows path contains backslashes, so this is the case most likely
    // to be hit in practice.
    let rendered =
        eval_windows(r"string([path('C:\\Users\\me'), path('C:\\tmp')])").to_display_string();
    assert_eq!(rendered, r#"["C:\\Users\\me", "C:\\tmp"]"#);
    let parsed: Vec<String> = serde_json::from_str(&rendered).unwrap();
    assert_eq!(parsed, vec![r"C:\Users\me", r"C:\tmp"]);
}

#[test]
fn format_string_interpolation_matches_string_fn() {
    // The spec requires interpolation with surrounding text to convert the
    // value to a string the same way `string()` does.
    let st = SymbolTable::new();
    let interpolated = FormatString::new(r#"items: {{ ['a"b'] }}"#)
        .unwrap()
        .resolve_string_with(&st, &FormatStringOptions::default())
        .unwrap();
    assert_eq!(interpolated, r#"items: ["a\"b"]"#);
    assert_eq!(
        interpolated,
        format!("items: {}", eval(r#"string(['a"b'])"#).to_display_string())
    );
}

#[test]
fn repr_json_still_escapes_non_ascii() {
    // Guards the deliberate difference between the two JSON producers.
    assert_eq!(
        eval("repr_json(['café'])").to_display_string(),
        r#"["caf\u00e9"]"#
    );
    assert_eq!(
        eval(r#"repr_json(['a"b'])"#).to_display_string(),
        r#"["a\"b"]"#
    );
}
