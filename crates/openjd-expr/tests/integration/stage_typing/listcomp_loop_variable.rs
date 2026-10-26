// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! The loop variable of a list comprehension over an unresolved iterable
//! (2026-02 Expression Language §1.3.7): the element type of a list, `int`
//! for a `range_expr`, the union of a union's element types, and `any` for an
//! iterable of unknown type or one that cannot be iterated.

use super::common::*;
use openjd_expr::*;

// ── Iterables of known type ──

#[test]
fn union_iterable_binds_the_union_of_element_types() {
    // Item 28 of the resolved-value-limits design: the loop variable was
    // bound as `int`.
    assert_stages(
        "[x for x in (Param.L if Task.Param.C else Param.LS)]",
        "unresolved[list[int | string | nulltype]]",
        "unresolved[list[int | string | nulltype]]",
        "list[int]",
        "[1, 2, 3]",
    );
    let [p8, _, _] = states();
    let mut strs = states()[2].clone();
    strs.set("Task.Param.C", ExprValue::Bool(false)).unwrap();
    let expr = "[x.upper() for x in (Param.L if Task.Param.C else Param.LS)]";
    assert_eq!(
        eval(expr, &p8).expr_type(),
        tp("unresolved[list[string | nulltype]]")
    );
    assert_eq!(eval(expr, &strs).to_display_string(), r#"["A"]"#);
}

// ── Iterables of unknown type ──

#[test]
fn type_variable_iterable_binds_any() {
    // F1 of audit round 7: an iterable typed `T3` (an element of a
    // concatenation with a `list[any]`) was taken as never iterable, so
    // validation rejected comprehensions that run.
    let [p8, g2, rt] =
        states_with_any_list(ExprValue::make_list(vec![], ExprType::NULLTYPE).unwrap());
    let expr = "[y + 1 for y in (Task.Param.A + [[5, 6]])[0]]";
    for st in [&p8, &g2] {
        assert_eq!(
            eval(expr, st).expr_type(),
            tp("unresolved[list[any]]"),
            "{expr}"
        );
    }
    assert_eq!(eval(expr, &rt).to_display_string(), "[6, 7]");
}

#[test]
fn any_iterable_binds_any() {
    // F2 of audit round 7: the loop variable over an `any` iterable is
    // `any`, so its body types as the run's would.
    let [mut p8, _, _] = states();
    p8.set("U", ExprValue::unresolved(ExprType::ANY)).unwrap();
    assert_eq!(
        eval("[x.upper() for x in U]", &p8).expr_type(),
        tp("unresolved[list[any]]")
    );
}

#[test]
fn every_type_variable_iterable_binds_any() {
    // F2 of audit round 9: a `T1`-typed iterable element (`sorted` of an
    // `any` is a `list[T1]`), not only `T3`, binds `any`; every type
    // variable code is covered by the unit tests of `comprehension_var`.
    let [mut p8, _, mut rt] = states();
    p8.set("U", ExprValue::unresolved(ExprType::ANY)).unwrap();
    rt.set("U", ExprValue::ListInt(vec![2, 1])).unwrap();
    let expr = "[z for z in sorted(U) if z in [1, 2]][0]";
    assert_eq!(eval(expr, &p8).expr_type(), tp("unresolved"));
    assert_eq!(eval(expr, &rt).to_display_string(), "1");
}

#[test]
fn type_variable_element_type_binds_any() {
    // F1 of audit round 10: over `Param.R + Param.L` (a `list[T2]`) the loop
    // variable was `T2`, so `x in [1, 2]` failed to type, the comprehension
    // was `list[nulltype]`, and indexing it was rejected though every run
    // gives `1` (§1.3.7: a type variable binds `any`).
    let [p8, g2, rt] = states();
    let expr = "[x for x in Param.R + Param.L if x in [1, 2]][0]";
    // (`Param.R + Param.L` is now a `list[int]`, round 11.)
    assert_eq!(eval(expr, &p8).expr_type(), tp("unresolved[int]"));
    for st in [&g2, &rt] {
        assert_eq!(eval(expr, st).to_display_string(), "1");
    }
}

#[test]
fn bare_unresolved_element_type_binds_any() {
    // F1 of audit round 8: a `list[unresolved]` iterable (built through
    // `ExprType::new`) panicked in union normalization; base typed it.
    let mut st = states()[0].clone();
    let bare = ExprType::new(TypeCode::Unresolved, vec![]);
    st.set("LU", ExprValue::unresolved(ExprType::list(bare.clone())))
        .unwrap();
    st.set("U", ExprValue::unresolved(bare)).unwrap();
    for (expr, ty) in [
        ("[x for x in LU]", "unresolved[list[any]]"),
        ("[x for x in U]", "unresolved[list[any]]"),
        (
            "[x for x in Param.L if U]",
            "unresolved[list[int | nulltype]]",
        ),
    ] {
        assert_eq!(eval(expr, &st).expr_type(), tp(ty), "{expr}");
    }
}

// ── Iterables that cannot be iterated ──

#[test]
fn unresolved_iterable_that_cannot_be_iterated() {
    // An iterable whose value is not known, and whose type cannot be
    // iterated, is not rejected at validation (loop audit 2, round 2): its
    // type may come from typing an operation on values of unknown type
    // (round 4 makes such operations `any`), so the loop variable is `any`. A run, which
    // has the value, reports it.
    let expr = "[x for x in Task.Param.I]";
    let [p8, g2, rt] = states();
    for st in [&p8, &g2] {
        assert_eq!(eval(expr, st).expr_type(), tp("unresolved[list[any]]"));
    }
    assert_err_in(
        expr,
        &rt,
        "run time",
        &[
            "Cannot iterate over int\n",
            &format!("  {expr}\n"),
            "  ^~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
    // A type that comes from such an operation, though the run iterates a
    // list (`[[5, 6]][0] * 2`).
    let [p8, _, rt] = states_with_nested_task_list();
    let expr = "[x for x in (([] if Task.Param.C else Param.R) + Task.Param.LL)[0] * 2]";
    assert_eq!(eval(expr, &p8).expr_type(), tp("unresolved[list[any]]"));
    assert_eq!(eval(expr, &rt).to_display_string(), "[5, 6, 5, 6]");
}

#[test]
fn union_iterable_member_that_cannot_be_iterated_binds_any() {
    // F3 of audit round 6, revised in loop audit 2 round 3: a member whose
    // type cannot be iterated (here `string`) may come from typing an
    // operation on values of unknown type, so it binds `any` rather than
    // nothing; a run that takes it reports it.
    assert_stages(
        "[x for x in (Param.L if Task.Param.C else Param.LS[0])]",
        "unresolved[list[any]]",
        "unresolved[list[any]]",
        "list[int]",
        "[1, 2, 3]",
    );
}

#[test]
fn union_iterable_member_of_a_non_iterable_type_binds_any() {
    // A-F1, B-F1: a member typed `int` by an operation on values of
    // unknown type (`([] + LL)[0] * 2`) contributed nothing, so with an
    // empty other branch the comprehension was the empty list and indexing
    // it was rejected, though the run gives 5.
    let [p8, g2, rt] = states_with_nested_task_list();
    let e1 = "((([] if Task.Param.C else Param.R) + Task.Param.LL)[0] * 2)";
    for (expr, ty, value) in [
        (
            format!("[x for x in ({e1} if Task.Param.C else [])][0]"),
            "unresolved",
            "5",
        ),
        (
            format!("[x + 1 for x in ({e1} if Task.Param.C else Param.LS)][0]"),
            "unresolved",
            "6",
        ),
    ] {
        for st in [&p8, &g2] {
            assert_eq!(eval(&expr, st).expr_type(), tp(ty), "{expr}");
        }
        assert_eq!(eval(&expr, &rt).to_display_string(), value, "{expr}");
    }
    // A `null` member fails every run that takes it: it binds nothing.
    assert_stages(
        "[x for x in (Param.L if Task.Param.C else None)]",
        "unresolved[list[int | nulltype]]",
        "unresolved[list[int | nulltype]]",
        "list[int]",
        "[1, 2, 3]",
    );
}

#[test]
fn every_non_iterable_member_type_binds_any() {
    // L2: float, bool and path members, like int and string, bind `any`.
    let mut st = states()[0].clone();
    for (name, ty) in [("F", "float"), ("B", "bool"), ("P", "path")] {
        st.set(&format!("Task.Param.{name}"), ExprValue::unresolved(tp(ty)))
            .unwrap();
    }
    for name in ["F", "B", "P"] {
        let expr = format!("[x for x in (Task.Param.{name} if Task.Param.C else Param.L)]");
        assert_eq!(
            eval(&expr, &st).expr_type(),
            tp("unresolved[list[any]]"),
            "{expr}"
        );
    }
}
