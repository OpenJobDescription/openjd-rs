// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! The type of a list comprehension (2026-02 Expression Language §1.3.7)
//! and lists whose element type is a union (§1.2.1).
//!
//! Each case is evaluated in the three symbol states a template sees:
//! template validation ("pass 8": every symbol unresolved), job creation
//! ("gate 2": `Param.*` concrete, `Task.*` unresolved), and run time
//! (everything concrete). A run that keeps no element never evaluates the
//! element expression and is the untyped empty list, so a stage that cannot
//! run the comprehension types it as `list[T | nulltype]`, and as
//! `list[nulltype]` when the filter or body fails to type: only a run that
//! keeps no element can succeed. No stage rejects a comprehension that a
//! later stage's values let succeed.

use openjd_expr::*;

fn tp(s: &str) -> ExprType {
    ExprType::parse(s).unwrap()
}

/// The three symbol states: `Param.L = [1, 2, 3]`, `Param.LS = ['a']`,
/// `Param.R = 1-3`, `Param.Z = 0`, `Param.EL` an empty `list[list[int]]`
/// (as job creation binds an empty `LIST[LIST[INT]]`); `Task.Param.I = 2`,
/// `Task.Param.C = true`.
fn states() -> [SymbolTable; 3] {
    let params = [
        ("Param.L", "list[int]", ExprValue::ListInt(vec![1, 2, 3])),
        (
            "Param.LS",
            "list[string]",
            ExprValue::make_list(vec![ExprValue::String("a".into())], ExprType::STRING).unwrap(),
        ),
        (
            "Param.R",
            "range_expr",
            ExprValue::RangeExpr("1-3".parse().unwrap()),
        ),
        ("Param.Z", "int", ExprValue::Int(0)),
        (
            "Param.EL",
            "list[list[int]]",
            ExprValue::make_list(vec![], ExprType::list(ExprType::INT)).unwrap(),
        ),
    ];
    let tasks = [
        ("Task.Param.I", "int", ExprValue::Int(2)),
        ("Task.Param.C", "bool", ExprValue::Bool(true)),
    ];
    let (mut p8, mut g2, mut rt) = (SymbolTable::new(), SymbolTable::new(), SymbolTable::new());
    for (k, t, v) in params {
        p8.set(k, ExprValue::unresolved(tp(t))).unwrap();
        g2.set(k, v.clone()).unwrap();
        rt.set(k, v).unwrap();
    }
    for (k, t, v) in tasks {
        p8.set(k, ExprValue::unresolved(tp(t))).unwrap();
        g2.set(k, ExprValue::unresolved(tp(t))).unwrap();
        rt.set(k, v).unwrap();
    }
    [p8, g2, rt]
}

fn eval(expr: &str, st: &SymbolTable) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(st))
        .unwrap_or_else(|e| panic!("{expr}: {e}"))
}

/// Assert the type at pass 8 and gate 2, and the type and display value
/// at run time.
fn assert_stages(expr: &str, pass8: &str, gate2: &str, run: &str, run_value: &str) {
    let [p8, g2, rt] = states();
    assert_eq!(eval(expr, &p8).expr_type(), tp(pass8), "pass 8: {expr}");
    assert_eq!(eval(expr, &g2).expr_type(), tp(gate2), "gate 2: {expr}");
    let v = eval(expr, &rt);
    assert_eq!(v.expr_type(), tp(run), "run time: {expr}");
    assert_eq!(v.to_display_string(), run_value, "run time: {expr}");
}

/// Assert `expr` fails in `st` with exactly the message, expression line,
/// and caret in `expected`.
fn assert_err_in(expr: &str, st: &SymbolTable, stage: &str, expected: &[&str]) {
    let e = match ParsedExpression::new(expr).and_then(|p| p.evaluate(st)) {
        Ok(v) => panic!("{stage}: {expr}: expected an error, got {v:?}"),
        Err(e) => e.to_string(),
    };
    let joined = expected.concat();
    assert!(
        e.contains(&joined),
        "{stage}: got:\n{e}\nexpected:\n{joined}"
    );
}

/// Assert `expr` fails with `expected` at every stage.
fn assert_err_all(expr: &str, expected: &[&str]) {
    for (stage, st) in ["pass 8", "gate 2", "run time"].iter().zip(states()) {
        assert_err_in(expr, &st, stage, expected);
    }
}

/// Assert `expr` passes template validation as `pass8` and fails at job
/// creation and run time, where the values reach the failure, with
/// `expected`.
fn assert_fails_once_run(expr: &str, pass8: &str, expected: &[&str]) {
    let [p8, g2, rt] = states();
    assert_eq!(eval(expr, &p8).expr_type(), tp(pass8), "pass 8: {expr}");
    for (stage, st) in [("gate 2", &g2), ("run time", &rt)] {
        assert_err_in(expr, st, stage, expected);
    }
}

/// Assert `expr` has type `stage_type` at template validation and job
/// creation, and fails at run time, where the task values reach the
/// failure, with `expected`.
fn assert_fails_once_run_only(expr: &str, stage_type: &str, expected: &[&str]) {
    let [p8, g2, rt] = states();
    for (stage, st) in [("pass 8", &p8), ("gate 2", &g2)] {
        assert_eq!(
            eval(expr, st).expr_type(),
            tp(stage_type),
            "{stage}: {expr}"
        );
    }
    assert_err_in(expr, &rt, "run time", expected);
}

/// A symbol table with `name` bound to an empty list of `elem`, as job
/// creation binds an empty `LIST[...]` parameter value.
fn typed_empty(name: &str, elem: ExprType) -> SymbolTable {
    let mut st = SymbolTable::new();
    st.set(name, ExprValue::make_list(vec![], elem).unwrap())
        .unwrap();
    st
}

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
            tp("unresolved[list[int | nulltype]]"),
            "{expr}"
        );
    }
    assert_eq!(eval(expr, &rt).to_display_string(), "[6, 7]");
}

/// [`states`] with `Task.Param.A` a `list[any]` at template validation and
/// job creation, and `rt_value` at run time.
fn states_with_any_list(rt_value: ExprValue) -> [SymbolTable; 3] {
    let [mut p8, mut g2, mut rt] = states();
    for st in [&mut p8, &mut g2] {
        st.set("Task.Param.A", ExprValue::unresolved(tp("list[any]")))
            .unwrap();
    }
    rt.set("Task.Param.A", rt_value).unwrap();
    [p8, g2, rt]
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

// ── The loop variable ──

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
fn unresolved_iterable_that_cannot_be_iterated() {
    // An iterable whose value is not known, and whose type cannot be
    // iterated, is not rejected at validation (loop audit 2, round 2): its
    // type may come from typing an operation on values of unknown type
    // (`T3 * 2` is typed `int`), so the loop variable is `any`. A run, which
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

// ── Lists with a union element type (§1.2.1) ──

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
    // typed `any`, and `any[0]` takes the `string` overload, so validation
    // rejected `+ 1` though every run gives an int. The body has nine
    // types, so each comprehension's element type has ten forms, and `+`
    // of two has a hundred.
    let b = "(1 if Param.Z == 0 else 2.5 if Param.Z == 1 else 'a' if Param.Z == 2 \
             else True if Param.Z == 3 else [1] if Param.Z == 4 else ['a'] if Param.Z == 5 \
             else [2.5] if Param.Z == 6 else Param.R if Param.Z == 7 else [True])";
    assert_stages(
        &format!("([{b} for x in Param.L] + [{b} for x in Param.L])[0] + 1"),
        "unresolved[int]",
        "int",
        "int",
        "2",
    );
}

// ── Nesting ──

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
    // every run builds `[[1, 2]]`. (The body here is now a `list[T3]`;
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

// ── Indexing an always-empty list ──

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

// ── Concatenation and sum of typed empty lists ──

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

// ── Audit round 7 ──

/// [`states`] with `Task.Param.LL` a `list[list[int]]`, `[[5, 6]]` at run
/// time.
fn states_with_nested_task_list() -> [SymbolTable; 3] {
    let [mut p8, mut g2, mut rt] = states();
    for st in [&mut p8, &mut g2] {
        st.set(
            "Task.Param.LL",
            ExprValue::unresolved(tp("list[list[int]]")),
        )
        .unwrap();
    }
    rt.set(
        "Task.Param.LL",
        ExprValue::make_list(
            vec![ExprValue::ListInt(vec![5, 6])],
            ExprType::list(ExprType::INT),
        )
        .unwrap(),
    )
    .unwrap();
    [p8, g2, rt]
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

#[test]
fn any_iterable_binds_any() {
    // F2 of audit round 7: the loop variable over an `any` iterable is
    // `any`, so its body types as the run's would.
    let [mut p8, _, _] = states();
    p8.set("U", ExprValue::unresolved(ExprType::ANY)).unwrap();
    assert_eq!(
        eval("[x.upper() for x in U]", &p8).expr_type(),
        tp("unresolved[list[string | nulltype]]")
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

// ── Audit round 8 ──

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
        tp("unresolved[string]")
    );
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

// ── Audit round 9 ──

#[test]
fn bare_unresolved_types_do_not_panic() {
    // F1 of audit round 9: a bare `unresolved` (built through
    // `ExprType::new`) as a comprehension body, or inside a list element
    // union, panicked in union normalization. It reads as `unresolved[any]`.
    let mut st = states()[0].clone();
    let bare = ExprType::new(TypeCode::Unresolved, vec![]);
    st.set("Task.Param.X", ExprValue::unresolved(bare.clone()))
        .unwrap();
    let elem = ExprType::new(
        TypeCode::Union,
        vec![ExprType::INT, ExprType::new(TypeCode::List, vec![bare])],
    );
    st.set(
        "Task.Param.Y",
        ExprValue::unresolved(ExprType::new(TypeCode::List, vec![elem])),
    )
    .unwrap();
    for (expr, ty) in [
        ("[Task.Param.X for y in Param.L]", "unresolved[list[any]]"),
        ("Task.Param.Y[0]", "unresolved[int | list[any]]"),
    ] {
        assert_eq!(eval(expr, &st).expr_type(), tp(ty), "{expr}");
    }
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

// ── Audit round 10 ──

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
fn bare_unresolved_argument_matches_any_signature() {
    // F1 of audit round 10: a bare `unresolved` argument reads as
    // `unresolved[any]`, so calls on it are typed.
    let mut st = states()[0].clone();
    st.set(
        "Task.Param.X",
        ExprValue::unresolved(ExprType::new(TypeCode::Unresolved, vec![])),
    )
    .unwrap();
    for (expr, ty) in [
        ("Task.Param.X + 1", "unresolved[int]"),
        ("len(Task.Param.X)", "unresolved[int]"),
    ] {
        assert_eq!(eval(expr, &st).expr_type(), tp(ty), "{expr}");
    }
}

// ── Audit loop 2, round 1 ──

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
    assert_eq!(eval(expr, &g2).expr_type(), tp("unresolved[list[int]]"));
    assert_eq!(eval(expr, &rt).to_display_string(), "[1, 2, 3]");
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

// ── Audit loop 2, round 3 ──

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
            "unresolved[int]",
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
        tp("unresolved[int]")
    );
}
