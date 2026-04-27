// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for AST validation — unsupported Python constructs must produce
//! descriptive error messages matching the Python implementation's quality.

use openjd_expr::{ParsedExpression, SymbolTable};

fn assert_err(expr: &str, expected: &[&str]) {
    let e = ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

// ══════════════════════════════════════════════════════════════
// Unsupported expression types
// ══════════════════════════════════════════════════════════════

#[test]
fn reject_lambda() {
    assert_err(
        "lambda x: x",
        &[
            "Lambda expressions are not supported\n",
            "  lambda x: x\n",
            "  ^~~~~~~~~~~",
        ],
    );
}
#[test]
fn reject_dict() {
    assert_err(
        "{'a': 1}",
        &[
            "Dict literals are not supported\n",
            "  {'a': 1}\n",
            "  ^~~~~~~~",
        ],
    );
}
#[test]
fn reject_set() {
    assert_err(
        "{1, 2, 3}",
        &[
            "Set literals are not supported\n",
            "  {1, 2, 3}\n",
            "  ^~~~~~~~~",
        ],
    );
}
#[test]
fn reject_set_comp() {
    assert_err(
        "{x for x in [1]}",
        &[
            "Set comprehensions are not supported; only list comprehensions are allowed\n",
            "  {x for x in [1]}\n",
            "  ^~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn reject_dict_comp() {
    assert_err(
        "{k: v for k, v in []}",
        &[
            "Dict comprehensions are not supported; only list comprehensions are allowed\n",
            "  {k: v for k, v in []}\n",
            "  ^~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn reject_generator() {
    assert_err(
        "sum(x for x in [1])",
        &[
            "Generator expressions are not supported; use a list comprehension\n",
            "  sum(x for x in [1])\n",
            "      ^~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn reject_tuple() {
    assert_err(
        "(1, 2)",
        &[
            "Tuple literals are not supported; use a list instead\n",
            "  (1, 2)\n",
            "  ^~~~~~",
        ],
    );
}
#[test]
fn reject_walrus() {
    assert_err(
        "(x := 5)",
        &[
            "Walrus operator (:=) is not supported\n",
            "  (x := 5)\n",
            "   ^~~~~~",
        ],
    );
}
#[test]
fn reject_fstring() {
    assert_err(
        "f'hello {1}'",
        &[
            "f-strings are not supported; use string concatenation\n",
            "  f'hello {1}'\n",
            "  ^~~~~~~~~~~~",
        ],
    );
}

// ══════════════════════════════════════════════════════════════
// Unsupported operators
// ══════════════════════════════════════════════════════════════

#[test]
fn reject_bitand() {
    assert_err(
        "1 & 2",
        &["Bitwise AND (&) is not supported\n", "  1 & 2\n", "  ~~^~~"],
    );
}
#[test]
fn reject_bitor() {
    assert_err(
        "1 | 2",
        &["Bitwise OR (|) is not supported\n", "  1 | 2\n", "  ~~^~~"],
    );
}
#[test]
fn reject_bitxor() {
    assert_err(
        "1 ^ 2",
        &["Bitwise XOR (^) is not supported\n", "  1 ^ 2\n", "  ~~^~~"],
    );
}
#[test]
fn reject_bitnot() {
    assert_err(
        "~1",
        &["Bitwise NOT (~) is not supported\n", "  ~1\n", "  ^~"],
    );
}
#[test]
fn reject_lshift() {
    assert_err(
        "1 << 2",
        &[
            "Left shift (<<) is not supported\n",
            "  1 << 2\n",
            "  ~~~^~~",
        ],
    );
}
#[test]
fn reject_rshift() {
    assert_err(
        "1 >> 2",
        &[
            "Right shift (>>) is not supported\n",
            "  1 >> 2\n",
            "  ~~~^~~",
        ],
    );
}
#[test]
fn reject_is() {
    assert_err(
        "1 is 1",
        &[
            "'is' operator is not supported; use '=='\n",
            "  1 is 1\n",
            "  ^~~~~~",
        ],
    );
}
#[test]
fn reject_is_not() {
    assert_err(
        "1 is not 2",
        &[
            "'is not' operator is not supported; use '!='\n",
            "  1 is not 2\n",
            "  ^~~~~~~~~~",
        ],
    );
}

// ══════════════════════════════════════════════════════════════
// Unsupported string literals
// ══════════════════════════════════════════════════════════════

#[test]
fn reject_u_string() {
    assert_err(
        "u'hello'",
        &[
            "Unicode string prefix u'...' is not supported. Use '...' or \"...\" instead.\n",
            "  u'hello'\n",
            "  ^~~~~~~~",
        ],
    );
}
#[test]
fn reject_b_string() {
    assert_err(
        "b'hello'",
        &[
            "Byte strings (b'...') are not supported. Use '...' or \"...\" instead.\n",
            "  b'hello'\n",
            "  ^~~~~~~~",
        ],
    );
}
#[test]
fn reject_ellipsis() {
    assert_err(
        "...",
        &["Ellipsis (...) is not supported\n", "  ...\n", "  ^~~"],
    );
}

// ══════════════════════════════════════════════════════════════
// Comprehension structural validation
// ══════════════════════════════════════════════════════════════

#[test]
fn reject_multi_for() {
    assert_err(
        "[x for x in [1] for y in [2]]",
        &[
            "Multiple 'for' clauses in list comprehensions are not supported\n",
            "  [x for x in [1] for y in [2]]\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn reject_multi_if() {
    assert_err(
        "[x for x in [1] if x > 0 if x < 5]",
        &[
            "Multiple 'if' clauses in a list comprehension are not supported; combine with 'and'\n",
            "  [x for x in [1] if x > 0 if x < 5]\n",
            "  ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
    );
}
#[test]
fn reject_tuple_unpack() {
    assert_err(
        "[x for x, y in [(1,2)]]",
        &[
            "Tuple unpacking in list comprehension is not supported\n",
            "  [x for x, y in [(1,2)]]\n",
            "         ^~~~",
        ],
    );
}
#[test]
fn reject_upper_loop_var() {
    assert_err(
        "[X for X in [1]]",
        &[
            "Loop variable 'X' must start with a lowercase letter or underscore\n",
            "  [X for X in [1]]\n",
            "         ^",
        ],
    );
}

// ══════════════════════════════════════════════════════════════
// Unsupported call syntax
// ══════════════════════════════════════════════════════════════

#[test]
fn reject_keyword_arg() {
    assert_err(
        "len(x=1)",
        &[
            "Keyword arguments are not supported\n",
            "  len(x=1)\n",
            "  ^~~~~~~~",
        ],
    );
}
