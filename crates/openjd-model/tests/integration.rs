// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Consolidated integration-test binary.
//!
//! Each `tests/integration/test_*.rs` file is included as a module here so
//! cargo links one test executable for this crate instead of one per file.

#[path = "integration/test_actions_and_steps.rs"]
mod test_actions_and_steps;
#[path = "integration/test_caller_limits.rs"]
mod test_caller_limits;
#[path = "integration/test_capabilities.rs"]
mod test_capabilities;
#[path = "integration/test_chunk_int.rs"]
mod test_chunk_int;
#[path = "integration/test_combination_expr.rs"]
mod test_combination_expr;
#[path = "integration/test_create_job.rs"]
mod test_create_job;
#[path = "integration/test_embedded.rs"]
mod test_embedded;
#[path = "integration/test_environment_template.rs"]
mod test_environment_template;
#[path = "integration/test_error_messages.rs"]
mod test_error_messages;
#[path = "integration/test_expr_parameters.rs"]
mod test_expr_parameters;
#[path = "integration/test_feature_bundle_1.rs"]
mod test_feature_bundle_1;
#[path = "integration/test_host_requirements.rs"]
mod test_host_requirements;
#[path = "integration/test_job_parameters.rs"]
mod test_job_parameters;
#[path = "integration/test_job_template.rs"]
mod test_job_template;
#[path = "integration/test_let_bindings.rs"]
mod test_let_bindings;
#[path = "integration/test_merge_job_parameters.rs"]
mod test_merge_job_parameters;
#[path = "integration/test_misc_v2023_09.rs"]
mod test_misc_v2023_09;
#[path = "integration/test_model_profile.rs"]
mod test_model_profile;
#[path = "integration/test_parameter_space.rs"]
mod test_parameter_space;
#[path = "integration/test_parse.rs"]
mod test_parse;
#[path = "integration/test_path_param_scope.rs"]
mod test_path_param_scope;
#[path = "integration/test_range_expr.rs"]
mod test_range_expr;
#[path = "integration/test_redacted_env_vars.rs"]
mod test_redacted_env_vars;
#[path = "integration/test_resolved_bindings.rs"]
mod test_resolved_bindings;
#[path = "integration/test_scope_library_split.rs"]
mod test_scope_library_split;
#[path = "integration/test_simple_action_let.rs"]
mod test_simple_action_let;
#[path = "integration/test_step_dependency_graph.rs"]
mod test_step_dependency_graph;
#[path = "integration/test_step_param_space_iter.rs"]
mod test_step_param_space_iter;
#[path = "integration/test_template_posix_paths.rs"]
mod test_template_posix_paths;
#[path = "integration/test_template_public_api.rs"]
mod test_template_public_api;
#[path = "integration/test_template_variables.rs"]
mod test_template_variables;
#[path = "integration/test_template_windows_paths.rs"]
mod test_template_windows_paths;
#[path = "integration/test_wrap_actions.rs"]
mod test_wrap_actions;
