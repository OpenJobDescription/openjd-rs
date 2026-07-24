// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! `openjd run` command — run a job template locally.

mod execution;
mod params;
mod result;

pub use params::parse_cli_parameters;

use clap::Args;
use openjd_model::job::{Environment, Job, Step};
use openjd_model::template::parse;
use openjd_model::template::EnvironmentTemplate;
use openjd_model::types::{JobParameterValues, ModelProfile, TaskParameterSet};
use openjd_model::StepDependencyGraph;
use openjd_sessions::action::ActionState;
use openjd_sessions::path_mapping::PathMappingRule;
use openjd_sessions::session::Session;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

use params::*;
use result::RunResult;

type RunError = Box<dyn std::error::Error>;
type EnvSymtab = Option<openjd_expr::SerializedSymbolTable>;

struct PreparedRun {
    job: Job,
    env_templates: Vec<EnvironmentTemplate>,
    param_values: JobParameterValues,
    path_rules: Vec<PathMappingRule>,
    revision_profile: ModelProfile,
}

struct RunSelection {
    selected_step_idx: Option<usize>,
    explicit_task_params: Option<Vec<HashMap<String, String>>>,
    steps_to_run: Vec<usize>,
}

struct EnteredEnvironment {
    identifier: String,
    name: String,
    symtab: EnvSymtab,
}

struct RunContext {
    session: Session,
    entered_envs: Vec<EnteredEnvironment>,
    interrupted: Arc<AtomicBool>,
    started_at: Instant,
    tasks_run: usize,
    session_failed: bool,
}

impl RunContext {
    fn timestamp(&self) -> String {
        crate::format_log_timestamp()
    }

    fn print_action_banner(&self, label: &str) {
        println!("{}\t", self.timestamp());
        self.print_banner(label);
    }

    fn print_banner(&self, label: &str) {
        println!(
            "{}\t==============================================",
            self.timestamp()
        );
        println!("{}\t--------- {label}", self.timestamp());
        println!(
            "{}\t==============================================",
            self.timestamp()
        );
    }

    fn is_stopping(&self) -> bool {
        self.session_failed || self.interrupted.load(Ordering::SeqCst)
    }

    async fn enter_environment(&mut self, env: &Environment, name: &str, symtab: EnvSymtab) {
        self.print_action_banner(&format!("Entering Environment: {name}"));
        match self
            .session
            .enter_environment(env, symtab.as_ref(), None, None)
            .await
        {
            Ok(identifier) => self.entered_envs.push(EnteredEnvironment {
                identifier,
                name: name.to_string(),
                symtab,
            }),
            Err(e) => {
                eprintln!("ERROR: Environment setup failed: {e}");
                if let Some(code) = self.session.action_status().and_then(|s| s.exit_code) {
                    println!("{}\tProcess exited with code: {code}", self.timestamp());
                }

                // Failed enter actions remain on the Session stack and must
                // still be exited. Rejections before entry do not.
                if let Some(identifier) = self.session.environments_entered().last() {
                    if !self
                        .entered_envs
                        .iter()
                        .any(|entered| &entered.identifier == identifier)
                    {
                        self.entered_envs.push(EnteredEnvironment {
                            identifier: identifier.clone(),
                            name: name.to_string(),
                            symtab,
                        });
                    }
                }
                self.session_failed = true;
            }
        }
    }

    async fn exit_environments_down_to(&mut self, baseline: usize) {
        while self.entered_envs.len() > baseline {
            let entered = self
                .entered_envs
                .pop()
                .expect("environment count checked by loop guard");
            self.print_action_banner(&format!("Exiting Environment: {}", entered.name));
            if let Err(e) = self
                .session
                .exit_environment(&entered.identifier, entered.symtab.as_ref(), true, None)
                .await
            {
                eprintln!("ERROR: Environment teardown failed: {e}");
                self.session_failed = true;
            }
        }
    }

    async fn run_task(
        &mut self,
        step: &Step,
        task_values: Option<&TaskParameterSet>,
    ) -> Result<f64, RunError> {
        let task_start = Instant::now();
        let result = self
            .session
            .run_task(
                &step.name,
                &step.script,
                task_values,
                step.resolved_symtab.as_ref(),
                None,
            )
            .await
            .map_err(|e| format!("Step '{}': {e}", step.name))?;
        let task_duration = task_start.elapsed().as_secs_f64();

        println!(
            "{}\tProcess exited with code: {}",
            self.timestamp(),
            result.exit_code.unwrap_or(-1)
        );
        self.tasks_run += 1;
        if result.state != ActionState::Success {
            self.session_failed = true;
        }
        Ok(task_duration)
    }

    fn record_interruption(&mut self) {
        if self.interrupted.load(Ordering::SeqCst) {
            println!("{}\tInterruption signal received.", self.timestamp());
            self.session_failed = true;
        }
    }
}

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

pub async fn execute(args: RunArgs) -> Result<(), RunError> {
    execution::execute(args).await
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_interruption_marks_run_failed() {
        let root = tempfile::tempdir().unwrap();
        let session = Session::with_config(openjd_sessions::session::SessionConfig {
            session_id: "interrupt-test".to_string(),
            job_parameter_values: HashMap::new(),
            path_mapping_rules: None,
            retain_working_dir: false,
            callback: None,
            os_env_vars: None,
            session_root_directory: Some(root.path().to_path_buf()),
            user: None,
            profile: None,
            cancel_token: None,
            sticky_bit_policy: Default::default(),
            debug_collect_stdout: false,
            echo_openjd_directives: true,
        })
        .unwrap();
        let interrupted = Arc::new(AtomicBool::new(false));
        let mut ctx = RunContext {
            session,
            entered_envs: Vec::new(),
            interrupted: interrupted.clone(),
            started_at: Instant::now(),
            tasks_run: 0,
            session_failed: false,
        };

        ctx.record_interruption();
        assert!(!ctx.session_failed);

        interrupted.store(true, Ordering::SeqCst);
        ctx.record_interruption();
        assert!(ctx.session_failed);
        ctx.session.cleanup();
    }
}
