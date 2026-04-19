// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_path_format_mismatch.py

use openjd_expr::*;

fn eval_fmt(expr: &str, st: &SymbolTable, fmt: PathFormat) -> Result<ExprValue, ExpressionError> {
    let parsed = ParsedExpression::new(expr)?;
    let symtabs = [st];
    parsed.with_path_format(fmt).evaluate(&symtabs)
}

#[test]
fn matching_format_no_error() {
    let mut st = SymbolTable::new();
    st.set(
        "P",
        ExprValue::new_path("/tmp/render.exr", PathFormat::Posix),
    )
    .unwrap();
    let r = eval_fmt("P", &st, PathFormat::Posix).unwrap();
    assert_eq!(r.as_str_repr(), "/tmp/render.exr");
}

#[test]
fn evaluator_defaults_to_host_format() {
    let host = PathFormat::host();
    let expected = if cfg!(windows) {
        "\\tmp\\render.exr"
    } else {
        "/tmp/render.exr"
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::new_path("/tmp/render.exr", host))
        .unwrap();
    let r = evaluate_expression("P", &st).unwrap();
    assert_eq!(r.as_str_repr(), expected);
}

#[test]
fn non_path_types_no_check() {
    let mut st = SymbolTable::new();
    st.set("x", ExprValue::Int(42)).unwrap();
    st.set("s", ExprValue::String("hello".into())).unwrap();
    st.set("b", ExprValue::Bool(true)).unwrap();
    assert!(matches!(
        eval_fmt("x", &st, PathFormat::Posix).unwrap(),
        ExprValue::Int(42)
    ));
    assert!(
        matches!(eval_fmt("s", &st, PathFormat::Posix).unwrap(), ExprValue::String(ref s) if s == "hello")
    );
    assert!(matches!(
        eval_fmt("b", &st, PathFormat::Posix).unwrap(),
        ExprValue::Bool(true)
    ));
}

#[test]
fn posix_value_in_windows_evaluator() {
    let mut st = SymbolTable::new();
    st.set(
        "P",
        ExprValue::new_path("/tmp/render.exr", PathFormat::Posix),
    )
    .unwrap();
    let err = eval_fmt("P", &st, PathFormat::Windows).unwrap_err();
    assert!(
        err.to_string().contains("Path format mismatch"),
        "expected mismatch error, got: {err}"
    );
}

#[test]
fn windows_value_in_posix_evaluator() {
    let mut st = SymbolTable::new();
    st.set(
        "P",
        ExprValue::new_path(r"C:\renders\shot01", PathFormat::Windows),
    )
    .unwrap();
    let err = eval_fmt("P", &st, PathFormat::Posix).unwrap_err();
    assert!(
        err.to_string().contains("Path format mismatch"),
        "expected mismatch error, got: {err}"
    );
}

#[test]
fn list_path_matching_no_error() {
    let mut st = SymbolTable::new();
    st.set(
        "Paths",
        ExprValue::make_list(
            vec![
                ExprValue::new_path("/a", PathFormat::Posix),
                ExprValue::new_path("/b", PathFormat::Posix),
            ],
            ExprType::PATH,
        )
        .unwrap(),
    )
    .unwrap();
    let r = eval_fmt("Paths", &st, PathFormat::Posix).unwrap();
    assert!(r.is_list());
}

#[test]
fn list_path_mismatch() {
    let mut st = SymbolTable::new();
    st.set(
        "Paths",
        ExprValue::make_list(
            vec![
                ExprValue::new_path("/a", PathFormat::Posix),
                ExprValue::new_path("/b", PathFormat::Posix),
            ],
            ExprType::PATH,
        )
        .unwrap(),
    )
    .unwrap();
    let err = eval_fmt("Paths", &st, PathFormat::Windows).unwrap_err();
    assert!(
        err.to_string().contains("Path format mismatch"),
        "expected mismatch error, got: {err}"
    );
}

#[test]
fn mismatch_error_message_includes_variable_name() {
    let mut st = SymbolTable::new();
    st.set(
        "Param.InputFile",
        ExprValue::new_path("/tmp/test", PathFormat::Windows),
    )
    .unwrap();
    let err = eval_fmt("Param.InputFile", &st, PathFormat::Posix).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Path format mismatch"),
        "expected mismatch error, got: {msg}"
    );
    assert!(
        msg.contains("Param.InputFile"),
        "expected variable name in error, got: {msg}"
    );
}
