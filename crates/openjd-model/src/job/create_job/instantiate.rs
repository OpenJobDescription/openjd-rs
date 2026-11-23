// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Step and environment instantiation — converting template types to job types.

use openjd_expr::format_string::copy_symbol_value;
use openjd_expr::path_mapping::PathFormat;
use openjd_expr::symbol_table::SymbolTable;

use crate::error::{path_field, path_index, ModelError, PathElement, ValidationErrors};
use crate::job;
use crate::template;
use crate::template::validate_v2023_09::EffectiveLimits;
use openjd_expr::ExpressionError;

use super::{host_requirements, ranges};
/// Instantiate a StepTemplate into a Step.
///
/// The step's carried-forward resolved-value checks — and its `let`
/// bindings, step-level and script/environment-level — report into
/// `check_errors`, which `create_job` threads through every step and job
/// environment and converts into one `ModelValidation` error at the end
/// (whole-template aggregation, as pass 8 reports). Resolution failures
/// that leave the step uninstantiable — host requirements, the
/// parameter space — are returned as `Err` and abort job creation
/// immediately.
///
/// Returns `Ok(None)` when a step-level `let` binding failed: the
/// failed names are bound `Unresolved` so the step's carried-forward
/// checks still run (as at pass 8), but host requirements and the
/// parameter space need the concrete values, so they are skipped. The
/// failure is already recorded in `check_errors`, so job creation fails.
#[allow(clippy::too_many_arguments)]
pub(super) fn instantiate_step(
    st: &template::StepTemplate,
    symtab: &SymbolTable,
    has_expr: bool,
    template_expr: bool,
    limits: &EffectiveLimits,
    ctx: &crate::types::ValidationContext,
    step_index: usize,
    budgets: super::EvalBudgets,
    check_errors: &mut ValidationErrors,
) -> Result<Option<job::Step>, ModelError> {
    let mut step_symtab = symtab.clone();

    let step_name = st.name.clone();
    let step_path = [
        PathElement::Field("steps".to_string()),
        PathElement::Index(step_index),
    ];

    if has_expr {
        step_symtab.set(
            "Step.Name",
            openjd_expr::ExprValue::String(step_name.clone()),
        )?;
    }

    // Evaluate step-level let bindings (TEMPLATE scope — no PATH Param.*,
    // no host context), reported at `steps[i] -> let[j]` like pass 8.
    let mut step_let_failed = false;
    if has_expr {
        if let Some(bindings) = &st.let_bindings {
            let before = check_errors.len();
            crate::template::validate_v2023_09::format_strings::check_carried_forward_let_bindings(
                bindings,
                &path_field(&step_path, "let"),
                &mut step_symtab,
                ctx,
                openjd_expr::HostContext::None,
                check_errors,
            );
            step_let_failed = check_errors.len() > before;
        }
    }

    let script_template = st.resolve_syntax_sugar();
    let script = script_template.as_ref().map(convert_step_script);
    // When the script came from a SimpleAction (§8), diagnostics on the
    // desugared form must point at the field the author wrote.
    let sugar_kind = if st.script.is_none() {
        st.simple_actions().next().map(|(kind, _)| kind)
    } else {
        None
    };

    // Re-run the carried-forward format-string resolved-value checks —
    // the script's `let` bindings, the `onRun` action's
    // `command`/`args`/`timeout`/`cancelation`, and each embedded file's
    // `data` — against check symbol tables where job parameters are
    // bound to real values. A violation template validation could only
    // lower-bound is decidable here: fail at submission, not on every
    // worker. These are exactly the checks pass 8 applies — to the same
    // desugared form for a SimpleAction step, with the same path remap
    // back onto the authored field.
    if let Some(s) = &script_template {
        // A SimpleAction's checks run into a scratch collection rooted at
        // the desugared `script` node and are re-rooted afterwards.
        let mut scratch = ValidationErrors::default();
        let (script_path, target): (Vec<PathElement>, &mut ValidationErrors) = match sugar_kind {
            Some(_) => (Vec::new(), &mut scratch),
            None => (path_field(&step_path, "script"), &mut *check_errors),
        };
        // Check symbol table for the step script's carried-forward
        // (session/task-scope) format strings: `step_symtab`'s concrete
        // values plus `Unresolved` placeholders for everything only a
        // session can bind, with the script-level `let` bindings
        // evaluated in (reported at `<script> -> let[j]`).
        let cst =
            build_task_check_symtab(st, s, &step_symtab, has_expr, ctx, &script_path, target)?;
        crate::template::validate_v2023_09::format_strings::check_carried_forward_step_script(
            s,
            &cst,
            &step_symtab,
            ctx,
            &script_path,
            target,
        );
        if let Some(kind) = sugar_kind {
            let sa_path = path_field(&step_path, kind.field_name());
            check_errors.extend_remapped(scratch, |rel| {
                let mut p = sa_path.clone();
                p.extend(kind.remap_desugared_path(rel));
                p
            });
        }
    }

    // The same checks for this step's environments (session scope):
    // `let` bindings, `variables` values, every action's
    // `command`/`args`/`timeout`/`cancelation`, embedded-file `data`.
    if let Some(envs) = &st.step_environments {
        for (j, env) in envs.iter().enumerate() {
            let env_path = path_index(&path_field(&step_path, "stepEnvironments"), j);
            let env_symtab =
                build_env_check_symtab(env, &step_symtab, has_expr, ctx, &env_path, check_errors)?;
            crate::template::validate_v2023_09::format_strings::check_carried_forward_environment(
                env,
                &env_symtab,
                &step_symtab,
                ctx,
                &env_path,
                check_errors,
            );
        }
    }

    // Host requirements and the parameter space are resolved for real
    // and need the step-level `let` values; with one of those failed
    // (already reported) the step cannot be instantiated.
    if step_let_failed {
        return Ok(None);
    }

    let host_requirements = st
        .host_requirements
        .as_ref()
        .map(|hr| {
            host_requirements::resolve_host_requirements(hr, &step_symtab, ctx, step_index, budgets)
        })
        .transpose()?;

    let parameter_space = st
        .parameter_space
        .as_ref()
        .map(|ps| ranges::resolve_parameter_space(ps, &step_symtab, template_expr, limits, budgets))
        .transpose()?;

    // Validate the resolved parameter space (e.g. association length mismatches)
    if let Some(ref ps) = parameter_space {
        let _ = crate::job::step_param_space::StepParameterSpaceIterator::new(ps)?;
    }

    let step_environments = st
        .step_environments
        .as_ref()
        .map(|envs| envs.iter().map(convert_environment).collect());

    let dependencies = st.dependencies.as_ref().map(|deps| {
        deps.iter()
            .map(|d| job::StepDependency {
                depends_on: d.depends_on.clone(),
            })
            .collect()
    });

    let script = script.ok_or_else(|| {
        ModelError::DecodeValidation("Step must have a script or SimpleAction".to_string())
    })?;
    let filtered_symtab = filter_symtab_for_step(
        &step_symtab,
        Some(&script),
        &step_environments,
        st.let_bindings.as_deref(),
    );

    Ok(Some(job::Step {
        name: step_name,
        description: st.description.as_ref().map(|d| d.0.clone()),
        script,
        step_environments,
        parameter_space,
        host_requirements,
        dependencies,
        resolved_symtab: Some(openjd_expr::SerializedSymbolTable::from_symtab(
            &filtered_symtab,
        )),
    }))
}

/// Add `Unresolved` placeholders for the symbols only a session can
/// bind: `Session.*`, plus `Param.*` for PATH-typed job parameters
/// (excluded from the job-creation symtab because they require
/// session-time path mapping — the Param type is derived from the
/// `RawParam.*` value that is present).
///
/// Seed failures (a key path colliding with an existing scalar) are
/// propagated rather than ignored — a silently failed seed would
/// degrade the downstream checks to no-ops. The seed keys are
/// uppercase-rooted and `let` bindings must start lowercase, so this
/// only fails if an internal invariant is broken.
fn add_unresolved_session_symbols(symtab: &mut SymbolTable) -> Result<(), ModelError> {
    let raw_param_names: Vec<String> = symtab
        .get_table("RawParam")
        .map(|t| t.keys().map(str::to_string).collect())
        .unwrap_or_default();
    for name in raw_param_names {
        let param_key = format!("Param.{name}");
        if !symtab.contains(&param_key) {
            let raw_key = format!("RawParam.{name}");
            let unresolved_type = match symtab.get_value(&raw_key) {
                Some(
                    openjd_expr::ExprValue::ListPath(..) | openjd_expr::ExprValue::ListString(..),
                ) => crate::types::fixed_type(openjd_expr::ExprType::list(
                    openjd_expr::ExprType::PATH,
                )),
                _ => openjd_expr::ExprType::PATH,
            };
            symtab.set(
                &param_key,
                openjd_expr::ExprValue::Unresolved(unresolved_type),
            )?;
        }
    }
    symtab.set(
        "Session.WorkingDirectory",
        openjd_expr::ExprValue::Unresolved(openjd_expr::ExprType::PATH),
    )?;
    symtab.set(
        "Session.HasPathMappingRules",
        openjd_expr::ExprValue::Unresolved(openjd_expr::ExprType::BOOL),
    )?;
    symtab.set(
        "Session.PathMappingRulesFile",
        openjd_expr::ExprValue::Unresolved(openjd_expr::ExprType::PATH),
    )?;
    Ok(())
}

/// Build the check symbol table for a step script's carried-forward
/// format strings (task scope): the step's symtab (concrete `Param.*` /
/// `RawParam.*` / `Job.Name` / `Step.Name` / step-level `let`
/// bindings) plus `Unresolved` placeholders for `Session.*`, PATH
/// `Param.*`, `Task.Param.*` / `Task.RawParam.*`, and `Task.File.*` —
/// the same scope the session runtime binds when it resolves these
/// format strings at run time. A step-level binding holding a path is
/// unresolved too: the session gives it the worker's path format
/// (`unresolve_template_paths`).
///
/// Script-level `let` bindings are evaluated into the table with
/// unresolved host context through pass 8's own evaluation helper
/// (`check_carried_forward_let_bindings`): a binding that fails with the
/// real parameter values is reported into `errors` at
/// `<script_path> -> let[j]` with pass 8's message and caret, and bound
/// `Unresolved` so the script's remaining checks still run.
///
/// `Err` only for a failed symbol seed — an internal invariant (see
/// [`add_unresolved_session_symbols`]).
fn build_task_check_symtab(
    st: &template::StepTemplate,
    script: &template::StepScript,
    step_symtab: &SymbolTable,
    has_expr: bool,
    ctx: &crate::types::ValidationContext,
    script_path: &[PathElement],
    errors: &mut ValidationErrors,
) -> Result<SymbolTable, ModelError> {
    let mut check_symtab = step_symtab.clone();
    crate::template::validate_v2023_09::format_strings::unresolve_template_paths(&mut check_symtab);
    add_unresolved_session_symbols(&mut check_symtab)?;

    if let Some(ps) = &st.parameter_space {
        for tp in &ps.task_parameter_definitions {
            let tp_type = match tp {
                crate::template::TaskParameterDefinition::INT(_) => openjd_expr::ExprType::INT,
                crate::template::TaskParameterDefinition::CHUNK_INT(_) => {
                    openjd_expr::ExprType::RANGE_EXPR
                }
                crate::template::TaskParameterDefinition::FLOAT(_) => openjd_expr::ExprType::FLOAT,
                crate::template::TaskParameterDefinition::STRING(_) => {
                    openjd_expr::ExprType::STRING
                }
                crate::template::TaskParameterDefinition::PATH(_) => openjd_expr::ExprType::PATH,
            };
            check_symtab.set(
                &format!("Task.Param.{}", tp.name()),
                openjd_expr::ExprValue::Unresolved(tp_type.clone()),
            )?;
            let raw_type = match tp {
                crate::template::TaskParameterDefinition::PATH(_) => openjd_expr::ExprType::STRING,
                _ => tp_type,
            };
            check_symtab.set(
                &format!("Task.RawParam.{}", tp.name()),
                openjd_expr::ExprValue::Unresolved(raw_type),
            )?;
        }
    }

    if let Some(files) = &script.embedded_files {
        for f in files {
            check_symtab.set(
                &format!("Task.File.{}", f.name),
                openjd_expr::ExprValue::Unresolved(openjd_expr::ExprType::PATH),
            )?;
        }
    }

    if has_expr {
        if let Some(bindings) = &script.let_bindings {
            crate::template::validate_v2023_09::format_strings::check_carried_forward_let_bindings(
                bindings,
                &path_field(script_path, "let"),
                &mut check_symtab,
                ctx,
                openjd_expr::HostContext::Unresolved,
                errors,
            );
        }
    }

    Ok(check_symtab)
}

/// Build the check symbol table for an environment's carried-forward
/// format strings (session scope; job-level or step-level
/// environments): the base symtab (concrete `Param.*` / `RawParam.*` /
/// `Job.Name`, plus `Step.Name` and step-level `let` bindings when
/// `base` is a step's table) with `Unresolved` placeholders for
/// `Session.*`, PATH `Param.*`, this environment's `Env.File.*`, and
/// step-level bindings holding a path (see [`build_task_check_symtab`]),
/// and the environment's script-level `let` bindings evaluated in —
/// mirroring what the session runtime binds when it resolves the
/// environment at run time.
///
/// A `let` binding that fails to evaluate is reported into `errors` at
/// `<env_path> -> script -> let[j]` exactly as for a step script (see
/// [`build_task_check_symtab`]): the bindings only evaluate when the
/// context profile enables EXPR, their expressions already type-checked
/// at pass 8 with everything unresolved, so a failure here comes from
/// the real parameter values and would deterministically recur in every
/// session entering the environment.
pub(super) fn build_env_check_symtab(
    env: &template::Environment,
    base: &SymbolTable,
    has_expr: bool,
    ctx: &crate::types::ValidationContext,
    env_path: &[PathElement],
    errors: &mut ValidationErrors,
) -> Result<SymbolTable, ModelError> {
    let mut symtab = base.clone();
    crate::template::validate_v2023_09::format_strings::unresolve_template_paths(&mut symtab);
    add_unresolved_session_symbols(&mut symtab)?;
    if let Some(script) = &env.script {
        if let Some(files) = &script.embedded_files {
            for f in files {
                symtab.set(
                    &format!("Env.File.{}", f.name),
                    openjd_expr::ExprValue::Unresolved(openjd_expr::ExprType::PATH),
                )?;
            }
        }
        if has_expr {
            if let Some(bindings) = &script.let_bindings {
                crate::template::validate_v2023_09::format_strings::check_carried_forward_let_bindings(
                    bindings,
                    &path_field(&path_field(env_path, "script"), "let"),
                    &mut symtab,
                    ctx,
                    openjd_expr::HostContext::Unresolved,
                    errors,
                );
            }
        }
    }
    Ok(symtab)
}

fn convert_action(a: &template::Action) -> job::Action {
    job::Action {
        command: a.command.clone(),
        args: a.args.clone(),
        timeout: a.timeout.clone(),
        cancelation: a.cancelation.as_ref().map(|c| match c {
            template::CancelationMode::Terminate => job::CancelationMode::Terminate,
            template::CancelationMode::NotifyThenTerminate {
                notify_period_in_seconds,
            } => job::CancelationMode::NotifyThenTerminate {
                notify_period_in_seconds: notify_period_in_seconds.clone(),
            },
            template::CancelationMode::DeferredMode {
                mode,
                notify_period_in_seconds,
            } => job::CancelationMode::DeferredMode {
                mode: mode.clone(),
                notify_period_in_seconds: notify_period_in_seconds.clone(),
            },
        }),
    }
}

fn convert_step_script(s: &template::StepScript) -> job::StepScript {
    job::StepScript {
        let_bindings: s.let_bindings.clone(),
        actions: job::StepActions {
            on_run: convert_action(&s.actions.on_run),
        },
        embedded_files: s
            .embedded_files
            .as_ref()
            .map(|files| files.iter().map(convert_embedded_file).collect()),
    }
}

fn convert_embedded_file(f: &template::EmbeddedFile) -> job::EmbeddedFile {
    job::EmbeddedFile {
        name: f.name.clone(),
        file_type: f.file_type,
        filename: f.filename.clone(),
        data: f.data.clone(),
        runnable: f.runnable,
        end_of_line: f.end_of_line,
    }
}

/// Convert a template Environment to a job Environment (SESSION scope — keep FormatString).
#[must_use]
pub fn convert_environment(env: &template::Environment) -> job::Environment {
    convert_environment_with_symtab(env, None)
}

/// Convert a template Environment to a job Environment, optionally filtering
/// the symbol table to only symbols referenced by this environment's format strings.
#[must_use]
pub fn convert_environment_with_symtab(
    env: &template::Environment,
    symtab: Option<&SymbolTable>,
) -> job::Environment {
    let converted = job::Environment {
        name: env.name.clone(),
        description: env.description.as_ref().map(|d| d.0.clone()),
        script: env.script.as_ref().map(|s| job::EnvironmentScript {
            let_bindings: s.let_bindings.clone(),
            actions: job::EnvironmentActions {
                on_enter: s.actions.on_enter.as_ref().map(convert_action),
                on_wrap_env_enter: s.actions.on_wrap_env_enter.as_ref().map(convert_action),
                on_wrap_task_run: s.actions.on_wrap_task_run.as_ref().map(convert_action),
                on_wrap_env_exit: s.actions.on_wrap_env_exit.as_ref().map(convert_action),
                on_exit: s.actions.on_exit.as_ref().map(convert_action),
            },
            embedded_files: s
                .embedded_files
                .as_ref()
                .map(|files| files.iter().map(convert_embedded_file).collect()),
        }),
        variables: env.variables.clone(),
        resolved_symtab: None,
    };
    match symtab {
        Some(st) => {
            let filtered = filter_symtab_for_environment(&converted, st);
            job::Environment {
                resolved_symtab: Some(openjd_expr::SerializedSymbolTable::from_symtab(&filtered)),
                ..converted
            }
        }
        None => converted,
    }
}

/// Evaluate let bindings and return a new symbol table with bound values.
///
/// `memory_limit` / `operation_limit` bound each binding's expression
/// evaluation (the Expression Language spec's "Memory-bounded evaluation"
/// budgets — `CallerLimits::max_eval_memory_bytes` /
/// `max_eval_operations`); `None` uses the spec-recommended defaults.
pub fn evaluate_let_bindings(
    bindings: &[String],
    symtab: &SymbolTable,
    library: Option<&openjd_expr::function_library::FunctionLibrary>,
    path_format: PathFormat,
    memory_limit: Option<usize>,
    operation_limit: Option<usize>,
) -> Result<SymbolTable, ModelError> {
    let mut result = symtab.clone();
    for binding in bindings {
        let eq_pos = binding.find('=').ok_or_else(|| {
            ModelError::Expression(ExpressionError::new(format!(
                "Missing '=' in let binding: {binding}"
            )))
        })?;
        let name = binding[..eq_pos].trim();
        let expr = binding[eq_pos + 1..].trim();
        let prefix = &binding
            [..eq_pos + 1 + binding[eq_pos + 1..].len() - binding[eq_pos + 1..].trim_start().len()];
        let parsed = openjd_expr::ParsedExpression::new(expr).map_err(|e| {
            ModelError::Expression(ExpressionError::new(format!(
                "Error evaluating let binding '{name}': {}",
                e.message_with_expr_prefix(prefix)
            )))
        })?;
        let mut builder = parsed.with_path_format(path_format);
        if let Some(lib) = library {
            builder = builder.with_library(lib);
        }
        if let Some(m) = memory_limit {
            builder = builder.with_memory_limit(m);
        }
        if let Some(o) = operation_limit {
            builder = builder.with_operation_limit(o);
        }
        let value = builder.evaluate(&[&result as &SymbolTable]).map_err(|e| {
            ModelError::Expression(ExpressionError::new(format!(
                "Error evaluating let binding '{name}': {}",
                e.message_with_expr_prefix(prefix)
            )))
        })?;
        result.set(name, value).map_err(|e| {
            ModelError::Expression(ExpressionError::new(format!(
                "Error setting let binding '{name}': {e}"
            )))
        })?;
    }
    Ok(result)
}

// ── Host-context symbol table filtering ─────────────────────────────
//
// `resolved_symtab` is transported to the worker host that runs the job.
// The host evaluates the format strings that remain unresolved after job
// creation, so we filter the full symbol table down to exactly the symbols
// those format strings reference. Most of these are host-context
// (SESSION/TASK scope); action `timeout`/`notifyPeriodInSeconds` are
// template scope (validation restricts them to job-creation-stage symbols)
// but also resolve on the worker, so their references are included too.
//
// Step and Environment have different sets of these format strings:
//   Step  — step-level let bindings, script (actions, embedded files,
//           script-level let bindings), and step-scoped environments
//           (variables, actions, embedded files).
//   Env   — variables, script (actions, embedded files, script-level
//           let bindings).
//
// Both apply the RawParam fallback uniformly: for PATH-typed parameters,
// `Param.X` is absent from the template-scope symtab, so when a format
// string references `Param.X` we include `RawParam.X` instead, allowing
// the session to construct `Param.X` with path mapping at runtime.

fn filter_symtab_for_step(
    full: &SymbolTable,
    script: Option<&job::StepScript>,
    step_environments: &Option<Vec<job::Environment>>,
    step_let_bindings: Option<&[String]>,
) -> SymbolTable {
    let mut filtered = SymbolTable::new();

    if let Some(bindings) = step_let_bindings {
        collect_let_binding_refs(bindings, full, &mut filtered);
    }

    if let Some(s) = script {
        s.actions
            .on_run
            .command
            .copy_used_symtab_values(full, &mut filtered);
        if let Some(args) = &s.actions.on_run.args {
            for a in args {
                a.copy_used_symtab_values(full, &mut filtered);
            }
        }
        if let Some(t) = &s.actions.on_run.timeout {
            t.copy_used_symtab_values(full, &mut filtered);
        }
        match &s.actions.on_run.cancelation {
            Some(job::CancelationMode::NotifyThenTerminate {
                notify_period_in_seconds: Some(n),
            }) => n.copy_used_symtab_values(full, &mut filtered),
            Some(job::CancelationMode::DeferredMode {
                mode,
                notify_period_in_seconds,
            }) => {
                mode.copy_used_symtab_values(full, &mut filtered);
                if let Some(n) = notify_period_in_seconds {
                    n.copy_used_symtab_values(full, &mut filtered);
                }
            }
            _ => {}
        }
        if let Some(files) = &s.embedded_files {
            for f in files {
                if let Some(d) = &f.data {
                    d.copy_used_symtab_values(full, &mut filtered);
                }
            }
        }
        if let Some(bindings) = &s.let_bindings {
            collect_let_binding_refs(bindings, full, &mut filtered);
        }
    }

    if let Some(envs) = step_environments {
        for env in envs {
            if let Some(vars) = &env.variables {
                for fs in vars.values() {
                    fs.copy_used_symtab_values(full, &mut filtered);
                }
            }
            if let Some(es) = &env.script {
                collect_env_action_refs(&es.actions, full, &mut filtered);
                if let Some(files) = &es.embedded_files {
                    for f in files {
                        if let Some(d) = &f.data {
                            d.copy_used_symtab_values(full, &mut filtered);
                        }
                    }
                }
            }
        }
    }

    // For PATH/LIST[PATH] params, Param.X is excluded from the template-scope symtab
    // (host-context only). When a format string references Param.X and it's missing from
    // full, include RawParam.X so the session can construct Param.X with path mapping.
    let all_symbols = collect_all_accessed_symbols(script, step_environments, step_let_bindings);
    include_raw_param_fallbacks(&all_symbols, full, &mut filtered);

    filtered
}

/// For PATH/LIST[PATH] params, `Param.X` is excluded from the template-scope symtab
/// (host-context only). When a format string references `Param.X` and it's missing from
/// `full`, include `RawParam.X` so the session can construct `Param.X` with path mapping.
fn include_raw_param_fallbacks(
    symbols: &std::collections::HashSet<String>,
    full: &SymbolTable,
    filtered: &mut SymbolTable,
) {
    for symbol in symbols {
        if let Some(rest) = symbol.strip_prefix("Param.") {
            // Extract just the parameter name (first component), ignoring
            // property/method access like Param.X.name or Param.X.upper().
            let param_name = rest.split('.').next().unwrap_or(rest);
            let param_key = format!("Param.{param_name}");
            if full.get_value(&param_key).is_none() {
                let raw_key = format!("RawParam.{param_name}");
                copy_symbol_value(&raw_key, full, filtered);
            }
        }
    }
}

/// Collect all symbol names accessed by format strings in a step's script,
/// step environments, and let bindings.
fn collect_all_accessed_symbols(
    script: Option<&job::StepScript>,
    step_environments: &Option<Vec<job::Environment>>,
    step_let_bindings: Option<&[String]>,
) -> std::collections::HashSet<String> {
    let mut symbols = std::collections::HashSet::new();

    fn collect_from_fs(
        fs: &openjd_expr::FormatString,
        out: &mut std::collections::HashSet<String>,
    ) {
        out.extend(fs.accessed_symbols());
    }

    fn collect_from_action(a: &job::Action, out: &mut std::collections::HashSet<String>) {
        collect_from_fs(&a.command, out);
        if let Some(args) = &a.args {
            for fs in args {
                collect_from_fs(fs, out);
            }
        }
        if let Some(t) = &a.timeout {
            collect_from_fs(t, out);
        }
        match &a.cancelation {
            Some(job::CancelationMode::NotifyThenTerminate {
                notify_period_in_seconds: Some(n),
            }) => collect_from_fs(n, out),
            Some(job::CancelationMode::DeferredMode {
                mode,
                notify_period_in_seconds,
            }) => {
                collect_from_fs(mode, out);
                if let Some(n) = notify_period_in_seconds {
                    collect_from_fs(n, out);
                }
            }
            _ => {}
        }
    }

    if let Some(bindings) = step_let_bindings {
        for binding in bindings {
            if let Some(eq_pos) = binding.find('=') {
                let expr = binding[eq_pos + 1..].trim();
                if let Ok(parsed) = openjd_expr::eval::ParsedExpression::new(expr) {
                    symbols.extend(parsed.accessed_symbols().iter().cloned());
                }
            }
        }
    }

    if let Some(s) = script {
        collect_from_action(&s.actions.on_run, &mut symbols);
        if let Some(files) = &s.embedded_files {
            for f in files {
                if let Some(d) = &f.data {
                    collect_from_fs(d, &mut symbols);
                }
            }
        }
        if let Some(bindings) = &s.let_bindings {
            for binding in bindings {
                if let Some(eq_pos) = binding.find('=') {
                    let expr = binding[eq_pos + 1..].trim();
                    if let Ok(parsed) = openjd_expr::eval::ParsedExpression::new(expr) {
                        symbols.extend(parsed.accessed_symbols().iter().cloned());
                    }
                }
            }
        }
    }

    if let Some(envs) = step_environments {
        for env in envs {
            if let Some(vars) = &env.variables {
                for fs in vars.values() {
                    collect_from_fs(fs, &mut symbols);
                }
            }
            if let Some(es) = &env.script {
                for action in es.actions.iter_actions() {
                    collect_from_action(action, &mut symbols);
                }
                if let Some(files) = &es.embedded_files {
                    for f in files {
                        if let Some(d) = &f.data {
                            collect_from_fs(d, &mut symbols);
                        }
                    }
                }
            }
        }
    }

    symbols
}

fn collect_let_binding_refs(bindings: &[String], full: &SymbolTable, filtered: &mut SymbolTable) {
    for binding in bindings {
        if let Some(eq_pos) = binding.find('=') {
            let expr = binding[eq_pos + 1..].trim();
            // Bindings have already been validated and evaluated by this point.
            // Parse here only to discover referenced symbols for the filtered symtab.
            // A parse failure is unreachable but harmless — the symbol just won't
            // appear in the filtered output.
            if let Ok(parsed) = openjd_expr::eval::ParsedExpression::new(expr) {
                for symbol in parsed.accessed_symbols() {
                    copy_symbol_value(symbol, full, filtered);
                }
            }
        }
    }
}

fn collect_env_action_refs(
    actions: &job::EnvironmentActions,
    full: &SymbolTable,
    filtered: &mut SymbolTable,
) {
    for action in actions.iter_actions() {
        action.command.copy_used_symtab_values(full, filtered);
        if let Some(args) = &action.args {
            for a in args {
                a.copy_used_symtab_values(full, filtered);
            }
        }
        if let Some(t) = &action.timeout {
            t.copy_used_symtab_values(full, filtered);
        }
        match &action.cancelation {
            Some(job::CancelationMode::NotifyThenTerminate {
                notify_period_in_seconds: Some(n),
            }) => n.copy_used_symtab_values(full, filtered),
            // A deferred (format-string) cancelation mode and its period are
            // resolved at run time; their referenced symbols must survive the
            // filter or resolution fails with "Undefined variable". Keep in
            // sync with collect_env_accessed_symbols below, which collects
            // the same fields for the RawParam.* fallback pass.
            Some(job::CancelationMode::DeferredMode {
                mode,
                notify_period_in_seconds,
            }) => {
                mode.copy_used_symtab_values(full, filtered);
                if let Some(n) = notify_period_in_seconds {
                    n.copy_used_symtab_values(full, filtered);
                }
            }
            _ => {}
        }
    }
}

fn filter_symtab_for_environment(env: &job::Environment, full: &SymbolTable) -> SymbolTable {
    let mut filtered = SymbolTable::new();
    if let Some(vars) = &env.variables {
        for fs in vars.values() {
            fs.copy_used_symtab_values(full, &mut filtered);
        }
    }
    if let Some(es) = &env.script {
        collect_env_action_refs(&es.actions, full, &mut filtered);
        if let Some(files) = &es.embedded_files {
            for f in files {
                if let Some(d) = &f.data {
                    d.copy_used_symtab_values(full, &mut filtered);
                }
            }
        }
        if let Some(bindings) = &es.let_bindings {
            collect_let_binding_refs(bindings, full, &mut filtered);
        }
    }
    let symbols = collect_env_accessed_symbols(env);
    include_raw_param_fallbacks(&symbols, full, &mut filtered);
    filtered
}

/// Collect all symbol names accessed by an environment's worker-resolved
/// format strings (host-context fields plus template-scope timeouts).
fn collect_env_accessed_symbols(env: &job::Environment) -> std::collections::HashSet<String> {
    let mut symbols = std::collections::HashSet::new();
    if let Some(vars) = &env.variables {
        for fs in vars.values() {
            symbols.extend(fs.accessed_symbols());
        }
    }
    if let Some(es) = &env.script {
        for action in es.actions.iter_actions() {
            symbols.extend(action.command.accessed_symbols());
            if let Some(args) = &action.args {
                for fs in args {
                    symbols.extend(fs.accessed_symbols());
                }
            }
            if let Some(t) = &action.timeout {
                symbols.extend(t.accessed_symbols());
            }
            match &action.cancelation {
                Some(job::CancelationMode::NotifyThenTerminate {
                    notify_period_in_seconds: Some(n),
                }) => symbols.extend(n.accessed_symbols()),
                // Deferred (format-string) cancelation fields resolve at run
                // time; their symbols feed include_raw_param_fallbacks so a
                // PATH/LIST[PATH] Param.* referenced only here still gets its
                // RawParam.* fallback in the filtered symtab. Keep in sync
                // with collect_env_action_refs above.
                Some(job::CancelationMode::DeferredMode {
                    mode,
                    notify_period_in_seconds,
                }) => {
                    symbols.extend(mode.accessed_symbols());
                    if let Some(n) = notify_period_in_seconds {
                        symbols.extend(n.accessed_symbols());
                    }
                }
                _ => {}
            }
        }
        if let Some(files) = &es.embedded_files {
            for f in files {
                if let Some(d) = &f.data {
                    symbols.extend(d.accessed_symbols());
                }
            }
        }
        if let Some(bindings) = &es.let_bindings {
            for binding in bindings {
                if let Some(eq_pos) = binding.find('=') {
                    let expr = binding[eq_pos + 1..].trim();
                    if let Ok(parsed) = openjd_expr::eval::ParsedExpression::new(expr) {
                        symbols.extend(parsed.accessed_symbols().iter().cloned());
                    }
                }
            }
        }
    }
    symbols
}
