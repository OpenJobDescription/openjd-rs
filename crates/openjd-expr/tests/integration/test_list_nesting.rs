// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for max 2 nesting level validation in make_list.

use openjd_expr::{ExprType, ExprValue, ParsedExpression, SymbolTable};

fn eval(expr: &str) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap()
}

fn eval_err(expr: &str) -> String {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
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

// === make_list rejects unresolved elements ===
//
// make_list constructs concrete lists: an Unresolved element is rejected
// uniformly rather than silently nesting inside a ListList (which would
// break the invariant that a top-level is_unresolved() check is a
// complete concreteness test — validate_expressions relies on it for
// `resolved_value` soundness). Validation-time list expressions with
// unknown elements are the evaluator's job: list literals and
// comprehensions hoist to a top-level unresolved(list[T]).

#[test]
fn make_list_rejects_unresolved_scalar_element() {
    let err = ExprValue::make_list(
        vec![ExprValue::Int(1), ExprValue::unresolved(ExprType::INT)],
        ExprType::NULLTYPE,
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("make_list expected concrete elements, got unresolved"),
        "got: {err}"
    );
}

#[test]
fn make_list_rejects_unresolved_list_element() {
    // Previously this silently built a ListList holding an Unresolved — a
    // nested shape whose top-level is_unresolved() was false.
    let inner = ExprValue::make_list(vec![ExprValue::Int(1)], ExprType::INT).unwrap();
    let err = ExprValue::make_list(
        vec![inner, ExprValue::unresolved(ExprType::list(ExprType::INT))],
        ExprType::NULLTYPE,
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("make_list expected concrete elements, got unresolved"),
        "got: {err}"
    );
}

#[test]
fn make_list_rejects_lone_unresolved_element() {
    let err = ExprValue::make_list(
        vec![ExprValue::unresolved(ExprType::STRING)],
        ExprType::NULLTYPE,
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("make_list expected concrete elements, got unresolved"),
        "got: {err}"
    );
}

// === Evaluator hoists unresolved list elements ===

fn eval_with_unresolved(expr: &str) -> Result<ExprValue, String> {
    let mut st = SymbolTable::new();
    st.set("Param.X", ExprValue::unresolved(ExprType::INT))
        .unwrap();
    st.set(
        "Param.Y",
        ExprValue::unresolved(ExprType::list(ExprType::INT)),
    )
    .unwrap();
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&st))
        .map_err(|e| e.to_string())
}

#[test]
fn list_literal_with_unresolved_element_hoists() {
    let val = eval_with_unresolved("[Param.X, 1]").unwrap();
    assert!(matches!(
        val,
        ExprValue::Unresolved(ref t) if *t == ExprType::list(ExprType::INT)
    ));
}

#[test]
fn list_literal_with_unresolved_list_element_hoists() {
    let val = eval_with_unresolved("[[1], Param.Y]").unwrap();
    assert!(matches!(
        val,
        ExprValue::Unresolved(ref t) if *t == ExprType::list(ExprType::list(ExprType::INT))
    ));
}

#[test]
fn comprehension_with_unresolved_body_hoists() {
    // The iterable is concrete but the body references an unresolved
    // symbol, so every element is unresolved. This must hoist like a list
    // literal — it previously failed validation because the unresolved
    // elements reached make_list.
    let val = eval_with_unresolved("[Param.X + i for i in [1, 2]]").unwrap();
    assert!(matches!(
        val,
        ExprValue::Unresolved(ref t) if *t == ExprType::list(ExprType::INT)
    ));
}

#[test]
fn comprehension_with_partially_unresolved_body_hoists() {
    // Only some elements are unresolved (the body is conditional on the
    // loop variable): still hoists, with the promoted element type.
    let val = eval_with_unresolved("[Param.X + i if i > 1 else i for i in [1, 2]]").unwrap();
    assert!(matches!(
        val,
        ExprValue::Unresolved(ref t) if *t == ExprType::list(ExprType::INT)
    ));
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

#[test]
fn make_list_string_rejects_non_string() {
    let result = ExprValue::make_list(
        vec![ExprValue::String("a".into()), ExprValue::Int(1)],
        ExprType::STRING,
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("make_list expected string element, got int"),
        "got: {err}"
    );
}

#[test]
fn make_list_path_rejects_non_path_non_string() {
    let result = ExprValue::make_list(
        vec![
            ExprValue::new_path("/a", openjd_expr::PathFormat::Posix),
            ExprValue::Int(42),
        ],
        ExprType::PATH,
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("make_list expected path element, got int"),
        "got: {err}"
    );
}

#[test]
fn make_list_incompatible_types_error() {
    // Null + Int has no promotion rule — must error, not fall back to ListString
    let result = ExprValue::make_list(vec![ExprValue::Null, ExprValue::Int(1)], ExprType::NULLTYPE);
    assert!(result.is_err(), "incompatible types must error");
}
