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
    assert_eq!(e.message(), "Type nesting depth exceeded limit (64)");
}

#[test]
fn max_type_depth_is_64() {
    assert_eq!(MAX_TYPE_DEPTH, 64);
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
        ExpressionErrorKind::TypeTooDeep { limit: 64 }
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
    let over = deep_list(MAX_TYPE_DEPTH);
    assert_too_deep(lib.derive_return_type("wrap", &[over]).map(Option::unwrap));
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
        matches!(err.kind(), ExpressionErrorKind::TypeTooDeep { limit: 64 }),
        "{:?}",
        err.kind()
    );
    assert_eq!(err.message(), "Type nesting depth exceeded limit (64)");
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
