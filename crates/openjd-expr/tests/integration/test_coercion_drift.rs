// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Drift test between concrete coercion and its type-level mirror.
//!
//! `ExprValue::coerce` decides with the value's payload in hand; coercing an
//! `ExprValue::Unresolved` runs the same two steps (satisfaction, then
//! conversion) at the type level. The two tables are maintained by hand, so
//! this test sweeps a catalog of sample values against a catalog of target
//! types and mechanically checks the invariants that keep them consistent
//! (specs/expr/values.md §"Unresolved Values"):
//!
//! 1. **Never-reject**: if the concrete coercion succeeds, the type-level
//!    coercion must succeed too. The reverse is deliberately allowed — the
//!    type level cannot see the payload, so it may accept a pair the payload
//!    later rejects, like `unresolved[string]` to `int` versus `"hello"`.
//! 2. **Soundness**: every successful result — the concrete value's type or
//!    the narrowed constraint — satisfies the target.
//! 3. **Over-approximation**: the narrowed constraint describes every value
//!    the payload could produce, so a successful concrete result's type
//!    always satisfies it. For a non-union target only one destination
//!    exists, so the constraint is exactly the concrete result's type.

use openjd_expr::types::TypeCode;
use openjd_expr::value::Float64;
use openjd_expr::{ExprType, ExprValue, PathFormat};

fn t(s: &str) -> ExprType {
    ExprType::parse(s).unwrap()
}

/// One sample value per concrete type, plus payload variants (whole and
/// fractional floats; strings whose text parses as int, float, bool, and
/// range_expr, or as none of them; empty and non-empty lists) so that
/// payload-dependent conversion rules are exercised from both sides.
fn sample_values() -> Vec<ExprValue> {
    let from =
        |s: &str, ty: &str| ExprValue::from_str_coerce(s, &t(ty), PathFormat::Posix).unwrap();
    let float = |v: f64| ExprValue::Float(Float64::new(v).unwrap());
    let list = |values: Vec<ExprValue>, elem: &str| ExprValue::make_list(values, t(elem)).unwrap();
    vec![
        ExprValue::Null,
        ExprValue::Bool(true),
        ExprValue::Int(7),
        float(2.0),
        float(2.5),
        ExprValue::String("7".to_string()),
        ExprValue::String("7.5".to_string()),
        ExprValue::String("true".to_string()),
        ExprValue::String("1-3".to_string()),
        ExprValue::String("hello".to_string()),
        from("out/dir", "path"),
        from("1-3", "range_expr"),
        list(vec![], "int"),
        list(vec![ExprValue::Int(1), ExprValue::Int(2)], "int"),
        list(vec![float(1.0), float(1.5)], "float"),
        list(vec![ExprValue::String("a".to_string())], "string"),
        list(vec![list(vec![ExprValue::Int(1)], "int")], "list[int]"),
    ]
}

/// Every shape of target the coercion code distinguishes: each scalar, the
/// unconstrained `any`, non-targets (`noreturn`, type variables, symbolic
/// lists), list targets with narrower/wider/equal element types, and unions
/// mixing scalars, lists, and `nulltype`.
const TARGETS: &[&str] = &[
    "bool",
    "int",
    "float",
    "string",
    "path",
    "range_expr",
    "nulltype",
    "any",
    "noreturn",
    "T1",
    "list[int]",
    "list[float]",
    "list[string]",
    "list[any]",
    "list[list[int]]",
    "list[T1]",
    "int | string",
    "float | string",
    "int | nulltype",
    "string | nulltype",
    "list[int] | string",
    "list[int] | list[string]",
    "path | nulltype | list[path]",
    "int | T1",
    "list[int] | T1",
];

/// The narrowed constraint of a type-level coercion result.
fn constraint_of(unresolved_result: &ExprValue) -> ExprType {
    let ty = unresolved_result.expr_type();
    assert_eq!(
        ty.code(),
        TypeCode::Unresolved,
        "type-level coercion must return an unresolved value, got {ty}"
    );
    ty.params()[0].clone()
}

/// Run `check` for every (value, target) pair in the catalogs, passing the
/// concrete and type-level coercion results.
fn for_each_pair(
    mut check: impl FnMut(&ExprValue, &ExprType, &Result<ExprValue, String>, &Result<ExprValue, String>),
) {
    for target_str in TARGETS {
        let target = t(target_str);
        for value in sample_values() {
            let concrete = value.clone().coerce(&target, PathFormat::Posix);
            let type_level =
                ExprValue::unresolved(value.expr_type()).coerce(&target, PathFormat::Posix);
            check(&value, &target, &concrete, &type_level);
        }
    }
}

#[test]
fn type_level_never_rejects_what_concrete_accepts() {
    for_each_pair(|value, target, concrete, type_level| {
        if let Ok(cv) = concrete {
            assert!(
                type_level.is_ok(),
                "{} → {target}: concrete coercion of {value:?} produced {cv:?}, \
                 but the type level rejected it: {}",
                value.expr_type(),
                type_level.as_ref().unwrap_err(),
            );
        }
    });
}

#[test]
fn successful_results_satisfy_the_target() {
    for_each_pair(|value, target, concrete, type_level| {
        if let Ok(cv) = concrete {
            assert!(
                cv.expr_type().satisfies(target),
                "{} → {target}: concrete result {cv:?} of type {} does not satisfy the target",
                value.expr_type(),
                cv.expr_type(),
            );
        }
        if let Ok(tv) = type_level {
            let constraint = constraint_of(tv);
            assert!(
                constraint.satisfies(target),
                "{} → {target}: narrowed constraint {constraint} does not satisfy the target",
                value.expr_type(),
            );
        }
    });
}

#[test]
fn concrete_result_type_satisfies_the_narrowed_constraint() {
    for_each_pair(|value, target, concrete, type_level| {
        if let Ok(cv) = concrete {
            let constraint = constraint_of(type_level.as_ref().unwrap());
            assert!(
                cv.expr_type().satisfies(&constraint),
                "{} → {target}: concrete result {cv:?} of type {} is not described \
                 by the narrowed constraint {constraint}",
                value.expr_type(),
                cv.expr_type(),
            );
        }
    });
}

#[test]
fn non_union_targets_narrow_to_the_concrete_result_type() {
    for_each_pair(|value, target, concrete, type_level| {
        if target.code() == TypeCode::Union {
            return; // union target: only the payload knows which member wins
        }
        if let Ok(cv) = concrete {
            let constraint = constraint_of(type_level.as_ref().unwrap());
            assert_eq!(
                cv.expr_type(),
                constraint,
                "{} → {target}: concrete result type and narrowed constraint disagree",
                value.expr_type(),
            );
        }
    });
}

/// Constraints with no concrete counterpart — `any`, unions, and
/// wildcard-bearing types (unbound variables read as `any`) — exercise the
/// type-level-only source arms. Their results must still satisfy the
/// target.
#[test]
fn abstract_sources_narrow_soundly() {
    const CONSTRAINTS: &[&str] = &[
        "any",
        "int | string",
        "int | list[int]",
        "float | string",
        "T1",
        "list[T1]",
    ];
    for constraint_str in CONSTRAINTS {
        let constraint = t(constraint_str);
        for target_str in TARGETS {
            let target = t(target_str);
            if let Ok(tv) =
                ExprValue::unresolved(constraint.clone()).coerce(&target, PathFormat::Posix)
            {
                let narrowed = constraint_of(&tv);
                assert!(
                    narrowed.satisfies(&target),
                    "unresolved[{constraint}] → {target}: narrowed constraint {narrowed} \
                     does not satisfy the target",
                );
            }
        }
    }
}

// ── Coverage ratchet ──────────────────────────────────────────────────

/// The invariant tests above are all conditional: each asserts only about
/// pairs that coerced successfully, which is what makes them invariants
/// rather than a table of expected results. That shape means they would all
/// pass vacuously if coercion regressed to always failing, so this test
/// guards the antecedent — the catalogs must keep exercising both steps.
///
/// The two steps are counted separately, because a count of successes alone
/// does not distinguish them: satisfaction returns the value unchanged, so a
/// coercion that only ever satisfies still reports plenty of successes while
/// converting nothing. A conversion is identified by the result's type
/// differing from the source's.
///
/// The floors are deliberately well below the current counts (they are
/// coverage minimums, not golden values), so adding catalog entries never
/// breaks this test — only removing coverage or breaking a step does.
#[test]
fn catalogs_exercise_both_coercion_steps() {
    let (mut satisfied, mut converted, mut type_level_ok) = (0, 0, 0);
    for_each_pair(|value, _target, concrete, type_level| {
        if let Ok(cv) = concrete {
            if cv.expr_type() == value.expr_type() {
                satisfied += 1;
            } else {
                converted += 1;
            }
        }
        if type_level.is_ok() {
            type_level_ok += 1;
        }
    });
    assert!(
        satisfied >= 50,
        "catalogs exercised only {satisfied} satisfaction passes"
    );
    assert!(
        converted >= 40,
        "catalogs exercised only {converted} conversions"
    );
    assert!(
        type_level_ok >= 100,
        "catalogs exercised only {type_level_ok} type-level coercions"
    );
}
