// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_parse_expression.py

use openjd_expr::{ExprValue, ParsedExpression, SymbolTable};
use std::collections::HashSet;

fn syms(expr: &str) -> HashSet<String> {
    ParsedExpression::new(expr).unwrap().accessed_symbols
}
fn funcs(expr: &str) -> HashSet<String> {
    ParsedExpression::new(expr).unwrap().called_functions
}
fn locals(expr: &str) -> HashSet<String> {
    ParsedExpression::new(expr).unwrap().local_bindings
}
fn set(items: &[&str]) -> HashSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

// ══════════════════════════════════════════════════════════════
// TestParseExpression — accessed_symbols
// ══════════════════════════════════════════════════════════════

#[test]
fn sym_simple_variable() {
    assert_eq!(syms("Param.InputFile"), set(&["Param.InputFile"]));
}
#[test]
fn sym_property_access() {
    assert_eq!(syms("Param.InputFile.stem"), set(&["Param.InputFile.stem"]));
}

#[test]
fn sym_method_call() {
    // .upper() is a method call, not part of the symbol
    let s = syms("Param.InputFile.stem.upper()");
    assert_eq!(s, set(&["Param.InputFile.stem"]));
}

#[test]
fn sym_arithmetic() {
    assert_eq!(
        syms("Param.Start + Param.End"),
        set(&["Param.Start", "Param.End"])
    );
}

#[test]
fn sym_conditional() {
    assert_eq!(
        syms("Param.A if Param.Flag else Param.B"),
        set(&["Param.A", "Param.Flag", "Param.B"])
    );
}

#[test]
fn sym_slicing() {
    assert_eq!(syms("Param.Items[1:3]"), set(&["Param.Items"]));
}

#[test]
fn sym_list_comprehension() {
    // x is loop variable, should NOT be in accessed_symbols
    assert_eq!(syms("[x for x in Param.Items]"), set(&["Param.Items"]));
}

#[test]
fn sym_list_comprehension_with_filter() {
    assert_eq!(
        syms("[x for x in Param.Items if x > Param.Min]"),
        set(&["Param.Items", "Param.Min"])
    );
}

#[test]
fn sym_list_comprehension_nested_expression() {
    assert_eq!(
        syms("[x * 2 for x in Param.Values]"),
        set(&["Param.Values"])
    );
}

#[test]
fn sym_list_comprehension_with_external_in_body() {
    assert_eq!(
        syms("[x + Param.Offset for x in Param.Items]"),
        set(&["Param.Items", "Param.Offset"])
    );
}

#[test]
fn sym_builtin_function_not_in_symbols() {
    assert_eq!(syms("string(Param.Count)"), set(&["Param.Count"]));
}

#[test]
fn sym_multiple_builtin_functions() {
    assert_eq!(
        syms("len(Param.Items) + int(Param.Value)"),
        set(&["Param.Items", "Param.Value"])
    );
}

#[test]
fn sym_min_max_functions() {
    assert_eq!(syms("min(Param.A, Param.B)"), set(&["Param.A", "Param.B"]));
}

#[test]
fn sym_method_on_int_literal_fails() {
    let e = match ParsedExpression::new("42.zfill(5)") {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected error"),
    };
    assert!(e.contains("Syntax error"), "got: {e}");
}

#[test]
fn sym_method_on_int_literal_with_parens() {
    assert_eq!(syms("(42).zfill(5)"), set(&[]));
}

#[test]
fn sym_name_matching_builtin_is_symbol_when_not_called() {
    // A variable named "min" should be a symbol when used as a value, not a call
    assert_eq!(syms("min + 1"), set(&["min"]));
}

#[test]
fn sym_name_matching_builtin_is_function_when_called() {
    // "min" in call position is a function, not a symbol
    assert_eq!(syms("min(Param.A, Param.B)"), set(&["Param.A", "Param.B"]));
    assert!(funcs("min(Param.A, Param.B)").contains("min"));
}

// ══════════════════════════════════════════════════════════════
// TestCalledFunctions
// ══════════════════════════════════════════════════════════════

#[test]
fn func_no_calls() {
    assert_eq!(funcs("Param.A + Param.B"), set(&[]));
}
#[test]
fn func_builtin() {
    assert_eq!(funcs("min(Param.A, Param.B)"), set(&["min"]));
}
#[test]
fn func_method() {
    assert_eq!(funcs("Param.Name.upper()"), set(&["upper"]));
}
#[test]
fn func_method_with_args() {
    assert!(funcs("Param.File.stem.replace('a', 'b')").contains("replace"));
}
#[test]
fn func_apply_path_mapping() {
    assert!(funcs("RawParam.File.apply_path_mapping()").contains("apply_path_mapping"));
}

#[test]
fn func_chained_methods() {
    let f = funcs("Param.Items.split(',').join(';')");
    assert!(f.contains("split"));
    assert!(f.contains("join"));
}

#[test]
fn func_in_list_comprehension() {
    assert!(funcs("[string(x) for x in Param.Values]").contains("string"));
}

#[test]
fn func_multiple() {
    let f = funcs("min(len(Param.A), len(Param.B))");
    assert!(f.contains("min"));
    assert!(f.contains("len"));
}

#[test]
fn func_in_conditional() {
    let f = funcs("Param.A.upper() if Param.Flag else Param.B.lower()");
    assert!(f.contains("upper"));
    assert!(f.contains("lower"));
}

#[test]
fn func_nested_method() {
    assert!(funcs("Param.Path.parent.name.upper()").contains("upper"));
}

#[test]
fn func_and_method_combined() {
    let f = funcs("len(Param.Name.upper())");
    assert!(f.contains("len"));
    assert!(f.contains("upper"));
}

// ══════════════════════════════════════════════════════════════
// TestLocalBindings
// ══════════════════════════════════════════════════════════════

#[test]
fn local_no_comprehension() {
    assert_eq!(locals("x + y"), set(&[]));
    assert_eq!(syms("x + y"), set(&["x", "y"]));
}

#[test]
fn local_simple_comprehension() {
    assert_eq!(locals("[x * 2 for x in items]"), set(&["x"]));
    assert_eq!(syms("[x * 2 for x in items]"), set(&["items"]));
}

#[test]
fn local_comprehension_with_filter() {
    assert_eq!(locals("[x for x in items if x > 0]"), set(&["x"]));
    assert_eq!(syms("[x for x in items if x > 0]"), set(&["items"]));
}

#[test]
fn local_nested_comprehension() {
    let l = locals("[[y for y in x] for x in items]");
    assert!(l.contains("x"));
    assert!(l.contains("y"));
    assert_eq!(syms("[[y for y in x] for x in items]"), set(&["items"]));
}

// ══════════════════════════════════════════════════════════════
// ParsedExpression.evaluate()
// ══════════════════════════════════════════════════════════════

#[test]
fn parsed_evaluate_basic() {
    let parsed = ParsedExpression::new("1 + 2").unwrap();
    let metrics = parsed
        .with_memory_limit(openjd_expr::DEFAULT_MEMORY_LIMIT)
        .evaluate_with_metrics(&[&SymbolTable::new()])
        .unwrap();
    assert_eq!(metrics.value.to_display_string(), "3");
    assert!(metrics.peak_memory > 0);
    assert!(metrics.operation_count > 0);
}

#[test]
fn parsed_evaluate_with_symtab() {
    let st = SymbolTable::from_pairs(vec![("X", ExprValue::Int(10)), ("Y", ExprValue::Int(20))])
        .unwrap();
    let parsed = ParsedExpression::new("X + Y").unwrap();
    let result = parsed.evaluate(&st).unwrap();
    assert_eq!(result.to_display_string(), "30");
}

// === Additional parse_expression tests ===
#[test]
fn parse_list_comprehension_with_filter() {
    let p = ParsedExpression::new("[x for x in L if x > 0]").unwrap();
    assert!(p.accessed_symbols.contains("L"));
}
#[test]
fn parse_list_comprehension_nested() {
    let p = ParsedExpression::new("[x + Y for x in L]").unwrap();
    assert!(p.accessed_symbols.contains("L"));
    assert!(p.accessed_symbols.contains("Y"));
}
#[test]
fn parse_same_name_outside_and_inside() {
    let p = ParsedExpression::new("x + [x for x in L]").unwrap();
    assert!(p.accessed_symbols.contains("x"));
    assert!(p.accessed_symbols.contains("L"));
}
#[test]
fn parse_no_function_calls() {
    let p = ParsedExpression::new("1 + 2").unwrap();
    assert!(p.called_functions.is_empty());
}
#[test]
fn parse_builtin_function() {
    let p = ParsedExpression::new("len('hello')").unwrap();
    assert!(p.called_functions.contains("len"));
}
#[test]
fn parse_method_with_args() {
    let p = ParsedExpression::new("'hello'.replace('l', 'r')").unwrap();
    assert!(p.called_functions.contains("replace"));
}
#[test]
fn parse_chained_methods() {
    let p = ParsedExpression::new("'hello'.upper().strip()").unwrap();
    assert!(p.called_functions.contains("upper"));
    assert!(p.called_functions.contains("strip"));
}
#[test]
fn parse_function_in_comprehension() {
    let p = ParsedExpression::new("[len(x) for x in L]").unwrap();
    assert!(p.called_functions.contains("len"));
}
#[test]
fn parse_apply_path_mapping() {
    let p = ParsedExpression::new("P.apply_path_mapping()").unwrap();
    assert!(p.called_functions.contains("apply_path_mapping"));
}

// ══════════════════════════════════════════════════════════════
// Missing tests ported from Python test_parse_expression.py
// ══════════════════════════════════════════════════════════════

// --- TestParseExpression: accessed_symbols ---

#[test]
fn sym_path_join() {
    assert_eq!(
        syms("Param.Dir / Param.File.name"),
        set(&["Param.Dir", "Param.File.name"])
    );
}

#[test]
fn sym_same_name_outside_and_inside_comprehension() {
    // x outside comprehension IS accessed, x inside is loop var (not accessed)
    assert_eq!(syms("[x] + [x for x in []]"), set(&["x"]));
}

#[test]
fn sym_different_name_outside_comprehension() {
    // y outside is accessed, x inside is loop var (not accessed)
    assert_eq!(syms("[y] + [x for x in []]"), set(&["y"]));
}

// --- TestCalledFunctions (exact set assertions for completeness) ---

#[test]
fn func_builtin_exact() {
    assert_eq!(funcs("min(Param.A, Param.B)"), set(&["min"]));
}

#[test]
fn func_method_exact() {
    assert_eq!(funcs("Param.Name.upper()"), set(&["upper"]));
}

#[test]
fn func_method_with_args_exact() {
    assert_eq!(
        funcs("Param.File.stem.replace('a', 'b')"),
        set(&["replace"])
    );
}

#[test]
fn func_apply_path_mapping_exact() {
    assert_eq!(
        funcs("RawParam.File.apply_path_mapping()"),
        set(&["apply_path_mapping"])
    );
}

#[test]
fn func_chained_methods_exact() {
    assert_eq!(
        funcs("Param.Items.split(',').join(';')"),
        set(&["split", "join"])
    );
}

#[test]
fn func_in_list_comprehension_exact() {
    assert_eq!(funcs("[string(x) for x in Param.Values]"), set(&["string"]));
}

#[test]
fn func_multiple_exact() {
    assert_eq!(
        funcs("min(len(Param.A), len(Param.B))"),
        set(&["min", "len"])
    );
}

#[test]
fn func_in_conditional_exact() {
    assert_eq!(
        funcs("Param.A.upper() if Param.Flag else Param.B.lower()"),
        set(&["upper", "lower"])
    );
}

#[test]
fn func_nested_method_exact() {
    assert_eq!(funcs("Param.Path.parent.name.upper()"), set(&["upper"]));
}

#[test]
fn func_and_method_combined_exact() {
    assert_eq!(funcs("len(Param.Name.upper())"), set(&["len", "upper"]));
}

// --- TestDictConvenience ---

#[test]
fn evaluate_expression_with_dict_values() {
    let st =
        SymbolTable::from_pairs(vec![("X", ExprValue::Int(1)), ("Y", ExprValue::Int(2))]).unwrap();
    let result = openjd_expr::evaluate_expression("X + Y", &st).unwrap();
    assert_eq!(result.to_display_string(), "3");
}

// --- TestLocalBindings ---

#[test]
fn local_multiple_generators_rejected() {
    match ParsedExpression::new("[x + y for x in a for y in b]") {
        Err(e) => assert!(e.to_string().contains("Multiple 'for'"), "got: {}", e),
        Ok(_) => panic!("expected error"),
    }
}

#[test]
fn local_independent_branches() {
    let p = ParsedExpression::new(
        "[x for x in Param.Values] if Param.Boolean else [[y for y in z] for z in Param.Nested]",
    )
    .unwrap();
    assert_eq!(p.local_bindings, set(&["x", "y", "z"]));
    assert_eq!(
        p.accessed_symbols,
        set(&["Param.Values", "Param.Boolean", "Param.Nested"])
    );
}

#[test]
fn local_nested_shadowing_rejected() {
    match ParsedExpression::new("[[x for x in Param.A] for x in Param.B]") {
        Err(e) => assert!(e.to_string().contains("shadows"), "got: {}", e),
        Ok(_) => panic!("expected error"),
    }
}

#[test]
fn local_sibling_comprehensions_same_var_allowed() {
    let p = ParsedExpression::new("[x for x in a] + [x for x in b] + [x for x in c]").unwrap();
    assert_eq!(p.local_bindings, set(&["x"]));
    assert_eq!(p.accessed_symbols, set(&["a", "b", "c"]));
}
