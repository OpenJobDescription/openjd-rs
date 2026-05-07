// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! `openjd-rs summary` command — print summary information about a Job Template.

use clap::Args;
use openjd_model::job;
use openjd_model::parse::{self, DocumentType};
use std::path::PathBuf;

#[derive(Args)]
pub struct SummaryArgs {
    /// Path to the job template file
    pub path: PathBuf,

    /// Print information about this Step only
    #[arg(long)]
    pub step: Option<String>,

    /// Job parameters (Key=Value, file://path, or inline JSON)
    #[arg(short = 'p', long = "job-param", alias = "parameter")]
    pub parameters: Vec<String>,

    /// Extensions to support (comma-separated). Empty string disables all.
    #[arg(long = "extensions")]
    pub extensions: Option<String>,

    /// Environment template files
    #[arg(long = "environment", alias = "env")]
    pub environments: Vec<PathBuf>,

    /// How to format the command's output
    #[arg(long = "output", value_parser = ["human-readable", "json", "yaml"], default_value = "human-readable")]
    pub output: String,
}

pub fn execute(args: SummaryArgs) -> Result<(), Box<dyn std::error::Error>> {
    let path = &args.path;
    let content = crate::common::read_input_file(path)?;
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

    // Preserve template parameter definition order
    let param_order: Vec<String> = job_template
        .parameter_definitions_list()
        .iter()
        .map(|p| p.name().to_string())
        .collect();
    let param_descriptions: std::collections::HashMap<&str, &str> = job_template
        .parameter_definitions_list()
        .iter()
        .filter_map(|p| p.description().map(|d| (p.name(), d)))
        .collect();

    // Load environment templates
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

    // Parse parameters
    let input_values = crate::run::parse_cli_parameters(&args.parameters)?;
    let job_template_dir = crate::run::strip_extended_prefix(
        std::fs::canonicalize(path)?
            .parent()
            .unwrap_or_else(|| std::path::Path::new(".")),
    );
    let current_working_dir = crate::run::strip_extended_prefix(&std::env::current_dir()?);
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
    )?;

    let ctx = {
        let mut exts = std::collections::HashSet::new();
        if let Some(ext_list) = &job_template.extensions {
            exts.extend(
                ext_list
                    .iter()
                    .filter_map(|e| e.as_str().parse::<openjd_model::KnownExtension>().ok()),
            );
        }
        openjd_model::ValidationContext::with_extensions(
            openjd_model::SpecificationRevision::V2023_09,
            exts,
        )
    };

    let the_job = openjd_model::create_job(&job_template, &param_values, &ctx)?;

    if let Some(step_name) = &args.step {
        output_step_summary(&the_job, step_name, &args.output)
    } else {
        output_job_summary(&the_job, &param_order, &param_descriptions, &args.output)
    }
}

fn task_param_type_name(tp: &job::TaskParameter) -> &'static str {
    match tp {
        job::TaskParameter::Int { .. } => "INT",
        job::TaskParameter::Float { .. } => "FLOAT",
        job::TaskParameter::String { .. } => "STRING",
        job::TaskParameter::Path { .. } => "PATH",
        job::TaskParameter::ChunkInt { .. } => "CHUNK[INT]",
    }
}

fn step_total_tasks(step: &job::Step) -> usize {
    match &step.parameter_space {
        None => 1,
        Some(ps) => {
            match openjd_model::StepParameterSpaceIterator::new_with_chunk_override(ps, Some(1)) {
                Ok(it) => it.len(),
                Err(_) => 1,
            }
        }
    }
}

fn output_job_summary(
    job: &job::Job,
    param_order: &[String],
    param_descriptions: &std::collections::HashMap<&str, &str>,
    output_format: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Collect step summaries
    let mut step_envs_info: Vec<EnvInfo> = Vec::new();
    let step_summaries: Vec<StepInfo> = job
        .steps
        .iter()
        .map(|s| {
            let total_tasks = step_total_tasks(s);
            let task_params: Vec<(String, String)> = s
                .parameter_space
                .as_ref()
                .map(|ps| {
                    let mut v: Vec<_> = ps
                        .task_parameter_definitions
                        .iter()
                        .map(|(name, tp)| (name.clone(), task_param_type_name(tp).to_string()))
                        .collect();
                    v.sort_by(|a, b| a.0.cmp(&b.0));
                    v
                })
                .unwrap_or_default();
            let envs: Vec<String> = s
                .step_environments
                .as_ref()
                .map(|envs| envs.iter().map(|e| e.name.clone()).collect())
                .unwrap_or_default();
            if let Some(envs) = &s.step_environments {
                for e in envs {
                    step_envs_info.push(EnvInfo {
                        name: e.name.clone(),
                        description: e.description.clone(),
                        parent: s.name.clone(),
                    });
                }
            }
            let deps: Vec<String> = s
                .dependencies
                .as_ref()
                .map(|deps| deps.iter().map(|d| d.depends_on.clone()).collect())
                .unwrap_or_default();
            StepInfo {
                name: s.name.clone(),
                description: s.description.clone(),
                total_tasks,
                task_params,
                envs,
                deps,
            }
        })
        .collect();

    let total_tasks: usize = step_summaries.iter().map(|s| s.total_tasks).sum();
    let step_envs: usize = step_summaries.iter().map(|s| s.envs.len()).sum();
    let root_envs: Vec<EnvInfo> = job
        .job_environments
        .as_ref()
        .map(|envs| {
            envs.iter()
                .map(|e| EnvInfo {
                    name: e.name.clone(),
                    description: e.description.clone(),
                    parent: "root".into(),
                })
                .collect()
        })
        .unwrap_or_default();
    let total_envs = root_envs.len() + step_envs;

    // Collect parameter summaries in template definition order
    let params: Vec<ParamInfo> = param_order
        .iter()
        .filter_map(|name| {
            job.parameters.get(name).map(|p| ParamInfo {
                name: name.clone(),
                description: param_descriptions.get(name.as_str()).map(|s| s.to_string()),
                param_type: p.param_type.as_spec_str().to_string(),
                value: p.value.to_display_string(),
            })
        })
        .collect();

    let result = JobSummaryResult {
        name: job.name.clone(),
        params,
        step_summaries,
        root_envs,
        step_envs: step_envs_info,
        total_tasks,
        total_envs,
    };
    crate::common::print_cli_result(&result, output_format);
    Ok(())
}

fn output_step_summary(
    job: &job::Job,
    step_name: &str,
    output_format: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let step = job
        .steps
        .iter()
        .find(|s| s.name == step_name)
        .ok_or_else(|| format!("Step '{step_name}' does not exist in Job '{}'.", job.name))?;

    let total_tasks = step_total_tasks(step);
    let task_params: Vec<(String, String)> = step
        .parameter_space
        .as_ref()
        .map(|ps| {
            let mut v: Vec<_> = ps
                .task_parameter_definitions
                .iter()
                .map(|(name, tp)| (name.clone(), task_param_type_name(tp).to_string()))
                .collect();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            v
        })
        .unwrap_or_default();
    let envs: Vec<EnvInfo> = step
        .step_environments
        .as_ref()
        .map(|envs| {
            envs.iter()
                .map(|e| EnvInfo {
                    name: e.name.clone(),
                    description: e.description.clone(),
                    parent: step.name.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    let deps: Vec<String> = step
        .dependencies
        .as_ref()
        .map(|deps| deps.iter().map(|d| d.depends_on.clone()).collect())
        .unwrap_or_default();

    let result = StepSummaryResult {
        job_name: job.name.clone(),
        step_name: step_name.to_string(),
        total_tasks,
        task_params,
        envs,
        deps,
    };
    crate::common::print_cli_result(&result, output_format);
    Ok(())
}

// --- Result types ---

use crate::common::CliResult;
use std::fmt;

struct JobSummaryResult {
    name: String,
    params: Vec<ParamInfo>,
    step_summaries: Vec<StepInfo>,
    root_envs: Vec<EnvInfo>,
    step_envs: Vec<EnvInfo>,
    total_tasks: usize,
    total_envs: usize,
}

impl CliResult for JobSummaryResult {
    fn to_json_value(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();
        obj.insert("status".into(), "success".into());
        obj.insert(
            "message".into(),
            format!("Summary for '{}'", self.name).into(),
        );
        obj.insert("name".into(), self.name.clone().into());
        if !self.params.is_empty() {
            obj.insert(
                "parameter_definitions".into(),
                serde_json::json!(self
                    .params
                    .iter()
                    .map(|p| {
                        let mut m = serde_json::Map::new();
                        m.insert("name".into(), p.name.clone().into());
                        if let Some(d) = &p.description {
                            m.insert("description".into(), d.clone().into());
                        }
                        m.insert("type".into(), p.param_type.clone().into());
                        m.insert("value".into(), p.value.clone().into());
                        serde_json::Value::Object(m)
                    })
                    .collect::<Vec<_>>()),
            );
        }
        obj.insert("total_steps".into(), self.step_summaries.len().into());
        obj.insert(
            "total_tasks".into(),
            serde_json::Value::Number(self.total_tasks.into()),
        );
        if self.total_envs > 0 {
            obj.insert("total_environments".into(), self.total_envs.into());
        }
        if !self.root_envs.is_empty() {
            obj.insert(
                "root_environments".into(),
                serde_json::json!(self
                    .root_envs
                    .iter()
                    .map(|e| serde_json::json!({"name": e.name, "parent": e.parent}))
                    .collect::<Vec<_>>()),
            );
        }
        let steps_json: Vec<serde_json::Value> = self
            .step_summaries
            .iter()
            .map(|s| {
                let mut m = serde_json::Map::new();
                m.insert("name".into(), s.name.clone().into());
                if let Some(d) = &s.description {
                    m.insert("description".into(), d.clone().into());
                }
                m.insert("total_tasks".into(), s.total_tasks.into());
                if !s.task_params.is_empty() {
                    m.insert(
                        "parameter_definitions".into(),
                        serde_json::json!(s
                            .task_params
                            .iter()
                            .map(|(n, t)| serde_json::json!({"name": n, "type": t}))
                            .collect::<Vec<_>>()),
                    );
                }
                if !s.envs.is_empty() {
                    m.insert("environments".into(), s.envs.len().into());
                }
                if !s.deps.is_empty() {
                    m.insert("dependencies".into(), s.deps.len().into());
                }
                serde_json::Value::Object(m)
            })
            .collect();
        obj.insert("steps".into(), steps_json.into());
        serde_json::Value::Object(obj)
    }
}

impl fmt::Display for JobSummaryResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f)?;
        writeln!(f, "--- Summary for '{}' ---", self.name)?;
        if !self.params.is_empty() {
            writeln!(f)?;
            writeln!(f, "Parameters:")?;
            for p in &self.params {
                if p.value.is_empty() {
                    writeln!(f, "  - {} ({})", p.name, p.param_type)?;
                } else {
                    writeln!(f, "  - {} ({}): {}", p.name, p.param_type, p.value)?;
                }
            }
        }
        writeln!(f)?;
        writeln!(f, "Total steps: {}", self.step_summaries.len())?;
        writeln!(f, "Total tasks: {}", self.total_tasks)?;
        writeln!(f, "Total environments: {}", self.total_envs)?;
        writeln!(f)?;
        writeln!(f, "--- Steps in '{}' ---", self.name)?;
        writeln!(f)?;
        for (i, s) in self.step_summaries.iter().enumerate() {
            writeln!(f, "{}. '{}' ({} total Tasks)", i + 1, s.name, s.total_tasks)?;
            if !s.task_params.is_empty() {
                writeln!(f, "  Task parameters:")?;
                for (name, tp) in &s.task_params {
                    writeln!(f, "    - {name} ({tp})")?;
                }
            }
            if !s.envs.is_empty() {
                writeln!(f, "  {} environments", s.envs.len())?;
            }
            if !s.deps.is_empty() {
                writeln!(f, "  {} dependencies", s.deps.len())?;
            }
            writeln!(f)?;
        }
        if self.total_envs > 0 {
            writeln!(f)?;
            writeln!(f, "--- Environments in '{}' ---", self.name)?;
            for e in &self.root_envs {
                write!(f, "  - {} (from '{}')", e.name, e.parent)?;
                if let Some(d) = &e.description {
                    write!(f, "\n {d}")?;
                }
                writeln!(f)?;
            }
            for e in &self.step_envs {
                write!(f, "  - {} (from '{}')", e.name, e.parent)?;
                if let Some(d) = &e.description {
                    write!(f, "\n {d}")?;
                }
                writeln!(f)?;
            }
        }
        Ok(())
    }
}

struct StepSummaryResult {
    job_name: String,
    step_name: String,
    total_tasks: usize,
    task_params: Vec<(String, String)>,
    envs: Vec<EnvInfo>,
    deps: Vec<String>,
}

impl CliResult for StepSummaryResult {
    fn to_json_value(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();
        obj.insert("status".into(), "success".into());
        obj.insert(
            "message".into(),
            format!(
                "Summary for Step '{}' in Job '{}'",
                self.step_name, self.job_name
            )
            .into(),
        );
        obj.insert("job_name".into(), self.job_name.clone().into());
        obj.insert("step_name".into(), self.step_name.clone().into());
        obj.insert("total_tasks".into(), self.total_tasks.into());
        obj.insert("total_parameters".into(), self.task_params.len().into());
        obj.insert("total_environments".into(), self.envs.len().into());
        if !self.deps.is_empty() {
            obj.insert(
                "dependencies".into(),
                serde_json::json!(self
                    .deps
                    .iter()
                    .map(|d| serde_json::json!({"step_name": d}))
                    .collect::<Vec<_>>()),
            );
        }
        if !self.task_params.is_empty() {
            obj.insert(
                "parameter_definitions".into(),
                serde_json::json!(self
                    .task_params
                    .iter()
                    .map(|(n, t)| serde_json::json!({"name": n, "type": t}))
                    .collect::<Vec<_>>()),
            );
        }
        serde_json::Value::Object(obj)
    }
}

impl fmt::Display for StepSummaryResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f)?;
        writeln!(
            f,
            "--- Summary for Step '{}' in Job '{}' ---",
            self.step_name, self.job_name
        )?;
        writeln!(f)?;
        writeln!(f, "Total tasks: {}", self.total_tasks)?;
        writeln!(f, "Total task parameters: {}", self.task_params.len())?;
        writeln!(f, "Total environments: {}", self.envs.len())?;
        if !self.deps.is_empty() {
            writeln!(f)?;
            writeln!(f, "Dependencies ({}):", self.deps.len())?;
            for d in &self.deps {
                writeln!(f, "- '{d}'")?;
            }
        }
        if !self.task_params.is_empty() {
            writeln!(f)?;
            writeln!(f, "Parameters:")?;
            for (name, tp) in &self.task_params {
                writeln!(f, "- {name} ({tp})")?;
            }
        }
        if !self.envs.is_empty() {
            writeln!(f)?;
            writeln!(f, "Environments:")?;
            for e in &self.envs {
                write!(f, "- {} (from '{}')", e.name, e.parent)?;
                if let Some(d) = &e.description {
                    write!(f, "\n {d}")?;
                }
                writeln!(f)?;
            }
        }
        Ok(())
    }
}

// --- Data types ---

struct StepInfo {
    name: String,
    description: Option<String>,
    total_tasks: usize,
    task_params: Vec<(String, String)>,
    envs: Vec<String>,
    deps: Vec<String>,
}

struct ParamInfo {
    name: String,
    description: Option<String>,
    param_type: String,
    value: String,
}

struct EnvInfo {
    name: String,
    description: Option<String>,
    parent: String,
}
