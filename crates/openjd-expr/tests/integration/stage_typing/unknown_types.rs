// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Operations on values of unknown type (2026-02 Expression Language, Static
//! Type Checking): `any`, type variables static typing leaves unbound, lists
//! of unknown elements, and bare `unresolved` types.

use super::common::*;
use openjd_expr::*;

// ── Calls with an argument of unknown type ──

#[test]
fn operation_on_an_argument_of_unknown_type_is_of_unknown_type() {
    // A-F1: with a non-iterable union member binding `any`, `x[0]` took
    // the first `__getitem__` overload (`string`), so `+ 1` was rejected
    // at validation though every run gives 2. An argument of unknown type
    // matches the first signature in the dispatch order, which says nothing
    // of the run's value: the result is `any`.
    let [p8, _, mut rt] = states();
    rt.set(
        "Param.EL",
        ExprValue::make_list(
            vec![ExprValue::ListInt(vec![1]), ExprValue::ListInt(vec![2, 3])],
            ExprType::list(ExprType::INT),
        )
        .unwrap(),
    )
    .unwrap();
    rt.set("Task.Param.C", ExprValue::Bool(false)).unwrap();
    let expr = "[x for x in (Param.Z if Task.Param.C else Param.EL)][0][0] + 1";
    assert_eq!(eval(expr, &p8).expr_type(), tp("unresolved"));
    assert_eq!(eval(expr, &rt).to_display_string(), "2");
    let mut st = states()[0].clone();
    st.set("U", ExprValue::unresolved(ExprType::ANY)).unwrap();
    for expr in ["U[0] + 1", "U + 1", "len(U) + 1"] {
        assert!(
            ParsedExpression::new(expr)
                .and_then(|p| p.evaluate(&st))
                .is_ok(),
            "{expr}"
        );
    }
    assert_eq!(eval("U[0]", &st).expr_type(), tp("unresolved"));
}

#[test]
fn every_unknown_argument_type_gives_any() {
    // M1, L1: each type-variable code and `any`, through `call` and
    // `derive_return_type`.
    let lib = FunctionLibrary::for_profile(&ExprProfile::current());
    for t in ["any", "T", "T1", "T2", "T3"] {
        for (name, args) in [("len", vec![tp(t)]), ("__add__", vec![tp(t), tp("int")])] {
            assert_eq!(
                lib.derive_return_type(name, &args),
                Some(ExprType::ANY),
                "{name}({t})"
            );
        }
    }
    assert_eq!(
        lib.derive_return_type("__getitem__", &[tp("any"), tp("int")]),
        Some(ExprType::ANY)
    );
}

#[test]
fn type_variable_index_or_membership_item_is_of_unknown_type() {
    // H1: `flatten` of a `list[any]` is a `list[T1]`; its element as an
    // index was "Index must be an integer", and as an `in` item matched no
    // signature, though every run succeeds.
    let [p8, _, rt] = states();
    let flat = "flatten([x for x in (Param.Z if Task.Param.C else Param.EL)])[0]";
    let mut rt = rt;
    rt.set(
        "Param.EL",
        ExprValue::make_list(
            vec![ExprValue::ListInt(vec![1]), ExprValue::ListInt(vec![2, 3])],
            ExprType::list(ExprType::INT),
        )
        .unwrap(),
    )
    .unwrap();
    rt.set("Task.Param.C", ExprValue::Bool(false)).unwrap();
    for (expr, ty, value) in [
        (format!("'abcdefg'[{flat}]"), "unresolved", "b"),
        (format!("{flat} in [1, 2]"), "unresolved[bool]", "true"),
        (format!("Param.L[{flat}] + 1"), "unresolved", "3"),
    ] {
        assert_eq!(eval(&expr, &p8).expr_type(), tp(ty), "{expr}");
        assert_eq!(eval(&expr, &rt).to_display_string(), value, "{expr}");
    }
}

#[test]
fn wrong_argument_count_with_an_argument_of_unknown_type_is_an_error() {
    // F3 / L6: an argument of unknown type does not hide a wrong number of
    // arguments, which fails every run.
    let mut st = states()[0].clone();
    st.set("U", ExprValue::unresolved(ExprType::ANY)).unwrap();
    let expr = "len(U, 1)";
    assert_err_in(
        expr,
        &st,
        "pass 8",
        &[
            "len() takes 1 argument(s), but 2 were given\n",
            &format!("  {expr}\n"),
            "  ^~~~~~~~~",
        ],
    );
}

#[test]
fn argument_of_unknown_type_with_several_arities() {
    // M2: `max` has signatures of several arities; one of the call's arity
    // is enough. L1: too few arguments is still an error.
    let [p8, _, rt] = states_with_matrix();
    let flat = "flatten([x for x in (Param.Z if Task.Param.C else Param.EL)])";
    let expr = format!("max(1.5, 1, {flat}[0])");
    assert_eq!(eval(&expr, &p8).expr_type(), tp("unresolved"));
    assert_eq!(eval(&expr, &rt).to_display_string(), "1.5");
    let mut st = p8.clone();
    st.set("U", ExprValue::unresolved(ExprType::ANY)).unwrap();
    let expr = "startswith(U)";
    assert_err_in(
        expr,
        &st,
        "pass 8",
        &[
            "startswith() takes 2 argument(s), but 1 were given\n",
            &format!("  {expr}\n"),
            "  ^~~~~~~~~~~~",
        ],
    );
}

#[test]
fn argument_type_variable_is_not_captured_by_the_signature() {
    // Loop 3 round 5 A-F1 (a regression): `flatten(list[list[T1]]) ->
    // list[T1]` bound its `T1` to the argument's own `list[T1]` (`flatten`
    // of a `list[any]`), typing a second `flatten` `list[list[T1]]`, so
    // `[flatten(flatten(..)), [1]]` was rejected though a run builds it.
    // An argument's type variable is typed as `any`.
    let [p8, g2, rt] = states();
    let mut p8a = p8.clone();
    p8a.set("Task.Param.A", ExprValue::unresolved(ExprType::ANY))
        .unwrap();
    assert_eq!(
        eval("flatten(flatten([Task.Param.A]))", &p8a).expr_type(),
        tp("unresolved[list[T1]]")
    );
    let expr = "[flatten(flatten([x for x in (Param.EL if Task.Param.C else Param.Z)])), [1]]";
    for (stage, st) in [("pass 8", &p8), ("gate 2", &g2)] {
        assert_eq!(
            eval(expr, st).expr_type(),
            tp("unresolved[list[list[any]]]"),
            "{stage}"
        );
    }
    assert_eq!(eval(expr, &rt).to_display_string(), "[[], [1]]");
}

#[test]
fn membership_in_a_list_of_unknown_elements_is_a_bool() {
    // B-F1 (a regression) and A-F1: `flatten` of a `list[any]` is a
    // `list[T1]`, and a concatenation of no common element type a
    // `list[T3]`: lists of elements of unknown type. `1 in` such a list was
    // rejected though a run succeeds. A call types an argument's type
    // variables as `any` (round 5), and one that matches no signature of
    // its arity with such an argument is `any`; a membership test is a
    // `bool`.
    let mut p8 = SymbolTable::new();
    let mut rt = SymbolTable::new();
    p8.set("Task.Param.X", ExprValue::unresolved(ExprType::ANY))
        .unwrap();
    rt.set("Task.Param.X", ExprValue::Int(3)).unwrap();
    p8.set("Param.N", ExprValue::unresolved(tp("list[list[int]]")))
        .unwrap();
    rt.set(
        "Param.N",
        ExprValue::make_list(vec![], ExprType::list(ExprType::INT)).unwrap(),
    )
    .unwrap();
    p8.set("Param.S", ExprValue::unresolved(tp("list[string]")))
        .unwrap();
    rt.set(
        "Param.S",
        ExprValue::make_list(vec![ExprValue::String("a".into())], ExprType::STRING).unwrap(),
    )
    .unwrap();
    for (expr, value) in [
        ("1 in flatten([Task.Param.X])", "false"),
        ("3 not in flatten([Task.Param.X])", "false"),
        ("[1] in [flatten([Task.Param.X])]", "false"),
        ("'a' in (flatten(Param.N) + Param.S)", "true"),
    ] {
        assert_eq!(
            eval(expr, &p8).expr_type(),
            tp("unresolved[bool]"),
            "{expr}"
        );
        assert_eq!(eval(expr, &rt).to_display_string(), value, "{expr}");
    }
    // A method no signature of which matches a list of unknown elements is
    // `any` too (every run fails here, which validation may accept).
    assert_eq!(
        eval("flatten([Task.Param.X]).upper()", &p8).expr_type(),
        tp("unresolved")
    );
    // A wrong argument count is still an error.
    let expr = "len(flatten([Task.Param.X]), 2)";
    assert_err_in(
        expr,
        &p8,
        "pass 8",
        &[
            "len() takes 1 argument(s), but 2 were given\n",
            &format!("  {expr}\n"),
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

// ── List literals with an element of unknown type ──

#[test]
fn list_literal_with_an_element_of_unknown_type_is_a_list_of_any() {
    // A-F1: `[s, path('/b')]` with `s` of unknown type joined as
    // `list[path]`, so `.upper()` on an element was rejected at validation,
    // though a run with a string gives `list[string]`.
    let [p8, g2, rt] = states_with_nested_strings();
    let xs = "[y for y in (Param.Z if Task.Param.C else Param.LLS[0])][0]";
    for expr in [
        format!("[{xs}.upper(), path('/b')][0].upper()"),
        "[[x.upper(), path('/b')][0].upper() for x in (Param.LLS[0] if not Task.Param.C else Param.Z)][0]".to_string(),
    ] {
        for st in [&p8, &g2] {
            assert!(
                ParsedExpression::new(&expr)
                    .and_then(|p| p.evaluate(st))
                    .is_ok(),
                "{expr}"
            );
        }
        assert_eq!(eval(&expr, &rt).to_display_string(), "A", "{expr}");
    }
}

#[test]
fn list_literal_with_a_type_variable_element_is_a_list_of_any() {
    // A-F1, B-H1: `flatten` of a `list[any]` is a `list[T1]`; beside a typed
    // element in a list literal it was "incompatible types", though every
    // run builds the list. B-M1: also nested one level.
    let [p8, g2, rt] = states_with_matrix();
    let flat = "flatten([x for x in (Param.Z if Task.Param.C else Param.EL)])";
    for (expr, value) in [
        (format!("[{flat}[0], 1]"), "[1, 1]"),
        (format!("[{flat}, Param.L]"), "[[1, 2, 3], [1, 2, 3]]"),
        (format!("[[({flat}[0] + 1)], [1]]"), "[[2], [1]]"),
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
    // B-M2: an index of unknown type (`any`) is accepted.
    let expr = format!("Param.L[({flat}[0] + 1)]");
    assert_eq!(eval(&expr, &p8).expr_type(), tp("unresolved"));
    assert_eq!(eval(&expr, &rt).to_display_string(), "3");
}

#[test]
fn type_variable_elements_join_as_any_on_either_side() {
    // M1, F2, F4: a type-variable element on the right, and two of them
    // (which may be an int and a float), give `list[any]`.
    let [p8, g2, rt] = states_with_matrix();
    let flat = "flatten([x for x in (Param.Z if Task.Param.C else Param.EL)])";
    for (expr, ty, value) in [
        (format!("[1, {flat}[0]]"), "unresolved[list[any]]", "[1, 1]"),
        (
            format!("[Param.L, {flat}]"),
            "unresolved[list[list[any]]]",
            "[[1, 2, 3], [1, 2, 3]]",
        ),
        (
            format!("[[1], [{flat}[0]]]"),
            "unresolved[list[list[any]]]",
            "[[1], [1]]",
        ),
        (
            format!("[{flat}[0], {flat}[1]]"),
            "unresolved[list[any]]",
            "[1, 2]",
        ),
        (format!("[{flat}[0], 1]"), "unresolved[list[any]]", "[1, 1]"),
    ] {
        for st in [&p8, &g2] {
            assert_eq!(eval(&expr, st).expr_type(), tp(ty), "{expr}");
        }
        assert_eq!(eval(&expr, &rt).to_display_string(), value, "{expr}");
    }
}

#[test]
fn lone_list_element_of_unknown_type_is_any() {
    // A-F2: a one-element list literal whose element is a type variable
    // kept it as the element type (`list[T1]`), so `1 in` it was rejected,
    // though two such elements join as `any` (§ Operations on Values of
    // Unknown Type). A call types an argument's type variable as `any`
    // (loop 3 round 5 A-F1), so an element of a `list[T1]` is `any`; a
    // symbol of a type left unbound still reaches the join.
    let mut p8 = SymbolTable::new();
    p8.set("Param.A", ExprValue::unresolved(ExprType::ANY))
        .unwrap();
    p8.set("Param.V", ExprValue::unresolved(tp("T1"))).unwrap();
    let mut rt = SymbolTable::new();
    rt.set("Param.V", ExprValue::Int(1)).unwrap();
    assert_eq!(
        eval("flatten([Param.A])[0]", &p8).expr_type(),
        tp("unresolved")
    );
    assert_eq!(
        eval("[Param.V]", &p8).expr_type(),
        tp("unresolved[list[any]]")
    );
    let expr = "1 in [Param.V]";
    assert_eq!(eval(expr, &p8).expr_type(), tp("unresolved[bool]"));
    assert_eq!(eval(expr, &rt).to_display_string(), "true");
}

#[test]
fn lone_list_element_with_a_member_of_unknown_type_is_any() {
    // B-F1: a lone element whose type is a union with a type variable is
    // `any`, as it is when it joins another element.
    let mut p8 = SymbolTable::new();
    p8.set("Param.V", ExprValue::unresolved(tp("T1"))).unwrap();
    p8.set("Task.Param.C", ExprValue::unresolved(ExprType::BOOL))
        .unwrap();
    assert_eq!(
        eval("Param.V if Task.Param.C else 1", &p8).expr_type(),
        tp("unresolved[T1 | int]")
    );
    assert_eq!(
        eval("[Param.V if Task.Param.C else 1]", &p8).expr_type(),
        tp("unresolved[list[any]]")
    );
}

#[test]
fn list_literal_element_of_a_range_concatenation_with_no_common_type_is_any() {
    // B-F2: `range_expr + list[string]` has no common element type
    // (`list[T2]`), and its element joins with another element as `any`.
    let [p8, _, _] = states();
    assert_eq!(
        eval("[(Param.R + Param.LS)[0], 1]", &p8).expr_type(),
        tp("unresolved[list[any]]")
    );
}

// ── Bare unresolved types ──

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
