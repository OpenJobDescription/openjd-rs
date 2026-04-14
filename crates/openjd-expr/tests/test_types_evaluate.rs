// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_types_evaluate.py

use openjd_expr::{evaluate_expression, SymbolTable, ExprValue};


#[allow(dead_code)]
fn eval(expr: &str) -> ExprValue { evaluate_expression(expr, &SymbolTable::new()).unwrap() }

#[allow(dead_code)]
fn eval_fails(expr: &str) -> bool { evaluate_expression(expr, &SymbolTable::new()).is_err() }

fn assert_err(expr: &str, expected: &[&str]) {
    let e = evaluate_expression(expr, &SymbolTable::new()).unwrap_err().to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

#[allow(dead_code)]
fn eval_err(expr: &str) -> String { evaluate_expression(expr, &SymbolTable::new()).unwrap_err().message() }

// === TestLiteralTypes ===
#[test] fn empty_list_type() { assert_eq!(eval("[]").expr_type().to_string(), "list[nulltype]"); }
#[test] fn int_list_type() { assert_eq!(eval("[1, 2, 3]").expr_type().to_string(), "list[int]"); }
#[test] fn float_list_type() { assert_eq!(eval("[1.0, 2.0]").expr_type().to_string(), "list[float]"); }
#[test] fn string_list_type() { assert_eq!(eval("['a', 'b']").expr_type().to_string(), "list[string]"); }
#[test] fn bool_list_type() { assert_eq!(eval("[True, False]").expr_type().to_string(), "list[bool]"); }

#[test]
fn nested_list_type() {
    let mut st = SymbolTable::new();
    st.set("nested", ExprValue::make_list(
        vec![
            ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Int(2)], openjd_expr::ExprType::INT).unwrap(),
            ExprValue::make_list(vec![ExprValue::Int(3), ExprValue::Int(4)], openjd_expr::ExprType::INT).unwrap(),
        ],
        openjd_expr::ExprType::list(openjd_expr::ExprType::INT),
    ).unwrap()).unwrap();
    assert_eq!(evaluate_expression("nested", &st).unwrap().expr_type().to_string(), "list[list[int]]");
}

#[test] fn int_type() { assert_eq!(eval("42").expr_type().to_string(), "int"); }
#[test] fn float_type() { assert_eq!(eval("3.14").expr_type().to_string(), "float"); }
#[test] fn string_type() { assert_eq!(eval("'hello'").expr_type().to_string(), "string"); }
#[test] fn bool_type() { assert_eq!(eval("True").expr_type().to_string(), "bool"); }
#[test] fn null_type() { assert_eq!(eval("null").expr_type().to_string(), "nulltype"); }
#[test] fn none_type() { assert_eq!(eval("None").expr_type().to_string(), "nulltype"); }

// === TestJsonLiterals ===
#[test] fn json_null() { assert!(matches!(eval("null"), ExprValue::Null)); }
#[test] fn json_true() { assert_eq!(eval("true").to_display_string(), "true"); }
#[test] fn json_false() { assert_eq!(eval("false").to_display_string(), "false"); }
#[test] fn json_mixed() { assert_eq!(eval("true if True else false").to_display_string(), "true"); }

// === TestTypeConversion ===
#[test] fn string_from_string() { assert_eq!(eval("string(\"abc\")").to_display_string(), "abc"); }
#[test] fn int_from_int() { assert_eq!(eval("int(42)").to_display_string(), "42"); }
#[test] fn float_from_float() { assert_eq!(eval("float(3.14)").to_display_string(), "3.14"); }

#[test]
fn bool_from_bool() {
    assert_eq!(eval("bool(True)").to_display_string(), "true");
    assert_eq!(eval("bool(False)").to_display_string(), "false");
    assert_eq!(eval("bool(true)").to_display_string(), "true");
    assert_eq!(eval("bool(false)").to_display_string(), "false");
}

#[test]
fn bool_from_null() {
    assert_eq!(eval("bool(null)").to_display_string(), "false");
    assert_eq!(eval("bool(None)").to_display_string(), "false");
}

#[test]
fn bool_from_int() {
    assert_eq!(eval("bool(0)").to_display_string(), "false");
    assert_eq!(eval("bool(1)").to_display_string(), "true");
    assert_eq!(eval("bool(-1)").to_display_string(), "true");
}

#[test]
fn bool_from_float() {
    assert_eq!(eval("bool(0.0)").to_display_string(), "false");
    assert_eq!(eval("bool(1.0)").to_display_string(), "true");
}

#[test]
fn bool_from_string_true() {
    for s in &["1", "true", "TRUE", "on", "yes"] {
        assert_eq!(eval(&format!("bool(\"{}\")", s)).to_display_string(), "true", "failed for {}", s);
    }
}

#[test]
fn bool_from_string_false() {
    for s in &["0", "false", "FALSE", "off", "no"] {
        assert_eq!(eval(&format!("bool(\"{}\")", s)).to_display_string(), "false", "failed for {}", s);
    }
}

#[test] fn bool_from_string_invalid() {
    assert_err("bool(\"invalid\")", &["Cannot convert 'invalid' to bool. Expected one of: 1, true, on, yes, 0, false, off, no\n", "  bool(\"invalid\")\n", "  ^~~~~~~~~~~~~~~"]);
}
#[test] fn bool_from_empty_string_rejected() {
    assert_err("bool(\"\")", &["Cannot convert '' to bool. Expected one of: 1, true, on, yes, 0, false, off, no\n", "  bool(\"\")\n", "  ^~~~~~~"]);
}
#[test] fn bool_from_path_rejected() {
    assert_err("bool(path(\"/tmp\"))", &["Cannot convert path to bool"]);
}
#[test] fn bool_from_list_rejected() {
    assert_err("bool([1, 2, 3])", &["Cannot convert list to bool"]);
}

#[test] fn string_from_int() { assert_eq!(eval("string(42)").to_display_string(), "42"); }
#[test] fn string_from_bool() { assert_eq!(eval("string(True)").to_display_string(), "true"); }
#[test] fn string_from_null() { assert_eq!(eval("string(null)").to_display_string(), "null"); assert_eq!(eval("string(None)").to_display_string(), "null"); }
#[test] fn string_from_list_int() { assert_eq!(eval("string([1, 2, 3])").to_display_string(), "[1, 2, 3]"); }
#[test] fn string_from_list_string() { assert_eq!(eval("string(['a', 'b', 'c'])").to_display_string(), "[\"a\", \"b\", \"c\"]"); }
#[test] fn string_from_list_float() { assert_eq!(eval("string([1.5, 2.5])").to_display_string(), "[1.5, 2.5]"); }
#[test] fn string_from_list_bool() { assert_eq!(eval("string([true, false])").to_display_string(), "[true, false]"); }
#[test] fn string_from_list_nested() { assert_eq!(eval("string([[1, 2], [3, 4]])").to_display_string(), "[[1, 2], [3, 4]]"); }
#[test] fn string_from_list_empty() { assert_eq!(eval("string([])").to_display_string(), "[]"); }
#[test] fn int_from_string() { assert_eq!(eval("int('42')").to_display_string(), "42"); }
#[test] fn int_from_string_invalid() {
    assert_err("int('abc')", &["Cannot convert 'abc' to int\n", "  int('abc')\n", "  ^~~~~~~~~~"]);
}
#[test] fn float_from_int() { assert_eq!(eval("float(42)").to_display_string(), "42.0"); }

// ══════════════════════════════════════════════════════════════
// Prohibited conversions per RFC 0006
// ══════════════════════════════════════════════════════════════

#[test]
fn int_from_bool_rejected() {
    // Spec: int(value: int | float | string) — bool is not accepted.
    assert_err("int(True)", &["No matching signature for int(bool)"]);
}

#[test]
fn bool_from_path_is_type_error() {
    // Spec: "Calling bool() on path or list[T] values is an error."
    assert_err("bool(path('/tmp'))", &["Cannot convert path to bool"]);
}

#[test]
fn bool_from_list_is_type_error() {
    // Spec: "Calling bool() on path or list[T] values is an error."
    assert_err("bool([1, 2, 3])", &["Cannot convert list to bool"]);
}

// ══════════════════════════════════════════════════════════════
// Extra path/join functions not in RFC 0006
// ══════════════════════════════════════════════════════════════

#[test]
fn path_join_not_in_spec() {
    assert_err("path_join('/a', 'b')", &["Unknown function: 'path_join'"]);
}

#[test]
fn add_string_path_coerces_to_string_concat() {
    // With explicit __add__(string, path) removed, path coerces to string
    // and __add__(string, string) matches — this is string concatenation, not path joining.
    assert_eq!(eval("'prefix' + path('/tmp')").to_display_string(), "prefix/tmp");
}

#[test]
fn add_path_path_coerces_to_string_concat() {
    // With explicit __add__(path, path) removed, both coerce to string.
    assert_eq!(eval("path('/a') + path('/b')").to_display_string(), "/a/b");
}

#[test]
fn join_reversed_args_not_in_spec() {
    assert_err("','.join(['a', 'b'])", &["join() is not available for string"]);
}
