// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Expression parsing and evaluation.
//!
//! Uses `ruff_python_parser` to parse Python expression syntax into an AST,
//! then evaluates with a custom bounded evaluator. This mirrors the Python
//! implementation which uses `ast.parse()` + a custom `Evaluator` class.

pub(crate) mod evaluator;
mod parse;

pub(crate) use evaluator::Evaluator;
pub use evaluator::{EvaluationResult, DEFAULT_MEMORY_LIMIT, DEFAULT_OPERATION_LIMIT};
pub use parse::{EvaluationBuilder, ParsedExpression};
