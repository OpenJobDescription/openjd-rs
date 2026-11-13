// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! `job_diff`: diff two OpenJD templates and preview how the diff applies.
//!
//! ```text
//! cargo run -p openjd-model --example job_diff -- OLD.yaml NEW.yaml [options]
//!
//!   --max-depth N        do not expand nodes at depth N or deeper
//!   --no-symtab          do not compare resolvedSymTab (hides parameter changes in steps)
//!   --param K=V          job parameter for both sides (repeatable)
//!   --old-param K=V      job parameter for the old side only
//!   --new-param K=V      job parameter for the new side only
//!   --plan               print `job::apply::plan(old, new)` (job templates only;
//!                        it always diffs in full with default options)
//! ```
//!
//! Both files must be job templates, or both environment templates.
//! Sample inputs are in `examples/job_diff/`.
//!
//! For job templates, use `openjd diff` instead (`specs/cli/diff.md`): it
//! takes the CLI's parameter forms and extension allowlist, and prints
//! origins, task summaries and JSON.

use std::path::Path;
use std::process::ExitCode;

use openjd_model::job::apply::{plan, Action, Op, TaskChange, TaskOp};
use openjd_model::job::diff::{diff_environments, diff_jobs, DiffOptions, DiffTree};
use openjd_model::job::task_delta::TaskBox;
use openjd_model::job::TaskParamRange;
use openjd_model::job::{Job, TaskParameter};
use openjd_model::{
    convert_environment, create_job, decode_template, preprocess_job_parameters, CallerLimits,
    DecodedTemplate, JobParameterInputValues, PathParameterOptions,
};

struct Args {
    old: String,
    new: String,
    options: DiffOptions,
    old_params: Vec<(String, String)>,
    new_params: Vec<(String, String)>,
    plan: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut positional = Vec::new();
    let mut options = DiffOptions::default();
    let (mut old_params, mut new_params) = (Vec::new(), Vec::new());
    let mut plan = false;
    let mut it = std::env::args().skip(1);
    let kv = |s: Option<String>, flag: &str| -> Result<(String, String), String> {
        let s = s.ok_or_else(|| format!("{flag} needs K=V"))?;
        let (k, v) = s
            .split_once('=')
            .ok_or_else(|| format!("{flag} needs K=V, got {s}"))?;
        Ok((k.to_string(), v.to_string()))
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--max-depth" => {
                let n = it.next().ok_or("--max-depth needs N")?;
                let n = n.parse().map_err(|e| format!("--max-depth {n}: {e}"))?;
                options = options.with_max_depth(n);
            }
            "--no-symtab" => {
                options = options.with_resolved_symtab(false);
            }
            "--param" => {
                let p = kv(it.next(), "--param")?;
                old_params.push(p.clone());
                new_params.push(p);
            }
            "--old-param" => old_params.push(kv(it.next(), "--old-param")?),
            "--new-param" => new_params.push(kv(it.next(), "--new-param")?),
            "--plan" => plan = true,
            "-h" | "--help" => return Err(USAGE.into()),
            s if s.starts_with('-') => return Err(format!("unknown option {s}\n{USAGE}")),
            _ => positional.push(a),
        }
    }
    let [old, new]: [String; 2] = positional
        .try_into()
        .map_err(|_| format!("expected OLD and NEW\n{USAGE}"))?;
    Ok(Args {
        old,
        new,
        options,
        old_params,
        new_params,
        plan,
    })
}

const USAGE: &str = "usage: job_diff OLD NEW [--max-depth N] [--no-symtab] \
[--param K=V] [--old-param K=V] [--new-param K=V] [--plan]";

fn load(path: &str) -> Result<DecodedTemplate, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let v: serde_json::Value = serde_saphyr::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    decode_template(v, None, &CallerLimits::default()).map_err(|e| format!("{path}: {e}"))
}

fn instantiate(
    path: &str,
    jt: &openjd_model::template::JobTemplate,
    params: &[(String, String)],
) -> Result<Job, String> {
    let mut input = JobParameterInputValues::new();
    for (k, v) in params {
        input.insert(k.clone(), openjd_expr::ExprValue::String(v.clone()));
    }
    let dir = Path::new(path)
        .parent()
        .and_then(|p| std::fs::canonicalize(p).ok())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| ".".into());
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into());
    let values = preprocess_job_parameters(
        jt,
        &input,
        &[],
        &PathParameterOptions {
            job_template_dir: &dir,
            current_working_dir: &cwd,
            path_format: openjd_expr::path_mapping::PathFormat::host(),
            allow_template_dir_walk_up: true,
            allow_uri_path_values: true,
        },
    )
    .map_err(|e| format!("{path}: {e}"))?;
    create_job(jt, &values, &jt.default_validation_context()).map_err(|e| format!("{path}: {e}"))
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    match (load(&args.old)?, load(&args.new)?) {
        (DecodedTemplate::Job(a), DecodedTemplate::Job(b)) => {
            let old = instantiate(&args.old, &a, &args.old_params)?;
            let new = instantiate(&args.new, &b, &args.new_params)?;
            let tree = diff_jobs(&old, &new, &args.options);
            print_tree(&tree);
            print_steps(&tree);
            if args.plan {
                // The plan always diffs in full with default options.
                print_plan(&old, &new)?;
            }
        }
        (DecodedTemplate::Environment(a), DecodedTemplate::Environment(b)) => {
            let old = convert_environment(a.environment());
            let new = convert_environment(b.environment());
            print_tree(&diff_environments(&old, &new, &args.options));
        }
        _ => {
            return Err(
                "OLD and NEW must both be job templates or both environment templates".into(),
            )
        }
    }
    Ok(())
}

fn print_tree(tree: &DiffTree) {
    println!("== diff ({} nodes) ==", tree.nodes().len());
    if tree.is_changed() {
        print!("{tree}");
    } else {
        println!("no changes");
    }
}

/// Every step the diff saw, with its name and its position on each side.
fn print_steps(tree: &DiffTree) {
    let steps: Vec<_> = tree
        .root()
        .children
        .iter()
        .filter_map(|&c| tree.node(c))
        .filter(|n| n.segment.starts_with("steps["))
        .collect();
    if steps.is_empty() {
        return;
    }
    println!("\n== step identity ==");
    println!(
        "{:<10} {:<10} {:>5} {:>5}",
        "name", "status", "old#", "new#"
    );
    let idx = |i: Option<usize>| i.map_or("-".into(), |i| i.to_string());
    for n in steps {
        println!(
            "{:<10} {:<10} {:>5} {:>5}",
            n.key.as_deref().unwrap_or("?"),
            format!("{:?}", n.status),
            idx(n.old_index),
            idx(n.new_index)
        );
    }
}

fn print_plan(old: &Job, new: &Job) -> Result<(), String> {
    println!("\n== apply plan (job::apply::plan) ==");
    let plan = plan(old, new).map_err(|e| e.to_string())?;
    if plan.ops.is_empty() {
        println!("nothing to apply");
        return Ok(());
    }
    for (n, op) in plan.ops.iter().enumerate() {
        let kinds: Vec<String> = op.kinds.iter().map(|k| format!("{k:?}")).collect();
        let what = match &op.action {
            Action::Job(_) => "job        MODIFY".to_string(),
            Action::Parameter(o) => format!("param      {}", name_of(o, |p| &p.name)),
            Action::Environment(i, o) => format!("jobEnv#{i:<2}  {}", name_of(o, |e| &e.name)),
            Action::Step(s, o) => {
                format!("step#{:<2}    {} {}", s.index, verb(o), s.name)
            }
            Action::Tasks(step, t) => {
                format!("step#{:<2}    TASKS  {}", step.index, describe_change(t))
            }
            _ => "?".into(),
        };
        println!("{:>3}. {what:<60} [{}]", n + 1, kinds.join(", "));
    }
    println!("     kinds needed: {:?}", plan.kinds());
    Ok(())
}

fn verb<T>(o: &Op<'_, T>) -> &'static str {
    match o {
        Op::Add { .. } => "ADD   ",
        Op::Modify { .. } => "MODIFY",
        Op::Remove { .. } => "REMOVE",
    }
}

fn name_of<'a, T>(op: &Op<'a, T>, f: impl Fn(&'a T) -> &'a String) -> String {
    match *op {
        Op::Add { new } => format!("ADD    {}", f(new)),
        Op::Modify { new, .. } => format!("MODIFY {}", f(new)),
        Op::Remove { old } => format!("REMOVE {}", f(old)),
    }
}

fn describe_change(t: &TaskOp<'_>) -> String {
    match &t.change {
        TaskChange::Boxes(d) => {
            let mut parts = vec![format!("keep {} [{}]", d.kept.len(), describe(&d.kept))];
            for b in &d.added {
                parts.push(format!("add {} [{}]", b.len(), describe(b)));
            }
            for b in &d.removed {
                parts.push(format!("remove {} [{}]", b.len(), describe(b)));
            }
            parts.join("; ")
        }
        TaskChange::All => format!("all {}", t.added_count().unwrap_or(0)),
        TaskChange::Replace => format!(
            "replace all with {} (no per-task mapping)",
            t.added_count().unwrap_or(0)
        ),
    }
}

/// One line per box: `Frame∈{11..12} × Cam∈{B,C}`.
fn describe(b: &TaskBox) -> String {
    let dims: Vec<String> = b
        .space()
        .task_parameter_definitions
        .iter()
        .map(|(name, p)| {
            let vals: Vec<String> = match p {
                TaskParameter::Int {
                    range: TaskParamRange::List(v),
                    ..
                } => return format!("{name}∈{}", int_runs(v)),
                TaskParameter::String { range } | TaskParameter::Path { range } => range.clone(),
                TaskParameter::Float { range } => {
                    range.iter().map(|f| f.to_display_string()).collect()
                }
                other => vec![format!("{other:?}")],
            };
            format!("{name}∈{{{}}}", vals.join(","))
        })
        .collect();
    if dims.is_empty() {
        return "(no params)".into();
    }
    dims.join(" × ")
}

/// `[1,2,3,7,8]` -> `{1..3,7..8}`.
fn int_runs(v: &[i64]) -> String {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < v.len() {
        let mut j = i;
        while j + 1 < v.len() && v[j + 1] == v[j] + 1 {
            j += 1;
        }
        parts.push(if i == j {
            v[i].to_string()
        } else {
            format!("{}..{}", v[i], v[j])
        });
        i = j + 1;
    }
    format!("{{{}}}", parts.join(","))
}
