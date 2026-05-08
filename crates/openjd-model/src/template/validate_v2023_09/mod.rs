// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

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
use crate::types::{JobParameterType, ModelExtension, TaskParameterType, ValidationContext};

/// Numeric limits computed from revision + extensions.
#[derive(Debug, Clone)]
pub struct EffectiveLimits {
    pub max_identifier_len: usize,
    pub max_job_name_len: usize,
    pub max_step_name_len: usize,
    pub max_env_name_len: usize,
    pub max_param_count: usize,
    /// Maximum parameter count in an *environment template*. This is held
    /// separately from `max_param_count` because environment templates
    /// are always capped at 50 in 2023-09, even when `FEATURE_BUNDLE_1`
    /// raises the job-template limit to 200.
    pub max_env_template_param_count: usize,
    pub max_filename_len: usize,
    pub max_task_param_range_len: usize,
    pub max_task_param_string_len: usize,
    pub max_job_param_string_len: usize,
    pub max_command_len: usize,
    pub max_description_len: usize,
}

// Note: there is no `Default` impl for `EffectiveLimits`. All call sites
// must go through `from_context(&ValidationContext)` so that the limits
// applied match the template's declared revision and extensions. A
// stand-alone "default" value would duplicate the revision-specific
// baseline below and could drift silently on the next revision bump.

impl EffectiveLimits {
    pub fn from_context(ctx: &ValidationContext) -> Self {
        // Dispatch on the revision first so that a future revision can
        // change baseline limits — or the set of extensions that affect
        // them — without reshaping this function. Today there is only one
        // revision; the match records intent and localizes where the first
        // revision bump needs to plug in.
        match ctx.profile.revision() {
            crate::types::SpecificationRevision::V2023_09 => Self::from_context_v2023_09(ctx),
        }
    }

    fn from_context_v2023_09(ctx: &ValidationContext) -> Self {
        let fb1 = ctx.profile.has_extension(ModelExtension::FeatureBundle1);
        Self {
            max_identifier_len: if fb1 { 512 } else { 64 },
            max_job_name_len: if fb1 { 512 } else { 128 },
            max_step_name_len: if fb1 { 512 } else { 64 },
            max_env_name_len: if fb1 { 512 } else { 64 },
            max_param_count: if fb1 { 200 } else { 50 },
            // Environment-template param count is NOT raised by FB1 in 2023-09.
            max_env_template_param_count: 50,
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
        let expr = ctx.profile.has_extension(ModelExtension::Expr);
        let fb1 = ctx.profile.has_extension(ModelExtension::FeatureBundle1);
        let chunking = ctx.profile.has_extension(ModelExtension::TaskChunking);

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
    structure::validate_structure(jt, &limits, &rules, ctx, &mut errors);

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

    // Parameter definitions — environment templates have their own cap,
    // held in `limits.max_env_template_param_count` so the rule is
    // revision-aware (and does NOT scale with FEATURE_BUNDLE_1 in 2023-09).
    if let Some(params) = &et.parameter_definitions {
        if params.is_empty() {
            errors.add(
                &[],
                "parameterDefinitions, if provided, must contain at least one element.",
            );
        }
        if params.len() > limits.max_env_template_param_count {
            errors.add(
                &[],
                format!(
                    "parameterDefinitions must not contain more than {} elements.",
                    limits.max_env_template_param_count
                ),
            );
        }
        let mut param_names = std::collections::HashSet::new();
        for p in params {
            if !param_names.insert(p.name().to_string()) {
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
