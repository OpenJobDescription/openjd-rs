// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! String literal escapes (Expression Language §1.1.5) and the `re_sub`
//! control-character rule (§2.2.5), from openjd-rs#311.
//!
//! Every expected value below is what CPython 3.12 gives for the same
//! literal, except where a test says otherwise: a surrogate escape, which
//! CPython keeps in the string and EXPR rejects.

use openjd_expr::{ExprValue, ParsedExpression, SymbolTable};

fn eval_str(expr: &str) -> String {
    match ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_or_else(|e| panic!("{expr}: {e}"))
    {
        ExprValue::String(s) => s,
        other => panic!("{expr}: expected a string, got {other:?}"),
    }
}

fn err(expr: &str) -> String {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string()
}

fn assert_err(expr: &str, expected: &[&str]) {
    let e = err(expr);
    let joined = expected.concat();
    assert_eq!(e, joined, "expression: {expr}");
}

// ══════════════════════════════════════════════════════════════
// Every escape in the §1.1.5 table
// ══════════════════════════════════════════════════════════════

#[test]
fn single_character_escapes() {
    let cases: &[(&str, &str)] = &[
        (r"'\\'", "\\"),
        (r"'\''", "'"),
        (r#"'\"'"#, "\""),
        (r"'\a'", "\u{7}"),
        (r"'\b'", "\u{8}"),
        (r"'\f'", "\u{c}"),
        (r"'\n'", "\n"),
        (r"'\r'", "\r"),
        (r"'\t'", "\t"),
        (r"'\v'", "\u{b}"),
    ];
    for (expr, want) in cases {
        assert_eq!(eval_str(expr), *want, "{expr}");
    }
}

#[test]
fn octal_escapes() {
    let cases: &[(&str, &str)] = &[
        (r"'\0'", "\u{0}"),
        (r"'\1'", "\u{1}"),
        (r"'\7'", "\u{7}"),
        (r"'\12'", "\n"),
        (r"'\37'", "\u{1f}"),
        (r"'\101'", "A"),
        (r"'\377'", "\u{ff}"),
        // At most 3 digits are read.
        (r"'\1234'", "S4"),
        (r"'\0000'", "\u{0}0"),
        // 8 and 9 are not octal digits, so they end the escape.
        (r"'\18'", "\u{1}8"),
        (r"'\08'", "\u{0}8"),
        (r"'\779'", "\u{3f}9"),
    ];
    for (expr, want) in cases {
        assert_eq!(eval_str(expr), *want, "{expr}");
    }
}

/// A 3-digit octal escape above `\377` is the character with that code
/// point. CPython 3.12 warns about it but still gives that character, and
/// §1.1.5 pins the meaning.
#[test]
fn octal_escape_above_377() {
    assert_eq!(eval_str(r"'\400'"), "\u{100}");
    assert_eq!(eval_str(r"'\777'"), "\u{1ff}");
}

#[test]
fn hex_and_unicode_escapes() {
    let cases: &[(&str, &str)] = &[
        (r"'\x41'", "A"),
        (r"'\x7E\x7e'", "~~"),
        (r"'\xff'", "\u{ff}"),
        (r"'\u00e9'", "\u{e9}"),
        (r"'\uFFFF'", "\u{ffff}"),
        (r"'\U0001F600'", "\u{1f600}"),
        (r"'\U0010FFFF'", "\u{10ffff}"),
        (r"'\N{BULLET}'", "\u{2022}"),
        // Names match without regard to case, and aliases are accepted.
        (r"'\N{bullet}'", "\u{2022}"),
        (r"'\N{LATIN CAPITAL LETTER GHA}'", "\u{1a2}"),
    ];
    for (expr, want) in cases {
        assert_eq!(eval_str(expr), *want, "{expr}");
    }
}

/// The edges of the surrogate range are ordinary characters.
#[test]
fn unicode_escapes_next_to_the_surrogate_range() {
    assert_eq!(eval_str(r"'\ud7ff'"), "\u{d7ff}");
    assert_eq!(eval_str(r"'\ue000'"), "\u{e000}");
    assert_eq!(eval_str(r"'\U0000D7FF'"), "\u{d7ff}");
    assert_eq!(eval_str(r"'\U0000E000'"), "\u{e000}");
}

#[test]
fn backslash_newline_is_a_line_continuation() {
    assert_eq!(eval_str("'ab\\\ncd'"), "abcd");
    assert_eq!(eval_str("\"ab\\\ncd\""), "abcd");
    assert_eq!(eval_str("'''ab\\\ncd'''"), "abcd");
    assert_eq!(eval_str("\"\"\"ab\\\ncd\"\"\""), "abcd");
    // In a raw string the backslash and the newline both stay.
    assert_eq!(eval_str("r'ab\\\ncd'"), "ab\\\ncd");
}

/// A `\` before CR LF continues the line too, as CPython reads source.
#[test]
fn backslash_crlf_is_a_line_continuation() {
    assert_eq!(eval_str("'ab\\\r\ncd'"), "abcd");
}

// ══════════════════════════════════════════════════════════════
// Unrecognized escapes keep their backslash, permanently
// ══════════════════════════════════════════════════════════════

#[test]
fn unrecognized_escapes_stay_verbatim() {
    let cases: &[(&str, &str)] = &[
        (r"'\d'", "\\d"),
        (r"'\q'", "\\q"),
        (r"'\8'", "\\8"),
        (r"'\9'", "\\9"),
        (r"'\ '", "\\ "),
        (r"'\é'", "\\é"),
        (r"'\o'", "\\o"),
        (r#""\w+""#, "\\w+"),
        (r"'''\s'''", "\\s"),
    ];
    for (expr, want) in cases {
        assert_eq!(eval_str(expr), *want, "{expr}");
    }
}

/// An unrecognized escape followed by a backslash: the second backslash
/// starts its own escape.
#[test]
fn unrecognized_escape_then_escaped_backslash() {
    assert_eq!(eval_str(r"'\q\\'"), "\\q\\");
}

// ══════════════════════════════════════════════════════════════
// Malformed escapes are syntax errors
// ══════════════════════════════════════════════════════════════

#[test]
fn surrogate_u_escape_is_an_error() {
    assert_err(
        r"'\ud800'",
        &[
            "Syntax error: \\ud800 is a surrogate code point, which a string can't hold\n",
            "  '\\ud800'\n",
            "   ^~~~~~",
        ],
    );
}

#[test]
fn surrogate_u_escape_upper_hex_is_an_error() {
    assert_err(
        r"'ab\uDFFF'",
        &[
            "Syntax error: \\uDFFF is a surrogate code point, which a string can't hold\n",
            "  'ab\\uDFFF'\n",
            "     ^~~~~~",
        ],
    );
}

#[test]
fn surrogate_capital_u_escape_is_an_error() {
    assert_err(
        r#"len("x" + "\U0000dc00")"#,
        &[
            "Syntax error: \\U0000dc00 is a surrogate code point, which a string can't hold\n",
            "  len(\"x\" + \"\\U0000dc00\")\n",
            "             ^~~~~~~~~~",
        ],
    );
}

/// A surrogate escape in a triple-quoted string, in the second part of an
/// implicit concatenation, and after an escaped backslash is still found.
#[test]
fn surrogate_escape_found_in_every_string_form() {
    assert!(err(r"'''a\ud800'''").contains("\\ud800 is a surrogate code point"));
    assert!(err(r"'ok' '\udbff'").contains("\\udbff is a surrogate code point"));
    assert!(err(r"'\\\ud800'").contains("\\ud800 is a surrogate code point"));
    assert!(err("'a\\\n\\ud800'").contains("\\ud800 is a surrogate code point"));
}

/// `\\u` is an escaped backslash followed by `u`, not a `\u` escape, and a
/// raw string has no escapes at all.
#[test]
fn surrogate_shaped_text_that_is_not_an_escape() {
    assert_eq!(eval_str(r"'\\ud800'"), "\\ud800");
    assert_eq!(eval_str(r"r'\ud800'"), "\\ud800");
    assert_eq!(eval_str(r"R'\U0000d800'"), "\\U0000d800");
    assert_eq!(eval_str(r"'\N{LATIN SMALL LETTER U}d800'"), "ud800");
}

#[test]
fn truncated_hex_escape_is_an_error() {
    assert!(err(r"'\x4g'").starts_with("Syntax error: "));
    assert!(err(r"'\u12'").starts_with("Syntax error: "));
    assert!(err(r"'\U1234'").starts_with("Syntax error: "));
}

#[test]
fn unknown_or_missing_unicode_name_is_an_error() {
    assert!(err(r"'\N{NOT A CHARACTER NAME}'").starts_with("Syntax error: "));
    assert!(err(r"'\N'").starts_with("Syntax error: "));
}

#[test]
fn unicode_escape_above_max_code_point_is_an_error() {
    assert!(err(r"'\U00110000'").starts_with("Syntax error: "));
}

// ══════════════════════════════════════════════════════════════
// re_sub rejects control characters in the replacement
// ══════════════════════════════════════════════════════════════

/// The #311 case: a group reference written non-raw decodes to U+0001
/// before `re_sub` sees it.
#[test]
fn re_sub_non_raw_group_reference_is_rejected() {
    assert_err(
        r"re_sub('a1', '(\d)', '\1')",
        &[
            "Control characters in replacement strings are not supported (U+0001)\n",
            "  re_sub('a1', '(\\d)', '\\1')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn re_sub_rejects_every_disallowed_control_character() {
    let allowed = ['\t', '\n', '\r'];
    for c in (0u32..=0x1f).chain(0x7f..=0x9f).filter_map(char::from_u32) {
        if allowed.contains(&c) {
            continue;
        }
        let expr = format!("re_sub('a-b', '-', '\\x{:02x}')", u32::from(c));
        let e = err(&expr);
        assert!(
            e.starts_with(&format!(
                "Control characters in replacement strings are not supported (U+{:04X})\n",
                u32::from(c)
            )),
            "{expr}: {e}"
        );
    }
}

/// Only the first control character is named.
#[test]
fn re_sub_names_the_first_control_character() {
    assert!(err(r"re_sub('a-b', '-', 'x\x7f\x01')")
        .starts_with("Control characters in replacement strings are not supported (U+007F)\n"));
}

#[test]
fn re_sub_allows_tab_newline_and_carriage_return() {
    assert_eq!(eval_str(r"re_sub('a,b', ',', '\t')"), "a\tb");
    assert_eq!(eval_str(r"re_sub('a,b', ',', '\n')"), "a\nb");
    assert_eq!(eval_str(r"re_sub('a,b', ',', '\r\n')"), "a\r\nb");
    // The octal spellings of those three decode to them, so they're allowed.
    assert_eq!(eval_str(r"re_sub('a,b', ',', '\11\12\15')"), "a\t\n\rb");
}

/// Characters next to the control ranges, and other invisible characters
/// that aren't in category Cc, are allowed.
#[test]
fn re_sub_allows_characters_outside_category_cc() {
    let cases: &[(&str, &str)] = &[
        (r"re_sub('a,b', ',', '\x20')", "a b"),
        (r"re_sub('a,b', ',', '\x7e')", "a~b"),
        (r"re_sub('a,b', ',', '\xa0')", "a\u{a0}b"),
        (r"re_sub('a,b', ',', '\u200b')", "a\u{200b}b"),
        (r"re_sub('a,b', ',', '\u2028')", "a\u{2028}b"),
        (r"re_sub('a,b', ',', '\101')", "aAb"),
    ];
    for (expr, want) in cases {
        assert_eq!(eval_str(expr), *want, "{expr}");
    }
}

/// The rule applies to the replacement only, not to the input or pattern,
/// and an empty replacement is fine.
#[test]
fn re_sub_control_characters_elsewhere_are_allowed() {
    assert_eq!(eval_str(r"re_sub('a\x01b', 'b', 'c')"), "a\u{1}c");
    assert_eq!(eval_str(r"re_sub('a\x01b', '\x01', '-')"), "a-b");
    assert_eq!(eval_str(r"re_sub('a,b', ',', '')"), "ab");
}

/// A control character from a symbol, not a literal, is rejected too.
#[test]
fn re_sub_control_character_from_a_symbol_is_rejected() {
    let mut st = SymbolTable::new();
    st.set("Param.Repl", ExprValue::String("x\u{1b}y".into()))
        .unwrap();
    let e = ParsedExpression::new("re_sub('a-b', '-', Param.Repl)")
        .and_then(|p| p.evaluate(&st))
        .unwrap_err()
        .to_string();
    assert!(
        e.starts_with("Control characters in replacement strings are not supported (U+001B)\n"),
        "{e}"
    );
}

/// A group reference written raw is still rejected as a group reference.
#[test]
fn re_sub_group_reference_still_rejected_raw() {
    assert!(err(r"re_sub('a1', '(\d)', r'\1')")
        .starts_with("Group references in replacement strings are not supported\n"));
}
