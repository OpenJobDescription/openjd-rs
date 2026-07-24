// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Execution phases for the `openjd run` command.

use super::*;

pub(super) async fn execute(args: RunArgs) -> Result<(), RunError> {
    if args.verbose {
        log::set_max_level(log::LevelFilter::Debug);
    }

    let started_at = Instant::now();
    let _ = crate::SESSION_START.set(started_at);
    let _ = crate::TIMESTAMP_FORMAT.set(args.timestamp_format.clone());

    let prepared = prepare_run(&args)?;
    let selection = prepare_selection(&args, &prepared.job)?;
    validate_wrap_environment_stacks(
        &prepared.job,
        &prepared.env_templates,
        &selection.steps_to_run,
    )?;
    let env_template_symtab = openjd_model::build_symbol_table(&prepared.param_values)
        .map_err(|e| format!("Failed to build environment template symbol table: {e}"))?;

    let cancel_token = CancellationToken::new();
    let session = create_session(&args, &prepared, cancel_token.clone())?;
    let working_dir = session.working_directory().to_path_buf();
    let interrupted = install_signal_handler(cancel_token);
    let mut ctx = RunContext {
        session,
        entered_envs: Vec::new(),
        interrupted,
        started_at,
        tasks_run: 0,
        session_failed: false,
    };

    println!("{}\tSession start", ctx.timestamp());
    println!("{}\tRunning job '{}'", ctx.timestamp(), prepared.job.name);
    let execution_result = run_workload(
        &mut ctx,
        &args,
        &prepared.job,
        &prepared.env_templates,
        &env_template_symtab,
        &selection,
    )
    .await;

    // Once a Session exists, errors must not bypass environment cleanup.
    ctx.exit_environments_down_to(0).await;
    ctx.record_interruption();
    if let Err(e) = execution_result {
        if !args.preserve {
            ctx.session.cleanup();
        }
        return Err(e);
    }

    report_result(&mut ctx, &args, &prepared.job, &working_dir);
    if ctx.session_failed {
        std::process::exit(1);
    }
    Ok(())
}

fn prepare_run(args: &RunArgs) -> Result<PreparedRun, RunError> {
    let path = &args.path;
    let content = crate::common::read_input_file(path)?;
    let template_value = parse::document_string_to_object(
        &content,
        crate::common::document_type(path),
        &openjd_model::CallerLimits::default(),
    )?;
    let exts = crate::common::parse_extensions(&args.extensions)?;
    let supported_exts: Vec<&str> = exts.iter().map(String::as_str).collect();
    let job_template = parse::decode_job_template(
        template_value,
        Some(&supported_exts),
        &openjd_model::CallerLimits::default(),
    )?;

    let mut env_templates = Vec::new();
    for env_path in &args.environments {
        let env_content = std::fs::read_to_string(env_path)?;
        let env_value = parse::document_string_to_object(
            &env_content,
            crate::common::document_type(env_path),
            &openjd_model::CallerLimits::default(),
        )?;
        env_templates.push(parse::decode_environment_template(
            env_value,
            Some(&supported_exts),
        )?);
    }

    let input_values = parse_cli_parameters(&args.parameters)?;
    let path_rules = load_path_mapping_rules(&args.path_mapping_rules)?;
    let canonical_path = std::fs::canonicalize(path)?;
    let job_template_dir = strip_extended_prefix(
        canonical_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new(".")),
    );
    let current_working_dir = strip_extended_prefix(&std::env::current_dir()?);
    let param_values = openjd_model::preprocess_job_parameters(
        &job_template,
        &input_values,
        &env_templates,
        &openjd_model::PathParameterOptions {
            job_template_dir: job_template_dir.to_str().unwrap_or("."),
            current_working_dir: current_working_dir.to_str().unwrap_or("."),
            path_format: openjd_expr::path_mapping::PathFormat::host(),
            allow_template_dir_walk_up: false,
            allow_uri_path_values: true,
        },
    )
    .map_err(|e| format!("{e}\n\n{}", crate::help::format_help(&job_template, path)))?;

    let mut profile_extensions = std::collections::HashSet::new();
    if let Some(ext_list) = &job_template.extensions {
        profile_extensions.extend(ext_list.iter().filter_map(|e| {
            e.as_str()
                .parse::<openjd_model::types::ModelExtension>()
                .ok()
        }));
    }
    let revision_profile = ModelProfile::new(openjd_model::types::SpecificationRevision::V2023_09)
        .with_extensions(profile_extensions);
    let revision_ctx =
        openjd_model::types::ValidationContext::from_profile(revision_profile.clone());
    let job = openjd_model::create_job(&job_template, &param_values, &revision_ctx)
        .map_err(|e| format!("{e}\n\n{}", crate::help::format_help(&job_template, path)))?;

    Ok(PreparedRun {
        job,
        env_templates,
        param_values,
        path_rules,
        revision_profile,
    })
}

fn prepare_selection(args: &RunArgs, job: &Job) -> Result<RunSelection, RunError> {
    let selected_step_idx = if let Some(step_name) = &args.step {
        Some(
            job.steps
                .iter()
                .position(|step| step.name == *step_name)
                .ok_or_else(|| {
                    format!(
                        "No Step with name '{}' is defined in the given Job Template.",
                        step_name
                    )
                })?,
        )
    } else if job.steps.len() == 1 {
        Some(0)
    } else {
        if !args.task_params.is_empty() || args.tasks.is_some() {
            return Err(format!(
                "Providing task parameters requires a specified step or a job with a single step.\n{} steps: {:?}.",
                job.steps.len(),
                job.steps.iter().map(|step| &step.name).collect::<Vec<_>>()
            )
            .into());
        }
        None
    };

    let explicit_task_params = if !args.task_params.is_empty() {
        Some(vec![parse_task_params(&args.task_params)?])
    } else if let Some(tasks_arg) = &args.tasks {
        Some(parse_tasks_arg(tasks_arg)?)
    } else {
        None
    };
    if explicit_task_params.is_some() {
        let selected_idx = selected_step_idx.ok_or(
            "Providing task parameters requires a specified step or a job with a single step.",
        )?;
        if job.steps[selected_idx].parameter_space.is_none() {
            let option = if args.task_params.is_empty() {
                "--tasks"
            } else {
                "--task-param"
            };
            return Err(format!(
                "Step '{}' does not define a parameterSpace; {option} cannot be used.",
                job.steps[selected_idx].name
            )
            .into());
        }
    }
    let steps_to_run = if let Some(selected_idx) = selected_step_idx {
        if args.run_dependencies {
            resolve_step_dependencies(job, selected_idx)
        } else {
            vec![selected_idx]
        }
    } else {
        StepDependencyGraph::new(job)?.topo_sorted()?
    };

    Ok(RunSelection {
        selected_step_idx,
        explicit_task_params,
        steps_to_run,
    })
}

fn validate_wrap_environment_stacks(
    job: &Job,
    env_templates: &[EnvironmentTemplate],
    steps_to_run: &[usize],
) -> Result<(), RunError> {
    let has_wrap_hook = |env: &Environment| {
        env.script
            .as_ref()
            .is_some_and(|script| script.actions.has_any_wrap_hook())
    };
    let mut base_wrap_envs: Vec<&str> = env_templates
        .iter()
        .filter(|template| {
            template
                .environment
                .script
                .as_ref()
                .is_some_and(|script| script.actions.has_any_wrap_hook())
        })
        .map(|template| template.environment.name.as_str())
        .collect();
    if let Some(job_envs) = &job.job_environments {
        base_wrap_envs.extend(
            job_envs
                .iter()
                .filter(|env| has_wrap_hook(env))
                .map(|env| env.name.as_str()),
        );
    }
    reject_multiple_wrap_environments(&base_wrap_envs)?;

    for &step_idx in steps_to_run {
        let mut stack_wrap_envs = base_wrap_envs.clone();
        if let Some(step_envs) = &job.steps[step_idx].step_environments {
            stack_wrap_envs.extend(
                step_envs
                    .iter()
                    .filter(|env| has_wrap_hook(env))
                    .map(|env| env.name.as_str()),
            );
        }
        reject_multiple_wrap_environments(&stack_wrap_envs)?;
    }
    Ok(())
}

fn reject_multiple_wrap_environments(names: &[&str]) -> Result<(), RunError> {
    if names.len() <= 1 {
        return Ok(());
    }
    Err(format!(
        "RFC 0008: a session may have at most one Environment defining wrap hooks \
         (onWrapEnvEnter / onWrapTaskRun / onWrapEnvExit). Found {}: {}.",
        names.len(),
        names.join(", ")
    )
    .into())
}

fn create_session(
    args: &RunArgs,
    prepared: &PreparedRun,
    cancel_token: CancellationToken,
) -> Result<Session, RunError> {
    let session_config = openjd_sessions::session::SessionConfig {
        session_id: format!("cli-{}", std::process::id()),
        job_parameter_values: prepared.param_values.clone(),
        path_mapping_rules: (!prepared.path_rules.is_empty()).then(|| prepared.path_rules.clone()),
        retain_working_dir: args.preserve,
        callback: None,
        os_env_vars: None,
        session_root_directory: None,
        user: None,
        profile: Some(prepared.revision_profile.clone()),
        cancel_token: Some(cancel_token),
        sticky_bit_policy: Default::default(),
        debug_collect_stdout: false,
        echo_openjd_directives: true,
    };
    Session::with_config(session_config)
        .map_err(|e| format!("Failed to create session: {e}").into())
}

fn install_signal_handler(cancel_token: CancellationToken) -> Arc<AtomicBool> {
    let interrupted = Arc::new(AtomicBool::new(false));
    let handler_interrupted = interrupted.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            let mut sigint =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                    .expect("failed to install SIGINT handler");
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("failed to install SIGTERM handler");
            tokio::select! {
                _ = sigint.recv() => {}
                _ = sigterm.recv() => {}
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        handler_interrupted.store(true, Ordering::SeqCst);
        cancel_token.cancel();
    });
    interrupted
}

async fn run_workload(
    ctx: &mut RunContext,
    args: &RunArgs,
    job: &Job,
    env_templates: &[EnvironmentTemplate],
    env_template_symtab: &openjd_expr::SymbolTable,
    selection: &RunSelection,
) -> Result<(), RunError> {
    for template in env_templates {
        if ctx.is_stopping() {
            break;
        }
        let env = openjd_model::convert_environment_with_symtab(
            &template.environment,
            Some(env_template_symtab),
        );
        ctx.enter_environment(&env, &template.environment.name, None)
            .await;
    }
    if let Some(job_envs) = &job.job_environments {
        for env in job_envs {
            if ctx.is_stopping() {
                break;
            }
            ctx.enter_environment(env, &env.name, None).await;
        }
    }

    for &step_idx in &selection.steps_to_run {
        if ctx.is_stopping() {
            break;
        }
        execute_step(ctx, args, &job.steps[step_idx], step_idx, selection).await?;
    }
    Ok(())
}

async fn execute_step(
    ctx: &mut RunContext,
    args: &RunArgs,
    step: &Step,
    step_idx: usize,
    selection: &RunSelection,
) -> Result<(), RunError> {
    println!("{}\tRunning step '{}'", ctx.timestamp(), step.name);
    let environment_baseline = ctx.entered_envs.len();
    if let Some(step_envs) = &step.step_environments {
        for env in step_envs {
            if ctx.is_stopping() {
                break;
            }
            ctx.enter_environment(env, &env.name, step.resolved_symtab.clone())
                .await;
        }
    }

    let result = if ctx.session_failed {
        Ok(())
    } else {
        execute_step_tasks(ctx, args, step, step_idx, selection).await
    };
    // This unwind happens before propagating task setup/runtime errors.
    ctx.exit_environments_down_to(environment_baseline).await;
    result
}

async fn execute_step_tasks(
    ctx: &mut RunContext,
    args: &RunArgs,
    step: &Step,
    step_idx: usize,
    selection: &RunSelection,
) -> Result<(), RunError> {
    let Some(parameter_space) = &step.parameter_space else {
        ctx.print_banner("Running Task");
        ctx.run_task(step, None).await?;
        return Ok(());
    };

    let mut iter = openjd_model::StepParameterSpaceIterator::new(parameter_space)?;
    if selection.selected_step_idx == Some(step_idx) {
        if let Some(task_param_sets) = selection.explicit_task_params.as_deref() {
            return execute_explicit_tasks(ctx, step, parameter_space, task_param_sets, &iter)
                .await;
        }
    }
    execute_parameter_space_tasks(ctx, args.maximum_tasks, step, &mut iter).await
}

async fn execute_explicit_tasks(
    ctx: &mut RunContext,
    step: &Step,
    parameter_space: &openjd_model::job::StepParameterSpace,
    task_param_sets: &[HashMap<String, String>],
    iter: &openjd_model::StepParameterSpaceIterator,
) -> Result<(), RunError> {
    let mut typed_sets = Vec::with_capacity(task_param_sets.len());
    for (index, params) in task_param_sets.iter().enumerate() {
        let values = coerce_task_params(params, parameter_space)?;
        iter.validate_containment(&values)
            .map_err(|e| format!("Task parameter set {index}: {e}"))?;
        typed_sets.push(values);
    }

    for (params, values) in task_param_sets.iter().zip(&typed_sets) {
        if ctx.is_stopping() {
            break;
        }
        ctx.print_banner("Running Task");
        println!("{}\tParameter values:", ctx.timestamp());
        for (name, value) in params {
            println!("{}\t{name} = {value}", ctx.timestamp());
        }
        ctx.run_task(step, Some(values)).await?;
    }
    Ok(())
}

async fn execute_parameter_space_tasks(
    ctx: &mut RunContext,
    maximum_tasks: i64,
    step: &Step,
    iter: &mut openjd_model::StepParameterSpaceIterator,
) -> Result<(), RunError> {
    let is_adaptive = iter.chunks_adaptive();
    let chunks_param_name = iter.chunks_parameter_name().map(String::from);
    let target_runtime_seconds = adaptive_target_runtime(step, is_adaptive);
    let mut completed_task_count = 0usize;
    let mut completed_task_duration = 0.0;
    let mut remaining_tasks = if maximum_tasks > 0 {
        maximum_tasks
    } else {
        i64::MAX
    };

    while remaining_tasks > 0 && !ctx.is_stopping() {
        let Some(task_params) = iter.next() else {
            break;
        };
        ctx.print_banner("Running Task");
        println!("{}\tParameter values:", ctx.timestamp());
        for (name, value) in &task_params {
            println!(
                "{}\t{}({}) = {}",
                ctx.timestamp(),
                name,
                value.param_type.as_spec_str(),
                value.value.to_display_string()
            );
        }
        let task_values: TaskParameterSet = task_params
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        let task_duration = ctx.run_task(step, Some(&task_values)).await?;
        remaining_tasks -= 1;

        if is_adaptive && !ctx.is_stopping() {
            let chunk_items = chunks_param_name
                .as_ref()
                .and_then(|name| task_params.get(name))
                .map(|value| match &value.value {
                    openjd_expr::ExprValue::RangeExpr(range) => range.len(),
                    _ => 1,
                })
                .unwrap_or(1);
            completed_task_count += chunk_items;
            completed_task_duration += task_duration;
            adjust_adaptive_chunk_size(
                ctx,
                iter,
                completed_task_count,
                completed_task_duration,
                target_runtime_seconds,
            );
        }
    }
    Ok(())
}

fn adaptive_target_runtime(step: &Step, is_adaptive: bool) -> f64 {
    if !is_adaptive {
        return 0.0;
    }
    step.parameter_space
        .as_ref()
        .and_then(|space| {
            space
                .task_parameter_definitions
                .values()
                .find_map(|definition| match definition {
                    openjd_model::job::TaskParameter::ChunkInt { chunks, .. } => {
                        chunks.target_runtime_seconds.map(|seconds| seconds as f64)
                    }
                    _ => None,
                })
        })
        .unwrap_or(0.0)
}

fn adjust_adaptive_chunk_size(
    ctx: &RunContext,
    iter: &mut openjd_model::StepParameterSpaceIterator,
    completed_task_count: usize,
    completed_task_duration: f64,
    target_runtime_seconds: f64,
) {
    let duration_per_task = completed_task_duration / completed_task_count as f64;
    let mut adaptive_chunk_size = target_runtime_seconds / duration_per_task;
    if completed_task_count < 10 {
        let current = iter.chunks_default_task_count().unwrap_or(1) as f64;
        if adaptive_chunk_size > current {
            adaptive_chunk_size = 0.75 * current + 0.25 * adaptive_chunk_size;
        }
    }

    let adaptive_chunk_size = (adaptive_chunk_size as usize).max(1);
    if Some(adaptive_chunk_size) != iter.chunks_default_task_count() {
        println!(
            "{}\tAdjusting chunk size to {adaptive_chunk_size}",
            ctx.timestamp()
        );
        iter.set_chunks_default_task_count(adaptive_chunk_size);
    }
}

fn report_result(ctx: &mut RunContext, args: &RunArgs, job: &Job, working_dir: &std::path::Path) {
    println!("{}\t", ctx.timestamp());
    if ctx.session_failed {
        println!("{}\tSession ended with errors.", ctx.timestamp());
    } else {
        println!("{}\tAll actions completed successfully!", ctx.timestamp());
    }
    println!("{}\tLocal session ended.", ctx.timestamp());

    let duration = ctx.started_at.elapsed().as_secs_f64();
    let preserved_msg = if args.preserve {
        format!(
            "\nWorking directory preserved at: {}",
            working_dir.display()
        )
    } else {
        ctx.session.cleanup();
        String::new()
    };
    let (status, message) = if ctx.session_failed {
        (
            "error",
            format!("Session ended with errors; see Task logs for details{preserved_msg}"),
        )
    } else {
        (
            "success",
            format!("Session ended successfully{preserved_msg}"),
        )
    };
    let result = RunResult {
        status: status.to_string(),
        message,
        job_name: job.name.clone(),
        step_name: args.step.clone(),
        duration,
        chunks_run: ctx.tasks_run,
    };
    crate::common::print_cli_result(&result, &args.output);
}
