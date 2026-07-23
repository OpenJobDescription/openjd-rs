// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

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
                "Expression operation count (3001) exceeded limit (100)\n",
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
                "Expression operation count (1002) exceeded limit (100)\n",
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
                "Expression operation count (1002) exceeded limit (100)\n",
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
                "Expression operation count (1002) exceeded limit (100)\n",
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

#[test]
fn list_containment_precharges_candidate_count() {
    let mut st = SymbolTable::new();
    st.set(
        "L",
        ExprValue::make_list((0..1000).map(ExprValue::Int).collect(), ExprType::INT).unwrap(),
    )
    .unwrap();
    let e = ParsedExpression::new("999 in L")
        .and_then(|p| {
            p.with_memory_limit(usize::MAX)
                .with_operation_limit(100)
                .evaluate_with_metrics(&[&st])
        })
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            "Expression operation count (1002) exceeded limit (100)\n",
            "  999 in L\n",
            "  ^~~~~~~~",
        ]
        .concat()
    );
}

#[test]
fn list_range_equality_bounded_by_list_length() {
    // Comparing a materialized list with a huge symbolic range must not
    // expand the range: cost is O(list_len + 1), decided by walking the
    // range lazily, so it succeeds under a tight operation limit.
    let mut st = SymbolTable::new();
    st.set("L", ExprValue::ListInt(vec![1])).unwrap();
    st.set("R", ExprValue::RangeExpr("1-100000000".parse().unwrap()))
        .unwrap();
    let r = ParsedExpression::new("L == R")
        .and_then(|p| {
            p.with_memory_limit(usize::MAX)
                .with_operation_limit(100)
                .evaluate_with_metrics(&[&st])
        })
        .unwrap();
    assert_eq!(r.value.to_display_string(), "false");
    assert!(
        r.operation_count <= 10,
        "operation_count={} should be bounded by list length, not range length",
        r.operation_count
    );
}

#[test]
fn list_in_list_of_ranges_bounded_by_list_length() {
    // Containment scans must not charge symbolic range lengths either:
    // each candidate comparison is bounded by the needle's length.
    let mut st = SymbolTable::new();
    st.set(
        "L",
        ExprValue::make_list(
            vec![
                ExprValue::RangeExpr("1-100000000".parse().unwrap()),
                ExprValue::RangeExpr("1-2".parse().unwrap()),
            ],
            ExprType::RANGE_EXPR,
        )
        .unwrap(),
    )
    .unwrap();
    let r = ParsedExpression::new("[1, 2] in L")
        .and_then(|p| {
            p.with_memory_limit(usize::MAX)
                .with_operation_limit(100)
                .evaluate_with_metrics(&[&st])
        })
        .unwrap();
    assert_eq!(r.value.to_display_string(), "true");
}

#[test]
fn same_variant_list_equality_charges_budget() {
    // Typed-list fast paths must charge the operation budget like the
    // generic arm: comparing two 1000-element int lists under a limit of
    // 100 must fail, not sneak through the slice-compare shortcut.
    let mut st = SymbolTable::new();
    st.set("A", ExprValue::ListInt((0..1000).collect()))
        .unwrap();
    st.set("B", ExprValue::ListInt((0..1000).collect()))
        .unwrap();
    let e = ParsedExpression::new("A == B")
        .and_then(|p| {
            p.with_memory_limit(usize::MAX)
                .with_operation_limit(100)
                .evaluate_with_metrics(&[&st])
        })
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            "Expression operation count (101) exceeded limit (100)\n",
            "  A == B\n",
            "  ^~~~~~",
        ]
        .concat()
    );
}

#[test]
fn same_variant_list_length_mismatch_uncharged() {
    // A length mismatch is O(1) and free, even for same-variant lists.
    let mut st = SymbolTable::new();
    st.set("A", ExprValue::ListInt((0..1000).collect()))
        .unwrap();
    st.set("B", ExprValue::ListInt((0..999).collect())).unwrap();
    let r = ParsedExpression::new("A == B")
        .and_then(|p| {
            p.with_memory_limit(usize::MAX)
                .with_operation_limit(100)
                .evaluate_with_metrics(&[&st])
        })
        .unwrap();
    assert_eq!(r.value.to_display_string(), "false");
}

#[test]
fn list_range_length_mismatch_charged_by_steps_taken() {
    // A long list against a short range must fail fast and cheap: the
    // walk charges per comparison performed, so the mismatch at the
    // range's exhaustion point costs O(range_len), not O(list_len).
    let mut st = SymbolTable::new();
    st.set("L", ExprValue::ListInt((1..=1000).collect()))
        .unwrap();
    st.set("R", ExprValue::RangeExpr("1".parse().unwrap()))
        .unwrap();
    let r = ParsedExpression::new("L == R")
        .and_then(|p| {
            p.with_memory_limit(usize::MAX)
                .with_operation_limit(100)
                .evaluate_with_metrics(&[&st])
        })
        .unwrap();
    assert_eq!(r.value.to_display_string(), "false");
    assert!(
        r.operation_count <= 10,
        "operation_count={} should reflect steps taken, not list length",
        r.operation_count
    );
}

#[test]
fn comprehension_over_huge_range_bounded_lazily() {
    // A huge symbolic range is iterated lazily: the default memory
    // limit trips while the result is being built (honest usage
    // figure), rather than an up-front materialization of ~4.6e18
    // elements or a pre-charge with an astronomical op count.
    let e = ParsedExpression::new("[x for x in range_expr('0-4611686018427387902')]")
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            // 134217824 ~ 128 MiB: the Vec's projected post-doubling
            // capacity, charged before the growth allocation happens.
            "Expression memory usage (134217824 bytes) exceeded limit (100000000 bytes)
",
            "  [x for x in range_expr('0-4611686018427387902')]
",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ]
        .concat()
    );
}

#[test]
fn range_comprehension_charges_iteration_once() {
    // Range comprehensions pre-charge N for the materializing collect and
    // must not charge again per item: total cost matches the +N model
    // that list comprehensions follow (plus constant overhead).
    let range_cost = op_count("[x for x in range_expr('1-100')]");
    let list_cost = op_count("[x for x in [1,2,3,4,5,6,7,8,9,10]]");
    // 100 elements → cost within [100, 110]: N plus small constant, not 2N.
    assert!(
        (100..=110).contains(&range_cost),
        "range comprehension cost {range_cost} should be ~N, not 2N"
    );
    // Sanity: the equivalent list comprehension follows the same model.
    assert!(
        (10..=20).contains(&list_cost),
        "list comprehension cost {list_cost} should be ~N"
    );
}

#[test]
fn same_variant_list_mismatch_at_head_cheap() {
    // A first-element mismatch must cost O(1), not the list length.
    let mut st = SymbolTable::new();
    st.set("A", ExprValue::ListInt((0..1000).collect()))
        .unwrap();
    st.set("B", ExprValue::ListInt((1..1001).collect()))
        .unwrap();
    let r = ParsedExpression::new("A == B")
        .and_then(|p| {
            p.with_memory_limit(usize::MAX)
                .with_operation_limit(100)
                .evaluate_with_metrics(&[&st])
        })
        .unwrap();
    assert_eq!(r.value.to_display_string(), "false");
    assert!(
        r.operation_count <= 10,
        "operation_count={} should reflect comparisons performed",
        r.operation_count
    );
}

#[test]
fn mixed_list_mismatch_at_head_cheap() {
    // A ListInt vs ListFloat comparison goes through the generic
    // (mixed-variant) arm; a first-element mismatch must cost O(1),
    // not the list length.
    let mut st = SymbolTable::new();
    st.set("A", ExprValue::ListInt((0..1000).collect()))
        .unwrap();
    st.set(
        "B",
        ExprValue::ListFloat(
            (0..1000)
                .map(|i| openjd_expr::value::Float64::new(i as f64 + 0.5).unwrap())
                .collect(),
        ),
    )
    .unwrap();
    let r = ParsedExpression::new("A == B")
        .and_then(|p| {
            p.with_memory_limit(usize::MAX)
                .with_operation_limit(100)
                .evaluate_with_metrics(&[&st])
        })
        .unwrap();
    assert_eq!(r.value.to_display_string(), "false");
    assert!(
        r.operation_count <= 10,
        "operation_count={} should reflect comparisons performed",
        r.operation_count
    );
}
