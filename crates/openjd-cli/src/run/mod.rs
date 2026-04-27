// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! `openjd-rs run` command — run a job template locally.

mod params;
mod result;

pub use params::parse_cli_parameters;

use clap::Args;
use openjd_model::parse::{self, DocumentType};
use openjd_model::StepDependencyGraph;
use openjd_sessions::action::ActionState;
use openjd_sessions::session::Session;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

use params::*;
use result::RunResult;

/// Strip the `\\?\` extended-length path prefix that Rust's `canonicalize()` and
/// `current_dir()` add on Windows. Most tools (bash, Python, etc.) don't understand it.
pub fn strip_extended_prefix(path: &std::path::Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

#[derive(Args)]
pub struct RunArgs {
    /// Path to the job template file
    pub path: PathBuf,

    /// The name of the Step to run (if omitted, runs all steps; auto-selects if only one step)
    #[arg(long)]
    pub step: Option<String>,

    /// Job parameters (Key=Value, file://path, or inline JSON '{"Key": "Value"}')
    #[arg(short = 'p', long = "job-param", alias = "parameter")]
    pub parameters: Vec<String>,

    /// Run a single task with explicit parameter values (PARAM=VALUE, repeatable).
    /// Mutually exclusive with --tasks and --maximum-tasks.
    #[arg(long = "task-param", short = 't', action = clap::ArgAction::Append, conflicts_with_all = ["tasks", "maximum_tasks"])]
    pub task_params: Vec<String>,

    /// Run specific tasks from a JSON/YAML file (file://path) or inline JSON array.
    /// Mutually exclusive with --task-param and --maximum-tasks.
    #[arg(long = "tasks", conflicts_with_all = ["task_params", "maximum_tasks"])]
    pub tasks: Option<String>,

    /// Environment template files
    #[arg(long = "environment", alias = "env")]
    pub environments: Vec<PathBuf>,

    /// Path mapping rules (file://path or inline JSON). Must have version 'pathmapping-1.0'.
    #[arg(long = "path-mapping-rules")]
    pub path_mapping_rules: Option<String>,

    /// Run dependency steps before the target step
    #[arg(long = "run-dependencies")]
    pub run_dependencies: bool,

    /// Do not run dependency steps (default)
    #[arg(long = "no-run-dependencies")]
    pub no_run_dependencies: bool,

    /// Maximum number of tasks to run (-1 for all).
    /// Mutually exclusive with --task-param and --tasks.
    #[arg(long = "maximum-tasks", default_value = "-1")]
    pub maximum_tasks: i64,

    /// Extensions to support (comma-separated or repeated). Empty string disables all.
    #[arg(long = "extensions")]
    pub extensions: Option<String>,

    /// Preserve session working directory after completion
    #[arg(long)]
    pub preserve: bool,

    /// Enable verbose logging while running the Session
    #[arg(long)]
    pub verbose: bool,

    /// How to format log output timestamps
    #[arg(long = "timestamp-format", value_parser = ["relative", "local", "utc"], default_value = "relative")]
    pub timestamp_format: String,

    /// How to format the command's output
    #[arg(long = "output", value_parser = ["human-readable", "json", "yaml"], default_value = "human-readable")]
    pub output: String,
}

pub async fn execute(args: RunArgs) -> Result<(), Box<dyn std::error::Error>> {
    let path = &args.path;
    if !path.exists() {
        return Err(format!("'{}' does not exist.", path.display()).into());
    }

    // Verbose logging
    if args.verbose {
        log::set_max_level(log::LevelFilter::Debug);
    }

    let session_start = Instant::now();
    // Initialize global timestamp state for the logger
    let _ = crate::SESSION_START.set(session_start);
    let _ = crate::TIMESTAMP_FORMAT.set(args.timestamp_format.clone());
    let ts_format = args.timestamp_format.clone();
    let fmt_elapsed = move |start: &Instant| -> String {
        match ts_format.as_str() {
            "local" => chrono::Local::now()
                .format("%Y-%m-%dT%H:%M:%S%.3f")
                .to_string(),
            "utc" => chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
            _ => {
                let d = start.elapsed();
                let total_secs = d.as_secs();
                let h = total_secs / 3600;
                let m = (total_secs % 3600) / 60;
                let s = total_secs % 60;
                let ms = d.subsec_millis();
                format!("{h}:{m:02}:{s:02}.{ms:03}")
            }
        }
    };

    // Parse templates
    let content = std::fs::read_to_string(path)?;
    let doc_type = if path.extension().and_then(|e| e.to_str()) == Some("json") {
        DocumentType::Json
    } else {
        DocumentType::Yaml
    };
    let template_value = parse::document_string_to_object(
        &content,
        doc_type,
        &openjd_model::CallerLimits::default(),
    )?;
    let exts = crate::common::parse_extensions(&args.extensions)?;
    let supported_exts: Vec<&str> = exts.iter().map(|s| s.as_str()).collect();
    let job_template = parse::decode_job_template(
        template_value,
        Some(&supported_exts),
        &openjd_model::CallerLimits::default(),
    )?;

    let mut env_templates = Vec::new();
    for env_path in &args.environments {
        let env_content = std::fs::read_to_string(env_path)?;
        let env_doc_type = if env_path.extension().and_then(|e| e.to_str()) == Some("json") {
            DocumentType::Json
        } else {
            DocumentType::Yaml
        };
        let env_value = parse::document_string_to_object(
            &env_content,
            env_doc_type,
            &openjd_model::CallerLimits::default(),
        )?;
        env_templates.push(parse::decode_environment_template(
            env_value,
            Some(&supported_exts),
        )?);
    }

    // Preprocess parameters (model API)
    let input_values = parse_cli_parameters(&args.parameters)?;
    let path_rules = load_path_mapping_rules(&args.path_mapping_rules)?;
    let job_template_dir = strip_extended_prefix(
        std::fs::canonicalize(path)?
            .parent()
            .unwrap_or_else(|| std::path::Path::new(".")),
    );
    let current_working_dir = strip_extended_prefix(&std::env::current_dir()?);
    let param_values = match openjd_model::preprocess_job_parameters(
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
    ) {
        Ok(v) => v,
        Err(e) => {
            let help = crate::help::format_help(&job_template, path);
            return Err(format!("{e}\n\n{help}").into());
        }
    };

    // Create instantiated job
    let job = match openjd_model::create_job(
        &job_template,
        &param_values,
        &openjd_model::CallerLimits::default(),
    ) {
        Ok(j) => j,
        Err(e) => {
            let help = crate::help::format_help(&job_template, path);
            return Err(format!("{e}\n\n{help}").into());
        }
    };

    // Build host-context library with path mapping rules
    let host_library = openjd_expr::default_library::get_default_library()
        .clone()
        .with_host_context(path_rules.clone());

    // Set up session
    let revision_ctx = {
        let mut exts = std::collections::HashSet::new();
        if let Some(ext_list) = &job.extensions {
            exts.extend(
                ext_list
                    .iter()
                    .filter_map(|s| s.parse::<openjd_model::types::KnownExtension>().ok()),
            );
        }
        openjd_model::types::ValidationContext::with_extensions(
            openjd_model::types::SpecificationRevision::V2023_09,
            exts,
        )
    };
    let cancel_token = CancellationToken::new();
    let session_config = openjd_sessions::session::SessionConfig {
        session_id: format!("cli-{}", std::process::id()),
        job_parameter_values: param_values.clone(),
        path_mapping_rules: if path_rules.is_empty() {
            None
        } else {
            Some(path_rules.clone())
        },
        retain_working_dir: args.preserve,
        callback: None,
        os_env_vars: None,
        session_root_directory: None,
        user: None,
        revision_extensions: Some(revision_ctx),
        cancel_token: Some(cancel_token.clone()),
        debug_collect_stdout: false,
    };
    let mut session = Session::with_config(session_config)
        .map_err(|e| format!("Failed to create session: {e}"))?;
    session = session.with_library(host_library.clone());

    let working_dir = session.working_directory().to_path_buf();

    // Install signal handler for graceful cancellation.
    let interrupted = Arc::new(AtomicBool::new(false));
    {
        let interrupted = interrupted.clone();
        let token = cancel_token.clone();
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                let mut sigint =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                        .unwrap();
                let mut sigterm =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .unwrap();
                tokio::select! {
                    _ = sigint.recv() => {}
                    _ = sigterm.recv() => {}
                }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
            interrupted.store(true, Ordering::SeqCst);
            token.cancel();
        });
    }

    println!("{}\tSession start", fmt_elapsed(&session_start));
    println!(
        "{}\tRunning job '{}'",
        fmt_elapsed(&session_start),
        job.name
    );

    // Enter environment template environments
    for et in &env_templates {
        println!("{}\t", fmt_elapsed(&session_start));
        println!(
            "{}\t==============================================",
            fmt_elapsed(&session_start)
        );
        println!(
            "{}\t--------- Entering Environment: {}",
            fmt_elapsed(&session_start),
            et.environment.name
        );
        println!(
            "{}\t==============================================",
            fmt_elapsed(&session_start)
        );
        let job_env = openjd_model::convert_environment(&et.environment);
        let _out = session
            .enter_environment(&job_env, None, None, None)
            .await
            .map_err(|e| format!("Environment template setup failed: {e}"))?;
    }
    // Enter job environments
    if let Some(job_envs) = &job.job_environments {
        for env in job_envs {
            println!("{}\t", fmt_elapsed(&session_start));
            println!(
                "{}\t==============================================",
                fmt_elapsed(&session_start)
            );
            println!(
                "{}\t--------- Entering Environment: {}",
                fmt_elapsed(&session_start),
                env.name
            );
            println!(
                "{}\t==============================================",
                fmt_elapsed(&session_start)
            );
            let _out = session
                .enter_environment(env, None, None, None)
                .await
                .map_err(|e| format!("Environment setup failed: {e}"))?;
        }
    }

    let mut tasks_run: usize = 0;
    let mut session_failed = false;

    // Resolve selected step
    let selected_step_idx: Option<usize> = if let Some(step_name) = &args.step {
        let idx = job
            .steps
            .iter()
            .position(|s| s.name == *step_name)
            .ok_or_else(|| {
                format!(
                    "No Step with name '{}' is defined in the given Job Template.",
                    step_name
                )
            })?;
        Some(idx)
    } else if job.steps.len() == 1 {
        Some(0)
    } else {
        if !args.task_params.is_empty() || args.tasks.is_some() {
            return Err(format!(
                "Providing task parameters requires a specified step or a job with a single step.\n{} steps: {:?}.",
                job.steps.len(), job.steps.iter().map(|s| &s.name).collect::<Vec<_>>()
            ).into());
        }
        None
    };

    // Parse explicit task parameter sets
    let explicit_task_params: Option<Vec<HashMap<String, String>>> = if !args.task_params.is_empty()
    {
        Some(vec![parse_task_params(&args.task_params)?])
    } else if let Some(ref tasks_arg) = args.tasks {
        Some(parse_tasks_arg(tasks_arg)?)
    } else {
        None
    };

    // Determine which steps to run
    let steps_to_run: Vec<usize> = if let Some(sel_idx) = selected_step_idx {
        if args.run_dependencies {
            resolve_step_dependencies(&job, sel_idx)
        } else {
            vec![sel_idx]
        }
    } else {
        StepDependencyGraph::new(&job)?.topo_sorted()?
    };

    // Execute steps
    for &step_idx in &steps_to_run {
        if session_failed || interrupted.load(Ordering::SeqCst) {
            break;
        }

        let step = &job.steps[step_idx];
        println!(
            "{}\tRunning step '{}'",
            fmt_elapsed(&session_start),
            step.name
        );

        let step_symtab = step.resolved_symtab.as_ref();

        // Enter step environments
        if let Some(step_envs) = &step.step_environments {
            for env in step_envs {
                let _out = session
                    .enter_environment(env, step_symtab, None, None)
                    .await
                    .map_err(|e| format!("Step environment setup failed: {e}"))?;
            }
        }

        // Build iterator
        let has_params;
        let is_adaptive;
        let mut iter = if let Some(ps) = &step.parameter_space {
            let it = openjd_model::StepParameterSpaceIterator::new(ps)?;
            has_params = true;
            is_adaptive = it.chunks_adaptive();
            Some(it)
        } else {
            has_params = false;
            is_adaptive = false;
            None
        };

        // Adaptive chunking state
        let chunks_param_name = iter
            .as_ref()
            .and_then(|it| it.chunks_parameter_name().map(String::from));
        let mut completed_task_count: usize = 0;
        let mut completed_task_duration: f64 = 0.0;

        let target_runtime_seconds: f64 = if is_adaptive {
            step.parameter_space
                .as_ref()
                .and_then(|ps| {
                    ps.task_parameter_definitions
                        .values()
                        .find_map(|d| match d {
                            openjd_model::job::TaskParameter::ChunkInt { chunks, .. } => {
                                chunks.target_runtime_seconds.map(|t| t as f64)
                            }
                            _ => None,
                        })
                })
                .unwrap_or(0.0)
        } else {
            0.0
        };

        if !has_params {
            // No parameter space — run a single task
            println!(
                "{}\t==============================================",
                fmt_elapsed(&session_start)
            );
            println!("{}\t--------- Running Task", fmt_elapsed(&session_start));
            println!(
                "{}\t==============================================",
                fmt_elapsed(&session_start)
            );
            let result = session
                .run_task(&step.script, None, step_symtab, None)
                .await
                .map_err(|e| format!("Step '{}': {e}", step.name))?;
            println!(
                "{}\tProcess exited with code: {}",
                fmt_elapsed(&session_start),
                result.exit_code.unwrap_or(-1)
            );
            tasks_run += 1;
            if result.state != ActionState::Success {
                session_failed = true;
            }
            if interrupted.load(Ordering::SeqCst) {
                println!(
                    "{}\tInterruption signal received.",
                    fmt_elapsed(&session_start)
                );
                session_failed = true;
            }
        } else {
            let is_selected = selected_step_idx == Some(step_idx);
            let use_explicit = is_selected && explicit_task_params.is_some();

            if use_explicit {
                let task_param_sets = explicit_task_params.as_ref().unwrap();
                let typed_sets = if let Some(ref space) = step.parameter_space {
                    let mut sets = Vec::new();
                    for ps in task_param_sets {
                        sets.push(coerce_task_params(ps, space)?);
                    }
                    if let Some(ref it) = iter {
                        for (i, ts) in sets.iter().enumerate() {
                            if let Err(e) = it.validate_containment(ts) {
                                return Err(format!("Task parameter set {i}: {e}").into());
                            }
                        }
                    }
                    sets
                } else {
                    task_param_sets
                        .iter()
                        .map(|ps| {
                            ps.iter()
                                .map(|(k, v)| {
                                    (
                                        k.clone(),
                                        openjd_model::types::TaskParameterValue {
                                            param_type:
                                                openjd_model::types::TaskParameterType::String,
                                            value: openjd_expr::ExprValue::String(v.clone()),
                                        },
                                    )
                                })
                                .collect()
                        })
                        .collect()
                };

                for (task_param_set, task_values) in task_param_sets.iter().zip(typed_sets.iter()) {
                    if session_failed || interrupted.load(Ordering::SeqCst) {
                        break;
                    }
                    println!(
                        "{}\t==============================================",
                        fmt_elapsed(&session_start)
                    );
                    println!("{}\t--------- Running Task", fmt_elapsed(&session_start));
                    println!(
                        "{}\t==============================================",
                        fmt_elapsed(&session_start)
                    );
                    println!("{}\tParameter values:", fmt_elapsed(&session_start));
                    for (name, value) in task_param_set {
                        println!("{}\t{} = {}", fmt_elapsed(&session_start), name, value);
                    }
                    let result = session
                        .run_task(&step.script, Some(task_values), step_symtab, None)
                        .await
                        .map_err(|e| format!("Step '{}': {e}", step.name))?;
                    println!(
                        "{}\tProcess exited with code: {}",
                        fmt_elapsed(&session_start),
                        result.exit_code.unwrap_or(-1)
                    );
                    tasks_run += 1;
                    if result.state != ActionState::Success {
                        session_failed = true;
                    }
                }
            } else {
                // Lazy iteration over parameter space
                let mut remaining_tasks = if args.maximum_tasks > 0 {
                    args.maximum_tasks
                } else {
                    i64::MAX
                };
                let it = iter.as_mut().unwrap();
                while remaining_tasks > 0 && !session_failed && !interrupted.load(Ordering::SeqCst)
                {
                    let Some(task_params) = it.next() else { break };

                    println!(
                        "{}\t==============================================",
                        fmt_elapsed(&session_start)
                    );
                    println!("{}\t--------- Running Task", fmt_elapsed(&session_start));
                    println!(
                        "{}\t==============================================",
                        fmt_elapsed(&session_start)
                    );
                    println!("{}\tParameter values:", fmt_elapsed(&session_start));
                    for (name, tv) in &task_params {
                        println!(
                            "{}\t{}({}) = {}",
                            fmt_elapsed(&session_start),
                            name,
                            tv.param_type.as_spec_str(),
                            tv.value.to_display_string()
                        );
                    }
                    let task_values: openjd_model::types::TaskParameterSet = task_params
                        .iter()
                        .map(|(name, tv)| (name.clone(), tv.clone()))
                        .collect();

                    let task_start = Instant::now();
                    let result = session
                        .run_task(&step.script, Some(&task_values), step_symtab, None)
                        .await
                        .map_err(|e| format!("Step '{}': {e}", step.name))?;
                    let task_duration = task_start.elapsed().as_secs_f64();

                    println!(
                        "{}\tProcess exited with code: {}",
                        fmt_elapsed(&session_start),
                        result.exit_code.unwrap_or(-1)
                    );
                    tasks_run += 1;
                    remaining_tasks -= 1;
                    if result.state != ActionState::Success {
                        session_failed = true;
                        break;
                    }
                    if interrupted.load(Ordering::SeqCst) {
                        println!(
                            "{}\tInterruption signal received.",
                            fmt_elapsed(&session_start)
                        );
                        session_failed = true;
                        break;
                    }

                    // Adaptive chunking: measure and adjust chunk size
                    if is_adaptive {
                        let chunk_items = chunks_param_name
                            .as_ref()
                            .and_then(|cpn| task_params.get(cpn))
                            .map(|tv| match &tv.value {
                                openjd_expr::ExprValue::RangeExpr(r) => r.len(),
                                _ => 1,
                            })
                            .unwrap_or(1);

                        completed_task_count += chunk_items;
                        completed_task_duration += task_duration;

                        let duration_per_task =
                            completed_task_duration / completed_task_count as f64;
                        let mut adaptive_chunk_size = target_runtime_seconds / duration_per_task;

                        if completed_task_count < 10 {
                            let current = it.chunks_default_task_count().unwrap_or(1) as f64;
                            if adaptive_chunk_size > current {
                                adaptive_chunk_size = 0.75 * current + 0.25 * adaptive_chunk_size;
                            }
                        }

                        let adaptive_chunk_size = (adaptive_chunk_size as usize).max(1);
                        if Some(adaptive_chunk_size) != it.chunks_default_task_count() {
                            println!(
                                "{}\tAdjusting chunk size to {adaptive_chunk_size}",
                                fmt_elapsed(&session_start)
                            );
                            it.set_chunks_default_task_count(adaptive_chunk_size);
                        }
                    }
                }
            }
        }

        // Exit step environments in reverse
        if let Some(step_envs) = &step.step_environments {
            for _env in step_envs.iter().rev() {
                if let Some(id) = session.environments_entered().last().cloned() {
                    if let Ok(_out) = session.exit_environment(&id, step_symtab, true, None).await {
                    }
                }
            }
        }
    }

    // Exit job environments in reverse
    if let Some(job_envs) = &job.job_environments {
        for env in job_envs.iter().rev() {
            println!("{}\t", fmt_elapsed(&session_start));
            println!(
                "{}\t==============================================",
                fmt_elapsed(&session_start)
            );
            println!(
                "{}\t--------- Exiting Environment: {}",
                fmt_elapsed(&session_start),
                env.name
            );
            println!(
                "{}\t==============================================",
                fmt_elapsed(&session_start)
            );
            if let Some(id) = session.environments_entered().last().cloned() {
                if let Ok(_out) = session.exit_environment(&id, None, true, None).await {}
            }
        }
    }
    for et in env_templates.iter().rev() {
        println!("{}\t", fmt_elapsed(&session_start));
        println!(
            "{}\t==============================================",
            fmt_elapsed(&session_start)
        );
        println!(
            "{}\t--------- Exiting Environment: {}",
            fmt_elapsed(&session_start),
            et.environment.name
        );
        println!(
            "{}\t==============================================",
            fmt_elapsed(&session_start)
        );
        if let Some(id) = session.environments_entered().last().cloned() {
            if let Ok(_out) = session.exit_environment(&id, None, true, None).await {}
        }
    }

    println!("{}\t", fmt_elapsed(&session_start));
    if session_failed {
        println!(
            "{}\tSession ended with errors.",
            fmt_elapsed(&session_start)
        );
    } else {
        println!(
            "{}\tAll actions completed successfully!",
            fmt_elapsed(&session_start)
        );
    }
    println!("{}\tLocal session ended.", fmt_elapsed(&session_start));

    let duration = session_start.elapsed().as_secs_f64();
    let step_name = args.step.clone();
    let preserved_msg = if args.preserve {
        format!(
            "\nWorking directory preserved at: {}",
            working_dir.display()
        )
    } else {
        session.cleanup();
        String::new()
    };

    let status = if session_failed { "error" } else { "success" };
    let message = if session_failed {
        format!("Session ended with errors; see Task logs for details{preserved_msg}")
    } else {
        format!("Session ended successfully{preserved_msg}")
    };

    let result = RunResult {
        status: status.to_string(),
        message,
        job_name: job.name.clone(),
        step_name: step_name.clone(),
        duration,
        chunks_run: tasks_run,
    };
    crate::common::print_cli_result(&result, &args.output);

    if session_failed {
        std::process::exit(1);
    }
    Ok(())
}

/// Resolve step dependencies transitively, returning indices in execution order.
fn resolve_step_dependencies(job: &openjd_model::job::Job, target_idx: usize) -> Vec<usize> {
    let step_name_to_idx: HashMap<String, usize> = job
        .steps
        .iter()
        .enumerate()
        .map(|(i, s)| (s.name.clone(), i))
        .collect();
    let mut visited = std::collections::HashSet::new();
    let mut order = Vec::new();
    fn visit(
        job: &openjd_model::job::Job,
        idx: usize,
        name_to_idx: &HashMap<String, usize>,
        visited: &mut std::collections::HashSet<usize>,
        order: &mut Vec<usize>,
    ) {
        if !visited.insert(idx) {
            return;
        }
        if let Some(deps) = &job.steps[idx].dependencies {
            for dep in deps {
                if let Some(&dep_idx) = name_to_idx.get(&dep.depends_on) {
                    visit(job, dep_idx, name_to_idx, visited, order);
                }
            }
        }
        order.push(idx);
    }
    visit(job, target_idx, &step_name_to_idx, &mut visited, &mut order);
    order
}
