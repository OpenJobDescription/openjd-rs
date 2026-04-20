// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_string_operation_counting.py

use openjd_expr::*;

fn eval_bounded(expr: &str, mem: usize, ops: usize) -> Result<EvalResult, ExpressionError> {
    ParsedExpression::new(expr).and_then(|p| {
        p.with_memory_limit(mem)
            .with_operation_limit(ops)
            .evaluate_with_metrics(&[&SymbolTable::new()])
    })
}

/// Assert that a bounded evaluation produces an operation-limit error.
/// The first element of `expected` should be `"_OP_MSG\n"` as a placeholder —
/// it will be replaced with the actual `"Expression operation count (N) exceeded limit (M)\n"`.
fn assert_op_limit_err(expr: &str, mem: usize, ops: usize, expected: &[&str]) {
    let e = eval_bounded(expr, mem, ops).unwrap_err().to_string();
    // Verify the first line has the right format and limit
    let first_line = e.lines().next().unwrap_or("");
    assert!(
        first_line.starts_with("Expression operation count (")
            && first_line.contains(&format!("exceeded limit ({ops})")),
        "wrong first line:\n{e}"
    );
    // Verify the expression + caret lines match exactly
    let rest_actual = e.split_once('\n').map_or("", |x| x.1);
    let rest_expected: String = expected[1..].concat();
    assert_eq!(
        rest_actual, rest_expected,
        "got:\n{e}\nexpected rest:\n{rest_expected}"
    );
}

/// Assert error for expressions with very long literal strings where we can't
/// match the full line, but verify structure and key fragments.
fn assert_op_limit_err_long(expr: &str, mem: usize, ops: usize, fragments: &[&str]) {
    let e = eval_bounded(expr, mem, ops).unwrap_err().to_string();
    let lines: Vec<&str> = e.lines().collect();
    assert_eq!(
        lines.len(),
        3,
        "expected 3-line error, got {} lines:\n{e}",
        lines.len()
    );
    assert!(
        lines[0].starts_with("Expression operation count ("),
        "wrong message line:\n{e}"
    );
    assert!(
        lines[0].contains(&format!("exceeded limit ({ops})")),
        "wrong limit in message:\n{e}"
    );
    for frag in fragments {
        assert!(e.contains(frag), "error should contain {frag:?}:\n{e}");
    }
}

// === String operation counting ===
#[test]
fn short_string_upper() {
    let r = eval_bounded("'hello'.upper()", 100_000_000, 10_000_000).unwrap();
    assert!(r.operation_count >= 1);
}
#[test]
fn empty_string_upper() {
    let r = eval_bounded("''.upper()", 100_000_000, 10_000_000).unwrap();
    assert!(r.operation_count >= 1);
}
#[test]
fn string_replace() {
    let r = eval_bounded("'hello world'.replace('o', 'O')", 100_000_000, 10_000_000).unwrap();
    assert!(r.operation_count >= 1);
}
#[test]
fn string_split() {
    let r = eval_bounded("'a,b,c'.split(',')", 100_000_000, 10_000_000).unwrap();
    assert!(r.operation_count >= 1);
}
#[test]
fn string_concat() {
    let r = eval_bounded("'hello' + ' ' + 'world'", 100_000_000, 10_000_000).unwrap();
    let _ = r.operation_count; /* always >= 0 for usize */
}
#[test]
fn string_contains() {
    let r = eval_bounded("'ell' in 'hello'", 100_000_000, 10_000_000).unwrap();
    let _ = r.operation_count; /* always >= 0 for usize */
}
#[test]
fn regex_search() {
    let r = eval_bounded(r"re_search('hello123', r'\d+')", 100_000_000, 10_000_000).unwrap();
    assert!(r.operation_count >= 1);
}
#[test]
fn len_does_not_count() {
    let r = eval_bounded("len('hello')", 100_000_000, 10_000_000).unwrap();
    assert!(r.operation_count >= 1);
}
#[test]
fn path_name() {
    let r = eval_bounded("path('/a/b/file.txt').name", 100_000_000, 10_000_000).unwrap();
    assert!(r.operation_count >= 1);
}
#[test]
fn path_parent() {
    let r = eval_bounded("path('/a/b/file.txt').parent", 100_000_000, 10_000_000).unwrap();
    assert!(r.operation_count >= 1);
}
#[test]
fn short_string_functions() {
    let r = eval_bounded("'hello'.upper().lower().strip()", 100_000_000, 10_000_000).unwrap();
    assert!(r.operation_count >= 3);
}
#[test]
fn join_counts() {
    let r = eval_bounded("join(['a', 'b', 'c'], ',')", 100_000_000, 10_000_000).unwrap();
    assert!(r.operation_count >= 1);
}
#[test]
fn zfill_counts() {
    let r = eval_bounded("zfill(42, 8)", 100_000_000, 10_000_000).unwrap();
    assert!(r.operation_count >= 1);
}

// === Large string exceeds limit (literal big strings) ===
#[test]
fn large_string_upper_exceeds() {
    let big = "x".repeat(1000);
    let expr = format!("'{}'.upper()", big);
    assert_op_limit_err_long(&expr, 100_000_000, 1, &[".upper()", "^~~~~~~"]);
}
#[test]
fn chained_ops_accumulate() {
    let r = eval_bounded(
        "'hello'.upper().lower().strip().upper()",
        100_000_000,
        10_000_000,
    )
    .unwrap();
    assert!(r.operation_count >= 4);
}

// === Exact Python name matches ===
#[test]
fn string_256_upper() {
    let s = "x".repeat(256);
    let r = eval_bounded(&format!("'{}'.upper()", s), 100_000_000, 10_000_000).unwrap();
    assert!(r.operation_count >= 1);
}
#[test]
fn string_257_upper() {
    let s = "x".repeat(257);
    let r = eval_bounded(&format!("'{}'.upper()", s), 100_000_000, 10_000_000).unwrap();
    assert!(r.operation_count >= 1);
}
#[test]
fn string_1000_upper() {
    let s = "x".repeat(1000);
    let r = eval_bounded(&format!("'{}'.upper()", s), 100_000_000, 10_000_000).unwrap();
    assert!(r.operation_count >= 1);
}
#[test]
fn string_repetition() {
    let r = eval_bounded("'ab' * 100", 100_000_000, 10_000_000).unwrap();
    assert!(r.operation_count >= 1);
}
#[test]
fn repr_sh_string() {
    let r = eval_bounded("repr_sh('hello world')", 100_000_000, 10_000_000).unwrap();
    assert!(r.operation_count >= 1);
}
#[test]
fn len_does_not_count_string_ops() {
    let r = eval_bounded("len('hello')", 100_000_000, 10_000_000).unwrap();
    assert!(r.operation_count >= 1);
}
#[test]
fn path_add_suffix() {
    let r = eval_bounded(
        "path('/a/b/file.txt').with_suffix('.png')",
        100_000_000,
        10_000_000,
    )
    .unwrap();
    assert!(r.operation_count >= 1);
}
#[test]
fn join_counts_list_and_string() {
    let r = eval_bounded("join(['a', 'b', 'c'], ',')", 100_000_000, 10_000_000).unwrap();
    assert!(r.operation_count >= 1);
}
#[test]
fn zfill_counts_string_ops() {
    let r = eval_bounded("zfill(42, 8)", 100_000_000, 10_000_000).unwrap();
    assert!(r.operation_count >= 1);
}
#[test]
fn large_string_replace_exceeds() {
    let big = "x".repeat(1000);
    let expr = format!("'{}'.replace('x', 'y')", big);
    assert_op_limit_err_long(
        &expr,
        100_000_000,
        1,
        &[".replace('x', 'y')", "^~~~~~~~~~~~~~~~~"],
    );
}
#[test]
fn large_string_split_exceeds() {
    let big = "x,".repeat(500);
    let expr = format!("'{}'.split(',')", big);
    assert_op_limit_err_long(&expr, 100_000_000, 1, &[".split(',')", "^~~~~~~~~"]);
}
#[test]
fn large_string_regex_exceeds() {
    let big = "x".repeat(1000);
    let expr = format!("re_search('{}', r'x+')", big);
    assert_op_limit_err_long(&expr, 100_000_000, 1, &["re_search(", "r'x+')", "^~"]);
}
#[test]
fn large_string_repetition_exceeds() {
    assert_op_limit_err(
        "'x' * 10000",
        100_000_000,
        1,
        &[
            "Operation limit exceeded\n",
            "  'x' * 10000\n",
            "  ~~~~^~~~~~~",
        ],
    );
}
#[test]
fn string_concat_large_exceeds() {
    let big = "x".repeat(1000);
    let expr = format!("'{}' + '{}'", big, big);
    assert_op_limit_err_long(&expr, 100_000_000, 1, &["^", "~"]);
}
#[test]
fn large_path_operation_exceeds() {
    let big = "/".to_string() + &"a/".repeat(500);
    let expr = format!("path('{}').name", big);
    assert_op_limit_err_long(&expr, 100_000_000, 1, &["path(", ".name", "^~"]);
}

// ==========================================================================
// Tests ported from Python: TestStringOperationCounting (exact counts)
// ==========================================================================

#[test]
fn exact_short_string_upper() {
    let r = eval_bounded("'hello'.upper()", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 2);
}
#[test]
fn exact_empty_string_upper() {
    let r = eval_bounded("''.upper()", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 1);
}
#[test]
fn exact_256_char_string_upper() {
    let r = eval_bounded("('a' * 256).upper()", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 4);
}
#[test]
fn exact_257_char_string_upper() {
    let r = eval_bounded("('a' * 257).upper()", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 6);
}
#[test]
fn exact_1000_char_string_upper() {
    let r = eval_bounded("('a' * 1000).upper()", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 10);
}
#[test]
fn exact_string_replace() {
    let r = eval_bounded("('abc' * 100).replace('a', 'x')", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 6);
}
#[test]
fn exact_string_split() {
    let r = eval_bounded("('a,' * 200).split(',')", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 6);
}
#[test]
fn exact_string_concat() {
    let r = eval_bounded("('a' * 300) + ('b' * 300)", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 10);
}
#[test]
fn exact_string_repetition() {
    let r = eval_bounded("'a' * 1000", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 5);
}
#[test]
fn exact_string_contains() {
    let r = eval_bounded("'x' in ('a' * 500)", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 7);
}
#[test]
fn exact_regex_search() {
    let r = eval_bounded("re_search('a' * 500, r'b')", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 6);
}
#[test]
fn exact_repr_sh_string() {
    let r = eval_bounded("repr_sh('a' * 500)", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 6);
}
#[test]
fn exact_len_does_not_count_string_ops() {
    let r = eval_bounded("len('a' * 1000)", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 6);
}

// ==========================================================================
// Tests ported from Python: TestPathOperationCounting (exact counts)
// ==========================================================================

#[test]
fn exact_path_name() {
    let r = eval_bounded("path('/a/b/c/d/e/f').name", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 4);
}
#[test]
fn exact_path_parent() {
    let r = eval_bounded("path('/a/b/c').parent", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 4);
}
#[test]
fn exact_path_join() {
    let r = eval_bounded("path('/a/b') / 'c/d'", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 4);
}
#[test]
fn exact_path_add_suffix() {
    let r = eval_bounded("path('/a/b/file') + '.txt'", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 4);
}

// ==========================================================================
// Tests ported from Python: TestStringOpLimitExceeded (with in-expression mul)
// Full 3-line error message assertions.
// ==========================================================================

#[test]
fn limit_large_string_upper_exceeds() {
    assert_op_limit_err(
        "('a' * 10000).upper()",
        100_000_000,
        5,
        &[
            "Operation limit exceeded\n",
            "  ('a' * 10000).upper()\n",
            "   ~~~~^~~~~~~",
        ],
    );
}
#[test]
fn limit_large_string_replace_exceeds() {
    assert_op_limit_err(
        "('a' * 10000).replace('a', 'b')",
        100_000_000,
        5,
        &[
            "Operation limit exceeded\n",
            "  ('a' * 10000).replace('a', 'b')\n",
            "   ~~~~^~~~~~~",
        ],
    );
}
#[test]
fn limit_large_string_split_exceeds() {
    assert_op_limit_err(
        "('a,' * 5000).split(',')",
        100_000_000,
        5,
        &[
            "Operation limit exceeded\n",
            "  ('a,' * 5000).split(',')\n",
            "   ~~~~~^~~~~~",
        ],
    );
}
#[test]
fn limit_large_string_regex_exceeds() {
    assert_op_limit_err(
        "re_search('a' * 10000, r'b')",
        100_000_000,
        5,
        &[
            "Operation limit exceeded\n",
            "  re_search('a' * 10000, r'b')\n",
            "            ~~~~^~~~~~~",
        ],
    );
}
#[test]
fn limit_large_string_repetition_exceeds() {
    assert_op_limit_err(
        "'a' * 100000",
        100_000_000,
        5,
        &[
            "Operation limit exceeded\n",
            "  'a' * 100000\n",
            "  ~~~~^~~~~~~~",
        ],
    );
}
#[test]
fn limit_chained_string_ops_accumulate() {
    assert_op_limit_err(
        "('a' * 1000).upper().lower().strip()",
        100_000_000,
        5,
        &[
            "Operation limit exceeded\n",
            "  ('a' * 1000).upper().lower().strip()\n",
            "   ~~~~^~~~~~",
        ],
    );
}
#[test]
fn limit_large_path_operation_exceeds() {
    assert_op_limit_err(
        "path('a' * 1000).name",
        100_000_000,
        5,
        &[
            "Operation limit exceeded\n",
            "  path('a' * 1000).name\n",
            "       ~~~~^~~~~~",
        ],
    );
}
#[test]
fn limit_string_concat_large_exceeds() {
    assert_op_limit_err(
        "('a' * 5000) + ('b' * 5000)",
        100_000_000,
        5,
        &[
            "Operation limit exceeded\n",
            "  ('a' * 5000) + ('b' * 5000)\n",
            "   ~~~~^~~~~~",
        ],
    );
}

// ==========================================================================
// Tests ported from Python: TestStringOpCountPrecise
// ==========================================================================

#[test]
fn precise_lower() {
    let r = eval_bounded("'hello'.lower()", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 2);
}
#[test]
fn precise_strip() {
    let r = eval_bounded("'  hi  '.strip()", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 2);
}
#[test]
fn precise_startswith() {
    let r = eval_bounded("'hello'.startswith('he')", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 2);
}
#[test]
fn precise_endswith() {
    let r = eval_bounded("'hello'.endswith('lo')", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 2);
}
#[test]
fn precise_find() {
    let r = eval_bounded("'hello'.find('l')", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 2);
}
#[test]
fn precise_count() {
    let r = eval_bounded("'hello'.count('l')", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 2);
}
#[test]
fn precise_capitalize() {
    let r = eval_bounded("'hello'.capitalize()", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 2);
}
#[test]
fn precise_title() {
    let r = eval_bounded("'hello'.title()", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 2);
}
#[test]
fn precise_isdigit() {
    let r = eval_bounded("'123'.isdigit()", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 2);
}
#[test]
fn precise_removeprefix() {
    let r = eval_bounded("'hello'.removeprefix('he')", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 2);
}
#[test]
fn precise_re_escape() {
    let r = eval_bounded("re_escape('he[l]')", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 2);
}
#[test]
fn precise_repr_sh() {
    let r = eval_bounded("repr_sh('hello')", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 2);
}
#[test]
fn precise_repr_py() {
    let r = eval_bounded("repr_py('hello')", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 2);
}
#[test]
fn precise_repr_json() {
    let r = eval_bounded("repr_json('hello')", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 2);
}
#[test]
fn precise_repr_cmd() {
    let r = eval_bounded("repr_cmd('hello')", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 2);
}
#[test]
fn precise_repr_pwsh() {
    let r = eval_bounded("repr_pwsh('hello')", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 2);
}
#[test]
fn precise_join_method() {
    let r = eval_bounded("['a','b','c'].join(',')", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 4);
}
#[test]
fn precise_zfill() {
    let r = eval_bounded("('a' * 300).zfill(500)", 100_000_000, 10_000_000).unwrap();
    assert_eq!(r.operation_count, 6);
}
