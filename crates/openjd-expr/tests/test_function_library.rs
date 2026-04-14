// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests for FunctionLibrary public API: dispatch phases, error messages,
//! method vs function coercion, host context, and type derivation.

use openjd_expr::*;
use openjd_expr::function_library::{FunctionLibrary, EvalContext};
use openjd_expr::value::Float64;

// ── Helpers ──

fn eval(expr: &str) -> ExprValue {
    evaluate_expression(expr, &SymbolTable::new()).unwrap()
}

fn eval_with(expr: &str, st: &SymbolTable) -> ExprValue {
    evaluate_expression(expr, st).unwrap()
}

fn eval_err_with(expr: &str, st: &SymbolTable) -> String {
    evaluate_expression(expr, st).unwrap_err().to_string()
}

struct MockCtx;
impl EvalContext for MockCtx {
    fn path_format(&self) -> PathFormat { PathFormat::Posix }
    fn path_mapping_rules(&self) -> &[path_mapping::PathMappingRule] { &[] }
    fn count_op(&mut self) -> Result<(), ExpressionError> { Ok(()) }
    fn count_ops(&mut self, _n: usize) -> Result<(), ExpressionError> { Ok(()) }
    fn count_string_ops(&mut self, _len: usize) -> Result<(), ExpressionError> { Ok(()) }
}

fn custom_add(_ctx: &mut dyn EvalContext, args: &[ExprValue]) -> Result<ExprValue, ExpressionError> {
    match (&args[0], &args[1]) {
        (ExprValue::Int(a), ExprValue::Int(b)) => Ok(ExprValue::Int(a * 100 + b)),
        _ => Err(ExpressionError::new("custom_add requires ints")),
    }
}

fn identity(_ctx: &mut dyn EvalContext, args: &[ExprValue]) -> Result<ExprValue, ExpressionError> {
    Ok(args[0].clone())
}

fn add_float(_ctx: &mut dyn EvalContext, args: &[ExprValue]) -> Result<ExprValue, ExpressionError> {
    match (&args[0], &args[1]) {
        (ExprValue::Float(a), ExprValue::Float(b)) => Ok(ExprValue::Float(Float64::new(a.0 + b.0)?)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

// ── Evaluator integration (original 2 tests) ──

#[test]
fn library_function_called_from_evaluator() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("custom_add", "(int, int) -> int", custom_add);

    let parsed = ParsedExpression::new("custom_add(3, 7)").unwrap();
    let st = SymbolTable::new();
    let symtabs = [&st];
    let mut evaluator = parsed.evaluator(&symtabs).with_library(&lib);
    let result = evaluator.evaluate(&parsed.ast).unwrap();
    assert_eq!(result, ExprValue::Int(307));
}

#[test]
fn library_function_with_unresolved() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("custom_add", "(int, int) -> int", custom_add);

    let mut st = SymbolTable::new();
    st.set("X", ExprValue::unresolved(ExprType::INT)).unwrap();
    let parsed = ParsedExpression::new("custom_add(X, 7)").unwrap();
    let symtabs = [&st];
    let mut evaluator = parsed.evaluator(&symtabs).with_library(&lib);
    let result = evaluator.evaluate(&parsed.ast).unwrap();
    assert!(result.is_unresolved());
    assert_eq!(result.expr_type(), ExprType::unresolved(ExprType::INT));
}

// ── 3-phase dispatch via call() ──

#[test]
fn phase1_exact_match_preferred_over_coercion() {
    // When both exact and coerced matches exist, exact wins
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(int, int) -> int", custom_add);       // exact: returns a*100+b
    lib.register_sig("f", "(float, float) -> float", add_float);  // coerced would go here
    let mut ctx = MockCtx;
    let r = lib.call("f", &[ExprValue::Int(2), ExprValue::Int(3)], &mut ctx).unwrap();
    assert_eq!(r, ExprValue::Int(203)); // proves exact match ran, not coerced
}

#[test]
fn phase2_coercion_int_to_float() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(float, float) -> float", add_float);
    let mut ctx = MockCtx;
    // No exact (int,int) match → coerce both ints to float
    let r = lib.call("f", &[ExprValue::Int(2), ExprValue::Int(3)], &mut ctx).unwrap();
    assert_eq!(r, ExprValue::Float(Float64::new(5.0).unwrap()));
}

#[test]
fn phase2_coercion_path_to_string() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(string) -> string", identity);
    let mut ctx = MockCtx;
    let path = ExprValue::Path { value: "/tmp/test".into(), format: PathFormat::Posix };
    let r = lib.call("f", &[path], &mut ctx).unwrap();
    assert_eq!(r, ExprValue::String("/tmp/test".into()));
}

#[test]
fn phase3_generic_match() {
    fn first(_ctx: &mut dyn EvalContext, args: &[ExprValue]) -> Result<ExprValue, ExpressionError> {
        args[0].list_get(0).ok_or_else(|| ExpressionError::new("empty"))
    }
    let mut lib = FunctionLibrary::new();
    lib.register_sig("first", "(list[T1]) -> T1", first);
    let mut ctx = MockCtx;
    let list = ExprValue::make_list(vec![ExprValue::Int(42)], ExprType::INT).unwrap();
    let r = lib.call("first", &[list], &mut ctx).unwrap();
    assert_eq!(r, ExprValue::Int(42));
}

#[test]
fn phase3_generic_with_coercion() {
    // Generic signature where arg needs coercion before matching
    fn list_of(_ctx: &mut dyn EvalContext, args: &[ExprValue]) -> Result<ExprValue, ExpressionError> {
        Ok(ExprValue::make_list(vec![args[0].clone()], args[0].expr_type())?)
    }
    let mut lib = FunctionLibrary::new();
    // Only accepts (float) → list[float], but we'll pass int
    lib.register_sig("list_of", "(float) -> list[float]", list_of);
    let mut ctx = MockCtx;
    let r = lib.call("list_of", &[ExprValue::Int(5)], &mut ctx).unwrap();
    assert!(r.is_list());
}

// ── call_method: skip receiver coercion ──

#[test]
fn call_method_skips_receiver_coercion() {
    // Method call should NOT coerce the receiver (first arg)
    let mut lib = FunctionLibrary::new();
    lib.register_sig("upper", "(string) -> string", identity);
    let mut ctx = MockCtx;
    // path.upper() as method → should fail because path receiver can't be coerced
    let path = ExprValue::Path { value: "/tmp".into(), format: PathFormat::Posix };
    let r = lib.call_method("upper", &[path], &mut ctx);
    assert!(r.is_err());
    assert!(r.unwrap_err().to_string().contains("not available for path"));
}

#[test]
fn call_method_coerces_non_receiver_args() {
    // Method call SHOULD coerce non-receiver args
    fn concat(_ctx: &mut dyn EvalContext, args: &[ExprValue]) -> Result<ExprValue, ExpressionError> {
        match (&args[0], &args[1]) {
            (ExprValue::String(a), ExprValue::String(b)) => Ok(ExprValue::String(format!("{a}{b}"))),
            _ => Err(ExpressionError::type_error("type error")),
        }
    }
    let mut lib = FunctionLibrary::new();
    lib.register_sig("concat", "(string, string) -> string", concat);
    let mut ctx = MockCtx;
    // "hello".concat(path) → path coerced to string for second arg
    let path = ExprValue::Path { value: "/tmp".into(), format: PathFormat::Posix };
    let r = lib.call_method("concat", &[ExprValue::String("hello".into()), path], &mut ctx).unwrap();
    assert_eq!(r, ExprValue::String("hello/tmp".into()));
}

#[test]
fn call_as_function_coerces_all_args() {
    // Function call (not method) SHOULD coerce all args including first
    let mut lib = FunctionLibrary::new();
    lib.register_sig("upper", "(string) -> string", identity);
    let mut ctx = MockCtx;
    let path = ExprValue::Path { value: "/tmp".into(), format: PathFormat::Posix };
    let r = lib.call("upper", &[path], &mut ctx).unwrap();
    assert_eq!(r, ExprValue::String("/tmp".into()));
}

// ── Error messages ──

#[test]
fn error_wrong_arg_count() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(int, int) -> int", custom_add);
    let mut ctx = MockCtx;
    let e = lib.call("f", &[ExprValue::Int(1)], &mut ctx).unwrap_err().to_string();
    assert!(e.contains("takes 2") && e.contains("1 were given"), "got: {e}");
}

#[test]
fn error_wrong_arg_count_multiple_arities() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(int) -> int", identity);
    lib.register_sig("f", "(int, int, int) -> int", identity);
    let mut ctx = MockCtx;
    let e = lib.call("f", &[ExprValue::Int(1), ExprValue::Int(2)], &mut ctx).unwrap_err().to_string();
    assert!(e.contains("1, 3") && e.contains("2 were given"), "got: {e}");
}

#[test]
fn error_unknown_function() {
    let lib = FunctionLibrary::new();
    let mut ctx = MockCtx;
    let e = lib.call("nonexistent", &[ExprValue::Int(1)], &mut ctx).unwrap_err().to_string();
    assert!(e.contains("nonexistent"), "got: {e}");
}

#[test]
fn error_operator_type_mismatch() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("__add__", "(int, int) -> int", custom_add);
    let mut ctx = MockCtx;
    let e = lib.call("__add__", &[ExprValue::String("a".into()), ExprValue::Int(1)], &mut ctx).unwrap_err().to_string();
    assert!(e.contains("'+'") && e.contains("string") && e.contains("int"), "got: {e}");
}

#[test]
fn error_method_wrong_receiver_type() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("upper", "(string) -> string", identity);
    let mut ctx = MockCtx;
    let e = lib.call_method("upper", &[ExprValue::Int(42)], &mut ctx).unwrap_err().to_string();
    assert!(e.contains("not available for int") && e.contains("string"), "got: {e}");
}

#[test]
fn error_no_matching_signature() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(int) -> int", identity);
    let mut ctx = MockCtx;
    let e = lib.call("f", &[ExprValue::String("x".into())], &mut ctx).unwrap_err().to_string();
    assert!(e.contains("No matching signature") && e.contains("f(string)"), "got: {e}");
}

// ── Unresolved dispatch ──

#[test]
fn unresolved_exact_match_returns_unresolved() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(int, int) -> string", identity);
    let mut ctx = MockCtx;
    let r = lib.call("f", &[ExprValue::unresolved(ExprType::INT), ExprValue::Int(1)], &mut ctx).unwrap();
    assert!(r.is_unresolved());
    assert_eq!(r.expr_type(), ExprType::unresolved(ExprType::STRING));
}

#[test]
fn unresolved_coerced_match_returns_unresolved() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(float, float) -> float", add_float);
    let mut ctx = MockCtx;
    // unresolved(int) + float → coerce → unresolved(float)
    let r = lib.call("f", &[ExprValue::unresolved(ExprType::INT), ExprValue::Float(Float64::new(1.0).unwrap())], &mut ctx).unwrap();
    assert!(r.is_unresolved());
    assert_eq!(r.expr_type(), ExprType::unresolved(ExprType::FLOAT));
}

#[test]
fn unresolved_generic_returns_substituted_type() {
    fn first(_ctx: &mut dyn EvalContext, args: &[ExprValue]) -> Result<ExprValue, ExpressionError> {
        Ok(args[0].clone())
    }
    let mut lib = FunctionLibrary::new();
    lib.register_sig("first", "(list[T1]) -> T1", first);
    let mut ctx = MockCtx;
    let r = lib.call("first", &[ExprValue::unresolved(ExprType::list(ExprType::STRING))], &mut ctx).unwrap();
    assert!(r.is_unresolved());
    assert_eq!(r.expr_type(), ExprType::unresolved(ExprType::STRING));
}

// ── derive_return_type ──

#[test]
fn derive_return_type_no_match_returns_none() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(int) -> int", identity);
    assert_eq!(lib.derive_return_type("f", &[ExprType::STRING]), None);
}

#[test]
fn derive_return_type_unknown_function_returns_none() {
    let lib = FunctionLibrary::new();
    assert_eq!(lib.derive_return_type("nonexistent", &[ExprType::INT]), None);
}

#[test]
fn derive_return_type_with_coercion() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(float) -> float", identity);
    // int → coerce to float
    assert_eq!(lib.derive_return_type("f", &[ExprType::INT]), Some(ExprType::FLOAT));
}

#[test]
fn derive_return_type_generic_substitution() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("first", "(list[T1]) -> T1", identity);
    assert_eq!(
        lib.derive_return_type("first", &[ExprType::list(ExprType::PATH)]),
        Some(ExprType::PATH)
    );
}

#[test]
fn derive_return_type_union_expansion() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(int) -> string", identity);
    lib.register_sig("f", "(float) -> path", identity);
    let union_arg = ExprType::union(vec![ExprType::INT, ExprType::FLOAT]);
    let result = lib.derive_return_type("f", &[union_arg]).unwrap();
    assert_eq!(result, ExprType::union(vec![ExprType::PATH, ExprType::STRING]));
}

// ── host context ──

#[test]
fn with_host_context_enables_apply_path_mapping() {
    let lib = openjd_expr::default_library::get_default_library().clone().with_host_context();
    assert!(!lib.get_signatures("apply_path_mapping").is_empty());
}

#[test]
fn without_host_context_no_apply_path_mapping() {
    let lib = openjd_expr::default_library::get_default_library();
    assert!(lib.get_signatures("apply_path_mapping").is_empty());
}

#[test]
fn with_unresolved_host_context_has_signatures() {
    let lib = openjd_expr::default_library::get_default_library().clone().with_unresolved_host_context();
    assert!(!lib.get_signatures("apply_path_mapping").is_empty());
}

// ── merge ──

#[test]
fn merge_combines_overloads() {
    let mut a = FunctionLibrary::new();
    a.register_sig("f", "(int) -> int", identity);
    let mut b = FunctionLibrary::new();
    b.register_sig("f", "(string) -> string", identity);
    let merged = a.merge(b);
    assert_eq!(merged.get_signatures("f").len(), 2);
}

#[test]
fn merge_preserves_distinct_functions() {
    let mut a = FunctionLibrary::new();
    a.register_sig("foo", "(int) -> int", identity);
    let mut b = FunctionLibrary::new();
    b.register_sig("bar", "(int) -> int", identity);
    let merged = a.merge(b);
    assert_eq!(merged.get_signatures("foo").len(), 1);
    assert_eq!(merged.get_signatures("bar").len(), 1);
}

// ── End-to-end dispatch through evaluator ──

#[test]
fn evaluator_method_call_skips_receiver_coercion() {
    // path.startswith() should fail — startswith is for string, not path
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::Path { value: "/tmp/test".into(), format: PathFormat::Posix }).unwrap();
    let e = eval_err_with("P.startswith('/tmp')", &st);
    assert!(e.contains("not available for path"), "got: {e}");
}

#[test]
fn evaluator_function_call_coerces_path_to_string() {
    // startswith(path, str) as function call should coerce path → string
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::Path { value: "/tmp/test".into(), format: PathFormat::Posix }).unwrap();
    assert_eq!(eval_with("startswith(P, '/tmp')", &st), ExprValue::Bool(true));
}

#[test]
fn evaluator_int_float_coercion_in_arithmetic() {
    // 1 + 2.5 → coerce int to float
    assert_eq!(eval("1 + 2.5"), ExprValue::Float(Float64::new(3.5).unwrap()));
}

#[test]
fn evaluator_multiple_overloads_select_correct() {
    // len() works on string, list, path, range — each is a different overload
    assert_eq!(eval("len('hello')"), ExprValue::Int(5));
    assert_eq!(eval("len([1, 2, 3])"), ExprValue::Int(3));
    assert_eq!(eval("len(range_expr('1-10'))"), ExprValue::Int(10));
}
