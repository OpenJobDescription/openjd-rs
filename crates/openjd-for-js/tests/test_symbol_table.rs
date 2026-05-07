// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests for the redesigned `SymbolTable` JS surface.
//!
//! Resolves F6: replace the two-arg `(scope, name)` split with a
//! single dotted-key API that delegates to the underlying
//! `openjd_expr::SymbolTable::set`, which correctly returns an
//! error when nesting would overwrite a scalar.
//!
//! Also aligns with the Python bindings' `__setitem__` / `__getitem__`
//! / `__contains__` / `get` surface (see
//! openjd-model-for-python/rust/src/expr/symbol_table.rs).

use openjd_for_js::expr::{JsExprValue, JsSymbolTable};

// ── Round-trip via dotted keys ──────────────────────────────────────

#[test]
fn set_string_and_get_round_trip() {
    let mut st = JsSymbolTable::new();
    st.set_string_rs("Param.Frames", "1-10")
        .expect("set_string must succeed at a fresh dotted key");
    let got = st.get("Param.Frames").expect("must retrieve what was set");
    assert_eq!(got.to_display_string(), "1-10");
}

#[test]
fn set_expr_value_and_get_round_trip() {
    let mut st = JsSymbolTable::new();
    let v = JsExprValue::from_int(42);
    st.set_rs("Param.Count", &v)
        .expect("set must accept a dotted key");
    let got = st.get("Param.Count").expect("must retrieve what was set");
    assert_eq!(got.to_display_string(), "42");
}

#[test]
fn has_returns_true_for_existing_scalar_and_false_otherwise() {
    let mut st = JsSymbolTable::new();
    st.set_string_rs("Param.Frames", "1-10").unwrap();
    assert!(st.has("Param.Frames"));
    assert!(!st.has("Param.Missing"));
    // A dotted key that doesn't exist at the leaf level returns false
    // even if the intermediate scope does.
    assert!(!st.has("Param"));
}

#[test]
fn get_returns_none_for_unset_key() {
    let st = JsSymbolTable::new();
    assert!(st.get("Param.Missing").is_none());
}

#[test]
fn all_paths_enumerates_all_leaves() {
    let mut st = JsSymbolTable::new();
    st.set_string_rs("Param.A", "1").unwrap();
    st.set_string_rs("Param.B", "2").unwrap();
    st.set_string_rs("RawParam.X", "3").unwrap();
    let paths = st.all_paths();
    assert!(paths.contains(&"Param.A".to_string()));
    assert!(paths.contains(&"Param.B".to_string()));
    assert!(paths.contains(&"RawParam.X".to_string()));
    assert_eq!(paths.len(), 3);
}

// ── F6 regression guards: loud failures on collision ────────────────

/// Setting `"A.B"` after `"A"` has already been set to a scalar must
/// return an error. The Rust `SymbolTable::set` detects this and
/// emits a `SymbolTableError`; the JS binding must propagate it as
/// a `JsError` rather than silently overwriting the scalar.
#[test]
fn set_rejects_nesting_under_existing_scalar() {
    let mut st = JsSymbolTable::new();
    st.set_string_rs("A", "leaf value").unwrap();

    let v = JsExprValue::from_int(1);
    let err = st
        .set_rs("A.B", &v)
        .expect_err("must reject nesting under a scalar");
    assert!(
        err.contains("A"),
        "error must reference the conflicting key; got: {err}"
    );
}

/// `setString` has the same collision behavior.
#[test]
fn set_string_rejects_nesting_under_existing_scalar() {
    let mut st = JsSymbolTable::new();
    st.set_string_rs("A", "scalar").unwrap();

    let err = st
        .set_string_rs("A.Nested", "child")
        .expect_err("must reject nesting under a scalar");
    assert!(
        err.contains("A"),
        "error must reference the conflicting key; got: {err}"
    );
}

/// Setting a scalar at a key that already holds a subtable must also
/// fail — the reverse collision direction.
#[test]
fn set_rejects_scalar_on_existing_subtable_key() {
    let mut st = JsSymbolTable::new();
    st.set_string_rs("Param.Frames", "1-10").unwrap();
    // Now `Param` is a subtable. Trying to set `Param` as a scalar
    // must fail.
    let v = JsExprValue::from_int(99);
    let err = st
        .set_rs("Param", &v)
        .expect_err("must reject scalar assignment over existing subtable");
    assert!(
        err.contains("Param"),
        "error must reference the conflicting key; got: {err}"
    );
}

/// Deep-nesting round-trip: `"A.B.C"` works when no conflicts exist.
#[test]
fn deep_nesting_round_trip() {
    let mut st = JsSymbolTable::new();
    st.set_string_rs("A.B.C", "deep").unwrap();
    let got = st.get("A.B.C").expect("must retrieve deeply-nested value");
    assert_eq!(got.to_display_string(), "deep");
    assert!(st.has("A.B.C"));
    assert!(!st.has("A.B")); // intermediate is a subtable, not a leaf
    assert!(!st.has("A"));
}

/// Overwriting a scalar at the same dotted key is fine — this is the
/// normal update path. Only conflicts between scalar and subtable
/// shapes error.
#[test]
fn overwriting_scalar_at_same_key_is_allowed() {
    let mut st = JsSymbolTable::new();
    st.set_string_rs("Param.X", "old").unwrap();
    st.set_string_rs("Param.X", "new")
        .expect("overwriting a scalar at the same key is allowed");
    assert_eq!(st.get("Param.X").unwrap().to_display_string(), "new");
}

// ── Python parity — dotted-key surface ──────────────────────────────

/// The Python bindings expose `__setitem__(dotted_key, value)`. The
/// JS binding's `set` / `setString` / `get` / `has` must accept the
/// same dotted-key shape for parity.
#[test]
fn single_dotted_key_matches_python_setitem_shape() {
    let mut st = JsSymbolTable::new();
    // Python: st["Param.Frames"] = "1-10"
    st.set_string_rs("Param.Frames", "1-10").unwrap();
    // Python: "Param.Frames" in st
    assert!(st.has("Param.Frames"));
    // Python: st["Param.Frames"]
    assert_eq!(st.get("Param.Frames").unwrap().to_display_string(), "1-10");
}
