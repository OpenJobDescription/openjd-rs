// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Format string parsing and resolution.

use crate::error::ExpressionError;
use crate::symbol_table::{SymbolTable, SymbolTableEntry};
use crate::value::ExprValue;
use serde::de::{self, Deserializer};
use std::fmt;

// Only stored in Vec<Segment> (heap-allocated), so the per-element size doesn't
// matter much. Boxing Expression's ParsedExpression would add pointer indirection
// on every evaluation for negligible memory savings.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
enum Segment {
    Literal(String),
    SimpleName {
        start: usize,
        end: usize,
        name: String,
    },
    Expression {
        start: usize,
        end: usize,
        parsed: crate::eval::ParsedExpression,
    },
}

fn is_simple_dotted_name(expr: &str) -> bool {
    !expr.is_empty()
        && expr.split('.').all(|part| {
            !part.is_empty() && {
                let mut chars = part.chars();
                let first = chars.next().expect("part is non-empty, checked above");
                (first.is_alphabetic() || first == '_')
                    && chars.all(|c| c.is_alphanumeric() || c == '_')
            }
        })
}

#[derive(Debug, Clone)]
pub struct FormatString {
    raw: String,
    segments: Vec<Segment>,
}

impl FormatString {
    pub fn new(input: &str) -> Result<Self, ExpressionError> {
        let segments = parse_segments(input)?;
        Ok(Self {
            raw: input.to_string(),
            segments,
        })
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Resolve all interpolations to a string.
    ///
    /// Every segment is evaluated and concatenated. Single-expression format
    /// strings lose their typed value — use `resolve` when you need the
    /// native `ExprValue`.
    ///
    /// `path_format` controls how `path()` values are constructed:
    /// - `PathFormat::Posix` in template context (create_job, let bindings)
    /// - `PathFormat::host()` in session/host context (action execution)
    pub fn resolve_string(
        &self,
        symtab: &SymbolTable,
        library: Option<&crate::function_library::FunctionLibrary>,
        path_mapping_rules: &[crate::path_mapping::PathMappingRule],
    ) -> Result<String, ExpressionError> {
        self.resolve_string_with_format(
            symtab,
            library,
            path_mapping_rules,
            crate::path_mapping::PathFormat::host(),
        )
    }

    /// Like `resolve_string` but with an explicit path format.
    pub fn resolve_string_with_format(
        &self,
        symtab: &SymbolTable,
        library: Option<&crate::function_library::FunctionLibrary>,
        path_mapping_rules: &[crate::path_mapping::PathMappingRule],
        path_format: crate::path_mapping::PathFormat,
    ) -> Result<String, ExpressionError> {
        let mut result = String::new();
        for seg in &self.segments {
            match seg {
                Segment::Literal(s) => result.push_str(s),
                Segment::SimpleName { name, start, end } => {
                    // Fast path: direct string lookup for simple dotted names
                    match symtab.get_string(name) {
                        Some(val) => result.push_str(val),
                        None => {
                            // Fall back to evaluator (handles keyword attributes, etc.)
                            let val = self
                                .eval_segment(
                                    name,
                                    symtab,
                                    library,
                                    path_mapping_rules,
                                    path_format,
                                    None,
                                )
                                .map_err(|_| {
                                    ExpressionError::new(format!(
                                    "Failed to parse interpolation expression at [{start}, {end}]. \
                                     Reason: Undefined variable '{name}'."
                                ))
                                })?;
                            if !matches!(val, ExprValue::Null) {
                                result.push_str(&val.to_display_string());
                            }
                        }
                    }
                }
                Segment::Expression { parsed, .. } => {
                    let val = self.eval_parsed(
                        parsed,
                        symtab,
                        library,
                        path_mapping_rules,
                        path_format,
                        None,
                    )?;
                    // None/null renders as empty string in format strings
                    if !matches!(val, ExprValue::Null) {
                        result.push_str(&val.to_display_string());
                    }
                }
            }
        }
        Ok(result)
    }

    /// Resolve to a typed `ExprValue`.
    ///
    /// If the format string is a single expression with no surrounding literal
    /// text, returns the raw `ExprValue` (which may be int, list, path, etc.).
    /// Otherwise, resolves all segments to strings and concatenates, returning
    /// `ExprValue::String`.
    pub fn resolve(
        &self,
        symtab: &SymbolTable,
        library: Option<&crate::function_library::FunctionLibrary>,
        path_mapping_rules: &[crate::path_mapping::PathMappingRule],
    ) -> Result<ExprValue, ExpressionError> {
        self.resolve_with_format(
            symtab,
            library,
            path_mapping_rules,
            crate::path_mapping::PathFormat::host(),
        )
    }

    /// Like `resolve` but with an explicit path format.
    pub fn resolve_with_format(
        &self,
        symtab: &SymbolTable,
        library: Option<&crate::function_library::FunctionLibrary>,
        path_mapping_rules: &[crate::path_mapping::PathMappingRule],
        path_format: crate::path_mapping::PathFormat,
    ) -> Result<ExprValue, ExpressionError> {
        self.resolve_inner(symtab, library, path_mapping_rules, path_format, None)
    }

    /// Like `resolve` but with a target type for coercion.
    pub fn resolve_typed(
        &self,
        symtab: &SymbolTable,
        library: Option<&crate::function_library::FunctionLibrary>,
        path_mapping_rules: &[crate::path_mapping::PathMappingRule],
        target_type: &crate::types::ExprType,
    ) -> Result<ExprValue, ExpressionError> {
        self.resolve_inner(
            symtab,
            library,
            path_mapping_rules,
            crate::path_mapping::PathFormat::host(),
            Some(target_type),
        )
    }

    /// Like `resolve_typed` but with an explicit path format.
    pub fn resolve_typed_with_format(
        &self,
        symtab: &SymbolTable,
        library: Option<&crate::function_library::FunctionLibrary>,
        path_mapping_rules: &[crate::path_mapping::PathMappingRule],
        path_format: crate::path_mapping::PathFormat,
        target_type: &crate::types::ExprType,
    ) -> Result<ExprValue, ExpressionError> {
        self.resolve_inner(
            symtab,
            library,
            path_mapping_rules,
            path_format,
            Some(target_type),
        )
    }

    fn resolve_inner(
        &self,
        symtab: &SymbolTable,
        library: Option<&crate::function_library::FunctionLibrary>,
        path_mapping_rules: &[crate::path_mapping::PathMappingRule],
        path_format: crate::path_mapping::PathFormat,
        target_type: Option<&crate::types::ExprType>,
    ) -> Result<ExprValue, ExpressionError> {
        if self.segments.len() == 1 {
            match &self.segments[0] {
                Segment::SimpleName { name, .. } => {
                    return self.eval_segment(
                        name,
                        symtab,
                        library,
                        path_mapping_rules,
                        path_format,
                        target_type,
                    )
                }
                Segment::Expression { parsed, .. } => {
                    return self.eval_parsed(
                        parsed,
                        symtab,
                        library,
                        path_mapping_rules,
                        path_format,
                        target_type,
                    )
                }
                _ => {}
            }
        }
        self.resolve_string_with_format(symtab, library, path_mapping_rules, path_format)
            .map(ExprValue::String)
    }

    fn eval_segment(
        &self,
        expr: &str,
        symtab: &SymbolTable,
        library: Option<&crate::function_library::FunctionLibrary>,
        path_mapping_rules: &[crate::path_mapping::PathMappingRule],
        path_format: crate::path_mapping::PathFormat,
        target_type: Option<&crate::types::ExprType>,
    ) -> Result<ExprValue, ExpressionError> {
        let parsed = crate::eval::ParsedExpression::new(expr)?;
        self.eval_parsed(
            &parsed,
            symtab,
            library,
            path_mapping_rules,
            path_format,
            target_type,
        )
    }

    fn eval_parsed(
        &self,
        parsed: &crate::eval::ParsedExpression,
        symtab: &SymbolTable,
        library: Option<&crate::function_library::FunctionLibrary>,
        path_mapping_rules: &[crate::path_mapping::PathMappingRule],
        path_format: crate::path_mapping::PathFormat,
        target_type: Option<&crate::types::ExprType>,
    ) -> Result<ExprValue, ExpressionError> {
        let symtabs = [symtab];
        let mut evaluator = parsed.evaluator(&symtabs).with_path_format(path_format);
        if let Some(lib) = library {
            evaluator = evaluator.with_library(lib);
        }
        if !path_mapping_rules.is_empty() {
            evaluator = evaluator.with_path_mapping_rules(path_mapping_rules);
        }
        if let Some(tt) = target_type {
            evaluator = evaluator.with_target_type(tt);
        }
        evaluator.evaluate(&parsed.ast)
    }

    /// Validate all expressions in this format string against a symbol table.
    /// The symbol table should contain `ExprValue::unresolved(T)` for symbols
    /// whose values are not yet known. This is the spec's approach to static
    /// type checking — just evaluate normally with unresolved types.
    pub fn validate_expressions(
        &self,
        symtab: &SymbolTable,
        lib: &crate::function_library::FunctionLibrary,
    ) -> Result<(), FormatStringValidationError> {
        for seg in &self.segments {
            let (parsed, start, end) = match seg {
                Segment::Literal(_) => continue,
                Segment::SimpleName { name, start, end } => {
                    let p = crate::eval::ParsedExpression::new(name.as_str()).map_err(|e| {
                        FormatStringValidationError {
                            message: e.to_string(),
                            input: self.raw.clone(),
                            start: *start,
                            end: *end,
                        }
                    })?;
                    // Evaluate inline since SimpleName doesn't store a ParsedExpression
                    let symtabs = [symtab];
                    let mut evaluator = p.evaluator(&symtabs).with_library(lib);
                    if let Err(e) = evaluator.evaluate(&p.ast) {
                        return Err(FormatStringValidationError {
                            message: e.to_string(),
                            input: self.raw.clone(),
                            start: *start,
                            end: *end,
                        });
                    }
                    continue;
                }
                Segment::Expression { parsed, start, end } => (parsed, *start, *end),
            };
            let symtabs = [symtab];
            let mut evaluator = parsed.evaluator(&symtabs).with_library(lib);
            if let Err(e) = evaluator.evaluate(&parsed.ast) {
                return Err(FormatStringValidationError {
                    message: e.to_string(),
                    input: self.raw.clone(),
                    start,
                    end,
                });
            }
        }
        Ok(())
    }

    /// Validate list comprehension loop variables in expressions.
    /// Checks: must be lowercase, must not shadow let bindings.
    pub fn validate_comprehension_vars(
        &self,
        let_names: &std::collections::HashSet<String>,
    ) -> Result<(), ExpressionError> {
        for seg in &self.segments {
            let parsed = match seg {
                Segment::Literal(_) => continue,
                Segment::SimpleName { name, .. } => {
                    match crate::eval::ParsedExpression::new(name.as_str()) {
                        Ok(p) => p,
                        Err(_) => continue,
                    }
                }
                Segment::Expression { parsed, .. } => {
                    check_comprehension_vars(&parsed.ast, let_names)?;
                    continue;
                }
            };
            check_comprehension_vars(&parsed.ast, let_names)?;
        }
        Ok(())
    }

    pub fn has_complex_expressions(&self) -> bool {
        self.segments
            .iter()
            .any(|s| matches!(s, Segment::Expression { .. }))
    }

    pub fn expression_names(&self) -> Vec<&str> {
        self.segments
            .iter()
            .filter_map(|s| match s {
                Segment::SimpleName { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn is_literal(&self) -> bool {
        self.segments
            .iter()
            .all(|s| matches!(s, Segment::Literal(_)))
    }

    /// Copy symbol table entries referenced by this format string's expressions
    /// from `source` into `dest`. Only copies the actual symtab values that are
    /// referenced, not properties/methods called on them.
    ///
    /// For example, if an expression references `Param.Name.upper()`, the symbol
    /// `Param.Name` is a Value in the symtab (`.upper()` is a method call).
    /// This method walks the dotted path into `source`, stops when it finds a
    /// Value (not a Table), and copies that value into `dest` at the same path.
    pub fn copy_used_symtab_values(&self, source: &SymbolTable, dest: &mut SymbolTable) {
        use crate::eval::ParsedExpression;

        for segment in &self.segments {
            match segment {
                Segment::SimpleName { name, .. } => {
                    let parsed = match ParsedExpression::new(name.as_str()) {
                        Ok(p) => p,
                        Err(_) => continue,
                    };
                    for symbol in &parsed.accessed_symbols {
                        copy_symbol_value(symbol, source, dest);
                    }
                }
                Segment::Expression { parsed, .. } => {
                    for symbol in &parsed.accessed_symbols {
                        copy_symbol_value(symbol, source, dest);
                    }
                }
                Segment::Literal(_) => continue,
            }
        }
    }
}

/// Walk a dotted symbol name into `source`, find the deepest Value entry,
/// and copy it into `dest` at the same path.
///
/// E.g. for "Param.Name.upper", if source has Param.Name = "hello" (a Value),
/// we copy Param.Name into dest. The ".upper" part is a method call, not a
/// symtab key.
pub fn copy_symbol_value(symbol: &str, source: &SymbolTable, dest: &mut SymbolTable) {
    let parts: Vec<&str> = symbol.split('.').collect();
    // Walk into source, building the dotted key as we go.
    // Stop when we find a Value (the rest is property/method access).
    let mut current = source;
    for i in 0..parts.len() {
        match current.table.get(parts[i]) {
            Some(SymbolTableEntry::Value(v)) => {
                // Found the value — copy it at this dotted path
                let key = parts[..=i].join(".");
                let _ = dest.set(&key, v.clone());
                return;
            }
            Some(SymbolTableEntry::Table(t)) => {
                current = t;
                // Continue walking deeper
            }
            None => return, // Symbol not in source, skip
        }
    }
    // Reached the end and it's a table — copy the whole subtable
    let key = parts.join(".");
    dest.set_table(&key, current.clone());
}

impl fmt::Display for FormatString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.raw)
    }
}
impl PartialEq for FormatString {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}
impl Eq for FormatString {}
impl<'de> serde::Deserialize<'de> for FormatString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FsVisitor;
        impl<'de> serde::de::Visitor<'de> for FsVisitor {
            type Value = FormatString;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a string or number")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<FormatString, E> {
                FormatString::new(v).map_err(de::Error::custom)
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<FormatString, E> {
                FormatString::new(&v).map_err(de::Error::custom)
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<FormatString, E> {
                FormatString::new(&v.to_string()).map_err(de::Error::custom)
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<FormatString, E> {
                FormatString::new(&v.to_string()).map_err(de::Error::custom)
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<FormatString, E> {
                FormatString::new(&v.to_string()).map_err(de::Error::custom)
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<FormatString, E> {
                FormatString::new(&v.to_string()).map_err(de::Error::custom)
            }
        }
        deserializer.deserialize_any(FsVisitor)
    }
}
impl serde::Serialize for FormatString {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.raw.serialize(serializer)
    }
}

/// Check list comprehension loop variables in an AST.
fn check_comprehension_vars(
    node: &ruff_python_ast::Expr,
    let_names: &std::collections::HashSet<String>,
) -> Result<(), ExpressionError> {
    use ruff_python_ast as ast;
    match node {
        ast::Expr::ListComp(lc) => {
            for gen in &lc.generators {
                if let ast::Expr::Name(n) = &gen.target {
                    let var = n.id.as_str();
                    // Must start with lowercase or underscore
                    if let Some(first) = var.chars().next() {
                        if !first.is_ascii_lowercase() && first != '_' {
                            return Err(ExpressionError::new(format!(
                                "List comprehension variable '{var}' must start with a lowercase letter or underscore"
                            )));
                        }
                    }
                    // Must not shadow let bindings
                    if let_names.contains(var) {
                        return Err(ExpressionError::new(format!(
                            "List comprehension variable '{var}' shadows a let binding"
                        )));
                    }
                }
            }
            check_comprehension_vars(&lc.elt, let_names)?;
        }
        ast::Expr::BinOp(b) => {
            check_comprehension_vars(&b.left, let_names)?;
            check_comprehension_vars(&b.right, let_names)?;
        }
        ast::Expr::UnaryOp(u) => {
            check_comprehension_vars(&u.operand, let_names)?;
        }
        ast::Expr::Compare(c) => {
            check_comprehension_vars(&c.left, let_names)?;
            for r in &c.comparators {
                check_comprehension_vars(r, let_names)?;
            }
        }
        ast::Expr::BoolOp(b) => {
            for v in &b.values {
                check_comprehension_vars(v, let_names)?;
            }
        }
        ast::Expr::If(i) => {
            check_comprehension_vars(&i.test, let_names)?;
            check_comprehension_vars(&i.body, let_names)?;
            check_comprehension_vars(&i.orelse, let_names)?;
        }
        ast::Expr::Call(c) => {
            check_comprehension_vars(&c.func, let_names)?;
            for a in &c.arguments.args {
                check_comprehension_vars(a, let_names)?;
            }
        }
        ast::Expr::List(l) => {
            for e in &l.elts {
                check_comprehension_vars(e, let_names)?;
            }
        }
        ast::Expr::Tuple(t) => {
            for e in &t.elts {
                check_comprehension_vars(e, let_names)?;
            }
        }
        ast::Expr::Subscript(s) => {
            check_comprehension_vars(&s.value, let_names)?;
            check_comprehension_vars(&s.slice, let_names)?;
        }
        ast::Expr::Attribute(a) => {
            check_comprehension_vars(&a.value, let_names)?;
        }
        _ => {}
    }
    Ok(())
}

fn parse_segments(input: &str) -> Result<Vec<Segment>, ExpressionError> {
    let mut segments = Vec::new();
    let len = input.len();
    let mut pos = 0;
    while pos < len {
        match input[pos..].find("{{") {
            None => {
                if let Some(co) = input[pos..].find("}}") {
                    let cp = pos + co;
                    return Err(ExpressionError::new(format!(
                        "Failed to parse interpolation expression at [{pos}, {}]. Reason: Missing opening braces.", cp + 2
                    )));
                }
                let rest = &input[pos..];
                if !rest.is_empty() {
                    segments.push(Segment::Literal(rest.to_string()));
                }
                break;
            }
            Some(offset) => {
                let op = pos + offset;
                if let Some(co) = input[pos..].find("}}") {
                    if pos + co < op {
                        return Err(ExpressionError::new(format!(
                            "Failed to parse interpolation expression at [{pos}, {len}]. Reason: Braces mismatch."
                        )));
                    }
                }
                if op > pos {
                    segments.push(Segment::Literal(input[pos..op].to_string()));
                }
                let es = op + 2;
                match input[es..].find("}}") {
                    None => return Err(ExpressionError::new(format!(
                        "Failed to parse interpolation expression at [{op}, {len}]. Reason: Braces mismatch."
                    ))),
                    Some(co) => {
                        let ee = es + co;
                        let be = ee + 2;
                        let et = input[es..ee].trim();
                        if et.is_empty() {
                            return Err(ExpressionError::new(format!(
                                "Failed to parse interpolation expression at [{op}, {be}]. Reason: Empty expression."
                            )));
                        }
                        if is_simple_dotted_name(et) {
                            segments.push(Segment::SimpleName { start: op, end: be, name: et.to_string() });
                        } else {
                            let parsed = crate::eval::ParsedExpression::new(et)?;
                            segments.push(Segment::Expression { start: op, end: be, parsed });
                        }
                        pos = be;
                    }
                }
            }
        }
    }
    Ok(segments)
}

/// Structured error from [`FormatString::validate_expressions`].
///
/// Carries the position of the failing interpolation within the format string
/// so callers can produce caret-style diagnostics or structured error responses.
#[derive(Debug, Clone)]
pub struct FormatStringValidationError {
    /// Description of what went wrong (e.g. "Undefined variable 'Param.X'").
    pub message: String,
    /// The raw format string that was being validated.
    pub input: String,
    /// Byte offset of the `{{` that opens the failing interpolation.
    pub start: usize,
    /// Byte offset of the `}}` that closes the failing interpolation.
    pub end: usize,
}

impl std::fmt::Display for FormatStringValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Failed to parse interpolation expression at [{}, {}]. {}",
            self.start, self.end, self.message
        )
    }
}

impl std::error::Error for FormatStringValidationError {}

/// Escape `{{` and `}}` in a string so the format string parser treats them as literals.
#[must_use]
pub fn escape_format_string(value: &str) -> String {
    let mut result = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' && chars.peek() == Some(&'{') {
            chars.next();
            result.push_str("{{ \"{{\" }}");
        } else if c == '}' && chars.peek() == Some(&'}') {
            chars.next();
            result.push_str("{{ \"}\" + \"}\" }}");
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_only() {
        let fs = FormatString::new("hello").unwrap();
        assert!(fs.is_literal());
        assert_eq!(
            fs.resolve_string(&SymbolTable::new(), None, &[]).unwrap(),
            "hello"
        );
    }
    #[test]
    fn simple_expr() {
        let fs = FormatString::new("{{Param.X}}").unwrap();
        let mut st = SymbolTable::new();
        st.set_string("Param.X", "42").unwrap();
        assert_eq!(fs.resolve_string(&st, None, &[]).unwrap(), "42");
    }
    #[test]
    fn complex_parses() {
        let fs = FormatString::new("{{Param.X + 1}}").unwrap();
        assert!(fs.has_complex_expressions());
    }
    #[test]
    fn missing_close() {
        assert!(FormatString::new("{{x").is_err());
    }
    #[test]
    fn missing_open() {
        assert!(FormatString::new("x}}").is_err());
    }
    #[test]
    fn empty_expr() {
        assert!(FormatString::new("{{}}").is_err());
    }
    #[test]
    fn resolve_expr_arithmetic() {
        let fs = FormatString::new("{{ Param.X + 3 }}").unwrap();
        let mut st = SymbolTable::new();
        st.set("Param.X", ExprValue::Int(10)).unwrap();
        assert_eq!(fs.resolve_string(&st, None, &[]).unwrap(), "13");
    }
    #[test]
    fn validate_catches_bitwise() {
        // Bitwise ops are rejected at parse time (structural validation in ParsedExpression::new)
        assert!(FormatString::new("{{ 5 & 3 }}").is_err());
    }
    #[test]
    fn validate_catches_dict() {
        // Dict literals are rejected at parse time (structural validation in ParsedExpression::new)
        assert!(FormatString::new("{{ {1: 2} }}").is_err());
    }
    #[test]
    fn validate_catches_unknown_func() {
        let fs = FormatString::new("{{ bad_func(1) }}").unwrap();
        let host_lib = crate::default_library::get_default_library()
            .clone()
            .with_host_context();
        assert!(fs
            .validate_expressions(&SymbolTable::new(), &host_lib)
            .is_err());
    }
    #[test]
    fn validate_catches_empty_regex_pattern() {
        // First verify the expression itself errors
        let st = SymbolTable::new();
        let result = crate::evaluate_expression("re_replace('hello', '', 'x')", &st);
        assert!(
            result.is_err(),
            "Direct eval should error, got: {:?}",
            result.map(|v| v.to_display_string())
        );

        let host_lib = crate::default_library::get_default_library()
            .clone()
            .with_host_context();
        let fs = FormatString::new("{{ re_replace('hello', '', 'x') }}").unwrap();
        let result = fs.validate_expressions(&SymbolTable::new(), &host_lib);
        assert!(
            result.is_err(),
            "Format string validation should error, got: {:?}",
            result
        );
    }
    #[test]
    fn validate_catches_regex_group_ref() {
        let st = SymbolTable::new();
        // Backslash group ref
        let result = crate::evaluate_expression(r"re_replace('hello', '(h)', r'\1')", &st);
        assert!(
            result.is_err(),
            "Should reject \\1 group ref, got: {:?}",
            result.map(|v| v.to_display_string())
        );
        // Dollar group ref
        let result = crate::evaluate_expression("re_replace('hello', '(h)', '$1')", &st);
        assert!(
            result.is_err(),
            "Should reject $1 group ref, got: {:?}",
            result.map(|v| v.to_display_string())
        );
    }
    #[test]
    fn validate_allows_known_func() {
        let fs = FormatString::new("{{ len(Param.X) }}").unwrap();
        let mut st = SymbolTable::new();
        st.set(
            "Param.X",
            crate::ExprValue::unresolved(crate::ExprType::list(crate::ExprType::INT)),
        )
        .unwrap();
        let host_lib = crate::default_library::get_default_library()
            .clone()
            .with_host_context();
        assert!(fs.validate_expressions(&st, &host_lib).is_ok());
    }
    #[test]
    fn validate_allows_arithmetic() {
        let fs = FormatString::new("{{ Param.X + 3 }}").unwrap();
        let mut st = SymbolTable::new();
        st.set(
            "Param.X",
            crate::ExprValue::unresolved(crate::ExprType::INT),
        )
        .unwrap();
        let host_lib = crate::default_library::get_default_library()
            .clone()
            .with_host_context();
        assert!(fs.validate_expressions(&st, &host_lib).is_ok());
    }

    #[test]
    fn escape_format_string_no_special_chars() {
        assert_eq!(escape_format_string("hello world"), "hello world");
    }
    #[test]
    fn escape_format_string_double_open_braces() {
        assert_eq!(escape_format_string("{{"), "{{ \"{{\" }}");
    }
    #[test]
    fn escape_format_string_double_close_braces() {
        assert_eq!(escape_format_string("}}"), "{{ \"}\" + \"}\" }}");
    }
    #[test]
    fn escape_format_string_mixed() {
        assert_eq!(
            escape_format_string("a{{b}}c"),
            "a{{ \"{{\" }}b{{ \"}\" + \"}\" }}c"
        );
    }
    #[test]
    fn escape_format_string_empty() {
        assert_eq!(escape_format_string(""), "");
    }
    #[test]
    fn resolve_value_single_expr_int() {
        let fs = FormatString::new("{{Param.X}}").unwrap();
        let mut st = SymbolTable::new();
        st.set("Param.X", ExprValue::Int(42)).unwrap();
        let val = fs.resolve(&st, None, &[]).unwrap();
        assert!(matches!(val, ExprValue::Int(42)));
    }
    #[test]
    fn resolve_value_single_expr_string() {
        let fs = FormatString::new("{{Param.X}}").unwrap();
        let mut st = SymbolTable::new();
        st.set("Param.X", ExprValue::String("hello".into()))
            .unwrap();
        let val = fs.resolve(&st, None, &[]).unwrap();
        assert!(matches!(val, ExprValue::String(ref s) if s == "hello"));
    }
    #[test]
    fn resolve_value_mixed() {
        let fs = FormatString::new("hello {{Param.X}}").unwrap();
        let mut st = SymbolTable::new();
        st.set("Param.X", ExprValue::Int(42)).unwrap();
        let val = fs.resolve(&st, None, &[]).unwrap();
        assert!(matches!(val, ExprValue::String(ref s) if s == "hello 42"));
    }
    #[test]
    fn resolve_value_pure_literal() {
        let fs = FormatString::new("hello").unwrap();
        let val = fs.resolve(&SymbolTable::new(), None, &[]).unwrap();
        assert!(matches!(val, ExprValue::String(ref s) if s == "hello"));
    }

    #[test]
    fn resolve_with_target_type_coerces_int_to_float() {
        let fs = FormatString::new("{{Param.X}}").unwrap();
        let mut st = SymbolTable::new();
        st.set("Param.X", ExprValue::Int(42)).unwrap();
        let val = fs
            .resolve_typed(&st, None, &[], &crate::types::ExprType::FLOAT)
            .unwrap();
        assert!(matches!(val, ExprValue::Float(ref f) if f.value() == 42.0));
    }

    #[test]
    fn resolve_with_target_type_none_preserves_int() {
        let fs = FormatString::new("{{Param.X}}").unwrap();
        let mut st = SymbolTable::new();
        st.set("Param.X", ExprValue::Int(42)).unwrap();
        let val = fs.resolve(&st, None, &[]).unwrap();
        assert!(matches!(val, ExprValue::Int(42)));
    }

    #[test]
    fn resolve_with_target_type_path() {
        let fs = FormatString::new("{{Param.X}}").unwrap();
        let mut st = SymbolTable::new();
        st.set("Param.X", ExprValue::String("/foo/bar".into()))
            .unwrap();
        let target = crate::types::ExprType::PATH;
        let val = fs.resolve_typed(&st, None, &[], &target).unwrap();
        assert!(matches!(val, ExprValue::Path { ref value, .. } if value == "/foo/bar"));
    }

    #[test]
    fn copy_used_symtab_values_simple() {
        let mut src = SymbolTable::new();
        src.set("Param.Frame", ExprValue::Int(42)).unwrap();
        src.set("Param.Name", ExprValue::String("test".into()))
            .unwrap();
        src.set("Param.Unused", ExprValue::Int(99)).unwrap();

        let fs = FormatString::new("render --frame {{Param.Frame}}").unwrap();
        let mut dest = SymbolTable::new();
        fs.copy_used_symtab_values(&src, &mut dest);

        assert!(dest.get_value("Param.Frame").is_some());
        assert!(dest.get_value("Param.Name").is_none());
        assert!(dest.get_value("Param.Unused").is_none());
    }

    #[test]
    fn copy_used_symtab_values_method_call() {
        // Param.Name.upper() — "Param.Name" is the value, ".upper()" is a method
        let mut src = SymbolTable::new();
        src.set("Param.Name", ExprValue::String("hello".into()))
            .unwrap();

        let fs = FormatString::new("{{Param.Name.upper()}}").unwrap();
        let mut dest = SymbolTable::new();
        fs.copy_used_symtab_values(&src, &mut dest);

        assert_eq!(
            dest.get_value("Param.Name").unwrap(),
            &ExprValue::String("hello".into())
        );
    }

    #[test]
    fn copy_used_symtab_values_multiple_format_strings() {
        let mut src = SymbolTable::new();
        src.set("Param.Frame", ExprValue::Int(1)).unwrap();
        src.set("Param.Name", ExprValue::String("job".into()))
            .unwrap();
        src.set("Task.Param.Index", ExprValue::Int(5)).unwrap();

        let mut dest = SymbolTable::new();
        FormatString::new("{{Param.Frame}}")
            .unwrap()
            .copy_used_symtab_values(&src, &mut dest);
        FormatString::new("{{Task.Param.Index}}")
            .unwrap()
            .copy_used_symtab_values(&src, &mut dest);

        assert!(dest.get_value("Param.Frame").is_some());
        assert!(dest.get_value("Task.Param.Index").is_some());
        assert!(dest.get_value("Param.Name").is_none());
    }

    #[test]
    fn copy_used_symtab_values_literal_no_copy() {
        let mut src = SymbolTable::new();
        src.set("Param.X", ExprValue::Int(1)).unwrap();

        let fs = FormatString::new("just a literal").unwrap();
        let mut dest = SymbolTable::new();
        fs.copy_used_symtab_values(&src, &mut dest);

        assert!(dest.keys().next().is_none());
    }

    #[test]
    fn copy_used_symtab_values_expression_with_multiple_refs() {
        let mut src = SymbolTable::new();
        src.set("Param.Start", ExprValue::Int(1)).unwrap();
        src.set("Param.End", ExprValue::Int(10)).unwrap();
        src.set("Param.Other", ExprValue::Int(99)).unwrap();

        let fs = FormatString::new("{{Param.Start + Param.End}}").unwrap();
        let mut dest = SymbolTable::new();
        fs.copy_used_symtab_values(&src, &mut dest);

        assert!(dest.get_value("Param.Start").is_some());
        assert!(dest.get_value("Param.End").is_some());
        assert!(dest.get_value("Param.Other").is_none());
    }

    #[test]
    fn copy_used_symtab_values_property_access_stops_at_value() {
        // "Param.Name.upper()" — accessed_symbols is {"Param.Name.upper"}
        // but Param.Name is a Value, so we stop there and don't create Param.Name.upper
        let mut src = SymbolTable::new();
        src.set("Param.Name", ExprValue::String("hello".into()))
            .unwrap();

        let fs = FormatString::new("{{Param.Name.upper()}}").unwrap();
        let mut dest = SymbolTable::new();
        fs.copy_used_symtab_values(&src, &mut dest);

        // Param.Name is copied
        assert_eq!(
            dest.get_value("Param.Name"),
            Some(&ExprValue::String("hello".into()))
        );
        // Param.Name.upper is NOT a key (upper is a method, not a symtab entry)
        assert!(dest.get("Param.Name.upper").is_none());
    }

    #[test]
    fn copy_used_symtab_values_chained_property() {
        // "Param.Path.stem.upper()" — Param.Path is a Value
        let mut src = SymbolTable::new();
        src.set("Param.Path", ExprValue::String("/foo/bar.exr".into()))
            .unwrap();

        let fs = FormatString::new("{{Param.Path.stem.upper()}}").unwrap();
        let mut dest = SymbolTable::new();
        fs.copy_used_symtab_values(&src, &mut dest);

        assert_eq!(
            dest.get_value("Param.Path"),
            Some(&ExprValue::String("/foo/bar.exr".into()))
        );
        assert!(dest.get("Param.Path.stem").is_none());
    }

    #[test]
    fn copy_used_symtab_values_missing_symbol_no_error() {
        // Reference a symbol that doesn't exist in source — should silently skip
        let src = SymbolTable::new(); // empty

        let fs = FormatString::new("{{Param.Missing + Task.Param.Also.Missing}}").unwrap();
        let mut dest = SymbolTable::new();
        fs.copy_used_symtab_values(&src, &mut dest);

        // dest should be empty, no errors
        assert!(dest.keys().next().is_none());
    }

    #[test]
    fn copy_used_symtab_values_partial_missing() {
        // One symbol exists, one doesn't
        let mut src = SymbolTable::new();
        src.set("Param.Frame", ExprValue::Int(1)).unwrap();

        let fs = FormatString::new("{{Param.Frame + Param.Missing}}").unwrap();
        let mut dest = SymbolTable::new();
        fs.copy_used_symtab_values(&src, &mut dest);

        assert_eq!(dest.get_value("Param.Frame"), Some(&ExprValue::Int(1)));
        assert!(dest.get("Param.Missing").is_none());
    }
}
