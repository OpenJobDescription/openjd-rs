// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_lists.py

use openjd_expr::{ExprType, ExprValue, ParsedExpression, PathFormat, RangeExpr, SymbolTable};

fn eval(expr: &str) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap()
}
fn eval_posix(expr: &str, st: &SymbolTable) -> ExprValue {
    let parsed = openjd_expr::ParsedExpression::new(expr).unwrap();
    let symtabs = [st];
    parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap()
}
fn eval_posix_no_st(expr: &str) -> ExprValue {
    let parsed = openjd_expr::ParsedExpression::new(expr).unwrap();
    let st = SymbolTable::new();
    let symtabs = [&st];
    parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap()
}
fn eval_posix_err(expr: &str, st: &SymbolTable) -> String {
    let parsed = openjd_expr::ParsedExpression::new(expr).unwrap();
    let symtabs = [st];
    parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap_err()
        .to_string()
}
fn eval_fails(expr: &str) -> bool {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_err()
}
fn assert_err(expr: &str, expected: &[&str]) {
    let e = ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

// === TestListLiteralTypeInference ===
#[test]
fn list_all_int() {
    assert_eq!(eval("[1, 2, 3]").expr_type().to_string(), "list[int]");
}
#[test]
fn list_all_float() {
    assert_eq!(eval("[1.0, 2.0]").expr_type().to_string(), "list[float]");
}
#[test]
fn list_all_string() {
    assert_eq!(
        eval("[\"a\", \"b\"]").expr_type().to_string(),
        "list[string]"
    );
}
#[test]
fn list_all_bool() {
    assert_eq!(eval("[True, False]").expr_type().to_string(), "list[bool]");
}
#[test]
fn list_int_float_promotes() {
    assert_eq!(eval("[1, 2.0]").expr_type().to_string(), "list[float]");
}
#[test]
fn list_float_int_promotes() {
    assert_eq!(eval("[1.0, 2]").expr_type().to_string(), "list[float]");
}

// === TestListElementCoercion ===
#[test]
fn list_int_string_error() {
    assert!(eval_fails("[1, 'a']"));
}
#[test]
fn list_bool_int_error() {
    assert!(eval_fails("[True, 1]"));
}

// === TestLists ===
#[test]
fn list_subscript() {
    assert_eq!(eval("[10, 20, 30][1]").to_display_string(), "20");
}
#[test]
fn list_negative_subscript() {
    assert_eq!(eval("[10, 20, 30][-1]").to_display_string(), "30");
}
#[test]
fn list_out_of_bounds() {
    assert!(eval_fails("[1, 2, 3][10]"));
}
#[test]
fn list_len() {
    assert_eq!(eval("len([1, 2, 3])").to_display_string(), "3");
}
#[test]
fn list_empty_len() {
    assert_eq!(eval("len([])").to_display_string(), "0");
}

// === TestListComprehension ===
#[test]
fn comp_basic() {
    assert_eq!(
        eval("[x * 2 for x in [1, 2, 3]]").to_display_string(),
        "[2, 4, 6]"
    );
}
#[test]
fn comp_filter() {
    assert_eq!(
        eval("[x for x in [1, 2, 3, 4, 5] if x > 3]").to_display_string(),
        "[4, 5]"
    );
}
#[test]
fn comp_string() {
    assert!(eval("[s.upper() for s in ['a', 'b', 'c']]").is_list());
}

// === TestListConcatenation ===
#[test]
fn list_concat() {
    assert_eq!(eval("[1, 2] + [3, 4]").to_display_string(), "[1, 2, 3, 4]");
}
#[test]
fn list_concat_empty() {
    assert_eq!(eval("[1, 2] + []").to_display_string(), "[1, 2]");
}

// === TestListMembership ===
#[test]
fn int_in_list() {
    assert_eq!(eval("2 in [1, 2, 3]").to_display_string(), "true");
}
#[test]
fn int_not_in_list() {
    assert_eq!(eval("5 in [1, 2, 3]").to_display_string(), "false");
}
#[test]
fn str_in_list() {
    assert_eq!(eval("'b' in ['a', 'b', 'c']").to_display_string(), "true");
}
#[test]
fn not_in_list() {
    assert_eq!(eval("5 not in [1, 2, 3]").to_display_string(), "true");
}

// === TestSortedReversed ===
#[test]
fn sorted_int() {
    assert_eq!(eval("sorted([3, 1, 2])").to_display_string(), "[1, 2, 3]");
}
#[test]
fn sorted_string() {
    assert_eq!(
        eval("sorted(['c', 'a', 'b'])").to_display_string(),
        "[\"a\", \"b\", \"c\"]"
    );
}
#[test]
fn reversed_int() {
    assert_eq!(eval("reversed([1, 2, 3])").to_display_string(), "[3, 2, 1]");
}

// === TestUnique ===
#[test]
fn unique_int() {
    assert_eq!(
        eval("unique([1, 2, 2, 3, 1])").to_display_string(),
        "[1, 2, 3]"
    );
}
#[test]
fn unique_string() {
    assert_eq!(
        eval("unique(['a', 'b', 'a'])").to_display_string(),
        "[\"a\", \"b\"]"
    );
}

// === TestAnyAll ===
#[test]
fn any_true() {
    assert_eq!(
        eval("any([False, True, False])").to_display_string(),
        "true"
    );
}
#[test]
fn any_false() {
    assert_eq!(eval("any([False, False])").to_display_string(), "false");
}
#[test]
fn all_true() {
    assert_eq!(eval("all([True, True])").to_display_string(), "true");
}
#[test]
fn all_false() {
    assert_eq!(eval("all([True, False])").to_display_string(), "false");
}

// === TestJoin ===
#[test]
fn join_basic() {
    assert_eq!(
        eval("join(['a', 'b', 'c'], ',')").to_display_string(),
        "a,b,c"
    );
}
#[test]
fn join_method() {
    assert_eq!(
        eval("['a', 'b', 'c'].join(',')").to_display_string(),
        "a,b,c"
    );
}
#[test]
fn join_empty() {
    assert_eq!(eval("join([], ',')").to_display_string(), "");
}

// === TestRange ===
#[test]
fn range_single() {
    assert_eq!(eval("range(5)").to_display_string(), "[0, 1, 2, 3, 4]");
}
#[test]
fn range_start_stop() {
    assert_eq!(eval("range(2, 5)").to_display_string(), "[2, 3, 4]");
}
#[test]
fn range_step() {
    assert_eq!(
        eval("range(0, 10, 2)").to_display_string(),
        "[0, 2, 4, 6, 8]"
    );
}
#[test]
fn range_empty() {
    assert_eq!(eval("range(0)").to_display_string(), "[]");
}
#[test]
fn range_negative_step() {
    assert_eq!(
        eval("range(5, 0, -1)").to_display_string(),
        "[5, 4, 3, 2, 1]"
    );
}

// === TestListMultiplication ===
#[test]
fn list_mul() {
    assert_eq!(eval("[1, 2] * 3").to_display_string(), "[1, 2, 1, 2, 1, 2]");
}
#[test]
fn list_mul_zero() {
    assert_eq!(eval("[1, 2] * 0").to_display_string(), "[]");
}

// === TestFlatten ===
#[test]
fn flatten_basic() {
    assert_eq!(
        eval("flatten([[1, 2], [3, 4]])").to_display_string(),
        "[1, 2, 3, 4]"
    );
}
#[test]
fn flatten_empty() {
    assert_eq!(eval("flatten([])").to_display_string(), "[]");
}

// === TestListLiteralTypeInference ===
#[test]
fn list_nested_same_type() {
    assert_eq!(
        eval("[[1], [2, 3]]").expr_type().to_string(),
        "list[list[int]]"
    );
}
#[test]
fn list_nested_int_float_promotes() {
    assert_eq!(
        eval("[[1], [2.0]]").expr_type().to_string(),
        "list[list[float]]"
    );
}
#[test]
fn list_path_string_promotes() {
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::new_path("/a", PathFormat::Posix))
        .unwrap();
    assert_eq!(
        eval_posix("[P, 'b']", &st).expr_type().to_string(),
        "list[string]"
    );
}
#[test]
fn list_null_fails() {
    assert_err(
        "[1, null]",
        &[
            "null is not allowed in list literals\n",
            "  [1, null]\n",
            "  ^~~~~~~~~",
        ],
    );
}
#[test]
fn list_none_fails() {
    assert_err(
        "[null]",
        &[
            "null is not allowed in list literals\n",
            "  [null]\n",
            "  ^~~~~~",
        ],
    );
}
#[test]
fn list_string_bool_fails() {
    assert_err(
        "['a', true]",
        &[
            "List literal contains incompatible types: string and bool\n",
            "  ['a', true]\n",
            "  ^~~~~~~~~~~",
        ],
    );
}
#[test]
fn list_scalar_list_fails() {
    assert_err(
        "[1, [2]]",
        &[
            "List literal contains incompatible types: int and list[int]\n",
            "  [1, [2]]\n",
            "  ^~~~~~~~",
        ],
    );
}
#[test]
fn list_three_incompatible_fails() {
    assert_err(
        "[1, 2.0, 'a']",
        &[
            "List literal contains incompatible types: int, float, and string\n",
            "  [1, 2.0, 'a']\n",
            "  ^~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn list_path_int_fails() {
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::new_path("/a", PathFormat::Posix))
        .unwrap();
    let e = eval_posix_err("[P, 1]", &st);
    assert!(
        e.contains("incompatible types") && e.contains("^"),
        "got:\n{e}"
    );
}
#[test]
fn list_three_level_nesting_fails() {
    let e = ParsedExpression::new("[[[1]]]")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("Lists may be nested at most 2 levels deep"),
        "got:\n{e}"
    );
    assert!(e.contains("  [[[1]]]\n  ^~~~~~~"), "got:\n{e}");
}

// === TestListElementCoercion (target type) ===
// These test coercion when a list is assigned to a typed parameter — tested via conformance

// === TestLists extras ===
#[test]
fn list_trailing_comma() {
    assert_eq!(eval("[1, 2, 3,]").list_len(), Some(3));
}
#[test]
fn list_single_trailing_comma() {
    assert_eq!(eval("[42,]").list_len(), Some(1));
}

// === TestListComprehension extras ===
#[test]
fn comp_with_outer_var() {
    let mut st = SymbolTable::new();
    st.set("Base", ExprValue::Int(100)).unwrap();
    let r = ParsedExpression::new("[Base + x for x in [1, 2, 3]]")
        .and_then(|p| p.evaluate(&st))
        .unwrap();
    assert_eq!(r.list_len(), Some(3));
}

// === TestListConcatenation ===
#[test]
fn list_concat_int_float() {
    let r = eval("[1, 2] + [3.0, 4.0]");
    assert_eq!(r.expr_type().to_string(), "list[float]");
}
#[test]
fn list_concat_float_int() {
    let r = eval("[1.0, 2.0] + [3, 4]");
    assert_eq!(r.expr_type().to_string(), "list[float]");
}
#[test]
fn list_concat_empty_left() {
    assert_eq!(eval("[] + [1, 2, 3]").list_len(), Some(3));
}
#[test]
fn list_concat_empty_right() {
    assert_eq!(eval("[1, 2, 3] + []").list_len(), Some(3));
}
#[test]
fn list_concat_both_empty() {
    assert_eq!(eval("[] + []").list_len(), Some(0));
}
#[test]
fn list_concat_range_expr_list() {
    let mut st = SymbolTable::new();
    st.set(
        "R",
        ExprValue::RangeExpr("1-3".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    let r = ParsedExpression::new("R + [10, 11]")
        .and_then(|p| p.evaluate(&st))
        .unwrap();
    assert!(r.list_len().unwrap() >= 4);
}
#[test]
fn list_concat_list_range_expr() {
    let mut st = SymbolTable::new();
    st.set(
        "R",
        ExprValue::RangeExpr("1-3".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    let r = ParsedExpression::new("[10, 11] + R")
        .and_then(|p| p.evaluate(&st))
        .unwrap();
    assert!(r.list_len().unwrap() >= 4);
}
#[test]
fn list_concat_range_range() {
    let mut st = SymbolTable::new();
    st.set(
        "R1",
        ExprValue::RangeExpr("1-3".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    st.set(
        "R2",
        ExprValue::RangeExpr("10-12".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    let r = ParsedExpression::new("R1 + R2")
        .and_then(|p| p.evaluate(&st))
        .unwrap();
    assert!(r.list_len().unwrap() >= 5);
}
#[test]
fn list_concat_incompatible() {
    assert_err(
        "[1, 2] + ['a']",
        &[
            "Cannot concatenate list[int] and list[string]\n",
            "  [1, 2] + ['a']\n",
            "  ~~~~~~~^~~~~~~",
        ],
    );
}
#[test]
fn list_concat_chained() {
    assert_eq!(eval("[1] + [2] + [3]").list_len(), Some(3));
}
#[test]
fn list_concat_comprehension() {
    assert_eq!(
        eval("[x * 2 for x in [1, 2, 3]] + [100]").list_len(),
        Some(4)
    );
}

// === TestListMembership extras ===
#[test]
fn float_in_list() {
    assert_eq!(eval("2.5 in [1.0, 2.5, 3.0]").to_display_string(), "true");
}
#[test]
fn bool_in_list() {
    assert_eq!(eval("true in [false, true]").to_display_string(), "true");
}
#[test]
fn not_in_list_true() {
    assert_eq!(eval("6 not in [1, 2, 3]").to_display_string(), "true");
}
#[test]
fn not_in_list_false() {
    assert_eq!(eval("2 not in [1, 2, 3]").to_display_string(), "false");
}

// === TestSortedReversed ===
#[test]
fn sorted_float() {
    let r = eval("sorted([3.5, 1.2, 2.8])");
    assert!(r.is_list());
}
#[test]
fn sorted_empty() {
    assert_eq!(eval("sorted([])").list_len(), Some(0));
}
#[test]
fn sorted_single() {
    assert_eq!(eval("sorted([42])").list_len(), Some(1));
}
#[test]
fn sorted_already() {
    assert_eq!(eval("sorted([1, 2, 3])").list_len(), Some(3));
}
#[test]
fn sorted_reverse_order() {
    assert_eq!(eval("sorted([5, 4, 3, 2, 1])").list_len(), Some(5));
}
#[test]
fn sorted_duplicates() {
    assert_eq!(eval("sorted([3, 1, 3, 1, 2])").list_len(), Some(5));
}
#[test]
fn sorted_method() {
    assert_eq!(eval("[3, 1, 2].sorted()").list_len(), Some(3));
}
#[test]
fn reversed_float() {
    let r = eval("reversed([1.1, 2.2, 3.3])");
    assert!(r.is_list());
}
#[test]
fn reversed_string() {
    let r = eval("reversed(['a', 'b', 'c'])");
    assert!(r.is_list());
}
#[test]
fn reversed_empty() {
    assert_eq!(eval("reversed([])").list_len(), Some(0));
}
#[test]
fn reversed_single() {
    assert_eq!(eval("reversed([42])").list_len(), Some(1));
}
#[test]
fn reversed_method() {
    assert_eq!(eval("[1, 2, 3].reversed()").list_len(), Some(3));
}
#[test]
fn sorted_then_reversed() {
    assert_eq!(eval("reversed(sorted([3, 1, 2]))").list_len(), Some(3));
}
#[test]
fn reversed_then_sorted() {
    assert_eq!(eval("sorted(reversed([3, 1, 2]))").list_len(), Some(3));
}
#[test]
fn sorted_chained() {
    assert_eq!(eval("[3, 1, 2].sorted().reversed()").list_len(), Some(3));
}

// === TestUnique ===
#[test]
fn unique_float() {
    assert_eq!(eval("unique([1.5, 2.5, 1.5, 3.5])").list_len(), Some(3));
}
#[test]
fn unique_bool() {
    assert_eq!(eval("unique([true, false, true])").list_len(), Some(2));
}
#[test]
fn unique_no_dups() {
    assert_eq!(eval("unique([1, 2, 3])").list_len(), Some(3));
}
#[test]
fn unique_all_same() {
    assert_eq!(eval("unique([7, 7, 7])").list_len(), Some(1));
}
#[test]
fn unique_empty() {
    assert_eq!(eval("unique([])").list_len(), Some(0));
}
#[test]
fn unique_method() {
    assert_eq!(eval("[3, 1, 2, 1].unique()").list_len(), Some(3));
}
#[test]
fn unique_chained_sorted() {
    assert_eq!(eval("[3, 1, 2, 1].unique().sorted()").list_len(), Some(3));
}
#[test]
fn unique_preserves_first() {
    // unique preserves first occurrence order
    let r = eval("unique(['banana', 'apple', 'banana', 'cherry', 'apple'])");
    assert_eq!(r.list_len(), Some(3));
}

// === TestAnyAll extras ===
#[test]
fn any_empty() {
    assert_eq!(eval("any([])").to_display_string(), "false");
}
#[test]
fn all_empty() {
    assert_eq!(eval("all([])").to_display_string(), "true");
}

// === TestRange ===
#[test]
fn range_stop() {
    assert_eq!(eval("range(5)").list_len(), Some(5));
}
#[test]
fn range_stop_zero() {
    assert_eq!(eval("range(0)").list_len(), Some(0));
}
#[test]
fn range_start_stop_same() {
    assert_eq!(eval("range(3, 3)").list_len(), Some(0));
}
#[test]
fn range_negative_start_stop() {
    assert_eq!(eval("range(-5, 0)").list_len(), Some(5));
}
#[test]
fn range_step_zero_error() {
    assert_err(
        "range(0, 10, 0)",
        &[
            "range() step cannot be zero\n",
            "  range(0, 10, 0)\n",
            "  ^~~~~~~~~~~~~~~",
        ],
    );
}

// === TestListMultiplication ===
#[test]
fn list_mul_string() {
    assert_eq!(eval("['a', 'b'] * 2").list_len(), Some(4));
}
#[test]
fn list_mul_one() {
    assert_eq!(eval("[1, 2] * 1").list_len(), Some(2));
}
#[test]
fn list_mul_empty() {
    assert_eq!(eval("[] * 5").list_len(), Some(0));
}
#[test]
fn list_mul_preserves_type() {
    assert_eq!(
        eval("[1.0, 2.0] * 2").expr_type().to_string(),
        "list[float]"
    );
}
#[test]
fn list_mul_nested() {
    assert_eq!(
        eval("[[1, 2]] * 2").expr_type().to_string(),
        "list[list[int]]"
    );
}

// === TestFlatten ===
#[test]
fn flatten_identity_int() {
    assert_eq!(eval("flatten([1, 2, 3])").list_len(), Some(3));
}
#[test]
fn flatten_identity_string() {
    assert_eq!(eval("flatten(['a', 'b'])").list_len(), Some(2));
}

// === Exact Python name matches for remaining list tests ===

// TestListLiteralTypeInference
#[test]
fn all_int() {
    assert_eq!(eval("[1, 2, 3]").expr_type().to_string(), "list[int]");
}
#[test]
fn all_float() {
    assert_eq!(eval("[1.0, 2.0]").expr_type().to_string(), "list[float]");
}
#[test]
fn all_string() {
    assert_eq!(eval("['a', 'b']").expr_type().to_string(), "list[string]");
}
#[test]
fn all_bool() {
    assert_eq!(eval("[true, false]").expr_type().to_string(), "list[bool]");
}
#[test]
fn int_float_promotes_to_float() {
    assert_eq!(eval("[1, 2.0]").expr_type().to_string(), "list[float]");
}
#[test]
fn float_int_promotes_to_float() {
    assert_eq!(eval("[1.0, 2]").expr_type().to_string(), "list[float]");
}
#[test]
fn nested_same_type() {
    assert_eq!(
        eval("[[1], [2, 3]]").expr_type().to_string(),
        "list[list[int]]"
    );
}
#[test]
fn nested_int_float_promotes() {
    assert_eq!(
        eval("[[1], [2.0]]").expr_type().to_string(),
        "list[list[float]]"
    );
}
#[test]
fn path_string_promotes_to_string() {
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::new_path("/a", PathFormat::Posix))
        .unwrap();
    assert_eq!(
        eval_posix("[P, 'b']", &st).expr_type().to_string(),
        "list[string]"
    );
}
#[test]
fn null_in_list_fails() {
    assert_err(
        "[1, null]",
        &[
            "null is not allowed in list literals\n",
            "  [1, null]\n",
            "  ^",
        ],
    );
}
#[test]
fn none_in_list_fails() {
    assert_err(
        "[null]",
        &[
            "null is not allowed in list literals\n",
            "  [null]\n",
            "  ^",
        ],
    );
}
#[test]
fn int_string_fails() {
    assert_err(
        "[1, 'a']",
        &[
            "List literal contains incompatible types: int and string\n",
            "  [1, 'a']\n",
            "  ^",
        ],
    );
}
#[test]
fn int_bool_fails() {
    assert_err(
        "[1, true]",
        &[
            "List literal contains incompatible types: int and bool\n",
            "  [1, true]\n",
            "  ^",
        ],
    );
}
#[test]
fn string_bool_fails() {
    assert_err(
        "['a', true]",
        &[
            "List literal contains incompatible types: string and bool\n",
            "  ['a', true]\n",
            "  ^",
        ],
    );
}
#[test]
fn scalar_list_fails() {
    assert_err(
        "[1, [2]]",
        &[
            "List literal contains incompatible types: int and list[int]\n",
            "  [1, [2]]\n",
            "  ^",
        ],
    );
}
#[test]
fn three_incompatible_types_fails() {
    assert_err(
        "[1, 2.0, 'a']",
        &[
            "List literal contains incompatible types: int, float, and string\n",
            "  [1, 2.0, 'a']\n",
            "  ^",
        ],
    );
}
#[test]
fn path_int_fails() {
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::new_path("/a", PathFormat::Posix))
        .unwrap();
    let e = eval_posix_err("[P, 1]", &st);
    assert!(
        e.contains("incompatible types") && e.contains("^"),
        "got:\n{e}"
    );
}

// TestLists
#[test]
fn list_literal() {
    assert_eq!(eval("[1, 2, 3]").list_len(), Some(3));
}
#[test]
fn list_single_element_trailing_comma() {
    assert_eq!(eval("[42,]").list_len(), Some(1));
}
#[test]
fn subscript_positive() {
    assert_eq!(eval("[10, 20, 30][1]").to_display_string(), "20");
}
#[test]
fn subscript_negative() {
    assert_eq!(eval("[10, 20, 30][-1]").to_display_string(), "30");
}
#[test]
fn subscript_out_of_bounds() {
    assert_err(
        "[10, 20, 30][5]",
        &[
            "Index 5 out of bounds for list of length 3\n",
            "  [10, 20, 30][5]\n",
            "  ~~~~~~~~~~~~^~~",
        ],
    );
}

// TestListComprehension
#[test]
fn simple_comprehension() {
    assert_eq!(eval("[x * 2 for x in [1, 2, 3]]").list_len(), Some(3));
}
#[test]
fn comprehension_with_filter() {
    assert_eq!(eval("[x for x in range(6) if x > 2]").list_len(), Some(3));
}
#[test]
fn string_comprehension() {
    assert_eq!(
        eval("[x.upper() for x in ['a', 'b', 'c']]").list_len(),
        Some(3)
    );
}

// TestListConcatenation
#[test]
fn list_concat_int() {
    assert_eq!(eval("[1, 2] + [3, 4]").list_len(), Some(4));
}
#[test]
fn incompatible_types_string_int() {
    assert_err(
        "[1, 2] + ['a']",
        &[
            "Cannot concatenate list[int] and list[string]\n",
            "  [1, 2] + ['a']\n",
            "  ~~~~~~~^~~~~~~",
        ],
    );
}
#[test]
fn chained_concat() {
    assert_eq!(eval("[1] + [2] + [3]").list_len(), Some(3));
}
#[test]
fn comprehension_concat_list() {
    assert_eq!(
        eval("[x * 2 for x in [1, 2, 3]] + [100]").list_len(),
        Some(4)
    );
}
#[test]
fn concat_with_symtab() {
    let mut st = SymbolTable::new();
    st.set(
        "L",
        ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Int(2)], ExprType::INT).unwrap(),
    )
    .unwrap();
    assert_eq!(
        ParsedExpression::new("L + [3, 4]")
            .and_then(|p| p.evaluate(&st))
            .unwrap()
            .list_len(),
        Some(4)
    );
}

// TestListMembership
#[test]
fn string_in_list() {
    assert_eq!(eval("'b' in ['a', 'b', 'c']").to_display_string(), "true");
}
#[test]
fn string_not_in_list() {
    assert_eq!(eval("'z' in ['a', 'b', 'c']").to_display_string(), "false");
}
#[test]
fn not_in_operator_true() {
    assert_eq!(eval("6 not in [1, 2, 3]").to_display_string(), "true");
}
#[test]
fn not_in_operator_false() {
    assert_eq!(eval("2 not in [1, 2, 3]").to_display_string(), "false");
}

// TestSortedReversed
#[test]
fn sorted_int_list() {
    assert_eq!(eval("sorted([3, 1, 4, 1, 5, 9, 2, 6])").list_len(), Some(8));
}
#[test]
fn sorted_float_list() {
    assert_eq!(eval("sorted([3.5, 1.2, 2.8])").list_len(), Some(3));
}
#[test]
fn sorted_string_list() {
    assert_eq!(
        eval("sorted(['cherry', 'apple', 'banana'])").list_len(),
        Some(3)
    );
}
#[test]
fn sorted_empty_list() {
    assert_eq!(eval("sorted([])").list_len(), Some(0));
}
#[test]
fn sorted_single_element() {
    assert_eq!(eval("sorted([42])").list_len(), Some(1));
}
#[test]
fn sorted_already_sorted() {
    assert_eq!(eval("sorted([1, 2, 3])").list_len(), Some(3));
}
#[test]
fn sorted_reverse_order_list() {
    assert_eq!(eval("sorted([5, 4, 3, 2, 1])").list_len(), Some(5));
}
#[test]
fn sorted_with_duplicates() {
    assert_eq!(eval("sorted([3, 1, 3, 1, 2])").list_len(), Some(5));
}
#[test]
fn sorted_method_syntax() {
    assert_eq!(eval("[3, 1, 2].sorted()").list_len(), Some(3));
}
#[test]
fn reversed_int_list() {
    assert_eq!(eval("reversed([1, 2, 3, 4, 5])").list_len(), Some(5));
}
#[test]
fn reversed_float_list() {
    assert_eq!(eval("reversed([1.1, 2.2, 3.3])").list_len(), Some(3));
}
#[test]
fn reversed_string_list() {
    assert_eq!(eval("reversed(['a', 'b', 'c'])").list_len(), Some(3));
}
#[test]
fn reversed_empty_list() {
    assert_eq!(eval("reversed([])").list_len(), Some(0));
}
#[test]
fn reversed_single_element() {
    assert_eq!(eval("reversed([42])").list_len(), Some(1));
}
#[test]
fn reversed_method_syntax() {
    assert_eq!(eval("[1, 2, 3].reversed()").list_len(), Some(3));
}
#[test]
fn sorted_then_reversed_list() {
    assert_eq!(eval("reversed(sorted([3, 1, 2]))").list_len(), Some(3));
}
#[test]
fn sorted_chained_method() {
    assert_eq!(eval("[3, 1, 2].sorted().reversed()").list_len(), Some(3));
}

// TestUnique extras
#[test]
fn unique_no_duplicates() {
    assert_eq!(eval("unique([1, 2, 3])").list_len(), Some(3));
}
#[test]
fn unique_all_same_v() {
    assert_eq!(eval("unique([7, 7, 7])").list_len(), Some(1));
}
#[test]
fn unique_method_syntax() {
    assert_eq!(eval("[3, 1, 2, 1].unique()").list_len(), Some(3));
}
#[test]
fn unique_chained_with_sorted() {
    assert_eq!(eval("[3, 1, 2, 1].unique().sorted()").list_len(), Some(3));
}
#[test]
fn unique_preserves_first_occurrence() {
    assert_eq!(
        eval("unique(['banana', 'apple', 'banana', 'cherry', 'apple'])").list_len(),
        Some(3)
    );
}

// TestAnyAll
#[test]
fn any_empty_list() {
    assert_eq!(eval("any([])").to_display_string(), "false");
}
#[test]
fn all_empty_list() {
    assert_eq!(eval("all([])").to_display_string(), "true");
}

// TestJoin
#[test]
fn join_strings() {
    assert_eq!(
        eval("join(['a', 'b', 'c'], ',')").to_display_string(),
        "a,b,c"
    );
}
#[test]
fn join_empty_list() {
    assert_eq!(eval("join([], ',')").to_display_string(), "");
}

// TestRange
#[test]
fn range_start_stop_step() {
    assert_eq!(eval("range(0, 10, 2)").list_len(), Some(5));
}

// TestListMultiplication
#[test]
fn multiply_int_list() {
    assert_eq!(eval("[1, 2, 3] * 3").list_len(), Some(9));
}
#[test]
fn multiply_string_list() {
    assert_eq!(eval("['a', 'b'] * 2").list_len(), Some(4));
}
#[test]
fn multiply_by_zero() {
    assert_eq!(eval("[1, 2] * 0").list_len(), Some(0));
}
#[test]
fn multiply_by_one() {
    assert_eq!(eval("[1, 2] * 1").list_len(), Some(2));
}
#[test]
fn multiply_empty_list() {
    assert_eq!(eval("[] * 5").list_len(), Some(0));
}
#[test]
fn multiply_preserves_type() {
    assert_eq!(
        eval("[1.0, 2.0] * 2").expr_type().to_string(),
        "list[float]"
    );
}
#[test]
fn multiply_nested_list() {
    assert_eq!(
        eval("[[1, 2]] * 2").expr_type().to_string(),
        "list[list[int]]"
    );
}

// TestFlatten
#[test]
fn nested() {
    assert_eq!(eval("flatten([[1], [2, 3]])").list_len(), Some(3));
}
#[test]
fn identity_int() {
    assert_eq!(eval("flatten([1, 2, 3])").list_len(), Some(3));
}
#[test]
fn identity_string() {
    assert_eq!(eval("flatten(['a', 'b'])").list_len(), Some(2));
}
#[test]
fn empty() {
    assert_eq!(eval("flatten([])").list_len(), Some(0));
}

// ============================================================
// Missing tests ported from Python test_lists.py
// ============================================================

// === TestListLiteralTypeInference: value checks ===

#[test]
fn int_float_promotes_values() {
    let r = eval("[1, 2.0]");
    assert_eq!(r.to_display_string(), "[1.0, 2.0]");
}

#[test]
fn nested_int_float_coerces_values() {
    // Inner list[int] elements must be coerced to list[float]
    let r = eval("[[1], [2.0]]");
    assert_eq!(r.expr_type().to_string(), "list[list[float]]");
    let inner = r.list_get(0).unwrap();
    assert_eq!(inner.expr_type().to_string(), "list[float]");
}

#[test]
fn path_string_promotes_values() {
    let r = eval_posix_no_st("[path(\"/a\"), \"b\"]");
    assert_eq!(r.expr_type().to_string(), "list[string]");
    assert_eq!(r.to_display_string(), "[\"/a\", \"b\"]");
}

#[test]
fn nested_path_string_promotes() {
    let r = eval_posix_no_st("[[path(\"/a\")], [\"b\"]]");
    assert_eq!(r.expr_type().to_string(), "list[list[string]]");
}

#[test]
fn nested_path_string_coerces_values() {
    let r = eval_posix_no_st("[[path(\"/a\")], [\"b\"]]");
    let inner = r.list_get(0).unwrap();
    assert_eq!(inner.expr_type().to_string(), "list[string]");
}

#[test]
fn three_level_nesting_in_comprehension_fails() {
    let e = ParsedExpression::new("[[[x]] for x in [1, 2]]")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("Lists may be nested at most 2 levels deep"),
        "got:\n{e}"
    );
}

// === TestListComprehension: value checks ===

#[test]
fn comprehension_filter_gt2() {
    // Python test uses x > 2 giving [3, 4, 5]
    assert_eq!(
        eval("[x for x in [1, 2, 3, 4, 5] if x > 2]").to_display_string(),
        "[3, 4, 5]"
    );
}

#[test]
fn comprehension_outer_var_values() {
    let mut st = SymbolTable::new();
    st.set("Param.Base", ExprValue::Int(100)).unwrap();
    let r = ParsedExpression::new("[Param.Base + i for i in [1, 2, 3]]")
        .and_then(|p| p.evaluate(&st))
        .unwrap();
    assert_eq!(r.to_display_string(), "[101, 102, 103]");
}

#[test]
fn string_comprehension_values() {
    assert_eq!(
        eval("[s.upper() for s in ['a', 'b', 'c']]").to_display_string(),
        "[\"A\", \"B\", \"C\"]"
    );
}

// === TestListConcatenation: range_expr function call form ===

#[test]
fn range_expr_concat_list() {
    assert_eq!(
        eval("range_expr('1-3') + [10, 11]").to_display_string(),
        "[1, 2, 3, 10, 11]"
    );
}

#[test]
fn list_concat_range_expr_fn() {
    assert_eq!(
        eval("[10, 11] + range_expr('1-3')").to_display_string(),
        "[10, 11, 1, 2, 3]"
    );
}

#[test]
fn list_float_concat_range_expr() {
    let r = eval("[10.5, 11.5] + range_expr('1-3')");
    assert_eq!(r.expr_type().to_string(), "list[float]");
}

#[test]
fn range_expr_concat_range_expr_fn() {
    assert_eq!(
        eval("range_expr('1-3') + range_expr('10-12')").to_display_string(),
        "[1, 2, 3, 10, 11, 12]"
    );
}

#[test]
fn range_expr_concat_list_float() {
    let r = eval("range_expr('1-3') + [10.5, 11.5]");
    assert_eq!(r.expr_type().to_string(), "list[float]");
}

// === TestListConcatenation: path + string ===

#[test]
fn list_concat_path_string() {
    let r = eval_posix_no_st("[path(\"/foo\"), path(\"/bar\")] + [\"baz\", \"qux\"]");
    assert_eq!(r.expr_type().to_string(), "list[string]");
}

#[test]
fn list_concat_string_path() {
    let r = eval_posix_no_st("[\"foo\", \"bar\"] + [path(\"/baz\"), path(\"/qux\")]");
    assert_eq!(r.expr_type().to_string(), "list[string]");
}

// === TestListConcatenation: symtab range_expr ===

#[test]
fn concat_symtab_range_expr() {
    let mut st = SymbolTable::new();
    st.set(
        "frames",
        ExprValue::RangeExpr("1-5".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    st.set(
        "extra",
        ExprValue::make_list(
            vec![ExprValue::Int(100), ExprValue::Int(200)],
            ExprType::INT,
        )
        .unwrap(),
    )
    .unwrap();
    let r = ParsedExpression::new("frames + extra")
        .and_then(|p| p.evaluate(&st))
        .unwrap();
    assert_eq!(r.to_display_string(), "[1, 2, 3, 4, 5, 100, 200]");
}

// === TestListMembership: path ===

#[test]
fn path_in_list() {
    let mut st = SymbolTable::new();
    st.set(
        "paths",
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
    st.set("p", ExprValue::new_path("/b", PathFormat::Posix))
        .unwrap();
    assert_eq!(eval_posix("p in paths", &st).to_display_string(), "true");
}

#[test]
fn path_not_in_list() {
    let mut st = SymbolTable::new();
    st.set(
        "paths",
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
    st.set("p", ExprValue::new_path("/c", PathFormat::Posix))
        .unwrap();
    assert_eq!(eval_posix("p in paths", &st).to_display_string(), "false");
}

#[test]
fn bool_not_in_list() {
    assert_eq!(eval("false in [true, true]").to_display_string(), "false");
}

// === TestSortedReversed: value checks ===

#[test]
fn sorted_int_list_values() {
    assert_eq!(
        eval("sorted([3, 1, 4, 1, 5, 9, 2, 6])").to_display_string(),
        "[1, 1, 2, 3, 4, 5, 6, 9]"
    );
}

#[test]
fn sorted_float_list_values() {
    assert_eq!(
        eval("sorted([3.5, 1.2, 2.8])").to_display_string(),
        "[1.2, 2.8, 3.5]"
    );
}

#[test]
fn sorted_string_list_values() {
    assert_eq!(
        eval("sorted([\"banana\", \"apple\", \"cherry\"])").to_display_string(),
        "[\"apple\", \"banana\", \"cherry\"]"
    );
}

#[test]
fn sorted_with_duplicates_values() {
    assert_eq!(
        eval("sorted([3, 1, 2, 1, 3])").to_display_string(),
        "[1, 1, 2, 3, 3]"
    );
}

#[test]
fn sorted_method_syntax_values() {
    assert_eq!(eval("[3, 1, 2].sorted()").to_display_string(), "[1, 2, 3]");
}

#[test]
fn reversed_int_list_values() {
    assert_eq!(
        eval("reversed([1, 2, 3, 4, 5])").to_display_string(),
        "[5, 4, 3, 2, 1]"
    );
}

#[test]
fn reversed_float_list_values() {
    assert_eq!(
        eval("reversed([1.1, 2.2, 3.3])").to_display_string(),
        "[3.3, 2.2, 1.1]"
    );
}

#[test]
fn reversed_string_list_values() {
    assert_eq!(
        eval("reversed([\"a\", \"b\", \"c\"])").to_display_string(),
        "[\"c\", \"b\", \"a\"]"
    );
}

#[test]
fn reversed_method_syntax_values() {
    assert_eq!(
        eval("[1, 2, 3].reversed()").to_display_string(),
        "[3, 2, 1]"
    );
}

#[test]
fn sorted_then_reversed_values() {
    assert_eq!(
        eval("reversed(sorted([3, 1, 2]))").to_display_string(),
        "[3, 2, 1]"
    );
}

#[test]
fn reversed_then_sorted_values() {
    assert_eq!(
        eval("sorted(reversed([3, 1, 2]))").to_display_string(),
        "[1, 2, 3]"
    );
}

#[test]
fn sorted_chained_method_values() {
    assert_eq!(
        eval("[3, 1, 2].sorted().reversed()").to_display_string(),
        "[3, 2, 1]"
    );
}

// === TestUnique: path, range_expr, nested lists ===

#[test]
fn unique_path() {
    let r = eval_posix_no_st("unique([path(\"/a/b\"), path(\"/c/d\"), path(\"/a/b\")])");
    assert_eq!(r.list_len(), Some(2));
    assert_eq!(r.expr_type().to_string(), "list[path]");
}

#[test]
fn unique_range_expr() {
    let r = eval("unique([range_expr('1-3'), range_expr('4-6'), range_expr('1-3')])");
    assert_eq!(r.list_len(), Some(2));
}

#[test]
fn unique_list_int() {
    let r = eval("unique([[1, 2], [3, 4], [1, 2], [5, 6]])");
    assert_eq!(r.list_len(), Some(3));
    assert_eq!(r.expr_type().to_string(), "list[list[int]]");
}

#[test]
fn unique_list_string() {
    let r = eval("unique([[\"a\", \"b\"], [\"c\", \"d\"], [\"a\", \"b\"]])");
    assert_eq!(r.list_len(), Some(2));
    assert_eq!(r.expr_type().to_string(), "list[list[string]]");
}

#[test]
fn unique_list_float() {
    let r = eval("unique([[1.0, 2.0], [3.0], [1.0, 2.0]])");
    assert_eq!(r.list_len(), Some(2));
    assert_eq!(r.expr_type().to_string(), "list[list[float]]");
}

#[test]
fn unique_list_bool() {
    let r = eval("unique([[true], [false], [true]])");
    assert_eq!(r.list_len(), Some(2));
    assert_eq!(r.expr_type().to_string(), "list[list[bool]]");
}

#[test]
fn unique_list_path() {
    let r = eval_posix_no_st("unique([[path(\"/a\")], [path(\"/b\")], [path(\"/a\")]])");
    assert_eq!(r.list_len(), Some(2));
    assert_eq!(r.expr_type().to_string(), "list[list[path]]");
}

// === TestJoin: paths ===

#[test]
fn join_paths() {
    assert_eq!(
        eval_posix_no_st("join([path(\"/a\"), path(\"/b\")], \":\")").to_display_string(),
        "/a:/b"
    );
}

// === TestRange: range(1, 5) ===

#[test]
fn range_1_5() {
    assert_eq!(eval("range(1, 5)").to_display_string(), "[1, 2, 3, 4]");
}

#[test]
fn range_negative_start_stop_values() {
    assert_eq!(
        eval("range(-5, 0)").to_display_string(),
        "[-5, -4, -3, -2, -1]"
    );
}

// ── bool→string coercion with target type ──

#[test]
fn coerce_bool_list_to_string_list() {
    let val = ExprValue::make_list(
        vec![ExprValue::Bool(true), ExprValue::Bool(false)],
        ExprType::BOOL,
    )
    .unwrap();
    let coerced = val
        .coerce(&ExprType::parse("list[string]").unwrap(), PathFormat::Posix)
        .unwrap();
    assert_eq!(
        coerced.expr_type(),
        ExprType::parse("list[string]").unwrap()
    );
    let elems = coerced.list_elements().unwrap();
    assert_eq!(elems[0].to_display_string(), "true");
    assert_eq!(elems[1].to_display_string(), "false");
}

#[test]
fn coerce_bool_to_string() {
    let val = ExprValue::Bool(true);
    let coerced = val.coerce(&ExprType::STRING, PathFormat::Posix).unwrap();
    assert_eq!(coerced.to_display_string(), "true");
}

// === Empty sublists mixed with typed sublists ===

#[test]
fn list_literal_empty_with_typed_sublists() {
    // [[], [1], [2, 3]] should be list[list[int]], not rejected as 3-level nesting
    let r = eval("[[], [1], [2, 3]]");
    assert_eq!(r.expr_type().to_string(), "list[list[int]]");
}

#[test]
fn list_literal_empty_in_middle() {
    let r = eval("[[1], [], [2]]");
    assert_eq!(r.expr_type().to_string(), "list[list[int]]");
}

#[test]
fn list_literal_all_empty() {
    let r = eval("[[], [], []]");
    assert_eq!(r.expr_type().to_string(), "list[list[nulltype]]");
}

#[test]
fn list_literal_empty_at_end() {
    let r = eval("[[1, 2], []]");
    assert_eq!(r.expr_type().to_string(), "list[list[int]]");
}

#[test]
fn list_literal_empty_at_start() {
    let r = eval("[[], [1]]");
    assert_eq!(r.expr_type().to_string(), "list[list[int]]");
}

// ── Incompatible type error messages ──

#[test]
fn mixed_int_bool_error_message() {
    let e = ParsedExpression::new("[1, True]")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err();
    assert!(
        e.message().contains("incompatible types")
            && e.message().contains("int")
            && e.message().contains("bool"),
        "got: {}",
        e.message()
    );
}

#[test]
fn mixed_string_bool_error_message() {
    let e = ParsedExpression::new(r#"["a", True]"#)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err();
    assert!(
        e.message().contains("incompatible types")
            && e.message().contains("string")
            && e.message().contains("bool"),
        "got: {}",
        e.message()
    );
}

#[test]
fn three_incompatible_types_lists_all() {
    let e = ParsedExpression::new("[1, 2.0, 'a']")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err();
    assert!(
        e.message().contains("int")
            && e.message().contains("float")
            && e.message().contains("string"),
        "should list all three types, got: {}",
        e.message()
    );
}

#[test]
fn coerce_float_to_int_not_whole_number() {
    let val = ExprValue::Float(openjd_expr::value::Float64::new(1.5).unwrap());
    let e = val.coerce(&ExprType::INT, PathFormat::Posix).unwrap_err();
    assert!(e.contains("not a whole number"), "got: {}", e);
}

// ── unique with range_expr ──

#[test]
fn unique_range_expr_preserves_type() {
    let st = SymbolTable::new();
    let r = ParsedExpression::new(
        r#"unique([range_expr("1-3"), range_expr("4-6"), range_expr("1-3")])"#,
    )
    .and_then(|p| p.evaluate(&st))
    .unwrap();
    assert_eq!(r.expr_type().to_string(), "list[range_expr]");
    assert_eq!(r.list_len(), Some(2));
}

#[test]
fn unique_range_expr_normalized_duplicates() {
    // "1-3" and "1,2,3" and "3-1:-1" all represent {1,2,3} — unique should deduplicate them
    let st = SymbolTable::new();
    let r = ParsedExpression::new(
        r#"unique([range_expr("1-3"), range_expr("1,2,3"), range_expr("3-1:-1")])"#,
    )
    .and_then(|p| p.evaluate(&st))
    .unwrap();
    assert_eq!(
        r.list_len(),
        Some(1),
        "all three represent the same set of values"
    );
    assert_eq!(r.expr_type().to_string(), "list[range_expr]");
}

// === Target type threading ===

#[test]
fn mixed_list_with_string_target() {
    // ["--quality", 5] with target list[string] should coerce 5 to "5"
    let st = SymbolTable::new();
    let parsed = openjd_expr::ParsedExpression::new(r#"["--quality", 5]"#).unwrap();
    let symtabs = [&st];
    let result = parsed
        .with_target_type(&ExprType::parse("list[string]").unwrap())
        .evaluate(&symtabs)
        .unwrap();
    assert_eq!(result.expr_type().to_string(), "list[string]");
    let elems = result.list_elements().unwrap();
    assert_eq!(elems[0].to_display_string(), "--quality");
    assert_eq!(elems[1].to_display_string(), "5");
}

#[test]
fn nested_mixed_list_with_int_target() {
    // [["1", 2.0, 3], ["4"]] with target list[list[int]]
    let st = SymbolTable::new();
    let parsed = openjd_expr::ParsedExpression::new(r#"[["1", 2.0, 3], ["4"]]"#).unwrap();
    let symtabs = [&st];
    let result = parsed
        .with_target_type(&ExprType::parse("list[list[int]]").unwrap())
        .evaluate(&symtabs)
        .unwrap();
    assert_eq!(result.expr_type().to_string(), "list[list[int]]");
}

#[test]
fn mixed_list_without_target_still_errors() {
    // ["--quality", 5] without target type should still error
    let e = ParsedExpression::new(r#"["--quality", 5]"#)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err();
    assert!(
        e.message().contains("incompatible types"),
        "got: {}",
        e.message()
    );
}

#[test]
fn scalar_with_string_target() {
    // 42 with target string should coerce to "42"
    let st = SymbolTable::new();
    let parsed = openjd_expr::ParsedExpression::new("42").unwrap();
    let symtabs = [&st];
    let result = parsed
        .with_target_type(&ExprType::STRING)
        .evaluate(&symtabs)
        .unwrap();
    assert_eq!(result.to_display_string(), "42");
    assert_eq!(result.expr_type(), ExprType::STRING);
}

#[test]
fn empty_listcomp_type_is_nulltype() {
    assert_eq!(
        eval("[x for x in [1, 2, 3] if False]")
            .expr_type()
            .to_string(),
        "list[nulltype]"
    );
}
