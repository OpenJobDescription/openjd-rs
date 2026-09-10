// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Integration tests for format string interpolation ({{Param.Name}} and {{Expr.Name}} syntax).
//! Complements the inline unit tests in src/format_string.rs by exercising end-to-end
//! resolution through the expression evaluator with real symbol tables.

use openjd_expr::{
    symtab, ExprProfile, ExprType, ExprValue, FormatString, FormatStringOptions, FunctionLibrary,
    HostContext, SymbolTable,
};

fn resolve_str(input: &str, st: &SymbolTable) -> String {
    FormatString::new(input)
        .unwrap()
        .resolve_string_with(st, &FormatStringOptions::default())
        .unwrap()
}

fn resolve_val(input: &str, st: &SymbolTable) -> ExprValue {
    FormatString::new(input)
        .unwrap()
        .resolve_with(st, &FormatStringOptions::default())
        .unwrap()
}

fn resolve_err(input: &str, st: &SymbolTable) -> String {
    FormatString::new(input)
        .unwrap()
        .resolve_string_with(st, &FormatStringOptions::default())
        .unwrap_err()
        .to_string()
}

fn parse_err(input: &str) -> String {
    FormatString::new(input).unwrap_err().to_string()
}

// === Parsing ===

#[test]
fn parse_literal_only() {
    let fs = FormatString::new("no interpolation here").unwrap();
    assert!(fs.is_literal());
}

#[test]
fn parse_single_simple_name() {
    let fs = FormatString::new("{{Param.Frame}}").unwrap();
    assert!(!fs.is_literal());
    assert_eq!(fs.expression_names(), vec!["Param.Frame"]);
}

#[test]
fn parse_multiple_interpolations() {
    let fs = FormatString::new("{{Param.A}}_{{Param.B}}").unwrap();
    assert_eq!(fs.expression_names(), vec!["Param.A", "Param.B"]);
}

#[test]
fn parse_complex_expression_not_in_names() {
    let fs = FormatString::new("{{Param.X + 1}}").unwrap();
    assert!(fs.has_complex_expressions());
    // Complex expressions are not returned by expression_names()
    assert!(fs.expression_names().is_empty());
}

#[test]
fn parse_empty_expression_error() {
    let err = parse_err("{{}}");
    assert!(err.contains("Empty expression"), "got: {err}");
}

#[test]
fn parse_missing_close_braces() {
    let err = parse_err("{{Param.X");
    assert!(err.contains("Braces mismatch"), "got: {err}");
}

#[test]
fn parse_missing_open_braces() {
    let err = parse_err("Param.X}}");
    assert!(err.contains("Missing opening braces"), "got: {err}");
}

// === Simple name resolution ===

#[test]
fn resolve_simple_string_param() {
    let st = symtab!("Param.Name" => "shot_01");
    assert_eq!(resolve_str("render_{{Param.Name}}", &st), "render_shot_01");
}

#[test]
fn resolve_simple_int_param() {
    let st = symtab!("Param.Frame" => 42);
    assert_eq!(resolve_str("frame_{{Param.Frame}}", &st), "frame_42");
}

#[test]
fn resolve_undefined_variable_error() {
    let err = resolve_err("{{Param.Missing}}", &SymbolTable::new());
    assert!(err.contains("Undefined variable"), "got: {err}");
}

// === Expression resolution ===

#[test]
fn resolve_arithmetic_expression() {
    let st = symtab!("Param.Frame" => 10);
    assert_eq!(resolve_str("{{Param.Frame + 1}}", &st), "11");
}

#[test]
fn resolve_string_method_expression() {
    let st = symtab!("Param.Name" => "hello");
    assert_eq!(resolve_str("{{Param.Name.upper()}}", &st), "HELLO");
}

#[test]
fn resolve_conditional_expression() {
    let st = symtab!("Param.X" => 5);
    assert_eq!(resolve_str("{{Param.X if Param.X > 3 else 0}}", &st), "5");
}

// === Typed resolution ===

#[test]
fn resolve_typed_single_expr_preserves_int() {
    let st = symtab!("Param.X" => 42);
    let val = resolve_val("{{Param.X}}", &st);
    assert!(matches!(val, ExprValue::Int(42)));
}

#[test]
fn resolve_typed_mixed_becomes_string() {
    let st = symtab!("Param.X" => 42);
    let val = resolve_val("prefix_{{Param.X}}", &st);
    assert!(matches!(val, ExprValue::String(ref s) if s == "prefix_42"));
}

#[test]
fn resolve_typed_single_expr_preserves_list() {
    let st = symtab!(
        "Param.Items" => ExprValue::make_list(
            vec![ExprValue::Int(1), ExprValue::Int(2), ExprValue::Int(3)],
            ExprType::INT,
        ).unwrap()
    );
    let val = resolve_val("{{Param.Items}}", &st);
    assert!(val.is_list());
    assert_eq!(val.to_display_string(), "[1, 2, 3]");
}

// === Validation ===

#[test]
fn validate_catches_undefined_variable() {
    let fs = FormatString::new("{{Param.Missing}}").unwrap();
    let lib = FunctionLibrary::for_profile(&ExprProfile::current().with_host_context(
        HostContext::with_rules(Vec::<openjd_expr::PathMappingRule>::new()),
    ));
    let result = fs.validate_expressions(&SymbolTable::new(), &lib, None);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Param.Missing"), "got: {err}");
}

#[test]
fn validate_passes_with_unresolved_types() {
    let fs = FormatString::new("{{Param.X + 1}}").unwrap();
    let st = symtab!("Param.X" => ExprValue::unresolved(ExprType::INT));
    let lib = FunctionLibrary::for_profile(&ExprProfile::current().with_host_context(
        HostContext::with_rules(Vec::<openjd_expr::PathMappingRule>::new()),
    ));
    assert!(fs.validate_expressions(&st, &lib, None).is_ok());
}

// === StaticResolution (lower bound + static value) ===

fn static_resolution(input: &str, st: &SymbolTable) -> openjd_expr::StaticResolution {
    let lib = FunctionLibrary::for_profile(&ExprProfile::current().with_host_context(
        HostContext::with_rules(Vec::<openjd_expr::PathMappingRule>::new()),
    ));
    FormatString::new(input)
        .unwrap()
        .validate_expressions(st, &lib, None)
        .unwrap()
}

#[test]
fn static_resolution_literal_only() {
    let sr = static_resolution("hello", &SymbolTable::new());
    assert_eq!(sr.min_resolved_string_len, 5);
    assert!(matches!(sr.resolved_value, Some(ExprValue::String(ref s)) if s == "hello"));
}

#[test]
fn static_resolution_fully_static_expression() {
    let sr = static_resolution("{{ 'A' * 5 }}", &SymbolTable::new());
    assert_eq!(sr.min_resolved_string_len, 5);
    assert!(matches!(sr.resolved_value, Some(ExprValue::String(ref s)) if s == "AAAAA"));
}

#[test]
fn static_resolution_single_expression_keeps_typed_value() {
    let sr = static_resolution("{{ 1 + 2 }}", &SymbolTable::new());
    assert_eq!(sr.min_resolved_string_len, 1); // "3"
    assert!(matches!(sr.resolved_value, Some(ExprValue::Int(3))));
}

#[test]
fn static_resolution_multi_segment_concatenates() {
    let sr = static_resolution("x{{ 'A' * 3 }}y", &SymbolTable::new());
    assert_eq!(sr.min_resolved_string_len, 5);
    assert!(matches!(sr.resolved_value, Some(ExprValue::String(ref s)) if s == "xAAAy"));
}

#[test]
fn static_resolution_unresolved_contributes_zero() {
    // The static suffix bounds the resolved length even though the
    // Session.* prefix is unknown until run time.
    let st = symtab!("Session.WorkingDirectory" => ExprValue::unresolved(ExprType::PATH));
    let sr = static_resolution("{{ Session.WorkingDirectory }}/{{ 'A' * 4 }}", &st);
    assert_eq!(sr.min_resolved_string_len, 5); // "/" + "AAAA"
    assert!(sr.resolved_value.is_none());
}

#[test]
fn static_resolution_fully_unresolved() {
    let st = symtab!("Param.X" => ExprValue::unresolved(ExprType::STRING));
    let sr = static_resolution("{{ Param.X }}", &st);
    assert_eq!(sr.min_resolved_string_len, 0);
    assert!(sr.resolved_value.is_none());
}

#[test]
fn static_resolution_null_interpolates_as_empty() {
    let sr = static_resolution("a{{ null }}b", &SymbolTable::new());
    assert_eq!(sr.min_resolved_string_len, 2);
    assert!(matches!(sr.resolved_value, Some(ExprValue::String(ref s)) if s == "ab"));
}

#[test]
fn static_resolution_single_null_expression_is_typed_null() {
    let sr = static_resolution("{{ null }}", &SymbolTable::new());
    assert_eq!(sr.min_resolved_string_len, 0);
    assert!(matches!(sr.resolved_value, Some(ExprValue::Null)));
}

#[test]
fn static_resolution_concrete_list_value() {
    let sr = static_resolution("{{ [1, 2, 3] }}", &SymbolTable::new());
    let val = sr.resolved_value.expect("fully static");
    assert!(val.is_list());
    // Bound matches the interpolated display form ("[1, 2, 3]").
    assert_eq!(
        sr.min_resolved_string_len,
        val.to_display_string().chars().count()
    );
}

#[test]
fn static_resolution_list_with_unresolved_element_is_not_static() {
    let st = symtab!("Param.X" => ExprValue::unresolved(ExprType::STRING));
    let sr = static_resolution("{{ [Param.X, 'a'] }}", &st);
    assert!(sr.resolved_value.is_none());
    assert_eq!(sr.min_resolved_string_len, 0);
}

#[test]
fn static_resolution_len_counts_characters_not_bytes() {
    let sr = static_resolution("{{ 'é' * 4 }}", &SymbolTable::new());
    assert_eq!(sr.min_resolved_string_len, 4);
}

// === StaticResolution under a target type (RFC 0005 coercion) ===
//
// `resolve_with` coerces the root value of a single-expression format
// string toward the caller's target type (e.g. a float literal in an
// INT-typed template field resolves to `1`, not `1.0`). The
// StaticResolution contract — `min_resolved_string_len` is a lower bound on the
// length of any string the format string can resolve to, and
// `resolved_value` is the exact resolved value when everything is concrete
// — must therefore hold for the *coerced* resolution a typed field
// performs, not just the untyped one.

/// Validate and resolve the same format string the way a typed template
/// field would, and assert the StaticResolution contract holds for that
/// resolution: the bound never exceeds the actual resolved length, and a
/// concrete `resolved_value` interpolates identically to the resolved value.
fn check_static_resolution_against_typed_resolution(
    input: &str,
    st: &SymbolTable,
    target: &ExprType,
) -> openjd_expr::StaticResolution {
    let lib = FunctionLibrary::for_profile(&ExprProfile::current().with_host_context(
        HostContext::with_rules(Vec::<openjd_expr::PathMappingRule>::new()),
    ));
    let fs = FormatString::new(input).unwrap();
    let sr = fs.validate_expressions(st, &lib, Some(target)).unwrap();
    let resolved = fs
        .resolve_with(
            st,
            &FormatStringOptions::default()
                .with_library(&*lib)
                .with_target_type(target),
        )
        .unwrap();
    let resolved_display = resolved.to_display_string();
    let resolved_len = resolved_display.chars().count();
    assert!(
        sr.min_resolved_string_len <= resolved_len,
        "min_resolved_string_len ({}) exceeds the actual resolved length ({}) for {input:?} \
         with target type {target}: resolves to {resolved_display:?}",
        sr.min_resolved_string_len,
        resolved_len,
    );
    if let Some(ref sv) = sr.resolved_value {
        assert_eq!(
            sv.to_display_string(),
            resolved_display,
            "resolved_value {sv:?} does not match the resolved value for {input:?} \
             with target type {target}",
        );
        // For a fully concrete resolution, resolved_type must be the type
        // of the value resolution actually produces.
        assert_eq!(
            sr.resolved_type,
            resolved.expr_type(),
            "resolved_type does not match the resolved value's type for {input:?} \
             with target type {target}",
        );
    }
    sr
}

#[test]
fn static_resolution_bound_holds_for_float_literal_in_int_field() {
    // {{ 1.0 }} in an INT-typed field resolves to "1" (1 char).
    let sr = check_static_resolution_against_typed_resolution(
        "{{ 1.0 }}",
        &SymbolTable::new(),
        &ExprType::INT,
    );
    assert_eq!(sr.min_resolved_string_len, 1);
    assert!(matches!(sr.resolved_value, Some(ExprValue::Int(1))));
    assert_eq!(sr.resolved_type, ExprType::INT);
}

#[test]
fn static_resolution_bound_holds_for_multi_digit_float_literal_in_int_field() {
    // {{ 100.0 }} in an INT-typed field resolves to "100" (3 chars).
    let sr = check_static_resolution_against_typed_resolution(
        "{{ 100.0 }}",
        &SymbolTable::new(),
        &ExprType::INT,
    );
    assert_eq!(sr.min_resolved_string_len, 3);
    assert!(matches!(sr.resolved_value, Some(ExprValue::Int(100))));
    assert_eq!(sr.resolved_type, ExprType::INT);
}

#[test]
fn static_resolution_bound_holds_for_int_literal_in_float_field() {
    // {{ 1 }} in a FLOAT-typed field resolves to "1.0" (3 chars).
    let sr = check_static_resolution_against_typed_resolution(
        "{{ 1 }}",
        &SymbolTable::new(),
        &ExprType::FLOAT,
    );
    assert_eq!(sr.min_resolved_string_len, 3);
    assert!(matches!(sr.resolved_value, Some(ExprValue::Float(ref f)) if f.value() == 1.0));
    assert_eq!(sr.resolved_type, ExprType::FLOAT);
}

#[test]
fn static_resolution_bound_holds_for_bool_literal_in_string_field() {
    // {{ true }} in a STRING-typed field resolves to "true" (4 chars).
    let sr = check_static_resolution_against_typed_resolution(
        "{{ true }}",
        &SymbolTable::new(),
        &ExprType::STRING,
    );
    assert_eq!(sr.min_resolved_string_len, 4);
    assert!(matches!(sr.resolved_value, Some(ExprValue::String(ref s)) if s == "true"));
    // Coerced toward the string target.
    assert_eq!(sr.resolved_type, ExprType::STRING);
}

// === StaticResolution.resolved_type ===
//
// resolved_type is the static type the format string resolves to under the
// given target — the type of the value resolution will eventually produce.
// The run-time value is never unresolved, so unresolved[T] placeholders
// read through to their constraint T. This is what lets a consumer tell
// "this field resolves to a string, so min_resolved_string_len is a true length
// bound" from "this resolves to a typed list, whose display form the bound
// merely measures" — even when resolved_value is None.

#[test]
fn resolved_type_is_string_for_multi_segment() {
    let sr = static_resolution("x{{ 1 + 2 }}", &SymbolTable::new());
    assert_eq!(sr.resolved_type, ExprType::STRING);
}

#[test]
fn resolved_type_is_string_for_literal_only() {
    let sr = static_resolution("hello", &SymbolTable::new());
    assert_eq!(sr.resolved_type, ExprType::STRING);
}

#[test]
fn resolved_type_is_list_for_single_list_expression() {
    // The list case the report flags: min_resolved_string_len measures the display
    // form, and resolved_type tells the consumer it is not a plain string.
    let sr = static_resolution("{{ [1, 2, 3] }}", &SymbolTable::new());
    assert_eq!(sr.resolved_type, ExprType::list(ExprType::INT));
}

#[test]
fn resolved_type_available_for_unresolved_single_expression() {
    // resolved_value is None because the value is unknown, but the type
    // the resolution will eventually produce is known: string. The
    // unresolved marker is a validation-time artifact and reads through.
    let st = symtab!("Param.X" => ExprValue::unresolved(ExprType::STRING));
    let sr = static_resolution("{{ Param.X }}", &st);
    assert!(sr.resolved_value.is_none());
    assert_eq!(sr.resolved_type, ExprType::STRING);
}

#[test]
fn resolved_type_reflects_target_coercion_of_unresolved() {
    // An unresolved FLOAT coerced toward an INT target reports the coerced
    // type the resolution will produce.
    let lib = FunctionLibrary::for_profile(&ExprProfile::current().with_host_context(
        HostContext::with_rules(Vec::<openjd_expr::PathMappingRule>::new()),
    ));
    let st = symtab!("Param.X" => ExprValue::unresolved(ExprType::FLOAT));
    let sr = FormatString::new("{{ Param.X }}")
        .unwrap()
        .validate_expressions(&st, &lib, Some(&ExprType::INT))
        .unwrap();
    assert!(sr.resolved_value.is_none());
    assert_eq!(sr.resolved_type, ExprType::INT);
}

// === Union targets: type known, value unknown (args-shaped fields) ===
//
// Fields like a command's `args` entries accept `nulltype | string |
// list[string]`. At validation time the model seeds symbols like
// `WrappedAction.Args` as `unresolved(list[string])` — the type is known,
// the value is not. These tests pin what StaticResolution reports for
// that combination under the union target.

/// The target type of an args-like field: `nulltype | string | list[string]`.
fn args_target() -> ExprType {
    ExprType::union(vec![
        ExprType::NULLTYPE,
        ExprType::STRING,
        ExprType::list(ExprType::STRING),
    ])
}

fn static_resolution_with_target(
    input: &str,
    st: &SymbolTable,
    target: &ExprType,
) -> openjd_expr::StaticResolution {
    let lib = FunctionLibrary::for_profile(&ExprProfile::current().with_host_context(
        HostContext::with_rules(Vec::<openjd_expr::PathMappingRule>::new()),
    ));
    FormatString::new(input)
        .unwrap()
        .validate_expressions(st, &lib, Some(target))
        .unwrap()
}

#[test]
fn args_union_target_unresolved_list_of_string_satisfies() {
    // list[string] is a union member, so the unresolved value passes
    // through with its constraint intact: the consumer knows the field
    // resolves to a list[string] even though no value is known.
    let st =
        symtab!("WrappedAction.Args" => ExprValue::unresolved(ExprType::list(ExprType::STRING)));
    let sr = static_resolution_with_target("{{ WrappedAction.Args }}", &st, &args_target());
    assert!(sr.resolved_value.is_none());
    assert_eq!(sr.min_resolved_string_len, 0);
    assert_eq!(sr.resolved_type, ExprType::list(ExprType::STRING));
}

#[test]
fn args_union_target_unresolved_string_satisfies() {
    let st = symtab!("Param.Flag" => ExprValue::unresolved(ExprType::STRING));
    let sr = static_resolution_with_target("{{ Param.Flag }}", &st, &args_target());
    assert!(sr.resolved_value.is_none());
    assert_eq!(sr.resolved_type, ExprType::STRING);
}

#[test]
fn args_union_target_unresolved_list_of_int_converts_to_list_of_string() {
    // list[int] is not a member, but a list→list conversion rule applies
    // (element compatibility is deferred until the payload is known), and
    // there is no list→string rule — so the only destination is
    // list[string], and the type promises the consumer a list.
    let st = symtab!("Param.Frames" => ExprValue::unresolved(ExprType::list(ExprType::INT)));
    let sr = static_resolution_with_target("{{ Param.Frames }}", &st, &args_target());
    assert!(sr.resolved_value.is_none());
    assert_eq!(sr.resolved_type, ExprType::list(ExprType::STRING));
}

#[test]
fn args_union_target_unresolved_union_source_keeps_satisfying_members() {
    // Timeout-shaped symbol: unresolved(int | nulltype). Both members have
    // a path into the target (int converts to string, nulltype satisfies),
    // so the result is the union of the per-member outcomes.
    let st = symtab!(
        "WrappedAction.Timeout" => ExprValue::unresolved(
            ExprType::union(vec![ExprType::INT, ExprType::NULLTYPE])
        )
    );
    let sr = static_resolution_with_target("{{ WrappedAction.Timeout }}", &st, &args_target());
    assert!(sr.resolved_value.is_none());
    assert_eq!(sr.min_resolved_string_len, 0);
    // Pin the exact result so any change to union coerce_type is visible.
    assert_eq!(
        sr.resolved_type,
        ExprType::union(vec![ExprType::STRING, ExprType::NULLTYPE])
    );
}

#[test]
fn args_union_target_resolved_type_is_union_when_payload_decides() {
    // A source that can become a string or a list[string] but never null:
    // unresolved(string | list[int]) under nulltype | string | list[string].
    // string satisfies; list[int] converts to list[string]; nulltype is
    // unreachable. Without the payload no single member can be promised,
    // so resolved_type is the union of the possible outcomes — narrower
    // than the target, with null correctly excluded.
    let st = symtab!(
        "Param.Args" => ExprValue::unresolved(
            ExprType::union(vec![ExprType::STRING, ExprType::list(ExprType::INT)])
        )
    );
    let sr = static_resolution_with_target("{{ Param.Args }}", &st, &args_target());
    assert!(sr.resolved_value.is_none());
    assert_eq!(
        sr.resolved_type,
        ExprType::union(vec![ExprType::STRING, ExprType::list(ExprType::STRING)])
    );
}

#[test]
fn args_union_target_concrete_null_passes_through() {
    // null satisfies the nulltype member: typed passthrough keeps the
    // Null value, which interpolates as the empty string (0 chars).
    let sr = check_static_resolution_against_typed_resolution(
        "{{ null }}",
        &SymbolTable::new(),
        &args_target(),
    );
    assert!(matches!(sr.resolved_value, Some(ExprValue::Null)));
    assert_eq!(sr.min_resolved_string_len, 0);
    assert_eq!(sr.resolved_type, ExprType::NULLTYPE);
}

#[test]
fn args_union_target_concrete_list_passes_through() {
    let sr = check_static_resolution_against_typed_resolution(
        "{{ ['-v', '--frame', '7'] }}",
        &SymbolTable::new(),
        &args_target(),
    );
    assert!(sr.resolved_value.as_ref().is_some_and(ExprValue::is_list));
    assert_eq!(sr.resolved_type, ExprType::list(ExprType::STRING));
}

#[test]
fn args_union_target_concrete_int_converts_to_string() {
    // int is not a union member; the scalar rules land it on string.
    let sr = check_static_resolution_against_typed_resolution(
        "{{ 42 }}",
        &SymbolTable::new(),
        &args_target(),
    );
    assert!(matches!(sr.resolved_value, Some(ExprValue::String(ref s)) if s == "42"));
    assert_eq!(sr.min_resolved_string_len, 2);
    assert_eq!(sr.resolved_type, ExprType::STRING);
}

#[test]
fn args_union_target_ignored_in_multi_segment() {
    // "--flag {{ Args }}" concatenates: the union target is ignored
    // (mirroring resolution) and the type is string; the literal prefix
    // still bounds the length.
    let st =
        symtab!("WrappedAction.Args" => ExprValue::unresolved(ExprType::list(ExprType::STRING)));
    let sr = static_resolution_with_target("--flag {{ WrappedAction.Args }}", &st, &args_target());
    assert!(sr.resolved_value.is_none());
    assert_eq!(sr.min_resolved_string_len, 7); // "--flag "
    assert_eq!(sr.resolved_type, ExprType::STRING);
}

#[test]
fn static_resolution_target_ignored_for_multi_segment() {
    // Mirrors resolution: a multi-segment format string concatenates to a
    // string and never applies the target, so validation must not either.
    let sr = check_static_resolution_against_typed_resolution(
        "x{{ 1.0 }}",
        &SymbolTable::new(),
        &ExprType::INT,
    );
    assert_eq!(sr.min_resolved_string_len, 4); // "x1.0"
    assert!(matches!(sr.resolved_value, Some(ExprValue::String(ref s)) if s == "x1.0"));
}

#[test]
fn static_resolution_target_ignored_for_single_literal_segment() {
    // A pure literal has no expression segment, so the typed passthrough
    // rule does not apply and the target is ignored, as in resolution.
    let sr = check_static_resolution_against_typed_resolution(
        "hello",
        &SymbolTable::new(),
        &ExprType::INT,
    );
    assert_eq!(sr.min_resolved_string_len, 5);
    assert!(matches!(sr.resolved_value, Some(ExprValue::String(ref s)) if s == "hello"));
}

#[test]
fn static_resolution_unresolved_symbol_coerces_at_type_level() {
    // An unresolved FLOAT under an INT target stays unresolved (the
    // coercion is checked at the type level), contributes 0 to the bound,
    // and blocks the static value — same as without a target.
    let lib = FunctionLibrary::for_profile(&ExprProfile::current().with_host_context(
        HostContext::with_rules(Vec::<openjd_expr::PathMappingRule>::new()),
    ));
    let st = symtab!("Param.X" => ExprValue::unresolved(ExprType::FLOAT));
    let sr = FormatString::new("{{ Param.X }}")
        .unwrap()
        .validate_expressions(&st, &lib, Some(&ExprType::INT))
        .unwrap();
    assert_eq!(sr.min_resolved_string_len, 0);
    assert!(sr.resolved_value.is_none());
}

#[test]
fn static_resolution_uncoercible_value_fails_validation_like_resolution() {
    // A value that cannot coerce to the target fails resolution, so it
    // must fail validation with the same diagnostic.
    let lib = FunctionLibrary::for_profile(&ExprProfile::current().with_host_context(
        HostContext::with_rules(Vec::<openjd_expr::PathMappingRule>::new()),
    ));
    let fs = FormatString::new("{{ 'abc' }}").unwrap();
    let st = SymbolTable::new();
    let target = ExprType::INT;
    let resolve_err = fs
        .resolve_with(
            &st,
            &FormatStringOptions::default()
                .with_library(&*lib)
                .with_target_type(&target),
        )
        .unwrap_err();
    let validate_err = fs
        .validate_expressions(&st, &lib, Some(&target))
        .unwrap_err();
    assert_eq!(
        validate_err.message,
        resolve_err.to_string(),
        "validation must report the same failure resolution does",
    );
}

// === Resolved-value cap (MAX_STATIC_RESOLVED_VALUE_LEN) ===
//
// The per-segment evaluator memory limit does not compose across segments:
// many small expressions can each evaluate within their own limit yet
// concatenate to a huge string. validate_expressions caps the concatenation
// it materializes; min_resolved_string_len keeps counting past the cap and
// is enough to reject the string.

#[test]
fn resolved_value_capped_for_oversized_concatenation() {
    // Two segments of 6,000,000 chars each: 12,000,000 bytes total, over
    // the 10 MiB cap. Everything is concrete, but the concatenation is not
    // materialized; the bound still counts the full length exactly.
    let sr = static_resolution(
        "{{ 'A' * 6000000 }}{{ 'A' * 6000000 }}",
        &SymbolTable::new(),
    );
    assert_eq!(sr.min_resolved_string_len, 12_000_000);
    assert!(
        sr.resolved_value.is_none(),
        "concatenation over MAX_STATIC_RESOLVED_VALUE_LEN must not be materialized",
    );
    assert_eq!(sr.resolved_type, ExprType::STRING);
}

#[test]
fn resolved_value_at_cap_boundary_is_materialized() {
    // Exactly MAX_STATIC_RESOLVED_VALUE_LEN bytes (1 literal + cap-1 from the
    // expression) is allowed; the cap only excludes strictly larger values.
    let n = openjd_expr::format_string::MAX_STATIC_RESOLVED_VALUE_LEN - 1;
    let sr = static_resolution(&format!("x{{{{ 'A' * {n} }}}}"), &SymbolTable::new());
    assert_eq!(
        sr.min_resolved_string_len,
        openjd_expr::format_string::MAX_STATIC_RESOLVED_VALUE_LEN
    );
    let val = sr.resolved_value.expect("at the cap is still materialized");
    assert!(matches!(val, ExprValue::String(ref s)
        if s.len() == openjd_expr::format_string::MAX_STATIC_RESOLVED_VALUE_LEN));
}

#[test]
fn resolved_value_one_past_cap_is_dropped() {
    let n = openjd_expr::format_string::MAX_STATIC_RESOLVED_VALUE_LEN;
    let sr = static_resolution(&format!("x{{{{ 'A' * {n} }}}}"), &SymbolTable::new());
    assert_eq!(sr.min_resolved_string_len, n + 1);
    assert!(sr.resolved_value.is_none());
}

#[test]
fn resolved_value_cap_does_not_apply_to_typed_passthrough() {
    // A single-expression format string performs no concatenation: the
    // typed value passes through regardless of size (it is bounded by the
    // evaluator's own memory limit).
    let n = openjd_expr::format_string::MAX_STATIC_RESOLVED_VALUE_LEN + 1;
    let sr = static_resolution(&format!("{{{{ 'A' * {n} }}}}"), &SymbolTable::new());
    assert_eq!(sr.min_resolved_string_len, n);
    assert!(matches!(sr.resolved_value, Some(ExprValue::String(ref s)) if s.len() == n));
}

#[test]
fn capped_concatenation_still_counts_later_segments() {
    // Segments after the cap still contribute to the bound: the count is
    // complete even though accumulation stopped.
    let sr = static_resolution(
        "{{ 'A' * 6000000 }}{{ 'A' * 6000000 }}tail{{ 'B' * 5 }}",
        &SymbolTable::new(),
    );
    assert_eq!(sr.min_resolved_string_len, 12_000_009);
    assert!(sr.resolved_value.is_none());
}

// === Null handling ===

#[test]
fn null_renders_as_empty_in_string_context() {
    let st = symtab!("Param.X" => ExprValue::Null);
    assert_eq!(resolve_str("a{{Param.X}}b", &st), "ab");
}

// === Whitespace in expressions ===

#[test]
fn whitespace_around_expression_is_trimmed() {
    let st = symtab!("Param.X" => 5);
    assert_eq!(resolve_str("{{  Param.X  }}", &st), "5");
}

// === Adjacent interpolations ===

#[test]
fn adjacent_interpolations() {
    let st = symtab!("Param.A" => "hello", "Param.B" => "world");
    assert_eq!(resolve_str("{{Param.A}}{{Param.B}}", &st), "helloworld");
}

// === Deeply nested dotted names ===

#[test]
fn deeply_nested_dotted_name() {
    let st = symtab!("Task.Param.Render.Frame" => 100);
    assert_eq!(resolve_str("f{{Task.Param.Render.Frame}}", &st), "f100");
}

// ══════════════════════════════════════════════════════════════
// Equality + hashing (by raw source text)
// ══════════════════════════════════════════════════════════════

fn hash_of<T: std::hash::Hash>(v: &T) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

#[test]
fn format_string_hash_consistent_with_eq() {
    let a = FormatString::new("prefix {{Param.Frame}} suffix").unwrap();
    let b = FormatString::new("prefix {{Param.Frame}} suffix").unwrap();
    let c = FormatString::new("prefix {{Param.Other}} suffix").unwrap();
    assert_eq!(a, b);
    assert_eq!(hash_of(&a), hash_of(&b));
    assert_ne!(a, c);
}

#[test]
// The interior mutability clippy flags is in the ruff AST's atomic node
// indices; FormatString's Hash/Eq are over the raw source only, which is
// immutable, so set keys are stable.
#[allow(clippy::mutable_key_type)]
fn format_string_usable_in_hash_set() {
    let mut set = std::collections::HashSet::new();
    set.insert(FormatString::new("{{Param.A}}").unwrap());
    set.insert(FormatString::new("{{Param.A}}").unwrap());
    set.insert(FormatString::new("{{Param.B}}").unwrap());
    assert_eq!(set.len(), 2);
    assert!(set.contains(&FormatString::new("{{Param.A}}").unwrap()));
}
