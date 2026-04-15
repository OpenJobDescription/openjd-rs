// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests for max 2 nesting level validation in make_list.

use openjd_expr::{evaluate_expression, ExprType, ExprValue, SymbolTable};

fn eval(expr: &str) -> ExprValue {
    evaluate_expression(expr, &SymbolTable::new()).unwrap()
}

fn eval_err(expr: &str) -> String {
    evaluate_expression(expr, &SymbolTable::new())
        .unwrap_err()
        .to_string()
}

#[test]
fn one_level_nesting_ok() {
    let val = eval("[[1, 2], [3, 4]]");
    assert!(val.is_list());
    assert_eq!(val.to_display_string(), "[[1, 2], [3, 4]]");
}

#[test]
fn two_levels_nesting_rejected_via_expression() {
    let err = eval_err("[[[1, 2]]]");
    assert!(
        err.contains("Lists may be nested at most 2 levels deep"),
        "got: {err}"
    );
}

#[test]
fn make_list_rejects_listlist_element() {
    let inner = ExprValue::make_list(vec![ExprValue::Int(1)], ExprType::INT).unwrap();
    let mid = ExprValue::make_list(vec![inner], ExprType::list(ExprType::INT)).unwrap();
    let result = ExprValue::make_list(vec![mid], ExprType::list(ExprType::list(ExprType::INT)));
    assert!(result.is_err(), "make_list should reject 3+ nesting levels");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Lists may be nested at most 2 levels deep"),
        "got: {err}"
    );
}

// === make_list type mismatch errors ===

#[test]
fn make_list_bool_rejects_non_bool() {
    let result = ExprValue::make_list(
        vec![ExprValue::Bool(true), ExprValue::Int(1)],
        ExprType::BOOL,
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("make_list expected bool element, got int"),
        "got: {err}"
    );
}

#[test]
fn make_list_int_rejects_non_int() {
    let result = ExprValue::make_list(
        vec![ExprValue::Int(1), ExprValue::Bool(true)],
        ExprType::INT,
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("make_list expected int element, got bool"),
        "got: {err}"
    );
}

#[test]
fn make_list_float_rejects_non_float() {
    use openjd_expr::value::Float64;
    let result = ExprValue::make_list(
        vec![
            ExprValue::Float(Float64::new(1.0).unwrap()),
            ExprValue::Bool(false),
        ],
        ExprType::FLOAT,
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("make_list expected float element, got bool"),
        "got: {err}"
    );
}
