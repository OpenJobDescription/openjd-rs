// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Step and environment instantiation — converting template types to job types.

use openjd_expr::format_string::copy_symbol_value;
use openjd_expr::path_mapping::PathFormat;
use openjd_expr::symbol_table::SymbolTable;

use crate::error::OpenJdError;
use crate::job;
use crate::template;
use crate::template::validate_v2023_09::EffectiveLimits;

use super::ranges;

/// Instantiate a StepTemplate into a Step.
pub(super) fn instantiate_step(
    st: &template::StepTemplate,
    symtab: &SymbolTable,
    has_expr: bool,
    limits: &EffectiveLimits,
) -> Result<job::Step, OpenJdError> {
    let mut step_symtab = symtab.clone();

    let step_name = st.name.clone();

    if has_expr {
        step_symtab.set(
            "Step.Name",
            openjd_expr::ExprValue::String(step_name.clone()),
        )?;
    }

    // Evaluate step-level let bindings (TEMPLATE scope — no PATH Param.*, no host context)
    if has_expr {
        if let Some(bindings) = &st.let_bindings {
            let lib = openjd_expr::default_library::get_default_library().clone();
            for binding in bindings {
                if let Some(eq_pos) = binding.find('=') {
                    let name = binding[..eq_pos].trim();
                    let expr = binding[eq_pos + 1..].trim();
                    if !name.is_empty() && !expr.is_empty() {
                        let parsed =
                            openjd_expr::eval::ParsedExpression::new(expr).map_err(|e| {
                                OpenJdError::Expression(format!("let binding '{name}': {e}"))
                            })?;
                        let symtabs = [&step_symtab as &SymbolTable];
                        let mut evaluator = parsed
                            .evaluator(&symtabs)
                            .with_path_format(PathFormat::Posix)
                            .with_library(&lib);
                        let val = evaluator.evaluate(&parsed.ast).map_err(|e| {
                            OpenJdError::Expression(format!("let binding '{name}': {e}"))
                        })?;
                        step_symtab.set(name, val)?;
                    }
                }
            }
        }
    }

    let script_template = st.resolve_syntax_sugar()?.or_else(|| st.script.clone());
    let script = script_template.as_ref().map(convert_step_script);

    // Type-check script-level let bindings with unresolved host context
    if has_expr {
        if let Some(s) = &script_template {
            if let Some(bindings) = &s.let_bindings {
                let mut check_symtab = step_symtab.clone();

                // PATH Param.* are excluded from the template-scope symtab (they
                // require session-time path mapping). Add them as Unresolved(PATH)
                // so script-level let bindings can reference them for type-checking.
                if let Some(raw_param_table) = step_symtab.get_table("RawParam") {
                    for name in raw_param_table.keys() {
                        let param_key = format!("Param.{name}");
                        if !step_symtab.contains(&param_key) {
                            let _ = check_symtab.set(
                                &param_key,
                                openjd_expr::ExprValue::Unresolved(openjd_expr::ExprType::PATH),
                            );
                        }
                    }
                }

                let _ = check_symtab.set(
                    "Session.WorkingDirectory",
                    openjd_expr::ExprValue::Unresolved(openjd_expr::ExprType::PATH),
                );
                let _ = check_symtab.set(
                    "Session.HasPathMappingRules",
                    openjd_expr::ExprValue::Unresolved(openjd_expr::ExprType::BOOL),
                );
                let _ = check_symtab.set(
                    "Session.PathMappingRulesFile",
                    openjd_expr::ExprValue::Unresolved(openjd_expr::ExprType::PATH),
                );

                if let Some(ps) = &st.parameter_space {
                    for tp in &ps.task_parameter_definitions {
                        let tp_type = match tp {
                            crate::template::TaskParameterDefinition::INT(_) => {
                                openjd_expr::ExprType::INT
                            }
                            crate::template::TaskParameterDefinition::ChunkInt(_) => {
                                openjd_expr::ExprType::RANGE_EXPR
                            }
                            crate::template::TaskParameterDefinition::FLOAT(_) => {
                                openjd_expr::ExprType::FLOAT
                            }
                            crate::template::TaskParameterDefinition::STRING(_) => {
                                openjd_expr::ExprType::STRING
                            }
                            crate::template::TaskParameterDefinition::PATH(_) => {
                                openjd_expr::ExprType::PATH
                            }
                        };
                        let _ = check_symtab.set(
                            &format!("Task.Param.{}", tp.name()),
                            openjd_expr::ExprValue::Unresolved(tp_type.clone()),
                        );
                        let raw_type = match tp {
                            crate::template::TaskParameterDefinition::PATH(_) => {
                                openjd_expr::ExprType::STRING
                            }
                            _ => tp_type,
                        };
                        let _ = check_symtab.set(
                            &format!("Task.RawParam.{}", tp.name()),
                            openjd_expr::ExprValue::Unresolved(raw_type),
                        );
                    }
                }

                if let Some(files) = &s.embedded_files {
                    for f in files {
                        let _ = check_symtab.set(
                            &format!("Task.File.{}", f.name),
                            openjd_expr::ExprValue::Unresolved(openjd_expr::ExprType::PATH),
                        );
                    }
                }

                let lib = openjd_expr::default_library::get_default_library()
                    .clone()
                    .with_unresolved_host_context();
                for binding in bindings {
                    if let Some(eq_pos) = binding.find('=') {
                        let name = binding[..eq_pos].trim();
                        let expr = binding[eq_pos + 1..].trim();
                        if !name.is_empty() && !expr.is_empty() {
                            let parsed =
                                openjd_expr::eval::ParsedExpression::new(expr).map_err(|e| {
                                    OpenJdError::Expression(format!(
                                        "script let binding '{name}': {e}"
                                    ))
                                })?;
                            let symtabs = [&check_symtab as &SymbolTable];
                            let mut evaluator = parsed
                                .evaluator(&symtabs)
                                .with_path_format(PathFormat::Posix)
                                .with_library(&lib);
                            let val = evaluator.evaluate(&parsed.ast).map_err(|e| {
                                OpenJdError::Expression(format!("script let binding '{name}': {e}"))
                            })?;
                            check_symtab.set(name, val)?;
                        }
                    }
                }
            }
        }
    }

    let host_requirements = st
        .host_requirements
        .as_ref()
        .map(|hr| resolve_host_requirements(hr, &step_symtab))
        .transpose()?;

    let parameter_space = st
        .parameter_space
        .as_ref()
        .map(|ps| ranges::resolve_parameter_space(ps, &step_symtab, limits))
        .transpose()?;

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
        OpenJdError::DecodeValidation("Step must have a script or SimpleAction".to_string())
    })?;
    let filtered_symtab = filter_symtab_for_step(
        &step_symtab,
        Some(&script),
        &step_environments,
        st.let_bindings.as_deref(),
    );

    Ok(job::Step {
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
    })
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

fn resolve_host_requirements(
    hr: &template::HostRequirements,
    symtab: &SymbolTable,
) -> Result<job::HostRequirements, OpenJdError> {
    let amounts = hr
        .amounts
        .as_ref()
        .map(|amts| {
            amts.iter()
                .map(|a| {
                    let min = a
                        .min
                        .as_ref()
                        .map(|fs| ranges::resolve_to_f64(fs, symtab, "hostRequirements amount min"))
                        .transpose()?;
                    let max = a
                        .max
                        .as_ref()
                        .map(|fs| ranges::resolve_to_f64(fs, symtab, "hostRequirements amount max"))
                        .transpose()?;
                    Ok(job::AmountRequirement {
                        name: a.name.clone(),
                        min,
                        max,
                    })
                })
                .collect::<Result<Vec<_>, OpenJdError>>()
        })
        .transpose()?;

    let attributes = hr
        .attributes
        .as_ref()
        .map(|attrs| {
            attrs
                .iter()
                .map(|a| {
                    let any_of = a
                        .any_of
                        .as_ref()
                        .map(|vals| ranges::resolve_string_list(vals, symtab))
                        .transpose()?;
                    let all_of = a
                        .all_of
                        .as_ref()
                        .map(|vals| ranges::resolve_string_list(vals, symtab))
                        .transpose()?;
                    Ok(job::AttributeRequirement {
                        name: a.name.clone(),
                        any_of,
                        all_of,
                    })
                })
                .collect::<Result<Vec<_>, OpenJdError>>()
        })
        .transpose()?;

    Ok(job::HostRequirements {
        amounts,
        attributes,
    })
}

/// Evaluate let bindings and return a new symbol table with bound values.
pub fn evaluate_let_bindings(
    bindings: &[String],
    symtab: &SymbolTable,
    library: Option<&openjd_expr::function_library::FunctionLibrary>,
    path_format: PathFormat,
) -> Result<SymbolTable, OpenJdError> {
    let mut result = symtab.clone();
    for binding in bindings {
        let eq_pos = binding.find('=').ok_or_else(|| {
            OpenJdError::Expression(format!("Missing '=' in let binding: {binding}"))
        })?;
        let name = binding[..eq_pos].trim();
        let expr = binding[eq_pos + 1..].trim();
        let prefix = &binding
            [..eq_pos + 1 + binding[eq_pos + 1..].len() - binding[eq_pos + 1..].trim_start().len()];
        let parsed = openjd_expr::ParsedExpression::new(expr).map_err(|e| {
            OpenJdError::Expression(format!(
                "Error evaluating let binding '{name}': {}",
                e.message_with_expr_prefix(prefix)
            ))
        })?;
        let symtabs = [&result as &SymbolTable];
        let mut evaluator = parsed.evaluator(&symtabs).with_path_format(path_format);
        if let Some(lib) = library {
            evaluator = evaluator.with_library(lib);
        }
        let value = evaluator.evaluate(&parsed.ast).map_err(|e| {
            OpenJdError::Expression(format!(
                "Error evaluating let binding '{name}': {}",
                e.message_with_expr_prefix(prefix)
            ))
        })?;
        result.set(name, value).map_err(|e| {
            OpenJdError::Expression(format!("Error setting let binding '{name}': {e}"))
        })?;
    }
    Ok(result)
}

// ── Symbol table filtering ──────────────────────────────────────────

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
        if let Some(job::CancelationMode::NotifyThenTerminate {
            notify_period_in_seconds: Some(n),
        }) = &s.actions.on_run.cancelation
        {
            n.copy_used_symtab_values(full, &mut filtered);
        }
        if let Some(files) = &s.embedded_files {
            for f in files {
                if let Some(d) = &f.data {
                    d.copy_used_symtab_values(full, &mut filtered);
                }
                if let Some(n) = &f.filename {
                    n.copy_used_symtab_values(full, &mut filtered);
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
                        if let Some(n) = &f.filename {
                            n.copy_used_symtab_values(full, &mut filtered);
                        }
                    }
                }
            }
        }
    }

    filtered
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
                for symbol in &parsed.accessed_symbols {
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
    for action in [&actions.on_enter, &actions.on_exit].into_iter().flatten() {
        action.command.copy_used_symtab_values(full, filtered);
        if let Some(args) = &action.args {
            for a in args {
                a.copy_used_symtab_values(full, filtered);
            }
        }
        if let Some(t) = &action.timeout {
            t.copy_used_symtab_values(full, filtered);
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
                if let Some(n) = &f.filename {
                    n.copy_used_symtab_values(full, &mut filtered);
                }
            }
        }
        if let Some(bindings) = &es.let_bindings {
            collect_let_binding_refs(bindings, full, &mut filtered);
        }
    }
    filtered
}
