// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test_rfc_examples.py

use openjd_expr::{ExprValue, ParsedExpression, PathFormat, SymbolTable};

fn eval(expr: &str) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap()
}
fn eval_with(expr: &str, st: &SymbolTable) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(st))
        .unwrap()
}

// Basic RFC examples
#[test]
fn rfc_arithmetic() {
    assert_eq!(eval("1 + 2 * 3").to_display_string(), "7");
}
#[test]
fn rfc_string_concat() {
    assert_eq!(
        eval("'hello' + ' ' + 'world'").to_display_string(),
        "hello world"
    );
}
#[test]
fn rfc_conditional() {
    assert_eq!(eval("'yes' if True else 'no'").to_display_string(), "yes");
}
#[test]
fn rfc_list_comp() {
    assert_eq!(
        eval("[x * 2 for x in [1, 2, 3]]").to_display_string(),
        "[2, 4, 6]"
    );
}

#[test]
fn rfc_symbol_table() {
    let mut st = SymbolTable::new();
    st.set("Param.Frame", ExprValue::Int(42)).unwrap();
    assert_eq!(eval_with("Param.Frame", &st).to_display_string(), "42");
}

#[test]
fn rfc_string_formatting() {
    let mut st = SymbolTable::new();
    st.set("Param.Frame", ExprValue::Int(42)).unwrap();
    assert_eq!(
        eval_with("zfill(Param.Frame, 4)", &st).to_display_string(),
        "0042"
    );
}

// === Additional RFC examples ===
#[test]
fn rfc_string_manipulation() {
    assert_eq!(
        eval("'hello world'.upper()").to_display_string(),
        "HELLO WORLD"
    );
}
#[test]
fn rfc_path_with_suffix() {
    let mut st = SymbolTable::new();
    st.set(
        "P",
        ExprValue::new_path("/renders/scene.exr", PathFormat::Posix),
    )
    .unwrap();
    assert_eq!(
        eval_with_path_format("P.with_suffix('.png')", &st, PathFormat::Posix).to_display_string(),
        "/renders/scene.png"
    );
}
#[test]
fn rfc_repr_sh_string() {
    assert_eq!(
        eval("repr_sh('hello world')").to_display_string(),
        "'hello world'"
    );
}
#[test]
fn rfc_repr_sh_list() {
    assert_eq!(
        eval("repr_sh(['echo', 'hello world'])").to_display_string(),
        "echo 'hello world'"
    );
}
#[test]
fn rfc_gpu_flag_true() {
    let mut st = SymbolTable::new();
    st.set("UseGPU", ExprValue::Bool(true)).unwrap();
    let r = ParsedExpression::new("'--gpu' if UseGPU else ''")
        .and_then(|p| p.evaluate(&st))
        .unwrap();
    assert_eq!(r.to_display_string(), "--gpu");
}
#[test]
fn rfc_gpu_flag_false() {
    let mut st = SymbolTable::new();
    st.set("UseGPU", ExprValue::Bool(false)).unwrap();
    let r = ParsedExpression::new("'--gpu' if UseGPU else ''")
        .and_then(|p| p.evaluate(&st))
        .unwrap();
    assert_eq!(r.to_display_string(), "");
}
#[test]
fn rfc_quality_list() {
    let mut st = SymbolTable::new();
    st.set("Quality", ExprValue::String("high".into())).unwrap();
    let r = ParsedExpression::new("'--quality ' + Quality")
        .and_then(|p| p.evaluate(&st))
        .unwrap();
    assert_eq!(r.to_display_string(), "--quality high");
}

// Helper for evaluating with a specific path format
fn eval_with_path_format(expr: &str, st: &SymbolTable, fmt: PathFormat) -> ExprValue {
    let parsed = ParsedExpression::new(expr).unwrap();
    let symtabs = [st];
    parsed.with_path_format(fmt).evaluate(&symtabs).unwrap()
}

// === Tests ported from Python test_rfc_examples.py ===

// --- RFC 0005: Arithmetic on frame ranges ---
#[test]
fn rfc_frame_range_arithmetic() {
    let mut st = SymbolTable::new();
    st.set("Param.FrameStart", ExprValue::Int(1)).unwrap();
    st.set("Param.FrameEnd", ExprValue::Int(100)).unwrap();
    st.set("Param.FramesPerTask", ExprValue::Int(10)).unwrap();
    st.set("Task.Param.Frame", ExprValue::Int(21)).unwrap();
    let r = eval_with(
        "min(Task.Param.Frame + Param.FramesPerTask, Param.FrameEnd) - 1",
        &st,
    );
    assert_eq!(r, ExprValue::Int(30));
}

// --- RFC 0005: Conditional expressions ---
#[test]
fn rfc_conditional_draft() {
    let mut st = SymbolTable::new();
    st.set("Param.Quality", ExprValue::String("draft".into()))
        .unwrap();
    let r = eval_with("16 if Param.Quality == 'final' else 4", &st);
    assert_eq!(r, ExprValue::Int(4));
}

#[test]
fn rfc_conditional_final() {
    let mut st = SymbolTable::new();
    st.set("Param.Quality", ExprValue::String("final".into()))
        .unwrap();
    let r = eval_with("16 if Param.Quality == 'final' else 4", &st);
    assert_eq!(r, ExprValue::Int(16));
}

// --- RFC 0005: List flattening / null dropping ---
#[test]
fn rfc_verbose_true() {
    let mut st = SymbolTable::new();
    st.set("Param.Verbose", ExprValue::Bool(true)).unwrap();
    let r = eval_with("'--verbose' if Param.Verbose else null", &st);
    assert_eq!(r, ExprValue::String("--verbose".into()));
}

#[test]
fn rfc_verbose_false() {
    let mut st = SymbolTable::new();
    st.set("Param.Verbose", ExprValue::Bool(false)).unwrap();
    let r = eval_with("'--verbose' if Param.Verbose else null", &st);
    assert_eq!(r, ExprValue::Null);
}

#[test]
fn rfc_quality_list_with_value() {
    // Python test uses internal Evaluator API with target_type for auto-coercion.
    // At the expression level, use str() to explicitly convert int to string in list context.
    let mut st = SymbolTable::new();
    st.set("Param.Quality", ExprValue::Int(5)).unwrap();
    let r = eval_with(
        "['--quality', string(Param.Quality)] if Param.Quality > 0 else null",
        &st,
    );
    assert_eq!(r.to_display_string(), "[\"--quality\", \"5\"]");
}

// --- RFC 0006: String manipulation with path ---
#[test]
fn rfc_string_manipulation_path() {
    let mut st = SymbolTable::new();
    st.set(
        "Param.InputFile",
        ExprValue::new_path("/renders/scene_v2.exr", PathFormat::Posix),
    )
    .unwrap();
    let r = eval_with_path_format(
        "Param.InputFile.stem.upper() + '_final' + Param.InputFile.suffix",
        &st,
        PathFormat::Posix,
    );
    assert_eq!(r.to_display_string(), "SCENE_V2_final.exr");
}

// --- RFC 0006: Path operations with division ---
#[test]
fn rfc_path_division_with_suffix() {
    let mut st = SymbolTable::new();
    st.set(
        "Param.InputFile",
        ExprValue::new_path("/renders/scene.exr", PathFormat::Posix),
    )
    .unwrap();
    st.set(
        "Param.OutputDir",
        ExprValue::new_path("/output", PathFormat::Posix),
    )
    .unwrap();
    let r = eval_with_path_format(
        "(Param.OutputDir / Param.InputFile.name).with_suffix('.png')",
        &st,
        PathFormat::Posix,
    );
    assert_eq!(r.to_display_string(), "/output/scene.png");
}

// --- RFC 0006: Shell quoting ---
#[test]
fn rfc_repr_sh_string_with_quotes() {
    let mut st = SymbolTable::new();
    st.set(
        "Task.Command",
        ExprValue::String("echo 'hello world'".into()),
    )
    .unwrap();
    let r = eval_with("repr_sh(Task.Command)", &st);
    let s = r.to_display_string();
    assert_eq!(s, "\"echo 'hello world'\"");
}

#[test]
fn rfc_repr_sh_list_strings() {
    let r = eval("repr_sh(['file with spaces.txt', '--flag', 'value'])");
    assert_eq!(r.to_display_string(), "'file with spaces.txt' --flag value");
}

#[test]
fn rfc_repr_sh_list_path() {
    let st = SymbolTable::new();
    let r = eval_with_path_format(
        "repr_sh([path('/tmp/a b.txt'), path('/tmp/c.txt')])",
        &st,
        PathFormat::Posix,
    );
    assert_eq!(r.to_display_string(), "'/tmp/a b.txt' /tmp/c.txt");
}

// --- RFC 0007: Boolean parameters with null ---
#[test]
fn rfc_gpu_flag_true_null() {
    let mut st = SymbolTable::new();
    st.set("Param.UseGpu", ExprValue::Bool(true)).unwrap();
    let r = eval_with("'--gpu' if Param.UseGpu else null", &st);
    assert_eq!(r, ExprValue::String("--gpu".into()));
}

#[test]
fn rfc_gpu_flag_false_null() {
    let mut st = SymbolTable::new();
    st.set("Param.UseGpu", ExprValue::Bool(false)).unwrap();
    let r = eval_with("'--gpu' if Param.UseGpu else null", &st);
    assert_eq!(r, ExprValue::Null);
}

// ══════════════════════════════════════════════════════════════
// RFC 0005 § "Implicit Type Coercion" — the examples stated in the
// specification's satisfaction/conversion rules, pinned verbatim.
// ══════════════════════════════════════════════════════════════

use openjd_expr::{ExprType, RangeExpr};

fn ty(s: &str) -> ExprType {
    ExprType::parse(s).unwrap()
}
fn coerce(v: ExprValue, target: &str) -> Result<ExprValue, String> {
    v.coerce(&ty(target), PathFormat::Posix)
}

/// "Because satisfaction is checked first, a result whose type the target
/// already admits is never converted."
#[test]
fn rfc_coercion_satisfaction_is_checked_first() {
    assert_eq!(
        coerce(ExprValue::Int(7), "int | string").unwrap(),
        ExprValue::Int(7)
    );
    assert_eq!(coerce(ExprValue::Null, "string?").unwrap(), ExprValue::Null);
    for target in ["list[any]", "list[int | string]"] {
        assert_eq!(
            coerce(ExprValue::ListInt(vec![1, 2]), target).unwrap(),
            ExprValue::ListInt(vec![1, 2]),
            "target={target}"
        );
    }
}

/// "in a format string context where the target type is `string?`, an `int`
/// result is coerced to `string`" — the `nulltype` member is set aside, so
/// `string` is the only destination.
#[test]
fn rfc_coercion_optional_string_target_converts_int() {
    assert_eq!(
        coerce(ExprValue::Int(7), "string?").unwrap(),
        ExprValue::String("7".into())
    );
}

/// "Destinations are ordered non-list before list … a `range_expr` against a
/// `list[int] | string` target becomes the string `"1-5"` … rather than the
/// expanded list."
#[test]
fn rfc_coercion_orders_non_list_destinations_first() {
    let range = ExprValue::RangeExpr(RangeExpr::from_values(vec![1, 2, 3, 4, 5]).unwrap());
    assert_eq!(
        coerce(range.clone(), "list[int] | string").unwrap(),
        ExprValue::String("1-5".into())
    );
    // With no non-list destination, the list conversion still runs.
    assert_eq!(
        coerce(range, "list[int] | bool").unwrap(),
        ExprValue::ListInt(vec![1, 2, 3, 4, 5])
    );
}

/// "`range_expr` → `list[int]` … the destination is accepted exactly when a
/// `list[int]` value would satisfy it … and rejected for any other element
/// type".
#[test]
fn rfc_coercion_range_expr_list_destinations() {
    let range = || ExprValue::RangeExpr(RangeExpr::from_values(vec![1, 2, 3, 4, 5]).unwrap());
    for target in ["list[int]", "list[any]", "list[int | string]"] {
        assert!(coerce(range(), target).is_ok(), "target={target}");
    }
    for target in ["list[float]", "list[string]", "list[bool]"] {
        assert!(coerce(range(), target).is_err(), "target={target}");
    }
}

/// "`nulltype` is never a destination … in particular, a `string` whose text
/// happens to be `"null"` does not become `null`."
#[test]
fn rfc_coercion_never_converts_into_null() {
    assert_eq!(
        coerce(ExprValue::String("null".into()), "string?").unwrap(),
        ExprValue::String("null".into())
    );
    assert!(coerce(ExprValue::String("null".into()), "int?").is_err());
    assert!(coerce(ExprValue::String("null".into()), "nulltype").is_err());
}

/// "a type variable, `noreturn`, `unresolved[T]`, or a `list` parameterized by
/// any of those contributes no destination, so a target composed only of such
/// types cannot be coerced to at all." Holds "at any nesting depth".
#[test]
fn rfc_coercion_targets_without_destinations() {
    for target in ["T", "T1", "list[T1]", "list[list[T1]]", "noreturn"] {
        assert!(
            coerce(ExprValue::Int(1), target).is_err(),
            "target={target}"
        );
        assert!(
            coerce(ExprValue::unresolved(ExprType::INT), target).is_err(),
            "target={target}"
        );
    }
}

/// "`list[nulltype]` → `list[T]` for any `T`" — the empty list literal.
#[test]
fn rfc_coercion_empty_list_to_any_list_type() {
    let empty = || ExprValue::make_list(vec![], ExprType::NULLTYPE).unwrap();
    assert_eq!(empty().expr_type().to_string(), "list[nulltype]");
    for target in [
        "list[int]",
        "list[string]",
        "list[float]",
        "list[list[int]]",
    ] {
        assert!(coerce(empty(), target).is_ok(), "target={target}");
    }
}

/// RFC 0005 § "Coercion of Unresolved Values": the narrowed constraint is the
/// type the applicable step produces, and is not a promise about which
/// destination the payload will reach.
#[test]
fn rfc_coercion_unresolved_narrowing() {
    // Satisfaction keeps the source type rather than widening to the target.
    assert_eq!(
        coerce(ExprValue::unresolved(ty("list[int]")), "list[any]").unwrap(),
        ExprValue::unresolved(ty("list[int]"))
    );
    // Conversion yields the type its own rule produces.
    assert_eq!(
        coerce(ExprValue::unresolved(ty("range_expr")), "list[any]").unwrap(),
        ExprValue::unresolved(ty("list[int]"))
    );
    // Conversion against a union cannot predict the destination — a 3.5
    // payload fails the float -> int rule and falls through to string — so
    // the constraint is the union of every destination with a rule.
    assert_eq!(
        coerce(ExprValue::unresolved(ExprType::FLOAT), "int | string").unwrap(),
        ExprValue::unresolved(ty("int | string"))
    );
    let f35 = ExprValue::Float(openjd_expr::value::Float64::new(3.5).unwrap());
    assert_eq!(
        coerce(f35, "int | string").unwrap(),
        ExprValue::String("3.5".into())
    );
}

/// RFC 0005 § "Implicit Type Coercion": within each shape group, destinations
/// are tried in the source type's preference order. A value prefers to stay
/// within its own kind, and a conversion that can fail is tried before one
/// that always succeeds. Each row of the specification's table, pinned.
#[test]
fn rfc_coercion_destination_preference_per_source_type() {
    let float = |v: f64| ExprValue::Float(openjd_expr::value::Float64::new(v).unwrap());
    // int: float before string — stays a number.
    assert_eq!(
        coerce(ExprValue::Int(5), "float | string").unwrap(),
        float(5.0)
    );
    // float: int (exact wholes only) before string.
    assert_eq!(
        coerce(float(3.0), "int | string").unwrap(),
        ExprValue::Int(3)
    );
    assert_eq!(
        coerce(float(3.5), "int | string").unwrap(),
        ExprValue::String("3.5".into())
    );
    // string: int before float — every int-string also parses as float, so
    // the stricter parse must come first. The string's own lexical form then
    // routes it.
    assert_eq!(
        coerce(ExprValue::String("5".into()), "int | float").unwrap(),
        ExprValue::Int(5)
    );
    assert_eq!(
        coerce(ExprValue::String("5.5".into()), "int | float")
            .unwrap()
            .expr_type(),
        ExprType::FLOAT
    );
    // string: every string is a valid path, so path is the last resort.
    assert_eq!(
        coerce(ExprValue::String("5".into()), "int | path").unwrap(),
        ExprValue::Int(5)
    );
    assert_eq!(
        coerce(ExprValue::String("abc".into()), "int | path").unwrap(),
        ExprValue::new_path("abc", PathFormat::Posix)
    );
    assert_eq!(
        coerce(ExprValue::String("yes".into()), "bool | path").unwrap(),
        ExprValue::Bool(true)
    );
    // list[S]: list destinations follow S's preference, recursively, and a
    // payload that fails the preferred rule falls through to the next.
    let lf = |v: Vec<f64>| {
        ExprValue::ListFloat(
            v.into_iter()
                .map(|x| openjd_expr::value::Float64::new(x).unwrap())
                .collect(),
        )
    };
    assert_eq!(
        coerce(lf(vec![1.0]), "list[int] | list[string]").unwrap(),
        ExprValue::ListInt(vec![1])
    );
    assert_eq!(
        coerce(lf(vec![1.5]), "list[int] | list[string]")
            .unwrap()
            .expr_type(),
        ExprType::list(ExprType::STRING)
    );
    assert_eq!(
        coerce(
            ExprValue::ListString(vec!["5".into()], 1),
            "list[int] | list[float]"
        )
        .unwrap(),
        ExprValue::ListInt(vec![5])
    );
    // The type level cannot see which destination the payload will reach
    // ("5" becomes an int, "5.5" a float), so it narrows to the union of
    // every destination with a rule rather than betting on the first.
    assert_eq!(
        coerce(ExprValue::unresolved(ExprType::STRING), "int | float").unwrap(),
        ExprValue::unresolved(ty("int | float"))
    );
    assert_eq!(
        coerce(
            ExprValue::unresolved(ty("list[string]")),
            "list[int] | list[float]"
        )
        .unwrap(),
        ExprValue::unresolved(ty("list[int] | list[float]"))
    );
}

/// RFC 0005 § "Implicit Type Coercion": an empty list converts to every
/// list type, and the nominal element type it carries follows the first
/// list destination in the union's **normalized** member order — the same
/// either way the union is written.
#[test]
fn rfc_empty_list_nominal_element_type_follows_normalized_order() {
    let empty = || ExprValue::make_list(vec![], ExprType::NULLTYPE).unwrap();
    for target in ["list[string] | list[int]", "list[int] | list[string]"] {
        let coerced = empty().coerce(&ty(target), PathFormat::Posix).unwrap();
        assert_eq!(coerced, ExprValue::ListInt(vec![]), "target={target}");
    }
}
