// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_int64_bounds.py

use openjd_expr::{evaluate_expression, ExprValue, SymbolTable};

fn eval(expr: &str) -> ExprValue {
    evaluate_expression(expr, &SymbolTable::new()).unwrap()
}
fn eval_fails(expr: &str) -> bool {
    evaluate_expression(expr, &SymbolTable::new()).is_err()
}
#[allow(dead_code)]
fn eval_err(expr: &str) -> String {
    evaluate_expression(expr, &SymbolTable::new())
        .unwrap_err()
        .message()
}
fn assert_err(expr: &str, expected: &[&str]) {
    let e = evaluate_expression(expr, &SymbolTable::new())
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
    let e = evaluate_expression("9223372036854775808", &SymbolTable::new())
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
    let e = evaluate_expression("-x", &st).unwrap_err().to_string();
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
