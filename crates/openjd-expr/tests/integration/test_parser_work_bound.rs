// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Regression tests for #489: parsing an expression must take time roughly
//! linear in its length, whatever its bracket nesting, up to the length cap.
//! The one known exception is unclosed nested f-strings, which are
//! quadratic (see `nested_f_strings_are_bounded`).
//!
//! The `rustpython-ruff_python_parser` 0.16.10 crate this crate used to
//! depend on computed CPython-style error hints by parsing ahead, rewinding,
//! and parsing the same tokens again. Hints nested inside brackets, so each
//! unclosed bracket level doubled the work (`"f(a " * 24 + "x"`, 97 bytes,
//! took 12 s in a release build), and a long run of juxtaposed tokens inside
//! one bracket was cubic (`"[" + "a " * 2000`, 4 KB, took 3.7 s). An
//! expression is untrusted template input, so that was a denial of service
//! on `openjd check` and job creation.
//!
//! Each input below is one of the shapes measured for #489. Most are sized
//! so the old parser would take minutes or more in a release build; the
//! single fuzz reproducer is kept as found (2.5 s in release, over the bound
//! only in a debug build), and its repeated form covers it in release. Each
//! must be rejected with an error within [`TIME_BOUND`]. The bound is far above what a linear parser
//! needs (tens of milliseconds in a debug build) so that a slow CI machine
//! cannot flake, and far below what the exponential or cubic behavior costs,
//! so a parser change that brings it back fails here instead of hanging.

use openjd_expr::{FormatString, ParsedExpression, SymbolTable, MAX_PARSE_INPUT_LEN};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Generous per-input bound. See the module docs.
const TIME_BOUND: Duration = Duration::from_secs(10);

/// The 363-byte fuzz reproducer from #489 (mostly `.`, ten brackets left
/// open). It took 2.5 s to parse with the old parser in a release build.
const FUZZ_SLOW_DOTS: &str = "ginaen([manifestVersion.......................(............(..............................join.................(.....................n.................(...........................destination_55555path.(..............data............................@....so, 1],.so[vted().d(......destination_55555path.(..............data................................so, dx in e";

/// Run `f` on its own thread and fail the test if it doesn't finish within
/// [`TIME_BOUND`]. A thread that overruns is left running; the test binary
/// exits when the harness finishes.
fn within_bound<T: Send + 'static>(label: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    let start = Instant::now();
    std::thread::Builder::new()
        .name(format!("parse-bound-{label}"))
        .spawn(move || {
            let _ = tx.send(f());
        })
        .expect("spawn test thread");
    match rx.recv_timeout(TIME_BOUND) {
        Ok(v) => v,
        Err(mpsc::RecvTimeoutError::Timeout) => panic!(
            "{label}: parsing took longer than {TIME_BOUND:?} (#489: error recovery must not \
             be exponential or polynomial in the input)"
        ),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("{label}: parse thread panicked after {:?}", start.elapsed())
        }
    }
}

/// Assert that `expr` is rejected with a syntax error, and quickly.
fn expect_fast_syntax_error(label: &str, expr: String) {
    assert!(
        expr.len() <= MAX_PARSE_INPUT_LEN,
        "{label}: input is {} bytes, so it would be rejected by the length cap \
         before the parser runs, which tests nothing",
        expr.len()
    );
    let len = expr.len();
    let msg = within_bound(label, move || {
        ParsedExpression::new(&expr)
            .map(|_| ())
            .map_err(|e| e.message())
    })
    .expect_err(&format!("{label}: a {len}-byte invalid expression parsed"));
    assert!(
        msg.starts_with("Syntax error: "),
        "{label}: expected a syntax error, got: {msg}"
    );
}

/// Repeat `unit` until the result fills as much of `MAX_PARSE_INPUT_LEN` as
/// whole copies allow, leaving `reserve` bytes for a prefix and suffix.
fn fill(unit: &str, reserve: usize) -> String {
    unit.repeat((MAX_PARSE_INPUT_LEN - reserve) / unit.len())
}

// ── Exponential in unclosed bracket depth ──

#[test]
fn unclosed_if_call_brackets_from_issue() {
    // The issue's reproducer is 20 levels (0.8 s); 40 would have taken days.
    expect_fast_syntax_error("[i f( x40", format!("{}x", "[i f(".repeat(40)));
}

#[test]
fn unclosed_calls_with_juxtaposed_arguments() {
    expect_fast_syntax_error("f(a x40", format!("{}x", "f(a ".repeat(40)));
}

#[test]
fn unclosed_sets_with_juxtaposed_elements() {
    expect_fast_syntax_error("{a x40", format!("{}x", "{a ".repeat(40)));
}

#[test]
fn unclosed_lists_with_juxtaposed_elements() {
    expect_fast_syntax_error("[a b x40", format!("{}x", "[a b ".repeat(40)));
}

#[test]
fn unclosed_comprehensions_missing_in() {
    expect_fast_syntax_error("[x for x40", format!("{}x", "[x for ".repeat(40)));
}

#[test]
fn unclosed_calls_with_string_then_name() {
    // A string literal next to a name: the old parser's
    // "Is this intended to be part of the string?" hint, the steepest case
    // measured (`"f(\"a\" b " * 12`, 84 bytes, took 9 s).
    expect_fast_syntax_error("f(\"a\" b x40", format!("{}x", "f(\"a\" b ".repeat(40)));
}

#[test]
fn unclosed_subscripts_with_juxtaposed_index() {
    expect_fast_syntax_error("x[1 x40", format!("{}x", "x[1 ".repeat(40)));
}

#[test]
fn closed_if_call_brackets() {
    // Closing the brackets doesn't help the old parser: the doubling comes
    // from the hints inside them.
    expect_fast_syntax_error(
        "[i f( x40 closed",
        format!("{}x{}", "[i f(".repeat(40), ")]".repeat(40)),
    );
}

#[test]
fn fuzz_reproducer_slow_dots() {
    expect_fast_syntax_error("fuzz slow-dots", FUZZ_SLOW_DOTS.to_string());
}

// ── Polynomial at bracket depth 1, up to the length cap ──

#[test]
fn juxtaposed_names_in_one_bracket_at_length_cap() {
    expect_fast_syntax_error("[ a a a ... 64 KB", format!("[{}", fill("a ", 1)));
}

#[test]
fn dots_in_one_bracket_at_length_cap() {
    expect_fast_syntax_error("[ ..... 64 KB", format!("[{}", fill(".", 1)));
}

#[test]
fn repeated_depth_12_blocks_at_length_cap() {
    let block = format!("{}x{}, ", "f(a ".repeat(12), ")".repeat(12));
    expect_fast_syntax_error("depth-12 blocks 64 KB", format!("[{}x]", fill(&block, 3)));
}

#[test]
fn repeated_fuzz_reproducer_at_length_cap() {
    let unit = format!("{FUZZ_SLOW_DOTS} ");
    expect_fast_syntax_error("fuzz slow-dots x180", fill(&unit, 0));
}

#[test]
fn keyword_attribute_chain_at_length_cap() {
    // Contextual keywords used as attribute names are renamed in one lexer
    // pass. openjd-expr used to re-parse the whole expression once per
    // keyword, which took 89 s for this input in a release build. It is
    // valid syntax, so the AST depth limit rejects it after parsing.
    let expr = format!("a{}", fill(".if", 1));
    let msg = within_bound("a.if.if... 64 KB", move || {
        ParsedExpression::new(&expr)
            .map(|_| ())
            .map_err(|e| e.message())
    })
    .expect_err("an attribute chain that deep must be rejected");
    assert!(
        msg.contains("nesting depth"),
        "expected a depth error, got: {msg}"
    );
}

#[test]
fn keyword_attributes_in_a_long_sum_parse() {
    // Many keyword attributes in one expression, all renamed in the same
    // lexer pass and sharing one placeholder. A flat list keeps the AST
    // shallow, so this parses.
    let body = "Param.if, ".repeat(400);
    let expr = format!("sum([{}])", body.trim_end_matches([',', ' ']));
    let parsed = within_bound("Param.if x400", move || ParsedExpression::new(&expr))
        .expect("keyword attributes in a list must parse");
    assert_eq!(
        parsed.accessed_symbols().iter().collect::<Vec<_>>(),
        vec!["Param.if"]
    );
}

#[test]
fn nested_f_strings_are_bounded() {
    // Unclosed nested f-strings are the one shape still worse than linear
    // with this parser: quadratic, about 0.5 s at 64 KB in a release build
    // and 15 s in a debug build. At 8 KB (an eighth of the cap, so about a
    // sixty-fourth of the work) this stays well inside the bound in both.
    let expr = format!("{}x", "f'{".repeat(8 * 1024 / 3));
    expect_fast_syntax_error("f'{ x2730", expr);
}

// ── The template-field route ──

#[test]
fn format_string_interpolation_is_bounded() {
    // Template fields reach the parser through FormatString, one
    // `{{ ... }}` interpolation at a time.
    let src = format!("value: {{{{ {}x }}}}", "f(a ".repeat(40));
    let result = within_bound("format string f(a x40", move || {
        FormatString::new(&src)
            .map(|_| ())
            .map_err(|e| e.to_string())
    });
    let msg = result.expect_err("an invalid interpolation must be rejected");
    assert!(
        msg.contains("Syntax error: "),
        "expected a syntax error, got: {msg}"
    );
}

// ── Valid input of the same sizes still parses ──

#[test]
fn valid_expression_at_length_cap_parses() {
    // Exactly MAX_PARSE_INPUT_LEN bytes: the cap is inclusive.
    let expr = format!("'{}'", "a".repeat(MAX_PARSE_INPUT_LEN - 2));
    assert_eq!(expr.len(), MAX_PARSE_INPUT_LEN);
    let parsed = within_bound("64 KB string literal", move || ParsedExpression::new(&expr))
        .expect("a 64 KB string literal must parse");
    let value = parsed
        .evaluate(&SymbolTable::new())
        .expect("a string literal must evaluate");
    assert_eq!(value.to_display_string().len(), MAX_PARSE_INPUT_LEN - 2);
}

#[test]
fn valid_list_near_length_cap_parses() {
    // A long flat list: the shape of a large inline value, with commas
    // where the invalid inputs above have juxtaposed names.
    let body = fill("a, ", 2);
    let expr = format!("[{}]", body.trim_end_matches([',', ' ']));
    let parsed = within_bound("64 KB list", move || ParsedExpression::new(&expr))
        .expect("a 64 KB list literal must parse");
    assert_eq!(
        parsed.accessed_symbols().iter().collect::<Vec<_>>(),
        vec!["a"]
    );
}
