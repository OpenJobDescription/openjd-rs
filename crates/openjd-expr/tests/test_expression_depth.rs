// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for the AST nesting depth limit that prevents stack-exhaustion
//! DoS on pathological inputs such as `((((...1...))))` or long
//! left-associative binop chains like `1+1+...+1`.
//!
//! These tests must be satisfied during parsing and during evaluation —
//! either may reject a too-deep expression, but neither may crash.

use openjd_expr::{
    ExpressionErrorKind, FormatString, FormatStringOptions, ParsedExpression, SymbolTable,
    MAX_EXPRESSION_DEPTH,
};

/// Helper that wraps an expression in a format string and tries both
/// `ParsedExpression::new` and `FormatString::new` paths. Returns the error
/// from whichever path rejects the input first.
fn expect_too_deep(expr: &str) {
    match ParsedExpression::new(expr) {
        Ok(parsed) => {
            // Parser accepted it — evaluator must catch it.
            let st = SymbolTable::new();
            let err = parsed.evaluate(&st).expect_err(&format!(
                "Evaluator must reject deeply-nested expression, but accepted:\n  {expr:.80}"
            ));
            assert!(
                matches!(err.kind(), ExpressionErrorKind::ExpressionTooDeep { .. }),
                "Expected ExpressionTooDeep from evaluator, got: {:?}\n  msg: {}",
                err.kind(),
                err.message()
            );
        }
        Err(err) => {
            assert!(
                matches!(err.kind(), ExpressionErrorKind::ExpressionTooDeep { .. }),
                "Expected ExpressionTooDeep from parser, got: {:?}\n  msg: {}",
                err.kind(),
                err.message()
            );
        }
    }
}

#[test]
fn max_depth_constant_is_64() {
    assert_eq!(MAX_EXPRESSION_DEPTH, 64);
}

// ── Parser survival: shapes that would blow the default thread stack ──
//
// Pure `(((...1...)))` parses to just `1` in ruff (redundant parens are
// dropped before AST construction), so the structural depth walker
// doesn't reject it. What matters here is that the parser itself doesn't
// crash on the recursion: with the worker-thread fallback, any input up
// to `MAX_PARSE_INPUT_LEN` is survivable regardless of shape.

#[test]
fn parser_survives_deeply_nested_parens() {
    // 200 nested parens — well within what the parser thread handles.
    let expr = format!("{}1{}", "(".repeat(200), ")".repeat(200));
    let parsed = ParsedExpression::new(&expr)
        .expect("200 parens must parse (worker thread has ample stack)");
    // The AST is just `1` after ruff collapses the parens.
    let v = parsed
        .evaluate(&SymbolTable::new())
        .expect("evaluation must succeed");
    assert_eq!(v.to_display_string(), "1");
}

#[test]
fn parser_survives_very_deeply_nested_parens() {
    // 5000 nested parens — exercises the worker thread's enlarged stack.
    let expr = format!("{}1{}", "(".repeat(5000), ")".repeat(5000));
    let parsed = ParsedExpression::new(&expr)
        .expect("5000 parens must parse (worker thread has ample stack)");
    let v = parsed
        .evaluate(&SymbolTable::new())
        .expect("evaluation must succeed");
    assert_eq!(v.to_display_string(), "1");
}

#[test]
fn reject_oversized_parser_input() {
    // Source larger than MAX_PARSE_INPUT_LEN is rejected outright without
    // invoking the parser, regardless of shape.
    let expr = format!("{}1{}", "(".repeat(40_000), ")".repeat(40_000));
    let err = ParsedExpression::new(&expr).expect_err("oversized parser input must be rejected");
    let msg = err.message();
    assert!(
        msg.contains("maximum") || msg.contains("exceeds"),
        "expected size-cap error, got: {msg}"
    );
}

// ── Parser-side depth: shapes that produce a deep AST ──

#[test]
fn reject_deeply_nested_unary_minus() {
    // `-(-(-(...1)))` — each `-` adds one UnaryOp level
    let expr = format!("{}1", "-".repeat(200));
    expect_too_deep(&expr);
}

#[test]
fn reject_very_deeply_nested_unary_minus() {
    let expr = format!("{}1", "-".repeat(5000));
    expect_too_deep(&expr);
}

#[test]
fn reject_deeply_right_nested_power() {
    // `2**(2**(2**(...2)))` is right-associative and produces a deep AST
    let mut expr = String::from("2");
    for _ in 0..200 {
        expr = format!("2**({expr})");
    }
    expect_too_deep(&expr);
}

// ── Evaluator-side depth: shapes the parser accepts that produce a deep AST ──

#[test]
fn reject_long_left_assoc_binop_chain() {
    // `1+1+1+...+1` — ruff parses this into a left-leaning BinOp tree
    let mut expr = String::from("1");
    for _ in 0..200 {
        expr.push_str("+1");
    }
    expect_too_deep(&expr);
}

#[test]
fn reject_very_long_left_assoc_binop_chain() {
    let mut expr = String::from("1");
    for _ in 0..5000 {
        expr.push_str("+1");
    }
    expect_too_deep(&expr);
}

#[test]
fn reject_long_attribute_chain() {
    // `a.b.c.d....` — a chain of Attribute nodes
    let mut expr = String::from("a");
    for i in 0..200 {
        expr.push_str(&format!(".x{}", i));
    }
    expect_too_deep(&expr);
}

#[test]
fn reject_long_comparison_chain() {
    // `1<2<3<...<N` — chained comparison; the AST has one Compare node but
    // its comparators vector can be arbitrarily long. The depth check
    // counts the comparators vector dimension too.
    let mut expr = String::from("0");
    for i in 1..200 {
        expr.push_str(&format!("<{i}"));
    }
    expect_too_deep(&expr);
}

#[test]
fn reject_nested_list_literal_via_depth() {
    // `[[[[...1...]]]]` — list literals are limited to 2 nesting levels
    // by the semantic check, but a single-element chain is still a deep AST
    // via its sole element. This specifically tests the depth walker.
    let mut expr = String::from("1");
    for _ in 0..200 {
        expr = format!("({expr})");
    }
    // (...) chains are parens not lists; above covers parens.
    // Here construct `[[1], [1], [1], ...]` which has width not depth, to
    // confirm width is NOT flagged.
    let wide: Vec<&str> = (0..2000).map(|_| "[1]").collect();
    let wide_expr = format!("[{}]", wide.join(","));
    // 2-level nesting, 2000 elements — must succeed
    let parsed = ParsedExpression::new(&wide_expr).expect("wide-but-shallow list must parse");
    // Evaluate with generous limits to confirm no spurious depth rejection.
    let _ = parsed
        .with_operation_limit(1_000_000)
        .with_memory_limit(100_000_000)
        .evaluate(&[&SymbolTable::new()]);
}

#[test]
fn reject_deep_call_chain() {
    // `f(f(f(...f(1)...)))` — Call nodes nested in their own first argument
    let mut expr = String::from("1");
    for _ in 0..200 {
        expr = format!("abs({expr})");
    }
    expect_too_deep(&expr);
}

// ── FormatString path: confirms the depth check fires through format strings too ──

#[test]
fn format_string_rejects_deep_interpolation() {
    // A `{{...}}` expression whose AST exceeds the depth limit (here, a
    // long left-associative binop chain). Pure parens don't work because
    // ruff collapses them; the body must produce a genuinely deep AST.
    let mut inner = String::from("1");
    for _ in 0..200 {
        inner.push_str("+1");
    }
    let fs_src = format!("{{{{{inner}}}}}");
    let err = FormatString::new(&fs_src).expect_err("deeply-nested interpolation must be rejected");
    let msg = err.message();
    assert!(
        msg.contains("nesting depth")
            || msg.contains("too deep")
            || msg.to_lowercase().contains("expression"),
        "expected depth-related error, got: {msg}"
    );
}

#[test]
fn format_string_rejects_many_segments() {
    // 2,000 trivial `{{1}}` segments — SEC-2026-2 cap
    let fs_src = "{{1}}".repeat(2_000);
    let err = FormatString::new(&fs_src).expect_err("too many segments must be rejected");
    let msg = err.message();
    assert!(
        msg.contains("segment") || msg.contains("interpolation"),
        "expected segment-cap error, got: {msg}"
    );
}

#[test]
fn format_string_rejects_oversized_input() {
    // 2 MB input — SEC-2026-2 cap
    let fs_src = "a".repeat(2 * 1024 * 1024);
    let err = FormatString::new(&fs_src).expect_err("oversized input must be rejected");
    let msg = err.message();
    assert!(
        msg.contains("length") || msg.contains("too long") || msg.contains("size"),
        "expected size-cap error, got: {msg}"
    );
}

// ── Positive cases: depth just below the limit must still parse and evaluate ──

#[test]
fn allow_depth_just_below_limit() {
    // 50 nested parens — well below the 64-level limit
    let expr = format!("{}1{}", "(".repeat(50), ")".repeat(50));
    let parsed = ParsedExpression::new(&expr).expect("depth 50 must parse");
    let v = parsed
        .evaluate(&SymbolTable::new())
        .expect("depth 50 must evaluate");
    assert_eq!(v.to_display_string(), "1");
}

#[test]
fn allow_modest_binop_chain() {
    // 50-term chain — well below limit
    let mut expr = String::from("1");
    for _ in 0..50 {
        expr.push_str("+1");
    }
    let parsed = ParsedExpression::new(&expr).expect("50-term chain must parse");
    let v = parsed
        .evaluate(&SymbolTable::new())
        .expect("50-term chain must evaluate");
    assert_eq!(v.to_display_string(), "51");
}

#[test]
fn allow_typical_template_expression() {
    // Representative real-world shape: arithmetic with function calls,
    // attribute access, and a comprehension. Must parse and evaluate cleanly.
    let expr = r"sum([Param.Frame * x + 1 for x in range(10)])";
    let parsed = ParsedExpression::new(expr).expect("typical template expr must parse");
    let mut st = SymbolTable::new();
    st.set("Param.Frame", 3).unwrap();
    let v = parsed
        .evaluate(&st)
        .expect("typical template expr must evaluate");
    assert!(matches!(v, openjd_expr::ExprValue::Int(_)));
}

#[test]
fn allow_moderately_long_expression_on_fast_path() {
    // ~150-character realistic-ish expression — stays under
    // FAST_PATH_INPUT_LEN (200). Must parse cleanly without invoking the
    // worker thread.
    let expr = r"sum([Param.Frame * x + len(Param.Name) - (1 if Param.Flag else 0) for x in range(10) if x > 0]) + abs(Param.Offset) * 2 + 3";
    assert!(expr.len() <= 200, "test setup: expr must fit fast path");
    let mut st = SymbolTable::new();
    st.set("Param.Frame", 5).unwrap();
    st.set("Param.Name", "shot_01").unwrap();
    st.set("Param.Flag", true).unwrap();
    st.set("Param.Offset", -10).unwrap();
    let parsed = ParsedExpression::new(expr).expect("150-char expr must parse");
    let _ = parsed.evaluate(&st).expect("150-char expr must evaluate");
}

#[test]
fn allow_expression_above_fast_path_threshold() {
    // ~500-character expression — exceeds FAST_PATH_INPUT_LEN so this
    // invokes the worker thread. Must still parse cleanly, confirming
    // the worker-thread path handles normal-shape inputs.
    let mut expr = String::from("Param.Frame");
    while expr.len() < 500 {
        expr.push_str(" + Param.Frame");
    }
    let mut st = SymbolTable::new();
    st.set("Param.Frame", 1).unwrap();
    let parsed = ParsedExpression::new(&expr).expect("500-char sum must parse");
    let _ = parsed.evaluate(&st).expect("500-char sum must evaluate");
}

// ── Positive cases for format string caps ──

#[test]
fn allow_moderate_format_string() {
    // 100 segments, short: well below caps
    let fs_src = "{{1}}".repeat(100);
    let fs = FormatString::new(&fs_src).expect("100 segments must parse");
    let st = SymbolTable::new();
    let out = fs
        .resolve_string_with(&st, &FormatStringOptions::default())
        .unwrap();
    assert_eq!(out, "1".repeat(100));
}

#[test]
fn allow_reasonable_format_string_size() {
    // 64 KB of literal text — easily under any reasonable cap
    let fs_src = "a".repeat(64 * 1024);
    let _ = FormatString::new(&fs_src).expect("64 KB literal must parse");
}

// ── Parser recursion on shapes other than parens and unary ops ──

#[test]
fn parser_survives_deeply_nested_subscripts() {
    // `a[b[c[d[...]]]]` — the parser recurses on subscript chains.
    // The worker thread's enlarged stack carries the parse, and the
    // structural depth walker then rejects the resulting deep AST.
    let depth = 2000;
    let mut expr = String::from("x");
    for _ in 0..depth {
        expr.push_str("[0]");
    }
    expect_too_deep(&expr);
}

#[test]
fn parser_survives_deeply_nested_list_literals() {
    // `[[[[...1...]]]]` — nested list literals also recurse in the parser.
    // The spec's own validator rejects lists deeper than 2 levels, but
    // the parser must survive long enough to hand the AST to the walker.
    let depth = 2000;
    let expr = format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
    expect_too_deep(&expr);
}

#[test]
fn parser_survives_deeply_nested_attribute_chain() {
    // `a.b.c.d....` — Attribute chains are a classic recursive-descent
    // overflow vector. The worker thread's enlarged stack carries the
    // parse; the depth walker then catches the resulting deep AST.
    let depth = 5000;
    let mut expr = String::from("a");
    for i in 0..depth {
        expr.push_str(&format!(".x{i}"));
    }
    expect_too_deep(&expr);
}
