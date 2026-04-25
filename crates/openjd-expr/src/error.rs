// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Expression language error types.

use std::fmt;

/// Structured error kinds for expression evaluation.
///
/// Callers can match on specific variants to handle errors programmatically
/// without parsing error message strings.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum ExpressionErrorKind {
    /// A referenced variable was not found in any symbol table.
    #[error("Undefined variable: '{name}'.{suggestion}")]
    UndefinedVariable { name: String, suggestion: String },

    /// A referenced function was not found in the function library.
    #[error("Unknown function: '{name}'")]
    UnknownFunction { name: String },

    /// Argument types did not match any overload signature.
    #[error("{message}")]
    TypeError { message: String },

    /// An integer operation overflowed the 64-bit signed range.
    #[error("Integer overflow: result is outside the 64-bit signed range")]
    IntegerOverflow,

    /// Division or modulo by zero.
    #[error("{op} by zero")]
    DivisionByZero { op: &'static str },

    /// A float operation produced infinity or NaN.
    #[error("{message}")]
    FloatError { message: String },

    /// An index was out of bounds for a list, string, or range expression.
    #[error("{message}")]
    IndexOutOfBounds { message: String },

    /// Expression memory usage exceeded the configured limit.
    #[error("Expression memory usage ({used} bytes) exceeded limit ({limit} bytes)")]
    MemoryLimitExceeded { used: usize, limit: usize },

    /// Expression operation count exceeded the configured limit.
    #[error("Expression operation count ({count}) exceeded limit ({limit})")]
    OperationLimitExceeded { count: usize, limit: usize },

    /// A Python syntax feature that is not supported in the expression language.
    #[error("{feature}")]
    UnsupportedSyntax { feature: String },

    /// The `fail()` function was called explicitly.
    #[error("{0}")]
    ExplicitFail(String),

    /// A parse error from the underlying Python parser.
    #[error("{0}")]
    ParseError(String),

    /// Any other error that doesn't fit a structured variant.
    #[error("{0}")]
    Other(String),
}

/// Base error for expression evaluation.
///
/// Wraps an [`ExpressionErrorKind`] with optional source location context
/// for caret-style error formatting.
#[derive(Debug, Clone)]
pub struct ExpressionError {
    kind: Box<ExpressionErrorKind>,
    expr: Option<String>,
    col_offset: Option<usize>,
    end_col_offset: Option<usize>,
    /// Position of `^` relative to col_offset. None = 0 (start of span).
    caret_offset: Option<usize>,
    /// Sub-errors for compound failures (e.g., both branches of an if/else).
    sub_errors: Option<Vec<ExpressionError>>,
}

impl ExpressionError {
    /// Create an error from a structured kind.
    pub fn from_kind(kind: ExpressionErrorKind) -> Self {
        Self {
            kind: Box::new(kind),
            expr: None,
            col_offset: None,
            end_col_offset: None,
            caret_offset: None,
            sub_errors: None,
        }
    }

    /// Create an error with a plain message string.
    ///
    /// This is a convenience for call sites that don't need a structured kind.
    /// Prefer [`ExpressionError::from_kind`] with a specific variant when the
    /// error category is known.
    pub fn new(message: impl Into<String>) -> Self {
        Self::from_kind(ExpressionErrorKind::Other(message.into()))
    }

    // ── Convenience constructors for common error kinds ──

    /// Integer overflow error.
    pub fn integer_overflow() -> Self {
        Self::from_kind(ExpressionErrorKind::IntegerOverflow)
    }

    /// Division or modulo by zero.
    pub fn division_by_zero(op: &'static str) -> Self {
        Self::from_kind(ExpressionErrorKind::DivisionByZero { op })
    }

    /// Float produced infinity or NaN.
    pub fn float_error(message: impl Into<String>) -> Self {
        Self::from_kind(ExpressionErrorKind::FloatError {
            message: message.into(),
        })
    }

    /// Type mismatch error.
    pub fn type_error(message: impl Into<String>) -> Self {
        Self::from_kind(ExpressionErrorKind::TypeError {
            message: message.into(),
        })
    }

    /// Index out of bounds.
    pub fn index_out_of_bounds(message: impl Into<String>) -> Self {
        Self::from_kind(ExpressionErrorKind::IndexOutOfBounds {
            message: message.into(),
        })
    }

    /// Unsupported Python syntax feature.
    pub fn unsupported(feature: impl Into<String>) -> Self {
        Self::from_kind(ExpressionErrorKind::UnsupportedSyntax {
            feature: feature.into(),
        })
    }

    /// Explicit fail() call.
    pub fn explicit_fail(message: impl Into<String>) -> Self {
        Self::from_kind(ExpressionErrorKind::ExplicitFail(message.into()))
    }

    /// A parse-stage error (underlying parser, range expression parser, etc.).
    pub fn parse_error(message: impl Into<String>) -> Self {
        Self::from_kind(ExpressionErrorKind::ParseError(message.into()))
    }

    /// The structured error kind.
    pub fn kind(&self) -> &ExpressionErrorKind {
        &self.kind
    }

    /// The human-readable error message (the Display output of the kind).
    pub fn message(&self) -> String {
        self.kind.to_string()
    }

    /// Sub-errors for compound failures (e.g., both branches of an if/else).
    pub fn sub_errors(&self) -> &[ExpressionError] {
        match &self.sub_errors {
            Some(v) => v.as_slice(),
            None => &[],
        }
    }

    /// Attach sub-errors (consumes and returns self for chaining).
    pub fn with_sub_errors(mut self, sub_errors: Vec<ExpressionError>) -> Self {
        if !sub_errors.is_empty() {
            self.sub_errors = Some(sub_errors);
        }
        self
    }

    /// The expression source text, if attached.
    pub fn expr(&self) -> Option<&str> {
        self.expr.as_deref()
    }

    /// The start column offset within the expression, if attached.
    pub fn col_offset(&self) -> Option<usize> {
        self.col_offset
    }

    /// The end column offset within the expression, if attached.
    pub fn end_col_offset(&self) -> Option<usize> {
        self.end_col_offset
    }

    /// The caret offset relative to col_offset, if attached.
    pub fn caret_offset(&self) -> Option<usize> {
        self.caret_offset
    }

    /// Attach expression source and AST node span for caret formatting.
    #[must_use]
    pub fn with_node(mut self, expr_source: &str, node: &ruff_python_ast::Expr) -> Self {
        use ruff_text_size::Ranged;
        if self.expr.is_some() {
            return self;
        }
        self.expr = Some(expr_source.to_string());
        let range = node.range();
        self.col_offset = Some(range.start().to_usize());
        self.end_col_offset = Some(range.end().to_usize());
        self.caret_offset = Some(compute_caret_offset(expr_source, node));
        self
    }

    /// Attach expression source with explicit span (no AST node).
    #[must_use]
    pub fn with_span(mut self, expr_source: &str, col: usize, end_col: usize) -> Self {
        if self.expr.is_some() {
            return self;
        }
        self.expr = Some(expr_source.to_string());
        self.col_offset = Some(col);
        self.end_col_offset = Some(end_col);
        self
    }

    /// Set the expression source and span directly (for cases where `with_node`
    /// cannot be used because the span is computed manually).
    pub fn set_source_span(
        &mut self,
        expr_source: &str,
        col: usize,
        end_col: usize,
        caret_offset: usize,
    ) {
        self.expr = Some(expr_source.to_string());
        self.col_offset = Some(col);
        self.end_col_offset = Some(end_col);
        self.caret_offset = Some(caret_offset);
    }

    /// Format the error with a prefix prepended to the expression line.
    ///
    /// The caret position is shifted right by `prefix.len()` so it still
    /// points at the correct column in the combined `prefix + expr` string.
    /// Used for let-binding errors where the expression is part of a larger
    /// construct like `"x = Param.Frame + \"oops\""`.
    ///
    /// Only applies to single-line expressions with caret indicators.
    /// Falls back to the normal `Display` output for multi-line or
    /// context-free errors.
    pub fn message_with_expr_prefix(&self, prefix: &str) -> String {
        let (Some(expr), Some(col), Some(end_col)) =
            (&self.expr, self.col_offset, self.end_col_offset)
        else {
            return self.to_string();
        };
        if expr.contains('\n') {
            return self.to_string();
        }
        let msg = self.message();
        let mut out = msg;
        out.push_str("\n  ");
        out.push_str(prefix);
        out.push_str(expr);
        out.push_str("\n  ");
        let _ = write_caret_line(
            &mut out,
            col + prefix.len(),
            end_col + prefix.len(),
            self.caret_offset.unwrap_or(0),
        );
        out
    }
}

impl fmt::Display for ExpressionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)?;
        if let (Some(expr), Some(col), Some(end_col)) =
            (&self.expr, self.col_offset, self.end_col_offset)
        {
            let is_multiline = expr.contains('\n');
            // For multi-line, the parser wraps in parens shifting offsets by 1
            let (col, end_col) = if is_multiline {
                (col.saturating_sub(1), end_col.saturating_sub(1))
            } else {
                (col, end_col)
            };

            // Find the line containing col_offset and adjust to line-relative offsets
            let (expr_line, line_col, line_end_col) = if is_multiline {
                let mut pos = 0;
                let mut found_line = expr.as_str();
                let mut line_start = 0;
                for line in expr.split('\n') {
                    if pos + line.len() >= col {
                        found_line = line;
                        line_start = pos;
                        break;
                    }
                    pos += line.len() + 1; // +1 for \n
                }
                let lc = col - line_start;
                let lec = if end_col > line_start {
                    (end_col - line_start).min(found_line.len())
                } else {
                    lc + 1
                };
                (found_line, lc, lec)
            } else {
                (expr.as_str(), col, end_col)
            };

            write!(f, "\n  {expr_line}\n  ")?;
            write_caret_line(f, line_col, line_end_col, self.caret_offset.unwrap_or(0))?;
        }
        Ok(())
    }
}

impl std::error::Error for ExpressionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.kind)
    }
}

/// Render a caret-annotation line matching the format used by
/// [`ExpressionError`]'s `Display` impl: `col` leading spaces, then
/// `caret_idx` tildes, then `^`, then trailing tildes to cover
/// `end_col - col`.
///
/// `col`, `end_col`, and `caret_idx` are all zero-based column offsets
/// measured within the displayed line (so callers that add indentation
/// should add it into `col` before calling).
///
/// If the span is zero- or one-wide, only `^` is drawn (no tildes).
///
/// This is the single source of truth for caret rendering across the
/// crate — `Display for ExpressionError`, `message_with_expr_prefix`,
/// and the if/else both-branches-fail renderer in `eval_ifexp` all call
/// it to guarantee identical output.
pub(crate) fn write_caret_line(
    w: &mut dyn std::fmt::Write,
    col: usize,
    end_col: usize,
    caret_offset: usize,
) -> std::fmt::Result {
    let span_len = end_col.saturating_sub(col);
    for _ in 0..col {
        w.write_char(' ')?;
    }
    if span_len > 1 {
        let caret_idx = caret_offset.min(span_len.saturating_sub(1));
        for _ in 0..caret_idx {
            w.write_char('~')?;
        }
        w.write_char('^')?;
        for _ in 0..span_len.saturating_sub(caret_idx + 1) {
            w.write_char('~')?;
        }
    } else {
        w.write_char('^')?;
    }
    Ok(())
}

/// Compute the caret offset within a node's span based on node type.
fn compute_caret_offset(expr: &str, node: &ruff_python_ast::Expr) -> usize {
    use ruff_python_ast as ast;
    use ruff_text_size::Ranged;
    match node {
        ast::Expr::BinOp(b) => {
            let left_end = b.left.range().end().to_usize();
            let right_start = b.right.range().start().to_usize();
            let node_start = node.range().start().to_usize();
            // Scan backwards from right operand to find operator
            let bytes = expr.as_bytes();
            let mut i = right_start.saturating_sub(1);
            while i > left_end
                && i < bytes.len()
                && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'(')
            {
                i -= 1;
            }
            // Check for two-char operators (**, //)
            if i > left_end
                && i < bytes.len()
                && i >= 1
                && (bytes[i - 1..=i] == *b"**" || bytes[i - 1..=i] == *b"//")
            {
                return (i - 1) - node_start;
            }
            if i >= left_end && i < bytes.len() {
                i - node_start
            } else {
                0
            }
        }
        ast::Expr::Attribute(a) => {
            let value_end = a.value.range().end().to_usize();
            let node_start = node.range().start().to_usize();
            (value_end + 1).saturating_sub(node_start) // +1 for the dot
        }
        ast::Expr::Call(c) => {
            if let ast::Expr::Attribute(a) = &*c.func {
                let value_end = a.value.range().end().to_usize();
                let node_start = node.range().start().to_usize();
                (value_end + 1).saturating_sub(node_start)
            } else {
                0
            }
        }
        ast::Expr::Subscript(s) => {
            let value_end = s.value.range().end().to_usize();
            let node_start = node.range().start().to_usize();
            value_end.saturating_sub(node_start)
        }
        _ => 0,
    }
}
