// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for AST-based regex pattern validation (SEC-2026-3).
//!
//! Replaces the previous substring-matching validator with a pass that
//! parses the pattern via `regex_syntax` and walks the resulting HIR.
//! These tests verify both the rejection set (lookaround, backreferences,
//! etc.) and the false-positive cases where the old substring match
//! would misfire.

use openjd_expr::{ParsedExpression, SymbolTable};

fn eval_expr(expr: &str) -> Result<openjd_expr::ExprValue, openjd_expr::ExpressionError> {
    ParsedExpression::new(expr).and_then(|p| p.evaluate(&SymbolTable::new()))
}

// ── Rejection cases: regex features the spec explicitly disallows ──

#[test]
fn reject_positive_lookahead() {
    let err = eval_expr(r"re_match('foo', 'foo(?=bar)')").unwrap_err();
    assert!(
        err.message().contains("lookahead") || err.message().contains("look-around"),
        "got: {}",
        err.message()
    );
}

#[test]
fn reject_negative_lookahead() {
    let err = eval_expr(r"re_match('foo', 'foo(?!bar)')").unwrap_err();
    assert!(
        err.message().contains("lookahead") || err.message().contains("look-around"),
        "got: {}",
        err.message()
    );
}

#[test]
fn reject_positive_lookbehind() {
    let err = eval_expr(r"re_match('foo', '(?<=bar)foo')").unwrap_err();
    assert!(
        err.message().contains("lookbehind") || err.message().contains("look-around"),
        "got: {}",
        err.message()
    );
}

#[test]
fn reject_negative_lookbehind() {
    let err = eval_expr(r"re_match('foo', '(?<!bar)foo')").unwrap_err();
    assert!(
        err.message().contains("lookbehind") || err.message().contains("look-around"),
        "got: {}",
        err.message()
    );
}

#[test]
fn reject_empty_pattern() {
    let err = eval_expr(r"re_match('foo', '')").unwrap_err();
    assert!(
        err.message().to_lowercase().contains("empty"),
        "got: {}",
        err.message()
    );
}

// ── False-positive repair cases: patterns the OLD validator wrongly rejected ──

#[test]
fn allow_question_mark_in_character_class() {
    // `[(?=]` — the `(?=` substring appears inside a character class and is
    // literal. The old validator rejected this; the new one must allow it.
    let result = eval_expr(r"re_match('?', '[(?=]')").unwrap();
    // Match is a list (group 0) on success, null on no match
    match &result {
        openjd_expr::ExprValue::ListString(v, _) => {
            assert_eq!(v.len(), 1);
            assert_eq!(v[0], "?");
        }
        other => panic!("expected ListString, got: {:?}", other),
    }
}

#[test]
fn allow_regex_comment_containing_lookahead_syntax() {
    // `(?#lookahead)` is a regex comment, not a lookahead. The old validator
    // would have blown up. The Rust regex crate doesn't support Python-style
    // `(?#...)` comments — this should either parse OK (if treated as a
    // trivial group) or fail with a parser error, NOT with a spurious
    // "lookahead" rejection. Either outcome is acceptable for this test;
    // we just assert we don't get the bogus lookahead error.
    let result = eval_expr(r"re_match('abc', '(?#comment)abc')");
    match result {
        Ok(_) => {} // regex accepted it
        Err(e) => assert!(
            !e.message().to_lowercase().contains("lookahead"),
            "spurious lookahead rejection: {}",
            e.message()
        ),
    }
}

#[test]
fn allow_pattern_with_escaped_question_mark_followed_by_equals() {
    // `\?=` is a literal "?=" sequence, not a lookahead start. Old validator
    // could be fooled by this.
    let result = eval_expr(r"re_match('a?=b', 'a\?=b')").unwrap();
    match &result {
        openjd_expr::ExprValue::ListString(v, _) => {
            assert_eq!(v[0], "a?=b");
        }
        other => panic!("expected match, got: {:?}", other),
    }
}

// ── Normal patterns that must continue to work ──

#[test]
fn allow_simple_patterns() {
    assert!(eval_expr(r"re_match('abc', 'abc')").is_ok());
    assert!(eval_expr(r"re_match('abc', '[a-z]+')").is_ok());
    assert!(eval_expr(r"re_match('abc123', '\w+')").is_ok());
    assert!(eval_expr(r"re_match('abc', '(a)(b)(c)')").is_ok());
}

#[test]
fn allow_named_groups() {
    assert!(eval_expr(r"re_search('foo123', '(?P<name>\d+)')").is_ok());
}

#[test]
fn allow_non_capturing_groups() {
    assert!(eval_expr(r"re_match('abab', '(?:ab)+')").is_ok());
}

#[test]
fn allow_inline_flags() {
    assert!(eval_expr(r"re_match('ABC', '(?i)abc')").is_ok());
    assert!(eval_expr(r"re_match('abc', '(?m)^abc$')").is_ok());
}

#[test]
fn allow_unicode_patterns() {
    assert!(eval_expr(r"re_match('héllo', 'h.llo')").is_ok());
}

// ── Backreference rejection remains correct ──

#[test]
fn reject_backreference_in_replacement() {
    // Use the Python raw-string prefix so the backslash reaches the replacement
    // validator instead of being interpreted as an escape. Without `r`, `'\1'`
    // is the single byte 0x01, not `\1`.
    let err = eval_expr(r"re_sub('aaa', '(a)', r'\1')").unwrap_err();
    assert!(
        err.message().to_lowercase().contains("group reference")
            || err.message().to_lowercase().contains("backreference"),
        "got: {}",
        err.message()
    );
}

// ── Rust-only features (spec §2.2.5 rejects these too) ──
//
// `regex_syntax::Parser` happily parses these because they're valid Rust
// regex syntax. The validator must reject them anyway to preserve
// Python/Rust cross-platform compatibility as required by the spec.

#[test]
fn reject_backslash_lower_z_end_anchor() {
    // `\z` is the Rust end-of-string anchor, not supported in Python `re`.
    // Conformance test: expr2.2.5--re-backslash-lower-z.invalid.yaml
    let err = eval_expr(r#"re_search('hello', r'llo\z')"#).unwrap_err();
    assert!(
        err.message()
            .to_lowercase()
            .contains("end-of-string anchor")
            || err.message().contains(r"\z"),
        "got: {}",
        err.message()
    );
}

#[test]
fn reject_backslash_upper_z_end_anchor() {
    // `\Z` is Python's end-of-string anchor, not supported in Rust `regex`.
    // Conformance test: expr2.2.5--re-backslash-upper-Z.invalid.yaml
    let err = eval_expr(r#"re_search('hello', r'llo\Z')"#).unwrap_err();
    assert!(
        err.message()
            .to_lowercase()
            .contains("end-of-string anchor")
            || err.message().contains(r"\Z"),
        "got: {}",
        err.message()
    );
}

#[test]
fn reject_unicode_brace_lowercase_x() {
    // `\x{HHHH}` is Rust-only Unicode brace syntax, not supported in Python.
    let err = eval_expr(r"re_match('A', r'\x{0041}')").unwrap_err();
    assert!(
        err.message().to_lowercase().contains("unicode brace") || err.message().contains(r"\x{"),
        "got: {}",
        err.message()
    );
}

#[test]
fn reject_unicode_brace_lowercase_u() {
    // `\u{HHHH}` is Rust-only Unicode brace syntax, not supported in Python.
    let err = eval_expr(r"re_match('A', r'\u{0041}')").unwrap_err();
    assert!(
        err.message().to_lowercase().contains("unicode brace") || err.message().contains(r"\u{"),
        "got: {}",
        err.message()
    );
}

#[test]
fn reject_unicode_brace_uppercase_u() {
    // `\U{HHHH}` is Rust-only Unicode brace syntax, not supported in Python.
    let err = eval_expr(r"re_match('A', r'\U{0041}')").unwrap_err();
    assert!(
        err.message().to_lowercase().contains("unicode brace") || err.message().contains(r"\U{"),
        "got: {}",
        err.message()
    );
}

// ── Positive cases that must still be accepted ──

#[test]
fn allow_non_brace_hex_escape() {
    // `\xHH` (no braces) is supported by both Python and Rust.
    let result = eval_expr(r"re_match('A', r'\x41')").unwrap();
    match &result {
        openjd_expr::ExprValue::ListString(v, _) => assert_eq!(v[0], "A"),
        other => panic!("expected match, got: {:?}", other),
    }
}

#[test]
fn allow_non_brace_unicode_escape() {
    // `\uHHHH` (no braces) is supported by both Python and Rust.
    let result = eval_expr(r"re_match('A', r'\u0041')").unwrap();
    match &result {
        openjd_expr::ExprValue::ListString(v, _) => assert_eq!(v[0], "A"),
        other => panic!("expected match, got: {:?}", other),
    }
}

#[test]
fn allow_dollar_end_anchor() {
    // `$` is the portable end-of-line anchor supported by both engines.
    let result = eval_expr(r"re_search('hello', 'llo$')").unwrap();
    match &result {
        openjd_expr::ExprValue::ListString(v, _) => assert_eq!(v[0], "llo"),
        other => panic!("expected match, got: {:?}", other),
    }
}
