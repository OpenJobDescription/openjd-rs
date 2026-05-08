// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Format string parsing and resolution.

use crate::error::ExpressionError;
use crate::profile::ExprProfile;
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
    Expression {
        start: usize,
        end: usize,
        parsed: crate::eval::ParsedExpression,
    },
}

#[derive(Debug, Clone)]
pub struct FormatString {
    raw: String,
    segments: Vec<Segment>,
}

/// Maximum permitted input length (in bytes) for [`FormatString::new`].
///
/// Caps the raw source text a format string may carry. Templates this large
/// are almost certainly pathological inputs rather than real user content —
/// 1 MB is several orders of magnitude above the size of the largest known
/// legitimate template fields.
///
/// See `specs/expr/format-string.md` (Defensive caps) for rationale.
pub const MAX_FORMAT_STRING_LEN: usize = 1024 * 1024;

/// Maximum number of `{{...}}` interpolation segments in a single format string.
///
/// Each segment triggers a full ruff parse and an allocation of a
/// `ParsedExpression`. Thousands of segments in one template field indicate
/// abuse; 1,000 is two orders of magnitude above any realistic usage.
///
/// See `specs/expr/format-string.md` (Defensive caps) for rationale.
pub const MAX_FORMAT_STRING_SEGMENTS: usize = 1_000;

impl FormatString {
    /// Parse a format string using the "latest" profile
    /// ([`ExprProfile::latest`]): the current revision with every known
    /// extension enabled.
    ///
    /// **This constructor is intentionally unstable across crate
    /// versions.** The set of accepted syntax, functions, and types
    /// grows as new extensions and revisions land. Use it for ad-hoc
    /// parsing or prototyping.
    ///
    /// For stability across crate versions, build a profile with an
    /// explicit revision and extension set and use
    /// [`with_profile`](Self::with_profile).
    pub fn new(input: &str) -> Result<Self, ExpressionError> {
        Self::with_profile(input, &ExprProfile::latest())
    }

    /// Parse a format string under a caller-supplied profile.
    ///
    /// Each `{{...}}` interpolation is parsed as an expression under
    /// the same profile, so the syntax features accepted inside
    /// interpolations are determined by the profile in exactly the same
    /// way as [`ParsedExpression::with_profile`](crate::ParsedExpression::with_profile).
    pub fn with_profile(input: &str, profile: &ExprProfile) -> Result<Self, ExpressionError> {
        if input.len() > MAX_FORMAT_STRING_LEN {
            return Err(ExpressionError::new(format!(
                "Format string length ({} bytes) exceeds maximum allowed size ({} bytes)",
                input.len(),
                MAX_FORMAT_STRING_LEN
            )));
        }
        let segments = parse_segments(input, profile)?;
        if segments.len() > MAX_FORMAT_STRING_SEGMENTS {
            return Err(ExpressionError::new(format!(
                "Format string contains too many interpolation segments ({}); maximum is {}",
                segments.len(),
                MAX_FORMAT_STRING_SEGMENTS
            )));
        }
        Ok(Self {
            raw: input.to_string(),
            segments,
        })
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Resolve to an `ExprValue`.
    ///
    /// Configures evaluation via [`FormatStringOptions`]. If the format string
    /// is a single expression with no surrounding literal text, returns the
    /// raw typed `ExprValue` (which may be int, list, path, etc.); otherwise
    /// it concatenates all segments and returns `ExprValue::String`.
    ///
    /// Pass `&FormatStringOptions::default()` for the simplest call. Build
    /// options with `.with_library(...)`, `.with_path_format(...)`, and
    /// `.with_target_type(...)`. Path mapping rules, if needed, are baked
    /// into the library via `FunctionLibrary::with_host_context(rules)`.
    pub fn resolve_with(
        &self,
        symtab: &SymbolTable,
        opts: &FormatStringOptions<'_>,
    ) -> Result<ExprValue, ExpressionError> {
        self.resolve_inner(symtab, opts.library, opts.path_format, opts.target_type)
    }

    /// Resolve to `String`, concatenating every segment.
    ///
    /// Single-expression format strings lose their typed value — use
    /// [`resolve_with`](Self::resolve_with) when you need the native
    /// `ExprValue`. The `target_type` field on `opts` is ignored here.
    ///
    /// `path_format` on the options controls how `path()` values are
    /// constructed:
    /// - `PathFormat::Posix` in template context (create_job, let bindings)
    /// - `PathFormat::host()` in session/host context (action execution)
    pub fn resolve_string_with(
        &self,
        symtab: &SymbolTable,
        opts: &FormatStringOptions<'_>,
    ) -> Result<String, ExpressionError> {
        let FormatStringOptions {
            library,
            path_format,
            target_type: _,
        } = *opts;
        let mut result = String::new();
        for seg in &self.segments {
            match seg {
                Segment::Literal(s) => result.push_str(s),
                Segment::Expression { parsed, .. } => {
                    let val = self.eval_parsed(parsed, symtab, library, path_format, None)?;
                    // None/null renders as empty string in format strings
                    if !matches!(val, ExprValue::Null) {
                        result.push_str(&val.to_display_string());
                    }
                }
            }
        }
        Ok(result)
    }

    fn resolve_inner(
        &self,
        symtab: &SymbolTable,
        library: Option<&crate::function_library::FunctionLibrary>,
        path_format: crate::path_mapping::PathFormat,
        target_type: Option<&crate::types::ExprType>,
    ) -> Result<ExprValue, ExpressionError> {
        if self.segments.len() == 1 {
            if let Segment::Expression { parsed, .. } = &self.segments[0] {
                return self.eval_parsed(parsed, symtab, library, path_format, target_type);
            }
        }
        self.resolve_string_with(
            symtab,
            &FormatStringOptions {
                library,
                path_format,
                target_type: None,
            },
        )
        .map(ExprValue::String)
    }

    fn eval_parsed(
        &self,
        parsed: &crate::eval::ParsedExpression,
        symtab: &SymbolTable,
        library: Option<&crate::function_library::FunctionLibrary>,
        path_format: crate::path_mapping::PathFormat,
        target_type: Option<&crate::types::ExprType>,
    ) -> Result<ExprValue, ExpressionError> {
        let mut builder = parsed.with_path_format(path_format);
        if let Some(lib) = library {
            builder = builder.with_library(lib);
        }
        if let Some(tt) = target_type {
            builder = builder.with_target_type(tt);
        }
        builder.evaluate(&[symtab])
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
                Segment::Expression { parsed, start, end } => (parsed, *start, *end),
            };
            if let Err(e) = parsed.with_library(lib).evaluate(&[symtab]) {
                return Err(FormatStringValidationError {
                    message: e.to_string(),
                    input: self.raw.clone(),
                    start,
                    end,
                    expression_error: Some(Box::new(e)),
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
            if let Segment::Expression { parsed, .. } = seg {
                check_comprehension_vars(&parsed.ast, let_names)?;
            }
        }
        Ok(())
    }

    /// True if any `{{...}}` segment is more than a bare dotted-name lookup
    /// (e.g., contains arithmetic, function calls, or list comprehensions —
    /// anything that requires the EXPR extension).
    pub fn has_complex_expressions(&self) -> bool {
        self.segments.iter().any(|s| match s {
            Segment::Expression { parsed, .. } => parsed.as_name_lookup().is_none(),
            Segment::Literal(_) => false,
        })
    }

    /// Names of all bare dotted-name interpolations (`{{Param.Name}}`-style).
    /// Complex expressions (arithmetic, function calls, etc.) are not
    /// included — callers that want all referenced symbols should use
    /// [`accessed_symbols`](Self::accessed_symbols).
    pub fn expression_names(&self) -> Vec<&str> {
        self.segments
            .iter()
            .filter_map(|s| match s {
                Segment::Expression { parsed, .. } => parsed.as_name_lookup(),
                Segment::Literal(_) => None,
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
        for segment in &self.segments {
            if let Segment::Expression { parsed, .. } = segment {
                for symbol in &parsed.accessed_symbols {
                    copy_symbol_value(symbol, source, dest);
                }
            }
        }
    }

    /// Returns the set of symbol names accessed by this format string.
    pub fn accessed_symbols(&self) -> std::collections::HashSet<String> {
        let mut symbols = std::collections::HashSet::new();
        for segment in &self.segments {
            if let Segment::Expression { parsed, .. } = segment {
                symbols.extend(parsed.accessed_symbols.iter().cloned());
            }
        }
        symbols
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

fn parse_segments(input: &str, profile: &ExprProfile) -> Result<Vec<Segment>, ExpressionError> {
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
                    ))
                    .with_span(input, cp, cp + 2));
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
                        let cp = pos + co;
                        return Err(ExpressionError::new(format!(
                            "Failed to parse interpolation expression at [{pos}, {len}]. Reason: Braces mismatch."
                        ))
                        .with_span(input, cp, cp + 2));
                    }
                }
                if op > pos {
                    segments.push(Segment::Literal(input[pos..op].to_string()));
                }
                let es = op + 2;
                match input[es..].find("}}") {
                    None => return Err(ExpressionError::new(format!(
                        "Failed to parse interpolation expression at [{op}, {len}]. Reason: Braces mismatch."
                    ))
                    .with_span(input, op, op + 2)),
                    Some(co) => {
                        let ee = es + co;
                        let be = ee + 2;
                        let et = input[es..ee].trim();
                        if et.is_empty() {
                            return Err(ExpressionError::new(format!(
                                "Failed to parse interpolation expression at [{op}, {be}]. Reason: Empty expression."
                            ))
                            .with_span(input, op, be));
                        }
                        let parsed = crate::eval::ParsedExpression::with_profile(et, profile)
                            .map_err(|e| {
                                // Attach the raw format-string source + {{...}} span so
                                // users see a caret on the failing interpolation rather
                                // than a bare parse error.
                                ExpressionError::new(format!(
                                "Failed to parse interpolation expression at [{op}, {be}]. Reason: {}",
                                e.message()
                            ))
                                .with_span(input, op, be)
                            })?;
                        segments.push(Segment::Expression { start: op, end: be, parsed });
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
    /// The original expression error, if available. Contains sub_errors
    /// for compound failures (e.g., if/else where both branches fail).
    pub expression_error: Option<Box<ExpressionError>>,
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

/// Builder-style options for [`FormatString::resolve_with`] and
/// [`FormatString::resolve_string_with`].
///
/// All fields are optional; defaults match the zero-argument shortcuts on
/// `FormatString` (host-native path format, no library, no path mapping rules,
/// no target type coercion). Chain the `with_*` methods to override any subset.
///
/// ```
/// # use openjd_expr::{FormatString, FormatStringOptions, SymbolTable, PathFormat, ExprType};
/// let fs = FormatString::new("{{Param.Frame}}").unwrap();
/// let st = SymbolTable::new();
/// let target = ExprType::INT;
/// let opts = FormatStringOptions::new()
///     .with_path_format(PathFormat::Posix)
///     .with_target_type(&target);
/// let _ = fs.resolve_with(&st, &opts);
/// ```
#[derive(Clone)]
pub struct FormatStringOptions<'a> {
    library: Option<&'a crate::function_library::FunctionLibrary>,
    path_format: crate::path_mapping::PathFormat,
    target_type: Option<&'a crate::types::ExprType>,
}

impl<'a> Default for FormatStringOptions<'a> {
    fn default() -> Self {
        Self {
            library: None,
            path_format: crate::path_mapping::PathFormat::host(),
            target_type: None,
        }
    }
}

impl<'a> FormatStringOptions<'a> {
    /// Construct options with all defaults (equivalent to `Default::default()`).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Use the given function library instead of the default (built-in) one.
    ///
    /// Accepts either `&FunctionLibrary` or `Option<&FunctionLibrary>`; `None`
    /// means "use the default library". To enable `apply_path_mapping`,
    /// pass a library built with `.with_host_context(rules)`.
    #[must_use]
    pub fn with_library(
        mut self,
        library: impl Into<Option<&'a crate::function_library::FunctionLibrary>>,
    ) -> Self {
        self.library = library.into();
        self
    }

    /// Construct `path()` values in this format (default: host-native).
    #[must_use]
    pub fn with_path_format(mut self, fmt: crate::path_mapping::PathFormat) -> Self {
        self.path_format = fmt;
        self
    }

    /// Coerce the resolved value toward this target type.
    ///
    /// Ignored by `resolve_string_with`.
    #[must_use]
    pub fn with_target_type(mut self, t: &'a crate::types::ExprType) -> Self {
        self.target_type = Some(t);
        self
    }
}

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
            fs.resolve_string_with(&SymbolTable::new(), &FormatStringOptions::default())
                .unwrap(),
            "hello"
        );
    }
    #[test]
    fn simple_expr() {
        let fs = FormatString::new("{{Param.X}}").unwrap();
        let mut st = SymbolTable::new();
        st.set_string("Param.X", "42").unwrap();
        assert_eq!(
            fs.resolve_string_with(&st, &FormatStringOptions::default())
                .unwrap(),
            "42"
        );
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
        assert_eq!(
            fs.resolve_string_with(&st, &FormatStringOptions::default())
                .unwrap(),
            "13"
        );
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
        let host_lib =
            crate::FunctionLibrary::for_profile(&crate::ExprProfile::current().with_host_context(
                crate::HostContext::with_rules(Vec::<crate::path_mapping::PathMappingRule>::new()),
            ));
        assert!(fs
            .validate_expressions(&SymbolTable::new(), &host_lib)
            .is_err());
    }
    #[test]
    fn validate_catches_empty_regex_pattern() {
        // First verify the expression itself errors
        let st = SymbolTable::new();
        let result = crate::ParsedExpression::new("re_replace('hello', '', 'x')")
            .and_then(|p| p.evaluate(&st));
        assert!(
            result.is_err(),
            "Direct eval should error, got: {:?}",
            result.map(|v| v.to_display_string())
        );

        let host_lib =
            crate::FunctionLibrary::for_profile(&crate::ExprProfile::current().with_host_context(
                crate::HostContext::with_rules(Vec::<crate::path_mapping::PathMappingRule>::new()),
            ));
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
        let result = crate::ParsedExpression::new(r"re_replace('hello', '(h)', r'\1')")
            .and_then(|p| p.evaluate(&st));
        assert!(
            result.is_err(),
            "Should reject \\1 group ref, got: {:?}",
            result.map(|v| v.to_display_string())
        );
        // Dollar group ref
        let result = crate::ParsedExpression::new("re_replace('hello', '(h)', '$1')")
            .and_then(|p| p.evaluate(&st));
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
        let host_lib =
            crate::FunctionLibrary::for_profile(&crate::ExprProfile::current().with_host_context(
                crate::HostContext::with_rules(Vec::<crate::path_mapping::PathMappingRule>::new()),
            ));
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
        let host_lib =
            crate::FunctionLibrary::for_profile(&crate::ExprProfile::current().with_host_context(
                crate::HostContext::with_rules(Vec::<crate::path_mapping::PathMappingRule>::new()),
            ));
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
        let val = fs
            .resolve_with(&st, &FormatStringOptions::default())
            .unwrap();
        assert!(matches!(val, ExprValue::Int(42)));
    }
    #[test]
    fn resolve_value_single_expr_string() {
        let fs = FormatString::new("{{Param.X}}").unwrap();
        let mut st = SymbolTable::new();
        st.set("Param.X", ExprValue::String("hello".into()))
            .unwrap();
        let val = fs
            .resolve_with(&st, &FormatStringOptions::default())
            .unwrap();
        assert!(matches!(val, ExprValue::String(ref s) if s == "hello"));
    }
    #[test]
    fn resolve_value_mixed() {
        let fs = FormatString::new("hello {{Param.X}}").unwrap();
        let mut st = SymbolTable::new();
        st.set("Param.X", ExprValue::Int(42)).unwrap();
        let val = fs
            .resolve_with(&st, &FormatStringOptions::default())
            .unwrap();
        assert!(matches!(val, ExprValue::String(ref s) if s == "hello 42"));
    }
    #[test]
    fn resolve_value_pure_literal() {
        let fs = FormatString::new("hello").unwrap();
        let val = fs
            .resolve_with(&SymbolTable::new(), &FormatStringOptions::default())
            .unwrap();
        assert!(matches!(val, ExprValue::String(ref s) if s == "hello"));
    }

    #[test]
    fn resolve_with_target_type_coerces_int_to_float() {
        let fs = FormatString::new("{{Param.X}}").unwrap();
        let mut st = SymbolTable::new();
        st.set("Param.X", ExprValue::Int(42)).unwrap();
        let target = crate::types::ExprType::FLOAT;
        let val = fs
            .resolve_with(
                &st,
                &FormatStringOptions::default().with_target_type(&target),
            )
            .unwrap();
        assert!(matches!(val, ExprValue::Float(ref f) if f.value() == 42.0));
    }

    #[test]
    fn resolve_with_target_type_none_preserves_int() {
        let fs = FormatString::new("{{Param.X}}").unwrap();
        let mut st = SymbolTable::new();
        st.set("Param.X", ExprValue::Int(42)).unwrap();
        let val = fs
            .resolve_with(&st, &FormatStringOptions::default())
            .unwrap();
        assert!(matches!(val, ExprValue::Int(42)));
    }

    #[test]
    fn resolve_with_target_type_path() {
        let fs = FormatString::new("{{Param.X}}").unwrap();
        let mut st = SymbolTable::new();
        st.set("Param.X", ExprValue::String("/foo/bar".into()))
            .unwrap();
        let target = crate::types::ExprType::PATH;
        let val = fs
            .resolve_with(
                &st,
                &FormatStringOptions::default()
                    .with_path_format(crate::path_mapping::PathFormat::Posix)
                    .with_target_type(&target),
            )
            .unwrap();
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

    #[test]
    fn accessed_symbols_simple() {
        let fs = FormatString::new("render --frame {{Param.Frame}}").unwrap();
        let syms = fs.accessed_symbols();
        assert!(syms.contains("Param.Frame"));
        assert_eq!(syms.len(), 1);
    }

    #[test]
    fn accessed_symbols_multiple_refs() {
        let fs = FormatString::new("{{Param.Start + Param.End}}").unwrap();
        let syms = fs.accessed_symbols();
        assert!(syms.contains("Param.Start"));
        assert!(syms.contains("Param.End"));
        assert_eq!(syms.len(), 2);
    }

    #[test]
    fn accessed_symbols_literal_returns_empty() {
        let fs = FormatString::new("just a literal").unwrap();
        assert!(fs.accessed_symbols().is_empty());
    }

    #[test]
    fn accessed_symbols_method_call() {
        let fs = FormatString::new("{{Param.Name.upper()}}").unwrap();
        let syms = fs.accessed_symbols();
        // The parser resolves the attribute chain to the base symbol
        assert!(syms.contains("Param.Name"));
    }

    #[test]
    fn accessed_symbols_multiple_segments() {
        let fs = FormatString::new("{{Param.A}}_{{Param.B}}").unwrap();
        let syms = fs.accessed_symbols();
        assert!(syms.contains("Param.A"));
        assert!(syms.contains("Param.B"));
        assert_eq!(syms.len(), 2);
    }

    // ── FormatStringOptions builder tests ──

    #[test]
    fn options_default_matches_host_format() {
        let opts = FormatStringOptions::new();
        assert_eq!(opts.path_format, crate::path_mapping::PathFormat::host());
        assert!(opts.library.is_none());
        assert!(opts.target_type.is_none());
    }

    #[test]
    fn options_with_path_format() {
        let fs = FormatString::new("{{path('/tmp/out')}}").unwrap();
        let st = SymbolTable::new();
        let opts =
            FormatStringOptions::new().with_path_format(crate::path_mapping::PathFormat::Posix);
        let val = fs.resolve_with(&st, &opts).unwrap();
        match val {
            ExprValue::Path { format, .. } => {
                assert_eq!(format, crate::path_mapping::PathFormat::Posix);
            }
            _ => panic!("expected path value, got {:?}", val),
        }
    }

    #[test]
    fn options_with_target_type_coerces() {
        let fs = FormatString::new("{{42}}").unwrap();
        let st = SymbolTable::new();
        let target = crate::types::ExprType::FLOAT;
        let opts = FormatStringOptions::new().with_target_type(&target);
        let val = fs.resolve_with(&st, &opts).unwrap();
        assert!(matches!(val, ExprValue::Float(_)), "got {:?}", val);
    }

    #[test]
    fn options_default_equivalent_to_builder() {
        // FormatStringOptions::default() behaves the same as manually constructing
        // with all defaults, and both produce the correct Int result from a
        // single-expression format string.
        let fs = FormatString::new("{{Param.X + 1}}").unwrap();
        let mut st = SymbolTable::new();
        st.set("Param.X", ExprValue::Int(10)).unwrap();

        let a = fs
            .resolve_with(&st, &FormatStringOptions::default())
            .unwrap();
        let b = fs.resolve_with(&st, &FormatStringOptions::new()).unwrap();
        match (a, b) {
            (ExprValue::Int(11), ExprValue::Int(11)) => {}
            (a, b) => panic!("expected Int(11) for both; got {:?} vs {:?}", a, b),
        }
    }

    #[test]
    fn options_resolve_string_with_ignores_target_type() {
        // resolve_string_with always concatenates to string; target_type is ignored.
        let fs = FormatString::new("{{Param.X}}").unwrap();
        let mut st = SymbolTable::new();
        st.set("Param.X", ExprValue::Int(42)).unwrap();
        let t = crate::types::ExprType::FLOAT;
        let opts = FormatStringOptions::new().with_target_type(&t);
        let s = fs.resolve_string_with(&st, &opts).unwrap();
        assert_eq!(s, "42");
    }

    #[test]
    fn options_with_library_is_plumbed() {
        // Build a library that only registers `len`, without `upper`, and confirm that
        // when we set it on the options, evaluation uses the restricted library.
        let fs = FormatString::new("{{ upper('hi') }}").unwrap();
        let st = SymbolTable::new();
        let mut minimal = crate::function_library::FunctionLibrary::new();
        minimal
            .register_sig("len", "(string) -> int", crate::functions::misc::len_string)
            .unwrap();
        let opts = FormatStringOptions::new().with_library(&minimal);
        assert!(
            fs.resolve_with(&st, &opts).is_err(),
            "should reject unknown function"
        );
    }
}
