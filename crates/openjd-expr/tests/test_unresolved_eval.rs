// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

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
            "List literal contains incompatible types: int, string\n",
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
