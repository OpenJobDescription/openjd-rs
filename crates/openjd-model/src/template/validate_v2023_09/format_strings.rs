// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Pass 8: Format string validation.
//!
//! Validates all format string references resolve to defined variables by
//! building scope-appropriate symbol tables and evaluating expressions against
//! them. This works for both base spec (simple `{{Param.X}}` references) and
//! EXPR (full expressions) — the evaluator handles both.

use std::collections::HashSet;

use openjd_expr::eval::ParsedExpression;
use openjd_expr::function_library::FunctionLibrary;
use openjd_expr::path_mapping::PathFormat;
use openjd_expr::symbol_table::SymbolTable;
use openjd_expr::types::ExprType;
use openjd_expr::value::ExprValue;
use openjd_expr::FormatString;

use crate::error::{path_field, path_index, PathElement, ValidationErrors};
use crate::template::*;
use crate::types::{ModelExtension, ValidationContext};

/// Build a symbol table containing Param/RawParam entries from job parameter definitions.
/// RawParam.* is always STRING for PATH types and LIST_STRING for LIST_PATH types,
/// matching Python behavior where RawParam holds the raw unprocessed value.
/// Takes a parameter slice (not a whole template) so both job templates and
/// environment templates can use it.
fn build_param_symtab(params: Option<&[JobParameterDefinition]>) -> SymbolTable {
    use crate::types::JobParameterType;
    let mut symtab = SymbolTable::new();
    if let Some(params) = params {
        for p in params {
            let pt = p.job_param_type();
            let expr_type = pt.expr_type();
            symtab
                .set(
                    &format!("Param.{}", p.name()),
                    ExprValue::unresolved(expr_type.clone()),
                )
                .expect("symtab");
            let raw_type = match pt {
                JobParameterType::Path => ExprType::STRING,
                JobParameterType::ListPath => ExprType::list(ExprType::STRING),
                _ => expr_type,
            };
            symtab
                .set(
                    &format!("RawParam.{}", p.name()),
                    ExprValue::unresolved(raw_type),
                )
                .expect("symtab");
        }
    }
    symtab
}

/// Build the template-scope symtab (for job name, host requirements, parameter
/// space ranges, and fields resolved at job creation such as `timeout` and
/// `notifyPeriodInSeconds`). PATH and LIST[PATH] Param.* are excluded
/// (host-only); RawParam.* is always STRING for path types. Takes a parameter
/// slice so it works for both job templates and standalone environment
/// templates.
fn build_template_scope_symtab(params: Option<&[JobParameterDefinition]>) -> SymbolTable {
    use crate::types::JobParameterType;
    let mut symtab = SymbolTable::new();
    if let Some(params) = params {
        for p in params {
            let pt = p.job_param_type();
            let expr_type = pt.expr_type();
            let is_path = matches!(pt, JobParameterType::Path | JobParameterType::ListPath);
            if !is_path {
                symtab
                    .set(
                        &format!("Param.{}", p.name()),
                        ExprValue::unresolved(expr_type.clone()),
                    )
                    .expect("symtab");
            }
            let raw_type = match pt {
                JobParameterType::Path => ExprType::STRING,
                JobParameterType::ListPath => ExprType::list(ExprType::STRING),
                _ => expr_type,
            };
            symtab
                .set(
                    &format!("RawParam.{}", p.name()),
                    ExprValue::unresolved(raw_type),
                )
                .expect("symtab");
        }
    }
    symtab
}

/// Build the session-scope symtab (for environment scripts/variables).
/// Contains: Param.*, RawParam.*, Session.*, Env.File.*, let bindings
/// When `is_step_env` is true, also includes Step.Name (EXPR only).
/// Takes a parameter slice so it works for both job templates and standalone
/// environment templates.
fn build_session_scope_symtab(
    params: Option<&[JobParameterDefinition]>,
    env: &Environment,
    is_step_env: bool,
    expr_active: bool,
) -> SymbolTable {
    let mut symtab = build_param_symtab(params);
    symtab
        .set(
            "Session.WorkingDirectory",
            ExprValue::unresolved(ExprType::PATH),
        )
        .expect("symtab");
    symtab
        .set(
            "Session.HasPathMappingRules",
            ExprValue::unresolved(ExprType::BOOL),
        )
        .expect("symtab");
    symtab
        .set(
            "Session.PathMappingRulesFile",
            ExprValue::unresolved(ExprType::PATH),
        )
        .expect("symtab");
    // Step.Name available in step environments with EXPR
    if is_step_env && expr_active {
        symtab
            .set("Step.Name", ExprValue::unresolved(ExprType::STRING))
            .expect("symtab");
    }
    // Job.Name available in all environments with EXPR
    if expr_active {
        symtab
            .set("Job.Name", ExprValue::unresolved(ExprType::STRING))
            .expect("symtab");
    }
    // Env.File.* from this environment's script
    if let Some(script) = &env.script {
        if let Some(files) = &script.embedded_files {
            for f in files {
                symtab
                    .set(
                        &format!("Env.File.{}", f.name),
                        ExprValue::unresolved(ExprType::PATH),
                    )
                    .expect("symtab");
            }
        }
    }
    symtab
}

/// Build the task-scope symtab (for step scripts).
/// Contains: Param.*, RawParam.*, Session.*, Task.Param.*, Task.RawParam.*,
///           Task.File.*, Job.Name, Step.Name, Env.File.* from step envs,
///           plus let bindings.
fn build_task_scope_symtab(
    jt: &JobTemplate,
    step: &StepTemplate,
    expr_active: bool,
) -> SymbolTable {
    let mut symtab = build_param_symtab(jt.parameter_definitions.as_deref());

    // Session scope
    symtab
        .set(
            "Session.WorkingDirectory",
            ExprValue::unresolved(ExprType::PATH),
        )
        .expect("symtab");
    symtab
        .set(
            "Session.HasPathMappingRules",
            ExprValue::unresolved(ExprType::BOOL),
        )
        .expect("symtab");
    symtab
        .set(
            "Session.PathMappingRulesFile",
            ExprValue::unresolved(ExprType::PATH),
        )
        .expect("symtab");

    // EXPR-only built-ins
    if expr_active {
        symtab
            .set("Job.Name", ExprValue::unresolved(ExprType::STRING))
            .expect("symtab");
        symtab
            .set("Step.Name", ExprValue::unresolved(ExprType::STRING))
            .expect("symtab");
    }

    // Task parameters
    if let Some(ps) = &step.parameter_space {
        for tp in &ps.task_parameter_definitions {
            let tp_type = match tp {
                TaskParameterDefinition::INT(_) => ExprType::INT,
                TaskParameterDefinition::CHUNK_INT(_) => ExprType::RANGE_EXPR,
                TaskParameterDefinition::FLOAT(_) => ExprType::FLOAT,
                TaskParameterDefinition::STRING(_) => ExprType::STRING,
                TaskParameterDefinition::PATH(_) => ExprType::PATH,
            };
            symtab
                .set(
                    &format!("Task.Param.{}", tp.name()),
                    ExprValue::unresolved(tp_type.clone()),
                )
                .expect("symtab");
            // Task.RawParam.* for PATH is STRING (raw unprocessed value)
            let raw_type = match tp {
                TaskParameterDefinition::PATH(_) => ExprType::STRING,
                _ => tp_type,
            };
            symtab
                .set(
                    &format!("Task.RawParam.{}", tp.name()),
                    ExprValue::unresolved(raw_type),
                )
                .expect("symtab");
        }
    }

    // Task.File.* from step script embedded files. In scope for script-level
    // `let` bindings too: `filename` is a plain string (never an expression),
    // so the runtime allocates embedded file paths — defining Task.File.* —
    // before evaluating `let`, mirroring the environment runner's Env.File.*
    // ordering.
    if let Some(script) = step
        .resolve_syntax_sugar()
        .ok()
        .flatten()
        .as_ref()
        .or(step.script.as_ref())
    {
        if let Some(files) = &script.embedded_files {
            for f in files {
                symtab
                    .set(
                        &format!("Task.File.{}", f.name),
                        ExprValue::unresolved(ExprType::PATH),
                    )
                    .expect("symtab");
            }
        }
    }

    // Env.File.* is NOT available in step scripts — it is only available
    // within environment scripts (§7.3). The task-scope symtab is used for
    // step script validation, so we do not add Env.File.* here.

    // Let bindings are evaluated during validation (validate_let_bindings),
    // which adds them to the symtab with inferred types. The symtab passed
    // here is cloned and mutated during validation, so we don't add them here.

    symtab
}

/// Validate a format string against a symbol table, reporting errors at the given path.
fn validate_fs(
    fs: &FormatString,
    symtab: &SymbolTable,
    lib: &FunctionLibrary,
    path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    if fs.is_literal() {
        return;
    }
    if let Err(e) = fs.validate_expressions(symtab, lib) {
        let mut spans = Vec::new();
        if let Some(ref expr_err) = e.expression_error {
            if !expr_err.sub_errors().is_empty() {
                // Compound error (e.g., if/else both branches fail) — one span per sub-error
                for sub in expr_err.sub_errors() {
                    if let (Some(expr), Some(col), Some(end_col)) =
                        (sub.expr(), sub.col_offset(), sub.end_col_offset())
                    {
                        spans.push(crate::error::DiagnosticSpan {
                            summary: sub.message(),
                            source: expr.to_string(),
                            start: col,
                            end: end_col,
                            caret: sub.caret_offset().unwrap_or(0),
                        });
                    }
                }
            }
        }
        // Fallback: single span covering the whole interpolation
        if spans.is_empty() {
            spans.push(crate::error::DiagnosticSpan {
                summary: e.message.clone(),
                source: e.input.clone(),
                start: e.start,
                end: e.end,
                caret: 0,
            });
        }
        let summary = if let Some(ref expr_err) = e.expression_error {
            expr_err.message()
        } else {
            e.message.clone()
        };
        let detail = crate::error::ErrorDetail { summary, spans };
        errors.add_with_detail(
            path,
            format!(
                "Failed to parse interpolation expression at [{}, {}]. {}",
                e.start, e.end, e.message
            ),
            detail,
        );
    }
}

/// Validate a format string in an action (command + args).
fn validate_action_fs(
    action: &Action,
    symtab: &SymbolTable,
    lib: &FunctionLibrary,
    action_path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    validate_fs(
        &action.command,
        symtab,
        lib,
        &path_field(action_path, "command"),
        errors,
    );
    if let Some(args) = &action.args {
        let args_path = path_field(action_path, "args");
        for (j, arg) in args.iter().enumerate() {
            validate_fs(arg, symtab, lib, &path_index(&args_path, j), errors);
        }
    }
}

pub fn validate_format_strings(
    jt: &JobTemplate,
    ctx: &ValidationContext,
    errors: &mut ValidationErrors,
) {
    let expr_active = ctx.profile.has_extension(ModelExtension::Expr);
    // Template/task-range validation uses HostContext::None (host functions
    // are not available in those scopes). Session/task scopes use
    // HostContext::Unresolved so apply_path_mapping type-checks.
    let template_profile = ctx.profile.to_expr_profile(openjd_expr::HostContext::None);
    let host_profile = ctx
        .profile
        .to_expr_profile(openjd_expr::HostContext::Unresolved);
    let template_lib = openjd_expr::FunctionLibrary::for_profile(&template_profile);
    let host_lib = openjd_expr::FunctionLibrary::for_profile(&host_profile);

    // ── Job name: template scope (Param/RawParam only) ──
    let template_symtab = build_template_scope_symtab(jt.parameter_definitions.as_deref());
    validate_fs(
        &jt.name,
        &template_symtab,
        &template_lib,
        &path_field(&[], "name"),
        errors,
    );

    // ── Host requirements: template scope + step let bindings ──
    for (i, step) in jt.steps.iter().enumerate() {
        let step_path = vec![PathElement::Field("steps".into()), PathElement::Index(i)];
        if let Some(hr) = &step.host_requirements {
            // Build a symtab with Param/RawParam + step let bindings
            let mut hr_symtab = build_template_scope_symtab(jt.parameter_definitions.as_deref());
            if expr_active {
                hr_symtab
                    .set("Job.Name", ExprValue::unresolved(ExprType::STRING))
                    .expect("symtab");
                hr_symtab
                    .set("Step.Name", ExprValue::unresolved(ExprType::STRING))
                    .expect("symtab");
                if let Some(bindings) = &step.let_bindings {
                    let let_path = path_field(&step_path, "let");
                    let mut hr_let_names = HashSet::new();
                    validate_let_bindings(
                        bindings,
                        &let_path,
                        &HashSet::new(),
                        &mut hr_let_names,
                        &mut hr_symtab,
                        &template_lib,
                        &template_profile,
                        errors,
                    );
                }
            }
            let hr_path = path_field(&step_path, "hostRequirements");
            if let Some(amounts) = &hr.amounts {
                for (j, amt) in amounts.iter().enumerate() {
                    let amt_path = path_index(&path_field(&hr_path, "amounts"), j);
                    if let Some(min) = &amt.min {
                        validate_fs(
                            min,
                            &hr_symtab,
                            &template_lib,
                            &path_field(&amt_path, "min"),
                            errors,
                        );
                    }
                    if let Some(max) = &amt.max {
                        validate_fs(
                            max,
                            &hr_symtab,
                            &template_lib,
                            &path_field(&amt_path, "max"),
                            errors,
                        );
                    }
                }
            }
            if let Some(attrs) = &hr.attributes {
                for (j, attr) in attrs.iter().enumerate() {
                    let attr_path = path_index(&path_field(&hr_path, "attributes"), j);
                    if let Some(any_of) = &attr.any_of {
                        for (k, v) in any_of.iter().enumerate() {
                            validate_fs(
                                v,
                                &hr_symtab,
                                &template_lib,
                                &path_index(&path_field(&attr_path, "anyOf"), k),
                                errors,
                            );
                        }
                    }
                    if let Some(all_of) = &attr.all_of {
                        for (k, v) in all_of.iter().enumerate() {
                            validate_fs(
                                v,
                                &hr_symtab,
                                &template_lib,
                                &path_index(&path_field(&attr_path, "allOf"), k),
                                errors,
                            );
                        }
                    }
                }
            }
        }
    }

    // ── Job environments: session scope (timeouts: template scope) ──
    if let Some(envs) = &jt.job_environments {
        let envs_path = path_field(&[], "jobEnvironments");
        // Job-creation-scope symtab for env `timeout`/`notifyPeriodInSeconds`:
        // Param.* (non-PATH), RawParam.*, and Job.Name with EXPR. No
        // Step.Name (job envs are not attached to a step) and no env-script
        // let bindings (those are evaluated at session time).
        let mut env_template_symtab =
            build_template_scope_symtab(jt.parameter_definitions.as_deref());
        if expr_active {
            env_template_symtab
                .set("Job.Name", ExprValue::unresolved(ExprType::STRING))
                .expect("symtab");
        }
        for (i, env) in envs.iter().enumerate() {
            let mut env_symtab = build_session_scope_symtab(
                jt.parameter_definitions.as_deref(),
                env,
                false,
                expr_active,
            );
            // Env script let bindings: validate and evaluate into the symtab
            // if EXPR, reject if not.
            if let Some(script) = &env.script {
                if let Some(bindings) = &script.let_bindings {
                    let let_path =
                        path_field(&path_field(&path_index(&envs_path, i), "script"), "let");
                    if !expr_active {
                        errors.add(&let_path, "'let' requires the EXPR extension.");
                    } else {
                        let mut env_let_names = HashSet::new();
                        validate_let_bindings(
                            bindings,
                            &let_path,
                            &HashSet::new(),
                            &mut env_let_names,
                            &mut env_symtab,
                            &host_lib,
                            &host_profile,
                            errors,
                        );
                    }
                }
            }
            validate_env_format_strings(
                env,
                &env_symtab,
                &host_lib,
                &env_template_symtab,
                &template_lib,
                &path_index(&envs_path, i),
                expr_active,
                errors,
            );
        }
    }

    // ── Steps ──
    for (i, step) in jt.steps.iter().enumerate() {
        let step_path = vec![PathElement::Field("steps".into()), PathElement::Index(i)];

        // Task parameter ranges use TEMPLATE scope (no PATH Param.*)
        if let Some(ps) = &step.parameter_space {
            let ps_path = path_field(&step_path, "parameterSpace");
            let tpd_path = path_field(&ps_path, "taskParameterDefinitions");
            let mut range_symtab = build_template_scope_symtab(jt.parameter_definitions.as_deref());
            if expr_active {
                range_symtab
                    .set("Job.Name", ExprValue::unresolved(ExprType::STRING))
                    .expect("symtab");
                range_symtab
                    .set("Step.Name", ExprValue::unresolved(ExprType::STRING))
                    .expect("symtab");
                // Step-level let bindings are in template scope and must be
                // visible in parameterSpace range expressions.
                if let Some(bindings) = &step.let_bindings {
                    for binding in bindings {
                        if let Some(eq_pos) = binding.find('=') {
                            let name = binding[..eq_pos].trim();
                            let expr_str = binding[eq_pos + 1..].trim();
                            if !name.is_empty() && !expr_str.is_empty() {
                                match openjd_expr::eval::ParsedExpression::with_profile(
                                    expr_str,
                                    &template_profile,
                                ) {
                                    Ok(parsed) => {
                                        match parsed
                                            .with_path_format(PathFormat::Posix)
                                            .with_library(&template_lib)
                                            .evaluate(&[&range_symtab as &SymbolTable])
                                        {
                                            Ok(val) => {
                                                let _ = range_symtab.set(name, val);
                                            }
                                            Err(e) => {
                                                errors.add(
                                                    &step_path,
                                                    format!("let binding '{name}': {e}"),
                                                );
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        errors
                                            .add(&step_path, format!("let binding '{name}': {e}"));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            for (j, tp) in ps.task_parameter_definitions.iter().enumerate() {
                let p_path = path_index(&tpd_path, j);
                match tp {
                    TaskParameterDefinition::INT(t) => {
                        if let crate::template::IntRange::Expression(expr) = &t.range {
                            validate_fs(
                                expr,
                                &range_symtab,
                                &template_lib,
                                &path_field(&p_path, "range"),
                                errors,
                            );
                        }
                    }
                    TaskParameterDefinition::STRING(t) => {
                        if let crate::template::StringRange::List(items) = &t.range {
                            for (k, item) in items.iter().enumerate() {
                                validate_fs(
                                    item,
                                    &range_symtab,
                                    &template_lib,
                                    &path_index(&path_field(&p_path, "range"), k),
                                    errors,
                                );
                            }
                        }
                    }
                    TaskParameterDefinition::PATH(t) => {
                        if let crate::template::StringRange::List(items) = &t.range {
                            for (k, item) in items.iter().enumerate() {
                                validate_fs(
                                    item,
                                    &range_symtab,
                                    &template_lib,
                                    &path_index(&path_field(&p_path, "range"), k),
                                    errors,
                                );
                            }
                        }
                    }
                    TaskParameterDefinition::CHUNK_INT(t) => {
                        if let crate::template::IntRange::Expression(expr) = &t.range {
                            validate_fs(
                                expr,
                                &range_symtab,
                                &template_lib,
                                &path_field(&p_path, "range"),
                                errors,
                            );
                        }
                    }
                    _ => {}
                }
            }
        }

        let mut task_symtab = build_task_scope_symtab(jt, step, expr_active);

        // Template-scope symtab for the step's job-creation-stage fields:
        // Param.* (non-PATH), RawParam.*, and with EXPR Job.Name, Step.Name,
        // and step-level let bindings — no Session.*, no Task.*. Used for
        // step let bindings and for `timeout`/`notifyPeriodInSeconds`
        // (plain @fmtstring: resolved at job creation, before any session
        // exists).
        let mut step_template_symtab =
            build_template_scope_symtab(jt.parameter_definitions.as_deref());
        if expr_active {
            step_template_symtab
                .set("Job.Name", ExprValue::unresolved(ExprType::STRING))
                .expect("symtab");
            step_template_symtab
                .set("Step.Name", ExprValue::unresolved(ExprType::STRING))
                .expect("symtab");
        }

        // Let bindings: validate and evaluate into symtab if EXPR, reject if not.
        // Step-level let bindings are TEMPLATE scope (template_lib, no Session.*, no PATH Param.*).
        // Script-level let bindings are TASK scope (host_lib).
        if let Some(bindings) = &step.let_bindings {
            let let_path = path_field(&step_path, "let");
            if !expr_active {
                errors.add(&let_path, "'let' requires the EXPR extension.");
            } else {
                let mut step_let_names = HashSet::new();
                validate_let_bindings(
                    bindings,
                    &let_path,
                    &HashSet::new(),
                    &mut step_let_names,
                    &mut step_template_symtab,
                    &template_lib,
                    &template_profile,
                    errors,
                );
                // Copy evaluated let bindings into task_symtab so script-level code can use them
                for name in &step_let_names {
                    if let Some(val) = step_template_symtab.get_value(name) {
                        let _ = task_symtab.set(name, val.clone());
                    }
                }
            }
        }

        if let Some(script) = &step.script {
            let script_path = path_field(&step_path, "script");

            // Script-level let bindings (TASK scope — host_lib). Task.File.*
            // is in scope: file paths are allocated before `let` evaluation
            // at runtime (filenames are plain strings, so allocation cannot
            // depend on `let` values).
            if let Some(bindings) = &script.let_bindings {
                let let_path = path_field(&script_path, "let");
                if !expr_active {
                    errors.add(&let_path, "'let' requires the EXPR extension.");
                } else {
                    let enclosing: HashSet<String> = step
                        .let_bindings
                        .as_ref()
                        .map(|bs| {
                            bs.iter()
                                .filter_map(|b| b.find('=').map(|eq| b[..eq].trim().to_string()))
                                .collect()
                        })
                        .unwrap_or_default();
                    let mut script_let_names = HashSet::new();
                    validate_let_bindings(
                        bindings,
                        &let_path,
                        &enclosing,
                        &mut script_let_names,
                        &mut task_symtab,
                        &host_lib,
                        &host_profile,
                        errors,
                    );
                }
            }

            // Complex expressions: reject if not EXPR
            if !expr_active {
                let action_path = path_field(&path_field(&script_path, "actions"), "onRun");
                if script.actions.on_run.command.has_complex_expressions() {
                    errors.add(
                        &path_field(&action_path, "command"),
                        "complex expressions require the EXPR extension.",
                    );
                }
                if let Some(args) = &script.actions.on_run.args {
                    let args_path = path_field(&action_path, "args");
                    for (j, arg) in args.iter().enumerate() {
                        if arg.has_complex_expressions() {
                            errors.add(
                                &path_index(&args_path, j),
                                "complex expressions require the EXPR extension.",
                            );
                        }
                    }
                }
            }

            // Validate all format string references (both base and EXPR)
            let action_path = path_field(&path_field(&script_path, "actions"), "onRun");
            validate_action_fs(
                &script.actions.on_run,
                &task_symtab,
                &host_lib,
                &action_path,
                errors,
            );

            // Timeout and notifyPeriodInSeconds are plain @fmtstring
            // (resolved at job creation, before any session exists), so
            // they validate against the template-scope symtab: no
            // Session.*, no Task.*, no Env.File.*, no host functions.
            if let Some(timeout) = &script.actions.on_run.timeout {
                validate_fs(
                    timeout,
                    &step_template_symtab,
                    &template_lib,
                    &path_field(&action_path, "timeout"),
                    errors,
                );
            }
            let (mode_fs, notify_fs) = match &script.actions.on_run.cancelation {
                Some(CancelationMode::NotifyThenTerminate {
                    notify_period_in_seconds,
                }) => (None, notify_period_in_seconds.as_ref()),
                Some(CancelationMode::DeferredMode {
                    mode,
                    notify_period_in_seconds,
                }) => (Some(mode), notify_period_in_seconds.as_ref()),
                _ => (None, None),
            };
            if let Some(mode) = mode_fs {
                validate_fs(
                    mode,
                    &step_template_symtab,
                    &template_lib,
                    &path_field(&action_path, "cancelation"),
                    errors,
                );
            }
            if let Some(notify) = notify_fs {
                validate_fs(
                    notify,
                    &step_template_symtab,
                    &template_lib,
                    &path_field(&action_path, "cancelation"),
                    errors,
                );
            }

            // Embedded files
            if let Some(files) = &script.embedded_files {
                let files_path = path_field(&script_path, "embeddedFiles");
                for (j, f) in files.iter().enumerate() {
                    let f_path = path_index(&files_path, j);
                    if let Some(data) = &f.data {
                        validate_fs(
                            data,
                            &task_symtab,
                            &host_lib,
                            &path_field(&f_path, "data"),
                            errors,
                        );
                    }
                    // `filename` is a plain string per the 2023-09 schema
                    // (not @fmtstring) — no format-string validation.
                }
            }

            // EXPR-only: comprehension variable validation
            if expr_active {
                let mut all_let_names: HashSet<String> = HashSet::new();
                if let Some(bindings) = &step.let_bindings {
                    for b in bindings {
                        if let Some(eq) = b.find('=') {
                            all_let_names.insert(b[..eq].trim().to_string());
                        }
                    }
                }
                if let Some(bindings) = &script.let_bindings {
                    for b in bindings {
                        if let Some(eq) = b.find('=') {
                            all_let_names.insert(b[..eq].trim().to_string());
                        }
                    }
                }
                if !all_let_names.is_empty() {
                    if let Err(e) = script
                        .actions
                        .on_run
                        .command
                        .validate_comprehension_vars(&all_let_names)
                    {
                        errors.add(&path_field(&action_path, "command"), e.to_string());
                    }
                    if let Some(args) = &script.actions.on_run.args {
                        let args_path = path_field(&action_path, "args");
                        for (j, arg) in args.iter().enumerate() {
                            if let Err(e) = arg.validate_comprehension_vars(&all_let_names) {
                                errors.add(&path_index(&args_path, j), e.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Step environments: session scope + step let bindings
        // (timeouts: template scope, via step_template_symtab)
        if let Some(envs) = &step.step_environments {
            let envs_path = path_field(&step_path, "stepEnvironments");
            for (j, env) in envs.iter().enumerate() {
                let mut env_symtab = build_session_scope_symtab(
                    jt.parameter_definitions.as_deref(),
                    env,
                    true,
                    expr_active,
                );
                // Copy step-level let binding values (already evaluated with inferred types)
                if expr_active {
                    if let Some(bindings) = &step.let_bindings {
                        for b in bindings {
                            if let Some(eq) = b.find('=') {
                                let name = b[..eq].trim();
                                if !name.is_empty() {
                                    if let Some(
                                        openjd_expr::symbol_table::SymbolTableEntry::Value(val),
                                    ) = task_symtab.get(name)
                                    {
                                        let _ = env_symtab.set(name, val.clone());
                                    }
                                }
                            }
                        }
                    }
                }
                // Env script let bindings: validate and evaluate into the
                // symtab if EXPR, reject if not.
                if let Some(script) = &env.script {
                    if let Some(bindings) = &script.let_bindings {
                        let env_let_path =
                            path_field(&path_field(&path_index(&envs_path, j), "script"), "let");
                        if !expr_active {
                            errors.add(&env_let_path, "'let' requires the EXPR extension.");
                        } else {
                            let enclosing: HashSet<String> = step
                                .let_bindings
                                .as_ref()
                                .map(|bs| {
                                    bs.iter()
                                        .filter_map(|b| {
                                            b.find('=').map(|eq| b[..eq].trim().to_string())
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            let mut env_let_names = HashSet::new();
                            validate_let_bindings(
                                bindings,
                                &env_let_path,
                                &enclosing,
                                &mut env_let_names,
                                &mut env_symtab,
                                &host_lib,
                                &host_profile,
                                errors,
                            );
                        }
                    }
                }
                // `step_template_symtab` carries Job.Name, Step.Name, and
                // step-level let bindings — all resolved at job creation, so
                // legitimately visible to env `timeout`/`notifyPeriodInSeconds`.
                validate_env_format_strings(
                    env,
                    &env_symtab,
                    &host_lib,
                    &step_template_symtab,
                    &template_lib,
                    &path_index(&envs_path, j),
                    expr_active,
                    errors,
                );
            }
        }

        // SimpleAction let bindings (requires both FB1 and EXPR)
        if expr_active {
            for sa in [
                &step.bash,
                &step.python,
                &step.cmd,
                &step.powershell,
                &step.node,
            ]
            .into_iter()
            .flatten()
            {
                let mut sa_let_names: HashSet<String> = HashSet::new();
                if let Some(bindings) = &step.let_bindings {
                    for b in bindings {
                        if let Some(eq) = b.find('=') {
                            sa_let_names.insert(b[..eq].trim().to_string());
                        }
                    }
                }
                if let Some(let_bindings) = &sa.let_bindings {
                    if ctx.profile.has_extension(ModelExtension::FeatureBundle1) {
                        let enclosing: HashSet<String> = step
                            .let_bindings
                            .as_ref()
                            .map(|bs| {
                                bs.iter()
                                    .filter_map(|b| {
                                        b.find('=').map(|eq| b[..eq].trim().to_string())
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        let mut new_names = HashSet::new();
                        validate_let_bindings(
                            let_bindings,
                            &step_path,
                            &enclosing,
                            &mut new_names,
                            &mut task_symtab,
                            &host_lib,
                            &host_profile,
                            errors,
                        );
                        sa_let_names.extend(new_names);
                    }
                }
                if !sa_let_names.is_empty() {
                    match FormatString::new(&sa.script) {
                        Ok(fs) => {
                            if let Err(e) = fs.validate_comprehension_vars(&sa_let_names) {
                                errors.add(&step_path, e.to_string());
                            }
                        }
                        Err(e) => {
                            errors.add(&step_path, format!("SimpleAction script: {e}"));
                        }
                    }
                    if let Some(args) = &sa.args {
                        for arg in args {
                            if let Err(e) = arg.validate_comprehension_vars(&sa_let_names) {
                                errors.add(&step_path, e.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // Environment script comprehension validation (EXPR only)
    if expr_active {
        validate_env_comprehensions(&jt.job_environments, errors);
        for step in &jt.steps {
            validate_env_comprehensions(&step.step_environments, errors);
        }
    }
}

/// Format string validation for a standalone environment template.
///
/// Scope selection mirrors how job-template environments are validated and
/// follows the spec's `@fmtstring` stage annotations. The environment body —
/// `variables`, action `command`/`args`, embedded files (`@fmtstring[host]`) —
/// is validated in session scope: `Param.*`/`RawParam.*` come from the
/// template's own `parameterDefinitions`, `Session.*` and `Env.File.*` are
/// available, and `Job.Name` is added with EXPR (the environment runs inside
/// some job's session at runtime). Action `timeout` and
/// `notifyPeriodInSeconds` (plain `@fmtstring`, resolved at job creation) are
/// validated in template scope instead: no `Session.*`, no `Env.File.*`, no
/// host functions. `Step.Name` is available in neither scope — an environment
/// template is not attached to a step.
pub fn validate_format_strings_environment_template(
    et: &EnvironmentTemplate,
    ctx: &ValidationContext,
    errors: &mut ValidationErrors,
) {
    let expr_active = ctx.profile.has_extension(ModelExtension::Expr);
    let host_profile = ctx
        .profile
        .to_expr_profile(openjd_expr::HostContext::Unresolved);
    let host_lib = openjd_expr::FunctionLibrary::for_profile(&host_profile);
    let template_profile = ctx.profile.to_expr_profile(openjd_expr::HostContext::None);
    let template_lib = openjd_expr::FunctionLibrary::for_profile(&template_profile);

    let env = &et.environment;
    let env_path = vec![PathElement::Field("environment".into())];
    let mut env_symtab =
        build_session_scope_symtab(et.parameter_definitions.as_deref(), env, false, expr_active);
    // Job-creation-scope symtab for `timeout`/`notifyPeriodInSeconds`.
    let mut env_template_symtab = build_template_scope_symtab(et.parameter_definitions.as_deref());
    if expr_active {
        env_template_symtab
            .set("Job.Name", ExprValue::unresolved(ExprType::STRING))
            .expect("symtab");
    }

    // Env script let bindings: validate and evaluate into the symtab if EXPR,
    // reject if not. Same treatment as environments in a job template.
    if let Some(script) = &env.script {
        if let Some(bindings) = &script.let_bindings {
            let let_path = path_field(&path_field(&env_path, "script"), "let");
            if !expr_active {
                errors.add(&let_path, "'let' requires the EXPR extension.");
            } else {
                let mut env_let_names = HashSet::new();
                validate_let_bindings(
                    bindings,
                    &let_path,
                    &HashSet::new(),
                    &mut env_let_names,
                    &mut env_symtab,
                    &host_lib,
                    &host_profile,
                    errors,
                );
            }
        }
    }

    validate_env_format_strings(
        env,
        &env_symtab,
        &host_lib,
        &env_template_symtab,
        &template_lib,
        &env_path,
        expr_active,
        errors,
    );

    // Comprehension loop-variable validation (EXPR only)
    if expr_active {
        validate_single_env_comprehensions(env, errors);
    }
}

/// Validate format strings within an environment (variables + script actions).
/// When `expr_active` is false, complex expressions (anything beyond a bare
/// `{{Name.Path}}` reference) are rejected — the base 2023-09 grammar only
/// permits dotted-name references.
///
/// Two symbol tables are needed because an environment mixes resolution
/// stages: `command`/`args`/`variables`/embedded-file fields are
/// `@fmtstring[host]` (session scope — `symtab`/`lib`), while `timeout` and
/// `notifyPeriodInSeconds` are plain `@fmtstring` (job-creation scope —
/// `template_symtab`/`template_lib`, no Session.*, no Env.File.*, no host
/// functions).
#[allow(clippy::too_many_arguments)]
fn validate_env_format_strings(
    env: &Environment,
    symtab: &SymbolTable,
    lib: &FunctionLibrary,
    template_symtab: &SymbolTable,
    template_lib: &FunctionLibrary,
    path: &[PathElement],
    expr_active: bool,
    errors: &mut ValidationErrors,
) {
    if let Some(vars) = &env.variables {
        let vars_path = path_field(path, "variables");
        for (name, value) in vars {
            let var_path = path_field(&vars_path, name);
            if !expr_active && value.has_complex_expressions() {
                errors.add(&var_path, "complex expressions require the EXPR extension.");
            }
            validate_fs(value, symtab, lib, &var_path, errors);
        }
    }
    if let Some(script) = &env.script {
        let script_path = path_field(path, "script");
        let actions_path = path_field(&script_path, "actions");
        if !expr_active {
            for (name, action) in script.actions.iter_named() {
                let action_path = path_field(&actions_path, name);
                if action.command.has_complex_expressions() {
                    errors.add(
                        &path_field(&action_path, "command"),
                        "complex expressions require the EXPR extension.",
                    );
                }
                if let Some(args) = &action.args {
                    let args_path = path_field(&action_path, "args");
                    for (j, arg) in args.iter().enumerate() {
                        if arg.has_complex_expressions() {
                            errors.add(
                                &path_index(&args_path, j),
                                "complex expressions require the EXPR extension.",
                            );
                        }
                    }
                }
            }
        }
        if let Some(action) = &script.actions.on_enter {
            validate_action_fs(
                action,
                symtab,
                lib,
                &path_field(&actions_path, "onEnter"),
                errors,
            );
        }
        // RFC 0008: all three wrap hooks see `WrappedAction.*`. `onWrapEnvEnter`
        // and `onWrapEnvExit` additionally see `WrappedEnv.Name`; `onWrapTaskRun`
        // additionally sees `WrappedStep.Name`. Referencing these outside the
        // permitted hook surfaces as a normal "Undefined variable" error.
        for (hook_name, action_opt, extra) in script.actions.wrap_hooks() {
            if let Some(action) = action_opt {
                let mut st = symtab.clone();
                add_wrapped_action_scope(&mut st);
                match extra {
                    WrapHookScope::EnvName => add_wrapped_env_name_scope(&mut st),
                    WrapHookScope::StepName => add_wrapped_step_name_scope(&mut st),
                }
                validate_action_fs(
                    action,
                    &st,
                    lib,
                    &path_field(&actions_path, hook_name),
                    errors,
                );
            }
        }
        if let Some(action) = &script.actions.on_exit {
            validate_action_fs(
                action,
                symtab,
                lib,
                &path_field(&actions_path, "onExit"),
                errors,
            );
        }
        // Timeout, cancelation mode (DeferredMode), and
        // notifyPeriodInSeconds on env actions are @fmtstring fields. On
        // the plain lifecycle actions they resolve at job creation, before
        // any session exists, so they validate against the template-scope
        // symtab. On the RFC 0008 wrap hooks they resolve at run time with
        // the `WrappedAction.*` variables seeded — that is what makes
        // round-trip forwarding (`timeout: "{{WrappedAction.Timeout}}"`,
        // `mode: "{{WrappedAction.Cancelation.Mode}}"`) possible — so they
        // validate against the wrapped-action scope.
        let wrap_hook_names: [&str; 3] = ["onWrapEnvEnter", "onWrapTaskRun", "onWrapEnvExit"];
        for (name, action) in script.actions.iter_named() {
            let action_path = path_field(&actions_path, name);
            let scoped_symtab: SymbolTable;
            let field_symtab: &SymbolTable = if wrap_hook_names.contains(&name) {
                let mut st = template_symtab.clone();
                add_wrapped_action_scope(&mut st);
                scoped_symtab = st;
                &scoped_symtab
            } else {
                template_symtab
            };
            if let Some(timeout) = &action.timeout {
                validate_fs(
                    timeout,
                    field_symtab,
                    template_lib,
                    &path_field(&action_path, "timeout"),
                    errors,
                );
            }
            let (mode_fs, notify_fs) = match &action.cancelation {
                Some(CancelationMode::NotifyThenTerminate {
                    notify_period_in_seconds,
                }) => (None, notify_period_in_seconds.as_ref()),
                Some(CancelationMode::DeferredMode {
                    mode,
                    notify_period_in_seconds,
                }) => (Some(mode), notify_period_in_seconds.as_ref()),
                _ => (None, None),
            };
            if let Some(mode) = mode_fs {
                validate_fs(
                    mode,
                    field_symtab,
                    template_lib,
                    &path_field(&action_path, "cancelation"),
                    errors,
                );
            }
            if let Some(notify) = notify_fs {
                validate_fs(
                    notify,
                    field_symtab,
                    template_lib,
                    &path_field(&action_path, "cancelation"),
                    errors,
                );
            }
        }
        if let Some(files) = &script.embedded_files {
            let files_path = path_field(&script_path, "embeddedFiles");
            for (j, f) in files.iter().enumerate() {
                let f_path = path_index(&files_path, j);
                if let Some(data) = &f.data {
                    let data_path = path_field(&f_path, "data");
                    if !expr_active && data.has_complex_expressions() {
                        errors.add(
                            &data_path,
                            "complex expressions require the EXPR extension.",
                        );
                    }
                    validate_fs(data, symtab, lib, &data_path, errors);
                }
                // `filename` is a plain string per the 2023-09 schema
                // (not @fmtstring) — no format-string validation.
            }
        }
    }
}

/// Augment a session-scope symtab with `WrappedAction.*`, available in
/// all three wrap hooks (RFC 0008):
///
/// - `WrappedAction.Command` — string
/// - `WrappedAction.Args` — list[string]
/// - `WrappedAction.Environment` — list[string] (entries of the form `"KEY=value"`)
/// - `WrappedAction.Timeout` — int? (seconds, or `null` when the wrapped
///   action specified no timeout)
/// - `WrappedAction.Cancelation.Mode` — string? (`"TERMINATE"`,
///   `"NOTIFY_THEN_TERMINATE"`, or `null` when the wrapped action defines
///   no `<Cancelation>`)
/// - `WrappedAction.Cancelation.NotifyPeriodInSeconds` — int? (the
///   effective grace period when the mode is `NOTIFY_THEN_TERMINATE`;
///   `null` for `TERMINATE` or when no `<Cancelation>` is defined)
///
/// The caller has already cloned the session symtab, so we mutate in place.
fn add_wrapped_action_scope(symtab: &mut SymbolTable) {
    for (name, ty) in [
        ("WrappedAction.Command", ExprType::STRING),
        ("WrappedAction.Args", ExprType::list(ExprType::STRING)),
        (
            "WrappedAction.Environment",
            ExprType::list(ExprType::STRING),
        ),
        (
            "WrappedAction.Timeout",
            ExprType::union(vec![ExprType::INT, ExprType::NULLTYPE]),
        ),
        (
            "WrappedAction.Cancelation.Mode",
            ExprType::union(vec![ExprType::STRING, ExprType::NULLTYPE]),
        ),
        (
            "WrappedAction.Cancelation.NotifyPeriodInSeconds",
            ExprType::union(vec![ExprType::INT, ExprType::NULLTYPE]),
        ),
    ] {
        symtab.set(name, ExprValue::unresolved(ty)).expect("symtab");
    }
}

/// Augment with `WrappedEnv.Name`, available only in `onWrapEnvEnter` and
/// `onWrapEnvExit` (RFC 0008).
fn add_wrapped_env_name_scope(symtab: &mut SymbolTable) {
    symtab
        .set("WrappedEnv.Name", ExprValue::unresolved(ExprType::STRING))
        .expect("symtab");
}

/// Augment with `WrappedStep.Name`, available only in `onWrapTaskRun`
/// (RFC 0008).
fn add_wrapped_step_name_scope(symtab: &mut SymbolTable) {
    symtab
        .set("WrappedStep.Name", ExprValue::unresolved(ExprType::STRING))
        .expect("symtab");
}

fn validate_env_comprehensions(envs: &Option<Vec<Environment>>, errors: &mut ValidationErrors) {
    if let Some(envs) = envs {
        for env in envs {
            validate_single_env_comprehensions(env, errors);
        }
    }
}

fn validate_single_env_comprehensions(env: &Environment, errors: &mut ValidationErrors) {
    if let Some(script) = &env.script {
        let mut env_let_names: HashSet<String> = HashSet::new();
        if let Some(bindings) = &script.let_bindings {
            for b in bindings {
                if let Some(eq) = b.find('=') {
                    env_let_names.insert(b[..eq].trim().to_string());
                }
            }
        }
        if !env_let_names.is_empty() {
            let path: Vec<PathElement> = vec![];
            for action in script.actions.iter_actions() {
                if let Err(e) = action.command.validate_comprehension_vars(&env_let_names) {
                    errors.add(&path, e.to_string());
                }
                if let Some(args) = &action.args {
                    for arg in args {
                        if let Err(e) = arg.validate_comprehension_vars(&env_let_names) {
                            errors.add(&path, e.to_string());
                        }
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_let_bindings(
    bindings: &[String],
    path: &[PathElement],
    enclosing_names: &HashSet<String>,
    out_names: &mut HashSet<String>,
    symtab: &mut SymbolTable,
    lib: &FunctionLibrary,
    profile: &openjd_expr::ExprProfile,
    errors: &mut ValidationErrors,
) {
    if bindings.is_empty() {
        errors.add(path, "if provided, must not be empty.");
        return;
    }
    if bindings.len() > 50 {
        errors.add(path, "must not contain more than 50 bindings.");
    }
    let mut names = HashSet::new();
    for (i, binding) in bindings.iter().enumerate() {
        let b_path = path_index(path, i);
        let Some(eq_pos) = binding.find('=') else {
            errors.add(&b_path, format!("missing '=' in '{binding}'."));
            continue;
        };
        let name = binding[..eq_pos].trim();
        let expr = binding[eq_pos + 1..].trim();
        if name.is_empty() {
            errors.add(&b_path, "has empty name.");
            continue;
        }
        if expr.is_empty() {
            errors.add(
                &b_path,
                format!("binding '{name}' has no expression after '='."),
            );
            continue;
        }
        let first = name.chars().next().unwrap();
        if !first.is_ascii_lowercase() && first != '_' {
            errors.add(
                &b_path,
                format!("name '{name}' must start with lowercase letter or underscore."),
            );
        }
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            errors.add(
                &b_path,
                format!("name '{name}' contains invalid characters."),
            );
        }
        if !names.insert(name.to_string()) {
            errors.add(&b_path, format!("duplicate name '{name}'."));
        }
        if enclosing_names.contains(name) {
            errors.add(&b_path, format!("'{name}' shadows enclosing scope."));
        }
        out_names.insert(name.to_string());

        // Evaluate the expression for type checking (Phase 1: static type check).
        // The prefix is included in error messages so caret positions align with
        // the full binding string, matching Python's behavior.
        let expr_start =
            eq_pos + 1 + binding[eq_pos + 1..].len() - binding[eq_pos + 1..].trim_start().len();
        let prefix = &binding[..expr_start];
        match ParsedExpression::with_profile(expr, profile) {
            Ok(parsed) => {
                // Check self-reference using the parsed AST's accessed symbols
                // rather than heuristic regex matching on the raw expression string.
                if parsed.accessed_symbols().contains(name) {
                    errors.add(&b_path, format!("'{name}' references itself."));
                }
                match parsed.with_library(lib).evaluate(&[symtab as &SymbolTable]) {
                    Ok(result) => {
                        // Set the binding in the symtab with its inferred value/type
                        // so subsequent bindings and format strings see the correct type.
                        let _ = symtab.set(name, result);
                    }
                    Err(e) => {
                        errors.add(
                            &b_path,
                            format!(
                                "Invalid expression in let binding '{name}': {}",
                                e.message_with_expr_prefix(prefix)
                            ),
                        );
                        // Still add as unresolved(ANY) so later bindings don't cascade errors
                        let _ = symtab.set(name, ExprValue::unresolved(ExprType::ANY));
                    }
                }
            }
            Err(e) => {
                errors.add(
                    &b_path,
                    format!(
                        "Invalid expression in let binding '{name}': {}",
                        e.message_with_expr_prefix(prefix)
                    ),
                );
                let _ = symtab.set(name, ExprValue::unresolved(ExprType::ANY));
            }
        }
    }
}
