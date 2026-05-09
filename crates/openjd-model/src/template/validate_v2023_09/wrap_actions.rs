// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Validate or reject `WRAP_ACTIONS` features (RFC 0008).
//!
//! Three fields are gated by the `WRAP_ACTIONS` extension:
//! - `onWrapEnter` on `<EnvironmentActions>`
//! - `onWrapTaskRun` on `<EnvironmentActions>`
//! - `onWrapExit` on `<EnvironmentActions>`
//! - `runOnHost` on `<Action>` (any action, including task `onRun`)
//!
//! When the extension is not enabled, using any of these fields is a
//! validation error. When it is enabled, we additionally enforce the
//! single-wrap-layer rule from RFC 0008: at most one environment in the
//! session stack (job environments + each step's step environments) may
//! define any wrap hook.

use crate::error::{path_field, path_index, PathElement, ValidationErrors};
use crate::template::actions::EnvironmentActions;
use crate::template::{Action, Environment, EnvironmentTemplate, JobTemplate};
use crate::types::{ModelExtension, ValidationContext};

/// Check a single `Action` for `runOnHost` usage. The caller provides the
/// path down to the action's own field (e.g. `steps[0] -> script -> actions -> onRun`),
/// and we append `runOnHost` ourselves.
fn check_action_run_on_host(
    action: &Action,
    action_path: &[PathElement],
    active: bool,
    errors: &mut ValidationErrors,
) {
    if action.run_on_host.is_some() && !active {
        let p = path_field(action_path, "runOnHost");
        errors.add(&p, "runOnHost requires the WRAP_ACTIONS extension.");
    }
}

/// Check one environment's `<EnvironmentActions>` for wrap hook usage.
///
/// Reports every offending field individually so users see a complete list
/// rather than having to fix them one at a time.
fn check_environment_actions(
    actions: &EnvironmentActions,
    actions_path: &[PathElement],
    active: bool,
    errors: &mut ValidationErrors,
) {
    let wrap_hooks: [(&str, &Option<Action>); 3] = [
        ("onWrapEnter", &actions.on_wrap_enter),
        ("onWrapTaskRun", &actions.on_wrap_task_run),
        ("onWrapExit", &actions.on_wrap_exit),
    ];
    for (name, slot) in wrap_hooks {
        if slot.is_some() && !active {
            errors.add(
                &path_field(actions_path, name),
                format!("{name} requires the WRAP_ACTIONS extension."),
            );
        }
    }
}

/// Check every action under an environment for `runOnHost` usage.
fn check_environment_actions_run_on_host(
    actions: &EnvironmentActions,
    actions_path: &[PathElement],
    active: bool,
    errors: &mut ValidationErrors,
) {
    let every: [(&str, &Option<Action>); 5] = [
        ("onEnter", &actions.on_enter),
        ("onWrapEnter", &actions.on_wrap_enter),
        ("onWrapTaskRun", &actions.on_wrap_task_run),
        ("onWrapExit", &actions.on_wrap_exit),
        ("onExit", &actions.on_exit),
    ];
    for (name, slot) in every {
        if let Some(action) = slot {
            check_action_run_on_host(action, &path_field(actions_path, name), active, errors);
        }
    }
}

/// Walk one environment for WRAP_ACTIONS gating and return whether it
/// defined any wrap hook (used for the single-layer check upstream).
fn check_env(
    env: &Environment,
    path: &[PathElement],
    active: bool,
    errors: &mut ValidationErrors,
) -> bool {
    let Some(script) = &env.script else {
        return false;
    };
    let script_path = path_field(path, "script");
    let actions_path = path_field(&script_path, "actions");
    check_environment_actions(&script.actions, &actions_path, active, errors);
    check_environment_actions_run_on_host(&script.actions, &actions_path, active, errors);
    script.actions.on_wrap_enter.is_some()
        || script.actions.on_wrap_task_run.is_some()
        || script.actions.on_wrap_exit.is_some()
}

/// Validate RFC 0008 constraints for a job template.
///
/// This runs regardless of whether `WRAP_ACTIONS` is enabled:
/// - When disabled, it rejects templates that attempt to use any of the
///   new fields.
/// - When enabled, it additionally enforces the single-wrap-layer rule
///   per session: every (job-envs + one step's step-envs) combination
///   contains at most one environment with any wrap hook.
pub fn validate_wrap_actions_job_template(
    jt: &JobTemplate,
    ctx: &ValidationContext,
    errors: &mut ValidationErrors,
) {
    let active = ctx.profile.has_extension(ModelExtension::WrapActions);

    // Walk the step script's onRun too — runOnHost is valid there.
    for (i, step) in jt.steps.iter().enumerate() {
        let Some(script) = &step.script else {
            continue;
        };
        let step_path = path_index(&path_field(&[], "steps"), i);
        let script_path = path_field(&step_path, "script");
        let actions_path = path_field(&script_path, "actions");
        check_action_run_on_host(
            &script.actions.on_run,
            &path_field(&actions_path, "onRun"),
            active,
            errors,
        );
    }

    // Count job-envs with wrap hooks (for the single-layer rule).
    // Also record their indices for precise error paths.
    let mut job_env_wrap_indices: Vec<usize> = Vec::new();
    if let Some(envs) = &jt.job_environments {
        let envs_path = path_field(&[], "jobEnvironments");
        for (i, env) in envs.iter().enumerate() {
            let p = path_index(&envs_path, i);
            if check_env(env, &p, active, errors) {
                job_env_wrap_indices.push(i);
            }
        }
    }

    // Walk each step's step environments.
    for (i, step) in jt.steps.iter().enumerate() {
        if let Some(envs) = &step.step_environments {
            let base = path_index(&path_field(&[], "steps"), i);
            let envs_path = path_field(&base, "stepEnvironments");
            let mut step_env_wrap_indices: Vec<usize> = Vec::new();
            for (j, env) in envs.iter().enumerate() {
                let p = path_index(&envs_path, j);
                if check_env(env, &p, active, errors) {
                    step_env_wrap_indices.push(j);
                }
            }
            // Single-wrap-layer rule: across job-envs and this step's
            // step-envs, at most one environment may define any wrap hook.
            if active {
                let total = job_env_wrap_indices.len() + step_env_wrap_indices.len();
                if total > 1 {
                    errors.add(
                        &envs_path,
                        "only one environment in the session stack may define any of onWrapEnter, onWrapTaskRun, onWrapExit (RFC 0008).",
                    );
                }
            }
        } else if active && job_env_wrap_indices.len() > 1 {
            // Job-envs-only case: even with no step envs, two wrap layers
            // in job envs alone is still invalid. Reported once per step
            // walk to keep the error path at a step boundary; dedupe by
            // only emitting on the first step to avoid N copies.
            if i == 0 {
                errors.add(
                    &path_field(&[], "jobEnvironments"),
                    "only one environment in the session stack may define any of onWrapEnter, onWrapTaskRun, onWrapExit (RFC 0008).",
                );
            }
        }
    }

    // Degenerate case: multiple job-envs with wrap hooks but no steps would
    // have failed structural validation earlier (a job template needs at
    // least one step), so the above loop always emits the error if needed.
}

/// Validate RFC 0008 constraints for an environment template.
///
/// An environment template defines one environment, so the single-layer
/// rule is trivially satisfied. We only gate the new fields on the
/// extension being enabled.
pub fn validate_wrap_actions_environment_template(
    et: &EnvironmentTemplate,
    ctx: &ValidationContext,
    errors: &mut ValidationErrors,
) {
    let active = ctx.profile.has_extension(ModelExtension::WrapActions);
    let env_path = path_field(&[], "environment");
    check_env(&et.environment, &env_path, active, errors);
}

#[cfg(test)]
mod tests {
    //! Integration tests in `tests/integration/test_wrap_actions.rs`
    //! exercise the full decode + validate pipeline against real templates.
    //! No direct unit tests here: every helper is pure-data and already
    //! covered through the integration surface.
}
