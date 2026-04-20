// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_operation_limit.py

use openjd_expr::*;

fn eval_op(expr: &str, limit: usize) -> Result<EvalResult, ExpressionError> {
    ParsedExpression::new(expr).and_then(|p| {
        p.with_memory_limit(usize::MAX)
            .with_operation_limit(limit)
            .evaluate_with_metrics(&[&SymbolTable::new()])
    })
}

fn op_count(expr: &str) -> usize {
    ParsedExpression::new(expr)
        .unwrap()
        .evaluate_with_metrics(&[&SymbolTable::new()])
        .unwrap()
        .operation_count
}

fn op_count_with(expr: &str, st: &SymbolTable) -> usize {
    ParsedExpression::new(expr)
        .unwrap()
        .evaluate_with_metrics(&[st])
        .unwrap()
        .operation_count
}

// === TestDefaultOperationLimit ===

#[test]
fn default_is_10_million() {
    assert_eq!(DEFAULT_OPERATION_LIMIT, 10_000_000);
}

// === TestOperationLimitExceeded ===

#[test]
fn function_calls_count() {
    let e = eval_op("1 + 1", 0).unwrap_err().to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (1) exceeded limit (0)\n",
                "  1 + 1\n",
                "  ~~^~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn range_iterations_count() {
    let e = eval_op("range(100)", 50).unwrap_err().to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (51) exceeded limit (50)\n",
                "  range(100)\n",
                "  ^~~~~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn list_comprehension_iterations_count() {
    let e = eval_op("[x for x in range(1000)]", 50)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (51) exceeded limit (50)\n",
                "  [x for x in range(1000)]\n",
                "              ^~~~~~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn sum_iterations_count() {
    let e = eval_op("sum(range(1000))", 100).unwrap_err().to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (101) exceeded limit (100)\n",
                "  sum(range(1000))\n",
                "      ^~~~~~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn min_iterations_count() {
    let e = eval_op("min(range(1000))", 100).unwrap_err().to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (101) exceeded limit (100)\n",
                "  min(range(1000))\n",
                "      ^~~~~~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn max_iterations_count() {
    let e = eval_op("max(range(1000))", 100).unwrap_err().to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (101) exceeded limit (100)\n",
                "  max(range(1000))\n",
                "      ^~~~~~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn sorted_iterations_count() {
    let e = eval_op("sorted(range(1000))", 100).unwrap_err().to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (101) exceeded limit (100)\n",
                "  sorted(range(1000))\n",
                "         ^~~~~~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn reversed_iterations_count() {
    let e = eval_op("reversed(range(1000))", 100)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (101) exceeded limit (100)\n",
                "  reversed(range(1000))\n",
                "           ^~~~~~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn join_iterations_count() {
    let e = eval_op("['a','b','c','d','e'].join(',')", 2)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (3) exceeded limit (2)\n",
                "  ['a','b','c','d','e'].join(',')\n",
                "  ~~~~~~~~~~~~~~~~~~~~~~^~~~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn contains_iterations_count() {
    let e = eval_op("99 in range(1000)", 100).unwrap_err().to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (101) exceeded limit (100)\n",
                "  99 in range(1000)\n",
                "        ^~~~~~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn list_concat_iterations_count() {
    let e = eval_op("range(500) + range(500)", 100)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (101) exceeded limit (100)\n",
                "  range(500) + range(500)\n",
                "  ^~~~~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn list_multiply_iterations_count() {
    let e = eval_op("[1, 2, 3] * 1000", 100).unwrap_err().to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (101) exceeded limit (100)\n",
                "  [1, 2, 3] * 1000\n",
                "  ~~~~~~~~~~^~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn any_iterations_count() {
    let e = eval_op("any([False] * 1000)", 100).unwrap_err().to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (101) exceeded limit (100)\n",
                "  any([False] * 1000)\n",
                "      ~~~~~~~~^~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn all_iterations_count() {
    let e = eval_op("all([True] * 1000)", 100).unwrap_err().to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (101) exceeded limit (100)\n",
                "  all([True] * 1000)\n",
                "      ~~~~~~~^~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn flatten_iterations_count() {
    let e = eval_op("flatten([[1,2],[3,4]] * 500)", 100)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (101) exceeded limit (100)\n",
                "  flatten([[1,2],[3,4]] * 500)\n",
                "          ~~~~~~~~~~~~~~^~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn repr_sh_list_iterations_count() {
    let e = eval_op("repr_sh(range(1000))", 100)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (101) exceeded limit (100)\n",
                "  repr_sh(range(1000))\n",
                "          ^~~~~~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn list_equality_iterations_count() {
    let e = eval_op("range(1000) == range(1000)", 100)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (101) exceeded limit (100)\n",
                "  range(1000) == range(1000)\n",
                "  ^~~~~~~~~~~"
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

// === TestOperationLimitWithinBounds ===

#[test]
fn simple_arithmetic_within_limit() {
    assert_eq!(eval_op("1 + 2", 10).unwrap().value.to_display_string(), "3");
}

#[test]
fn small_range_within_limit() {
    assert!(eval_op("range(5)", 1000).is_ok());
}

#[test]
fn small_comprehension_within_limit() {
    assert!(eval_op("[x * 2 for x in range(5)]", 1000).is_ok());
}

#[test]
fn default_limit_handles_normal() {
    let r = ParsedExpression::new("sum(range(100))")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap();
    assert_eq!(r.to_display_string(), "4950");
}

#[test]
fn string_operations_within_limit() {
    assert_eq!(
        eval_op("'hello'.upper()", 100)
            .unwrap()
            .value
            .to_display_string(),
        "HELLO"
    );
}

#[test]
fn chained_operations_within_limit() {
    assert!(eval_op("'a,b,c'.split(',').join(';')", 100).is_ok());
}

// === TestOperationCount ===

#[test]
fn operation_count_returned() {
    assert!(op_count("1 + 2") > 0);
}

#[test]
fn constant_has_zero_operations() {
    assert_eq!(op_count("42"), 0);
}

#[test]
fn single_function_call_is_one_operation() {
    assert_eq!(op_count("1 + 2"), 1);
}

#[test]
fn range_counts_call_plus_iterations() {
    // 1 call + 10 iterations = 11
    assert_eq!(op_count("range(10)"), 11);
}

#[test]
fn sum_range_counts_both() {
    // range: 1 call + 10 iterations = 11
    // sum: 1 call + 10 iterations = 11
    // total = 22
    assert_eq!(op_count("sum(range(10))"), 22);
}

#[test]
fn list_comprehension_counts_iterations() {
    // 3 iterations from comprehension + 3 __mul__ calls = 6
    assert_eq!(op_count("[x * 2 for x in [1, 2, 3]]"), 6);
}

#[test]
fn operation_count_increases_with_list_size() {
    let small = op_count("sum(range(10))");
    let large = op_count("sum(range(100))");
    assert!(large > small);
}

#[test]
fn operation_count_resets_each_call() {
    let mut st_large = SymbolTable::new();
    st_large.set("Param.N", ExprValue::Int(100)).unwrap();
    let mut st_small = SymbolTable::new();
    st_small.set("Param.N", ExprValue::Int(5)).unwrap();
    let large = op_count_with("sum(range(Param.N))", &st_large);
    let small = op_count_with("sum(range(Param.N))", &st_small);
    assert!(small < large);
}

#[test]
fn nested_comprehension_accumulates() {
    // range: 1 call + 10 iterations = 11
    // comprehension: 10 iterations
    // 10 __add__ calls
    // total = 31
    assert_eq!(op_count("[x + 1 for x in range(10)]"), 31);
}

// === Additional operation limit tests ===
#[test]
fn constant_has_zero_operations_v2() {
    let r = ParsedExpression::new("42")
        .and_then(|p| {
            p.with_memory_limit(100_000_000)
                .with_operation_limit(10_000_000)
                .evaluate_with_metrics(&[&SymbolTable::new()])
        })
        .unwrap();
    assert_eq!(r.operation_count, 0);
}
#[test]
fn single_function_call_is_one_v2() {
    let r = ParsedExpression::new("abs(-5)")
        .and_then(|p| {
            p.with_memory_limit(100_000_000)
                .with_operation_limit(10_000_000)
                .evaluate_with_metrics(&[&SymbolTable::new()])
        })
        .unwrap();
    assert!(r.operation_count >= 1);
}
#[test]
fn range_counts_call_plus_iterations_v2() {
    let r = ParsedExpression::new("range(5)")
        .and_then(|p| {
            p.with_memory_limit(100_000_000)
                .with_operation_limit(10_000_000)
                .evaluate_with_metrics(&[&SymbolTable::new()])
        })
        .unwrap();
    assert!(r.operation_count >= 5);
}

// === TestOperationAccountingGaps ===
// Verify that functions which do O(N) work charge the operation budget.
// These were previously uncharged (took `_: Ctx`).

#[test]
fn sorted_charges_per_element() {
    let mut st = SymbolTable::new();
    st.set(
        "L",
        ExprValue::make_list((0..100).map(ExprValue::Int).collect(), ExprType::INT).unwrap(),
    )
    .unwrap();
    let ops = op_count_with("sorted(L)", &st);
    assert!(
        ops >= 100,
        "sorted(100 elements) should charge >= 100 ops, got {ops}"
    );
}

#[test]
fn reversed_charges_per_element() {
    let mut st = SymbolTable::new();
    st.set(
        "L",
        ExprValue::make_list((0..100).map(ExprValue::Int).collect(), ExprType::INT).unwrap(),
    )
    .unwrap();
    let ops = op_count_with("reversed(L)", &st);
    assert!(
        ops >= 100,
        "reversed(100 elements) should charge >= 100 ops, got {ops}"
    );
}

#[test]
fn unique_charges_per_element() {
    let mut st = SymbolTable::new();
    st.set(
        "L",
        ExprValue::make_list((0..100).map(ExprValue::Int).collect(), ExprType::INT).unwrap(),
    )
    .unwrap();
    let ops = op_count_with("unique(L)", &st);
    assert!(
        ops >= 100,
        "unique(100 elements) should charge >= 100 ops, got {ops}"
    );
}

#[test]
fn min_charges_per_element() {
    let mut st = SymbolTable::new();
    st.set(
        "L",
        ExprValue::make_list((0..100).map(ExprValue::Int).collect(), ExprType::INT).unwrap(),
    )
    .unwrap();
    let ops = op_count_with("min(L)", &st);
    assert!(
        ops >= 100,
        "min(100 elements) should charge >= 100 ops, got {ops}"
    );
}

#[test]
fn max_charges_per_element() {
    let mut st = SymbolTable::new();
    st.set(
        "L",
        ExprValue::make_list((0..100).map(ExprValue::Int).collect(), ExprType::INT).unwrap(),
    )
    .unwrap();
    let ops = op_count_with("max(L)", &st);
    assert!(
        ops >= 100,
        "max(100 elements) should charge >= 100 ops, got {ops}"
    );
}

#[test]
fn list_concat_charges_per_element() {
    let mut st = SymbolTable::new();
    st.set(
        "A",
        ExprValue::make_list((0..50).map(ExprValue::Int).collect(), ExprType::INT).unwrap(),
    )
    .unwrap();
    st.set(
        "B",
        ExprValue::make_list((0..50).map(ExprValue::Int).collect(), ExprType::INT).unwrap(),
    )
    .unwrap();
    let ops = op_count_with("A + B", &st);
    assert!(
        ops >= 100,
        "list concat (50+50) should charge >= 100 ops, got {ops}"
    );
}

#[test]
fn list_slice_charges_per_output_element() {
    let mut st = SymbolTable::new();
    st.set(
        "L",
        ExprValue::make_list((0..100).map(ExprValue::Int).collect(), ExprType::INT).unwrap(),
    )
    .unwrap();
    let ops = op_count_with("L[10:60]", &st);
    assert!(
        ops >= 50,
        "list slice of 50 elements should charge >= 50 ops, got {ops}"
    );
}

#[test]
fn string_slice_charges_string_ops() {
    let ops = op_count("'abcdefghij'[2:8]");
    // count_string_ops charges ceil(len/256)+1 for the 10-char string = 2
    assert!(ops >= 2, "string slice should charge string ops, got {ops}");
}

#[test]
fn list_from_range_charges_per_element() {
    let mut st = SymbolTable::new();
    st.set("R", ExprValue::RangeExpr("1-100".parse().unwrap()))
        .unwrap();
    let ops = op_count_with("list(R)", &st);
    assert!(
        ops >= 100,
        "list(range of 100) should charge >= 100 ops, got {ops}"
    );
}

#[test]
fn re_match_charges_string_ops() {
    let ops = op_count("re_match('a+', 'aaaaaaaaaa')");
    assert!(ops >= 2, "re_match should charge string ops, got {ops}");
}

#[test]
fn re_replace_charges_string_ops() {
    let ops = op_count("re_sub('a', 'b', 'aaaaaaaaaa')");
    assert!(ops >= 2, "re_sub should charge string ops, got {ops}");
}

#[test]
fn re_split_charges_string_ops() {
    let ops = op_count("re_split('a,b,c,d,e', ',')");
    assert!(ops >= 2, "re_split should charge string ops, got {ops}");
}

#[test]
fn center_charges_string_ops() {
    let ops = op_count("'hi'.center(100)");
    assert!(ops >= 2, "center should charge string ops, got {ops}");
}

#[test]
fn ljust_charges_string_ops() {
    let ops = op_count("'hi'.ljust(100)");
    assert!(ops >= 2, "ljust should charge string ops, got {ops}");
}

#[test]
fn rjust_charges_string_ops() {
    let ops = op_count("'hi'.rjust(100)");
    assert!(ops >= 2, "rjust should charge string ops, got {ops}");
}

// Verify that the operation limit is actually enforced for these functions
// when the input comes from a symbol table (not from range() which charges its own ops)

#[test]
fn sorted_exceeds_limit_via_symtab() {
    let mut st = SymbolTable::new();
    st.set(
        "L",
        ExprValue::make_list((0..1000).map(ExprValue::Int).collect(), ExprType::INT).unwrap(),
    )
    .unwrap();
    let r = ParsedExpression::new("sorted(L)").and_then(|p| {
        p.with_memory_limit(usize::MAX)
            .with_operation_limit(100)
            .evaluate_with_metrics(&[&st])
    });
    assert!(
        r.is_err(),
        "sorted(1000 elements) with limit 100 should fail"
    );
}

#[test]
fn unique_exceeds_limit_via_symtab() {
    let mut st = SymbolTable::new();
    st.set(
        "L",
        ExprValue::make_list((0..1000).map(ExprValue::Int).collect(), ExprType::INT).unwrap(),
    )
    .unwrap();
    let r = ParsedExpression::new("unique(L)").and_then(|p| {
        p.with_memory_limit(usize::MAX)
            .with_operation_limit(100)
            .evaluate_with_metrics(&[&st])
    });
    assert!(
        r.is_err(),
        "unique(1000 elements) with limit 100 should fail"
    );
}

#[test]
fn min_exceeds_limit_via_symtab() {
    let mut st = SymbolTable::new();
    st.set(
        "L",
        ExprValue::make_list((0..1000).map(ExprValue::Int).collect(), ExprType::INT).unwrap(),
    )
    .unwrap();
    let r = ParsedExpression::new("min(L)").and_then(|p| {
        p.with_memory_limit(usize::MAX)
            .with_operation_limit(100)
            .evaluate_with_metrics(&[&st])
    });
    assert!(r.is_err(), "min(1000 elements) with limit 100 should fail");
}

#[test]
fn operation_limit_error_kind_from_evaluator() {
    // Trigger the evaluator's private count_op via chained binary ops
    let mut symtab = SymbolTable::new();
    symtab.set("x", ExprValue::Int(1)).unwrap();
    let parsed = ParsedExpression::new("x + x + x + x").unwrap();
    let err = parsed
        .with_operation_limit(2)
        .evaluate(&[&symtab])
        .unwrap_err();
    assert!(
        matches!(
            err.kind(),
            ExpressionErrorKind::OperationLimitExceeded { .. }
        ),
        "Expected OperationLimitExceeded, got: {:?}",
        err.kind()
    );
}
