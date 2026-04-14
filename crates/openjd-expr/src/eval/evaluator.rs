// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Expression evaluator with memory-bounded and operation-bounded execution.
//!
//! Walks the Python AST produced by `ruff_python_parser` and evaluates it
//! against a symbol table, mirroring the Python `Evaluator` class in
//! `openjd.expr._eval._evaluator`.

use ruff_python_ast as ast;

use crate::error::{ExpressionError, ExpressionErrorKind};
use crate::symbol_table::SymbolTable;
use crate::value::{ExprValue, Float64};
use crate::path_mapping::PathFormat;

/// Default memory limit: 100 million bytes.
pub const DEFAULT_MEMORY_LIMIT: usize = 100_000_000; // 100 million bytes per spec

/// Default operation limit: 10 million.
pub const DEFAULT_OPERATION_LIMIT: usize = 10_000_000;

/// Result of expression evaluation.
#[derive(Debug)]
pub struct EvaluationResult {
    pub value: ExprValue,
    pub peak_memory: usize,
    pub operation_count: usize,
}

/// Expression evaluator with resource bounds.
///
/// Walks a parsed Python AST and evaluates it against symbol tables, using a
/// function library for operator and function dispatch. Tracks memory and
/// operation counts to enforce configurable resource limits.
///
/// # Builder Pattern
///
/// Create via [`ParsedExpression::evaluator`] or [`Evaluator::new`], then
/// configure with `with_*` methods before calling [`evaluate`](Self::evaluate):
///
/// ```
/// use openjd_expr::{ParsedExpression, SymbolTable, PathFormat, ExprValue};
/// use openjd_expr::path_mapping::PathMappingRule;
///
/// let parsed = ParsedExpression::new("Param.Frame * 2").unwrap();
/// let mut symtab = SymbolTable::new();
/// symtab.set("Param.Frame", 5).unwrap();
/// let symtabs = [&symtab];
///
/// let mut ev = parsed.evaluator(&symtabs)
///     .with_path_format(PathFormat::Posix)
///     .with_memory_limit(50_000_000)
///     .with_operation_limit(1_000_000);
/// let result = ev.evaluate(&parsed.ast).unwrap();
/// assert_eq!(result, ExprValue::Int(10));
/// ```
pub struct Evaluator<'a> {
    symtabs: &'a [&'a SymbolTable],
    path_format: PathFormat,
    expr_source: Option<&'a str>,
    memory_limit: usize,
    operation_limit: usize,
    current_memory: usize,
    peak_memory: usize,
    operation_count: usize,
    keyword_renames: &'a std::collections::HashMap<String, String>,
    library: &'a crate::function_library::FunctionLibrary,
    path_mapping_rules: &'a [crate::path_mapping::PathMappingRule],
    target_type: Option<crate::types::ExprType>,
    regex_cache: std::collections::HashMap<String, regex::Regex>,
}

static EMPTY_KEYWORD_RENAMES: std::sync::LazyLock<std::collections::HashMap<String, String>> = std::sync::LazyLock::new(std::collections::HashMap::new);
static EMPTY_RULES: &[crate::path_mapping::PathMappingRule] = &[];

impl<'a> Evaluator<'a> {
    /// Create a new evaluator with default settings.
    ///
    /// Symbol tables are searched in order during name resolution.
    /// Use [`ParsedExpression::evaluator`] instead when evaluating a parsed
    /// expression, as it pre-configures keyword renames and source context.
    pub fn new(symtabs: &'a [&'a SymbolTable]) -> Self {
        Self {
            symtabs,
            path_format: PathFormat::host(),
            expr_source: None,
            memory_limit: DEFAULT_MEMORY_LIMIT,
            operation_limit: DEFAULT_OPERATION_LIMIT,
            current_memory: 0,
            peak_memory: 0,
            operation_count: 0,
            keyword_renames: &EMPTY_KEYWORD_RENAMES,
            library: crate::default_library::get_default_library(),
            path_mapping_rules: EMPTY_RULES,
            target_type: None, regex_cache: std::collections::HashMap::new(),
        }
    }

    /// Set a custom function library for operator and function dispatch.
    ///
    /// Default: [`get_default_library()`](crate::default_library::get_default_library),
    /// which includes all built-in functions. Use a custom library to add
    /// host-context functions or restrict available operations.
    #[must_use]
    pub fn with_library(mut self, library: &'a crate::function_library::FunctionLibrary) -> Self {
        self.library = library; self
    }

    /// Set the maximum memory (in bytes) that evaluation may consume.
    ///
    /// Default: [`DEFAULT_MEMORY_LIMIT`] (100 MB). Every intermediate value
    /// is tracked; exceeding this limit returns
    /// [`MemoryLimitExceeded`](crate::ExpressionErrorKind::MemoryLimitExceeded).
    #[must_use]
    pub fn with_memory_limit(mut self, limit: usize) -> Self {
        self.memory_limit = limit; self
    }

    /// Set the maximum number of operations that evaluation may perform.
    ///
    /// Default: [`DEFAULT_OPERATION_LIMIT`] (10M). Each function call costs 1,
    /// list iterations cost N, and string processing costs ceil(len/256).
    /// Exceeding this limit returns
    /// [`OperationLimitExceeded`](crate::ExpressionErrorKind::OperationLimitExceeded).
    #[must_use]
    pub fn with_operation_limit(mut self, limit: usize) -> Self {
        self.operation_limit = limit; self
    }

    /// Set the path format for path operations and validation.
    ///
    /// Default: host-native format ([`PathFormat::host()`]). Controls how
    /// path properties (`.name`, `.parent`, etc.), path construction, and
    /// `apply_path_mapping` behave. Also validates that `Path` values in the
    /// symbol table match this format.
    #[must_use]
    pub fn with_path_format(mut self, format: PathFormat) -> Self {
        self.path_format = format; self
    }

    /// Set the target type for context-dependent coercion.
    ///
    /// When set, influences how empty list literals infer their element type
    /// and how mixed-type expressions are coerced. Used internally by the
    /// model layer when the expected type of a template field is known.
    #[must_use]
    pub fn with_target_type(mut self, t: &crate::types::ExprType) -> Self {
        self.target_type = Some(t.clone()); self
    }

    /// Set path mapping rules for `apply_path_mapping()` calls.
    ///
    /// Default: empty (no mappings). These rules are only accessible to
    /// host-context functions via the [`EvalContext`](crate::function_library::EvalContext)
    /// trait. First matching rule wins.
    #[must_use]
    pub fn with_path_mapping_rules(mut self, rules: &'a [crate::path_mapping::PathMappingRule]) -> Self {
        self.path_mapping_rules = rules;
        self
    }

    /// Set the original expression source text for error messages.
    ///
    /// When set, errors include the source text with caret positioning.
    /// Automatically configured by [`ParsedExpression::evaluator`].
    #[must_use]
    pub fn with_expr_source(mut self, source: &'a str) -> Self {
        self.expr_source = Some(source); self
    }

    /// Set keyword rename mappings for Python keyword attribute access.
    ///
    /// Maps renamed identifiers back to their original names (e.g.,
    /// `if_` → `if`) so that `Param.if` works despite `if` being a Python
    /// keyword. Automatically configured by [`ParsedExpression::evaluator`].
    #[must_use]
    pub fn with_keyword_renames(mut self, renames: &'a std::collections::HashMap<String, String>) -> Self {
        self.keyword_renames = renames; self
    }

    pub fn peak_memory(&self) -> usize { self.peak_memory }
    pub fn operation_count(&self) -> usize { self.operation_count }

    /// Evaluate an AST expression node.
    pub fn evaluate(&mut self, node: &ast::Expr) -> Result<ExprValue, ExpressionError> {
        let result = self.evaluate_inner(node);
        // Attach caret context to any error that doesn't already have it
        match result {
            Err(e) if e.expr().is_none() => {
                if let Some(src) = &self.expr_source {
                    Err(e.with_node(src, node))
                } else {
                    Err(e)
                }
            }
            Ok(val) => {
                if let Some(ref tt) = self.target_type {
                    val.coerce(tt, self.path_format)
                        .map_err(|msg| {
                            let e = ExpressionError::new(msg);
                            if let Some(src) = &self.expr_source {
                                e.with_node(src, node)
                            } else {
                                e
                            }
                        })
                } else {
                    Ok(val)
                }
            }
            other => other,
        }
    }

    fn evaluate_inner(&mut self, node: &ast::Expr) -> Result<ExprValue, ExpressionError> {
        match node {
            ast::Expr::NumberLiteral(n) => self.eval_number(n),
            ast::Expr::StringLiteral(s) => self.eval_string(s),
            ast::Expr::BooleanLiteral(b) => { self.track(ExprValue::Bool(b.value)) }
            ast::Expr::NoneLiteral(_) => { self.track(ExprValue::Null) }
            ast::Expr::Name(n) => self.eval_name(n),
            ast::Expr::Attribute(a) => self.eval_attribute(a),
            ast::Expr::BinOp(b) => self.eval_binop(b),
            ast::Expr::UnaryOp(u) => self.eval_unaryop(u),
            ast::Expr::BoolOp(b) => self.eval_boolop(b),
            ast::Expr::Compare(c) => self.eval_compare(c),
            ast::Expr::If(i) => self.eval_ifexp(i),
            ast::Expr::Call(c) => self.eval_call(c),
            ast::Expr::List(l) => self.eval_list(l),
            ast::Expr::Subscript(s) => self.eval_subscript(s),
            ast::Expr::ListComp(lc) => {
                // Validate restrictions
                if lc.generators.len() != 1 {
                    return Err(ExpressionError::unsupported("Multiple 'for' clauses in list comprehensions are not supported"));
                }
                let gen = &lc.generators[0];
                if gen.ifs.len() > 1 {
                    return Err(ExpressionError::unsupported("Multiple 'if' clauses in a list comprehension are not supported; combine with 'and'"));
                }
                if !matches!(&gen.target, ast::Expr::Name(_)) {
                    return Err(ExpressionError::unsupported("Tuple unpacking in list comprehension is not supported"));
                }
                // Reject loop variables that don't start with lowercase or underscore
                if let ast::Expr::Name(n) = &gen.target {
                    let var_name = n.id.as_str();
                    if !var_name.is_empty() && !var_name.starts_with(|c: char| c.is_lowercase() || c == '_') {
                        return Err(ExpressionError::new(format!(
                            "Loop variable '{var_name}' must start with a lowercase letter or underscore"
                        )));
                    }
                }
                // Evaluate the iterable
                let iterable = self.evaluate(&gen.iter)?;
                if iterable.is_unresolved() {
                    // Determine element type from the iterable's inner type
                    let inner = unwrap_unresolved(&iterable.expr_type());
                    let elem_type = if let Some(et) = inner.list_element_type() {
                        et.clone()
                    } else if inner.code == crate::types::TypeCode::RangeExpr {
                        ExprType::INT
                    } else {
                        ExprType::INT
                    };
                    // Evaluate the body with an unresolved loop variable to determine output type
                    let var_name = match &gen.target {
                        ast::Expr::Name(n) => n.id.to_string(),
                        _ => unreachable!(),
                    };
                    let mut tmp = crate::symbol_table::SymbolTable::new();
                    tmp.set(&var_name, ExprValue::unresolved(elem_type.clone())).map_err(|e| ExpressionError::new(e.to_string()))?;
                    let combined: Vec<&crate::symbol_table::SymbolTable> = {
                        let mut v: Vec<&crate::symbol_table::SymbolTable> = self.symtabs.to_vec();
                        v.push(&tmp);
                        v
                    };
                    let mut child = Evaluator {
                        symtabs: &combined,
                        path_format: self.path_format,
                        expr_source: self.expr_source,
                        memory_limit: self.memory_limit,
                        operation_limit: self.operation_limit,
                        current_memory: self.current_memory,
                        peak_memory: self.peak_memory,
                        operation_count: self.operation_count,
                        keyword_renames: self.keyword_renames,
                        library: self.library,
                        path_mapping_rules: self.path_mapping_rules,
                        target_type: None, regex_cache: std::collections::HashMap::new(),
                    };
                    let body_val = child.evaluate(&lc.elt)?;
                    self.current_memory = child.current_memory;
                    self.peak_memory = child.peak_memory;
                    self.operation_count = child.operation_count;
                    let body_type = unwrap_unresolved(&body_val.expr_type());
                    return self.track(ExprValue::unresolved(ExprType::list(body_type)));
                }
                let items: Vec<ExprValue> = if let Some(iter) = iterable.list_iter() {
                    iter.collect()
                } else if let ExprValue::RangeExpr(r) = &iterable {
                    r.iter().map(ExprValue::Int).collect()
                } else {
                    return Err(ExpressionError::type_error(format!("Cannot iterate over {}", iterable.expr_type())));
                };
                // Release the iterable — we've extracted the items
                self.release(&iterable);
                // Get loop variable name
                let var_name = match &gen.target {
                    ast::Expr::Name(n) => n.id.to_string(),
                    _ => unreachable!(),
                };
                // Evaluate each element
                let mut result = Vec::new();
                let base_symtabs: Vec<&crate::symbol_table::SymbolTable> = self.symtabs.to_vec();
                for item in &items {
                    self.count_op()?;
                    let mut tmp = crate::symbol_table::SymbolTable::new();
                    tmp.set(&var_name, item.clone()).map_err(|e| ExpressionError::new(e.to_string()))?;
                    let mut combined = base_symtabs.clone();
                    combined.push(&tmp);
                    let mut child = Evaluator {
                        symtabs: &combined,
                        path_format: self.path_format,
                        expr_source: self.expr_source,
                        memory_limit: self.memory_limit,
                        operation_limit: self.operation_limit,
                        current_memory: self.current_memory,
                        peak_memory: self.peak_memory,
                        operation_count: self.operation_count,
                        keyword_renames: self.keyword_renames,
                        library: self.library,
                        path_mapping_rules: self.path_mapping_rules,
                        target_type: None, regex_cache: std::collections::HashMap::new(),
                    };
                    // Check filter
                    let mut include = true;
                    if let Some(if_clause) = gen.ifs.first() {
                        let cond = child.evaluate(if_clause)?;
                        if let ExprValue::Bool(b) = cond { include = b; }
                    }
                    if include {
                        let val = child.evaluate(&lc.elt)?;
                        result.push(val);
                    }
                    // Propagate counters back
                    self.current_memory = child.current_memory;
                    self.peak_memory = child.peak_memory;
                    self.operation_count = child.operation_count;
                    // Release the loop variable's memory for this iteration
                    self.current_memory = self.current_memory.saturating_sub(item.memory_size());
                }
                // Check nesting depth — same as list literals
                for e in &result {
                    let t = e.expr_type();
                    if let Some(inner) = t.list_element_type() {
                        if inner.list_element_type().is_some() {
                            return Err(ExpressionError::new(
                                "Lists may be nested at most 2 levels deep"
                            ));
                        }
                    }
                }
                let elem_type = if result.is_empty() { ExprType::INT } else { result[0].expr_type() };
                self.track(ExprValue::make_list(result, elem_type)?)
            }
            ast::Expr::Slice(s) => {
                // Check for step=0
                if let Some(step) = &s.step {
                    let step_val = self.evaluate(step)?;
                    if let ExprValue::Int(0) = step_val {
                        return Err(ExpressionError::new("Slice step cannot be zero"));
                    }
                }
                self.track(ExprValue::unresolved(ExprType::INT))
            }
            ast::Expr::Starred(_) => Err(ExpressionError::unsupported("Star unpacking is not supported")),
            ast::Expr::Lambda(_) => Err(ExpressionError::unsupported("Lambda expressions are not supported")),
            ast::Expr::Dict(_) => Err(ExpressionError::unsupported("Dict literals are not supported")),
            ast::Expr::Set(_) => Err(ExpressionError::unsupported("Set literals are not supported")),
            ast::Expr::DictComp(_) => Err(ExpressionError::unsupported("Dict comprehensions are not supported")),
            ast::Expr::SetComp(_) => Err(ExpressionError::unsupported("Set comprehensions are not supported")),
            ast::Expr::Generator(_) => Err(ExpressionError::unsupported("Generator expressions are not supported")),
            ast::Expr::Await(_) => Err(ExpressionError::unsupported("Await expressions are not supported")),
            ast::Expr::FString(_) => Err(ExpressionError::unsupported("f-strings are not supported; use string concatenation")),
            ast::Expr::BytesLiteral(_) => Err(ExpressionError::unsupported("Byte strings are not supported")),
            ast::Expr::EllipsisLiteral(_) => Err(ExpressionError::unsupported("Ellipsis is not supported")),
            ast::Expr::Tuple(_) => Err(ExpressionError::unsupported("Tuple literals are not supported")),
            ast::Expr::Named(_) => Err(ExpressionError::unsupported("Walrus operator (:=) is not supported")),
            _ => Err(ExpressionError::unsupported("Unsupported expression type")),
        }
    }

    fn count_op(&mut self) -> Result<(), ExpressionError> {
        self.operation_count += 1;
        if self.operation_count > self.operation_limit {
            Err(ExpressionError::from_kind(ExpressionErrorKind::OperationLimitExceeded))
        } else { Ok(()) }
    }

    /// Collect all available symbol names from all symbol tables.
    fn collect_symbol_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for symtab in self.symtabs {
            symtab.all_paths("", &mut names);
        }
        names.sort();
        names.dedup();
        names
    }

    fn track(&mut self, value: ExprValue) -> Result<ExprValue, ExpressionError> {
        // Check for infinity/NaN in float results
        if let ExprValue::Float(f) = &value {
            if f.0.is_infinite() { return Err(ExpressionError::float_error("Float operation produced infinity")); }
            if f.0.is_nan() { return Err(ExpressionError::float_error("Float operation produced NaN")); }
        }
        self.current_memory += value.memory_size();
        if self.current_memory > self.peak_memory { self.peak_memory = self.current_memory; }
        if self.current_memory > self.memory_limit {
            Err(ExpressionError::from_kind(ExpressionErrorKind::MemoryLimitExceeded {
                used: self.current_memory,
                limit: self.memory_limit,
            }))
        } else { Ok(value) }
    }

    fn release(&mut self, value: &ExprValue) {
        let size = value.memory_size();
        self.current_memory = self.current_memory.saturating_sub(size);
    }

    fn dispatch_with_node(&mut self, name: &str, args: Vec<ExprValue>, node: Option<&ast::Expr>) -> Result<ExprValue, ExpressionError> {
        self.count_op()?;
        let input_size: usize = args.iter().map(|a| a.memory_size()).sum();
        let lib = self.library;
        let result = lib.call(name, &args, self).map_err(|e| {
            if let (Some(src), Some(n)) = (self.expr_source.as_deref(), node) {
                e.with_node(src, n)
            } else { e }
        })?;
        self.current_memory = self.current_memory.saturating_sub(input_size);
        self.track(result)
    }

    fn eval_number(&mut self, n: &ast::ExprNumberLiteral) -> Result<ExprValue, ExpressionError> {
        match &n.value {
            ast::Number::Int(i) => {
                let val: i64 = i.as_i64().ok_or_else(|| {
                    ExpressionError::integer_overflow()
                })?;
                self.track(ExprValue::Int(val))
            }
            ast::Number::Float(f) => {
                if f.is_infinite() {
                    return Err(ExpressionError::float_error("Float operation produced infinity"));
                }
                if f.is_nan() {
                    return Err(ExpressionError::float_error("Float operation produced NaN"));
                }
                // Preserve original source text for passthrough (e.g., "1.5e3", "3.500")
                let original = self.expr_source.as_ref().and_then(|src| {
                    let start = n.range.start().to_usize();
                    let end = n.range.end().to_usize();
                    if end <= src.len() {
                        let s = &src[start..end];
                        // Don't preserve malformed forms like "3." or ".5" or underscore-containing
                        if s.ends_with('.') || s.starts_with('.') || s.contains('_') {
                            None
                        } else {
                            Some(s.to_string())
                        }
                    } else { None }
                });
                self.track(ExprValue::Float(if let Some(s) = original { Float64::with_str(*f, s)? } else { Float64::new(*f)? }))
            }
            ast::Number::Complex { .. } => Err(ExpressionError::unsupported("Complex numbers are not supported")),
        }
    }

    fn eval_string(&mut self, s: &ast::ExprStringLiteral) -> Result<ExprValue, ExpressionError> {
        // Reject u'...' prefix (Python 2 compat, not supported in EXPR)
        for part in &s.value {
            if matches!(part.flags.prefix(), ast::str_prefix::StringLiteralPrefix::Unicode) {
                return Err(ExpressionError::new(
                    "Unicode string prefix u'...' is not supported. Use '...' or \"...\" instead."
                ));
            }
        }
        self.track(ExprValue::String(s.value.to_string()))
    }

    fn check_path_format(&self, value: &ExprValue, name: &str) -> Result<(), ExpressionError> {
        match value {
            ExprValue::Path { format, .. } if *format != self.path_format => {
                Err(ExpressionError::new(format!(
                    "Path format mismatch for '{name}': value has {format:?} but evaluator uses {:?}",
                    self.path_format
                )))
            }
            ExprValue::ListPath(items, fmt, _) if !items.is_empty() && *fmt != self.path_format => {
                Err(ExpressionError::new(format!(
                    "Path format mismatch for '{name}': value has {fmt:?} but evaluator uses {:?}",
                    self.path_format
                )))
            }
            _ => Ok(()),
        }
    }

    fn eval_name(&mut self, n: &ast::ExprName) -> Result<ExprValue, ExpressionError> {
        let name = n.id.as_str();
        match name {
            "True" | "true" => return self.track(ExprValue::Bool(true)),
            "False" | "false" => return self.track(ExprValue::Bool(false)),
            "None" | "null" => return self.track(ExprValue::Null),
            _ => {}
        }
        for symtab in self.symtabs.iter().rev() {
            if let Some(val) = symtab.get_value(name) {
                self.check_path_format(&val, name)?;
                return self.track(val.clone());
            }
        }
        let available = self.collect_symbol_names();
        let suggestion = crate::edit_distance::suggest_closest(name, &available.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        Err(ExpressionError::from_kind(ExpressionErrorKind::UndefinedVariable {
            name: name.to_string(),
            suggestion,
        }))
    }

    fn eval_attribute(&mut self, a: &ast::ExprAttribute) -> Result<ExprValue, ExpressionError> {
        // Try full dotted path lookup, resolving keyword renames
        let dotted_path = build_dotted_name(&ast::Expr::Attribute(a.clone()));
        if let Some(ref path) = dotted_path {
            let resolved = resolve_keyword_renames(path, &self.keyword_renames);
            for symtab in self.symtabs.iter().rev() {
                if let Some(val) = symtab.get_value(&resolved) {
                    self.count_op()?;
                    self.check_path_format(&val, &resolved)?;
                    return self.track(val.clone());
                }
            }
        }
        // Fall back: evaluate the value, then access the attribute via library.
        // If the base evaluation fails (e.g., "Param" is a subtable not a value),
        // and we had a dotted path, report the dotted path as undefined with suggestions.
        let value = match self.evaluate(&a.value) {
            Ok(v) => v,
            Err(_) if dotted_path.is_some() => {
                let path = dotted_path.as_ref().unwrap();
                let available = self.collect_symbol_names();
                let suggestion = crate::edit_distance::suggest_closest(
                    path, &available.iter().map(|s| s.as_str()).collect::<Vec<_>>()
                );
                let src = self.expr_source.as_deref().unwrap_or("");
                let attr_node = ast::Expr::Attribute(a.clone());
                return Err(ExpressionError::from_kind(ExpressionErrorKind::UndefinedVariable {
                    name: path.clone(),
                    suggestion,
                }).with_node(src, &attr_node));
            }
            Err(e) => return Err(e),
        };
        let attr = a.attr.as_str();
        let prop_name = format!("__property_{attr}__");
        let attr_node = ast::Expr::Attribute(a.clone());
        match self.dispatch_with_node(&prop_name, vec![value.clone()], Some(&attr_node)) {
            Ok(v) => Ok(v),
            Err(_) => {
                let src = self.expr_source.as_deref().unwrap_or("");
                let val_type = value.expr_type();
                let val_type_str = if val_type.code == crate::types::TypeCode::Unresolved && !val_type.params.is_empty() {
                    val_type.params[0].to_string()
                } else { val_type.to_string() };

                // Check if attr is a known method (not a property)
                {
                    let lib = self.library;
                    if !lib.get_signatures(attr).is_empty() {
                        return Err(ExpressionError::new(format!(
                            "'{attr}' is a method, not a property. Did you mean {attr}()?"
                        )).with_node(src, &attr_node));
                    }
                }

                // Show available types for this property
                {
                    let lib = self.library;
                    let valid_types: Vec<String> = lib.get_signatures(&prop_name).iter()
                        .filter_map(|e| e.signature.sig_params().first())
                        .filter(|t: &&crate::types::ExprType| !t.is_symbolic())
                        .map(|t: &crate::types::ExprType| t.to_string())
                        .collect::<std::collections::BTreeSet<_>>().into_iter().collect();
                    if !valid_types.is_empty() {
                        return Err(ExpressionError::new(format!(
                            "'{attr}' property is not available for {val_type_str}. Available for: {}",
                            valid_types.join(", ")
                        )).with_node(src, &attr_node));
                    }
                }

                Err(ExpressionError::new(format!(
                    "Cannot access attribute '{attr}' on {val_type_str}"
                )).with_node(src, &attr_node))
            }
        }
    }

    fn eval_binop(&mut self, b: &ast::ExprBinOp) -> Result<ExprValue, ExpressionError> {
        // Reject unsupported operators early
        let op_name = match b.op {
            ast::Operator::Add => "__add__",
            ast::Operator::Sub => "__sub__",
            ast::Operator::Mult => "__mul__",
            ast::Operator::Div => "__truediv__",
            ast::Operator::FloorDiv => "__floordiv__",
            ast::Operator::Mod => "__mod__",
            ast::Operator::Pow => "__pow__",
            ast::Operator::BitAnd => return Err(ExpressionError::unsupported("Bitwise AND (&) is not supported")),
            ast::Operator::BitOr => return Err(ExpressionError::unsupported("Bitwise OR (|) is not supported")),
            ast::Operator::BitXor => return Err(ExpressionError::unsupported("Bitwise XOR (^) is not supported")),
            ast::Operator::LShift => return Err(ExpressionError::unsupported("Left shift (<<) is not supported")),
            ast::Operator::RShift => return Err(ExpressionError::unsupported("Right shift (>>) is not supported")),
            ast::Operator::MatMult => return Err(ExpressionError::unsupported("Matrix multiply (@) is not supported")),
        };

        let left = self.evaluate(&b.left)?;
        let right = self.evaluate(&b.right)?;
        self.dispatch_with_node(op_name, vec![left, right], Some(&ast::Expr::BinOp(b.clone())))
    }

    fn eval_unaryop(&mut self, u: &ast::ExprUnaryOp) -> Result<ExprValue, ExpressionError> {
        self.count_op()?;
        match u.op {
            ast::UnaryOp::Invert => return Err(ExpressionError::unsupported("Bitwise NOT (~) is not supported")),
            _ => {}
        }
        // Fold -<int literal> to handle INT64_MIN which can't be represented
        // as a positive literal followed by negation (matching Python trick)
        if matches!(u.op, ast::UnaryOp::USub) {
            if let ast::Expr::NumberLiteral(n) = &*u.operand {
                if let ast::Number::Int(i) = &n.value {
                    // Try positive first, then negate
                    if let Some(pos) = i.as_i64() {
                        return self.track(ExprValue::Int(pos.checked_neg().ok_or_else(|| {
                            ExpressionError::integer_overflow()
                        })?));
                    }
                    // Handle i64::MAX + 1 = 9223372036854775808 → -i64::MIN
                    if let Some(pos) = i.as_u64() {
                        if pos == 9223372036854775808u64 {
                            return self.track(ExprValue::Int(i64::MIN));
                        }
                    }
                    return Err(ExpressionError::integer_overflow());
                }
            }
        }
        let operand = self.evaluate(&u.operand)?;
        let op_name = match u.op {
            ast::UnaryOp::USub => "__neg__",
            ast::UnaryOp::UAdd => "__pos__",
            ast::UnaryOp::Not => "__not__",
            ast::UnaryOp::Invert => unreachable!(),
        };
        self.dispatch_with_node(op_name, vec![operand], Some(&ast::Expr::UnaryOp(u.clone())))
    }

    fn eval_boolop(&mut self, b: &ast::ExprBoolOp) -> Result<ExprValue, ExpressionError> {
        self.count_op()?;
        let mut last = ExprValue::Bool(match b.op { ast::BoolOp::And => true, ast::BoolOp::Or => false });
        let mut seen_unresolved = false;
        for node in &b.values {
            if seen_unresolved {
                // After an unresolved operand, suppress errors in subsequent operands
                // (the unresolved value might short-circuit at runtime).
                // But if a subsequent operand determines the result, return it.
                match self.evaluate(node) {
                    Ok(val) => {
                        match b.op {
                            ast::BoolOp::And => {
                                if matches!(&val, ExprValue::Null | ExprValue::Bool(false)) { return Ok(val); }
                            }
                            ast::BoolOp::Or => {
                                if !matches!(&val, ExprValue::Null | ExprValue::Bool(false)) { return Ok(val); }
                            }
                        }
                    }
                    Err(_) => { /* suppressed — unresolved might short-circuit */ }
                }
                continue;
            }
            last = self.evaluate(node)?;
            if last.is_unresolved() {
                seen_unresolved = true;
                continue;
            }
            match b.op {
                // EXPR semantics: and/or only short-circuit on null and bool false
                ast::BoolOp::And => {
                    if matches!(&last, ExprValue::Null | ExprValue::Bool(false)) { return Ok(last); }
                }
                ast::BoolOp::Or => {
                    if !matches!(&last, ExprValue::Null | ExprValue::Bool(false)) { return Ok(last); }
                }
            }
        }
        if seen_unresolved { return self.track(ExprValue::unresolved(ExprType::BOOL)); }
        Ok(last)
    }

    fn eval_compare(&mut self, c: &ast::ExprCompare) -> Result<ExprValue, ExpressionError> {
        self.count_op()?;
        // Reject is/is not
        for op in &c.ops {
            match op {
                ast::CmpOp::Is => return Err(ExpressionError::unsupported("'is' operator is not supported; use '=='")),
                ast::CmpOp::IsNot => return Err(ExpressionError::unsupported("'is not' operator is not supported; use '!='")),
                _ => {}
            }
        }
        let mut left = self.evaluate(&c.left)?;
        for (op, right_node) in c.ops.iter().zip(c.comparators.iter()) {
            let right = self.evaluate(right_node)?;
            if left.is_unresolved() || right.is_unresolved() {
                self.release(&left);
                self.release(&right);
                return self.track(ExprValue::unresolved(ExprType::BOOL));
            }
            let (op_name, args) = match op {
                ast::CmpOp::Eq => ("__eq__", vec![left.clone(), right.clone()]),
                ast::CmpOp::NotEq => ("__ne__", vec![left.clone(), right.clone()]),
                ast::CmpOp::Lt => ("__lt__", vec![left.clone(), right.clone()]),
                ast::CmpOp::LtE => ("__le__", vec![left.clone(), right.clone()]),
                ast::CmpOp::Gt => ("__gt__", vec![left.clone(), right.clone()]),
                ast::CmpOp::GtE => ("__ge__", vec![left.clone(), right.clone()]),
                // For 'in'/'not in', container is first arg (right), item is second (left)
                ast::CmpOp::In => ("__contains__", vec![right.clone(), left.clone()]),
                ast::CmpOp::NotIn => ("__not_contains__", vec![right.clone(), left.clone()]),
                ast::CmpOp::Is => return Err(ExpressionError::unsupported("'is' operator is not supported; use '=='")),
                ast::CmpOp::IsNot => return Err(ExpressionError::unsupported("'is not' operator is not supported; use '!='")),
            };

            // Build a synthetic node spanning left..right for error caret positioning
            let node = ast::Expr::Compare(c.clone());
            let result_val = self.dispatch_with_node(op_name, args, Some(&node))?;
            let result = match &result_val {
                ExprValue::Bool(b) => *b,
                _ => true,
            };
            self.release(&result_val);

            if !result {
                self.release(&left);
                self.release(&right);
                return self.track(ExprValue::Bool(false));
            }
            self.release(&left);
            left = right;
        }
        self.release(&left);
        self.track(ExprValue::Bool(true))
    }

    fn eval_ifexp(&mut self, i: &ast::ExprIf) -> Result<ExprValue, ExpressionError> {
        self.count_op()?;
        let test = self.evaluate(&i.test)?;
        if test.is_unresolved() {
            // Check that the unresolved type is compatible with bool
            let inner = unwrap_unresolved(&test.expr_type());
            let is_bool_compatible = inner == ExprType::BOOL
                || inner.code == crate::types::TypeCode::Unresolved
                || inner.code == crate::types::TypeCode::Any
                || (inner.code == crate::types::TypeCode::Union && inner.params.contains(&ExprType::BOOL));
            if !is_bool_compatible {
                let err = ExpressionError::new(format!(
                    "Condition must be a boolean, got {}", inner
                ));
                self.release(&test);
                return Err(if let Some(src) = self.expr_source.as_deref() {
                    err.with_node(src, &i.test)
                } else { err });
            }
            self.release(&test);
            // Try both branches, catching errors (e.g. fail() in one branch)
            let body = self.evaluate(&i.body);
            let orelse = self.evaluate(&i.orelse);
            match (body, orelse) {
                (Err(be), Err(oe)) => {
                    let mut msg = format!(
                        "Both branches fail in the if/else:\n  if-branch: {}\n",
                        be.message()
                    );
                    if let Some(src) = be.expr() {
                        msg.push_str(&format!("  {src}\n"));
                        if let (Some(col), Some(end)) = (be.col_offset(), be.end_col_offset()) {
                            let caret_off = be.caret_offset().unwrap_or(0);
                            let prefix = " ".repeat(2 + col + caret_off);
                            let width = end.saturating_sub(col).saturating_sub(caret_off);
                            msg.push_str(&format!("{prefix}^{}\n", "~".repeat(width.saturating_sub(1))));
                        }
                    }
                    msg.push_str(&format!("  else-branch: {}\n", oe.message()));
                    if let Some(src) = oe.expr() {
                        msg.push_str(&format!("  {src}\n"));
                        if let (Some(col), Some(end)) = (oe.col_offset(), oe.end_col_offset()) {
                            let caret_off = oe.caret_offset().unwrap_or(0);
                            let prefix = " ".repeat(2 + col + caret_off);
                            let width = end.saturating_sub(col).saturating_sub(caret_off);
                            msg.push_str(&format!("{prefix}^{}", "~".repeat(width.saturating_sub(1))));
                        }
                    }
                    let mut err = ExpressionError::new(msg);
                    if let Some(src) = self.expr_source.as_deref() {
                        use ruff_text_size::Ranged;
                        let start = i.range().start().to_usize();
                        let end = i.range().end().to_usize();
                        err.set_source_span(src, start, end, 0);
                    }
                    Err(err)
                }
                (Ok(b), Err(_)) => {
                    let t = unwrap_unresolved(&b.expr_type());
                    self.track(ExprValue::unresolved(t))
                }
                (Err(_), Ok(o)) => {
                    let t = unwrap_unresolved(&o.expr_type());
                    self.track(ExprValue::unresolved(t))
                }
                (Ok(b), Ok(o)) => {
                    let bt = unwrap_unresolved(&b.expr_type());
                    let ot = unwrap_unresolved(&o.expr_type());
                    if bt == ot {
                        self.track(ExprValue::unresolved(bt))
                    } else {
                        self.track(ExprValue::unresolved(ExprType::union(vec![bt, ot])))
                    }
                }
            }
        } else {
            // Condition must be bool
            if !matches!(&test, ExprValue::Bool(_)) {
                let err = ExpressionError::new(format!(
                    "Condition must be a boolean, got {}", test.expr_type()
                ));
                return Err(if let Some(src) = self.expr_source.as_deref() {
                    err.with_node(src, &i.test)
                } else { err });
            }
            if test.is_truthy() {
                self.release(&test);
                self.evaluate(&i.body)
            } else {
                self.release(&test);
                self.evaluate(&i.orelse)
            }
        }
    }

    fn eval_call(&mut self, c: &ast::ExprCall) -> Result<ExprValue, ExpressionError> {
        self.count_op()?;
        // Reject keyword args and **kwargs
        if !c.arguments.keywords.is_empty() {
            return Err(ExpressionError::unsupported("Keyword arguments are not supported"));
        }
        // Get function name and check receiver
        let mut receiver_value: Option<ExprValue> = None;
        let is_method_call;
        let func_name = match &*c.func {
            ast::Expr::Name(n) => { is_method_call = false; Some(n.id.to_string()) }
            ast::Expr::Attribute(a) => {
                let receiver = self.evaluate(&a.value)?;
                receiver_value = Some(receiver);
                is_method_call = true;
                Some(a.attr.to_string())
            }
            _ => { is_method_call = false; None }
        };
        // Evaluate arguments
        let mut args = Vec::new();
        for arg in &c.arguments.args {
            args.push(self.evaluate(arg)?);
        }

        // Normalize call convention: receiver (if any) becomes args[0].
        // After this, every function sees a uniform args list regardless of
        // whether it was called as obj.method(x) or method(obj, x).
        if let Some(recv) = receiver_value {
            args.insert(0, recv);
        }

        let _any_unresolved = args.iter().any(|a| a.is_unresolved());
        // Release args from memory tracking — they're consumed by the function.
        // Done before dispatch so early returns don't leak.
        let args_size: usize = args.iter().map(|a| a.memory_size()).sum();
        self.current_memory = self.current_memory.saturating_sub(args_size);

        // Dispatch through function library
        if let Some(name) = &func_name {
            if name.starts_with("__") && name.ends_with("__") {
                return Err(ExpressionError::new(format!("Cannot call '{}' directly", name)));
            }
            {
                let lib = self.library;
                let result = if is_method_call { lib.call_method(name, &args, self) } else { lib.call(name, &args, self) };
                let result = result.map_err(|e| {
                    let src = self.expr_source.as_deref().unwrap_or("");
                    let call_node = ast::Expr::Call(c.clone());
                    if is_method_call && !lib.get_signatures(&format!("__property_{name}__")).is_empty() {
                        return ExpressionError::new(format!(
                            "'{name}' is a property, not a method. Use .{name} instead of .{name}()"
                        )).with_node(src, &call_node);
                    }
                    e.with_node(src, &call_node)
                })?;
                self.track(result)
            }
        } else {
            Err(ExpressionError::new("Cannot call non-function expression"))
        }
    }

    fn eval_list(&mut self, l: &ast::ExprList) -> Result<ExprValue, ExpressionError> {
        // Extract element target type from list[T] target
        let list_elem_target = self.target_type.as_ref().and_then(|tt| {
            if tt.code == crate::types::TypeCode::List && tt.params.len() == 1 {
                Some(tt.params[0].clone())
            } else {
                None
            }
        });

        // Thread element target type down for nested list evaluation
        let saved_target = self.target_type.take();
        if let Some(ref elem_t) = list_elem_target {
            self.target_type = Some(elem_t.clone());
        }

        let mut elements = Vec::new();
        for elt in &l.elts {
            let val = self.evaluate_inner(elt)?;
            if matches!(&val, ExprValue::Null) {
                self.target_type = saved_target;
                return Err(ExpressionError::new("null is not allowed in list literals"));
            }
            elements.push(val);
        }

        // Restore target type
        self.target_type = saved_target;

        // Check nesting depth — max 2 levels (list[list[T]] ok, list[list[list[T]]] not)
        for e in &elements {
            let t = e.expr_type();
            if let Some(inner) = t.list_element_type() {
                if inner.list_element_type().is_some() {
                    return Err(ExpressionError::new(
                        "Lists may be nested at most 2 levels deep"
                    ));
                }
            }
        }
        if elements.iter().any(|e| e.is_unresolved()) {
            // Check type compatibility even with unresolved elements
            if !elements.is_empty() {
                let first_type = unwrap_unresolved(&elements[0].expr_type());
                for (_i, e) in elements.iter().enumerate().skip(1) {
                    let t = unwrap_unresolved(&e.expr_type());
                    // Allow int/float and path/string mixing
                    if (first_type == ExprType::INT && t == ExprType::FLOAT) ||
                       (first_type == ExprType::FLOAT && t == ExprType::INT) ||
                       (first_type == ExprType::PATH && t == ExprType::STRING) ||
                       (first_type == ExprType::STRING && t == ExprType::PATH) { continue; }
                    if t.code == crate::types::TypeCode::Unresolved || first_type.code == crate::types::TypeCode::Unresolved { continue; }
                    if t != first_type {
                        return Err(ExpressionError::new(format!(
                            "List literal contains incompatible types: {first_type}, {t}"
                        )));
                    }
                }
            }
            let elem_type = if elements.is_empty() {
                ExprType::NULLTYPE
            } else {
                // Compute coerced element type: int+float→float, path+string→string
                let mut result = unwrap_unresolved(&elements[0].expr_type());
                for e in elements.iter().skip(1) {
                    let t = unwrap_unresolved(&e.expr_type());
                    if t.code == crate::types::TypeCode::Unresolved { continue; }
                    if result.code == crate::types::TypeCode::Unresolved { result = t; continue; }
                    if (result == ExprType::INT && t == ExprType::FLOAT) || (result == ExprType::FLOAT && t == ExprType::INT) {
                        result = ExprType::FLOAT;
                    } else if (result == ExprType::PATH && t == ExprType::STRING) || (result == ExprType::STRING && t == ExprType::PATH) {
                        result = ExprType::STRING;
                    }
                }
                result
            };
            return self.track(ExprValue::unresolved(ExprType::list(elem_type)));
        }
        // If we have a list element target, coerce each element and skip homogeneity check
        if let Some(ref elem_t) = list_elem_target {
            let coerced: Result<Vec<ExprValue>, _> = elements.into_iter()
                .map(|e| e.coerce(elem_t, self.path_format)
                    .map_err(|msg| ExpressionError::new(msg)))
                .collect();
            return self.track(ExprValue::make_list(coerced?, elem_t.clone())?);
        }
        // Check type consistency
        if !elements.is_empty() {
            let mut seen_types: Vec<ExprType> = Vec::new();
            for e in elements.iter() {
                let t = e.expr_type();
                if !seen_types.iter().any(|s| *s == t) { seen_types.push(t); }
            }
            // Check compatibility: allow int/float mixing and path/string mixing
            let dominated: Vec<&ExprType> = seen_types.iter().filter(|t| {
                // nulltype is compatible with anything
                t.code == crate::types::TypeCode::Null ||
                // int is compatible if float is also present (promotion)
                (**t == ExprType::INT && seen_types.iter().any(|s| *s == ExprType::FLOAT)) ||
                // float is compatible if int is also present (promotion)
                (**t == ExprType::FLOAT && seen_types.iter().any(|s| *s == ExprType::INT)) ||
                // path is compatible if string is also present
                (**t == ExprType::PATH && seen_types.iter().any(|s| *s == ExprType::STRING)) ||
                // string is compatible if path is also present
                (**t == ExprType::STRING && seen_types.iter().any(|s| *s == ExprType::PATH))
            }).collect();
            // If all types are in compatible pairs, it's fine; otherwise error
            let compatible = dominated.len() == seen_types.len() ||
                seen_types.len() == 1 ||
                // All list types are compatible (make_list handles inner promotion)
                seen_types.iter().all(|t| t.code == crate::types::TypeCode::List || t.code == crate::types::TypeCode::Null);
            if !compatible {
                let type_strs: Vec<String> = seen_types.iter().map(|t| t.to_string()).collect();
                let msg = if type_strs.len() == 2 {
                    format!("List literal contains incompatible types: {} and {}", type_strs[0], type_strs[1])
                } else {
                    let last = type_strs.last().unwrap();
                    let rest = type_strs[..type_strs.len()-1].join(", ");
                    format!("List literal contains incompatible types: {rest}, and {last}")
                };
                return Err(ExpressionError::new(msg));
            }
        }
        let elem_type = if elements.is_empty() { ExprType::NULLTYPE } else { elements[0].expr_type() };
        self.track(ExprValue::make_list(elements, elem_type)?)
    }

    fn eval_subscript(&mut self, s: &ast::ExprSubscript) -> Result<ExprValue, ExpressionError> {
        self.count_op()?;
        let value = self.evaluate(&s.value)?;

        // Reject subscript on path type
        if matches!(&value, ExprValue::Path { .. }) {
            return Err(ExpressionError::new(format!("Cannot subscript type path")));
        }

        // Handle slice syntax: value[start:stop:step]
        if let ast::Expr::Slice(sl) = &*s.slice {
            let start = match sl.lower.as_ref().map(|e| self.evaluate(e)).transpose()? {
                Some(v) => v,
                None => ExprValue::Null,
            };
            let stop = match sl.upper.as_ref().map(|e| self.evaluate(e)).transpose()? {
                Some(v) => v,
                None => ExprValue::Null,
            };
            let step = match sl.step.as_ref().map(|e| self.evaluate(e)).transpose()? {
                Some(v) => v,
                None => ExprValue::Null,
            };

            if let ExprValue::Int(0) = &step {
                return Err(ExpressionError::new("Slice step cannot be zero"));
            }

            if value.is_unresolved() {
                let inner = unwrap_unresolved(&value.expr_type());
                if let Some(elem) = inner.list_element_type() {
                    return self.track(ExprValue::unresolved(ExprType::list(elem.clone())));
                }
                return self.track(ExprValue::unresolved(inner));
            }

            // If any slice bound is unresolved, propagate unresolved
            let any_bound_unresolved = start.is_unresolved()
                || stop.is_unresolved()
                || step.is_unresolved();
            if any_bound_unresolved {
                if value.is_list() {
                    let elem_type = value.list_elem_type().unwrap();
                    return self.track(ExprValue::unresolved(ExprType::list(elem_type.clone())));
                } else if matches!(&value, ExprValue::String(_)) {
                    return self.track(ExprValue::unresolved(ExprType::STRING));
                }
            }

            // Dispatch 4-arg __getitem__ through the library
            let node = ast::Expr::Subscript(s.clone());
            return self.dispatch_with_node("__getitem__", vec![value, start, stop, step], Some(&node));
        }

        let slice = self.evaluate(&s.slice)?;

        // Check index type for unresolved values
        if slice.is_unresolved() {
            let inner = unwrap_unresolved(&slice.expr_type());
            if inner != ExprType::INT && inner.code != crate::types::TypeCode::Unresolved && inner.code != crate::types::TypeCode::Any {
                let mut err = ExpressionError::new("Index must be an integer");
                if let Some(src) = self.expr_source.as_deref() {
                    use ruff_text_size::Ranged;
                    let start = s.value.range().start().to_usize();
                    let end = s.range().end().to_usize();
                    let left_end = s.value.range().end().to_usize();
                    err.set_source_span(src, start, end, left_end - start);
                }
                return Err(err);
            }
        }

        // Single-index access: dispatch through library
        self.dispatch_with_node("__getitem__", vec![value, slice], Some(&ast::Expr::Subscript(s.clone())))
    }
}

/// Try to build a dotted name from an attribute chain.
/// Applies keyword renames to restore original names.
impl<'a> crate::function_library::EvalContext for Evaluator<'a> {
    fn path_format(&self) -> PathFormat { self.path_format }
    fn path_mapping_rules(&self) -> &[crate::path_mapping::PathMappingRule] {
        &self.path_mapping_rules
    }
    fn count_op(&mut self) -> Result<(), ExpressionError> {
        self.operation_count += 1;
        if self.operation_count > self.operation_limit {
            Err(ExpressionError::from_kind(ExpressionErrorKind::OperationLimitExceeded))
        } else { Ok(()) }
    }
    fn count_ops(&mut self, n: usize) -> Result<(), ExpressionError> {
        self.operation_count += n;
        if self.operation_count > self.operation_limit {
            Err(ExpressionError::from_kind(ExpressionErrorKind::OperationLimitExceeded))
        } else { Ok(()) }
    }
    fn count_string_ops(&mut self, len: usize) -> Result<(), ExpressionError> {
        let ops = (len + 255) / 256;
        self.operation_count += ops;
        if self.operation_count > self.operation_limit {
            Err(ExpressionError::from_kind(ExpressionErrorKind::OperationLimitExceeded))
        } else { Ok(()) }
    }
    fn get_or_compile_regex(&mut self, pattern: &str) -> Result<regex::Regex, ExpressionError> {
        if let Some(re) = self.regex_cache.get(pattern) {
            return Ok(re.clone());
        }
        let re = regex::Regex::new(pattern).map_err(|e| ExpressionError::new(format!("Invalid regex: {e}")))?;
        self.regex_cache.insert(pattern.to_string(), re.clone());
        Ok(re)
    }
}

/// Unwrap unresolved[T] to T, or return the type as-is if not unresolved.
fn unwrap_unresolved(t: &ExprType) -> ExprType {
    if t.code == crate::types::TypeCode::Unresolved && !t.params.is_empty() {
        t.params[0].clone()
    } else {
        t.clone()
    }
}

fn build_dotted_name(expr: &ast::Expr) -> Option<String> {
    let mut parts = Vec::new();
    let mut current = expr;
    loop {
        match current {
            ast::Expr::Name(n) => { parts.push(n.id.as_str()); break; }
            ast::Expr::Attribute(a) => { parts.push(a.attr.as_str()); current = &a.value; }
            _ => return None,
        }
    }
    parts.reverse();
    Some(parts.join("."))
}

fn resolve_keyword_renames(name: &str, renames: &std::collections::HashMap<String, String>) -> String {
    if renames.is_empty() { return name.to_string(); }
    let mut result = name.to_string();
    for (replacement, original) in renames {
        // Replace .replacement with .original in dotted paths
        let from = format!(".{replacement}");
        let to = format!(".{original}");
        result = result.replace(&from, &to);
    }
    result
}

use crate::types::ExprType;

// Re-export for backward compatibility
pub use crate::uri_path::{UriParts, parse as parse_uri_parts};


