// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test_arithmetic.py

use openjd_expr::value::Float64;
use openjd_expr::{ExprValue, ExpressionError, ExpressionErrorKind, ParsedExpression, SymbolTable};

fn eval(expr: &str) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap()
}

fn eval_err(expr: &str) -> ExpressionError {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
}

fn assert_err(expr: &str, expected: &[&str]) {
    let e = eval_err(expr).to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

fn eval_with(expr: &str, st: &SymbolTable) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(st))
        .unwrap()
}

// === TestArithmetic ===

#[test]
fn int_add() {
    assert_eq!(eval("1 + 2").to_display_string(), "3");
}
#[test]
fn int_sub() {
    assert_eq!(eval("10 - 3").to_display_string(), "7");
}
#[test]
fn int_mul() {
    assert_eq!(eval("4 * 5").to_display_string(), "20");
}
#[test]
fn int_floordiv() {
    assert_eq!(eval("10 // 3").to_display_string(), "3");
}
#[test]
fn int_mod() {
    assert_eq!(eval("10 % 3").to_display_string(), "1");
}
#[test]
fn int_neg() {
    assert_eq!(eval("-5").to_display_string(), "-5");
}
#[test]
fn int_pos() {
    assert_eq!(eval("+5").to_display_string(), "5");
}
#[test]
fn int_precedence() {
    assert_eq!(eval("1 + 2 * 3").to_display_string(), "7");
}
#[test]
fn int_parens() {
    assert_eq!(eval("(1 + 2) * 3").to_display_string(), "9");
}

#[test]
fn float_add() {
    assert_eq!(eval("1.5 + 2.5").to_display_string(), "4.0");
}
#[test]
fn float_sub() {
    assert_eq!(eval("5.0 - 1.5").to_display_string(), "3.5");
}
#[test]
fn float_mul() {
    assert_eq!(eval("2.0 * 3.0").to_display_string(), "6.0");
}
#[test]
fn float_div() {
    assert_eq!(eval("7.0 / 2.0").to_display_string(), "3.5");
}

#[test]
fn float_precision_display() {
    assert_eq!(eval("0.1 + 0.2").to_display_string(), "0.30000000000000004");
}

#[test]
fn computed_float_display_pads_negative_exponents() {
    assert_eq!(eval("1e-7 + 0.0").to_display_string(), "1e-07");
    assert_eq!(eval("1e-8 + 0.0").to_display_string(), "1e-08");
    assert_eq!(eval("-1e-7 + 0.0").to_display_string(), "-1e-07");
    assert_eq!(eval("1e16 + 0.0").to_display_string(), "1e+16");
}

#[test]
fn float_passthrough_preserves_original() {
    let mut st = SymbolTable::new();
    st.set(
        "Param.V",
        ExprValue::Float(Float64::with_str(3.5, "3.500".to_string()).unwrap()),
    )
    .unwrap();
    assert_eq!(eval_with("Param.V", &st).to_display_string(), "3.500");
}

#[test]
fn float_passthrough_loses_original_after_operation() {
    let mut st = SymbolTable::new();
    st.set(
        "Param.V",
        ExprValue::Float(Float64::with_str(3.5, "3.500".to_string()).unwrap()),
    )
    .unwrap();
    assert_eq!(eval_with("Param.V + 0", &st).to_display_string(), "3.5");
}

#[test]
fn int_division_returns_float() {
    assert_eq!(eval("10 / 4").to_display_string(), "2.5");
}

#[test]
fn division_by_zero() {
    assert_err("1 / 0", &["Division by zero\n", "  1 / 0\n", "  ~~^~~"]);
}

#[test]
fn modulo_by_zero() {
    assert_err("1 % 0", &["Modulo by zero\n", "  1 % 0\n", "  ~~^~~"]);
}

// === TestFloorDivision ===

#[test]
fn floordiv_int_returns_int() {
    let r = eval("10 // 3");
    assert_eq!(r.to_display_string(), "3");
    assert!(matches!(r, ExprValue::Int(_)));
}

#[test]
fn floordiv_float_returns_int() {
    let r = eval("10.0 // 3.0");
    // Python returns int for floor division
    assert_eq!(r.to_display_string(), "3");
}

#[test]
fn floordiv_float_truncates_positive() {
    assert_eq!(eval("7.5 // 2.0").to_display_string(), "3");
}

#[test]
fn floordiv_float_truncates_negative() {
    assert_eq!(eval("-7.5 // 2.0").to_display_string(), "-4");
}

#[test]
fn floordiv_float_accounts_for_rounded_quotients() {
    assert_eq!(eval("205.0 // 0.1").to_display_string(), "2049");
    assert_eq!(eval("-205.0 // 0.1").to_display_string(), "-2050");
    assert_eq!(eval("205.0 // (-0.1)").to_display_string(), "-2050");
    assert_eq!(eval("-205.0 // (-0.1)").to_display_string(), "2049");
    assert_eq!(
        eval("4.485645604145502e-49 // -4.340298580774782e-64").to_display_string(),
        "-1033487793677267"
    );
    assert_eq!(
        eval("-1.609523844009722e-76 // -5.270510475043238e-92").to_display_string(),
        "3053829134067924"
    );
}

#[test]
fn floordiv_by_zero_int() {
    assert_err(
        "10 // 0",
        &["Division by zero\n", "  10 // 0\n", "  ~~~^~~~"],
    );
}

#[test]
fn floordiv_by_zero_float() {
    assert_err(
        "10.0 // 0.0",
        &["Division by zero\n", "  10.0 // 0.0\n", "  ~~~~~^~~~~~"],
    );
}

// === TestPowerOperator ===

#[test]
fn int_power_positive() {
    let r = eval("2 ** 3");
    assert_eq!(r.to_display_string(), "8");
    assert!(matches!(r, ExprValue::Int(_)));
}

#[test]
fn int_power_negative_exponent() {
    let r = eval("2 ** (-3)");
    assert_eq!(r.to_display_string(), "0.125");
    assert!(matches!(r, ExprValue::Float(_)));
}

#[test]
fn int_power_zero() {
    assert_eq!(eval("5 ** 0").to_display_string(), "1");
}

#[test]
fn float_power() {
    let r = eval("2.0 ** 3.0");
    assert_eq!(r.to_display_string(), "8.0");
    assert!(matches!(r, ExprValue::Float(_)));
}

#[test]
fn float_power_fractional() {
    assert_eq!(eval("4.0 ** 0.5").to_display_string(), "2.0");
}

#[test]
fn int_zero_to_negative_power_error() {
    assert_err(
        "0 ** (-1)",
        &[
            "Cannot raise zero to a negative power\n",
            "  0 ** (-1)\n",
            "  ~~^~~~~~",
        ],
    );
}

#[test]
fn float_negative_base_fractional_exponent_error() {
    assert_err(
        "(-2.0) ** 0.5",
        &[
            "Cannot compute -2 ** 0.5 (would produce complex number)\n",
            "  (-2.0) ** 0.5\n",
            "  ~~~~~~~^~~~~~",
        ],
    );
}

#[test]
fn float_overflow_error() {
    assert_err(
        "2.0 ** 1024.0",
        &[
            "Overflow computing 2 ** 1024 (result too large for float)\n",
            "  2.0 ** 1024.0\n",
            "  ~~~~^~~~~~~~~",
        ],
    );
}

#[test]
fn float_zero_to_negative_power_error() {
    assert_err(
        "0.0 ** (-1.0)",
        &[
            "Cannot raise zero to a negative power\n",
            "  0.0 ** (-1.0)\n",
            "  ~~~~^~~~~~~~~",
        ],
    );
}

#[test]
fn int_power_zero_base_large_exp() {
    assert_eq!(eval("0 ** 4294967296").to_display_string(), "0");
}

#[test]
fn int_power_zero_base_large_exp_odd() {
    assert_eq!(eval("0 ** 4294967297").to_display_string(), "0");
}

#[test]
fn int_power_one_base_large_exp() {
    assert_eq!(eval("1 ** 4294967296").to_display_string(), "1");
}

#[test]
fn int_power_neg_one_base_large_even_exp() {
    assert_eq!(eval("(-1) ** 4294967296").to_display_string(), "1");
}

#[test]
fn int_power_neg_one_base_large_odd_exp() {
    assert_eq!(eval("(-1) ** 4294967297").to_display_string(), "-1");
}

// === TestMathFunctions ===

#[test]
fn abs_negative() {
    assert_eq!(eval("abs(-5)").to_display_string(), "5");
}
#[test]
fn abs_positive() {
    assert_eq!(eval("abs(5)").to_display_string(), "5");
}
#[test]
fn min_two() {
    assert_eq!(eval("min(3, 7)").to_display_string(), "3");
}
#[test]
fn max_two() {
    assert_eq!(eval("max(3, 7)").to_display_string(), "7");
}
#[test]
fn floor_float() {
    assert_eq!(eval("floor(3.7)").to_display_string(), "3");
}
#[test]
fn ceil_float() {
    assert_eq!(eval("ceil(3.2)").to_display_string(), "4");
}
#[test]
fn round_half() {
    assert_eq!(eval("round(3.5)").to_display_string(), "4");
}

// === TestRoundToEven ===

#[test]
fn round_half_to_even_0_5() {
    assert_eq!(eval("round(0.5)").to_display_string(), "0");
}
#[test]
fn round_half_to_even_1_5() {
    assert_eq!(eval("round(1.5)").to_display_string(), "2");
}
#[test]
fn round_half_to_even_2_5() {
    assert_eq!(eval("round(2.5)").to_display_string(), "2");
}
#[test]
fn round_half_to_even_3_5() {
    assert_eq!(eval("round(3.5)").to_display_string(), "4");
}
#[test]
fn round_half_to_even_4_5() {
    assert_eq!(eval("round(4.5)").to_display_string(), "4");
}
#[test]
fn round_half_to_even_neg_0_5() {
    assert_eq!(eval("round(-0.5)").to_display_string(), "0");
}
#[test]
fn round_half_to_even_neg_1_5() {
    assert_eq!(eval("round(-1.5)").to_display_string(), "-2");
}
#[test]
fn round_half_to_even_neg_2_5() {
    assert_eq!(eval("round(-2.5)").to_display_string(), "-2");
}

#[test]
fn round_ndigits_0_125() {
    assert_eq!(eval("round(0.125, 2)").to_display_string(), "0.12");
}
#[test]
fn round_ndigits_0_375() {
    assert_eq!(eval("round(0.375, 2)").to_display_string(), "0.38");
}
#[test]
fn round_ndigits_0_25() {
    assert_eq!(eval("round(0.25, 1)").to_display_string(), "0.2");
}
#[test]
fn round_ndigits_0_75() {
    assert_eq!(eval("round(0.75, 1)").to_display_string(), "0.8");
}
// round(float, ndigits) returns int when ndigits <= 0 (spec §2.2.2,
// "returns `int` when `ndigits` ≤ 0, `float` when `ndigits` > 0").
#[test]
fn round_ndigits_2_5_0() {
    assert!(matches!(eval("round(2.5, 0)"), ExprValue::Int(2)));
}
#[test]
fn round_ndigits_3_5_0() {
    assert!(matches!(eval("round(3.5, 0)"), ExprValue::Int(4)));
}
#[test]
fn round_ndigits_zero_3_7() {
    assert!(matches!(eval("round(3.7, 0)"), ExprValue::Int(4)));
    assert_eq!(eval("round(3.7, 0)").to_display_string(), "4");
}

#[test]
fn round_ndigits_negative_returns_int() {
    // The spec's worked example: round(1234.5, -1) returns 1230.
    assert!(matches!(eval("round(1234.5, -1)"), ExprValue::Int(1230)));
    assert_eq!(eval("round(1234.5, -1)").to_display_string(), "1230");
    assert!(matches!(eval("round(-1234.5, -1)"), ExprValue::Int(-1230)));
    assert!(matches!(eval("round(1234.5, -3)"), ExprValue::Int(1000)));
    assert!(matches!(eval("round(0.4, -1)"), ExprValue::Int(0)));
}

#[test]
fn round_ndigits_negative_half_even() {
    // 1250 / 100 = 12.5 exactly, which rounds to the even 12.
    assert!(matches!(eval("round(1250.0, -2)"), ExprValue::Int(1200)));
    assert!(matches!(eval("round(1350.0, -2)"), ExprValue::Int(1400)));
    assert!(matches!(eval("round(-1250.0, -2)"), ExprValue::Int(-1200)));
}

#[test]
fn round_ndigits_non_positive_negative_zero_is_zero() {
    // -0.4 rounds to -0.0; the int result has no sign.
    assert!(matches!(eval("round(-0.4, 0)"), ExprValue::Int(0)));
    assert_eq!(eval("round(-0.4, 0)").to_display_string(), "0");
    assert!(matches!(eval("round(-4.0, -1)"), ExprValue::Int(0)));
}

#[test]
fn round_ndigits_non_positive_large_values_in_range() {
    // 9e15 is integral and inside the i64 range: it stays exact.
    assert!(matches!(
        eval("round(9e15, 0)"),
        ExprValue::Int(9_000_000_000_000_000)
    ));
    assert!(matches!(
        eval("round(1e17, -1)"),
        ExprValue::Int(100_000_000_000_000_000)
    ));
    // The largest f64 below 2^63 is in range.
    assert!(matches!(
        eval("round(9223372036854774784.0, 0)"),
        ExprValue::Int(9_223_372_036_854_774_784)
    ));
    assert!(matches!(
        eval("round(-9223372036854775808.0, 0)"),
        ExprValue::Int(i64::MIN)
    ));
}

#[test]
fn round_ndigits_zero_out_of_range_overflows() {
    assert_err(
        "round(1e300, 0)",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  round(1e300, 0)\n",
            "  ^~~~~~~~~~~~~~~",
        ],
    );
    assert_err(
        "round(-1e300, 0)",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  round(-1e300, 0)\n",
            "  ^~~~~~~~~~~~~~~~",
        ],
    );
    // 2^63 is one past i64::MAX.
    assert_err(
        "round(9223372036854775808.0, 0)",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  round(9223372036854775808.0, 0)\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn round_ndigits_negative_out_of_range_overflows() {
    assert_err(
        "round(1e300, -1)",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  round(1e300, -1)\n",
            "  ^~~~~~~~~~~~~~~~",
        ],
    );
    assert_err(
        "round(-1e300, -5)",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  round(-1e300, -5)\n",
            "  ^~~~~~~~~~~~~~~~~",
        ],
    );
    // Rounding 9e18 up to a multiple of 10^19 leaves the range.
    assert_err(
        "round(9e18, -19)",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  round(9e18, -19)\n",
            "  ^~~~~~~~~~~~~~~~",
        ],
    );
    // Near the top of the f64 range the result is far outside the range.
    assert_err(
        "round(1.7e308, -308)",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  round(1.7e308, -308)\n",
            "  ^~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn round_ndigits_negative_beyond_magnitude_is_zero() {
    // 5e18 / 10^19 = 0.5, which rounds to the even 0.
    assert!(matches!(eval("round(5e18, -19)"), ExprValue::Int(0)));
    assert!(matches!(eval("round(1e300, -301)"), ExprValue::Int(0)));
    assert!(matches!(eval("round(1.7e308, -309)"), ExprValue::Int(0)));
    assert!(matches!(eval("round(-1.7e308, -400)"), ExprValue::Int(0)));
}

#[test]
fn round_ndigits_positive_still_float() {
    assert!(matches!(eval("round(2.5, 1)"), ExprValue::Float(_)));
    assert_eq!(eval("round(3.5, 2)").to_display_string(), "3.50");
    assert_eq!(eval("round(0.125, 2)").to_display_string(), "0.12");
}

fn assert_overflow(expr: &str) {
    let e = eval_err(expr);
    assert!(
        matches!(e.kind(), ExpressionErrorKind::IntegerOverflow),
        "{expr}: {e}"
    );
}

// Exact rounding (§2.2.2): half-to-even on the exact value of the input, as
// CPython does, with no intermediate f64 scaling. Expected values are
// CPython's, from `round(x, n)` for ints and `round(Fraction(x), n)` for
// floats; test_round_cpython.rs checks a larger generated sample.

#[test]
fn round_int_negative_ndigits_is_exact() {
    // Scaling through f64 gave 3550693609030000128.
    assert!(matches!(
        eval("round(3550693609029500000, -6)"),
        ExprValue::Int(3_550_693_609_030_000_000)
    ));
    assert!(matches!(
        eval("round(3550693609028500000, -6)"),
        ExprValue::Int(3_550_693_609_028_000_000)
    ));
    assert!(matches!(eval("round(-155, -1)"), ExprValue::Int(-160)));
    assert!(matches!(eval("round(-250, -2)"), ExprValue::Int(-200)));
}

#[test]
fn round_int_negative_ndigits_i64_bounds() {
    // i64::MAX ends in 7, so rounding to tens goes up and out of range;
    // ...804 rounds down and stays in range.
    assert_overflow("round(9223372036854775807, -1)");
    assert!(matches!(
        eval("round(9223372036854775804, -1)"),
        ExprValue::Int(9_223_372_036_854_775_800)
    ));
    assert!(matches!(
        eval("round(9223372036854775807, -2)"),
        ExprValue::Int(9_223_372_036_854_775_800)
    ));
    assert_overflow("round(-9223372036854775808, -1)");
    assert!(matches!(
        eval("round(-9223372036854775808, -2)"),
        ExprValue::Int(-9_223_372_036_854_775_800)
    ));
    // k = 18, 19 and 20.
    assert!(matches!(
        eval("round(9223372036854775807, -18)"),
        ExprValue::Int(9_000_000_000_000_000_000)
    ));
    assert!(matches!(
        eval("round(-9223372036854775808, -18)"),
        ExprValue::Int(-9_000_000_000_000_000_000)
    ));
    assert_overflow("round(9223372036854775807, -19)");
    assert_overflow("round(-9223372036854775808, -19)");
    assert!(matches!(
        eval("round(4999999999999999999, -19)"),
        ExprValue::Int(0)
    ));
    // A tie at 10^19 / 2 goes to the even 0; one more goes to 10^19.
    assert!(matches!(
        eval("round(5000000000000000000, -19)"),
        ExprValue::Int(0)
    ));
    assert!(matches!(
        eval("round(-5000000000000000000, -19)"),
        ExprValue::Int(0)
    ));
    assert_overflow("round(5000000000000000001, -19)");
    assert_overflow("round(-5000000000000000001, -19)");
    assert!(matches!(
        eval("round(9223372036854775807, -20)"),
        ExprValue::Int(0)
    ));
    assert!(matches!(
        eval("round(-9223372036854775808, -20)"),
        ExprValue::Int(0)
    ));
}

#[test]
fn round_float_negative_ndigits_is_exact() {
    // 3.5506936090295e18 is stored as 3550693609029499904, below the tie.
    assert!(matches!(
        eval("round(3.5506936090295e18, -6)"),
        ExprValue::Int(3_550_693_609_029_000_000)
    ));
    // The f64 can be above 2^63 while the result is in range.
    assert!(matches!(
        eval("round(9.3e18, -18)"),
        ExprValue::Int(9_000_000_000_000_000_000)
    ));
    assert!(matches!(
        eval("round(-9.3e18, -18)"),
        ExprValue::Int(-9_000_000_000_000_000_000)
    ));
    assert!(matches!(
        eval("round(9.2e18, -17)"),
        ExprValue::Int(9_200_000_000_000_000_000)
    ));
    // 9.5e18 is exact, and the tie goes to the even 10·10^18.
    assert_overflow("round(9.5e18, -18)");
    assert_overflow("round(-9.5e18, -18)");
}

#[test]
fn round_float_non_positive_ndigits_i64_bounds() {
    // The largest f64 below 2^63 is 9223372036854774784.
    assert!(matches!(
        eval("round(9223372036854774784.0, -3)"),
        ExprValue::Int(9_223_372_036_854_775_000)
    ));
    assert!(matches!(
        eval("round(9223372036854774784.0, -15)"),
        ExprValue::Int(9_223_000_000_000_000_000)
    ));
    // Rounding up crosses 2^63.
    assert_overflow("round(9223372036854774784.0, -10)");
    assert_overflow("round(9.2e18, -19)");
    // 2^63 itself is out of range at any k that leaves it nonzero; -2^63
    // is in range at k = 0, but -2^63 rounded to tens is
    // -9223372036854775810, which is not. (CPython returns the float
    // -2.0**63 here, the nearest double to that value.)
    assert_overflow("round(9223372036854775808.0)");
    assert_overflow("round(9223372036854775808.0, -1)");
    assert!(matches!(
        eval("round(-9223372036854775808.0)"),
        ExprValue::Int(i64::MIN)
    ));
    assert_overflow("round(-9223372036854775808.0, -1)");
    assert!(matches!(
        eval("round(-9223372036854775808.0, -18)"),
        ExprValue::Int(-9_000_000_000_000_000_000)
    ));
    // k = 19 and 20.
    assert!(matches!(eval("round(4.9e18, -19)"), ExprValue::Int(0)));
    assert_overflow("round(5.000000000000001e18, -19)");
    assert!(matches!(eval("round(4e19, -20)"), ExprValue::Int(0)));
    assert!(matches!(eval("round(5e19, -20)"), ExprValue::Int(0)));
    assert_overflow("round(6e19, -20)");
}

#[test]
fn round_float_beyond_2_pow_127() {
    // Above 2^127 the result is 0 or out of range, decided exactly:
    // 5e300 is stored above 5·10^300, so it rounds to 10^301.
    assert_overflow("round(5e300, -301)");
    assert_overflow("round(-5e300, -301)");
    assert!(matches!(eval("round(4e300, -301)"), ExprValue::Int(0)));
    assert!(matches!(eval("round(1e300, -301)"), ExprValue::Int(0)));
    // 2^127 = 1.7e38 rounds to 2·10^38 at k = 38 and to 0 at k = 39.
    assert_overflow("round(170141183460469231731687303715884105728.0, -38)");
    assert!(matches!(
        eval("round(170141183460469231731687303715884105728.0, -39)"),
        ExprValue::Int(0)
    ));
    // The largest f64 below 2^127.
    assert_overflow("round(170141183460469212842221372237303250944.0, -38)");
    assert!(matches!(
        eval("round(170141183460469212842221372237303250944.0, -39)"),
        ExprValue::Int(0)
    ));
    assert!(matches!(eval("round(4.9e307, -308)"), ExprValue::Int(0)));
    assert_overflow("round(1.7976931348623157e308, -308)");
}

#[test]
fn round_float_positive_ndigits_rounds_once() {
    // 2.675 is stored as 2.67499999999999982236431605997495353221893310546875.
    assert_eq!(eval("round(2.675, 2)").to_display_string(), "2.67");
    assert_eq!(eval("round(-2.675, 2)").to_display_string(), "-2.67");
    // 1.05 is stored just above 1.05.
    assert_eq!(eval("round(1.05, 1)").to_display_string(), "1.1");
    assert_eq!(eval("round(0.685, 2)").to_display_string(), "0.69");
    assert_eq!(
        eval("round(123456.785, 2)").to_display_string(),
        "123456.79"
    );
    // Exact binary ties still go to even.
    assert_eq!(eval("round(0.125, 2)").to_display_string(), "0.12");
    assert_eq!(eval("round(0.375, 2)").to_display_string(), "0.38");
    // The result is the nearest f64 to the rounded decimal.
    let ExprValue::Float(v) = eval("round(2.675, 2)") else {
        panic!("round(2.675, 2) is a float");
    };
    assert_eq!(v.value(), 2.67);
}

#[test]
fn round_float_positive_ndigits_extremes_stay_finite() {
    let ExprValue::Float(v) = eval("round(1e308, 2)") else {
        panic!("round(1e308, 2) is a float");
    };
    assert_eq!(v.value(), 1e308);
    assert!(eval("round(1e308, 2)").to_display_string().ends_with(".00"));
    let ExprValue::Float(v) = eval("round(1.7976931348623157e308, 1)") else {
        panic!("round(f64::MAX, 1) is a float");
    };
    assert_eq!(v.value(), f64::MAX);
    // The smallest subnormal rounds to 0 at 323 places and keeps its value
    // at 324 and at 1074, where its expansion ends.
    let ExprValue::Float(v) = eval("round(5e-324, 323)") else {
        panic!("a float");
    };
    assert_eq!(v.value(), 0.0);
    for n in [324, 1074, 1100] {
        let ExprValue::Float(v) = eval(&format!("round(5e-324, {n})")) else {
            panic!("a float");
        };
        assert_eq!(v.value(), 5e-324, "n = {n}");
    }
}

#[test]
fn round_int_ndigits_42_0() {
    assert_eq!(eval("round(42, 0)").to_display_string(), "42");
}
#[test]
fn round_int_ndigits_42_2() {
    assert_eq!(eval("round(42, 2)").to_display_string(), "42");
}
#[test]
fn round_int_ndigits_neg7_1() {
    assert_eq!(eval("round(-7, 1)").to_display_string(), "-7");
}
#[test]
fn round_int_ndigits_11_neg1() {
    assert_eq!(eval("round(11, -1)").to_display_string(), "10");
}
#[test]
fn round_int_ndigits_155_neg1() {
    assert_eq!(eval("round(155, -1)").to_display_string(), "160");
}
#[test]
fn round_int_ndigits_155_neg2() {
    assert_eq!(eval("round(155, -2)").to_display_string(), "200");
}
#[test]
fn round_int_ndigits_150_neg2() {
    assert_eq!(eval("round(150, -2)").to_display_string(), "200");
}
#[test]
fn round_int_ndigits_250_neg2() {
    assert_eq!(eval("round(250, -2)").to_display_string(), "200");
}

// === TestFloorCeilReturnType ===

#[test]
fn floor_float_returns_int() {
    let r = eval("floor(3.7)");
    assert!(matches!(r, ExprValue::Int(3)));
}

#[test]
fn floor_int_returns_int() {
    let r = eval("floor(3)");
    assert!(matches!(r, ExprValue::Int(3)));
}

#[test]
fn ceil_float_returns_int() {
    let r = eval("ceil(3.2)");
    assert!(matches!(r, ExprValue::Int(4)));
}

#[test]
fn ceil_int_returns_int() {
    let r = eval("ceil(3)");
    assert!(matches!(r, ExprValue::Int(3)));
}

// === TestSum ===

#[test]
fn sum_int_list() {
    assert_eq!(eval("sum([1, 2, 3])").to_display_string(), "6");
}
#[test]
fn sum_float_list() {
    assert_eq!(eval("sum([1.5, 2.5])").to_display_string(), "4.0");
}
#[test]
fn sum_empty_list() {
    let r = eval("sum([])");
    assert_eq!(r.to_display_string(), "0");
    assert!(matches!(r, ExprValue::Int(0)));
}

// === TestFailFunction ===

#[test]
fn fail_raises_error() {
    assert_err(
        "fail(\"custom error message\")",
        &[
            "custom error message\n",
            "  fail(\"custom error message\")\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn fail_short_circuit_or_true() {
    assert_eq!(
        eval("true or fail(\"should not see\")").to_display_string(),
        "true"
    );
}

#[test]
fn fail_short_circuit_or_false() {
    assert_err(
        "false or fail(\"validation failed\")",
        &[
            "validation failed\n",
            "  false or fail(\"validation failed\")\n",
            "           ^~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn fail_short_circuit_and_false() {
    assert_eq!(
        eval("false and fail(\"should not see\")").to_display_string(),
        "false"
    );
}

#[test]
fn fail_short_circuit_and_true() {
    assert_err(
        "true and fail(\"validation failed\")",
        &[
            "validation failed\n",
            "  true and fail(\"validation failed\")\n",
            "           ^~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn fail_if_else_true_condition() {
    assert_eq!(
        eval("1.5 if true else fail(\"should not see\")").to_display_string(),
        "1.5"
    );
}

#[test]
fn fail_if_else_false_condition() {
    assert_err(
        "1 if false else fail(\"condition was false\")",
        &[
            "condition was false\n",
            "  1 if false else fail(\"condition was false\")\n",
            "                  ^~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn fail_validation_pattern() {
    assert_eq!(
        eval("5 > 0 or fail(\"must be positive\")").to_display_string(),
        "true"
    );
    assert_err(
        "-1 > 0 or fail(\"must be positive\")",
        &[
            "must be positive\n",
            "  -1 > 0 or fail(\"must be positive\")\n",
            "            ^~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

// === TestFloatSanitization ===

#[test]
fn overflow_to_infinity() {
    assert_err(
        "1e300 * 1e300",
        &[
            "Float operation produced infinity\n",
            "  1e300 * 1e300\n",
            "  ~~~~~~^~~~~~~",
        ],
    );
}

#[test]
fn literal_infinity() {
    assert_err(
        "1e3000",
        &[
            "Float operation produced infinity\n",
            "  1e3000\n",
            "  ^~~~~~",
        ],
    );
}

#[test]
fn float_from_inf_string() {
    assert_err(
        "float('inf')",
        &[
            "Cannot convert to float: infinity\n",
            "  float('inf')\n",
            "  ^~~~~~~~~~~~",
        ],
    );
}

#[test]
fn float_from_nan_string() {
    assert_err(
        "float('nan')",
        &[
            "Cannot convert to float: NaN\n",
            "  float('nan')\n",
            "  ^~~~~~~~~~~~",
        ],
    );
}

#[test]
fn negative_zero_normalized() {
    assert_eq!(eval("-0.0").to_display_string(), "0.0");
}

/// `round(v, n)` builds its display text with `{:.n$}`, so it renders to the
/// requested number of places: `round(1.5, 2)` is `1.50`. Zero is not an
/// exception to that, and the sign it does not have must not cost it the places.
#[test]
fn round_to_places_keeps_them_at_zero() {
    assert_eq!(eval("round(1.5, 2)").to_display_string(), "1.50");
    assert_eq!(eval("round(0.0004, 2)").to_display_string(), "0.00");
    assert_eq!(eval("round(-0.0004, 2)").to_display_string(), "0.00");
    assert_eq!(eval("round(0.0, 3)").to_display_string(), "0.000");
}

/// A signed zero loses its sign but keeps everything else, and text that reaches
/// zero only by underflow is not the value's rendering at all.
#[test]
fn with_str_zero_text_matches_the_value() {
    use openjd_expr::value::Float64;
    let disp = |v: f64, s: &str| {
        ExprValue::Float(Float64::with_str(v, s.to_string()).unwrap()).to_display_string()
    };
    assert_eq!(disp(0.0, "0.00"), "0.00");
    assert_eq!(disp(-0.0, "-0.0"), "0.0");
    assert_eq!(disp(-0.0, "-0.00"), "0.00");
    // An all-zero mantissa spells zero whatever the exponent, so the notation
    // survives here just as `1E+2` does for a non-zero value.
    assert_eq!(disp(0.0, "0e5"), "0e5");
    assert_eq!(disp(-0.0, "-0.0E+2"), "0.0E+2");
    // Underflow is not zero being spelled: the digits are the author's request,
    // and dropping them would render a different number.
    assert_eq!(disp(0.0, "1e-400"), "1e-400");
    assert_eq!(
        disp(0.0, "0.0000000000000000000000000000000001"),
        "0.0000000000000000000000000000000001"
    );
}

#[test]
fn negative_zero_from_arithmetic() {
    assert_eq!(eval("-1.0 * 0.0").to_display_string(), "0.0");
}

#[test]
fn abs_negative_zero() {
    assert_eq!(eval("abs(-0.0)").to_display_string(), "0.0");
}

#[test]
fn negative_zero_equals_zero() {
    assert_eq!(eval("-0.0 == 0.0").to_display_string(), "true");
}

#[test]
fn negative_zero_bit_normalized() {
    // Python normalizes -0.0 to 0.0 at the value level (copysign check).
    // Verify the Rust port does the same — the underlying f64 must be +0.0.
    if let ExprValue::Float(f) = eval("-0.0") {
        assert!(
            f.value().is_sign_positive(),
            "expected +0.0 but got -0.0 (bit pattern not normalized)"
        );
    } else {
        panic!("expected Float");
    }
    // Also check arithmetic that produces -0.0
    if let ExprValue::Float(f) = eval("-1.0 * 0.0") {
        assert!(
            f.value().is_sign_positive(),
            "expected +0.0 from -1.0 * 0.0"
        );
    } else {
        panic!("expected Float");
    }
}

// === Missing tests ported from Python ===

// TestPowerOperator - missing cases

#[test]
fn int_zero_to_negative_power_error_large() {
    assert_err(
        "0 ** (-5)",
        &[
            "Cannot raise zero to a negative power\n",
            "  0 ** (-5)\n",
            "  ~~^~~~~~",
        ],
    );
}

#[test]
fn float_negative_one_sqrt_error() {
    assert_err(
        "(-1.0) ** 0.5",
        &[
            "Cannot compute -1 ** 0.5 (would produce complex number)\n",
            "  (-1.0) ** 0.5\n",
            "  ~~~~~~~^~~~~~",
        ],
    );
}

// TestRoundToEven - missing ndigits cases

#[test]
fn round_ndigits_0_625() {
    assert_eq!(eval("round(0.625, 2)").to_display_string(), "0.62");
}
#[test]
fn round_ndigits_0_875() {
    assert_eq!(eval("round(0.875, 2)").to_display_string(), "0.88");
}
#[test]
fn round_ndigits_neg_0_125() {
    assert_eq!(eval("round(-0.125, 2)").to_display_string(), "-0.12");
}
#[test]
fn round_ndigits_neg_0_375() {
    assert_eq!(eval("round(-0.375, 2)").to_display_string(), "-0.38");
}
#[test]
fn round_ndigits_neg_0_25() {
    assert_eq!(eval("round(-0.25, 1)").to_display_string(), "-0.2");
}
#[test]
fn round_ndigits_neg_0_75() {
    assert_eq!(eval("round(-0.75, 1)").to_display_string(), "-0.8");
}
#[test]
fn round_ndigits_neg_2_5_0() {
    assert!(matches!(eval("round(-2.5, 0)"), ExprValue::Int(-2)));
}
#[test]
fn round_ndigits_neg_3_5_0() {
    assert!(matches!(eval("round(-3.5, 0)"), ExprValue::Int(-4)));
}

// TestFailFunction - missing cases

#[test]
fn fail_in_then_true_condition() {
    assert_err(
        "fail(\"condition was true\") if true else 1",
        &[
            "condition was true\n",
            "  fail(\"condition was true\") if true else 1\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn fail_in_then_false_condition() {
    assert_eq!(
        eval("fail(\"should not see\") if false else 2.5").to_display_string(),
        "2.5"
    );
}

#[test]
fn fail_if_else_various_types() {
    assert_eq!(
        eval("42 if true else fail(\"error\")").to_display_string(),
        "42"
    );
    assert_eq!(
        eval("\"hello\" if true else fail(\"error\")").to_display_string(),
        "hello"
    );
}

#[test]
fn fail_both_branches() {
    assert_err(
        "fail(\"then\") if true else fail(\"else\")",
        &[
            "then\n",
            "  fail(\"then\") if true else fail(\"else\")\n",
            "  ^~~~~~~~~~~~",
        ],
    );
}

// === TestFlooredDivisionAndModulo ===
// Python uses floored division (toward -∞), not truncated division (toward 0).
// These tests verify correct behavior with negative operands.

#[test]
fn floordiv_int_negative_dividend() {
    // Python: -7 // 2 == -4 (floored), not -3 (truncated)
    assert_eq!(eval("-7 // 2").to_display_string(), "-4");
}

#[test]
fn floordiv_int_negative_divisor() {
    // Python: 7 // -2 == -4 (floored), not -3 (truncated)
    assert_eq!(eval("7 // (-2)").to_display_string(), "-4");
}

#[test]
fn floordiv_int_both_negative() {
    // Python: -7 // -2 == 3 (floored), same as truncated here
    assert_eq!(eval("-7 // (-2)").to_display_string(), "3");
}

#[test]
fn floordiv_int_exact_negative() {
    // Exact division: no difference between floored and truncated
    assert_eq!(eval("-6 // 2").to_display_string(), "-3");
}

#[test]
fn floordiv_int_negative_one() {
    assert_eq!(eval("-1 // 2").to_display_string(), "-1");
}

#[test]
fn mod_int_negative_dividend() {
    // Python: -7 % 2 == 1 (floored), not -1 (truncated)
    assert_eq!(eval("-7 % 2").to_display_string(), "1");
}

#[test]
fn mod_int_negative_divisor() {
    // Python: 7 % -2 == -1 (floored), not 1 (truncated)
    assert_eq!(eval("7 % (-2)").to_display_string(), "-1");
}

#[test]
fn mod_int_both_negative() {
    // Python: -7 % -2 == -1 (floored), same as truncated here
    assert_eq!(eval("-7 % (-2)").to_display_string(), "-1");
}

#[test]
fn mod_int_exact_negative() {
    // Exact division: remainder is 0 regardless
    assert_eq!(eval("-6 % 2").to_display_string(), "0");
}

#[test]
fn mod_int_negative_one() {
    // Python: -1 % 2 == 1
    assert_eq!(eval("-1 % 2").to_display_string(), "1");
}

#[test]
fn mod_float_negative_dividend() {
    // Python: -7.0 % 2.0 == 1.0 (floored), not -1.0 (truncated)
    assert_eq!(eval("-7.0 % 2.0").to_display_string(), "1.0");
}

#[test]
fn mod_float_negative_divisor() {
    // Python: 7.0 % -2.0 == -1.0 (floored), not 1.0 (truncated)
    assert_eq!(eval("7.0 % (-2.0)").to_display_string(), "-1.0");
}

#[test]
fn mod_float_both_negative() {
    assert_eq!(eval("-7.0 % (-2.0)").to_display_string(), "-1.0");
}

#[test]
fn mod_float_exact_negative() {
    assert_eq!(eval("-6.0 % 2.0").to_display_string(), "0.0");
}

#[test]
fn mod_float_small_negative() {
    // Python: -0.5 % 1.0 == 0.5
    assert_eq!(eval("-0.5 % 1.0").to_display_string(), "0.5");
}

#[test]
fn mod_float_large_quotient_preserves_remainder() {
    assert_eq!(
        eval("9.2e18 % 0.1").to_display_string(),
        "0.09740867242801976"
    );
    assert_eq!(
        eval("-9.2e18 % 0.1").to_display_string(),
        "0.0025913275719802453"
    );
    assert_eq!(
        eval("9.2e18 % (-0.1)").to_display_string(),
        "-0.0025913275719802453"
    );
    assert_eq!(
        eval("-9.2e18 % (-0.1)").to_display_string(),
        "-0.09740867242801976"
    );
}

// Floored division/modulo invariant: a == (a // b) * b + (a % b)
#[test]
fn floordiv_mod_invariant_negative() {
    // -7 == (-7 // 2) * 2 + (-7 % 2) == -4 * 2 + 1 == -7
    assert_eq!(eval("(-7 // 2) * 2 + (-7 % 2)").to_display_string(), "-7");
}

// Float % edge cases using nextafter boundaries
#[test]
fn mod_float_nextafter_just_below_divisor() {
    // A value just below 2.0: the modulo should be that value itself
    // f64::from_bits(2.0_f64.to_bits() - 1) is the largest f64 < 2.0
    let just_below = f64::from_bits(2.0_f64.to_bits() - 1);
    let expr = format!("{just_below} % 2.0");
    let result = eval(&expr);
    // Just below 2.0 mod 2.0 should equal itself (positive, < 2.0)
    let val: f64 = match result {
        ExprValue::Float(f) => f.value(),
        _ => panic!("expected float"),
    };
    assert!(
        val > 0.0 && val < 2.0,
        "expected positive result < 2.0, got {val}"
    );
}

#[test]
fn mod_float_nextafter_just_above_neg_divisor() {
    // A value just above -2.0: mod 2.0 should be positive (floored semantics)
    let just_above_neg2 = f64::from_bits((-2.0_f64).to_bits() - 1); // toward zero
    let expr = format!("{just_above_neg2} % 2.0");
    let result = eval(&expr);
    let val: f64 = match result {
        ExprValue::Float(f) => f.value(),
        _ => panic!("expected float"),
    };
    assert!(
        (0.0..2.0).contains(&val),
        "floored mod must be in [0, 2.0), got {val}"
    );
}

#[test]
fn mod_float_result_sign_matches_divisor_positive() {
    // Floored modulo: result sign always matches divisor sign
    let val: f64 = match eval("-3.5 % 2.0") {
        ExprValue::Float(f) => f.value(),
        _ => panic!("expected float"),
    };
    assert!(
        val >= 0.0,
        "floored mod with positive divisor must be >= 0, got {val}"
    );
}

#[test]
fn mod_float_result_sign_matches_divisor_negative() {
    let val: f64 = match eval("3.5 % (-2.0)") {
        ExprValue::Float(f) => f.value(),
        _ => panic!("expected float"),
    };
    assert!(
        val <= 0.0,
        "floored mod with negative divisor must be <= 0, got {val}"
    );
}

// === Bug 1: sum_list integer overflow ===
#[test]
fn sum_int_list_overflow() {
    assert_err("sum([9223372036854775807, 1])", &["Integer overflow"]);
}

// === Bug 3: floordiv_float large result overflow ===
#[test]
fn floordiv_float_large_result_overflow() {
    assert_err("1e300 // 1.0", &["Integer overflow"]);
}

#[test]
fn string_mul_length_overflow() {
    // 4 bytes * 2^62 == 2^64 wraps to 0 with unchecked math, defeating
    // the operation and memory budgets before str::repeat panicked.
    assert_err(
        "'abcd' * 4611686018427387904",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  'abcd' * 4611686018427387904\n",
            "  ~~~~~~~^~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn list_mul_length_overflow() {
    assert_err(
        "[1, 2, 3, 4] * 4611686018427387904",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  [1, 2, 3, 4] * 4611686018427387904\n",
            "  ~~~~~~~~~~~~~^~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn empty_list_mul_huge_is_empty() {
    assert_eq!(eval("[] * 4611686018427387904").to_display_string(), "[]");
}
