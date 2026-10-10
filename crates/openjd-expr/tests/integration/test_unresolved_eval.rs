// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test_unresolved_eval.py — evaluating with unresolved values.

use openjd_expr::*;

fn st_unresolved(pairs: Vec<(&str, &str)>) -> SymbolTable {
    let mut st = SymbolTable::new();
    for (k, t) in pairs {
        st.set(k, ExprValue::unresolved(ExprType::parse(t).unwrap()))
            .unwrap();
    }
    st
}

fn eval_u(expr: &str, st: &SymbolTable) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(st))
        .unwrap()
}

#[allow(dead_code)]
fn eval_u_err(expr: &str, st: &SymbolTable) -> String {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(st))
        .unwrap_err()
        .message()
}

fn assert_err(expr: &str, expected: &[&str]) {
    let e = ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

fn assert_err_w(expr: &str, st: &SymbolTable, expected: &[&str]) {
    let e = ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(st))
        .unwrap_err()
        .to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

fn tp(s: &str) -> ExprType {
    ExprType::parse(s).unwrap()
}

// ══════════════════════════════════════════════════════════════
// TestUnknownPassThrough
// ══════════════════════════════════════════════════════════════

#[test]
fn passthrough_simple_name() {
    let r = eval_u("X", &st_unresolved(vec![("X", "int")]));
    assert_eq!(r.expr_type(), tp("unresolved[int]"));
}

#[test]
fn passthrough_dotted_name() {
    let r = eval_u("Param.Count", &st_unresolved(vec![("Param.Count", "int")]));
    assert_eq!(r.expr_type(), tp("unresolved[int]"));
}

#[test]
fn passthrough_different_types() {
    for t in &[
        "int",
        "float",
        "string",
        "path",
        "bool",
        "list[int]",
        "list[string]",
    ] {
        let r = eval_u("X", &st_unresolved(vec![("X", t)]));
        assert_eq!(r.expr_type(), tp(&format!("unresolved[{t}]")));
    }
}

#[test]
fn passthrough_unconstrained() {
    let r = eval_u("X", &st_unresolved(vec![("X", "any")]));
    assert_eq!(r.expr_type(), tp("unresolved"));
}

// ══════════════════════════════════════════════════════════════
// TestUnknownFunctionCalls
// ══════════════════════════════════════════════════════════════

#[test]
fn func_len_of_unknown_list() {
    let r = eval_u("len(X)", &st_unresolved(vec![("X", "list[int]")]));
    assert_eq!(r.expr_type(), tp("unresolved[int]"));
}

// ══════════════════════════════════════════════════════════════
// TestUnknownCoercion
// ══════════════════════════════════════════════════════════════

#[test]
fn coercion_unknown_int_plus_float() {
    let r = eval_u("X + 1.0", &st_unresolved(vec![("X", "int")]));
    assert_eq!(r.expr_type(), tp("unresolved[float]"));
}

// ══════════════════════════════════════════════════════════════
// TestUnknownComparisons
// ══════════════════════════════════════════════════════════════

#[test]
fn cmp_unknown_eq_concrete() {
    let r = eval_u("X == 5", &st_unresolved(vec![("X", "int")]));
    assert_eq!(r.expr_type(), tp("unresolved[bool]"));
}

#[test]
fn cmp_unknown_lt_concrete() {
    let r = eval_u("X < 10", &st_unresolved(vec![("X", "int")]));
    assert_eq!(r.expr_type(), tp("unresolved[bool]"));
}

// ══════════════════════════════════════════════════════════════
// TestUnknownBoolOps
// ══════════════════════════════════════════════════════════════

#[test]
fn boolop_unknown_or_false() {
    let r = eval_u("X or false", &st_unresolved(vec![("X", "bool")]));
    assert_eq!(r.expr_type(), tp("unresolved[bool]"));
}

#[test]
fn boolop_unknown_and_true() {
    let r = eval_u("X and true", &st_unresolved(vec![("X", "bool")]));
    assert_eq!(r.expr_type(), tp("unresolved[bool]"));
}

// ══════════════════════════════════════════════════════════════
// TestUnknownIfElse
// ══════════════════════════════════════════════════════════════

#[test]
fn ifelse_unknown_condition_both_succeed() {
    let st = st_unresolved(vec![("cond", "bool"), ("X", "int"), ("Y", "string")]);
    let r = eval_u("X if cond else Y", &st);
    assert_eq!(r.expr_type(), tp("unresolved[int | string]"));
}

#[test]
fn ifelse_unknown_condition_same_types() {
    let st = st_unresolved(vec![("cond", "bool"), ("X", "int"), ("Y", "int")]);
    let r = eval_u("X if cond else Y", &st);
    assert_eq!(r.expr_type(), tp("unresolved[int]"));
}

#[test]
fn ifelse_unknown_condition_concrete_branches() {
    let st = st_unresolved(vec![("cond", "bool")]);
    let r = eval_u("1 if cond else 'hello'", &st);
    assert_eq!(r.expr_type(), tp("unresolved[int | string]"));
}

#[test]
fn ifelse_known_condition_unknown_branches() {
    let st = st_unresolved(vec![("X", "int"), ("Y", "string")]);
    let r1 = eval_u("X if True else Y", &st);
    assert_eq!(r1.expr_type(), tp("unresolved[int]"));
    let r2 = eval_u("X if False else Y", &st);
    assert_eq!(r2.expr_type(), tp("unresolved[string]"));
}

#[test]
fn ifelse_unknown_condition_one_branch_fails() {
    let st = st_unresolved(vec![("cond", "bool"), ("X", "int")]);
    let r = eval_u("X if cond else X + 'bad'", &st);
    assert_eq!(r.expr_type(), tp("unresolved[int]"));
}

#[test]
fn ifelse_unknown_condition_other_branch_fails() {
    let st = st_unresolved(vec![("cond", "bool"), ("X", "int")]);
    let r = eval_u("X + 'bad' if cond else X", &st);
    assert_eq!(r.expr_type(), tp("unresolved[int]"));
}

#[test]
fn ifelse_unknown_condition_both_fail() {
    let st = st_unresolved(vec![("cond", "bool"), ("X", "int"), ("Y", "path")]);
    assert_err_w("X + 'a' if cond else Y * 'b'", &st, &["Both branches fail"]);
}

// ══════════════════════════════════════════════════════════════
// TestUnknownListLiterals
// ══════════════════════════════════════════════════════════════

#[test]
fn list_all_unknown_same() {
    let st = st_unresolved(vec![("X", "int"), ("Y", "int")]);
    let r = eval_u("[X, Y]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[int]]"));
}

#[test]
fn list_mix_concrete_and_unknown() {
    let st = st_unresolved(vec![("X", "int")]);
    let r = eval_u("[1, X, 3]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[int]]"));
}

// ══════════════════════════════════════════════════════════════
// TestUnknownListComprehensions
// ══════════════════════════════════════════════════════════════

#[test]
fn comp_unknown_list_iterable() {
    let st = st_unresolved(vec![("X", "list[int]")]);
    let r = eval_u("[x for x in X]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[int]]"));
}

// ══════════════════════════════════════════════════════════════
// Concrete iterable, unresolved filter — per-element inclusion is
// undecidable, so the whole comprehension concludes unresolved,
// exactly as the unresolved-iterable path does. The partially-bound
// symbol state is what job creation evaluates under (`Param.*`
// concrete, `Task.*`/`Session.*` unresolved); a hard error here
// rejected templates at submission that pass template validation and
// run cleanly.
// ══════════════════════════════════════════════════════════════

#[test]
fn comp_concrete_iterable_unresolved_filter() {
    let mut st = SymbolTable::new();
    st.set("S", ExprValue::unresolved(ExprType::STRING))
        .unwrap();
    let r = eval_u("[x for x in 'a,b,c'.split(',') if x != S]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[string]]"));
}

#[test]
fn comp_concrete_range_iterable_unresolved_filter() {
    let mut st = SymbolTable::new();
    st.set("N", ExprValue::unresolved(ExprType::INT)).unwrap();
    let r = eval_u("[x * 2 for x in range(3) if x > N]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[int]]"));
}

/// The body's type is derived with the loop variable unresolved, not
/// with the concrete element that triggered the bail-out — evaluating
/// the body on an element the runtime filter may exclude could raise a
/// spurious value error.
#[test]
fn comp_unresolved_filter_shields_body_value_error_on_excluded_element() {
    let mut st = SymbolTable::new();
    st.set("N", ExprValue::unresolved(ExprType::INT)).unwrap();
    let r = eval_u("[10 // x for x in [0, 2] if x > N]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[int]]"));
}

/// A filter that can never be a boolean is still refused, matching the
/// unresolved-iterable path's type check.
#[test]
fn comp_concrete_iterable_unresolved_nonbool_filter_rejected() {
    let mut st = SymbolTable::new();
    st.set("S", ExprValue::unresolved(ExprType::STRING))
        .unwrap();
    let e = ParsedExpression::new("[x for x in [1, 2] if S]")
        .and_then(|p| p.evaluate(&st))
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("List comprehension filter must be a boolean, got string"),
        "got:\n{e}"
    );
    assert!(e.contains("[x for x in [1, 2] if S]"), "got:\n{e}");
    assert!(e.contains("^"), "got:\n{e}");
}

/// Elements whose filter is decided concretely (short-circuit) before
/// the first unresolved condition are abandoned, not returned as a
/// partial list.
#[test]
fn comp_mixed_concrete_then_unresolved_filter_concludes_unresolved() {
    let mut st = SymbolTable::new();
    st.set("B", ExprValue::unresolved(ExprType::BOOL)).unwrap();
    // For x=0: `x > 0 and B` short-circuits to false (concrete).
    // For x=1: `1 > 0 and B` is unresolved — bail out.
    let r = eval_u("[x for x in [0, 1, 2] if x > 0 and B]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[int]]"));
}

// ══════════════════════════════════════════════════════════════
// TestUnknownSubscript
// ══════════════════════════════════════════════════════════════

#[test]
fn subscript_unknown_list() {
    let st = st_unresolved(vec![("X", "list[int]")]);
    let r = eval_u("X[0]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[int]"));
}

#[test]
fn subscript_concrete_list_unknown_index() {
    let mut st = SymbolTable::new();
    st.set("I", ExprValue::unresolved(ExprType::INT)).unwrap();
    let r = eval_u("[10, 20, 30][I]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[int]"));
}

// ══════════════════════════════════════════════════════════════
// TestUnknownFail
// ══════════════════════════════════════════════════════════════

#[test]
fn fail_with_unknown_message() {
    let st = st_unresolved(vec![("msg", "string")]);
    // fail() with unknown arg should still propagate as noreturn/error
    let r = ParsedExpression::new("fail(msg)").and_then(|p| p.evaluate(&st));
    // Either errors or returns unresolved — both acceptable
    assert!(r.is_err() || r.unwrap().is_unresolved());
}

// ══════════════════════════════════════════════════════════════
// TestSymbolTableWithTypes
// ══════════════════════════════════════════════════════════════

#[test]
fn symtab_with_types_evaluation() {
    let st = st_unresolved(vec![("Param.Count", "int")]);
    let r = eval_u("Param.Count + 1", &st);
    assert!(r.is_unresolved());
}

#[test]
fn symtab_mixed_concrete_and_types() {
    let mut st = SymbolTable::new();
    st.set("Param.Name", ExprValue::String("hello".into()))
        .unwrap();
    st.set("Param.Count", ExprValue::unresolved(ExprType::INT))
        .unwrap();
    // Concrete part evaluates normally
    let r1 = eval_u("Param.Name", &st);
    assert_eq!(r1.to_display_string(), "hello");
    // Unresolved part propagates
    let r2 = eval_u("Param.Count + 1", &st);
    assert!(r2.is_unresolved());
}

// ══════════════════════════════════════════════════════════════
// TestUnknownBoolOpErrorSuppression
// ══════════════════════════════════════════════════════════════

#[test]
fn boolop_unknown_or_fail_suppressed() {
    let st = st_unresolved(vec![("Flag", "bool")]);
    // Flag or fail('msg') — fail() suppressed because Flag might short-circuit
    let r = eval_u("Flag or fail('required')", &st);
    assert!(r.is_unresolved());
}

#[test]
fn boolop_concrete_false_or_fail_not_suppressed() {
    assert_err(
        "false or fail('required')",
        &[
            "required\n",
            "  false or fail('required')\n",
            "           ^~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn boolop_concrete_true_and_fail_not_suppressed() {
    assert_err(
        "true and fail('required')",
        &[
            "required\n",
            "  true and fail('required')\n",
            "           ^~~~~~~~~~~~~~~~",
        ],
    );
}

// === TestUnknownTypeErrors ===
#[test]
fn type_error_operator_incompatible() {
    let st = st_unresolved(vec![("A", "int"), ("B", "string")]);
    assert_err_w(
        "A + B",
        &st,
        &[
            "Cannot use '+' operator with int and string\n",
            "  A + B\n",
            "  ~~^~~",
        ],
    );
}
#[test]
fn type_error_operator_unknown_and_concrete() {
    let st = st_unresolved(vec![("A", "int")]);
    assert_err_w(
        "A + 'hello'",
        &st,
        &[
            "Cannot use '+' operator with int and string\n",
            "  A + 'hello'\n",
            "  ~~^~~~~~~~~",
        ],
    );
}
#[test]
fn type_error_function_wrong_type() {
    let st = st_unresolved(vec![("A", "int")]);
    assert_err_w(
        "A.upper()",
        &st,
        &[
            "upper() is not available for int. Available for: string\n",
            "  A.upper()\n",
            "  ~~^~~~~~~",
        ],
    );
}
#[test]
fn type_error_method_wrong_receiver() {
    let st = st_unresolved(vec![("A", "int")]);
    assert_err_w(
        "A.upper()",
        &st,
        &[
            "upper() is not available for int. Available for: string\n",
            "  A.upper()\n",
            "  ~~^~~~~~~",
        ],
    );
}
#[test]
fn type_error_property_wrong_receiver() {
    let st = st_unresolved(vec![("A", "int")]);
    assert_err_w(
        "A.name",
        &st,
        &[
            "'name' property is not available for int. Available for: path\n",
            "  A.name\n",
            "  ~~^~~~",
        ],
    );
}

// === TestUnknownIfElse extras ===
#[test]
fn ifelse_unknown_condition_one_branch_fails_different_types() {
    let st = st_unresolved(vec![("C", "bool")]);
    let r = eval_u("1 / 0 if C else 'ok'", &st);
    assert!(r.is_unresolved());
    assert_eq!(r.expr_type().to_string(), "unresolved[string]");
}
#[test]
fn ifelse_unknown_condition_wrong_constraint() {
    let st = st_unresolved(vec![("C", "int")]);
    assert_err_w(
        "1 if C else 2",
        &st,
        &[
            "Condition must be a boolean, got int\n",
            "  1 if C else 2\n",
            "       ^",
        ],
    );
}
#[test]
fn ifelse_unknown_bool_condition() {
    let st = st_unresolved(vec![("C", "bool")]);
    assert!(ParsedExpression::new("1 if C else 2")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn ifelse_unconstrained_unknown_condition() {
    let st = st_unresolved(vec![("C", "unresolved")]);
    assert!(ParsedExpression::new("1 if C else 2")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}

// === TestUnknownListLiterals extras ===
#[test]
fn list_all_unknown_int_float_coercion() {
    let st = st_unresolved(vec![("A", "int"), ("B", "float")]);
    assert!(ParsedExpression::new("[A, B]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn list_mix_concrete_and_unknown_coercion() {
    let st = st_unresolved(vec![("A", "int")]);
    assert!(ParsedExpression::new("[A, 1.5]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn list_unknown_path_string_coercion() {
    let st = st_unresolved(vec![("A", "path"), ("B", "string")]);
    assert!(ParsedExpression::new("[A, B]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn list_unknown_incompatible_error() {
    let st = st_unresolved(vec![("A", "int"), ("B", "string")]);
    assert_err_w(
        "[A, B]",
        &st,
        &[
            "List literal contains incompatible types: int and string\n",
            "  [A, B]\n",
            "  ^~~~~~",
        ],
    );
}
#[test]
fn list_empty_unchanged() {
    assert!(ParsedExpression::new("[]")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_ok());
}

// === TestUnknownListComprehensions extras ===
#[test]
fn comp_unknown_list_with_body() {
    let st = st_unresolved(vec![("L", "list[int]")]);
    assert!(ParsedExpression::new("[x + 1 for x in L]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn comp_unknown_range_iterable() {
    let st = st_unresolved(vec![("R", "range_expr")]);
    assert!(ParsedExpression::new("[x for x in R]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn comp_unknown_iterable_with_transform() {
    let st = st_unresolved(vec![("L", "list[string]")]);
    assert!(ParsedExpression::new("[x.upper() for x in L]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}

// === TestUnknownCoercion extras ===
#[test]
fn coercion_unknown_int_times_float() {
    let st = st_unresolved(vec![("A", "int")]);
    assert!(ParsedExpression::new("A * 2.5")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn coercion_unknown_path_in_string_context() {
    let st = st_unresolved(vec![("P", "path")]);
    assert!(ParsedExpression::new("'prefix_' + string(P)")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}

// === TestUnknownComparisons extras ===
#[test]
fn cmp_unknown_ne_concrete() {
    let st = st_unresolved(vec![("A", "int")]);
    assert!(ParsedExpression::new("A != 5")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn cmp_unknown_gt_concrete() {
    let st = st_unresolved(vec![("A", "int")]);
    assert!(ParsedExpression::new("A > 5")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}

// === TestUnknownBoolOp extras ===
#[test]
fn boolop_unknown_and_false() {
    let st = st_unresolved(vec![("A", "bool")]);
    assert!(ParsedExpression::new("A and false")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn boolop_type_error_not_suppressed() {
    // Type error AFTER unknown should be suppressed (unknown might short-circuit)
    let st = st_unresolved(vec![("A", "bool")]);
    let r = eval_u("A and (1 + 'x')", &st);
    assert!(r.is_unresolved());
}

// === TestUnknownSubscript extras ===
#[test]
fn subscript_unknown_list_unknown_index() {
    let st = st_unresolved(vec![("L", "list[int]"), ("I", "int")]);
    assert!(ParsedExpression::new("L[I]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn subscript_unknown_list_slice() {
    let st = st_unresolved(vec![("L", "list[int]")]);
    assert!(ParsedExpression::new("L[1:3]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn subscript_unknown_string_index_error() {
    // String indexing with unknown string should return unresolved(string)
    let st = st_unresolved(vec![("S", "string")]);
    let r = eval_u("S[0]", &st);
    assert!(r.is_unresolved());
    assert_eq!(r.expr_type().to_string(), "unresolved[string]");
}
#[test]
fn subscript_unknown_slice_bounds() {
    let st = st_unresolved(vec![("L", "list[int]"), ("A", "int"), ("B", "int")]);
    assert!(ParsedExpression::new("L[A:B]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}

// === TestUnknownFail extras ===
#[test]
fn fail_if_else_with_unknown() {
    // fail() in if-branch with unknown condition — suppressed, returns else type
    let st = st_unresolved(vec![("C", "bool")]);
    let r = eval_u("fail('bad') if C else 'ok'", &st);
    assert!(r.is_unresolved());
    assert_eq!(r.expr_type().to_string(), "unresolved[string]");
}
#[test]
fn fail_if_else_with_concrete() {
    assert_err(
        "fail('bad') if true else 'ok'",
        &[
            "bad\n",
            "  fail('bad') if true else 'ok'\n",
            "  ^~~~~~~~~~~",
        ],
    );
}

// === TestSymbolTableWithTypes extras ===
#[test]
fn symtab_simple_types() {
    let st = st_unresolved(vec![("X", "int"), ("Y", "string"), ("Z", "float")]);
    assert!(ParsedExpression::new("X + 1")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
    assert!(ParsedExpression::new("Y + 'a'")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
    assert!(ParsedExpression::new("Z * 2.0")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn symtab_dotted_paths() {
    let st = st_unresolved(vec![("A.B", "int"), ("A.C", "string")]);
    assert!(ParsedExpression::new("A.B + 1")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}

// === TestUnknownGenericBindingConflict ===
#[test]
fn in_operator_unknown_item_concrete_list() {
    let st = st_unresolved(vec![("X", "int")]);
    assert!(ParsedExpression::new("X in [1, 2, 3]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn not_in_operator_unknown_item() {
    let st = st_unresolved(vec![("X", "int")]);
    assert!(ParsedExpression::new("X not in [1, 2, 3]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn in_operator_concrete_item_unknown_list() {
    let st = st_unresolved(vec![("L", "list[int]")]);
    assert!(ParsedExpression::new("5 in L")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}

// === TestUnknownBoolOpErrorSuppression extras ===
#[test]
fn boolop_unknown_and_fail_suppressed() {
    let st = st_unresolved(vec![("C", "bool")]);
    assert!(ParsedExpression::new("C and fail('x')")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn boolop_type_error_after_unknown_suppressed() {
    let st = st_unresolved(vec![("C", "bool")]);
    assert!(ParsedExpression::new("C or (1 + 'x')")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn boolop_type_error_before_unknown_not_suppressed() {
    let st = st_unresolved(vec![("C", "bool")]);
    assert_err_w(
        "(1 + 'x') or C",
        &st,
        &[
            "Cannot use '+' operator with int and string\n",
            "  (1 + 'x') or C\n",
            "   ~~^~~~~",
        ],
    );
}

// === Exact Python name matches ===
#[test]
fn simple_name() {
    let st = st_unresolved(vec![("X", "int")]);
    assert!(ParsedExpression::new("X")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn dotted_name() {
    let st = st_unresolved(vec![("A.B", "int")]);
    assert!(ParsedExpression::new("A.B")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn different_types() {
    let st = st_unresolved(vec![("A", "int"), ("B", "string"), ("C", "float")]);
    assert!(ParsedExpression::new("A")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unconstrained_unknown() {
    let st = st_unresolved(vec![("X", "unresolved")]);
    assert!(ParsedExpression::new("X")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn len_of_unknown_list() {
    let st = st_unresolved(vec![("L", "list[int]")]);
    assert!(ParsedExpression::new("len(L)")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn operator_incompatible_unknown_types() {
    let st = st_unresolved(vec![("A", "int"), ("B", "string")]);
    assert_err_w(
        "A + B",
        &st,
        &[
            "Cannot use '+' operator with int and string\n",
            "  A + B\n",
            "  ~~^~~",
        ],
    );
}
#[test]
fn operator_unknown_and_concrete() {
    let st = st_unresolved(vec![("A", "int")]);
    assert_err_w(
        "A + 'hello'",
        &st,
        &[
            "Cannot use '+' operator with int and string\n",
            "  A + 'hello'\n",
            "  ~~^~~~~~~~~",
        ],
    );
}
#[test]
fn function_unknown_wrong_type() {
    let st = st_unresolved(vec![("A", "int")]);
    assert_err_w(
        "A.upper()",
        &st,
        &[
            "upper() is not available for int. Available for: string\n",
            "  A.upper()\n",
            "  ~~^~~~~~~",
        ],
    );
}
#[test]
fn method_wrong_receiver_type() {
    let st = st_unresolved(vec![("A", "int")]);
    assert_err_w(
        "A.upper()",
        &st,
        &[
            "upper() is not available for int. Available for: string\n",
            "  A.upper()\n",
            "  ~~^~~~~~~",
        ],
    );
}
#[test]
fn property_wrong_receiver_type() {
    let st = st_unresolved(vec![("A", "int")]);
    assert_err_w(
        "A.name",
        &st,
        &[
            "'name' property is not available for int. Available for: path\n",
            "  A.name\n",
            "  ~~^~~~",
        ],
    );
}
#[test]
fn unknown_condition_both_branches_succeed() {
    let st = st_unresolved(vec![("C", "bool")]);
    assert!(ParsedExpression::new("1 if C else 2")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_condition_same_branch_types() {
    let st = st_unresolved(vec![("C", "bool")]);
    assert!(ParsedExpression::new("'a' if C else 'b'")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_condition_concrete_branches() {
    let st = st_unresolved(vec![("C", "bool")]);
    assert!(ParsedExpression::new("1 if C else 2")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_condition_one_branch_fails() {
    let st = st_unresolved(vec![("C", "bool")]);
    let r = eval_u("1 / 0 if C else 1", &st);
    assert!(r.is_unresolved());
}
#[test]
fn unknown_condition_other_branch_fails() {
    let st = st_unresolved(vec![("C", "bool")]);
    let r = eval_u("1 if C else 1 / 0", &st);
    assert!(r.is_unresolved());
}
#[test]
fn unknown_condition_both_branches_fail() {
    let st = st_unresolved(vec![("C", "bool")]);
    assert_err_w("1/0 if C else 1/0", &st, &["Both branches fail"]);
}
#[test]
fn known_condition_unknown_branch_values() {
    let st = st_unresolved(vec![("A", "int"), ("B", "int")]);
    assert!(ParsedExpression::new("A if true else B")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_bool_condition_accepted() {
    let st = st_unresolved(vec![("C", "bool")]);
    assert!(ParsedExpression::new("1 if C else 2")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unconstrained_unknown_condition_accepted() {
    let st = st_unresolved(vec![("C", "unresolved")]);
    assert!(ParsedExpression::new("1 if C else 2")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn all_unknown_same_constraint() {
    let st = st_unresolved(vec![("A", "int"), ("B", "int")]);
    assert!(ParsedExpression::new("[A, B]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn mix_concrete_and_unknown_same_type() {
    let st = st_unresolved(vec![("A", "int")]);
    assert!(ParsedExpression::new("[A, 1]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn all_unknown_int_float_coercion() {
    let st = st_unresolved(vec![("A", "int"), ("B", "float")]);
    assert!(ParsedExpression::new("[A, B]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn mix_concrete_and_unknown_coercion() {
    let st = st_unresolved(vec![("A", "int")]);
    assert!(ParsedExpression::new("[A, 1.5]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn mix_unknown_int_and_concrete_float() {
    let st = st_unresolved(vec![("A", "int")]);
    assert!(ParsedExpression::new("[A, 1.5]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn all_unknown_path_string_coercion() {
    let st = st_unresolved(vec![("A", "path"), ("B", "string")]);
    assert!(ParsedExpression::new("[A, B]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn mix_concrete_string_and_unknown_path() {
    let st = st_unresolved(vec![("P", "path")]);
    assert!(ParsedExpression::new("[P, 'hello']")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn mix_unknown_path_and_concrete_string() {
    let st = st_unresolved(vec![("P", "path")]);
    assert!(ParsedExpression::new("['hello', P]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn mix_unknown_string_and_concrete_path() {
    let mut st = st_unresolved(vec![("S", "string")]);
    st.set("P", ExprValue::new_path("/a", PathFormat::Posix))
        .unwrap();
    let parsed = ParsedExpression::new("[S, P]").unwrap();
    assert!(parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&[&st])
        .is_ok());
}
#[test]
fn all_concrete_same_type() {
    assert!(ParsedExpression::new("[1, 2, 3]")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_ok());
}
#[test]
fn empty_list_unchanged() {
    assert!(ParsedExpression::new("[]")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_ok());
}
#[test]
fn unknown_list_iterable() {
    let st = st_unresolved(vec![("L", "list[int]")]);
    assert!(ParsedExpression::new("[x for x in L]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_list_with_body_expr() {
    let st = st_unresolved(vec![("L", "list[int]")]);
    assert!(ParsedExpression::new("[x + 1 for x in L]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_range_iterable() {
    let st = st_unresolved(vec![("R", "range_expr")]);
    assert!(ParsedExpression::new("[x for x in R]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_iterable_with_transform() {
    let st = st_unresolved(vec![("L", "list[string]")]);
    assert!(ParsedExpression::new("[x.upper() for x in L]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_int_plus_float() {
    let st = st_unresolved(vec![("A", "int")]);
    assert!(ParsedExpression::new("A + 1.5")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_int_times_float() {
    let st = st_unresolved(vec![("A", "int")]);
    assert!(ParsedExpression::new("A * 2.5")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_path_in_string_context() {
    let st = st_unresolved(vec![("P", "path")]);
    assert!(ParsedExpression::new("'prefix_' + string(P)")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn equality_with_unknown() {
    let st = st_unresolved(vec![("A", "int")]);
    assert!(ParsedExpression::new("A == 5")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_less_than() {
    let st = st_unresolved(vec![("A", "int")]);
    assert!(ParsedExpression::new("A < 5")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn chained_comparison() {
    let st = st_unresolved(vec![("A", "int")]);
    assert!(ParsedExpression::new("1 < A < 10")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_or_false() {
    let st = st_unresolved(vec![("A", "bool")]);
    assert!(ParsedExpression::new("A or false")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_or_true_is_true() {
    let st = st_unresolved(vec![("A", "bool")]);
    assert!(ParsedExpression::new("A or true")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_and_true() {
    let st = st_unresolved(vec![("A", "bool")]);
    assert!(ParsedExpression::new("A and true")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_and_false_is_false() {
    let st = st_unresolved(vec![("A", "bool")]);
    assert!(ParsedExpression::new("A and false")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn false_and_unknown() {
    let st = st_unresolved(vec![("A", "bool")]);
    assert!(ParsedExpression::new("false and A")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn true_or_unknown() {
    let st = st_unresolved(vec![("A", "bool")]);
    assert!(ParsedExpression::new("true or A")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn multiple_unknowns_and() {
    let st = st_unresolved(vec![("A", "bool"), ("B", "bool")]);
    assert!(ParsedExpression::new("A and B")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn chained_concrete_then_unknown() {
    let st = st_unresolved(vec![("A", "bool")]);
    assert!(ParsedExpression::new("true and A")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn cross_type_comparison_with_unknowns() {
    let st = st_unresolved(vec![("A", "int"), ("B", "float")]);
    assert!(ParsedExpression::new("A < B")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_list_index() {
    let st = st_unresolved(vec![("L", "list[int]")]);
    assert!(ParsedExpression::new("L[0]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn concrete_list_unknown_index() {
    let st = st_unresolved(vec![("I", "int")]);
    assert!(ParsedExpression::new("[1, 2, 3][I]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_list_unknown_index() {
    let st = st_unresolved(vec![("L", "list[int]"), ("I", "int")]);
    assert!(ParsedExpression::new("L[I]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_list_slice() {
    let st = st_unresolved(vec![("L", "list[int]")]);
    assert!(ParsedExpression::new("L[1:3]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_slice_bounds() {
    let st = st_unresolved(vec![("L", "list[int]"), ("A", "int"), ("B", "int")]);
    assert!(ParsedExpression::new("L[A:B]")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn simple_types() {
    let st = st_unresolved(vec![("X", "int"), ("Y", "string")]);
    assert!(ParsedExpression::new("X + 1")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn dotted_paths() {
    let st = st_unresolved(vec![("A.B", "int")]);
    assert!(ParsedExpression::new("A.B + 1")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn evaluation_with_types() {
    let st = st_unresolved(vec![("X", "int")]);
    assert!(ParsedExpression::new("X + 1")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn mixed_concrete_and_types() {
    let mut st = st_unresolved(vec![("X", "int")]);
    st.set("Y", ExprValue::Int(10)).unwrap();
    assert!(ParsedExpression::new("X + Y")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn in_operator_with_unknown_list() {
    let st = st_unresolved(vec![("L", "list[int]")]);
    assert!(ParsedExpression::new("5 in L")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_or_fail_suppressed() {
    let st = st_unresolved(vec![("C", "bool")]);
    assert!(ParsedExpression::new("C or fail('x')")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn unknown_and_fail_suppressed() {
    let st = st_unresolved(vec![("C", "bool")]);
    assert!(ParsedExpression::new("C and fail('x')")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn concrete_false_or_fail_not_suppressed() {
    assert_err(
        "false or fail('x')",
        &["x\n", "  false or fail('x')\n", "           ^~~~~~~~~"],
    );
}
#[test]
fn concrete_true_and_fail_not_suppressed() {
    assert_err(
        "true and fail('x')",
        &["x\n", "  true and fail('x')\n", "           ^~~~~~~~~"],
    );
}
#[test]
fn type_error_after_unknown_suppressed() {
    let st = st_unresolved(vec![("C", "bool")]);
    assert!(ParsedExpression::new("C or (1 + 'x')")
        .and_then(|p| p.evaluate(&st))
        .is_ok());
}
#[test]
fn type_error_before_unknown_not_suppressed() {
    let st = st_unresolved(vec![("C", "bool")]);
    assert_err_w(
        "(1 + 'x') or C",
        &st,
        &[
            "Cannot use '+' operator with int and string\n",
            "  (1 + 'x') or C\n",
            "   ~~^~~~~",
        ],
    );
}

// === TestUnknownTypeErrors ===

fn assert_err_contains(expr: &str, st: &SymbolTable, expected: &[&str]) {
    let err = ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(st))
        .unwrap_err();
    let msg = err.to_string();
    for line in expected {
        assert!(msg.contains(line), "Missing: {line:?}\nGot:\n{msg}");
    }
}

#[test]
fn unresolved_operator_incompatible_types() {
    let st = st_unresolved(vec![("X", "string"), ("Y", "int")]);
    assert_err_contains(
        "X + Y",
        &st,
        &[
            "Cannot use '+' operator with string and int",
            "X + Y",
            "~~^~~",
        ],
    );
}
#[test]
fn unresolved_operator_unknown_and_concrete() {
    let st = st_unresolved(vec![("X", "string")]);
    assert_err_contains(
        "X - 1",
        &st,
        &[
            "Cannot use '-' operator with string and int",
            "X - 1",
            "~~^~~",
        ],
    );
}
#[test]
fn unresolved_function_wrong_type() {
    let st = st_unresolved(vec![("X", "int")]);
    assert_err_contains(
        "len(X)",
        &st,
        &["No matching signature for len(int)", "len(X)", "^"],
    );
}
#[test]
fn unresolved_method_wrong_receiver_type() {
    let st = st_unresolved(vec![("X", "int")]);
    assert_err_contains(
        "X.upper()",
        &st,
        &["upper()", "not available for int", "Available for: string"],
    );
}
#[test]
fn unresolved_property_wrong_receiver_type() {
    let st = st_unresolved(vec![("X", "int")]);
    assert_err_contains(
        "X.stem",
        &st,
        &["stem", "not available for int", "Available for: path"],
    );
}

// === TestUnknownGenericBindingConflict ===

#[test]
fn unresolved_in_operator_unknown_item_concrete_list() {
    let st = st_unresolved(vec![("X", "int")]);
    let r = ParsedExpression::new("X in [1, 3, 5]")
        .and_then(|p| p.evaluate(&st))
        .unwrap();
    assert!(
        r.expr_type().to_string().contains("bool"),
        "got: {}",
        r.expr_type()
    );
}
#[test]
fn unresolved_not_in_operator_unknown_item() {
    let st = st_unresolved(vec![("X", "int")]);
    let r = ParsedExpression::new("X not in [2, 4, 6]")
        .and_then(|p| p.evaluate(&st))
        .unwrap();
    assert!(
        r.expr_type().to_string().contains("bool"),
        "got: {}",
        r.expr_type()
    );
}
#[test]
fn unresolved_in_operator_concrete_item_unknown_list() {
    let st = st_unresolved(vec![("L", "list[int]")]);
    let r = ParsedExpression::new("3 in L")
        .and_then(|p| p.evaluate(&st))
        .unwrap();
    assert!(
        r.expr_type().to_string().contains("bool"),
        "got: {}",
        r.expr_type()
    );
}

// ══════════════════════════════════════════════════════════════
// Missing Python test coverage — added below
// ══════════════════════════════════════════════════════════════

// --- TestUnknownIfElse: branch fails with different types (unresolved vars) ---

#[test]
fn ifelse_body_succeeds_else_fails_different_types() {
    // Python: test_unknown_condition_one_branch_fails_different_types
    // X=string, Y=int; body=X (ok), else=Y.upper() (int has no upper) → unresolved[string]
    let st = st_unresolved(vec![("cond", "bool"), ("X", "string"), ("Y", "int")]);
    let r = eval_u("X if cond else Y.upper()", &st);
    assert_eq!(r.expr_type(), tp("unresolved[string]"));
}

#[test]
fn ifelse_body_fails_else_succeeds_different_types() {
    // Python: test_unknown_condition_other_branch_fails_different_types
    // X=int, Y=string; body=X.upper() (int has no upper), else=Y (ok) → unresolved[string]
    let st = st_unresolved(vec![("cond", "bool"), ("X", "int"), ("Y", "string")]);
    let r = eval_u("X.upper() if cond else Y", &st);
    assert_eq!(r.expr_type(), tp("unresolved[string]"));
}

// --- TestUnknownIfElse: union condition ---

#[test]
fn ifelse_unknown_bool_union_condition_accepted() {
    // Python: test_unknown_bool_union_condition_accepted
    // unresolved[bool | int] as condition is valid (constraint includes bool)
    let st = st_unresolved(vec![("cond", "bool | int")]);
    let r = eval_u("1 if cond else 2", &st);
    assert!(r.is_unresolved());
}

// --- TestUnknownIfElse: both branches fail with full error message ---

#[test]
fn ifelse_both_branches_fail_full_error() {
    // Python: test_unknown_condition_both_branches_fail — exact error message
    let st = st_unresolved(vec![("cond", "bool"), ("X", "int"), ("Y", "path")]);
    assert_err_contains(
        "X + 'a' if cond else Y * 'b'",
        &st,
        &[
            "Both branches fail",
            "if-branch: Cannot use '+' operator with int and string",
            "else-branch: Cannot use '*' operator with path and string",
        ],
    );
}

// --- TestUnknownCoercion: upper(X) where X=path ---

#[test]
fn coercion_unknown_path_upper() {
    // Python: test_unknown_path_in_string_context — upper(X) where X=path → unresolved[string]
    let st = st_unresolved(vec![("X", "path")]);
    let r = eval_u("upper(X)", &st);
    assert_eq!(r.expr_type(), tp("unresolved[string]"));
}

// --- TestUnknownComparisons: chained 1 < 2 < X ---

#[test]
fn cmp_chained_concrete_then_unknown() {
    // Python: test_chained_concrete_then_unknown — 1 < 2 < X → unresolved[bool]
    let st = st_unresolved(vec![("X", "int")]);
    let r = eval_u("1 < 2 < X", &st);
    assert_eq!(r.expr_type(), tp("unresolved[bool]"));
}

// --- TestUnknownComparisons: cross-type with string vs list ---

#[test]
fn cmp_cross_type_string_vs_list() {
    // Python: test_cross_type_comparison_with_unknowns — X=string, Y=list[int]
    let st = st_unresolved(vec![("X", "string"), ("Y", "list[int]")]);
    let r = eval_u("X < Y", &st);
    assert_eq!(r.expr_type(), tp("unresolved[bool]"));
}

// --- TestUnknownBoolOps: concrete result assertions ---

#[test]
fn boolop_false_and_unknown_is_false() {
    // Python: test_false_and_unknown — False and X → False (short-circuit)
    let st = st_unresolved(vec![("X", "bool")]);
    let r = eval_u("false and X", &st);
    assert_eq!(r, ExprValue::Bool(false));
}

#[test]
fn boolop_true_or_unknown_is_true() {
    // Python: test_true_or_unknown — True or X → True (short-circuit)
    let st = st_unresolved(vec![("X", "bool")]);
    let r = eval_u("true or X", &st);
    assert_eq!(r, ExprValue::Bool(true));
}

#[test]
fn boolop_unknown_or_true_concrete_true() {
    // Python: test_unknown_or_true_is_true — X or True → True
    let st = st_unresolved(vec![("X", "bool")]);
    let r = eval_u("X or true", &st);
    assert_eq!(r, ExprValue::Bool(true));
}

#[test]
fn boolop_unknown_and_false_concrete_false() {
    // Python: test_unknown_and_false_is_false — X and False → False
    let st = st_unresolved(vec![("X", "bool")]);
    let r = eval_u("X and false", &st);
    assert_eq!(r, ExprValue::Bool(false));
}

#[test]
fn boolop_multiple_unknowns_and_result() {
    // Python: test_multiple_unknowns_and — X and Y and True → unresolved[bool]
    let st = st_unresolved(vec![("X", "bool"), ("Y", "bool")]);
    let r = eval_u("X and Y and true", &st);
    assert_eq!(r.expr_type(), tp("unresolved[bool]"));
}

// --- TestUnknownBoolOps: type error not suppressed ---

#[test]
fn boolop_type_error_in_boolop_not_suppressed() {
    // Python: test_type_error_in_boolop_not_suppressed
    // X.upper() or True → error (int has no upper), type errors always caught in boolops
    let st = st_unresolved(vec![("X", "int")]);
    let err = ParsedExpression::new("X.upper() or true")
        .and_then(|p| p.evaluate(&st))
        .unwrap_err()
        .to_string();
    assert!(err.contains("upper"), "expected upper error, got: {err}");
    assert!(
        err.contains("not available for int"),
        "expected type error, got: {err}"
    );
}

// --- TestUnknownSubscript: string index type error ---

#[test]
fn subscript_unknown_string_as_index_error() {
    // Python: test_unknown_string_index_error — [1, 2][I] where I=unresolved[string] → error
    let st = st_unresolved(vec![("I", "string")]);
    assert_err_w("[1, 2][I]", &st, &["Index must be an integer"]);
}

// --- TestUnknownFail: exact type assertions ---

#[test]
fn fail_unknown_message_returns_unresolved_noreturn() {
    // Python: test_fail_with_unknown_message — fail(unresolved[string]) → unresolved[noreturn]
    let st = st_unresolved(vec![("msg", "string")]);
    let r = eval_u("fail(msg)", &st);
    assert_eq!(r.expr_type(), tp("unresolved[noreturn]"));
}

#[test]
fn fail_ifelse_unknown_fail_in_else() {
    // Python: test_if_else_with_unknown_fail — X if cond else fail(msg) → unresolved[int]
    let st = st_unresolved(vec![("cond", "bool"), ("X", "int"), ("msg", "string")]);
    let r = eval_u("X if cond else fail(msg)", &st);
    assert_eq!(r.expr_type(), tp("unresolved[int]"));
}

#[test]
fn fail_ifelse_concrete_fail_in_else() {
    // Python: test_if_else_with_concrete_fail — X if cond else fail('bad') → unresolved[int]
    let st = st_unresolved(vec![("cond", "bool"), ("X", "int")]);
    let r = eval_u("X if cond else fail('bad')", &st);
    assert_eq!(r.expr_type(), tp("unresolved[int]"));
}

#[test]
fn fail_in_boolop_not_caught_during_type_check() {
    // Python: test_fail_in_boolop_not_caught_during_type_check
    // (fail('bad') if cond else False) or True → True
    let st = st_unresolved(vec![("cond", "bool")]);
    let r = eval_u("(fail('bad') if cond else false) or true", &st);
    assert_eq!(r, ExprValue::Bool(true));
}

// --- TestUnknownListLiterals: exact type assertions for coercion ---

#[test]
fn list_all_unknown_int_float_coercion_type() {
    // Python: test_all_unknown_int_float_coercion — [unresolved[int], unresolved[float]] → unresolved[list[float]]
    let st = st_unresolved(vec![("X", "int"), ("Y", "float")]);
    let r = eval_u("[X, Y]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[float]]"));
}

#[test]
fn list_mix_concrete_and_unknown_coercion_type() {
    // Python: test_mix_concrete_and_unknown_coercion — [1, unresolved[float]] → unresolved[list[float]]
    let st = st_unresolved(vec![("X", "float")]);
    let r = eval_u("[1, X]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[float]]"));
}

#[test]
fn list_mix_unknown_int_and_concrete_float_type() {
    // Python: test_mix_unknown_int_and_concrete_float — [unresolved[int], 1.0] → unresolved[list[float]]
    let st = st_unresolved(vec![("X", "int")]);
    let r = eval_u("[X, 1.0]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[float]]"));
}

#[test]
fn list_all_unknown_path_string_coercion_type() {
    // Python: test_all_unknown_path_string_coercion — [unresolved[path], unresolved[string]] → unresolved[list[string]]
    let st = st_unresolved(vec![("X", "path"), ("Y", "string")]);
    let r = eval_u("[X, Y]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[string]]"));
}

#[test]
fn list_mix_concrete_string_and_unknown_path_type() {
    // Python: test_mix_concrete_string_and_unknown_path — ['hello', unresolved[path]] → unresolved[list[string]]
    let st = st_unresolved(vec![("X", "path")]);
    let r = eval_u("['hello', X]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[string]]"));
}

#[test]
fn list_mix_unknown_path_and_concrete_string_type() {
    // Python: test_mix_unknown_path_and_concrete_string — [unresolved[path], 'hello'] → unresolved[list[string]]
    let st = st_unresolved(vec![("X", "path")]);
    let r = eval_u("[X, 'hello']", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[string]]"));
}

#[test]
fn list_mix_unknown_string_and_concrete_path_type() {
    // Python: test_mix_unknown_string_and_concrete_path — [unresolved[string], path('/a')] → unresolved[list[string]]
    let st = st_unresolved(vec![("X", "string")]);
    let parsed = ParsedExpression::new("[X, path('/a')]").unwrap();
    let symtabs = [&st];
    let r = parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap();
    assert_eq!(r.expr_type(), tp("unresolved[list[string]]"));
}

// --- TestUnknownListComprehensions: exact type assertions ---

#[test]
fn comp_unknown_list_with_body_type() {
    // Python: test_unknown_list_with_body_expr — [x + 1 for x in unresolved[list[int]]] → unresolved[list[int]]
    let st = st_unresolved(vec![("X", "list[int]")]);
    let r = eval_u("[x + 1 for x in X]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[int]]"));
}

#[test]
fn comp_unknown_range_iterable_type() {
    // Python: test_unknown_range_iterable — [x for x in unresolved[range_expr]] → unresolved[list[int]]
    let st = st_unresolved(vec![("X", "range_expr")]);
    let r = eval_u("[x for x in X]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[int]]"));
}

#[test]
fn comp_unknown_iterable_with_transform_type() {
    // Python: test_unknown_iterable_with_transform — [string(x) for x in unresolved[list[int]]] → unresolved[list[string]]
    let st = st_unresolved(vec![("X", "list[int]")]);
    let r = eval_u("[string(x) for x in X]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[string]]"));
}

// --- TestUnknownCoercion: exact type assertions ---

#[test]
fn coercion_unknown_int_times_float_type() {
    // Python: test_unknown_int_times_float — unresolved[int] * 2.0 → unresolved[float]
    let st = st_unresolved(vec![("X", "int")]);
    let r = eval_u("X * 2.0", &st);
    assert_eq!(r.expr_type(), tp("unresolved[float]"));
}

// --- TestUnknownComparisons: exact type assertions ---

#[test]
fn cmp_chained_comparison_type() {
    // Python: test_chained_comparison — 1 < X < 10 → unresolved[bool]
    let st = st_unresolved(vec![("X", "int")]);
    let r = eval_u("1 < X < 10", &st);
    assert_eq!(r.expr_type(), tp("unresolved[bool]"));
}

#[test]
fn cmp_equality_with_unknown_string() {
    // Python: test_equality_with_unknown — X == 'hello' where X=string → unresolved[bool]
    let st = st_unresolved(vec![("X", "string")]);
    let r = eval_u("X == 'hello'", &st);
    assert_eq!(r.expr_type(), tp("unresolved[bool]"));
}

#[test]
fn cmp_in_operator_with_unknown_list_type() {
    // Python: test_in_operator_with_unknown_list — 3 in unresolved[list[int]] → unresolved[bool]
    let st = st_unresolved(vec![("X", "list[int]")]);
    let r = eval_u("3 in X", &st);
    assert_eq!(r.expr_type(), tp("unresolved[bool]"));
}

// --- TestUnknownSubscript: exact type assertions ---

#[test]
fn subscript_unknown_list_index_type() {
    // Python: test_unknown_list_index — X[0] where X=list[int] → unresolved[int]
    let st = st_unresolved(vec![("X", "list[int]")]);
    let r = eval_u("X[0]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[int]"));
}

#[test]
fn subscript_concrete_list_unknown_index_type() {
    // Python: test_concrete_list_unknown_index — [1,2,3][I] where I=int → unresolved[int]
    let st = st_unresolved(vec![("I", "int")]);
    let r = eval_u("[1, 2, 3][I]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[int]"));
}

#[test]
fn subscript_unknown_list_unknown_index_type() {
    // Python: test_unknown_list_unknown_index — X[I] where X=list[string], I=int → unresolved[string]
    let st = st_unresolved(vec![("X", "list[string]"), ("I", "int")]);
    let r = eval_u("X[I]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[string]"));
}

#[test]
fn subscript_unknown_list_slice_type() {
    // Python: test_unknown_list_slice — X[1:3] where X=list[int] → unresolved[list[int]]
    let st = st_unresolved(vec![("X", "list[int]")]);
    let r = eval_u("X[1:3]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[int]]"));
}

#[test]
fn subscript_unknown_slice_bounds_type() {
    // Python: test_unknown_slice_bounds — [1,2,3][X:] where X=int → unresolved[list[int]]
    let st = st_unresolved(vec![("X", "int")]);
    let r = eval_u("[1, 2, 3][X:]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[int]]"));
}

// ══════════════════════════════════════════════════════════════
// round() return type with unresolved ndigits (§RFC 0006)
// ══════════════════════════════════════════════════════════════

#[test]
fn round_float_unresolved_ndigits_returns_union() {
    // round(float, int) -> float | int per spec: returns int when ndigits <= 0, float when > 0.
    // With unresolved ndigits, the return type should be unresolved[float | int].
    let st = st_unresolved(vec![("x", "int")]);
    let r = eval_u("round(31.5, x)", &st);
    assert_eq!(r.expr_type(), tp("unresolved[float | int]"));
}

// === List comprehension filter: unresolved type checking ===

#[test]
fn comp_filter_unresolved_int_is_error() {
    let st = st_unresolved(vec![("L", "list[int]"), ("P", "int")]);
    assert_err_w(
        "[x for x in L if P]",
        &st,
        &[
            "List comprehension filter must be a boolean, got int\n",
            "  [x for x in L if P]\n",
            "                   ^",
        ],
    );
}

#[test]
fn comp_filter_unresolved_bool_succeeds() {
    let st = st_unresolved(vec![("L", "list[int]"), ("P", "bool")]);
    let r = eval_u("[x for x in L if P]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[int]]"));
}

#[test]
fn comp_filter_unresolved_unconstrained_succeeds() {
    let st = st_unresolved(vec![("L", "list[int]"), ("P", "unresolved")]);
    let r = eval_u("[x for x in L if P]", &st);
    assert_eq!(r.expr_type(), tp("unresolved[list[int]]"));
}

// ══════════════════════════════════════════════════════════════
// List element join under mixed resolution (resolved-value-limits item 24)
//
// List literals and comprehensions with an unresolved element hoist to
// `unresolved[list[T]]`, with `T` computed by the same join `make_list`
// applies to the concrete list. The three symbol states a template sees:
// pass 8 (everything unresolved), gate 2 / `create_job` (`Param.*`
// concrete, `Task.*` / `Session.*` unresolved), and run time (everything
// concrete). Each case asserts the hoisted type at the unresolved stages
// and the concrete type at run time.
// ══════════════════════════════════════════════════════════════

/// The three symbol states for the item-24 cases: `Param.L = [1, 2, 3]`,
/// `Param.N = 3`, `Param.F = 2.5`, `Param.S = 'abc'`; `Task.Param.I/F/S/P`
/// int/float/string/path; `Session.HasPathMappingRules` bool.
fn join_states() -> [SymbolTable; 3] {
    let float = |f: f64| ExprValue::Float(value::Float64::new(f).unwrap());
    let params = [
        ("Param.L", "list[int]", ExprValue::ListInt(vec![1, 2, 3])),
        ("Param.N", "int", ExprValue::Int(3)),
        ("Param.F", "float", float(2.5)),
        ("Param.S", "string", ExprValue::String("abc".into())),
    ];
    let tasks = [
        ("Task.Param.I", "int", ExprValue::Int(2)),
        ("Task.Param.F", "float", float(1.5)),
        ("Task.Param.S", "string", ExprValue::String("x".into())),
        (
            "Task.Param.P",
            "path",
            ExprValue::new_path("/tmp/a", PathFormat::host()),
        ),
        ("Session.HasPathMappingRules", "bool", ExprValue::Bool(true)),
    ];
    let (mut pass8, mut gate2, mut run) =
        (SymbolTable::new(), SymbolTable::new(), SymbolTable::new());
    for (k, t, v) in params {
        pass8.set(k, ExprValue::unresolved(tp(t))).unwrap();
        gate2.set(k, v.clone()).unwrap();
        run.set(k, v).unwrap();
    }
    for (k, t, v) in tasks {
        pass8.set(k, ExprValue::unresolved(tp(t))).unwrap();
        gate2.set(k, ExprValue::unresolved(tp(t))).unwrap();
        run.set(k, v).unwrap();
    }
    [pass8, gate2, run]
}

/// Assert the result type of `expr` at pass 8, gate 2, and run time.
fn assert_join_types(expr: &str, pass8: &str, gate2: &str, run: &str) {
    let [p8, g2, rt] = join_states();
    assert_eq!(eval_u(expr, &p8).expr_type(), tp(pass8), "pass 8: {expr}");
    assert_eq!(eval_u(expr, &g2).expr_type(), tp(gate2), "gate 2: {expr}");
    assert_eq!(eval_u(expr, &rt).expr_type(), tp(run), "run time: {expr}");
}

#[test]
fn join_union_with_empty_list_member_beside_other_list_type() {
    // Auditor iteration-2 finding B1: the `[]` member of the conditional
    // joins with `list[string]` (the run with H true builds
    // `[[], ['x']]`), so the union's other member `list[int]` failing to
    // join does not reject the list.
    assert_join_types(
        "[([] if Session.HasPathMappingRules else [1]), [Task.Param.S]]",
        "unresolved[list[list[string]]]",
        "unresolved[list[list[string]]]",
        "list[list[string]]",
    );
    assert_join_types(
        "[[Task.Param.S], ([] if Session.HasPathMappingRules else Param.L)]",
        "unresolved[list[list[string]]]",
        "unresolved[list[list[string]]]",
        "list[list[string]]",
    );
}

#[test]
fn join_gate2_listcomp_empty_list_beside_unresolved_list() {
    // Previously gate 2: "List literal contains incompatible types:
    // list[int], list[nulltype]".
    assert_join_types(
        "[[] if x > 2 else [Task.Param.I] for x in Param.L]",
        "unresolved[list[list[int]]]",
        "unresolved[list[list[int]]]",
        "list[list[int]]",
    );
    // Auditor finding N1: over an unresolved iterable (pass 8) the body's
    // `list[string] | list[nulltype]` is joined to `list[string]`, so the
    // element's methods type-check as they do at gate 2 and run time.
    assert_join_types(
        "[[] if x > 2 else [Task.Param.S] for x in Param.L][0][0].upper()",
        "unresolved[string]",
        "unresolved[string]",
        "string",
    );
}

#[test]
fn join_gate2_empty_concrete_comprehension_beside_unresolved_list() {
    // An empty concrete comprehension is `list[nulltype]`; it yields to
    // the unresolved `list[int]` sibling, also through enclosing calls.
    let inner = "[[x for x in Param.L if x > 100], [Task.Param.I]]";
    assert_join_types(
        inner,
        "unresolved[list[list[int]]]",
        "unresolved[list[list[int]]]",
        "list[list[int]]",
    );
    assert_join_types(
        &format!("repr_json({inner})"),
        "unresolved[string]",
        "unresolved[string]",
        "string",
    );
    assert_join_types(
        &format!("flatten({inner})"),
        "unresolved[list[int]]",
        "unresolved[list[int]]",
        "list[int]",
    );
    assert_join_types(
        &format!("len({inner})"),
        "unresolved[int]",
        "unresolved[int]",
        "int",
    );
}

#[test]
fn join_gate2_listcomp_list_int_beside_list_float() {
    // Previously gate 2: "list[float], list[int]".
    assert_join_types(
        "[[x] if x > 2 else [Task.Param.F] for x in Param.L]",
        "unresolved[list[list[float] | list[int]]]",
        "unresolved[list[list[float]]]",
        "list[list[float]]",
    );
    assert_join_types(
        "[Param.L if x > 2 else [Task.Param.F] for x in Param.L]",
        "unresolved[list[list[float] | list[int]]]",
        "unresolved[list[list[float]]]",
        "list[list[float]]",
    );
}

#[test]
fn join_gate2_listcomp_list_string_beside_list_path() {
    // Previously gate 2: "list[path], list[string]".
    assert_join_types(
        "[[Task.Param.S] if x > 2 else [Task.Param.P] for x in Param.L]",
        "unresolved[list[list[path] | list[string]]]",
        "unresolved[list[list[string]]]",
        "list[list[string]]",
    );
}

#[test]
fn join_gate2_listcomp_union_beside_member() {
    // Previously gate 2: "float | int, int". The values decide whether the
    // run-time list is `list[int]` (here) or `list[float]`, so the hoisted
    // element type is the union of both.
    assert_join_types(
        "[x if x > 2 else (Task.Param.I if Session.HasPathMappingRules else Param.F) for x in Param.L]",
        "unresolved[list[float | int]]",
        "unresolved[list[float | int]]",
        "list[int]",
    );
}

#[test]
fn join_pass8_empty_list_beside_unresolved_list() {
    assert_join_types(
        "[[], [Task.Param.I]]",
        "unresolved[list[list[int]]]",
        "unresolved[list[list[int]]]",
        "list[list[int]]",
    );
}

#[test]
fn join_pass8_nested_int_float_and_string_path() {
    assert_join_types(
        "[[Param.N], [Task.Param.F]]",
        "unresolved[list[list[float]]]",
        "unresolved[list[list[float]]]",
        "list[list[float]]",
    );
    assert_join_types(
        "[[Param.S], [Task.Param.P]]",
        "unresolved[list[list[string]]]",
        "unresolved[list[list[string]]]",
        "list[list[string]]",
    );
    assert_join_types(
        "flatten([[Task.Param.F], Param.L])",
        "unresolved[list[float]]",
        "unresolved[list[float]]",
        "list[float]",
    );
}

#[test]
fn join_pass8_union_beside_member() {
    // `int ** int` is `float | int` (a negative exponent gives a float).
    assert_join_types(
        "[Task.Param.I ** 2, Param.N]",
        "unresolved[list[float | int]]",
        "unresolved[list[float | int]]",
        "list[int]",
    );
}

#[test]
fn join_stays_sound_nested_int_beside_string() {
    // The join does not accept what the concrete list rejects:
    // `list[int]` beside `list[string]` is incompatible at every stage.
    let [p8, g2, rt] = join_states();
    for st in [&p8, &g2, &rt] {
        assert_err_w(
            "[[Param.N], [Task.Param.S]]",
            st,
            &[
                "List literal contains incompatible types: list[int] and list[string]\n",
                "  [[Param.N], [Task.Param.S]]\n",
                "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~",
            ],
        );
    }
}

#[test]
fn join_stays_sound_listcomp_int_beside_string() {
    // Pass 8 types the body once as a union and accepts; gate 2 sees one
    // concrete `list[int]` beside an unresolved `list[string]` and rejects,
    // exactly as run time does when it builds the list.
    let expr = "[[x] if x > 2 else [Task.Param.S] for x in Param.L]";
    let [p8, g2, rt] = join_states();
    assert_eq!(
        eval_u(expr, &p8).expr_type(),
        tp("unresolved[list[list[int] | list[string]]]")
    );
    assert_err_w(
        expr,
        &g2,
        &[
            "List literal contains incompatible types: list[string] and list[int]\n",
            "  [[x] if x > 2 else [Task.Param.S] for x in Param.L]\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
    assert_err_w(
        expr,
        &rt,
        &[
            "make_list expected list[string] element, got list[int]\n",
            "  [[x] if x > 2 else [Task.Param.S] for x in Param.L]\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn join_union_drops_incompatible_and_null_members() {
    // A union element joins through its compatible members only: a value
    // of an incompatible member fails at run time, so it never contributes
    // to a list that is built.
    let st = st_unresolved(vec![("U", "int | string"), ("N", "int?"), ("I", "int")]);
    assert_eq!(
        eval_u("[U, I]", &st).expr_type(),
        tp("unresolved[list[int]]")
    );
    assert_eq!(
        eval_u("[N, 1]", &st).expr_type(),
        tp("unresolved[list[int]]")
    );
    assert_err_w(
        "[U, I, path('a')]",
        &st,
        &[
            "List literal contains incompatible types: int | string, int, and path\n",
            "  [U, I, path('a')]\n",
            "  ^~~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn join_optional_elements_drop_null_member() {
    // Auditor finding B1: a `null` element always fails, so an optional
    // element contributes only its non-null member — never a top-level
    // `nulltype` element type.
    let [p8, g2, rt] = join_states();
    let expr = "[Task.Param.I if Session.HasPathMappingRules else None]";
    assert_eq!(eval_u(expr, &p8).expr_type(), tp("unresolved[list[int]]"));
    assert_eq!(eval_u(expr, &g2).expr_type(), tp("unresolved[list[int]]"));
    assert_eq!(eval_u(expr, &rt).expr_type(), tp("list[int]"));
    let expr = "[Task.Param.I if Session.HasPathMappingRules else None, Param.F]";
    assert_eq!(eval_u(expr, &p8).expr_type(), tp("unresolved[list[float]]"));
    assert_eq!(eval_u(expr, &g2).expr_type(), tp("unresolved[list[float]]"));
    assert_eq!(eval_u(expr, &rt).expr_type(), tp("list[float]"));
}

#[test]
fn join_optional_elements_of_incompatible_types_are_rejected() {
    // Auditor finding B1, probe 1: `int?` beside `string?` previously
    // joined member-wise to `nulltype` and was accepted, although every
    // run-time value fails (int/string conflict, or a null element).
    let [p8, g2, _] = join_states();
    let expr = "[Task.Param.I if Session.HasPathMappingRules else None, \
                Task.Param.S if Session.HasPathMappingRules else None]";
    for st in [&p8, &g2] {
        assert_err_w(
            expr,
            st,
            &[
                "List literal contains incompatible types: int and string\n",
                &format!("  {expr}\n"),
                &format!("  ^{}", "~".repeat(expr.len() - 1)),
            ],
        );
    }
}

#[test]
fn join_nested_optional_elements_of_incompatible_types_are_rejected() {
    // Auditor finding B1, probe 3: each inner literal is `list[int]` /
    // `list[string]` (its optional element contributes the non-null
    // member), and those two lists do not join.
    let [p8, g2, rt] = join_states();
    let expr = "[[Task.Param.I if Session.HasPathMappingRules else None], \
                [Task.Param.S if Session.HasPathMappingRules else None]]";
    for st in [&p8, &g2] {
        assert_err_w(
            expr,
            st,
            &[
                "List literal contains incompatible types: list[int] and list[string]\n",
                &format!("  {expr}\n"),
                &format!("  ^{}", "~".repeat(expr.len() - 1)),
            ],
        );
    }
    assert_err_w(
        expr,
        &rt,
        &[
            "List literal contains incompatible types: list[int] and list[string]\n",
            &format!("  {expr}\n"),
            &format!("  ^{}", "~".repeat(expr.len() - 1)),
        ],
    );
}

#[test]
fn join_optional_list_elements_of_incompatible_types_are_rejected() {
    // Auditor finding B1, probe 2: `list[int]?` beside `list[string]?`.
    let st = st_unresolved(vec![
        ("L", "list[int]"),
        ("LS", "list[string]"),
        ("H", "bool"),
    ]);
    let expr = "[(L if H else None), (LS if H else None)]";
    assert_err_w(
        expr,
        &st,
        &[
            "List literal contains incompatible types: list[int] and list[string]\n",
            &format!("  {expr}\n"),
            &format!("  ^{}", "~".repeat(expr.len() - 1)),
        ],
    );
}

#[test]
fn join_union_empty_list_member_is_used_once() {
    // Auditor iteration-3 finding N2: the conditional's `[]` member joins
    // with `list[string]`, but the result `list[string]` then conflicts
    // with the trailing `[2]` (and the conditional's `list[int]` member
    // conflicts with `list[string]`), so no run builds the list and every
    // unresolved stage rejects it.
    let [p8, g2, rt] = join_states();
    let expr = "[([] if Session.HasPathMappingRules else [1]), [Task.Param.S], [2]]";
    for st in [&p8, &g2] {
        assert_err_w(
            expr,
            st,
            &[
                "List literal contains incompatible types: list[int] | list[nulltype], list[string], and list[int]\n",
                &format!("  {expr}\n"),
                &format!("  ^{}", "~".repeat(expr.len() - 1)),
            ],
        );
    }
    // Run time (H true) builds `[[], ['x'], [2]]`: `list[string]` beside
    // `list[int]`.
    assert_err_w(
        expr,
        &rt,
        &[
            "List literal contains incompatible types: list[nulltype], list[string], and list[int]\n",
            &format!("  {expr}\n"),
            &format!("  ^{}", "~".repeat(expr.len() - 1)),
        ],
    );
}

#[test]
fn join_gate2_listcomp_null_body_reports_null_element() {
    // Auditor iteration-3 finding N1: a concrete `None` produced by the
    // comprehension body beside an unresolved element fails at gate 2 with
    // the run-time message rather than an int/nulltype type conflict.
    let [p8, g2, rt] = join_states();
    let expr = "[None if x > 2 else Task.Param.I for x in Param.L]";
    assert_eq!(eval_u(expr, &p8).expr_type(), tp("unresolved[list[int]]"));
    for st in [&g2, &rt] {
        assert_err_w(
            expr,
            st,
            &[
                "Cannot create list from null elements\n",
                &format!("  {expr}\n"),
                &format!("  ^{}", "~".repeat(expr.len() - 1)),
            ],
        );
    }
}

#[test]
fn join_always_null_unresolved_element_reports_null_element() {
    // PR #434 review: an unresolved element whose type contributes nothing
    // to a list (`unresolved[nulltype]`) used to reach the incompatible-
    // types formatter with a single type, producing "incompatible types:
    // , and nulltype". Such an element is null on every run, so every
    // stage reports the run-time null error of the construct: a list
    // literal's, or a comprehension's (`make_list`'s).
    let [p8, g2, rt] = join_states();
    let err_lines = |expr: &str, msg: &str| {
        [
            format!("{msg}\n"),
            format!("  {expr}\n"),
            format!("  ^{}", "~".repeat(expr.len() - 1)),
        ]
    };
    for expr in [
        "[None if Session.HasPathMappingRules else None]",
        "[Task.Param.I, None if Session.HasPathMappingRules else None]",
    ] {
        let lines = err_lines(expr, "null is not allowed in list literals");
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        for st in [&p8, &g2, &rt] {
            assert_err_w(expr, st, &lines);
        }
    }
    // Over a concrete iterable (gate 2, run time) the comprehension
    // reports `make_list`'s null error. At pass 8 the iterable is
    // unresolved and may be empty, so the body type is kept and the
    // result is a sound `unresolved[list[nulltype]]`.
    let expr = "[(None if Session.HasPathMappingRules else None) for x in Param.L]";
    assert_eq!(
        eval_u(expr, &p8).expr_type(),
        tp("unresolved[list[nulltype]]")
    );
    let lines = err_lines(expr, "Cannot create list from null elements");
    let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
    for st in [&g2, &rt] {
        assert_err_w(expr, st, &lines);
    }
}

// ══════════════════════════════════════════════════════════════
// and/or under mixed resolution (resolved-value-limits item 25)
//
// After an unresolved operand an `and`/`or` never returns a later
// concrete operand: the unresolved one may decide the result at run
// time. The result is `unresolved[union of every type that could be
// returned]`, or a concrete `bool`/`null` when every possible result is
// that one value. Each case asserts the result at pass 8 (everything
// unresolved), gate 2 / `create_job` (`Param.*` concrete, `Task.*` /
// `Session.*` unresolved), and run time (everything concrete).
// ══════════════════════════════════════════════════════════════

/// The three symbol states for the item-25 cases: `Param.L = [1, 2, 3]`,
/// `Param.N = 3`, `Param.Z = 0`, `Param.S = 'abc'`, `Param.F = 2.5`,
/// `Param.Null` a `nulltype`; `Task.Param.I = 2` (int),
/// `Task.Param.F = 0.0` (float), `Task.Param.O = null` (`int | nulltype`),
/// `Task.Param.NB = null` (`bool | nulltype`), `Task.Param.A = 4` (`any`),
/// `Session.HasPathMappingRules` (bool) `has_rules`.
fn boolop_states(has_rules: bool) -> [SymbolTable; 3] {
    let float = |f: f64| ExprValue::Float(value::Float64::new(f).unwrap());
    let params = [
        ("Param.L", "list[int]", ExprValue::ListInt(vec![1, 2, 3])),
        ("Param.N", "int", ExprValue::Int(3)),
        ("Param.Z", "int", ExprValue::Int(0)),
        ("Param.S", "string", ExprValue::String("abc".into())),
        ("Param.F", "float", float(2.5)),
    ];
    let tasks = [
        ("Task.Param.I", "int", ExprValue::Int(2)),
        ("Task.Param.F", "float", float(0.0)),
        ("Task.Param.O", "int | nulltype", ExprValue::Null),
        ("Task.Param.NB", "bool | nulltype", ExprValue::Null),
        ("Task.Param.A", "any", ExprValue::Int(4)),
        (
            "Session.HasPathMappingRules",
            "bool",
            ExprValue::Bool(has_rules),
        ),
    ];
    let (mut pass8, mut gate2, mut run) =
        (SymbolTable::new(), SymbolTable::new(), SymbolTable::new());
    for (k, t, v) in params {
        pass8.set(k, ExprValue::unresolved(tp(t))).unwrap();
        gate2.set(k, v.clone()).unwrap();
        run.set(k, v).unwrap();
    }
    for (k, t, v) in tasks {
        pass8.set(k, ExprValue::unresolved(tp(t))).unwrap();
        gate2.set(k, ExprValue::unresolved(tp(t))).unwrap();
        run.set(k, v).unwrap();
    }
    [pass8, gate2, run]
}

fn eval_target(expr: &str, st: &SymbolTable, target: Option<&str>) -> ExprValue {
    let parsed = ParsedExpression::new(expr).unwrap();
    let tt = target.map(tp);
    let result = match &tt {
        Some(t) => parsed.with_target_type(t).evaluate(&[st]),
        None => parsed.evaluate(st),
    };
    result.unwrap_or_else(|e| panic!("{expr}: {e}"))
}

/// The full error (message, expression, caret) of `expr` coerced to
/// `target`.
fn eval_target_err(expr: &str, st: &SymbolTable, target: Option<&str>) -> String {
    let parsed = ParsedExpression::new(expr).unwrap();
    let tt = target.map(tp);
    let result = match &tt {
        Some(t) => parsed.with_target_type(t).evaluate(&[st]),
        None => parsed.evaluate(st),
    };
    match result {
        Ok(v) => panic!("{expr}: expected an error, got {v:?}"),
        Err(e) => e.to_string(),
    }
}

/// Assert `expr` coerced to `target` fails in `st` with exactly the
/// message, expression line, and caret in `expected`.
fn assert_target_err(expr: &str, st: &SymbolTable, target: Option<&str>, expected: &[&str]) {
    let e = eval_target_err(expr, st, target);
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

/// Assert the result type of `expr` (coerced to `target`) at pass 8 and
/// gate 2, and the value at run time (with `Session.HasPathMappingRules`
/// true and false).
fn assert_boolop(expr: &str, target: Option<&str>, pass8: &str, gate2: &str, run: [ExprValue; 2]) {
    let [p8, g2, _] = boolop_states(true);
    assert_eq!(
        eval_target(expr, &p8, target).expr_type(),
        tp(pass8),
        "pass 8: {expr}"
    );
    assert_eq!(
        eval_target(expr, &g2, target).expr_type(),
        tp(gate2),
        "gate 2: {expr}"
    );
    for (has_rules, expected) in [true, false].into_iter().zip(run) {
        let [_, _, rt] = boolop_states(has_rules);
        assert_eq!(
            eval_target(expr, &rt, target),
            expected,
            "run time (rules={has_rules}): {expr}"
        );
    }
}

#[test]
fn boolop_gate2_or_floordiv_by_later_zero_operand() {
    // Previously gate 2: "Division by zero" — `Task.Param.I or Param.Z`
    // returned `Param.Z`, but an int is never falsy, so run time always
    // returns `Task.Param.I`.
    assert_boolop(
        "10 // (Task.Param.I or Param.Z)",
        None,
        "unresolved[int]",
        "unresolved[int]",
        [ExprValue::Int(5), ExprValue::Int(5)],
    );
}

#[test]
fn boolop_gate2_or_index_by_later_out_of_range_operand() {
    // Previously gate 2: "Index 3 out of bounds for list of length 3"
    // (and the string equivalent).
    assert_boolop(
        "Param.L[Task.Param.I or Param.N]",
        None,
        "unresolved[int]",
        "unresolved[int]",
        [ExprValue::Int(3), ExprValue::Int(3)],
    );
    assert_boolop(
        "Param.S[Task.Param.I or Param.N]",
        None,
        "unresolved[string]",
        "unresolved[string]",
        [ExprValue::String("c".into()), ExprValue::String("c".into())],
    );
}

#[test]
fn boolop_gate2_or_bool_target_not_applied_to_later_operand() {
    // Previously gate 2: "Cannot convert 'abc' to bool". The result is
    // `true` (the flag) or `'abc'`, and the bool target applies to the
    // union.
    let [p8, g2, _] = boolop_states(true);
    let expr = "Session.HasPathMappingRules or Param.S";
    assert_eq!(
        eval_u(expr, &p8).expr_type(),
        tp("unresolved[bool | string]")
    );
    assert_eq!(
        eval_u(expr, &g2).expr_type(),
        tp("unresolved[bool | string]")
    );
    assert_eq!(
        eval_target(expr, &g2, Some("bool")).expr_type(),
        tp("unresolved[bool]")
    );
    let [_, _, rt] = boolop_states(true);
    assert_eq!(eval_target(expr, &rt, Some("bool")), ExprValue::Bool(true));
    // With the flag false run time returns `'abc'`, which the target
    // rejects.
    let [_, _, rt] = boolop_states(false);
    assert_target_err(
        expr,
        &rt,
        Some("bool"),
        &[
            "Cannot convert 'abc' to bool\n",
            &format!("  {expr}\n"),
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn boolop_gate2_or_int_target_not_applied_to_later_float() {
    // Previously gate 2: "Cannot coerce float to int: 2.5 is not a whole
    // number". Run time always returns `Task.Param.F` (0.0).
    assert_boolop(
        "Task.Param.F or Param.F",
        Some("int"),
        "unresolved[int]",
        "unresolved[int]",
        [ExprValue::Int(0), ExprValue::Int(0)],
    );
}

#[test]
fn boolop_pass8_and_returns_union_of_operand_types() {
    // Previously typed `unresolved[bool]` and rejected at pass 8 with
    // "upper() is not available for bool". Run time with the flag true
    // returns `Param.S`.
    let [p8, g2, rt] = boolop_states(true);
    let expr = "Session.HasPathMappingRules and Param.S";
    assert_eq!(
        eval_u(expr, &p8).expr_type(),
        tp("unresolved[bool | string]")
    );
    assert_eq!(
        eval_u(expr, &g2).expr_type(),
        tp("unresolved[bool | string]")
    );
    assert_eq!(eval_u(expr, &rt), ExprValue::String("abc".into()));
    let expr = "(Session.HasPathMappingRules and Param.S).upper()";
    assert_eq!(eval_u(expr, &p8).expr_type(), tp("unresolved[string]"));
    assert_eq!(eval_u(expr, &g2).expr_type(), tp("unresolved[string]"));
    assert_eq!(eval_u(expr, &rt), ExprValue::String("ABC".into()));
}

#[test]
fn boolop_unresolved_last_operand_keeps_its_type() {
    // Previously `unresolved[bool]`: the last operand is returned
    // whatever its value.
    assert_boolop(
        "Param.N and Task.Param.I",
        None,
        "unresolved[int]",
        "unresolved[int]",
        [ExprValue::Int(2), ExprValue::Int(2)],
    );
}

#[test]
fn boolop_and_never_decided_by_non_falsy_unresolved_operand() {
    // An `int` never decides `and`, so the later concrete operand is the
    // result — concrete at gate 2, as at run time.
    assert_boolop(
        "Task.Param.I and Param.N",
        None,
        "unresolved[int]",
        "int",
        [ExprValue::Int(3), ExprValue::Int(3)],
    );
}

#[test]
fn boolop_or_nullable_operand_unions_non_null_member_with_fallback() {
    // `int | nulltype` returns from `or` only as an int; otherwise the
    // fallback string is returned.
    assert_boolop(
        "Task.Param.O or Param.S",
        None,
        "unresolved[int | string]",
        "unresolved[int | string]",
        [
            ExprValue::String("abc".into()),
            ExprValue::String("abc".into()),
        ],
    );
    // `and` returns it only as `null`.
    assert_boolop(
        "Task.Param.O and Param.S",
        None,
        "unresolved[nulltype | string]",
        "unresolved[nulltype | string]",
        [ExprValue::Null, ExprValue::Null],
    );
}

#[test]
fn boolop_concrete_when_every_outcome_is_the_same_bool() {
    // The flag returns from `and` only as `false`, and the last operand is
    // `false`: every possible result is `false`.
    assert_boolop(
        "Session.HasPathMappingRules and false",
        None,
        "bool",
        "bool",
        [ExprValue::Bool(false), ExprValue::Bool(false)],
    );
    assert_boolop(
        "Session.HasPathMappingRules or Session.HasPathMappingRules or true",
        None,
        "bool",
        "bool",
        [ExprValue::Bool(true), ExprValue::Bool(true)],
    );
    // Outcomes `false` (the flag) and `true` (the last operand) differ.
    assert_boolop(
        "Session.HasPathMappingRules and true",
        None,
        "unresolved[bool]",
        "unresolved[bool]",
        [ExprValue::Bool(true), ExprValue::Bool(false)],
    );
}

#[test]
fn boolop_absorbed_operand_prevents_concrete_result() {
    // The flag returns from `or` only as `true`, but when it is false run
    // time fails in `fail()` — the result is not known to be `true`.
    let [p8, g2, _] = boolop_states(true);
    let expr = "Session.HasPathMappingRules or fail('no rules')";
    assert_eq!(eval_u(expr, &p8).expr_type(), tp("unresolved[bool]"));
    assert_eq!(eval_u(expr, &g2).expr_type(), tp("unresolved[bool]"));
    let [_, _, rt] = boolop_states(true);
    assert_eq!(eval_u(expr, &rt), ExprValue::Bool(true));
    let [_, _, rt] = boolop_states(false);
    assert_err_w(
        expr,
        &rt,
        &[
            "no rules\n",
            &format!("  {expr}\n"),
            "                                 ^~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn boolop_stops_after_unresolved_operand_that_always_decides() {
    // `Task.Param.I` always decides `or`, so run time never evaluates
    // the later operand and its budget exceedance is not charged.
    let [_, g2, _] = boolop_states(true);
    let r = ParsedExpression::new("Task.Param.I or len('A' * 10000000)")
        .and_then(|p| {
            p.with_memory_limit(1024 * 1024)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[&g2])
        })
        .unwrap();
    assert_eq!(r.value.expr_type(), tp("unresolved[int]"));
}

#[test]
fn boolop_error_method_on_operand_that_always_decides() {
    // `Task.Param.I` always decides `or`, so the result is an int at run
    // time and `upper()` fails. (Previously typed `unresolved[string]` at
    // pass 8 and gate 2 and accepted.)
    let expr = "(Task.Param.I or Param.S).upper()";
    for st in boolop_states(true) {
        assert_err_w(
            expr,
            &st,
            &[
                "upper() is not available for int. Available for: string\n",
                &format!("  {expr}\n"),
                "  ~~~~~~~~~~~~~~~~~~~~~~~~~^~~~~~~~",
            ],
        );
    }
}

#[test]
fn boolop_error_no_union_member_matches_operator() {
    // The result is `false` or `'abc'`; neither supports `- 1`.
    let [p8, g2, rt] = boolop_states(true);
    let expr = "(Session.HasPathMappingRules and Param.S) - 1";
    // The caret marks the `-` (index 42) inside the whole binop span.
    let caret = format!("  {}^~~", "~".repeat(42));
    for st in [&p8, &g2] {
        assert_err_w(
            expr,
            st,
            &[
                "Cannot use '-' operator with bool | string and int\n",
                &format!("  {expr}\n"),
                &caret,
            ],
        );
    }
    assert_err_w(
        expr,
        &rt,
        &[
            "Cannot use '-' operator with string and int\n",
            &format!("  {expr}\n"),
            &caret,
        ],
    );
}

#[test]
fn boolop_nested_cond_and_value_or_fallback_idiom() {
    // `cond and A or B`: the inner `and` returns the flag only as
    // `false`, which passes the outer `or` on, so `bool` is never a
    // possible result. (Before nested outcomes were kept, the inner
    // `and` merged to `unresolved[bool | int]` and the outer `or` kept
    // the `bool`, so the subscript was rejected with "Index must be an
    // integer".)
    assert_boolop(
        "Session.HasPathMappingRules and 1 or 0",
        None,
        "unresolved[int]",
        "unresolved[int]",
        [ExprValue::Int(1), ExprValue::Int(0)],
    );
    assert_boolop(
        "Param.L[Session.HasPathMappingRules and 1 or 0]",
        None,
        "unresolved[int]",
        "unresolved[int]",
        [ExprValue::Int(2), ExprValue::Int(1)],
    );
    let float = |f: f64| ExprValue::Float(value::Float64::new(f).unwrap());
    // Previously "Cannot use '*' operator with bool | float and int".
    assert_boolop(
        "(Session.HasPathMappingRules and Param.F or 1.0) * 2",
        None,
        "unresolved[float]",
        "unresolved[float]",
        [float(5.0), float(2.0)],
    );
    // A compare-typed condition works the same way.
    assert_boolop(
        "Task.Param.I > 1 and Param.S or 'none'",
        None,
        "unresolved[string]",
        "unresolved[string]",
        [
            ExprValue::String("abc".into()),
            ExprValue::String("abc".into()),
        ],
    );
}

#[test]
fn boolop_nested_exact_outcome_passes_outer_on() {
    // The inner `and` is concretely `false` at gate 2 (every outcome is
    // `false`), so the outer `or` returns its last, concrete operand.
    assert_boolop(
        "(Session.HasPathMappingRules and false) or Param.N",
        None,
        "unresolved[int]",
        "int",
        [ExprValue::Int(3), ExprValue::Int(3)],
    );
    // The inner `or` returns `true` (the flag) or `Param.S`; `true` passes
    // the outer `and` on, `Param.S` never decides it either, so the outer
    // result is `Param.N`'s.
    assert_boolop(
        "(Session.HasPathMappingRules or Param.S) and Param.N",
        None,
        "unresolved[int]",
        "int",
        [ExprValue::Int(3), ExprValue::Int(3)],
    );
}

#[test]
fn boolop_unresolved_operand_in_the_middle_of_a_chain() {
    // The leading `Param.Z == 1` is concretely `false` at gate 2 and never
    // decides `or` (at pass 8 it is an unresolved bool, returned as
    // `true`); `Task.Param.O` returns only as an int; the last operand is
    // returned otherwise.
    assert_boolop(
        "Param.Z == 1 or Task.Param.O or Param.S",
        None,
        "unresolved[bool | int | string]",
        "unresolved[int | string]",
        [
            ExprValue::String("abc".into()),
            ExprValue::String("abc".into()),
        ],
    );
    // A concrete non-deciding operand after the unresolved one is never
    // returned: only the flag's `false` and the last operand are.
    assert_boolop(
        "Param.N and Session.HasPathMappingRules and Param.S and Param.F",
        None,
        "unresolved[bool | float]",
        "unresolved[bool | float]",
        [
            ExprValue::Float(value::Float64::new(2.5).unwrap()),
            ExprValue::Bool(false),
        ],
    );
}

#[test]
fn boolop_nullable_bool_operand_and_false_is_not_concrete() {
    // `Task.Param.NB` (`bool | nulltype`) returns from `and` as `false`
    // or `null`, so the result is not concretely `false` (as it was
    // before: run time returns `null` here).
    assert_boolop(
        "Task.Param.NB and false",
        None,
        "unresolved[bool | nulltype]",
        "unresolved[bool | nulltype]",
        [ExprValue::Null, ExprValue::Null],
    );
    // From `or` it returns only as `true`.
    assert_boolop(
        "Task.Param.NB or Param.S",
        None,
        "unresolved[bool | string]",
        "unresolved[bool | string]",
        [
            ExprValue::String("abc".into()),
            ExprValue::String("abc".into()),
        ],
    );
}

#[test]
fn boolop_any_typed_operand() {
    // An `any` operand may be returned by `or` as anything, and by `and`
    // only as `false` or `null`.
    assert_boolop(
        "Task.Param.A or Param.S",
        None,
        "unresolved[any]",
        "unresolved[any]",
        [ExprValue::Int(4), ExprValue::Int(4)],
    );
    assert_boolop(
        "Task.Param.A and Param.S",
        None,
        "unresolved[bool | nulltype | string]",
        "unresolved[bool | nulltype | string]",
        [
            ExprValue::String("abc".into()),
            ExprValue::String("abc".into()),
        ],
    );
}

#[test]
fn boolop_error_every_reachable_operand_fails() {
    // `Task.Param.I` never decides `and`, so every run reaches `fail()`:
    // the absorbed error is reported, not "Cannot coerce noreturn to
    // string".
    let expr = "Task.Param.I and fail('x')";
    for st in boolop_states(true) {
        assert_target_err(
            expr,
            &st,
            Some("string"),
            &[
                "x\n",
                &format!("  {expr}\n"),
                "                   ^~~~~~~~~",
            ],
        );
    }
    // At gate 2 the division is concrete and fails; at pass 8 `Param.Z`
    // is unresolved and the division types as `int`.
    let expr = "Task.Param.I and (10 // Param.Z)";
    let [p8, g2, rt] = boolop_states(true);
    assert_eq!(
        eval_target(expr, &p8, Some("string")).expr_type(),
        tp("unresolved[string]")
    );
    for st in [&g2, &rt] {
        assert_target_err(
            expr,
            st,
            Some("string"),
            &[
                "Division by zero\n",
                &format!("  {expr}\n"),
                "                    ~~~^~~~~~~~~~",
            ],
        );
    }
}
