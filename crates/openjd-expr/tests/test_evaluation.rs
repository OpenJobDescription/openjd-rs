// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test_parsing.py

use openjd_expr::{ExprValue, ParsedExpression, SymbolTable};

#[allow(dead_code)]
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

#[allow(dead_code)]
fn eval_ok(expr: &str) -> bool {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_ok()
}

#[allow(dead_code)]
fn eval_fails(expr: &str) -> bool {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_err()
}
fn parse_ok(expr: &str) -> bool {
    ParsedExpression::new(expr).is_ok()
}

#[allow(dead_code)]
fn parse_fails(expr: &str) -> bool {
    ParsedExpression::new(expr).is_err()
}
fn eval_err_msg(expr: &str) -> String {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .message()
}

fn assert_err(expr: &str, expected: &[&str]) {
    let e = ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

// === TestContextualKeywords ===
macro_rules! keyword_attr_test {
    ($name:ident, $kw:expr) => {
        #[test]
        fn $name() {
            let mut st = SymbolTable::new();
            st.set(&format!("Param.{}", $kw), ExprValue::Int(42))
                .unwrap();
            assert_eq!(
                eval_with(&format!("Param.{}", $kw), &st).to_display_string(),
                "42"
            );
        }
    };
}
keyword_attr_test!(keyword_if, "if");
keyword_attr_test!(keyword_else, "else");
keyword_attr_test!(keyword_and, "and");
keyword_attr_test!(keyword_or, "or");
keyword_attr_test!(keyword_not, "not");
keyword_attr_test!(keyword_for, "for");
keyword_attr_test!(keyword_in, "in");
keyword_attr_test!(keyword_true, "True");
keyword_attr_test!(keyword_false, "False");
keyword_attr_test!(keyword_none, "None");

#[test]
fn keyword_in_expression_context() {
    let mut st = SymbolTable::new();
    st.set("Param.if", ExprValue::Int(10)).unwrap();
    st.set("Param.else", ExprValue::Int(20)).unwrap();
    assert_eq!(
        eval_with("Param.if + Param.else", &st).to_display_string(),
        "30"
    );
}

#[test]
fn keyword_with_conditional() {
    let mut st = SymbolTable::new();
    st.set("Param.if", ExprValue::Int(100)).unwrap();
    assert_eq!(
        eval_with("Param.if if True else 0", &st).to_display_string(),
        "100"
    );
}

#[test]
fn keyword_multiline_list() {
    let mut st = SymbolTable::new();
    st.set("Param.if", ExprValue::Int(10)).unwrap();
    st.set("Param.else", ExprValue::Int(20)).unwrap();
    let r = eval_with(
        "[\n            Param.if,\n            Param.else\n        ]",
        &st,
    );
    assert!(r.is_list());
}

#[test]
fn keyword_multiline_parens() {
    let mut st = SymbolTable::new();
    st.set("Param.if", ExprValue::Int(10)).unwrap();
    st.set("Param.else", ExprValue::Int(20)).unwrap();
    assert_eq!(
        eval_with(
            "(\n            Param.if +\n            Param.else\n        )",
            &st
        )
        .to_display_string(),
        "30"
    );
}

// === TestWhitespaceHandling ===
#[test]
fn leading_space() {
    assert_eq!(eval(" 1 + 2").to_display_string(), "3");
}
#[test]
fn leading_tab() {
    assert_eq!(eval("\t1 + 2").to_display_string(), "3");
}
#[test]
fn leading_trailing_ws() {
    assert_eq!(eval("  \t  3 * 4  \n").to_display_string(), "12");
}
#[test]
fn multiline_parens() {
    assert_eq!(
        eval("(\n            1 + 2 +\n            3\n        )").to_display_string(),
        "6"
    );
}
#[test]
fn multiline_list() {
    let r = eval("[\n            1,\n            2,\n            3\n        ]");
    assert!(r.is_list());
}
#[test]
fn multiline_conditional() {
    assert_eq!(
        eval("\n            1 if True else 2\n        ").to_display_string(),
        "1"
    );
}

// === TestSyntaxErrors ===
#[test]
fn invalid_syntax() {
    assert!(eval_err_msg("1 +").contains("Syntax error"));
}
#[test]
fn unclosed_paren() {
    assert!(eval_err_msg("(1 + 2").contains("Syntax error"));
}
#[test]
fn unclosed_bracket() {
    assert!(eval_err_msg("[1, 2, 3").contains("Syntax error"));
}
#[test]
fn colon_only() {
    assert!(eval_fails(":"));
}

#[test]
fn chained_comparison() {
    assert_eq!(eval("1 < 2 < 3").to_display_string(), "true");
    assert_eq!(eval("1 < 2 < 2").to_display_string(), "false");
    assert_eq!(eval("3 > 2 > 1").to_display_string(), "true");
    assert_eq!(eval("1 <= 2 <= 3").to_display_string(), "true");
    assert_eq!(eval("3 >= 2 >= 1").to_display_string(), "true");
    assert_eq!(eval("1 < 2 <= 2").to_display_string(), "true");
    assert_eq!(eval("1 <= 2 < 3").to_display_string(), "true");
    assert_eq!(eval("3 > 2 >= 2").to_display_string(), "true");
    assert_eq!(eval("3 >= 2 > 1").to_display_string(), "true");
}

// === TestDunderMethodsRejected ===
#[test]
fn dunder_add() {
    assert_err("(1).__add__(2)", &["Cannot call '__add__' directly\n"]);
}
#[test]
fn dunder_sub() {
    assert_err("(5).__sub__(3)", &["Cannot call '__sub__' directly\n"]);
}
#[test]
fn dunder_mul() {
    assert_err("(6).__mul__(7)", &["Cannot call '__mul__' directly\n"]);
}
#[test]
fn dunder_truediv() {
    assert_err(
        "(10).__truediv__(3)",
        &["Cannot call '__truediv__' directly\n"],
    );
}
#[test]
fn dunder_floordiv() {
    assert_err(
        "(10).__floordiv__(3)",
        &["Cannot call '__floordiv__' directly\n"],
    );
}
#[test]
fn dunder_mod() {
    assert_err("(10).__mod__(3)", &["Cannot call '__mod__' directly\n"]);
}
#[test]
fn dunder_neg() {
    assert_err("(-5).__neg__()", &["Cannot call '__neg__' directly\n"]);
}
#[test]
fn dunder_lt() {
    assert_err("(1).__lt__(2)", &["Cannot call '__lt__' directly\n"]);
}
#[test]
fn dunder_eq() {
    assert_err("(1).__eq__(1)", &["Cannot call '__eq__' directly\n"]);
}
#[test]
fn dunder_getitem() {
    assert_err(
        "[1,2,3].__getitem__(0)",
        &["Cannot call '__getitem__' directly\n"],
    );
}
#[test]
fn dunder_contains() {
    assert_err(
        "[1,2,3].__contains__(2)",
        &["Cannot call '__contains__' directly\n"],
    );
}

// === TestNumericLiteralFormats ===
#[test]
fn decimal_int() {
    assert_eq!(eval("42").to_display_string(), "42");
    assert_eq!(eval("0").to_display_string(), "0");
}
#[test]
fn hex_lower() {
    assert_eq!(eval("0x2a").to_display_string(), "42");
}
#[test]
fn hex_upper() {
    assert_eq!(eval("0X2A").to_display_string(), "42");
}
#[test]
fn hex_mixed() {
    assert_eq!(eval("0xDeAdBeEf").to_display_string(), "3735928559");
}
#[test]
fn octal_lower() {
    assert_eq!(eval("0o52").to_display_string(), "42");
}
#[test]
fn octal_upper() {
    assert_eq!(eval("0O52").to_display_string(), "42");
}
#[test]
fn old_octal_rejected() {
    assert_err("0777", &["Syntax error: Invalid decimal integer literal\n"]);
}
#[test]
fn leading_zero_rejected() {
    assert_err("007", &["Syntax error: Invalid decimal integer literal\n"]);
}
#[test]
fn double_zero() {
    assert_eq!(eval("00").to_display_string(), "0");
}
#[test]
fn binary_lower() {
    assert_eq!(eval("0b101010").to_display_string(), "42");
}
#[test]
fn binary_upper() {
    assert_eq!(eval("0B101010").to_display_string(), "42");
}
#[test]
fn underscore_decimal() {
    assert_eq!(eval("1_000_000").to_display_string(), "1000000");
}
#[test]
fn underscore_hex() {
    assert_eq!(eval("0xFF_FF").to_display_string(), "65535");
}
#[test]
fn underscore_octal() {
    assert_eq!(eval("0o7_7_7").to_display_string(), "511");
}
#[test]
fn underscore_binary() {
    assert_eq!(eval("0b1010_1010").to_display_string(), "170");
}
#[test]
fn underscore_start_is_var() {
    assert!(eval_err_msg("_123").contains("Undefined variable"));
}
#[test]
fn underscore_end_rejected() {
    assert_err("123_", &["Syntax error: Unexpected token"]);
}
#[test]
fn double_underscore_rejected() {
    assert_err("1__000", &["Syntax error: Unexpected token"]);
}
#[test]
fn underscore_after_hex_prefix() {
    assert_eq!(eval("0x_FF").to_display_string(), "255");
}
#[test]
fn underscore_after_octal_prefix() {
    assert_eq!(eval("0o_77").to_display_string(), "63");
}
#[test]
fn underscore_after_binary_prefix() {
    assert_eq!(eval("0b_10").to_display_string(), "2");
}
#[test]
fn decimal_float() {
    assert_eq!(eval("3.14").to_display_string(), "3.14");
}
#[test]
fn float_no_int_part() {
    assert_eq!(eval(".5").to_display_string(), "0.5");
}
#[test]
fn float_no_dec_part() {
    assert_eq!(eval("3.").to_display_string(), "3.0");
}
#[test]
fn sci_lower() {
    assert!(matches!(eval("1e10"), ExprValue::Float(_)));
}
#[test]
fn sci_upper() {
    assert!(matches!(eval("1E10"), ExprValue::Float(_)));
}
#[test]
fn sci_positive() {
    assert!(matches!(eval("1e+10"), ExprValue::Float(_)));
}
#[test]
fn sci_negative() {
    assert!(matches!(eval("1.5e-3"), ExprValue::Float(_)));
}
#[test]
fn underscore_float_int_part() {
    assert_eq!(eval("1_000.5").to_display_string(), "1000.5");
}
#[test]
fn underscore_float_dec_part() {
    assert_eq!(eval("3.141_592").to_display_string(), "3.141592");
}
#[test]
fn underscore_exponent() {
    assert!(matches!(eval("1e1_0"), ExprValue::Float(_)));
}
#[test]
fn underscore_adj_decimal_rejected() {
    assert_err("1_.5", &["Syntax error: Unexpected token"]);
}
#[test]
fn underscore_after_decimal_rejected() {
    assert_err("1._5", &["Syntax error:"]);
}
#[test]
fn underscore_adj_exponent_rejected() {
    assert_err("1_e10", &["Syntax error: Unexpected token"]);
}
#[test]
fn underscore_after_exponent_rejected() {
    assert_err("1e_10", &["Syntax error: Unexpected token"]);
}

// === TestUnsupportedPythonFeatures ===
// Statements
#[test]
fn stmt_assign() {
    assert!(eval_fails("x = 1"));
}
#[test]
fn stmt_walrus() {
    assert!(eval_fails("x := 1"));
}
#[test]
fn stmt_del() {
    assert!(eval_fails("del x"));
}
#[test]
fn stmt_pass() {
    assert!(eval_fails("pass"));
}
#[test]
fn stmt_return() {
    assert!(eval_fails("return 1"));
}
#[test]
fn stmt_import() {
    assert!(eval_fails("import os"));
}
// Compound statements
#[test]
fn stmt_if() {
    assert!(eval_fails("if True: 1"));
}
#[test]
fn stmt_for() {
    assert!(eval_fails("for x in []: pass"));
}
#[test]
fn stmt_while() {
    assert!(eval_fails("while True: pass"));
}
#[test]
fn stmt_def() {
    assert!(eval_fails("def f(): pass"));
}
#[test]
fn stmt_class() {
    assert!(eval_fails("class C: pass"));
}
// Unsupported expressions — covered by test_ast_validation.rs
#[test]
fn raw_string_supported() {
    assert_eq!(eval(r"r'\n'").to_display_string(), "\\n");
}
// Unsupported operators — covered by test_ast_validation.rs
// Unavailable builtins
#[test]
fn builtin_eval() {
    assert_err("eval('1')", &["Unknown function: 'eval'\n"]);
}
#[test]
fn builtin_exec() {
    assert_err("exec('1')", &["Unknown function: 'exec'\n"]);
}
#[test]
fn builtin_open() {
    assert_err("open('file')", &["Unknown function: 'open'\n"]);
}
#[test]
fn builtin_print() {
    assert_err("print(1)", &["Unknown function: 'print'\n"]);
}
#[test]
fn builtin_type() {
    assert_err("type(1)", &["Unknown function: 'type'\n"]);
}
#[test]
fn builtin_globals() {
    assert_err("globals()", &["Unknown function: 'globals'\n"]);
}
#[test]
fn builtin_locals() {
    assert_err("locals()", &["Unknown function: 'locals'\n"]);
}
#[test]
fn builtin_dir() {
    assert_err("dir()", &["Unknown function: 'dir'\n"]);
}
#[test]
fn builtin_id() {
    assert_err("id(1)", &["Unknown function: 'id'\n"]);
}
#[test]
fn builtin_hash() {
    assert_err("hash(1)", &["Unknown function: 'hash'\n"]);
}
#[test]
fn builtin_dict() {
    assert_err("dict()", &["Unknown function: 'dict'\n"]);
}
#[test]
fn builtin_set() {
    assert_err("set()", &["Unknown function: 'set'\n"]);
}
#[test]
fn builtin_tuple() {
    assert_err("tuple()", &["Unknown function: 'tuple'\n"]);
}
#[test]
fn builtin_complex() {
    assert_err("complex(1, 2)", &["Unknown function: 'complex'\n"]);
}
#[test]
fn builtin_oct() {
    assert_err("oct(8)", &["Unknown function: 'oct'\n"]);
}
#[test]
fn builtin_hex() {
    assert_err("hex(16)", &["Unknown function: 'hex'\n"]);
}
#[test]
fn builtin_bin() {
    assert_err("bin(2)", &["Unknown function: 'bin'\n"]);
}
#[test]
fn builtin_ord() {
    assert_err("ord('a')", &["Unknown function: 'ord'\n"]);
}
#[test]
fn builtin_chr() {
    assert_err("chr(97)", &["Unknown function: 'chr'\n"]);
}
#[test]
fn builtin_help() {
    assert_err("help()", &["Unknown function: 'help'\n"]);
}

// === TestASTNodeRejection — covered by test_ast_validation.rs ===

// Allowed syntax
#[test]
fn allowed_add() {
    assert!(parse_ok("1 + 2"));
}
#[test]
fn allowed_list() {
    assert!(parse_ok("[1, 2, 3]"));
}
#[test]
fn allowed_comp() {
    assert!(parse_ok("[x for x in [1, 2] if x > 0]"));
}
#[test]
fn allowed_method() {
    assert!(parse_ok("'hello'.upper()"));
}
#[test]
fn allowed_ternary() {
    assert!(parse_ok("x if True else y"));
}
#[test]
fn allowed_not() {
    assert!(parse_ok("not True"));
}
#[test]
fn allowed_neg() {
    assert!(parse_ok("-5"));
}
#[test]
fn allowed_pos() {
    assert!(parse_ok("+5"));
}
#[test]
fn allowed_chain() {
    assert!(parse_ok("1 < 2 < 3"));
}
#[test]
fn allowed_in() {
    assert!(parse_ok("x in [1, 2]"));
}
#[test]
fn allowed_not_in() {
    assert!(parse_ok("x not in [1, 2]"));
}
#[test]
fn allowed_eq() {
    assert!(parse_ok("1 == 2"));
}
#[test]
fn allowed_ne() {
    assert!(parse_ok("1 != 2"));
}
#[test]
fn allowed_pow() {
    assert!(parse_ok("2 ** 3"));
}
#[test]
fn allowed_subscript() {
    assert!(parse_ok("[1, 2][0]"));
}
#[test]
fn allowed_slice() {
    assert!(parse_ok("[1, 2][0:1]"));
}
#[test]
fn allowed_string() {
    assert!(parse_ok("'hello'"));
}
#[test]
fn allowed_raw_string() {
    assert!(parse_ok(r"r'raw\n'"));
}
#[test]
fn allowed_triple() {
    assert!(parse_ok("'''triple'''"));
}
#[test]
fn allowed_hex() {
    assert!(parse_ok("0x2A"));
}
#[test]
fn allowed_octal() {
    assert!(parse_ok("0o52"));
}
#[test]
fn allowed_binary() {
    assert!(parse_ok("0b101010"));
}
#[test]
fn allowed_underscore_num() {
    assert!(parse_ok("1_000"));
}
#[test]
fn allowed_sci() {
    assert!(parse_ok("1.5e3"));
}
#[test]
fn allowed_true() {
    assert!(parse_ok("True"));
}
#[test]
fn allowed_false() {
    assert!(parse_ok("False"));
}
#[test]
fn allowed_none() {
    assert!(parse_ok("None"));
}
#[test]
fn allowed_null() {
    assert!(parse_ok("null"));
}
#[test]
fn allowed_true_lower() {
    assert!(parse_ok("true"));
}
#[test]
fn allowed_false_lower() {
    assert!(parse_ok("false"));
}
#[test]
fn allowed_empty_list() {
    assert!(parse_ok("[]"));
}
#[test]
fn allowed_trailing_comma() {
    assert!(parse_ok("[1,]"));
}

// === TestLoopVariableValidation ===
#[test]
fn loop_var_lowercase() {
    assert!(parse_ok("[x for x in [1, 2]]"));
}
#[test]
fn loop_var_underscore() {
    assert!(parse_ok("[_x for _x in [1, 2]]"));
}
// Note: uppercase loop var rejection is a Python-specific validation

// === Missing tests ported from Python test_parsing.py ===

// TestSyntaxErrors: method on int literal
#[test]
fn method_on_int_literal_without_parens() {
    assert_err("42.zfill(5)", &["Syntax error:"]);
}
#[test]
fn method_on_int_literal_with_parens() {
    assert_eq!(eval("(42).zfill(5)").to_display_string(), "00042");
}

// TestDunderMethodsRejected: missing entries
#[test]
fn dunder_not() {
    assert_err("True.__not__()", &["Cannot call '__not__' directly\n"]);
}
#[test]
fn dunder_property_dunder() {
    assert_err(
        "path(\"/a/b\").__property_name__()",
        &["Cannot call '__property_name__' directly\n"],
    );
}

// TestNumericLiteralFormats: scientific with decimal
#[test]
fn sci_with_decimal() {
    assert!(matches!(eval("6.022e23"), ExprValue::Float(_)));
}

// TestUnsupportedPythonFeatures: missing statements
#[test]
fn stmt_augmented_assign() {
    assert!(eval_fails("x += 1"));
}
#[test]
fn stmt_break() {
    assert!(eval_fails("break"));
}
#[test]
fn stmt_continue() {
    assert!(eval_fails("continue"));
}
#[test]
fn stmt_raise() {
    assert!(eval_fails("raise Exception()"));
}
#[test]
fn stmt_yield() {
    assert!(eval_fails("yield 1"));
}
#[test]
fn stmt_yield_from() {
    assert!(eval_fails("yield from [1]"));
}
#[test]
fn stmt_global() {
    assert!(eval_fails("global x"));
}
#[test]
fn stmt_nonlocal() {
    assert!(eval_fails("nonlocal x"));
}
#[test]
fn stmt_assert() {
    assert!(eval_fails("assert True"));
}
#[test]
fn stmt_from_import() {
    assert!(eval_fails("from os import path"));
}
#[test]
fn stmt_type_alias() {
    assert!(eval_fails("type X = int"));
}

// TestUnsupportedPythonFeatures: missing compound statements
#[test]
fn stmt_with() {
    assert!(eval_fails("with open('f'): pass"));
}
#[test]
fn stmt_try_except() {
    assert!(eval_fails("try: 1\nexcept: 2"));
}
#[test]
fn stmt_async_def() {
    assert!(eval_fails("async def f(): pass"));
}
#[test]
fn stmt_async_for() {
    assert!(eval_fails("async for x in []: pass"));
}
#[test]
fn stmt_async_with() {
    assert!(eval_fails("async with x: pass"));
}
#[test]
fn stmt_match() {
    assert!(eval_fails("match x:\n  case 1: pass"));
}

// TestUnsupportedPythonFeatures: t-string
#[test]
fn expr_tstring() {
    assert_err("t'{x}'", &["Unsupported expression type\n"]);
}

// TestUnsupportedPythonFeatures: missing builtins
#[test]
fn builtin_compile() {
    assert_err(
        "compile('1', '', 'eval')",
        &["Unknown function: 'compile'\n"],
    );
}
#[test]
fn builtin_input() {
    assert_err("input()", &["Unknown function: 'input'\n"]);
}
#[test]
fn builtin_dunder_import() {
    assert_err("__import__('os')", &["Cannot call '__import__' directly\n"]);
}
#[test]
fn builtin_vars() {
    assert_err("vars()", &["Unknown function: 'vars'\n"]);
}
#[test]
fn builtin_isinstance() {
    assert_err("isinstance(1, int)", &["Undefined variable"]);
}
#[test]
fn builtin_issubclass() {
    assert_err("issubclass(int, object)", &["Undefined variable"]);
}
#[test]
fn builtin_getattr() {
    assert_err("getattr(1, 'x')", &["Unknown function: 'getattr'\n"]);
}
#[test]
fn builtin_setattr() {
    assert_err("setattr(1, 'x', 1)", &["Unknown function: 'setattr'\n"]);
}
#[test]
fn builtin_delattr() {
    assert_err("delattr(1, 'x')", &["Unknown function: 'delattr'\n"]);
}
#[test]
fn builtin_hasattr() {
    assert_err("hasattr(1, 'x')", &["Unknown function: 'hasattr'\n"]);
}
#[test]
fn builtin_callable() {
    assert_err("callable(1)", &["Unknown function: 'callable'\n"]);
}
#[test]
fn builtin_enumerate() {
    assert_err("enumerate([1])", &["Unknown function: 'enumerate'\n"]);
}
#[test]
fn builtin_zip() {
    assert_err("zip([1], [2])", &["Unknown function: 'zip'\n"]);
}
#[test]
fn builtin_map() {
    assert_err("map(str, [1])", &["Undefined variable"]);
}
#[test]
fn builtin_filter() {
    assert_err("filter(None, [1])", &["Unknown function: 'filter'\n"]);
}
#[test]
fn builtin_reduce() {
    assert_err(
        "reduce(lambda a,b: a+b, [1])",
        &["Lambda expressions are not supported\n"],
    );
}
#[test]
fn builtin_iter() {
    assert_err("iter([1])", &["Unknown function: 'iter'\n"]);
}
#[test]
fn builtin_next() {
    assert_err("next(iter([1]))", &["Unknown function: 'iter'\n"]);
}
#[test]
fn builtin_slice() {
    assert_err("slice(1)", &["Unknown function: 'slice'\n"]);
}
#[test]
fn builtin_object() {
    assert_err("object()", &["Unknown function: 'object'\n"]);
}
#[test]
fn builtin_super() {
    assert_err("super()", &["Unknown function: 'super'\n"]);
}
#[test]
fn builtin_property() {
    assert_err("property()", &["Unknown function: 'property'\n"]);
}
#[test]
fn builtin_staticmethod() {
    assert_err(
        "staticmethod(lambda: 1)",
        &["Lambda expressions are not supported\n"],
    );
}
#[test]
fn builtin_classmethod() {
    assert_err(
        "classmethod(lambda: 1)",
        &["Lambda expressions are not supported\n"],
    );
}
#[test]
fn builtin_memoryview() {
    assert_err("memoryview(b'')", &["Byte string"]);
}
#[test]
fn builtin_bytearray() {
    assert_err("bytearray()", &["Unknown function: 'bytearray'\n"]);
}
#[test]
fn builtin_bytes() {
    assert_err("bytes()", &["Unknown function: 'bytes'\n"]);
}
#[test]
fn builtin_frozenset() {
    assert_err("frozenset()", &["Unknown function: 'frozenset'\n"]);
}
#[test]
fn builtin_ascii() {
    assert_err("ascii('a')", &["Unknown function: 'ascii'\n"]);
}
#[test]
fn builtin_format() {
    assert_err("format(1, 'd')", &["Unknown function: 'format'\n"]);
}
#[test]
fn builtin_pow_3arg() {
    assert_err("pow(2, 3, 5)", &["Unknown function: 'pow'\n"]);
}
#[test]
fn builtin_divmod() {
    assert_err("divmod(7, 3)", &["Unknown function: 'divmod'\n"]);
}
#[test]
fn builtin_breakpoint() {
    assert_err("breakpoint()", &["Unknown function: 'breakpoint'\n"]);
}
#[test]
fn builtin_exit() {
    assert_err("exit()", &["Unknown function: 'exit'\n"]);
}
#[test]
fn builtin_quit() {
    assert_err("quit()", &["Unknown function: 'quit'\n"]);
}

// TestASTNodeRejection: missing entries
#[test]
fn reject_keyword_arg() {
    assert_err("f(x=1)", &["Keyword arguments are not supported\n"]);
}
#[test]
fn reject_double_star_arg() {
    assert_err("f(**d)", &["Keyword arguments are not supported\n"]);
}

// TestLoopVariableValidation: uppercase rejected (validated at eval time in Rust)
#[test]
fn loop_var_uppercase_rejected() {
    assert_err(
        "[X for X in [1, 2]]",
        &["Loop variable 'X' must start with"],
    );
}

// ── String prefix rejection at parse time ──

#[test]
fn test_unicode_prefix_rejected_at_parse() {
    let result = ParsedExpression::new(r#"u"hello""#);
    assert!(
        result.is_err(),
        "u-strings should be rejected at parse time"
    );
    let err = result.err().unwrap();
    assert!(
        err.to_string()
            .contains("Unicode string prefix u'...' is not supported"),
        "got: {err}"
    );
}

#[test]
fn test_unicode_prefix_in_expression_rejected_at_parse() {
    let result = ParsedExpression::new(r#"1 + u"hello""#);
    assert!(
        result.is_err(),
        "u-strings in expressions should be rejected at parse time"
    );
    let err = result.err().unwrap();
    assert!(
        err.to_string()
            .contains("Unicode string prefix u'...' is not supported"),
        "got: {err}"
    );
}

#[test]
fn test_byte_string_rejected_at_parse() {
    let result = ParsedExpression::new(r#"b"hello""#);
    assert!(
        result.is_err(),
        "b-strings should be rejected at parse time"
    );
    let err = result.err().unwrap();
    assert!(err.to_string().contains("Byte string"), "got: {err}");
}

#[test]
fn test_byte_string_in_expression_rejected_at_parse() {
    let result = ParsedExpression::new(r#"1 + b"hello""#);
    assert!(
        result.is_err(),
        "b-strings in expressions should be rejected at parse time"
    );
    let err = result.err().unwrap();
    assert!(err.to_string().contains("Byte string"), "got: {err}");
}

#[test]
fn test_raw_string_allowed_at_parse() {
    let result = ParsedExpression::new(r#"r"hello""#);
    assert!(
        result.is_ok(),
        "r-strings should be allowed: {:?}",
        result.err()
    );
}

// ── Unsupported constructs rejected at parse time ──

#[test]
fn reject_walrus_at_parse() {
    let e = ParsedExpression::new("(x := 5)").err().unwrap();
    assert!(
        e.message().contains("Walrus operator"),
        "got: {}",
        e.message()
    );
}
#[test]
fn reject_lambda_at_parse() {
    let e = ParsedExpression::new("lambda x: x + 1").err().unwrap();
    assert!(e.message().contains("Lambda"), "got: {}", e.message());
}
#[test]
fn reject_lambda_no_args_at_parse() {
    let e = ParsedExpression::new("lambda: 1").err().unwrap();
    assert!(e.message().contains("Lambda"), "got: {}", e.message());
}
#[test]
fn reject_tuple_at_parse() {
    let e = ParsedExpression::new("(1, 2, 3)").err().unwrap();
    assert!(e.message().contains("Tuple"), "got: {}", e.message());
}
#[test]
fn reject_dict_literal_at_parse() {
    let e = ParsedExpression::new("{'a': 1}").err().unwrap();
    assert!(e.message().contains("Dict"), "got: {}", e.message());
}
#[test]
fn reject_set_literal_at_parse() {
    let e = ParsedExpression::new("{1, 2, 3}").err().unwrap();
    assert!(e.message().contains("Set"), "got: {}", e.message());
}
#[test]
fn reject_dict_comp_at_parse() {
    let e = ParsedExpression::new("{k: k for k in [1]}").err().unwrap();
    assert!(e.message().contains("Dict comp"), "got: {}", e.message());
}
#[test]
fn reject_set_comp_at_parse() {
    let e = ParsedExpression::new("{x for x in [1]}").err().unwrap();
    assert!(e.message().contains("Set comp"), "got: {}", e.message());
}
#[test]
fn reject_generator_at_parse() {
    let e = ParsedExpression::new("(x for x in [1])").err().unwrap();
    assert!(e.message().contains("Generator"), "got: {}", e.message());
}
#[test]
fn reject_fstring_at_parse() {
    let e = ParsedExpression::new("f'hello'").err().unwrap();
    assert!(e.message().contains("f-string"), "got: {}", e.message());
}
#[test]
fn reject_ellipsis_at_parse() {
    let e = ParsedExpression::new("...").err().unwrap();
    assert!(e.message().contains("Ellipsis"), "got: {}", e.message());
}
#[test]
fn reject_star_unpack_at_parse() {
    let e = ParsedExpression::new("[*[1, 2], 3]").err().unwrap();
    assert!(e.message().contains("Star"), "got: {}", e.message());
}
#[test]
fn reject_await_at_parse() {
    let e = ParsedExpression::new("await x").err().unwrap();
    assert!(e.message().contains("Await"), "got: {}", e.message());
}
#[test]
fn reject_bitwise_and_at_parse() {
    let e = ParsedExpression::new("5 & 3").err().unwrap();
    assert!(e.message().contains("Bitwise AND"), "got: {}", e.message());
}
#[test]
fn reject_bitwise_or_at_parse() {
    let e = ParsedExpression::new("5 | 3").err().unwrap();
    assert!(e.message().contains("Bitwise OR"), "got: {}", e.message());
}
#[test]
fn reject_bitwise_xor_at_parse() {
    let e = ParsedExpression::new("5 ^ 3").err().unwrap();
    assert!(e.message().contains("Bitwise XOR"), "got: {}", e.message());
}
#[test]
fn reject_bitwise_not_at_parse() {
    let e = ParsedExpression::new("~5").err().unwrap();
    assert!(e.message().contains("Bitwise NOT"), "got: {}", e.message());
}
#[test]
fn reject_left_shift_at_parse() {
    let e = ParsedExpression::new("5 << 1").err().unwrap();
    assert!(e.message().contains("Left shift"), "got: {}", e.message());
}
#[test]
fn reject_right_shift_at_parse() {
    let e = ParsedExpression::new("5 >> 1").err().unwrap();
    assert!(e.message().contains("Right shift"), "got: {}", e.message());
}
#[test]
fn reject_matmul_at_parse() {
    let e = ParsedExpression::new("x @ y").err().unwrap();
    assert!(
        e.message().contains("Matrix multiply"),
        "got: {}",
        e.message()
    );
}
#[test]
fn reject_is_operator_at_parse() {
    let e = ParsedExpression::new("x is None").err().unwrap();
    assert!(e.message().contains("'is'"), "got: {}", e.message());
}
#[test]
fn reject_is_not_operator_at_parse() {
    let e = ParsedExpression::new("x is not None").err().unwrap();
    assert!(e.message().contains("'is not'"), "got: {}", e.message());
}
#[test]
fn reject_keyword_arg_at_parse() {
    let e = ParsedExpression::new("f(x=1)").err().unwrap();
    assert!(e.message().contains("Keyword"), "got: {}", e.message());
}
#[test]
fn reject_double_star_arg_at_parse() {
    let e = ParsedExpression::new("f(**d)").err().unwrap();
    assert!(
        e.message().contains("not supported"),
        "got: {}",
        e.message()
    );
}

// ── Comprehension restrictions at parse time ──

#[test]
fn reject_multi_if_in_comprehension_at_parse() {
    let e = ParsedExpression::new("[x for x in [1, 2, 3] if x > 1 if x < 3]")
        .err()
        .unwrap();
    assert!(
        e.message().contains("Multiple 'if'"),
        "got: {}",
        e.message()
    );
}

#[test]
fn reject_uppercase_loop_var_at_parse() {
    let e = ParsedExpression::new("[X for X in [1, 2, 3]]")
        .err()
        .unwrap();
    assert!(
        e.message().contains("must start with a lowercase"),
        "got: {}",
        e.message()
    );
}
