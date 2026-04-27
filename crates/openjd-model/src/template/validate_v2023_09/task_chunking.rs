// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Pass 9: TASK_CHUNKING — validate or reject.

use crate::error::{path_field, path_index, PathElement, ValidationErrors};
use crate::template::*;
use crate::types::{KnownExtension, ValidationContext};

pub fn validate_task_chunking(
    jt: &JobTemplate,
    ctx: &ValidationContext,
    errors: &mut ValidationErrors,
) {
    let active = ctx.has_extension(KnownExtension::TaskChunking);

    for (i, step) in jt.steps.iter().enumerate() {
        let step_path = vec![PathElement::Field("steps".into()), PathElement::Index(i)];
        if let Some(ps) = &step.parameter_space {
            let ps_path = path_field(&step_path, "parameterSpace");
            let tpd_path = path_field(&ps_path, "taskParameterDefinitions");
            let mut chunk_count = 0;
            for (j, param) in ps.task_parameter_definitions.iter().enumerate() {
                if let TaskParameterDefinition::CHUNK_INT(cp) = param {
                    let p_path = path_index(&tpd_path, j);
                    if !active {
                        errors.add(&p_path, "CHUNK[INT] requires the TASK_CHUNKING extension.");
                    } else {
                        chunk_count += 1;
                        if let Some(n) = cp.chunks.default_task_count.as_i64() {
                            if n < 1 {
                                errors.add(
                                    &path_field(&p_path, "chunks"),
                                    "defaultTaskCount must be >= 1.",
                                );
                            }
                        }
                        if let Some(target) = &cp.chunks.target_runtime_seconds {
                            if let Some(n) = target.as_i64() {
                                if n < 0 {
                                    errors.add(
                                        &path_field(&p_path, "chunks"),
                                        "targetRuntimeSeconds must be >= 0.",
                                    );
                                }
                            }
                        }
                    }
                }
            }
            if active && chunk_count > 1 {
                errors.add(
                    &tpd_path,
                    "only one CHUNK[INT] parameter is allowed per step.",
                );
            }
            // Check chunked param not in associative combination
            if active && chunk_count > 0 {
                if let Some(comb) = &ps.combination {
                    for param in &ps.task_parameter_definitions {
                        if let TaskParameterDefinition::CHUNK_INT(cp) = param {
                            let chunk_name = cp.name.as_str();
                            // Check if chunk_name appears inside parentheses
                            let mut in_parens = false;
                            let mut depth = 0i32;
                            let chars: Vec<char> = comb.chars().collect();
                            let mut current = String::new();
                            for &ch in &chars {
                                if ch.is_alphanumeric() || ch == '_' {
                                    current.push(ch);
                                } else {
                                    if current == chunk_name && depth > 0 {
                                        in_parens = true;
                                    }
                                    current.clear();
                                    if ch == '(' {
                                        depth += 1;
                                    } else if ch == ')' {
                                        depth -= 1;
                                    }
                                }
                            }
                            if current == chunk_name && depth > 0 {
                                in_parens = true;
                            }
                            if in_parens {
                                errors.add(&path_field(&ps_path, "combination"),
                                    format!("CHUNK[INT] parameter '{}' must not be in an associative combination.", chunk_name));
                            }
                        }
                    }
                }
            }
        }
    }
}
