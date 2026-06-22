// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Validate or reject `WRAP_ACTIONS` features (RFC 0008).
//!
//! Three fields are gated by the `WRAP_ACTIONS` extension:
//! - `onWrapEnvEnter` on `<EnvironmentActions>`
//! - `onWrapTaskRun` on `<EnvironmentActions>`
//! - `onWrapEnvExit` on `<EnvironmentActions>`
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
use crate::template::{Environment, EnvironmentTemplate, JobTemplate};
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
    let wrap_hooks = actions.wrap_hooks();
    for (name, slot, _) in wrap_hooks {
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
        let defined = wrap_hooks
            .iter()
            .filter(|(_, slot, _)| slot.is_some())
            .count();
        if defined > 0 && defined < wrap_hooks.len() {
            errors.add(
                actions_path,
                "an environment that defines any of onWrapEnvEnter, onWrapTaskRun, or onWrapEnvExit must define all three (RFC 0008).",
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
    script.actions.has_any_wrap_hook()
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

    // Single-wrap-layer enforcement (RFC 0008).
    //
    // The session model is: a session's environment stack is the job's
    // `jobEnvironments` plus exactly ONE step's `stepEnvironments`.
    // Different steps never share a session. So "only one wrap layer per
    // session" reduces to: for every step, (wrap envs in jobEnvironments)
    // + (wrap envs in that step's stepEnvironments) must be <= 1.
    //
    // `check_env` does double duty below: it gates the new fields on the
    // extension and returns true iff the env defines any wrap hook, which
    // is what we sum into the per-scope counts.

    // 1. Count wrap-defining envs in jobEnvironments (shared by every session).
    let mut job_env_wrap_count = 0usize;
    if let Some(envs) = &jt.job_environments {
        let envs_path = path_field(&[], "jobEnvironments");
        for (i, env) in envs.iter().enumerate() {
            if check_env(env, &path_index(&envs_path, i), active, errors) {
                job_env_wrap_count += 1;
            }
        }
    }

    // Job-envs-only violation: multiple wrap envs in jobEnvironments are
    // reachable from every session, regardless of any step's
    // stepEnvironments, so emit at the jobEnvironments path as soon as the
    // job-env count exceeds one. This is independent of the per-step check
    // below: a template with two job-env wrap layers AND a step that adds
    // its own should report both the jobEnvironments and stepEnvironments
    // violations.
    if active && job_env_wrap_count > 1 {
        errors.add(&path_field(&[], "jobEnvironments"), SINGLE_WRAP_LAYER_MSG);
    }

    // 2. For each step, count its stepEnvironments' wrap envs and add the
    //    job-env total — that sum is exactly the set of wrap envs reachable
    //    in that step's session.
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
        // This catches two job-env wrap layers, one job-env + one step-env,
        // and two step-env layers within the same step.
        if active && job_env_wrap_count + step_env_wrap_count > 1 {
            errors.add(&envs_path, SINGLE_WRAP_LAYER_MSG);
        }
    }
}

const SINGLE_WRAP_LAYER_MSG: &str =
    "only one environment in the session stack may define any of onWrapEnvEnter, onWrapTaskRun, onWrapEnvExit (RFC 0008).";

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
