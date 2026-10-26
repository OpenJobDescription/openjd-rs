// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! List literal joins and list concatenation at every stage (2026-02
//! Expression Language §1.2.6, §2.1.3): the common element type, the empty
//! list beside a typed one, and `range_expr` operands.

use super::common::*;
use openjd_expr::*;

// ── List literals ──

#[test]
fn list_literal_keeps_the_empty_form_of_a_union_element() {
    // A-F1/A-F3/A-F7: a list literal or comprehension whose element is
    // `list[string] | list[nulltype]` dropped the empty form, typing
    // `[(LS if c else [])]` a `list[list[string]]`, so `sum` of its element
    // was rejected, though a run that takes `[]` builds `[[]]`, whose
    // element is the untyped empty list (§1.2.1).
    for expr in [
        "sum([(Param.LS if not Task.Param.C else [])][0])",
        "sum(([(Param.LS if not Task.Param.C else [])] + [])[0])",
        "sum([(Param.LS if not Task.Param.C else []) for x in Param.L][0])",
    ] {
        assert_stages(expr, "unresolved[int]", "unresolved[int]", "int", "0");
    }
    // A-F2: with the empty form dropped, every form of the comprehension's
    // element was a list of lists, so it was always empty and indexing it
    // was rejected.
    assert_stages(
        "[y for y in [([Param.L] if not Task.Param.C else [])]][0] + [1]",
        "unresolved[list[int]]",
        "unresolved[list[int]]",
        "list[int]",
        "[1]",
    );
}

#[test]
fn empty_list_beside_a_typed_empty_list_takes_its_type() {
    // A-F2: in `[B, []]` with `B` an empty `list[string]` the run kept the
    // `[]` element untyped, so `sum` of it succeeded, while validation
    // (and a run with a non-empty `B`) typed it `list[string]`. A list
    // value's elements have its element type (§1.2.1).
    let mut p8 = SymbolTable::new();
    p8.set(
        "Param.B",
        ExprValue::unresolved(ExprType::list(ExprType::STRING)),
    )
    .unwrap();
    let mut rt = SymbolTable::new();
    rt.set(
        "Param.B",
        ExprValue::make_list(vec![], ExprType::STRING).unwrap(),
    )
    .unwrap();
    assert_eq!(
        eval("[Param.B, []][1]", &rt).expr_type(),
        tp("list[string]")
    );
    let expr = "sum([Param.B, []][1])";
    for (stage, st) in [("pass 8", &p8), ("run time", &rt)] {
        assert_err_in(
            expr,
            st,
            stage,
            &[
                "No matching signature for sum(list[string])\n",
                &format!("  {expr}\n"),
                "  ^~~~~~~~~~~~~~~~~~~~~",
            ],
        );
    }
}

#[test]
fn empty_lists_keep_their_element_type() {
    // F4 and F5 of audit round 7: two typed empty sides concatenate to
    // their type, and an empty list of ranges is a `list[range_expr]`.
    let st = typed_empty("EF", ExprType::FLOAT);
    let v = eval("EF + EF", &st);
    assert_eq!(v.expr_type(), tp("list[float]"));
    assert_eq!(eval("sum(EF + EF)", &st).to_display_string(), "0.0");
    let [_, g2, rt] = states();
    for st in [&g2, &rt] {
        assert_eq!(
            eval("[Param.R] * 0", st).expr_type(),
            tp("list[range_expr]")
        );
        let expr = "([Param.R] * 0) + ['a']";
        assert_err_in(
            expr,
            st,
            "concrete",
            &[
                "Cannot concatenate list[range_expr] and list[string]\n",
                &format!("  {expr}\n"),
                "  ~~~~~~~~~~~~~~~~^~~~~~~",
            ],
        );
    }
}

#[test]
fn list_of_range_lists_is_two_levels_deep() {
    // A list of `range_expr` values is a flat list: `[[r]]` nests two
    // levels, as §1.2.1 allows. Run time counted the ranges as a level.
    assert_stages(
        "[[Param.R]]",
        "unresolved[list[list[range_expr]]]",
        "list[list[range_expr]]",
        "list[list[range_expr]]",
        r#"[[1-3]]"#,
    );
}

// ── Concatenation ──

#[test]
fn concatenation_joins_inner_empty_lists() {
    // F10 of audit round 1: `[[]] + [[1]]` was rejected ("Cannot
    // concatenate list[list[nulltype]] and list[list[int]]").
    // Concatenation uses the list literal's join (§2.1.3).
    let st = SymbolTable::new();
    assert_eq!(eval("[[]] + [[1]]", &st).to_display_string(), "[[], [1]]");
    assert_eq!(
        eval("[[1]] + [[2.5]]", &st).expr_type(),
        tp("list[list[float]]")
    );
    assert_err_in(
        "[[1]] + [['a']]",
        &st,
        "concrete",
        &[
            "Cannot concatenate list[list[int]] and list[list[string]]\n",
            "  [[1]] + [['a']]\n",
            "  ~~~~~~^~~~~~~~~",
        ],
    );
}

#[test]
fn concatenation_with_an_empty_side_has_the_common_type() {
    // H3 of audit round 2: `make_list` took the type from the elements, so
    // an empty `list[float]` plus `[1]` was `list[int]` (2026-02 §2.1.3
    // gives `list[float]`).
    let st = typed_empty("EF", ExprType::FLOAT);
    let v = eval("EF + [1]", &st);
    assert_eq!(v.expr_type(), tp("list[float]"));
    assert_eq!(v.to_display_string(), "[1.0]");
    let st = typed_empty("EI", ExprType::list(ExprType::INT));
    let v = eval("EI + [[]]", &st);
    assert_eq!(v.expr_type(), tp("list[list[int]]"));
    let expr = "(EI + [[]]) + [['a']]";
    assert_err_in(
        expr,
        &st,
        "concrete",
        &[
            "Cannot concatenate list[list[int]] and list[list[string]]\n",
            &format!("  {expr}\n"),
            "  ~~~~~~~~~~~~^~~~~~~~~",
        ],
    );
}

#[test]
fn range_concatenation_with_an_empty_side_has_the_common_type() {
    // F1 of audit round 3: the range overloads took the type from the
    // elements, so a range and an empty `list[float]` gave `list[int]`
    // (2026-02 §2.1.3: `range_expr + list[T]` is `list[T]`).
    let st = typed_empty("EF", ExprType::FLOAT);
    for expr in ["EF + range_expr('1-2')", "range_expr('1-2') + EF"] {
        let v = eval(expr, &st);
        assert_eq!(v.expr_type(), tp("list[float]"), "{expr}");
        assert_eq!(v.to_display_string(), "[1.0, 2.0]", "{expr}");
    }
    let st = typed_empty("ES", ExprType::STRING);
    let expr = "range_expr('1-2') + ES";
    assert_err_in(
        expr,
        &st,
        "concrete",
        &[
            "Cannot concatenate range_expr and list[string]\n",
            &format!("  {expr}\n"),
            "  ~~~~~~~~~~~~~~~~~~^~~~",
        ],
    );
}

#[test]
fn unresolved_concatenation_has_the_common_element_type() {
    // F3 of audit round 7: concatenation builds the common element type
    // (§2.1.3), but its type was `list[T3]`, so indexing it twice took the
    // `string` overload and validation rejected `+ 1` on runs that succeed.
    let [p8, g2, rt] = states_with_nested_task_list();
    for (expr, ty, value) in [
        (
            "(Task.Param.LL + [[1.5]])[0][0] + 1",
            "unresolved[float]",
            "6.0",
        ),
        ("([[]] + Task.Param.LL)[1][0] + 1", "unresolved[int]", "6"),
    ] {
        for st in [&p8, &g2] {
            assert_eq!(eval(expr, st).expr_type(), tp(ty), "{expr}");
        }
        assert_eq!(eval(expr, &rt).to_display_string(), value, "{expr}");
    }
    // Operands of no common type keep the signature's type; an operand of
    // unknown elements (`any` or a type variable, at any depth, or an
    // operand of unknown type) gives `list[any]`; an unresolved operand is
    // read as its type.
    let lib = FunctionLibrary::for_profile(&ExprProfile::current());
    for (args, ty) in [
        (
            ["list[list[int]]", "list[list[float]]"],
            "list[list[float]]",
        ),
        (["list[any]", "list[int]"], "list[any]"),
        // B-F1..F6 of loop audit 2, round 2.
        (["list[T1]", "list[int]"], "list[any]"),
        (["list[list[any]]", "list[list[int]]"], "list[any]"),
        (["list[string]", "list[int]"], "list[T3]"),
        (["range_expr", "list[string]"], "list[T2]"),
        (["list[int]", "any"], "list[any]"),
        (["unresolved[list[int]]", "list[float]"], "list[float]"),
    ] {
        let args = args.map(tp);
        assert_eq!(
            lib.derive_return_type("__add__", &args),
            Some(tp(ty)),
            "{args:?}"
        );
    }
}

#[test]
fn concatenation_with_a_range_or_unknown_elements_types_its_uses() {
    // A-F1, B-F1: `Param.R + Param.L` was a `list[T2]`, and a concatenation
    // with an `any` operand a `list[T3]`, so their elements failed `in` and
    // arithmetic at validation though every run succeeds. A range
    // concatenates as a `list[int]`, and unknown elements give `list[any]`.
    for (expr, ty, value) in [
        (
            "2 in ([x for x in Param.R + Param.L] + [1])",
            "unresolved[bool]",
            "true",
        ),
        (
            "([[x] for x in Param.R + Param.L] + [[1]])[0][0] - 1",
            "unresolved[int]",
            "0",
        ),
        (
            "([x for x in Param.R + Param.L] + [2.5])[0] - 1",
            "unresolved[float]",
            "0.0",
        ),
        (
            "[x[0] for x in [Param.R + Param.L] if x[0] in [1, 2]][0]",
            "unresolved[int]",
            "1",
        ),
        (
            "(Param.R + Param.L)[0] in [1, 2]",
            "unresolved[bool]",
            "true",
        ),
    ] {
        let [p8, g2, rt] = states();
        assert_eq!(eval(expr, &p8).expr_type(), tp(ty), "{expr}");
        for st in [&g2, &rt] {
            assert_eq!(eval(expr, st).to_display_string(), value, "{expr}");
        }
    }
    // A-F2: job creation joined a `T3` element with an `int` and failed.
    let [p8, g2, rt] =
        states_with_any_list(ExprValue::make_list(vec![], ExprType::NULLTYPE).unwrap());
    let expr = "[x if x > 1 else (Task.Param.A + [1])[0] for x in Param.L]";
    assert_eq!(eval(expr, &p8).expr_type(), tp("unresolved[list[any]]"));
    assert_eq!(eval(expr, &g2).expr_type(), tp("unresolved[list[any]]"));
    assert_eq!(eval(expr, &rt).to_display_string(), "[1, 2, 3]");
}

#[test]
fn concatenation_with_an_any_operand_on_either_side_is_a_list_of_any() {
    // F2 of audit round 8: an `any` element type on the right was untested.
    // A `list[any]` value may be a `list[string]`, so `+ 'a'` on an element
    // must not be rejected.
    let lib = FunctionLibrary::for_profile(&ExprProfile::current());
    for args in [["list[any]", "list[int]"], ["list[int]", "list[any]"]] {
        let args = args.map(tp);
        assert_eq!(
            lib.derive_return_type("__add__", &args),
            Some(tp("list[any]")),
            "{args:?}"
        );
    }
    let [mut p8, _, _] = states();
    p8.set("Task.Param.A", ExprValue::unresolved(tp("list[any]")))
        .unwrap();
    assert_eq!(
        eval("(Param.L + Task.Param.A)[0] + 'a'", &p8).expr_type(),
        tp("unresolved")
    );
}

#[test]
fn concatenation_with_an_operand_of_unknown_type_is_a_list_of_any() {
    // B-F2: a bare type-variable operand (`(A + B)[0]` with no common type
    // is a `T3`) reads as a list of unknown elements.
    let lib = FunctionLibrary::for_profile(&ExprProfile::current());
    for args in [
        ["T", "list[int]"],
        ["T1", "list[int]"],
        ["T3", "list[int]"],
        ["list[int]", "T3"],
    ] {
        let args = args.map(tp);
        assert_eq!(
            lib.derive_return_type("__add__", &args),
            Some(tp("list[any]")),
            "{args:?}"
        );
    }
    let [mut p8, _, _] = states_with_nested_task_list();
    p8.set("A", ExprValue::unresolved(tp("list[list[string]]")))
        .unwrap();
    assert_eq!(
        eval("[y + 1 for y in (A + Task.Param.LL)[0] + [1]][0]", &p8).expr_type(),
        tp("unresolved")
    );
}

#[test]
fn concatenation_of_a_list_and_a_range_names_the_range() {
    // F3 of audit round 9: with the range on the right, the error names it.
    let [_, g2, rt] = states();
    let expr = "['a'] + Param.R";
    for (stage, st) in [("gate 2", &g2), ("run time", &rt)] {
        assert_err_in(
            expr,
            st,
            stage,
            &[
                "Cannot concatenate list[string] and range_expr\n",
                &format!("  {expr}\n"),
                "  ~~~~~~^~~~~~~~~",
            ],
        );
    }
}

#[test]
fn sum_of_an_empty_float_list_is_a_float() {
    // F2 of audit round 3: `sum(list[float]) -> float`, but an empty one
    // summed to `int 0`.
    let v = eval("sum(EF)", &typed_empty("EF", ExprType::FLOAT));
    assert_eq!(v.expr_type(), ExprType::FLOAT);
    assert_eq!(v.to_display_string(), "0.0");
    assert_eq!(
        eval("sum(EI)", &typed_empty("EI", ExprType::INT)),
        ExprValue::Int(0)
    );
}
