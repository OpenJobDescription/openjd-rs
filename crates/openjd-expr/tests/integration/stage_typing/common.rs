// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Shared states and assertions for the stage-typing tests.
//!
//! Each case is evaluated in the three symbol states a template sees:
//! template validation ("pass 8": every symbol unresolved), job creation
//! ("gate 2": `Param.*` concrete, `Task.*` unresolved), and run time
//! (everything concrete).

use openjd_expr::*;

pub(crate) fn tp(s: &str) -> ExprType {
    ExprType::parse(s).unwrap()
}

/// The three symbol states: `Param.L = [1, 2, 3]`, `Param.LS = ['a']`,
/// `Param.R = 1-3`, `Param.Z = 0`, `Param.EL` an empty `list[list[int]]`
/// (as job creation binds an empty `LIST[LIST[INT]]`); `Task.Param.I = 2`,
/// `Task.Param.C = true`.
pub(crate) fn states() -> [SymbolTable; 3] {
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

pub(crate) fn eval(expr: &str, st: &SymbolTable) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(st))
        .unwrap_or_else(|e| panic!("{expr}: {e}"))
}

/// Assert the type at pass 8 and gate 2, and the type and display value
/// at run time.
pub(crate) fn assert_stages(expr: &str, pass8: &str, gate2: &str, run: &str, run_value: &str) {
    let [p8, g2, rt] = states();
    assert_eq!(eval(expr, &p8).expr_type(), tp(pass8), "pass 8: {expr}");
    assert_eq!(eval(expr, &g2).expr_type(), tp(gate2), "gate 2: {expr}");
    let v = eval(expr, &rt);
    assert_eq!(v.expr_type(), tp(run), "run time: {expr}");
    assert_eq!(v.to_display_string(), run_value, "run time: {expr}");
}

/// Assert `expr` fails in `st` with an error containing the message,
/// expression line, and caret in `expected`, in that order and adjacent.
pub(crate) fn assert_err_in(expr: &str, st: &SymbolTable, stage: &str, expected: &[&str]) {
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
pub(crate) fn assert_err_all(expr: &str, expected: &[&str]) {
    for (stage, st) in ["pass 8", "gate 2", "run time"].iter().zip(states()) {
        assert_err_in(expr, &st, stage, expected);
    }
}

/// Assert `expr` passes template validation as `pass8` and fails at job
/// creation and run time, where the values reach the failure, with
/// `expected`.
pub(crate) fn assert_fails_once_run(expr: &str, pass8: &str, expected: &[&str]) {
    let [p8, g2, rt] = states();
    assert_eq!(eval(expr, &p8).expr_type(), tp(pass8), "pass 8: {expr}");
    for (stage, st) in [("gate 2", &g2), ("run time", &rt)] {
        assert_err_in(expr, st, stage, expected);
    }
}

/// Assert `expr` has type `stage_type` at template validation and job
/// creation, and fails at run time, where the task values reach the
/// failure, with `expected`.
pub(crate) fn assert_fails_once_run_only(expr: &str, stage_type: &str, expected: &[&str]) {
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
pub(crate) fn typed_empty(name: &str, elem: ExprType) -> SymbolTable {
    let mut st = SymbolTable::new();
    st.set(name, ExprValue::make_list(vec![], elem).unwrap())
        .unwrap();
    st
}

/// [`states`] with `Task.Param.A` a `list[any]` at template validation and
/// job creation, and `rt_value` at run time.
pub(crate) fn states_with_any_list(rt_value: ExprValue) -> [SymbolTable; 3] {
    let [mut p8, mut g2, mut rt] = states();
    for st in [&mut p8, &mut g2] {
        st.set("Task.Param.A", ExprValue::unresolved(tp("list[any]")))
            .unwrap();
    }
    rt.set("Task.Param.A", rt_value).unwrap();
    [p8, g2, rt]
}

/// [`states`] with `Task.Param.LL` a `list[list[int]]`, `[[5, 6]]` at run
/// time.
pub(crate) fn states_with_nested_task_list() -> [SymbolTable; 3] {
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

/// [`states`] with `Param.LLS` a `list[list[string]]`, `[['a']]` at run
/// time, and `Task.Param.C` false there.
pub(crate) fn states_with_nested_strings() -> [SymbolTable; 3] {
    let [mut p8, mut g2, mut rt] = states();
    p8.set("Param.LLS", ExprValue::unresolved(tp("list[list[string]]")))
        .unwrap();
    let lls = ExprValue::make_list(
        vec![ExprValue::make_list(vec![ExprValue::String("a".into())], ExprType::STRING).unwrap()],
        ExprType::list(ExprType::STRING),
    )
    .unwrap();
    g2.set("Param.LLS", lls.clone()).unwrap();
    rt.set("Param.LLS", lls).unwrap();
    rt.set("Task.Param.C", ExprValue::Bool(false)).unwrap();
    [p8, g2, rt]
}

/// [`states`] with `Param.EL = [[1], [2, 3]]` and `Task.Param.C` false at
/// run time.
pub(crate) fn states_with_matrix() -> [SymbolTable; 3] {
    let [p8, mut g2, mut rt] = states();
    let el = ExprValue::make_list(
        vec![ExprValue::ListInt(vec![1]), ExprValue::ListInt(vec![2, 3])],
        ExprType::list(ExprType::INT),
    )
    .unwrap();
    g2.set("Param.EL", el.clone()).unwrap();
    rt.set("Param.EL", el).unwrap();
    rt.set("Task.Param.C", ExprValue::Bool(false)).unwrap();
    [p8, g2, rt]
}
