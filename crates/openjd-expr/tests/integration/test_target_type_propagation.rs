// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test_target_type_propagation.py
//!
//! These cover RFC 0005's "Target Type Propagation Rules" table. The first
//! batch of tests exercises in-expression coercion via the `string()`
//! function. The "outer target_type" section at the bottom exercises the
//! same rules through the public `EvalBuilder::with_target_type` API,
//! which is the surface the bindings expose as `target_type=` to Python.
//!
//! Per RFC 0005, operands of `BinOp`, `UnaryOp`, `Compare` (and the test
//! position of `IfExp`) are evaluated unconstrained — the parent's target
//! type must not be propagated into them. Final coercion happens once,
//! against the root expression's value.

use openjd_expr::*;

fn eval(expr: &str) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap()
}

/// Evaluate `expr` with `target_type` applied via the outer
/// `EvalBuilder::with_target_type` API and the given symbol table. This
/// is the equivalent of the Python
/// `evaluate_expression(expr, values=..., target_type=...)` call path.
fn eval_with_target_type(
    expr: &str,
    target: &ExprType,
    symtab: &SymbolTable,
) -> Result<ExprValue, openjd_expr::ExpressionError> {
    ParsedExpression::new(expr)?
        .with_target_type(target)
        .evaluate(&[symtab])
}

/// Build a fresh `SymbolTable` populated with the given `(name, value)`
/// pairs. Convenience for tests that don't need to reuse a table.
fn symtab(entries: &[(&str, ExprValue)]) -> SymbolTable {
    let mut st = SymbolTable::new();
    for (k, v) in entries {
        st.set(k, v.clone()).unwrap();
    }
    st
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

// === Outer target_type API tests (RFC 0005) ===
//
// These tests exercise the `EvalBuilder::with_target_type` surface, which
// is what the Python bindings expose as `target_type=`. The expressions
// here do NOT wrap the result in `string(...)`; the coercion is the
// responsibility of the evaluator's outer "final coercion" step. Per
// RFC 0005, operators must not push the target type into their operands.

#[test]
fn outer_target_string_coerces_arithmetic_int_result() {
    // The exact example called out in RFC 0005 §"Target Type Propagation
    // Rules": `Param.Count - 1` with target_type=string evaluates the
    // operands unconstrained and coerces the int result to a string.
    let st = symtab(&[("Param.Count", ExprValue::Int(100))]);
    let r = eval_with_target_type("Param.Count - 1", &ExprType::STRING, &st).unwrap();
    assert_eq!(r, ExprValue::String("99".to_string()));
}

#[test]
fn outer_target_string_addition() {
    let st = symtab(&[
        ("Param.A", ExprValue::Int(10)),
        ("Param.B", ExprValue::Int(20)),
    ]);
    let r = eval_with_target_type("Param.A + Param.B", &ExprType::STRING, &st).unwrap();
    assert_eq!(r, ExprValue::String("30".to_string()));
}

#[test]
fn outer_target_string_unary_neg() {
    let st = symtab(&[("Param.N", ExprValue::Int(42))]);
    let r = eval_with_target_type("-Param.N", &ExprType::STRING, &st).unwrap();
    assert_eq!(r, ExprValue::String("-42".to_string()));
}

#[test]
fn outer_target_string_compare_returns_bool_then_coerces() {
    // Comparison operands are evaluated unconstrained → bool result is
    // produced, then coerced to "true"/"false".
    let st = symtab(&[
        ("Param.A", ExprValue::Int(5)),
        ("Param.B", ExprValue::Int(10)),
    ]);
    let r = eval_with_target_type("Param.A < Param.B", &ExprType::STRING, &st).unwrap();
    assert_eq!(r, ExprValue::String("true".to_string()));
}

#[test]
fn outer_target_string_complex_arithmetic() {
    let st = symtab(&[
        ("Param.ImageCount", ExprValue::Int(100)),
        ("Param.ChunkSize", ExprValue::Int(10)),
    ]);
    let r = eval_with_target_type(
        "(Param.ImageCount - 1) // Param.ChunkSize",
        &ExprType::STRING,
        &st,
    )
    .unwrap();
    assert_eq!(r, ExprValue::String("9".to_string()));
}

#[test]
fn outer_target_string_ifexp_inherits_into_branches() {
    // IfExp.test is evaluated unconstrained; body/orelse inherit the
    // parent target_type. With target_type=string, both int branches
    // should coerce cleanly to string.
    let st = symtab(&[
        ("Param.X", ExprValue::Int(7)),
        ("Param.Flag", ExprValue::Bool(true)),
    ]);
    let r = eval_with_target_type(
        "Param.X * 2 if Param.Flag else Param.X",
        &ExprType::STRING,
        &st,
    )
    .unwrap();
    assert_eq!(r, ExprValue::String("14".to_string()));
}

#[test]
fn outer_target_string_ifexp_test_must_be_bool_not_string() {
    // If `target_type=string` leaked into IfExp.test, the inner `2 < 3`
    // would coerce to `"true"` and the explicit bool-compatibility check
    // in `eval_ifexp` would reject it. Per RFC, the test slot is
    // unconstrained.
    let r =
        eval_with_target_type("1 if 2 < 3 else 0", &ExprType::STRING, &SymbolTable::new()).unwrap();
    assert_eq!(r, ExprValue::String("1".to_string()));
}

#[test]
fn outer_target_string_no_op_when_already_string() {
    // String literal with target_type=string is a no-op coercion.
    let r = eval_with_target_type("'hello'", &ExprType::STRING, &SymbolTable::new()).unwrap();
    assert_eq!(r, ExprValue::String("hello".to_string()));
}

#[test]
fn outer_target_int_passthrough() {
    // Arithmetic with target_type=int returns the int directly.
    let r = eval_with_target_type("5 - 3", &ExprType::INT, &SymbolTable::new()).unwrap();
    assert_eq!(r, ExprValue::Int(2));
}

#[test]
fn outer_target_float_promotes_int_result() {
    // RFC 0005 §"Implicit Type Coercion": int → float when target does not
    // include int. The arithmetic stays in int land, then the final
    // coercion promotes to float.
    let r = eval_with_target_type("3 + 4", &ExprType::FLOAT, &SymbolTable::new()).unwrap();
    match r {
        ExprValue::Float(f) => assert_eq!(f.value(), 7.0),
        other => panic!("expected float, got {other:?}"),
    }
}

#[test]
fn outer_target_string_subtraction_with_param() {
    // Plain `Param.End - 1` with target_type=string. Both operands
    // evaluate as int, `__sub__` runs in int land, the int result
    // coerces to a string.
    let st = symtab(&[("Param.End", ExprValue::Int(100))]);
    let r = eval_with_target_type("Param.End - 1", &ExprType::STRING, &st).unwrap();
    assert_eq!(r, ExprValue::String("99".to_string()));
}
