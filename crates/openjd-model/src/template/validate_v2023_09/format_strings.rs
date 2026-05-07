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
use crate::types::{KnownExtension, ValidationContext};

/// Build a symbol table containing Param/RawParam entries from job parameter definitions.
/// RawParam.* is always STRING for PATH types and LIST_STRING for LIST_PATH types,
/// matching Python behavior where RawParam holds the raw unprocessed value.
fn build_param_symtab(jt: &JobTemplate) -> SymbolTable {
    use crate::types::JobParameterType;
    let mut symtab = SymbolTable::new();
    if let Some(params) = &jt.parameter_definitions {
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

/// Build the template-scope symtab (for job name, host requirements, parameter space ranges).
/// PATH and LIST[PATH] Param.* are excluded (host-only); RawParam.* is always STRING for path types.
fn build_template_scope_symtab(jt: &JobTemplate) -> SymbolTable {
    use crate::types::JobParameterType;
    let mut symtab = SymbolTable::new();
    if let Some(params) = &jt.parameter_definitions {
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
fn build_session_scope_symtab(
    jt: &JobTemplate,
    env: &Environment,
    is_step_env: bool,
    expr_active: bool,
) -> SymbolTable {
    let mut symtab = build_param_symtab(jt);
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
    let mut symtab = build_param_symtab(jt);

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

    // Task.File.* from step script embedded files
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
    let expr_active = ctx.has_extension(KnownExtension::Expr);
    // Template/task-range validation uses HostContext::None (host functions
    // are not available in those scopes). Session/task scopes use
    // HostContext::Unresolved so apply_path_mapping type-checks.
    let default_lib = openjd_expr::FunctionLibrary::for_profile(
        &ctx.to_expr_profile(openjd_expr::HostContext::None),
    );
    let host_lib = openjd_expr::FunctionLibrary::for_profile(
        &ctx.to_expr_profile(openjd_expr::HostContext::Unresolved),
    );

    // ── Job name: template scope (Param/RawParam only) ──
    let template_symtab = build_template_scope_symtab(jt);
    validate_fs(
        &jt.name,
        &template_symtab,
        &default_lib,
        &path_field(&[], "name"),
        errors,
    );

    // ── Host requirements: template scope + step let bindings ──
    for (i, step) in jt.steps.iter().enumerate() {
        let step_path = vec![PathElement::Field("steps".into()), PathElement::Index(i)];
        if let Some(hr) = &step.host_requirements {
            // Build a symtab with Param/RawParam + step let bindings
            let mut hr_symtab = build_template_scope_symtab(jt);
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
                        &default_lib,
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
                            &default_lib,
                            &path_field(&amt_path, "min"),
                            errors,
                        );
                    }
                    if let Some(max) = &amt.max {
                        validate_fs(
                            max,
                            &hr_symtab,
                            &default_lib,
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
                                &default_lib,
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
                                &default_lib,
                                &path_index(&path_field(&attr_path, "allOf"), k),
                                errors,
                            );
                        }
                    }
                }
            }
        }
    }

    // ── Job environments: session scope ──
    if let Some(envs) = &jt.job_environments {
        let envs_path = path_field(&[], "jobEnvironments");
        for (i, env) in envs.iter().enumerate() {
            let mut env_symtab = build_session_scope_symtab(jt, env, false, expr_active);
            // Evaluate env script let bindings
            if expr_active {
                if let Some(script) = &env.script {
                    if let Some(bindings) = &script.let_bindings {
                        let let_path =
                            path_field(&path_field(&path_index(&envs_path, i), "script"), "let");
                        let mut env_let_names = HashSet::new();
                        validate_let_bindings(
                            bindings,
                            &let_path,
                            &HashSet::new(),
                            &mut env_let_names,
                            &mut env_symtab,
                            &host_lib,
                            errors,
                        );
                    }
                }
            }
            validate_env_format_strings(
                env,
                &env_symtab,
                &host_lib,
                &path_index(&envs_path, i),
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
            let mut range_symtab = build_template_scope_symtab(jt);
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
                                match openjd_expr::eval::ParsedExpression::new(expr_str) {
                                    Ok(parsed) => {
                                        match parsed
                                            .with_path_format(PathFormat::Posix)
                                            .with_library(&default_lib)
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
                                &default_lib,
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
                                    &default_lib,
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
                                    &default_lib,
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
                                &default_lib,
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

        // Let bindings: validate and evaluate into symtab if EXPR, reject if not.
        // Step-level let bindings are TEMPLATE scope (default_lib, no Session.*, no PATH Param.*).
        // Script-level let bindings are TASK scope (host_lib).
        if let Some(bindings) = &step.let_bindings {
            let let_path = path_field(&step_path, "let");
            if !expr_active {
                errors.add(&let_path, "'let' requires the EXPR extension.");
            } else {
                // Build a template-scope symtab for step let bindings:
                // Param.* (non-PATH), RawParam.*, Job.Name, Step.Name — no Session.*, no Task.*
                let mut step_let_symtab = build_template_scope_symtab(jt);
                step_let_symtab
                    .set("Job.Name", ExprValue::unresolved(ExprType::STRING))
                    .expect("symtab");
                step_let_symtab
                    .set("Step.Name", ExprValue::unresolved(ExprType::STRING))
                    .expect("symtab");
                let mut step_let_names = HashSet::new();
                validate_let_bindings(
                    bindings,
                    &let_path,
                    &HashSet::new(),
                    &mut step_let_names,
                    &mut step_let_symtab,
                    &default_lib,
                    errors,
                );
                // Copy evaluated let bindings into task_symtab so script-level code can use them
                for name in &step_let_names {
                    if let Some(val) = step_let_symtab.get_value(name) {
                        let _ = task_symtab.set(name, val.clone());
                    }
                }
            }
        }

        if let Some(script) = &step.script {
            let script_path = path_field(&step_path, "script");

            // Script-level let bindings (TASK scope — host_lib)
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

            // Timeout
            if let Some(timeout) = &script.actions.on_run.timeout {
                validate_fs(
                    timeout,
                    &task_symtab,
                    &host_lib,
                    &path_field(&action_path, "timeout"),
                    errors,
                );
            }
            if let Some(CancelationMode::NotifyThenTerminate {
                notify_period_in_seconds: Some(notify),
            }) = &script.actions.on_run.cancelation
            {
                validate_fs(
                    notify,
                    &task_symtab,
                    &host_lib,
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
                    if let Some(filename) = &f.filename {
                        validate_fs(
                            filename,
                            &task_symtab,
                            &host_lib,
                            &path_field(&f_path, "filename"),
                            errors,
                        );
                    }
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
        if let Some(envs) = &step.step_environments {
            let envs_path = path_field(&step_path, "stepEnvironments");
            for (j, env) in envs.iter().enumerate() {
                let mut env_symtab = build_session_scope_symtab(jt, env, true, expr_active);
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
                // Evaluate env script let bindings into env_symtab
                if let Some(script) = &env.script {
                    if let Some(bindings) = &script.let_bindings {
                        if expr_active {
                            let env_let_path = path_field(
                                &path_field(&path_index(&envs_path, j), "script"),
                                "let",
                            );
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
                                errors,
                            );
                        }
                    }
                }
                validate_env_format_strings(
                    env,
                    &env_symtab,
                    &host_lib,
                    &path_index(&envs_path, j),
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
                    if ctx.has_extension(KnownExtension::FeatureBundle1) {
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

/// Validate format strings within an environment (variables + script actions).
fn validate_env_format_strings(
    env: &Environment,
    symtab: &SymbolTable,
    lib: &FunctionLibrary,
    path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    if let Some(vars) = &env.variables {
        let vars_path = path_field(path, "variables");
        for (name, value) in vars {
            validate_fs(value, symtab, lib, &path_field(&vars_path, name), errors);
        }
    }
    if let Some(script) = &env.script {
        let script_path = path_field(path, "script");
        let actions_path = path_field(&script_path, "actions");
        if let Some(action) = &script.actions.on_enter {
            validate_action_fs(
                action,
                symtab,
                lib,
                &path_field(&actions_path, "onEnter"),
                errors,
            );
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
        if let Some(files) = &script.embedded_files {
            let files_path = path_field(&script_path, "embeddedFiles");
            for (j, f) in files.iter().enumerate() {
                let f_path = path_index(&files_path, j);
                if let Some(data) = &f.data {
                    validate_fs(data, symtab, lib, &path_field(&f_path, "data"), errors);
                }
                if let Some(filename) = &f.filename {
                    validate_fs(
                        filename,
                        symtab,
                        lib,
                        &path_field(&f_path, "filename"),
                        errors,
                    );
                }
            }
        }
    }
}

fn validate_env_comprehensions(envs: &Option<Vec<Environment>>, errors: &mut ValidationErrors) {
    if let Some(envs) = envs {
        for env in envs {
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
                    for action in [&script.actions.on_enter, &script.actions.on_exit]
                        .into_iter()
                        .flatten()
                    {
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
    }
}

fn validate_let_bindings(
    bindings: &[String],
    path: &[PathElement],
    enclosing_names: &HashSet<String>,
    out_names: &mut HashSet<String>,
    symtab: &mut SymbolTable,
    lib: &FunctionLibrary,
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
        match ParsedExpression::new(expr) {
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
