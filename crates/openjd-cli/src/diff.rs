// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! `openjd diff` command — diff two templates, one template under two
//! parameter sets, or two built jobs.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;

use clap::Args;
use openjd_model::job::apply::{Action, ApplyPlan, Op, TaskChange};
use openjd_model::job::diff::DiffOptions;
use openjd_model::job::template_diff::{
    diff_built_jobs, diff_job_template_with_parameter_sets, diff_templates, job_from_str, Change,
    Note, StepSummary, TemplateDiff, TemplateInput, TemplateSource,
};
use openjd_model::job::Job;
use serde::Serialize;

/// Exit code of `--exit-code` when the inputs differ.
const EXIT_DIFFERENT: i32 = 4;

/// Printed to stderr when the command starts; stdout stays the diff.
const UNSTABLE_WARNING: &str =
    "warning: 'openjd diff' is unstable; its flags and output may change.";

const EXAMPLES: &str = "\
Examples, from the openjd-rs repository root with E=crates/openjd-cli/examples/diff
and M=crates/openjd-model/examples/template_diff:
  openjd diff $E/frames.yaml --old-param Frames=1-100 --new-param Frames=1-200
  openjd diff $M/render_v1.yaml $M/render_v2.yaml -p Scene=//srv/s
  openjd diff $E/let_script_v1.yaml $E/let_script_v2.yaml
  openjd diff $E/extensions_v1.yaml $E/extensions_v2.yaml --plan
  openjd build $E/frames.yaml -p Frames=1-100 > old.json
  openjd diff --jobs old.json $E/frames_1_200.json --plan
  openjd diff $M/render_v1.yaml $M/render_v2.yaml -p Scene=//srv/s --stat
  openjd diff old.yaml - --output json --exit-code < new.yaml

Every field the diff reports, with its output: specs/cli/diff-examples.md";

#[derive(Args)]
#[command(after_help = EXAMPLES)]
pub struct DiffArgs {
    /// Old template file, or `-` for stdin
    pub old: String,

    /// New template file, or `-` for stdin. Omit to diff OLD against itself
    /// under --old-param and --new-param
    pub new: Option<String>,

    /// Job parameter for both sides (Key=Value, file://path, or inline JSON)
    #[arg(short = 'p', long = "job-param", alias = "parameter")]
    pub parameters: Vec<String>,

    /// Job parameter for the old side only; wins over --job-param
    #[arg(long = "old-param")]
    pub old_params: Vec<String>,

    /// Job parameter for the new side only; wins over --job-param
    #[arg(long = "new-param")]
    pub new_params: Vec<String>,

    /// Environment template file, for both sides
    #[arg(long = "environment", alias = "env")]
    pub environments: Vec<String>,

    /// Extensions to support (comma-separated). Empty string disables all
    #[arg(long = "extensions")]
    pub extensions: Option<String>,

    /// OLD and NEW are built jobs (the output of `openjd build`), not templates
    #[arg(long, requires = "new", conflicts_with_all = ["parameters", "old_params", "new_params", "environments", "extensions"])]
    pub jobs: bool,

    /// Show only this step and the job-level changes
    #[arg(long)]
    pub step: Option<String>,

    /// Show only the task change of each step
    #[arg(long, conflicts_with = "stat")]
    pub tasks: bool,

    /// Show one line per changed step and the totals
    #[arg(long)]
    pub stat: bool,

    /// Also print the apply plan and the mutation kinds it needs
    #[arg(long)]
    pub plan: bool,

    /// Do not expand the job diff below this depth
    #[arg(long)]
    pub depth: Option<usize>,

    /// Do not compare resolved symbol tables (hides parameter changes in steps)
    #[arg(long)]
    pub ignore_symbols: bool,

    /// How to format the output
    #[arg(long = "output", value_parser = ["human-readable", "json", "yaml"], default_value = "human-readable")]
    pub output: String,

    /// Also print the job diff tree; with --output json or yaml, also include
    /// the two jobs
    #[arg(long)]
    pub verbose: bool,

    /// Exit 0 when there are no differences and 4 when there are (errors exit 1);
    /// with --step, only the differences shown count
    #[arg(long)]
    pub exit_code: bool,
}

impl DiffArgs {
    /// A combination clap cannot reject on its own. At `--depth 0` the only
    /// change is the unexpanded job root, which `--step` cannot split.
    fn usage_error(&self) -> Option<&'static str> {
        (self.step.is_some() && self.depth == Some(0)).then_some(
            "--step cannot be used with --depth 0: the job root is not expanded, so it cannot be split by step",
        )
    }
}

/// Run `openjd diff`; returns the process exit code.
pub fn execute(args: DiffArgs) -> Result<i32, Box<dyn std::error::Error>> {
    if let Some(message) = args.usage_error() {
        clap::Error::raw(
            clap::error::ErrorKind::ArgumentConflict,
            format!("{message}\n"),
        )
        .exit();
    }
    eprintln!("{UNSTABLE_WARNING}");
    let mut options = DiffOptions::default().with_resolved_symtab(!args.ignore_symbols);
    if let Some(n) = args.depth {
        options = options.with_max_depth(n);
    }
    let mut stdin_used = false;
    let (header, mut diff) = if args.jobs {
        // clap requires NEW with --jobs.
        let new = args
            .new
            .as_deref()
            .ok_or("--jobs needs both OLD and NEW.")?;
        let old_job = read_job(&args.old, &mut stdin_used)?;
        let new_job = read_job(new, &mut stdin_used)?;
        (
            format!("jobs: {} -> {new}", args.old),
            diff_built_jobs(old_job, new_job, &options),
        )
    } else {
        let side = |extra: &[String]| {
            let all: Vec<String> = args.parameters.iter().chain(extra).cloned().collect();
            crate::run::parse_cli_parameters(&all)
        };
        let (old_params, new_params) = (side(&args.old_params)?, side(&args.new_params)?);
        let exts = crate::common::parse_extensions(&args.extensions)?;
        let exts: Vec<&str> = exts.iter().map(String::as_str).collect();
        // Each environment is read once and shared by both sides, so one of
        // them can come from stdin.
        let environments = args
            .environments
            .iter()
            .map(|env| read_template(env, &mut stdin_used))
            .collect::<Result<Vec<_>, _>>()?;
        let mut base = |arg: &str| -> Result<TemplateInput, Box<dyn std::error::Error>> {
            let mut input = TemplateInput::new(read_template(arg, &mut stdin_used)?)
                .extensions(&exts)
                .limits(crate::common::caller_limits());
            for env in &environments {
                input = input.environment(env.clone());
            }
            Ok(input)
        };
        match args.new.as_deref() {
            None => (
                format!("{}: old parameters -> new parameters", args.old),
                diff_job_template_with_parameter_sets(
                    &base(&args.old)?,
                    &old_params,
                    &new_params,
                    &options,
                )?,
            ),
            Some(new) => {
                let old_input = base(&args.old)?.params(old_params);
                let new_input = base(new)?.params(new_params);
                (
                    format!("{} -> {new}", args.old),
                    diff_templates(&old_input, &new_input, &options)?,
                )
            }
        }
    };
    if let Some(step) = &args.step {
        keep_step(&mut diff, step)?;
    }
    let plan = if args.plan { Some(diff.plan()?) } else { None };
    let plan = plan
        .as_ref()
        .map(|p| plan_doc(&diff, p, args.step.as_deref()));
    match args.output.as_str() {
        "json" | "yaml" => print_document(&diff, plan, &args)?,
        _ => print_human(&header, &diff, plan.as_ref(), &args),
    }
    Ok(if args.exit_code && diff.is_changed() {
        EXIT_DIFFERENT
    } else {
        0
    })
}

/// Read stdin once; a second `-` is an error.
fn read_stdin(stdin_used: &mut bool) -> Result<String, Box<dyn std::error::Error>> {
    if *stdin_used {
        return Err("Only one input can be read from stdin ('-').".into());
    }
    *stdin_used = true;
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text)?;
    Ok(text)
}

fn read_template(
    arg: &str,
    stdin_used: &mut bool,
) -> Result<TemplateSource, Box<dyn std::error::Error>> {
    if arg == "-" {
        return Ok(TemplateSource::Text(read_stdin(stdin_used)?));
    }
    // Read once here for the CLI's usual "does not exist" message; the
    // library reads the file again so its directory sets relative PATH defaults.
    crate::common::read_input_file(Path::new(arg))?;
    Ok(TemplateSource::Path(arg.into()))
}

fn read_job(arg: &str, stdin_used: &mut bool) -> Result<Job, Box<dyn std::error::Error>> {
    let text = if arg == "-" {
        read_stdin(stdin_used)?
    } else {
        crate::common::read_input_file(Path::new(arg))?
    };
    job_from_str(&text).map_err(|e| format!("'{arg}': {e}").into())
}

/// Keep the changes, summary and notes of one step, and the job-level ones.
/// Each is matched by the step it belongs to ([`Change::step`],
/// [`Note::step`]), not by its path, so `A` does not keep `A].x`.
fn keep_step(diff: &mut TemplateDiff, step: &str) -> Result<(), String> {
    if diff.step(step).is_none() {
        return Err(format!("Step '{step}' does not exist in either job."));
    }
    let kept = |owner: &Option<String>| owner.as_deref().is_none_or(|s| s == step);
    diff.changes.retain(|c| kept(&c.step));
    diff.steps.retain(|s| s.name == step);
    diff.notes.retain(|n| kept(&n.step));
    Ok(())
}

fn shown_steps(diff: &TemplateDiff) -> impl Iterator<Item = &StepSummary> {
    diff.steps.iter().filter(|s| {
        s.status != openjd_model::job::diff::DiffStatus::Unchanged
            || diff.changes_in_step(&s.name).next().is_some()
    })
}

fn print_human(header: &str, diff: &TemplateDiff, plan: Option<&PlanDoc>, args: &DiffArgs) {
    println!("{header}");
    if !(args.tasks || args.stat) {
        // `Display` ends with the notes.
        println!("{diff}");
    } else {
        if !diff.is_changed() {
            println!("no changes");
        } else {
            let lines: Vec<String> = shown_steps(diff)
                .map(|s| {
                    if args.stat {
                        s.stat_line()
                    } else {
                        s.to_string()
                    }
                })
                .collect();
            if lines.is_empty() {
                println!("job-level changes only");
            } else {
                println!("{}", lines.join("\n"));
            }
            if args.stat {
                println!("{}", diff.stat());
            }
        }
        for n in &diff.notes {
            println!("note: {n}");
        }
    }
    if let Some(plan) = plan {
        println!("\napply plan:");
        if plan.ops.is_empty() {
            println!("  nothing to apply");
        }
        for (n, op) in plan.ops.iter().enumerate() {
            println!("  {}. {}  [{}]", n + 1, op.action, op.kinds.join(", "));
        }
        if plan.kinds.is_empty() {
            println!("kinds: none");
        } else {
            println!("kinds: {}", plan.kinds.join(", "));
        }
    }
    if args.verbose {
        println!("\njob diff tree:");
        if diff.job.is_changed() {
            print!("{}", diff.job);
        } else {
            println!("no changes");
        }
    }
}

/// One op of the plan, for printing.
#[derive(Serialize)]
struct PlanOp {
    action: String,
    kinds: Vec<String>,
}

#[derive(Serialize)]
struct PlanDoc {
    ops: Vec<PlanOp>,
    kinds: Vec<String>,
}

/// The plan's ops, for printing. With `--step`, only that step's ops and
/// the job-level ones (job fields, parameters, job environments); `kinds`
/// is the union over the ops kept.
fn plan_doc(diff: &TemplateDiff, plan: &ApplyPlan<'_>, step: Option<&str>) -> PlanDoc {
    let step_name = |i: usize, removed: bool| {
        let job = if removed {
            &diff.old_job
        } else {
            &diff.new_job
        };
        job.steps.get(i).map_or("?", |s| s.name.as_str())
    };
    let mut kinds = BTreeSet::new();
    let mut ops = Vec::new();
    for op in &plan.ops {
        let (action, of_step) = match &op.action {
            Action::Job(_) => ("job modify".to_string(), None),
            Action::Parameter(o) => (
                format!("parameter {} {}", verb(o), op_name(o, |p| &p.name)),
                None,
            ),
            Action::Environment(_, o) => (
                format!("job environment {} {}", verb(o), op_name(o, |e| &e.name)),
                None,
            ),
            Action::Step(i, o) => {
                let name = step_name(*i, matches!(o, Op::Remove { .. }));
                (format!("step {} {name}", verb(o)), Some(name))
            }
            Action::Tasks { step, change, .. } => {
                let name = step_name(*step, false);
                (
                    format!("tasks of {name}: {}", task_change(change)),
                    Some(name),
                )
            }
            _ => ("other".to_string(), None),
        };
        if step.is_some() && of_step.is_some() && of_step != step {
            continue;
        }
        kinds.extend(op.kinds.iter().copied());
        ops.push(PlanOp {
            action,
            kinds: op.kinds.iter().map(|k| format!("{k:?}")).collect(),
        });
    }
    PlanDoc {
        ops,
        kinds: kinds.iter().map(|k| format!("{k:?}")).collect(),
    }
}

fn verb<T>(op: &Op<'_, T>) -> &'static str {
    match op {
        Op::Add { .. } => "add",
        Op::Modify { .. } => "modify",
        Op::Remove { .. } => "remove",
    }
}

fn op_name<'a, T>(op: &Op<'a, T>, name: impl Fn(&'a T) -> &'a String) -> &'a str {
    match *op {
        Op::Add { new } | Op::Modify { new, .. } => name(new),
        Op::Remove { old } => name(old),
    }
}

fn task_change(change: &TaskChange) -> String {
    match change {
        TaskChange::Boxes(d) => {
            let added: u64 = d.added.iter().map(|b| b.len()).sum();
            let removed: u64 = d.removed.iter().map(|b| b.len()).sum();
            let start = d.old.len();
            let at = if added > 0 {
                format!(
                    " at index {start}..{}",
                    start.saturating_add(added).saturating_sub(1)
                )
            } else {
                String::new()
            };
            format!("keep {}, add {added}{at}, remove {removed}", d.kept.len())
        }
        TaskChange::All => "all".to_string(),
        TaskChange::Replace => "replace all".to_string(),
    }
}

/// The `--output json|yaml` document: whether anything differs, then the
/// diff's serde form without the jobs and the tree, unless `--verbose`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Brief<'a> {
    changed: bool,
    changes: &'a [Change],
    steps: &'a [StepSummary],
    notes: &'a [Note],
    #[serde(skip_serializing_if = "Option::is_none")]
    plan: Option<PlanDoc>,
}

#[derive(Serialize)]
struct Full<'a> {
    changed: bool,
    #[serde(flatten)]
    diff: &'a TemplateDiff,
    #[serde(skip_serializing_if = "Option::is_none")]
    plan: Option<PlanDoc>,
}

fn print_document(
    diff: &TemplateDiff,
    plan: Option<PlanDoc>,
    args: &DiffArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    // Serialized directly, not through `serde_json::Value`, so the jobs keep
    // their map order (task parameter definition order is task order).
    let json = args.output == "json";
    let changed = diff.is_changed();
    let text = if args.verbose {
        let doc = Full {
            changed,
            diff,
            plan,
        };
        if json {
            serde_json::to_string_pretty(&doc)?
        } else {
            serde_saphyr::to_string(&doc)?
        }
    } else {
        let doc = Brief {
            changed,
            changes: &diff.changes,
            steps: &diff.steps,
            notes: &diff.notes,
            plan,
        };
        if json {
            serde_json::to_string_pretty(&doc)?
        } else {
            serde_saphyr::to_string(&doc)?
        }
    };
    if json {
        println!("{text}");
    } else {
        print!("{text}");
    }
    Ok(())
}
