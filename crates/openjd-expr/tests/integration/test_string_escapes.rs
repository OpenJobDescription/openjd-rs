// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! String literal escapes (Expression Language §1.1.5) and the `re_sub`
//! control-character rule (§2.2.5), from openjd-rs#311.
//!
//! Every expected value below is what CPython 3.12 gives for the same
//! literal, except where a test says otherwise: a surrogate escape, which
//! CPython keeps in the string and EXPR rejects, and a lowercase algorithmic
//! `\N{...}` name, which CPython accepts from 3.13.

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
        // An algorithmic name, in lowercase: CPython accepts it from 3.13.
        (r"'\N{cjk unified ideograph-4e00}'", "\u{4e00}"),
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

/// A `\` before CR LF or a lone CR continues the line too, as CPython reads
/// source.
#[test]
fn backslash_crlf_is_a_line_continuation() {
    assert_eq!(eval_str("'ab\\\r\ncd'"), "abcd");
    assert_eq!(eval_str("'ab\\\rcd'"), "abcd");
    assert_eq!(eval_str("'''ab\\\rcd'''"), "abcd");
}

// ══════════════════════════════════════════════════════════════
// A line break inside a string literal is one LF
// ══════════════════════════════════════════════════════════════

/// The line breaks that §1.1.5 reads as one LF, written literally.
const LINE_BREAKS: [(&str, &str); 3] = [("LF", "\n"), ("CR LF", "\r\n"), ("CR", "\r")];

/// CPython reads source with universal newlines, so a literal LF, CR LF or
/// lone CR inside a triple-quoted string, raw or not, is one LF.
#[test]
fn literal_line_break_in_a_triple_quoted_string_is_lf() {
    for (name, br) in LINE_BREAKS {
        for (open, close) in [
            ("'''", "'''"),
            ("\"\"\"", "\"\"\""),
            ("r'''", "'''"),
            ("R\"\"\"", "\"\"\""),
        ] {
            let expr = format!("{open}ab{br}cd{close}");
            assert_eq!(eval_str(&expr), "ab\ncd", "{name} in {expr:?}");
        }
    }
}

/// Each line break is its own LF: CR CR is two, and so is LF CR, which a
/// pairwise reading might take as one.
#[test]
fn adjacent_line_breaks_are_separate_lfs() {
    let cases: &[(&str, &str)] = &[
        ("'''ab\r\rcd'''", "ab\n\ncd"),
        ("'''ab\r\r\ncd'''", "ab\n\ncd"),
        ("'''ab\n\rcd'''", "ab\n\ncd"),
        ("'''ab\r\n\r\ncd'''", "ab\n\ncd"),
        ("r'''ab\r\r\ncd'''", "ab\n\ncd"),
        ("'''\r\nab\r\n'''", "\nab\n"),
    ];
    for (expr, want) in cases {
        assert_eq!(eval_str(expr), *want, "{expr:?}");
    }
}

/// Only literal line breaks change. A `\r` escape still gives CR, beside a
/// literal CR LF or not.
#[test]
fn escaped_cr_still_gives_cr() {
    let cases: &[(&str, &str)] = &[
        ("'''a\\r\r\nb'''", "a\r\nb"),
        ("'''a\r\n\\rb'''", "a\n\rb"),
        ("'''a\\r\\nb'''", "a\r\nb"),
        ("'''a\\x0d\rb'''", "a\r\nb"),
        ("'''a\\15\r\nb'''", "a\r\nb"),
        ("'a\\r'", "a\r"),
        // In a raw string `\r` is two characters, and the line break after
        // it is still one LF.
        ("r'''a\\r\r\nb'''", "a\\r\nb"),
    ];
    for (expr, want) in cases {
        assert_eq!(eval_str(expr), *want, "{expr:?}");
    }
}

/// A backslash before a line break still continues the line, in a short or
/// triple-quoted string. In a raw string the backslash stays and the line
/// break after it is one LF, as CPython gives.
#[test]
fn backslash_before_each_line_break() {
    for (name, br) in LINE_BREAKS {
        for expr in [
            format!("'ab\\{br}cd'"),
            format!("'''ab\\{br}cd'''"),
            format!("\"\"\"ab\\{br}cd\"\"\""),
        ] {
            assert_eq!(eval_str(&expr), "abcd", "{name} in {expr:?}");
        }
        for expr in [format!("r'ab\\{br}cd'"), format!("r'''ab\\{br}cd'''")] {
            assert_eq!(eval_str(&expr), "ab\\\ncd", "{name} in {expr:?}");
        }
        // An escaped backslash doesn't continue the line.
        let expr = format!("'''ab\\\\{br}cd'''");
        assert_eq!(eval_str(&expr), "ab\\\ncd", "{name} in {expr:?}");
    }
}

/// CPython ends a short string at a line break, so a literal LF, CR LF or
/// CR in one, raw or not, is a syntax error. CPython says "unterminated
/// string literal" and ruff says "missing closing quote in string literal".
#[test]
fn literal_line_break_in_a_short_string_is_an_error() {
    for (name, br) in LINE_BREAKS {
        for expr in [
            format!("'ab{br}cd'"),
            format!("\"ab{br}cd\""),
            format!("r'ab{br}cd'"),
        ] {
            let e = err(&expr);
            assert!(
                e.starts_with("Syntax error: missing closing quote in string literal"),
                "{name} in {expr:?}: {e}"
            );
        }
    }
}

/// Each part of an implicit concatenation is decoded on its own, and a part
/// with no line break is left as it is.
#[test]
fn line_breaks_in_concatenated_parts() {
    let cases: &[(&str, &str)] = &[
        ("'''a\rb''' '''c\r\nd'''", "a\nbc\nd"),
        ("'x\\r' '''a\r\nb''' 'y'", "x\ra\nby"),
        ("r'''a\r''' '\\r'", "a\n\r"),
    ];
    for (expr, want) in cases {
        assert_eq!(eval_str(expr), *want, "{expr:?}");
    }
}

/// A string with a line break in it still works in the rest of an
/// expression: as an argument, in a list, and in a multi-line expression.
#[test]
fn line_breaks_inside_larger_expressions() {
    assert_eq!(eval_str("'''a\r\nb'''.upper()"), "A\nB");
    assert_eq!(eval_str("len('''a\r\nb''') == 3 and 'x' or 'y'"), "x");
    assert_eq!(eval_str("['''a\rb''', '''c\r\nd'''][1]"), "c\nd");
    assert_eq!(eval_str("(\n'''a\r\nb'''\n)"), "a\nb");
    assert_eq!(eval_str("[s for s in ['''a\r\nb''']][0]"), "a\nb");
}

/// An error after a string with a CR LF in it keeps its caret on the
/// offending text: no offset moves, because the source isn't rewritten.
#[test]
fn error_caret_after_a_string_with_a_line_break() {
    let e = err("'''a\rb''' + '\\ud800'");
    assert!(e.starts_with(SURROGATE_D800), "{e}");
    assert!(e.ends_with("^~~~~~"), "{e}");
    assert_err(
        "'''a\r\nb''' + '\\ud800'",
        // The caret display shows the line the escape is on.
        &[SURROGATE_D800, "  b''' + '\\ud800'\n", "          ^~~~~~"],
    );
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

const SURROGATE_D800: &str =
    "Syntax error: \\ud800 is a surrogate code point, which a string can't hold\n";

/// A surrogate escape in a triple-quoted string, in the second part of an
/// implicit concatenation, after an escaped backslash, and after a line
/// continuation is still found.
#[test]
fn surrogate_escape_found_in_every_string_form() {
    assert_err(
        r"'''a\ud800'''",
        &[SURROGATE_D800, "  '''a\\ud800'''\n", "      ^~~~~~"],
    );
    assert_err(
        r"'ok' '\udbff'",
        &[
            "Syntax error: \\udbff is a surrogate code point, which a string can't hold\n",
            "  'ok' '\\udbff'\n",
            "        ^~~~~~",
        ],
    );
    assert_err(
        r"'\\\ud800'",
        &[SURROGATE_D800, "  '\\\\\\ud800'\n", "     ^~~~~~"],
    );
    // A multi-line expression shows the line the escape is on.
    assert_err(
        "'a\\\n\\ud800'",
        &[SURROGATE_D800, "  \\ud800'\n", "  ^~~~~~"],
    );
}

/// Two `\u` escapes are never combined, so a surrogate pair is an error
/// that names the first half (audit round 4: no test had a pair).
#[test]
fn surrogate_pair_is_an_error() {
    assert_err(
        r"'\uD83D\uDE00'",
        &[
            "Syntax error: \\uD83D is a surrogate code point, which a string can't hold\n",
            "  '\\uD83D\\uDE00'\n",
            "   ^~~~~~",
        ],
    );
}

/// A literal in a slice bound is checked like any other (audit round 1:
/// slice bounds weren't walked by AST validation).
#[test]
fn surrogate_escape_in_a_slice_bound_is_an_error() {
    assert_err(
        r"'abc'[0:len('\ud800')]",
        &[
            SURROGATE_D800,
            "  'abc'[0:len('\\ud800')]\n",
            "               ^~~~~~",
        ],
    );
    assert_err(
        r"'abc'[::len('\U0000d800')]",
        &[
            "Syntax error: \\U0000d800 is a surrogate code point, which a string can't hold\n",
            "  'abc'[::len('\\U0000d800')]\n",
            "               ^~~~~~~~~~",
        ],
    );
    assert_err(
        r"[1, 2, 3][len('\udfff'):]",
        &[
            "Syntax error: \\udfff is a surrogate code point, which a string can't hold\n",
            "  [1, 2, 3][len('\\udfff'):]\n",
            "                 ^~~~~~",
        ],
    );
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

// These messages are ruff's lexer's own.
const UNEXPECTED_UNICODE: &str = "Syntax error: Got unexpected unicode\n";

#[test]
fn truncated_hex_escape_is_an_error() {
    assert_err(r"'\x4g'", &[UNEXPECTED_UNICODE, "  '\\x4g'\n", "      ^"]);
    assert_err(r"'\u12'", &[UNEXPECTED_UNICODE, "  '\\u12'\n", "  ^~~~~~"]);
    assert_err(
        r"'\U1234'",
        &[UNEXPECTED_UNICODE, "  '\\U1234'\n", "  ^~~~~~~~"],
    );
}

#[test]
fn unknown_or_missing_unicode_name_is_an_error() {
    assert_err(
        r"'\N{NOT A CHARACTER NAME}'",
        &[
            UNEXPECTED_UNICODE,
            "  '\\N{NOT A CHARACTER NAME}'\n",
            "      ^~~~~~~~~~~~~~~~~~~~",
        ],
    );
    assert_err(
        r"'\N'",
        &[
            "Syntax error: Missing `{` in Unicode escape sequence\n",
            "  '\\N'\n",
            "  ^~~~",
        ],
    );
}

#[test]
fn unicode_escape_above_max_code_point_is_an_error() {
    assert_err(
        r"'\U00110000'",
        &[UNEXPECTED_UNICODE, "  '\\U00110000'\n", "  ^~~~~~~~~~~~"],
    );
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
        let message = format!(
            "Control characters in replacement strings are not supported (U+{:04X})\n",
            u32::from(c)
        );
        let source = format!("  {expr}\n");
        let caret = format!("  ^{}", "~".repeat(expr.len() - 1));
        assert_err(&expr, &[&message, &source, &caret]);
    }
}

/// Only the first control character is named.
#[test]
fn re_sub_names_the_first_control_character() {
    assert_err(
        r"re_sub('a-b', '-', 'x\x7f\x01')",
        &[
            "Control characters in replacement strings are not supported (U+007F)\n",
            "  re_sub('a-b', '-', 'x\\x7f\\x01')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

/// The group-reference check runs first, so a replacement with both a
/// control character and a group reference reports the group reference
/// (audit round 1: nothing pinned the order).
#[test]
fn re_sub_group_reference_reported_before_control_character() {
    assert_err(
        r"re_sub('a-b', '-', '\x01$1')",
        &[
            "Group references in replacement strings are not supported\n",
            "  re_sub('a-b', '-', '\\x01$1')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
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

/// The replacement is checked whether or not the pattern matches (audit
/// round 3: every other test's pattern matched).
#[test]
fn re_sub_checks_the_replacement_when_nothing_matches() {
    assert_err(
        r"re_sub('abc', 'x', '\x01')",
        &[
            "Control characters in replacement strings are not supported (U+0001)\n",
            "  re_sub('abc', 'x', '\\x01')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
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
    assert_eq!(
        e,
        concat!(
            "Control characters in replacement strings are not supported (U+001B)\n",
            "  re_sub('a-b', '-', Param.Repl)\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        )
    );
}

/// A group reference written raw is still rejected as a group reference.
#[test]
fn re_sub_group_reference_still_rejected_raw() {
    assert_err(
        r"re_sub('a1', '(\d)', r'\1')",
        &[
            "Group references in replacement strings are not supported\n",
            "  re_sub('a1', '(\\d)', r'\\1')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
