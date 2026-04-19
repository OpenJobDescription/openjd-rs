// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_function_context.py

use openjd_expr::*;

#[allow(dead_code)]
fn eval(expr: &str) -> ExprValue {
    evaluate_expression(expr, &SymbolTable::new()).unwrap()
}

fn eval_with_host_context(expr: &str) -> ExprValue {
    let parsed = ParsedExpression::new(expr).unwrap();
    let st = SymbolTable::new();
    let symtabs = [&st];
    let lib = default_library::get_default_library()
        .clone()
        .with_host_context(Vec::<PathMappingRule>::new());
    parsed.with_library(&lib).evaluate(&symtabs).unwrap()
}

#[allow(dead_code)]
fn eval_with_host_context_fails(expr: &str) -> bool {
    let parsed = ParsedExpression::new(expr).unwrap();
    let st = SymbolTable::new();
    let symtabs = [&st];
    let lib = default_library::get_default_library()
        .clone()
        .with_host_context(Vec::<PathMappingRule>::new());
    parsed.with_library(&lib).evaluate(&symtabs).is_err()
}

// === Default library availability ===
#[test]
fn path_functions_available_without_host_context() {
    assert!(evaluate_expression("path('/a/b').name", &SymbolTable::new()).is_ok());
}
#[test]
fn string_functions_available() {
    assert!(evaluate_expression("'hello'.upper()", &SymbolTable::new()).is_ok());
}
#[test]
fn math_functions_available() {
    assert!(evaluate_expression("abs(-5)", &SymbolTable::new()).is_ok());
}
#[test]
fn repr_functions_available() {
    assert!(evaluate_expression("repr_py(42)", &SymbolTable::new()).is_ok());
}
#[test]
fn regex_functions_available() {
    assert!(evaluate_expression(r"re_search('abc', r'\w+')", &SymbolTable::new()).is_ok());
}
#[test]
fn list_functions_available() {
    assert!(evaluate_expression("sorted([3, 1, 2])", &SymbolTable::new()).is_ok());
}
#[test]
fn conversion_functions_available() {
    assert!(evaluate_expression("int('42')", &SymbolTable::new()).is_ok());
}

// === Host context ===
#[test]
fn default_library_no_host_context() {
    let lib = default_library::get_default_library();
    assert!(!lib.host_context_enabled);
}
#[test]
fn with_host_context_returns_new_library() {
    let lib = default_library::get_default_library().clone();
    let result = lib
        .clone()
        .with_host_context(Vec::<openjd_expr::PathMappingRule>::new());
    assert!(result.host_context_enabled);
    assert!(!lib.host_context_enabled);
}
#[test]
fn with_host_context_chaining() {
    let lib = default_library::get_default_library()
        .clone()
        .with_host_context(Vec::<openjd_expr::PathMappingRule>::new());
    assert!(lib.host_context_enabled);
}

// === apply_path_mapping availability ===
#[test]
fn not_available_without_host_context() {
    let e = evaluate_expression("apply_path_mapping('/path')", &SymbolTable::new())
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Unknown function: 'apply_path_mapping'\n",
                "  apply_path_mapping('/path')\n",
                "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}
#[test]
fn not_available_with_default_library() {
    let e = evaluate_expression("apply_path_mapping('/path')", &SymbolTable::new())
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("Unknown function: 'apply_path_mapping'"),
        "got:\n{e}"
    );
}
#[test]
fn available_with_host_context() {
    let r = eval_with_host_context("apply_path_mapping('/some/path')");
    assert!(matches!(r, ExprValue::Path { .. }));
}
#[test]
fn method_syntax_without_host_context() {
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("/path".into())).unwrap();
    let e = evaluate_expression("P.apply_path_mapping()", &st)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Unknown function: 'apply_path_mapping'\n",
                "  P.apply_path_mapping()\n",
                "  ~~^~~~~~~~~~~~~~~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}
#[test]
fn method_syntax_with_host_context() {
    let parsed = ParsedExpression::new("P.apply_path_mapping()").unwrap();
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("/some/path".into())).unwrap();
    let lib = default_library::get_default_library()
        .clone()
        .with_host_context(Vec::<openjd_expr::PathMappingRule>::new());
    let r = parsed.with_library(&lib).evaluate(&[&st]).unwrap();
    assert!(matches!(r, ExprValue::Path { .. }));
}

// === Path mapping rules ===
#[test]
fn with_path_mapping_rules() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/old".into(),
        destination_path: "/new".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("/old/file.txt".into()))
        .unwrap();
    let lib = default_library::get_default_library()
        .clone()
        .with_host_context(vec![rule]);
    let parsed = ParsedExpression::new("P.apply_path_mapping()").unwrap();
    let symtabs = [&st];
    let r = parsed
        .with_library(&lib)
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap();
    assert_eq!(r.to_display_string(), "/new/file.txt");
}
#[test]
fn unmatched_path_unchanged() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/old".into(),
        destination_path: "/new".into(),
    };
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::String("/other/file.txt".into()))
        .unwrap();
    let lib = default_library::get_default_library()
        .clone()
        .with_host_context(vec![rule]);
    let parsed = ParsedExpression::new("P.apply_path_mapping()").unwrap();
    let symtabs = [&st];
    let r = parsed
        .with_library(&lib)
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap();
    assert_eq!(r.to_display_string(), "/other/file.txt");
}
#[test]
fn no_rules_returns_path_unchanged() {
    // Use Posix format so the path isn't normalized to backslashes on Windows
    let lib = default_library::get_default_library()
        .clone()
        .with_host_context(Vec::<openjd_expr::PathMappingRule>::new());
    let parsed = ParsedExpression::new("apply_path_mapping('/any/path')").unwrap();
    let st = SymbolTable::new();
    let symtabs = [&st];
    let r = parsed
        .with_library(&lib)
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap();
    assert_eq!(r.to_display_string(), "/any/path");
}
#[test]
fn submission_functions_available() {
    // All non-host functions should work without host context
    assert!(evaluate_expression("len('hello')", &SymbolTable::new()).is_ok());
    assert!(evaluate_expression("sorted([3, 1, 2])", &SymbolTable::new()).is_ok());
    assert!(evaluate_expression("path('/a/b').name", &SymbolTable::new()).is_ok());
}

// === Submission context with value assertions (from Python parametrized tests) ===
#[test]
fn submission_arithmetic_value() {
    let r = evaluate_expression("1 + 2", &SymbolTable::new()).unwrap();
    assert_eq!(r, ExprValue::Int(3));
}
#[test]
fn submission_min_value() {
    let r = evaluate_expression("min(5, 3)", &SymbolTable::new()).unwrap();
    assert_eq!(r, ExprValue::Int(3));
}
#[test]
fn submission_upper_value() {
    let r = evaluate_expression("upper('hello')", &SymbolTable::new()).unwrap();
    assert_eq!(r.to_display_string(), "HELLO");
}
#[test]
fn submission_len_value() {
    let r = evaluate_expression("len('test')", &SymbolTable::new()).unwrap();
    assert_eq!(r, ExprValue::Int(4));
}

// === Path functions without host context with value assertions ===
#[test]
fn path_stem_without_host_context() {
    let mut st = SymbolTable::new();
    st.set(
        "P",
        ExprValue::new_path("/projects/render.exr", PathFormat::Posix),
    )
    .unwrap();
    let parsed = ParsedExpression::new("P.stem").unwrap();
    let symtabs = [&st];
    let r = parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap();
    assert_eq!(r.to_display_string(), "render");
}
#[test]
fn path_suffix_without_host_context() {
    let mut st = SymbolTable::new();
    st.set(
        "P",
        ExprValue::new_path("/projects/render.exr", PathFormat::Posix),
    )
    .unwrap();
    let parsed = ParsedExpression::new("P.suffix").unwrap();
    let symtabs = [&st];
    let r = parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap();
    assert_eq!(r.to_display_string(), ".exr");
}
#[test]
fn path_with_suffix_without_host_context() {
    let mut st = SymbolTable::new();
    st.set(
        "P",
        ExprValue::new_path("/projects/render.exr", PathFormat::Posix),
    )
    .unwrap();
    let parsed = ParsedExpression::new("with_suffix(P, '.png')").unwrap();
    let symtabs = [&st];
    let r = parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap();
    assert!(
        r.to_display_string().ends_with("render.png"),
        "got: {}",
        r.to_display_string()
    );
}

// === Function-syntax apply_path_mapping with rules ===
#[test]
fn function_syntax_with_path_mapping_rules() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/old/path".into(),
        destination_path: "/new/path".into(),
    };
    let lib = default_library::get_default_library()
        .clone()
        .with_host_context(vec![rule]);
    let parsed = ParsedExpression::new("apply_path_mapping('/old/path/file.txt')").unwrap();
    let st = SymbolTable::new();
    let symtabs = [&st];
    let r = parsed
        .with_library(&lib)
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap();
    assert_eq!(r.to_display_string(), "/new/path/file.txt");
}
#[test]
fn function_syntax_unmatched_path_unchanged() {
    let rule = PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/specific/path".into(),
        destination_path: "/mapped/path".into(),
    };
    let lib = default_library::get_default_library()
        .clone()
        .with_host_context(vec![rule]);
    let parsed = ParsedExpression::new("apply_path_mapping('/other/path/file.txt')").unwrap();
    let st = SymbolTable::new();
    let symtabs = [&st];
    let r = parsed
        .with_library(&lib)
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap();
    assert_eq!(r.to_display_string(), "/other/path/file.txt");
}
