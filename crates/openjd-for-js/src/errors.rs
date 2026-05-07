// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! ECMAScript error types wrapping OpenJD errors.

use openjd_model::ModelError;
use wasm_bindgen::prelude::*;

/// Map a ModelError to a typed JS error.
pub fn to_js_error(e: ModelError) -> JsError {
    JsError::new(&e.to_string())
}

/// Map an ExpressionError to a JS error.
pub fn expr_to_js_error(e: openjd_expr::ExpressionError) -> JsError {
    JsError::new(&e.to_string())
}

/// Map a FormatStringValidationError to a JS error.
pub fn fmt_to_js_error(e: openjd_expr::FormatStringValidationError) -> JsError {
    JsError::new(&e.to_string())
}

/// Map a RangeExprError to a JS error.
pub fn range_to_js_error(e: openjd_expr::RangeExprError) -> JsError {
    JsError::new(&e.to_string())
}

/// Map a SymbolTableError to a JS error.
pub fn symtab_to_js_error(e: openjd_expr::SymbolTableError) -> JsError {
    JsError::new(&e.to_string())
}

// Convenience: convert serde_wasm_bindgen errors
pub fn serde_wasm_to_js_error(e: serde_wasm_bindgen::Error) -> JsError {
    JsError::new(&format!("Serialization error: {e}"))
}
