// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_memory.py — memory-bounded evaluation.

use openjd_expr::{ExprValue, ParsedExpression, SymbolTable, DEFAULT_OPERATION_LIMIT};

fn eval(expr: &str) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap()
}

// === TestEvaluateExpressionReturnsExprValue ===
#[test]
fn returns_expr_value() {
    assert_eq!(eval("42").to_display_string(), "42");
}
#[test]
fn has_type() {
    assert_eq!(eval("42").expr_type().to_string(), "int");
}

fn eval_bounded(
    expr: &str,
    mem: usize,
) -> Result<openjd_expr::EvalResult, openjd_expr::ExpressionError> {
    ParsedExpression::new(expr).and_then(|p| {
        p.with_memory_limit(mem)
            .with_operation_limit(DEFAULT_OPERATION_LIMIT)
            .evaluate_with_metrics(&[&SymbolTable::new()])
    })
}
fn eval_peak(expr: &str) -> usize {
    ParsedExpression::new(expr)
        .and_then(|p| {
            p.with_memory_limit(usize::MAX)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[&SymbolTable::new()])
        })
        .unwrap()
        .peak_memory
}
fn eval_peak_with(expr: &str, st: &SymbolTable) -> usize {
    ParsedExpression::new(expr)
        .and_then(|p| {
            p.with_memory_limit(usize::MAX)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[st])
        })
        .unwrap()
        .peak_memory
}

// ══════════════════════════════════════════════════════════════
// TestMemoryLimit
// ══════════════════════════════════════════════════════════════

#[test]
fn string_mul_exceeds_limit() {
    let e = eval_bounded("\"a\" * 10000000", 1000)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Expression memory usage (10000064 bytes) exceeded limit (1000 bytes)\n",
                "  \"a\" * 10000000\n",
                "  ~~~~^~~~~~~~~~",
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn list_mul_exceeds_limit() {
    let e = eval_bounded("[1, 2, 3] * 10000000", 10000)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (10000001) exceeded limit (10000000)\n",
                "  [1, 2, 3] * 10000000\n",
                "  ~~~~~~~~~~^~~~~~~~~~",
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn range_exceeds_limit() {
    let e = eval_bounded("range(10000000)", 1000)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (10000001) exceeded limit (10000000)\n",
                "  range(10000000)\n",
                "  ^~~~~~~~~~~~~~~",
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn range_start_stop_exceeds_limit() {
    let e = eval_bounded("range(0, 10000000)", 1000)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (10000001) exceeded limit (10000000)\n",
                "  range(0, 10000000)\n",
                "  ^~~~~~~~~~~~~~~~~~",
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn range_start_stop_step_exceeds_limit() {
    let e = eval_bounded("range(0, 10000000, 1)", 1000)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Expression operation count (10000001) exceeded limit (10000000)\n",
                "  range(0, 10000000, 1)\n",
                "  ^~~~~~~~~~~~~~~~~~~~~",
            ]
            .concat()
        ),
        "got:\n{e}"
    );
}

#[test]
fn normal_within_limit() {
    assert_eq!(eval("1 + 2 + 3").to_display_string(), "6");
}

#[test]
fn small_string_mul_within_limit() {
    let r = eval_bounded("\"ab\" * 5", 10000).unwrap();
    assert_eq!(r.value.to_display_string(), "ababababab");
}

#[test]
fn small_range_within_limit() {
    let r = eval_bounded("range(5)", 10000).unwrap();
    assert_eq!(r.value.to_display_string(), "[0, 1, 2, 3, 4]");
}

// ══════════════════════════════════════════════════════════════
// TestPeakMemory
// ══════════════════════════════════════════════════════════════

#[test]
fn peak_memory_returned() {
    assert!(eval_peak("1 + 2") > 0);
}

#[test]
fn peak_memory_increases_with_complexity() {
    let simple = eval_peak("1");
    let complex = eval_peak("[1, 2, 3, 4, 5]");
    assert!(complex > simple);
}

#[test]
fn peak_memory_for_string() {
    let short = eval_peak("\"a\"");
    let long = eval_peak("\"a\" * 100");
    assert!(long > short);
}

#[test]
fn intermediate_values_released() {
    // (1+2) + (3+4) should release intermediate results
    let r = ParsedExpression::new("(1 + 2) + (3 + 4)")
        .and_then(|p| {
            p.with_memory_limit(usize::MAX)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[&SymbolTable::new()])
        })
        .unwrap();
    assert_eq!(r.value.to_display_string(), "10");
    assert!(r.peak_memory > 0);
}

#[test]
fn peak_memory_resets_each_call() {
    let mut st = SymbolTable::new();
    st.set("Param.X", ExprValue::String("a".repeat(1000)))
        .unwrap();
    let large = eval_peak_with("Param.X * 100", &st);

    let mut st2 = SymbolTable::new();
    st2.set("Param.X", ExprValue::String("b".to_string()))
        .unwrap();
    let small = eval_peak_with("Param.X * 100", &st2);

    assert!(small < large);
}

// ══════════════════════════════════════════════════════════════
// TestMemoryReleasedInComprehensions
// ══════════════════════════════════════════════════════════════

#[test]
fn nested_comprehension_releases_inner_lists() {
    let single = eval_peak("len([i for i in range(100)])");
    let multi = eval_peak("[len([i for i in range(100)]) for k in range(100)]");
    // Without release, multi would be ~100x single. With release, modestly larger.
    assert!(
        multi < single * 5,
        "multi={multi}, single={single}, ratio={}",
        multi / single.max(1)
    );
}

#[test]
fn deeply_nested_comprehension_bounded_memory() {
    let r = ParsedExpression::new(
        "[len([i for i in [len(range(100)) for j in range(100)]]) for k in range(100)]",
    )
    .and_then(|p| {
        p.with_memory_limit(usize::MAX)
            .with_operation_limit(DEFAULT_OPERATION_LIMIT)
            .evaluate_with_metrics(&[&SymbolTable::new()])
    })
    .unwrap();
    assert!(r.peak_memory < 1_000_000, "peak_memory={}", r.peak_memory);
}

#[test]
fn comprehension_function_call_releases_args() {
    let multi = eval_peak("[len(sorted(range(50))) for i in range(50)]");
    // Result is 50 ints — peak should be bounded, not scaling with iterations
    assert!(multi < 50_000, "multi={multi}");
}

// ── Memory tracking accuracy ──

#[test]
fn peak_memory_int_literal() {
    let peak = eval_peak("50");
    let ev_size = std::mem::size_of::<ExprValue>();
    assert_eq!(
        peak, ev_size,
        "int literal should be one ExprValue, got {peak}"
    );
}

#[test]
fn peak_memory_range_50() {
    let peak = eval_peak("range(50)");
    let ev_size = std::mem::size_of::<ExprValue>();
    assert!(
        peak >= ev_size + 50 * 8,
        "range(50) peak={peak}, expected >= {} (ExprValue + 50 i64s)",
        ev_size + 50 * 8
    );
}

#[test]
fn peak_memory_max_range_50() {
    let peak = eval_peak("max(range(50))");
    let ev_size = std::mem::size_of::<ExprValue>();
    assert!(
        peak >= ev_size + 50 * 8,
        "max(range(50)) peak={peak}, expected >= {}",
        ev_size + 50 * 8
    );
}

#[test]
fn peak_memory_range_concat_list() {
    let peak = eval_peak("range(50) + [1, 2]");
    let ev_size = std::mem::size_of::<ExprValue>();
    assert!(
        peak >= ev_size + 52 * 8,
        "range(50)+[1,2] peak={peak}, expected >= {}",
        ev_size + 52 * 8
    );
}

#[test]
fn peak_memory_range_concat_range() {
    let peak = eval_peak("range(50) + range(50)");
    let ev_size = std::mem::size_of::<ExprValue>();
    assert!(
        peak >= ev_size + 100 * 8,
        "range(50)+range(50) peak={peak}, expected >= {}",
        ev_size + 100 * 8
    );
}
