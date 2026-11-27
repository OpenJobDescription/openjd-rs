// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test_slicing.py

use openjd_expr::{ExprType, ExprValue, ParsedExpression, PathFormat, RangeExpr, SymbolTable};

fn eval(expr: &str) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap()
}
fn eval_fails(expr: &str) -> bool {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .is_err()
}
fn assert_err(expr: &str, expected: &[&str]) {
    let e = ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap_err()
        .to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

// === TestListSlicing ===
#[test]
fn list_basic_slice() {
    assert_eq!(
        eval("[1, 2, 3, 4, 5][1:4]").to_display_string(),
        "[2, 3, 4]"
    );
}
#[test]
fn list_slice_from_start() {
    assert_eq!(eval("[1, 2, 3, 4, 5][:3]").to_display_string(), "[1, 2, 3]");
}
#[test]
fn list_slice_to_end() {
    assert_eq!(eval("[1, 2, 3, 4, 5][2:]").to_display_string(), "[3, 4, 5]");
}
#[test]
fn list_slice_step() {
    assert_eq!(
        eval("[1, 2, 3, 4, 5][::2]").to_display_string(),
        "[1, 3, 5]"
    );
}
#[test]
fn list_slice_reverse() {
    assert_eq!(
        eval("[1, 2, 3, 4, 5][::-1]").to_display_string(),
        "[5, 4, 3, 2, 1]"
    );
}
#[test]
fn list_slice_neg_start() {
    assert_eq!(
        eval("[1, 2, 3, 4, 5][-3:]").to_display_string(),
        "[3, 4, 5]"
    );
}
#[test]
fn list_slice_neg_stop() {
    assert_eq!(
        eval("[1, 2, 3, 4, 5][1:-1]").to_display_string(),
        "[2, 3, 4]"
    );
}
#[test]
fn list_slice_empty() {
    assert_eq!(eval("[1, 2, 3][5:10]").to_display_string(), "[]");
}
#[test]
fn list_slice_step_zero() {
    assert!(eval_fails("[1, 2, 3][::0]"));
}

// === TestStringSlicing ===
#[test]
fn str_basic_slice() {
    assert_eq!(eval("'hello'[1:4]").to_display_string(), "ell");
}
#[test]
fn str_slice_from_start() {
    assert_eq!(eval("'hello'[:3]").to_display_string(), "hel");
}
#[test]
fn str_slice_to_end() {
    assert_eq!(eval("'hello'[2:]").to_display_string(), "llo");
}
#[test]
fn str_slice_reverse() {
    assert_eq!(eval("'hello'[::-1]").to_display_string(), "olleh");
}
#[test]
fn str_slice_step() {
    assert_eq!(eval("'hello'[::2]").to_display_string(), "hlo");
}
#[test]
fn str_slice_multibyte_step_and_reverse() {
    assert_eq!(eval("'aé漢😀b'[1:4]").to_display_string(), "é漢😀");
    assert_eq!(eval("'aé漢😀b'[::2]").to_display_string(), "a漢b");
    assert_eq!(eval("'aé漢😀b'[::-1]").to_display_string(), "b😀漢éa");
    assert_eq!(eval("'aé漢😀b'[3:0:-1]").to_display_string(), "😀漢é");
}

/// A step near `i64::MAX` / `i64::MIN` selects at most one element; the
/// index walk must not overflow past it (Python: `'hello'[1::2**63-1]`
/// is `'e'`). Steps that are multiples of 2^32 (including `i64::MIN`)
/// truncate to zero if cast to a 32-bit `usize`, so they also guard the
/// string slice on 32-bit targets such as wasm32.
#[test]
fn slice_with_extreme_step_selects_one_element() {
    assert_eq!(
        eval("'hello'[1::9223372036854775807]").to_display_string(),
        "e"
    );
    assert_eq!(
        eval("'hello'[3::-9223372036854775808]").to_display_string(),
        "l"
    );
    assert_eq!(eval("'hello'[::4294967296]").to_display_string(), "h");
    assert_eq!(eval("'hello'[::-4294967296]").to_display_string(), "o");
    assert_eq!(
        eval("[1, 2, 3, 4, 5][1::9223372036854775807]").to_display_string(),
        "[2]"
    );
    assert_eq!(
        eval("[1, 2, 3, 4, 5][3::-9223372036854775808]").to_display_string(),
        "[4]"
    );
    assert_eq!(
        eval("[1, 2, 3, 4, 5][::4294967296]").to_display_string(),
        "[1]"
    );
}

// === TestRangeExprSlicing ===
#[test]
fn range_expr_slice() {
    assert_eq!(eval("range_expr('1-10')[2:5]").to_display_string(), "3-5");
}
#[test]
fn range_expr_slice_reverse() {
    assert_eq!(
        eval("range_expr('1-5')[::-1]").to_display_string(),
        "[5, 4, 3, 2, 1]"
    );
}

// === TestPathSlicing ===
// Path slicing delegates to string slicing in Python; not yet implemented in Rust

// === TestSlicingWithExpressions ===
#[test]
fn slice_with_var() {
    let mut st = SymbolTable::new();
    st.set("N", ExprValue::Int(2)).unwrap();
    assert_eq!(
        ParsedExpression::new("[10, 20, 30, 40, 50][:N]")
            .and_then(|p| p.evaluate(&st))
            .unwrap()
            .to_display_string(),
        "[10, 20]"
    );
}

// === Additional slicing tests ===
#[test]
fn list_slice_empty_result() {
    assert_eq!(eval("[1, 2, 3][5:10]").list_len(), Some(0));
}
#[test]
fn list_slice_step_zero_error() {
    assert_err(
        "[1,2,3][::0]",
        &[
            "Slice step cannot be zero\n",
            "  [1,2,3][::0]\n",
            "  ~~~~~~~^~~~~",
        ],
    );
}
#[test]
fn string_single_index() {
    assert_eq!(eval("'hello'[1]").to_display_string(), "e");
}
#[test]
fn string_negative_index() {
    assert_eq!(eval("'hello'[-1]").to_display_string(), "o");
}
#[test]
fn string_index_out_of_bounds() {
    assert_err(
        "'hello'[10]",
        &[
            "Index 10 out of bounds for string of length 5\n",
            "  'hello'[10]\n",
            "  ~~~~~~~^~~~",
        ],
    );
}
#[test]
fn string_slice_negative_indices() {
    assert_eq!(eval("'hello'[-3:-1]").to_display_string(), "ll");
}
#[test]
fn path_index_not_supported() {
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::new_path("/a/b", PathFormat::Posix))
        .unwrap();
    let parsed = openjd_expr::ParsedExpression::new("P[0]").unwrap();
    let symtabs = [&st];
    let e = parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(&["Cannot subscript type path\n", "  P[0]\n", "  ~^~~"].concat()),
        "got:\n{e}"
    );
}
#[test]
fn slice_with_variable_bounds() {
    let mut st = SymbolTable::new();
    st.set(
        "L",
        ExprValue::make_list(
            vec![
                ExprValue::Int(1),
                ExprValue::Int(2),
                ExprValue::Int(3),
                ExprValue::Int(4),
            ],
            ExprType::INT,
        )
        .unwrap(),
    )
    .unwrap();
    st.set("S", ExprValue::Int(1)).unwrap();
    st.set("E", ExprValue::Int(3)).unwrap();
    assert_eq!(
        ParsedExpression::new("L[S:E]")
            .and_then(|p| p.evaluate(&st))
            .unwrap()
            .list_len(),
        Some(2)
    );
}
#[test]
fn chained_slice() {
    assert!(eval("[1, 2, 3, 4, 5][1:4][0:2]").is_list());
}

// === Exact Python name matches for slicing ===
// List slicing
#[test]
fn basic_slice() {
    assert_eq!(eval("[1, 2, 3, 4, 5][1:3]").list_len(), Some(2));
}
#[test]
fn slice_from_start() {
    assert_eq!(eval("[1, 2, 3, 4, 5][:3]").list_len(), Some(3));
}
#[test]
fn slice_to_end() {
    assert_eq!(eval("[1, 2, 3, 4, 5][2:]").list_len(), Some(3));
}
#[test]
fn slice_with_step() {
    assert_eq!(eval("[1, 2, 3, 4, 5][::2]").list_len(), Some(3));
}
#[test]
fn slice_reverse() {
    assert_eq!(eval("[1, 2, 3][::-1]").list_len(), Some(3));
}
#[test]
fn slice_negative_start() {
    assert_eq!(eval("[1, 2, 3, 4, 5][-3:]").list_len(), Some(3));
}
#[test]
fn slice_negative_stop() {
    assert_eq!(eval("[1, 2, 3, 4, 5][:-2]").list_len(), Some(3));
}
#[test]
fn slice_empty_result_v() {
    assert_eq!(eval("[1, 2, 3][5:10]").list_len(), Some(0));
}
#[test]
fn slice_step_zero_error() {
    assert_err(
        "[1, 2, 3][::0]",
        &[
            "Slice step cannot be zero\n",
            "  [1, 2, 3][::0]\n",
            "  ~~~~~~~~~^~~~~",
        ],
    );
}
// String slicing
#[test]
fn string_basic_slice() {
    assert_eq!(eval("'hello'[1:3]").to_display_string(), "el");
}
#[test]
fn string_slice_from_start() {
    assert_eq!(eval("'hello'[:3]").to_display_string(), "hel");
}
#[test]
fn string_slice_to_end() {
    assert_eq!(eval("'hello'[2:]").to_display_string(), "llo");
}
#[test]
fn string_slice_reverse() {
    assert_eq!(eval("'hello'[::-1]").to_display_string(), "olleh");
}
#[test]
fn string_slice_with_step() {
    assert_eq!(eval("'hello'[::2]").to_display_string(), "hlo");
}
#[test]
fn single_index() {
    assert_eq!(eval("'hello'[1]").to_display_string(), "e");
}
#[test]
fn negative_index() {
    assert_eq!(eval("'hello'[-1]").to_display_string(), "o");
}
#[test]
fn index_out_of_bounds() {
    assert_err(
        "'hello'[10]",
        &[
            "Index 10 out of bounds for string of length 5\n",
            "  'hello'[10]\n",
            "  ~~~~~~~^~~~",
        ],
    );
}
// Range slicing
#[test]
fn range_basic_slice() {
    let mut st = SymbolTable::new();
    st.set(
        "R",
        ExprValue::RangeExpr("1-5".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    assert!(matches!(
        ParsedExpression::new("R[1:3]")
            .and_then(|p| p.evaluate(&st))
            .unwrap(),
        ExprValue::RangeExpr(_)
    ));
}
#[test]
fn range_slice_with_step() {
    let mut st = SymbolTable::new();
    st.set(
        "R",
        ExprValue::RangeExpr("1-10".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    assert!(matches!(
        ParsedExpression::new("R[::2]")
            .and_then(|p| p.evaluate(&st))
            .unwrap(),
        ExprValue::RangeExpr(_)
    ));
}
#[test]
fn range_slice_reverse() {
    let mut st = SymbolTable::new();
    st.set(
        "R",
        ExprValue::RangeExpr("1-5".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    assert!(ParsedExpression::new("R[::-1]")
        .and_then(|p| p.evaluate(&st))
        .unwrap()
        .is_list());
}
#[test]
fn slice_negative_indices() {
    assert_eq!(eval("'hello'[-3:-1]").to_display_string(), "ll");
}
#[test]
fn slice_on_split_result() {
    assert!(eval("'a,b,c,d'.split(',')[1:3]").is_list());
}

// === TestLargeRangeSlicing ===
#[test]
fn slice_large_range_no_materialization() {
    // A range with 1 billion elements — slicing should NOT materialize the whole range
    let mut st = SymbolTable::new();
    st.set(
        "R",
        ExprValue::RangeExpr("1-1000000000".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    assert_eq!(
        ParsedExpression::new("R[0:3]")
            .and_then(|p| p.evaluate(&st))
            .unwrap()
            .to_display_string(),
        "1-3"
    );
}

#[test]
fn slice_large_range_reverse_tail() {
    let mut st = SymbolTable::new();
    st.set(
        "R",
        ExprValue::RangeExpr("1-1000000000".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    assert_eq!(
        ParsedExpression::new("R[-3:]")
            .and_then(|p| p.evaluate(&st))
            .unwrap()
            .to_display_string(),
        "999999998-1000000000"
    );
}

#[test]
fn slice_large_range_returns_range_expr() {
    let mut st = SymbolTable::new();
    st.set(
        "R",
        ExprValue::RangeExpr("1-1000000000".parse::<RangeExpr>().unwrap()),
    )
    .unwrap();
    let result = ParsedExpression::new("R[0:3]")
        .and_then(|p| p.evaluate(&st))
        .unwrap();
    assert!(matches!(result, ExprValue::RangeExpr(_)));
}

// === Tests ported from Python test_slicing.py — missing coverage ===

// TestStringSlicing::test_slice_with_step (exact Python input: "abcdefg"[::2] -> "aceg")
#[test]
fn str_slice_step_abcdefg() {
    assert_eq!(eval("'abcdefg'[::2]").to_display_string(), "aceg");
}

// TestStringSlicing::test_single_index (exact Python input: "hello"[0] -> "h")
#[test]
fn str_single_index_zero() {
    assert_eq!(eval("'hello'[0]").to_display_string(), "h");
}

// TestRangeExprSlicing::test_slice_with_step (exact output check)
#[test]
fn range_expr_slice_step_exact() {
    assert_eq!(eval("range_expr('1-10')[::2]").to_display_string(), "1-9:2");
}

// TestRangeExprSlicing::test_slice_negative_indices
#[test]
fn range_expr_slice_neg_indices() {
    assert_eq!(eval("range_expr('1-10')[-3:]").to_display_string(), "8-10");
}

// TestPathSlicing::test_path_index_not_supported (via path() function, matching Python)
#[test]
fn path_func_index_not_supported() {
    assert_err(
        "path('/a/b/c')[0]",
        &[
            "Cannot subscript type path\n",
            "  path('/a/b/c')[0]\n",
            "  ~~~~~~~~~~~~~~^~~",
        ],
    );
}

// TestPathSlicing::test_path_slice_not_supported
#[test]
fn path_func_slice_not_supported() {
    assert_err(
        "path('/a/b/c')[1:3]",
        &[
            "Cannot subscript type path\n",
            "  path('/a/b/c')[1:3]\n",
            "  ~~~~~~~~~~~~~~^~~~~",
        ],
    );
}

// TestSlicingWithExpressions::test_slice_with_variable_bounds (exact Python: start=1, end=4)
#[test]
fn slice_with_start_end_vars() {
    let mut st = SymbolTable::new();
    st.set("start", ExprValue::Int(1)).unwrap();
    st.set("end", ExprValue::Int(4)).unwrap();
    assert_eq!(
        ParsedExpression::new("[1, 2, 3, 4, 5][start:end]")
            .and_then(|p| p.evaluate(&st))
            .unwrap()
            .to_display_string(),
        "[2, 3, 4]"
    );
}

// TestSlicingWithExpressions::test_chained_slice (exact Python: [1:4][::2] -> [2, 4])
#[test]
fn chained_slice_with_step() {
    assert_eq!(
        eval("[1, 2, 3, 4, 5][1:4][::2]").to_display_string(),
        "[2, 4]"
    );
}

// TestSlicingWithExpressions::test_slice_on_split_result (exact Python: semicolon split)
#[test]
fn slice_on_split_semicolon() {
    assert_eq!(
        eval("'a;b;c;d;e'.split(';')[:3]").to_display_string(),
        "[\"a\", \"b\", \"c\"]"
    );
}

// === A subscript's receiver is not coerced, concrete or unresolved ===
//
// 2026-02 §2.1.7 and §2.1.8 define `__getitem__` for `list`, `range_expr` and
// `string` only, and §1.3.8 says a path takes neither. A `path`, `int`,
// `float` or `bool` receiver is refused whether its value is known or only
// its type is (`unresolved[T]`, as at template validation), with the same
// message a run gives, so `openjd check` refuses what a run refuses.

fn eval_with(expr: &str, name: &str, value: ExprValue) -> Result<ExprValue, String> {
    let mut st = SymbolTable::new();
    st.set(name, value).unwrap();
    ParsedExpression::new(expr)
        .and_then(|p| p.with_path_format(PathFormat::Posix).evaluate(&[&st]))
        .map_err(|e| e.to_string())
}

fn not_subscriptable() -> Vec<(ExprValue, &'static str)> {
    vec![
        (ExprValue::new_path("/a/b", PathFormat::Posix), "path"),
        (ExprValue::Int(5), "int"),
        (
            ExprValue::Float(openjd_expr::value::Float64::new(2.5).unwrap()),
            "float",
        ),
        (ExprValue::Bool(true), "bool"),
    ]
}

#[test]
fn unresolved_receiver_is_refused_as_a_concrete_one_is() {
    for (value, type_name) in not_subscriptable() {
        let unresolved = ExprValue::unresolved(value.expr_type());
        for (expr, args, caret) in [
            ("X[0]", "int", "~^~~"),
            ("X[0:1]", "int, int, nulltype", "~^~~~~"),
            ("X[::-1]", "nulltype, nulltype, int", "~^~~~~~"),
        ] {
            let message = if type_name == "path" {
                "Cannot subscript type path".to_string()
            } else {
                format!("No matching signature for __getitem__({type_name}, {args})")
            };
            let expected = format!("{message}\n  {expr}\n  {caret}");
            for v in [value.clone(), unresolved.clone()] {
                let unresolved = v.is_unresolved();
                let err = eval_with(expr, "X", v).expect_err(&format!(
                    "{expr} on a {type_name} (unresolved: {unresolved})"
                ));
                assert_eq!(
                    err, expected,
                    "{expr} on a {type_name} (unresolved: {unresolved})"
                );
            }
        }
    }
}

#[test]
fn literal_path_receiver_is_refused_in_a_host_context_check() {
    use openjd_expr::{ExprProfile, FunctionLibrary, HostContext};
    // A host-context check reads `path('/a/b')` as `unresolved[path]`
    // (specs/expr/function-library.md § Host Context).
    let lib = FunctionLibrary::for_profile(
        &ExprProfile::current().with_host_context(HostContext::Unresolved),
    );
    for (expr, caret) in [
        ("path('/a/b')[0]", "~~~~~~~~~~~~^~~"),
        ("path('/a/b')[0:1]", "~~~~~~~~~~~~^~~~~"),
    ] {
        let err = ParsedExpression::new(expr)
            .and_then(|p| p.with_library(&lib).evaluate(&[&SymbolTable::new()]))
            .expect_err(expr)
            .to_string();
        assert_eq!(
            err,
            format!("Cannot subscript type path\n  {expr}\n  {caret}")
        );
    }
}

#[test]
fn unresolved_subscriptable_receivers_are_typed() {
    let unresolved = |t: &str| ExprValue::unresolved(ExprType::parse(t).unwrap());
    for (t, expr, result) in [
        ("string", "X[0]", "string"),
        ("string", "X[0:1]", "string"),
        ("list[int]", "X[0]", "int"),
        ("list[int]", "X[1:]", "list[int]"),
        ("range_expr", "X[0]", "int"),
        // A slice of a range_expr is a range_expr for a positive step and a
        // list[int] for a negative one, so the type is both.
        ("range_expr", "X[::-1]", "list[int] | range_expr"),
        // One member that takes a subscript is enough: a run may hold it.
        ("path | string", "X[0]", "string"),
        ("any", "X[0]", "any"),
        ("any", "X[0:1]", "any"),
    ] {
        let v = eval_with(expr, "X", unresolved(t)).unwrap_or_else(|e| panic!("{t} {expr}: {e}"));
        assert_eq!(
            v.expr_type(),
            ExprType::unresolved(ExprType::parse(result).unwrap()).unwrap(),
            "{t} {expr}"
        );
    }
}

/// The check matches the signatures of the call's arity: a library whose
/// only `__getitem__` takes an index refuses a slice of the same receiver.
#[test]
fn the_receiver_check_matches_signatures_of_the_calls_arity() {
    use openjd_expr::FunctionLibrary;
    let mut lib = FunctionLibrary::new();
    lib.register_sig("__getitem__", "(string, int) -> string", |_, args| {
        Ok(args[0].clone())
    })
    .unwrap();
    let st = {
        let mut st = SymbolTable::new();
        st.set("X", ExprValue::unresolved(ExprType::STRING))
            .unwrap();
        st
    };
    let eval = |expr: &str| {
        ParsedExpression::new(expr)
            .and_then(|p| p.with_library(&lib).evaluate(&[&st]))
            .map_err(|e| e.to_string())
    };
    eval("X[0]").unwrap();
    assert_eq!(
        eval("X[0:1]").unwrap_err(),
        "No matching signature for __getitem__(string, int, int, nulltype)\n  X[0:1]\n  ~^~~~~"
    );
}

/// A `noreturn` receiver gets the library's message, the one every other
/// type that takes no subscript gets.
#[test]
fn a_noreturn_receiver_gets_the_librarys_message() {
    let err = eval_with("fail(X)[0]", "X", ExprValue::unresolved(ExprType::STRING)).unwrap_err();
    assert_eq!(
        err,
        "No matching signature for __getitem__(noreturn, int)\n  fail(X)[0]\n  ~~~~~~~^~~"
    );
}

/// The index and bounds are checked before the receiver, so an undefined
/// name, a zero step and an index that cannot be an integer keep their own
/// messages, for a receiver that takes no subscript too.
#[test]
fn index_and_bound_errors_are_reported_before_the_receiver() {
    let path = || ExprValue::new_path("/a/b", PathFormat::Posix);
    for (expr, value, expected) in [
        (
            "X[Typo]",
            ExprValue::Int(5),
            "Undefined variable: 'Typo'. Did you mean: X\n  X[Typo]\n    ^~~~",
        ),
        (
            "X[::0]",
            ExprValue::Int(5),
            "Slice step cannot be zero\n  X[::0]\n  ~^~~~~",
        ),
        (
            "X[::0]",
            ExprValue::unresolved(ExprType::PATH),
            "Slice step cannot be zero\n  X[::0]\n  ~^~~~~",
        ),
        (
            "X[::0]",
            path(),
            "Slice step cannot be zero\n  X[::0]\n  ~^~~~~",
        ),
    ] {
        assert_eq!(eval_with(expr, "X", value).unwrap_err(), expected, "{expr}");
    }
    let mut st = SymbolTable::new();
    st.set("X", ExprValue::unresolved(ExprType::INT)).unwrap();
    st.set("S", ExprValue::unresolved(ExprType::STRING))
        .unwrap();
    let err = ParsedExpression::new("X[S]")
        .and_then(|p| p.evaluate(&st))
        .unwrap_err()
        .to_string();
    assert_eq!(err, "Index must be an integer\n  X[S]\n  ~^~~");
}
