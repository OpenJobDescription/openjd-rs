// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Typed `ExprType` / `ExprValue` generation.

use super::{flip, float, int, one_in, path_string, pick, small_int, string, upto};
use arbitrary::Unstructured;
use openjd_expr::value::Float64;
use openjd_expr::{ExprType, ExprValue, PathFormat, RangeExpr, TypeCode};
use std::str::FromStr;

/// Scalar types a symbol, parameter, or argument can carry.
pub const SCALAR_TYPES: &[ExprType] = &[
    ExprType::BOOL,
    ExprType::INT,
    ExprType::FLOAT,
    ExprType::STRING,
    ExprType::PATH,
    ExprType::RANGE_EXPR,
];

/// A concrete value type: a scalar, a list of scalars, or a list of lists.
pub fn concrete_type(u: &mut Unstructured<'_>) -> ExprType {
    let elem = pick(u, &SCALAR_TYPES[..5]).clone();
    match upto(u, 9) {
        0..=5 => pick(u, SCALAR_TYPES).clone(),
        6..=8 => ExprType::list(elem),
        _ => ExprType::list(ExprType::list(
            pick(u, &[ExprType::INT, ExprType::STRING]).clone(),
        )),
    }
}

/// Any type the type-string grammar can express, including unions,
/// optionals, `any`, type variables, `unresolved[T]`, and signatures.
pub fn any_type(u: &mut Unstructured<'_>, depth: u32) -> ExprType {
    if depth == 0 {
        return concrete_type(u);
    }
    match upto(u, 11) {
        0..=4 => concrete_type(u),
        5 => ExprType::list(any_type(u, depth - 1)),
        6 => {
            let n = upto(u, 3) + 1;
            ExprType::union((0..n).map(|_| any_type(u, depth - 1)).collect())
        }
        7 => ExprType::union(vec![any_type(u, depth - 1), ExprType::NULLTYPE]),
        8 => ExprType::unresolved(any_type(u, depth - 1)),
        9 => pick(
            u,
            &[
                ExprType::ANY,
                ExprType::NORETURN,
                ExprType::NULLTYPE,
                ExprType::T,
                ExprType::T1,
                ExprType::T2,
                ExprType::T3,
            ],
        )
        .clone(),
        10 => {
            let n = upto(u, 2);
            let params = (0..n).map(|_| any_type(u, depth - 1)).collect();
            ExprType::signature(params, any_type(u, depth - 1))
        }
        _ => ExprType::list(ExprType::T1),
    }
}

/// A range-expression string: usually small and valid, sometimes at the
/// `2^62` value bound or malformed.
pub fn range_text(u: &mut Unstructured<'_>) -> String {
    let n = upto(u, 3) + 1;
    let mut parts = Vec::new();
    for _ in 0..n {
        let a = if one_in(u, 6) { int(u) } else { small_int(u) };
        match upto(u, 3) {
            0 => parts.push(a.to_string()),
            1 => {
                let b = if one_in(u, 6) { int(u) } else { small_int(u) };
                parts.push(format!("{a}-{b}"));
            }
            _ => {
                let b = if one_in(u, 6) { int(u) } else { small_int(u) };
                let s = if one_in(u, 6) { int(u) } else { small_int(u) };
                parts.push(format!("{a}-{b}:{s}"));
            }
        }
    }
    parts.join(pick(u, &[",", ", ", " , "]))
}

/// A `RangeExpr`, falling back to `1-3` when the generated text is invalid.
pub fn range_expr(u: &mut Unstructured<'_>) -> RangeExpr {
    let text = range_text(u);
    RangeExpr::from_str(&text).unwrap_or_else(|_| RangeExpr::from_str("1-3").unwrap())
}

/// A `Float64`, sometimes carrying a preserved source spelling.
pub fn float64(u: &mut Unstructured<'_>) -> Float64 {
    let f = float(u);
    let made = if one_in(u, 4) {
        // A preserved spelling must denote the same value: `1.5` may be
        // spelled `1.50`, never something that parses differently.
        let base = format!("{f:?}");
        let spelling = if base.contains('.') && !base.contains('e') && flip(u) {
            format!("{base}0")
        } else {
            base
        };
        Float64::with_str(f, spelling)
    } else {
        Float64::new(f)
    };
    made.unwrap_or_else(|_| Float64::new(0.0).unwrap())
}

/// A concrete value of type `ty` (unions pick a member; `any` and type
/// variables pick a concrete type). Never returns `Unresolved`.
pub fn value_of(u: &mut Unstructured<'_>, ty: &ExprType, fmt: PathFormat) -> ExprValue {
    value_of_depth(u, ty, fmt, 0)
}

fn value_of_depth(u: &mut Unstructured<'_>, ty: &ExprType, fmt: PathFormat, d: u32) -> ExprValue {
    match ty.code() {
        TypeCode::NullType => ExprValue::Null,
        TypeCode::Bool => ExprValue::Bool(flip(u)),
        TypeCode::Int => ExprValue::Int(if one_in(u, 3) { small_int(u) } else { int(u) }),
        TypeCode::Float => ExprValue::Float(float64(u)),
        TypeCode::String => ExprValue::String(string(u)),
        TypeCode::Path => ExprValue::new_path(path_string(u), fmt),
        TypeCode::RangeExpr => ExprValue::RangeExpr(range_expr(u)),
        TypeCode::List if d < 2 => {
            let elem = ty.params().first().cloned().unwrap_or(ExprType::INT);
            let elem = if elem.is_concrete() {
                elem
            } else {
                pick(u, &SCALAR_TYPES[..5]).clone()
            };
            let n = upto(u, 5);
            let items = (0..n)
                .map(|_| value_of_depth(u, &elem, fmt, d + 1))
                .collect();
            ExprValue::make_list(items, elem).unwrap_or(ExprValue::Null)
        }
        TypeCode::Union => {
            let members = ty.params();
            if members.is_empty() {
                ExprValue::Null
            } else {
                let m = pick(u, members).clone();
                value_of_depth(u, &m, fmt, d)
            }
        }
        TypeCode::Unresolved => {
            let inner = ty.params().first().cloned().unwrap_or(ExprType::INT);
            value_of_depth(u, &inner, fmt, d)
        }
        _ => {
            let t = concrete_type(u);
            value_of_depth(u, &t, fmt, d + 1)
        }
    }
}

/// A random concrete value of a random type.
pub fn value(u: &mut Unstructured<'_>, fmt: PathFormat) -> ExprValue {
    if one_in(u, 12) {
        return ExprValue::Null;
    }
    let t = concrete_type(u);
    value_of(u, &t, fmt)
}

/// A path format.
pub fn path_format(u: &mut Unstructured<'_>) -> PathFormat {
    *pick(
        u,
        &[PathFormat::Posix, PathFormat::Windows, PathFormat::Uri],
    )
}
