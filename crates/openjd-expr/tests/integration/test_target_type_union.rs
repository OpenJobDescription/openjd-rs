// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for target_type union membership coercion.
//!
//! When `target_type` is a union, the expression result should be:
//!
//! 1. Returned as-is if it already satisfies the union — that is, if its type
//!    satisfies any member (RFC 0005 §"Implicit Type Coercion" — coercion is
//!    non-destructive where the intent is obvious; if the value already
//!    satisfies one of the targets, no coercion is needed).
//! 2. Otherwise, non-destructively converted toward one of the union's
//!    members, non-list members first.
//! 3. Otherwise, an error naming the whole union as the target.
//!
//! These tests exercise the public `EvalBuilder::with_target_type` and
//! `ExprValue::coerce` surfaces against union targets like `int | string`.

use openjd_expr::*;

fn eval_with_target_type(
    expr: &str,
    target: &ExprType,
    symtab: &SymbolTable,
) -> Result<ExprValue, ExpressionError> {
    ParsedExpression::new(expr)?
        .with_target_type(target)
        .evaluate(&[symtab])
}

/// Assert that `expr`, evaluated with `target` against `symtab`, fails
/// with a multi-line error whose concatenation contains `expected`.
/// Mirrors `assert_err` in `test_error_formatting.rs` — see AGENTS.md
/// "Test Quality Standard".
fn assert_eval_err(expr: &str, target: &ExprType, symtab: &SymbolTable, expected: &[&str]) {
    let e = eval_with_target_type(expr, target, symtab)
        .unwrap_err()
        .to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

fn parse_type(s: &str) -> ExprType {
    ExprType::parse(s).unwrap()
}

// ── Match-first: value type is already one of the union members ──────

#[test]
fn union_int_string_accepts_string_value_as_is() {
    // The exact case called out in the bindings report: a string value
    // against an `int | string` target should be returned unchanged.
    let r =
        eval_with_target_type("'42'", &parse_type("int | string"), &SymbolTable::new()).unwrap();
    assert_eq!(r, ExprValue::String("42".to_string()));
    assert_eq!(r.expr_type(), ExprType::STRING);
}

#[test]
fn union_int_string_accepts_int_value_as_is() {
    let r = eval_with_target_type("42", &parse_type("int | string"), &SymbolTable::new()).unwrap();
    assert_eq!(r, ExprValue::Int(42));
    assert_eq!(r.expr_type(), ExprType::INT);
}

#[test]
fn union_string_path_accepts_string_value_as_is() {
    let r = eval_with_target_type(
        "'/foo/bar'",
        &parse_type("string | path"),
        &SymbolTable::new(),
    )
    .unwrap();
    assert_eq!(r, ExprValue::String("/foo/bar".to_string()));
    assert_eq!(r.expr_type(), ExprType::STRING);
}

#[test]
fn union_int_float_accepts_int_value_as_is() {
    let r = eval_with_target_type("3", &parse_type("int | float"), &SymbolTable::new()).unwrap();
    assert_eq!(r, ExprValue::Int(3));
    assert_eq!(r.expr_type(), ExprType::INT);
}

#[test]
fn union_int_float_accepts_float_value_as_is() {
    let r = eval_with_target_type("3.5", &parse_type("int | float"), &SymbolTable::new()).unwrap();
    assert_eq!(r.expr_type(), ExprType::FLOAT);
    match r {
        ExprValue::Float(f) => assert_eq!(f.value(), 3.5),
        other => panic!("expected float, got {other:?}"),
    }
}

// ── Match-first via direct ExprValue::coerce, bypassing the evaluator ──

#[test]
fn coerce_string_to_int_or_string_union_returns_self() {
    let v = ExprValue::String("hello".to_string());
    let target = parse_type("int | string");
    let coerced = v.coerce(&target, PathFormat::Posix).unwrap();
    assert_eq!(coerced, ExprValue::String("hello".to_string()));
}

#[test]
fn coerce_int_to_int_or_string_union_returns_self() {
    let v = ExprValue::Int(99);
    let target = parse_type("int | string");
    let coerced = v.coerce(&target, PathFormat::Posix).unwrap();
    assert_eq!(coerced, ExprValue::Int(99));
}

#[test]
fn coerce_bool_to_int_or_bool_union_returns_self() {
    // bool/int are not interchangeable in EXPR (RFC 0005), so this
    // checks that `bool` against `int | bool` returns the bool value
    // unchanged rather than coercing to int.
    let v = ExprValue::Bool(true);
    let target = parse_type("int | bool");
    let coerced = v.coerce(&target, PathFormat::Posix).unwrap();
    assert_eq!(coerced, ExprValue::Bool(true));
}

// ── Per-member coercion: no member matches by identity but a non-destructive
//    coercion to one of them works ─────────────────────────────────────

#[test]
fn coerce_int_to_string_or_path_union_picks_string() {
    // int doesn't match string or path directly. `string | path` offers two
    // candidate destinations, but only one has a rule for an int source —
    // int → string — so RFC 0005's per-source destination order is not even
    // needed here; the string destination is the only one that can convert.
    let v = ExprValue::Int(42);
    let target = parse_type("string | path");
    let coerced = v.coerce(&target, PathFormat::Posix).unwrap();
    assert_eq!(coerced, ExprValue::String("42".to_string()));
}

#[test]
fn outer_target_int_or_path_coerces_int_param_to_string_via_member() {
    // `Param.A` returns int; target `string | path` has no int member;
    // int → string is a valid non-destructive coercion; result is a
    // string. Exercises the per-member coercion path through the
    // public `EvalBuilder::with_target_type` surface, with a single
    // attribute lookup so the outer target type only affects the final
    // coercion (not operand evaluation, which is a separate concern —
    // see test_target_type_propagation.rs).
    let mut st = SymbolTable::new();
    st.set("Param.A", ExprValue::Int(30)).unwrap();
    let r = eval_with_target_type("Param.A", &parse_type("string | path"), &st).unwrap();
    assert_eq!(r, ExprValue::String("30".to_string()));
}

// ── Errors when no member matches and no scalar coercion succeeds ─────

#[test]
fn list_to_int_or_string_union_errors() {
    // A list value cannot satisfy a scalar-only union by either match
    // or non-destructive coercion. The error includes the source
    // expression and a caret pointing at the list literal.
    assert_eval_err(
        "[1, 2, 3]",
        &parse_type("int | string"),
        &SymbolTable::new(),
        &[
            "Cannot coerce list[int] to int | string\n",
            "  [1, 2, 3]\n",
            "  ^~~~~~~~~",
        ],
    );
}

#[test]
fn path_to_int_or_float_union_errors() {
    // Path cannot be coerced to int or float. Use a symbol-table path
    // value (rather than `path('/foo')`) so the error caret points at
    // the variable reference, not at the function-call argument
    // (function-call arguments are evaluated with the parent target
    // type, which is a separate concern from union coercion).
    //
    // Pin the evaluator to `PathFormat::Posix` so the path value's
    // format matches regardless of the host OS — otherwise the
    // path-format-mismatch check fires first on Windows and we'd
    // never reach the coerce step.
    let mut st = SymbolTable::new();
    st.set("Param.P", ExprValue::new_path("/foo", PathFormat::Posix))
        .unwrap();
    let parsed = ParsedExpression::new("Param.P").unwrap();
    let target = parse_type("int | float");
    let err = parsed
        .with_target_type(&target)
        .with_path_format(PathFormat::Posix)
        .evaluate(&[&st])
        .unwrap_err()
        .to_string();
    let joined = [
        "Cannot coerce path to float | int\n",
        "  Param.P\n",
        "  ~~~~~~^",
    ]
    .concat();
    assert!(err.contains(&joined), "got:\n{err}\nexpected:\n{joined}");
}

// ── Optional types `T?` ── these are unions with nulltype ─────────────

#[test]
fn optional_string_accepts_string() {
    let r = eval_with_target_type("'hi'", &parse_type("string?"), &SymbolTable::new()).unwrap();
    assert_eq!(r, ExprValue::String("hi".to_string()));
}

#[test]
fn optional_string_accepts_null() {
    let r = eval_with_target_type("null", &parse_type("string?"), &SymbolTable::new()).unwrap();
    assert_eq!(r, ExprValue::Null);
}

#[test]
fn unresolved_nullable_sources_keep_successful_possibilities() {
    let cases = [
        ("WrappedAction.Timeout", "int?", "int", "int"),
        ("WrappedAction.Timeout", "int?", "int | string", "int"),
        (
            "WrappedAction.Cancelation.Mode",
            "string?",
            "int | string",
            "string",
        ),
        ("U.IntOpt", "int?", "int? | list[int]", "int?"),
        (
            "U.IntOptOrList",
            "int? | list[int]",
            "int | list[int]",
            "int | list[int]",
        ),
        (
            "U.StringOptOrList",
            "string? | list[string]",
            "string?",
            "string?",
        ),
    ];

    for (name, source, target, expected) in cases {
        let mut st = SymbolTable::new();
        st.set(name, ExprValue::unresolved(parse_type(source)))
            .unwrap();
        let result = eval_with_target_type(name, &parse_type(target), &st).unwrap();
        assert_eq!(
            result,
            ExprValue::unresolved(parse_type(expected)),
            "symbol={name}, source={source}, target={target}"
        );
    }
}

#[test]
fn wrapped_action_timeout_format_string_accepts_union_target() {
    let mut st = SymbolTable::new();
    st.set(
        "WrappedAction.Timeout",
        ExprValue::unresolved(parse_type("int?")),
    )
    .unwrap();
    let target = parse_type("int | string");
    let options = FormatStringOptions::default().with_target_type(&target);
    let result = FormatString::new("{{ WrappedAction.Timeout }}")
        .unwrap()
        .resolve_with(&st, &options)
        .unwrap();
    assert_eq!(result, ExprValue::unresolved(ExprType::INT));
}

#[test]
fn nullable_list_elements_narrow_from_list_target() {
    let mut st = SymbolTable::new();
    st.set("U.IntOpt", ExprValue::unresolved(parse_type("int?")))
        .unwrap();
    let result =
        eval_with_target_type("[U.IntOpt, U.IntOpt]", &parse_type("list[int]"), &st).unwrap();
    assert_eq!(result, ExprValue::unresolved(parse_type("list[int]")));
}

#[test]
fn optional_int_coerces_string_to_int() {
    // `int?` is `int | nulltype`. A string value matches neither
    // member directly. The per-member loop then tries `int` and `string`
    // → `int` is a valid non-destructive coercion, so the result is
    // `Int(42)`.
    let r = eval_with_target_type("'42'", &parse_type("int?"), &SymbolTable::new()).unwrap();
    assert_eq!(r, ExprValue::Int(42));
}

// ── Union targets accept whatever their members accept ────────────────

/// A union target must accept at least everything its members accept.
/// Previously list members were skipped entirely, so adding an alternative
/// to a target could *remove* an accepted conversion.
#[test]
fn union_target_accepts_what_its_list_member_accepts() {
    let range = ExprValue::RangeExpr(RangeExpr::from_values(vec![1, 2, 3]).unwrap());
    let cases = [
        // (value, bare target, same target with an alternative added)
        (
            ExprValue::ListInt(vec![1, 2, 3]),
            "list[float]",
            "list[float] | string",
        ),
        (range, "list[int]", "list[int] | list[float]"),
    ];
    for (value, bare, with_alternative) in cases {
        let direct = value
            .clone()
            .coerce(&parse_type(bare), PathFormat::Posix)
            .unwrap();
        let via_union = value
            .clone()
            .coerce(&parse_type(with_alternative), PathFormat::Posix)
            .unwrap_or_else(|e| panic!("{value:?} -> {with_alternative}: {e}"));
        assert_eq!(direct, via_union, "{value:?}: {bare} vs {with_alternative}");
    }
}

/// Non-list destinations are tried before list ones. Converting to a scalar
/// produces a single value whose cost does not depend on the source;
/// converting to a list materializes elements and can fail on size limits. A
/// range expression's canonical form already *is* a string, so
/// `list[int] | string` yields the string rather than expanding the range.
#[test]
fn union_target_prefers_scalar_destination_over_list() {
    let range = ExprValue::RangeExpr(RangeExpr::from_values(vec![1, 2, 3]).unwrap());
    let r = range
        .clone()
        .coerce(&parse_type("list[int] | string"), PathFormat::Posix)
        .unwrap();
    assert_eq!(r, ExprValue::String("1-3".to_string()));
    // With no scalar destination available, the list conversion still runs.
    let r = range
        .coerce(&parse_type("list[int] | bool"), PathFormat::Posix)
        .unwrap();
    assert_eq!(r, ExprValue::ListInt(vec![1, 2, 3]));
}

/// The type level cannot see the payload that decides which destination a
/// concrete value reaches, so against a union target it narrows to the union
/// of every destination with a rule — a sound description of every value the
/// coercion could produce. See `specs/expr/values.md`.
#[test]
fn unresolved_union_target_narrows_to_all_rule_destinations() {
    let mut st = SymbolTable::new();
    st.set("U.Range", ExprValue::unresolved(parse_type("range_expr")))
        .unwrap();
    let r = eval_with_target_type("U.Range", &parse_type("list[int] | string"), &st).unwrap();
    // A range_expr converts to `string` (its canonical form) and to
    // `list[int]` (materialization) — the payload's length cap decides.
    assert_eq!(r, ExprValue::unresolved(parse_type("list[int] | string")));
}

// ── Targets that are not sets of values ──────────────────────────────

/// Type variables are bound by signature matching before any value is
/// coerced, so reaching coercion with one is always an error — on the
/// concrete and the unresolved path alike, and at any nesting depth.
#[test]
fn type_variable_and_noreturn_targets_are_rejected() {
    for target in [
        "T1",
        "list[T1]",
        "noreturn",
        "list[list[T1]]",
        "T1 | list[T1]",
    ] {
        let target = parse_type(target);
        // Assert the full message, per AGENTS.md: an unusable target reports
        // the target itself, not a rule-specific hint about how the value
        // might have converted, since no rule applies to it at all.
        for (value, source) in [
            (ExprValue::Int(5), "int"),
            (ExprValue::ListInt(vec![1]), "list[int]"),
            (
                ExprValue::RangeExpr(RangeExpr::from_values(vec![1, 2, 3]).unwrap()),
                "range_expr",
            ),
            (ExprValue::unresolved(ExprType::INT), "int"),
            (ExprValue::unresolved(parse_type("list[int]")), "list[int]"),
        ] {
            assert_eq!(
                value.coerce(&target, PathFormat::Posix).unwrap_err(),
                format!("Cannot coerce {source} to {target}")
            );
        }
    }
}

/// A list destination's element type must be *fully* bindable, not merely
/// usable in part. `list[int | T1]` denotes runtime values through its `int`
/// member, but producing a list for that destination would carry the unbound
/// `T1` into the result's element type, breaking the guarantee that a
/// coercion result satisfies the target it was coerced to. Both paths reject
/// it, so the two stay in agreement.
#[test]
fn list_destination_with_a_partly_symbolic_element_is_rejected() {
    let target = parse_type("list[int | T1]");
    for (value, source) in [
        (ExprValue::ListString(vec!["5".into()], 1), "list[string]"),
        (ExprValue::ListBool(vec![true]), "list[bool]"),
        (
            ExprValue::unresolved(parse_type("list[string]")),
            "list[string]",
        ),
        (
            ExprValue::unresolved(parse_type("list[bool]")),
            "list[bool]",
        ),
    ] {
        assert_eq!(
            value.coerce(&target, PathFormat::Posix).unwrap_err(),
            format!("Cannot coerce {source} to {target}")
        );
    }
}

/// `ExprType::new` can build a param-less `unresolved`, which constrains
/// nothing. `satisfies` must not index into its missing constraint.
#[test]
fn satisfies_does_not_panic_on_a_param_less_unresolved() {
    let bare = ExprType::new(TypeCode::Unresolved, vec![]);
    assert!(!bare.satisfies(&ExprType::INT));
    assert!(bare.satisfies(&ExprType::ANY));
}

/// A union is the one place an unusable member is tolerated: the value only
/// has to land in one member, so a usable one alongside a type variable still
/// works. Such a union cannot arise from signature dispatch — `call_arg_targets`
/// drops symbolic positions before building a union — but the rule keeps
/// membership decisions per-member rather than all-or-nothing.
#[test]
fn union_with_one_usable_member_still_coerces() {
    let target = parse_type("list[int] | T1");
    let coerced = ExprValue::ListInt(vec![1])
        .coerce(&target, PathFormat::Posix)
        .unwrap();
    assert_eq!(coerced, ExprValue::ListInt(vec![1]));
}

// ── Unbound type variables in a source read as wildcards ──────────────

/// A concrete value can never have a type variable in its type, so an
/// unbound variable inside an unresolved source constraint reads as a
/// wildcard: `T1` behaves as `any`, and `list[T1]` as `list[any]` (see
/// specs/expr/values.md). The shape around the variable still constrains
/// the outcome — a list of unknown element type is still a list, and no
/// list coerces to a scalar.
#[test]
fn symbolic_source_keeps_its_shape() {
    let err = ExprValue::unresolved(parse_type("list[T1]"))
        .coerce(&parse_type("int"), PathFormat::Posix)
        .unwrap_err();
    assert_eq!(err, "Cannot coerce list[T1] to int");
    let converted = ExprValue::unresolved(parse_type("list[T1]"))
        .coerce(&parse_type("list[string]"), PathFormat::Posix)
        .unwrap();
    assert_eq!(converted, ExprValue::unresolved(parse_type("list[string]")));
}

#[test]
fn bare_type_variable_source_reads_as_any() {
    let coerced = ExprValue::unresolved(parse_type("T1"))
        .coerce(&parse_type("int"), PathFormat::Posix)
        .unwrap();
    assert_eq!(coerced, ExprValue::unresolved(ExprType::INT));
}

/// The promise for an unknown source covers only the target members a
/// value could land in: `T1` in `int | T1` denotes no runtime values, so
/// it is filtered out rather than copied into the constraint — copying it
/// would produce a constraint that does not satisfy the target.
#[test]
fn unknown_source_promise_filters_unusable_union_members() {
    let coerced = ExprValue::unresolved(parse_type("any"))
        .coerce(&parse_type("int | T1"), PathFormat::Posix)
        .unwrap();
    assert_eq!(coerced, ExprValue::unresolved(ExprType::INT));
}

/// A `List`-coded type built via `ExprType::new` with zero or several
/// parameters is malformed — no concrete value has such a type — so the
/// list/list rule's empty-list-could-convert argument does not apply, and
/// coercion rejects it rather than laundering it into a well-formed list
/// type.
#[test]
fn malformed_list_source_is_rejected() {
    use openjd_expr::types::TypeCode;
    for malformed in [
        ExprType::new(TypeCode::List, vec![]),
        ExprType::new(TypeCode::List, vec![ExprType::INT, ExprType::STRING]),
    ] {
        let err = ExprValue::unresolved(malformed.clone())
            .coerce(&parse_type("list[int]"), PathFormat::Posix)
            .unwrap_err();
        assert_eq!(err, format!("Cannot coerce {malformed} to list[int]"));
    }
}

// ── nulltype is satisfied, never converted into ───────────────────────

/// RFC 0005 lists no `X → nulltype` conversion, and discounts `nulltype` when
/// counting a union target's candidate scalar types. So `null` is reached only
/// by already being `null`. Turning the *text* `"null"` into `null` is a
/// transport-decode rule belonging to `from_str_coerce`, and it must not fire
/// as an implicit coercion — neither for a bare `nulltype` target nor inside a
/// union such as `int?`.
#[test]
fn string_null_does_not_implicitly_coerce_to_null() {
    // A bare `nulltype` target has no destination at all, so nothing converts.
    for target in ["nulltype", "int?", "int | nulltype"] {
        let target = parse_type(target);
        let err = ExprValue::String("null".to_string())
            .coerce(&target, PathFormat::Posix)
            .unwrap_err();
        assert_eq!(err, format!("Cannot coerce string to {target}"), "{target}");
    }
    // Discounting `nulltype` lets a real conversion win instead: every string
    // is a valid path, so `path?` yields the path, not `null`.
    assert_eq!(
        ExprValue::String("null".to_string())
            .coerce(&parse_type("path? | list[path]"), PathFormat::Posix)
            .unwrap(),
        ExprValue::new_path("null", PathFormat::Posix)
    );
    // At the type level, a bare `nulltype` target is rejected the same way,
    // while `int?` still defers the parse check to resolution.
    assert_eq!(
        ExprValue::unresolved(ExprType::STRING)
            .coerce(&ExprType::NULLTYPE, PathFormat::Posix)
            .unwrap_err(),
        "Cannot coerce string to nulltype"
    );
    assert_eq!(
        ExprValue::unresolved(ExprType::STRING)
            .coerce(&parse_type("int?"), PathFormat::Posix)
            .unwrap(),
        ExprValue::unresolved(ExprType::INT)
    );
    // Transport decoding is unaffected: this is the entry point from outside
    // the expression language, where strings are the only representation.
    assert_eq!(
        ExprValue::from_str_coerce("null", &ExprType::NULLTYPE, PathFormat::Posix).unwrap(),
        ExprValue::Null
    );
}

/// The `null` *value* is unaffected — it satisfies any union with a `nulltype`
/// member and passes through untouched.
#[test]
fn null_value_still_satisfies_optional_targets() {
    for target in ["nulltype", "int?", "string?", "path? | list[path]"] {
        let target = parse_type(target);
        assert_eq!(
            ExprValue::Null.coerce(&target, PathFormat::Posix).unwrap(),
            ExprValue::Null,
            "target={target}"
        );
    }
}

/// RFC 0005 §"None/null Semantics": a list item's target type is
/// `T? | list[T]`. That union has a `nulltype` member, a scalar member, and a
/// list member all at once, so it exercises every destination rule together.
#[test]
fn list_item_target_shape_from_the_spec() {
    let target = parse_type("string? | list[string]");
    let cases: [(ExprValue, ExprValue); 5] = [
        // Already acceptable — each satisfies a member, so passes through.
        (ExprValue::Null, ExprValue::Null),
        (ExprValue::String("x".into()), ExprValue::String("x".into())),
        (
            ExprValue::ListString(vec!["a".into()], 1),
            ExprValue::ListString(vec!["a".into()], 1),
        ),
        // Converted: `string` is the only candidate scalar, `nulltype` and the
        // list member being discounted for a scalar value.
        (ExprValue::Int(42), ExprValue::String("42".into())),
        // Converted element-wise toward the single list candidate.
        (
            ExprValue::ListInt(vec![1, 2]),
            ExprValue::ListString(vec!["1".into(), "2".into()], 2),
        ),
    ];
    for (value, expected) in cases {
        assert_eq!(
            value.clone().coerce(&target, PathFormat::Posix).unwrap(),
            expected,
            "value={value:?}"
        );
    }
}
