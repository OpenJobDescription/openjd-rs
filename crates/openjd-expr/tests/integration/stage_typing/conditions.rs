// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Conditional tests, indices, and chained comparisons whose operands are
//! unresolved (2026-02 Expression Language §1.3.6, Static Type Checking): a
//! type a run's value may make a `bool` or an `int` is accepted, and a
//! chain is its links joined by `and`.

use super::common::*;
use openjd_expr::*;

// ── Conditional tests ──

#[test]
fn conditional_test_of_unknown_type_can_be_a_boolean() {
    // B-H2: a conditional whose test is a type variable was "Condition must
    // be a boolean, got T1", though the run's value is a bool.
    let [p8, g2, mut rt] = states();
    rt.set("Task.Param.C", ExprValue::Bool(false)).unwrap();
    let expr = "1 if flatten([x for x in (Param.Z if Task.Param.C else [[True]])])[0] else 2";
    for st in [&p8, &g2] {
        assert_eq!(eval(expr, st).expr_type(), tp("unresolved[int]"), "{expr}");
    }
    assert_eq!(eval(expr, &rt).to_display_string(), "1");
}

#[test]
fn conditional_test_that_never_gives_a_value_is_an_error() {
    // B-F1: a `noreturn` test fails every run, so it is rejected as before,
    // although a comprehension filter accepts one.
    let [p8, _, _] = states();
    let expr = "1 if fail(Param.LS[0]) else 2";
    assert_err_in(
        expr,
        &p8,
        "pass 8",
        &[
            "Condition must be a boolean, got noreturn\n",
            &format!("  {expr}\n"),
            "       ^~~~~~~~~~~~~~~~~",
        ],
    );
}

// ── Indices ──

#[test]
fn union_index_with_a_member_that_can_be_an_int() {
    // B-H1: `T1 | int` (and `string | int`) as an index was "Index must be
    // an integer", though a run that takes the int succeeds.
    let [p8, g2, mut rt] = states();
    rt.set("Task.Param.C", ExprValue::Bool(false)).unwrap();
    let t1 = "flatten([x for x in (Param.Z if Task.Param.C else [[1]])])[0]";
    for (expr, value) in [
        (format!("[1, 2][{t1} if Task.Param.C else 0]"), "1"),
        (format!("'ab'[{t1} if Task.Param.C else 0]"), "a"),
        (
            "[1, 2][Param.LS[0] if Task.Param.C else Task.Param.I - 1]".to_string(),
            "2",
        ),
    ] {
        for st in [&p8, &g2] {
            assert!(
                ParsedExpression::new(&expr)
                    .and_then(|p| p.evaluate(st))
                    .is_ok(),
                "{expr}"
            );
        }
        assert_eq!(eval(&expr, &rt).to_display_string(), value, "{expr}");
    }
    // A `T3` index (no common element type) is accepted too (B-M1).
    let expr = "[1, 2][(Param.L + Param.LS)[0]]";
    assert_eq!(eval(expr, &p8).expr_type(), tp("unresolved"));
    // An index that can never be an int is still an error.
    let expr = "[1, 2][Param.LS[0]]";
    assert_err_in(
        expr,
        &p8,
        "pass 8",
        &[
            "Index must be an integer\n",
            &format!("  {expr}\n"),
            "  ~~~~~~^~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn union_index_with_no_member_that_can_be_an_int_is_an_error() {
    // B-F3: only a union with a member that can be an int is accepted as
    // an index.
    let [p8, g2, _] = states();
    let expr = "Param.L[Param.LS[0] if Task.Param.C else path('/a')]";
    for (stage, st) in [("pass 8", &p8), ("gate 2", &g2)] {
        assert_err_in(
            expr,
            st,
            stage,
            &[
                "Index must be an integer\n",
                &format!("  {expr}\n"),
                "  ~~~~~~~^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
            ],
        );
    }
}

// ── Chained comparisons ──

#[test]
fn chained_comparison_past_an_unresolved_link_absorbs_a_failing_link() {
    // A-F1: `a == b == c` is `a == b and b == c` (§1.3.6). A run stops at
    // a `false` link, so a later link that fails to type (an always-empty
    // index, or no signature) does not make the chain an error, as it
    // does not in the `and` form.
    for expr in [
        "Task.Param.I == 5 == [][Task.Param.I]",
        "Task.Param.I == 5 == [[x] for x in Param.EL][0]",
        "Task.Param.I == 5 == len(1)",
    ] {
        assert_stages(
            expr,
            "unresolved[bool]",
            "unresolved[bool]",
            "bool",
            "false",
        );
    }
    // The first link always runs, so its failure is an error.
    let expr = "len(1) == Task.Param.I == 5";
    assert_err_all(
        expr,
        &[
            "No matching signature for len(int)\n",
            &format!("  {expr}\n"),
            "  ^~~~~~",
        ],
    );
}
