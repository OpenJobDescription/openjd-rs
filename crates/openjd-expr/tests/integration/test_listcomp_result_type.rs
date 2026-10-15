// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! The type of a list comprehension (2026-02 Expression Language §1.3.7)
//! and lists whose element type is a union (§1.2.1).
//!
//! Each case is evaluated in the three symbol states a template sees:
//! template validation ("pass 8": every symbol unresolved), job creation
//! ("gate 2": `Param.*` concrete, `Task.*` unresolved), and run time
//! (everything concrete). A comprehension's type follows from its element
//! expression's type, not from how many elements its filter keeps, so the
//! stages agree on an empty result.

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

// ── Element expression with a single type ──

#[test]
fn empty_result_is_a_typed_empty_list() {
    assert_stages(
        "[x for x in Param.L if x > 100]",
        "unresolved[list[int]]",
        "list[int]",
        "list[int]",
        "[]",
    );
    assert_stages(
        "[[x, x] for x in Param.L if x > 100]",
        "unresolved[list[list[int]]]",
        "list[list[int]]",
        "list[list[int]]",
        "[]",
    );
    assert_stages(
        "[[r] for r in [Param.R] if r == 'x']",
        "unresolved[list[list[range_expr]]]",
        "list[list[range_expr]]",
        "list[list[range_expr]]",
        "[]",
    );
}

#[test]
fn empty_result_concatenates_by_its_element_type() {
    // Promotes as `[1] + [2.5]` does.
    assert_stages(
        "[x for x in Param.L if x > 100] + [2.5]",
        "unresolved[list[T3]]",
        "list[float]",
        "list[float]",
        "[2.5]",
    );
    // Was `['a']` at job creation and run time: the empty result was the
    // untyped empty list.
    let expr = "[x for x in Param.L if x > 100] + ['a']";
    let [_, g2, rt] = states();
    for (stage, st) in [("gate 2", &g2), ("run time", &rt)] {
        assert_err_in(
            expr,
            st,
            stage,
            &[
                "Cannot concatenate list[int] and list[string]\n",
                &format!("  {expr}\n"),
                "  ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~^~~~~~~",
            ],
        );
    }
}

#[test]
fn empty_result_dispatches_by_its_element_type() {
    // `sum` has no `list[string]` signature, empty or not.
    let expr = "sum([s for s in Param.LS if s == 'z'])";
    assert_err_all(
        expr,
        &[
            "No matching signature for sum(list[string])\n",
            &format!("  {expr}\n"),
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
    assert_stages(
        "sum([x for x in Param.L if x > 100])",
        "unresolved[int]",
        "int",
        "int",
        "0",
    );
}

#[test]
fn empty_result_inside_a_list_literal() {
    // Item 33 of the resolved-value-limits design: template validation
    // rejected this while job creation and run time accepted it; now every
    // stage rejects it, as `[[1], ['a']]` is rejected.
    let expr = "[[x for x in Param.L if x > 100], ['a']]";
    assert_err_all(
        expr,
        &[
            "List literal contains incompatible types: list[int] and list[string]\n",
            &format!("  {expr}\n"),
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
    assert_stages(
        "[[x for x in Param.L if x > 100], [3]]",
        "unresolved[list[list[int]]]",
        "list[list[int]]",
        "list[list[int]]",
        "[[], [3]]",
    );
}

#[test]
fn empty_result_type_does_not_read_symbol_values() {
    // The element type is the one template validation gives the element
    // expression, with every symbol read as its type: `Param.Z = 0` is
    // never divided by, as at run time, where the filter keeps nothing.
    assert_stages(
        "[1 // Param.Z for x in Param.L if Param.Z != 0]",
        "unresolved[list[int]]",
        "list[int]",
        "list[int]",
        "[]",
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
        "unresolved[list[T3]]",
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
    // element type is `int`, and an empty result is a typed `list[int]`.
    assert_stages(
        "[x if x > 1 else None for x in Param.L if x > 1]",
        "unresolved[list[int]]",
        "list[int]",
        "list[int]",
        "[2, 3]",
    );
    assert_stages(
        "[x if x > 1 else None for x in Param.L if x > 100]",
        "unresolved[list[int]]",
        "list[int]",
        "list[int]",
        "[]",
    );
}

#[test]
fn element_expression_with_no_value_gives_the_untyped_empty_list() {
    // Only a run with no elements succeeds. (The argument depends on `x`:
    // a literal `fail('x')` fails template validation, which evaluates it.)
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
    assert_eq!(eval(expr, &p8).expr_type(), tp("unresolved[list[string]]"));
    assert_eq!(eval(expr, &strs).to_display_string(), r#"["A"]"#);
}

#[test]
fn unresolved_iterable_that_cannot_be_iterated() {
    // Was accepted at template validation with the loop variable bound as
    // `int`; every run fails.
    let expr = "[x for x in Task.Param.I]";
    assert_err_all(expr, &["Cannot iterate over int"]);
}

// ── Lists with a union element type (§1.2.1) ──

#[test]
fn indexing_a_possibly_empty_list_gives_the_element_type() {
    // The empty form has no element: indexing it is out of bounds, so it
    // adds nothing to the type.
    let [p8, _, _] = states();
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
        &["upper() is not available for list[float | int | nulltype]. Available for: string"],
    );
}

// ── Nesting with range_expr elements ──

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

// ── Audit round 1 ──

#[test]
fn empty_list_overloads_apply_only_to_the_empty_list() {
    // F1: an argument of unknown element type (`list[T3]` from an
    // unresolved `+`) matched `__getitem__(list[nulltype], int) -> noreturn`
    // first, so validation rejected `[0] + 1`; only the empty list selects
    // that overload at run time.
    assert_stages(
        "(Param.L + Param.L)[0] + 1",
        "unresolved[int]",
        "int",
        "int",
        "2",
    );
    let [p8, _, _] = states();
    let mut any_list = SymbolTable::new();
    any_list
        .set("U", ExprValue::unresolved(tp("list[any]")))
        .unwrap();
    assert_eq!(eval("U[0]", &any_list).expr_type(), tp("unresolved"));
    assert_eq!(
        eval("min(Param.L + Param.L)", &p8).expr_type(),
        tp("unresolved[int]")
    );
}

#[test]
fn element_type_that_is_a_list_of_lists_is_an_error() {
    // F3: the comprehension would be a three-level list, which no value
    // can be (§1.2.1); every run, empty or not, fails.
    for (expr, caret) in [
        ("[[x] for x in Param.EL]", "  ^~~~~~~~~~~~~~~~~~~~~~~"),
        (
            "[[[x]] for x in Param.L if x > 100]",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ),
    ] {
        assert_err_all(
            expr,
            &[
                "Lists may be nested at most 2 levels deep\n",
                &format!("  {expr}\n"),
                caret,
            ],
        );
    }
    // A union element type with a form that nests correctly is not an
    // error: a run producing only that form builds a valid list.
    let [p8, _, _] = states();
    assert_eq!(
        eval("[[[x]] if x > 100 else [x] for x in Param.L]", &p8).expr_type(),
        tp("unresolved[list[list[int | list[int]] | nulltype]]")
    );
}

#[test]
fn indexing_an_always_empty_list_is_reported_where_it_happens() {
    // F7: the element of an always-empty list was `noreturn`, which leaked
    // into the next operation's message ("No matching signature for
    // string(noreturn)").
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
fn concatenation_joins_inner_empty_lists() {
    // F10: `[[]] + [[1]]` was rejected ("Cannot concatenate
    // list[list[nulltype]] and list[list[int]]"), so an empty `list[nulltype]`
    // body, now an empty `list[list[nulltype]]`, could no longer be
    // concatenated. Concatenation uses the list literal's join (§2.1.3).
    assert_stages(
        "[[] for x in Param.L if x > 100] + [[1]]",
        "unresolved[list[T3]]",
        "list[list[int]]",
        "list[list[int]]",
        "[[1]]",
    );
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
