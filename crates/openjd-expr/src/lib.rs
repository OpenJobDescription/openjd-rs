// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Open Job Description expression language.
//!
//! This crate implements the expression language for OpenJD templates:
//! - Format string parsing and resolution (`{{Expr.Name}}` syntax)
//! - EXPR extension expression evaluation (arithmetic, conditionals, functions)
//! - Type system, runtime values, and symbol tables
//! - Range expressions and path mapping
//!
//! Uses `ruff_python_parser` for EXPR extension expression parsing.
//! See `specs/expr/parser.md` for rationale.

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
    EvalBuilder, EvalResult, ParsedExpression, DEFAULT_MEMORY_LIMIT, DEFAULT_OPERATION_LIMIT,
    MAX_EXPRESSION_DEPTH, MAX_PARSE_INPUT_LEN,
};
pub use format_string::escape_format_string;
pub use format_string::FormatString;
pub use format_string::FormatStringOptions;
pub use format_string::FormatStringValidationError;
pub use function_library::{EvalContext, FunctionLibrary};
pub use path_mapping::{PathFormat, PathMappingRule};
pub use range_expr::{RangeExpr, RangeExprError, MAX_RANGE_EXPR_CHUNKS};
pub use symbol_table::{
    SerializedSymbolTable, SymbolTable, SymbolTableError, MAX_SYMBOL_TABLE_ENTRIES,
};
pub use types::{ExprType, TypeCode};
pub use value::ExprValue;
