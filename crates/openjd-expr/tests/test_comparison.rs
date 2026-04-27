// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test_comparison.py

use openjd_expr::{ExprValue, ParsedExpression, PathFormat, SymbolTable};

#[allow(dead_code)]
fn eval(expr: &str) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap()
}
fn eval_with(expr: &str, st: &SymbolTable) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(st))
        .unwrap()
}

fn eval_posix(expr: &str, st: &SymbolTable) -> ExprValue {
    let parsed = ParsedExpression::new(expr).unwrap();
    let symtabs = [st];
    parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap()
}

#[allow(dead_code)]
fn eval_err(expr: &str) -> String {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .message()
}

#[allow(dead_code)]
fn eval_err_with(expr: &str, st: &SymbolTable) -> String {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(st))
        .unwrap_err()
        .message()
}

// === TestComparison ===
#[test]
fn cmp_gt_true() {
    assert_eq!(eval("5 > 3").to_display_string(), "true");
}
#[test]
fn cmp_gt_false() {
    assert_eq!(eval("3 > 5").to_display_string(), "false");
}
#[test]
fn cmp_lt_false() {
    assert_eq!(eval("5 < 3").to_display_string(), "false");
}
#[test]
fn cmp_lt_true() {
    assert_eq!(eval("3 < 5").to_display_string(), "true");
}
#[test]
fn cmp_ge() {
    assert_eq!(eval("5 >= 5").to_display_string(), "true");
}
#[test]
fn cmp_le() {
    assert_eq!(eval("5 <= 5").to_display_string(), "true");
}
#[test]
fn cmp_eq() {
    assert_eq!(eval("5 == 5").to_display_string(), "true");
}
#[test]
fn cmp_eq_false() {
    assert_eq!(eval("5 == 6").to_display_string(), "false");
}
#[test]
fn cmp_ne() {
    assert_eq!(eval("5 != 6").to_display_string(), "true");
}
#[test]
fn cmp_ne_false() {
    assert_eq!(eval("5 != 5").to_display_string(), "false");
}

#[test]
fn str_lt() {
    assert_eq!(eval("'abc' < 'abd'").to_display_string(), "true");
}
#[test]
fn str_eq() {
    assert_eq!(eval("'abc' == 'abc'").to_display_string(), "true");
}
#[test]
fn str_ne() {
    assert_eq!(eval("'abc' != 'xyz'").to_display_string(), "true");
}

// === TestLogical ===
#[test]
fn and_tt() {
    assert_eq!(eval("True and True").to_display_string(), "true");
}
#[test]
fn and_tf() {
    assert_eq!(eval("True and False").to_display_string(), "false");
}
#[test]
fn and_ft() {
    assert_eq!(eval("False and True").to_display_string(), "false");
}
#[test]
fn and_ff() {
    assert_eq!(eval("False and False").to_display_string(), "false");
}
#[test]
fn or_tf() {
    assert_eq!(eval("True or False").to_display_string(), "true");
}
#[test]
fn or_ft() {
    assert_eq!(eval("False or True").to_display_string(), "true");
}
#[test]
fn or_ff() {
    assert_eq!(eval("False or False").to_display_string(), "false");
}
#[test]
fn not_true() {
    assert_eq!(eval("not True").to_display_string(), "false");
}
#[test]
fn not_false() {
    assert_eq!(eval("not False").to_display_string(), "true");
}

#[test]
fn short_circuit_and() {
    let mut st = SymbolTable::new();
    st.set("X", ExprValue::Int(0)).unwrap();
    assert_eq!(
        eval_with("False and (1 / X > 0)", &st).to_display_string(),
        "false"
    );
}

#[test]
fn short_circuit_or() {
    let mut st = SymbolTable::new();
    st.set("X", ExprValue::Int(0)).unwrap();
    assert_eq!(
        eval_with("True or (1 / X > 0)", &st).to_display_string(),
        "true"
    );
}

// Value-returning and/or
#[test]
fn or_null_returns_right() {
    assert_eq!(eval("null or \"fallback\"").to_display_string(), "fallback");
}
#[test]
fn or_false_returns_right() {
    assert_eq!(
        eval("false or \"fallback\"").to_display_string(),
        "fallback"
    );
}
#[test]
fn or_truthy_returns_left() {
    assert_eq!(
        eval("\"hello\" or \"fallback\"").to_display_string(),
        "hello"
    );
}
#[test]
fn or_zero_is_truthy() {
    assert_eq!(eval("0 or 5").to_display_string(), "0");
}
#[test]
fn or_empty_string_is_truthy() {
    assert_eq!(eval("\"\" or \"fallback\"").to_display_string(), "");
}
#[test]
fn or_empty_list_is_truthy() {
    assert_eq!(eval("[] or [1]").to_display_string(), "[]");
}
#[test]
fn and_null_returns_null() {
    assert!(matches!(eval("null and \"hello\""), ExprValue::Null));
}
#[test]
fn and_false_returns_false() {
    assert_eq!(eval("false and \"hello\"").to_display_string(), "false");
}
#[test]
fn and_truthy_returns_right() {
    assert_eq!(eval("\"hello\" and \"world\"").to_display_string(), "world");
}
#[test]
fn and_true_returns_right() {
    assert_eq!(eval("true and \"hello\"").to_display_string(), "hello");
}
#[test]
fn or_short_circuits_fail() {
    assert_eq!(
        eval("true or fail(\"should not reach\")").to_display_string(),
        "true"
    );
}
#[test]
fn and_short_circuits_fail() {
    assert!(matches!(
        eval("null and fail(\"should not reach\")"),
        ExprValue::Null
    ));
}

// === TestConditional ===
#[test]
fn if_true() {
    assert_eq!(eval("10 if True else 20").to_display_string(), "10");
}
#[test]
fn if_false() {
    assert_eq!(eval("10 if False else 20").to_display_string(), "20");
}
#[test]
fn nested_conditional() {
    assert_eq!(
        eval("1 if False else 2 if False else 3").to_display_string(),
        "3"
    );
}
#[test]
fn conditional_with_expression() {
    let mut st = SymbolTable::new();
    st.set("Param.Quality", ExprValue::String("final".to_string()))
        .unwrap();
    assert_eq!(
        eval_with("16 if Param.Quality == 'final' else 4", &st).to_display_string(),
        "16"
    );
}

// === TestCrossTypeEquality ===
#[test]
fn string_eq_path() {
    let mut st = SymbolTable::new();
    st.set("s", ExprValue::String("/tmp/home/user".to_string()))
        .unwrap();
    st.set(
        "p",
        ExprValue::new_path("/tmp/home/user", PathFormat::Posix),
    )
    .unwrap();
    assert_eq!(eval_posix("s == p", &st).to_display_string(), "true");
}

#[test]
fn string_eq_int() {
    assert_eq!(eval("'5' == 5").to_display_string(), "false");
}
#[test]
fn bool_eq_int() {
    assert_eq!(eval("True == 1").to_display_string(), "false");
}
#[test]
fn int_eq_list() {
    assert_eq!(eval("1 == [1]").to_display_string(), "false");
}
#[test]
fn int_eq_float() {
    assert_eq!(eval("5 == 5.0").to_display_string(), "true");
}
#[test]
fn list_eq_same() {
    assert_eq!(eval("[1, 2] == [1, 2]").to_display_string(), "true");
}
#[test]
fn nested_list_eq() {
    assert_eq!(eval("[[1]] == [[1]]").to_display_string(), "true");
}
#[test]
fn list_eq_range_expr() {
    assert_eq!(
        eval("[1, 2, 3] == range_expr(\"1-3\")").to_display_string(),
        "true"
    );
}
#[test]
fn list_lt() {
    assert_eq!(eval("[1, 2] < [1, 3]").to_display_string(), "true");
}
#[test]
fn list_le_equal() {
    assert_eq!(eval("[1, 2] <= [1, 2]").to_display_string(), "true");
}

// === TestCrossTypeInequality ===
#[test]
fn int_lt_float() {
    assert_eq!(eval("5 < 5.5").to_display_string(), "true");
}
#[test]
fn int_le_float_equal() {
    assert_eq!(eval("5 <= 5.0").to_display_string(), "true");
}

#[test]
fn string_lt_path() {
    let mut st = SymbolTable::new();
    st.set("s", ExprValue::String("/tmp/aaa".to_string()))
        .unwrap();
    st.set("p", ExprValue::new_path("/tmp/bbb", PathFormat::Posix))
        .unwrap();
    assert_eq!(eval_posix("s < p", &st).to_display_string(), "true");
}

// === TestCrossTypeOrderingErrors ===
#[test]
fn string_lt_int_errors() {
    let e = ParsedExpression::new("\"5\" < 5")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Cannot use '<' operator with string and int\n",
                "  \"5\" < 5\n",
                "  ^~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}
#[test]
fn string_gt_int_errors() {
    let e = ParsedExpression::new("\"abc\" > 123")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Cannot use '>' operator with string and int\n",
                "  \"abc\" > 123\n",
                "  ^~~~~~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}
#[test]
fn bool_lt_int_errors() {
    let e = ParsedExpression::new("True < 0")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Cannot use '<' operator with bool and int\n",
                "  True < 0\n",
                "  ^~~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn bool_ordering() {
    assert_eq!(eval("False < True").to_display_string(), "true");
    assert_eq!(eval("True > False").to_display_string(), "true");
    assert_eq!(eval("False <= False").to_display_string(), "true");
    assert_eq!(eval("True >= True").to_display_string(), "true");
}

#[test]
fn int_lt_string_errors() {
    let e = ParsedExpression::new("5 < \"5\"")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Cannot use '<' operator with int and string\n",
                "  5 < \"5\"\n",
                "  ^~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}
#[test]
fn float_gt_string_errors() {
    let e = ParsedExpression::new("3.14 > \"pi\"")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Cannot use '>' operator with float and string\n",
                "  3.14 > \"pi\"\n",
                "  ^~~~~~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}
