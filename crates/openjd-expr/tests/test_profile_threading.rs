// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for profile threading through `ParsedExpression::with_profile` and
//! `FormatString::with_profile`.
//!
//! Under 2026-02 every `SyntaxFeature` variant is rejected, so these tests
//! focus on API-level guarantees:
//!
//! - `ParsedExpression::new(expr)` is observably equivalent to
//!   `ParsedExpression::with_profile(expr, &ExprProfile::latest())`.
//! - `ParsedExpression::with_profile(expr, &ExprProfile::current())` rejects
//!   the same syntax features `new` does.
//! - Basic expressions parse successfully under all standard profile variants.
//! - `FormatString` follows the same shape.
//!
//! When a future revision flips any `SyntaxFeature` to allowed, the tests
//! marked TODO-future-revision should be updated (or paired alternate
//! profiles introduced) to exercise the gating.

use openjd_expr::{ExprProfile, ExprRevision, FormatString, ParsedExpression, SymbolTable};

// ── Parsed expression ────────────────────────────────────────

#[test]
fn new_matches_with_profile_latest_for_success() {
    // A valid expression must succeed identically under both APIs.
    let a = ParsedExpression::new("1 + 2")
        .expect("parses")
        .evaluate(&SymbolTable::new())
        .expect("evaluates");
    let b = ParsedExpression::with_profile("1 + 2", &ExprProfile::latest())
        .expect("parses")
        .evaluate(&SymbolTable::new())
        .expect("evaluates");
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
}

#[test]
fn new_matches_with_profile_latest_for_rejection() {
    // A parse-time rejection must produce the same error message under
    // `new` and `with_profile(latest)`.
    let a = ParsedExpression::new("lambda x: x")
        .err()
        .map(|e| e.message().to_string());
    let b = ParsedExpression::with_profile("lambda x: x", &ExprProfile::latest())
        .err()
        .map(|e| e.message().to_string());
    assert_eq!(a, b);
    assert!(a.unwrap().contains("Lambda expressions are not supported"));
}

#[test]
fn with_profile_current_rejects_lambda() {
    // Under the current (= only) revision, lambda is rejected.
    let err = ParsedExpression::with_profile("lambda x: x", &ExprProfile::current())
        .expect_err("lambda must be rejected under 2026-02");
    assert!(err
        .message()
        .contains("Lambda expressions are not supported"));
}

#[test]
fn with_profile_v2026_02_rejects_dict_literal() {
    let profile = ExprProfile::new(ExprRevision::V2026_02);
    let err = ParsedExpression::with_profile("{'a': 1}", &profile)
        .expect_err("dict literal must be rejected under 2026-02");
    assert!(err.message().contains("Dict literals are not supported"));
}

#[test]
fn with_profile_v2026_02_rejects_bitwise_or() {
    let profile = ExprProfile::new(ExprRevision::V2026_02);
    let err = ParsedExpression::with_profile("1 | 2", &profile)
        .expect_err("bitwise OR must be rejected under 2026-02");
    assert!(err.message().contains("Bitwise OR"));
}

#[test]
fn with_profile_v2026_02_rejects_keyword_arg() {
    let profile = ExprProfile::new(ExprRevision::V2026_02);
    let err = ParsedExpression::with_profile("f(key=1)", &profile)
        .expect_err("keyword arguments must be rejected under 2026-02");
    assert!(err
        .message()
        .contains("Keyword arguments are not supported"));
}

#[test]
fn with_profile_v2026_02_accepts_basic_arithmetic() {
    let profile = ExprProfile::new(ExprRevision::V2026_02);
    let p =
        ParsedExpression::with_profile("(1 + 2) * 3", &profile).expect("basic arithmetic parses");
    let v = p.evaluate(&SymbolTable::new()).expect("evaluates");
    assert_eq!(
        format!("{v:?}"),
        format!("{:?}", openjd_expr::ExprValue::Int(9))
    );
}

#[test]
fn with_profile_accepts_list_comprehension() {
    // The structural-validation gates that depend on profile (multiple for,
    // multiple if, tuple unpacking) must not affect a simple, legal list
    // comprehension.
    let profile = ExprProfile::current();
    let p = ParsedExpression::with_profile("[x * 2 for x in [1, 2, 3]]", &profile)
        .expect("simple list comprehension parses");
    let v = p.evaluate(&SymbolTable::new()).expect("evaluates");
    assert_eq!(
        format!("{v:?}"),
        format!("{:?}", openjd_expr::ExprValue::ListInt(vec![2, 4, 6]))
    );
}

#[test]
fn with_profile_rejects_multiple_for_clauses_under_v2026_02() {
    let profile = ExprProfile::current();
    let err = ParsedExpression::with_profile("[x for x in [1, 2] for y in [3, 4]]", &profile)
        .expect_err("multiple for clauses must be rejected under 2026-02");
    assert!(err
        .message()
        .contains("Multiple 'for' clauses in list comprehensions are not supported"));
}

#[test]
fn with_profile_rejects_multiple_if_clauses_under_v2026_02() {
    let profile = ExprProfile::current();
    let err = ParsedExpression::with_profile("[x for x in [1, 2, 3] if x > 0 if x < 3]", &profile)
        .expect_err("multiple if clauses must be rejected under 2026-02");
    assert!(err
        .message()
        .contains("Multiple 'if' clauses in a list comprehension are not supported"));
}

// ── Format string ────────────────────────────────────────────

#[test]
fn format_string_new_matches_with_profile_latest() {
    let a = FormatString::new("hello {{1 + 2}}")
        .expect("parses")
        .raw()
        .to_string();
    let b = FormatString::with_profile("hello {{1 + 2}}", &ExprProfile::latest())
        .expect("parses")
        .raw()
        .to_string();
    assert_eq!(a, b);
}

#[test]
fn format_string_with_profile_rejects_lambda_in_interpolation() {
    let profile = ExprProfile::current();
    let err = FormatString::with_profile("{{lambda x: x}}", &profile)
        .expect_err("lambda in interpolation must be rejected under 2026-02");
    assert!(err
        .message()
        .contains("Lambda expressions are not supported"));
}

#[test]
fn format_string_with_profile_accepts_literal_text() {
    let profile = ExprProfile::current();
    let fs = FormatString::with_profile("hello world", &profile).expect("literal text parses");
    assert_eq!(fs.raw(), "hello world");
}

#[test]
fn format_string_with_profile_accepts_simple_interpolation() {
    let profile = ExprProfile::current();
    let fs =
        FormatString::with_profile("x = {{1 + 2}}", &profile).expect("simple interpolation parses");
    assert_eq!(fs.raw(), "x = {{1 + 2}}");
}
