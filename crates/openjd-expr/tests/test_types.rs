// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_types.py — ExprType system tests.

use openjd_expr::types::{ExprType, TypeCode};
use std::collections::{HashMap, HashSet};

fn p(s: &str) -> ExprType {
    ExprType::parse(s).unwrap()
}

// ══════════════════════════════════════════════════════════════
// TestExprType
// ══════════════════════════════════════════════════════════════

#[test]
fn basic_types() {
    assert_eq!(ExprType::BOOL.code(), TypeCode::Bool);
    assert_eq!(ExprType::INT.code(), TypeCode::Int);
    assert_eq!(ExprType::FLOAT.code(), TypeCode::Float);
    assert_eq!(ExprType::STRING.code(), TypeCode::String);
    assert_eq!(ExprType::PATH.code(), TypeCode::Path);
}

#[test]
fn nullable() {
    let t = ExprType::union(vec![ExprType::INT, ExprType::NULLTYPE]);
    assert_eq!(t.code(), TypeCode::Union);
    assert_eq!(t.to_string(), "int?");
}

#[test]
fn list_of() {
    let t = ExprType::list(ExprType::INT);
    assert_eq!(t.code(), TypeCode::List);
    assert_eq!(t.params(), vec![ExprType::INT]);
}

#[test]
fn equality() {
    assert_eq!(ExprType::INT, ExprType::new(TypeCode::Int, vec![]));
    assert_ne!(ExprType::INT, ExprType::FLOAT);
    assert_eq!(ExprType::list(ExprType::INT), ExprType::list(ExprType::INT));
    assert_ne!(
        ExprType::list(ExprType::INT),
        ExprType::list(ExprType::STRING)
    );
}

#[test]
fn hash() {
    let mut s = HashSet::new();
    s.insert(ExprType::INT);
    s.insert(ExprType::FLOAT);
    s.insert(ExprType::STRING);
    s.insert(ExprType::INT);
    assert_eq!(s.len(), 3);
}

#[test]
fn display_str() {
    assert_eq!(ExprType::INT.to_string(), "int");
    assert_eq!(ExprType::BOOL.to_string(), "bool");
    assert_eq!(ExprType::list(ExprType::INT).to_string(), "list[int]");
    assert_eq!(
        ExprType::union(vec![ExprType::INT, ExprType::NULLTYPE]).to_string(),
        "int?"
    );
}

// ══════════════════════════════════════════════════════════════
// TestExprTypeStringConstruction
// ══════════════════════════════════════════════════════════════

#[test]
fn parse_basic_types() {
    assert_eq!(p("int"), ExprType::INT);
    assert_eq!(p("float"), ExprType::FLOAT);
    assert_eq!(p("string"), ExprType::STRING);
    assert_eq!(p("bool"), ExprType::BOOL);
    assert_eq!(p("path"), ExprType::PATH);
    assert_eq!(p("range_expr"), ExprType::RANGE_EXPR);
    assert_eq!(p("nulltype"), ExprType::NULLTYPE);
}

#[test]
fn parse_case_sensitive() {
    assert_eq!(
        ExprType::parse("INT").unwrap_err(),
        "Unknown type string: INT"
    );
    assert_eq!(
        ExprType::parse("String").unwrap_err(),
        "Unknown type string: String"
    );
}

#[test]
fn parse_whitespace_rejected() {
    assert_eq!(
        ExprType::parse(" int").unwrap_err(),
        "Unknown type string:  int"
    );
    assert_eq!(
        ExprType::parse("int ").unwrap_err(),
        "Unknown type string: int "
    );
    assert_eq!(
        ExprType::parse("list[int ]").unwrap_err(),
        "Unknown type string: int "
    );
    assert_eq!(
        ExprType::parse("list[ int]").unwrap_err(),
        "Unknown type string:  int"
    );
}

#[test]
fn parse_signature_basic() {
    let t = p("(int, int) -> int");
    assert_eq!(t.code(), TypeCode::Signature);
    assert_eq!(t.sig_params(), &[ExprType::INT, ExprType::INT]);
    assert_eq!(*t.sig_return(), ExprType::INT);
}

#[test]
fn parse_signature_union_return() {
    let t = p("(int, int) -> float | int");
    assert_eq!(t.code(), TypeCode::Signature);
    assert_eq!(t.sig_params(), &[ExprType::INT, ExprType::INT]);
    assert_eq!(t.sig_return().code(), TypeCode::Union);
    assert_eq!(t.sig_return().to_string(), "float | int");
}

#[test]
fn parse_signature_no_params() {
    let t = p("() -> string");
    assert_eq!(t.code(), TypeCode::Signature);
    assert!(t.sig_params().is_empty());
    assert_eq!(*t.sig_return(), ExprType::STRING);
}

#[test]
fn parse_signature_generic() {
    let t = p("(list[T1], int) -> T1");
    assert_eq!(t.code(), TypeCode::Signature);
    assert_eq!(t.sig_params().len(), 2);
    assert_eq!(*t.sig_return(), ExprType::T1);
}

#[test]
fn parse_list_types() {
    assert_eq!(p("list[int]"), ExprType::list(ExprType::INT));
    assert_eq!(p("list[string]"), ExprType::list(ExprType::STRING));
    assert_eq!(p("list[float]"), ExprType::list(ExprType::FLOAT));
    assert_eq!(p("list[path]"), ExprType::list(ExprType::PATH));
    assert_eq!(p("list[bool]"), ExprType::list(ExprType::BOOL));
}

#[test]
fn parse_nested_list() {
    assert_eq!(
        p("list[list[int]]"),
        ExprType::list(ExprType::list(ExprType::INT))
    );
}

#[test]
fn parse_optional_types() {
    let int_opt = p("int?");
    assert_eq!(int_opt.code(), TypeCode::Union);
    assert_eq!(int_opt.to_string(), "int?");

    let string_opt = p("string?");
    assert_eq!(string_opt.code(), TypeCode::Union);
    assert_eq!(string_opt.to_string(), "string?");

    let list_opt = p("list[int]?");
    assert_eq!(list_opt.code(), TypeCode::Union);
    assert_eq!(list_opt.to_string(), "list[int]?");
}

#[test]
fn parse_unknown_type_raises() {
    assert_eq!(
        ExprType::parse("notavalidtype").unwrap_err(),
        "Unknown type string: notavalidtype"
    );
}

// ══════════════════════════════════════════════════════════════
// TestUnionTypes
// ══════════════════════════════════════════════════════════════

#[test]
fn any_type() {
    assert_eq!(ExprType::ANY.code(), TypeCode::Any);
    assert_eq!(ExprType::ANY.to_string(), "any");
}

#[test]
fn noreturn_type() {
    assert_eq!(ExprType::NORETURN.code(), TypeCode::NoReturn);
    assert_eq!(ExprType::NORETURN.to_string(), "noreturn");
    assert_eq!(p("noreturn"), ExprType::NORETURN);
}

#[test]
fn noreturn_collapses_in_union() {
    assert_eq!(
        ExprType::union(vec![ExprType::INT, ExprType::NORETURN]),
        ExprType::INT
    );
}

#[test]
fn noreturn_collapses_with_multiple() {
    let u = ExprType::union(vec![ExprType::INT, ExprType::STRING, ExprType::NORETURN]);
    assert_eq!(u.code(), TypeCode::Union);
    assert!(!u.params().contains(&ExprType::NORETURN));
    assert_eq!(u.to_string(), "int | string");
}

#[test]
fn noreturn_only_union() {
    assert_eq!(
        ExprType::union(vec![ExprType::NORETURN, ExprType::NORETURN]),
        ExprType::NORETURN
    );
}

#[test]
fn union_construction() {
    let u = ExprType::union(vec![ExprType::INT, ExprType::STRING]);
    assert_eq!(u.code(), TypeCode::Union);
    assert_eq!(u.to_string(), "int | string");
}

#[test]
fn union_deduplication() {
    let u = ExprType::union(vec![ExprType::INT, ExprType::INT]);
    assert_eq!(u.code(), TypeCode::Int);
}

#[test]
fn union_single_element_unwrap() {
    let u = ExprType::union(vec![ExprType::STRING]);
    assert_eq!(u.code(), TypeCode::String);
}

#[test]
fn union_flattening() {
    let u1 = ExprType::union(vec![ExprType::INT, ExprType::STRING]);
    let u2 = ExprType::union(vec![ExprType::FLOAT, ExprType::BOOL]);
    let combined = ExprType::union(vec![u1, u2]);
    assert_eq!(combined.code(), TypeCode::Union);
    assert_eq!(combined.params().len(), 4);
    assert_eq!(combined.to_string(), "bool | float | int | string");
}

#[test]
fn union_any_absorption() {
    assert_eq!(
        ExprType::union(vec![ExprType::INT, ExprType::ANY]).code(),
        TypeCode::Any
    );
}

#[test]
fn union_string_parsing() {
    let u = p("int | string");
    assert_eq!(u.code(), TypeCode::Union);
    assert_eq!(u.to_string(), "int | string");
}

#[test]
fn union_string_parsing_multiple() {
    let u = p("bool | float | int");
    assert_eq!(u.code(), TypeCode::Union);
    assert_eq!(u.params().len(), 3);
}

#[test]
fn union_nullable_display_single() {
    assert_eq!(
        ExprType::union(vec![ExprType::INT, ExprType::NULLTYPE]).to_string(),
        "int?"
    );
}

#[test]
fn union_nullable_display_multiple() {
    let u = ExprType::union(vec![ExprType::INT, ExprType::FLOAT, ExprType::NULLTYPE]);
    assert_eq!(u.to_string(), "float | int | nulltype");
}

#[test]
fn union_equality_order_independent() {
    let u1 = ExprType::union(vec![ExprType::INT, ExprType::STRING]);
    let u2 = ExprType::union(vec![ExprType::STRING, ExprType::INT]);
    assert_eq!(u1, u2);
}

#[test]
fn union_hash_consistent() {
    let u1 = ExprType::union(vec![ExprType::INT, ExprType::STRING]);
    let u2 = p("int | string");
    let mut s = HashSet::new();
    s.insert(u1);
    s.insert(u2);
    assert_eq!(s.len(), 1);
}

#[test]
fn union_roundtrip() {
    let original = ExprType::union(vec![ExprType::INT, ExprType::STRING]);
    let roundtrip = p(&original.to_string());
    assert_eq!(roundtrip, original);
}

#[test]
fn union_inside_list() {
    let t = p("list[int | float]");
    assert_eq!(t.code(), TypeCode::List);
    assert_eq!(t.params()[0].code(), TypeCode::Union);
    assert_eq!(t.to_string(), "list[float | int]");
}

#[test]
fn union_of_list_with_union_element() {
    let t = p("list[int | string] | path");
    assert_eq!(t.code(), TypeCode::Union);
    assert_eq!(t.to_string(), "list[int | string] | path");
}

#[test]
fn any_matches_anything() {
    assert!(ExprType::ANY.match_type(&ExprType::INT).is_some());
    assert!(ExprType::ANY.match_type(&ExprType::STRING).is_some());
    assert!(ExprType::ANY
        .match_type(&ExprType::list(ExprType::INT))
        .is_some());
}

#[test]
fn anything_matches_any() {
    assert!(ExprType::INT.match_type(&ExprType::ANY).is_some());
    assert!(ExprType::STRING.match_type(&ExprType::ANY).is_some());
}

#[test]
fn is_concrete() {
    assert!(ExprType::INT.is_concrete());
    assert!(ExprType::STRING.is_concrete());
    assert!(ExprType::list(ExprType::INT).is_concrete());
    assert!(!ExprType::T1.is_concrete());
    assert!(!ExprType::list(ExprType::T1).is_concrete());
    assert!(!ExprType::ANY.is_concrete());
    assert!(!ExprType::union(vec![ExprType::INT, ExprType::STRING]).is_concrete());
    // Nested union inside list
    let list_union = ExprType::list(ExprType::union(vec![ExprType::INT, ExprType::STRING]));
    assert!(!list_union.is_concrete());
}

#[test]
fn union_matches_member() {
    let u = ExprType::union(vec![ExprType::INT, ExprType::STRING]);
    assert!(u.match_type(&ExprType::INT).is_some());
    assert!(u.match_type(&ExprType::STRING).is_some());
    assert!(u.match_type(&ExprType::FLOAT).is_none());
}

#[test]
fn member_matches_union() {
    let u = ExprType::union(vec![ExprType::INT, ExprType::STRING]);
    assert!(ExprType::INT.match_type(&u).is_some());
    assert!(ExprType::STRING.match_type(&u).is_some());
    assert!(ExprType::FLOAT.match_type(&u).is_none());
}

#[test]
fn union_matches_union_with_overlap() {
    let u1 = ExprType::union(vec![ExprType::INT, ExprType::STRING]);
    let u2 = ExprType::union(vec![ExprType::STRING, ExprType::FLOAT]);
    assert!(u1.match_type(&u2).is_some());
    assert!(u2.match_type(&u1).is_some());
}

#[test]
fn typevar_binds_to_union() {
    let u = ExprType::union(vec![ExprType::INT, ExprType::STRING]);
    let b = ExprType::T1.match_type(&u).unwrap();
    assert_eq!(b[&TypeCode::TypeVarT1], u);
    let b2 = u.match_type(&ExprType::T1).unwrap();
    assert_eq!(b2[&TypeCode::TypeVarT1], u);
}

#[test]
fn nested_typevar_binds_to_union() {
    let list_t1 = ExprType::list(ExprType::T1);
    let inner_union = ExprType::union(vec![ExprType::INT, ExprType::STRING]);
    let list_union = ExprType::list(inner_union.clone());
    let result = list_t1.match_type(&list_union).unwrap();
    assert_eq!(result[&TypeCode::TypeVarT1], inner_union);
}

#[test]
fn any_with_typevar() {
    let b = ExprType::T1.match_type(&ExprType::ANY).unwrap();
    assert_eq!(b[&TypeCode::TypeVarT1], ExprType::ANY);
    let b2 = ExprType::ANY.match_type(&ExprType::T1).unwrap();
    assert_eq!(b2[&TypeCode::TypeVarT1], ExprType::ANY);
}

// ══════════════════════════════════════════════════════════════
// TestTypeVariables
// ══════════════════════════════════════════════════════════════

#[test]
fn is_symbolic_simple() {
    assert!(ExprType::T1.is_symbolic());
    assert!(!ExprType::INT.is_symbolic());
}

#[test]
fn is_symbolic_nested() {
    assert!(ExprType::list(ExprType::T1).is_symbolic());
    assert!(!ExprType::list(ExprType::INT).is_symbolic());
}

#[test]
fn match_simple() {
    let b = ExprType::T1.match_type(&ExprType::INT).unwrap();
    assert_eq!(b[&TypeCode::TypeVarT1], ExprType::INT);
}

#[test]
fn match_nested() {
    let list_t1 = ExprType::list(ExprType::T1);
    let list_int = ExprType::list(ExprType::INT);
    let b = list_t1.match_type(&list_int).unwrap();
    assert_eq!(b[&TypeCode::TypeVarT1], ExprType::INT);
}

#[test]
fn match_no_match() {
    let list_t1 = ExprType::list(ExprType::T1);
    assert!(list_t1.match_type(&ExprType::INT).is_none());
    assert!(list_t1.match_type(&ExprType::STRING).is_none());
}

#[test]
fn substitute() {
    let list_t1 = ExprType::list(ExprType::T1);
    let mut bindings = HashMap::new();
    bindings.insert(TypeCode::TypeVarT1, ExprType::INT);
    assert_eq!(list_t1.substitute(&bindings), ExprType::list(ExprType::INT));
}

// ══════════════════════════════════════════════════════════════
// TestUnknownType
// ══════════════════════════════════════════════════════════════

#[test]
fn unknown_construct_with_constraint() {
    let t = ExprType::unresolved(ExprType::INT);
    assert_eq!(t.code(), TypeCode::Unresolved);
    assert_eq!(t.params(), vec![ExprType::INT]);
}

#[test]
fn unknown_parse_bare() {
    let t = p("unresolved");
    assert_eq!(t.code(), TypeCode::Unresolved);
    assert_eq!(t.params(), vec![ExprType::ANY]);
}

#[test]
fn unknown_parse_int() {
    let t = p("unresolved[int]");
    assert_eq!(t.code(), TypeCode::Unresolved);
    assert_eq!(t.params(), vec![ExprType::INT]);
}

#[test]
fn unknown_parse_list() {
    let t = p("unresolved[list[string]]");
    assert_eq!(t.code(), TypeCode::Unresolved);
    assert_eq!(t.params(), vec![ExprType::list(ExprType::STRING)]);
}

#[test]
fn unknown_parse_union() {
    let t = p("unresolved[int | float]");
    assert_eq!(t.code(), TypeCode::Unresolved);
    assert_eq!(t.params()[0].code(), TypeCode::Union);
}

#[test]
fn unknown_parse_any() {
    assert_eq!(p("unresolved[any]"), p("unresolved"));
}

#[test]
fn unknown_str_bare() {
    assert_eq!(p("unresolved").to_string(), "unresolved");
}
#[test]
fn unknown_str_any_is_bare() {
    assert_eq!(
        ExprType::unresolved(ExprType::ANY).to_string(),
        "unresolved"
    );
}
#[test]
fn unknown_str_int() {
    assert_eq!(p("unresolved[int]").to_string(), "unresolved[int]");
}
#[test]
fn unknown_str_list() {
    assert_eq!(
        p("unresolved[list[string]]").to_string(),
        "unresolved[list[string]]"
    );
}

#[test]
fn unknown_roundtrip_bare() {
    let t = p("unresolved");
    assert_eq!(p(&t.to_string()), t);
}

#[test]
fn unknown_roundtrip_constrained() {
    for s in &[
        "unresolved[int]",
        "unresolved[list[string]]",
        "unresolved[float | int]",
    ] {
        let t = p(s);
        assert_eq!(p(&t.to_string()), t, "roundtrip failed for {s}");
    }
}

#[test]
fn unknown_equality() {
    assert_eq!(p("unresolved[int]"), p("unresolved[int]"));
    assert_ne!(p("unresolved[int]"), p("unresolved[float]"));
    assert_eq!(p("unresolved"), p("unresolved[any]"));
}

#[test]
fn unknown_hash() {
    let mut s = HashSet::new();
    s.insert(p("unresolved[int]"));
    s.insert(p("unresolved[int]"));
    assert_eq!(s.len(), 1);
}

#[test]
fn unknown_not_concrete() {
    assert!(!p("unresolved").is_concrete());
    assert!(!p("unresolved[int]").is_concrete());
}

#[test]
fn unknown_not_symbolic() {
    assert!(!p("unresolved[int]").is_symbolic());
}

#[test]
fn unknown_symbolic_if_constraint_is() {
    let t = ExprType::unresolved(ExprType::T1);
    assert!(t.is_symbolic());
}

#[test]
fn unknown_match_delegates_to_constraint() {
    let t = p("unresolved[int]");
    assert!(t.match_type(&ExprType::INT).is_some());
    assert!(t.match_type(&ExprType::STRING).is_none());
    assert!(t.match_type(&ExprType::FLOAT).is_none());
}

#[test]
fn unknown_match_symmetric() {
    let t = p("unresolved[int]");
    assert!(ExprType::INT.match_type(&t).is_some());
    assert!(ExprType::STRING.match_type(&t).is_none());
}

#[test]
fn unknown_match_union_constraint() {
    let t = p("unresolved[int | float]");
    assert!(t.match_type(&ExprType::INT).is_some());
    assert!(t.match_type(&ExprType::FLOAT).is_some());
    assert!(t.match_type(&ExprType::STRING).is_none());
}

#[test]
fn unknown_match_any_constraint() {
    let t = p("unresolved");
    assert!(t.match_type(&ExprType::INT).is_some());
    assert!(t.match_type(&ExprType::STRING).is_some());
    assert!(t.match_type(&ExprType::list(ExprType::INT)).is_some());
}

#[test]
fn unknown_match_unknown_vs_unknown() {
    let t1 = p("unresolved[int]");
    let t2 = p("unresolved[int]");
    assert!(t1.match_type(&t2).is_some());
}

#[test]
fn unknown_match_unknown_vs_any() {
    let t = p("unresolved[int]");
    assert!(t.match_type(&ExprType::ANY).is_some());
    assert!(ExprType::ANY.match_type(&t).is_some());
}

#[test]
fn unknown_substitute_constraint() {
    let t = ExprType::unresolved(ExprType::T1);
    let mut bindings = HashMap::new();
    bindings.insert(TypeCode::TypeVarT1, ExprType::INT);
    assert_eq!(t.substitute(&bindings), ExprType::unresolved(ExprType::INT));
}

// ══════════════════════════════════════════════════════════════
// TestUnknownTypeNormalization
// ══════════════════════════════════════════════════════════════

#[test]
fn norm_list_of_unknown_hoists() {
    let t = p("list[unresolved[int]]");
    assert_eq!(t.code(), TypeCode::Unresolved);
    assert_eq!(t, p("unresolved[list[int]]"));
}

#[test]
fn norm_list_of_unknown_union_constraint() {
    let t = p("list[unresolved[int | float]]");
    assert_eq!(t.code(), TypeCode::Unresolved);
    assert_eq!(t, p("unresolved[list[float | int]]"));
}

#[test]
fn norm_list_of_concrete_unchanged() {
    assert_eq!(p("list[int]").code(), TypeCode::List);
}

#[test]
fn norm_union_with_unknown_hoists() {
    let t = p("string | unresolved[int]");
    assert_eq!(t.code(), TypeCode::Unresolved);
    assert_eq!(t, p("unresolved[int | string]"));
}

#[test]
fn norm_union_with_unknown_preserves_all() {
    let t = p("string | bool | unresolved[int]");
    assert_eq!(t.code(), TypeCode::Unresolved);
    assert_eq!(t, p("unresolved[bool | int | string]"));
}

#[test]
fn norm_union_without_unknown_unchanged() {
    assert_eq!(p("int | string").code(), TypeCode::Union);
}

#[test]
fn norm_union_of_two_unknowns_merges() {
    let t = p("unresolved[int] | unresolved[float]");
    assert_eq!(t.code(), TypeCode::Unresolved);
    assert_eq!(t, p("unresolved[float | int]"));
}

#[test]
fn norm_union_of_same_unknowns_dedup() {
    let t = p("unresolved[int] | unresolved[int]");
    assert_eq!(t.code(), TypeCode::Unresolved);
    assert_eq!(t, p("unresolved[int]"));
}

#[test]
fn norm_nested_unknown_flattens() {
    let t = p("unresolved[unresolved[int]]");
    assert_eq!(t.code(), TypeCode::Unresolved);
    assert_eq!(t, p("unresolved[int]"));
}

#[test]
fn norm_triple_nested_unknown_flattens() {
    let t = p("unresolved[unresolved[unresolved[int]]]");
    assert_eq!(t.code(), TypeCode::Unresolved);
    assert_eq!(t, p("unresolved[int]"));
}

#[test]
fn norm_list_unknown_then_union() {
    let t = p("list[unresolved[int]] | string");
    assert_eq!(t.code(), TypeCode::Unresolved);
    assert_eq!(t, p("unresolved[list[int] | string]"));
}

#[test]
fn norm_union_unknown_with_any_absorbs() {
    assert_eq!(p("unresolved[int] | any").code(), TypeCode::Any);
}

#[test]
fn norm_union_unknown_with_noreturn_collapses() {
    assert_eq!(p("unresolved[int] | noreturn"), p("unresolved[int]"));
}

#[test]
fn norm_unknown_never_inside_list() {
    let t = p("list[unresolved[string]]");
    assert_eq!(t.code(), TypeCode::Unresolved);
    assert_eq!(t.params()[0], p("list[string]"));
}

#[test]
fn norm_unknown_never_inside_union() {
    let t = p("int | unresolved[float]");
    assert_eq!(t.code(), TypeCode::Unresolved);
    let constraint = &t.params()[0];
    if constraint.code() == TypeCode::Union {
        for member in constraint.params() {
            assert_ne!(member.code(), TypeCode::Unresolved);
        }
    }
}

// ══════════════════════════════════════════════════════════════
// Extended type variable matching and substitution tests
// ══════════════════════════════════════════════════════════════

// -- Multiple type variables --

#[test]
fn match_two_independent_typevars() {
    // __eq__(T1, T2) -> bool pattern: T1 and T2 bind independently
    let sig_left = ExprType::T1;
    let sig_right = ExprType::T2;
    let b1 = sig_left.match_type(&ExprType::INT).unwrap();
    let b2 = sig_right.match_type(&ExprType::STRING).unwrap();
    assert_eq!(b1[&TypeCode::TypeVarT1], ExprType::INT);
    assert_eq!(b2[&TypeCode::TypeVarT2], ExprType::STRING);
}

#[test]
fn match_t_and_t1_are_distinct() {
    let b = ExprType::T.match_type(&ExprType::INT).unwrap();
    assert_eq!(b[&TypeCode::TypeVarT], ExprType::INT);
    assert!(!b.contains_key(&TypeCode::TypeVarT1));
}

// -- Same typevar used twice (consistent binding) --

#[test]
fn match_contains_signature() {
    // __contains__(list[T1], T1) -> bool
    // Match list[T1] against list[int] => T1=int
    // Then match T1 against int => T1=int (consistent)
    let list_t1 = ExprType::list(ExprType::T1);
    let list_int = ExprType::list(ExprType::INT);
    let b1 = list_t1.match_type(&list_int).unwrap();
    assert_eq!(b1[&TypeCode::TypeVarT1], ExprType::INT);
    let b2 = ExprType::T1.match_type(&ExprType::INT).unwrap();
    assert_eq!(b2[&TypeCode::TypeVarT1], ExprType::INT);
    // Both bind T1 to int — consistent
    assert_eq!(b1[&TypeCode::TypeVarT1], b2[&TypeCode::TypeVarT1]);
}

#[test]
fn match_conflicting_bindings_fail() {
    // list[T1] matched against list[int] gives T1=int
    // Then if we try to match T1 against string, that's a different binding
    // The match_type on a composite should detect this
    // Simulate: signature (list[T1], list[T1]) matched against (list[int], list[string])
    let sig = ExprType::list(ExprType::T1);
    let b1 = sig.match_type(&ExprType::list(ExprType::INT)).unwrap();
    let b2 = sig.match_type(&ExprType::list(ExprType::STRING)).unwrap();
    // T1 binds to int in first, string in second — conflict
    assert_ne!(b1[&TypeCode::TypeVarT1], b2[&TypeCode::TypeVarT1]);
}

#[test]
fn match_nested_list_consistent_binding() {
    // list[list[T1]] matched against list[list[int]]
    let sig = ExprType::list(ExprType::list(ExprType::T1));
    let concrete = ExprType::list(ExprType::list(ExprType::INT));
    let b = sig.match_type(&concrete).unwrap();
    assert_eq!(b[&TypeCode::TypeVarT1], ExprType::INT);
}

#[test]
fn match_nested_list_conflicting_inner() {
    // list[list[T1]] vs list[list[string]] — T1 binds to string
    let sig = ExprType::list(ExprType::list(ExprType::T1));
    let concrete = ExprType::list(ExprType::list(ExprType::STRING));
    let b = sig.match_type(&concrete).unwrap();
    assert_eq!(b[&TypeCode::TypeVarT1], ExprType::STRING);
}

// -- Getitem signature: list[T1] × int -> T1 --

#[test]
fn match_getitem_list_int() {
    let list_t1 = ExprType::list(ExprType::T1);
    let b = list_t1.match_type(&ExprType::list(ExprType::INT)).unwrap();
    // Return type T1 substituted gives int
    let ret = ExprType::T1.substitute(&b);
    assert_eq!(ret, ExprType::INT);
}

#[test]
fn match_getitem_list_string() {
    let list_t1 = ExprType::list(ExprType::T1);
    let b = list_t1
        .match_type(&ExprType::list(ExprType::STRING))
        .unwrap();
    let ret = ExprType::T1.substitute(&b);
    assert_eq!(ret, ExprType::STRING);
}

#[test]
fn match_getitem_list_list_int() {
    // list[list[int]][0] -> list[int]
    let list_t1 = ExprType::list(ExprType::T1);
    let b = list_t1
        .match_type(&ExprType::list(ExprType::list(ExprType::INT)))
        .unwrap();
    let ret = ExprType::T1.substitute(&b);
    assert_eq!(ret, ExprType::list(ExprType::INT));
}

// -- List concat: list[T1] + list[T2] -> list[T3] --

#[test]
fn match_list_concat_same_type() {
    let sig1 = ExprType::list(ExprType::T1);
    let sig2 = ExprType::list(ExprType::T2);
    let b1 = sig1.match_type(&ExprType::list(ExprType::INT)).unwrap();
    let b2 = sig2.match_type(&ExprType::list(ExprType::INT)).unwrap();
    assert_eq!(b1[&TypeCode::TypeVarT1], ExprType::INT);
    assert_eq!(b2[&TypeCode::TypeVarT2], ExprType::INT);
}

#[test]
fn match_list_concat_different_types() {
    let sig1 = ExprType::list(ExprType::T1);
    let sig2 = ExprType::list(ExprType::T2);
    let b1 = sig1.match_type(&ExprType::list(ExprType::INT)).unwrap();
    let b2 = sig2.match_type(&ExprType::list(ExprType::STRING)).unwrap();
    assert_eq!(b1[&TypeCode::TypeVarT1], ExprType::INT);
    assert_eq!(b2[&TypeCode::TypeVarT2], ExprType::STRING);
}

// -- Substitute with multiple bindings --

#[test]
fn substitute_multiple_typevars() {
    // Simulate resolving a return type that uses T1
    let ret_type = ExprType::list(ExprType::T1);
    let mut bindings = HashMap::new();
    bindings.insert(TypeCode::TypeVarT1, ExprType::FLOAT);
    bindings.insert(TypeCode::TypeVarT2, ExprType::STRING); // unused but present
    assert_eq!(
        ret_type.substitute(&bindings),
        ExprType::list(ExprType::FLOAT)
    );
}

#[test]
fn substitute_nested() {
    // list[list[T1]] with T1=int -> list[list[int]]
    let t = ExprType::list(ExprType::list(ExprType::T1));
    let mut b = HashMap::new();
    b.insert(TypeCode::TypeVarT1, ExprType::INT);
    assert_eq!(
        t.substitute(&b),
        ExprType::list(ExprType::list(ExprType::INT))
    );
}

#[test]
fn substitute_no_match_passthrough() {
    // T2 not in bindings -> stays as T2
    let mut b = HashMap::new();
    b.insert(TypeCode::TypeVarT1, ExprType::INT);
    assert_eq!(ExprType::T2.substitute(&b), ExprType::T2);
}

#[test]
fn substitute_in_union() {
    // T1 | string with T1=int -> int | string
    let t = ExprType::union(vec![ExprType::T1, ExprType::STRING]);
    let mut b = HashMap::new();
    b.insert(TypeCode::TypeVarT1, ExprType::INT);
    let result = t.substitute(&b);
    assert_eq!(
        result,
        ExprType::union(vec![ExprType::INT, ExprType::STRING])
    );
}

#[test]
fn substitute_in_unresolved() {
    // unresolved[T1] with T1=int -> unresolved[int]
    let t = ExprType::unresolved(ExprType::T1);
    let mut b = HashMap::new();
    b.insert(TypeCode::TypeVarT1, ExprType::INT);
    assert_eq!(t.substitute(&b), ExprType::unresolved(ExprType::INT));
}

// -- Concrete vs concrete matching (no type vars) --

#[test]
fn match_concrete_same() {
    assert_eq!(
        ExprType::INT.match_type(&ExprType::INT),
        Some(HashMap::new())
    );
}

#[test]
fn match_concrete_different() {
    assert!(ExprType::INT.match_type(&ExprType::STRING).is_none());
}

#[test]
fn match_concrete_list_same() {
    assert_eq!(
        ExprType::list(ExprType::INT).match_type(&ExprType::list(ExprType::INT)),
        Some(HashMap::new())
    );
}

#[test]
fn match_concrete_list_different_elem() {
    assert!(ExprType::list(ExprType::INT)
        .match_type(&ExprType::list(ExprType::STRING))
        .is_none());
}

#[test]
fn match_list_vs_nonlist() {
    assert!(ExprType::list(ExprType::INT)
        .match_type(&ExprType::INT)
        .is_none());
    assert!(ExprType::INT
        .match_type(&ExprType::list(ExprType::INT))
        .is_none());
}

// -- Typevar in union matching --

#[test]
fn match_typevar_in_union_signature() {
    // Signature like T1 | string matched against int -> T1=int
    let sig = ExprType::union(vec![ExprType::T1, ExprType::STRING]);
    let b = sig.match_type(&ExprType::INT).unwrap();
    assert_eq!(b[&TypeCode::TypeVarT1], ExprType::INT);
}

#[test]
fn match_typevar_in_union_matches_member() {
    // T1 | string matched against string -> empty bindings (string matches string directly)
    let sig = ExprType::union(vec![ExprType::T1, ExprType::STRING]);
    // T1 matches string first in iteration, so we get T1=string
    let b = sig.match_type(&ExprType::STRING).unwrap();
    // Either T1 binds to string, or string matches string with no bindings — both valid
    assert!(b.is_empty() || b.get(&TypeCode::TypeVarT1) == Some(&ExprType::STRING));
}

// -- All four type variables --

#[test]
fn all_four_typevars_distinct() {
    assert_ne!(ExprType::T, ExprType::T1);
    assert_ne!(ExprType::T1, ExprType::T2);
    assert_ne!(ExprType::T2, ExprType::T3);
    assert!(ExprType::T.is_symbolic());
    assert!(ExprType::T1.is_symbolic());
    assert!(ExprType::T2.is_symbolic());
    assert!(ExprType::T3.is_symbolic());
}

#[test]
fn match_all_four_independently() {
    let b_t = ExprType::T.match_type(&ExprType::INT).unwrap();
    let b_t1 = ExprType::T1.match_type(&ExprType::FLOAT).unwrap();
    let b_t2 = ExprType::T2.match_type(&ExprType::STRING).unwrap();
    let b_t3 = ExprType::T3.match_type(&ExprType::BOOL).unwrap();
    assert_eq!(b_t[&TypeCode::TypeVarT], ExprType::INT);
    assert_eq!(b_t1[&TypeCode::TypeVarT1], ExprType::FLOAT);
    assert_eq!(b_t2[&TypeCode::TypeVarT2], ExprType::STRING);
    assert_eq!(b_t3[&TypeCode::TypeVarT3], ExprType::BOOL);
}

// ══════════════════════════════════════════════════════════════
// Signature type tests
// ══════════════════════════════════════════════════════════════

// -- Construction and display --

#[test]
fn sig_display_no_params() {
    let sig = ExprType::signature(vec![], ExprType::INT);
    assert_eq!(sig.to_string(), "() -> int");
}

#[test]
fn sig_display_one_param() {
    let sig = ExprType::signature(vec![ExprType::STRING], ExprType::INT);
    assert_eq!(sig.to_string(), "(string) -> int");
}

#[test]
fn sig_display_two_params() {
    let sig = ExprType::signature(vec![ExprType::STRING, ExprType::STRING], ExprType::BOOL);
    assert_eq!(sig.to_string(), "(string, string) -> bool");
}

#[test]
fn sig_display_generic() {
    let sig = ExprType::signature(
        vec![ExprType::list(ExprType::T1), ExprType::INT],
        ExprType::T1,
    );
    assert_eq!(sig.to_string(), "(list[T1], int) -> T1");
}

#[test]
fn sig_display_contains() {
    let sig = ExprType::signature(
        vec![ExprType::list(ExprType::T1), ExprType::T1],
        ExprType::BOOL,
    );
    assert_eq!(sig.to_string(), "(list[T1], T1) -> bool");
}

// -- Parsing --

#[test]
fn sig_parse_no_params() {
    assert_eq!(p("() -> int"), ExprType::signature(vec![], ExprType::INT));
}

#[test]
fn sig_parse_one_param() {
    assert_eq!(
        p("(string) -> int"),
        ExprType::signature(vec![ExprType::STRING], ExprType::INT)
    );
}

#[test]
fn sig_parse_two_params() {
    assert_eq!(
        p("(string, string) -> bool"),
        ExprType::signature(vec![ExprType::STRING, ExprType::STRING], ExprType::BOOL)
    );
}

#[test]
fn sig_parse_generic() {
    assert_eq!(
        p("(list[T1], int) -> T1"),
        ExprType::signature(
            vec![ExprType::list(ExprType::T1), ExprType::INT],
            ExprType::T1
        )
    );
}

#[test]
fn sig_parse_contains() {
    assert_eq!(
        p("(list[T1], T1) -> bool"),
        ExprType::signature(
            vec![ExprType::list(ExprType::T1), ExprType::T1],
            ExprType::BOOL
        )
    );
}

#[test]
fn sig_parse_complex_return() {
    assert_eq!(
        p("(list[T1], list[T2]) -> list[T3]"),
        ExprType::signature(
            vec![ExprType::list(ExprType::T1), ExprType::list(ExprType::T2)],
            ExprType::list(ExprType::T3)
        )
    );
}

// -- Roundtrip --

#[test]
fn sig_roundtrip() {
    for s in &[
        "() -> int",
        "(string) -> int",
        "(list[T1], int) -> T1",
        "(list[T1], T1) -> bool",
        "(T1, T2) -> bool",
        "(list[T1], list[T2]) -> list[T3]",
    ] {
        let t = p(s);
        assert_eq!(p(&t.to_string()), t, "roundtrip failed for {s}");
    }
}

// -- Accessors --

#[test]
fn sig_params_and_return() {
    let sig = ExprType::signature(vec![ExprType::STRING, ExprType::INT], ExprType::BOOL);
    assert_eq!(sig.sig_params(), &[ExprType::STRING, ExprType::INT]);
    assert_eq!(sig.sig_return(), &ExprType::BOOL);
}

// -- match_call --

#[test]
fn match_call_concrete_exact() {
    // (string, string) -> bool matched with (string, string)
    let sig = p("(string, string) -> bool");
    let b = sig
        .match_call(&[ExprType::STRING, ExprType::STRING])
        .unwrap();
    assert!(b.is_empty());
}

#[test]
fn match_call_concrete_wrong_type() {
    let sig = p("(string, string) -> bool");
    assert!(sig.match_call(&[ExprType::STRING, ExprType::INT]).is_none());
}

#[test]
fn match_call_wrong_arity() {
    let sig = p("(string) -> int");
    assert!(sig
        .match_call(&[ExprType::STRING, ExprType::STRING])
        .is_none());
    assert!(sig.match_call(&[]).is_none());
}

#[test]
fn match_call_getitem() {
    // __getitem__(list[T1], int) -> T1
    let sig = p("(list[T1], int) -> T1");
    let b = sig
        .match_call(&[ExprType::list(ExprType::INT), ExprType::INT])
        .unwrap();
    assert_eq!(b[&TypeCode::TypeVarT1], ExprType::INT);
}

#[test]
fn match_call_contains_consistent() {
    // __contains__(list[T1], T1) -> bool
    let sig = p("(list[T1], T1) -> bool");
    let b = sig
        .match_call(&[ExprType::list(ExprType::INT), ExprType::INT])
        .unwrap();
    assert_eq!(b[&TypeCode::TypeVarT1], ExprType::INT);
}

#[test]
fn match_call_contains_conflict() {
    // __contains__(list[T1], T1) -> bool with list[int] and string -> conflict
    let sig = p("(list[T1], T1) -> bool");
    assert!(sig
        .match_call(&[ExprType::list(ExprType::INT), ExprType::STRING])
        .is_none());
}

#[test]
fn match_call_eq_any_types() {
    // __eq__(T1, T2) -> bool accepts any two types
    let sig = p("(T1, T2) -> bool");
    let b = sig.match_call(&[ExprType::INT, ExprType::STRING]).unwrap();
    assert_eq!(b[&TypeCode::TypeVarT1], ExprType::INT);
    assert_eq!(b[&TypeCode::TypeVarT2], ExprType::STRING);
}

// -- resolve_call --

#[test]
fn resolve_getitem_int() {
    let sig = p("(list[T1], int) -> T1");
    let ret = sig
        .resolve_call(&[ExprType::list(ExprType::INT), ExprType::INT])
        .unwrap();
    assert_eq!(ret, ExprType::INT);
}

#[test]
fn resolve_getitem_string() {
    let sig = p("(list[T1], int) -> T1");
    let ret = sig
        .resolve_call(&[ExprType::list(ExprType::STRING), ExprType::INT])
        .unwrap();
    assert_eq!(ret, ExprType::STRING);
}

#[test]
fn resolve_getitem_nested() {
    // list[list[int]][0] -> list[int]
    let sig = p("(list[T1], int) -> T1");
    let ret = sig
        .resolve_call(&[ExprType::list(ExprType::list(ExprType::INT)), ExprType::INT])
        .unwrap();
    assert_eq!(ret, ExprType::list(ExprType::INT));
}

#[test]
fn resolve_contains() {
    let sig = p("(list[T1], T1) -> bool");
    let ret = sig
        .resolve_call(&[ExprType::list(ExprType::INT), ExprType::INT])
        .unwrap();
    assert_eq!(ret, ExprType::BOOL);
}

#[test]
fn resolve_no_match_returns_none() {
    let sig = p("(list[T1], T1) -> bool");
    assert!(sig
        .resolve_call(&[ExprType::list(ExprType::INT), ExprType::STRING])
        .is_none());
}

#[test]
fn resolve_concrete_sig() {
    let sig = p("(string) -> int");
    assert_eq!(
        sig.resolve_call(&[ExprType::STRING]).unwrap(),
        ExprType::INT
    );
}

#[test]
fn resolve_sorted() {
    // sorted(list[T1]) -> list[T1]
    let sig = p("(list[T1]) -> list[T1]");
    let ret = sig
        .resolve_call(&[ExprType::list(ExprType::STRING)])
        .unwrap();
    assert_eq!(ret, ExprType::list(ExprType::STRING));
}

#[test]
fn resolve_list_mul() {
    // list * int: (list[T1], int) -> list[T1]
    let sig = p("(list[T1], int) -> list[T1]");
    let ret = sig
        .resolve_call(&[ExprType::list(ExprType::FLOAT), ExprType::INT])
        .unwrap();
    assert_eq!(ret, ExprType::list(ExprType::FLOAT));
}

// -- is_symbolic / is_concrete for signatures --

#[test]
fn sig_is_symbolic() {
    assert!(p("(list[T1], int) -> T1").is_symbolic());
    assert!(!p("(string, string) -> bool").is_symbolic());
}

#[test]
fn sig_not_concrete() {
    assert!(!p("(string) -> int").is_concrete());
    assert!(!p("(T1) -> T1").is_concrete());
}

// ══════════════════════════════════════════════════════════════
// TestExprValue — ported from Python test_types.py
// ══════════════════════════════════════════════════════════════

use openjd_expr::value::{ExprValue, Float64};
use openjd_expr::PathFormat;

#[test]
fn value_from_bool() {
    let val = ExprValue::Bool(true);
    assert_eq!(val.expr_type(), ExprType::BOOL);
    assert_eq!(val, ExprValue::Bool(true));
    assert!(!matches!(val, ExprValue::Null));
}

#[test]
fn value_from_int() {
    let val = ExprValue::Int(42);
    assert_eq!(val.expr_type(), ExprType::INT);
    assert_eq!(val, ExprValue::Int(42));
}

#[test]
fn value_from_float() {
    let val = ExprValue::Float(Float64::new(1.2345).unwrap());
    assert_eq!(val.expr_type(), ExprType::FLOAT);
}

#[test]
fn value_from_float_with_original_str() {
    let val = ExprValue::Float(Float64::with_str(3.5, "3.500".to_string()).unwrap());
    assert_eq!(val.to_display_string(), "3.500");
}

#[test]
fn value_from_string() {
    let val = ExprValue::String("hello".to_string());
    assert_eq!(val.expr_type(), ExprType::STRING);
    assert_eq!(val.to_display_string(), "hello");
}

#[test]
fn value_from_path() {
    let val = ExprValue::new_path("/tmp/test".to_string(), PathFormat::Posix);
    assert_eq!(val.expr_type(), ExprType::PATH);
    assert_eq!(val.to_display_string(), "/tmp/test");
}

#[test]
fn value_from_list() {
    let items = vec![ExprValue::Int(1), ExprValue::Int(2)];
    let val = ExprValue::make_list(items, ExprType::INT).unwrap();
    assert_eq!(val.expr_type().code(), TypeCode::List);
    assert_eq!(val.list_len(), Some(2));
}

#[test]
fn value_null() {
    let val = ExprValue::Null;
    assert!(matches!(val, ExprValue::Null));
    assert_eq!(val.expr_type().code(), TypeCode::Null);
}

#[test]
fn value_to_string_bool() {
    assert_eq!(ExprValue::Bool(true).to_display_string(), "true");
    assert_eq!(ExprValue::Bool(false).to_display_string(), "false");
}

#[test]
fn value_to_string_int() {
    assert_eq!(ExprValue::Int(42).to_display_string(), "42");
    assert_eq!(ExprValue::Int(-5).to_display_string(), "-5");
}

#[test]
fn value_to_string_float() {
    assert_eq!(
        ExprValue::Float(Float64::new(1.2345).unwrap()).to_display_string(),
        "1.2345"
    );
}

#[test]
fn value_to_string_null() {
    assert_eq!(ExprValue::Null.to_display_string(), "null");
}

#[test]
fn value_equality() {
    assert_eq!(ExprValue::Int(5), ExprValue::Int(5));
    assert_ne!(ExprValue::Int(5), ExprValue::Int(6));
    assert_ne!(ExprValue::Int(5), ExprValue::String("5".to_string()));
}

#[test]
fn value_direct_construction() {
    assert_eq!(ExprValue::from(42i64).expr_type(), ExprType::INT);
    assert_eq!(
        ExprValue::Float(Float64::new(1.2345).unwrap()).expr_type(),
        ExprType::FLOAT
    );
    assert_eq!(ExprValue::from("hello").expr_type(), ExprType::STRING);
    assert_eq!(ExprValue::from(true).expr_type(), ExprType::BOOL);
    assert!(matches!(ExprValue::Null, ExprValue::Null));
}

#[test]
fn value_list_empty() {
    let val = ExprValue::make_list(vec![], ExprType::NULLTYPE).unwrap();
    assert_eq!(val.list_len(), Some(0));
}

#[test]
fn value_list_int_float_mix() {
    let items = vec![
        ExprValue::Int(1),
        ExprValue::Float(Float64::new(2.0).unwrap()),
        ExprValue::Int(3),
    ];
    let val = ExprValue::make_list(items, ExprType::FLOAT).unwrap();
    assert_eq!(val.expr_type(), ExprType::list(ExprType::FLOAT));
}

#[test]
fn value_list_nested() {
    let inner1 =
        ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Int(2)], ExprType::INT).unwrap();
    let inner2 =
        ExprValue::make_list(vec![ExprValue::Int(3), ExprValue::Int(4)], ExprType::INT).unwrap();
    let val = ExprValue::make_list(vec![inner1, inner2], ExprType::list(ExprType::INT)).unwrap();
    assert_eq!(
        val.expr_type(),
        ExprType::list(ExprType::list(ExprType::INT))
    );
}

#[test]
fn value_list_path_string_mix() {
    let items = vec![
        ExprValue::new_path("/a".to_string(), PathFormat::Posix),
        ExprValue::String("b".to_string()),
    ];
    let val = ExprValue::make_list(items, ExprType::STRING).unwrap();
    assert_eq!(val.expr_type(), ExprType::list(ExprType::STRING));
}

#[test]
fn value_type_coercion_str_to_int() {
    let val = ExprValue::from_str_coerce("42", &ExprType::INT, PathFormat::Posix).unwrap();
    assert_eq!(val, ExprValue::Int(42));
}

#[test]
fn value_type_coercion_str_to_float() {
    let val = ExprValue::from_str_coerce("1.2345", &ExprType::FLOAT, PathFormat::Posix).unwrap();
    assert_eq!(val.expr_type(), ExprType::FLOAT);
}

#[test]
fn value_type_coercion_str_to_bool() {
    let val_true = ExprValue::from_str_coerce("true", &ExprType::BOOL, PathFormat::Posix).unwrap();
    assert_eq!(val_true, ExprValue::Bool(true));
    let val_false =
        ExprValue::from_str_coerce("false", &ExprType::BOOL, PathFormat::Posix).unwrap();
    assert_eq!(val_false, ExprValue::Bool(false));
}

#[test]
fn value_type_coercion_str_to_path() {
    let val = ExprValue::from_str_coerce("/tmp", &ExprType::PATH, PathFormat::Posix).unwrap();
    assert_eq!(val.expr_type(), ExprType::PATH);
}

// ── ExprValue + unresolved ──

#[test]
fn value_unresolved_creation() {
    let v = ExprValue::unresolved(ExprType::INT);
    assert_eq!(v.expr_type(), p("unresolved[int]"));
    assert_eq!(v.expr_type().code(), TypeCode::Unresolved);
}

#[test]
fn value_unresolved_from_list_string_type() {
    let v = ExprValue::unresolved(ExprType::list(ExprType::STRING));
    assert_eq!(v.expr_type(), p("unresolved[list[string]]"));
}

#[test]
fn value_unresolved_equality() {
    assert_eq!(
        ExprValue::unresolved(ExprType::INT),
        ExprValue::unresolved(ExprType::INT)
    );
    assert_ne!(
        ExprValue::unresolved(ExprType::INT),
        ExprValue::unresolved(ExprType::FLOAT)
    );
    assert_ne!(ExprValue::unresolved(ExprType::INT), ExprValue::Null);
}

#[test]
fn value_unresolved_is_unresolved() {
    assert!(ExprValue::unresolved(ExprType::INT).is_unresolved());
    assert!(!ExprValue::Int(42).is_unresolved());
}
