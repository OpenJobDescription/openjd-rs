// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Step and environment instantiation — converting template types to job types.

use openjd_expr::format_string::copy_symbol_value;
use openjd_expr::path_mapping::PathFormat;
use openjd_expr::symbol_table::SymbolTable;

use crate::error::ModelError;
use crate::job;
use crate::template;
use crate::template::validate_v2023_09::EffectiveLimits;
use openjd_expr::ExpressionError;

use super::ranges;

/// Instantiate a StepTemplate into a Step.
pub(super) fn instantiate_step(
    st: &template::StepTemplate,
    symtab: &SymbolTable,
    has_expr: bool,
    limits: &EffectiveLimits,
) -> Result<job::Step, ModelError> {
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
                                ModelError::Expression(ExpressionError::new(format!(
                                    "let binding '{name}': {e}"
                                )))
                            })?;
                        let val = parsed
                            .with_path_format(PathFormat::Posix)
                            .with_library(&lib)
                            .evaluate(&[&step_symtab as &SymbolTable])
                            .map_err(|e| {
                                ModelError::Expression(ExpressionError::new(format!(
                                    "let binding '{name}': {e}"
                                )))
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
                // require session-time path mapping). Add them as Unresolved with
                // the correct type so script-level let bindings can reference them
                // for type-checking.
                if let Some(raw_param_table) = step_symtab.get_table("RawParam") {
                    for name in raw_param_table.keys() {
                        let param_key = format!("Param.{name}");
                        if !step_symtab.contains(&param_key) {
                            // Derive the Param type from the RawParam value: if it's
                            // a list, use list(PATH); otherwise use PATH.
                            let raw_key = format!("RawParam.{name}");
                            let unresolved_type = match step_symtab.get_value(&raw_key) {
                                Some(
                                    openjd_expr::ExprValue::ListPath(..)
                                    | openjd_expr::ExprValue::ListString(..),
                                ) => openjd_expr::ExprType::list(openjd_expr::ExprType::PATH),
                                _ => openjd_expr::ExprType::PATH,
                            };
                            let _ = check_symtab.set(
                                &param_key,
                                openjd_expr::ExprValue::Unresolved(unresolved_type),
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
                            crate::template::TaskParameterDefinition::CHUNK_INT(_) => {
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
                                    ModelError::Expression(ExpressionError::new(format!(
                                        "script let binding '{name}': {e}"
                                    )))
                                })?;
                            let val = parsed
                                .with_path_format(PathFormat::Posix)
                                .with_library(&lib)
                                .evaluate(&[&check_symtab as &SymbolTable])
                                .map_err(|e| {
                                    ModelError::Expression(ExpressionError::new(format!(
                                        "script let binding '{name}': {e}"
                                    )))
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
) -> Result<job::HostRequirements, ModelError> {
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
                .collect::<Result<Vec<_>, ModelError>>()
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
                .collect::<Result<Vec<_>, ModelError>>()
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
// The host only evaluates host-context (SESSION/TASK scope) format strings,
// so we filter the full symbol table down to exactly the symbols those
// format strings reference.
//
// Step and Environment have different sets of host-context format strings:
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
        if let Some(job::CancelationMode::NotifyThenTerminate {
            notify_period_in_seconds: Some(n),
        }) = &s.actions.on_run.cancelation
        {
            collect_from_fs(n, &mut symbols);
        }
        if let Some(files) = &s.embedded_files {
            for f in files {
                if let Some(d) = &f.data {
                    collect_from_fs(d, &mut symbols);
                }
                if let Some(n) = &f.filename {
                    collect_from_fs(n, &mut symbols);
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
                for action in [&es.actions.on_enter, &es.actions.on_exit]
                    .into_iter()
                    .flatten()
                {
                    collect_from_action(action, &mut symbols);
                }
                if let Some(files) = &es.embedded_files {
                    for f in files {
                        if let Some(d) = &f.data {
                            collect_from_fs(d, &mut symbols);
                        }
                        if let Some(n) = &f.filename {
                            collect_from_fs(n, &mut symbols);
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
    let symbols = collect_env_accessed_symbols(env);
    include_raw_param_fallbacks(&symbols, full, &mut filtered);
    filtered
}

/// Collect all symbol names accessed by an environment's host-context format strings.
fn collect_env_accessed_symbols(env: &job::Environment) -> std::collections::HashSet<String> {
    let mut symbols = std::collections::HashSet::new();
    if let Some(vars) = &env.variables {
        for fs in vars.values() {
            symbols.extend(fs.accessed_symbols());
        }
    }
    if let Some(es) = &env.script {
        for action in [&es.actions.on_enter, &es.actions.on_exit]
            .into_iter()
            .flatten()
        {
            symbols.extend(action.command.accessed_symbols());
            if let Some(args) = &action.args {
                for fs in args {
                    symbols.extend(fs.accessed_symbols());
                }
            }
            if let Some(t) = &action.timeout {
                symbols.extend(t.accessed_symbols());
            }
        }
        if let Some(files) = &es.embedded_files {
            for f in files {
                if let Some(d) = &f.data {
                    symbols.extend(d.accessed_symbols());
                }
                if let Some(n) = &f.filename {
                    symbols.extend(n.accessed_symbols());
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
