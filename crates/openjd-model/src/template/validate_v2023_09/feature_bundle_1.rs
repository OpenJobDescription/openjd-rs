// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Pass 4: FEATURE_BUNDLE_1 — validate or reject.

use crate::error::{path_field, path_index, PathElement, ValidationErrors};
use crate::template::*;
use crate::types::{KnownExtension, ValidationContext};

pub fn validate_feature_bundle_1(
    jt: &JobTemplate,
    ctx: &ValidationContext,
    errors: &mut ValidationErrors,
) {
    let active = ctx.has_extension(KnownExtension::FeatureBundle1);

    for (i, step) in jt.steps.iter().enumerate() {
        let step_path = vec![PathElement::Field("steps".into()), PathElement::Index(i)];

        // SimpleAction fields
        let sa_fields: &[(&str, &Option<SimpleAction>)] = &[
            ("bash", &step.bash),
            ("python", &step.python),
            ("cmd", &step.cmd),
            ("powershell", &step.powershell),
            ("node", &step.node),
        ];
        let sa_count = sa_fields.iter().filter(|(_, f)| f.is_some()).count();
        if sa_count > 1 {
            errors.add(&step_path, "cannot have more than one simple action field.");
        }
        for &(sa_name, sa_field) in sa_fields {
            if sa_field.is_some() {
                let sa_path = path_field(&step_path, sa_name);
                if !active {
                    errors.add(
                        &sa_path,
                        format!("'{sa_name}' requires the FEATURE_BUNDLE_1 extension."),
                    );
                } else if step.script.is_some() {
                    errors.add(
                        &sa_path,
                        format!("cannot have both '{sa_name}' and 'script'."),
                    );
                }
            }
        }

        // endOfLine on embedded files
        if let Some(script) = &step.script {
            if let Some(files) = &script.embedded_files {
                let files_path = path_field(&path_field(&step_path, "script"), "embeddedFiles");
                for (j, f) in files.iter().enumerate() {
                    if let Some(_eol) = &f.end_of_line {
                        let eol_path = path_field(&path_index(&files_path, j), "endOfLine");
                        if !active {
                            errors.add(&eol_path, "requires the FEATURE_BUNDLE_1 extension.");
                        }
                    }
                }
            }
        }
    }

    // endOfLine in environment embedded files
    check_env_embedded_eol(
        &jt.job_environments,
        &[PathElement::Field("jobEnvironments".into())],
        active,
        errors,
    );
    for (i, step) in jt.steps.iter().enumerate() {
        let envs_path = vec![
            PathElement::Field("steps".into()),
            PathElement::Index(i),
            PathElement::Field("stepEnvironments".into()),
        ];
        check_env_embedded_eol(&step.step_environments, &envs_path, active, errors);
    }
}

fn check_env_embedded_eol(
    envs: &Option<Vec<Environment>>,
    base_path: &[PathElement],
    active: bool,
    errors: &mut ValidationErrors,
) {
    if let Some(envs) = envs {
        for (i, env) in envs.iter().enumerate() {
            if let Some(script) = &env.script {
                if let Some(files) = &script.embedded_files {
                    let files_path = path_field(
                        &path_field(&path_index(base_path, i), "script"),
                        "embeddedFiles",
                    );
                    for (j, f) in files.iter().enumerate() {
                        if let Some(_eol) = &f.end_of_line {
                            let eol_path = path_field(&path_index(&files_path, j), "endOfLine");
                            if !active {
                                errors.add(&eol_path, "requires the FEATURE_BUNDLE_1 extension.");
                            }
                        }
                    }
                }
            }
        }
    }
}
