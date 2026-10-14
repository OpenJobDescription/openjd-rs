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
    // 1920000384 = the `[1, 2, 3]` list (charged once; its element
    // literals are released when the list is built) plus the projected
    // 30M-element result.
    let e = eval_bounded("[1, 2, 3] * 10000000", 10000)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            &[
                "Expression memory usage (1920000384 bytes) exceeded limit (10000 bytes)\n",
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
fn re_split_large_result_respects_budgets() {
    // End-to-end: a large re_split fails cleanly under a tight memory cap.
    // Note this cannot distinguish make_list_checked from make_list: the
    // checked constructor only moves the limit check before the transient
    // allocation (track() charges the result either way).
    let mut st = SymbolTable::new();
    st.set("Param.Text", "word ".repeat(10_000)).unwrap();
    let e = ParsedExpression::new("re_split(Param.Text, ' ')")
        .and_then(|p| {
            p.with_memory_limit(TIGHT_MEM)
                .with_operation_limit(10_000_000)
                .evaluate_with_metrics(&[&st])
        })
        .unwrap_err()
        .to_string();
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
fn title_checks_memory_before_char_collect() {
    // title() collects the input into a transient `Vec<char>` whose upper
    // bound is 4 * s.len() bytes. The memory check runs *before* the
    // collect, so a 100 kB input under a 150 kB budget (input fits, the
    // 400 kB char buffer bound does not) errors without allocating it.
    let mut st = SymbolTable::new();
    st.set("Param.S", ExprValue::String("a".repeat(100_000)))
        .unwrap();
    let e = ParsedExpression::new("title(Param.S)")
        .and_then(|p| {
            p.with_memory_limit(150_000)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[&st])
        })
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("Expression memory usage") && e.contains("exceeded limit (150000 bytes)"),
        "expected memory-limit error from title(), got:\n{e}"
    );
}

#[test]
fn capitalize_checks_memory_before_char_collect() {
    // Same transient Vec<char> bound as title() — see above.
    let mut st = SymbolTable::new();
    st.set("Param.S", ExprValue::String("a".repeat(100_000)))
        .unwrap();
    let e = ParsedExpression::new("capitalize(Param.S)")
        .and_then(|p| {
            p.with_memory_limit(150_000)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[&st])
        })
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("Expression memory usage") && e.contains("exceeded limit (150000 bytes)"),
        "expected memory-limit error from capitalize(), got:\n{e}"
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

// === Regression tests: memory limits on range materialization ===

#[test]
fn comprehension_over_range_memory_bounded_incrementally() {
    // Ranges are iterated lazily and the growing result is checked
    // against the memory limit each iteration, so a small limit trips
    // after a few elements — with an honest usage figure — instead of
    // the comprehension materializing 2M elements (~128 MB) first.
    let e = eval_bounded("[x for x in range_expr('1-2000000')]", 10000)
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            // 16480 = logical result bytes (including the element being
            // pushed, charged once by BudgetedVec) plus the Vec's
            // projected post-doubling capacity slack: the check accounts
            // for the buffer the push is about to allocate, not just
            // elements. The iteration's own transients are not in the
            // figure — the footprint is reset to the pre-iteration
            // baseline before the push.
            "Expression memory usage (16480 bytes) exceeded limit (10000 bytes)
",
            "  [x for x in range_expr('1-2000000')]
",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ]
        .concat()
    );
}

#[test]
fn reverse_range_slice_memory_checked_before_walk() {
    // Same for reverse slices: the result size must be pre-checked.
    let e = eval_bounded("range_expr('1-2000000')[::-1]", 10000)
        .unwrap_err()
        .to_string();
    // 128,000,000 = 2,000,000 × size_of::<ExprValue>() (64); the
    // remainder is the tracked RangeExpr and the three slice operands:
    // the `-1` step and the two `Null` placeholders for the omitted
    // start and stop.
    assert_eq!(
        e,
        [
            "Expression memory usage (128000288 bytes) exceeded limit (10000 bytes)\n",
            "  range_expr('1-2000000')[::-1]\n",
            "  ~~~~~~~~~~~~~~~~~~~~~~~^~~~~~",
        ]
        .concat()
    );
}

#[test]
fn list_of_range_memory_checked_before_collect() {
    // list(range_expr(...)) materializes every element: the projected
    // allocation must be pre-checked, not discovered by
    // make_list_checked after ~128 MB is already allocated.
    let e = eval_bounded("list(range_expr('1-2000000'))", 10000)
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            "Expression memory usage (128000000 bytes) exceeded limit (10000 bytes)\n",
            "  list(range_expr('1-2000000'))\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ]
        .concat()
    );
}

#[test]
fn comprehension_over_string_list_no_double_footprint() {
    // List iterables are iterated in place (borrowing ListIter), not
    // collected into a second Vec: a comprehension over a list of large
    // strings must not transiently hold ~2x the list's memory. The list
    // itself is ~1 MB (tracked); peak should stay well under two full
    // copies plus per-iteration transients.
    let mut st = SymbolTable::new();
    let strings: Vec<ExprValue> = (0..100)
        .map(|i| ExprValue::String(format!("{i:0>10000}")))
        .collect();
    st.set(
        "L",
        ExprValue::make_list(strings, openjd_expr::types::ExprType::STRING).unwrap(),
    )
    .unwrap();
    let r = ParsedExpression::new("[len(x) for x in L]")
        .and_then(|p| {
            p.with_memory_limit(usize::MAX)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[&st])
        })
        .unwrap();
    assert_eq!(r.value.to_display_string().matches("10000").count(), 100);
    // One tracked copy (~1 MB) + one transient loop-variable clone
    // (~10 KB) + result ints. Two full copies would be ~2 MB.
    assert!(
        r.peak_memory < 1_500_000,
        "peak_memory={} suggests the list was copied wholesale",
        r.peak_memory
    );
}

#[test]
fn repr_json_preflights_nested_string_output_memory() {
    let inner = ExprValue::make_list(
        (0..32)
            .map(|_| ExprValue::String("\u{1}".repeat(100)))
            .collect(),
        openjd_expr::ExprType::STRING,
    )
    .unwrap();
    let value = ExprValue::make_list(
        vec![inner.clone(), inner],
        openjd_expr::ExprType::list(openjd_expr::ExprType::STRING),
    )
    .unwrap();
    let mut st = SymbolTable::new();
    st.set("Param.Items", value).unwrap();

    let e = ParsedExpression::new("repr_json(Param.Items)")
        .and_then(|p| {
            p.with_memory_limit(10_000)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[&st])
        })
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            "Expression memory usage (38660 bytes) exceeded limit (10000 bytes)\n",
            "  repr_json(Param.Items)\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~",
        ]
        .concat()
    );
}

#[test]
fn multibyte_padding_preflights_output_bytes() {
    // ljust on a 3000-byte / 1000-char string to width 1500 produces 3500
    // output bytes (3000 original + 500 spaces). check_memory(3500) is
    // called before the allocation, so a tight limit trips the preflight.
    let mut st = SymbolTable::new();
    st.set("Param.S", ExprValue::String("界".repeat(1000)))
        .unwrap();
    let expr = "ljust(Param.S, 1500)";
    let e = ParsedExpression::new(expr)
        .and_then(|p| {
            p.with_memory_limit(3_500)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[&st])
        })
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            "Expression memory usage (3564 bytes) exceeded limit (3500 bytes)\n",
            "  ljust(Param.S, 1500)\n",
            "  ^~~~~~~~~~~~~~~~~~~~",
        ]
        .concat()
    );
}

#[test]
fn repr_sh_preflights_expanded_output_memory() {
    let mut st = SymbolTable::new();
    st.set("Param.S", ExprValue::String("\\".repeat(1000)))
        .unwrap();
    let e = ParsedExpression::new("repr_sh(Param.S)")
        .and_then(|p| {
            p.with_memory_limit(3_000)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[&st])
        })
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            "Expression memory usage (6002 bytes) exceeded limit (3000 bytes)\n",
            "  repr_sh(Param.S)\n",
            "  ^~~~~~~~~~~~~~~~",
        ]
        .concat()
    );
}

#[test]
fn replace_preflights_amplified_output_memory() {
    let mut st = SymbolTable::new();
    st.set("Param.S", ExprValue::String("a".repeat(1000)))
        .unwrap();
    st.set("Param.New", ExprValue::String("x".repeat(1000)))
        .unwrap();
    let e = ParsedExpression::new("replace(Param.S, 'a', Param.New)")
        .and_then(|p| {
            p.with_memory_limit(100_000)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[&st])
        })
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            "Expression memory usage (1000000 bytes) exceeded limit (100000 bytes)\n",
            "  replace(Param.S, 'a', Param.New)\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ]
        .concat()
    );
}

#[test]
fn join_preflights_large_separator_output_memory() {
    let mut st = SymbolTable::new();
    st.set(
        "Param.Items",
        ExprValue::ListString(vec!["a".to_string(); 100], 0),
    )
    .unwrap();
    st.set("Param.Sep", ExprValue::String("x".repeat(1000)))
        .unwrap();
    let e = ParsedExpression::new("join(Param.Items, Param.Sep)")
        .and_then(|p| {
            p.with_memory_limit(50_000)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[&st])
        })
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            "Expression memory usage (99100 bytes) exceeded limit (50000 bytes)\n",
            "  join(Param.Items, Param.Sep)\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ]
        .concat()
    );
}

// ══════════════════════════════════════════════════════════════
// Budget errors propagate out of unresolved-test conditionals
// ══════════════════════════════════════════════════════════════

/// With an unresolved test, the evaluator runs both branches and
/// normally absorbs a single failing branch into an `Unresolved`
/// result (run time may select the healthy branch). Budget exceedances
/// are exempt from that absorption: the memory/operations were spent
/// in *this* evaluation no matter which branch run time takes.
#[test]
fn memory_limit_exceeded_in_one_branch_of_unresolved_conditional_propagates() {
    let mut st = SymbolTable::new();
    st.set(
        "Session.Flag",
        ExprValue::unresolved(openjd_expr::ExprType::BOOL),
    )
    .unwrap();
    let err = ParsedExpression::new("'A' * 10000000 if Session.Flag else 'B'")
        .and_then(|p| {
            p.with_memory_limit(1024 * 1024)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[&st])
        })
        .expect_err("the if-branch's budget exceedance must propagate");
    assert!(
        err.message()
            .contains("Expression memory usage (10000136 bytes) exceeded limit (1048576 bytes)"),
        "Got: {}",
        err.message()
    );
}

/// Control: a plain value error in one branch is still absorbed — the
/// resolved test may select the other branch at run time.
#[test]
fn value_error_in_one_branch_of_unresolved_conditional_is_absorbed() {
    let mut st = SymbolTable::new();
    st.set(
        "Session.Flag",
        ExprValue::unresolved(openjd_expr::ExprType::BOOL),
    )
    .unwrap();
    let result = ParsedExpression::new("int('nope') if Session.Flag else 7")
        .and_then(|p| {
            p.with_memory_limit(1024 * 1024)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[&st])
        })
        .expect("a value error in one branch must be absorbed");
    assert!(result.value.is_unresolved());
}

// ══════════════════════════════════════════════════════════════
// Budget errors propagate past unresolved boolop operands
// ══════════════════════════════════════════════════════════════

/// After an unresolved operand, `and`/`or` suppress errors in later
/// operands (a runtime short-circuit could make them unreachable) —
/// but budget exceedances are exempt, exactly as in unresolved-test
/// conditionals: the memory/operations were spent in this evaluation
/// no matter what a runtime short-circuit skips.
#[test]
fn memory_limit_exceeded_after_unresolved_boolop_operand_propagates() {
    let mut st = SymbolTable::new();
    st.set(
        "Session.Flag",
        ExprValue::unresolved(openjd_expr::ExprType::BOOL),
    )
    .unwrap();
    let err = ParsedExpression::new("Session.Flag or len('A' * 10000000) > 0")
        .and_then(|p| {
            p.with_memory_limit(1024 * 1024)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[&st])
        })
        .expect_err("the second operand's budget exceedance must propagate");
    // 10000136 = the 10 MB string plus per-value overhead. The
    // unresolved first operand is released (the result carries only
    // types), so it is not in the figure.
    assert!(
        err.message()
            .contains("Expression memory usage (10000136 bytes) exceeded limit (1048576 bytes)"),
        "Got: {}",
        err.message()
    );
}

/// Control: a plain value error after the unresolved operand is still
/// suppressed — the runtime short-circuit may never evaluate it.
#[test]
fn value_error_after_unresolved_boolop_operand_is_suppressed() {
    let mut st = SymbolTable::new();
    st.set(
        "Session.Flag",
        ExprValue::unresolved(openjd_expr::ExprType::BOOL),
    )
    .unwrap();
    let result = ParsedExpression::new("Session.Flag or int('nope') > 0")
        .and_then(|p| {
            p.with_memory_limit(1024 * 1024)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[&st])
        })
        .expect("a value error after the unresolved operand must be suppressed");
    assert!(result.value.is_unresolved());
}

/// Mirror arm of `memory_limit_exceeded_in_one_branch_of_unresolved_-
/// conditional_propagates`: the budget exceedance in the *else* branch
/// (the evaluator's `(Ok, Err)` arm — the earlier test only exercised
/// `(Err, Ok)`). Without this, deleting that arm's early return leaves
/// the suite green.
#[test]
fn memory_limit_exceeded_in_else_branch_of_unresolved_conditional_propagates() {
    let mut st = SymbolTable::new();
    st.set(
        "Session.Flag",
        ExprValue::unresolved(openjd_expr::ExprType::BOOL),
    )
    .unwrap();
    let err = ParsedExpression::new("'B' if Session.Flag else 'A' * 10000000")
        .and_then(|p| {
            p.with_memory_limit(1024 * 1024)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[&st])
        })
        .expect_err("the else-branch's budget exceedance must propagate");
    assert!(
        err.message()
            .contains("Expression memory usage (10000208 bytes) exceeded limit (1048576 bytes)"),
        "Got: {}",
        err.message()
    );
}

/// `and` shares the suppression arm with `or`; pin it separately.
#[test]
fn memory_limit_exceeded_after_unresolved_and_operand_propagates() {
    let mut st = SymbolTable::new();
    st.set(
        "Session.Flag",
        ExprValue::unresolved(openjd_expr::ExprType::BOOL),
    )
    .unwrap();
    let err = ParsedExpression::new("Session.Flag and 'A' * 10000000 == 'x'")
        .and_then(|p| {
            p.with_memory_limit(1024 * 1024)
                .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                .evaluate_with_metrics(&[&st])
        })
        .expect_err("the second operand's budget exceedance must propagate");
    assert!(
        err.message().contains("exceeded limit (1048576 bytes)"),
        "Got: {}",
        err.message()
    );
}

/// A boolop wrapped around an unresolved-test conditional must not
/// re-absorb the budget error the conditional just propagated — the
/// suppression exemptions compose.
#[test]
fn boolop_does_not_reabsorb_budget_error_from_nested_conditional() {
    let mut st = SymbolTable::new();
    st.set(
        "Session.Flag",
        ExprValue::unresolved(openjd_expr::ExprType::BOOL),
    )
    .unwrap();
    let err =
        ParsedExpression::new("Session.Flag or ('A' * 10000000 if Session.Flag else 'B') == 'x'")
            .and_then(|p| {
                p.with_memory_limit(1024 * 1024)
                    .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                    .evaluate_with_metrics(&[&st])
            })
            .expect_err("the nested conditional's budget exceedance must survive the boolop");
    assert!(
        err.message().contains("exceeded limit (1048576 bytes)"),
        "Got: {}",
        err.message()
    );
}

/// A budget error in the if-branch of an unresolved-test conditional
/// propagates before the else-branch is evaluated, and a boolop
/// suppression site does not absorb it. The else-branch's value error
/// must not appear in the message: it was never evaluated.
#[test]
fn budget_error_in_if_branch_propagates_before_else_branch_runs() {
    let mut st = SymbolTable::new();
    st.set(
        "Session.Flag",
        ExprValue::unresolved(openjd_expr::ExprType::BOOL),
    )
    .unwrap();
    let err = ParsedExpression::new(
        "Session.Flag or ('A' * 10000000 if Session.Flag else int('nope')) == 'x'",
    )
    .and_then(|p| {
        p.with_memory_limit(1024 * 1024)
            .with_operation_limit(DEFAULT_OPERATION_LIMIT)
            .evaluate_with_metrics(&[&st])
    })
    .expect_err("the if-branch's budget exceedance must propagate through the boolop");
    let msg = err.message();
    assert!(
        msg.starts_with("Expression memory usage (10000136 bytes) exceeded limit (1048576 bytes)"),
        "Got: {msg}"
    );
    assert!(
        !msg.contains("Both branches fail"),
        "the else-branch must not have run: {msg}"
    );
    assert!(
        !msg.contains("Cannot convert 'nope'"),
        "the else-branch must not have run: {msg}"
    );
}

/// Control: a compound both-branches-fail error with *no* budget
/// exceedance inside is still suppressed by the boolop.
#[test]
fn compound_both_branches_fail_without_budget_error_is_suppressed_by_boolop() {
    let mut st = SymbolTable::new();
    st.set(
        "Session.Flag",
        ExprValue::unresolved(openjd_expr::ExprType::BOOL),
    )
    .unwrap();
    let result =
        ParsedExpression::new("Session.Flag or (int('a') if Session.Flag else int('b')) == 7")
            .and_then(|p| {
                p.with_memory_limit(1024 * 1024)
                    .with_operation_limit(DEFAULT_OPERATION_LIMIT)
                    .evaluate_with_metrics(&[&st])
            })
            .expect("a compound value error must be suppressed");
    assert!(result.value.is_unresolved());
}

// ══════════════════════════════════════════════════════════════
// A failing comprehension's spend is absorbed into the parent
// ══════════════════════════════════════════════════════════════

/// A comprehension iteration that fails has still spent the memory and
/// operations its filter/body consumed before failing. When an
/// unresolved-test conditional absorbs the failure and continues, the
/// parent's counters must include that spend — otherwise every absorbed
/// comprehension failure evaluates under-metered. Observable through
/// `peak_memory`: the failing iteration builds a 1 MB string before
/// erroring, and that high-water mark must survive the failure.
#[test]
fn failing_comprehension_spend_is_absorbed_by_enclosing_conditional() {
    let mut st = SymbolTable::new();
    st.set(
        "Session.Flag",
        ExprValue::unresolved(openjd_expr::ExprType::BOOL),
    )
    .unwrap();
    // The body builds a 1 MB string and then fails converting it to int;
    // the conditional absorbs the value error (Session.Flag might select
    // the else branch at run time).
    let result = ParsedExpression::new("[int('A' * 1000000) for x in [1]] if Session.Flag else []")
        .and_then(|p| p.evaluate_with_metrics(&[&st]))
        .expect("the conditional absorbs the if-branch's value error");
    assert!(result.value.is_unresolved());
    assert!(
        result.peak_memory >= 1_000_000,
        "the failed iteration's 1 MB allocation must be reflected in peak memory; got {}",
        result.peak_memory
    );
}

/// Shared shape for the four other absorption sites: an enclosing
/// unresolved-test conditional swallows the comprehension's value
/// error, and the 1 MB the failing site allocated before erroring must
/// survive into the parent's high-water mark.
fn assert_absorbed_spend(expr: &str) {
    let mut st = SymbolTable::new();
    st.set(
        "Session.Flag",
        ExprValue::unresolved(openjd_expr::ExprType::BOOL),
    )
    .unwrap();
    st.set(
        "Session.List",
        ExprValue::unresolved(openjd_expr::ExprType::list(openjd_expr::ExprType::INT)),
    )
    .unwrap();
    let result = ParsedExpression::new(expr)
        .and_then(|p| p.evaluate_with_metrics(&[&st]))
        .unwrap_or_else(|e| panic!("the conditional must absorb the value error for {expr}: {e}"));
    assert!(result.value.is_unresolved());
    assert!(
        result.peak_memory >= 1_000_000,
        "{expr}: the failed site's 1 MB allocation must be reflected in peak memory; got {}",
        result.peak_memory
    );
}

/// Concrete loop, filter clause errors.
#[test]
fn failing_comprehension_filter_spend_is_absorbed() {
    assert_absorbed_spend("[x for x in [1] if int('A' * 1000000) > 0] if Session.Flag else []");
}

/// Concrete loop, filter evaluates to a non-boolean (the type-error
/// arm) after spending.
#[test]
fn nonbool_comprehension_filter_spend_is_absorbed() {
    assert_absorbed_spend("[x for x in [1] if 'A' * 1000000] if Session.Flag else []");
}

/// Unresolved-iterable path, filter clause errors.
#[test]
fn failing_unresolved_comprehension_filter_spend_is_absorbed() {
    assert_absorbed_spend(
        "[x for x in Session.List if int('A' * 1000000) > 0] if Session.Flag else []",
    );
}

/// Unresolved-iterable path, body errors.
#[test]
fn failing_unresolved_comprehension_body_spend_is_absorbed() {
    assert_absorbed_spend("[int('A' * 1000000) for x in Session.List] if Session.Flag else []");
}

// ══════════════════════════════════════════════════════════════
// An absorbed comprehension failure leaves no live footprint behind
// ══════════════════════════════════════════════════════════════

/// Absorbing a failing iteration's *spend* (peak, ops) must not also
/// carry its *live footprint* forward. The child's tracked values — the
/// loop-variable clone and the failed sub-expression's intermediates —
/// are dropped with it, and the enclosing construct that absorbs the
/// error keeps evaluating on this evaluator. `'A' * 1000000 - 1` fails
/// in the binop with the 1 MB string still tracked (dispatch releases
/// inputs only on success), so a stale footprint would charge the
/// later 600 KB allocation as 1.6 MB against a 1.5 MB limit.
fn assert_no_stale_footprint(expr: &str) {
    let mut st = SymbolTable::new();
    st.set(
        "Session.Flag",
        ExprValue::unresolved(openjd_expr::ExprType::BOOL),
    )
    .unwrap();
    st.set(
        "Session.List",
        ExprValue::unresolved(openjd_expr::ExprType::list(openjd_expr::ExprType::INT)),
    )
    .unwrap();
    let result = ParsedExpression::new(expr)
        .and_then(|p| p.with_memory_limit(1_500_000).evaluate_with_metrics(&[&st]))
        .unwrap_or_else(|e| panic!("{expr}: the dropped footprint must not be charged: {e}"));
    // Peak still reflects the failed iteration's genuine 1 MB spend.
    assert!(
        result.peak_memory >= 1_000_000,
        "{expr}: peak must include the failed iteration's spend; got {}",
        result.peak_memory
    );
}

#[test]
fn absorbed_comprehension_body_failure_leaves_no_stale_footprint() {
    assert_no_stale_footprint(
        "len(['A' * 1000000 - 1 for x in [1]] if Session.Flag else []) + len('B' * 600000)",
    );
}

#[test]
fn absorbed_comprehension_filter_failure_leaves_no_stale_footprint() {
    assert_no_stale_footprint(
        "len([x for x in [1] if 'A' * 1000000 - 1] if Session.Flag else []) + len('B' * 600000)",
    );
}

#[test]
fn absorbed_unresolved_comprehension_body_failure_leaves_no_stale_footprint() {
    assert_no_stale_footprint(
        "len(['A' * 1000000 - 1 for x in Session.List] if Session.Flag else []) + len('B' * 600000)",
    );
}

#[test]
fn absorbed_unresolved_comprehension_filter_failure_leaves_no_stale_footprint() {
    assert_no_stale_footprint(
        "len([x for x in Session.List if 'A' * 1000000 - 1] if Session.Flag else []) + len('B' * 600000)",
    );
}

// ══════════════════════════════════════════════════════════════
// The push pre-check charges each element once
// ══════════════════════════════════════════════════════════════

/// The element a comprehension is about to push is accounted by
/// BudgetedVec's pre-check. It must not *also* sit in the parent's
/// live footprint through the child's tracked result — that double-
/// charge would shrink the effective memory limit by one element per
/// push. Five 100 KB elements plus Vec slack fit comfortably under
/// 560 KB; a double-charge needs ~600 KB and fails.
#[test]
fn comprehension_push_precheck_charges_each_element_once() {
    let st = SymbolTable::new();
    let r = ParsedExpression::new("['A' * 100000 for x in range(5)]")
        .and_then(|p| p.with_memory_limit(560_000).evaluate_with_metrics(&[&st]))
        .expect("five 100 KB elements must fit under a 560 KB limit");
    assert_eq!(
        r.value.expr_type(),
        openjd_expr::ExprType::list(openjd_expr::ExprType::STRING)
    );
}

/// The reported `used` for a large-element exceedance is the honest
/// figure: at the third push, two 600 KB elements are held plus the
/// third being pushed plus the Vec's projected slack — ~1.8 MB. A
/// double-charge of the pushed element would have reported the same
/// ~1.8 MB one push *earlier*, at the second element, when only ~1.2 MB
/// was live (pinned here indirectly: the two-element variant fits).
#[test]
fn large_element_comprehension_exceedance_reports_true_usage() {
    let st = SymbolTable::new();
    ParsedExpression::new("['A' * 600000 for x in [1, 2]]")
        .and_then(|p| p.with_memory_limit(1_500_000).evaluate(&[&st]))
        .expect("two 600 KB elements must fit under a 1.5 MB limit");
    let e = ParsedExpression::new("['A' * 600000 for x in [1, 2, 3]]")
        .and_then(|p| p.with_memory_limit(1_500_000).evaluate(&[&st]))
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            // 1800576 = the `[1, 2, 3]` iterable (charged once) plus two
            // held 600 KB elements, the third being pushed, and the Vec's
            // projected slack.
            "Expression memory usage (1800576 bytes) exceeded limit (1500000 bytes)\n",
            "  ['A' * 600000 for x in [1, 2, 3]]\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ]
        .concat()
    );
}

// ══════════════════════════════════════════════════════════════
// An abandoned comprehension's iterable is not charged afterwards
// ══════════════════════════════════════════════════════════════

/// The iterable is tracked before the loop and, on the success path,
/// released after it ("consumed by the comprehension"). An error exit
/// that an enclosing construct absorbs must drop it too: the
/// comprehension is abandoned and nothing references the iterable, so
/// leaving its 600 KB in `current_memory` would charge every later
/// allocation for it — here the 600 KB `'B'` string, which then reads
/// as 1.2 MB against a 1 MB limit. The failures are chosen to be cheap
/// (`int()` releases its argument before dispatch; the type errors
/// allocate nothing) so the iterable's residue is what is observed,
/// not a body-time budget exceedance. The large list comes from the
/// symbol table: a list *literal* is charged for both its element and
/// itself while being built — a separate, pre-existing `eval_list`
/// accounting matter — which would trip the limit before the
/// comprehension runs.
fn assert_iterable_not_charged_after_abandonment(expr: &str) {
    let mut st = SymbolTable::new();
    st.set(
        "Session.Flag",
        ExprValue::unresolved(openjd_expr::ExprType::BOOL),
    )
    .unwrap();
    st.set(
        "Big",
        ExprValue::make_list(
            vec![ExprValue::String("C".repeat(600_000))],
            openjd_expr::ExprType::STRING,
        )
        .unwrap(),
    )
    .unwrap();
    ParsedExpression::new(expr)
        .and_then(|p| p.with_memory_limit(1_000_000).evaluate(&[&st]))
        .unwrap_or_else(|e| panic!("{expr}: the abandoned iterable must not be charged: {e}"));
}

/// Body error over a large iterable.
#[test]
fn abandoned_comprehension_body_error_drops_iterable_charge() {
    assert_iterable_not_charged_after_abandonment(
        "len([int('nope') for x in Big] if Session.Flag else []) + len('B' * 600000)",
    );
}

/// Filter error over a large iterable.
#[test]
fn abandoned_comprehension_filter_error_drops_iterable_charge() {
    assert_iterable_not_charged_after_abandonment(
        "len([x for x in Big if int('nope') > 0] if Session.Flag else []) + len('B' * 600000)",
    );
}

/// Non-boolean filter over a large iterable.
#[test]
fn abandoned_comprehension_nonbool_filter_drops_iterable_charge() {
    assert_iterable_not_charged_after_abandonment(
        "len([x for x in Big if 1] if Session.Flag else []) + len('B' * 600000)",
    );
}

/// "Cannot iterate" type error: the iterable itself is the large value
/// (a string is tracked once, so a literal is fine here).
#[test]
fn abandoned_comprehension_cannot_iterate_drops_iterable_charge() {
    assert_iterable_not_charged_after_abandonment(
        "len([x for x in 'C' * 600000] if Session.Flag else []) + len('B' * 600000)",
    );
}

// ══════════════════════════════════════════════════════════════
// Absorbed failures and discarded values leave no footprint
// ══════════════════════════════════════════════════════════════

/// Evaluate `expr` under a 1.5 MB limit. The expression absorbs a
/// failure or discards a value, then allocates 600 KB. If the discarded
/// value were still charged, the final allocation would exceed the
/// limit.
fn assert_fits_after_absorption(expr: &str) {
    let mut st = SymbolTable::new();
    st.set(
        "Session.Flag",
        ExprValue::unresolved(openjd_expr::ExprType::BOOL),
    )
    .unwrap();
    ParsedExpression::new(expr)
        .and_then(|p| p.with_memory_limit(1_500_000).evaluate(&[&st]))
        .unwrap_or_else(|e| panic!("{expr}: a discarded value must not stay charged: {e}"));
}

/// When one branch of an unresolved-test conditional fails, the other
/// branch's value is discarded (the result is an `Unresolved` carrying
/// only its type) and must be released. Both arms.
#[test]
fn ifexp_absorbing_if_branch_failure_releases_else_value() {
    assert_fits_after_absorption(
        "len(int('x') if Session.Flag else 'A' * 1000000) + len('B' * 600000)",
    );
}

#[test]
fn ifexp_absorbing_else_branch_failure_releases_if_value() {
    assert_fits_after_absorption(
        "len('A' * 1000000 if Session.Flag else int('x')) + len('B' * 600000)",
    );
}

/// When both branches succeed, both values are discarded for the union
/// type. Both are live together while the union is formed (2 MB peak),
/// which the limit allows; the trailing 1.5 MB allocation fits only if
/// both were released afterwards.
#[test]
fn ifexp_with_unresolved_test_releases_both_branch_values() {
    let mut st = SymbolTable::new();
    st.set(
        "Session.Flag",
        ExprValue::unresolved(openjd_expr::ExprType::BOOL),
    )
    .unwrap();
    let expr = "len('A' * 1000000 if Session.Flag else 'C' * 1000000) + len('B' * 1500000)";
    ParsedExpression::new(expr)
        .and_then(|p| p.with_memory_limit(2_500_000).evaluate(&[&st]))
        .unwrap_or_else(|e| panic!("{expr}: discarded branch values must not stay charged: {e}"));
}

/// `'A' * 1000000 - 1` fails inside the binop while the 1 MB string is a
/// live operand. Absorbed by a boolop after an unresolved operand, the
/// string must be released.
#[test]
fn boolop_past_unresolved_absorbing_failed_binop_releases_its_operands() {
    assert_fits_after_absorption(
        "[Session.Flag or 'A' * 1000000 - 1 == 'x', len('B' * 600000) > 0][1]",
    );
}

/// The same failed binop, absorbed by a conditional instead of a boolop.
#[test]
fn ifexp_absorbing_failed_binop_releases_its_operands() {
    assert_fits_after_absorption(
        "len(('A' * 1000000 - 1) if Session.Flag else '') + len('B' * 600000)",
    );
}

/// A concrete boolop operand that does not decide the result is
/// replaced by the next one and must be released, both before and after
/// the unresolved operand. The operands are the large strings themselves
/// (a non-empty string is truthy, so `and` moves past it).
#[test]
fn boolop_releases_non_deciding_concrete_operands() {
    assert_fits_after_absorption(
        "[('A' * 1000000 and Session.Flag and 'C' * 1000000), len('B' * 600000) > 0][1]",
    );
}

// ══════════════════════════════════════════════════════════════
// A list literal is charged once
// ══════════════════════════════════════════════════════════════

/// A list literal's elements are released when they are consumed into
/// the list, so the literal is charged for the list only, not for the
/// list and every element again. One 600 KB element plus a further
/// 300 KB fits under a 1 MB limit.
#[test]
fn list_literal_elements_are_charged_once() {
    let st = SymbolTable::new();
    ParsedExpression::new("len(['C' * 600000]) + len('B' * 300000)")
        .and_then(|p| p.with_memory_limit(1_000_000).evaluate(&[&st]))
        .expect("a 600 KB single-element literal plus 300 KB must fit under 1 MB");
    // With a second large literal following, the limit is exceeded while
    // the second element is being produced, before the second list exists
    // and before `+` runs (hence the caret on `'B' * 600000`). The figure
    // is one list charge plus one element in flight.
    let e = ParsedExpression::new("['C' * 600000] + ['B' * 600000]")
        .and_then(|p| p.with_memory_limit(1_000_000).evaluate(&[&st]))
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            // 1200224 = the 600 KB single-element list (600088) plus the
            // second 600 KB string as it is produced (600064), its 'B'
            // operand (72) and the operator's fixed overhead.
            "Expression memory usage (1200224 bytes) exceeded limit (1000000 bytes)\n",
            "  ['C' * 600000] + ['B' * 600000]\n",
            "                    ~~~~^~~~~~~~",
        ]
        .concat()
    );
}

// ══════════════════════════════════════════════════════════════
// A comparison releases its operands exactly once
// ══════════════════════════════════════════════════════════════

/// A comparison's operands are released once, by the dispatch that
/// consumes them. With a 1 MB string held, a comparison of two 200 KB
/// strings, and a further 700 KB allocation, the live footprint is
/// 1.7 MB and must exceed a 1.5 MB limit. If the comparison released
/// its operands twice, 400 KB of the held string would be uncharged
/// and the expression would fit.
#[test]
fn comparison_releases_operands_once() {
    let st = SymbolTable::new();
    let e = ParsedExpression::new(
        "['A' * 1000000, string('C' * 200000 == 'D' * 200000), 'B' * 700000]",
    )
    .and_then(|p| p.with_memory_limit(1_500_000).evaluate(&[&st]))
    .unwrap_err()
    .to_string();
    assert_eq!(
        e,
        [
            // 1700272 = the held 1 MB string (1000064), the "false" result
            // string (72), the 700 KB string being produced (700064) and
            // its 'B' operand (72). The comparison's operands are gone.
            "Expression memory usage (1700272 bytes) exceeded limit (1500000 bytes)\n",
            "  ['A' * 1000000, string('C' * 200000 == 'D' * 200000), 'B' * 700000]\n",
            "                                                        ~~~~^~~~~~~~",
        ]
        .concat()
    );
}

/// A chained comparison carries its middle operand from one link to the
/// next. The carried value is charged once across the chain and released
/// by the link that consumes it. Three 300 KB operands, two links, then
/// a 1.45 MB allocation under a 1.5 MB limit: it fits only if the chain
/// left nothing charged. This guards against a leak; the test above
/// guards against a double release.
#[test]
fn chained_comparison_carries_middle_operand_once() {
    let st = SymbolTable::new();
    ParsedExpression::new("[string('A' * 300000 < 'B' * 300000 < 'C' * 300000), 'D' * 1450000]")
        .and_then(|p| p.with_memory_limit(1_500_000).evaluate(&[&st]))
        .expect("the chain must leave no footprint");
}

// ══════════════════════════════════════════════════════════════
// A coerced value is tracked at its coerced size
// ══════════════════════════════════════════════════════════════

/// Target-type coercion runs after a node's value is tracked and can
/// change its size. The evaluator releases the original and tracks the
/// coerced value, so the memory limit applies to what the expression
/// actually produces. A `range_expr` is a few dozen bytes; coerced to
/// `list[int]` it becomes 100,000 ints (800 KB), which exceeds a 100 KB
/// limit. Previously only the small `range_expr` was ever tracked.
#[test]
fn root_target_coercion_is_charged_at_coerced_size() {
    let st = SymbolTable::new();
    let target = openjd_expr::ExprType::list(openjd_expr::ExprType::INT);
    let e = ParsedExpression::new("range_expr('1-100000')")
        .and_then(|p| {
            p.with_memory_limit(100_000)
                .with_target_type(&target)
                .evaluate(&[&st])
        })
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            // 800064 = the coerced list[int] alone (64 + 100,000 × 8). The
            // range_expr it replaced was released first.
            "Expression memory usage (800064 bytes) exceeded limit (100000 bytes)\n",
            "  range_expr('1-100000')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~",
        ]
        .concat()
    );
}

/// `eval_call` releases its arguments at their coerced size. If they had
/// been tracked at the pre-coercion size, the release would subtract more
/// than was added, and part of some other live value would go uncounted.
/// Under a `list[string]` target each element is evaluated toward
/// `string`, so `join` receives argument targets and `range(2000)` (16 KB
/// as `list[int]`) is coerced to `list[string]` (about 55 KB). A 1 MB
/// string is live at the same time; a further 500 KB then totals 1.51 MB
/// and fails a 1.5 MB limit. With the old under-count, 39 KB of the 1 MB
/// string went uncounted and the expression fit.
#[test]
fn call_argument_coercion_releases_what_was_charged() {
    let st = SymbolTable::new();
    let target = openjd_expr::ExprType::list(openjd_expr::ExprType::STRING);
    let e = ParsedExpression::new("['A' * 1000000, join(range(2000), ','), 'B' * 500000]")
        .and_then(|p| {
            p.with_memory_limit(1_500_000)
                .with_target_type(&target)
                .evaluate(&[&st])
        })
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            // 1509153 = the 1 MB string (1000064), the joined string
            // (8953: 6890 digits and 1999 commas), the 'B' operand (72),
            // the `500000` operand (64), and the 500,000 bytes the
            // multiplication checks for its result before building it.
            // The coerced argument has been released.
            "Expression memory usage (1509153 bytes) exceeded limit (1500000 bytes)\n",
            "  ['A' * 1000000, join(range(2000), ','), 'B' * 500000]\n",
            "                                          ~~~~^~~~~~~~",
        ]
        .concat()
    );
}

// ══════════════════════════════════════════════════════════════
// Attribute access never rewrites a budget error
// ══════════════════════════════════════════════════════════════

/// Symbol table with an unresolved `Session.Flag`, a `Big` path whose
/// string is 1 MB long (tracking it exceeds any limit below 1 MB), and a
/// `Deep` path of 200,000 components (`.parts` builds a list far past
/// the limits used here).
fn attribute_symtab() -> SymbolTable {
    let mut st = SymbolTable::new();
    st.set(
        "Session.Flag",
        ExprValue::unresolved(openjd_expr::ExprType::BOOL),
    )
    .unwrap();
    st.set(
        "Big",
        ExprValue::new_path(
            format!("/{}", "a".repeat(1_000_000)),
            openjd_expr::PathFormat::Posix,
        ),
    )
    .unwrap();
    st.set(
        "Deep",
        ExprValue::new_path("/a".repeat(200_000), openjd_expr::PathFormat::Posix),
    )
    .unwrap();
    st
}

fn eval_attribute_expr(expr: &str, mem: usize) -> Result<ExprValue, openjd_expr::ExpressionError> {
    let st = attribute_symtab();
    ParsedExpression::new(expr).and_then(|p| {
        p.with_memory_limit(mem)
            .with_path_format(openjd_expr::PathFormat::Posix)
            .evaluate(&[&st])
    })
}

/// `Big.name` is not a symbol, so the evaluator falls back to evaluating
/// `Big` and dispatching the property. Tracking the 1 MB path exceeds
/// the limit inside that base evaluation. The fallback that would
/// otherwise report "Undefined variable 'Big.name'" must let the budget
/// error through.
#[test]
fn attribute_base_lookup_propagates_memory_error() {
    let e = eval_attribute_expr("Big.name", 500_000)
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            "Expression memory usage (1000065 bytes) exceeded limit (500000 bytes)\n",
            "  Big.name\n",
            "  ^~~",
        ]
        .concat()
    );
}

/// The same base-lookup failure inside a construct that absorbs value
/// errors. If the budget error were rewritten as an undefined-variable
/// error, the `or` would absorb it and the expression would succeed
/// with an `Unresolved` result.
#[test]
fn attribute_base_lookup_memory_error_is_not_absorbed() {
    let e = eval_attribute_expr("Session.Flag or Big.name == 'x'", 500_000)
        .unwrap_err()
        .to_string();
    assert!(
        e.starts_with("Expression memory usage (1000065 bytes) exceeded limit (500000 bytes)"),
        "expected the memory error to propagate, got:\n{e}"
    );
}

/// `Deep.parts` fails inside the property dispatch: the 200,001-element
/// result list fails the memory check before it is built. The fallback
/// that would otherwise report "'parts' property is not available for
/// path" must let the budget error through.
#[test]
fn attribute_property_dispatch_propagates_memory_error() {
    let e = eval_attribute_expr("Deep.parts", 2_000_000)
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            // 13400129 = the 400 KB path (400064) plus the estimate for
            // the 200,001 one-character parts (the root "/" and 200,000
            // "a"s): 200,001 × 64 slots plus 200,001 bytes of heap.
            "Expression memory usage (13400129 bytes) exceeded limit (2000000 bytes)\n",
            "  Deep.parts\n",
            "  ~~~~~^~~~~",
        ]
        .concat()
    );
}

/// The same property-dispatch failure inside an absorbing construct.
#[test]
fn attribute_property_dispatch_memory_error_is_not_absorbed() {
    let e = eval_attribute_expr("Session.Flag or len(Deep.parts) == 0", 2_000_000)
        .unwrap_err()
        .to_string();
    assert!(
        e.starts_with("Expression memory usage ("),
        "expected the memory error to propagate, got:\n{e}"
    );
}

/// Control: a real value error in the property dispatch is still
/// rewritten to the friendlier message, and can still be absorbed.
#[test]
fn attribute_value_error_is_still_rewritten_and_absorbable() {
    let e = eval_attribute_expr("'abc'.name", 1_000_000)
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            "'name' property is not available for string. Available for: path\n",
            "  'abc'.name\n",
            "  ~~~~~~^~~~",
        ]
        .concat()
    );
    let v = eval_attribute_expr("Session.Flag or 'abc'.name == 'x'", 1_000_000).unwrap();
    assert!(v.is_unresolved());
}

/// `eval_call` has its own rewrite: a failed method call whose name is
/// also a property becomes "'name' is a property, not a method". No
/// method in the default library shares a name with a property, so this
/// needs a custom library. Here the default library is extended with a
/// `stem(path, int)` method that exhausts the operation budget. The
/// budget error must not be rewritten, so it must not be absorbable.
#[test]
fn call_property_method_rewrite_exempts_budget_errors() {
    use openjd_expr::function_library::{EvalContext, FunctionLibrary};
    fn exhausting_stem(
        ctx: &mut dyn EvalContext,
        _args: &[ExprValue],
    ) -> Result<ExprValue, openjd_expr::ExpressionError> {
        ctx.count_ops(usize::MAX / 2)?;
        Ok(ExprValue::String(String::new()))
    }
    let mut lib: FunctionLibrary =
        (*FunctionLibrary::for_profile(&openjd_expr::ExprProfile::current())).clone();
    lib.register_sig("stem", "(path, int) -> string", exhausting_stem)
        .unwrap();
    let mut st = SymbolTable::new();
    st.set(
        "Session.Flag",
        ExprValue::unresolved(openjd_expr::ExprType::BOOL),
    )
    .unwrap();
    st.set(
        "P",
        ExprValue::new_path("/a/b.txt", openjd_expr::PathFormat::Posix),
    )
    .unwrap();
    let run = |expr: &str| {
        ParsedExpression::new(expr).and_then(|p| {
            p.with_library(&lib)
                .with_path_format(openjd_expr::PathFormat::Posix)
                .evaluate(&[&st])
        })
    };

    // Top level: the budget error is reported as itself.
    let e = run("P.stem(1)").unwrap_err().to_string();
    assert!(
        e.starts_with("Expression operation count ("),
        "expected the operation-limit error to propagate, got:\n{e}"
    );
    // Inside a construct that absorbs value errors: not absorbed.
    let e = run("Session.Flag or P.stem(1) == 'x'")
        .unwrap_err()
        .to_string();
    assert!(
        e.starts_with("Expression operation count ("),
        "expected the operation-limit error to propagate, got:\n{e}"
    );
    // Control: a real value error still gets the rewrite.
    let e = run("P.stem()").unwrap_err().to_string();
    assert_eq!(
        e,
        [
            "'stem' is a property, not a method. Use .stem instead of .stem()\n",
            "  P.stem()\n",
            "  ~~^~~~~~",
        ]
        .concat()
    );
}

// ══════════════════════════════════════════════════════════════
// Slice operands are tracked and released symmetrically
// ══════════════════════════════════════════════════════════════

/// An omitted slice bound is passed to dispatch as a `Null` placeholder,
/// and dispatch releases every operand. The placeholders are tracked when
/// created so the release matches. This is only visible when something
/// else is live: with a 300 KB string live, `[:]` on a 500 KB string, and
/// then a 700 KB string, the reported figure is exact. Untracked
/// placeholders would have subtracted 192 bytes (three omitted bounds)
/// that were never added.
#[test]
fn slice_placeholders_are_charged_before_dispatch_releases_them() {
    let st = SymbolTable::new();
    let e = ParsedExpression::new("['C' * 300000, ('A' * 500000)[:], 'B' * 700000]")
        .and_then(|p| p.with_memory_limit(1_500_000).evaluate(&[&st]))
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            // 1500264 = the 300 KB string (300064), the sliced 500 KB
            // result (500064; the slice shrinks its buffer, so capacity
            // equals length), the 'B' operand (72), the `700000` operand
            // (64), and the 700,000 bytes the multiplication checks for
            // its result before building it.
            "Expression memory usage (1500264 bytes) exceeded limit (1500000 bytes)\n",
            "  ['C' * 300000, ('A' * 500000)[:], 'B' * 700000]\n",
            "                                    ~~~~^~~~~~~~",
        ]
        .concat()
    );
}

/// A string slice checks the memory budget for its result before
/// allocating anything. The input is still tracked at that point, so the
/// input and the projected result count together. A 1 MB string sliced
/// whole under a 1.5 MB limit fails at the slice, with the input, the
/// three placeholders, and the projected result in the figure.
/// Previously the slice built an untracked `Vec<char>` (4 MB) and index
/// vector (8 MB), then a result whose capacity had grown to 1048576, and
/// the expression passed because the input was released before the
/// result was tracked.
#[test]
fn string_slice_budgets_result_before_allocating() {
    let st = SymbolTable::new();
    let e = ParsedExpression::new("('A' * 1000000)[:]")
        .and_then(|p| p.with_memory_limit(1_500_000).evaluate(&[&st]))
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            // 2000256 = the 1 MB input (1000064), three `Null` placeholders
            // (192) and the projected 1,000,000-byte result.
            "Expression memory usage (2000256 bytes) exceeded limit (1500000 bytes)\n",
            "  ('A' * 1000000)[:]\n",
            "  ~~~~~~~~~~~~~~^~~~",
        ]
        .concat()
    );
    // The bound also depends on the number of selected characters: a
    // step-8 slice selects 125,000 characters and projects 500,000 bytes
    // (four per character), which fits.
    let v = ParsedExpression::new("len(('A' * 1000000)[::8])")
        .and_then(|p| p.with_memory_limit(1_600_000).evaluate(&[&st]))
        .expect("an eighth-size slice must fit where the whole does not");
    assert_eq!(v, ExprValue::Int(125000));
}

/// A list slice reserves its result's slots once, checked against the
/// budget, and charges each element as it is pushed. With the input list
/// still tracked (it is released by dispatch afterwards), a whole-list
/// slice of 50,000 ints under a limit that fits the input but not two
/// copies fails at the slice, and the figure is the input plus the
/// reservation. Previously the result grew by doubling with no check
/// until it was complete, and an index vector was allocated alongside.
#[test]
fn list_slice_reserves_result_before_building() {
    let mut st = SymbolTable::new();
    st.set("L", ExprValue::ListInt((0..50_000).collect()))
        .unwrap();
    let e = ParsedExpression::new("L[:]")
        .and_then(|p| p.with_memory_limit(3_500_000).evaluate(&[&st]))
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            // 3600256 = the 50,000-int input list (64 + 50,000 × 8 =
            // 400064), three `Null` placeholders (192), and the reserved
            // 50,000 result slots (50,000 × 64 = 3,200,000; the result
            // holds ExprValues until make_list_checked packs them). The
            // reservation alone is under the limit; the input still being
            // tracked pushes the total over.
            "Expression memory usage (3600256 bytes) exceeded limit (3500000 bytes)\n",
            "  L[:]\n",
            "  ~^~~",
        ]
        .concat()
    );
    // Elements are charged as they are pushed, so a slice of large
    // strings fails partway through, at the push that crosses the limit,
    // rather than after every element has been cloned. 100 strings of
    // 100 KB under a 12 MB limit: the input (about 10 MB) and the 100
    // reserved slots fit; the 20th clone does not.
    let mut st = SymbolTable::new();
    st.set(
        "S",
        ExprValue::make_list(
            (0..100)
                .map(|_| ExprValue::String("x".repeat(100_000)))
                .collect(),
            openjd_expr::ExprType::STRING,
        )
        .unwrap(),
    )
    .unwrap();
    let e = ParsedExpression::new("S[:]")
        .and_then(|p| p.with_memory_limit(12_000_000).evaluate(&[&st]))
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        [
            // 12009056 = the input list (64 + 100 × 24 for the String
            // headers + 100 × 100,000 = 10,002,464), three placeholders
            // (192), and 20 pushed clones (20 × 100,064 = 2,001,280) with
            // the remaining 80 reserved slots (80 × 64 = 5,120) still
            // counted. Before per-push charging, all 100 clones were built
            // first and the figure was the input plus the whole result.
            "Expression memory usage (12009056 bytes) exceeded limit (12000000 bytes)\n",
            "  S[:]\n",
            "  ~^~~",
        ]
        .concat()
    );
}

/// When a slice bound is unresolved the result is a type-only
/// `Unresolved`, and the sliced value is discarded. It must be released
/// on that exit. This is a success path, so nothing else would reset the
/// memory tracking, and the 1 MB string would otherwise stay counted and
/// the following 600 KB would exceed a 1.5 MB limit.
#[test]
fn slice_with_unresolved_bound_releases_sliced_value() {
    let mut st = SymbolTable::new();
    st.set(
        "Session.Start",
        ExprValue::unresolved(openjd_expr::ExprType::INT),
    )
    .unwrap();
    let expr = "len(('A' * 1000000)[Session.Start:]) + len('B' * 600000)";
    ParsedExpression::new(expr)
        .and_then(|p| p.with_memory_limit(1_500_000).evaluate(&[&st]))
        .unwrap_or_else(|e| panic!("{expr}: the discarded sliced value must be released: {e}"));
}
