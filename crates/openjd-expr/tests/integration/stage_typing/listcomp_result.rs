// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! The type of a list comprehension (2026-02 Expression Language §1.3.7): the
//! result over known and unknown elements, an empty result, a filter or body
//! that fails to type, an element expression with a union type, and nesting.
//!
//! A run that keeps no element never evaluates the element expression and is
//! the untyped empty list, so a stage that cannot run the comprehension types
//! it as `list[T | nulltype]`, and as `list[nulltype]` when the filter or body
//! fails to type: only a run that keeps no element can succeed.

use super::common::*;
use openjd_expr::*;

// ── A run that keeps no element ──

#[test]
fn empty_result_is_the_untyped_empty_list() {
    assert_stages(
        "[x for x in Param.L if x > 100]",
        "unresolved[list[int | nulltype]]",
        "list[nulltype]",
        "list[nulltype]",
        "[]",
    );
    assert_stages(
        "[[x, x] for x in Param.L if x > 100]",
        "unresolved[list[list[int] | nulltype]]",
        "list[nulltype]",
        "list[nulltype]",
        "[]",
    );
}

#[test]
fn empty_result_concatenates_with_any_list() {
    // Item 33 of the resolved-value-limits design: these succeed at job
    // creation and run time, so template validation accepts them too.
    assert_stages(
        "[x for x in Param.L if x > 100] + ['a']",
        "unresolved[list[string]]",
        "list[string]",
        "list[string]",
        r#"["a"]"#,
    );
    assert_stages(
        "[[x for x in Param.L if x > 100], ['a']]",
        "unresolved[list[list[string]]]",
        "list[list[string]]",
        "list[list[string]]",
        r#"[[], ["a"]]"#,
    );
}

#[test]
fn empty_result_dispatches_as_the_empty_list() {
    // `sum` has no `list[string]` signature, but has one for the empty
    // list: a run that keeps nothing sums to 0, one that keeps a string
    // fails.
    assert_stages(
        "sum([s for s in Param.LS if s == 'z'])",
        "unresolved[int]",
        "int",
        "int",
        "0",
    );
    let expr = "sum([s for s in Param.LS])";
    assert_fails_once_run(
        expr,
        "unresolved[int]",
        &[
            "No matching signature for sum(list[string])\n",
            &format!("  {expr}\n"),
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn empty_literal_iterable_gives_the_untyped_empty_list() {
    // The body never runs and is not type-checked: `x * 2` with `x` of no
    // type is never evaluated.
    for expr in ["[x for x in []]", "[x * 2 for x in []]"] {
        assert_stages(
            expr,
            "list[nulltype]",
            "list[nulltype]",
            "list[nulltype]",
            "[]",
        );
    }
}

#[test]
fn unresolved_iterable_that_can_only_be_empty() {
    // F2 of audit rounds 5 and 6: the body of a comprehension whose
    // iterable can only be `[]` is neither run nor type-checked, whatever
    // the stage: `1` does not make it a `list[int | nulltype]`, nor
    // `x.upper()` an error.
    for expr in [
        "[1 for x in ([] if Task.Param.C else [])]",
        "[x.upper() for x in ([] if Task.Param.C else [])]",
    ] {
        assert_stages(
            expr,
            "unresolved[list[nulltype]]",
            "unresolved[list[nulltype]]",
            "list[nulltype]",
            "[]",
        );
    }
    // So indexing it fails on every run.
    let expr = "[1 for x in ([] if Task.Param.C else [])][0]";
    let [p8, g2, rt] = states();
    let caret = "  ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~^~~";
    for (stage, st) in [("pass 8", &p8), ("gate 2", &g2)] {
        assert_err_in(
            expr,
            st,
            stage,
            &[
                "Index out of bounds: the list is always empty\n",
                &format!("  {expr}\n"),
                caret,
            ],
        );
    }
    assert_err_in(
        expr,
        &rt,
        "run time",
        &[
            "Index 0 out of bounds for list of length 0\n",
            &format!("  {expr}\n"),
            caret,
        ],
    );
}

// ── A filter or body that fails to type ──

#[test]
fn body_type_error_surfaces_when_an_element_reaches_it() {
    // Template validation cannot know whether `Param.L` will be empty, and
    // an empty run succeeds, so it types the comprehension as the only
    // run that can succeed: the empty list.
    let expr = "[x.upper() for x in Param.L]";
    assert_fails_once_run(
        expr,
        "unresolved[list[nulltype]]",
        &[
            "upper() is not available for int. Available for: string\n",
            &format!("  {expr}\n"),
            "   ~~^~~~~~~",
        ],
    );
    // A run that keeps no element never reaches it.
    assert_stages(
        "[x.upper() for x in Param.L if x > 100]",
        "unresolved[list[nulltype]]",
        "list[nulltype]",
        "list[nulltype]",
        "[]",
    );
}

#[test]
fn body_type_error_with_an_unresolved_filter() {
    // Job creation cannot decide the filter (`Task.Param.C` is unresolved),
    // so it types the comprehension as validation does; the run decides.
    let expr = "[x.upper() for x in Param.L if Task.Param.C]";
    let [p8, g2, rt] = states();
    assert_eq!(
        eval(expr, &p8).expr_type(),
        tp("unresolved[list[nulltype]]")
    );
    assert_eq!(
        eval(expr, &g2).expr_type(),
        tp("unresolved[list[nulltype]]")
    );
    assert_err_in(
        expr,
        &rt,
        "run time",
        &[
            "upper() is not available for int. Available for: string\n",
            &format!("  {expr}\n"),
            "   ~~^~~~~~~",
        ],
    );
    let mut off = rt.clone();
    off.set("Task.Param.C", ExprValue::Bool(false)).unwrap();
    assert_eq!(eval(expr, &off).to_display_string(), "[]");
}

#[test]
fn body_failure_on_an_element_before_an_unresolved_filter() {
    // F2 of audit rounds 5 and 6: the elements are evaluated in order until
    // a filter is unresolved (§1.3.7). An element before that whose filter
    // is concretely true is kept by every run, so its body's failure is an
    // error at every stage.
    let expr = "[x.upper() for x in [1, 2] if x == 1 or Task.Param.C]";
    assert_err_all(
        expr,
        &[
            "upper() is not available for int. Available for: string\n",
            &format!("  {expr}\n"),
            "   ~~^~~~~~~",
        ],
    );
    // The elements after it are not evaluated: the comprehension is typed
    // as a whole, so the failure is absorbed, and the run reports it.
    let expr = "[x.upper() for x in [2, 1] if x == 1 or Task.Param.C]";
    assert_fails_once_run_only(
        expr,
        "unresolved[list[nulltype]]",
        &[
            "upper() is not available for int. Available for: string\n",
            &format!("  {expr}\n"),
            "   ~~^~~~~~~",
        ],
    );
    // A kept element's value does not decide the result.
    let [p8, g2, _] = states();
    let expr = "[x for x in [1, 2] if x == 1 or Task.Param.C]";
    for st in [&p8, &g2] {
        assert_eq!(
            eval(expr, st).expr_type(),
            tp("unresolved[list[int | nulltype]]")
        );
    }
}

#[test]
fn undefined_symbol_in_a_body_surfaces_when_an_element_reaches_it() {
    // As in a conditional branch that may not run, a name the body cannot
    // resolve fails only a run that evaluates the body.
    let expr = "[Param.Typo for x in Param.L]";
    assert_fails_once_run(
        expr,
        "unresolved[list[nulltype]]",
        &[
            "Undefined variable: 'Param.Typo'. Did you mean one of: Param.EL, Param.L, Param.LS, Param.R, Param.Z\n",
            &format!("  {expr}\n"),
            "   ~~~~~~^~~~",
        ],
    );
}

#[test]
fn body_value_error_on_concrete_values() {
    // A body that fails on operands whose values are known fails every run
    // that keeps an element; template validation does not know whether one
    // will.
    let expr = "[1 // 0 for x in Param.L]";
    assert_fails_once_run(
        expr,
        "unresolved[list[nulltype]]",
        &["Division by zero\n", &format!("  {expr}\n"), "   ~~^~~~"],
    );
    assert_stages(
        "[1 // 0 for x in Param.L if x > 100]",
        "unresolved[list[nulltype]]",
        "list[nulltype]",
        "list[nulltype]",
        "[]",
    );
    // `1 // Param.Z` types as `int` at validation; at job creation, where
    // `Param.Z = 0`, the filter keeps nothing and the body never runs.
    assert_stages(
        "[1 // Param.Z for x in Param.L if Param.Z != 0]",
        "unresolved[list[int | nulltype]]",
        "list[nulltype]",
        "list[nulltype]",
        "[]",
    );
    // Validation accepts what the empty run accepts.
    assert_stages(
        "[1 // Param.Z for x in Param.L if Param.Z != 0].join(', ')",
        "unresolved[string]",
        "string",
        "string",
        "",
    );
}

#[test]
fn filter_that_fails_to_type_over_unknown_elements() {
    // B-F2: over an unresolved iterable, a filter that fails (here on
    // known operands) is not an error at validation: the comprehension is
    // the empty list, and the run reports the failure.
    let expr = "[x for x in Param.L if fail('m')]";
    assert_fails_once_run(
        expr,
        "unresolved[list[nulltype]]",
        &[
            "m\n",
            &format!("  {expr}\n"),
            "                         ^~~~~~~~~",
        ],
    );
    let expr = "[x for x in Param.L if fail('m')][0]";
    assert_err_in(
        expr,
        &states()[0],
        "pass 8",
        &[
            "Index out of bounds: the list is always empty, since a run that keeps an element \
             fails: m\n",
            &format!("  {expr}\n"),
            "  ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~^~~",
        ],
    );
}

#[test]
fn null_guarded_body_never_runs_on_null() {
    // H2 of audit round 2: a null-guarded body must not fail a run whose
    // guard keeps nothing.
    let expr = "[Opt.upper() for x in Param.L if Opt != None]";
    let mut p8 = states()[0].clone();
    p8.set("Opt", ExprValue::unresolved(tp("string?"))).unwrap();
    let mut rt = states()[2].clone();
    rt.set("Opt", ExprValue::Null).unwrap();
    assert_eq!(
        eval(expr, &p8).expr_type(),
        tp("unresolved[list[string | nulltype]]")
    );
    let v = eval(expr, &rt);
    assert_eq!(v.expr_type(), tp("list[nulltype]"));
    assert_eq!(v.to_display_string(), "[]");
}

#[test]
fn budget_exceedance_while_typing_propagates() {
    // A failure the typing absorbs is a failure of the body on every run;
    // the operations and memory are spent in this evaluation regardless.
    let [p8, _, _] = states();
    let err = ParsedExpression::new("[x + x + x + x + x + x + x + x for x in Param.L]")
        .and_then(|p| {
            p.with_memory_limit(usize::MAX)
                .with_operation_limit(4)
                .evaluate_with_metrics(&[&p8])
        })
        .unwrap_err();
    assert!(
        matches!(
            err.kind(),
            ExpressionErrorKind::OperationLimitExceeded { .. }
        ),
        "got {err}"
    );
}

// ── The filter's type ──

#[test]
fn type_variable_filter_can_be_a_boolean() {
    // F1 of audit round 6: a filter typed `T3` (an element of a
    // concatenation with a `list[any]`) was taken as never a boolean. Over
    // an unresolved iterable the comprehension was `list[nulltype]` and its
    // index an error; over a concrete one the filter was an error.
    let [p8, g2, rt] =
        states_with_any_list(ExprValue::make_list(vec![], ExprType::NULLTYPE).unwrap());
    // (Its loop variable is `any`, round 10.)
    let expr = "[x for x in (Task.Param.A + [Task.Param.C]) if x][0]";
    for st in [&p8, &g2] {
        assert_eq!(eval(expr, st).expr_type(), tp("unresolved"), "{expr}");
    }
    assert_eq!(eval(expr, &rt).to_display_string(), "true");
    let expr = "[x for x in [1, 2] if (Task.Param.A + [Task.Param.C])[0]]";
    for st in [&p8, &g2] {
        assert_eq!(
            eval(expr, st).expr_type(),
            tp("unresolved[list[int | nulltype]]"),
            "{expr}"
        );
    }
    assert_eq!(eval(expr, &rt).to_display_string(), "[1, 2]");
    // A `T1` filter (`flatten` of a `list[T3]`).
    let expr = "[x for x in Param.L if flatten(Task.Param.A + [[Task.Param.C]])[0]]";
    for st in [&p8, &g2] {
        assert_eq!(
            eval(expr, st).expr_type(),
            tp("unresolved[list[int | nulltype]]"),
            "{expr}"
        );
    }
}

#[test]
fn non_boolean_filter_surfaces_when_an_element_reaches_it() {
    let expr = "[x for x in Param.L if x]";
    assert_fails_once_run(
        expr,
        "unresolved[list[nulltype]]",
        &[
            "List comprehension filter must be a boolean, got int\n",
            &format!("  {expr}\n"),
            "                         ^",
        ],
    );
    assert_stages(
        "[x for x in Param.EL if x]",
        "unresolved[list[nulltype]]",
        "list[nulltype]",
        "list[nulltype]",
        "[]",
    );
}

#[test]
fn noreturn_filter_is_not_a_non_boolean() {
    // F10 of audit round 7: a filter that is `noreturn` for an element
    // (`x > 1 or fail(...)` with `x = 1`) was reported as not a boolean at
    // job creation; the run reports the failure itself.
    let expr = "[x for x in Param.L if x > 1 or fail(string(Task.Param.I))]";
    let [p8, g2, rt] = states();
    for st in [&p8, &g2] {
        assert_eq!(
            eval(expr, st).expr_type(),
            tp("unresolved[list[int | nulltype]]")
        );
    }
    assert_err_in(
        expr,
        &rt,
        "run time",
        &[
            "2\n",
            &format!("  {expr}\n"),
            "                                  ^~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

// ── Element expression with a union type ──

#[test]
fn union_element_type_empty_result_is_the_untyped_empty_list() {
    assert_stages(
        "[x if x > 1 else 'a' for x in Param.L if x > 100]",
        "unresolved[list[int | string | nulltype]]",
        "list[nulltype]",
        "list[nulltype]",
        "[]",
    );
    // So it concatenates with any list.
    assert_stages(
        "[x if x > 1 else 'a' for x in Param.L if x > 100] + [2.5]",
        "unresolved[list[float]]",
        "list[float]",
        "list[float]",
        "[2.5]",
    );
}

#[test]
fn union_element_type_non_empty_result_follows_the_values() {
    // Mixed int and float promote, as in a list literal.
    assert_stages(
        "[x if x > 1 else 2.5 for x in Param.L]",
        "unresolved[list[float | int | nulltype]]",
        "list[float]",
        "list[float]",
        "[2.5, 2.0, 3.0]",
    );
    // A single member's values give that member's list.
    assert_stages(
        "[x if x > 0 else 'a' for x in Param.L]",
        "unresolved[list[int | string | nulltype]]",
        "list[int]",
        "list[int]",
        "[1, 2, 3]",
    );
    // Mixed members with no common type fail as `[1, 'a']` does.
    let expr = "[x if x > 1 else 'a' for x in Param.L]";
    let [_, g2, rt] = states();
    for (stage, st) in [("gate 2", &g2), ("run time", &rt)] {
        assert_err_in(
            expr,
            st,
            stage,
            &[
                "make_list expected string element, got int\n",
                &format!("  {expr}\n"),
                "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
            ],
        );
    }
}

#[test]
fn nullable_element_type_drops_null() {
    // A list cannot hold `null`: a run that produces one fails, so the
    // element type is `int`.
    assert_stages(
        "[x if x > 1 else None for x in Param.L if x > 1]",
        "unresolved[list[int | nulltype]]",
        "list[int]",
        "list[int]",
        "[2, 3]",
    );
}

#[test]
fn element_expression_with_no_value_gives_the_untyped_empty_list() {
    // Only a run with no elements succeeds.
    assert_stages(
        "[fail(string(x)) for x in Param.L if x > 100]",
        "unresolved[list[nulltype]]",
        "list[nulltype]",
        "list[nulltype]",
        "[]",
    );
    assert_stages(
        "[None for x in Param.L if x > 100]",
        "unresolved[list[nulltype]]",
        "list[nulltype]",
        "list[nulltype]",
        "[]",
    );
}

// ── Nesting ──

#[test]
fn element_type_that_is_a_list_of_lists_fails_when_an_element_reaches_it() {
    // The comprehension would be a three-level list, which no value can be
    // (§1.2.1); only a run that keeps no element succeeds.
    assert_stages(
        "[[x] for x in Param.EL]",
        "unresolved[list[nulltype]]",
        "list[nulltype]",
        "list[nulltype]",
        "[]",
    );
    let expr = "[[[x]] for x in Param.L]";
    assert_fails_once_run(
        expr,
        "unresolved[list[nulltype]]",
        &[
            "Lists may be nested at most 2 levels deep\n",
            &format!("  {expr}\n"),
            "  ^~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
    // A union element type with a form that nests correctly: a run
    // producing only that form builds a valid list.
    let [p8, _, _] = states();
    assert_eq!(
        eval("[[[x]] if x > 100 else [x] for x in Param.L]", &p8).expr_type(),
        tp("unresolved[list[list[int | list[int]] | nulltype]]")
    );
}

#[test]
fn type_variable_element_type_is_not_a_list_of_lists() {
    // F1 of audit round 5: a body typed with a type variable does not say
    // what its values are; counting `list[list[T1]]` as a list of lists made
    // the comprehension `list[nulltype]` and its index an error, though
    // every run builds `[[1, 2]]`. (The body here is now a `list[any]`;
    // `comprehension_too_deep`'s unit test covers `list[list[T1]]`.)
    let [p8, g2, rt] =
        states_with_any_list(ExprValue::make_list(vec![], ExprType::list(ExprType::INT)).unwrap());
    let expr = "[(Task.Param.A + [[x]])[0] + [2] for x in Param.L][0]";
    for st in [&p8, &g2] {
        assert_eq!(
            eval(expr, st).expr_type(),
            tp("unresolved[list[any]]"),
            "{expr}"
        );
    }
    assert_eq!(eval(expr, &rt).to_display_string(), "[1, 2]");
}
