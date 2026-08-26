// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test_strings.py

use openjd_expr::{ExprValue, ParsedExpression, PathFormat, SymbolTable};

#[allow(dead_code)]
fn eval(expr: &str) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap()
}

#[allow(dead_code)]
fn eval_fmt(expr: &str, fmt: PathFormat) -> ExprValue {
    let parsed = ParsedExpression::new(expr).unwrap();
    let st = SymbolTable::new();
    let symtabs = [&st];
    parsed.with_path_format(fmt).evaluate(&symtabs).unwrap()
}

#[allow(dead_code)]
fn eval_err(expr: &str) -> String {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string()
}

fn assert_err(expr: &str, expected: &[&str]) {
    let e = eval_err(expr);
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

fn eval_posix_st(expr: &str, st: &SymbolTable) -> ExprValue {
    let parsed = ParsedExpression::new(expr).unwrap();
    let symtabs = [st];
    parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap()
}

// === TestStrings ===
#[test]
fn concatenation() {
    assert_eq!(
        eval("'hello' + ' ' + 'world'").to_display_string(),
        "hello world"
    );
}
#[test]
fn string_range_expr_concat() {
    assert_eq!(
        eval("'frames: ' + range_expr('1-3')").to_display_string(),
        "frames: 1-3"
    );
}
#[test]
fn range_expr_string_concat() {
    assert_eq!(
        eval("range_expr('1-3') + ' are frames'").to_display_string(),
        "1-3 are frames"
    );
}
#[test]
fn repetition() {
    assert_eq!(eval("'ab' * 3").to_display_string(), "ababab");
}
#[test]
fn upper() {
    assert_eq!(eval("upper('hello')").to_display_string(), "HELLO");
}
#[test]
fn lower() {
    assert_eq!(eval("lower('HELLO')").to_display_string(), "hello");
}
#[test]
fn strip() {
    assert_eq!(eval("strip('  hi  ')").to_display_string(), "hi");
}
#[test]
fn strip_chars() {
    assert_eq!(eval("strip('xxhelloxx', 'x')").to_display_string(), "hello");
}
#[test]
fn strip_dots() {
    assert_eq!(eval("strip('...hi...', '.')").to_display_string(), "hi");
}
#[test]
fn strip_multi() {
    assert_eq!(
        eval("strip('abcHELLOcba', 'abc')").to_display_string(),
        "HELLO"
    );
}
#[test]
fn lstrip_chars() {
    assert_eq!(
        eval("lstrip('xxhelloxx', 'x')").to_display_string(),
        "helloxx"
    );
}
#[test]
fn rstrip_chars() {
    assert_eq!(
        eval("rstrip('xxhelloxx', 'x')").to_display_string(),
        "xxhello"
    );
}
#[test]
fn method_strip() {
    assert_eq!(eval("'xxhelloxx'.strip('x')").to_display_string(), "hello");
}
#[test]
fn method_lstrip() {
    assert_eq!(
        eval("'xxhelloxx'.lstrip('x')").to_display_string(),
        "helloxx"
    );
}
#[test]
fn method_rstrip() {
    assert_eq!(
        eval("'xxhelloxx'.rstrip('x')").to_display_string(),
        "xxhello"
    );
}
#[test]
fn method_upper() {
    assert_eq!(eval("'hello'.upper()").to_display_string(), "HELLO");
}
#[test]
fn startswith() {
    assert_eq!(
        eval("startswith('hello', 'hel')").to_display_string(),
        "true"
    );
}
#[test]
fn endswith() {
    assert_eq!(eval("endswith('hello', 'lo')").to_display_string(), "true");
}

// String classification
#[test]
fn isdigit_true() {
    assert_eq!(eval("'123'.isdigit()").to_display_string(), "true");
}
#[test]
fn isdigit_false() {
    assert_eq!(eval("'12a'.isdigit()").to_display_string(), "false");
}
#[test]
fn isdigit_empty() {
    assert_eq!(eval("''.isdigit()").to_display_string(), "false");
}
#[test]
fn isalpha_true() {
    assert_eq!(eval("'abc'.isalpha()").to_display_string(), "true");
}
#[test]
fn isalpha_false() {
    assert_eq!(eval("'ab3'.isalpha()").to_display_string(), "false");
}
#[test]
fn isalnum_true() {
    assert_eq!(eval("'abc123'.isalnum()").to_display_string(), "true");
}
#[test]
fn isalnum_false() {
    assert_eq!(eval("'abc 123'.isalnum()").to_display_string(), "false");
}
#[test]
fn isupper_true() {
    assert_eq!(eval("'ABC'.isupper()").to_display_string(), "true");
}
#[test]
fn isupper_false() {
    assert_eq!(eval("'ABc'.isupper()").to_display_string(), "false");
}
#[test]
fn islower_true() {
    assert_eq!(eval("'abc'.islower()").to_display_string(), "true");
}
#[test]
fn islower_false() {
    assert_eq!(eval("'aBc'.islower()").to_display_string(), "false");
}
#[test]
fn isascii_true() {
    assert_eq!(eval("'hello'.isascii()").to_display_string(), "true");
}
#[test]
fn isascii_empty() {
    assert_eq!(eval("''.isascii()").to_display_string(), "true");
}

// === Python str-method parity on Unicode input (issue #309) ===
// Expected values in this section are CPython's answers for the same
// str method (verified against CPython 3.14 / Unicode 16.0.0).
#[test]
fn isdigit_arabic_indic_digit() {
    // U+0663 ARABIC-INDIC DIGIT THREE (Nd): Python isdigit is true.
    assert_eq!(eval("isdigit('\u{0663}')").to_display_string(), "true");
}
#[test]
fn isdigit_mixed_ascii_and_arabic_indic() {
    assert_eq!(eval("isdigit('12\u{0663}')").to_display_string(), "true");
}
#[test]
fn isdigit_superscript_two() {
    // U+00B2 (No, Numeric_Type=Digit): Python isdigit is true.
    assert_eq!(eval("isdigit('\u{00b2}')").to_display_string(), "true");
}
#[test]
fn isdigit_vulgar_fraction_false() {
    // U+00BD (No, Numeric_Type=Numeric): Python isdigit is false.
    assert_eq!(eval("isdigit('\u{00bd}')").to_display_string(), "false");
}
#[test]
fn isdigit_roman_numeral_false() {
    // U+216B ROMAN NUMERAL TWELVE (Nl): Python isdigit is false.
    assert_eq!(eval("isdigit('\u{216b}')").to_display_string(), "false");
}
#[test]
fn isalpha_roman_numeral_false() {
    // Nl is not L*: Python isalpha is false (Rust is_alphabetic says true).
    assert_eq!(eval("isalpha('\u{216b}')").to_display_string(), "false");
}
#[test]
fn isalpha_combining_mark_false() {
    // U+0345 COMBINING GREEK YPOGEGRAMMENI (Mn, Other_Alphabetic):
    // Python isalpha is false (Rust is_alphabetic says true).
    assert_eq!(eval("isalpha('\u{0345}')").to_display_string(), "false");
}
#[test]
fn isalpha_circled_letter_false() {
    // U+24B6 CIRCLED LATIN CAPITAL LETTER A (So, Other_Alphabetic).
    assert_eq!(eval("isalpha('\u{24b6}')").to_display_string(), "false");
}
#[test]
fn isalpha_cjk_and_accented_true() {
    assert_eq!(
        eval("isalpha('\u{4e94}\u{00f1}\u{02b0}')").to_display_string(),
        "true"
    );
}
#[test]
fn isalnum_arabic_indic_digit() {
    assert_eq!(eval("isalnum('\u{0663}')").to_display_string(), "true");
}
#[test]
fn isalnum_vulgar_fraction_true() {
    // Python isalnum includes isnumeric: true for U+00BD even though
    // isdigit and isalpha are both false.
    assert_eq!(eval("isalnum('\u{00bd}')").to_display_string(), "true");
}
#[test]
fn isalnum_circled_letter_false() {
    // Not L* and no numeric value: Python isalnum is false.
    assert_eq!(eval("isalnum('\u{24b6}')").to_display_string(), "false");
}
#[test]
fn isdigit_isalnum_consistent_for_decimal_digits() {
    // The reported inconsistency: a decimal digit must be both.
    assert_eq!(eval("isdigit('\u{0663}')").to_display_string(), "true");
    assert_eq!(eval("isalnum('\u{0663}')").to_display_string(), "true");
}
#[test]
fn isspace_information_separators() {
    // U+001C..U+001F are isspace in Python (not Unicode White_Space).
    assert_eq!(
        eval("isspace('\u{001c}\u{001d}\u{001e}\u{001f}')").to_display_string(),
        "true"
    );
}
#[test]
fn isspace_no_break_space() {
    assert_eq!(eval("isspace('\u{00a0}')").to_display_string(), "true");
}
#[test]
fn strip_information_separator() {
    // str.strip() uses the same Py_UNICODE_ISSPACE set as isspace,
    // including U+001C..U+001F — not Unicode White_Space, so Rust's
    // str::trim misses them.
    assert_eq!(
        eval("strip('a\\x1cb\\x1c')").to_display_string(),
        "a\u{001c}b"
    );
}
#[test]
fn lstrip_rstrip_information_separator() {
    assert_eq!(
        eval("lstrip('\\x1ca\\x1c')").to_display_string(),
        "a\u{001c}"
    );
    assert_eq!(
        eval("rstrip('\\x1ca\\x1c')").to_display_string(),
        "\u{001c}a"
    );
}
#[test]
fn strip_no_break_space() {
    // White_Space members keep working through the SPACE table.
    assert_eq!(eval("strip('\\xa0a\\xa0')").to_display_string(), "a");
}
#[test]
fn split_information_separator() {
    // No-separator split()/rsplit() split on Py_UNICODE_ISSPACE runs too:
    // 'a\x1cb'.split() == ['a', 'b'] in Python.
    assert_eq!(
        eval("split('a\\x1cb')").to_display_string(),
        r#"["a", "b"]"#
    );
    assert_eq!(
        eval("rsplit('a\\x1db')").to_display_string(),
        r#"["a", "b"]"#
    );
}
#[test]
fn split_mixed_whitespace_runs() {
    // Runs of mixed whitespace collapse and leading/trailing runs drop,
    // like str.split(): ' a\x1c b\x1c'.split() == ['a', 'b'].
    assert_eq!(
        eval("split(' a\\x1c b\\x1c')").to_display_string(),
        r#"["a", "b"]"#
    );
}
#[test]
fn int_trims_white_space_but_not_information_separators() {
    // CPython's int()/float() trim Unicode White_Space around the number
    // (NBSP works) but reject U+001C..U+001F — a narrower set than
    // str.strip(). Rust's str::trim is exactly White_Space, so the
    // conversion functions are CPython-exact as-is.
    assert_eq!(eval("int('\\xa05\\xa0')").to_display_string(), "5");
}
#[test]
fn islower_ignores_uncased_letters() {
    // Python islower ignores uncased characters (CJK): 'a五' is lowercase.
    assert_eq!(eval("islower('a\u{4e94}')").to_display_string(), "true");
}
#[test]
fn isupper_ignores_uncased_letters() {
    assert_eq!(eval("isupper('A\u{4e94}')").to_display_string(), "true");
}
#[test]
fn isupper_islower_uncased_only_false() {
    // No cased characters at all: both false in Python.
    assert_eq!(eval("isupper('\u{4e94}123')").to_display_string(), "false");
    assert_eq!(eval("islower('\u{4e94}123')").to_display_string(), "false");
}
#[test]
fn titlecase_char_neither_upper_nor_lower() {
    // U+01C5 LATIN CAPITAL LETTER D WITH SMALL LETTER Z WITH CARON (Lt)
    // is cased but neither uppercase nor lowercase in Python.
    assert_eq!(eval("isupper('\u{01c5}')").to_display_string(), "false");
    assert_eq!(eval("islower('\u{01c5}')").to_display_string(), "false");
}
#[test]
fn islower_pharyngeal_fricative() {
    // U+0295 (Ll): Python islower is true; Rust std disagreed pre-fix.
    assert_eq!(eval("islower('\u{0295}')").to_display_string(), "true");
}

// === int()/float() accept Unicode decimal digits (Nd), like CPython ===
// CPython's int() and float() replace Numeric_Type=Decimal characters with
// their ASCII values before parsing, so isdigit(x) passing implies int(x)
// succeeds for Nd digits. Expected values are CPython's answers (verified
// against CPython 3.14 / Unicode 16.0.0).
#[test]
fn int_arabic_indic_digit() {
    // int('٣') == 3
    assert_eq!(eval("int('\u{0663}')").to_display_string(), "3");
}
#[test]
fn int_arabic_indic_digits_multi() {
    // int('١٢٣') == 123
    assert_eq!(
        eval("int('\u{0661}\u{0662}\u{0663}')").to_display_string(),
        "123"
    );
}
#[test]
fn int_mixed_ascii_and_arabic_indic() {
    // CPython transforms per-character, so mixed scripts parse: int('12٣') == 123.
    assert_eq!(eval("int('12\u{0663}')").to_display_string(), "123");
}
#[test]
fn int_negative_arabic_indic() {
    assert_eq!(eval("int('-\u{0663}')").to_display_string(), "-3");
}
#[test]
fn int_devanagari_and_fullwidth_digits() {
    // U+0967 DEVANAGARI DIGIT ONE, U+FF17 FULLWIDTH DIGIT SEVEN.
    assert_eq!(eval("int('\u{0967}\u{ff17}')").to_display_string(), "17");
}
#[test]
fn int_isdigit_guard_pattern_nd_digit() {
    // The guard pattern from templates: passing isdigit implies int succeeds
    // for Nd digits (matching the CPython pair of semantics).
    assert_eq!(
        eval("int('\u{0663}') if isdigit('\u{0663}') else 0").to_display_string(),
        "3"
    );
}
#[test]
fn float_arabic_indic_digits() {
    // float('٣.٥') == 3.5 — the '.' is ASCII; Nd digits normalize around it.
    assert_eq!(
        eval("float('\u{0663}.\u{0665}')").to_display_string(),
        "3.5"
    );
    assert_eq!(eval("float('\u{0662}')").to_display_string(), "2.0");
}

// === Python str.title / str.capitalize parity ===
// Expected values are CPython's answers (verified against CPython 3.14 /
// Unicode 16.0.0).
#[test]
fn title_digit_starts_new_word() {
    // Word boundaries are uncased characters, so digits restart a word.
    assert_eq!(eval("title('1st')").to_display_string(), "1St");
    assert_eq!(eval("title('ab2cd')").to_display_string(), "Ab2Cd");
}
#[test]
fn title_apostrophe_starts_new_word() {
    assert_eq!(eval("title(\"ab'cd\")").to_display_string(), "Ab'Cd");
}
#[test]
fn title_ascii_words() {
    assert_eq!(
        eval("title('hello world')").to_display_string(),
        "Hello World"
    );
}
#[test]
fn title_uses_titlecase_mapping() {
    // U+01C6 dz-digraph titlecases to U+01C5, not uppercase U+01C4.
    assert_eq!(
        eval("title('\u{01c6}ab')").to_display_string(),
        "\u{01c5}ab"
    );
}
#[test]
fn title_sharp_s_expands_at_word_start_only() {
    // 'ssß'.title() == 'Ssß': ß mid-word lowercases to itself.
    assert_eq!(
        eval("title('ss\u{00df}')").to_display_string(),
        "Ss\u{00df}"
    );
}
#[test]
fn title_uncased_letters_start_words() {
    // CJK ideographs are uncased, so they end the current word.
    assert_eq!(
        eval("title('a\u{4e94}b')").to_display_string(),
        "A\u{4e94}B"
    );
}
#[test]
fn title_final_sigma() {
    // 'OΣ K'.title() == 'Oς K': sigma is word-final, needs context.
    assert_eq!(
        eval("title('O\u{03a3} K')").to_display_string(),
        "O\u{03c2} K"
    );
}
#[test]
fn title_sigma_not_final_before_cased() {
    // 'ΑΣB'.title() == 'Ασb': cased char follows, ordinary small sigma.
    assert_eq!(
        eval("title('\u{0391}\u{03a3}B')").to_display_string(),
        "\u{0391}\u{03c3}b"
    );
}
#[test]
fn capitalize_uses_titlecase_mapping() {
    // Python >= 3.8 titlecases the first character: ǆab -> ǅab.
    assert_eq!(
        eval("capitalize('\u{01c6}ab')").to_display_string(),
        "\u{01c5}ab"
    );
}
#[test]
fn capitalize_sharp_s_expands() {
    assert_eq!(eval("capitalize('\u{00df}x')").to_display_string(), "Ssx");
}
#[test]
fn capitalize_ligature_expands() {
    // U+FB02 LATIN SMALL LIGATURE FL: 'ﬂoor'.capitalize() == 'Floor'.
    assert_eq!(
        eval("capitalize('\u{fb02}oor')").to_display_string(),
        "Floor"
    );
}
#[test]
fn capitalize_uncased_first_char() {
    assert_eq!(eval("capitalize('1st')").to_display_string(), "1st");
}
#[test]
fn capitalize_lowercases_rest() {
    assert_eq!(eval("capitalize('ABC')").to_display_string(), "Abc");
}
#[test]
fn capitalize_final_sigma_in_rest() {
    // 'OΣ K'.capitalize() == 'Oς k'.
    assert_eq!(
        eval("capitalize('O\u{03a3} K')").to_display_string(),
        "O\u{03c2} k"
    );
}
#[test]
fn capitalize_empty() {
    assert_eq!(eval("capitalize('')").to_display_string(), "");
}

#[test]
fn replace() {
    assert_eq!(
        eval("replace('hello', 'l', 'L')").to_display_string(),
        "heLLo"
    );
}
#[test]
fn split_method() {
    assert_eq!(
        eval("'one  two'.split()").to_display_string(),
        r#"["one", "two"]"#
    );
}

#[test]
fn zfill_string() {
    assert_eq!(eval("zfill('42', 5)").to_display_string(), "00042");
}
#[test]
fn zfill_int() {
    assert_eq!(eval("zfill(42, 5)").to_display_string(), "00042");
}
#[test]
fn zfill_float() {
    assert_eq!(eval("zfill(3.14, 8)").to_display_string(), "00003.14");
}
#[test]
fn zfill_float_neg() {
    assert_eq!(eval("zfill(-2.5, 8)").to_display_string(), "-00002.5");
}
#[test]
fn zfill_method() {
    assert_eq!(eval("(42).zfill(5)").to_display_string(), "00042");
}

#[test]
fn len_string() {
    assert_eq!(eval("len('hello')").to_display_string(), "5");
}
#[test]
fn find_found() {
    assert_eq!(eval("find('hello', 'ell')").to_display_string(), "1");
}
#[test]
fn find_not_found() {
    assert_eq!(eval("find('hello', 'xyz')").to_display_string(), "-1");
}
#[test]
fn find_method() {
    assert_eq!(eval("'hello'.find('lo')").to_display_string(), "3");
}
#[test]
fn rfind_found() {
    assert_eq!(
        eval("rfind('hello hello', 'hello')").to_display_string(),
        "6"
    );
}
#[test]
fn rfind_not_found() {
    assert_eq!(eval("rfind('hello', 'xyz')").to_display_string(), "-1");
}
#[test]
fn index_found() {
    assert_eq!(eval("index('hello', 'ell')").to_display_string(), "1");
}
#[test]
fn index_not_found() {
    assert_err(
        "index('hello', 'xyz')",
        &[
            "index failed: substring 'xyz' not found\n",
            "  index('hello', 'xyz')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn rindex_found() {
    assert_eq!(
        eval("rindex('hello hello', 'hello')").to_display_string(),
        "6"
    );
}
#[test]
fn rindex_not_found() {
    assert_err(
        "rindex('hello', 'xyz')",
        &[
            "rindex failed: substring 'xyz' not found\n",
            "  rindex('hello', 'xyz')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn count_str() {
    assert_eq!(eval("count('hello', 'l')").to_display_string(), "2");
}
#[test]
fn find_empty_err() {
    assert_err(
        "find('hello', '')",
        &[
            "find failed: empty substring\n",
            "  find('hello', '')\n",
            "  ^~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn rfind_empty_err() {
    assert_err(
        "rfind('hello', '')",
        &[
            "rfind failed: empty substring\n",
            "  rfind('hello', '')\n",
            "  ^~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn index_empty_err() {
    assert_err(
        "index('hello', '')",
        &[
            "index failed: empty substring\n",
            "  index('hello', '')\n",
            "  ^~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn rindex_empty_err() {
    assert_err(
        "rindex('hello', '')",
        &[
            "rindex failed: empty substring\n",
            "  rindex('hello', '')\n",
            "  ^~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn count_empty_err() {
    assert_err(
        "count('hello', '')",
        &[
            "count failed: empty substring\n",
            "  count('hello', '')\n",
            "  ^~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn replace_empty_old_err() {
    assert_err(
        "'hello'.replace('', 'x')",
        &[
            "replace failed: empty old string\n",
            "  'hello'.replace('', 'x')\n",
            "  ~~~~~~~~^~~~~~~~~~~~~~~~",
        ],
    );
}

// === TestRemovePrefixSuffix ===
#[test]
fn removeprefix_present() {
    assert_eq!(
        eval("removeprefix('hello world', 'hello ')").to_display_string(),
        "world"
    );
}
#[test]
fn removeprefix_absent() {
    assert_eq!(
        eval("removeprefix('hello world', 'bye ')").to_display_string(),
        "hello world"
    );
}
#[test]
fn removeprefix_empty() {
    assert_eq!(
        eval("removeprefix('hello', '')").to_display_string(),
        "hello"
    );
}
#[test]
fn removeprefix_full() {
    assert_eq!(
        eval("removeprefix('hello', 'hello')").to_display_string(),
        ""
    );
}
#[test]
fn removeprefix_method() {
    assert_eq!(
        eval("'hello world'.removeprefix('hello ')").to_display_string(),
        "world"
    );
}
#[test]
fn removesuffix_present() {
    assert_eq!(
        eval("removesuffix('hello.txt', '.txt')").to_display_string(),
        "hello"
    );
}
#[test]
fn removesuffix_absent() {
    assert_eq!(
        eval("removesuffix('hello.txt', '.py')").to_display_string(),
        "hello.txt"
    );
}
#[test]
fn removesuffix_empty() {
    assert_eq!(
        eval("removesuffix('hello', '')").to_display_string(),
        "hello"
    );
}
#[test]
fn removesuffix_full() {
    assert_eq!(
        eval("removesuffix('hello', 'hello')").to_display_string(),
        ""
    );
}
#[test]
fn removesuffix_method() {
    assert_eq!(
        eval("'hello.txt'.removesuffix('.txt')").to_display_string(),
        "hello"
    );
}
#[test]
fn removesuffix_compound() {
    assert_eq!(
        eval("'archive.tar.gz'.removesuffix('.tar.gz')").to_display_string(),
        "archive"
    );
}

// === TestStringMembership ===
#[test]
fn substring_in() {
    assert_eq!(eval("\"ell\" in \"hello\"").to_display_string(), "true");
}
#[test]
fn substring_not_in() {
    assert_eq!(eval("\"xyz\" in \"hello\"").to_display_string(), "false");
}
#[test]
fn substring_not_in_op() {
    assert_eq!(eval("\"xyz\" not in \"hello\"").to_display_string(), "true");
}
#[test]
fn empty_in_string() {
    assert_eq!(eval("\"\" in \"hello\"").to_display_string(), "true");
}

// === TestReprFunctions ===
#[test]
fn repr_py_string() {
    assert_eq!(eval("repr_py('hello')").to_display_string(), "'hello'");
}
#[test]
fn repr_py_int() {
    assert_eq!(eval("repr_py(42)").to_display_string(), "42");
}
#[test]
fn repr_json_string() {
    assert_eq!(eval("repr_json('hello')").to_display_string(), "\"hello\"");
}
#[test]
fn repr_json_int() {
    assert_eq!(eval("repr_json(42)").to_display_string(), "42");
}
#[test]
fn repr_json_bool() {
    assert_eq!(eval("repr_json(true)").to_display_string(), "true");
}
#[test]
fn repr_json_null_value() {
    assert_eq!(eval("repr_json(null)").to_display_string(), "null");
}

// === TestStringLiteralFormats ===
#[test]
fn single_quote() {
    assert_eq!(eval("'hello'").to_display_string(), "hello");
}
#[test]
fn double_quote() {
    assert_eq!(eval("\"hello\"").to_display_string(), "hello");
}
#[test]
fn triple_single() {
    assert_eq!(eval("'''hello'''").to_display_string(), "hello");
}
#[test]
fn triple_double() {
    assert_eq!(eval("\"\"\"hello\"\"\"").to_display_string(), "hello");
}
#[test]
fn escape_newline() {
    assert_eq!(eval("'hello\\nworld'").to_display_string(), "hello\nworld");
}
#[test]
fn escape_tab() {
    assert_eq!(eval("'hello\\tworld'").to_display_string(), "hello\tworld");
}
#[test]
fn escape_backslash() {
    assert_eq!(eval("'hello\\\\world'").to_display_string(), "hello\\world");
}
#[test]
fn raw_string() {
    assert_eq!(
        eval("r'hello\\nworld'").to_display_string(),
        "hello\\nworld"
    );
}
#[test]
fn empty_string() {
    assert_eq!(eval("''").to_display_string(), "");
}

// === TestRejectedStringFormats ===
#[test]
fn fstring_rejected() {
    assert_err(
        "f'hello'",
        &[
            "f-strings are not supported; use string concatenation\n",
            "  f'hello'\n",
            "  ^~~~~~~~",
        ],
    );
}
#[test]
fn bytes_rejected() {
    assert_err(
        "b'hello'",
        &[
            "Byte strings (b'...') are not supported. Use '...' or \"...\" instead.\n",
            "  b'hello'\n",
            "  ^~~~~~~~",
        ],
    );
}

// === TestRegexWithRawStrings ===
#[test]
fn re_search_no_match() {
    assert!(matches!(
        eval(r"re_search('hello', r'\d+')"),
        ExprValue::Null
    ));
}
#[test]
fn re_match_at_start() {
    assert!(eval(r"re_match('hello', r'hel')").is_list());
}
#[test]
fn re_match_not_at_start() {
    assert!(matches!(
        eval(r"re_match('hello', r'llo')"),
        ExprValue::Null
    ));
}
// re_replace is not in the spec — use re_sub instead

// === TestReprCmdComprehensive ===
#[test]
fn repr_cmd_simple() {
    assert_eq!(eval("repr_cmd('hello')").to_display_string(), "hello");
}
#[test]
fn repr_cmd_space() {
    assert_eq!(
        eval("repr_cmd('hello world')").to_display_string(),
        "\"hello world\""
    );
}

// === TestReprPwshComprehensive ===
#[test]
fn repr_pwsh_simple() {
    assert_eq!(eval("repr_pwsh('hello')").to_display_string(), "'hello'");
}
#[test]
fn repr_pwsh_space() {
    assert_eq!(
        eval("repr_pwsh('hello world')").to_display_string(),
        "'hello world'"
    );
}

// === TestReprShComprehensive ===
#[test]
fn repr_sh_simple() {
    assert_eq!(eval("repr_sh('hello')").to_display_string(), "hello");
}
#[test]
fn repr_sh_space() {
    assert_eq!(
        eval("repr_sh('hello world')").to_display_string(),
        "'hello world'"
    );
}

// === Additional TestStringMembership ===
#[test]
fn substring_at_start() {
    assert_eq!(eval("\"hel\" in \"hello\"").to_display_string(), "true");
}
#[test]
fn substring_at_end() {
    assert_eq!(eval("\"llo\" in \"hello\"").to_display_string(), "true");
}
#[test]
fn full_string_match() {
    assert_eq!(eval("\"hello\" in \"hello\"").to_display_string(), "true");
}
#[test]
fn string_in_empty() {
    assert_eq!(eval("\"x\" in \"\"").to_display_string(), "false");
}
#[test]
fn empty_in_empty() {
    assert_eq!(eval("\"\" in \"\"").to_display_string(), "true");
}
#[test]
fn not_in_true() {
    assert_eq!(eval("\"xyz\" not in \"hello\"").to_display_string(), "true");
}
#[test]
fn not_in_false() {
    assert_eq!(
        eval("\"ell\" not in \"hello\"").to_display_string(),
        "false"
    );
}
#[test]
fn case_sensitive() {
    assert_eq!(eval("\"Hello\" in \"hello\"").to_display_string(), "false");
}
#[test]
fn with_spaces() {
    assert_eq!(eval("\" \" in \"hello world\"").to_display_string(), "true");
}

// === Additional TestStringLiteralFormats ===
#[test]
fn triple_multiline() {
    assert_eq!(
        eval("'''line1\nline2'''").to_display_string(),
        "line1\nline2"
    );
}
#[test]
fn escape_single_quote() {
    assert_eq!(eval("'it\\'s'").to_display_string(), "it's");
}
#[test]
fn escape_double_quote() {
    assert_eq!(eval("\"say \\\"hi\\\"\"").to_display_string(), "say \"hi\"");
}
#[test]
fn escape_hex() {
    assert_eq!(eval("'\\x41'").to_display_string(), "A");
}
#[test]
fn raw_string_upper_r() {
    assert_eq!(eval("R'\\n'").to_display_string(), "\\n");
}
#[test]
fn raw_string_double() {
    assert_eq!(eval("r\"\\n\"").to_display_string(), "\\n");
}
#[test]
fn unicode_chars() {
    assert_eq!(eval("'café'").to_display_string(), "café");
}

// === Additional TestRegexWithRawStrings ===
#[test]
fn re_search_with_group() {
    let r = eval("re_search('hello123', r'(\\d+)')");
    assert!(r.is_list());
}
#[test]
fn re_search_no_groups() {
    let r = eval("re_search('hello123', r'\\d+')");
    assert!(r.is_list());
}
#[test]
fn re_match_with_groups() {
    let r = eval("re_match('v042_final', r'v(\\d+)')");
    assert!(r.is_list());
}
#[test]
fn re_findall_multiple() {
    let r = eval("re_findall('a1b2c3', r'\\d+')");
    assert!(r.is_list());
}
#[test]
fn re_findall_no_matches() {
    let r = eval("re_findall('hello', r'\\d+')");
    assert!(r.is_list());
    assert_eq!(r.list_len(), Some(0));
}
#[test]
fn re_sub_digits() {
    assert_eq!(
        eval("re_sub('a1b2c3', r'\\d', 'X')").to_display_string(),
        "aXbXcX"
    );
}
#[test]
fn re_sub_whitespace() {
    assert_eq!(
        eval("re_sub('a b  c', r'\\s+', '-')").to_display_string(),
        "a-b-c"
    );
}
#[test]
fn re_sub_group_ref_backslash() {
    assert_err(
        "re_sub('hello', '(h)', r'\\1')",
        &[
            "Group references in replacement strings are not supported\n",
            "  re_sub('hello', '(h)', r'\\1')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn re_sub_group_ref_dollar() {
    assert_err(
        "re_sub('hello', '(h)', '$1')",
        &[
            "Group references in replacement strings are not supported\n",
            "  re_sub('hello', '(h)', '$1')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn re_search_empty_pattern() {
    assert_err(
        "re_search('hello', '')",
        &[
            "Empty regex pattern is not allowed\n",
            "  re_search('hello', '')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn re_match_empty_pattern() {
    assert_err(
        "re_match('hello', '')",
        &[
            "Empty regex pattern is not allowed\n",
            "  re_match('hello', '')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn re_findall_empty_pattern() {
    assert_err(
        "re_findall('hello', '')",
        &[
            "Empty regex pattern is not allowed\n",
            "  re_findall('hello', '')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn re_sub_empty_pattern() {
    assert_err(
        "re_sub('hello', '', 'x')",
        &[
            "Empty regex pattern is not allowed\n",
            "  re_sub('hello', '', 'x')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn re_escape_metacharacters() {
    assert_eq!(eval("re_escape('a.b*c')").to_display_string(), "a\\.b\\*c");
}
#[test]
fn re_split_digits() {
    assert!(eval("re_split('a1b2c3', r'\\d')").is_list());
}
#[test]
fn re_split_whitespace() {
    assert!(eval("re_split('a b  c', r'\\s+')").is_list());
}
#[test]
fn re_split_maxsplit() {
    assert!(eval("re_split('a1b2c3', r'\\d', 1)").is_list());
}
#[test]
fn re_split_no_match() {
    assert!(eval("re_split('hello', r'\\d')").is_list());
}
#[test]
fn re_split_invalid_pattern() {
    assert_err(
        "re_split('hello', '[')",
        &["Invalid regex pattern: regex parse error"],
    );
}
#[test]
fn re_split_empty_pattern() {
    assert_err(
        "re_split('hello', '')",
        &[
            "Empty regex pattern is not allowed\n",
            "  re_split('hello', '')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

// === TestRegexReturnValues ===

// re_match: returns [full_match, group1, ...] or null
#[test]
fn re_match_value_no_groups() {
    assert_eq!(
        eval("re_match('hello123', r'hello')").to_display_string(),
        "[\"hello\"]"
    );
}
#[test]
fn re_match_value_one_group() {
    assert_eq!(
        eval("re_match('v042_final', r'v(\\d+)')").to_display_string(),
        "[\"v042\", \"042\"]"
    );
}
#[test]
fn re_match_value_multi_groups() {
    assert_eq!(
        eval("re_match('v1.2.3', r'v(\\d+)\\.(\\d+)\\.(\\d+)')").to_display_string(),
        "[\"v1.2.3\", \"1\", \"2\", \"3\"]"
    );
}
#[test]
fn re_match_null_on_no_match() {
    assert!(matches!(
        eval("re_match('hello', r'\\d+')"),
        ExprValue::Null
    ));
}

// re_search: returns [full_match, group1, ...] or null
#[test]
fn re_search_value_no_groups() {
    assert_eq!(
        eval("re_search('hello123', r'\\d+')").to_display_string(),
        "[\"123\"]"
    );
}
#[test]
fn re_search_value_one_group() {
    assert_eq!(
        eval("re_search('hello123', r'(\\d+)')").to_display_string(),
        "[\"123\", \"123\"]"
    );
}
#[test]
fn re_search_value_multi_groups() {
    assert_eq!(
        eval("re_search('2024-01-15', r'(\\d{4})-(\\d{2})-(\\d{2})')").to_display_string(),
        "[\"2024-01-15\", \"2024\", \"01\", \"15\"]"
    );
}
#[test]
fn re_search_null_on_no_match() {
    assert!(matches!(
        eval("re_search('hello', r'\\d+')"),
        ExprValue::Null
    ));
}

// re_findall: no groups -> list[string], one group -> list[string], multi groups -> list[list[string]]
#[test]
fn re_findall_no_groups_values() {
    assert_eq!(
        eval("re_findall('a1b2c3', r'\\d+')").to_display_string(),
        "[\"1\", \"2\", \"3\"]"
    );
}
#[test]
fn re_findall_one_group_values() {
    assert_eq!(
        eval("re_findall('a1b2c3', r'(\\d+)')").to_display_string(),
        "[\"1\", \"2\", \"3\"]"
    );
}
#[test]
fn re_findall_multi_groups_values() {
    let r = eval("re_findall('v1.2 and v3.4', r'v(\\d+)\\.(\\d+)')");
    assert_eq!(r.to_display_string(), "[[\"1\", \"2\"], [\"3\", \"4\"]]");
}
#[test]
fn re_findall_no_match_empty_list() {
    assert_eq!(eval("re_findall('hello', r'\\d+')").list_len(), Some(0));
}

// re_sub: replaces all, returns unchanged on no match
#[test]
fn re_sub_no_match_unchanged() {
    assert_eq!(
        eval("re_sub('hello', r'\\d+', 'X')").to_display_string(),
        "hello"
    );
}

// re_split: actual values
#[test]
fn re_split_values() {
    assert_eq!(
        eval("re_split('a1b2c3', r'\\d')").to_display_string(),
        "[\"a\", \"b\", \"c\", \"\"]"
    );
}
#[test]
fn re_split_maxsplit_values() {
    assert_eq!(
        eval("re_split('a1b2c3', r'\\d', 1)").to_display_string(),
        "[\"a\", \"b2c3\"]"
    );
}
#[test]
fn re_split_no_match_single_element() {
    assert_eq!(
        eval("re_split('hello', r'\\d')").to_display_string(),
        "[\"hello\"]"
    );
}

// === TestRegexUnsupportedFeatures ===
#[test]
fn backreference_rejected() {
    // Cover \1–\9: all must produce the friendly "backreferences" error
    // rather than falling through to the underlying regex crate's message.
    for n in 1..=9 {
        let groups: String = (0..n).map(|_| "(a)").collect();
        let expr = format!("re_search('a', r'{groups}\\{n}')");
        assert_err(&expr, &["Unsupported regex feature: backreferences\n"]);
    }
}
#[test]
fn lookahead_rejected() {
    assert_err(
        "re_search('hello', r'h(?=e)')",
        &[
            "Unsupported regex feature: lookahead\n",
            "  re_search('hello', r'h(?=e)')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn negative_lookahead_rejected() {
    assert_err(
        "re_search('hello', r'h(?!x)')",
        &[
            "Unsupported regex feature: negative lookahead\n",
            "  re_search('hello', r'h(?!x)')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn lookbehind_rejected() {
    assert_err(
        "re_search('hello', r'(?<=h)e')",
        &[
            "Unsupported regex feature: lookbehind\n",
            "  re_search('hello', r'(?<=h)e')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn negative_lookbehind_rejected() {
    assert_err(
        "re_search('hello', r'(?<!x)e')",
        &[
            "Unsupported regex feature: negative lookbehind\n",
            "  re_search('hello', r'(?<!x)e')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn end_of_string_z_rejected() {
    assert_err(
        "re_search('hello', r'o\\Z')",
        &[
            "Unsupported regex feature: end-of-string anchor \\Z\n",
            "  re_search('hello', r'o\\Z')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

// === Constructs outside the Python/Rust intersection (issue #310) ===
#[test]
fn unicode_property_class_rejected() {
    assert_err(
        "re_search('3', r'\\p{Nd}')",
        &[
            "Unsupported regex feature: Unicode property class \\p{...}\n",
            "  re_search('3', r'\\p{Nd}')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn unicode_property_class_inside_class_rejected() {
    assert_err(
        "re_search('a', r'[\\p{L}]')",
        &[
            "Unsupported regex feature: Unicode property class \\p{...}\n",
            "  re_search('a', r'[\\p{L}]')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn rust_capture_group_name_syntax_rejected() {
    assert_err(
        "re_search('ab', r'(?<n>a)b')",
        &[
            "Unsupported regex feature: (?<name>...) capture group; use (?P<name>...)\n",
            "  re_search('ab', r'(?<n>a)b')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn python_capture_group_name_syntax_accepted() {
    assert_eq!(
        eval("re_search('ab', r'(?P<n>a)b')").to_display_string(),
        "[\"ab\", \"a\"]"
    );
}
#[test]
fn posix_character_class_rejected() {
    assert_err(
        "re_search('a', r'[[:alpha:]]')",
        &[
            "Unsupported regex feature: POSIX character class [[:alpha:]]\n",
            "  re_search('a', r'[[:alpha:]]')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn negated_posix_character_class_rejected() {
    assert_err(
        "re_search('5', r'[[:^digit:]]')",
        &[
            "Unsupported regex feature: POSIX character class [[:^digit:]]\n",
            "  re_search('5', r'[[:^digit:]]')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn class_set_difference_rejected() {
    assert_err(
        "re_search('b', r'[a-z--[aeiou]]')",
        &[
            "Unsupported regex feature: character class difference --\n",
            "  re_search('b', r'[a-z--[aeiou]]')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn class_set_intersection_rejected() {
    assert_err(
        "re_search('e', r'[a-z&&[aeiou]]')",
        &[
            "Unsupported regex feature: character class intersection &&\n",
            "  re_search('e', r'[a-z&&[aeiou]]')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn class_set_symmetric_difference_rejected() {
    assert_err(
        "re_search('e', r'[a-z~~[aeiou]]')",
        &[
            "Unsupported regex feature: character class symmetric difference ~~\n",
            "  re_search('e', r'[a-z~~[aeiou]]')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn nested_character_class_rejected() {
    assert_err(
        "re_search('b', r'[a[bc]]')",
        &[
            "Unsupported regex feature: nested character class\n",
            "  re_search('b', r'[a[bc]]')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn swap_greed_inline_flag_rejected() {
    assert_err(
        "re_search('aaa', r'(?U)a+')",
        &[
            "Unsupported regex feature: inline flag U (swap greed)\n",
            "  re_search('aaa', r'(?U)a+')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn crlf_inline_flag_rejected() {
    assert_err(
        "re_search('a', r'(?R)^a')",
        &[
            "Unsupported regex feature: inline flag R (CRLF mode)\n",
            "  re_search('a', r'(?R)^a')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn scoped_swap_greed_flag_rejected() {
    assert_err(
        "re_search('aaa', r'(?U:a+)')",
        &[
            "Unsupported regex feature: inline flag U (swap greed)\n",
            "  re_search('aaa', r'(?U:a+)')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn global_flag_negation_rejected() {
    assert_err(
        "re_search('A', r'(?-i)a')",
        &[
            "Unsupported regex feature: global flag negation (?-...); use a scoped group (?-i:...)\n",
            "  re_search('A', r'(?-i)a')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn negated_unicode_flag_rejected() {
    assert_err(
        "re_search('a', r'(?-u:a)')",
        &[
            "Unsupported regex feature: negated Unicode flag -u\n",
            "  re_search('a', r'(?-u:a)')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn shared_inline_flags_accepted() {
    // `i`, `m`, `s`, `x` (and positive `u`) are in the Python/Rust
    // intersection, bare and scoped, including scoped negation.
    assert!(eval("re_search('HELLO', r'(?i)hello')").is_list());
    assert!(eval("re_search('a\\nb', r'(?m)^b')").is_list());
    assert!(eval("re_search('a\\nb', r'(?s)a.b')").is_list());
    assert!(eval("re_search('ab', r'(?x)a b')").is_list());
    assert!(eval("re_search('a', r'(?u)\\w')").is_list());
    assert!(eval("re_search('HELLO', r'(?i:hello)')").is_list());
    assert!(eval("re_search('hello', r'(?-i:hello)')").is_list());
}
#[test]
fn word_boundary_start_rejected() {
    assert_err(
        "re_search('ab', r'\\b{start}a')",
        &[
            "Unsupported regex feature: word boundary assertion \\b{start}\n",
            "  re_search('ab', r'\\b{start}a')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn word_boundary_end_rejected() {
    assert_err(
        "re_search('ab', r'a\\b{end}')",
        &[
            "Unsupported regex feature: word boundary assertion \\b{end}\n",
            "  re_search('ab', r'a\\b{end}')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn word_boundary_start_angle_rejected() {
    assert_err(
        "re_search('ab', r'\\<a')",
        &[
            "Unsupported regex feature: word boundary assertion \\<\n",
            "  re_search('ab', r'\\<a')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn word_boundary_end_angle_rejected() {
    assert_err(
        "re_search('ab', r'a\\>')",
        &[
            "Unsupported regex feature: word boundary assertion \\>\n",
            "  re_search('ab', r'a\\>')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn word_boundary_start_half_rejected() {
    assert_err(
        "re_search('ab', r'\\b{start-half}a')",
        &[
            "Unsupported regex feature: word boundary assertion \\b{start-half}\n",
            "  re_search('ab', r'\\b{start-half}a')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn word_boundary_end_half_rejected() {
    assert_err(
        "re_search('ab', r'a\\b{end-half}')",
        &[
            "Unsupported regex feature: word boundary assertion \\b{end-half}\n",
            "  re_search('ab', r'a\\b{end-half}')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn portable_word_boundaries_accepted() {
    assert!(eval("re_search('hello world', r'\\bworld\\b')").is_list());
    assert!(eval("re_search('hello', r'l\\Bl')").is_list());
}
#[test]
fn verbose_mode_class_whitespace_rejected() {
    assert_err(
        "re_search('a', r'(?x)[a b]')",
        &[
            "Unsupported regex feature: verbose mode (?x) with whitespace or '#' in a character class; Python treats them as literals\n",
            "  re_search('a', r'(?x)[a b]')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn scoped_verbose_mode_class_whitespace_rejected() {
    assert_err(
        "re_search('a', r'(?x:[a b])')",
        &[
            "Unsupported regex feature: verbose mode (?x) with whitespace or '#' in a character class; Python treats them as literals\n",
            "  re_search('a', r'(?x:[a b])')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn verbose_mode_escaped_space_in_class_accepted() {
    // An escaped space is a literal space in both engines, even under
    // verbose mode.
    assert!(eval("re_search(' ', r'(?x)[a\\ b]')").is_list());
}
#[test]
fn verbose_mode_class_without_whitespace_accepted() {
    assert!(eval("re_search('a', r'(?x) [ab] c?')").is_list());
}
#[test]
fn bare_flags_mid_pattern_rejected() {
    // Python 3.11+ raises "global flags not at the start of the
    // expression"; pre-3.11 applied the flag to the whole pattern where
    // Rust applies it only forward.
    assert_err(
        "re_search('ab', r'a(?i)b')",
        &[
            "Unsupported regex feature: bare inline flags not at the start of the pattern; use a scoped group like (?i:...)\n",
            "  re_search('ab', r'a(?i)b')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn multiple_leading_bare_flags_accepted() {
    // Consecutive flag groups at the very start are fine in both engines.
    assert!(eval("re_search('A B', r'(?i)(?s)a.b')").is_list());
}
#[test]
fn capture_name_with_dot_rejected() {
    // regex_syntax permits `.` in group names; Python raises "bad
    // character in group name".
    assert_err(
        "re_search('ab', r'(?P<a.b>a)b')",
        &[
            "Unsupported regex feature: capture group name 'a.b' is not a valid Python identifier\n",
            "  re_search('ab', r'(?P<a.b>a)b')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn capture_name_with_bracket_rejected() {
    assert_err(
        "re_search('ab', r'(?P<a[b>a)b')",
        &[
            "Unsupported regex feature: capture group name 'a[b' is not a valid Python identifier\n",
            "  re_search('ab', r'(?P<a[b>a)b')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn capture_name_with_non_xid_alphanumeric_rejected() {
    // `²` (U+00B2, category No) passes Rust's char::is_alphanumeric but is
    // not XID_Continue: "x²".isidentifier() is false, and CPython raises
    // "bad character in group name 'x²'".
    assert_err(
        "re_search('ab', r'(?P<x\u{b2}>a)b')",
        &[
            "Unsupported regex feature: capture group name 'x\u{b2}' is not a valid Python identifier\n",
            "  re_search('ab', r'(?P<x\u{b2}>a)b')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn capture_name_with_underscore_and_digits_accepted() {
    assert!(eval("re_search('ab', r'(?P<my_name1>a)b')").is_list());
}
#[test]
fn capture_name_non_ascii_xid_accepted() {
    // Non-ASCII XID characters are valid Python identifiers: "имя" and
    // "名前" pass str.isidentifier(), and CPython accepts them as group
    // names. The check must not over-reject them.
    assert!(eval("re_search('ab', r'(?P<\u{438}\u{43c}\u{44f}>a)b')").is_list());
    assert!(eval("re_search('ab', r'(?P<\u{540d}\u{524d}>a)b')").is_list());
}

// === TestRegexEscapedPatternsAccepted ===
#[test]
fn escaped_lookahead_accepted() {
    assert!(eval("re_search('(?=test', r'\\(\\?=')").is_list());
}

// === Additional TestReprFunctions ===
#[test]
fn repr_py_list_string() {
    assert_eq!(
        eval("repr_py(['a', 'b'])").to_display_string(),
        "['a', 'b']"
    );
}
#[test]
fn repr_py_list_int() {
    assert_eq!(eval("repr_py([1, 2])").to_display_string(), "[1, 2]");
}
#[test]
fn repr_py_list_bool() {
    assert_eq!(
        eval("repr_py([true, false])").to_display_string(),
        "[True, False]"
    );
}
#[test]
fn repr_json_list_string() {
    assert_eq!(
        eval("repr_json(['a', 'b'])").to_display_string(),
        "[\"a\", \"b\"]"
    );
}
#[test]
fn repr_json_list_int() {
    assert_eq!(eval("repr_json([1, 2])").to_display_string(), "[1, 2]");
}
#[test]
fn repr_json_null_explicit() {
    assert_eq!(eval("repr_json(null)").to_display_string(), "null");
}
#[test]
fn repr_py_null() {
    assert_eq!(eval("repr_py(null)").to_display_string(), "None");
}
#[test]
fn repr_py_range_expr() {
    assert_eq!(
        eval("repr_py(range_expr('1-5'))").to_display_string(),
        "'1-5'"
    );
}
#[test]
fn repr_json_range_expr() {
    assert_eq!(
        eval("repr_json(range_expr('1-5'))").to_display_string(),
        "\"1-5\""
    );
}

// === TestReprPwshComprehensive ===
#[test]
fn pwsh_string_with_single_quote() {
    // repr_pwsh("it's") -> 'it''s'
    let r = eval(r#"repr_pwsh("it's")"#);
    assert_eq!(r.to_display_string(), "'it''s'");
}
#[test]
fn pwsh_string_empty() {
    assert_eq!(eval("repr_pwsh('')").to_display_string(), "''");
}
#[test]
fn pwsh_int_positive() {
    assert_eq!(eval("repr_pwsh(42)").to_display_string(), "42");
}
#[test]
fn pwsh_int_negative() {
    assert_eq!(eval("repr_pwsh(-5)").to_display_string(), "-5");
}
#[test]
fn pwsh_float_simple() {
    assert_eq!(eval("repr_pwsh(3.14)").to_display_string(), "3.14");
}
#[test]
fn pwsh_bool_true() {
    assert_eq!(eval("repr_pwsh(true)").to_display_string(), "$true");
}
#[test]
fn pwsh_bool_false() {
    assert_eq!(eval("repr_pwsh(false)").to_display_string(), "$false");
}
#[test]
fn pwsh_list_empty() {
    assert_eq!(eval("repr_pwsh([])").to_display_string(), "@()");
}
#[test]
fn pwsh_list_ints() {
    assert_eq!(
        eval("repr_pwsh([1, 2, 3])").to_display_string(),
        "@(1, 2, 3)"
    );
}
#[test]
fn pwsh_range_expr() {
    assert_eq!(
        eval("repr_pwsh(range_expr('1-5'))").to_display_string(),
        "'1-5'"
    );
}

// === TestReprCmdComprehensive ===
#[test]
fn cmd_string_empty() {
    assert_eq!(eval("repr_cmd('')").to_display_string(), "\"\"");
}
#[test]
fn cmd_string_ampersand() {
    assert_eq!(eval("repr_cmd('a & b')").to_display_string(), "\"a & b\"");
}
#[test]
fn cmd_string_pipe() {
    assert_eq!(eval("repr_cmd('x | y')").to_display_string(), "\"x | y\"");
}
#[test]
fn cmd_string_caret() {
    assert_eq!(eval("repr_cmd('a ^ b')").to_display_string(), "\"a ^^ b\"");
}
#[test]
fn cmd_list_empty() {
    assert_eq!(eval("repr_cmd([])").to_display_string(), "");
}
#[test]
fn sh_list_empty() {
    assert_eq!(eval("repr_sh([])").to_display_string(), "");
}
#[test]
fn cmd_list_single() {
    assert_eq!(eval("repr_cmd(['hello'])").to_display_string(), "hello");
}
#[test]
fn cmd_list_with_spaces() {
    assert_eq!(
        eval("repr_cmd(['hello world'])").to_display_string(),
        "\"hello world\""
    );
}

// === Additional split/rsplit tests ===
#[test]
fn split_whitespace_empty() {
    assert_eq!(eval("split('   ')").to_display_string(), "[]");
}
#[test]
fn rsplit_whitespace_method() {
    assert_eq!(
        eval("'one  two'.rsplit()").to_display_string(),
        r#"["one", "two"]"#
    );
}
#[test]
fn rsplit_empty_separator() {
    assert_err(
        "'abc'.rsplit('')",
        &[
            "split failed: empty separator\n",
            "  'abc'.rsplit('')\n",
            "  ~~~~~~^~~~~~~~~~",
        ],
    );
}

// === Additional zfill tests ===
#[test]
fn zfill_float_preserves_round() {
    assert_eq!(
        eval("zfill(round(0.3, 2), 7)").to_display_string(),
        "0000.30"
    );
}
#[test]
fn zfill_float_method() {
    assert_eq!(eval("(3.14).zfill(8)").to_display_string(), "00003.14");
}

// === Unicode edge cases ===
#[test]
fn unicode_cjk() {
    assert_eq!(eval("'日本語'").to_display_string(), "日本語");
}
#[test]
fn unicode_emoji() {
    assert_eq!(eval("'🎉'").to_display_string(), "🎉");
}
#[test]
fn escape_unicode_16bit() {
    assert_eq!(eval(r"'\u0041'").to_display_string(), "A");
}
#[test]
fn escape_unicode_32bit() {
    assert_eq!(eval(r"'\U00000041'").to_display_string(), "A");
}

// === Repr with path values ===
#[test]
fn repr_py_path() {
    let mut st = openjd_expr::SymbolTable::new();
    st.set(
        "P",
        openjd_expr::ExprValue::new_path("/tmp/file.txt", openjd_expr::PathFormat::Posix),
    )
    .unwrap();
    let r = eval_posix_st("repr_py(P)", &st);
    assert_eq!(r.to_display_string(), "'/tmp/file.txt'");
}

#[test]
fn repr_py_list_path() {
    let mut st = openjd_expr::SymbolTable::new();
    st.set(
        "P",
        openjd_expr::ExprValue::make_list(
            vec![
                openjd_expr::ExprValue::new_path("/a", openjd_expr::PathFormat::Posix),
                openjd_expr::ExprValue::new_path("/b", openjd_expr::PathFormat::Posix),
            ],
            openjd_expr::ExprType::PATH,
        )
        .unwrap(),
    )
    .unwrap();
    let r = eval_posix_st("repr_py(P)", &st);
    assert_eq!(r.to_display_string(), "['/a', '/b']");
}

#[test]
fn repr_json_list_path() {
    let mut st = openjd_expr::SymbolTable::new();
    st.set(
        "P",
        openjd_expr::ExprValue::make_list(
            vec![openjd_expr::ExprValue::new_path(
                "/a",
                openjd_expr::PathFormat::Posix,
            )],
            openjd_expr::ExprType::PATH,
        )
        .unwrap(),
    )
    .unwrap();
    let r = eval_posix_st("repr_json(P)", &st);
    assert_eq!(r.to_display_string(), "[\"/a\"]");
}

#[test]
fn repr_pwsh_path() {
    let mut st = openjd_expr::SymbolTable::new();
    st.set(
        "P",
        openjd_expr::ExprValue::new_path("/tmp/file.txt", openjd_expr::PathFormat::Posix),
    )
    .unwrap();
    let r = eval_posix_st("repr_pwsh(P)", &st);
    assert_eq!(r.to_display_string(), "'/tmp/file.txt'");
}

// === Additional repr_pwsh comprehensive ===
#[test]
fn pwsh_string_with_double_quote() {
    assert_eq!(
        eval(r#"repr_pwsh('say "hi"')"#).to_display_string(),
        "'say \"hi\"'"
    );
}
#[test]
fn pwsh_string_with_dollar() {
    assert_eq!(eval("repr_pwsh('$var')").to_display_string(), "'$var'");
}
#[test]
fn pwsh_int_zero() {
    assert_eq!(eval("repr_pwsh(0)").to_display_string(), "0");
}
#[test]
fn pwsh_float_negative() {
    assert_eq!(eval("repr_pwsh(-2.5)").to_display_string(), "-2.5");
}
#[test]
fn pwsh_float_integer_value() {
    assert_eq!(eval("repr_pwsh(3.0)").to_display_string(), "3.0");
}
#[test]
fn pwsh_list_single() {
    assert_eq!(
        eval("repr_pwsh(['hello'])").to_display_string(),
        "@('hello')"
    );
}
#[test]
fn pwsh_list_multiple() {
    assert_eq!(
        eval("repr_pwsh(['a', 'b', 'c'])").to_display_string(),
        "@('a', 'b', 'c')"
    );
}
#[test]
fn pwsh_list_of_bools() {
    assert_eq!(
        eval("repr_pwsh([true, false])").to_display_string(),
        "@($true, $false)"
    );
}

// === Additional repr_cmd comprehensive ===
#[test]
fn cmd_string_newline() {
    // Newlines are stripped (security: prevents command injection via embedded newline).
    assert_eq!(eval("repr_cmd('a\\nb')").to_display_string(), "ab");
}
#[test]
fn cmd_string_newline_with_caret() {
    // \n stripped, ^ escaped inside quotes.
    assert_eq!(eval("repr_cmd('a\\n^b')").to_display_string(), "\"a^^b\"");
}
#[test]
fn cmd_string_newline_with_quote() {
    // \n stripped, " escaped as ^".
    assert_eq!(
        eval(r#"repr_cmd('a\n"b')"#).to_display_string(),
        "\"a^\"b\""
    );
}
#[test]
fn cmd_string_newline_with_percent() {
    // \n stripped, % doubled.
    assert_eq!(eval("repr_cmd('a\\n%b')").to_display_string(), "\"a%%b\"");
}
#[test]
fn cmd_string_newline_with_bang() {
    // \n stripped, ! escaped as ^^!.
    assert_eq!(eval("repr_cmd('a\\n!b')").to_display_string(), "\"a^^!b\"");
}
#[test]
fn cmd_string_cr_with_special() {
    // \r stripped, & triggers quoting but is literal inside quotes.
    assert_eq!(eval("repr_cmd('a\\r&b')").to_display_string(), "\"a&b\"");
}
#[test]
fn cmd_string_crlf_with_special() {
    // \r\n stripped, ^ escaped.
    assert_eq!(
        eval("repr_cmd('a\\r\\n^b')").to_display_string(),
        "\"a^^b\""
    );
}
#[test]
fn cmd_string_less_than() {
    assert_eq!(eval("repr_cmd('a < b')").to_display_string(), "\"a < b\"");
}
#[test]
fn cmd_string_greater_than() {
    assert_eq!(eval("repr_cmd('a > b')").to_display_string(), "\"a > b\"");
}
#[test]
fn cmd_string_double_quote() {
    let r = eval(r#"repr_cmd('say "hi"')"#);
    assert!(
        r.to_display_string().contains("^\"") || r.to_display_string().contains("\\\""),
        "got: {}",
        r.to_display_string()
    );
}
#[test]
fn cmd_string_multiple_special() {
    let r = eval("repr_cmd('a & b | c')");
    assert!(
        r.to_display_string().starts_with('"'),
        "got: {}",
        r.to_display_string()
    );
}
#[test]
fn cmd_string_windows_path() {
    assert_eq!(
        eval(r"repr_cmd('C:\\Users\\test')").to_display_string(),
        "C:\\Users\\test"
    );
}
#[test]
fn cmd_list_multiple() {
    let r = eval("repr_cmd(['a', 'b', 'c'])");
    assert_eq!(r.to_display_string(), "a b c");
}
#[test]
fn cmd_list_with_special() {
    let r = eval("repr_cmd(['hello', 'a & b'])");
    assert!(
        r.to_display_string().contains("\"a & b\""),
        "got: {}",
        r.to_display_string()
    );
}

// === Additional string literal formats ===
#[test]
fn triple_with_quotes_inside() {
    assert_eq!(
        eval("'''it's a \"test\"'''").to_display_string(),
        "it's a \"test\""
    );
}
#[test]
fn raw_string_backslash_preserved() {
    assert_eq!(
        eval(r"r'C:\Users\test'").to_display_string(),
        "C:\\Users\\test"
    );
}
#[test]
fn raw_triple_quoted() {
    assert_eq!(
        eval(r"r'''hello\nworld'''").to_display_string(),
        "hello\\nworld"
    );
}

// === Additional rejected formats ===
#[test]
fn raw_bytes_rejected() {
    assert_err(
        "rb'hello'",
        &["Byte strings (b'...') are not supported. Use '...' or \"...\" instead.\n"],
    );
}
#[test]
fn raw_fstring_rejected() {
    assert_err(
        "rf'hello'",
        &["f-strings are not supported; use string concatenation\n"],
    );
}

// === Additional TestRemovePrefixSuffix ===
#[test]
fn removesuffix_with_suffixes_join() {
    let mut st = openjd_expr::SymbolTable::new();
    st.set(
        "P",
        openjd_expr::ExprValue::new_path("/data/archive.tar.gz", openjd_expr::PathFormat::Posix),
    )
    .unwrap();
    let r = eval_posix_st("P.name.removesuffix(P.suffixes.join(''))", &st);
    assert_eq!(r.to_display_string(), "archive");
}

// === Remaining Python tests with matching names ===
#[test]
fn single_quoted() {
    assert_eq!(eval("'hello'").to_display_string(), "hello");
}
#[test]
fn double_quoted() {
    assert_eq!(eval("\"hello\"").to_display_string(), "hello");
}
#[test]
fn triple_single_quoted() {
    assert_eq!(eval("'''hello'''").to_display_string(), "hello");
}
#[test]
fn triple_double_quoted() {
    assert_eq!(eval("\"\"\"hello\"\"\"").to_display_string(), "hello");
}
#[test]
fn unicode_characters() {
    assert_eq!(eval("'café'").to_display_string(), "café");
}
#[test]
fn string_range_expr_concatenation() {
    assert_eq!(
        eval("range_expr('1-3') + ' are frames'").to_display_string(),
        "1-3 are frames"
    );
}
#[test]
fn method_syntax() {
    assert_eq!(eval("'hello'.upper()").to_display_string(), "HELLO");
}
#[test]
fn len() {
    assert_eq!(eval("len('hello')").to_display_string(), "5");
}
#[test]
fn find_at_start() {
    assert_eq!(eval("find('hello', 'hel')").to_display_string(), "0");
}
#[test]
fn find_method_syntax() {
    assert_eq!(eval("'hello'.find('lo')").to_display_string(), "3");
}
#[test]
fn rfind_method_syntax() {
    assert_eq!(eval("'abcabc'.rfind('bc')").to_display_string(), "4");
}
#[test]
fn index_method_syntax() {
    assert_eq!(eval("'hello'.index('lo')").to_display_string(), "3");
}
#[test]
fn rindex_method_syntax() {
    assert_eq!(eval("'abcabc'.rindex('bc')").to_display_string(), "4");
}
#[test]
fn removeprefix_not_present() {
    assert_eq!(
        eval("removeprefix('hello world', 'bye ')").to_display_string(),
        "hello world"
    );
}
#[test]
fn removeprefix_empty_prefix() {
    assert_eq!(
        eval("removeprefix('hello', '')").to_display_string(),
        "hello"
    );
}
#[test]
fn removeprefix_full_string() {
    assert_eq!(
        eval("removeprefix('hello', 'hello')").to_display_string(),
        ""
    );
}
#[test]
fn removeprefix_method_syntax() {
    assert_eq!(
        eval("'hello world'.removeprefix('hello ')").to_display_string(),
        "world"
    );
}
#[test]
fn removesuffix_not_present() {
    assert_eq!(
        eval("removesuffix('hello.txt', '.py')").to_display_string(),
        "hello.txt"
    );
}
#[test]
fn removesuffix_empty_suffix() {
    assert_eq!(
        eval("removesuffix('hello', '')").to_display_string(),
        "hello"
    );
}
#[test]
fn removesuffix_full_string() {
    assert_eq!(
        eval("removesuffix('hello', 'hello')").to_display_string(),
        ""
    );
}
#[test]
fn removesuffix_method_syntax() {
    assert_eq!(
        eval("'hello.txt'.removesuffix('.txt')").to_display_string(),
        "hello"
    );
}
#[test]
fn removesuffix_compound_extension() {
    assert_eq!(
        eval("'archive.tar.gz'.removesuffix('.tar.gz')").to_display_string(),
        "archive"
    );
}
#[test]
fn substring_in_string() {
    assert_eq!(eval("\"ell\" in \"hello\"").to_display_string(), "true");
}
#[test]
fn substring_not_in_string() {
    assert_eq!(eval("\"xyz\" in \"hello\"").to_display_string(), "false");
}
#[test]
fn not_in_operator_true() {
    assert_eq!(eval("\"xyz\" not in \"hello\"").to_display_string(), "true");
}
#[test]
fn not_in_operator_false() {
    assert_eq!(
        eval("\"ell\" not in \"hello\"").to_display_string(),
        "false"
    );
}
#[test]
fn empty_string_in_string() {
    assert_eq!(eval("\"\" in \"hello\"").to_display_string(), "true");
}
#[test]
fn string_in_empty_string() {
    assert_eq!(eval("\"x\" in \"\"").to_display_string(), "false");
}
#[test]
fn rsplit_whitespace() {
    assert_eq!(
        eval("rsplit('  hello \\t world  ')").to_display_string(),
        r#"["hello", "world"]"#
    );
}
#[test]
fn zfill_float_negative() {
    assert_eq!(eval("zfill(-2.5, 8)").to_display_string(), "-00002.5");
}
#[test]
fn zfill_float_preserves_round_precision() {
    assert_eq!(
        eval("zfill(round(0.3, 2), 7)").to_display_string(),
        "0000.30"
    );
}
#[test]
fn zfill_float_method_syntax() {
    assert_eq!(eval("(3.14).zfill(8)").to_display_string(), "00003.14");
}
#[test]
fn raw_string_lowercase_r() {
    assert_eq!(
        eval(r"r'hello\nworld'").to_display_string(),
        "hello\\nworld"
    );
}
#[test]
fn raw_string_uppercase_r() {
    assert_eq!(
        eval(r"R'hello\nworld'").to_display_string(),
        "hello\\nworld"
    );
}
#[test]
fn raw_string_double_quoted() {
    assert_eq!(
        eval(r#"r"hello\nworld""#).to_display_string(),
        "hello\\nworld"
    );
}
#[test]
fn re_search_method_syntax() {
    assert!(eval("re_search('hello123', r'\\d+')").is_list());
}
#[test]
fn re_sub_method_syntax() {
    assert_eq!(
        eval("re_sub('a1b2', r'\\d', 'X')").to_display_string(),
        "aXbX"
    );
}
#[test]
fn re_search_boolean_check() {
    assert_eq!(
        eval(r"re_search('hello123', r'\d+') != null").to_display_string(),
        "true"
    );
}
#[test]
fn re_escape_with_search() {
    // re_escape produces a pattern that matches literally
    let r = eval("re_search('a.b', re_escape('a.b'))");
    assert!(r.is_list());
}
#[test]
fn re_findall_with_groups() {
    assert!(eval(r"re_findall('a1b2c3', r'([a-z])(\d)')").is_list());
}
#[test]
fn re_split_multi_char() {
    assert!(eval("re_split('a::b::c', '::')").is_list());
}
#[test]
fn re_split_date_separators() {
    assert!(eval(r"re_split('2024-01-15', r'[-/]')").is_list());
}
#[test]
fn re_split_kv_pairs() {
    assert!(eval(r"re_split('key=value', r'[=:]')").is_list());
}
#[test]
fn re_split_method_syntax() {
    assert!(eval("re_split('a,b,c', ',')").is_list());
}
#[test]
fn re_split_maxsplit_empty_pattern() {
    assert_err(
        "re_split('hello', '', 1)",
        &[
            "Empty regex pattern is not allowed\n",
            "  re_split('hello', '', 1)\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn re_sub_group_ref_dollar_brace() {
    assert_err(
        "re_sub('hello', '(h)', '${1}')",
        &[
            "Group references in replacement strings are not supported\n",
            "  re_sub('hello', '(h)', '${1}')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn re_sub_group_ref_named() {
    assert_err(
        "re_sub('hello', '(h)', r'\\g<1>')",
        &[
            "Group references in replacement strings are not supported\n",
            "  re_sub('hello', '(h)', r'\\g<1>')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn double_backslash_lookahead_rejected() {
    assert_err(
        "re_search('test', '\\\\\\\\(?=x)')",
        &["Unsupported regex feature: lookahead\n"],
    );
}
#[test]
fn escaped_backreference_accepted() {
    assert!(
        matches!(eval(r"re_search('a1b', r'\\1')"), ExprValue::Null)
            || eval(r"re_search('a1b', r'\\1')").is_list()
    );
}
#[test]
fn escaped_lookbehind_accepted() {
    assert!(eval(r"re_search('(?<=test', r'\(\?<=')").is_list());
}
#[test]
fn escaped_negative_lookahead_accepted() {
    assert!(eval(r"re_search('(?!test', r'\(\?!')").is_list());
}
#[test]
fn escaped_negative_lookbehind_accepted() {
    assert!(eval(r"re_search('(?<!test', r'\(\?<!')").is_list());
}
// pwsh comprehensive extras
#[test]
fn pwsh_string_simple() {
    assert_eq!(eval("repr_pwsh('hello')").to_display_string(), "'hello'");
}
#[test]
fn pwsh_string_with_spaces() {
    assert_eq!(
        eval("repr_pwsh('hello world')").to_display_string(),
        "'hello world'"
    );
}
#[test]
fn pwsh_string_with_multiple_quotes() {
    assert_eq!(
        eval(r#"repr_pwsh("it's John's")"#).to_display_string(),
        "'it''s John''s'"
    );
}
#[test]
fn pwsh_string_with_backtick() {
    assert_eq!(eval("repr_pwsh('a`b')").to_display_string(), "'a`b'");
}
#[test]
fn pwsh_list_with_spaces() {
    assert_eq!(
        eval("repr_pwsh(['hello world'])").to_display_string(),
        "@('hello world')"
    );
}
#[test]
fn pwsh_list_with_quotes() {
    assert_eq!(
        eval(r#"repr_pwsh(["it's"])"#).to_display_string(),
        "@('it''s')"
    );
}
#[test]
fn pwsh_list_of_floats() {
    assert_eq!(
        eval("repr_pwsh([1.5, 2.5])").to_display_string(),
        "@(1.5, 2.5)"
    );
}
#[test]
fn pwsh_range_expr_with_step() {
    assert_eq!(
        eval("repr_pwsh(range_expr('1-10:2'))").to_display_string(),
        "'1-9:2'"
    );
}
// cmd comprehensive extras
#[test]
fn cmd_string_simple() {
    assert_eq!(eval("repr_cmd('hello')").to_display_string(), "hello");
}
#[test]
fn cmd_string_with_spaces() {
    assert_eq!(
        eval("repr_cmd('hello world')").to_display_string(),
        "\"hello world\""
    );
}
#[test]
fn cmd_string_carriage_return() {
    // Carriage returns are stripped alongside newlines.
    assert_eq!(eval("repr_cmd('a\\rb')").to_display_string(), "ab");
}
#[test]
fn cmd_string_all_special() {
    assert!(eval("repr_cmd('a & b | c ^ d')")
        .to_display_string()
        .starts_with('"'));
}
#[test]
fn cmd_string_path_with_spaces() {
    assert!(eval("repr_cmd('C:\\\\Program Files\\\\app')")
        .to_display_string()
        .contains("\""));
}
#[test]
fn cmd_list_with_quotes() {
    assert!(eval(r#"repr_cmd(['say "hi"'])"#)
        .to_display_string()
        .contains("^"));
}
#[test]
fn cmd_set_variable_pattern() {
    assert!(eval("repr_cmd('FOO=bar baz')")
        .to_display_string()
        .contains("\""));
}
// repr extras
#[test]
fn repr_pwsh_string() {
    assert_eq!(eval("repr_pwsh('hello')").to_display_string(), "'hello'");
}
#[test]
fn repr_pwsh_int() {
    assert_eq!(eval("repr_pwsh(42)").to_display_string(), "42");
}
#[test]
fn repr_pwsh_float() {
    assert_eq!(eval("repr_pwsh(3.14)").to_display_string(), "3.14");
}
#[test]
fn repr_pwsh_bool() {
    assert_eq!(eval("repr_pwsh(true)").to_display_string(), "$true");
}
#[test]
fn repr_pwsh_list() {
    assert_eq!(eval("repr_pwsh([1, 2])").to_display_string(), "@(1, 2)");
}
#[test]
fn repr_json_list_bool() {
    assert_eq!(
        eval("repr_json([true, false])").to_display_string(),
        "[true, false]"
    );
}
#[test]
fn repr_json_null() {
    assert_eq!(eval("repr_json(null)").to_display_string(), "null");
}

// =============================================================================
// Missing Python tests ported below
// =============================================================================

// --- String classification: missing parametrized cases ---
#[test]
fn isalpha_empty() {
    assert_eq!(eval("''.isalpha()").to_display_string(), "false");
}
#[test]
fn isalnum_empty() {
    assert_eq!(eval("''.isalnum()").to_display_string(), "false");
}
#[test]
fn isspace_true() {
    assert_eq!(eval("'  \\t\\n'.isspace()").to_display_string(), "true");
}
#[test]
fn isspace_false() {
    assert_eq!(eval("' hi '.isspace()").to_display_string(), "false");
}
#[test]
fn isspace_empty() {
    assert_eq!(eval("''.isspace()").to_display_string(), "false");
}
#[test]
fn isupper_digits() {
    assert_eq!(eval("'123'.isupper()").to_display_string(), "false");
}
#[test]
fn islower_digits() {
    assert_eq!(eval("'123'.islower()").to_display_string(), "false");
}
#[test]
fn isascii_non_ascii() {
    assert_eq!(eval("'h\\xe9llo'.isascii()").to_display_string(), "false");
}

// --- lstrip/rstrip with dots (from parametrized test_string_functions) ---
#[test]
fn lstrip_dots() {
    assert_eq!(eval("lstrip('...hi...', '.')").to_display_string(), "hi...");
}
#[test]
fn rstrip_dots() {
    assert_eq!(eval("rstrip('...hi...', '.')").to_display_string(), "...hi");
}

// --- Split/rsplit exact values ---
#[test]
fn split_exact_values() {
    assert_eq!(
        eval("split('a,b,c', ',')").to_display_string(),
        r#"["a", "b", "c"]"#
    );
}
#[test]
fn split_whitespace_exact_values() {
    assert_eq!(
        eval("split('  hello \\t world  ')").to_display_string(),
        r#"["hello", "world"]"#
    );
}
#[test]
fn split_whitespace_method_exact() {
    assert_eq!(
        eval("'one  two\\tthree\\nfour'.split()").to_display_string(),
        r#"["one", "two", "three", "four"]"#
    );
}
#[test]
fn split_maxsplit_exact() {
    assert_eq!(
        eval("split('a,b,c,d', ',', 2)").to_display_string(),
        r#"["a", "b", "c,d"]"#
    );
}
#[test]
fn split_maxsplit_method_exact() {
    assert_eq!(
        eval("'a/b/c/d'.split('/', 1)").to_display_string(),
        r#"["a", "b/c/d"]"#
    );
}
#[test]
fn rsplit_exact_values() {
    assert_eq!(
        eval("rsplit('a,b,c', ',')").to_display_string(),
        r#"["a", "b", "c"]"#
    );
}
#[test]
fn rsplit_whitespace_method_exact() {
    assert_eq!(
        eval("'one  two\\tthree'.rsplit()").to_display_string(),
        r#"["one", "two", "three"]"#
    );
}
#[test]
fn rsplit_maxsplit_exact() {
    assert_eq!(
        eval("rsplit('a,b,c,d', ',', 2)").to_display_string(),
        r#"["a,b", "c", "d"]"#
    );
}
#[test]
fn rsplit_maxsplit_method_exact() {
    assert_eq!(
        eval("'a/b/c/d'.rsplit('/', 1)").to_display_string(),
        r#"["a/b/c", "d"]"#
    );
}

// Regression: negative maxsplit wrapped through `as usize`, then `n + 1`
// overflowed (debug panic; release returned []). Python treats any
// negative maxsplit as "no limit".
#[test]
fn split_negative_maxsplit_means_no_limit() {
    assert_eq!(
        eval("split('a b c', ' ', -1)").to_display_string(),
        r#"["a", "b", "c"]"#
    );
    assert_eq!(
        eval("split('a,b,c,d', ',', -9223372036854775808)").to_display_string(),
        r#"["a", "b", "c", "d"]"#
    );
}

#[test]
fn rsplit_negative_maxsplit_means_no_limit() {
    assert_eq!(
        eval("rsplit('a b c', ' ', -1)").to_display_string(),
        r#"["a", "b", "c"]"#
    );
    assert_eq!(
        eval("rsplit('a,b,c,d', ',', -2)").to_display_string(),
        r#"["a", "b", "c", "d"]"#
    );
}

#[test]
fn split_zero_maxsplit_means_no_splits() {
    // Boundary of the negative-maxsplit fix: 0 must still mean "no splits".
    assert_eq!(
        eval("split('a b c', ' ', 0)").to_display_string(),
        r#"["a b c"]"#
    );
    assert_eq!(
        eval("rsplit('a b c', ' ', 0)").to_display_string(),
        r#"["a b c"]"#
    );
}
// Regression: the `i64 -> usize` maxsplit conversion used a plain `as` cast,
// which is lossy where `usize` is narrower than `i64`. On a 32-bit target a
// maxsplit above `u32::MAX` wrapped to a small number (2^32 wrapped to 0,
// giving no splits at all) and `n + 1` could overflow `usize`. The conversion
// now saturates to `usize::MAX` and the `+ 1` saturates too, so "maxsplit
// larger than the string" means "no limit" on every target width.
//
// NOTE ON FALSIFIABILITY: on a 64-bit host every non-negative `i64` converts
// losslessly and `n + 1` cannot overflow `usize`, so these assertions pass with
// or without the fix. They are load-bearing only on a 32-bit target, where the
// unfixed code returns `["a b c"]` for the 2^32 case. Treat them as a contract
// pinned for 32-bit builds, not as proof on this host.
#[test]
fn split_huge_maxsplit_means_no_limit() {
    // 2^32: wraps to 0 under a lossy cast on 32-bit, i.e. "no splits".
    assert_eq!(
        eval("split('a b c', ' ', 4294967296)").to_display_string(),
        r#"["a", "b", "c"]"#
    );
    // u32::MAX: survives the cast on 32-bit, but then `n + 1` overflows usize.
    assert_eq!(
        eval("split('a b c', ' ', 4294967295)").to_display_string(),
        r#"["a", "b", "c"]"#
    );
    // i64::MAX: the widest value the parser can hand us.
    assert_eq!(
        eval("split('a,b,c,d', ',', 9223372036854775807)").to_display_string(),
        r#"["a", "b", "c", "d"]"#
    );
}

#[test]
fn rsplit_huge_maxsplit_means_no_limit() {
    assert_eq!(
        eval("rsplit('a b c', ' ', 4294967296)").to_display_string(),
        r#"["a", "b", "c"]"#
    );
    assert_eq!(
        eval("rsplit('a b c', ' ', 4294967295)").to_display_string(),
        r#"["a", "b", "c"]"#
    );
    assert_eq!(
        eval("rsplit('a,b,c,d', ',', 9223372036854775807)").to_display_string(),
        r#"["a", "b", "c", "d"]"#
    );
}

#[test]
fn re_split_huge_maxsplit_means_no_limit() {
    // re_split() shares the same lossy-cast shape as split()/rsplit().
    assert_eq!(
        eval("re_split('a1b2c', '[0-9]', 4294967296)").to_display_string(),
        r#"["a", "b", "c"]"#
    );
    assert_eq!(
        eval("re_split('a1b2c', '[0-9]', 4294967295)").to_display_string(),
        r#"["a", "b", "c"]"#
    );
    assert_eq!(
        eval("re_split('a1b2c3d', '[0-9]', 9223372036854775807)").to_display_string(),
        r#"["a", "b", "c", "d"]"#
    );
}

// Negative control for the saturation: a small positive maxsplit must still
// limit the split. A mutation that clamped every positive maxsplit up to
// "unlimited" would pass the three tests above and fail this one.
#[test]
fn small_positive_maxsplit_still_limits() {
    assert_eq!(
        eval("split('a b c d', ' ', 1)").to_display_string(),
        r#"["a", "b c d"]"#
    );
    assert_eq!(
        eval("rsplit('a b c d', ' ', 1)").to_display_string(),
        r#"["a b c", "d"]"#
    );
    assert_eq!(
        eval("re_split('a1b2c3d', '[0-9]', 1)").to_display_string(),
        r#"["a", "b2c3d"]"#
    );
}

#[test]
fn rsplit_no_match_exact() {
    assert_eq!(eval("rsplit('abc', ',')").to_display_string(), r#"["abc"]"#);
}
#[test]
fn split_empty_string_exact() {
    assert_eq!(eval("''.split(',')").to_display_string(), r#"[""]"#);
}

// --- String membership: missing assertions ---
#[test]
fn case_sensitive_reverse() {
    assert_eq!(eval("\"hello\" in \"HELLO\"").to_display_string(), "false");
}
#[test]
fn with_spaces_substring() {
    assert_eq!(
        eval("\"lo wo\" in \"hello world\"").to_display_string(),
        "true"
    );
}
#[test]
fn membership_via_symtab() {
    let mut st = openjd_expr::SymbolTable::new();
    st.set(
        "haystack",
        openjd_expr::ExprValue::String("hello world".into()),
    )
    .unwrap();
    st.set("needle", openjd_expr::ExprValue::String("world".into()))
        .unwrap();
    let r = openjd_expr::ParsedExpression::new("needle in haystack")
        .and_then(|p| p.evaluate(&st))
        .unwrap();
    assert_eq!(r.to_display_string(), "true");
}

// --- repr_py/repr_json with 3-element lists ---
#[test]
fn repr_py_list_string_3() {
    assert_eq!(
        eval("repr_py(['a', 'b', 'c'])").to_display_string(),
        "['a', 'b', 'c']"
    );
}
#[test]
fn repr_py_list_int_3() {
    assert_eq!(eval("repr_py([1, 2, 3])").to_display_string(), "[1, 2, 3]");
}
#[test]
fn repr_json_list_string_3() {
    assert_eq!(
        eval("repr_json(['a', 'b', 'c'])").to_display_string(),
        r#"["a", "b", "c"]"#
    );
}
#[test]
fn repr_json_list_int_3() {
    assert_eq!(
        eval("repr_json([1, 2, 3])").to_display_string(),
        "[1, 2, 3]"
    );
}

// --- repr_json/repr_py with None keyword ---
#[test]
fn repr_json_none_keyword() {
    assert_eq!(eval("repr_json(None)").to_display_string(), "null");
}
#[test]
fn repr_py_none_keyword() {
    assert_eq!(eval("repr_py(None)").to_display_string(), "None");
}

// --- repr_pwsh list with quotes ---
#[test]
fn pwsh_list_with_quotes_done() {
    assert_eq!(
        eval(r#"repr_pwsh(["it's", 'done'])"#).to_display_string(),
        "@('it''s', 'done')"
    );
}

// --- Regex: exact return values for missing tests ---
#[test]
fn re_search_multiple_groups_exact() {
    assert_eq!(
        eval(r"re_search('hello123world', r'(\d+)(\w+)')").to_display_string(),
        r#"["123world", "123", "world"]"#
    );
}
#[test]
fn re_findall_with_one_group_exact() {
    assert_eq!(
        eval(r"re_findall('shot010_shot020', r'shot(\d+)')").to_display_string(),
        r#"["010", "020"]"#
    );
}
#[test]
fn re_findall_with_multiple_groups_exact() {
    assert_eq!(
        eval(r"re_findall('v1.2.3 and v4.5.6', r'v(\d+)\.(\d+)\.(\d+)')").to_display_string(),
        r#"[["1", "2", "3"], ["4", "5", "6"]]"#
    );
}
#[test]
fn re_sub_method_syntax_exact() {
    assert_eq!(
        eval(r"'hello'.re_sub(r'l+', 'L')").to_display_string(),
        "heLo"
    );
}
#[test]
fn re_search_boolean_check_false() {
    assert_eq!(
        eval(r"re_search('hello', r'\d+') != null").to_display_string(),
        "false"
    );
}

// --- re_escape exact values ---
#[test]
fn re_escape_brackets() {
    assert_eq!(
        eval(r"re_escape('file[1].txt')").to_display_string(),
        r"file\[1\]\.txt"
    );
}
#[test]
fn re_escape_with_search_exact() {
    assert_eq!(
        eval(r"re_search('file[1].txt', re_escape('[1]'))").to_display_string(),
        r#"["[1]"]"#
    );
}

// --- re_split exact values ---
#[test]
fn re_split_comma_semicolon_exact() {
    assert_eq!(
        eval(r"re_split('one,two;three', r'[,;]')").to_display_string(),
        r#"["one", "two", "three"]"#
    );
}
#[test]
fn re_split_digits_exact() {
    assert_eq!(
        eval(r"re_split('abc123def4567ghi89', r'[0-9]+')").to_display_string(),
        r#"["abc", "def", "ghi", ""]"#
    );
}
#[test]
fn re_split_whitespace_exact() {
    assert_eq!(
        eval(r"re_split('  hello   world  ', r'\s+')").to_display_string(),
        r#"["", "hello", "world", ""]"#
    );
}
#[test]
fn re_split_multi_char_exact() {
    assert_eq!(
        eval(r"re_split('foo::bar:::baz', r':+')").to_display_string(),
        r#"["foo", "bar", "baz"]"#
    );
}
#[test]
fn re_split_date_exact() {
    assert_eq!(
        eval(r"re_split('2024-01-15', r'[-/]')").to_display_string(),
        r#"["2024", "01", "15"]"#
    );
}
#[test]
fn re_split_maxsplit_exact() {
    assert_eq!(
        eval(r"re_split('a1b2c3d4e', r'[0-9]+', 2)").to_display_string(),
        r#"["a", "b", "c3d4e"]"#
    );
}
#[test]
fn re_split_negative_maxsplit_means_no_splits() {
    // Regression: negative maxsplit wrapped via `as usize`, then `n + 1`
    // panicked in debug builds. Python's re.split (unlike str.split) treats
    // negative maxsplit as "no splits at all".
    assert_eq!(
        eval("re_split('a b c', ' ', -1)").to_display_string(),
        r#"["a b c"]"#
    );
    assert_eq!(
        eval("re_split('a b c', ' ', -9223372036854775808)").to_display_string(),
        r#"["a b c"]"#
    );
}
#[test]
fn re_split_zero_maxsplit_means_unlimited() {
    // Python re.split parity: maxsplit=0 means unlimited (str.split's 0 means
    // no splits). Previously Rust returned ["a b c"] here.
    assert_eq!(
        eval("re_split('a b c', ' ', 0)").to_display_string(),
        r#"["a", "b", "c"]"#
    );
}
#[test]
fn re_split_kv_exact() {
    assert_eq!(
        eval(r"re_split('key1=val1,key2=val2', r'[=,]')").to_display_string(),
        r#"["key1", "val1", "key2", "val2"]"#
    );
}
#[test]
fn re_split_method_exact() {
    assert_eq!(
        eval(r#"'one::two::three'.re_split(r'::')"#).to_display_string(),
        r#"["one", "two", "three"]"#
    );
}

// --- Regex unsupported features: re_match/re_findall/re_sub validate too ---
#[test]
fn re_match_backreference_rejected() {
    assert_err(
        "re_match('abab', r'(ab)\\1')",
        &[
            "Unsupported regex feature: backreferences\n",
            "  re_match('abab', r'(ab)\\1')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn re_findall_lookahead_rejected() {
    assert_err(
        "re_findall('foobar', r'foo(?=bar)')",
        &[
            "Unsupported regex feature: lookahead\n",
            "  re_findall('foobar', r'foo(?=bar)')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn re_sub_lookbehind_rejected() {
    assert_err(
        "re_sub('foobar', r'(?<=foo)bar', 'X')",
        &[
            "Unsupported regex feature: lookbehind\n",
            "  re_sub('foobar', r'(?<=foo)bar', 'X')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}

// --- Escaped regex patterns: exact return values ---
#[test]
fn escaped_backreference_exact() {
    // The matched string is `\1`; the backslash is JSON-escaped in the list
    // display form.
    assert_eq!(
        eval(r#"re_search('test\\1', r'\\1')"#).to_display_string(),
        r#"["\\1"]"#
    );
}
#[test]
fn escaped_lookahead_exact() {
    assert_eq!(
        eval(r"re_search('foo(?=bar)', r'\(\?=bar\)')").to_display_string(),
        r#"["(?=bar)"]"#
    );
}
#[test]
fn escaped_lookbehind_exact() {
    assert_eq!(
        eval(r"re_search('(?<=foo)bar', r'\(\?<=foo\)')").to_display_string(),
        r#"["(?<=foo)"]"#
    );
}
#[test]
fn escaped_negative_lookahead_exact() {
    assert_eq!(
        eval(r"re_search('foo(?!baz)', r'\(\?!baz\)')").to_display_string(),
        r#"["(?!baz)"]"#
    );
}
#[test]
fn escaped_negative_lookbehind_exact() {
    assert_eq!(
        eval(r"re_search('(?<!baz)bar', r'\(\?<!baz\)')").to_display_string(),
        r#"["(?<!baz)"]"#
    );
}

// --- Rejected string formats ---
#[test]
fn unicode_prefix_rejected() {
    assert_err(
        "u\"hello\"",
        &["Unicode string prefix u'...' is not supported. Use '...' or \"...\" instead.\n"],
    );
}
#[test]
fn br_bytes_rejected() {
    assert_err(
        r#"br"hello""#,
        &["Byte strings (b'...') are not supported. Use '...' or \"...\" instead.\n"],
    );
}
#[test]
fn fr_string_rejected() {
    assert_err(
        r#"fr"hello {1}""#,
        &["f-strings are not supported; use string concatenation\n"],
    );
}

// --- Escape unicode name ---
#[test]
fn escape_unicode_name() {
    assert_eq!(
        eval(r"'\N{LATIN CAPITAL LETTER A}'").to_display_string(),
        "A"
    );
}

// --- Triple quoted with double quotes inside (Python: """he said "hi" """) ---
#[test]
fn triple_double_with_quotes_inside() {
    assert_eq!(
        eval(r#""""he said "hi" """"#).to_display_string(),
        r#"he said "hi" "#
    );
}

// --- repr_cmd exact values ---
#[test]
fn cmd_string_all_special_exact() {
    assert_eq!(
        eval(r#"repr_cmd('&|<>^"')"#).to_display_string(),
        "\"&|<>^^^\"\""
    );
}
#[test]
fn cmd_string_double_quote_exact() {
    assert_eq!(
        eval(r#"repr_cmd('say "hi"')"#).to_display_string(),
        "\"say ^\"hi^\"\""
    );
}
#[test]
fn cmd_string_windows_path_exact() {
    assert_eq!(
        eval(r"repr_cmd('C:\\Program Files\\App')").to_display_string(),
        r#""C:\Program Files\App""#
    );
}
#[test]
fn cmd_string_path_with_spaces_exact() {
    assert_eq!(
        eval(r"repr_cmd('C:\\My Files\\data.txt')").to_display_string(),
        r#""C:\My Files\data.txt""#
    );
}
#[test]
fn cmd_list_multiple_exact() {
    assert_eq!(
        eval("repr_cmd(['echo', 'hello', 'world'])").to_display_string(),
        "echo hello world"
    );
}
#[test]
fn cmd_list_with_spaces_exact() {
    assert_eq!(
        eval("repr_cmd(['echo', 'hello world'])").to_display_string(),
        r#"echo "hello world""#
    );
}
#[test]
fn cmd_list_with_special_exact() {
    assert_eq!(
        eval("repr_cmd(['cmd', '/c', 'echo a & b'])").to_display_string(),
        r#"cmd /c "echo a & b""#
    );
}
#[test]
fn cmd_list_with_quotes_exact() {
    assert_eq!(
        eval(r#"repr_cmd(['echo', 'say "hi"'])"#).to_display_string(),
        "echo \"say ^\"hi^\"\""
    );
}

// --- repr_cmd set variable patterns ---
#[test]
fn cmd_set_variable_pattern_exact() {
    assert_eq!(
        eval_fmt(
            r"'set ' + repr_cmd('OUTPUT_DIR=' + path('C:\\Users\\test&user\\output'))",
            PathFormat::Windows
        )
        .to_display_string(),
        r#"set "OUTPUT_DIR=C:\Users\test&user\output""#
    );
}
#[test]
fn cmd_set_variable_with_spaces_exact() {
    assert_eq!(
        eval_fmt(
            r"'set ' + repr_cmd('MY_PATH=' + path('C:\\Program Files\\App'))",
            PathFormat::Windows
        )
        .to_display_string(),
        r#"set "MY_PATH=C:\Program Files\App""#
    );
}

// --- repr_cmd newline/carriage_return exact ---
#[test]
fn cmd_string_newline_exact() {
    // \n is stripped before quoting; "ab" has no special chars → unquoted.
    assert_eq!(eval(r"repr_cmd('a\nb')").to_display_string(), "ab");
}
#[test]
fn cmd_string_carriage_return_exact() {
    assert_eq!(eval(r"repr_cmd('a\rb')").to_display_string(), "ab");
}
#[test]
fn cmd_string_crlf_stripped() {
    assert_eq!(eval(r"repr_cmd('a\r\nb')").to_display_string(), "ab");
}
#[test]
fn cmd_string_newline_with_special_retains_quoting() {
    // After stripping "\n", "a&b" still contains special chars → quoted.
    assert_eq!(eval(r"repr_cmd('a\n&\nb')").to_display_string(), "\"a&b\"");
}
#[test]
fn cmd_string_only_newlines_becomes_empty_quoted() {
    // An all-newline string becomes empty after stripping; empty strings are quoted.
    assert_eq!(eval(r"repr_cmd('\n\r\n')").to_display_string(), "\"\"");
}

// --- pwsh: missing exact values ---
#[test]
fn pwsh_int_negative_123() {
    assert_eq!(eval("repr_pwsh(-123)").to_display_string(), "-123");
}
#[test]
fn pwsh_list_of_ints_3() {
    assert_eq!(
        eval("repr_pwsh([1, 2, 3])").to_display_string(),
        "@(1, 2, 3)"
    );
}
#[test]
fn pwsh_range_expr_as_list() {
    let mut st = openjd_expr::SymbolTable::new();
    st.set(
        "Frames",
        openjd_expr::ExprValue::RangeExpr("1-3".parse::<openjd_expr::RangeExpr>().unwrap()),
    )
    .unwrap();
    let r = openjd_expr::ParsedExpression::new("repr_pwsh(list(Frames))")
        .and_then(|p| p.evaluate(&st))
        .unwrap();
    assert_eq!(r.to_display_string(), "@(1, 2, 3)");
}

// --- pwsh backtick with nworld (Python test uses hello`nworld) ---
#[test]
fn pwsh_string_backtick_nworld() {
    assert_eq!(
        eval("repr_pwsh('hello`nworld')").to_display_string(),
        "'hello`nworld'"
    );
}

// --- pwsh float 5.0 ---
#[test]
fn pwsh_float_5_0() {
    assert_eq!(eval("repr_pwsh(5.0)").to_display_string(), "5.0");
}

// --- re_search method syntax with group ---
#[test]
fn re_search_method_syntax_with_group() {
    assert_eq!(
        eval(r#"'test123'.re_search(r'(\d+)')"#).to_display_string(),
        r#"["123", "123"]"#
    );
}

// --- re_sub method syntax exact ---
#[test]
fn re_sub_method_syntax_hello() {
    assert_eq!(
        eval(r#"'hello'.re_sub(r'l+', 'L')"#).to_display_string(),
        "heLo"
    );
}

#[test]
fn regex_in_list_comprehension_uses_shared_cache() {
    // Regex pattern is the same on every iteration — with cache sharing,
    // it's compiled once. Without sharing, it would be compiled per iteration.
    let result =
        eval(r"[x for x in ['shot_01', 'bg', 'shot_02', 'ref'] if re_search(x, 'shot') != null]");
    assert_eq!(result.list_len(), Some(2));
}

// --- repr_cmd delayed expansion: ! is escaped as ^^! ---
// repr_cmd escapes ! as ^^! as a best-effort defense against EnableDelayedExpansion
// contexts in cmd.exe (see spec §2.2.6).
#[test]
fn cmd_string_exclamation_escaped() {
    assert_eq!(
        eval("repr_cmd('hello!')").to_display_string(),
        "\"hello^^!\""
    );
}
#[test]
fn cmd_string_exclamation_variable_escaped() {
    assert_eq!(
        eval("repr_cmd('!PATH!')").to_display_string(),
        "\"^^!PATH^^!\""
    );
}
#[test]
fn cmd_string_exclamation_with_caret_escaped() {
    // Both ^ and ! require escaping: ^ → ^^, ! → ^^!.
    assert_eq!(
        eval("repr_cmd('a ^ b!')").to_display_string(),
        "\"a ^^ b^^!\""
    );
}
#[test]
fn cmd_string_only_exclamation_is_quoted_and_escaped() {
    assert_eq!(eval("repr_cmd('!')").to_display_string(), "\"^^!\"");
}

// === F1: repr_sh must propagate errors from shlex on null bytes ===

#[test]
fn repr_sh_string_with_null_byte_returns_error() {
    // A string containing a null byte cannot be shell-quoted.
    // repr_sh should return an error, not silently produce an empty string.
    let mut st = SymbolTable::new();
    st.set("Param.S", ExprValue::String("hello\0world".into()))
        .unwrap();
    let parsed = ParsedExpression::new("repr_sh(Param.S)").unwrap();
    let result = parsed.evaluate(&st);
    assert!(result.is_err(), "repr_sh on null-byte string should error");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("shell-quote"),
        "error should mention shell-quoting, got: {msg}"
    );
}

#[test]
fn repr_sh_path_with_null_byte_returns_error() {
    let mut st = SymbolTable::new();
    st.set(
        "Param.P",
        ExprValue::new_path("/tmp/bad\0path", PathFormat::Posix),
    )
    .unwrap();
    let parsed = ParsedExpression::new("repr_sh(Param.P)").unwrap();
    let result = parsed.evaluate(&st);
    assert!(result.is_err(), "repr_sh on null-byte path should error");
}

#[test]
fn repr_sh_list_with_null_byte_returns_error() {
    // A list containing a string with a null byte should also error.
    let mut st = SymbolTable::new();
    st.set("Param.S", ExprValue::String("has\0null".into()))
        .unwrap();
    let parsed = ParsedExpression::new("repr_sh([Param.S])").unwrap();
    let result = parsed.evaluate(&st);
    assert!(
        result.is_err(),
        "repr_sh on list with null-byte string should error"
    );
}

#[test]
fn center_odd_padding_matches_python_bias() {
    assert_eq!(eval("center('ab', 5)").to_display_string(), "  ab ");
    assert_eq!(eval("center('abc', 6)").to_display_string(), " abc  ");
    assert_eq!(eval("center('a', 4)").to_display_string(), " a  ");
}

#[test]
fn padding_width_counts_characters_but_budgets_bytes() {
    assert_eq!(eval("center('界', 4)").to_display_string(), " 界  ");
    assert_eq!(eval("ljust('界', 4)").to_display_string(), "界   ");
    assert_eq!(eval("rjust('界', 4)").to_display_string(), "   界");
    assert_eq!(eval("zfill('界', 4)").to_display_string(), "000界");
}

#[test]
fn negative_padding_width_returns_input_unchanged() {
    assert_eq!(eval("center('界', -1)").to_display_string(), "界");
    assert_eq!(eval("ljust('界', -1)").to_display_string(), "界");
    assert_eq!(eval("rjust('界', -1)").to_display_string(), "界");
    assert_eq!(eval("zfill('界', -1)").to_display_string(), "界");
}

#[test]
fn pwsh_repr_preserves_nested_list_item_structure() {
    assert_eq!(
        eval("repr_pwsh([[1, 2], [3]])").to_display_string(),
        "@(@(1, 2), @(3))"
    );
}

#[test]
fn pwsh_repr_nested_string_lists_quote_elements() {
    assert_eq!(
        eval(r#"repr_pwsh([["it's", "b"], ["c d"]])"#).to_display_string(),
        "@(@('it''s', 'b'), @('c d'))"
    );
}

#[test]
fn pwsh_repr_single_nested_list_uses_unary_comma() {
    // `@(@(1, 2))` would flatten to `@(1, 2)` in PowerShell; the unary
    // comma preserves the one-element outer array.
    assert_eq!(
        eval("repr_pwsh([[1, 2]])").to_display_string(),
        "@(,@(1, 2))"
    );
}

#[test]
fn repr_cmd_accepts_path_lists_via_string_coercion() {
    assert_eq!(
        eval("repr_cmd([path('hello world'), path('output')])").to_display_string(),
        "\"hello world\" output"
    );
}

#[test]
fn repr_cmd_accepts_scalar_path_via_exact_overload() {
    assert_eq!(
        eval("repr_cmd(path('hello world'))").to_display_string(),
        "\"hello world\""
    );
}

#[test]
fn repr_cmd_rejects_unsupported_lists() {
    assert_err(
        "repr_cmd([[1, 2], [3]])",
        &[
            "No matching signature for repr_cmd(list[list[int]])\n",
            "  repr_cmd([[1, 2], [3]])\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
    assert_err(
        "repr_cmd([1, 2, 3])",
        &[
            "No matching signature for repr_cmd(list[int])\n",
            "  repr_cmd([1, 2, 3])\n",
            "  ^~~~~~~~~~~~~~~~~~~",
        ],
    );
}

#[test]
fn repr_sh_rejects_nested_lists() {
    assert_err(
        "repr_sh([[1, 2], [3]])",
        &[
            "No matching signature for repr_sh(list[list[int]])\n",
            "  repr_sh([[1, 2], [3]])\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
