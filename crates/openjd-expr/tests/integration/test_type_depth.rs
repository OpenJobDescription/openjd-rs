// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! The `ExprType` nesting depth limit, [`MAX_TYPE_DEPTH`].
//!
//! Every walk over a type (`==`, `Hash`, `Display`, `Drop`, `satisfies`,
//! coercion) is recursive, so a type built deep enough through the Rust
//! API used to overflow the stack and abort the process. The constructors
//! now reject a type deeper than the limit, and these tests check every
//! way to build one, and that a type at the limit is safe to walk on a
//! small stack.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use openjd_expr::{
    ExprType, ExprValue, ExpressionError, ExpressionErrorKind, FunctionLibrary, PathFormat,
    SymbolTable, TypeCode, MAX_TYPE_DEPTH,
};

/// `list[list[...[int]...]]`, `depth` deep (`int` is 1 deep).
fn deep_list(depth: usize) -> ExprType {
    let mut t = ExprType::INT;
    for _ in 1..depth {
        t = ExprType::list(t).unwrap();
    }
    t
}

fn assert_too_deep(r: Result<ExprType, ExpressionError>) {
    let e = r.expect_err("a type deeper than MAX_TYPE_DEPTH must be rejected");
    assert!(
        matches!(e.kind(), ExpressionErrorKind::TypeTooDeep { limit } if *limit == MAX_TYPE_DEPTH),
        "{:?}",
        e.kind()
    );
    assert_eq!(e.message(), "Type nesting depth exceeded limit (160)");
}

#[test]
fn max_type_depth_is_160() {
    assert_eq!(MAX_TYPE_DEPTH, 160);
}

/// Every type constant is 1 deep: wrapping it in lists reaches the limit
/// after exactly `MAX_TYPE_DEPTH - 1` lists.
#[test]
fn every_constant_is_one_level_deep() {
    for c in [
        ExprType::BOOL,
        ExprType::INT,
        ExprType::FLOAT,
        ExprType::STRING,
        ExprType::PATH,
        ExprType::RANGE_EXPR,
        ExprType::NULLTYPE,
        ExprType::ANY,
        ExprType::NORETURN,
        ExprType::T,
        ExprType::T1,
        ExprType::T2,
        ExprType::T3,
    ] {
        let mut t = c.clone();
        let mut wraps = 0;
        while let Ok(next) = ExprType::list(t.clone()) {
            t = next;
            wraps += 1;
        }
        assert_eq!(wraps, MAX_TYPE_DEPTH - 1, "{c}");
    }
}

/// The issue's reproduction: wrapping `int` in 50,000 lists aborted the
/// process with a stack overflow in `coerce`, `==`, `Hash` or `Drop`. Now
/// the wrap that would pass the limit fails and nothing deeper exists.
#[test]
fn building_50_000_levels_stops_at_the_limit() {
    let mut ty = ExprType::INT;
    let mut levels = 1;
    let err = loop {
        match ExprType::list(ty.clone()) {
            Ok(t) => {
                ty = t;
                levels += 1;
            }
            Err(e) => break e,
        }
        assert!(levels <= 50_000);
    };
    assert_eq!(levels, MAX_TYPE_DEPTH);
    assert!(matches!(
        err.kind(),
        ExpressionErrorKind::TypeTooDeep {
            limit: MAX_TYPE_DEPTH
        }
    ));
    // What was built is walkable, and dropping it does not abort.
    let _ = ExprValue::Int(1).coerce(&ty, PathFormat::Posix);
    assert_eq!(ty, ty.clone());
    drop(ty);
}

#[test]
fn list_at_and_over_the_limit() {
    assert!(ExprType::list(deep_list(MAX_TYPE_DEPTH - 1)).is_ok());
    assert_too_deep(ExprType::list(deep_list(MAX_TYPE_DEPTH)));
}

#[test]
fn union_at_and_over_the_limit() {
    let at = ExprType::union(vec![deep_list(MAX_TYPE_DEPTH - 1), ExprType::STRING]).unwrap();
    assert_eq!(at.code(), TypeCode::Union);
    assert_too_deep(ExprType::union(vec![
        deep_list(MAX_TYPE_DEPTH),
        ExprType::STRING,
    ]));
    // A union that normalizes to its one member is that member, so it is
    // no deeper than it.
    assert_eq!(
        ExprType::union(vec![deep_list(MAX_TYPE_DEPTH)]).unwrap(),
        deep_list(MAX_TYPE_DEPTH)
    );
    // A nested union is flattened before the depth is counted.
    let inner = ExprType::union(vec![deep_list(MAX_TYPE_DEPTH - 1), ExprType::PATH]).unwrap();
    let flat = ExprType::union(vec![inner, ExprType::STRING]).unwrap();
    assert_eq!(flat.params().len(), 3);
}

/// `unresolved[T]` is as deep as `T`, so it never passes the limit, and
/// wrapping it in a list counts as wrapping `T`.
#[test]
fn unresolved_adds_no_level() {
    let at = ExprType::unresolved(deep_list(MAX_TYPE_DEPTH)).unwrap();
    assert_eq!(at.code(), TypeCode::Unresolved);
    // `unresolved[unresolved[T]]` is `unresolved[T]`.
    assert_eq!(ExprType::unresolved(at.clone()).unwrap(), at);
    assert_too_deep(ExprType::list(at));
}

#[test]
fn signature_at_and_over_the_limit() {
    let below = deep_list(MAX_TYPE_DEPTH - 1);
    let over = deep_list(MAX_TYPE_DEPTH);
    assert!(ExprType::signature(vec![below.clone()], ExprType::INT).is_ok());
    assert!(ExprType::signature(vec![ExprType::INT], below).is_ok());
    assert_too_deep(ExprType::signature(vec![over.clone()], ExprType::INT));
    assert_too_deep(ExprType::signature(vec![], over));
}

#[test]
fn new_at_and_over_the_limit_for_every_code() {
    let below = deep_list(MAX_TYPE_DEPTH - 1);
    let over = deep_list(MAX_TYPE_DEPTH);
    for code in [
        TypeCode::List,
        TypeCode::Union,
        TypeCode::Unresolved,
        TypeCode::Signature,
        // A code that takes no parameters is built with them as given,
        // through the same check.
        TypeCode::Int,
    ] {
        if code == TypeCode::Unresolved {
            continue; // as deep as its parameter: `unresolved_adds_no_level`
        }
        let params = if code == TypeCode::Union {
            vec![ExprType::STRING]
        } else {
            vec![]
        };
        let mut ok = params.clone();
        ok.push(below.clone());
        assert!(ExprType::new(code, ok).is_ok(), "{code:?} at the limit");
        let mut too_deep = params;
        too_deep.push(over.clone());
        assert_too_deep(ExprType::new(code, too_deep));
    }
}

/// Normalization rearranges a type; the depth is checked on what it
/// builds, so a rearranged type cannot pass the limit.
#[test]
fn normalization_does_not_slip_past_the_limit() {
    // list[unresolved[T]] is unresolved[list[T]], as deep as list[T].
    let u = ExprType::unresolved(deep_list(MAX_TYPE_DEPTH - 1)).unwrap();
    let hoisted = ExprType::list(u).unwrap();
    assert_eq!(hoisted.code(), TypeCode::Unresolved);
    let u = ExprType::unresolved(deep_list(MAX_TYPE_DEPTH)).unwrap();
    assert_too_deep(ExprType::list(u));
    // T | unresolved[S] is unresolved[T | S], as deep as T | S.
    let u = ExprType::unresolved(deep_list(MAX_TYPE_DEPTH - 1)).unwrap();
    let hoisted = ExprType::union(vec![ExprType::INT, u]).unwrap();
    assert_eq!(hoisted.code(), TypeCode::Unresolved);
    let u = ExprType::unresolved(deep_list(MAX_TYPE_DEPTH)).unwrap();
    assert_too_deep(ExprType::union(vec![ExprType::INT, u]));
}

/// Substituting below the outermost parameter rebuilds every level, so a
/// binding deep inside the type is checked too.
#[test]
fn nested_substitution_past_the_limit_is_an_error() {
    let sig = ExprType::parse("(T1) -> list[list[T1]]").unwrap();
    assert!(sig
        .resolve_call(&[deep_list(MAX_TYPE_DEPTH - 2)])
        .unwrap()
        .is_some());
    assert_too_deep(
        sig.resolve_call(&[deep_list(MAX_TYPE_DEPTH - 1)])
            .map(Option::unwrap),
    );
    // Here the inner `list[T1]` is already too deep: its error must reach
    // the caller rather than leave the outer list without an element.
    assert_too_deep(
        sig.resolve_call(&[deep_list(MAX_TYPE_DEPTH)])
            .map(Option::unwrap),
    );
}

/// Operations that build a type from other types are checked too:
/// substituting a deep binding into `list[T1]` adds a level.
#[test]
fn substitution_past_the_limit_is_an_error() {
    let sig = ExprType::parse("(T1) -> list[T1]").unwrap();
    let below = deep_list(MAX_TYPE_DEPTH - 1);
    let over = deep_list(MAX_TYPE_DEPTH);
    assert_eq!(
        sig.resolve_call(std::slice::from_ref(&below)).unwrap(),
        Some(ExprType::list(below).unwrap())
    );
    assert_too_deep(
        sig.resolve_call(std::slice::from_ref(&over))
            .map(Option::unwrap),
    );
    let bindings = HashMap::from([(TypeCode::TypeVarT1, over)]);
    assert_too_deep(sig.sig_return().substitute(&bindings));
}

fn identity(
    _: &mut dyn openjd_expr::EvalContext,
    a: &[ExprValue],
) -> Result<ExprValue, ExpressionError> {
    Ok(a[0].clone())
}

#[test]
fn derive_return_type_past_the_limit_is_an_error() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("wrap", "(T1) -> list[T1]", identity)
        .unwrap();
    lib.register_sig("__property_wrap__", "(T1) -> list[T1]", identity)
        .unwrap();
    let over = deep_list(MAX_TYPE_DEPTH);
    assert_too_deep(lib.derive_return_type("wrap", &[over]).map(Option::unwrap));
    // An argument of several forms: the union of the forms' returns.
    let lists =
        |leaf: ExprType| (2..MAX_TYPE_DEPTH - 1).fold(leaf, |t, _| ExprType::list(t).unwrap());
    let forms = ExprType::union(vec![lists(ExprType::INT), lists(ExprType::STRING)]).unwrap();
    assert!(lib
        .derive_return_type("wrap", std::slice::from_ref(&forms))
        .unwrap()
        .is_some());
    let forms = ExprType::union(vec![
        ExprType::list(lists(ExprType::INT)).unwrap(),
        ExprType::list(lists(ExprType::STRING)).unwrap(),
    ])
    .unwrap();
    assert_too_deep(
        lib.derive_return_type("wrap", std::slice::from_ref(&forms))
            .map(Option::unwrap),
    );
    assert_too_deep(lib.get_property_type(&forms, "wrap").map(Option::unwrap));
    // A type-only call through the library: the same error.
    let mut st = SymbolTable::new();
    st.set("U", ExprValue::unresolved(forms)).unwrap();
    st.set("X", ExprValue::unresolved(deep_list(MAX_TYPE_DEPTH)))
        .unwrap();
    for expr in ["wrap(X)", "X.wrap", "U.wrap"] {
        let parsed = openjd_expr::ParsedExpression::new(expr).unwrap();
        let err = parsed.with_library(&lib).evaluate(&[&st]).expect_err(expr);
        assert!(
            matches!(err.kind(), ExpressionErrorKind::TypeTooDeep { .. }),
            "{expr}: {:?}",
            err.kind()
        );
    }
}

/// An expression over a symbol whose type is at the limit fails with the
/// error rather than aborting.
#[test]
fn evaluating_past_the_limit_is_an_error() {
    let mut st = SymbolTable::new();
    st.set("X", ExprValue::unresolved(deep_list(MAX_TYPE_DEPTH)))
        .unwrap();
    let err = openjd_expr::ParsedExpression::new("[X]")
        .unwrap()
        .evaluate(&st)
        .expect_err("a list of a type at the limit is too deep");
    assert!(
        matches!(err.kind(), ExpressionErrorKind::TypeTooDeep { limit: 160 }),
        "{:?}",
        err.kind()
    );
    assert_eq!(err.message(), "Type nesting depth exceeded limit (160)");
}

/// Unresolved symbols for the evaluation tests: `X` at the limit, `XS` its
/// `string` twin, `C` and `C2` unknown `bool`s, `N` an unknown `int`, and
/// `M` a list whose elements join with `X`'s into a union a level deeper.
fn deep_symbols() -> SymbolTable {
    let mut st = SymbolTable::new();
    let deep_strings =
        (1..MAX_TYPE_DEPTH).fold(ExprType::STRING, |t, _| ExprType::list(t).unwrap());
    let members = ExprType::parse("list[list[nulltype] | list[any]]").unwrap();
    // `list[int | list^(limit-3)[int]]`, at the limit: a comprehension
    // over it adds `| nulltype` to a union one level down.
    let v = ExprType::list(
        ExprType::union(vec![ExprType::INT, deep_list(MAX_TYPE_DEPTH - 2)]).unwrap(),
    )
    .unwrap();
    for (name, t) in [
        ("X", deep_list(MAX_TYPE_DEPTH)),
        ("XS", deep_strings),
        ("C", ExprType::BOOL),
        ("C2", ExprType::BOOL),
        ("N", ExprType::INT),
        ("M", members),
        ("V", v),
    ] {
        st.set(name, ExprValue::unresolved(t)).unwrap();
    }
    // A concrete list whose element type hint is at the limit, which
    // only the enum variant can build.
    st.set(
        "LL",
        ExprValue::ListList(vec![], deep_list(MAX_TYPE_DEPTH), 0),
    )
    .unwrap();
    st
}

fn eval_err(expr: &str, st: &SymbolTable) -> ExpressionError {
    openjd_expr::ParsedExpression::new(expr)
        .unwrap()
        .evaluate(st)
        .expect_err(expr)
}

/// Every evaluator path that builds a type from deep operands reports the
/// limit rather than a wrong type.
#[test]
fn evaluator_paths_past_the_limit_are_errors() {
    let st = deep_symbols();
    for expr in [
        // A union of two branches at the limit.
        "X if C else XS",
        // A list literal whose element join is a level deeper than either
        // element (the join's list arm, then the literal's list type).
        "[M, X]",
        // A comprehension's list type: `list[T | nulltype]` over a `T` at the
        // limit.
        "[x if C else 1 for x in X]",
        // Slicing with an unknown bound builds `list[hint]`.
        "LL[N:]",
        // A boolean operator's outcomes are a union.
        "C or X",
        // A type-only call with an argument of several forms is the union
        // of their returns (`M`'s forms join with `X` a level deeper).
        "M + X",
        // A concatenation builds `list[hint]` for each operand.
        "LL + []",
        // A comprehension over a union element type adds `| nulltype` to
        // it.
        "[V for x in [1] if C]",
    ] {
        let err = eval_err(expr, &st);
        assert!(
            matches!(err.kind(), ExpressionErrorKind::TypeTooDeep { limit: 160 }),
            "{expr}: {:?}",
            err.kind()
        );
    }
}

/// Static typing evaluates a conditional's branches, a boolean operator's
/// operands and a comprehension's body speculatively, absorbing a failure
/// a run might skip. A type past the limit is not such a failure: the
/// sub-expression has no type, so it is reported, not absorbed into a
/// type that leaves it out.
#[test]
fn speculative_evaluation_does_not_absorb_the_limit() {
    let st = deep_symbols();
    for expr in [
        "1 if C else ([X] if C2 else 1)",
        "[X] if C else 1",
        "C and [X]",
        "C or [X]",
        "[[X] for x in [1, 2] if C]",
        "[len([[x]]) for x in X]",
        "[1 for x in X if len([[x]]) > 0]",
    ] {
        let err = eval_err(expr, &st);
        assert!(
            matches!(err.kind(), ExpressionErrorKind::TypeTooDeep { limit: 160 }),
            "{expr}: {:?}",
            err.kind()
        );
    }
}

/// `make_list` reports a list of lists whose element type is at the limit,
/// which would have no type, whether that type is the hint of an empty
/// list or the elements' join. A hint the list does not use is ignored.
#[test]
fn make_list_whose_element_type_is_at_the_limit_is_an_error() {
    let too_deep = |r: Result<ExprValue, ExpressionError>| {
        let e = r.expect_err("the list's element type is at the limit");
        assert!(matches!(
            e.kind(),
            ExpressionErrorKind::TypeTooDeep { limit: 160 }
        ));
    };
    too_deep(ExprValue::make_list(vec![], deep_list(MAX_TYPE_DEPTH)));
    // An element whose type (`list[U]`) is at the limit.
    let u = ExprType::union(vec![deep_list(MAX_TYPE_DEPTH - 2), ExprType::STRING]).unwrap();
    let elem = ExprValue::ListList(vec![], u, 0);
    too_deep(ExprValue::make_list(vec![elem], ExprType::NULLTYPE));
    // One level below, the type is exact.
    let below = deep_list(MAX_TYPE_DEPTH - 1);
    assert_eq!(
        ExprValue::make_list(vec![], below.clone())
            .unwrap()
            .expr_type(),
        ExprType::list(below).unwrap()
    );
    // A hint the list does not use is not checked.
    let ints = ExprValue::make_list(
        vec![ExprValue::Int(1), ExprValue::Int(2)],
        deep_list(MAX_TYPE_DEPTH),
    )
    .unwrap();
    assert_eq!(ints.expr_type(), ExprType::parse("list[int]").unwrap());
    let deep_union = ExprType::union(vec![deep_list(MAX_TYPE_DEPTH), ExprType::STRING]);
    assert!(deep_union.is_err());
    let empty = ExprValue::make_list(
        vec![],
        ExprType::union(vec![deep_list(MAX_TYPE_DEPTH - 1), ExprType::STRING]).unwrap(),
    )
    .unwrap();
    assert_eq!(
        empty.expr_type(),
        ExprType::parse("list[nulltype]").unwrap()
    );
}

/// The deepest types one expression can build from a template come from
/// nesting list comprehensions over an unresolved value: each is
/// `list[T | nulltype]`, two levels over its element. The deepest nesting
/// the parser accepts types without reaching the limit, and so does the
/// deepest `list[list[int]]` parameter iterated at every level.
#[test]
fn the_deepest_comprehension_nesting_types_below_the_limit() {
    fn depth(t: &ExprType) -> usize {
        let below = t.params().iter().map(depth).max().unwrap_or(0);
        match (t.code(), t.params()) {
            (TypeCode::Unresolved, [_]) => below,
            _ => 1 + below,
        }
    }
    let mut st = SymbolTable::new();
    st.set("N", ExprValue::unresolved(ExprType::INT)).unwrap();
    st.set("C", ExprValue::unresolved(ExprType::BOOL)).unwrap();
    st.set(
        "L",
        ExprValue::unresolved(ExprType::parse("list[list[int]]").unwrap()),
    )
    .unwrap();
    for iterable in ["[1]", "L"] {
        let mut expr = "N".to_string();
        let mut deepest = 0;
        for i in 0.. {
            let next = format!("[{expr} for v{i} in {iterable} if C]");
            let Ok(parsed) = openjd_expr::ParsedExpression::new(&next) else {
                break;
            };
            let t = parsed
                .evaluate(&st)
                .unwrap_or_else(|e| panic!("{} nested comprehensions: {e}", i + 1))
                .expr_type();
            deepest = deepest.max(depth(&t));
            expr = next;
        }
        assert!(
            (120..MAX_TYPE_DEPTH).contains(&deepest),
            "over {iterable}: {deepest}"
        );
    }
}

/// Only a one-parameter `unresolved` adds no level. A bare or
/// many-parameter one, which only `new` builds, counts a level, so a chain
/// of them stops at the limit.
#[test]
fn many_parameter_unresolved_counts_a_level() {
    let mut t = ExprType::INT;
    let mut levels = 1;
    loop {
        match ExprType::new(TypeCode::Unresolved, vec![t.clone(), ExprType::INT]) {
            Ok(next) => {
                t = next;
                levels += 1;
            }
            Err(e) => {
                assert!(matches!(
                    e.kind(),
                    ExpressionErrorKind::TypeTooDeep { limit: 160 }
                ));
                break;
            }
        }
        assert!(levels <= MAX_TYPE_DEPTH, "the chain passed the limit");
    }
    assert_eq!(levels, MAX_TYPE_DEPTH);
}

/// A `ListList` whose element type hint is at the limit has no list type
/// one level up, so `expr_type` reads the elements' type as unknown. An
/// unresolved value's type is as deep as its constraint, so it is exact.
#[test]
fn expr_type_of_a_value_holding_a_type_at_the_limit() {
    let at = deep_list(MAX_TYPE_DEPTH);
    assert_eq!(
        ExprValue::ListList(vec![], at.clone(), 0).expr_type(),
        ExprType::parse("list[any]").unwrap()
    );
    assert_eq!(
        ExprValue::Unresolved(at.clone()).expr_type(),
        ExprType::unresolved(at).unwrap()
    );
    // One level below the limit, the type is exact.
    let below = deep_list(MAX_TYPE_DEPTH - 1);
    assert_eq!(
        ExprValue::ListList(vec![], below.clone(), 0).expr_type(),
        ExprType::list(below).unwrap()
    );
}

fn hash_of(t: &ExprType) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

/// Types at the limit, of each shape a walk recurses through.
fn types_at_the_limit() -> Vec<ExprType> {
    let lists = deep_list(MAX_TYPE_DEPTH);
    // Alternating union and list levels, each union with two members.
    let mut unions = ExprType::INT;
    for i in 1..MAX_TYPE_DEPTH {
        unions = if i % 2 == 1 {
            ExprType::union(vec![unions, ExprType::STRING]).unwrap()
        } else {
            ExprType::list(unions).unwrap()
        };
    }
    // Nested signatures, and an unresolved type over a deep list.
    let mut sigs = ExprType::T1;
    for _ in 1..MAX_TYPE_DEPTH {
        sigs = ExprType::signature(vec![sigs, ExprType::STRING], ExprType::INT).unwrap();
    }
    // The most levels a type at the limit nests: a signature level and an
    // `unresolved` (which counts as none) in turn.
    let mut alternating = ExprType::INT;
    for _ in 1..MAX_TYPE_DEPTH {
        let u = ExprType::unresolved(alternating).unwrap();
        alternating = ExprType::signature(vec![u], ExprType::INT).unwrap();
    }
    let alternating = ExprType::unresolved(alternating).unwrap();
    let unresolved = ExprType::unresolved(deep_list(MAX_TYPE_DEPTH)).unwrap();
    vec![lists, unions, sigs, alternating, unresolved]
}

/// Every recursive walk over a type is safe up to the limit: on a 2 MiB
/// stack (a test thread's default), in whatever profile the tests are
/// built, each walk completes on types at the limit.
#[test]
fn walks_over_types_at_the_limit_fit_a_2_mib_stack() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            for t in types_at_the_limit() {
                let copy = t.clone();
                assert_eq!(t, copy);
                assert_eq!(hash_of(&t), hash_of(&copy));
                let shown = t.to_string();
                assert!(!shown.is_empty());
                assert!(!format!("{t:?}").is_empty());
                let _ = serde_json::to_string(&t).unwrap();
                let _ = t.satisfies(&copy);
                let _ = t.satisfies(&ExprType::ANY);
                let _ = t.match_type(&copy);
                let _ = t.is_symbolic();
                let _ = t.is_concrete();
                let bindings = HashMap::from([(TypeCode::TypeVarT1, ExprType::INT)]);
                let _ = t.substitute(&bindings).unwrap();
                let _ = ExprValue::Int(1).coerce(&t, PathFormat::Posix);
                let _ = ExprValue::unresolved(t.clone()).coerce(&copy, PathFormat::Posix);
                let _ = ExprType::union(vec![t.clone(), copy.clone()]);
                drop(copy);
                drop(t);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

// ── Every type-building path that can pass the limit ──

fn deep_strings(depth: usize) -> ExprType {
    (1..depth).fold(ExprType::STRING, |t, _| ExprType::list(t).unwrap())
}

fn l(t: ExprType) -> ExprType {
    ExprType::list(t).unwrap()
}

fn u(types: Vec<ExprType>) -> ExprType {
    ExprType::union(types).unwrap()
}

/// `list[A | B]` at the limit, whose forms `list[A]` and `list[B]` are a
/// level below it.
fn list_of_union_at_the_limit() -> ExprType {
    l(u(vec![
        deep_list(MAX_TYPE_DEPTH - 2),
        deep_strings(MAX_TYPE_DEPTH - 2),
    ]))
}

/// A union of 65 members, past the 64 form combinations a call expands,
/// one of them a level below the limit.
fn union_past_the_form_cap() -> ExprType {
    let mut members: Vec<ExprType> = (1..=64).map(deep_list).collect();
    members.push(deep_strings(MAX_TYPE_DEPTH - 1));
    u(members)
}

/// `list[list[B]] | list[int | list[A]]` at the limit, which merges into
/// `list[list[B] | int | list[A]]`, a level deeper.
fn list_union_that_merges_past_the_limit() -> ExprType {
    u(vec![
        l(l(deep_list(MAX_TYPE_DEPTH - 3))),
        l(u(vec![ExprType::INT, l(deep_strings(MAX_TYPE_DEPTH - 4))])),
    ])
}

fn reach_symbols() -> SymbolTable {
    let members = ExprType::parse("list[nulltype] | list[any]").unwrap();
    let mut st = SymbolTable::new();
    for (name, t) in [
        ("W", list_union_that_merges_past_the_limit()),
        ("LAB", list_of_union_at_the_limit()),
        ("U65", union_past_the_form_cap()),
        ("C", ExprType::BOOL),
    ] {
        st.set(name, ExprValue::unresolved(t)).unwrap();
    }
    for (name, hint) in [
        ("LL", deep_list(MAX_TYPE_DEPTH)),
        ("LM", members),
        ("LX", deep_list(MAX_TYPE_DEPTH - 1)),
    ] {
        st.set(name, ExprValue::ListList(vec![], hint, 0)).unwrap();
    }
    st
}

fn identity_lib(sigs: &[(&str, &str)]) -> FunctionLibrary {
    let mut lib = FunctionLibrary::new();
    for (name, sig) in sigs {
        lib.register_sig(name, sig, identity).unwrap();
    }
    lib
}

fn assert_eval_too_deep(expr: &str, lib: Option<&FunctionLibrary>, st: &SymbolTable) {
    let parsed = openjd_expr::ParsedExpression::new(expr).unwrap();
    let r = match lib {
        Some(lib) => parsed.with_library(lib).evaluate(&[st]),
        None => parsed.evaluate(st),
    };
    match r {
        Err(e) => assert!(
            matches!(e.kind(), ExpressionErrorKind::TypeTooDeep { limit: 160 }),
            "{expr}: {}",
            e.message()
        ),
        Ok(v) => panic!("{expr}: Ok({})", v.expr_type()),
    }
}

/// Concatenations build `list[hint]` for either operand, a `range_expr`
/// operand included, and join the operands' element types.
#[test]
fn concatenation_past_the_limit_is_an_error() {
    let st = reach_symbols();
    for expr in [
        "[] + LL",
        "LL + range_expr('1-3')",
        "range_expr('1-3') + LL",
        // `list[nulltype] | list[any]` joined with a list a level below
        // the limit is a union of two lists, one level over it.
        "LM + LX",
        // The union arm of `merge_list_union` builds a list a level
        // deeper than its input.
        "W + []",
    ] {
        assert_eval_too_deep(expr, None, &st);
    }
}

#[test]
fn comprehension_over_a_merging_union_past_the_limit_is_an_error() {
    assert_eval_too_deep("[W for x in [1] if C]", None, &reach_symbols());
}

/// Type-only calls through the library: per-form returns, the union of
/// several forms' returns, the fallback past the form cap, and a signature
/// whose return adds two levels.
#[test]
fn library_typing_past_the_limit_is_an_error() {
    let st = reach_symbols();
    let wrap = identity_lib(&[("wrap", "(T1) -> list[T1]")]);
    let wrap2 = identity_lib(&[("wrap2", "(T1) -> list[list[T1]]")]);
    for (lib, name, arg) in [
        (&wrap, "wrap", list_of_union_at_the_limit()),
        (&wrap, "wrap", union_past_the_form_cap()),
        (&wrap2, "wrap2", list_of_union_at_the_limit()),
        (&wrap2, "wrap2", union_past_the_form_cap()),
    ] {
        assert_too_deep(
            lib.derive_return_type(name, std::slice::from_ref(&arg))
                .map(Option::unwrap),
        );
    }
    for (lib, expr) in [
        (&wrap, "wrap(LAB)"),
        (&wrap, "wrap(U65)"),
        (&wrap2, "wrap2(LAB)"),
        (&wrap2, "wrap2(U65)"),
    ] {
        assert_eval_too_deep(expr, Some(lib), &st);
    }
}

/// Static typing can be driven past the limit by a contrived expression:
/// wrapping comprehension bodies in `([x] if C else 1)` adds two levels a
/// wrap, compounding along the chain. Such an expression fails typing
/// with the limit's error (a deliberate hardening limit, see
/// type-system.md § Depth limit), quickly and without overflowing a 2 MiB
/// stack, while a run of it succeeds. Three comprehensions with 26 wraps
/// each type below the limit and with 27 past it; ten with 25 type as
/// about 500 deep without the limit.
#[test]
fn contrived_expression_past_the_limit_fails_typing_but_runs() {
    fn wrap(x: &str, w: usize) -> String {
        (0..w).fold(x.to_string(), |e, _| format!("([{e}] if C else 1)"))
    }
    fn chain(n: usize, w: usize) -> String {
        (0..n).fold("[N]".to_string(), |e, k| {
            format!("[{} for y{k} in {e}]", wrap(&format!("y{k}"), w))
        })
    }
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut typing = SymbolTable::new();
            typing
                .set("N", ExprValue::unresolved(ExprType::INT))
                .unwrap();
            typing
                .set("C", ExprValue::unresolved(ExprType::BOOL))
                .unwrap();
            let mut run = SymbolTable::new();
            run.set("N", ExprValue::Int(1)).unwrap();
            run.set("C", ExprValue::Bool(false)).unwrap();
            // Below the limit, typing succeeds.
            let below = openjd_expr::ParsedExpression::new(&chain(3, 26)).unwrap();
            assert!(below.evaluate(&typing).is_ok());
            for (n, w) in [(3, 27), (10, 25)] {
                let parsed = openjd_expr::ParsedExpression::new(&chain(n, w)).unwrap();
                let start = std::time::Instant::now();
                let err = parsed.evaluate(&typing).expect_err("types past the limit");
                assert!(
                    start.elapsed() < std::time::Duration::from_secs(10),
                    "n={n} w={w}: {:?}",
                    start.elapsed()
                );
                assert!(
                    matches!(err.kind(), ExpressionErrorKind::TypeTooDeep { limit: 160 }),
                    "n={n} w={w}: {:?}",
                    err.kind()
                );
                assert_eq!(
                    parsed.evaluate(&run).unwrap().expr_type(),
                    ExprType::parse("list[int]").unwrap()
                );
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

/// The cached depth sits in padding: an `ExprType` is no larger than its
/// code and parameters alone.
#[test]
fn the_cached_depth_does_not_grow_expr_type() {
    assert_eq!(
        std::mem::size_of::<ExprType>(),
        std::mem::size_of::<(TypeCode, Vec<ExprType>)>()
    );
}

/// `int | list[A] | list[B]` at the limit, `A` and `B` 158 deep: merging
/// its lists (`int | list[A | B]`) adds a level.
fn union_with_lists_that_merge_past_the_limit() -> ExprType {
    u(vec![
        ExprType::INT,
        l(deep_list(MAX_TYPE_DEPTH - 2)),
        l((1..MAX_TYPE_DEPTH - 2).fold(ExprType::FLOAT, |t, _| l(t))),
    ])
}

/// Concatenation and union typing merge an operand's or a return's list
/// members, which can add a level.
#[test]
fn merging_list_members_past_the_limit_is_an_error() {
    let u3 = union_with_lists_that_merge_past_the_limit();
    let mut st = reach_symbols();
    st.set("U", ExprValue::unresolved(u3.clone())).unwrap();
    st.set("X", ExprValue::unresolved(deep_list(MAX_TYPE_DEPTH)))
        .unwrap();
    for expr in ["X + U", "U + [1]", "U + X"] {
        assert_eval_too_deep(expr, None, &st);
    }
    let lib = openjd_expr::FunctionLibrary::for_profile(&openjd_expr::ExprProfile::current());
    assert_too_deep(
        lib.derive_return_type("__add__", &[u3.clone(), u3.clone()])
            .map(Option::unwrap),
    );
    // A signature whose return type is the union of its parameters'.
    let pair = identity_lib(&[("pair", "(T1, T2) -> T1 | T2")]);
    assert_too_deep(
        pair.derive_return_type("pair", &[u3, list_union_that_merges_past_the_limit()])
            .map(Option::unwrap),
    );
}

/// Coercing an unresolved value to a deep target can build a result type
/// past the limit. `coerce` reports the limit's message, and static
/// typing reports the error and never absorbs it in a branch.
#[test]
fn coercing_an_unresolved_value_past_the_limit_is_an_error() {
    // `nulltype | float | list^157[int]`, 159 deep; the target `list[X]`
    // is at the limit.
    let x = u(vec![
        ExprType::NULLTYPE,
        ExprType::FLOAT,
        deep_list(MAX_TYPE_DEPTH - 2),
    ]);
    let target = l(x);
    let source = ExprType::parse("list[int] | list[nulltype]").unwrap();
    let err = ExprValue::unresolved(source.clone())
        .coerce(&target, PathFormat::Posix)
        .expect_err("the coerced type is past the limit");
    assert_eq!(err, "Type nesting depth exceeded limit (160)");
    let err = ExprValue::unresolved(union_with_lists_that_merge_past_the_limit())
        .coerce(&list_of_union_at_the_limit(), PathFormat::Posix)
        .expect_err("the coerced type is past the limit");
    assert_eq!(err, "Type nesting depth exceeded limit (160)");
    let mut st = SymbolTable::new();
    st.set("A", ExprValue::unresolved(source)).unwrap();
    st.set("C", ExprValue::unresolved(ExprType::BOOL)).unwrap();
    for expr in ["A", "A if C else []"] {
        let parsed = openjd_expr::ParsedExpression::new(expr).unwrap();
        let err = parsed
            .with_target_type(&target)
            .evaluate(&[&st])
            .expect_err(expr);
        assert!(
            matches!(err.kind(), ExpressionErrorKind::TypeTooDeep { limit: 160 }),
            "{expr}: {:?}",
            err.kind()
        );
    }
}

/// A list type of irregular shape: a spine of `depth` lists from a scalar
/// leaf, with a third of the levels a union of the list and one or two
/// scalars or `list[nulltype]`, picked by a fixed xorshift sequence.
fn irregular_list_type(seed: u64, depth: usize) -> ExprType {
    let mut state = seed;
    let mut next = |k: u64| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % k) as usize
    };
    let scalars = [
        ExprType::INT,
        ExprType::FLOAT,
        ExprType::BOOL,
        ExprType::PATH,
        ExprType::STRING,
        ExprType::RANGE_EXPR,
        l(ExprType::NULLTYPE),
    ];
    let mut t = scalars[next(7)].clone();
    for _ in 0..depth {
        let list = l(t);
        t = if next(3) == 0 {
            let mut members = vec![list];
            for _ in 0..1 + next(2) {
                members.push(scalars[next(7)].clone());
            }
            u(members)
        } else {
            list
        };
    }
    t
}

#[test]
fn concatenation_join_past_the_limit_is_an_error() {
    // Typing `A[0] + B` joins the operands' element types. Against
    // `list[list[nulltype]] | list[nulltype]`, joining the union members
    // with an irregular operand 155 deep builds a type past the limit.
    // The regular shapes tried (a union at every level, every other level,
    // and so on) stay within it: this shape was found by search.
    let a0 = irregular_list_type(817_566_216_853, 120);
    let fnv1a = a0
        .to_string()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    assert_eq!(fnv1a, 0x32e7_1301_10a2_92d9, "the generator changed");
    let mut st = SymbolTable::new();
    st.set("A", ExprValue::unresolved(l(a0))).unwrap();
    st.set(
        "B",
        ExprValue::unresolved(
            ExprType::parse("list[list[list[nulltype]] | list[nulltype]]").unwrap(),
        ),
    )
    .unwrap();
    for expr in ["A[0] + B", "B + A[0]"] {
        assert_eval_too_deep(expr, None, &st);
    }
}

/// More paths whose join or merge recurses and can add a level below the
/// top: a comprehension's list type merging an iterable's list members at
/// depth, concatenation typing past the form cap, and the list-element
/// join of union members inside a list.
#[test]
fn nested_joins_and_merges_past_the_limit_are_errors() {
    // `merge_list_union` recurses into a list's element: the
    // comprehension `[[x] for x in U]` over a union of lists of every
    // depth from 2 to 6 and one of strings 1 to 6 levels below the limit.
    // At 3 levels below, only the recursive merge's error makes the type
    // too deep: the later checks would accept it.
    for below in 1..=6 {
        let mut members: Vec<ExprType> = (0..64).map(|i| deep_list((i % 5) + 2)).collect();
        members.push(deep_strings(MAX_TYPE_DEPTH - below));
        let mut st = SymbolTable::new();
        st.set("U", ExprValue::unresolved(u(members))).unwrap();
        assert_eval_too_deep("[[x] for x in U]", None, &st);
    }

    // A concatenation typed past the form cap refines its return type
    // by the operands' join.
    let lib = openjd_expr::FunctionLibrary::for_profile(&openjd_expr::ExprProfile::latest());
    let u65 = union_past_the_form_cap();
    assert_too_deep(
        lib.derive_return_type("__add__", &[l(ExprType::INT), u65.clone()])
            .map(Option::unwrap),
    );
    let mut st = SymbolTable::new();
    st.set("U", ExprValue::unresolved(u65)).unwrap();
    st.set("L", ExprValue::unresolved(l(ExprType::INT)))
        .unwrap();
    assert_eval_too_deep("U + L", None, &st);

    // Joining `list[nulltype] | list[list[nulltype] | list[list[nulltype]
    // | list[any]]]` with `list[list^d[int]]`: the members' joins inside
    // the lists build a union of lists a level deeper at each depth.
    let nl = ExprType::parse("list[nulltype]").unwrap();
    let la = ExprType::parse("list[any]").unwrap();
    let a = u(vec![nl.clone(), l(u(vec![nl.clone(), la]))]);
    let hint_a = u(vec![nl, l(a)]);
    for d in [MAX_TYPE_DEPTH - 3, MAX_TYPE_DEPTH - 2] {
        let hint_b = l(deep_list(d));
        let mut st = SymbolTable::new();
        st.set("A", ExprValue::ListList(vec![], hint_a.clone(), 0))
            .unwrap();
        st.set("B", ExprValue::ListList(vec![], hint_b.clone(), 0))
            .unwrap();
        st.set("UA", ExprValue::unresolved(l(hint_a.clone())))
            .unwrap();
        st.set("UB", ExprValue::unresolved(l(hint_b))).unwrap();
        for expr in ["A + B", "[UA, UB]"] {
            assert_eval_too_deep(expr, None, &st);
        }
    }
}
