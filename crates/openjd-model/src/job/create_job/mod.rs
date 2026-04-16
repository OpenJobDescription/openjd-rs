// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Job creation: parameter preprocessing and template instantiation.
//!
//! Mirrors Python `_create_job.py` and `_merge_job_parameter.py`.

mod instantiate;
pub mod parameters;
mod ranges;

use indexmap::IndexMap;

use openjd_expr::path_mapping::PathFormat;

use crate::error::OpenJdError;
use crate::job;
use crate::template::validate_v2023_09::EffectiveLimits;
use crate::template::JobTemplate;
use crate::types::{JobParameterValues, SpecificationRevision, ValidationContext};

// Re-exports — preserve the existing public API
pub use instantiate::{
    convert_environment, convert_environment_with_symtab, evaluate_let_bindings,
};
pub use parameters::{
    build_symbol_table, merge_job_parameter_definitions, preprocess_job_parameters,
    MergedParameterDefinition, PathParameterOptions,
};

/// Create an instantiated Job from a validated JobTemplate and preprocessed parameter values.
///
/// Environment template parameters should already be merged into `job_parameter_values`
/// via [`preprocess_job_parameters`] before calling this function.
pub fn create_job(
    job_template: &JobTemplate,
    job_parameter_values: &JobParameterValues,
) -> Result<job::Job, OpenJdError> {
    let mut symtab = build_symbol_table(job_parameter_values)?;

    let has_expr = job_template
        .extensions
        .as_ref()
        .map(|exts| exts.iter().any(|e| e.as_str() == "EXPR"))
        .unwrap_or(false);

    let ctx = ValidationContext::with_extensions(
        SpecificationRevision::V2023_09,
        job_template
            .extensions
            .as_ref()
            .map(|exts| {
                exts.iter()
                    .filter_map(|e| e.as_str().parse::<crate::types::KnownExtension>().ok())
                    .collect()
            })
            .unwrap_or_default(),
    );
    let limits = EffectiveLimits::from_context(&ctx);

    let job_name = job_template
        .name
        .resolve_string_with_format(&symtab, None, &[], PathFormat::Posix)
        .map_err(|e| OpenJdError::FormatStringError {
            message: format!("Failed to resolve job name: {e}"),
            input: Some(job_template.name.raw().to_string()),
            start: None,
            end: None,
        })?;

    if has_expr {
        symtab.set("Job.Name", openjd_expr::ExprValue::String(job_name.clone()))?;
    }

    let parameters: IndexMap<String, job::JobParameter> = job_parameter_values
        .iter()
        .map(|(name, pv)| {
            (
                name.clone(),
                job::JobParameter {
                    name: name.clone(),
                    param_type: pv.param_type,
                    value: pv.value.clone(),
                },
            )
        })
        .collect();

    let steps = job_template
        .steps
        .iter()
        .map(|st| instantiate::instantiate_step(st, &symtab, has_expr, &limits))
        .collect::<Result<Vec<_>, _>>()?;

    let job_environments = job_template.job_environments.as_ref().map(|envs| {
        envs.iter()
            .map(|e| instantiate::convert_environment_with_symtab(e, Some(&symtab)))
            .collect()
    });

    let extensions = job_template
        .extensions
        .as_ref()
        .map(|exts| exts.iter().map(|e| e.as_str().to_string()).collect());

    Ok(job::Job {
        name: job_name,
        description: job_template.description.as_ref().map(|d| d.0.clone()),
        extensions,
        parameters,
        steps,
        job_environments,
    })
}
