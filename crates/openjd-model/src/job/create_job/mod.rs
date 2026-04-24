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

use crate::error::ModelError;
use crate::job;
use crate::template::validate_v2023_09::EffectiveLimits;
use crate::template::JobTemplate;
use crate::types::{CallerLimits, JobParameterValues, SpecificationRevision, ValidationContext};

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
///
/// When `caller_limits.max_task_count` is set, the total task count across all steps
/// is checked after parameter spaces are resolved.
pub fn create_job(
    job_template: &JobTemplate,
    job_parameter_values: &JobParameterValues,
    caller_limits: &CallerLimits,
) -> Result<job::Job, ModelError> {
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
        .resolve_string_with(
            &symtab,
            &openjd_expr::FormatStringOptions::new().with_path_format(PathFormat::Posix),
        )
        .map_err(|e| ModelError::FormatStringError {
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

    // Caller-imposed total task count limit across all steps
    if let Some(max_task_count) = caller_limits.max_task_count {
        let mut total: u64 = 0;
        for step in &steps {
            let step_tasks = step
                .parameter_space
                .as_ref()
                .map(|ps| {
                    crate::job::step_param_space::StepParameterSpaceIterator::new_with_chunk_override(ps, Some(1))
                        .map(|iter| iter.len() as u64)
                })
                .transpose()?
                .unwrap_or(1); // Steps without a parameter space have 1 task
            total = total.saturating_add(step_tasks);
        }
        if total > max_task_count {
            return Err(ModelError::ModelValidation(format!(
                "Total task count ({total}) exceeds caller limit of {max_task_count}."
            )));
        }
    }

    let job_environments = job_template.job_environments.as_ref().map(|envs| {
        envs.iter()
            .map(|e| instantiate::convert_environment_with_symtab(e, Some(&symtab)))
            .collect()
    });

    let extensions = job_template
        .extensions
        .as_ref()
        .map(|exts| exts.iter().map(|e| e.as_str().to_string()).collect());

    // Caller-imposed step script size limit (JSON-encoded bytes)
    if let Some(max) = caller_limits.max_step_script_size {
        for step in &steps {
            let size = serde_json::to_string(&step.script)
                .map(|s| s.len())
                .unwrap_or(0);
            if size > max {
                return Err(ModelError::ModelValidation(format!(
                    "Step '{}' script size ({size} bytes) exceeds caller limit of {max} bytes.",
                    step.name
                )));
            }
        }
    }

    // Caller-imposed environment size limit (JSON-encoded bytes)
    if let Some(max) = caller_limits.max_environment_size {
        let all_envs = steps
            .iter()
            .flat_map(|s| s.step_environments.iter().flatten())
            .chain(job_environments.iter().flatten());
        for env in all_envs {
            let size = serde_json::to_string(env).map(|s| s.len()).unwrap_or(0);
            if size > max {
                return Err(ModelError::ModelValidation(format!(
                    "Environment '{}' size ({size} bytes) exceeds caller limit of {max} bytes.",
                    env.name
                )));
            }
        }
    }

    Ok(job::Job {
        name: job_name,
        description: job_template.description.as_ref().map(|d| d.0.clone()),
        extensions,
        parameters,
        steps,
        job_environments,
    })
}
