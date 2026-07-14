// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test_function_context.py

use openjd_expr::*;

// Build a host-context library (no path-mapping rules) for use in the
// expression-evaluation helpers below. Goes through the profile-cached
// construction path — same library the session path uses.
fn host_lib_empty() -> std::sync::Arc<FunctionLibrary> {
    FunctionLibrary::for_profile(
        &ExprProfile::current()
            .with_host_context(HostContext::with_rules(Vec::<PathMappingRule>::new())),
    )
}

#[allow(dead_code)]
fn eval(expr: &str) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap()
}

fn eval_with_host_context(expr: &str) -> ExprValue {
    let parsed = ParsedExpression::new(expr).unwrap();
    let st = SymbolTable::new();
    let symtabs = [&st];
    let lib = host_lib_empty();
    parsed.with_library(&lib).evaluate(&symtabs).unwrap()
}

#[allow(dead_code)]
fn eval_with_host_context_fails(expr: &str) -> bool {
    let parsed = ParsedExpression::new(expr).unwrap();
    let st = SymbolTable::new();
    let symtabs = [&st];
    let lib = host_lib_empty();
    parsed.with_library(&lib).evaluate(&symtabs).is_err()
}

// === Default library availability ===
#[test]
fn path_functions_available_without_host_context() {
    assert!(ParsedExpression::new("path('/a/b').name")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_ok());
}
#[test]
fn string_functions_available() {
    assert!(ParsedExpression::new("'hello'.upper()")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_ok());
}
#[test]
fn math_functions_available() {
    assert!(ParsedExpression::new("abs(-5)")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_ok());
}
#[test]
fn repr_functions_available() {
    assert!(ParsedExpression::new("repr_py(42)")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_ok());
}
#[test]
fn regex_functions_available() {
    assert!(ParsedExpression::new(r"re_search('abc', r'\w+')")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_ok());
}
#[test]
fn list_functions_available() {
    assert!(ParsedExpression::new("sorted([3, 1, 2])")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_ok());
}
#[test]
fn conversion_functions_available() {
    assert!(ParsedExpression::new("int('42')")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_ok());
}

// === Host context ===
#[test]
fn default_library_no_host_context() {
    // A profile with HostContext::None yields a library where
    // apply_path_mapping is absent.
    let lib = FunctionLibrary::for_profile(&ExprProfile::current());
    assert!(lib.get_signatures("apply_path_mapping").is_empty());
}
#[test]
fn with_host_context_returns_new_library() {
    // for_profile's cache returns the same Arc for two calls with the
    // same profile, but HostContext::None and HostContext::WithRules
    // are distinct profiles that yield distinct libraries — the first
    // lacks apply_path_mapping, the second has it.
    let base = FunctionLibrary::for_profile(&ExprProfile::current());
    let with_host = host_lib_empty();
    assert!(!with_host.get_signatures("apply_path_mapping").is_empty());
    assert!(base.get_signatures("apply_path_mapping").is_empty());
}
#[test]
fn with_host_context_chaining() {
    let lib = host_lib_empty();
    assert!(!lib.get_signatures("apply_path_mapping").is_empty());
}

// === apply_path_mapping availability ===
#[test]
fn not_available_without_host_context() {
    let e = ParsedExpression::new("apply_path_mapping('/path')")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
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
    let e = ParsedExpression::new("apply_path_mapping('/path')")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
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
    let e = ParsedExpression::new("P.apply_path_mapping()")
        .and_then(|p| p.evaluate(&st))
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
    let lib = host_lib_empty();
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
    let lib = FunctionLibrary::for_profile(
        &ExprProfile::current().with_host_context(HostContext::with_rules(vec![rule])),
    );
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
    let lib = FunctionLibrary::for_profile(
        &ExprProfile::current().with_host_context(HostContext::with_rules(vec![rule])),
    );
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
    let lib = host_lib_empty();
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
    assert!(ParsedExpression::new("len('hello')")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_ok());
    assert!(ParsedExpression::new("sorted([3, 1, 2])")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_ok());
    assert!(ParsedExpression::new("path('/a/b').name")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_ok());
}

// === Submission context with value assertions (from Python parametrized tests) ===
#[test]
fn submission_arithmetic_value() {
    let r = ParsedExpression::new("1 + 2")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap();
    assert_eq!(r, ExprValue::Int(3));
}
#[test]
fn submission_min_value() {
    let r = ParsedExpression::new("min(5, 3)")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap();
    assert_eq!(r, ExprValue::Int(3));
}
#[test]
fn submission_upper_value() {
    let r = ParsedExpression::new("upper('hello')")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap();
    assert_eq!(r.to_display_string(), "HELLO");
}
#[test]
fn submission_len_value() {
    let r = ParsedExpression::new("len('test')")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap();
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
    let lib = FunctionLibrary::for_profile(
        &ExprProfile::current().with_host_context(HostContext::with_rules(vec![rule])),
    );
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
    let lib = FunctionLibrary::for_profile(
        &ExprProfile::current().with_host_context(HostContext::with_rules(vec![rule])),
    );
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
