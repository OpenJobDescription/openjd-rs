// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_rfc_examples.py

use openjd_expr::{evaluate_expression, ExprValue, PathFormat, SymbolTable};

fn eval(expr: &str) -> ExprValue {
    evaluate_expression(expr, &SymbolTable::new()).unwrap()
}
fn eval_with(expr: &str, st: &SymbolTable) -> ExprValue {
    evaluate_expression(expr, st).unwrap()
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
        ExprValue::Path {
            value: "/renders/scene.exr".into(),
            format: PathFormat::Posix,
        },
    )
    .unwrap();
    assert_eq!(
        evaluate_expression("P.with_suffix('.png')", &st)
            .unwrap()
            .to_display_string(),
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
    assert_eq!(eval("repr_sh([1, 2, 3])").to_display_string(), "1 2 3");
}
#[test]
fn rfc_gpu_flag_true() {
    let mut st = SymbolTable::new();
    st.set("UseGPU", ExprValue::Bool(true)).unwrap();
    let r = evaluate_expression("'--gpu' if UseGPU else ''", &st).unwrap();
    assert_eq!(r.to_display_string(), "--gpu");
}
#[test]
fn rfc_gpu_flag_false() {
    let mut st = SymbolTable::new();
    st.set("UseGPU", ExprValue::Bool(false)).unwrap();
    let r = evaluate_expression("'--gpu' if UseGPU else ''", &st).unwrap();
    assert_eq!(r.to_display_string(), "");
}
#[test]
fn rfc_quality_list() {
    let mut st = SymbolTable::new();
    st.set("Quality", ExprValue::String("high".into())).unwrap();
    let r = evaluate_expression("'--quality ' + Quality", &st).unwrap();
    assert_eq!(r.to_display_string(), "--quality high");
}

// Helper for evaluating with a specific path format
fn eval_with_path_format(expr: &str, st: &SymbolTable, fmt: PathFormat) -> ExprValue {
    use openjd_expr::ParsedExpression;
    let parsed = ParsedExpression::new(expr).unwrap();
    let symtabs = [st];
    let mut ev = parsed.evaluator(&symtabs).with_path_format(fmt);
    ev.evaluate(&parsed.ast).unwrap()
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
        ExprValue::Path {
            value: "/renders/scene_v2.exr".into(),
            format: PathFormat::Posix,
        },
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
        ExprValue::Path {
            value: "/renders/scene.exr".into(),
            format: PathFormat::Posix,
        },
    )
    .unwrap();
    st.set(
        "Param.OutputDir",
        ExprValue::Path {
            value: "/output".into(),
            format: PathFormat::Posix,
        },
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
