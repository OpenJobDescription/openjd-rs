// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Expression parsing using the ruff Python parser.

use crate::error::ExpressionError;
use ruff_python_ast as ast;
use ruff_python_parser;
use std::collections::HashMap;

/// A parsed expression ready for evaluation.
#[derive(Debug, Clone)]
#[must_use]
pub struct ParsedExpression {
    pub(crate) ast: ast::Expr,
    pub(crate) expr: String,
    #[allow(dead_code)] // preserves original untrimmed input for potential future use
    source: String,
    pub(crate) keyword_renames: HashMap<String, String>,
    pub(crate) accessed_symbols: HashSet<String>,
    pub(crate) called_functions: HashSet<String>,
    pub(crate) local_bindings: HashSet<String>,
}

impl ParsedExpression {
    /// The trimmed expression string.
    pub fn expression(&self) -> &str {
        &self.expr
    }

    /// Symbol names accessed by this expression (e.g. `Param.Name`).
    pub fn accessed_symbols(&self) -> &HashSet<String> {
        &self.accessed_symbols
    }

    /// Function names called by this expression (e.g. `len`, `upper`).
    pub fn called_functions(&self) -> &HashSet<String> {
        &self.called_functions
    }

    /// Loop variable names bound by list comprehensions in this expression.
    pub fn local_bindings(&self) -> &HashSet<String> {
        &self.local_bindings
    }
}

use std::collections::HashSet;

/// Python keywords that may appear as attribute names in OpenJD expressions.
const PYTHON_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

/// Generate a same-length replacement identifier that doesn't appear in the source.
fn make_replacement(keyword: &str, source: &str) -> String {
    // Use a deterministic scheme: prefix with underscores to match length
    // e.g., "if" → "xf", "def" → "xef", "else" → "xlse"
    // If that collides, try incrementing the first char
    let len = keyword.len();
    for prefix in b'a'..=b'z' {
        let mut replacement = String::with_capacity(len);
        replacement.push(prefix as char);
        for (_i, c) in keyword.chars().enumerate().skip(1) {
            // Use the original chars but shifted, to maintain length
            replacement.push(if c.is_alphabetic() { c } else { 'x' });
        }
        if replacement.len() == len
            && !source.contains(&replacement)
            && !PYTHON_KEYWORDS.contains(&replacement.as_str())
        {
            return replacement;
        }
    }
    // Fallback: all x's
    "x".repeat(len)
}

/// Parse a Python expression, handling contextual keywords (Python keywords
/// used as attribute names after '.').
///
/// Matches the Python implementation's approach:
/// 1. Try to parse
/// 2. On syntax error, find keyword after '.' at error position
/// 3. Replace with same-length identifier (preserves error positions)
/// 4. Retry
/// 5. After success, record renames so evaluator can map back
impl ParsedExpression {
    pub fn new(expr: &str) -> Result<ParsedExpression, ExpressionError> {
        let expr_str = expr.trim();
        if expr_str.is_empty() {
            return Err(ExpressionError::new("Empty expression"));
        }

        // Hard cap on parser input length. A recursive-descent parser can
        // recurse at most once per input character; the 8 MB worker-thread
        // stack comfortably handles inputs up to
        // [`MAX_PARSE_INPUT_LEN`], but beyond that we would risk overflow
        // even in the dedicated thread. Inputs exceeding this size are
        // rejected without invoking the parser at all.
        if expr_str.len() > MAX_PARSE_INPUT_LEN {
            return Err(ExpressionError::new(format!(
                "Expression source length ({} bytes) exceeds maximum allowed ({} bytes)",
                expr_str.len(),
                MAX_PARSE_INPUT_LEN
            )));
        }

        // Short-input fast path: a recursive-descent parser cannot recurse
        // more times than there are input characters to consume, and the
        // ruff parser's empirical overflow threshold on Rust's default
        // 2 MB thread stack is well above `FAST_PATH_INPUT_LEN`. Any
        // source at or below that length is safe to parse on the current
        // thread regardless of shape. The structural depth walker still
        // catches excessive AST depth that might arise from the parser's
        // own node-building patterns.
        if expr_str.len() <= FAST_PATH_INPUT_LEN {
            return parse_inner(expr, expr_str);
        }

        // Longer inputs could drive the parser's recursive descent past
        // the thread's default stack (Rust cannot recover from stack
        // overflow — it aborts the process). Run the parse + structural
        // validation + symbol collection on a dedicated thread with an
        // enlarged stack. The depth walker still rejects inputs whose AST
        // exceeds `MAX_EXPRESSION_DEPTH`, so this thread allocation is
        // purely about surviving long enough to apply that check.
        let expr_owned = expr.to_string();
        let handle = std::thread::Builder::new()
            .name("openjd-expr-parse".into())
            .stack_size(PARSER_THREAD_STACK_SIZE)
            .spawn(move || {
                let expr_str = expr_owned.trim();
                parse_inner(&expr_owned, expr_str)
            })
            .map_err(|e| ExpressionError::new(format!("Failed to spawn parser thread: {e}")))?;
        match handle.join() {
            Ok(result) => result,
            Err(_) => Err(ExpressionError::new(
                "Parser thread panicked while parsing expression",
            )),
        }
    }

    /// Evaluate this parsed expression against a single symbol table.
    ///
    /// Convenience shortcut that returns just the value. See
    /// [`evaluate_with_metrics`](Self::evaluate_with_metrics) to also
    /// observe resource usage.
    pub fn evaluate(
        &self,
        values: &crate::symbol_table::SymbolTable,
    ) -> Result<crate::value::ExprValue, crate::error::ExpressionError> {
        EvalBuilder::new(self).evaluate(&[values])
    }

    /// Evaluate this parsed expression and return the value alongside
    /// resource-usage metrics.
    ///
    /// Convenience shortcut equivalent to
    /// `parsed.with_*(default).evaluate_with_metrics(&symtabs)` — call this
    /// when you don't need any `with_*` configuration.
    pub fn evaluate_with_metrics(
        &self,
        symtabs: &[&crate::symbol_table::SymbolTable],
    ) -> Result<crate::eval::EvalResult, crate::error::ExpressionError> {
        EvalBuilder::new(self).evaluate_with_metrics(symtabs)
    }

    /// Start a configured evaluation using the [`EvalBuilder`].
    pub fn with_library<'a>(
        &'a self,
        library: &'a crate::function_library::FunctionLibrary,
    ) -> EvalBuilder<'a> {
        EvalBuilder::new(self).with_library(library)
    }

    /// Start a configured evaluation with a memory limit.
    pub fn with_memory_limit(&self, limit: usize) -> EvalBuilder<'_> {
        EvalBuilder::new(self).with_memory_limit(limit)
    }

    /// Start a configured evaluation with an operation limit.
    pub fn with_operation_limit(&self, limit: usize) -> EvalBuilder<'_> {
        EvalBuilder::new(self).with_operation_limit(limit)
    }

    /// Start a configured evaluation with an explicit path format.
    pub fn with_path_format(&self, format: crate::path_mapping::PathFormat) -> EvalBuilder<'_> {
        EvalBuilder::new(self).with_path_format(format)
    }

    /// Start a configured evaluation with a target type for coercion.
    pub fn with_target_type<'a>(
        &'a self,
        target_type: &'a crate::types::ExprType,
    ) -> EvalBuilder<'a> {
        EvalBuilder::new(self).with_target_type(target_type)
    }

    /// Create an Evaluator pre-configured with this expression's internal
    /// state (keyword renames, source context).
    ///
    /// Private: called only by [`EvalBuilder::build_evaluator`].
    /// The public API routes through `with_*(...).evaluate(&symtabs)` on
    /// `ParsedExpression` / `EvalBuilder`.
    fn evaluator<'a>(
        &'a self,
        symtabs: &'a [&'a crate::symbol_table::SymbolTable],
    ) -> crate::eval::Evaluator<'a> {
        crate::eval::Evaluator::new(symtabs)
            .with_keyword_renames(&self.keyword_renames)
            .with_expr_source(&self.expr)
    }

    /// If this expression is a simple dotted name (e.g. `Param.Name`),
    /// return it as a string. Returns `None` for anything more complex
    /// (arithmetic, function calls, subscripts, etc.).
    pub fn as_name_lookup(&self) -> Option<&str> {
        if self.called_functions.is_empty()
            && self.local_bindings.is_empty()
            && self.accessed_symbols.len() == 1
        {
            // Verify the AST is actually just an attribute chain / bare name
            if build_symbol_name(&self.ast, &self.keyword_renames).is_some() {
                return self.accessed_symbols.iter().next().map(|s| s.as_str());
            }
        }
        None
    }
}

/// A builder bound to a [`ParsedExpression`] that defers choosing symbol
/// tables until [`evaluate`](Self::evaluate) is called.
///
/// Obtained from `parsed.with_*(...)` on a [`ParsedExpression`]. Every
/// `with_*` method is chainable. The terminal `evaluate(&symtabs)` and
/// `evaluate_with_metrics(&symtabs)` methods build an internal evaluator
/// from the captured configuration and run it.
///
/// ```
/// use openjd_expr::{ExprValue, ParsedExpression, PathFormat, SymbolTable};
///
/// let parsed = ParsedExpression::new("Param.Frame * 2").unwrap();
/// let mut st = SymbolTable::new();
/// st.set("Param.Frame", 5).unwrap();
///
/// let value = parsed
///     .with_path_format(PathFormat::Posix)
///     .with_memory_limit(50_000_000)
///     .evaluate(&[&st])
///     .unwrap();
/// assert_eq!(value, ExprValue::Int(10));
/// ```
#[must_use]
pub struct EvalBuilder<'a> {
    parsed: &'a ParsedExpression,
    library: Option<&'a crate::function_library::FunctionLibrary>,
    memory_limit: Option<usize>,
    operation_limit: Option<usize>,
    path_format: Option<crate::path_mapping::PathFormat>,
    target_type: Option<&'a crate::types::ExprType>,
}

impl<'a> EvalBuilder<'a> {
    fn new(parsed: &'a ParsedExpression) -> Self {
        Self {
            parsed,
            library: None,
            memory_limit: None,
            operation_limit: None,
            path_format: None,
            target_type: None,
        }
    }

    /// Use the given function library instead of the default one.
    pub fn with_library(mut self, library: &'a crate::function_library::FunctionLibrary) -> Self {
        self.library = Some(library);
        self
    }

    /// Cap evaluation memory (bytes). Default: [`DEFAULT_MEMORY_LIMIT`](crate::DEFAULT_MEMORY_LIMIT).
    pub fn with_memory_limit(mut self, limit: usize) -> Self {
        self.memory_limit = Some(limit);
        self
    }

    /// Cap evaluation operations. Default: [`DEFAULT_OPERATION_LIMIT`](crate::DEFAULT_OPERATION_LIMIT).
    pub fn with_operation_limit(mut self, limit: usize) -> Self {
        self.operation_limit = Some(limit);
        self
    }

    /// Normalize path values according to `format`. Default: host-native.
    pub fn with_path_format(mut self, format: crate::path_mapping::PathFormat) -> Self {
        self.path_format = Some(format);
        self
    }

    /// Coerce the final value toward `target_type`. Default: no coercion.
    pub fn with_target_type(mut self, target_type: &'a crate::types::ExprType) -> Self {
        self.target_type = Some(target_type);
        self
    }

    /// Build the configured evaluator against the supplied symbol tables.
    ///
    /// Private helper shared by `evaluate` and `evaluate_with_metrics`.
    fn build_evaluator(
        &self,
        symtabs: &'a [&'a crate::symbol_table::SymbolTable],
    ) -> crate::eval::Evaluator<'a> {
        let mut ev = self.parsed.evaluator(symtabs);
        if let Some(lib) = self.library {
            ev = ev.with_library(lib);
        }
        if let Some(limit) = self.memory_limit {
            ev = ev.with_memory_limit(limit);
        }
        if let Some(limit) = self.operation_limit {
            ev = ev.with_operation_limit(limit);
        }
        if let Some(fmt) = self.path_format {
            ev = ev.with_path_format(fmt);
        }
        if let Some(tt) = self.target_type {
            ev = ev.with_target_type(tt);
        }
        ev
    }

    /// Evaluate against the supplied symbol tables and return the resulting value.
    pub fn evaluate(
        self,
        symtabs: &'a [&'a crate::symbol_table::SymbolTable],
    ) -> Result<crate::value::ExprValue, crate::error::ExpressionError> {
        let mut ev = self.build_evaluator(symtabs);
        ev.evaluate(&self.parsed.ast)
    }

    /// Evaluate and return the value alongside resource-usage metrics.
    pub fn evaluate_with_metrics(
        self,
        symtabs: &'a [&'a crate::symbol_table::SymbolTable],
    ) -> Result<crate::eval::EvalResult, crate::error::ExpressionError> {
        let mut ev = self.build_evaluator(symtabs);
        let value = ev.evaluate(&self.parsed.ast)?;
        Ok(crate::eval::EvalResult {
            value,
            peak_memory: ev.peak_memory(),
            operation_count: ev.operation_count(),
        })
    }
}

/// Build a dotted symbol name from an attribute chain, resolving keyword renames.
fn build_symbol_name(expr: &ast::Expr, renames: &HashMap<String, String>) -> Option<String> {
    match expr {
        ast::Expr::Name(n) => {
            let name = n.id.as_str();
            Some(
                renames
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| name.to_string()),
            )
        }
        ast::Expr::Attribute(a) => {
            let base = build_symbol_name(&a.value, renames)?;
            let attr = a.attr.as_str();
            let attr_resolved = renames
                .get(attr)
                .cloned()
                .unwrap_or_else(|| attr.to_string());
            Some(format!("{base}.{attr_resolved}"))
        }
        _ => None,
    }
}

/// Maximum permitted AST nesting depth.
///
/// Applied during structural validation (parse phase) and during evaluation
/// to prevent stack exhaustion on pathological inputs such as
/// `((((...1...))))` or long left-associative binop chains like
/// `1+1+...+1` that produce a deep AST even when the source is short.
///
/// A limit of 64 is well above any legitimate template expression: the
/// spec already caps list nesting at 2 and list comprehensions at a
/// single generator, and real-world expressions rarely exceed ~10
/// levels of nesting. Exceeding this limit raises
/// [`ExpressionErrorKind::ExpressionTooDeep`](crate::ExpressionErrorKind::ExpressionTooDeep).
///
/// See `specs/expr/parser.md` (Depth limit) and `specs/expr/evaluator.md`
/// (Depth limit) for the rationale.
pub const MAX_EXPRESSION_DEPTH: usize = 64;

/// Input-length threshold below which [`ParsedExpression::new`] parses on
/// the current thread instead of spawning a worker thread.
///
/// A recursive-descent parser cannot recurse more times than there are
/// input characters to consume, and the ruff parser's empirical overflow
/// threshold on the default 2 MB Rust thread stack is well above this
/// value in release builds (≥ 500 frames observed). 200 characters
/// leaves a comfortable safety margin in both release and debug profiles
/// while keeping the vast majority of real template expressions — which
/// are typically well under 100 characters — on the fast path.
///
/// This is deliberately separate from [`MAX_EXPRESSION_DEPTH`]: the depth
/// limit is a semantic abuse ceiling on AST nesting, while this threshold
/// is a pragmatic tuning knob for parser stack-safety on the caller's
/// thread.
const FAST_PATH_INPUT_LEN: usize = 200;

/// Maximum permitted source length (in bytes) for a single
/// [`ParsedExpression`]. An absolute cap independent of any AST-shape
/// analysis: the parser's recursive descent can recurse at most once per
/// input byte, so bounding input length bounds worst-case stack use. Set
/// to a value that leaves generous headroom in a debug-profile build on
/// the parser worker thread (32 MB stack).
///
/// Realistic template expressions are well under 1 KB; 64 KB is two
/// orders of magnitude above that.
pub const MAX_PARSE_INPUT_LEN: usize = 64 * 1024;

/// Stack size for the parser thread used when the source string is longer
/// than [`FAST_PATH_INPUT_LEN`]. 32 MB is large enough to handle any input
/// up to [`MAX_PARSE_INPUT_LEN`] even in debug builds, where each parser
/// frame can be several KB. In release builds this leaves orders of
/// magnitude of headroom; the allocation is a one-time reservation per
/// parse and is reclaimed when the thread ends.
const PARSER_THREAD_STACK_SIZE: usize = 32 * 1024 * 1024;

/// The actual parse loop: runs the ruff parser, handles the
/// keyword-rename retry cycle, and wires up the resulting AST into a
/// `ParsedExpression`. Factored out of [`ParsedExpression::new`] so it can
/// be invoked either on the current thread (short inputs) or on a
/// stack-enlarged worker thread (long inputs), without duplicating the
/// loop body.
fn parse_inner(raw: &str, expr_str: &str) -> Result<ParsedExpression, ExpressionError> {
    let mut source = expr_str.to_string();
    let mut keyword_renames: HashMap<String, String> = HashMap::new();

    // Wrap multi-line expressions in parentheses for implicit line continuation
    let is_multiline = source.contains('\n');

    loop {
        let to_parse = if is_multiline {
            format!("({})", source)
        } else {
            source.clone()
        };

        match ruff_python_parser::parse_expression(&to_parse) {
            Ok(parsed) => {
                let expr_node = parsed.into_expr();
                // Structural validation (matches Python implementation)
                validate_structure(&expr_node, expr_str)?;
                let mut accessed_symbols = HashSet::new();
                let mut called_functions = HashSet::new();
                let mut local_bindings = HashSet::new();
                collect_symbols(
                    &expr_node,
                    &keyword_renames,
                    &mut accessed_symbols,
                    &mut called_functions,
                    &mut local_bindings,
                );
                // Check for nested comprehension variable shadowing
                check_comprehension_shadowing(&expr_node, &HashSet::new(), expr_str)?;
                return Ok(ParsedExpression {
                    ast: expr_node,
                    source: raw.to_string(),
                    expr: expr_str.to_string(),
                    keyword_renames,
                    accessed_symbols,
                    called_functions,
                    local_bindings,
                });
            }
            Err(e) => {
                // Try to find a keyword after '.' that caused the error
                let mut found = false;
                for kw in PYTHON_KEYWORDS {
                    let pattern = format!(".{kw}");
                    if let Some(pos) = source.find(&pattern) {
                        // Check that the keyword is followed by a non-identifier char
                        let after_pos = pos + 1 + kw.len();
                        let after_char = source.chars().nth(after_pos);
                        if after_char.is_none_or(|c| !c.is_alphanumeric() && c != '_') {
                            let replacement = keyword_renames
                                .iter()
                                .find(|(_, v)| v.as_str() == *kw)
                                .map(|(k, _)| k.clone())
                                .unwrap_or_else(|| {
                                    let r = make_replacement(kw, &source);
                                    keyword_renames.insert(r.clone(), kw.to_string());
                                    r
                                });
                            // Replace in source — same length, preserves positions
                            source = format!(
                                "{}.{}{}",
                                &source[..pos],
                                replacement,
                                &source[after_pos..]
                            );
                            found = true;
                            break;
                        }
                    }
                }
                if !found {
                    let msg = format!("Syntax error: {}", e.error);
                    let start = e.location.start().to_usize();
                    let end = e.location.end().to_usize();
                    // For zero-width ranges (like EOF), point at the last char
                    let (col, end_col) = if start == end && !source.is_empty() {
                        (0, source.len())
                    } else {
                        (start.min(source.len()), end.min(source.len()))
                    };
                    let mut err = ExpressionError::new(msg);
                    if !source.is_empty() {
                        err = err.with_span(&source, col, end_col.max(col + 1));
                    }
                    return Err(err);
                }
            }
        }
    }
}

/// Validate structural constraints on the AST that can't be caught by the parser.
fn validate_structure(node: &ast::Expr, source: &str) -> Result<(), ExpressionError> {
    validate_structure_inner(node, source, 0)
}

fn validate_structure_inner(
    node: &ast::Expr,
    source: &str,
    depth: usize,
) -> Result<(), ExpressionError> {
    if depth > MAX_EXPRESSION_DEPTH {
        return Err(
            ExpressionError::expression_too_deep(depth, MAX_EXPRESSION_DEPTH)
                .with_node(source, node),
        );
    }
    fn err(msg: &str, source: &str, node: &ast::Expr) -> Result<(), ExpressionError> {
        Err(ExpressionError::new(msg).with_node(source, node))
    }
    match node {
        ast::Expr::ListComp(lc) => {
            if lc.generators.len() > 1 {
                return Err(ExpressionError::new(
                    "Multiple 'for' clauses in list comprehensions are not supported",
                )
                .with_span(source, 0, source.len()));
            }
            for gen in &lc.generators {
                if matches!(&gen.target, ast::Expr::Tuple(_)) {
                    return Err(ExpressionError::new(
                        "Tuple unpacking in list comprehension is not supported",
                    )
                    .with_node(source, &gen.target));
                }
                if gen.ifs.len() > 1 {
                    return Err(ExpressionError::new(
                        "Multiple 'if' clauses in a list comprehension are not supported; combine with 'and'",
                    ).with_span(source, 0, source.len()));
                }
                if let ast::Expr::Name(n) = &gen.target {
                    let name = n.id.as_str();
                    if !name.starts_with(|c: char| c.is_ascii_lowercase() || c == '_') {
                        return Err(ExpressionError::new(format!(
                            "Loop variable '{}' must start with a lowercase letter or underscore",
                            name
                        ))
                        .with_node(source, &gen.target));
                    }
                }
            }
            validate_structure_inner(&lc.elt, source, depth + 1)?;
            for gen in &lc.generators {
                validate_structure_inner(&gen.iter, source, depth + 1)?;
                for if_clause in &gen.ifs {
                    validate_structure_inner(if_clause, source, depth + 1)?;
                }
            }
        }
        ast::Expr::BinOp(b) => {
            match b.op {
                ast::Operator::BitAnd => {
                    return err("Bitwise AND (&) is not supported", source, node)
                }
                ast::Operator::BitOr => {
                    return err("Bitwise OR (|) is not supported", source, node)
                }
                ast::Operator::BitXor => {
                    return err("Bitwise XOR (^) is not supported", source, node)
                }
                ast::Operator::LShift => {
                    return err("Left shift (<<) is not supported", source, node)
                }
                ast::Operator::RShift => {
                    return err("Right shift (>>) is not supported", source, node)
                }
                ast::Operator::MatMult => {
                    return err("Matrix multiply (@) is not supported", source, node)
                }
                _ => {}
            }
            validate_structure_inner(&b.left, source, depth + 1)?;
            validate_structure_inner(&b.right, source, depth + 1)?;
        }
        ast::Expr::UnaryOp(u) => {
            if matches!(u.op, ast::UnaryOp::Invert) {
                return err("Bitwise NOT (~) is not supported", source, node);
            }
            validate_structure_inner(&u.operand, source, depth + 1)?;
        }
        ast::Expr::BoolOp(b) => {
            for v in &b.values {
                validate_structure_inner(v, source, depth + 1)?;
            }
        }
        ast::Expr::Compare(c) => {
            for op in &c.ops {
                match op {
                    ast::CmpOp::Is => {
                        return err("'is' operator is not supported; use '=='", source, node)
                    }
                    ast::CmpOp::IsNot => {
                        return err("'is not' operator is not supported; use '!='", source, node)
                    }
                    _ => {}
                }
            }
            validate_structure_inner(&c.left, source, depth + 1)?;
            // Chained comparisons (1<2<3<...) also grow without bound via the
            // comparators vector; treat each comparator as +1 depth to catch
            // `1<2<3<...<N` patterns that would otherwise be O(N) wide but
            // shallow in terms of recursion.
            if c.comparators.len() > MAX_EXPRESSION_DEPTH {
                return Err(ExpressionError::expression_too_deep(
                    c.comparators.len(),
                    MAX_EXPRESSION_DEPTH,
                )
                .with_node(source, node));
            }
            for comp in &c.comparators {
                validate_structure_inner(comp, source, depth + 1)?;
            }
        }
        ast::Expr::If(i) => {
            validate_structure_inner(&i.test, source, depth + 1)?;
            validate_structure_inner(&i.body, source, depth + 1)?;
            validate_structure_inner(&i.orelse, source, depth + 1)?;
        }
        ast::Expr::Call(c) => {
            validate_structure_inner(&c.func, source, depth + 1)?;
            for arg in &c.arguments.args {
                validate_structure_inner(arg, source, depth + 1)?;
            }
            if !c.arguments.keywords.is_empty() {
                return err("Keyword arguments are not supported", source, node);
            }
        }
        ast::Expr::List(l) => {
            for elt in &l.elts {
                validate_structure_inner(elt, source, depth + 1)?;
            }
        }
        ast::Expr::Subscript(s) => {
            validate_structure_inner(&s.value, source, depth + 1)?;
            validate_structure_inner(&s.slice, source, depth + 1)?;
        }
        ast::Expr::Attribute(a) => {
            validate_structure_inner(&a.value, source, depth + 1)?;
        }
        ast::Expr::StringLiteral(s) => {
            for part in &s.value {
                if matches!(
                    part.flags.prefix(),
                    ruff_python_ast::str_prefix::StringLiteralPrefix::Unicode
                ) {
                    return Err(ExpressionError::new(
                        "Unicode string prefix u'...' is not supported. Use '...' or \"...\" instead."
                    ).with_node(source, &ast::Expr::StringLiteral(s.clone())));
                }
            }
        }
        ast::Expr::BytesLiteral(b) => {
            return Err(ExpressionError::new(
                "Byte strings (b'...') are not supported. Use '...' or \"...\" instead.",
            )
            .with_node(source, &ast::Expr::BytesLiteral(b.clone())));
        }
        // Unsupported expression types
        ast::Expr::Named(_) => return err("Walrus operator (:=) is not supported", source, node),
        ast::Expr::Lambda(_) => return err("Lambda expressions are not supported", source, node),
        ast::Expr::Tuple(_) => {
            return err(
                "Tuple literals are not supported; use a list instead",
                source,
                node,
            )
        }
        ast::Expr::Dict(_) => return err("Dict literals are not supported", source, node),
        ast::Expr::Set(_) => return err("Set literals are not supported", source, node),
        ast::Expr::DictComp(_) => {
            return err(
                "Dict comprehensions are not supported; only list comprehensions are allowed",
                source,
                node,
            )
        }
        ast::Expr::SetComp(_) => {
            return err(
                "Set comprehensions are not supported; only list comprehensions are allowed",
                source,
                node,
            )
        }
        ast::Expr::Generator(_) => {
            return err(
                "Generator expressions are not supported; use a list comprehension",
                source,
                node,
            )
        }
        ast::Expr::FString(_) => {
            return err(
                "f-strings are not supported; use string concatenation",
                source,
                node,
            )
        }
        ast::Expr::EllipsisLiteral(_) => {
            return err("Ellipsis (...) is not supported", source, node)
        }
        ast::Expr::Starred(_) => return err("Star expressions are not supported", source, node),
        ast::Expr::Await(_) => return err("Await expressions are not supported", source, node),
        _ => {}
    }
    Ok(())
}

/// Check that nested comprehensions don't shadow outer comprehension variables.
fn check_comprehension_shadowing(
    node: &ast::Expr,
    outer_scope: &HashSet<String>,
    source: &str,
) -> Result<(), ExpressionError> {
    match node {
        ast::Expr::ListComp(lc) => {
            let mut comp_bindings = HashSet::new();
            for gen in &lc.generators {
                if let ast::Expr::Name(n) = &gen.target {
                    let name = n.id.to_string();
                    if outer_scope.contains(&name) {
                        return Err(ExpressionError::new(
                            format!("List comprehension variable '{}' shadows an outer comprehension variable", name),
                        ).with_span(source, 0, source.len()));
                    }
                    comp_bindings.insert(name);
                }
            }
            let new_scope: HashSet<String> = outer_scope.union(&comp_bindings).cloned().collect();
            check_comprehension_shadowing(&lc.elt, &new_scope, source)?;
            for gen in &lc.generators {
                check_comprehension_shadowing(&gen.iter, outer_scope, source)?;
                for if_clause in &gen.ifs {
                    check_comprehension_shadowing(if_clause, &new_scope, source)?;
                }
            }
        }
        ast::Expr::BinOp(b) => {
            check_comprehension_shadowing(&b.left, outer_scope, source)?;
            check_comprehension_shadowing(&b.right, outer_scope, source)?;
        }
        ast::Expr::UnaryOp(u) => {
            check_comprehension_shadowing(&u.operand, outer_scope, source)?;
        }
        ast::Expr::BoolOp(b) => {
            for v in &b.values {
                check_comprehension_shadowing(v, outer_scope, source)?;
            }
        }
        ast::Expr::Compare(c) => {
            check_comprehension_shadowing(&c.left, outer_scope, source)?;
            for comp in &c.comparators {
                check_comprehension_shadowing(comp, outer_scope, source)?;
            }
        }
        ast::Expr::If(i) => {
            check_comprehension_shadowing(&i.test, outer_scope, source)?;
            check_comprehension_shadowing(&i.body, outer_scope, source)?;
            check_comprehension_shadowing(&i.orelse, outer_scope, source)?;
        }
        ast::Expr::Call(c) => {
            check_comprehension_shadowing(&c.func, outer_scope, source)?;
            for arg in &c.arguments.args {
                check_comprehension_shadowing(arg, outer_scope, source)?;
            }
        }
        ast::Expr::List(l) => {
            for elt in &l.elts {
                check_comprehension_shadowing(elt, outer_scope, source)?;
            }
        }
        ast::Expr::Subscript(s) => {
            check_comprehension_shadowing(&s.value, outer_scope, source)?;
            check_comprehension_shadowing(&s.slice, outer_scope, source)?;
        }
        _ => {}
    }
    Ok(())
}

/// Collect accessed symbols, called functions, and local bindings from an AST.
/// Symbol collection is structural: names in call position are functions, not symbols.
fn collect_symbols(
    node: &ast::Expr,
    renames: &HashMap<String, String>,
    symbols: &mut HashSet<String>,
    functions: &mut HashSet<String>,
    locals: &mut HashSet<String>,
) {
    match node {
        ast::Expr::Name(n) => {
            let name = renames
                .get(n.id.as_str())
                .cloned()
                .unwrap_or_else(|| n.id.to_string());
            // Constants are not symbols
            if name == "True"
                || name == "False"
                || name == "None"
                || name == "true"
                || name == "false"
                || name == "null"
            {
                return;
            }
            if !locals.contains(&name) {
                symbols.insert(name);
            }
        }
        ast::Expr::Attribute(a) => {
            // Collect full dotted path as a symbol
            if let Some(full_name) = build_symbol_name(node, renames) {
                let base = full_name.split('.').next().unwrap_or("");
                if !locals.contains(base)
                    && base != "True"
                    && base != "False"
                    && base != "None"
                    && base != "true"
                    && base != "false"
                    && base != "null"
                {
                    symbols.insert(full_name);
                }
            } else {
                // Can't build dotted name (e.g. (1+2).attr) — recurse into value
                collect_symbols(&a.value, renames, symbols, functions, locals);
            }
        }
        ast::Expr::Call(c) => {
            // Function/method calls: the func itself is NOT a symbol reference
            match &*c.func {
                ast::Expr::Name(n) => {
                    // Bare function call: name is a function, not a symbol
                    let name = renames
                        .get(n.id.as_str())
                        .cloned()
                        .unwrap_or_else(|| n.id.to_string());
                    functions.insert(name);
                }
                ast::Expr::Attribute(a) => {
                    // Method call: collect method name as function, recurse into receiver as symbol
                    let method = a.attr.as_str();
                    let method_resolved = renames
                        .get(method)
                        .cloned()
                        .unwrap_or_else(|| method.to_string());
                    functions.insert(method_resolved);
                    // The receiver (a.value) IS a symbol reference
                    collect_symbols(&a.value, renames, symbols, functions, locals);
                }
                _ => collect_symbols(&c.func, renames, symbols, functions, locals),
            }
            for arg in &c.arguments.args {
                collect_symbols(arg, renames, symbols, functions, locals);
            }
        }
        ast::Expr::BinOp(b) => {
            collect_symbols(&b.left, renames, symbols, functions, locals);
            collect_symbols(&b.right, renames, symbols, functions, locals);
        }
        ast::Expr::UnaryOp(u) => {
            collect_symbols(&u.operand, renames, symbols, functions, locals);
        }
        ast::Expr::BoolOp(b) => {
            for v in &b.values {
                collect_symbols(v, renames, symbols, functions, locals);
            }
        }
        ast::Expr::Compare(c) => {
            collect_symbols(&c.left, renames, symbols, functions, locals);
            for comp in &c.comparators {
                collect_symbols(comp, renames, symbols, functions, locals);
            }
        }
        ast::Expr::If(i) => {
            collect_symbols(&i.test, renames, symbols, functions, locals);
            collect_symbols(&i.body, renames, symbols, functions, locals);
            collect_symbols(&i.orelse, renames, symbols, functions, locals);
        }
        ast::Expr::List(l) => {
            for elt in &l.elts {
                collect_symbols(elt, renames, symbols, functions, locals);
            }
        }
        ast::Expr::Subscript(s) => {
            collect_symbols(&s.value, renames, symbols, functions, locals);
            collect_symbols(&s.slice, renames, symbols, functions, locals);
        }
        ast::Expr::Slice(s) => {
            if let Some(l) = &s.lower {
                collect_symbols(l, renames, symbols, functions, locals);
            }
            if let Some(u) = &s.upper {
                collect_symbols(u, renames, symbols, functions, locals);
            }
            if let Some(st) = &s.step {
                collect_symbols(st, renames, symbols, functions, locals);
            }
        }
        ast::Expr::ListComp(lc) => {
            // Visit iterables first (before adding loop vars)
            for gen in &lc.generators {
                collect_symbols(&gen.iter, renames, symbols, functions, locals);
            }
            // Add loop variables to local scope
            for gen in &lc.generators {
                if let ast::Expr::Name(n) = &gen.target {
                    locals.insert(n.id.to_string());
                }
            }
            // Visit element expression and filters (with loop vars in scope)
            collect_symbols(&lc.elt, renames, symbols, functions, locals);
            for gen in &lc.generators {
                for if_clause in &gen.ifs {
                    collect_symbols(if_clause, renames, symbols, functions, locals);
                }
            }
            // Note: we don't remove loop vars from locals — they persist for
            // the local_bindings set. This matches Python's collect behavior.
        }
        ast::Expr::Starred(s) => {
            collect_symbols(&s.value, renames, symbols, functions, locals);
        }
        // Literals — no symbols
        ast::Expr::NumberLiteral(_)
        | ast::Expr::StringLiteral(_)
        | ast::Expr::BooleanLiteral(_)
        | ast::Expr::NoneLiteral(_)
        | ast::Expr::EllipsisLiteral(_)
        | ast::Expr::BytesLiteral(_) => {}
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_arithmetic() {
        let parsed = ParsedExpression::new("1 + 2").unwrap();
        assert!(matches!(parsed.ast, ast::Expr::BinOp(_)));
    }

    #[test]
    fn parse_attribute_access() {
        let parsed = ParsedExpression::new("Param.Name").unwrap();
        assert!(matches!(parsed.ast, ast::Expr::Attribute(_)));
    }

    #[test]
    fn parse_function_call() {
        let parsed = ParsedExpression::new("len(Param.Files)").unwrap();
        assert!(matches!(parsed.ast, ast::Expr::Call(_)));
    }

    #[test]
    fn parse_contextual_keyword_if() {
        let parsed = ParsedExpression::new("Param.if").unwrap();
        assert!(matches!(parsed.ast, ast::Expr::Attribute(_)));
        assert!(!parsed.keyword_renames.is_empty());
    }

    #[test]
    fn parse_contextual_keyword_def() {
        let parsed = ParsedExpression::new("Param.def").unwrap();
        assert!(!parsed.keyword_renames.is_empty());
    }

    #[test]
    fn parse_conditional() {
        let parsed = ParsedExpression::new("Param.X if Param.Flag else Param.Y").unwrap();
        assert!(matches!(parsed.ast, ast::Expr::If(_)));
    }

    #[test]
    fn parse_empty_error() {
        assert!(ParsedExpression::new("").is_err());
    }

    #[test]
    fn parse_syntax_error() {
        assert!(ParsedExpression::new("1 +").is_err());
    }

    #[test]
    fn keyword_replacement_same_length() {
        let parsed = ParsedExpression::new("Param.if").unwrap();
        for (replacement, original) in &parsed.keyword_renames {
            assert_eq!(
                replacement.len(),
                original.len(),
                "Replacement '{}' must be same length as original '{}'",
                replacement,
                original
            );
        }
    }
}
