// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test_int64_bounds.py

use openjd_expr::{ExprValue, ParsedExpression, SymbolTable};

fn eval(expr: &str) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap()
}
fn eval_fails(expr: &str) -> bool {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_err()
}
#[allow(dead_code)]
fn eval_err(expr: &str) -> String {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .message()
}
fn assert_err(expr: &str, expected: &[&str]) {
    let e = ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

// === TestInt64ValidBounds ===
#[test]
fn int64_max_literal() {
    assert_eq!(
        eval("9223372036854775807").to_display_string(),
        "9223372036854775807"
    );
}
#[test]
fn int64_min_literal() {
    assert_eq!(
        eval("-9223372036854775808").to_display_string(),
        "-9223372036854775808"
    );
}
#[test]
fn int64_min_plus_max() {
    assert_eq!(
        eval("-9223372036854775808 + 9223372036854775807").to_display_string(),
        "-1"
    );
}

// === TestInt64OverflowLiterals ===
#[test]
fn positive_literal_overflow() {
    let e = ParsedExpression::new("9223372036854775808")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("Integer overflow") && e.contains("9223372036854775808") && e.contains("^"),
        "got:\n{e}"
    );
}
#[test]
fn negative_literal_overflow() {
    assert!(eval_fails("-9223372036854775809"));
}

#[test]
fn negative_literal_overflow_message() {
    assert_err(
        "-9223372036854775809",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  -9223372036854775809\n",
            "  ^~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

// === TestInt64OverflowArithmetic ===
#[test]
fn add_overflow() {
    assert!(eval_fails("9223372036854775807 + 1"));
}
#[test]
fn sub_overflow() {
    assert!(eval_fails("-9223372036854775808 - 1"));
}
#[test]
fn mul_overflow() {
    assert!(eval_fails("9223372036854775807 * 2"));
}

#[test]
fn add_overflow_message() {
    assert_err(
        "9223372036854775807 + 1",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  9223372036854775807 + 1\n",
            "  ~~~~~~~~~~~~~~~~~~~~^~~",
        ],
    );
}

#[test]
fn sub_overflow_message() {
    assert_err(
        "-9223372036854775808 - 1",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  -9223372036854775808 - 1\n",
            "  ~~~~~~~~~~~~~~~~~~~~~^~~",
        ],
    );
}

#[test]
fn mul_overflow_positive() {
    assert_err(
        "4611686018427387905 * 2",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  4611686018427387905 * 2\n",
            "  ~~~~~~~~~~~~~~~~~~~~^~~",
        ],
    );
}

#[test]
fn mul_overflow_negative() {
    assert_err(
        "-4611686018427387905 * 2",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  -4611686018427387905 * 2\n",
            "  ~~~~~~~~~~~~~~~~~~~~~^~~",
        ],
    );
}

// === TestInt64ModByOne ===
#[test]
fn int64_min_mod_neg1() {
    assert_eq!(eval("-9223372036854775808 % -1").to_display_string(), "0");
}
#[test]
fn int64_min_mod_1() {
    assert_eq!(eval("-9223372036854775808 % 1").to_display_string(), "0");
}

// === TestInt64OverflowPow ===
#[test]
fn pow_overflow() {
    assert!(eval_fails("2 ** 63"));
}
#[test]
fn pow_large_base() {
    assert!(eval_fails("10 ** 19"));
}

#[test]
fn pow_overflow_64() {
    assert_err(
        "2 ** 64",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  2 ** 64\n",
            "  ~~^~~~~",
        ],
    );
}

#[test]
fn pow_overflow_large_exponent() {
    assert_err(
        "2 ** 1000000",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  2 ** 1000000\n",
            "  ~~^~~~~~~~~~",
        ],
    );
}

// === TestInt64OverflowConversion ===
#[test]
fn int_from_large_float() {
    assert!(eval_fails("int(1e19)"));
}
#[test]
fn int_from_string_overflow() {
    assert!(eval_fails("int('99999999999999999999')"));
}

#[test]
fn float_to_int_overflow_message() {
    assert_err(
        "int(9.3e18)",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  int(9.3e18)\n",
            "  ^~~~~~~~~~~",
        ],
    );
}

// === TestInt64OverflowNegation ===
#[test]
fn negate_int_min_via_variable() {
    let mut st = SymbolTable::new();
    st.set("x", ExprValue::Int(i64::MIN)).unwrap();
    let e = ParsedExpression::new("-x")
        .and_then(|p| p.evaluate(&st))
        .unwrap_err()
        .to_string();
    assert!(e.contains("Integer overflow"), "got:\n{e}");
}

// === TestInt64FloatUnaffected ===
#[test]
fn float_large_ok() {
    assert!(matches!(eval("1e300"), ExprValue::Float(_)));
}
#[test]
fn float_small_ok() {
    assert!(matches!(eval("1e-300"), ExprValue::Float(_)));
}

#[test]
fn large_positive_float() {
    assert_eq!(
        eval("9223372036854775808.0").to_display_string(),
        "9223372036854775808.0"
    );
}

#[test]
fn large_negative_float() {
    assert_eq!(
        eval("-9223372036854775809.0").to_display_string(),
        "-9.223372036854776e+18"
    );
}

// === Bug 2: floor/ceil/round with large floats ===
#[test]
fn floor_large_float_overflow() {
    assert_err("floor(1e300)", &["Integer overflow"]);
}

#[test]
fn ceil_large_float_overflow() {
    assert_err("ceil(1e300)", &["Integer overflow"]);
}

#[test]
fn round_large_float_overflow() {
    assert_err("round(1e300)", &["Integer overflow"]);
}

#[test]
fn floor_large_negative_float_overflow() {
    assert_err("floor(-1e300)", &["Integer overflow"]);
}

#[test]
fn ceil_large_negative_float_overflow() {
    assert_err("ceil(-1e300)", &["Integer overflow"]);
}

// === Bug 4: int_from_float boundary ===
#[test]
fn int_from_float_boundary_overflow() {
    // i64::MAX as f64 rounds up to 9223372036854775808.0 which exceeds i64::MAX
    assert_err("int(9223372036854775808.0)", &["Integer overflow"]);
}

// === Regression tests: exact f64→i64 boundary (2^63) ===
// `v.abs() > i64::MAX as f64` was the wrong guard: i64::MAX rounds up
// to exactly 2^63 in f64, so an input of exactly 2^63 passed the guard
// and the `as i64` cast silently saturated to i64::MAX.

#[test]
fn floor_exactly_2_63_overflow() {
    assert_err("floor(9223372036854775808.0)", &["Integer overflow"]);
}
#[test]
fn ceil_exactly_2_63_overflow() {
    assert_err("ceil(9223372036854775808.0)", &["Integer overflow"]);
}
#[test]
fn round_exactly_2_63_overflow() {
    assert_err("round(9223372036854775808.0)", &["Integer overflow"]);
}
#[test]
fn floordiv_exactly_2_63_overflow() {
    assert_err("9223372036854775808.0 // 1.0", &["Integer overflow"]);
}
#[test]
fn floor_largest_f64_below_2_63_ok() {
    // The largest f64 below 2^63 is exactly representable in i64.
    assert_eq!(
        eval("floor(9223372036854774784.0)").to_display_string(),
        "9223372036854774784"
    );
}
#[test]
fn floor_exactly_i64_min_ok() {
    // -2^63 == i64::MIN is a valid i64; an abs()-based guard would
    // wrongly reject it.
    assert_eq!(
        eval("floor(-9223372036854775808.0)").to_display_string(),
        "-9223372036854775808"
    );
}

// === Regression tests: round() at extreme ndigits ===

#[test]
fn round_float_i64_min_ndigits() {
    // Negating i64::MIN previously panicked in debug builds. Rounding
    // any float at 10^|huge| precision is 0.
    assert_eq!(
        eval("round(1.5, -9223372036854775808)").to_display_string(),
        "0.0"
    );
}
#[test]
fn round_int_i64_min_ndigits() {
    assert_eq!(
        eval("round(15, -9223372036854775808)").to_display_string(),
        "0"
    );
}
#[test]
fn round_float_huge_positive_ndigits_hits_limit() {
    // The formatted output grows with ndigits; a huge precision must
    // trip the operation budget instead of allocating to match.
    assert!(eval_fails("round(1.5, 9223372036854775807)"));
}
#[test]
fn round_moderate_precision_succeeds() {
    // format!("{:.prec$}") panics for precision above u16::MAX; round
    // formats at most an f64's 1074 meaningful fractional digits and
    // zero-fills the rest.
    let v = eval("len(string(round(1.5, 100000)))");
    // "1." + 100000 fractional digits
    assert_eq!(v.to_display_string(), "100002");
}
#[test]
fn round_moderate_precision_value_prefix() {
    let v = eval("string(round(1.5, 100000))[0:6]").to_display_string();
    assert_eq!(v, "1.5000");
}
#[test]
fn round_precision_over_u16_boundary() {
    // Just above the fmt machinery's u16::MAX limit.
    let v = eval("len(string(round(2.25, 65536)))");
    assert_eq!(v.to_display_string(), "65538");
}

// === Regression tests: exact int↔float equality at the 2^63 boundary ===
// `(i as f64) == v` rounds i first, so i64::MAX compared equal to 2^63.
// Python compares exactly: float(2**63) == 2**63 - 1 is False.

#[test]
fn int_float_equality_exact_at_boundary() {
    // 9223372036854775807.0 parses to the f64 2^63, which is NOT i64::MAX.
    assert_eq!(
        eval("9223372036854775807.0 == 9223372036854775807").to_display_string(),
        "false"
    );
    assert_eq!(
        eval("9223372036854775807.0 in [9223372036854775807]").to_display_string(),
        "false"
    );
    // An exactly-representable integer still compares equal.
    assert_eq!(
        eval("9223372036854774784.0 == 9223372036854774784").to_display_string(),
        "true"
    );
}

#[test]
fn range_and_list_containment_agree_at_boundary() {
    // Both containment paths use the same exact equality rule. Range
    // values are bounded to |v| < 2^62, so probe at that scale: the f64
    // nearest to 2^62 - 1 is 2^62, which is NOT a member of a range
    // ending at 2^62 - 1 — the exact rule (not widening) decides this.
    assert_eq!(
        eval("4611686018427387903.0 in range_expr('4611686018427387903')").to_display_string(),
        "false"
    );
    // An exactly-representable value at the same scale is a member.
    assert_eq!(
        eval("4611686018427387392.0 in range_expr('4611686018427387392')").to_display_string(),
        "true"
    );
    assert_eq!(
        eval("4611686018427387903.0 in [4611686018427387903]").to_display_string(),
        "false"
    );
}

// === Regression tests: exact int↔float ordering at the 2^63 boundary ===
// Ordering must agree with the exact equality rule: the widening
// comparison made i64::MAX < 2^63f64 false while equality said they
// differ.

#[test]
fn int_float_ordering_exact_at_boundary() {
    // 9223372036854775807.0 is the f64 2^63, which is > i64::MAX.
    assert_eq!(
        eval("9223372036854775807 < 9223372036854775807.0").to_display_string(),
        "true"
    );
    assert_eq!(
        eval("9223372036854775807.0 > 9223372036854775807").to_display_string(),
        "true"
    );
    // And consistent with equality on exactly-representable values.
    assert_eq!(
        eval("9223372036854774784 <= 9223372036854774784.0").to_display_string(),
        "true"
    );
    assert_eq!(
        eval("9223372036854774784 < 9223372036854774784.0").to_display_string(),
        "false"
    );
}

#[test]
fn int_float_ordering_fractional_ties() {
    assert_eq!(eval("2 < 2.5").to_display_string(), "true");
    assert_eq!(eval("3 > 2.5").to_display_string(), "true");
    assert_eq!(eval("-2 > -2.5").to_display_string(), "true");
    assert_eq!(eval("-3 < -2.5").to_display_string(), "true");
}
