// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests that all string functions operate on Unicode codepoints (matching Python behavior),
//! NOT on UTF-8 byte offsets. Each test case has been verified against CPython 3.x.
//!
//! Test strings:
//!   "café"     — 4 codepoints, 5 UTF-8 bytes (é = U+00E9, 2 bytes)
//!   "日本語"   — 3 codepoints, 9 UTF-8 bytes (each CJK char = 3 bytes)
//!   "hello🌍"  — 6 codepoints, 8 UTF-8 bytes (🌍 = U+1F30D, 4 bytes)
//!   "👨\u{200D}👩\u{200D}👧" — 5 codepoints (ZWJ family emoji, 1 grapheme cluster)

use openjd_expr::{ExprValue, ParsedExpression, SymbolTable};

fn eval(expr: &str) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap()
}

#[allow(dead_code)]
fn eval_err(expr: &str) -> String {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string()
}

#[allow(dead_code)]
fn eval_fails(expr: &str) -> bool {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_err()
}

// === len — codepoint count, not byte count ===

#[test]
fn len_cafe() {
    assert_eq!(eval("len('café')"), ExprValue::Int(4));
}
#[test]
fn len_nihongo() {
    assert_eq!(eval("len('日本語')"), ExprValue::Int(3));
}
#[test]
fn len_emoji() {
    assert_eq!(eval("len('hello🌍')"), ExprValue::Int(6));
}
#[test]
fn len_zwj_family() {
    // ZWJ emoji: 👨 + ZWJ + 👩 + ZWJ + 👧 = 5 codepoints (1 grapheme cluster)
    // Python: len("👨\u200d👩\u200d👧") == 5
    assert_eq!(eval("len('👨\u{200D}👩\u{200D}👧')"), ExprValue::Int(5));
}

// === find — codepoint offset, not byte offset ===

#[test]
fn find_cafe_accent() {
    // Python: "café".find("é") == 3
    assert_eq!(eval("find('café', 'é')"), ExprValue::Int(3));
}
#[test]
fn find_nihongo_last() {
    // Python: "日本語".find("語") == 2
    assert_eq!(eval("find('日本語', '語')"), ExprValue::Int(2));
}
#[test]
fn find_after_emoji() {
    // Python: "hello🌍world".find("world") == 6
    assert_eq!(eval("find('hello🌍world', 'world')"), ExprValue::Int(6));
}

// === rfind — codepoint offset ===

#[test]
fn rfind_with_multibyte() {
    // Python: "abcéabc".rfind("abc") == 4
    assert_eq!(eval("rfind('abcéabc', 'abc')"), ExprValue::Int(4));
}
#[test]
fn rfind_cjk() {
    // Python: "日本語日".rfind("日") == 3
    assert_eq!(eval("rfind('日本語日', '日')"), ExprValue::Int(3));
}

// === index — codepoint offset ===

#[test]
fn index_cafe_accent() {
    // Python: "café".index("é") == 3
    assert_eq!(eval("index('café', 'é')"), ExprValue::Int(3));
}
#[test]
fn index_nihongo() {
    // Python: "日本語".index("語") == 2
    assert_eq!(eval("index('日本語', '語')"), ExprValue::Int(2));
}

// === rindex — codepoint offset ===

#[test]
fn rindex_multibyte() {
    // Python: "caféfé".rindex("fé") == 4
    assert_eq!(eval("rindex('caféfé', 'fé')"), ExprValue::Int(4));
}
#[test]
fn rindex_cjk() {
    // Python: "日本語日".rindex("日") == 3
    assert_eq!(eval("rindex('日本語日', '日')"), ExprValue::Int(3));
}

// === center — codepoint-based width ===

#[test]
fn center_cafe() {
    // Python: "café".center(10) == "   café   "
    // 4 codepoints, width 10 → 6 padding (3 left, 3 right)
    assert_eq!(eval("center('café', 10)").to_display_string(), "   café   ");
}
#[test]
fn center_cjk() {
    // Python: "日本語".center(9) == "   日本語   "
    // 3 codepoints, width 9 → 6 padding (3 left, 3 right)
    assert_eq!(
        eval("center('日本語', 9)").to_display_string(),
        "   日本語   "
    );
}
#[test]
fn center_emoji() {
    // Python: "🌍".center(5) == "  🌍  "
    // 1 codepoint, width 5 → 4 padding (2 left, 2 right)
    assert_eq!(eval("center('🌍', 5)").to_display_string(), "  🌍  ");
}
#[test]
fn center_already_wide() {
    // Python: "café".center(2) == "café" (no padding, 4 >= 2)
    assert_eq!(eval("center('café', 2)").to_display_string(), "café");
}

// === ljust — codepoint-based width ===

#[test]
fn ljust_cafe() {
    // Python: "café".ljust(10) == "café      "
    assert_eq!(eval("ljust('café', 10)").to_display_string(), "café      ");
}
#[test]
fn ljust_cjk() {
    // Python: "日本語".ljust(9) == "日本語      "
    assert_eq!(
        eval("ljust('日本語', 9)").to_display_string(),
        "日本語      "
    );
}
#[test]
fn ljust_emoji() {
    // Python: "🌍".ljust(5) == "🌍    "
    assert_eq!(eval("ljust('🌍', 5)").to_display_string(), "🌍    ");
}

// === rjust — codepoint-based width ===

#[test]
fn rjust_cafe() {
    // Python: "café".rjust(10) == "      café"
    assert_eq!(eval("rjust('café', 10)").to_display_string(), "      café");
}
#[test]
fn rjust_cjk() {
    // Python: "日本語".rjust(9) == "      日本語"
    assert_eq!(
        eval("rjust('日本語', 9)").to_display_string(),
        "      日本語"
    );
}
#[test]
fn rjust_emoji() {
    // Python: "🌍".rjust(5) == "    🌍"
    assert_eq!(eval("rjust('🌍', 5)").to_display_string(), "    🌍");
}

// === zfill — codepoint-based width ===

#[test]
fn zfill_cafe() {
    // Python: "café".zfill(8) == "0000café"
    assert_eq!(eval("zfill('café', 8)").to_display_string(), "0000café");
}
#[test]
fn zfill_neg_cafe() {
    // Python: "-café".zfill(10) == "-00000café"
    assert_eq!(eval("zfill('-café', 10)").to_display_string(), "-00000café");
}

// === repr_json — control character escaping (matching json.dumps ensure_ascii=True) ===

#[test]
fn repr_json_newline() {
    // Python: json.dumps("hello\nworld") == '"hello\\nworld"'
    assert_eq!(
        eval(r#"repr_json("hello\nworld")"#).to_display_string(),
        r#""hello\nworld""#
    );
}
#[test]
fn repr_json_tab() {
    // Python: json.dumps("hello\tworld") == '"hello\\tworld"'
    assert_eq!(
        eval(r#"repr_json("hello\tworld")"#).to_display_string(),
        r#""hello\tworld""#
    );
}
#[test]
fn repr_json_carriage_return() {
    // Python: json.dumps("hello\rworld") == '"hello\\rworld"'
    assert_eq!(
        eval(r#"repr_json("hello\rworld")"#).to_display_string(),
        r#""hello\rworld""#
    );
}
#[test]
fn repr_json_ensure_ascii_cafe() {
    // Python: json.dumps("café") == '"caf\\u00e9"'
    assert_eq!(
        eval("repr_json('café')").to_display_string(),
        r#""caf\u00e9""#
    );
}
#[test]
fn repr_json_ensure_ascii_cjk() {
    // Python: json.dumps("日本") == '"\\u65e5\\u672c"'
    assert_eq!(
        eval("repr_json('日本')").to_display_string(),
        r#""\u65e5\u672c""#
    );
}
#[test]
fn repr_json_ensure_ascii_emoji() {
    // Python: json.dumps("🌍") == '"\\ud83c\\udf0d"' (surrogate pair)
    assert_eq!(
        eval("repr_json('🌍')").to_display_string(),
        r#""\ud83c\udf0d""#
    );
}

// === Grapheme cluster tests — Python counts codepoints, not grapheme clusters ===
// These confirm that the Rust implementation matches Python's codepoint semantics,
// NOT grapheme cluster semantics (which would give different results).

#[test]
fn find_after_zwj_emoji() {
    // "a" + ZWJ family (5 codepoints) + "b" = 7 codepoints total
    // Python: "a👨\u200d👩\u200d👧b".find("b") == 6
    assert_eq!(
        eval("find('a👨\u{200D}👩\u{200D}👧b', 'b')"),
        ExprValue::Int(6)
    );
}
#[test]
fn len_string_with_zwj() {
    // Python: len("a👨\u200d👩\u200d👧b") == 7
    assert_eq!(eval("len('a👨\u{200D}👩\u{200D}👧b')"), ExprValue::Int(7));
}
