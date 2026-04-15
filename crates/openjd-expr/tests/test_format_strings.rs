// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for format string interpolation ({{Param.Name}} and {{Expr.Name}} syntax).
//! Complements the inline unit tests in src/format_string.rs by exercising end-to-end
//! resolution through the expression evaluator with real symbol tables.

use openjd_expr::{symtab, ExprType, ExprValue, FormatString, SymbolTable};

fn resolve_str(input: &str, st: &SymbolTable) -> String {
    FormatString::new(input)
        .unwrap()
        .resolve_string(st, None, &[])
        .unwrap()
}

fn resolve_val(input: &str, st: &SymbolTable) -> ExprValue {
    FormatString::new(input)
        .unwrap()
        .resolve(st, None, &[])
        .unwrap()
}

fn resolve_err(input: &str, st: &SymbolTable) -> String {
    FormatString::new(input)
        .unwrap()
        .resolve_string(st, None, &[])
        .unwrap_err()
        .to_string()
}

fn parse_err(input: &str) -> String {
    FormatString::new(input).unwrap_err().to_string()
}

// === Parsing ===

#[test]
fn parse_literal_only() {
    let fs = FormatString::new("no interpolation here").unwrap();
    assert!(fs.is_literal());
}

#[test]
fn parse_single_simple_name() {
    let fs = FormatString::new("{{Param.Frame}}").unwrap();
    assert!(!fs.is_literal());
    assert_eq!(fs.expression_names(), vec!["Param.Frame"]);
}

#[test]
fn parse_multiple_interpolations() {
    let fs = FormatString::new("{{Param.A}}_{{Param.B}}").unwrap();
    assert_eq!(fs.expression_names(), vec!["Param.A", "Param.B"]);
}

#[test]
fn parse_complex_expression_not_in_names() {
    let fs = FormatString::new("{{Param.X + 1}}").unwrap();
    assert!(fs.has_complex_expressions());
    // Complex expressions are not returned by expression_names()
    assert!(fs.expression_names().is_empty());
}

#[test]
fn parse_empty_expression_error() {
    let err = parse_err("{{}}");
    assert!(err.contains("Empty expression"), "got: {err}");
}

#[test]
fn parse_missing_close_braces() {
    let err = parse_err("{{Param.X");
    assert!(err.contains("Braces mismatch"), "got: {err}");
}

#[test]
fn parse_missing_open_braces() {
    let err = parse_err("Param.X}}");
    assert!(err.contains("Missing opening braces"), "got: {err}");
}

// === Simple name resolution ===

#[test]
fn resolve_simple_string_param() {
    let st = symtab!("Param.Name" => "shot_01");
    assert_eq!(resolve_str("render_{{Param.Name}}", &st), "render_shot_01");
}

#[test]
fn resolve_simple_int_param() {
    let st = symtab!("Param.Frame" => 42);
    assert_eq!(resolve_str("frame_{{Param.Frame}}", &st), "frame_42");
}

#[test]
fn resolve_undefined_variable_error() {
    let err = resolve_err("{{Param.Missing}}", &SymbolTable::new());
    assert!(err.contains("Undefined variable"), "got: {err}");
}

// === Expression resolution ===

#[test]
fn resolve_arithmetic_expression() {
    let st = symtab!("Param.Frame" => 10);
    assert_eq!(resolve_str("{{Param.Frame + 1}}", &st), "11");
}

#[test]
fn resolve_string_method_expression() {
    let st = symtab!("Param.Name" => "hello");
    assert_eq!(resolve_str("{{Param.Name.upper()}}", &st), "HELLO");
}

#[test]
fn resolve_conditional_expression() {
    let st = symtab!("Param.X" => 5);
    assert_eq!(resolve_str("{{Param.X if Param.X > 3 else 0}}", &st), "5");
}

// === Typed resolution ===

#[test]
fn resolve_typed_single_expr_preserves_int() {
    let st = symtab!("Param.X" => 42);
    let val = resolve_val("{{Param.X}}", &st);
    assert!(matches!(val, ExprValue::Int(42)));
}

#[test]
fn resolve_typed_mixed_becomes_string() {
    let st = symtab!("Param.X" => 42);
    let val = resolve_val("prefix_{{Param.X}}", &st);
    assert!(matches!(val, ExprValue::String(ref s) if s == "prefix_42"));
}

#[test]
fn resolve_typed_single_expr_preserves_list() {
    let st = symtab!(
        "Param.Items" => ExprValue::make_list(
            vec![ExprValue::Int(1), ExprValue::Int(2), ExprValue::Int(3)],
            ExprType::INT,
        ).unwrap()
    );
    let val = resolve_val("{{Param.Items}}", &st);
    assert!(val.is_list());
    assert_eq!(val.to_display_string(), "[1, 2, 3]");
}

// === Validation ===

#[test]
fn validate_catches_undefined_variable() {
    let fs = FormatString::new("{{Param.Missing}}").unwrap();
    let lib = openjd_expr::default_library::get_default_library()
        .clone()
        .with_host_context();
    let result = fs.validate_expressions(&SymbolTable::new(), &lib);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Param.Missing"), "got: {err}");
}

#[test]
fn validate_passes_with_unresolved_types() {
    let fs = FormatString::new("{{Param.X + 1}}").unwrap();
    let st = symtab!("Param.X" => ExprValue::unresolved(ExprType::INT));
    let lib = openjd_expr::default_library::get_default_library()
        .clone()
        .with_host_context();
    assert!(fs.validate_expressions(&st, &lib).is_ok());
}

// === Null handling ===

#[test]
fn null_renders_as_empty_in_string_context() {
    let st = symtab!("Param.X" => ExprValue::Null);
    assert_eq!(resolve_str("a{{Param.X}}b", &st), "ab");
}

// === Whitespace in expressions ===

#[test]
fn whitespace_around_expression_is_trimmed() {
    let st = symtab!("Param.X" => 5);
    assert_eq!(resolve_str("{{  Param.X  }}", &st), "5");
}

// === Adjacent interpolations ===

#[test]
fn adjacent_interpolations() {
    let st = symtab!("Param.A" => "hello", "Param.B" => "world");
    assert_eq!(resolve_str("{{Param.A}}{{Param.B}}", &st), "helloworld");
}

// === Deeply nested dotted names ===

#[test]
fn deeply_nested_dotted_name() {
    let st = symtab!("Task.Param.Render.Frame" => 100);
    assert_eq!(resolve_str("f{{Task.Param.Render.Frame}}", &st), "f100");
}
