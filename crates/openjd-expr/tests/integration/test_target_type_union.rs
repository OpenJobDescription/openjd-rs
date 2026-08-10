// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for target_type union membership coercion.
//!
//! When `target_type` is a union, the expression result should be:
//!
//! 1. Returned as-is if its type is one of the union's members
//!    (RFC 0005 §"Implicit Type Coercion" — coercion is non-destructive
//!    where the intent is obvious; if the value already satisfies one of
//!    the targets, no coercion is needed).
//! 2. Otherwise, non-destructively coerced to one of the scalar union
//!    members.
//! 3. Otherwise, an error.
//!
//! These tests exercise the public `EvalBuilder::with_target_type` and
//! `ExprValue::coerce` surfaces against union targets like `int | string`.

use openjd_expr::*;

fn eval_with_target_type(
    expr: &str,
    target: &ExprType,
    symtab: &SymbolTable,
) -> Result<ExprValue, ExpressionError> {
    ParsedExpression::new(expr)?
        .with_target_type(target)
        .evaluate(&[symtab])
}

/// Assert that `expr`, evaluated with `target` against `symtab`, fails
/// with a multi-line error whose concatenation contains `expected`.
/// Mirrors `assert_err` in `test_error_formatting.rs` — see AGENTS.md
/// "Test Quality Standard".
fn assert_eval_err(expr: &str, target: &ExprType, symtab: &SymbolTable, expected: &[&str]) {
    let e = eval_with_target_type(expr, target, symtab)
        .unwrap_err()
        .to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

fn parse_type(s: &str) -> ExprType {
    ExprType::parse(s).unwrap()
}

// ── Match-first: value type is already one of the union members ──────

#[test]
fn union_int_string_accepts_string_value_as_is() {
    // The exact case called out in the bindings report: a string value
    // against an `int | string` target should be returned unchanged.
    let r =
        eval_with_target_type("'42'", &parse_type("int | string"), &SymbolTable::new()).unwrap();
    assert_eq!(r, ExprValue::String("42".to_string()));
    assert_eq!(r.expr_type(), ExprType::STRING);
}

#[test]
fn union_int_string_accepts_int_value_as_is() {
    let r = eval_with_target_type("42", &parse_type("int | string"), &SymbolTable::new()).unwrap();
    assert_eq!(r, ExprValue::Int(42));
    assert_eq!(r.expr_type(), ExprType::INT);
}

#[test]
fn union_string_path_accepts_string_value_as_is() {
    let r = eval_with_target_type(
        "'/foo/bar'",
        &parse_type("string | path"),
        &SymbolTable::new(),
    )
    .unwrap();
    assert_eq!(r, ExprValue::String("/foo/bar".to_string()));
    assert_eq!(r.expr_type(), ExprType::STRING);
}

#[test]
fn union_int_float_accepts_int_value_as_is() {
    let r = eval_with_target_type("3", &parse_type("int | float"), &SymbolTable::new()).unwrap();
    assert_eq!(r, ExprValue::Int(3));
    assert_eq!(r.expr_type(), ExprType::INT);
}

#[test]
fn union_int_float_accepts_float_value_as_is() {
    let r = eval_with_target_type("3.5", &parse_type("int | float"), &SymbolTable::new()).unwrap();
    assert_eq!(r.expr_type(), ExprType::FLOAT);
    match r {
        ExprValue::Float(f) => assert_eq!(f.value(), 3.5),
        other => panic!("expected float, got {other:?}"),
    }
}

// ── Match-first via direct ExprValue::coerce, bypassing the evaluator ──

#[test]
fn coerce_string_to_int_or_string_union_returns_self() {
    let v = ExprValue::String("hello".to_string());
    let target = parse_type("int | string");
    let coerced = v.coerce(&target, PathFormat::Posix).unwrap();
    assert_eq!(coerced, ExprValue::String("hello".to_string()));
}

#[test]
fn coerce_int_to_int_or_string_union_returns_self() {
    let v = ExprValue::Int(99);
    let target = parse_type("int | string");
    let coerced = v.coerce(&target, PathFormat::Posix).unwrap();
    assert_eq!(coerced, ExprValue::Int(99));
}

#[test]
fn coerce_bool_to_int_or_bool_union_returns_self() {
    // bool/int are not interchangeable in EXPR (RFC 0005), so this
    // checks that `bool` against `int | bool` returns the bool value
    // unchanged rather than coercing to int.
    let v = ExprValue::Bool(true);
    let target = parse_type("int | bool");
    let coerced = v.coerce(&target, PathFormat::Posix).unwrap();
    assert_eq!(coerced, ExprValue::Bool(true));
}

// ── Per-member coercion: no member matches by identity but a non-destructive
//    coercion to one of them works ─────────────────────────────────────

#[test]
fn coerce_int_to_string_or_path_union_picks_string() {
    // int doesn't match string or path directly, but int → string is a
    // valid non-destructive coercion. The reference picks the first
    // member whose non-destructive coercion succeeds.
    let v = ExprValue::Int(42);
    let target = parse_type("string | path");
    let coerced = v.coerce(&target, PathFormat::Posix).unwrap();
    assert_eq!(coerced, ExprValue::String("42".to_string()));
}

#[test]
fn outer_target_int_or_path_coerces_int_param_to_string_via_member() {
    // `Param.A` returns int; target `string | path` has no int member;
    // int → string is a valid non-destructive coercion; result is a
    // string. Exercises the per-member coercion path through the
    // public `EvalBuilder::with_target_type` surface, with a single
    // attribute lookup so the outer target type only affects the final
    // coercion (not operand evaluation, which is a separate concern —
    // see test_target_type_propagation.rs).
    let mut st = SymbolTable::new();
    st.set("Param.A", ExprValue::Int(30)).unwrap();
    let r = eval_with_target_type("Param.A", &parse_type("string | path"), &st).unwrap();
    assert_eq!(r, ExprValue::String("30".to_string()));
}

// ── Errors when no member matches and no scalar coercion succeeds ─────

#[test]
fn list_to_int_or_string_union_errors() {
    // A list value cannot satisfy a scalar-only union by either match
    // or non-destructive coercion. The error includes the source
    // expression and a caret pointing at the list literal.
    assert_eval_err(
        "[1, 2, 3]",
        &parse_type("int | string"),
        &SymbolTable::new(),
        &[
            "Cannot coerce list[int] to int | string\n",
            "  [1, 2, 3]\n",
            "  ^~~~~~~~~",
        ],
    );
}

#[test]
fn path_to_int_or_float_union_errors() {
    // Path cannot be coerced to int or float. Use a symbol-table path
    // value (rather than `path('/foo')`) so the error caret points at
    // the variable reference, not at the function-call argument
    // (function-call arguments are evaluated with the parent target
    // type, which is a separate concern from union coercion).
    //
    // Pin the evaluator to `PathFormat::Posix` so the path value's
    // format matches regardless of the host OS — otherwise the
    // path-format-mismatch check fires first on Windows and we'd
    // never reach the coerce step.
    let mut st = SymbolTable::new();
    st.set("Param.P", ExprValue::new_path("/foo", PathFormat::Posix))
        .unwrap();
    let parsed = ParsedExpression::new("Param.P").unwrap();
    let target = parse_type("int | float");
    let err = parsed
        .with_target_type(&target)
        .with_path_format(PathFormat::Posix)
        .evaluate(&[&st])
        .unwrap_err()
        .to_string();
    let joined = [
        "Cannot coerce path to float | int\n",
        "  Param.P\n",
        "  ~~~~~~^",
    ]
    .concat();
    assert!(err.contains(&joined), "got:\n{err}\nexpected:\n{joined}");
}

// ── Optional types `T?` ── these are unions with nulltype ─────────────

#[test]
fn optional_string_accepts_string() {
    let r = eval_with_target_type("'hi'", &parse_type("string?"), &SymbolTable::new()).unwrap();
    assert_eq!(r, ExprValue::String("hi".to_string()));
}

#[test]
fn optional_string_accepts_null() {
    let r = eval_with_target_type("null", &parse_type("string?"), &SymbolTable::new()).unwrap();
    assert_eq!(r, ExprValue::Null);
}

#[test]
fn unresolved_nullable_sources_keep_successful_possibilities() {
    let cases = [
        ("WrappedAction.Timeout", "int?", "int", "int"),
        ("WrappedAction.Timeout", "int?", "int | string", "int"),
        (
            "WrappedAction.Cancelation.Mode",
            "string?",
            "int | string",
            "string",
        ),
        ("U.IntOpt", "int?", "int? | list[int]", "int?"),
        (
            "U.IntOptOrList",
            "int? | list[int]",
            "int | list[int]",
            "int | list[int]",
        ),
        (
            "U.StringOptOrList",
            "string? | list[string]",
            "string?",
            "string?",
        ),
    ];

    for (name, source, target, expected) in cases {
        let mut st = SymbolTable::new();
        st.set(name, ExprValue::unresolved(parse_type(source)))
            .unwrap();
        let result = eval_with_target_type(name, &parse_type(target), &st).unwrap();
        assert_eq!(
            result,
            ExprValue::unresolved(parse_type(expected)),
            "symbol={name}, source={source}, target={target}"
        );
    }
}

#[test]
fn wrapped_action_timeout_format_string_accepts_union_target() {
    let mut st = SymbolTable::new();
    st.set(
        "WrappedAction.Timeout",
        ExprValue::unresolved(parse_type("int?")),
    )
    .unwrap();
    let target = parse_type("int | string");
    let options = FormatStringOptions::default().with_target_type(&target);
    let result = FormatString::new("{{ WrappedAction.Timeout }}")
        .unwrap()
        .resolve_with(&st, &options)
        .unwrap();
    assert_eq!(result, ExprValue::unresolved(ExprType::INT));
}

#[test]
fn nullable_list_elements_narrow_from_list_target() {
    let mut st = SymbolTable::new();
    st.set("U.IntOpt", ExprValue::unresolved(parse_type("int?")))
        .unwrap();
    let result =
        eval_with_target_type("[U.IntOpt, U.IntOpt]", &parse_type("list[int]"), &st).unwrap();
    assert_eq!(result, ExprValue::unresolved(parse_type("list[int]")));
}

#[test]
fn optional_int_coerces_string_to_int() {
    // `int?` is `int | nulltype`. A string value matches neither
    // member directly. The per-member loop then tries `int` and `string`
    // → `int` is a valid non-destructive coercion, so the result is
    // `Int(42)`.
    let r = eval_with_target_type("'42'", &parse_type("int?"), &SymbolTable::new()).unwrap();
    assert_eq!(r, ExprValue::Int(42));
}
