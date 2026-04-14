// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_arithmetic.py

use openjd_expr::value::Float64;
use openjd_expr::{evaluate_expression, ExprValue, ExpressionError, SymbolTable};

fn eval(expr: &str) -> ExprValue {
    evaluate_expression(expr, &SymbolTable::new()).unwrap()
}

fn eval_err(expr: &str) -> ExpressionError {
    evaluate_expression(expr, &SymbolTable::new()).unwrap_err()
}

fn assert_err(expr: &str, expected: &[&str]) {
    let e = eval_err(expr).to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

fn eval_with(expr: &str, st: &SymbolTable) -> ExprValue {
    evaluate_expression(expr, st).unwrap()
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
#[test]
fn round_ndigits_2_5_0() {
    assert_eq!(eval("round(2.5, 0)").to_display_string(), "2.0");
}
#[test]
fn round_ndigits_3_5_0() {
    assert_eq!(eval("round(3.5, 0)").to_display_string(), "4.0");
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
    assert_eq!(eval("round(-2.5, 0)").to_display_string(), "-2.0");
}
#[test]
fn round_ndigits_neg_3_5_0() {
    assert_eq!(eval("round(-3.5, 0)").to_display_string(), "-4.0");
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
