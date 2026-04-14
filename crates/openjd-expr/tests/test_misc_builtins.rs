// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Coverage tests for builtin functions in functions/misc.rs:
//! fail_fn, zfill_fn, any_fn, all_fn, abs_int, abs_float,
//! len_string, len_path, len_list, len_range, path_fn, path_join_fn

use openjd_expr::{evaluate_expression, ExprValue, PathFormat, RangeExpr, SymbolTable};
use openjd_expr::error::ExpressionError;
use openjd_expr::function_library::EvalContext;
use openjd_expr::functions::misc::*;
use openjd_expr::path_mapping::PathMappingRule;
use openjd_expr::types::ExprType;

fn eval(expr: &str) -> ExprValue {
    evaluate_expression(expr, &SymbolTable::new()).unwrap()
}
fn eval_with(expr: &str, st: &SymbolTable) -> ExprValue {
    evaluate_expression(expr, st).unwrap()
}
fn assert_err(expr: &str, expected: &[&str]) {
    let e = evaluate_expression(expr, &SymbolTable::new()).unwrap_err().to_string();
    let joined = expected.concat();
    assert!(e.contains(&joined), "got:\n{e}\nexpected:\n{joined}");
}

struct Ctx;
impl EvalContext for Ctx {
    fn path_format(&self) -> PathFormat { PathFormat::Posix }
    fn path_mapping_rules(&self) -> &[PathMappingRule] { &[] }
    fn count_op(&mut self) -> Result<(), ExpressionError> { Ok(()) }
    fn count_ops(&mut self, _: usize) -> Result<(), ExpressionError> { Ok(()) }
    fn count_string_ops(&mut self, _: usize) -> Result<(), ExpressionError> { Ok(()) }
}

// === fail_fn ===
#[test] fn fail_with_message() {
    assert_err("fail('error message')", &[
        "error message\n",
        "  fail('error message')\n",
    ]);
}
#[test] fn fail_no_args() {
    assert_err("fail()", &[
        "fail() takes 1 argument(s), but 0 were given\n",
        "  fail()\n",
    ]);
}
#[test] fn fail_fn_direct_no_args() {
    // Exercise the a.is_empty() branch in fail_fn directly
    let err = fail_fn(&mut Ctx, &[]).unwrap_err();
    assert!(err.to_string().contains("fail() called"));
}

// === zfill_fn ===
#[test] fn zfill_string() { assert_eq!(eval("zfill('42', 5)").to_display_string(), "00042"); }
#[test] fn zfill_negative() { assert_eq!(eval("zfill('-42', 6)").to_display_string(), "-00042"); }
#[test] fn zfill_int_input() { assert_eq!(eval("zfill(42, 5)").to_display_string(), "00042"); }
#[test] fn zfill_already_wide() { assert_eq!(eval("zfill('12345', 3)").to_display_string(), "12345"); }
#[test] fn zfill_plus_sign() { assert_eq!(eval("zfill('+7', 5)").to_display_string(), "+0007"); }

// === any_fn ===
#[test] fn any_with_true() { assert_eq!(eval("any([True, False])").to_display_string(), "true"); }
#[test] fn any_all_false() { assert_eq!(eval("any([False, False])").to_display_string(), "false"); }
#[test] fn any_empty() { assert_eq!(eval("any([])").to_display_string(), "false"); }
#[test] fn any_all_true() { assert_eq!(eval("any([True, True])").to_display_string(), "true"); }

// === all_fn ===
#[test] fn all_with_true() { assert_eq!(eval("all([True, True])").to_display_string(), "true"); }
#[test] fn all_with_false() { assert_eq!(eval("all([True, False])").to_display_string(), "false"); }
#[test] fn all_empty() { assert_eq!(eval("all([])").to_display_string(), "true"); }
#[test] fn all_all_false() { assert_eq!(eval("all([False, False])").to_display_string(), "false"); }

// === abs_int / abs_float ===
#[test] fn abs_neg_int() { assert_eq!(eval("abs(-5)").to_display_string(), "5"); }
#[test] fn abs_pos_int() { assert_eq!(eval("abs(5)").to_display_string(), "5"); }
#[test] fn abs_zero() { assert_eq!(eval("abs(0)").to_display_string(), "0"); }
#[test] fn abs_neg_float() { assert_eq!(eval("abs(-1.5)").to_display_string(), "1.5"); }
#[test] fn abs_pos_float() { assert_eq!(eval("abs(1.5)").to_display_string(), "1.5"); }

// === len_string ===
#[test] fn len_string_basic() { assert_eq!(eval("len('hello')").to_display_string(), "5"); }
#[test] fn len_string_empty() { assert_eq!(eval("len('')").to_display_string(), "0"); }

// === len_path ===
#[test] fn len_path_basic() {
    let mut st = SymbolTable::new();
    st.set("p", ExprValue::Path { value: "/foo/bar".into(), format: PathFormat::Posix }).unwrap();
    assert_eq!(eval_with("len(p)", &st).to_display_string(), "8");
}
#[test] fn len_path_direct() {
    // Call len_path directly to ensure coverage
    let r = len_path(&mut Ctx, &[ExprValue::Path { value: "/x".into(), format: PathFormat::Posix }]).unwrap();
    assert_eq!(r, ExprValue::Int(2));
}

// === len_list ===
#[test] fn len_list_basic() { assert_eq!(eval("len([1, 2, 3])").to_display_string(), "3"); }
#[test] fn len_list_empty() { assert_eq!(eval("len([])").to_display_string(), "0"); }

// === len_range ===
#[test] fn len_range_basic() {
    let mut st = SymbolTable::new();
    st.set("r", ExprValue::RangeExpr("1-10".parse::<RangeExpr>().unwrap())).unwrap();
    assert_eq!(eval_with("len(r)", &st).to_display_string(), "10");
}

// === path_fn ===
#[test] fn path_from_string() {
    assert!(matches!(eval("path('/tmp/file')"), ExprValue::Path { .. }));
}
#[test] fn path_from_path() {
    let mut st = SymbolTable::new();
    st.set("p", ExprValue::Path { value: "/foo".into(), format: PathFormat::Posix }).unwrap();
    assert!(matches!(eval_with("path(p)", &st), ExprValue::Path { .. }));
}
#[test] fn path_from_list() {
    assert_eq!(eval("path(['/a', 'b', 'c'])").to_display_string(), "/a/b/c");
}
#[test] fn path_from_list_with_non_string() {
    // Exercise the _ => e.to_display_string() branch in path_fn list handling (line 82)
    let list = ExprValue::make_list(
        vec![ExprValue::from("/a"), ExprValue::Int(42)],
        ExprType::union(vec![ExprType::STRING, ExprType::INT]),
    ).unwrap();
    let r = path_fn(&mut Ctx, &[list]).unwrap();
    // PathBuf::push with "42" appends it
    assert!(matches!(r, ExprValue::Path { .. }));
}
#[test] fn path_fn_unsupported_type() {
    // Exercise the error branch for unsupported type (line 95)
    let err = path_fn(&mut Ctx, &[ExprValue::Bool(true)]).unwrap_err();
    assert!(err.to_string().contains("path() not supported for"));
}

// path_join_fn removed — not in spec (RFC 0006)
