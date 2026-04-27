// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Pass 5: Enforce limits.
//!
//! Walks the template and checks every name length, list count, and string length
//! against `EffectiveLimits`. No extension checks — just enforces computed limits.

use super::EffectiveLimits;
use crate::error::{path_field, path_index, PathElement, ValidationErrors};
use crate::template::*;
pub fn enforce_limits(jt: &JobTemplate, limits: &EffectiveLimits, errors: &mut ValidationErrors) {
    let root: Vec<PathElement> = vec![];

    // Job name
    let name = jt.name.raw();
    if name.len() > limits.max_job_name_len {
        errors.add(
            &path_field(&root, "name"),
            format!("exceeds {} characters.", limits.max_job_name_len),
        );
    }

    // Parameter definitions count
    if let Some(params) = &jt.parameter_definitions {
        if params.len() > limits.max_param_count {
            errors.add(
                &path_field(&root, "parameterDefinitions"),
                format!(
                    "must not contain more than {} elements.",
                    limits.max_param_count
                ),
            );
        }
        let pd_path = path_field(&root, "parameterDefinitions");
        for (i, p) in params.iter().enumerate() {
            let p_path = path_index(&pd_path, i);
            if p.name().len() > limits.max_identifier_len {
                errors.add(
                    &p_path,
                    format!("name exceeds {} characters.", limits.max_identifier_len),
                );
            }
        }
    }

    // Steps
    for (i, step) in jt.steps.iter().enumerate() {
        let step_path = vec![PathElement::Field("steps".into()), PathElement::Index(i)];
        let name = &step.name;
        if name.len() > limits.max_step_name_len {
            errors.add(
                &path_field(&step_path, "name"),
                format!("exceeds {} characters.", limits.max_step_name_len),
            );
        }

        // Step environments
        if let Some(envs) = &step.step_environments {
            let envs_path = path_field(&step_path, "stepEnvironments");
            for (j, env) in envs.iter().enumerate() {
                let env_path = path_index(&envs_path, j);
                enforce_environment_limits(env, &env_path, limits, errors);
            }
        }

        // Embedded files
        if let Some(script) = &step.script {
            if let Some(files) = &script.embedded_files {
                let files_path = path_field(&path_field(&step_path, "script"), "embeddedFiles");
                for (j, f) in files.iter().enumerate() {
                    let f_path = path_index(&files_path, j);
                    if f.name.len() > limits.max_identifier_len {
                        errors.add(
                            &path_field(&f_path, "name"),
                            format!("exceeds {} characters.", limits.max_identifier_len),
                        );
                    }
                    if let Some(filename) = &f.filename {
                        if filename.raw().len() > limits.max_filename_len {
                            errors.add(
                                &path_field(&f_path, "filename"),
                                format!("exceeds {} characters.", limits.max_filename_len),
                            );
                        }
                    }
                }
            }
        }

        // Task parameter names
        if let Some(ps) = &step.parameter_space {
            let ps_path = path_field(&step_path, "parameterSpace");
            let tpd_path = path_field(&ps_path, "taskParameterDefinitions");
            for (j, tp) in ps.task_parameter_definitions.iter().enumerate() {
                let tp_path = path_index(&tpd_path, j);
                if tp.name().len() > limits.max_identifier_len {
                    errors.add(
                        &tp_path,
                        format!("name exceeds {} characters.", limits.max_identifier_len),
                    );
                }
            }
        }
    }

    // Job environments
    if let Some(envs) = &jt.job_environments {
        let envs_path = path_field(&root, "jobEnvironments");
        for (i, env) in envs.iter().enumerate() {
            let env_path = path_index(&envs_path, i);
            enforce_environment_limits(env, &env_path, limits, errors);
        }
    }
}

fn enforce_environment_limits(
    env: &Environment,
    path: &[PathElement],
    limits: &EffectiveLimits,
    errors: &mut ValidationErrors,
) {
    if env.name.len() > limits.max_env_name_len {
        errors.add(
            &path_field(path, "name"),
            format!("exceeds {} characters.", limits.max_env_name_len),
        );
    }
    if let Some(script) = &env.script {
        if let Some(files) = &script.embedded_files {
            let files_path = path_field(&path_field(path, "script"), "embeddedFiles");
            for (j, f) in files.iter().enumerate() {
                let f_path = path_index(&files_path, j);
                if f.name.len() > limits.max_identifier_len {
                    errors.add(
                        &path_field(&f_path, "name"),
                        format!("exceeds {} characters.", limits.max_identifier_len),
                    );
                }
                if let Some(filename) = &f.filename {
                    if filename.raw().len() > limits.max_filename_len {
                        errors.add(
                            &path_field(&f_path, "filename"),
                            format!("exceeds {} characters.", limits.max_filename_len),
                        );
                    }
                }
            }
        }
    }
}
