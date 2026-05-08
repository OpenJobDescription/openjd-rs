// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

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
use crate::types::{JobParameterValues, ValidationContext};

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
/// The `ctx` parameter carries the specification revision, enabled
/// extensions, and caller limits that apply to this job instance. It
/// is the caller's responsibility to pass a `ValidationContext`
/// compatible with the one the template was validated against —
/// typically the same one. Passing a context whose extensions differ
/// from the template's declared extensions allows application-level
/// policy (e.g. "strip EXPR even if the template requests it" or
/// "enforce stricter caller limits on this queue") to apply at
/// job-creation time as well as at decode time.
///
/// When `ctx.caller_limits.max_task_count` is set, the total task count
/// across all steps is checked after parameter spaces are resolved.
pub fn create_job(
    job_template: &JobTemplate,
    job_parameter_values: &JobParameterValues,
    ctx: &ValidationContext,
) -> Result<job::Job, ModelError> {
    // Validate parameter values against template constraints.
    let merged = parameters::merge_job_parameter_definitions(job_template, &[])?;
    for param in &merged {
        if let Some(jpv) = job_parameter_values.get(&param.name) {
            param.check_constraints(&jpv.value)?;
        }
    }

    let mut symtab = build_symbol_table(job_parameter_values)?;

    let has_expr = ctx
        .profile
        .has_extension(crate::types::ModelExtension::Expr);
    let limits = EffectiveLimits::from_context(ctx);

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

    if job_name.len() > limits.max_job_name_len {
        return Err(ModelError::DecodeValidation(format!(
            "Job name exceeds maximum length of {} characters (got {})",
            limits.max_job_name_len,
            job_name.len()
        )));
    }

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
        .map(|st| instantiate::instantiate_step(st, &symtab, has_expr, &limits, ctx))
        .collect::<Result<Vec<_>, _>>()?;

    // Caller-imposed total task count limit across all steps
    if let Some(max_task_count) = ctx.caller_limits.max_task_count {
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
            return Err(ModelError::ModelValidation(
                crate::error::ValidationErrors::single(format!(
                    "Total task count ({total}) exceeds caller limit of {max_task_count}."
                )),
            ));
        }
    }

    let job_environments = job_template.job_environments.as_ref().map(|envs| {
        envs.iter()
            .map(|e| instantiate::convert_environment_with_symtab(e, Some(&symtab)))
            .collect()
    });

    // job_template.extensions is `Option<Vec<ExtensionName>>` where every
    // entry has already passed decode-time recognition: any string that
    // isn't a valid ModelExtension would have been rejected in parse.rs.
    // Map into the typed Job.extensions form; in the unexpected case that
    // an entry doesn't parse (e.g. directly-constructed JobTemplate
    // bypassing decode), skip it — the decode path is the single source
    // of truth for extension recognition.
    let extensions = job_template.extensions.as_ref().map(|exts| {
        exts.iter()
            .filter_map(|e| std::str::FromStr::from_str(e.as_str()).ok())
            .collect()
    });

    // Caller-imposed step script size limit (JSON-encoded bytes)
    if let Some(max) = ctx.caller_limits.max_step_script_size {
        for step in &steps {
            let size = serde_json::to_string(&step.script)
                .map(|s| s.len())
                .unwrap_or(0);
            if size > max {
                return Err(ModelError::ModelValidation(
                    crate::error::ValidationErrors::single(format!(
                        "Step '{}' script size ({size} bytes) exceeds caller limit of {max} bytes.",
                        step.name
                    )),
                ));
            }
        }
    }

    // Caller-imposed environment size limit (JSON-encoded bytes)
    if let Some(max) = ctx.caller_limits.max_environment_size {
        let all_envs = steps
            .iter()
            .flat_map(|s| s.step_environments.iter().flatten())
            .chain(job_environments.iter().flatten());
        for env in all_envs {
            let size = serde_json::to_string(env).map(|s| s.len()).unwrap_or(0);
            if size > max {
                return Err(ModelError::ModelValidation(
                    crate::error::ValidationErrors::single(format!(
                        "Environment '{}' size ({size} bytes) exceeds caller limit of {max} bytes.",
                        env.name
                    )),
                ));
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
