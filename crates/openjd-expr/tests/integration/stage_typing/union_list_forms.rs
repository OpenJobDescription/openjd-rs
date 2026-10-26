// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Lists with a union element type and union arguments (2026-02 Expression
//! Language §1.2.1): a call is typed by each form of its arguments, and
//! indexing the empty form is `noreturn` (§2.1.7), reported where a list is
//! always empty.

use super::common::*;
use openjd_expr::*;

// ── Calls typed by forms ──

#[test]
fn calls_match_each_form_of_a_union_element_list() {
    // `sum` has `list[int]`, `list[float]`, and `list[nulltype]`
    // signatures; each form of `list[float | int | nulltype]` matches one.
    let [p8, _, rt] = states();
    let expr = "sum([x if x > 1 else 2.5 for x in Param.L])";
    assert_eq!(eval(expr, &p8).expr_type(), tp("unresolved[float | int]"));
    assert_eq!(eval(expr, &rt).to_display_string(), "7.5");
    // A form-wise result of several list types is the one list type it
    // equals.
    assert_eq!(
        eval("sorted([x if x > 1 else 2.5 for x in Param.L])", &p8).expr_type(),
        tp("unresolved[list[float | int | nulltype]]")
    );
    // No form matches: `upper` takes a string.
    let expr = "[x if x > 1 else 2.5 for x in Param.L].upper()";
    assert_err_in(
        expr,
        &p8,
        "pass 8",
        &[
            "upper() is not available for list[float | int | nulltype]. Available for: string\n",
            &format!("  {expr}\n"),
            &format!("  {}^~~~~~~", "~".repeat(39)),
        ],
    );
}

#[test]
fn calls_on_union_element_lists_within_the_cap_are_typed_by_their_forms() {
    // F3 of audit round 7: four forms, each typed by `sum`'s signatures.
    assert_stages(
        "sum([x if x > 1 else 2.5 if x > 0 else 'a' for x in Param.L])",
        "unresolved[float | int]",
        "float",
        "float",
        "7.5",
    );
    // A form-wise result is merged at every depth.
    let [p8, _, _] = states();
    assert_eq!(
        eval("sorted([[x] if x > 1 else ['a'] for x in Param.L])", &p8).expr_type(),
        tp("unresolved[list[list[int | string] | nulltype]]")
    );
    // `derive_return_type` types the forms as `call` does, and applies a
    // `list[nulltype]` overload only to an argument that is exactly the
    // empty list.
    let lib = FunctionLibrary::for_profile(&ExprProfile::current());
    assert_eq!(
        lib.derive_return_type("sorted", &[tp("list[int | float | nulltype]")]),
        Some(tp("list[float | int | nulltype]"))
    );
    assert_eq!(
        lib.derive_return_type("__getitem__", &[tp("list[any]"), tp("int")]),
        Some(ExprType::ANY)
    );
    assert_eq!(
        lib.derive_return_type("__getitem__", &[tp("list[nulltype]"), tp("int")]),
        Some(ExprType::NORETURN)
    );
}

#[test]
fn union_of_list_types_is_typed_by_its_members() {
    // A-F1/B-F5: a union of list types, as a conditional gives, was
    // dispatched by its first member, so `(LS if c else L)[0]` was an
    // `int` and `.upper()` of it was rejected, though a run that takes
    // `LS` succeeds. `list[A] | list[B]` has the forms of `list[A | B]`
    // (§1.2.1).
    assert_stages(
        "(Param.LS if Task.Param.C else Param.L)[0].upper()",
        "unresolved[string]",
        "unresolved[string]",
        "string",
        "A",
    );
    assert_stages(
        "'a' in (Param.LS if Task.Param.C else Param.L)",
        "unresolved[bool]",
        "unresolved[bool]",
        "bool",
        "true",
    );
    assert_stages(
        "([] if not Task.Param.C else Param.LS)[0].upper()",
        "unresolved[string]",
        "unresolved[string]",
        "string",
        "A",
    );
    assert_stages(
        "sum(Param.L if Task.Param.C else [2.5])",
        "unresolved[float | int]",
        "unresolved[float | int]",
        "int",
        "6",
    );
    // A-F2: concatenating a union of list types took no common element
    // type (`list[T3]`), so `1 in` it was rejected. The operand reads as the
    // one list type it equals (`list[string | nulltype]`).
    assert_stages(
        "Param.L + (Param.LS if not Task.Param.C else [])",
        "unresolved[list[int]]",
        "unresolved[list[int]]",
        "list[int]",
        "[1, 2, 3]",
    );
    assert_stages(
        "1 in Param.L + (Param.LS if not Task.Param.C else [])",
        "unresolved[bool]",
        "unresolved[bool]",
        "bool",
        "true",
    );
}

#[test]
fn empty_list_overloads_apply_only_to_the_empty_list() {
    // F1 of audit round 1: an argument of unknown element type (`list[T3]`
    // from an unresolved `+`, or `list[any]`) matched
    // `__getitem__(list[nulltype], int) -> noreturn` first, so validation
    // rejected `[0] + 1`; only the empty list selects that overload at run
    // time. So with `flatten(list[nulltype])`.
    assert_stages(
        "(Param.L + Param.L)[0] + 1",
        "unresolved[int]",
        "int",
        "int",
        "2",
    );
    let mut st = states()[0].clone();
    st.set("U", ExprValue::unresolved(tp("list[any]"))).unwrap();
    for (expr, ty) in [
        ("U[0]", "unresolved"),
        ("flatten(U)", "unresolved[list[T1]]"),
        ("min(Param.L + Param.L)", "unresolved[int]"),
    ] {
        assert_eq!(eval(expr, &st).expr_type(), tp(ty), "{expr}");
    }
}

#[test]
fn calls_past_the_form_combination_cap_are_typed_by_the_argument_types() {
    // F1 and F5 of audit round 6: past 64 combinations of forms a call was
    // typed `any`, and `any[0]` then took the `string` overload, so validation
    // rejected `+ 1` though every run gives an int. The body has nine
    // types, so each comprehension's element type has ten forms, and `+`
    // of two has a hundred. The element's `+ 1` is typed by its nine forms
    // (`2.5 + 1` is a float).
    let b = "(1 if Param.Z == 0 else 2.5 if Param.Z == 1 else 'a' if Param.Z == 2 \
             else True if Param.Z == 3 else [1] if Param.Z == 4 else ['a'] if Param.Z == 5 \
             else [2.5] if Param.Z == 6 else Param.R if Param.Z == 7 else [True])";
    assert_stages(
        &format!("([{b} for x in Param.L] + [{b} for x in Param.L])[0] + 1"),
        "unresolved[float | int]",
        "int",
        "int",
        "2",
    );
}

#[test]
fn call_past_the_form_combination_cap_covers_every_member() {
    // A-F4: past 64 combinations a call took the first signature a union
    // matches (`max` of `int`s), while a run that takes the `0.5` member
    // is a float. It is typed as `derive_return_type` types it, by every
    // signature a member matches. Five members, three arguments: 125.
    let p = "(Param.LS[0] if Task.Param.I == 0 else 1 if Task.Param.I == 1 \
             else 0.5 if Task.Param.I == 2 else True if Task.Param.I == 3 else path('/a'))";
    assert_stages(
        &format!("max({p}, {p}, {p})"),
        "unresolved[float | int]",
        "unresolved[float | int]",
        "float",
        "0.5",
    );
}

// ── Indexing the empty form ──

#[test]
fn indexing_a_possibly_empty_list_gives_the_element_type() {
    // The empty form has no element: indexing it is out of bounds, so it
    // adds nothing to the type.
    let [p8, _, _] = states();
    assert_eq!(
        eval("[x for x in Param.L][0]", &p8).expr_type(),
        tp("unresolved[int]")
    );
    assert_eq!(
        eval("[x if x > 1 else 'a' for x in Param.L][0]", &p8).expr_type(),
        tp("unresolved[int | string]")
    );
    assert_eq!(
        eval("[x if x > 1 else 2.5 for x in Param.L][0]", &p8).expr_type(),
        tp("unresolved[float | int]")
    );
    let expr = "[][0]";
    let st = SymbolTable::new();
    assert_eq!(
        ParsedExpression::new(expr)
            .unwrap()
            .evaluate(&st)
            .unwrap_err()
            .to_string(),
        "Index 0 out of bounds for list of length 0\n  [][0]\n  ~~^~~"
    );
}

#[test]
fn indexing_an_always_empty_list_is_reported_where_it_happens() {
    // F7 of audit round 1: the element of an always-empty list was
    // `noreturn`, which leaked into the next operation's message ("No
    // matching signature for string(noreturn)").
    let expr = "string([None for x in Param.L][0])";
    let [p8, _, _] = states();
    assert_err_in(
        expr,
        &p8,
        "pass 8",
        &[
            "Index out of bounds: the list is always empty\n",
            &format!("  {expr}\n"),
            "         ~~~~~~~~~~~~~~~~~~~~~~~^~~",
        ],
    );
}

#[test]
fn unresolved_index_into_a_concrete_empty_list() {
    // F3 of audit round 6: `[]` indexed by an unresolved int is typed by
    // `__getitem__(list[nulltype], int) -> noreturn`, which leaked into the
    // next call's message ("No matching signature for string(noreturn)").
    let expr = "string([][Task.Param.I])";
    let [p8, g2, rt] = states();
    for (stage, st) in [("pass 8", &p8), ("gate 2", &g2)] {
        assert_err_in(
            expr,
            st,
            stage,
            &[
                "Index out of bounds: the list is always empty\n",
                &format!("  {expr}\n"),
                "         ~~^~~~~~~~~~~~~~",
            ],
        );
    }
    assert_err_in(
        expr,
        &rt,
        "run time",
        &[
            "Index 2 out of bounds for list of length 0\n",
            &format!("  {expr}\n"),
            "         ~~^~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn always_empty_index_in_a_body_that_never_runs() {
    // F2 of audit round 4: in a comprehension body the failing index may
    // never run, so validation does not reject it.
    assert_stages(
        "[[None for y in Param.L][0] for x in Param.L if x > 100]",
        "unresolved[list[nulltype]]",
        "list[nulltype]",
        "list[nulltype]",
        "[]",
    );
}

#[test]
fn indexing_a_comprehension_that_is_always_empty_names_the_failure() {
    // F2 of audit round 7: a comprehension whose every kept element fails
    // is `list[nulltype]`, so indexing it reported only that the list is
    // always empty.
    let [p8, g2, rt] = states();
    let expr = "[x.upper() for x in Param.L][0]";
    assert_err_in(
        expr,
        &p8,
        "pass 8",
        &[
            "Index out of bounds: the list is always empty, since a run that keeps an element \
             fails: upper() is not available for int. Available for: string\n",
            &format!("  {expr}\n"),
            "  ~~~~~~~~~~~~~~~~~~~~~~~~~~~~^~~",
        ],
    );
    for (stage, st) in [("gate 2", &g2), ("run time", &rt)] {
        assert_err_in(
            expr,
            st,
            stage,
            &[
                "upper() is not available for int. Available for: string\n",
                &format!("  {expr}\n"),
                "   ~~^~~~~~~",
            ],
        );
    }
}

#[test]
fn the_failure_named_is_the_indexed_comprehensions() {
    // F3 of audit round 8: the absorbed failure is that of the comprehension
    // being indexed, not of another one in the same expression.
    let expr = "[[x.upper() for x in Param.L], [None for x in Param.L][0]]";
    assert_err_in(
        expr,
        &states()[0],
        "pass 8",
        &[
            "Index out of bounds: the list is always empty\n",
            &format!("  {expr}\n"),
            "                                 ~~~~~~~~~~~~~~~~~~~~~~~^~~",
        ],
    );
}

#[test]
fn a_list_of_noreturn_elements_is_not_always_empty() {
    // F9 of audit round 7: indexing `[fail(...)]`, a one-element list,
    // reported that the list is always empty.
    let expr = "[fail(string(Task.Param.I))][0]";
    let [p8, g2, rt] = states();
    for st in [&p8, &g2] {
        assert_eq!(eval(expr, st).expr_type(), tp("unresolved[noreturn]"));
    }
    assert_err_in(
        expr,
        &rt,
        "run time",
        &[
            "2\n",
            &format!("  {expr}\n"),
            "   ^~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn indexing_a_list_with_a_non_empty_form_is_not_always_empty() {
    // B-F3: only a receiver every form of which is the empty list is
    // "always empty"; `Task.Param.I if c else []` has an `int` form.
    let [p8, _, _] = states();
    assert_eq!(
        eval("(Task.Param.I if Task.Param.C else [])[0]", &p8).expr_type(),
        tp("unresolved[noreturn]")
    );
}
