// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test_range_expr.py

use openjd_expr::{ExprValue, ParsedExpression, RangeExpr, SymbolTable};

fn eval(expr: &str) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap()
}
fn eval_with(expr: &str, st: &SymbolTable) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(st))
        .unwrap()
}

fn assert_err(expr: &str, expected: &[&str]) {
    let e = ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

#[test]
fn range_expr_from_string() {
    assert!(matches!(
        eval("range_expr('1-10')"),
        ExprValue::RangeExpr(_)
    ));
}
#[test]
fn range_expr_len() {
    assert_eq!(eval("len(range_expr('1-10'))").to_display_string(), "10");
}
#[test]
fn range_expr_subscript_0() {
    assert_eq!(eval("range_expr('1-10')[0]").to_display_string(), "1");
}
#[test]
fn range_expr_subscript_9() {
    assert_eq!(eval("range_expr('1-10')[9]").to_display_string(), "10");
}
#[test]
fn range_expr_subscript_neg() {
    assert_eq!(eval("range_expr('1-10')[-1]").to_display_string(), "10");
}
#[test]
fn range_expr_subscript_oob() {
    assert_err(
        "range_expr('1-10')[100]",
        &[
            "Index 100 out of bounds for range_expr of length 10\n",
            "  range_expr('1-10')[100]\n",
            "  ~~~~~~~~~~~~~~~~~~^~~~~",
        ],
    );
}
#[test]
fn range_expr_to_list() {
    assert_eq!(
        eval("list(range_expr('1-5'))").to_display_string(),
        "[1, 2, 3, 4, 5]"
    );
}
#[test]
fn range_expr_to_string() {
    assert_eq!(
        eval("string(range_expr('1-5,10-15'))").to_display_string(),
        "1-5,10-15"
    );
}
#[test]
fn range_expr_with_step() {
    assert_eq!(
        eval("list(range_expr('1-10:2'))").to_display_string(),
        "[1, 3, 5, 7, 9]"
    );
}

#[test]
fn range_expr_from_symtab() {
    let mut st = SymbolTable::new();
    st.set(
        "Param.Frames",
        ExprValue::RangeExpr("1-100:10".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    assert_eq!(eval_with("Param.Frames[0]", &st).to_display_string(), "1");
    assert_eq!(
        eval_with("len(Param.Frames)", &st).to_display_string(),
        "10"
    );
}

#[test]
fn range_expr_invalid() {
    assert_err(
        "range_expr('not-a-range')",
        &[
            "Expected integer in 'not-a-range'\n",
            "  range_expr('not-a-range')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn range_expr_empty() {
    assert_err(
        "range_expr('')",
        &[
            "Empty expression\n",
            "  range_expr('')\n",
            "  ^~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn range_expr_from_list() {
    assert!(matches!(
        eval("range_expr([1, 2, 3])"),
        ExprValue::RangeExpr(_)
    ));
}
#[test]
fn range_expr_empty_list() {
    assert_err(
        "range_expr([])",
        &[
            "range_expr() requires at least one value\n",
            "  range_expr([])\n",
            "  ^~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn range_expr_in_comp() {
    assert_eq!(
        eval("[x * 2 for x in range_expr('1-5')]").to_display_string(),
        "[2, 4, 6, 8, 10]"
    );
}
#[test]
fn range_expr_comp_filter() {
    assert_eq!(
        eval("[x for x in range_expr('1-10') if x > 5]").to_display_string(),
        "[6, 7, 8, 9, 10]"
    );
}

#[test]
fn range_expr_min() {
    assert_eq!(eval("min(range_expr('5-10'))").to_display_string(), "5");
}
#[test]
fn range_expr_max() {
    assert_eq!(eval("max(range_expr('5-10'))").to_display_string(), "10");
}
#[test]
fn range_expr_sum() {
    assert_eq!(eval("sum(range_expr('1-5'))").to_display_string(), "15");
}

// === Additional range_expr tests ===
#[test]
fn range_expr_from_list_v2() {
    let r = ParsedExpression::new("range_expr([1, 2, 3])")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap();
    assert!(matches!(r, ExprValue::RangeExpr(_)));
}
#[test]
fn range_expr_min_v2() {
    let mut st = SymbolTable::new();
    st.set(
        "R",
        ExprValue::RangeExpr("3-7".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    assert_eq!(
        ParsedExpression::new("min(R)")
            .and_then(|p| p.evaluate(&st))
            .unwrap()
            .to_display_string(),
        "3"
    );
}
#[test]
fn range_expr_max_v2() {
    let mut st = SymbolTable::new();
    st.set(
        "R",
        ExprValue::RangeExpr("3-7".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    assert_eq!(
        ParsedExpression::new("max(R)")
            .and_then(|p| p.evaluate(&st))
            .unwrap()
            .to_display_string(),
        "7"
    );
}
#[test]
fn range_expr_sum_v2() {
    let mut st = SymbolTable::new();
    st.set(
        "R",
        ExprValue::RangeExpr("1-3".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    let r = ParsedExpression::new("sum(R)")
        .and_then(|p| p.evaluate(&st))
        .unwrap();
    assert_eq!(r.to_display_string(), "6");
}

// === RangeExpr::get() with negative indexing ===
#[test]
fn range_get_positive() {
    let r = "1-5,10-15".parse::<RangeExpr>().unwrap();
    assert_eq!(r.get(0), Some(1));
    assert_eq!(r.get(4), Some(5));
    assert_eq!(r.get(5), Some(10));
    assert_eq!(r.get(10), Some(15));
}
#[test]
fn range_get_negative() {
    let r = "1-5,10-15".parse::<RangeExpr>().unwrap();
    assert_eq!(r.get(-1), Some(15));
    assert_eq!(r.get(-6), Some(10));
    assert_eq!(r.get(-7), Some(5));
    assert_eq!(r.get(-11), Some(1));
}
#[test]
fn range_get_oob() {
    let r = "1-5".parse::<RangeExpr>().unwrap();
    assert_eq!(r.get(5), None);
    assert_eq!(r.get(-6), None);
}

// === RangeExpr::contains() via binary search ===
#[test]
fn range_contains_hit() {
    let r = "1-5,10-15,100-200".parse::<RangeExpr>().unwrap();
    assert!(r.contains(1));
    assert!(r.contains(5));
    assert!(r.contains(12));
    assert!(r.contains(100));
    assert!(r.contains(200));
}
#[test]
fn range_contains_miss() {
    let r = "1-5,10-15,100-200".parse::<RangeExpr>().unwrap();
    assert!(!r.contains(0));
    assert!(!r.contains(6));
    assert!(!r.contains(99));
    assert!(!r.contains(201));
}
#[test]
fn range_contains_stepped() {
    let r = "1-10:2".parse::<RangeExpr>().unwrap(); // 1,3,5,7,9
    assert!(r.contains(1));
    assert!(r.contains(9));
    assert!(!r.contains(2));
    assert!(!r.contains(10));
}

// === RangeExprError ===
use openjd_expr::RangeExprError;

#[test]
fn range_expr_error_display() {
    let e = RangeExprError::new("1-5,bad", "Unexpected 'b'");
    assert_eq!(e.to_string(), "Unexpected 'b': '1-5,bad'");
}
#[test]
fn range_expr_error_with_position() {
    let e = RangeExprError::at("1-5,bad", "Unexpected 'b'", 4);
    assert_eq!(e.to_string(), "Unexpected 'b' in '1-5,bad' after '1-5,'");
}

// === Display format tests ===

#[test]
fn display_single_value() {
    assert_eq!("5".parse::<RangeExpr>().unwrap().to_string(), "5");
}

#[test]
fn display_two_consecutive_as_comma() {
    // Two consecutive integers with step 1 display as "7,8" (matching Python: len==2 → comma form)
    assert_eq!("7-8".parse::<RangeExpr>().unwrap().to_string(), "7,8");
}

#[test]
fn display_two_nonconsecutive_as_comma() {
    // Two non-consecutive integers should display as "3,7"
    assert_eq!("3,7".parse::<RangeExpr>().unwrap().to_string(), "3,7");
}

#[test]
fn display_contiguous_range() {
    assert_eq!("1-10".parse::<RangeExpr>().unwrap().to_string(), "1-10");
}

#[test]
fn display_stepped_range() {
    assert_eq!("1-10:2".parse::<RangeExpr>().unwrap().to_string(), "1-9:2");
}

#[test]
fn display_multi_range() {
    assert_eq!(
        "1-3,5,7-9".parse::<RangeExpr>().unwrap().to_string(),
        "1-3,5,7-9"
    );
}

// === Tests ported from Python that were missing in Rust ===

#[test]
fn range_expr_subscript_neg2() {
    assert_eq!(eval("range_expr('1-10')[-2]").to_display_string(), "9");
}

#[test]
fn range_expr_from_list_non_contiguous() {
    assert_eq!(
        eval("list(range_expr([1, 3, 5, 10]))").to_display_string(),
        "[1, 3, 5, 10]"
    );
}

#[test]
fn range_expr_from_list_duplicates() {
    assert_eq!(
        eval("string(range_expr([1, 1, 1]))").to_display_string(),
        "1"
    );
}

#[test]
fn range_expr_from_list_reverse() {
    assert_eq!(
        eval("string(range_expr([9, 8, 7, 6]))").to_display_string(),
        "6-9"
    );
}

#[test]
fn range_expr_comp_from_symtab() {
    let mut st = SymbolTable::new();
    st.set(
        "Frames",
        ExprValue::RangeExpr("1-100:10".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    let result = eval_with("[f + 1000 for f in Frames]", &st);
    assert_eq!(
        result.to_display_string(),
        "[1001, 1011, 1021, 1031, 1041, 1051, 1061, 1071, 1081, 1091]"
    );
}

#[test]
fn range_expr_min_descending() {
    assert_eq!(eval("min(range_expr('10-5:-1'))").to_display_string(), "5");
}

#[test]
fn range_expr_min_comma_separated() {
    assert_eq!(eval("min(range_expr('1,5,10,3'))").to_display_string(), "1");
}

#[test]
fn range_expr_max_descending() {
    assert_eq!(eval("max(range_expr('10-5:-1'))").to_display_string(), "10");
}

#[test]
fn range_expr_max_comma_separated() {
    assert_eq!(
        eval("max(range_expr('1,5,10,3'))").to_display_string(),
        "10"
    );
}

#[test]
fn range_expr_sum_stepped() {
    assert_eq!(eval("sum(range_expr('1-10:2'))").to_display_string(), "25"); // 1+3+5+7+9
}

#[test]
fn range_expr_min_max_from_symtab() {
    let mut st = SymbolTable::new();
    st.set(
        "Frames",
        ExprValue::RangeExpr("10-50:5".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    assert_eq!(eval_with("min(Frames)", &st).to_display_string(), "10");
    assert_eq!(eval_with("max(Frames)", &st).to_display_string(), "50");
}

#[test]
fn range_expr_max_correctness() {
    assert_eq!(eval("max(range_expr('1-10'))").to_display_string(), "10");
}

#[test]
fn range_expr_max_large_range_perf() {
    let start = std::time::Instant::now();
    assert_eq!(
        eval("max(range_expr('1-1000000000'))").to_display_string(),
        "1000000000"
    );
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 1,
        "max(range_expr('1-1000000000')) took {elapsed:?}, expected < 1s"
    );
}
