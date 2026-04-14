// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_error_formatting.py
//!
//! Gold standard: every error test asserts the full multi-line error message
//! (message + expression line + caret indicator) as a single concatenated string.

use openjd_expr::*;

fn eval_err(expr: &str) -> String {
    evaluate_expression(expr, &SymbolTable::new()).unwrap_err().to_string()
}

fn eval_err_with(expr: &str, st: &SymbolTable) -> String {
    evaluate_expression(expr, st).unwrap_err().to_string()
}

fn assert_err(expr: &str, expected: &[&str]) {
    let e = eval_err(expr);
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

fn assert_err_with(expr: &str, st: &SymbolTable, expected: &[&str]) {
    let e = eval_err_with(expr, st);
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

// === TestErrorCaretPointers ===

#[test] fn type_error_in_middle() {
    assert_err("1 + int('bad') + 2", &[
        "Cannot convert 'bad' to int\n",
        "  1 + int('bad') + 2\n",
        "      ^~~~~~~~~~",
    ]);
}

#[test] fn type_error_at_start() {
    assert_err("int('bad') + 1 + 2", &[
        "Cannot convert 'bad' to int\n",
        "  int('bad') + 1 + 2\n",
        "  ^~~~~~~~~~",
    ]);
}

#[test] fn type_error_at_end() {
    assert_err("1 + 2 + int('bad')", &[
        "Cannot convert 'bad' to int\n",
        "  1 + 2 + int('bad')\n",
        "          ^~~~~~~~~~",
    ]);
}

#[test] fn operator_error_friendly_name() {
    assert_err("'hello' + 5", &[
        "Cannot use '+' operator with string and int\n",
        "  'hello' + 5\n",
        "  ~~~~~~~~^~~",
    ]);
}

#[test] fn operator_error_in_middle() {
    assert_err("1 + 'hello' + 5", &[
        "Cannot use '+' operator with int and string\n",
        "  1 + 'hello' + 5\n",
        "  ~~^~~~~~~~~",
    ]);
}

#[test] fn division_by_zero_in_middle() {
    assert_err("10 + 5 / 0 + 2", &[
        "Division by zero\n",
        "  10 + 5 / 0 + 2\n",
        "       ~~^~~",
    ]);
}

#[test] fn index_out_of_bounds_shows_length() {
    assert_err("[1,2,3][10]", &[
        "Index 10 out of bounds for list of length 3\n",
        "  [1,2,3][10]\n",
        "  ~~~~~~~^~~~",
    ]);
}

#[test] fn unknown_property_friendly_name() {
    assert_err("path('/a/b').unknown", &[
        "Cannot access attribute 'unknown' on path\n",
        "  path('/a/b').unknown\n",
        "  ~~~~~~~~~~~~~^~~~~~~",
    ]);
}

#[test] fn unknown_property_in_chain() {
    assert_err("path('/a/b').parent.unknown", &[
        "Cannot access attribute 'unknown' on path\n",
        "  path('/a/b').parent.unknown\n",
        "  ~~~~~~~~~~~~~~~~~~~~^~~~~~~",
    ]);
}

#[test] fn min_empty_list_error() {
    assert_err("1 + min([]) + 2", &[
        "min() requires a non-empty list\n",
        "  1 + min([]) + 2\n",
        "      ^~~~~~~",
    ]);
}

#[test] fn split_empty_separator() {
    assert_err("10 + 'x'.split('') + 5", &[
        "split failed: empty separator\n",
        "  10 + 'x'.split('') + 5\n",
        "       ~~~~^~~~~~~~~",
    ]);
}

#[test] fn deeply_nested_error() {
    assert_err("1 + (2 + (3 + int('x')))", &[
        "Cannot convert 'x' to int\n",
        "  1 + (2 + (3 + int('x')))\n",
        "                ^~~~~~~~",
    ]);
}

#[test] fn error_in_function_argument() {
    assert_err("min(1, int('x'), 3)", &[
        "Cannot convert 'x' to int\n",
        "  min(1, int('x'), 3)\n",
        "         ^~~~~~~~",
    ]);
}

#[test] fn error_in_condition() {
    assert_err("1 if int('x') else 2", &[
        "Cannot convert 'x' to int\n",
        "  1 if int('x') else 2\n",
        "       ^~~~~~~~",
    ]);
}

#[test] fn error_in_comprehension_body() {
    assert_err("[int('x') for i in [1,2]]", &[
        "Cannot convert 'x' to int\n",
        "  [int('x') for i in [1,2]]\n",
        "   ^~~~~~~~",
    ]);
}

#[test] fn error_in_comprehension_filter() {
    assert_err("[i for i in [1,2] if int('x')]", &[
        "Cannot convert 'x' to int\n",
        "  [i for i in [1,2] if int('x')]\n",
        "                       ^~~~~~~~",
    ]);
}

#[test] fn chained_method_error() {
    assert_err("'hello'.upper().nonexistent()", &[
        "Unknown function: 'nonexistent'\n",
        "  'hello'.upper().nonexistent()\n",
        "  ~~~~~~~~~~~~~~~~^~~~~~~~~~~~~",
    ]);
}

#[test] fn undefined_variable() {
    assert_err("X + 1", &[
        "Undefined variable: 'X'.\n",
        "  X + 1\n",
        "  ^",
    ]);
}

#[test] fn undefined_variable_with_suggestion() {
    // Param.Frane is close to Param.Frame — should suggest it
    let mut st = openjd_expr::SymbolTable::new();
    st.set("Param.Frame", openjd_expr::ExprValue::Int(1)).unwrap();
    st.set("Param.Scene", openjd_expr::ExprValue::String("forest".into())).unwrap();
    let result = openjd_expr::evaluate_expression("Param.Frane + 1", &st);
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Did you mean: Param.Frame"), "Expected suggestion in: {err}");
}

#[test] fn undefined_variable_no_suggestion_when_distant() {
    let mut st = openjd_expr::SymbolTable::new();
    st.set("Param.Frame", openjd_expr::ExprValue::Int(1)).unwrap();
    let result = openjd_expr::evaluate_expression("CompletelyWrong + 1", &st);
    let err = result.unwrap_err().to_string();
    assert!(!err.contains("Did you mean"), "Should not suggest for distant names: {err}");
}

#[test] fn float_literal_infinity() {
    assert_err("1e3000", &[
        "Float operation produced infinity\n",
        "  1e3000\n",
        "  ^~~~~~",
    ]);
}

#[test] fn float_literal_infinity_in_expression() {
    assert_err("1e3000 + 1", &[
        "Float operation produced infinity\n",
        "  1e3000 + 1\n",
        "  ^~~~~~",
    ]);
}

// === TestErrorCaretWithWhitespace ===

#[test] fn leading_whitespace_stripped() {
    assert_err("  1 + int('bad')", &[
        "Cannot convert 'bad' to int\n",
        "  1 + int('bad')\n",
        "      ^~~~~~~~~~",
    ]);
}

#[test] fn leading_whitespace_error_in_middle() {
    assert_err("  1 + int('bad') + 2", &[
        "Cannot convert 'bad' to int\n",
        "  1 + int('bad') + 2\n",
        "      ^~~~~~~~~~",
    ]);
}

// === Exact format tests ===

#[test] fn exact_division_by_zero_format() {
    assert_err("5 / 0", &[
        "Division by zero\n",
        "  5 / 0\n",
        "  ~~^~~",
    ]);
}

#[test] fn exact_int_conversion_error_format() {
    assert_err("int('bad')", &[
        "Cannot convert 'bad' to int\n",
        "  int('bad')\n",
        "  ^~~~~~~~~~",
    ]);
}

#[test] fn exact_modulo_by_zero_format() {
    assert_err("10 % 0", &[
        "Modulo by zero\n",
        "  10 % 0\n",
        "  ~~~^~~",
    ]);
}

#[test] fn exact_fail_format() {
    assert_err("fail(\"oops\")", &[
        "oops\n",
        "  fail(\"oops\")\n",
        "  ^~~~~~~~~~~~",
    ]);
}

#[test] fn exact_index_out_of_bounds_format() {
    assert_err("[1, 2, 3][10]", &[
        "Index 10 out of bounds for list of length 3\n",
        "  [1, 2, 3][10]\n",
        "  ~~~~~~~~~^~~~",
    ]);
}

#[test] fn exact_condition_must_be_bool() {
    assert_err("1 if 'hello' else 2", &[
        "  1 if 'hello' else 2\n",
        "       ^~~~~~~",
    ]);
}

// === TestImprovedErrorMessages ===

#[test] fn wrong_arg_count_zero() {
    assert_err("len()", &[
        "len() takes 1 argument(s), but 0 were given\n",
        "  len()\n",
        "  ^~~~~",
    ]);
}

#[test] fn wrong_arg_count_too_many() {
    assert_err("len('a', 'b')", &[
        "len() takes 1 argument(s), but 2 were given\n",
        "  len('a', 'b')\n",
        "  ^~~~~~~~~~~~~",
    ]);
}

#[test] fn wrong_arg_count_multiple_arities() {
    assert_err("min()", &[
        "min() takes 1, 2, 3 arguments, but 0 were given\n",
        "  min()\n",
        "  ^~~~~",
    ]);
}

#[test] fn method_on_wrong_type() {
    assert_err("path(\"/a/b\").startswith(\"/a\")", &[
        "startswith() is not available for path. Available for: string\n",
        "  path(\"/a/b\").startswith(\"/a\")\n",
        "  ~~~~~~~~~~~~~^~~~~~~~~~~~~~~~",
    ]);
}

#[test] fn method_on_wrong_type_upper() {
    assert_err("(5).upper()", &[
        "upper() is not available for int. Available for: string\n",
        "  (5).upper()\n",
        "  ~~~^~~~~~~~",
    ]);
}

#[test] fn property_on_wrong_type() {
    assert_err("True.stem", &[
        "'stem' property is not available for bool. Available for: path\n",
        "  True.stem\n",
        "  ~~~~~^~~~",
    ]);
}

#[test] fn property_on_wrong_type_parent() {
    assert_err("'hello'.parent", &[
        "'parent' property is not available for string. Available for: path\n",
        "  'hello'.parent\n",
        "  ~~~~~~~~^~~~~~",
    ]);
}

#[test] fn attribute_without_call_suggests_parens() {
    assert_err("'hello'.upper", &[
        "'upper' is a method, not a property. Did you mean upper()?\n",
        "  'hello'.upper\n",
        "  ~~~~~~~~^~~~~",
    ]);
}

#[test] fn attribute_without_call_split() {
    assert_err("'hello'.split", &[
        "'split' is a method, not a property. Did you mean split()?\n",
        "  'hello'.split\n",
        "  ~~~~~~~~^~~~~",
    ]);
}

#[test] fn property_called_as_method() {
    assert_err("path('/a/b').stem()", &[
        "'stem' is a property, not a method. Use .stem instead of .stem()\n",
        "  path('/a/b').stem()\n",
        "  ~~~~~~~~~~~~~^~~~~~",
    ]);
}

// === Additional caret tests ===

#[test] fn division_by_zero_caret() {
    assert_err("10 / 0", &[
        "Division by zero\n",
        "  10 / 0\n",
        "  ~~~^~~",
    ]);
}

#[test] fn index_out_of_bounds_caret() {
    let mut st = SymbolTable::new();
    st.set("L", ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Int(2), ExprValue::Int(3)], ExprType::INT).unwrap()).unwrap();
    assert_err_with("L[10]", &st, &[
        "Index 10 out of bounds for list of length 3\n",
        "  L[10]\n",
        "  ~^~~~",
    ]);
}

#[test] fn operator_error_caret_at_operator() {
    assert_err("'hello' + 5", &[
        "Cannot use '+' operator with string and int\n",
        "  'hello' + 5\n",
        "  ~~~~~~~~^~~",
    ]);
}

#[test] fn function_call_error_caret() {
    assert_err("int('bad')", &[
        "Cannot convert 'bad' to int\n",
        "  int('bad')\n",
        "  ^~~~~~~~~~",
    ]);
}

#[test] fn method_call_error_caret() {
    assert_err("'x'.split('')", &[
        "split failed: empty separator\n",
        "  'x'.split('')\n",
        "  ~~~~^~~~~~~~~",
    ]);
}

#[test] fn power_error_caret() {
    assert_err("0 ** -1", &[
        "Cannot raise zero to a negative power\n",
        "  0 ** -1\n",
        "  ~~^~~~~",
    ]);
}

#[test] fn fail_function_caret() {
    assert_err("fail('boom')", &[
        "boom\n",
        "  fail('boom')\n",
        "  ^~~~~~~~~~~~",
    ]);
}

#[test] fn leading_whitespace_preserved() {
    assert_err("  1 / 0", &[
        "Division by zero\n",
        "  1 / 0\n",
        "  ~~^~~",
    ]);
}

#[test] fn syntax_error_has_message() {
    assert_err("1 +", &[
        "Syntax error: Expected an expression\n",
        "  1 +\n",
        "  ^",
    ]);
}

// === TestSyntaxErrorCarets ===

#[test] fn unclosed_paren() {
    assert_err("(1 + 2", &[
        "Syntax error: unexpected EOF while parsing\n",
        "  (1 + 2\n",
        "  ^",
    ]);
}

#[test] fn unclosed_bracket() {
    assert_err("[1, 2", &[
        "Syntax error: unexpected EOF while parsing\n",
        "  [1, 2\n",
        "  ^",
    ]);
}

#[test] fn unclosed_string() {
    assert_err("'hello", &[
        "Syntax error: missing closing quote in string literal\n",
        "  'hello\n",
        "  ^",
    ]);
}

// === TestMultiLineExpressions ===

#[test] fn multiline_error_in_parens() {
    assert_err("(\n  1 + int('bad')\n)", &[
        "Cannot convert 'bad' to int\n",
        "    1 + int('bad')\n",
        "        ^~~~~~~~~~",
    ]);
}

#[test] fn multiline_error_in_list() {
    assert_err("[\n  1,\n  int('bad'),\n  3\n]", &[
        "Cannot convert 'bad' to int\n",
        "    int('bad'),\n",
        "    ^~~~~~~~~~",
    ]);
}

#[test] fn multiline_error_on_first_line() {
    assert_err("(int('bad') +\n  1)", &[
        "Cannot convert 'bad' to int\n",
        "  (int('bad') +\n",
        "   ^~~~~~~~~~",
    ]);
}

#[test] fn multiline_error_shows_correct_line() {
    assert_err("(\n  1 +\n  int('bad') +\n  3\n)", &[
        "Cannot convert 'bad' to int\n",
        "    int('bad') +\n",
        "    ^~~~~~~~~~",
    ]);
}

// === TestImplicitLineContinuation ===

#[test] fn multiline_addition() {
    let r = evaluate_expression("(\n  1 +\n  2 +\n  3\n)", &SymbolTable::new()).unwrap();
    assert_eq!(r.to_display_string(), "6");
}

#[test] fn multiline_comparison() {
    let r = evaluate_expression("(\n  1 <\n  2\n)", &SymbolTable::new()).unwrap();
    assert_eq!(r.to_display_string(), "true");
}

#[test] fn multiline_three_lines() {
    let r = evaluate_expression("(\n  'hello' +\n  ' ' +\n  'world'\n)", &SymbolTable::new()).unwrap();
    assert_eq!(r.to_display_string(), "hello world");
}

#[test] fn multiline_error_shows_correct_line_v2() {
    assert_err("(\n  1 +\n  int('bad') +\n  3\n)", &[
        "Cannot convert 'bad' to int\n",
        "    int('bad') +\n",
        "    ^~~~~~~~~~",
    ]);
}

#[test] fn deeply_nested_multiline() {
    assert_err("(\n  1 +\n  (2 + int('x'))\n)", &[
        "Cannot convert 'x' to int\n",
        "    (2 + int('x'))\n",
        "         ^~~~~~~~",
    ]);
}

#[test] fn error_in_parentheses_multiline() {
    assert_err("(\n  1 + int('bad')\n)", &[
        "Cannot convert 'bad' to int\n",
        "    1 + int('bad')\n",
        "        ^~~~~~~~~~",
    ]);
}

#[test] fn error_in_list_multiline() {
    assert_err("[\n  1,\n  int('bad'),\n  3\n]", &[
        "Cannot convert 'bad' to int\n",
        "    int('bad'),\n",
        "    ^~~~~~~~~~",
    ]);
}

#[test] fn error_on_first_line_multiline() {
    assert_err("(int('bad') +\n  1)", &[
        "Cannot convert 'bad' to int\n",
        "  (int('bad') +\n",
        "   ^~~~~~~~~~",
    ]);
}

// === Multiline success tests ===

#[test] fn multiline_addition_works() {
    let r = evaluate_expression("1 +\n2", &SymbolTable::new()).unwrap();
    assert_eq!(r.to_display_string(), "3");
}

#[test] fn multiline_comparison_works() {
    let r = evaluate_expression("1 <\n2", &SymbolTable::new()).unwrap();
    assert_eq!(r.to_display_string(), "true");
}

#[test] fn multiline_three_lines_works() {
    let r = evaluate_expression("1 +\n2 +\n3", &SymbolTable::new()).unwrap();
    assert_eq!(r.to_display_string(), "6");
}

// ══════════════════════════════════════════════════════════════
// message_with_expr_prefix
// ══════════════════════════════════════════════════════════════

fn eval_err_obj(expr: &str) -> ExpressionError {
    evaluate_expression(expr, &SymbolTable::new()).unwrap_err()
}

fn eval_err_obj_with(expr: &str, st: &SymbolTable) -> ExpressionError {
    evaluate_expression(expr, st).unwrap_err()
}

#[test]
fn prefix_binop_type_error() {
    // Simulates: let x = Param.Frame + "oops"
    // The expression engine only sees: Param.Frame + "oops"
    let st = symtab! { "Param.Frame" => 42 };
    let err = eval_err_obj_with("Param.Frame + \"oops\"", &st);
    let prefixed = err.message_with_expr_prefix("x = ");

    // The prefix shifts the caret right by 4 ("x = ".len())
    assert!(prefixed.contains("x = Param.Frame + \"oops\""), "got:\n{prefixed}");
    // The caret should point at the + operator, shifted by prefix
    // Without prefix: col 12 for the +
    // With prefix "x = ": col 16 for the +
    let lines: Vec<&str> = prefixed.lines().collect();
    let caret_line = lines.last().unwrap();
    let caret_pos = caret_line.find('^').unwrap();
    // "  " (2 indent) + "x = " (4 prefix) + "Param.Frame " (12) = 18
    assert_eq!(caret_pos, 18, "caret at wrong position in:\n{prefixed}");
}

#[test]
fn prefix_let_binding_style() {
    // Simulates: let result = 1 / 0
    let err = eval_err_obj("1 / 0");
    let prefixed = err.message_with_expr_prefix("result = ");
    assert!(prefixed.contains("result = 1 / 0"), "got:\n{prefixed}");
}

#[test]
fn prefix_single_caret() {
    // Error with a single-char span (just ^, no tildes)
    let err = eval_err_obj("xyz");
    let prefixed = err.message_with_expr_prefix("let v = ");
    assert!(prefixed.contains("let v = xyz"), "got:\n{prefixed}");
    let lines: Vec<&str> = prefixed.lines().collect();
    let caret_line = lines.last().unwrap();
    let caret_pos = caret_line.find('^').unwrap();
    // "  " (2) + "let v = " (8) + offset into "xyz"
    assert!(caret_pos >= 10, "caret at wrong position in:\n{prefixed}");
}

#[test]
fn prefix_no_context_falls_back() {
    // Error without expression context — prefix has no effect
    let err = ExpressionError::new("bare error");
    let prefixed = err.message_with_expr_prefix("ignored = ");
    assert_eq!(prefixed, "bare error");
}

#[test]
fn prefix_empty_string() {
    // Empty prefix — should produce same output as Display
    let err = eval_err_obj("1 + \"x\"");
    let normal = err.to_string();
    let prefixed = err.message_with_expr_prefix("");
    assert_eq!(prefixed, normal);
}

#[test]
fn prefix_preserves_tilde_span() {
    // The ~~~^~~~ pattern should be present and shifted
    let st = symtab! { "Param.X" => 42 };
    let err = eval_err_obj_with("Param.X + \"bad\"", &st);
    let prefixed = err.message_with_expr_prefix("val = ");
    // Should have tildes around the caret
    let lines: Vec<&str> = prefixed.lines().collect();
    let caret_line = lines.last().unwrap().trim();
    assert!(caret_line.contains('~'), "expected tildes in: {caret_line}");
    assert!(caret_line.contains('^'), "expected caret in: {caret_line}");
}

// ══════════════════════════════════════════════════════════════
// Tests ported from Python that were missing in Rust
// ══════════════════════════════════════════════════════════════

// --- TestErrorCaretPointers: operator_error_in_middle with symbol table ---

#[test] fn operator_error_in_middle_with_symtab() {
    let st = symtab! { "Param.A" => 5, "Param.B" => "hello" };
    assert_err_with("1 + (Param.A + Param.B) + 2", &st, &[
        "Cannot use '+' operator with int and string\n",
        "  1 + (Param.A + Param.B) + 2\n",
        "       ~~~~~~~~^~~~~~~~~",
    ]);
}

// --- TestErrorCaretPointers: index_out_of_bounds with symbol table ---

#[test] fn index_out_of_bounds_with_symtab() {
    let mut st = SymbolTable::new();
    st.set("Param.List", ExprValue::make_list(
        vec![ExprValue::Int(1), ExprValue::Int(2), ExprValue::Int(3)],
        ExprType::INT,
    ).unwrap()).unwrap();
    assert_err_with("Param.List[10] + 1", &st, &[
        "Index 10 out of bounds for list of length 3\n",
        "  Param.List[10] + 1\n",
        "  ~~~~~~~~~~^~~~",
    ]);
}

// --- TestErrorCaretPointers: unknown_property_in_chain with path symbol table ---

#[test] fn unknown_property_in_chain_with_path_symtab() {
    let mut st = SymbolTable::new();
    st.set("Param.X", ExprValue::Path { value: "/test/file.exr".into(), format: PathFormat::Posix }).unwrap();
    assert_err_with("Param.X.name.unknown", &st, &[
        "Cannot access attribute 'unknown' on string\n",
        "  Param.X.name.unknown\n",
        "  ~~~~~~~~~~~~~^~~~~~~",
    ]);
}

// --- TestErrorCaretPointers: comprehension body matching Python expression ---

#[test] fn error_in_comprehension_body_python() {
    assert_err("[x + int('y') for x in [1,2,3]]", &[
        "Cannot convert 'y' to int\n",
        "  [x + int('y') for x in [1,2,3]]\n",
        "       ^~~~~~~~",
    ]);
}

// --- TestErrorCaretPointers: comprehension filter matching Python expression ---

#[test] fn error_in_comprehension_filter_python() {
    assert_err("[x for x in [1,2,3] if int('bad')]", &[
        "Cannot convert 'bad' to int\n",
        "  [x for x in [1,2,3] if int('bad')]\n",
        "                         ^~~~~~~~~~",
    ]);
}

// --- TestErrorCaretPointers: chained method error matching Python expression ---

#[test] fn chained_method_error_python() {
    assert_err("'a'.upper() + 'b'.split('')[0]", &[
        "split failed: empty separator\n",
        "  'a'.upper() + 'b'.split('')[0]\n",
        "                ~~~~^~~~~~~~~",
    ]);
}

// --- TestErrorCaretPointers: undefined dotted variable with suggestion ---

#[test] fn undefined_dotted_variable_with_suggestion() {
    let mut st = SymbolTable::new();
    st.set("Param.InputFile", ExprValue::Path { value: "/test/file.exr".into(), format: PathFormat::Posix }).unwrap();
    assert_err_with("Param.InputFiel + 1", &st, &[
        "Undefined variable: 'Param.InputFiel'. Did you mean: Param.InputFile\n",
        "  Param.InputFiel + 1\n",
        "  ~~~~~~^~~~~~~~~",
    ]);
}

// --- TestMultiLineExpressions: type error variants matching Python ---

#[test] fn multiline_type_error_in_parens() {
    assert_err("(\n  1 + 'x'\n)", &[
        "Cannot use '+' operator with int and string\n",
        "    1 + 'x'\n",
        "    ~~~^~~~",
    ]);
}

#[test] fn multiline_type_error_in_list() {
    assert_err("[\n  1,\n  2 + 'x',\n  3\n]", &[
        "Cannot use '+' operator with int and string\n",
        "    2 + 'x',\n",
        "    ~~~^~~~",
    ]);
}

#[test] fn multiline_type_error_on_first_line() {
    assert_err("1 + 'x' + (\n2)", &[
        "Cannot use '+' operator with int and string\n",
        "  1 + 'x' + (\n",
        "  ~~~^~~~",
    ]);
}

#[test] fn multiline_type_error_deeply_nested() {
    assert_err("(\n  [\n    1 + 'x'\n  ]\n)", &[
        "Cannot use '+' operator with int and string\n",
        "      1 + 'x'\n",
        "      ~~~^~~~",
    ]);
}

// --- TestImplicitLineContinuation: bare multiline error ---

#[test] fn bare_multiline_error_shows_correct_line() {
    assert_err("1 +\n'x'", &[
        "Cannot use '+' operator with int and string\n",
        "  1 +\n",
        "  ~~^",
    ]);
}
