// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Function implementations for the expression language.

pub mod arithmetic;
pub mod comparison;
pub mod conversion;
pub mod list;
pub mod math;
pub mod misc;
pub mod path;
pub mod path_parse;
pub mod regex;
pub mod repr;
pub mod string;

use crate::error::ExpressionError;
use crate::function_library::EvalContext;
use crate::value::ExprValue;

/// A checked upper bound for a string result built by a function.
struct StringOutputBudget {
    max_bytes: usize,
}

impl StringOutputBudget {
    fn reserve(
        ctx: &mut dyn EvalContext,
        work_bytes: usize,
        max_bytes: usize,
    ) -> Result<Self, ExpressionError> {
        ctx.count_string_ops(work_bytes)?;
        ctx.check_memory(max_bytes)?;
        Ok(Self { max_bytes })
    }

    fn finish(self, output: String) -> ExprValue {
        debug_assert!(output.len() <= self.max_bytes);
        ExprValue::String(output)
    }
}
