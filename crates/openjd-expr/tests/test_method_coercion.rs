// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_method_coercion.py and test_target_type_propagation.py

use openjd_expr::{evaluate_expression, ExprValue, ParsedExpression, PathFormat, SymbolTable};

fn eval(expr: &str) -> ExprValue {
    evaluate_expression(expr, &SymbolTable::new()).unwrap()
}

fn eval_posix(expr: &str) -> ExprValue {
    let parsed = ParsedExpression::new(expr).unwrap();
    let st = SymbolTable::new();
    let symtabs = [&st];
    parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap()
}

fn eval_posix_err(expr: &str) -> String {
    let parsed = ParsedExpression::new(expr).unwrap();
    let st = SymbolTable::new();
    let symtabs = [&st];
    parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap_err()
        .to_string()
}

// === TestMethodCallNoReceiverCoercion ===
#[test]
fn method_on_int_zfill() {
    assert_eq!(eval("(42).zfill(5)").to_display_string(), "00042");
}
#[test]
fn method_on_float_zfill() {
    assert_eq!(eval("(3.14).zfill(8)").to_display_string(), "00003.14");
}

// === TestArithmeticInStringContext (basic) ===
#[test]
fn string_concat_int() {
    assert_eq!(
        eval("'value: ' + string(42)").to_display_string(),
        "value: 42"
    );
}
#[test]
fn string_concat_float() {
    assert_eq!(
        eval("'pi: ' + string(3.14)").to_display_string(),
        "pi: 3.14"
    );
}

// === Additional method coercion tests ===
#[test]
fn path_startswith_as_function() {
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::new_path("/a/b/c", PathFormat::Posix))
        .unwrap();
    let parsed = ParsedExpression::new("startswith(string(P), '/a')").unwrap();
    assert!(parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&[&st])
        .is_ok());
}
#[test]
fn path_endswith_as_function() {
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::new_path("/a/b/c.txt", PathFormat::Posix))
        .unwrap();
    let parsed = ParsedExpression::new("endswith(string(P), '.txt')").unwrap();
    assert!(parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&[&st])
        .is_ok());
}
#[test]
fn string_method_on_string_works() {
    assert!(evaluate_expression("'hello'.upper()", &SymbolTable::new()).is_ok());
}
#[test]
fn function_call_coerces_all_args() {
    assert!(evaluate_expression("startswith('hello', 'hel')", &SymbolTable::new()).is_ok());
}
#[test]
fn method_call_coerces_non_receiver() {
    assert!(evaluate_expression(
        "'hello world'.replace('world', 'rust')",
        &SymbolTable::new()
    )
    .is_ok());
}
#[test]
fn int_method_coercion_blocked() {
    let e = evaluate_expression("(42).upper()", &SymbolTable::new())
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "upper() is not available for int. Available for: string\n",
                "  (42).upper()\n",
                "  ~~~~^~~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

// === Tests ported from Python TestMethodCallNoReceiverCoercion ===

#[test]
fn path_startswith_as_method_fails() {
    let e = eval_posix_err("path('/foo/bar').startswith('/foo')");
    assert!(
        e.contains("startswith() is not available for path"),
        "got:\n{e}"
    );
}

#[test]
fn path_endswith_as_method_fails() {
    let e = eval_posix_err("path('/foo/bar').endswith('bar')");
    assert!(
        e.contains("endswith() is not available for path"),
        "got:\n{e}"
    );
}

#[test]
fn path_split_as_method_fails() {
    let e = eval_posix_err("path('/foo/bar').split('/')");
    assert!(e.contains("split() is not available for path"), "got:\n{e}");
}

#[test]
fn path_split_as_function_succeeds() {
    let r = eval_posix("split(path('/foo/bar'), '/')");
    assert_eq!(r.to_display_string(), "[\"\", \"foo\", \"bar\"]");
}

#[test]
fn path_startswith_as_function_with_coercion() {
    let r = eval_posix("startswith(path('/foo/bar'), '/foo')");
    assert_eq!(r.to_display_string(), "true");
}

#[test]
fn path_endswith_as_function_with_coercion() {
    let r = eval_posix("endswith(path('/foo/bar'), 'bar')");
    assert_eq!(r.to_display_string(), "true");
}

#[test]
fn string_startswith_method() {
    let r = eval("'hello'.startswith('hel')");
    assert_eq!(r.to_display_string(), "true");
}

#[test]
fn function_call_coerces_min() {
    let r = eval("min(1, 2.5)");
    assert_eq!(r.to_display_string(), "1.0");
}

#[test]
fn method_replace_result() {
    let r = eval("'hello'.replace('l', 'L')");
    assert_eq!(r.to_display_string(), "heLLo");
}
