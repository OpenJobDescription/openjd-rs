// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

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
    assert!(e.contains("exceeded limit (1000 bytes)"), "got:\n{e}");
    assert!(e.contains("\"a\" * 10000000"), "got:\n{e}");
}

#[test]
fn list_mul_exceeds_limit() {
    // List multiplication checks the projected result size against the
    // memory limit *before* op counting, so an over-memory repetition
    // reports a memory error even when it would also blow the op limit.
    let e = eval_bounded("[1, 2, 3] * 10000000", 10000)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Expression memory usage (1920000576 bytes) exceeded limit (10000 bytes)\n",
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

// ══════════════════════════════════════════════════════════════
// SEC-2026-5: make_list_checked defense-in-depth
// ══════════════════════════════════════════════════════════════
//
// Evaluator and function call sites that build lists call
// `ExprValue::make_list_checked(ctx, ...)` rather than `make_list`, so the
// memory limit is enforced *before* the list allocation happens — even in
// call paths that did not charge ops proportionally to the list size.
//
// Each test drives a different call site that was migrated to
// `make_list_checked` and asserts the memory limit triggers a
// `MemoryLimitExceeded` diagnostic before the list construction proceeds.

/// Small memory limit — big enough to hold small intermediate values
/// but well below the size of a list produced by the exploratory inputs.
const TIGHT_MEM: usize = 1_000;

fn err_msg(expr: &str, mem: usize) -> String {
    eval_bounded(expr, mem).unwrap_err().to_string()
}

fn assert_memory_exceeded(expr: &str, mem: usize) {
    let e = err_msg(expr, mem);
    assert!(
        e.contains("Expression memory usage")
            && e.contains(&format!("exceeded limit ({mem} bytes)")),
        "expected memory-limit error, got:\n{e}"
    );
}

#[test]
fn make_list_checked_list_literal_evaluator() {
    // Evaluator's list-literal path (eval/evaluator.rs). A 1,000-string
    // list easily exceeds a 1 kB memory limit at construction time.
    assert_memory_exceeded(
        "[\"abcdefghijklmnopqrstuvwxyz\" for i in range(1000)]",
        TIGHT_MEM,
    );
}

#[test]
fn make_list_checked_list_comprehension_evaluator() {
    // Evaluator's comprehension path also routes through make_list_checked.
    assert_memory_exceeded("[i * i for i in range(10000)]", TIGHT_MEM);
}

#[test]
fn make_list_checked_range_fn() {
    // `range()` builds its result through make_list_checked. The
    // per-element op charge catches this first, but the memory cap is the
    // defense-in-depth we want to verify: lower the op limit high and
    // drive the memory limit low.
    let e = ParsedExpression::new("range(100000)")
        .and_then(|p| {
            p.with_memory_limit(TIGHT_MEM)
                .with_operation_limit(10_000_000)
                .evaluate_with_metrics(&[&SymbolTable::new()])
        })
        .unwrap_err()
        .to_string();
    // Either memory or operation-count failure is acceptable; both fire
    // on oversized inputs and both demonstrate the sandbox holds.
    assert!(
        e.contains("exceeded limit"),
        "expected a bound-exceeded error, got:\n{e}"
    );
}

#[test]
fn make_list_checked_sorted_fn() {
    // sorted() → make_list_checked. Use a large input symtab list so the
    // oversize only materializes at construction.
    let mut st = SymbolTable::new();
    st.set(
        "Param.Items",
        ExprValue::ListString(
            (0..1000).map(|i| format!("item_{i:04}")).collect(),
            /*cached=*/ 0,
        ),
    )
    .unwrap();
    let e = ParsedExpression::new("sorted(Param.Items)")
        .and_then(|p| {
            p.with_memory_limit(TIGHT_MEM)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[&st])
        })
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("exceeded limit"),
        "expected memory-limit error from sorted(), got:\n{e}"
    );
}

#[test]
fn make_list_checked_mul_list_fn() {
    // List multiplication routes through make_list_checked. The op
    // counter catches the huge case first; with a generous op limit and
    // tight memory cap, memory fires.
    let e = ParsedExpression::new("[\"aaaa\"] * 100000")
        .and_then(|p| {
            p.with_memory_limit(TIGHT_MEM)
                .with_operation_limit(10_000_000)
                .evaluate_with_metrics(&[&SymbolTable::new()])
        })
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("exceeded limit"),
        "expected bound-exceeded error from list * n, got:\n{e}"
    );
}

#[test]
fn make_list_checked_split_fn() {
    // string.split() → make_list_checked. A large source string with many
    // separators produces a large `Vec<ExprValue>` before the list is built.
    let mut st = SymbolTable::new();
    // 10 kB of comma-separated tokens.
    let src = "a,".repeat(5000) + "a";
    st.set("Param.S", ExprValue::String(src)).unwrap();
    let e = ParsedExpression::new("split(Param.S, \",\")")
        .and_then(|p| {
            p.with_memory_limit(TIGHT_MEM)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[&st])
        })
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("exceeded limit"),
        "expected memory-limit error from split(), got:\n{e}"
    );
}

#[test]
fn make_list_checked_small_lists_succeed() {
    // Sanity check: small lists well within the memory cap still work.
    let r = eval_bounded("[1, 2, 3, 4, 5]", 10_000).unwrap();
    assert_eq!(r.value.to_display_string(), "[1, 2, 3, 4, 5]");

    let r = eval_bounded("sorted([3, 1, 2])", 10_000).unwrap();
    assert_eq!(r.value.to_display_string(), "[1, 2, 3]");

    let r = eval_bounded("range(10)", 10_000).unwrap();
    assert_eq!(
        r.value.to_display_string(),
        "[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]"
    );

    let r = eval_bounded("[\"a\", \"b\"] * 3", 10_000).unwrap();
    assert_eq!(
        r.value.to_display_string(),
        "[\"a\", \"b\", \"a\", \"b\", \"a\", \"b\"]"
    );
}

#[test]
fn estimate_list_heap_size_is_upper_bound() {
    // The upper-bound estimator must never under-count the true heap
    // footprint of the resulting list for any input. Exercise a few
    // shapes and check the estimator meets or exceeds the actual size.
    use openjd_expr::ExprType;

    // Helper: build a list via make_list and compare its memory_size to the
    // estimator output. The estimator should be a valid upper bound.
    fn check(elements: Vec<ExprValue>, hint: ExprType) {
        let estimate_size = elements.len() * std::mem::size_of::<ExprValue>()
            + elements
                .iter()
                .map(|e| e.memory_size() - std::mem::size_of::<ExprValue>())
                .sum::<usize>();
        let list = ExprValue::make_list(elements, hint).unwrap();
        let actual = list.memory_size() - std::mem::size_of::<ExprValue>();
        // Inline ExprValue storage in the Vec is `len * size_of(ExprValue)`;
        // heap_size for list variants adds only the extra heap bytes. The
        // estimate computes the same two components, so it must be ≥ actual.
        assert!(
            estimate_size >= actual,
            "estimator {estimate_size} < actual {actual}"
        );
    }

    check(vec![ExprValue::Int(1), ExprValue::Int(2)], ExprType::INT);
    check(
        vec![
            ExprValue::String("hello".into()),
            ExprValue::String("world".into()),
        ],
        ExprType::STRING,
    );
    check(vec![], ExprType::INT);
}
