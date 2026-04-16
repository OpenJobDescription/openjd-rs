// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_strings.py

use openjd_expr::{evaluate_expression, ExprValue, ParsedExpression, PathFormat, SymbolTable};

#[allow(dead_code)]
fn eval(expr: &str) -> ExprValue {
    evaluate_expression(expr, &SymbolTable::new()).unwrap()
}

#[allow(dead_code)]
fn eval_fmt(expr: &str, fmt: PathFormat) -> ExprValue {
    let parsed = ParsedExpression::new(expr).unwrap();
    let st = SymbolTable::new();
    let symtabs = [&st];
    let mut ev = parsed.evaluator(&symtabs).with_path_format(fmt);
    ev.evaluate(&parsed.ast).unwrap()
}

#[allow(dead_code)]
fn eval_err(expr: &str) -> String {
    evaluate_expression(expr, &SymbolTable::new())
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
    let mut ev = parsed
        .evaluator(&symtabs)
        .with_path_format(PathFormat::Posix);
    ev.evaluate(&parsed.ast).unwrap()
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
            "  ^~~~~~~~~~~~~~~~~~~~~~~",
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
            "  ^~~~~~~~~~~~~~~~~~~~~~~~",
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
        &["Invalid regex: regex parse error"],
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
    assert_err(
        "re_search('aa', r'(a)\\1')",
        &[
            "Unsupported regex feature: backreferences\n",
            "  re_search('aa', r'(a)\\1')\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
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
        openjd_expr::ExprValue::Path {
            value: "/tmp/file.txt".into(),
            format: openjd_expr::PathFormat::Posix,
        },
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
                openjd_expr::ExprValue::Path {
                    value: "/a".into(),
                    format: openjd_expr::PathFormat::Posix,
                },
                openjd_expr::ExprValue::Path {
                    value: "/b".into(),
                    format: openjd_expr::PathFormat::Posix,
                },
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
            vec![openjd_expr::ExprValue::Path {
                value: "/a".into(),
                format: openjd_expr::PathFormat::Posix,
            }],
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
        openjd_expr::ExprValue::Path {
            value: "/tmp/file.txt".into(),
            format: openjd_expr::PathFormat::Posix,
        },
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
    assert!(eval("repr_cmd('a\\nb')").to_display_string().contains("\""));
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
        openjd_expr::ExprValue::Path {
            value: "/data/archive.tar.gz".into(),
            format: openjd_expr::PathFormat::Posix,
        },
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
            "  ^~~~~~~~~~~~~~~~~~~~~~~~",
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
    assert!(eval("repr_cmd('a\\rb')").to_display_string().contains("\""));
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
    let r = openjd_expr::evaluate_expression("needle in haystack", &st).unwrap();
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
    assert_eq!(
        eval(r#"re_search('test\\1', r'\\1')"#).to_display_string(),
        r#"["\1"]"#
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
    assert_eq!(eval(r"repr_cmd('a\nb')").to_display_string(), "\"a\nb\"");
}
#[test]
fn cmd_string_carriage_return_exact() {
    assert_eq!(eval(r"repr_cmd('a\rb')").to_display_string(), "\"a\rb\"");
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
    let r = openjd_expr::evaluate_expression("repr_pwsh(list(Frames))", &st).unwrap();
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
