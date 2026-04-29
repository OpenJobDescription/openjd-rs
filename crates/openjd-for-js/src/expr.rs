// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Expression engine bindings: ExprValue, FormatString, SymbolTable,
//! FunctionLibrary, PathMappingRule, ParsedExpression.

use crate::errors::*;
use wasm_bindgen::prelude::*;

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

/// Path format: Posix or Windows.
#[wasm_bindgen(js_name = "PathFormat")]
pub enum JsPathFormat {
    Posix = 0,
    Windows = 1,
}

impl JsPathFormat {
    pub fn into_inner(self) -> openjd_expr::PathFormat {
        match self {
            JsPathFormat::Posix => openjd_expr::PathFormat::Posix,
            JsPathFormat::Windows => openjd_expr::PathFormat::Windows,
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

    /// Set a scoped value: `set("Param", "Frames", ExprValue.string("1-10"))`.
    pub fn set(&mut self, scope: &str, name: &str, value: &JsExprValue) -> Result<(), JsError> {
        let mut subtable = self
            .inner
            .get_table(scope)
            .cloned()
            .unwrap_or_else(openjd_expr::SymbolTable::new);
        subtable
            .set(name, value.inner.clone())
            .map_err(symtab_to_js_error)?;
        self.inner.set_table(scope, subtable);
        Ok(())
    }

    /// Set a string value directly: `setString("Param", "Frames", "1-10")`.
    #[wasm_bindgen(js_name = "setString")]
    pub fn set_string(&mut self, scope: &str, name: &str, value: &str) -> Result<(), JsError> {
        let mut subtable = self
            .inner
            .get_table(scope)
            .cloned()
            .unwrap_or_else(openjd_expr::SymbolTable::new);
        subtable
            .set_string(name, value)
            .map_err(symtab_to_js_error)?;
        self.inner.set_table(scope, subtable);
        Ok(())
    }

    /// Get a value by scope and name.
    pub fn get(&self, scope: &str, name: &str) -> Option<JsExprValue> {
        self.inner
            .get_table(scope)
            .and_then(|t| t.get_value(name))
            .map(|v| JsExprValue { inner: v.clone() })
    }

    /// Check if a scoped key exists.
    pub fn has(&self, scope: &str, name: &str) -> bool {
        self.inner
            .get_table(scope)
            .map(|t| t.contains(name))
            .unwrap_or(false)
    }

    /// Get all symbol paths (e.g., ["Param.Frames", "Param.OutputDir"]).
    #[wasm_bindgen(js_name = "allPaths")]
    pub fn all_paths(&self) -> Vec<String> {
        self.inner.all_paths("")
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
        JsFunctionLibrary {
            inner: openjd_expr::default_library::get_default_library().clone(),
        }
    }

    /// Create a library with path mapping rules.
    #[wasm_bindgen(js_name = "withPathMappingRules")]
    pub fn with_path_mapping_rules(rules: Vec<JsPathMappingRule>) -> JsFunctionLibrary {
        let rust_rules: Vec<openjd_expr::PathMappingRule> =
            rules.into_iter().map(|r| r.inner).collect();
        let lib = openjd_expr::FunctionLibrary::new().with_host_context(rust_rules);
        JsFunctionLibrary { inner: lib }
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
    pub fn evaluate(
        &self,
        symbols: &JsSymbolTable,
        library: Option<JsFunctionLibrary>,
    ) -> Result<JsExprValue, JsError> {
        let value = match library.as_ref() {
            Some(lib) => self
                .inner
                .with_library(&lib.inner)
                .evaluate(&[&symbols.inner])
                .map_err(expr_to_js_error)?,
            None => self
                .inner
                .evaluate(&symbols.inner)
                .map_err(expr_to_js_error)?,
        };
        Ok(JsExprValue { inner: value })
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
#[wasm_bindgen(js_name = "evaluateExpression")]
pub fn evaluate_expression(
    expr: &str,
    symbols: &JsSymbolTable,
    library: Option<JsFunctionLibrary>,
) -> Result<JsExprValue, JsError> {
    let parsed = openjd_expr::ParsedExpression::new(expr).map_err(expr_to_js_error)?;
    let value = match library.as_ref() {
        Some(lib) => parsed
            .with_library(&lib.inner)
            .evaluate(&[&symbols.inner])
            .map_err(expr_to_js_error)?,
        None => parsed.evaluate(&symbols.inner).map_err(expr_to_js_error)?,
    };
    Ok(JsExprValue { inner: value })
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
#[wasm_bindgen(js_name = "parseRangeExpr")]
pub fn parse_range_expr(expr: &str) -> Result<Vec<i64>, JsError> {
    let range: openjd_expr::RangeExpr = expr
        .parse()
        .map_err(|e: openjd_expr::ExpressionError| JsError::new(&e.to_string()))?;
    Ok(range.iter().collect())
}

/// Default memory limit for expression evaluation.
#[wasm_bindgen(js_name = "getDefaultMemoryLimit")]
pub fn get_default_memory_limit() -> usize {
    openjd_expr::DEFAULT_MEMORY_LIMIT
}

/// Default operation limit for expression evaluation.
#[wasm_bindgen(js_name = "getDefaultOperationLimit")]
pub fn get_default_operation_limit() -> usize {
    openjd_expr::DEFAULT_OPERATION_LIMIT
}
