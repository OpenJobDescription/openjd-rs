// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Conditional tests, indices, and chained comparisons whose operands are
//! unresolved (2026-02 Expression Language §1.3.5, §1.3.6, §2.1.7, and Static
//! Type Checking via Unresolved Values): a type a run's value may make a
//! `bool` or an `int` is accepted, and a chain is its links joined by `and`.

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

// ── Names that resolve nowhere ──

#[test]
fn name_that_resolves_nowhere_in_a_part_a_run_may_skip_is_an_error() {
    // PR #462 review: whether a symbol, function or method exists does not
    // depend on any value, so a sub-expression that names one that does
    // not exist fails every run that evaluates it. Static checking reports
    // it even where a run may not evaluate it: the branch of a conditional
    // with an unresolved test, an `and`/`or` operand after an unresolved
    // one, and a chained comparison's link after an unresolved one.
    let [p8, g2, _] = states();
    for (expr, msg, caret) in [
        (
            "1 if Task.Param.C else strr(1)",
            "Unknown function: 'strr'",
            "                         ^~~~~~~",
        ),
        (
            "Param.Typo if Task.Param.C else 1",
            "Undefined variable: 'Param.Typo'. Did you mean one of: Param.EL, Param.L, Param.LS, Param.R, Param.Z",
            "  ~~~~~~^~~~",
        ),
        (
            "Task.Param.C or 'a'.uper()",
            "Unknown function: 'uper'",
            "                  ~~~~^~~~~~",
        ),
        (
            "Task.Param.I == 5 == strr(1)",
            "Unknown function: 'strr'",
            "                       ^~~~~~~",
        ),
    ] {
        for (stage, st) in [("pass 8", &p8), ("gate 2", &g2)] {
            assert_err_in(
                expr,
                st,
                stage,
                &[&format!("{msg}\n"), &format!("  {expr}\n"), caret],
            );
        }
    }
    // A failure that depends on types is still absorbed there.
    assert_eq!(
        eval("1 if Task.Param.C else 'a' + 1", &p8).expr_type(),
        tp("unresolved[int]")
    );
    // Audit A-F1/B-F1: a dotted attribute whose base fails on its value's
    // type (`int` has no `parent`) is type-dependent, not an undefined
    // name, and is absorbed too; a dotted path rooted at an undefined
    // name is still reported.
    assert_eq!(
        eval("1 if Task.Param.C else Task.Param.I.parent.name", &p8).expr_type(),
        tp("unresolved[int]")
    );
    let expr = "1 if Task.Param.C else Param.Typo.Deep";
    assert_err_in(
        expr,
        &p8,
        "pass 8",
        &[
            "Undefined variable: 'Param.Typo.Deep'.\n",
            &format!("  {expr}\n"),
            "                         ~~~~~~~~~~~^~~~",
        ],
    );
}

#[test]
fn member_some_type_has_in_a_custom_library_is_absorbed() {
    // Round 2 B-F1/B-F2: with a library where the member exists for some
    // type, a misuse fails on the value's type and is absorbed in a part a
    // run may skip: a property of a generic receiver that fails, a method
    // that is also a property of another type, and a property that is also
    // a method of another type.
    fn failing(_: &mut dyn EvalContext, _: &[ExprValue]) -> Result<ExprValue, ExpressionError> {
        Err(ExpressionError::new("first: the list is empty"))
    }
    fn stem2(_: &mut dyn EvalContext, a: &[ExprValue]) -> Result<ExprValue, ExpressionError> {
        Ok(a[0].clone())
    }
    let mut lib = (*FunctionLibrary::for_profile(&ExprProfile::current())).clone();
    lib.register_sig("__property_first__", "(list[T]) -> T", failing)
        .unwrap();
    lib.register_sig("stem", "(path, int) -> path", stem2)
        .unwrap();
    lib.register_sig("__property_upper__", "(path) -> string", failing)
        .unwrap();
    let [p8, _, _] = states();
    for expr in [
        "Task.Param.C or [1].first == 1",
        "Task.Param.C or path('/a').stem() == 'x'",
        "Task.Param.C or 'a'.upper == 'x'",
    ] {
        let v = ParsedExpression::new(expr)
            .unwrap()
            .with_library(&lib)
            .evaluate(&[&p8])
            .unwrap_or_else(|e| panic!("{expr}: {e}"));
        assert_eq!(v.expr_type(), tp("unresolved[bool]"), "{expr}");
    }
}
