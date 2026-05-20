// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Validate or reject `WRAP_ACTIONS` features (RFC 0008).
//!
//! Three fields are gated by the `WRAP_ACTIONS` extension:
//! - `onWrapEnter` on `<EnvironmentActions>`
//! - `onWrapTaskRun` on `<EnvironmentActions>`
//! - `onWrapExit` on `<EnvironmentActions>`
//!
//! When the extension is not enabled, using any of these fields is a
//! validation error. When it is enabled, we additionally enforce the
//! constraints from RFC 0008:
//!
//! - **All-or-nothing.** An environment that defines any of the three
//!   wrap hooks must define all three.
//! - **Single-layer.** At most one environment in the session stack
//!   (job environments + each step's step environments) may define any
//!   wrap hook.
//! - **EXPR prerequisite.** A template that lists `WRAP_ACTIONS` in
//!   `extensions:` must also list `EXPR`.

use crate::error::{path_field, path_index, PathElement, ValidationErrors};
use crate::template::actions::EnvironmentActions;
use crate::template::{Action, Environment, EnvironmentTemplate, JobTemplate};
use crate::types::{ModelExtension, ValidationContext};

/// Check one environment's `<EnvironmentActions>` for wrap hook usage.
///
/// Reports every offending field individually so users see a complete list
/// rather than having to fix them one at a time. Also enforces the
/// all-or-nothing rule when the extension is enabled.
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

    // All-or-nothing rule (RFC 0008): an env that defines any wrap hook
    // must define all three. Only enforced when the extension is active —
    // otherwise the per-hook errors above already cover it.
    if active {
        let defined: Vec<&str> = wrap_hooks
            .iter()
            .filter_map(|(name, slot)| slot.as_ref().map(|_| *name))
            .collect();
        if !defined.is_empty() && defined.len() < 3 {
            errors.add(
                actions_path,
                "an environment that defines any of onWrapEnter, onWrapTaskRun, or onWrapExit must define all three (RFC 0008).",
            );
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
    script.actions.on_wrap_enter.is_some()
        || script.actions.on_wrap_task_run.is_some()
        || script.actions.on_wrap_exit.is_some()
}

/// Enforce the EXPR prerequisite: when `WRAP_ACTIONS` is listed in a
/// template's `extensions:`, `EXPR` must also be listed (RFC 0008).
fn check_expr_prerequisite(ctx: &ValidationContext, errors: &mut ValidationErrors) {
    let has_wrap = ctx.profile.has_extension(ModelExtension::WrapActions);
    let has_expr = ctx.profile.has_extension(ModelExtension::Expr);
    if has_wrap && !has_expr {
        errors.add(
            &path_field(&[], "extensions"),
            "WRAP_ACTIONS requires EXPR; both must be listed in the template's `extensions` (RFC 0008).",
        );
    }
}

/// Validate RFC 0008 constraints for a job template.
///
/// This runs regardless of whether `WRAP_ACTIONS` is enabled:
/// - When disabled, it rejects templates that attempt to use any of the
///   new fields.
/// - When enabled, it additionally enforces the EXPR prerequisite, the
///   all-or-nothing rule, and the single-wrap-layer rule per session.
pub fn validate_wrap_actions_job_template(
    jt: &JobTemplate,
    ctx: &ValidationContext,
    errors: &mut ValidationErrors,
) {
    let active = ctx.profile.has_extension(ModelExtension::WrapActions);
    check_expr_prerequisite(ctx, errors);

    // Walk every environment, gating new fields and counting wrap layers.
    let mut job_env_wrap_count = 0usize;
    if let Some(envs) = &jt.job_environments {
        let envs_path = path_field(&[], "jobEnvironments");
        for (i, env) in envs.iter().enumerate() {
            if check_env(env, &path_index(&envs_path, i), active, errors) {
                job_env_wrap_count += 1;
            }
        }
    }
    let mut any_step_env_violation = false;
    for (i, step) in jt.steps.iter().enumerate() {
        let Some(envs) = &step.step_environments else {
            continue;
        };
        let base = path_index(&path_field(&[], "steps"), i);
        let envs_path = path_field(&base, "stepEnvironments");
        let mut step_env_wrap_count = 0usize;
        for (j, env) in envs.iter().enumerate() {
            if check_env(env, &path_index(&envs_path, j), active, errors) {
                step_env_wrap_count += 1;
            }
        }
        // Single-wrap-layer rule: a session is built from the job's
        // jobEnvironments plus one step's stepEnvironments, so checking
        // each step's combined total catches every reachable session.
        if active && job_env_wrap_count + step_env_wrap_count > 1 {
            errors.add(&envs_path, SINGLE_WRAP_LAYER_MSG);
            any_step_env_violation = true;
        }
    }
    // Job-envs-only violation: multiple wrap envs in jobEnvironments
    // would be reachable from every session, but the per-step loop above
    // only fires for steps that have stepEnvironments. Emit once at the
    // jobEnvironments path when no step-env loop would have caught it.
    if active && job_env_wrap_count > 1 && !any_step_env_violation {
        errors.add(&path_field(&[], "jobEnvironments"), SINGLE_WRAP_LAYER_MSG);
    }
}

const SINGLE_WRAP_LAYER_MSG: &str =
    "only one environment in the session stack may define any of onWrapEnter, onWrapTaskRun, onWrapExit (RFC 0008).";

/// Validate RFC 0008 constraints for an environment template.
///
/// An environment template defines one environment, so the single-layer
/// rule is trivially satisfied. We gate the new fields on the extension
/// being enabled, enforce the EXPR prerequisite, and enforce the
/// all-or-nothing rule via `check_env`.
pub fn validate_wrap_actions_environment_template(
    et: &EnvironmentTemplate,
    ctx: &ValidationContext,
    errors: &mut ValidationErrors,
) {
    let active = ctx.profile.has_extension(ModelExtension::WrapActions);
    check_expr_prerequisite(ctx, errors);
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
