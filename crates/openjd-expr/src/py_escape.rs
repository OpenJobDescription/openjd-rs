// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Python string-literal rendering, shared by `repr_py` (Expression Language
//! §2.2.6) and `ExprValue::repr_python`, so the two cannot disagree about how a
//! value is spelled.
//!
//! Sibling of [`crate::json_escape`], kept separate because Python picks its
//! delimiter per value and keeps printable non-ASCII verbatim, while JSON always
//! quotes with `"` and escapes all non-ASCII. `repr_pwsh` is a third scheme and
//! stays in `functions::repr`.
//!
//! The printability rule reads
//! [`NONPRINTABLE`][crate::functions::unicode_tables::NONPRINTABLE], generated
//! from the same pinned CPython as every other Python-parity table in the crate,
//! so `repr_py` and `str.isalpha` can never answer from different Unicode
//! versions.

use crate::functions::unicode_tables::{in_table, NONPRINTABLE};
use std::fmt::Write;

/// Write `value` as a Python string literal, matching CPython's `repr` of a `str`.
///
/// The output always parses back as a Python literal equal to `value`, for every
/// Unicode scalar value. Byte equality with a *particular* CPython also needs
/// that CPython's Unicode version to match the crate's; see [`is_non_printable`].
///
/// ```text
/// hello  -> 'hello'      it's      -> "it's"
/// a<LF>b -> 'a\nb'       a<NBSP>b  -> 'a\xa0b'
/// ```
pub(crate) fn write_py_string_literal(value: &str, buf: &mut String) {
    let quote = select_quote(value);
    buf.push(quote);
    for c in value.chars() {
        match c {
            // Delimiter and backslash first, as CPython does, so a later arm
            // cannot re-escape either.
            '\\' => buf.push_str("\\\\"),
            c if c == quote => {
                buf.push('\\');
                buf.push(c);
            }
            '\n' => buf.push_str("\\n"),
            '\r' => buf.push_str("\\r"),
            '\t' => buf.push_str("\\t"),
            c if is_non_printable(c) => write_escaped_code_point(c, buf),
            c => buf.push(c),
        }
    }
    buf.push(quote);
}

/// [`write_py_string_literal`] for callers that build their output with
/// `format!` rather than into a shared buffer.
pub(crate) fn py_string_literal(value: &str) -> String {
    let mut buf = String::new();
    write_py_string_literal(value, &mut buf);
    buf
}

/// CPython prefers `'`, switching to `"` only to avoid escaping an embedded
/// `'`. A value holding both quote characters keeps `'` and escapes it.
fn select_quote(value: &str) -> char {
    let mut has_single = false;
    let mut has_double = false;
    for c in value.chars() {
        match c {
            '\'' => has_single = true,
            '"' => has_double = true,
            _ => {}
        }
    }
    if has_single && !has_double {
        '"'
    } else {
        '\''
    }
}

/// `Py_UNICODE_ISPRINTABLE` inverted: not printable when the general category is
/// `C*` or `Z*`, except `U+0020`.
///
/// Below `U+0080` the answer is fixed for all time, so that range is decided
/// arithmetically and skips the table lookup.
fn is_non_printable(c: char) -> bool {
    if c.is_ascii() {
        return (c as u32) < 0x20 || c == '\x7f';
    }
    in_table(NONPRINTABLE, c)
}

/// Write `c` as CPython's numeric escape, narrowest form that fits. The widest,
/// `\UNNNNNNNN`, needs four input bytes, so the worst ratio is `\x00`'s
/// four-for-one, under `functions::repr`'s `MAX_ESCAPE_EXPANSION` of 6.
fn write_escaped_code_point(c: char, buf: &mut String) {
    let cp = c as u32;
    if cp <= 0xff {
        let _ = write!(buf, "\\x{cp:02x}");
    } else if cp <= 0xffff {
        let _ = write!(buf, "\\u{cp:04x}");
    } else {
        let _ = write!(buf, "\\U{cp:08x}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(value: &str) -> String {
        let mut buf = String::new();
        write_py_string_literal(value, &mut buf);
        buf
    }

    // ── Delimiter selection ──
    //
    // Every expectation in this module was read out of CPython and agrees with
    // 3.11.12, 3.12.10, 3.13.7 and 3.14.0b4.

    #[test]
    fn plain_value_uses_single_quotes() {
        assert_eq!(lit("hello"), "'hello'");
    }

    #[test]
    fn empty_value_uses_single_quotes() {
        assert_eq!(lit(""), "''");
    }

    #[test]
    fn embedded_single_quote_switches_the_delimiter() {
        assert_eq!(lit("it's"), "\"it's\"");
    }

    #[test]
    fn embedded_double_quote_keeps_single_quotes_unescaped() {
        assert_eq!(lit("say \"hi\""), "'say \"hi\"'");
    }

    #[test]
    fn both_quotes_keep_single_quotes_and_escape_them() {
        assert_eq!(lit("it's a \"x\""), "'it\\'s a \"x\"'");
    }

    #[test]
    fn double_quote_is_not_escaped_under_single_quotes() {
        assert_eq!(lit("\"x\""), "'\"x\"'");
    }

    #[test]
    fn single_quote_is_not_escaped_under_double_quotes() {
        assert_eq!(lit("'"), "\"'\"");
    }

    // ── Backslash ──

    #[test]
    fn backslash_doubles() {
        assert_eq!(lit("a\\b"), "'a\\\\b'");
    }

    #[test]
    fn trailing_backslash_doubles() {
        assert_eq!(lit("a\\"), "'a\\\\'");
    }

    #[test]
    fn backslash_before_a_quote_does_not_escape_the_delimiter_twice() {
        // Input is a backslash then a quote. The quote flips the delimiter to
        // `"`, so only the backslash is escaped.
        assert_eq!(lit("\\'"), "\"\\\\'\"");
    }

    // ── Named control escapes ──

    #[test]
    fn newline_escapes() {
        assert_eq!(lit("hello\nworld"), "'hello\\nworld'");
    }

    #[test]
    fn carriage_return_escapes() {
        assert_eq!(lit("a\rb"), "'a\\rb'");
    }

    #[test]
    fn crlf_escapes_both() {
        assert_eq!(lit("a\r\nb"), "'a\\r\\nb'");
    }

    #[test]
    fn tab_escapes() {
        assert_eq!(lit("a\tb"), "'a\\tb'");
    }

    // ── Numeric control escapes ──

    #[test]
    fn nul_escapes_as_hex() {
        assert_eq!(lit("a\u{0}b"), "'a\\x00b'");
    }

    #[test]
    fn escape_char_escapes_as_hex() {
        assert_eq!(lit("a\u{1b}b"), "'a\\x1bb'");
    }

    #[test]
    fn delete_escapes_as_hex() {
        assert_eq!(lit("a\u{7f}b"), "'a\\x7fb'");
    }

    #[test]
    fn vertical_tab_and_form_feed_escape_as_hex() {
        // CPython has no `\v` or `\f` in `repr`, unlike JSON's `\f`.
        assert_eq!(lit("a\u{b}\u{c}b"), "'a\\x0b\\x0cb'");
    }

    #[test]
    fn every_ascii_control_escapes_and_no_printable_ascii_does() {
        for cp in 0u32..0x80 {
            let c = char::from_u32(cp).unwrap();
            let out = lit(&c.to_string());
            let escaped = out.contains('\\');
            // A lone `'` is the one printable that could need escaping and
            // does not: it flips the delimiter to `"` instead, giving `"'"`.
            let want_escaped = cp < 0x20 || cp == 0x7f || c == '\\';
            assert_eq!(
                escaped, want_escaped,
                "U+{cp:04X} rendered as {out} (escaped={escaped}, want={want_escaped})"
            );
        }
    }

    #[test]
    fn no_code_point_closes_its_own_literal_early() {
        // The property that makes the output parseable: inside the delimiters,
        // the delimiter never appears unescaped and no raw line terminator
        // survives. Also the tightest place to check the preflight bound, since
        // a one-code-point value has no slack. Every scalar value.
        let mut buf = String::new();
        for cp in 0u32..=0x10FFFF {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            buf.clear();
            let mut one = [0u8; 4];
            write_py_string_literal(c.encode_utf8(&mut one), &mut buf);
            assert_no_early_close(&buf, cp, "bare");
            assert!(
                buf.len() - 2 <= c.len_utf8() * 6,
                "U+{cp:04X} renders {} bytes from {}, over the preflight bound",
                buf.len() - 2,
                c.len_utf8()
            );
        }
    }

    #[test]
    fn no_code_point_closes_a_literal_that_already_holds_both_quotes() {
        // The escaped-delimiter path is unreachable from a single-code-point
        // value: one holding only `'` flips the delimiter to `"` instead. These
        // shapes keep a `"` present so the delimiter stays `'` and has to be
        // escaped, which is the only way this assertion can fail.
        //
        // Not swept over the full range: the delimiter decision reads only the
        // two quote characters, so a code point can only matter by being one of
        // them or by escaping. Every ASCII value plus one representative per
        // non-printable category covers both.
        let interesting = (0u32..0x80)
            .chain([
                0x85, 0xa0, 0xad, 0x378, 0x2028, 0x2029, 0xe000, 0x3000, 0x100000,
            ])
            .chain([0xe9, 0x1f600, 0x4e00, 0xe0100]);
        for cp in interesting {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            for shape in [format!("it's a \"{c}\""), format!("{c}'{c}\"{c}")] {
                let out = lit(&shape);
                assert_eq!(
                    out.chars().next(),
                    Some('\''),
                    "U+{cp:04X} in {shape:?}: both quotes present, delimiter must stay `'`"
                );
                assert_no_early_close(&out, cp, &shape);
            }
        }
    }

    /// Walk a rendered literal's body and fail if the delimiter appears
    /// unescaped, or if a raw line terminator survives. Both end the literal in
    /// Python's lexer.
    fn assert_no_early_close(out: &str, cp: u32, shape: &str) {
        let quote = out.chars().next().unwrap();
        let body: Vec<char> = out.chars().skip(1).take(out.chars().count() - 2).collect();
        let mut i = 0;
        while i < body.len() {
            if body[i] == '\\' {
                i += 2;
                continue;
            }
            assert_ne!(
                body[i], quote,
                "U+{cp:04X} in {shape}: {out} closes its own literal early"
            );
            i += 1;
        }
        assert!(
            !out.contains('\n') && !out.contains('\r'),
            "U+{cp:04X} in {shape}: {out:?} carries a raw line terminator"
        );
    }

    // ── Non-ASCII: escaped only when the category says non-printable ──

    #[test]
    fn printable_non_ascii_stays_verbatim() {
        assert_eq!(lit("café"), "'café'");
        assert_eq!(lit("a😀b"), "'a😀b'");
    }

    #[test]
    fn c1_control_escapes_as_hex() {
        assert_eq!(lit("a\u{85}b"), "'a\\x85b'");
    }

    #[test]
    fn space_separator_escapes_but_plain_space_does_not() {
        // U+00A0 NBSP and U+3000 IDEOGRAPHIC SPACE are `Zs`, and so is U+0020.
        // CPython escapes the first two and keeps the third.
        assert_eq!(lit("a\u{a0}b"), "'a\\xa0b'");
        assert_eq!(lit("a\u{3000}b"), "'a\\u3000b'");
        assert_eq!(lit("a b"), "'a b'");
    }

    #[test]
    fn format_category_escapes() {
        // U+200B ZERO WIDTH SPACE and U+00AD SOFT HYPHEN are `Cf`.
        assert_eq!(lit("a\u{200b}b"), "'a\\u200bb'");
        assert_eq!(lit("a\u{ad}b"), "'a\\xadb'");
    }

    #[test]
    fn line_and_paragraph_separators_escape() {
        assert_eq!(lit("a\u{2028}b"), "'a\\u2028b'");
        assert_eq!(lit("a\u{2029}b"), "'a\\u2029b'");
    }

    #[test]
    fn private_use_escapes() {
        assert_eq!(lit("a\u{e000}b"), "'a\\ue000b'");
    }

    #[test]
    fn astral_non_printable_uses_the_widest_escape() {
        // U+100000 is `Co`, so this is the only shape that reaches `\U`.
        assert_eq!(lit("a\u{100000}b"), "'a\\U00100000b'");
    }

    #[test]
    fn astral_printable_stays_verbatim() {
        // U+E0100 VARIATION SELECTOR-17 is `Mn`, which CPython prints.
        assert_eq!(lit("a\u{e0100}b"), "'a\u{e0100}b'");
    }

    // ── The category table itself ──

    #[test]
    fn each_non_printable_category_is_represented() {
        // Without this, a regression that stopped consulting the categories
        // would leave every ASCII test above passing and silently emit raw
        // non-ASCII. One representative per category the rule names, except
        // `Cs`, which no `char` can be.
        for (c, category) in [
            ('\u{85}', "Cc"),
            ('\u{ad}', "Cf"),
            ('\u{e000}', "Co"),
            ('\u{378}', "Cn"),
            ('\u{2028}', "Zl"),
            ('\u{2029}', "Zp"),
            ('\u{a0}', "Zs"),
        ] {
            assert!(
                is_non_printable(c),
                "U+{:04X} ({category}) must be non-printable",
                c as u32
            );
        }
    }

    #[test]
    fn representative_printable_categories_are_not_escaped() {
        // The negative control for the test above: an over-broad predicate
        // that escaped everything would pass it.
        for (c, category) in [
            ('a', "Ll"),
            ('A', "Lu"),
            ('1', "Nd"),
            (' ', "Zs, the U+0020 exception"),
            ('é', "Ll"),
            ('😀', "So"),
            ('\u{e0100}', "Mn"),
            ('\u{4e00}', "Lo"),
        ] {
            assert!(
                !is_non_printable(c),
                "U+{:04X} ({category}) must be printable",
                c as u32
            );
        }
    }

    #[test]
    fn ascii_fast_path_agrees_with_the_table() {
        // The fast path is an optimisation, so it must return exactly what the
        // generated table would. Skipping this range is only safe while so.
        for cp in 0u32..0x80 {
            let c = char::from_u32(cp).unwrap();
            assert_eq!(
                is_non_printable(c),
                in_table(NONPRINTABLE, c),
                "U+{cp:04X}: fast path disagrees with NONPRINTABLE"
            );
        }
    }

    #[test]
    fn the_pinned_unicode_version_has_not_moved() {
        // The reason this module reads the generated table rather than a
        // Unicode-property crate is that the version is pinned and asserted.
        // Regenerating on a newer Unicode fails here, so the move is a
        // decision rather than a silent drift.
        assert_eq!(
            crate::functions::unicode_tables::UNICODE_VERSION,
            "16.0.0",
            "NONPRINTABLE was regenerated on a different Unicode version; \
             confirm the repr_py expectations still match CPython and update \
             this assertion"
        );
    }

    // ── Escape width ──

    #[test]
    fn escape_width_follows_the_code_point() {
        let cases = [
            ('\u{0}', "\\x00"),
            ('\u{7f}', "\\x7f"),
            ('\u{ff}', "\\xff"),
            ('\u{100}', "\\u0100"),
            ('\u{ffff}', "\\uffff"),
            ('\u{10000}', "\\U00010000"),
            ('\u{10ffff}', "\\U0010ffff"),
        ];
        for (c, want) in cases {
            let mut buf = String::new();
            write_escaped_code_point(c, &mut buf);
            assert_eq!(buf, want, "U+{:04X}", c as u32);
        }
    }
}
