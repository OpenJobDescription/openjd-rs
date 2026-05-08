// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test_target_type_propagation.py

use openjd_expr::*;

fn eval(expr: &str) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap()
}

#[test]
fn subtraction_with_string_target() {
    assert_eq!(eval("string(5 - 3)").to_display_string(), "2");
}
#[test]
fn addition_with_string_target() {
    assert_eq!(eval("string(2 + 3)").to_display_string(), "5");
}
#[test]
fn multiplication_with_string_target() {
    assert_eq!(eval("string(2 * 3)").to_display_string(), "6");
}
#[test]
fn division_with_string_target() {
    assert_eq!(eval("string(6 / 2)").to_display_string(), "3.0");
}
#[test]
fn floor_division_with_string_target() {
    assert_eq!(eval("string(7 // 2)").to_display_string(), "3");
}
#[test]
fn modulo_with_string_target() {
    assert_eq!(eval("string(7 % 3)").to_display_string(), "1");
}
#[test]
fn complex_expression() {
    assert_eq!(eval("string((2 + 3) * 4)").to_display_string(), "20");
}
#[test]
fn nested_arithmetic() {
    assert_eq!(eval("string(1 + 2 + 3)").to_display_string(), "6");
}
#[test]
fn negation_with_string_target() {
    assert_eq!(eval("string(-5)").to_display_string(), "-5");
}
#[test]
fn not_with_string_target() {
    assert_eq!(eval("string(not true)").to_display_string(), "false");
}
#[test]
fn conditional_with_string_target() {
    assert_eq!(eval("string(1 if true else 2)").to_display_string(), "1");
}
#[test]
fn conditional_arithmetic() {
    assert_eq!(
        eval("string(1 + 2 if true else 3 + 4)").to_display_string(),
        "3"
    );
}
#[test]
fn less_than_with_string_target() {
    assert_eq!(eval("string(1 < 2)").to_display_string(), "true");
}
#[test]
fn equality_with_string_target() {
    assert_eq!(eval("string(1 == 1)").to_display_string(), "true");
}
#[test]
fn subtraction_in_range_context() {
    // range(10 - 5) should work — subtraction result used as range stop
    let r = ParsedExpression::new("range(10 - 5)")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap();
    assert_eq!(r.list_len(), Some(5));
}
#[test]
fn floor_division_in_range_context() {
    let r = ParsedExpression::new("range(10 // 2)")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap();
    assert_eq!(r.list_len(), Some(5));
}

// === Tests with symbol table parameters (ported from Python) ===
// The Python tests use Param.X style parameters to verify that parameter
// resolution works correctly when results are converted to string.

fn eval_with_params(expr: &str, params: &[(&str, ExprValue)]) -> ExprValue {
    let mut st = SymbolTable::new();
    for (k, v) in params {
        st.set(k, v.clone()).unwrap();
    }
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&st))
        .unwrap()
}

#[test]
fn param_subtraction_with_string_target() {
    let r = eval_with_params(
        "string(Param.Count - 1)",
        &[("Param.Count", ExprValue::Int(100))],
    );
    assert_eq!(r.to_display_string(), "99");
}
#[test]
fn param_addition_with_string_target() {
    let r = eval_with_params(
        "string(Param.A + Param.B)",
        &[
            ("Param.A", ExprValue::Int(10)),
            ("Param.B", ExprValue::Int(20)),
        ],
    );
    assert_eq!(r.to_display_string(), "30");
}
#[test]
fn param_multiplication_with_string_target() {
    let r = eval_with_params("string(Param.X * 6)", &[("Param.X", ExprValue::Int(7))]);
    assert_eq!(r.to_display_string(), "42");
}
#[test]
fn param_division_with_string_target() {
    let r = eval_with_params("string(Param.N / 4)", &[("Param.N", ExprValue::Int(10))]);
    assert_eq!(r.to_display_string(), "2.5");
}
#[test]
fn param_floor_division_with_string_target() {
    let r = eval_with_params("string(Param.N // 3)", &[("Param.N", ExprValue::Int(10))]);
    assert_eq!(r.to_display_string(), "3");
}
#[test]
fn param_modulo_with_string_target() {
    let r = eval_with_params("string(Param.N % 3)", &[("Param.N", ExprValue::Int(10))]);
    assert_eq!(r.to_display_string(), "1");
}
#[test]
fn param_complex_expression_with_string_target() {
    let r = eval_with_params(
        "string((Param.ImageCount - 1) // Param.ChunkSize)",
        &[
            ("Param.ImageCount", ExprValue::Int(100)),
            ("Param.ChunkSize", ExprValue::Int(10)),
        ],
    );
    assert_eq!(r.to_display_string(), "9");
}
#[test]
fn param_nested_arithmetic_with_string_target() {
    let r = eval_with_params(
        "string((Param.End - Param.Start) // Param.Step)",
        &[
            ("Param.Start", ExprValue::Int(0)),
            ("Param.End", ExprValue::Int(100)),
            ("Param.Step", ExprValue::Int(10)),
        ],
    );
    assert_eq!(r.to_display_string(), "10");
}
#[test]
fn param_less_than_with_string_target() {
    let r = eval_with_params(
        "string(Param.A < Param.B)",
        &[
            ("Param.A", ExprValue::Int(5)),
            ("Param.B", ExprValue::Int(10)),
        ],
    );
    assert_eq!(r.to_display_string(), "true");
}
#[test]
fn param_equality_with_string_target() {
    let r = eval_with_params("string(Param.X == 42)", &[("Param.X", ExprValue::Int(42))]);
    assert_eq!(r.to_display_string(), "true");
}
#[test]
fn param_negation_with_string_target() {
    let r = eval_with_params("string(-Param.N)", &[("Param.N", ExprValue::Int(42))]);
    assert_eq!(r.to_display_string(), "-42");
}
#[test]
fn param_not_with_string_target() {
    let r = eval_with_params(
        "string(not Param.Flag)",
        &[("Param.Flag", ExprValue::Bool(true))],
    );
    assert_eq!(r.to_display_string(), "false");
}
#[test]
fn param_conditional_with_string_target() {
    let r = eval_with_params(
        "string(100 if Param.Quality == 'high' else 50)",
        &[("Param.Quality", ExprValue::String("high".into()))],
    );
    assert_eq!(r.to_display_string(), "100");
}
#[test]
fn param_conditional_arithmetic_with_string_target() {
    let r = eval_with_params(
        "string(Param.N * 2 if Param.Flag else Param.N)",
        &[
            ("Param.N", ExprValue::Int(10)),
            ("Param.Flag", ExprValue::Bool(true)),
        ],
    );
    assert_eq!(r.to_display_string(), "20");
}
#[test]
fn param_subtraction_in_range_context() {
    let r = eval_with_params(
        "range(Param.End - 1)",
        &[("Param.End", ExprValue::Int(100))],
    );
    assert_eq!(r.list_len(), Some(99));
}
#[test]
fn param_floor_division_in_range_context() {
    let r = eval_with_params(
        "range((Param.Total - 1) // Param.Chunk)",
        &[
            ("Param.Total", ExprValue::Int(100)),
            ("Param.Chunk", ExprValue::Int(10)),
        ],
    );
    assert_eq!(r.list_len(), Some(9));
}
