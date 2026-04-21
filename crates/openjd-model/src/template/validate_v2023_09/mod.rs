// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Template validation pipeline.
//!
//! Validates templates through multiple passes (passes 5–9 of the decode pipeline):
//! - Pass 5: Enforce limits (EffectiveLimits)
//! - Pass 6: Structural validation (EffectiveRules)
//! - Pass 7: FEATURE_BUNDLE_1 (validate or reject)
//! - Pass 8: Format strings (base or EXPR profile)
//! - Pass 9: TASK_CHUNKING (validate or reject)

mod feature_bundle_1;
mod format_strings;
pub(crate) mod helpers;
mod limits;
mod structure;
mod task_chunking;

use crate::error::{ModelError, ValidationErrors};
use crate::template::*;
use crate::types::{JobParameterType, KnownExtension, TaskParameterType, ValidationContext};

/// Numeric limits computed from revision + extensions.
#[derive(Debug, Clone)]
pub struct EffectiveLimits {
    pub max_identifier_len: usize,
    pub max_job_name_len: usize,
    pub max_step_name_len: usize,
    pub max_env_name_len: usize,
    pub max_param_count: usize,
    pub max_filename_len: usize,
    pub max_task_param_range_len: usize,
    pub max_task_param_string_len: usize,
    pub max_job_param_string_len: usize,
    pub max_command_len: usize,
    pub max_description_len: usize,
}

impl Default for EffectiveLimits {
    fn default() -> Self {
        Self {
            max_identifier_len: 64,
            max_job_name_len: 128,
            max_step_name_len: 64,
            max_env_name_len: 64,
            max_param_count: 50,
            max_filename_len: 64,
            max_task_param_range_len: 1024,
            max_task_param_string_len: 1024,
            max_job_param_string_len: 1024,
            max_command_len: 1024,
            max_description_len: 2048,
        }
    }
}

impl EffectiveLimits {
    pub fn from_context(ctx: &ValidationContext) -> Self {
        let fb1 = ctx.has_extension(KnownExtension::FeatureBundle1);
        Self {
            max_identifier_len: if fb1 { 512 } else { 64 },
            max_job_name_len: if fb1 { 512 } else { 128 },
            max_step_name_len: if fb1 { 512 } else { 64 },
            max_env_name_len: if fb1 { 512 } else { 64 },
            max_param_count: if fb1 { 200 } else { 50 },
            max_filename_len: if fb1 { 256 } else { 64 },
            max_task_param_range_len: 1024,
            max_task_param_string_len: 1024,
            max_job_param_string_len: 1024,
            max_command_len: 1024,
            max_description_len: 2048,
        }
    }
}

/// Behavioral rules computed from revision + extensions.
#[derive(Debug, Clone)]
pub struct EffectiveRules {
    pub allowed_job_param_types: std::collections::HashSet<JobParameterType>,
    pub allowed_task_param_types: std::collections::HashSet<TaskParameterType>,
    pub allow_fmtstring_in_numeric_fields: bool,
}

impl EffectiveRules {
    pub fn from_context(ctx: &ValidationContext) -> Self {
        let expr = ctx.has_extension(KnownExtension::Expr);
        let fb1 = ctx.has_extension(KnownExtension::FeatureBundle1);
        let chunking = ctx.has_extension(KnownExtension::TaskChunking);

        let mut job_param_types: std::collections::HashSet<JobParameterType> = [
            JobParameterType::String,
            JobParameterType::Int,
            JobParameterType::Float,
            JobParameterType::Path,
        ]
        .into_iter()
        .collect();
        if expr {
            job_param_types.extend([
                JobParameterType::Bool,
                JobParameterType::RangeExpr,
                JobParameterType::ListString,
                JobParameterType::ListInt,
                JobParameterType::ListFloat,
                JobParameterType::ListPath,
                JobParameterType::ListBool,
                JobParameterType::ListListInt,
            ]);
        }

        let mut task_param_types: std::collections::HashSet<TaskParameterType> = [
            TaskParameterType::Int,
            TaskParameterType::Float,
            TaskParameterType::String,
            TaskParameterType::Path,
        ]
        .into_iter()
        .collect();
        if chunking {
            task_param_types.insert(TaskParameterType::ChunkInt);
        }

        Self {
            allowed_job_param_types: job_param_types,
            allowed_task_param_types: task_param_types,
            allow_fmtstring_in_numeric_fields: fb1,
        }
    }
}

/// Validate a job template through all passes.
pub(crate) fn validate_job_template(
    jt: &JobTemplate,
    ctx: &ValidationContext,
) -> Result<(), ModelError> {
    let limits = EffectiveLimits::from_context(ctx);
    let rules = EffectiveRules::from_context(ctx);
    let mut errors = ValidationErrors::default();

    // Pass 5: Enforce limits
    limits::enforce_limits(jt, &limits, &mut errors);

    // Pass 6: Structural validation
    structure::validate_structure(jt, &limits, &rules, &mut errors);

    // Pass 7: FEATURE_BUNDLE_1 (validate or reject)
    feature_bundle_1::validate_feature_bundle_1(jt, ctx, &mut errors);

    // Pass 8: Format strings (base or EXPR profile)
    format_strings::validate_format_strings(jt, ctx, &mut errors);

    // Pass 9: TASK_CHUNKING (validate or reject)
    task_chunking::validate_task_chunking(jt, ctx, &mut errors);

    errors.into_result("JobTemplate")
}

/// Validate an environment template through all passes.
pub fn validate_environment_template(
    et: &EnvironmentTemplate,
    ctx: &ValidationContext,
) -> Result<(), ModelError> {
    let limits = EffectiveLimits::from_context(ctx);
    let rules = EffectiveRules::from_context(ctx);
    let mut errors = ValidationErrors::default();

    // Parameter definitions
    if let Some(params) = &et.parameter_definitions {
        if params.is_empty() {
            errors.add(
                &[],
                "parameterDefinitions, if provided, must contain at least one element.",
            );
        }
        if params.len() > limits.max_param_count {
            errors.add(
                &[],
                format!(
                    "parameterDefinitions must not contain more than {} elements.",
                    limits.max_param_count
                ),
            );
        }
        let mut param_names = std::collections::HashSet::new();
        for p in params {
            if !param_names.insert(p.name().to_lowercase()) {
                errors.add(&[], format!("Duplicate parameter name: '{}'", p.name()));
            }
        }
    }

    // Validate the environment
    let env = &et.environment;
    let env_path = vec![crate::error::PathElement::Field("environment".into())];
    if env.script.is_none() && env.variables.is_none() {
        errors.add(
            &env_path,
            "must have at least one of 'script' or 'variables'.".to_string(),
        );
    }
    structure::validate_single_environment(env, &limits, &rules, &env_path, &mut errors);

    errors.into_result("EnvironmentTemplate")
}
