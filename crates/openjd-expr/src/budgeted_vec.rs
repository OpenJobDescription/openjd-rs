// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Memory-budgeted collection of expression values.

use crate::error::ExpressionError;
use crate::function_library::EvalContext;
use crate::value::ExprValue;

/// A `Vec<ExprValue>` that checks its projected heap footprint before growth.
pub(crate) struct BudgetedVec {
    values: Vec<ExprValue>,
    value_bytes: usize,
}

impl BudgetedVec {
    /// Create an empty dynamically growing vector.
    pub(crate) fn new() -> Self {
        Self {
            values: Vec::new(),
            value_bytes: 0,
        }
    }

    /// Create a vector with an exact, pre-checked capacity.
    pub(crate) fn with_capacity(
        ctx: &mut dyn EvalContext,
        capacity: usize,
    ) -> Result<Self, ExpressionError> {
        ctx.check_memory(capacity.saturating_mul(size_of::<ExprValue>()))?;
        Ok(Self {
            values: Vec::with_capacity(capacity),
            value_bytes: 0,
        })
    }

    /// Push a value after checking its bytes and the projected capacity slack.
    pub(crate) fn push(
        &mut self,
        ctx: &mut dyn EvalContext,
        value: ExprValue,
    ) -> Result<(), ExpressionError> {
        let value_bytes = self.value_bytes.saturating_add(value.memory_size());
        // Vec growth doubles a full allocation, with a minimum non-zero
        // capacity of four for ExprValue-sized elements.
        let projected_capacity = if self.values.len() == self.values.capacity() {
            self.values.capacity().saturating_mul(2).max(4)
        } else {
            self.values.capacity()
        };
        let slack = projected_capacity.saturating_sub(self.values.len() + 1);
        let projected_bytes =
            value_bytes.saturating_add(slack.saturating_mul(size_of::<ExprValue>()));
        ctx.check_memory(projected_bytes)?;

        self.values.push(value);
        self.value_bytes = value_bytes;
        Ok(())
    }

    /// Consume the wrapper and return the collected values.
    pub(crate) fn into_vec(self) -> Vec<ExprValue> {
        self.values
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path_mapping::PathFormat;
    use std::cell::RefCell;

    #[derive(Default)]
    struct TestContext {
        checks: RefCell<Vec<usize>>,
    }

    impl EvalContext for TestContext {
        fn path_format(&self) -> PathFormat {
            PathFormat::host()
        }

        fn count_op(&mut self) -> Result<(), ExpressionError> {
            Ok(())
        }

        fn count_ops(&mut self, _n: usize) -> Result<(), ExpressionError> {
            Ok(())
        }

        fn count_string_ops(&mut self, _len: usize) -> Result<(), ExpressionError> {
            Ok(())
        }

        fn check_memory(&self, bytes: usize) -> Result<(), ExpressionError> {
            self.checks.borrow_mut().push(bytes);
            Ok(())
        }
    }

    #[test]
    fn checks_exact_capacity_before_allocation() {
        let mut ctx = TestContext::default();
        let values = BudgetedVec::with_capacity(&mut ctx, 3).unwrap();

        assert_eq!(values.values.capacity(), 3);
        assert_eq!(*ctx.checks.borrow(), vec![3 * size_of::<ExprValue>()]);
    }

    #[test]
    fn push_checks_projected_doubling_and_value_heap() {
        let mut ctx = TestContext::default();
        let mut values = BudgetedVec::new();
        values
            .push(&mut ctx, ExprValue::String("abc".to_owned()))
            .unwrap();

        assert_eq!(*ctx.checks.borrow(), vec![4 * size_of::<ExprValue>() + 3]);
    }

    #[test]
    fn push_checks_doubled_capacity_before_fifth_value() {
        let mut ctx = TestContext::default();
        let mut values = BudgetedVec::new();
        for value in 0..5 {
            values.push(&mut ctx, ExprValue::Int(value)).unwrap();
        }

        assert_eq!(
            *ctx.checks.borrow(),
            vec![
                4 * size_of::<ExprValue>(),
                4 * size_of::<ExprValue>(),
                4 * size_of::<ExprValue>(),
                4 * size_of::<ExprValue>(),
                8 * size_of::<ExprValue>(),
            ]
        );
    }
}
