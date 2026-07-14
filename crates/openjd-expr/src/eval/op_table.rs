// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Operator-to-dunder dispatch table.
//!
//! Centralizes, for every Python AST operator (`ast::Operator`,
//! `ast::CmpOp`, `ast::UnaryOp`), either the dunder function name it
//! dispatches to in the `FunctionLibrary` (`__add__`, `__eq__`, `__neg__`,
//! …) or the [`SyntaxFeature`] gate and error message for operators outside
//! the expression language.
//!
//! This is the single place that answers "which operators are active and
//! what do they dispatch to?" — and it has two consumers:
//!
//! - The **parser** (`validate_structure` in `eval/parse.rs`) consults the
//!   `*_spec` lookups together with the profile: a `Gated` operator is
//!   rejected unless `ExprProfile::allows_syntax` grants its feature.
//! - The **evaluator** (`eval_binop`, `eval_unaryop`, `eval_compare`)
//!   consults the `Result`-returning lookups, which reject `Gated`
//!   operators unconditionally. The evaluator holds no profile; this is
//!   sound because under every current profile the parser gate has already
//!   rejected these operators, so the evaluator arms are a defense-in-depth
//!   backstop. When an extension first grants an operator feature, the
//!   evaluator additionally needs profile plumbing (e.g. a table derived
//!   from the `FunctionLibrary`) and a registered dunder for the operator.
//!
//! A future revision or extension that adds, removes, or remaps an operator
//! changes this table — both layers follow, and the parse-time and
//! eval-time error messages cannot drift apart. The matches inside the
//! lookup methods are exhaustive without wildcards, so a new operator
//! variant in a `ruff_python_ast` upgrade is a compile error here — one
//! file — instead of at every dispatch site.

use ruff_python_ast as ast;

use crate::error::ExpressionError;
use crate::profile::SyntaxFeature;

/// How a binary or unary operator is handled.
#[derive(Clone, Copy)]
pub(crate) enum OpSpec {
    /// Active operator: dispatches to this dunder name.
    Dunder(&'static str),
    /// Operator outside the baseline syntax: allowed only when a profile
    /// grants `feature`, rejected with `message` otherwise.
    Gated {
        feature: SyntaxFeature,
        message: &'static str,
    },
}

/// How a comparison operator dispatches.
#[derive(Clone, Copy)]
pub(crate) struct CmpDispatch {
    /// Dunder name registered in the library (`__eq__`, `__contains__`, …).
    pub(crate) dunder: &'static str,
    /// When true (`in` / `not in`), the container — the *right* operand —
    /// is passed as the first argument and the item as the second.
    pub(crate) container_first: bool,
}

/// How a comparison operator is handled.
#[derive(Clone, Copy)]
pub(crate) enum CmpOpSpec {
    /// Active operator with its dispatch info.
    Dispatch(CmpDispatch),
    /// Operator outside the baseline syntax: allowed only when a profile
    /// grants `feature`, rejected with `message` otherwise.
    Gated {
        feature: SyntaxFeature,
        message: &'static str,
    },
}

/// The operator dispatch table for the current expression language revision.
///
/// Today there is a single baseline table shared by all profiles. When a
/// revision or extension needs different operator behavior, construct the
/// table from the profile and store the differences as data here.
pub(crate) struct OperatorTable;

impl OperatorTable {
    /// The table for the current (only) revision.
    pub(crate) fn current() -> &'static OperatorTable {
        static TABLE: OperatorTable = OperatorTable;
        &TABLE
    }

    /// Spec for a binary operator: its dunder, or its syntax gate.
    pub(crate) fn binop_spec(&self, op: ast::Operator) -> OpSpec {
        match op {
            ast::Operator::Add => OpSpec::Dunder("__add__"),
            ast::Operator::Sub => OpSpec::Dunder("__sub__"),
            ast::Operator::Mult => OpSpec::Dunder("__mul__"),
            ast::Operator::Div => OpSpec::Dunder("__truediv__"),
            ast::Operator::FloorDiv => OpSpec::Dunder("__floordiv__"),
            ast::Operator::Mod => OpSpec::Dunder("__mod__"),
            ast::Operator::Pow => OpSpec::Dunder("__pow__"),
            ast::Operator::BitAnd => OpSpec::Gated {
                feature: SyntaxFeature::BitwiseAnd,
                message: "Bitwise AND (&) is not supported",
            },
            ast::Operator::BitOr => OpSpec::Gated {
                feature: SyntaxFeature::BitwiseOr,
                message: "Bitwise OR (|) is not supported",
            },
            ast::Operator::BitXor => OpSpec::Gated {
                feature: SyntaxFeature::BitwiseXor,
                message: "Bitwise XOR (^) is not supported",
            },
            ast::Operator::LShift => OpSpec::Gated {
                feature: SyntaxFeature::LeftShift,
                message: "Left shift (<<) is not supported",
            },
            ast::Operator::RShift => OpSpec::Gated {
                feature: SyntaxFeature::RightShift,
                message: "Right shift (>>) is not supported",
            },
            ast::Operator::MatMult => OpSpec::Gated {
                feature: SyntaxFeature::MatMult,
                message: "Matrix multiply (@) is not supported",
            },
        }
    }

    /// Spec for a unary operator: its dunder, or its syntax gate.
    pub(crate) fn unaryop_spec(&self, op: ast::UnaryOp) -> OpSpec {
        match op {
            ast::UnaryOp::USub => OpSpec::Dunder("__neg__"),
            ast::UnaryOp::UAdd => OpSpec::Dunder("__pos__"),
            ast::UnaryOp::Not => OpSpec::Dunder("__not__"),
            ast::UnaryOp::Invert => OpSpec::Gated {
                feature: SyntaxFeature::BitwiseNot,
                message: "Bitwise NOT (~) is not supported",
            },
        }
    }

    /// Spec for a comparison operator: its dispatch info, or its syntax gate.
    pub(crate) fn cmpop_spec(&self, op: ast::CmpOp) -> CmpOpSpec {
        let dispatch = |dunder, container_first| {
            CmpOpSpec::Dispatch(CmpDispatch {
                dunder,
                container_first,
            })
        };
        match op {
            ast::CmpOp::Eq => dispatch("__eq__", false),
            ast::CmpOp::NotEq => dispatch("__ne__", false),
            ast::CmpOp::Lt => dispatch("__lt__", false),
            ast::CmpOp::LtE => dispatch("__le__", false),
            ast::CmpOp::Gt => dispatch("__gt__", false),
            ast::CmpOp::GtE => dispatch("__ge__", false),
            ast::CmpOp::In => dispatch("__contains__", true),
            ast::CmpOp::NotIn => dispatch("__not_contains__", true),
            ast::CmpOp::Is => CmpOpSpec::Gated {
                feature: SyntaxFeature::IsOperator,
                message: "'is' operator is not supported; use '=='",
            },
            ast::CmpOp::IsNot => CmpOpSpec::Gated {
                feature: SyntaxFeature::IsNotOperator,
                message: "'is not' operator is not supported; use '!='",
            },
        }
    }

    /// Dunder name for a binary operator, or an `unsupported` error for
    /// operators outside the expression language.
    ///
    /// Gated operators are rejected unconditionally — see the module doc
    /// for why the evaluator does not consult the profile.
    pub(crate) fn binop(&self, op: ast::Operator) -> Result<&'static str, ExpressionError> {
        match self.binop_spec(op) {
            OpSpec::Dunder(dunder) => Ok(dunder),
            OpSpec::Gated { message, .. } => Err(ExpressionError::unsupported(message)),
        }
    }

    /// Dunder name for a unary operator, or an `unsupported` error.
    ///
    /// Gated operators are rejected unconditionally — see the module doc
    /// for why the evaluator does not consult the profile.
    pub(crate) fn unaryop(&self, op: ast::UnaryOp) -> Result<&'static str, ExpressionError> {
        match self.unaryop_spec(op) {
            OpSpec::Dunder(dunder) => Ok(dunder),
            OpSpec::Gated { message, .. } => Err(ExpressionError::unsupported(message)),
        }
    }

    /// Dispatch info for a comparison operator, or an `unsupported` error.
    ///
    /// Gated operators are rejected unconditionally — see the module doc
    /// for why the evaluator does not consult the profile.
    pub(crate) fn cmpop(&self, op: ast::CmpOp) -> Result<CmpDispatch, ExpressionError> {
        match self.cmpop_spec(op) {
            CmpOpSpec::Dispatch(dispatch) => Ok(dispatch),
            CmpOpSpec::Gated { message, .. } => Err(ExpressionError::unsupported(message)),
        }
    }
}
