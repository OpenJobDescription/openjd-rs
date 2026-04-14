// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Open Job Description expression language.
//!
//! This crate implements the expression language for OpenJD templates:
//! - Format string parsing and resolution (`{{Expr.Name}}` syntax)
//! - EXPR extension expression evaluation (arithmetic, conditionals, functions)
//! - Type system, runtime values, and symbol tables
//! - Range expressions and path mapping
//!
//! Uses `ruff_python_parser` for EXPR extension expression parsing.
//! See `specs/parser-selection.md` for rationale.

pub mod default_library;
pub(crate) mod edit_distance;
pub mod error;
pub mod eval;
pub mod format_string;
pub mod function_library;
pub mod functions;
pub mod path_mapping;
pub mod range_expr;
pub mod symbol_table;
pub mod types;
pub mod uri_path;
pub mod value;

pub use error::{ExpressionError, ExpressionErrorKind};
pub use eval::{
    EvaluationResult, Evaluator, ParsedExpression, DEFAULT_MEMORY_LIMIT, DEFAULT_OPERATION_LIMIT,
};
pub use format_string::escape_format_string;
pub use format_string::FormatString;
pub use format_string::FormatStringValidationError;
pub use path_mapping::{PathFormat, PathMappingRule};
pub use range_expr::{RangeExpr, RangeExprError};
pub use symbol_table::{SerializedSymbolTable, SymbolTable, SymbolTableError};
pub use types::{ExprType, TypeCode};
pub use value::ExprValue;

/// Evaluate a Python expression string against a symbol table.
///
/// This is the simplest entry point for expression evaluation, using
/// host-native path format and the default function library.
///
/// For custom `path_format`, `library`, or resource limits, use
/// [`ParsedExpression`] with the builder pattern on [`Evaluator`]:
///
/// ```
/// use openjd_expr::{ParsedExpression, SymbolTable, PathFormat, ExprValue};
/// use openjd_expr::default_library::get_default_library;
///
/// let symtab = SymbolTable::new();
/// let lib = get_default_library();
/// let parsed = ParsedExpression::new("1 + 2").unwrap();
/// let symtabs = [&symtab];
/// let mut ev = parsed.evaluator(&symtabs)
///     .with_path_format(PathFormat::Posix)
///     .with_library(lib)
///     .with_memory_limit(10_000_000);
/// let result = ev.evaluate(&parsed.ast).unwrap();
/// assert_eq!(result, ExprValue::Int(3));
/// ```
pub fn evaluate_expression(expr: &str, symtab: &SymbolTable) -> Result<ExprValue, ExpressionError> {
    let parsed = ParsedExpression::new(expr)?;
    let symtabs = [symtab];
    let mut evaluator = parsed.evaluator(&symtabs);
    evaluator.evaluate(&parsed.ast)
}

/// Evaluate with explicit resource limits.
///
/// Returns an [`EvaluationResult`] containing the value plus
/// `peak_memory` and `operation_count` metrics.
pub fn evaluate_expression_bounded(
    expr: &str,
    symtab: &SymbolTable,
    memory_limit: usize,
    operation_limit: usize,
) -> Result<EvaluationResult, ExpressionError> {
    let parsed = ParsedExpression::new(expr)?;
    let symtabs = [symtab];
    let mut evaluator = parsed
        .evaluator(&symtabs)
        .with_memory_limit(memory_limit)
        .with_operation_limit(operation_limit);
    let value = evaluator.evaluate(&parsed.ast)?;
    Ok(EvaluationResult {
        value,
        peak_memory: evaluator.peak_memory(),
        operation_count: evaluator.operation_count(),
    })
}
