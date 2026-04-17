// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Expression parsing and evaluation.
//!
//! Uses `ruff_python_parser` to parse Python expression syntax into an AST,
//! then evaluates with a custom bounded evaluator. This mirrors the Python
//! implementation which uses `ast.parse()` + a custom `Evaluator` class.

pub mod evaluator;
mod parse;

pub use evaluator::{EvaluationResult, Evaluator, DEFAULT_MEMORY_LIMIT, DEFAULT_OPERATION_LIMIT};
pub use parse::ParsedExpression;
