// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test_slicing.py

use openjd_expr::{ExprType, ExprValue, ParsedExpression, PathFormat, RangeExpr, SymbolTable};

fn eval(expr: &str) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap()
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

// === TestListSlicing ===
#[test]
fn list_basic_slice() {
    assert_eq!(
        eval("[1, 2, 3, 4, 5][1:4]").to_display_string(),
        "[2, 3, 4]"
    );
}
#[test]
fn list_slice_from_start() {
    assert_eq!(eval("[1, 2, 3, 4, 5][:3]").to_display_string(), "[1, 2, 3]");
}
#[test]
fn list_slice_to_end() {
    assert_eq!(eval("[1, 2, 3, 4, 5][2:]").to_display_string(), "[3, 4, 5]");
}
#[test]
fn list_slice_step() {
    assert_eq!(
        eval("[1, 2, 3, 4, 5][::2]").to_display_string(),
        "[1, 3, 5]"
    );
}
#[test]
fn list_slice_reverse() {
    assert_eq!(
        eval("[1, 2, 3, 4, 5][::-1]").to_display_string(),
        "[5, 4, 3, 2, 1]"
    );
}
#[test]
fn list_slice_neg_start() {
    assert_eq!(
        eval("[1, 2, 3, 4, 5][-3:]").to_display_string(),
        "[3, 4, 5]"
    );
}
#[test]
fn list_slice_neg_stop() {
    assert_eq!(
        eval("[1, 2, 3, 4, 5][1:-1]").to_display_string(),
        "[2, 3, 4]"
    );
}
#[test]
fn list_slice_empty() {
    assert_eq!(eval("[1, 2, 3][5:10]").to_display_string(), "[]");
}
#[test]
fn list_slice_step_zero() {
    assert!(eval_fails("[1, 2, 3][::0]"));
}

// === TestStringSlicing ===
#[test]
fn str_basic_slice() {
    assert_eq!(eval("'hello'[1:4]").to_display_string(), "ell");
}
#[test]
fn str_slice_from_start() {
    assert_eq!(eval("'hello'[:3]").to_display_string(), "hel");
}
#[test]
fn str_slice_to_end() {
    assert_eq!(eval("'hello'[2:]").to_display_string(), "llo");
}
#[test]
fn str_slice_reverse() {
    assert_eq!(eval("'hello'[::-1]").to_display_string(), "olleh");
}
#[test]
fn str_slice_step() {
    assert_eq!(eval("'hello'[::2]").to_display_string(), "hlo");
}

// === TestRangeExprSlicing ===
#[test]
fn range_expr_slice() {
    assert_eq!(eval("range_expr('1-10')[2:5]").to_display_string(), "3-5");
}
#[test]
fn range_expr_slice_reverse() {
    assert_eq!(
        eval("range_expr('1-5')[::-1]").to_display_string(),
        "[5, 4, 3, 2, 1]"
    );
}

// === TestPathSlicing ===
// Path slicing delegates to string slicing in Python; not yet implemented in Rust

// === TestSlicingWithExpressions ===
#[test]
fn slice_with_var() {
    let mut st = SymbolTable::new();
    st.set("N", ExprValue::Int(2)).unwrap();
    assert_eq!(
        ParsedExpression::new("[10, 20, 30, 40, 50][:N]")
            .and_then(|p| p.evaluate(&st))
            .unwrap()
            .to_display_string(),
        "[10, 20]"
    );
}

// === Additional slicing tests ===
#[test]
fn list_slice_empty_result() {
    assert_eq!(eval("[1, 2, 3][5:10]").list_len(), Some(0));
}
#[test]
fn list_slice_step_zero_error() {
    assert_err(
        "[1,2,3][::0]",
        &[
            "Slice step cannot be zero\n",
            "  [1,2,3][::0]\n",
            "  ~~~~~~~^~~~~",
        ],
    );
}
#[test]
fn string_single_index() {
    assert_eq!(eval("'hello'[1]").to_display_string(), "e");
}
#[test]
fn string_negative_index() {
    assert_eq!(eval("'hello'[-1]").to_display_string(), "o");
}
#[test]
fn string_index_out_of_bounds() {
    assert_err(
        "'hello'[10]",
        &[
            "Index 10 out of bounds for string of length 5\n",
            "  'hello'[10]\n",
            "  ~~~~~~~^~~~",
        ],
    );
}
#[test]
fn string_slice_negative_indices() {
    assert_eq!(eval("'hello'[-3:-1]").to_display_string(), "ll");
}
#[test]
fn path_index_not_supported() {
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::new_path("/a/b", PathFormat::Posix))
        .unwrap();
    let parsed = openjd_expr::ParsedExpression::new("P[0]").unwrap();
    let symtabs = [&st];
    let e = parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(&["Cannot subscript type path\n", "  P[0]\n", "  ~^~~"].concat()),
        "got:\n{e}"
    );
}
#[test]
fn slice_with_variable_bounds() {
    let mut st = SymbolTable::new();
    st.set(
        "L",
        ExprValue::make_list(
            vec![
                ExprValue::Int(1),
                ExprValue::Int(2),
                ExprValue::Int(3),
                ExprValue::Int(4),
            ],
            ExprType::INT,
        )
        .unwrap(),
    )
    .unwrap();
    st.set("S", ExprValue::Int(1)).unwrap();
    st.set("E", ExprValue::Int(3)).unwrap();
    assert_eq!(
        ParsedExpression::new("L[S:E]")
            .and_then(|p| p.evaluate(&st))
            .unwrap()
            .list_len(),
        Some(2)
    );
}
#[test]
fn chained_slice() {
    assert!(eval("[1, 2, 3, 4, 5][1:4][0:2]").is_list());
}

// === Exact Python name matches for slicing ===
// List slicing
#[test]
fn basic_slice() {
    assert_eq!(eval("[1, 2, 3, 4, 5][1:3]").list_len(), Some(2));
}
#[test]
fn slice_from_start() {
    assert_eq!(eval("[1, 2, 3, 4, 5][:3]").list_len(), Some(3));
}
#[test]
fn slice_to_end() {
    assert_eq!(eval("[1, 2, 3, 4, 5][2:]").list_len(), Some(3));
}
#[test]
fn slice_with_step() {
    assert_eq!(eval("[1, 2, 3, 4, 5][::2]").list_len(), Some(3));
}
#[test]
fn slice_reverse() {
    assert_eq!(eval("[1, 2, 3][::-1]").list_len(), Some(3));
}
#[test]
fn slice_negative_start() {
    assert_eq!(eval("[1, 2, 3, 4, 5][-3:]").list_len(), Some(3));
}
#[test]
fn slice_negative_stop() {
    assert_eq!(eval("[1, 2, 3, 4, 5][:-2]").list_len(), Some(3));
}
#[test]
fn slice_empty_result_v() {
    assert_eq!(eval("[1, 2, 3][5:10]").list_len(), Some(0));
}
#[test]
fn slice_step_zero_error() {
    assert_err(
        "[1, 2, 3][::0]",
        &[
            "Slice step cannot be zero\n",
            "  [1, 2, 3][::0]\n",
            "  ~~~~~~~~~^~~~~",
        ],
    );
}
// String slicing
#[test]
fn string_basic_slice() {
    assert_eq!(eval("'hello'[1:3]").to_display_string(), "el");
}
#[test]
fn string_slice_from_start() {
    assert_eq!(eval("'hello'[:3]").to_display_string(), "hel");
}
#[test]
fn string_slice_to_end() {
    assert_eq!(eval("'hello'[2:]").to_display_string(), "llo");
}
#[test]
fn string_slice_reverse() {
    assert_eq!(eval("'hello'[::-1]").to_display_string(), "olleh");
}
#[test]
fn string_slice_with_step() {
    assert_eq!(eval("'hello'[::2]").to_display_string(), "hlo");
}
#[test]
fn single_index() {
    assert_eq!(eval("'hello'[1]").to_display_string(), "e");
}
#[test]
fn negative_index() {
    assert_eq!(eval("'hello'[-1]").to_display_string(), "o");
}
#[test]
fn index_out_of_bounds() {
    assert_err(
        "'hello'[10]",
        &[
            "Index 10 out of bounds for string of length 5\n",
            "  'hello'[10]\n",
            "  ~~~~~~~^~~~",
        ],
    );
}
// Range slicing
#[test]
fn range_basic_slice() {
    let mut st = SymbolTable::new();
    st.set(
        "R",
        ExprValue::RangeExpr("1-5".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    assert!(matches!(
        ParsedExpression::new("R[1:3]")
            .and_then(|p| p.evaluate(&st))
            .unwrap(),
        ExprValue::RangeExpr(_)
    ));
}
#[test]
fn range_slice_with_step() {
    let mut st = SymbolTable::new();
    st.set(
        "R",
        ExprValue::RangeExpr("1-10".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    assert!(matches!(
        ParsedExpression::new("R[::2]")
            .and_then(|p| p.evaluate(&st))
            .unwrap(),
        ExprValue::RangeExpr(_)
    ));
}
#[test]
fn range_slice_reverse() {
    let mut st = SymbolTable::new();
    st.set(
        "R",
        ExprValue::RangeExpr("1-5".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    assert!(ParsedExpression::new("R[::-1]")
        .and_then(|p| p.evaluate(&st))
        .unwrap()
        .is_list());
}
#[test]
fn slice_negative_indices() {
    assert_eq!(eval("'hello'[-3:-1]").to_display_string(), "ll");
}
#[test]
fn slice_on_split_result() {
    assert!(eval("'a,b,c,d'.split(',')[1:3]").is_list());
}

// === TestLargeRangeSlicing ===
#[test]
fn slice_large_range_no_materialization() {
    // A range with 1 billion elements — slicing should NOT materialize the whole range
    let mut st = SymbolTable::new();
    st.set(
        "R",
        ExprValue::RangeExpr("1-1000000000".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    assert_eq!(
        ParsedExpression::new("R[0:3]")
            .and_then(|p| p.evaluate(&st))
            .unwrap()
            .to_display_string(),
        "1-3"
    );
}

#[test]
fn slice_large_range_reverse_tail() {
    let mut st = SymbolTable::new();
    st.set(
        "R",
        ExprValue::RangeExpr("1-1000000000".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    assert_eq!(
        ParsedExpression::new("R[-3:]")
            .and_then(|p| p.evaluate(&st))
            .unwrap()
            .to_display_string(),
        "999999998-1000000000"
    );
}

#[test]
fn slice_large_range_returns_range_expr() {
    let mut st = SymbolTable::new();
    st.set(
        "R",
        ExprValue::RangeExpr("1-1000000000".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    let result = ParsedExpression::new("R[0:3]")
        .and_then(|p| p.evaluate(&st))
        .unwrap();
    assert!(matches!(result, ExprValue::RangeExpr(_)));
}

// === Tests ported from Python test_slicing.py — missing coverage ===

// TestStringSlicing::test_slice_with_step (exact Python input: "abcdefg"[::2] -> "aceg")
#[test]
fn str_slice_step_abcdefg() {
    assert_eq!(eval("'abcdefg'[::2]").to_display_string(), "aceg");
}

// TestStringSlicing::test_single_index (exact Python input: "hello"[0] -> "h")
#[test]
fn str_single_index_zero() {
    assert_eq!(eval("'hello'[0]").to_display_string(), "h");
}

// TestRangeExprSlicing::test_slice_with_step (exact output check)
#[test]
fn range_expr_slice_step_exact() {
    assert_eq!(eval("range_expr('1-10')[::2]").to_display_string(), "1-9:2");
}

// TestRangeExprSlicing::test_slice_negative_indices
#[test]
fn range_expr_slice_neg_indices() {
    assert_eq!(eval("range_expr('1-10')[-3:]").to_display_string(), "8-10");
}

// TestPathSlicing::test_path_index_not_supported (via path() function, matching Python)
#[test]
fn path_func_index_not_supported() {
    assert_err(
        "path('/a/b/c')[0]",
        &[
            "Cannot subscript type path\n",
            "  path('/a/b/c')[0]\n",
            "  ~~~~~~~~~~~~~~^~~",
        ],
    );
}

// TestPathSlicing::test_path_slice_not_supported
#[test]
fn path_func_slice_not_supported() {
    assert_err(
        "path('/a/b/c')[1:3]",
        &[
            "Cannot subscript type path\n",
            "  path('/a/b/c')[1:3]\n",
            "  ~~~~~~~~~~~~~~^~~~~",
        ],
    );
}

// TestSlicingWithExpressions::test_slice_with_variable_bounds (exact Python: start=1, end=4)
#[test]
fn slice_with_start_end_vars() {
    let mut st = SymbolTable::new();
    st.set("start", ExprValue::Int(1)).unwrap();
    st.set("end", ExprValue::Int(4)).unwrap();
    assert_eq!(
        ParsedExpression::new("[1, 2, 3, 4, 5][start:end]")
            .and_then(|p| p.evaluate(&st))
            .unwrap()
            .to_display_string(),
        "[2, 3, 4]"
    );
}

// TestSlicingWithExpressions::test_chained_slice (exact Python: [1:4][::2] -> [2, 4])
#[test]
fn chained_slice_with_step() {
    assert_eq!(
        eval("[1, 2, 3, 4, 5][1:4][::2]").to_display_string(),
        "[2, 4]"
    );
}

// TestSlicingWithExpressions::test_slice_on_split_result (exact Python: semicolon split)
#[test]
fn slice_on_split_semicolon() {
    assert_eq!(
        eval("'a;b;c;d;e'.split(';')[:3]").to_display_string(),
        "[\"a\", \"b\", \"c\"]"
    );
}
