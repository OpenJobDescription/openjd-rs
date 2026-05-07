// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests for `EvalOptions` — the host-configurable caps that bound
//! memory and operation use during expression evaluation.
//!
//! Resolves F5 (Medium) and F9 (Low) from the security review:
//!
//! * F5: `evaluateExpression` and `ParsedExpression.evaluate` must
//!   accept per-call memory/operation limit overrides so hosts can
//!   tighten below the defaults.
//! * F9: the `getDefaultMemoryLimit` / `getDefaultOperationLimit`
//!   getters stay — informational. Adding the setter surface
//!   through `EvalOptions` resolves F9 as a by-catch.
//!
//! `EvalOptions` mirrors the F4 `CallerLimits` design: a
//! plain-object/structural type at the JS boundary, so callers can
//! construct literals and reuse them across calls without
//! ownership ceremony.

use openjd_for_js::expr::{
    evaluate_expression_str, parsed_expression_evaluate, JsEvalOptions, JsSymbolTable,
};

// ── Constructor + defaults ──────────────────────────────────────────

#[test]
fn default_is_all_none() {
    let opts = JsEvalOptions::default();
    assert!(opts.memory_limit.is_none());
    assert!(opts.operation_limit.is_none());

    let via_new = JsEvalOptions::new();
    assert!(via_new.memory_limit.is_none());
    assert!(via_new.operation_limit.is_none());
}

#[test]
fn fields_round_trip() {
    let mut opts = JsEvalOptions::new();
    opts.memory_limit = Some(50_000_000);
    opts.operation_limit = Some(1_000_000);
    assert_eq!(opts.memory_limit, Some(50_000_000));
    assert_eq!(opts.operation_limit, Some(1_000_000));

    opts.memory_limit = None;
    assert!(opts.memory_limit.is_none());
}

#[test]
fn deserializes_camel_case_keys() {
    let json = r#"{ "memoryLimit": 100, "operationLimit": 200 }"#;
    let opts: JsEvalOptions = serde_json::from_str(json).expect("camelCase keys deserialize");
    assert_eq!(opts.memory_limit, Some(100));
    assert_eq!(opts.operation_limit, Some(200));
}

#[test]
fn rejects_unknown_fields_via_serde_json() {
    // serde_wasm_bindgen does not enforce deny_unknown_fields from a
    // JS object (documented limitation from F4). This test verifies
    // that the Rust-side intent is preserved via `serde_json`.
    let json = r#"{"memoryLimit": 1, "bogus": 99}"#;
    let err = serde_json::from_str::<JsEvalOptions>(json)
        .expect_err("unknown field must be rejected by serde_json");
    assert!(
        err.to_string().contains("bogus") || err.to_string().contains("unknown"),
        "expected unknown-field error, got: {err}"
    );
}

// ── F5 regression guards: limits enforced end-to-end ────────────────

/// A trivial expression runs fine under a very small memory budget —
/// shows the plumbing works and doesn't over-apply the cap.
#[test]
fn evaluate_under_default_limits_works() {
    let symbols = JsSymbolTable::new();
    evaluate_expression_str("1 + 1", &symbols, None, None)
        .expect("trivial expression evaluates under defaults");
}

/// `memoryLimit` override rejects expressions that would exceed the
/// cap. A string-repeat that allocates ~1 MB must fail under a
/// 1000-byte memory limit.
///
/// This is the exact attack scenario the review described: a tight
/// budget lets a browser tab reject poisoned input quickly instead
/// of burning through the 100 MB default first.
#[test]
fn evaluate_respects_memory_limit_override() {
    let symbols = JsSymbolTable::new();
    let opts = JsEvalOptions {
        memory_limit: Some(1_000),
        operation_limit: None,
    };
    let err = match evaluate_expression_str("'x' * 1000000", &symbols, None, Some(&opts)) {
        Ok(v) => panic!(
            "must reject memory-blowing expression; got value: {}",
            v.to_display_string()
        ),
        Err(e) => e,
    };
    assert!(
        err.to_lowercase().contains("memory"),
        "expected memory-limit error, got: {err}"
    );
}

/// `operationLimit` override rejects expressions that exceed the
/// operation budget. A list comprehension with many iterations
/// must fail under a small operation cap.
#[test]
fn evaluate_respects_operation_limit_override() {
    let symbols = JsSymbolTable::new();
    let opts = JsEvalOptions {
        memory_limit: None,
        operation_limit: Some(10),
    };
    let err = match evaluate_expression_str(
        "sum([i for i in range(10000)])",
        &symbols,
        None,
        Some(&opts),
    ) {
        Ok(v) => panic!(
            "must reject operation-exhausting expression; got value: {}",
            v.to_display_string()
        ),
        Err(e) => e,
    };
    assert!(
        err.to_lowercase().contains("operation"),
        "expected operation-limit error, got: {err}"
    );
}

/// Same expression under the default (no override) succeeds, proving
/// the limit is opt-in.
#[test]
fn evaluate_without_override_uses_defaults() {
    let symbols = JsSymbolTable::new();
    let result = evaluate_expression_str("sum([i for i in range(100)])", &symbols, None, None)
        .expect("modest sum under defaults succeeds");
    // result is 0+1+...+99 = 4950.
    assert_eq!(result.to_display_string(), "4950");
}

/// `None` options must behave identically to an empty `EvalOptions`
/// literal — neither overrides any default.
#[test]
fn evaluate_none_and_empty_options_are_equivalent() {
    let symbols = JsSymbolTable::new();
    let r1 = evaluate_expression_str("1 + 1", &symbols, None, None).unwrap();
    let r2 = evaluate_expression_str("1 + 1", &symbols, None, Some(&JsEvalOptions::new())).unwrap();
    assert_eq!(r1.to_display_string(), r2.to_display_string());
}

// ── ParsedExpression.evaluate parity ────────────────────────────────

/// `ParsedExpression.evaluate` must accept the same `EvalOptions`
/// shape as `evaluateExpression` — same plumbing under a different
/// entry point.
#[test]
fn parsed_expression_respects_memory_limit() {
    let symbols = JsSymbolTable::new();
    let opts = JsEvalOptions {
        memory_limit: Some(1_000),
        operation_limit: None,
    };
    let err = match parsed_expression_evaluate("'x' * 1000000", &symbols, None, Some(&opts)) {
        Ok(_) => panic!("must reject memory-blowing expression"),
        Err(e) => e,
    };
    assert!(
        err.to_lowercase().contains("memory"),
        "expected memory-limit error, got: {err}"
    );
}

#[test]
fn parsed_expression_without_override_uses_defaults() {
    let symbols = JsSymbolTable::new();
    let result = parsed_expression_evaluate("2 + 3", &symbols, None, None).unwrap();
    assert_eq!(result.to_display_string(), "5");
}
