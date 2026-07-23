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

// === Regression tests: bounded value domain (|v| < 2^62) ===
// Range endpoints and steps are restricted below the full i64 domain so
// all derived arithmetic (spans, counts, index products, cumulative
// sums) stays exactly representable in i64/u64. Inputs at or beyond the
// bound raise an integer overflow error at construction.

#[test]
fn endpoint_at_bound_rejected_with_overflow_error() {
    assert_err(
        "range_expr('0-4611686018427387904')",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  range_expr('0-4611686018427387904')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
    assert_err(
        "range_expr('-4611686018427387904')",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  range_expr('-4611686018427387904')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn step_at_bound_rejected_with_overflow_error() {
    assert_err(
        "range_expr('0-10:4611686018427387904')",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  range_expr('0-10:4611686018427387904')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn from_value_list_at_bound_rejected() {
    assert_err(
        "range_expr([1, 4611686018427387904])",
        &[
            "Integer overflow: result is outside the 64-bit signed range\n",
            "  range_expr([1, 4611686018427387904])\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn widest_legal_range_exact_everywhere() {
    // Largest legal magnitude: 2^62 - 1. Every derived quantity is
    // exact — len, tail indexing, membership, min/max, equality.
    let max = (1i64 << 62) - 1;
    let mut st = SymbolTable::new();
    st.set(
        "R",
        ExprValue::RangeExpr(format!("{}-{}", -max, max).parse().unwrap()),
    )
    .unwrap();
    assert_eq!(
        eval_with("len(R)", &st).to_display_string(),
        "9223372036854775807" // 2 * (2^62 - 1) + 1
    );
    assert_eq!(eval_with("R[-1]", &st).to_display_string(), max.to_string());
    assert_eq!(
        eval_with("R[0]", &st).to_display_string(),
        (-max).to_string()
    );
    assert_eq!(eval_with("5 in R", &st).to_display_string(), "true");
    assert_eq!(
        eval_with("max(R)", &st).to_display_string(),
        max.to_string()
    );
    assert_eq!(
        eval_with("min(R)", &st).to_display_string(),
        (-max).to_string()
    );
    assert_eq!(eval_with("R == []", &st).to_display_string(), "false");
}

#[test]
fn widest_legal_range_slice_exact() {
    let max = (1i64 << 62) - 1;
    let mut st = SymbolTable::new();
    st.set(
        "R",
        ExprValue::RangeExpr(format!("{}-{}", -max, max).parse().unwrap()),
    )
    .unwrap();
    let sliced = eval_with("R[0:3]", &st).to_display_string();
    assert_eq!(sliced, format!("{}-{}", -max, -max + 2));
    // Tail slice resolves against the exact length. (Two-element ranges
    // display in comma form.)
    let tail = eval_with("R[-2:]", &st).to_display_string();
    assert_eq!(tail, format!("{},{}", max - 1, max));
}

#[test]
fn reverse_slice_with_huge_step_single_element() {
    // A reverse step of huge magnitude visits only the start element,
    // and must not overflow the walk arithmetic.
    assert_eq!(
        eval("range_expr('1-5')[::-4611686018427387903]").to_display_string(),
        "[5]"
    );
}

#[test]
fn merge_at_value_bound_no_overflow() {
    // Two singletons adjacent at the top of the legal domain: the merge
    // check computes end + step without overflow.
    let max = (1i64 << 62) - 1;
    let r: RangeExpr = format!("{},{}", max - 1, max).parse().unwrap();
    assert_eq!(r.len(), 2);
    assert_eq!(r.get(-1), Some(max));
}

#[test]
fn parse_duplicate_bound_singletons_overlap_error() {
    // Duplicates are an overlap error (and the merge check's end + step
    // stays in i64 at the domain edge).
    let max = (1i64 << 62) - 1;
    let expr = format!("range_expr('{max},{max}')");
    let e = ParsedExpression::new(&expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string();
    assert!(e.contains("overlapping ranges"), "got:\n{e}");
}

// === Regression tests: range containment by value equality ===

#[test]
fn float_in_range_matches_python() {
    assert_eq!(eval("1.0 in range_expr('1-3')").to_display_string(), "true");
    assert_eq!(
        eval("1.5 in range_expr('1-3')").to_display_string(),
        "false"
    );
    assert_eq!(
        eval("'a' in range_expr('1-3')").to_display_string(),
        "false"
    );
    assert_eq!(
        eval("4.0 not in range_expr('1-3')").to_display_string(),
        "true"
    );
}

// === Regression tests: reverse-slice stop clamping ===

#[test]
fn reverse_slice_huge_negative_stop_clamped() {
    // A stop far below -len must clamp to the -1 sentinel ("walk to
    // index 0"), not leave the reverse walk spinning through ~i64::MAX
    // uncharged below-zero indices.
    assert_eq!(
        eval("range_expr('1-5')[:-9223372036854775807:-1]").to_display_string(),
        "[5, 4, 3, 2, 1]"
    );
    // Same for a huge negative start.
    assert_eq!(
        eval("range_expr('1-5')[-9223372036854775807::-1]").to_display_string(),
        "[]"
    );
}

#[test]
fn list_and_string_reverse_slice_huge_negative_stop_clamped() {
    // Same clamp in the shared compute_slice_indices helper: list and
    // string reverse slices previously spun through uncharged
    // below-zero indices for stops far below -len.
    assert_eq!(
        eval("[1, 2, 3][:-9223372036854775807:-1]").to_display_string(),
        "[3, 2, 1]"
    );
    assert_eq!(
        eval("'abc'[:-9223372036854775807:-1]").to_display_string(),
        "cba"
    );
}

// === Regression tests: hashing on boundary values ===

#[test]
fn list_float_hash_consistent_with_exact_equality() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    fn hash_of(v: &ExprValue) -> u64 {
        let mut h = DefaultHasher::new();
        v.hash(&mut h);
        h.finish()
    }
    // 2^63 as f64 is NOT exactly representable in i64, so it must hash
    // under the float tag in both the top-level Float arm and the
    // ListFloat arm — and a ListFloat must hash identically to the
    // equal ListList of the same floats (a == b requires
    // hash(a) == hash(b)).
    let boundary = 9223372036854775808.0f64; // 2^63
    let f = openjd_expr::value::Float64::with_str(boundary, "x".into()).unwrap();
    let list_float = ExprValue::ListFloat(vec![f.clone()]);
    // Externally constructed ListList holding the same float (ExprValue's
    // variants are public API): equality is generic across list variants,
    // so hashing must agree too.
    let list_list = ExprValue::ListList(
        vec![ExprValue::Float(f)],
        openjd_expr::types::ExprType::FLOAT,
        0,
    );
    assert!(matches!(list_list, ExprValue::ListList(..)));
    assert_eq!(list_float, list_list);
    assert_eq!(hash_of(&list_float), hash_of(&list_list));
}

#[test]
fn coerce_large_range_to_list_rejected() {
    // ExprValue::coerce runs outside any evaluation budget; a range
    // larger than the default operation limit must be rejected, not
    // materialized.
    let r: RangeExpr = "1-100000000".parse().unwrap();
    let v = ExprValue::RangeExpr(r);
    let err = v
        .coerce(
            &openjd_expr::types::ExprType::list(openjd_expr::types::ExprType::INT),
            openjd_expr::path_mapping::PathFormat::Posix,
        )
        .unwrap_err();
    assert!(err.contains("Cannot coerce range_expr"), "got: {err}");
    // A small range still coerces.
    let small = ExprValue::RangeExpr("1-3".parse::<RangeExpr>().unwrap());
    assert_eq!(
        small
            .coerce(
                &openjd_expr::types::ExprType::list(openjd_expr::types::ExprType::INT),
                openjd_expr::path_mapping::PathFormat::Posix,
            )
            .unwrap()
            .to_display_string(),
        "[1, 2, 3]"
    );
}

// === Regression tests: seventh review round ===

#[test]
fn from_values_far_apart_values_stay_singletons() {
    // The gap between two in-bound values can span up to just under
    // 2^63, exceeding the per-chunk step bound (2^62): the pair must
    // stay singleton chunks, and the extrapolation must not overflow.
    let max = (1i64 << 62) - 1;
    let r = RangeExpr::from_values(vec![-max, max]).unwrap();
    assert_eq!(r.ranges().len(), 2);
    assert_eq!(r.get(0), Some(-max));
    assert_eq!(r.get(1), Some(max));
    // Three equally-spaced values whose step (2^62 - 1) is within the
    // bound form a legal stepped chunk — and extrapolating one step past
    // `max` (which would overflow i64) must terminate cleanly.
    let r = RangeExpr::from_values(vec![-max, 0, max]).unwrap();
    assert_eq!(r.ranges().len(), 1);
    assert_eq!(r.ranges()[0].step(), max);
    assert_eq!(r.get(-1), Some(max));
}

#[test]
fn int_range_deserialize_revalidates_invariants() {
    // Fields are private, and the manual Deserialize implementation
    // re-validates serialized input through IntRange::new.
    let err = serde_json::from_str::<openjd_expr::range_expr::IntRange>(
        r#"{"start": 0, "end": 1, "step": 0}"#,
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("step must not be zero"),
        "got: {err}"
    );
    // A descending pair is rejected rather than corrupting invariants.
    assert!(serde_json::from_str::<openjd_expr::range_expr::IntRange>(
        r#"{"start": 5, "end": 1, "step": 1}"#,
    )
    .is_err());
}
