// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Pass 6: Structural validation.
//!
//! Validates template structure using EffectiveRules.

use std::collections::{HashMap, HashSet};

use super::helpers::*;
use super::EffectiveRules;
use crate::capabilities;
use crate::error::{path_field, path_index, PathElement, ValidationErrors};
use crate::template::*;
use crate::types::ValidationContext;

pub fn validate_structure(
    jt: &JobTemplate,
    limits: &super::EffectiveLimits,
    rules: &EffectiveRules,
    ctx: &ValidationContext,
    errors: &mut ValidationErrors,
) {
    let root: Vec<PathElement> = vec![];

    // Template-level
    if jt.steps.is_empty() {
        errors.add(&root, "must have at least one step.");
    }
    // Caller-imposed step count limit
    if let Some(max) = ctx.caller_limits.max_step_count {
        if jt.steps.len() > max {
            errors.add(
                &path_field(&root, "steps"),
                format!(
                    "exceeds caller limit of {} steps ({} steps).",
                    max,
                    jt.steps.len()
                ),
            );
        }
    }
    if jt.name.raw().is_empty() {
        errors.add(&path_field(&root, "name"), "must not be empty.");
    }
    if jt.name.raw().chars().any(|c| c.is_control()) {
        errors.add(&path_field(&root, "name"), "contains control characters.");
    }

    // Empty extensions list is rejected early in parse.rs (pass 4).

    if let Some(desc) = &jt.description {
        let dp = path_field(&root, "description");
        if desc.0.chars().count() > limits.max_description_len {
            errors.add(
                &dp,
                format!("exceeds {} characters.", limits.max_description_len),
            );
        }
        if has_control_chars(&desc.0) {
            errors.add(&dp, "contains control characters.");
        }
    }

    // Parameter definitions
    if let Some(params) = &jt.parameter_definitions {
        let pd_path = path_field(&root, "parameterDefinitions");
        if params.is_empty() {
            errors.add(&pd_path, "if provided, must contain at least one element.");
        }
        let mut names = HashSet::new();
        for (i, p) in params.iter().enumerate() {
            let p_path = path_index(&pd_path, i);
            if !names.insert(p.name().to_string()) {
                errors.add(&p_path, format!("duplicate parameter name: '{}'", p.name()));
            }
            // Check type is allowed
            if !rules.allowed_job_param_types.contains(&p.job_param_type()) {
                errors.add(
                    &p_path,
                    format!("parameter type '{}' is not allowed.", p.type_name()),
                );
            }
            if let Err(param_errors) = p.validate_definition(limits) {
                for e in param_errors {
                    errors.add(&p_path, e);
                }
            }
        }
    }

    // Environment name uniqueness across all environments
    let mut env_names = HashSet::new();
    if let Some(envs) = &jt.job_environments {
        let envs_path = path_field(&root, "jobEnvironments");
        if envs.is_empty() {
            errors.add(&envs_path, "must not be empty.");
        }
        for (i, env) in envs.iter().enumerate() {
            if !env_names.insert(env.name.clone()) {
                errors.add(
                    &path_index(&envs_path, i),
                    format!("duplicate environment name: '{}'", env.name),
                );
            }
        }
    }
    for (i, step) in jt.steps.iter().enumerate() {
        if let Some(envs) = &step.step_environments {
            let envs_path = path_field(
                &[PathElement::Field("steps".into()), PathElement::Index(i)],
                "stepEnvironments",
            );
            if envs.is_empty() {
                errors.add(&envs_path, "must not be empty.");
            }
            for (j, env) in envs.iter().enumerate() {
                if !env_names.insert(env.name.clone()) {
                    errors.add(
                        &path_index(&envs_path, j),
                        format!("duplicate environment name: '{}'", env.name),
                    );
                }
            }
        }
    }

    // Caller-imposed total environment count limit
    if let Some(max) = ctx.caller_limits.max_env_count {
        if env_names.len() > max {
            errors.add(
                &root,
                format!(
                    "total environments ({}) exceeds caller limit of {}.",
                    env_names.len(),
                    max
                ),
            );
        }
    }

    // Step validation
    let all_step_names: HashSet<String> = jt.steps.iter().map(|s| s.name.clone()).collect();
    let mut step_names = HashSet::new();
    for (i, step) in jt.steps.iter().enumerate() {
        let step_path = vec![PathElement::Field("steps".into()), PathElement::Index(i)];
        let name = step.name.clone();
        if !step_names.insert(name.clone()) {
            errors.add(
                &path_field(&step_path, "name"),
                format!("duplicate step name: '{name}'"),
            );
        }
        if name.is_empty() {
            errors.add(&path_field(&step_path, "name"), "must not be empty.");
        }
        if name.chars().any(|c| c.is_control()) {
            errors.add(
                &path_field(&step_path, "name"),
                "contains control characters.",
            );
        }
        if name.contains("{{") {
            errors.add(
                &path_field(&step_path, "name"),
                "must not contain format string expressions.",
            );
        }
        if let Some(desc) = &step.description {
            let dp = path_field(&step_path, "description");
            if desc.0.chars().count() > limits.max_description_len {
                errors.add(
                    &dp,
                    format!("exceeds {} characters.", limits.max_description_len),
                );
            }
            if has_control_chars(&desc.0) {
                errors.add(&dp, "contains control characters.");
            }
        }
        // Must have script or SimpleAction
        if step.script.is_none()
            && step.bash.is_none()
            && step.python.is_none()
            && step.cmd.is_none()
            && step.powershell.is_none()
            && step.node.is_none()
        {
            errors.add(&step_path, "must have 'script' or a simple action field.");
        }

        // Dependencies
        if let Some(deps) = &step.dependencies {
            let deps_path = path_field(&step_path, "dependencies");
            if deps.is_empty() {
                errors.add(&deps_path, "must not be empty.");
            }
            let mut dep_names = HashSet::new();
            for (j, dep) in deps.iter().enumerate() {
                let dep_path = path_index(&deps_path, j);
                if dep.depends_on == step.name {
                    errors.add(&dep_path, "cannot depend on itself.");
                }
                if !step_names.contains(&dep.depends_on)
                    && !all_step_names.contains(&dep.depends_on)
                {
                    errors.add(
                        &dep_path,
                        format!("dependency '{}' not found.", dep.depends_on),
                    );
                }
                if !dep_names.insert(&dep.depends_on) {
                    errors.add(
                        &dep_path,
                        format!("duplicate dependency '{}'.", dep.depends_on),
                    );
                }
            }
        }

        // Host requirements
        if let Some(hr) = &step.host_requirements {
            let hr_path = path_field(&step_path, "hostRequirements");
            match (
                capabilities::standard_amount_capability_names(ctx.revision, &ctx.extensions),
                capabilities::standard_attribute_capability_names(ctx.revision, &ctx.extensions),
            ) {
                (Ok(std_amounts), Ok(std_attrs)) => {
                    validate_host_requirements(
                        hr,
                        &hr_path,
                        rules,
                        std_amounts,
                        &std_attrs,
                        errors,
                    );
                }
                _ => {
                    let ext_list: Vec<_> = ctx.extensions.iter().map(|e| e.as_str()).collect();
                    errors.add(
                        &hr_path,
                        format!(
                            "cannot validate: no capability definitions for revision {} with extensions {:?}.",
                            ctx.revision, ext_list
                        ),
                    );
                }
            }
        }

        // Parameter space
        if let Some(ps) = &step.parameter_space {
            validate_step_param_space(
                ps,
                &path_field(&step_path, "parameterSpace"),
                limits,
                rules,
                errors,
            );
        }

        // Script actions
        if let Some(script) = &step.script {
            let script_path = path_field(&step_path, "script");
            let action_path = path_field(&path_field(&script_path, "actions"), "onRun");
            validate_action(&script.actions.on_run, &action_path, limits, rules, errors);

            // Task.File.* references
            let file_names: HashSet<String> = script
                .embedded_files
                .as_ref()
                .map(|files| files.iter().map(|f| f.name.clone()).collect())
                .unwrap_or_default();
            let all_fs: Vec<&openjd_expr::FormatString> = {
                let mut v = vec![&script.actions.on_run.command];
                if let Some(args) = &script.actions.on_run.args {
                    v.extend(args.iter());
                }
                v
            };
            for fs in &all_fs {
                for name in fs.expression_names() {
                    if let Some(file_name) = name.strip_prefix("Task.File.") {
                        if !file_names.contains(file_name) {
                            errors.add(
                                &script_path,
                                format!("references undefined embedded file '{file_name}'."),
                            );
                        }
                    }
                }
            }

            // Embedded files
            if let Some(files) = &script.embedded_files {
                let files_path = path_field(&script_path, "embeddedFiles");
                if files.is_empty() {
                    errors.add(&files_path, "must not be empty.");
                }
                validate_embedded_files(files, &files_path, errors);
            }
        }
    }

    // Cycle detection
    detect_dependency_cycles(&jt.steps, errors);

    // Environments
    if let Some(envs) = &jt.job_environments {
        let envs_path = path_field(&root, "jobEnvironments");
        for (i, env) in envs.iter().enumerate() {
            validate_single_environment(env, limits, rules, &path_index(&envs_path, i), errors);
        }
    }
    for (i, step) in jt.steps.iter().enumerate() {
        if let Some(envs) = &step.step_environments {
            let envs_path = path_field(
                &[PathElement::Field("steps".into()), PathElement::Index(i)],
                "stepEnvironments",
            );
            for (j, env) in envs.iter().enumerate() {
                validate_single_environment(env, limits, rules, &path_index(&envs_path, j), errors);
            }
        }
    }
}

pub fn validate_single_environment(
    env: &Environment,
    limits: &super::EffectiveLimits,
    rules: &EffectiveRules,
    path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    if env.script.is_none() && env.variables.is_none() {
        errors.add(path, "must have at least one of 'script' or 'variables'.");
    }
    let name_path = path_field(path, "name");
    if env.name.is_empty() {
        errors.add(&name_path, "must not be empty.");
    }
    if env.name.len() > limits.max_env_name_len {
        errors.add(
            &name_path,
            format!("exceeds {} characters.", limits.max_env_name_len),
        );
    }
    if env.name.chars().any(|c| c.is_control()) {
        errors.add(&name_path, "contains control characters.");
    }

    if let Some(vars) = &env.variables {
        let vars_path = path_field(path, "variables");
        if vars.is_empty() {
            errors.add(&vars_path, "if provided, must not be empty.");
        }
        for (name, value) in vars {
            let var_path = path_field(&vars_path, name);
            validate_env_var_name(name, &var_path, errors);
            if value.raw().chars().count() > limits.max_description_len {
                errors.add(
                    &var_path,
                    format!("value exceeds {} characters.", limits.max_description_len),
                );
            }
        }
    }

    if let Some(script) = &env.script {
        let script_path = path_field(path, "script");
        let actions_path = path_field(&script_path, "actions");
        if script.actions.on_enter.is_none() {
            errors.add(&actions_path, "onEnter is required.");
        }
        if let Some(action) = &script.actions.on_enter {
            validate_action(
                action,
                &path_field(&actions_path, "onEnter"),
                limits,
                rules,
                errors,
            );
        }
        if let Some(action) = &script.actions.on_exit {
            validate_action(
                action,
                &path_field(&actions_path, "onExit"),
                limits,
                rules,
                errors,
            );
        }
        if let Some(files) = &script.embedded_files {
            let files_path = path_field(&script_path, "embeddedFiles");
            if files.is_empty() {
                errors.add(&files_path, "must not be empty.");
            }
            validate_embedded_files(files, &files_path, errors);
        }
    }
}

fn validate_action(
    action: &Action,
    path: &[PathElement],
    limits: &super::EffectiveLimits,
    rules: &EffectiveRules,
    errors: &mut ValidationErrors,
) {
    let cmd = action.command.raw();
    if cmd.is_empty() {
        errors.add(&path_field(path, "command"), "must not be empty.");
    }
    if cmd.len() > limits.max_command_len {
        errors.add(
            &path_field(path, "command"),
            format!("exceeds {} characters.", limits.max_command_len),
        );
    }
    if cmd.chars().any(|c| c.is_control()) {
        errors.add(&path_field(path, "command"), "contains control characters.");
    }
    if let Some(args) = &action.args {
        let args_path = path_field(path, "args");
        if args.is_empty() {
            errors.add(&args_path, "if provided, must not be empty.");
        }
        for (i, arg) in args.iter().enumerate() {
            if arg
                .raw()
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\r')
            {
                errors.add(&path_index(&args_path, i), "contains control characters.");
            }
        }
    }
    if let Some(CancelationMode::NotifyThenTerminate {
        notify_period_in_seconds: Some(period),
    }) = &action.cancelation
    {
        let raw = period.raw().trim();
        if !period.has_complex_expressions() && !raw.contains("{{") {
            match raw.parse::<i64>() {
                Ok(v) if v <= 0 => errors.add(path, "notifyPeriodInSeconds must be > 0."),
                Ok(v) if v > 600 => errors.add(path, "notifyPeriodInSeconds must not exceed 600."),
                Ok(_) => {}
                Err(_) => errors.add(path, "notifyPeriodInSeconds must be a positive integer."),
            }
        } else if !rules.allow_fmtstring_in_numeric_fields {
            errors.add(
                path,
                "format strings in notifyPeriodInSeconds are not allowed.",
            );
        }
    }
    if let Some(timeout) = &action.timeout {
        let raw = timeout.raw().trim();
        if !timeout.has_complex_expressions() && !raw.contains("{{") {
            match raw.parse::<i64>() {
                Ok(v) if v <= 0 => errors.add(path, "timeout must be > 0."),
                Ok(_) => {}
                Err(_) => errors.add(path, "timeout must be a positive integer."),
            }
        } else if !rules.allow_fmtstring_in_numeric_fields {
            errors.add(path, "format strings in timeout are not allowed.");
        }
    }
}

fn validate_host_requirements(
    hr: &HostRequirements,
    path: &[PathElement],
    rules: &EffectiveRules,
    standard_amounts: &[&str],
    standard_attrs: &[&str],
    errors: &mut ValidationErrors,
) {
    let has_amounts = hr.amounts.as_ref().is_some_and(|a| !a.is_empty());
    let has_attrs = hr.attributes.as_ref().is_some_and(|a| !a.is_empty());
    if !has_amounts && !has_attrs {
        errors.add(path, "must have at least one of amounts or attributes.");
    }
    let total =
        hr.amounts.as_ref().map_or(0, |a| a.len()) + hr.attributes.as_ref().map_or(0, |a| a.len());
    if total > 50 {
        errors.add(path, "total amounts + attributes must not exceed 50.");
    }

    if let Some(amounts) = &hr.amounts {
        let amounts_path = path_field(path, "amounts");
        let mut names = HashSet::new();
        for (i, amt) in amounts.iter().enumerate() {
            let amt_path = path_index(&amounts_path, i);
            if !names.insert(amt.name.to_lowercase()) {
                errors.add(&amt_path, format!("duplicate amount name '{}'.", amt.name));
            }
            if amt.name.len() > 100 {
                errors.add(
                    &amt_path,
                    format!("name '{}' exceeds 100 characters.", amt.name),
                );
            }
            if !AMOUNT_CAP_RE.is_match(&amt.name) {
                errors.add(
                    &amt_path,
                    format!(
                        "name '{}' does not match capability name pattern.",
                        amt.name
                    ),
                );
            }
            check_capability_reserved_scope(&amt.name, standard_amounts, &amt_path, errors);
            if amt.min.is_none() && amt.max.is_none() {
                errors.add(&amt_path, "must have at least one of min or max.");
            }
            for (field, fs) in [("min", &amt.min), ("max", &amt.max)] {
                if let Some(fs) = fs {
                    if (fs.has_complex_expressions() || fs.raw().contains("{{"))
                        && !rules.allow_fmtstring_in_numeric_fields
                    {
                        errors.add(
                            &path_field(&amt_path, field),
                            "format strings are not allowed.",
                        );
                    }
                }
            }
            let min_val = amt
                .min
                .as_ref()
                .and_then(|fs| fs.raw().trim().parse::<f64>().ok());
            let max_val = amt
                .max
                .as_ref()
                .and_then(|fs| fs.raw().trim().parse::<f64>().ok());
            if let Some(min) = min_val {
                if min < 0.0 {
                    errors.add(&path_field(&amt_path, "min"), "must be non-negative.");
                }
            }
            if let Some(max) = max_val {
                if max <= 0.0 {
                    errors.add(&path_field(&amt_path, "max"), "must be positive.");
                }
            }
            if let (Some(min), Some(max)) = (min_val, max_val) {
                if min > max {
                    errors.add(&amt_path, format!("min ({min}) > max ({max})."));
                }
            }
        }
    }

    if let Some(attrs) = &hr.attributes {
        let attrs_path = path_field(path, "attributes");
        let mut names = HashSet::new();
        for (i, attr) in attrs.iter().enumerate() {
            let attr_path = path_index(&attrs_path, i);
            if !names.insert(attr.name.to_lowercase()) {
                errors.add(
                    &attr_path,
                    format!("duplicate attribute name '{}'.", attr.name),
                );
            }
            if attr.name.len() > 100 {
                errors.add(
                    &attr_path,
                    format!("name '{}' exceeds 100 characters.", attr.name),
                );
            }
            if !ATTR_CAP_RE.is_match(&attr.name) {
                errors.add(
                    &attr_path,
                    format!(
                        "name '{}' does not match capability name pattern.",
                        attr.name
                    ),
                );
            }
            check_capability_reserved_scope(&attr.name, standard_attrs, &attr_path, errors);
            if attr.any_of.is_none() && attr.all_of.is_none() {
                errors.add(&attr_path, "must have at least one of anyOf or allOf.");
            }
            for (field, vals) in [("anyOf", &attr.any_of), ("allOf", &attr.all_of)] {
                if let Some(vals) = vals {
                    let field_path = path_field(&attr_path, field);
                    if vals.is_empty() {
                        errors.add(&field_path, "must not be empty.");
                    }
                    if vals.len() > 50 {
                        errors.add(&field_path, "exceeds 50 elements.");
                    }
                    for (j, v) in vals.iter().enumerate() {
                        let v_path = path_index(&field_path, j);
                        let s = v.raw();
                        if s.is_empty() {
                            errors.add(&v_path, "must not be empty.");
                        }
                        if s.len() > 100 {
                            errors.add(&v_path, "exceeds 100 characters.");
                        }
                        if !s.is_empty() && !ATTR_VALUE_RE.is_match(s) && v.is_literal() {
                            errors.add(
                                &v_path,
                                format!("value '{}' contains invalid characters.", s),
                            );
                        }
                    }
                }
            }
            // Standard capability value checks
            let attr_lower = attr.name.to_lowercase();
            let is_single_valued =
                attr_lower == "attr.worker.os.family" || attr_lower == "attr.worker.cpu.arch";
            if is_single_valued {
                if let Some(vals) = &attr.all_of {
                    if vals.len() > 1 {
                        errors.add(
                            &path_field(&attr_path, "allOf"),
                            "single-valued attribute cannot have more than 1 element.",
                        );
                    }
                }
            }
            if attr_lower == "attr.worker.os.family" {
                let valid = ["linux", "windows", "macos"];
                for (field, vals) in [("anyOf", &attr.any_of), ("allOf", &attr.all_of)] {
                    if let Some(vals) = vals {
                        for v in vals {
                            if v.is_literal()
                                && !valid.iter().any(|vv| vv.eq_ignore_ascii_case(v.raw()))
                            {
                                errors.add(
                                    &path_field(&attr_path, field),
                                    format!(
                                        "value '{}' is not valid for attr.worker.os.family.",
                                        v.raw()
                                    ),
                                );
                            }
                        }
                    }
                }
            }
            if attr_lower == "attr.worker.cpu.arch" {
                let valid = ["x86_64", "arm64"];
                for (field, vals) in [("anyOf", &attr.any_of), ("allOf", &attr.all_of)] {
                    if let Some(vals) = vals {
                        for v in vals {
                            if v.is_literal()
                                && !valid.iter().any(|vv| vv.eq_ignore_ascii_case(v.raw()))
                            {
                                errors.add(
                                    &path_field(&attr_path, field),
                                    format!(
                                        "value '{}' is not valid for attr.worker.cpu.arch.",
                                        v.raw()
                                    ),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

fn validate_step_param_space(
    space: &StepParameterSpaceDefinition,
    path: &[PathElement],
    limits: &super::EffectiveLimits,
    rules: &EffectiveRules,
    errors: &mut ValidationErrors,
) {
    let tpd_path = path_field(path, "taskParameterDefinitions");
    if space.task_parameter_definitions.is_empty() {
        errors.add(&tpd_path, "must not be empty.");
    }
    if space.task_parameter_definitions.len() > 16 {
        errors.add(&tpd_path, "exceeds 16 elements.");
    }
    let mut names = HashSet::new();
    for (i, p) in space.task_parameter_definitions.iter().enumerate() {
        let p_path = path_index(&tpd_path, i);
        if !names.insert(p.name().to_string()) {
            errors.add(
                &p_path,
                format!("duplicate task parameter name '{}'.", p.name()),
            );
        }
        // Check type is allowed
        if !rules
            .allowed_task_param_types
            .contains(&p.task_param_type())
        {
            errors.add(
                &p_path,
                format!(
                    "task parameter type '{}' is not allowed.",
                    p.task_param_type()
                ),
            );
        }
        validate_task_param_range(p, &p_path, limits, errors);
    }
    if let Some(comb) = &space.combination {
        validate_combination_expr(comb, &names, &path_field(path, "combination"), errors);
    }
}

fn validate_combination_expr(
    expr: &str,
    param_names: &HashSet<String>,
    path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    for ch in expr.chars() {
        if !ch.is_alphanumeric()
            && ch != '_'
            && ch != '*'
            && ch != '('
            && ch != ')'
            && ch != ','
            && ch != ' '
        {
            errors.add(path, format!("contains disallowed character '{ch}'."));
            return;
        }
    }
    if expr.len() > 1280 {
        errors.add(path, "exceeds 1280 characters.");
    }
    let mut depth = 0i32;
    for ch in expr.chars() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    errors.add(path, "unmatched ')'.");
                    return;
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        errors.add(path, "unmatched '('.");
        return;
    }

    // Tokenize into names and operators
    #[derive(Debug, PartialEq)]
    enum Token {
        Name(String),
        Star,
        LParen,
        RParen,
        Comma,
    }
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in expr.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            current.push(ch);
        } else {
            if !current.is_empty() {
                tokens.push(Token::Name(current.clone()));
                current.clear();
            }
            match ch {
                '*' => tokens.push(Token::Star),
                '(' => tokens.push(Token::LParen),
                ')' => tokens.push(Token::RParen),
                ',' => tokens.push(Token::Comma),
                ' ' => {}
                _ => {}
            }
        }
    }
    if !current.is_empty() {
        tokens.push(Token::Name(current));
    }

    if tokens.is_empty() {
        errors.add(path, "combination expression is empty.");
        return;
    }

    // Structural checks
    let mut prev_was_name = false;
    let mut all_names: Vec<String> = Vec::new();

    for (i, tok) in tokens.iter().enumerate() {
        match tok {
            Token::Name(name) => {
                if prev_was_name {
                    errors.add(path, "missing operator between parameters.");
                    return;
                }
                if !param_names.contains(name) {
                    errors.add(path, format!("references unknown parameter '{name}'."));
                }
                all_names.push(name.clone());
                prev_was_name = true;
            }
            Token::Star => {
                if !(prev_was_name || i > 0 && matches!(tokens.get(i - 1), Some(Token::RParen))) {
                    errors.add(path, "operator '*' without left operand.");
                    return;
                }
                prev_was_name = false;
            }
            Token::LParen => {
                prev_was_name = false;
            }
            Token::RParen => {
                if !prev_was_name {
                    errors.add(path, "empty group in combination expression.");
                    return;
                }
                prev_was_name = true; // ) acts like a name for operator adjacency
            }
            Token::Comma => {
                if !prev_was_name {
                    errors.add(path, "empty element in combination expression.");
                    return;
                }
                prev_was_name = false;
            }
        }
    }
    // Check trailing operator
    if let Some(last) = tokens.last() {
        if matches!(last, Token::Star | Token::Comma) {
            errors.add(path, "trailing operator in combination expression.");
        }
    }

    // Check duplicate references
    let mut seen = HashSet::new();
    for name in &all_names {
        if !seen.insert(name.clone()) {
            errors.add(
                path,
                format!("parameter '{name}' appears more than once in combination."),
            );
        }
    }

    // Check all params are referenced
    let found_set: HashSet<String> = all_names.into_iter().collect();
    for name in param_names {
        if !found_set.contains(name) {
            errors.add(
                path,
                format!("parameter '{name}' missing from combination expression."),
            );
        }
    }
}

fn validate_embedded_files(
    files: &[EmbeddedFile],
    path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    let mut names = HashSet::new();
    for (i, f) in files.iter().enumerate() {
        let f_path = path_index(path, i);
        if !names.insert(&f.name) {
            errors.add(
                &f_path,
                format!("duplicate embedded file name '{}'.", f.name),
            );
        }
        if f.name.is_empty()
            || !f
                .name
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
            || !f.name.chars().all(|c| c.is_alphanumeric() || c == '_')
        {
            errors.add(
                &path_field(&f_path, "name"),
                format!("'{}' is not a valid identifier.", f.name),
            );
        }
        if f.data.is_none() {
            errors.add(
                &f_path,
                format!("embedded file '{}' is missing 'data' field.", f.name),
            );
        }
        if let Some(data) = &f.data {
            if data.raw().is_empty() {
                errors.add(&path_field(&f_path, "data"), "must not be empty.");
            }
        }
        if let Some(filename) = &f.filename {
            let fname = filename.raw();
            if fname.is_empty() {
                errors.add(&path_field(&f_path, "filename"), "must not be empty.");
            }
            if fname.contains('/') || fname.contains('\\') {
                errors.add(
                    &path_field(&f_path, "filename"),
                    "must not contain path separators.",
                );
            }
        }
    }
}

fn detect_dependency_cycles(steps: &[StepTemplate], errors: &mut ValidationErrors) {
    let step_names: HashSet<String> = steps.iter().map(|s| s.name.clone()).collect();
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for step in steps {
        let name = step.name.clone();
        let deps: Vec<String> = step
            .dependencies
            .as_ref()
            .map(|d| d.iter().map(|dep| dep.depends_on.clone()).collect())
            .unwrap_or_default();
        adj.insert(name, deps);
    }
    // Kahn's algorithm
    let mut in_degree: HashMap<String, usize> = HashMap::new();
    for name in &step_names {
        in_degree.insert(name.clone(), 0);
    }
    for deps in adj.values() {
        for dep in deps {
            if let Some(d) = in_degree.get_mut(dep) {
                *d += 1;
            }
        }
    }
    let mut queue: Vec<String> = in_degree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(n, _)| n.clone())
        .collect();
    let mut visited = 0;
    while let Some(node) = queue.pop() {
        visited += 1;
        if let Some(deps) = adj.get(&node) {
            for dep in deps {
                if let Some(d) = in_degree.get_mut(dep) {
                    *d -= 1;
                    if *d == 0 {
                        queue.push(dep.clone());
                    }
                }
            }
        }
    }
    if visited < step_names.len() {
        errors.add(&[], "step dependencies contain a cycle.");
    }
}

fn validate_task_param_range(
    p: &TaskParameterDefinition,
    path: &[PathElement],
    limits: &super::EffectiveLimits,
    errors: &mut ValidationErrors,
) {
    match p {
        TaskParameterDefinition::INT(tp) => match &tp.range {
            IntRange::List(items) => {
                if items.is_empty() {
                    errors.add(
                        path,
                        format!("INT parameter '{}' range must not be empty.", tp.name),
                    );
                }
                if items.len() > limits.max_task_param_range_len {
                    errors.add(
                        path,
                        format!(
                            "INT parameter '{}' range exceeds {} elements.",
                            tp.name, limits.max_task_param_range_len
                        ),
                    );
                }
            }
            IntRange::Expression(expr) => {
                let raw = expr.raw();
                if !raw.contains("{{") {
                    match raw.parse::<openjd_expr::RangeExpr>() {
                        Ok(range) => {
                            if range.len() > limits.max_task_param_range_len {
                                errors.add(path, format!("INT parameter '{}' range expression expands to {} elements (max {}).", tp.name, range.len(), limits.max_task_param_range_len));
                            }
                        }
                        Err(e) => errors.add(
                            path,
                            format!("INT parameter '{}' range expression error: {e}", tp.name),
                        ),
                    }
                }
            }
        },
        TaskParameterDefinition::FLOAT(tp) => match &tp.range {
            FloatRange::List(items) => {
                if items.is_empty() {
                    errors.add(
                        path,
                        format!("FLOAT parameter '{}' range must not be empty.", tp.name),
                    );
                }
                if items.len() > limits.max_task_param_range_len {
                    errors.add(
                        path,
                        format!(
                            "FLOAT parameter '{}' range exceeds {} elements.",
                            tp.name, limits.max_task_param_range_len
                        ),
                    );
                }
            }
            FloatRange::Expression(_) => {}
        },
        TaskParameterDefinition::STRING(tp) => {
            if let StringRange::List(items) = &tp.range {
                if items.is_empty() {
                    errors.add(
                        path,
                        format!("STRING parameter '{}' range must not be empty.", tp.name),
                    );
                }
                if items.len() > limits.max_task_param_range_len {
                    errors.add(
                        path,
                        format!(
                            "STRING parameter '{}' range exceeds {} elements.",
                            tp.name, limits.max_task_param_range_len
                        ),
                    );
                }
            }
        }
        TaskParameterDefinition::PATH(tp) => {
            if let StringRange::List(items) = &tp.range {
                if items.is_empty() {
                    errors.add(
                        path,
                        format!("PATH parameter '{}' range must not be empty.", tp.name),
                    );
                }
                if items.len() > limits.max_task_param_range_len {
                    errors.add(
                        path,
                        format!(
                            "PATH parameter '{}' range exceeds {} elements.",
                            tp.name, limits.max_task_param_range_len
                        ),
                    );
                }
                for (i, item) in items.iter().enumerate() {
                    if item.raw().is_empty() {
                        errors.add(
                            path,
                            format!("PATH parameter '{}' range[{i}] must not be empty.", tp.name),
                        );
                    }
                }
            }
        }
        TaskParameterDefinition::CHUNK_INT(tp) => match &tp.range {
            IntRange::List(items) => {
                if items.is_empty() {
                    errors.add(
                        path,
                        format!(
                            "CHUNK[INT] parameter '{}' range must not be empty.",
                            tp.name
                        ),
                    );
                }
                if items.len() > limits.max_task_param_range_len {
                    errors.add(
                        path,
                        format!(
                            "CHUNK[INT] parameter '{}' range exceeds {} elements.",
                            tp.name, limits.max_task_param_range_len
                        ),
                    );
                }
            }
            IntRange::Expression(expr) => {
                if !expr.raw().contains("{{") {
                    if let Err(e) = expr.raw().parse::<openjd_expr::RangeExpr>() {
                        errors.add(
                            path,
                            format!(
                                "CHUNK[INT] parameter '{}' range expression error: {e}",
                                tp.name
                            ),
                        );
                    }
                }
            }
        },
    }
}
