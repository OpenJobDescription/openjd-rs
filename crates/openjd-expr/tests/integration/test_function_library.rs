// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for FunctionLibrary public API: dispatch phases, error messages,
//! method vs function coercion, host context, and type derivation.

use openjd_expr::function_library::{EvalContext, FunctionLibrary};
use openjd_expr::value::Float64;
use openjd_expr::*;

// ── Helpers ──

fn eval(expr: &str) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap()
}

#[allow(dead_code)]
fn eval_with(expr: &str, st: &SymbolTable) -> ExprValue {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(st))
        .unwrap()
}

#[allow(dead_code)]
fn eval_err_with(expr: &str, st: &SymbolTable) -> String {
    ParsedExpression::new(expr)
        .and_then(|p| p.evaluate(st))
        .unwrap_err()
        .to_string()
}

struct MockCtx;
impl EvalContext for MockCtx {
    fn path_format(&self) -> PathFormat {
        PathFormat::Posix
    }
    fn count_op(&mut self) -> Result<(), ExpressionError> {
        Ok(())
    }
    fn count_ops(&mut self, _n: usize) -> Result<(), ExpressionError> {
        Ok(())
    }
    fn count_string_ops(&mut self, _len: usize) -> Result<(), ExpressionError> {
        Ok(())
    }
    fn check_memory(&self, _bytes: usize) -> Result<(), ExpressionError> {
        Ok(())
    }
}

fn custom_add(
    _ctx: &mut dyn EvalContext,
    args: &[ExprValue],
) -> Result<ExprValue, ExpressionError> {
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
        (ExprValue::Float(a), ExprValue::Float(b)) => {
            Ok(ExprValue::Float(Float64::new(a.value() + b.value())?))
        }
        _ => Err(ExpressionError::type_error("type error")),
    }
}

// ── Evaluator integration (original 2 tests) ──

#[test]
fn library_function_called_from_evaluator() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("custom_add", "(int, int) -> int", custom_add)
        .unwrap();

    let parsed = ParsedExpression::new("custom_add(3, 7)").unwrap();
    let st = SymbolTable::new();
    let result = parsed.with_library(&lib).evaluate(&[&st]).unwrap();
    assert_eq!(result, ExprValue::Int(307));
}

#[test]
fn library_function_with_unresolved() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("custom_add", "(int, int) -> int", custom_add)
        .unwrap();

    let mut st = SymbolTable::new();
    st.set("X", ExprValue::unresolved(ExprType::INT)).unwrap();
    let parsed = ParsedExpression::new("custom_add(X, 7)").unwrap();
    let result = parsed.with_library(&lib).evaluate(&[&st]).unwrap();
    assert!(result.is_unresolved());
    assert_eq!(result.expr_type(), ExprType::unresolved(ExprType::INT));
}

// ── 3-phase dispatch via call() ──

#[test]
fn phase1_exact_match_preferred_over_coercion() {
    // When both exact and coerced matches exist, exact wins
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(int, int) -> int", custom_add)
        .unwrap(); // exact: returns a*100+b
    lib.register_sig("f", "(float, float) -> float", add_float)
        .unwrap(); // coerced would go here
    let mut ctx = MockCtx;
    let r = lib
        .call("f", &[ExprValue::Int(2), ExprValue::Int(3)], &mut ctx)
        .unwrap();
    assert_eq!(r, ExprValue::Int(203)); // proves exact match ran, not coerced
}

#[test]
fn phase2_coercion_int_to_float() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(float, float) -> float", add_float)
        .unwrap();
    let mut ctx = MockCtx;
    // No exact (int,int) match → coerce both ints to float
    let r = lib
        .call("f", &[ExprValue::Int(2), ExprValue::Int(3)], &mut ctx)
        .unwrap();
    assert_eq!(r, ExprValue::Float(Float64::new(5.0).unwrap()));
}

#[test]
fn phase2_coercion_path_to_string() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(string) -> string", identity)
        .unwrap();
    let mut ctx = MockCtx;
    let path = ExprValue::new_path("/tmp/test", PathFormat::Posix);
    let r = lib.call("f", &[path], &mut ctx).unwrap();
    assert_eq!(r, ExprValue::String("/tmp/test".into()));
}

#[test]
fn phase3_generic_match() {
    fn first(_ctx: &mut dyn EvalContext, args: &[ExprValue]) -> Result<ExprValue, ExpressionError> {
        args[0]
            .list_get(0)
            .ok_or_else(|| ExpressionError::new("empty"))
    }
    let mut lib = FunctionLibrary::new();
    lib.register_sig("first", "(list[T1]) -> T1", first)
        .unwrap();
    let mut ctx = MockCtx;
    let list = ExprValue::make_list(vec![ExprValue::Int(42)], ExprType::INT).unwrap();
    let r = lib.call("first", &[list], &mut ctx).unwrap();
    assert_eq!(r, ExprValue::Int(42));
}

#[test]
fn phase3_generic_with_coercion() {
    // Generic signature where arg needs coercion before matching
    fn list_of(
        _ctx: &mut dyn EvalContext,
        args: &[ExprValue],
    ) -> Result<ExprValue, ExpressionError> {
        ExprValue::make_list(vec![args[0].clone()], args[0].expr_type())
    }
    let mut lib = FunctionLibrary::new();
    // Only accepts (float) → list[float], but we'll pass int
    lib.register_sig("list_of", "(float) -> list[float]", list_of)
        .unwrap();
    let mut ctx = MockCtx;
    let r = lib.call("list_of", &[ExprValue::Int(5)], &mut ctx).unwrap();
    assert!(r.is_list());
}

// ── call_method: skip receiver coercion ──

#[test]
fn call_method_skips_receiver_coercion() {
    // Method call should NOT coerce the receiver (first arg)
    let mut lib = FunctionLibrary::new();
    lib.register_sig("upper", "(string) -> string", identity)
        .unwrap();
    let mut ctx = MockCtx;
    // path.upper() as method → should fail because path receiver can't be coerced
    let path = ExprValue::new_path("/tmp", PathFormat::Posix);
    let r = lib.call_method("upper", &[path], &mut ctx);
    assert!(r.is_err());
    assert!(r
        .unwrap_err()
        .to_string()
        .contains("not available for path"));
}

#[test]
fn call_method_coerces_non_receiver_args() {
    // Method call SHOULD coerce non-receiver args
    fn concat(
        _ctx: &mut dyn EvalContext,
        args: &[ExprValue],
    ) -> Result<ExprValue, ExpressionError> {
        match (&args[0], &args[1]) {
            (ExprValue::String(a), ExprValue::String(b)) => {
                Ok(ExprValue::String(format!("{a}{b}")))
            }
            _ => Err(ExpressionError::type_error("type error")),
        }
    }
    let mut lib = FunctionLibrary::new();
    lib.register_sig("concat", "(string, string) -> string", concat)
        .unwrap();
    let mut ctx = MockCtx;
    // "hello".concat(path) → path coerced to string for second arg
    let path = ExprValue::new_path("/tmp", PathFormat::Posix);
    let r = lib
        .call_method(
            "concat",
            &[ExprValue::String("hello".into()), path],
            &mut ctx,
        )
        .unwrap();
    assert_eq!(r, ExprValue::String("hello/tmp".into()));
}

#[test]
fn call_as_function_coerces_all_args() {
    // Function call (not method) SHOULD coerce all args including first
    let mut lib = FunctionLibrary::new();
    lib.register_sig("upper", "(string) -> string", identity)
        .unwrap();
    let mut ctx = MockCtx;
    let path = ExprValue::new_path("/tmp", PathFormat::Posix);
    let r = lib.call("upper", &[path], &mut ctx).unwrap();
    assert_eq!(r, ExprValue::String("/tmp".into()));
}

// ── Error messages ──

#[test]
fn error_wrong_arg_count() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(int, int) -> int", custom_add)
        .unwrap();
    let mut ctx = MockCtx;
    let e = lib
        .call("f", &[ExprValue::Int(1)], &mut ctx)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("takes 2") && e.contains("1 were given"),
        "got: {e}"
    );
}

#[test]
fn error_wrong_arg_count_multiple_arities() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(int) -> int", identity).unwrap();
    lib.register_sig("f", "(int, int, int) -> int", identity)
        .unwrap();
    let mut ctx = MockCtx;
    let e = lib
        .call("f", &[ExprValue::Int(1), ExprValue::Int(2)], &mut ctx)
        .unwrap_err()
        .to_string();
    assert!(e.contains("1, 3") && e.contains("2 were given"), "got: {e}");
}

#[test]
fn error_unknown_function() {
    let lib = FunctionLibrary::new();
    let mut ctx = MockCtx;
    let e = lib
        .call("nonexistent", &[ExprValue::Int(1)], &mut ctx)
        .unwrap_err()
        .to_string();
    assert!(e.contains("nonexistent"), "got: {e}");
}

#[test]
fn error_operator_type_mismatch() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("__add__", "(int, int) -> int", custom_add)
        .unwrap();
    let mut ctx = MockCtx;
    let e = lib
        .call(
            "__add__",
            &[ExprValue::String("a".into()), ExprValue::Int(1)],
            &mut ctx,
        )
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("'+'") && e.contains("string") && e.contains("int"),
        "got: {e}"
    );
}

#[test]
fn error_method_wrong_receiver_type() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("upper", "(string) -> string", identity)
        .unwrap();
    let mut ctx = MockCtx;
    let e = lib
        .call_method("upper", &[ExprValue::Int(42)], &mut ctx)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("not available for int") && e.contains("string"),
        "got: {e}"
    );
}

#[test]
fn error_no_matching_signature() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(int) -> int", identity).unwrap();
    let mut ctx = MockCtx;
    let e = lib
        .call("f", &[ExprValue::String("x".into())], &mut ctx)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("No matching signature") && e.contains("f(string)"),
        "got: {e}"
    );
}

// ── Unresolved dispatch ──

#[test]
fn unresolved_exact_match_returns_unresolved() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(int, int) -> string", identity)
        .unwrap();
    let mut ctx = MockCtx;
    let r = lib
        .call(
            "f",
            &[ExprValue::unresolved(ExprType::INT), ExprValue::Int(1)],
            &mut ctx,
        )
        .unwrap();
    assert!(r.is_unresolved());
    assert_eq!(r.expr_type(), ExprType::unresolved(ExprType::STRING));
}

#[test]
fn unresolved_coerced_match_returns_unresolved() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(float, float) -> float", add_float)
        .unwrap();
    let mut ctx = MockCtx;
    // unresolved(int) + float → coerce → unresolved(float)
    let r = lib
        .call(
            "f",
            &[
                ExprValue::unresolved(ExprType::INT),
                ExprValue::Float(Float64::new(1.0).unwrap()),
            ],
            &mut ctx,
        )
        .unwrap();
    assert!(r.is_unresolved());
    assert_eq!(r.expr_type(), ExprType::unresolved(ExprType::FLOAT));
}

#[test]
fn unresolved_generic_returns_substituted_type() {
    fn first(_ctx: &mut dyn EvalContext, args: &[ExprValue]) -> Result<ExprValue, ExpressionError> {
        Ok(args[0].clone())
    }
    let mut lib = FunctionLibrary::new();
    lib.register_sig("first", "(list[T1]) -> T1", first)
        .unwrap();
    let mut ctx = MockCtx;
    let r = lib
        .call(
            "first",
            &[ExprValue::unresolved(ExprType::list(ExprType::STRING))],
            &mut ctx,
        )
        .unwrap();
    assert!(r.is_unresolved());
    assert_eq!(r.expr_type(), ExprType::unresolved(ExprType::STRING));
}

// ── derive_return_type ──

#[test]
fn derive_return_type_no_match_returns_none() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(int) -> int", identity).unwrap();
    assert_eq!(lib.derive_return_type("f", &[ExprType::STRING]), None);
}

#[test]
fn derive_return_type_unknown_function_returns_none() {
    let lib = FunctionLibrary::new();
    assert_eq!(
        lib.derive_return_type("nonexistent", &[ExprType::INT]),
        None
    );
}

#[test]
fn derive_return_type_with_coercion() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(float) -> float", identity).unwrap();
    // int → coerce to float
    assert_eq!(
        lib.derive_return_type("f", &[ExprType::INT]),
        Some(ExprType::FLOAT)
    );
}

#[test]
fn derive_return_type_generic_substitution() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("first", "(list[T1]) -> T1", identity)
        .unwrap();
    assert_eq!(
        lib.derive_return_type("first", &[ExprType::list(ExprType::PATH)]),
        Some(ExprType::PATH)
    );
}

#[test]
fn derive_return_type_union_expansion() {
    let mut lib = FunctionLibrary::new();
    lib.register_sig("f", "(int) -> string", identity).unwrap();
    lib.register_sig("f", "(float) -> path", identity).unwrap();
    let union_arg = ExprType::union(vec![ExprType::INT, ExprType::FLOAT]);
    let result = lib.derive_return_type("f", &[union_arg]).unwrap();
    assert_eq!(
        result,
        ExprType::union(vec![ExprType::PATH, ExprType::STRING])
    );
}

// ── host context ──

#[test]
fn with_host_context_enables_apply_path_mapping() {
    let lib = FunctionLibrary::for_profile(&ExprProfile::current().with_host_context(
        HostContext::with_rules(Vec::<openjd_expr::PathMappingRule>::new()),
    ));
    assert!(!lib.get_signatures("apply_path_mapping").is_empty());
}

#[test]
fn without_host_context_no_apply_path_mapping() {
    let lib = FunctionLibrary::for_profile(&ExprProfile::current());
    assert!(lib.get_signatures("apply_path_mapping").is_empty());
}

#[test]
fn with_unresolved_host_context_has_signatures() {
    let lib = FunctionLibrary::for_profile(
        &ExprProfile::current().with_host_context(HostContext::Unresolved),
    );
    assert!(!lib.get_signatures("apply_path_mapping").is_empty());
}

// ── merge ──

#[test]
fn merge_combines_overloads() {
    let mut a = FunctionLibrary::new();
    a.register_sig("f", "(int) -> int", identity).unwrap();
    let mut b = FunctionLibrary::new();
    b.register_sig("f", "(string) -> string", identity).unwrap();
    let merged = a.merge(b);
    assert_eq!(merged.get_signatures("f").len(), 2);
}

#[test]
fn merge_preserves_distinct_functions() {
    let mut a = FunctionLibrary::new();
    a.register_sig("foo", "(int) -> int", identity).unwrap();
    let mut b = FunctionLibrary::new();
    b.register_sig("bar", "(int) -> int", identity).unwrap();
    let merged = a.merge(b);
    assert_eq!(merged.get_signatures("foo").len(), 1);
    assert_eq!(merged.get_signatures("bar").len(), 1);
}

// ── End-to-end dispatch through evaluator ──

#[test]
fn evaluator_method_call_skips_receiver_coercion() {
    // path.startswith() should fail — startswith is for string, not path
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::new_path("/tmp/test", PathFormat::Posix))
        .unwrap();
    let parsed = ParsedExpression::new("P.startswith('/tmp')").unwrap();
    let symtabs = [&st];
    let e = parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&symtabs)
        .unwrap_err()
        .to_string();
    assert!(e.contains("not available for path"), "got: {e}");
}

#[test]
fn evaluator_function_call_coerces_path_to_string() {
    // startswith(path, str) as function call should coerce path → string
    let mut st = SymbolTable::new();
    st.set("P", ExprValue::new_path("/tmp/test", PathFormat::Posix))
        .unwrap();
    let parsed = ParsedExpression::new("startswith(P, '/tmp')").unwrap();
    assert_eq!(
        parsed
            .with_path_format(PathFormat::Posix)
            .evaluate(&[&st])
            .unwrap(),
        ExprValue::Bool(true)
    );
}

#[test]
fn evaluator_int_float_coercion_in_arithmetic() {
    // 1 + 2.5 → coerce int to float
    assert_eq!(
        eval("1 + 2.5"),
        ExprValue::Float(Float64::new(3.5).unwrap())
    );
}

#[test]
fn evaluator_multiple_overloads_select_correct() {
    // len() works on string, list, path, range — each is a different overload
    assert_eq!(eval("len('hello')"), ExprValue::Int(5));
    assert_eq!(eval("len([1, 2, 3])"), ExprValue::Int(3));
    assert_eq!(eval("len(range_expr('1-10'))"), ExprValue::Int(10));
}

// ── Refactor coverage: closure-based function registration ──

#[test]
fn register_accepts_bare_fn() {
    // Bare `fn` still works unchanged.
    fn double_int(_: &mut dyn EvalContext, a: &[ExprValue]) -> Result<ExprValue, ExpressionError> {
        match &a[0] {
            ExprValue::Int(n) => Ok(ExprValue::Int(n * 2)),
            _ => Err(ExpressionError::type_error("expected int")),
        }
    }

    let mut lib = FunctionLibrary::new();
    lib.register_sig("double", "(int) -> int", double_int)
        .unwrap();

    let mut ctx = MockCtx;
    let r = lib.call("double", &[ExprValue::Int(21)], &mut ctx).unwrap();
    assert!(matches!(r, ExprValue::Int(42)));
}

#[test]
fn register_accepts_closure_with_captured_state() {
    // Closures that capture `Send + Sync + 'static` state can now be registered.
    // This is the motivating use case for the Arc<dyn Fn> change.
    use std::sync::Arc;
    let offset: Arc<i64> = Arc::new(100);
    let offset_cloned = Arc::clone(&offset);

    let mut lib = FunctionLibrary::new();
    lib.register_sig(
        "add_offset",
        "(int) -> int",
        move |_ctx: &mut dyn EvalContext, a: &[ExprValue]| match &a[0] {
            ExprValue::Int(n) => Ok(ExprValue::Int(n + *offset_cloned)),
            _ => Err(ExpressionError::type_error("expected int")),
        },
    )
    .unwrap();

    let mut ctx = MockCtx;
    let r = lib
        .call("add_offset", &[ExprValue::Int(5)], &mut ctx)
        .unwrap();
    assert!(matches!(r, ExprValue::Int(105)));
    // offset is still live via the Arc in the FunctionLibrary.
    assert_eq!(*offset, 100);
}

#[test]
fn function_library_stays_clone_after_closure_registration() {
    // The Arc<dyn Fn> change preserved Clone on FunctionLibrary.
    let mut lib = FunctionLibrary::new();
    lib.register_sig(
        "id",
        "(int) -> int",
        |_: &mut dyn EvalContext, a: &[ExprValue]| Ok(a[0].clone()),
    )
    .unwrap();
    let cloned = lib.clone();

    let mut ctx = MockCtx;
    let r_orig = lib.call("id", &[ExprValue::Int(7)], &mut ctx).unwrap();
    let r_clone = cloned.call("id", &[ExprValue::Int(7)], &mut ctx).unwrap();
    assert!(matches!(r_orig, ExprValue::Int(7)));
    assert!(matches!(r_clone, ExprValue::Int(7)));
}

#[test]
fn function_library_is_send_sync_with_closure_impl() {
    // Verify the library can cross thread boundaries with a closure-registered
    // function — this would fail to compile if Arc<dyn Fn> weren't Send + Sync.
    fn assert_send_sync<T: Send + Sync>(_: &T) {}
    let mut lib = FunctionLibrary::new();
    lib.register_sig(
        "k",
        "(int) -> int",
        |_: &mut dyn EvalContext, _: &[ExprValue]| Ok(ExprValue::Int(0)),
    )
    .unwrap();
    assert_send_sync(&lib);
}

// ── End-to-end integration: external crate users registering custom functions ──
//
// These tests exercise the full public API path an external crate would use
// to add a custom function: build a FunctionLibrary, register an
// `impl Fn(&mut dyn EvalContext, &[ExprValue]) -> Result<_, _>`, and
// evaluate a ParsedExpression through `with_library(&lib).evaluate(&[&st])`.
// This is the path our own openjd-model and openjd-sessions use, and what
// any third-party extension would do.

#[test]
fn external_custom_fn_evaluates_via_with_library() {
    fn triple(_: &mut dyn EvalContext, a: &[ExprValue]) -> Result<ExprValue, ExpressionError> {
        match &a[0] {
            ExprValue::Int(n) => Ok(ExprValue::Int(n * 3)),
            _ => Err(ExpressionError::type_error("triple expects int")),
        }
    }
    // Start from the default library so `+`, literals, etc. still work.
    let mut lib = (*FunctionLibrary::for_profile(&ExprProfile::current())).clone();
    lib.register_sig("triple", "(int) -> int", triple).unwrap();

    let parsed = ParsedExpression::new("triple(7) + 1").unwrap();
    let r = parsed
        .with_library(&lib)
        .evaluate(&[&SymbolTable::new()])
        .unwrap();
    assert_eq!(r, ExprValue::Int(22));
}

#[test]
fn external_closure_fn_with_captured_state_evaluates() {
    // Closure captures a scale factor; verify end-to-end that the captured
    // state survives through the evaluator, not just a direct lib.call.
    use std::sync::Arc;
    let scale: Arc<i64> = Arc::new(10);
    let scale_cloned = Arc::clone(&scale);

    let mut lib = (*FunctionLibrary::for_profile(&ExprProfile::current())).clone();
    lib.register_sig(
        "scaled",
        "(int) -> int",
        move |_ctx: &mut dyn EvalContext, a: &[ExprValue]| match &a[0] {
            ExprValue::Int(n) => Ok(ExprValue::Int(n * *scale_cloned)),
            _ => Err(ExpressionError::type_error("scaled expects int")),
        },
    )
    .unwrap();

    let parsed = ParsedExpression::new("scaled(5) + scaled(2)").unwrap();
    let r = parsed
        .with_library(&lib)
        .evaluate(&[&SymbolTable::new()])
        .unwrap();
    assert_eq!(r, ExprValue::Int(70));
    drop(scale); // original Arc drop is safe; lib still holds its clone.
}

#[test]
fn external_custom_fn_reads_path_format_from_evalcontext() {
    use std::sync::{Arc, Mutex};
    // Confirm that an external function accessing `ctx.path_format()` sees the
    // value the user configured via `.with_path_format(...)` on the builder.
    let seen: Arc<Mutex<Option<openjd_expr::PathFormat>>> = Arc::new(Mutex::new(None));
    let seen_cloned = Arc::clone(&seen);

    let mut lib = (*FunctionLibrary::for_profile(&ExprProfile::current())).clone();
    lib.register_sig(
        "probe",
        "(int) -> int",
        move |ctx: &mut dyn EvalContext, a: &[ExprValue]| {
            *seen_cloned.lock().unwrap() = Some(ctx.path_format());
            Ok(a[0].clone())
        },
    )
    .unwrap();

    let parsed = ParsedExpression::new("probe(1)").unwrap();
    let _ = parsed
        .with_library(&lib)
        .with_path_format(openjd_expr::PathFormat::Windows)
        .evaluate(&[&SymbolTable::new()])
        .unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        Some(openjd_expr::PathFormat::Windows)
    );
}

#[test]
fn external_custom_fn_error_propagates_through_evaluator() {
    fn always_fail(_: &mut dyn EvalContext, _: &[ExprValue]) -> Result<ExprValue, ExpressionError> {
        Err(ExpressionError::new("boom"))
    }
    let mut lib = (*FunctionLibrary::for_profile(&ExprProfile::current())).clone();
    lib.register_sig("always_fail", "(int) -> int", always_fail)
        .unwrap();

    let parsed = ParsedExpression::new("always_fail(1)").unwrap();
    let err = parsed
        .with_library(&lib)
        .evaluate(&[&SymbolTable::new()])
        .unwrap_err();
    // Evaluator attaches source + caret to the bare error so the user sees
    // where the failure occurred in their expression.
    let msg = err.to_string();
    assert!(msg.contains("boom"), "missing message: {msg}");
    assert!(msg.contains("always_fail(1)"), "missing source: {msg}");
    assert!(msg.contains('^'), "missing caret: {msg}");
}

#[test]
fn external_custom_fn_respects_operation_limit_via_count_op() {
    // An external function that calls `ctx.count_op()` inside a tight loop
    // gets properly cut off when the user sets a tight operation limit.
    fn busy_loop(ctx: &mut dyn EvalContext, _: &[ExprValue]) -> Result<ExprValue, ExpressionError> {
        for _ in 0..1_000_000 {
            ctx.count_op()?;
        }
        Ok(ExprValue::Int(0))
    }
    let mut lib = (*FunctionLibrary::for_profile(&ExprProfile::current())).clone();
    lib.register_sig("busy_loop", "(int) -> int", busy_loop)
        .unwrap();

    let parsed = ParsedExpression::new("busy_loop(0)").unwrap();
    let err = parsed
        .with_library(&lib)
        .with_operation_limit(100)
        .evaluate(&[&SymbolTable::new()])
        .unwrap_err();
    assert!(matches!(
        err.kind(),
        openjd_expr::ExpressionErrorKind::OperationLimitExceeded { .. }
    ));
}

// === F2: register_sig must return Result, not panic on bad signatures ===

#[test]
fn register_sig_valid_signature_succeeds() {
    let mut lib = FunctionLibrary::new();
    let result = lib.register_sig("len", "(string) -> int", |_, args| {
        Ok(args.first().cloned().unwrap_or(ExprValue::Null))
    });
    assert!(result.is_ok());
    assert_eq!(lib.get_signatures("len").len(), 1);
}

#[test]
fn register_sig_invalid_signature_returns_error() {
    let mut lib = FunctionLibrary::new();
    let result = lib.register_sig("bad", "not a valid signature!!!", |_, _| {
        Ok(ExprValue::Null)
    });
    assert!(result.is_err(), "malformed signature should return Err");
    // The function should NOT be registered
    assert_eq!(lib.get_signatures("bad").len(), 0);
}

#[test]
fn register_sig_empty_signature_returns_error() {
    let mut lib = FunctionLibrary::new();
    let result = lib.register_sig("bad", "", |_, _| Ok(ExprValue::Null));
    assert!(result.is_err());
}
