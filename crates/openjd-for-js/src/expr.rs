// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Expression engine bindings: ExprValue, FormatString, SymbolTable,
//! FunctionLibrary, PathMappingRule, ParsedExpression.

use crate::errors::*;
use wasm_bindgen::prelude::*;

// ── EvalOptions ────────────────────────────────────────────────────

/// Optional per-call caps for expression evaluation. Mirrors the
/// relevant knobs on [`openjd_expr::EvalBuilder`]: `memory_limit`
/// and `operation_limit`.
///
/// Exposed to JS as a plain structural type (same shape pattern as
/// [`crate::model::JsCallerLimits`]). Callers pass object literals
/// and reuse them freely — no `.free()` ceremony, no by-value
/// handle consumption.
///
/// ```ignore
/// mod.evaluateExpression(expr, symbols, undefined, {
///     memoryLimit: 50_000_000,   // 50 MB
///     operationLimit: 1_000_000, // 1M ops
/// });
/// ```
///
/// Every field is optional; `undefined` (the default) means "use
/// the spec-default budget from `openjd_expr`". The defaults are
/// accessible via [`get_default_memory_limit`] and
/// [`get_default_operation_limit`], mostly for informational use.
///
/// Resolves review findings F5 (Medium) and F9 (Low).
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JsEvalOptions {
    #[serde(default)]
    pub memory_limit: Option<usize>,
    #[serde(default)]
    pub operation_limit: Option<usize>,
}

impl JsEvalOptions {
    /// Factory used by rlib tests to build an empty options value.
    /// JS callers use the plain-object literal form instead.
    pub fn new() -> JsEvalOptions {
        JsEvalOptions::default()
    }

    /// Deserialize a JS value into `JsEvalOptions`. Returns `None`
    /// when the caller passes `undefined`/`null` (the "use defaults"
    /// sentinel); returns `Err` when the value is present but
    /// malformed.
    pub fn from_js_value(value: JsValue) -> Result<Option<JsEvalOptions>, JsError> {
        if value.is_undefined() || value.is_null() {
            return Ok(None);
        }
        let opts: JsEvalOptions =
            serde_wasm_bindgen::from_value(value).map_err(serde_wasm_to_js_error)?;
        Ok(Some(opts))
    }
}

// ── ExprValue ──────────────────────────────────────────────────────

/// An expression value (string, int, float, bool, path, list, range).
#[wasm_bindgen(js_name = "ExprValue")]
pub struct JsExprValue {
    pub(crate) inner: openjd_expr::ExprValue,
}

#[wasm_bindgen(js_class = "ExprValue")]
impl JsExprValue {
    /// Create a string value.
    #[wasm_bindgen(js_name = "string")]
    pub fn from_string(v: &str) -> JsExprValue {
        JsExprValue {
            inner: openjd_expr::ExprValue::String(v.to_string()),
        }
    }

    /// Create an integer value.
    #[wasm_bindgen(js_name = "int")]
    pub fn from_int(v: i64) -> JsExprValue {
        JsExprValue {
            inner: openjd_expr::ExprValue::Int(v),
        }
    }

    /// Create a float value.
    #[wasm_bindgen(js_name = "float")]
    pub fn from_float(v: f64) -> Result<JsExprValue, JsError> {
        let f = openjd_expr::value::Float64::new(v).map_err(expr_to_js_error)?;
        Ok(JsExprValue {
            inner: openjd_expr::ExprValue::Float(f),
        })
    }

    /// Create a boolean value.
    #[wasm_bindgen(js_name = "bool")]
    pub fn from_bool(v: bool) -> JsExprValue {
        JsExprValue {
            inner: openjd_expr::ExprValue::Bool(v),
        }
    }

    /// Create a path value.
    #[wasm_bindgen(js_name = "path")]
    pub fn from_path(v: &str, format: JsPathFormat) -> JsExprValue {
        JsExprValue {
            inner: openjd_expr::ExprValue::new_path(v, format.into_inner()),
        }
    }

    /// Get the type name.
    #[wasm_bindgen(getter, js_name = "type")]
    pub fn expr_type(&self) -> String {
        self.inner.type_name().to_string()
    }

    /// Convert to a display string.
    #[wasm_bindgen(js_name = "toString")]
    pub fn to_display_string(&self) -> String {
        self.inner.to_display_string()
    }

    /// Convert to a native JS value via JSON.
    #[wasm_bindgen(js_name = "toJSON")]
    pub fn to_json(&self) -> Result<JsValue, JsError> {
        serde_wasm_bindgen::to_value(&self.inner.to_json_transport())
            .map_err(serde_wasm_to_js_error)
    }
}

// ── PathFormat ─────────────────────────────────────────────────────

/// Path format: Posix, Windows, or URI. Mirrors [`openjd_expr::PathFormat`].
#[wasm_bindgen(js_name = "PathFormat")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JsPathFormat {
    Posix = 0,
    Windows = 1,
    Uri = 2,
}

impl JsPathFormat {
    pub fn into_inner(self) -> openjd_expr::PathFormat {
        match self {
            JsPathFormat::Posix => openjd_expr::PathFormat::Posix,
            JsPathFormat::Windows => openjd_expr::PathFormat::Windows,
            JsPathFormat::Uri => openjd_expr::PathFormat::Uri,
        }
    }

    pub fn from_inner(f: openjd_expr::PathFormat) -> Self {
        match f {
            openjd_expr::PathFormat::Posix => JsPathFormat::Posix,
            openjd_expr::PathFormat::Windows => JsPathFormat::Windows,
            openjd_expr::PathFormat::Uri => JsPathFormat::Uri,
        }
    }
}

// ── PathMappingRule ────────────────────────────────────────────────

/// A path mapping rule for the function library.
#[wasm_bindgen(js_name = "PathMappingRule")]
pub struct JsPathMappingRule {
    pub(crate) inner: openjd_expr::PathMappingRule,
}

#[wasm_bindgen(js_class = "PathMappingRule")]
impl JsPathMappingRule {
    #[wasm_bindgen(constructor)]
    pub fn new(
        source_format: JsPathFormat,
        source_path: &str,
        dest_path: &str,
    ) -> JsPathMappingRule {
        JsPathMappingRule {
            inner: openjd_expr::PathMappingRule {
                source_path_format: source_format.into_inner(),
                source_path: source_path.to_string(),
                destination_path: dest_path.to_string(),
            },
        }
    }
}

// ── SymbolTable ────────────────────────────────────────────────────

/// A symbol table for format string resolution and expression evaluation.
#[wasm_bindgen(js_name = "SymbolTable")]
pub struct JsSymbolTable {
    pub(crate) inner: openjd_expr::SymbolTable,
}

#[wasm_bindgen(js_class = "SymbolTable")]
#[allow(clippy::new_without_default)]
impl JsSymbolTable {
    #[wasm_bindgen(constructor)]
    pub fn new() -> JsSymbolTable {
        JsSymbolTable {
            inner: openjd_expr::SymbolTable::new(),
        }
    }

    /// Set an `ExprValue` at a dotted key.
    ///
    /// Dotted keys work like Python's `symtab["Param.Frames"] = ...`
    /// — intermediate tables are created as needed. Mirrors the
    /// Rust [`openjd_expr::SymbolTable::set`] API and the Python
    /// binding's `__setitem__`.
    ///
    /// Returns an error if the key would collide with an existing
    /// entry of the wrong shape — setting `"A.B"` when `"A"` already
    /// holds a scalar, or setting `"A"` as a scalar when `"A.B"` is
    /// already a subtable. Resolves review finding F6 ("silent
    /// failures are worse than loud failures").
    pub fn set(&mut self, key: &str, value: &JsExprValue) -> Result<(), JsError> {
        self.inner
            .set(key, value.inner.clone())
            .map_err(symtab_to_js_error)
    }

    /// Set a string value at a dotted key. Convenience for the
    /// common case of parameter values arriving as strings from
    /// the host — see [`Self::set`] for key and collision semantics.
    #[wasm_bindgen(js_name = "setString")]
    pub fn set_string(&mut self, key: &str, value: &str) -> Result<(), JsError> {
        self.inner
            .set_string(key, value)
            .map_err(symtab_to_js_error)
    }

    /// Get the value at a dotted key, or `undefined` if the key is
    /// unset or resolves to a subtable rather than a leaf value.
    pub fn get(&self, key: &str) -> Option<JsExprValue> {
        match self.inner.get(key) {
            Some(openjd_expr::symbol_table::SymbolTableEntry::Value(v)) => {
                Some(JsExprValue { inner: v.clone() })
            }
            _ => None,
        }
    }

    /// True if the dotted key resolves to a leaf value (not a
    /// subtable, not missing).
    pub fn has(&self, key: &str) -> bool {
        matches!(
            self.inner.get(key),
            Some(openjd_expr::symbol_table::SymbolTableEntry::Value(_))
        )
    }

    /// Get all leaf-value paths as dotted strings (e.g.,
    /// `["Param.Frames", "Param.OutputDir"]`).
    #[wasm_bindgen(js_name = "allPaths")]
    pub fn all_paths(&self) -> Vec<String> {
        self.inner.all_paths("")
    }
}

/// Rust-side helpers on `JsSymbolTable` used by rlib tests. Exposed
/// outside the `#[wasm_bindgen]` impl so they can return plain
/// `Result<_, String>` rather than `JsError`, which formats through
/// wasm-bindgen glue and panics on non-wasm targets.
impl JsSymbolTable {
    /// See [`Self::set`]. Returns `String` error for rlib tests.
    pub fn set_rs(&mut self, key: &str, value: &JsExprValue) -> Result<(), String> {
        self.inner
            .set(key, value.inner.clone())
            .map_err(|e| e.to_string())
    }

    /// See [`Self::set_string`]. Returns `String` error for rlib tests.
    pub fn set_string_rs(&mut self, key: &str, value: &str) -> Result<(), String> {
        self.inner.set_string(key, value).map_err(|e| e.to_string())
    }
}

// ── FormatString ───────────────────────────────────────────────────

/// A parsed format string (e.g., `"{{Param.Frames}}/output"`).
#[wasm_bindgen(js_name = "FormatString")]
pub struct JsFormatString {
    pub(crate) inner: openjd_expr::FormatString,
}

#[wasm_bindgen(js_class = "FormatString")]
impl JsFormatString {
    #[wasm_bindgen(constructor)]
    pub fn new(input: &str) -> Result<JsFormatString, JsError> {
        let fs = openjd_expr::FormatString::new(input).map_err(expr_to_js_error)?;
        Ok(JsFormatString { inner: fs })
    }

    /// The raw format string text.
    #[wasm_bindgen(getter)]
    pub fn raw(&self) -> String {
        self.inner.raw().to_string()
    }

    /// Resolve the format string against a symbol table.
    pub fn resolve(&self, symbols: &JsSymbolTable) -> Result<String, JsError> {
        let opts = openjd_expr::FormatStringOptions::new();
        self.inner
            .resolve_string_with(&symbols.inner, &opts)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Get referenced symbol names (e.g., ["Param.Frames"]).
    #[wasm_bindgen(getter)]
    pub fn references(&self) -> Vec<String> {
        self.inner.accessed_symbols().into_iter().collect()
    }

    /// Whether this is a literal string (no interpolations).
    #[wasm_bindgen(getter, js_name = "isLiteral")]
    pub fn is_literal(&self) -> bool {
        self.inner.is_literal()
    }

    /// Get expression names (the parts inside `{{}}`).
    #[wasm_bindgen(getter, js_name = "expressionNames")]
    pub fn expression_names(&self) -> Vec<String> {
        self.inner
            .expression_names()
            .into_iter()
            .map(|s| s.to_string())
            .collect()
    }
}

// ── FunctionLibrary ────────────────────────────────────────────────

/// Function library for expression evaluation.
#[wasm_bindgen(js_name = "FunctionLibrary")]
pub struct JsFunctionLibrary {
    pub(crate) inner: openjd_expr::FunctionLibrary,
}

#[wasm_bindgen(js_class = "FunctionLibrary")]
impl JsFunctionLibrary {
    /// Get the default function library with all builtins.
    #[wasm_bindgen(js_name = "default")]
    pub fn get_default() -> JsFunctionLibrary {
        // `for_profile` returns `Arc<FunctionLibrary>`; `JsFunctionLibrary`
        // owns an inner `FunctionLibrary` by value, so clone out of the Arc.
        let inner =
            (*openjd_expr::FunctionLibrary::for_profile(&openjd_expr::ExprProfile::current()))
                .clone();
        JsFunctionLibrary { inner }
    }

    /// Create a library with path mapping rules.
    #[wasm_bindgen(js_name = "withPathMappingRules")]
    pub fn with_path_mapping_rules(rules: Vec<JsPathMappingRule>) -> JsFunctionLibrary {
        let rust_rules: Vec<openjd_expr::PathMappingRule> =
            rules.into_iter().map(|r| r.inner).collect();
        let inner = (*openjd_expr::FunctionLibrary::for_profile(
            &openjd_expr::ExprProfile::current()
                .with_host_context(openjd_expr::HostContext::with_rules(rust_rules)),
        ))
        .clone();
        JsFunctionLibrary { inner }
    }
}

// ── ParsedExpression ───────────────────────────────────────────────

/// A parsed expression ready for evaluation.
#[wasm_bindgen(js_name = "ParsedExpression")]
pub struct JsParsedExpression {
    inner: openjd_expr::ParsedExpression,
}

#[wasm_bindgen(js_class = "ParsedExpression")]
impl JsParsedExpression {
    /// The expression text.
    #[wasm_bindgen(getter)]
    pub fn expression(&self) -> String {
        self.inner.expression().to_string()
    }

    /// Symbol names accessed by this expression.
    #[wasm_bindgen(getter, js_name = "accessedSymbols")]
    pub fn accessed_symbols(&self) -> Vec<String> {
        self.inner.accessed_symbols().iter().cloned().collect()
    }

    /// Evaluate the expression against symbol tables.
    ///
    /// `options` (optional, plain-object shape) lets the host cap
    /// `memoryLimit` and/or `operationLimit` for this specific call.
    /// Omitted / `undefined` / `{}` keeps the built-in defaults.
    pub fn evaluate(
        &self,
        symbols: &JsSymbolTable,
        library: Option<JsFunctionLibrary>,
        options: JsValue,
    ) -> Result<JsExprValue, JsError> {
        let parsed_opts = JsEvalOptions::from_js_value(options)?;
        let inner_result = evaluate_inner(
            &self.inner,
            &symbols.inner,
            library.as_ref().map(|lib| &lib.inner),
            parsed_opts.as_ref(),
        );
        inner_result
            .map(|v| JsExprValue { inner: v })
            .map_err(expr_to_js_error)
    }
}

/// Rust-native helper for evaluating an expression string directly.
/// Exposed so rlib tests can exercise the evaluation surface without
/// going through the `JsValue` boundary.
pub fn evaluate_expression_str(
    expr: &str,
    symbols: &JsSymbolTable,
    library: Option<&JsFunctionLibrary>,
    options: Option<&JsEvalOptions>,
) -> Result<JsExprValue, String> {
    let parsed = openjd_expr::ParsedExpression::new(expr).map_err(|e| e.to_string())?;
    let value = evaluate_inner(
        &parsed,
        &symbols.inner,
        library.map(|lib| &lib.inner),
        options,
    )
    .map_err(|e| e.to_string())?;
    Ok(JsExprValue { inner: value })
}

/// Rust-native helper for evaluating a pre-parsed expression. Mirrors
/// the public [`JsParsedExpression::evaluate`] surface without the
/// `JsValue` boundary.
pub fn parsed_expression_evaluate(
    expr: &str,
    symbols: &JsSymbolTable,
    library: Option<&JsFunctionLibrary>,
    options: Option<&JsEvalOptions>,
) -> Result<JsExprValue, String> {
    let parsed = openjd_expr::ParsedExpression::new(expr).map_err(|e| e.to_string())?;
    let value = evaluate_inner(
        &parsed,
        &symbols.inner,
        library.map(|lib| &lib.inner),
        options,
    )
    .map_err(|e| e.to_string())?;
    Ok(JsExprValue { inner: value })
}

/// Configure and run an evaluation. Private helper shared by the
/// free-function entry point and the `ParsedExpression::evaluate`
/// method. Applies each `with_*` on the builder only if the caller
/// supplied a value — letting `openjd_expr` defaults pass through
/// unchanged when the caller doesn't override.
fn evaluate_inner(
    parsed: &openjd_expr::ParsedExpression,
    symbols: &openjd_expr::SymbolTable,
    library: Option<&openjd_expr::FunctionLibrary>,
    options: Option<&JsEvalOptions>,
) -> Result<openjd_expr::ExprValue, openjd_expr::ExpressionError> {
    // Short-circuit when nothing is overridden: the default direct
    // evaluate path is the cheapest.
    let has_overrides = library.is_some() || options.map(|o| o.has_any_limit()).unwrap_or(false);
    if !has_overrides {
        return parsed.evaluate(symbols);
    }

    // Bootstrap the builder. `ParsedExpression::with_library` and
    // `with_memory_limit` / `with_operation_limit` each create a
    // fresh `EvalBuilder`, after which the builder itself chains.
    let mut builder = match library {
        Some(lib) => parsed.with_library(lib),
        None => parsed.with_memory_limit(openjd_expr::DEFAULT_MEMORY_LIMIT),
    };
    if let Some(opts) = options {
        if let Some(limit) = opts.memory_limit {
            builder = builder.with_memory_limit(limit);
        }
        if let Some(limit) = opts.operation_limit {
            builder = builder.with_operation_limit(limit);
        }
    }
    builder.evaluate(&[symbols])
}

impl JsEvalOptions {
    /// Returns true if at least one limit field is set. Used by the
    /// fast-path check in `evaluate_inner`.
    fn has_any_limit(&self) -> bool {
        self.memory_limit.is_some() || self.operation_limit.is_some()
    }
}

// ── Free functions ─────────────────────────────────────────────────

/// Parse an expression string for later evaluation.
#[wasm_bindgen(js_name = "parseExpression")]
pub fn parse_expression(expr: &str) -> Result<JsParsedExpression, JsError> {
    let parsed = openjd_expr::ParsedExpression::new(expr).map_err(expr_to_js_error)?;
    Ok(JsParsedExpression { inner: parsed })
}

/// Evaluate an expression string directly.
///
/// `options` (optional, plain-object shape) lets the host cap
/// `memoryLimit` and/or `operationLimit` for this specific call.
/// Omitted / `undefined` / `{}` keeps the built-in defaults.
#[wasm_bindgen(js_name = "evaluateExpression")]
pub fn evaluate_expression(
    expr: &str,
    symbols: &JsSymbolTable,
    library: Option<JsFunctionLibrary>,
    options: JsValue,
) -> Result<JsExprValue, JsError> {
    let parsed_opts = JsEvalOptions::from_js_value(options)?;
    evaluate_expression_str(expr, symbols, library.as_ref(), parsed_opts.as_ref())
        .map_err(|e| JsError::new(&e))
}

/// Get the default function library.
#[wasm_bindgen(js_name = "getDefaultLibrary")]
pub fn get_default_library() -> JsFunctionLibrary {
    JsFunctionLibrary::get_default()
}

/// Escape `{{` and `}}` in a string for literal use in format strings.
#[wasm_bindgen(js_name = "escapeFormatString")]
pub fn escape_format_string(s: &str) -> String {
    openjd_expr::escape_format_string(s)
}

/// Parse a range expression (e.g., "1-10:2") into an array of integers.
///
/// Rejects ranges whose logical element count exceeds the default
/// operation limit: this export materializes every element and runs
/// outside any evaluation budget, and a symbolic range like
/// `"1-9223372036854775807"` parses in O(1) but would hang the host
/// building ~2^63 integers.
#[wasm_bindgen(js_name = "parseRangeExpr")]
pub fn parse_range_expr(expr: &str) -> Result<Vec<i64>, JsError> {
    let range: openjd_expr::RangeExpr = expr
        .parse()
        .map_err(|e: openjd_expr::ExpressionError| JsError::new(&e.to_string()))?;
    if range.len() > openjd_expr::DEFAULT_OPERATION_LIMIT {
        return Err(JsError::new(&format!(
            "Range expression has too many elements to materialize \
             ({} > {})",
            range.len(),
            openjd_expr::DEFAULT_OPERATION_LIMIT
        )));
    }
    Ok(range.iter().collect())
}

/// Default memory limit for expression evaluation.
///
/// Exported as a function because wasm-bindgen does not support
/// exporting `const` or `static` items to JS. Informational —
/// hosts that want to tighten the limit pass `EvalOptions.memoryLimit`
/// on the decode/evaluate call.
#[wasm_bindgen(js_name = "getDefaultMemoryLimit")]
pub fn get_default_memory_limit() -> usize {
    openjd_expr::DEFAULT_MEMORY_LIMIT
}

/// Default operation limit for expression evaluation. See
/// [`get_default_memory_limit`] for rationale on the getter style.
#[wasm_bindgen(js_name = "getDefaultOperationLimit")]
pub fn get_default_operation_limit() -> usize {
    openjd_expr::DEFAULT_OPERATION_LIMIT
}
