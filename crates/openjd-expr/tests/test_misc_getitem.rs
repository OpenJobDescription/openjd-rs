// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Coverage tests for getitem operators in functions/misc.rs:
//! getitem_list, getitem_string, getitem_range

use openjd_expr::{ExprValue, ParsedExpression, RangeExpr, SymbolTable};

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
fn assert_err(expr: &str, expected: &[&str]) {
    let e = ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}
fn assert_err_with(expr: &str, st: &SymbolTable, expected: &[&str]) {
    let e = ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(st))
        .unwrap_err()
        .to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

// === getitem_list ===
#[test]
fn list_index_first() {
    assert_eq!(eval("[10, 20, 30][0]").to_display_string(), "10");
}
#[test]
fn list_index_last() {
    assert_eq!(eval("[10, 20, 30][2]").to_display_string(), "30");
}
#[test]
fn list_index_neg() {
    assert_eq!(eval("[10, 20, 30][-1]").to_display_string(), "30");
}
#[test]
fn list_index_neg2() {
    assert_eq!(eval("[10, 20, 30][-2]").to_display_string(), "20");
}
#[test]
fn list_index_oob() {
    assert_err(
        "[10, 20, 30][5]",
        &[
            "Index 5 out of bounds for list of length 3\n",
            "  [10, 20, 30][5]\n",
        ],
    );
}
#[test]
fn list_index_neg_oob() {
    assert_err(
        "[10, 20][-5]",
        &[
            "Index -5 out of bounds for list of length 2\n",
            "  [10, 20][-5]\n",
        ],
    );
}

// === getitem_string ===
#[test]
fn string_index_first() {
    assert_eq!(eval("'hello'[0]").to_display_string(), "h");
}
#[test]
fn string_index_last() {
    assert_eq!(eval("'hello'[4]").to_display_string(), "o");
}
#[test]
fn string_index_neg() {
    assert_eq!(eval("'hello'[-1]").to_display_string(), "o");
}
#[test]
fn string_index_neg2() {
    assert_eq!(eval("'hello'[-2]").to_display_string(), "l");
}
#[test]
fn string_index_oob() {
    assert_err(
        "'hello'[10]",
        &[
            "Index 10 out of bounds for string of length 5\n",
            "  'hello'[10]\n",
        ],
    );
}

// === getitem_range ===
#[test]
fn range_index_first() {
    let mut st = SymbolTable::new();
    st.set(
        "r",
        ExprValue::RangeExpr("1-5".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    assert_eq!(eval_with("r[0]", &st).to_display_string(), "1");
}
#[test]
fn range_index_last() {
    let mut st = SymbolTable::new();
    st.set(
        "r",
        ExprValue::RangeExpr("1-5".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    assert_eq!(eval_with("r[4]", &st).to_display_string(), "5");
}
#[test]
fn range_index_neg() {
    let mut st = SymbolTable::new();
    st.set(
        "r",
        ExprValue::RangeExpr("1-5".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    assert_eq!(eval_with("r[-1]", &st).to_display_string(), "5");
}
#[test]
fn range_index_oob() {
    let mut st = SymbolTable::new();
    st.set(
        "r",
        ExprValue::RangeExpr("1-5".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    assert_err_with(
        "r[100]",
        &st,
        &[
            "Index 100 out of bounds for range_expr of length 5\n",
            "  r[100]\n",
        ],
    );
}
