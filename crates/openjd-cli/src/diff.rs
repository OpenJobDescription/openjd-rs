// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! `openjd diff` command — diff two templates, or one template under two
//! parameter sets.

use std::collections::{BTreeSet, HashSet};
use std::io::Read;
use std::path::Path;

use clap::Args;
use openjd_model::job::apply::{Action, ApplyPlan, Op, TaskChange};
use openjd_model::job::diff::DiffOptions;
use openjd_model::job::template_diff::{
    diff_job_template_with_parameter_sets, diff_templates, Change, Note, StepSummary, TemplateDiff,
    TemplateInput, TemplateSource,
};
use openjd_model::template::{parse, EnvironmentTemplate};
use serde::Serialize;

/// Exit code of `--exit-code` when the inputs differ.
const EXIT_DIFFERENT: i32 = 4;

/// Printed to stderr when the command starts; stdout stays the diff.
const UNSTABLE_WARNING: &str =
    "warning: 'openjd diff' is unstable; its flags and output may change.";

const EXAMPLES: &str = "\
Examples, from the openjd-rs repository root with E=crates/openjd-cli/examples/diff:
  openjd diff $E/frame_range.yaml --old-parameter Frames=1-3 --new-parameter Frames=1-5
  openjd diff $E/render_v1.yaml $E/render_v2.yaml --new-parameter Frames=1-4
  openjd diff $E/tasks_v1.yaml $E/tasks_v2.yaml --plan
  openjd diff $E/pipeline_v1.yaml $E/pipeline_v2.yaml -p file://$E/shot.json --new-parameter '{\"Quality\": null}'
  openjd diff $E/tasks_v1.yaml - --output json --exit-code < $E/tasks_v2.yaml

A walkthrough of these examples with their output: crates/openjd-cli/examples/diff/README.md";

#[derive(Args)]
#[command(after_help = EXAMPLES)]
pub struct DiffArgs {
    /// Template file, or `-` for stdin. With only this template, the old and
    /// new sides differ only by --old-parameter and --new-parameter
    #[arg(value_name = "TEMPLATE")]
    pub old: String,

    /// New template file, or `-` for stdin; TEMPLATE is then the old side
    #[arg(value_name = "NEW_TEMPLATE")]
    pub new: Option<String>,

    /// Job parameter for both sides: Key=Value, a JSON or YAML file as
    /// file://path, or inline JSON. Repeatable; a later value wins. Examples:
    /// -p Frames=1-200, -p file://params.json, -p '{"Frames": "1-200"}'
    #[arg(
        short = 'p',
        long = "parameter",
        alias = "job-param",
        value_name = "PARAM"
    )]
    pub parameters: Vec<String>,

    /// Job parameter for the old side only, in the same forms as -p; wins
    /// over -p. Example: --old-parameter Frames=1-100
    #[arg(long = "old-parameter", alias = "old-param", value_name = "PARAM")]
    pub old_params: Vec<String>,

    /// Job parameter for the new side only, in the same forms as -p; wins
    /// over -p. Example: --new-parameter Frames=1-200
    #[arg(long = "new-parameter", alias = "new-param", value_name = "PARAM")]
    pub new_params: Vec<String>,

    /// Environment template file, for both sides
    #[arg(long = "environment", alias = "env")]
    pub environments: Vec<String>,

    /// Extensions to support (comma-separated). Empty string disables all
    #[arg(long = "extensions")]
    pub extensions: Option<String>,

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
    let exts = crate::common::parse_extensions(&args.extensions)?;
    let exts: Vec<&str> = exts.iter().map(String::as_str).collect();
    // Each environment is read once and shared by both sides, so one of
    // them can come from stdin.
    let environments = args
        .environments
        .iter()
        .map(|env| read_template(env, &mut stdin_used))
        .collect::<Result<Vec<_>, _>>()?;
    let decoded_environments: Vec<_> = environments
        .iter()
        .filter_map(|env| decode_environment(env, &exts))
        .collect();
    let old_source = read_template(&args.old, &mut stdin_used)?;
    let new_side = match args.new.as_deref() {
        Some(new) => Some((new, read_template(new, &mut stdin_used)?)),
        None => None,
    };
    // Each side's FLOAT names come from its own templates.
    let old_floats = float_names(&old_source, &decoded_environments, &exts);
    let new_floats = match &new_side {
        Some((_, source)) => float_names(source, &decoded_environments, &exts),
        None => old_floats.clone(),
    };
    // `-p` first, so a later side-specific value wins over it.
    let side = |extra: &[String], floats: &HashSet<String>| {
        let all: Vec<String> = args.parameters.iter().chain(extra).cloned().collect();
        crate::run::parse_cli_parameters(&all, floats)
    };
    let old_params = side(&args.old_params, &old_floats)?;
    let new_params = side(&args.new_params, &new_floats)?;
    let base = |source: TemplateSource| -> TemplateInput {
        let mut input = TemplateInput::new(source)
            .extensions(&exts)
            .limits(crate::common::caller_limits());
        for env in &environments {
            input = input.environment(env.clone());
        }
        input
    };
    let (header, mut diff) = match new_side {
        Some((new, new_source)) => {
            let old_input = base(old_source).params(old_params);
            let new_input = base(new_source).params(new_params);
            (
                format!("{} -> {new}", args.old),
                diff_templates(&old_input, &new_input, &options)?,
            )
        }
        None => (
            format!("{}: old parameters -> new parameters", args.old),
            diff_job_template_with_parameter_sets(
                &base(old_source),
                &old_params,
                &new_params,
                &options,
            )?,
        ),
    };
    if let Some(step) = &args.step {
        keep_step(&mut diff, step)?;
    }
    let plan = if args.plan { Some(diff.plan()?) } else { None };
    let plan = plan.as_ref().map(|p| plan_doc(p, args.step.as_deref()));
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

/// The FLOAT parameter names of a job template and the environments, for
/// `parse_cli_parameters`. A template that does not decode gives none: the
/// diff then reports its decode error, with its side.
fn float_names(
    source: &TemplateSource,
    environments: &[EnvironmentTemplate],
    exts: &[&str],
) -> HashSet<String> {
    let limits = crate::common::caller_limits();
    let template = source_document(source)
        .and_then(|doc| parse::decode_job_template(doc, Some(exts), &limits).ok());
    match template {
        Some(t) => crate::run::float_parameter_names(&t, environments),
        None => HashSet::new(),
    }
}

/// An environment template, decoded for [`float_names`]; `None` when it
/// does not decode, which the diff then reports.
fn decode_environment(source: &TemplateSource, exts: &[&str]) -> Option<EnvironmentTemplate> {
    let limits = crate::common::caller_limits();
    source_document(source)
        .and_then(|doc| parse::decode_environment_template(doc, Some(exts), &limits).ok())
}

/// The document of a source `read_template` returned, read as the library
/// reads it: a path is JSON when its extension is `.json`, text when it
/// starts with `{`.
fn source_document(source: &TemplateSource) -> Option<serde_json::Value> {
    let limits = crate::common::caller_limits();
    let (text, kind) = match source {
        TemplateSource::Path(p) => (
            std::fs::read_to_string(p).ok()?,
            crate::common::document_type(p),
        ),
        TemplateSource::Text(t) => {
            let kind = if t.trim_start().starts_with('{') {
                parse::DocumentType::Json
            } else {
                parse::DocumentType::Yaml
            };
            (t.clone(), kind)
        }
        _ => return None,
    };
    parse::document_string_to_object(&text, kind, &limits).ok()
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
fn plan_doc(plan: &ApplyPlan<'_>, step: Option<&str>) -> PlanDoc {
    let mut kinds = BTreeSet::new();
    let mut ops = Vec::new();
    for op in &plan.ops {
        let (action, of_step) = match &op.action {
            Action::Job(_) => ("job modify".to_string(), None),
            Action::Parameter(o) => (
                format!("parameter {} {}", o.verb(), op_name(o, |p| &p.name)),
                None,
            ),
            Action::Environment(_, o) => (
                format!("job environment {} {}", o.verb(), op_name(o, |e| &e.name)),
                None,
            ),
            Action::Step(s, o) => (format!("step {} {}", o.verb(), s.name), Some(s.name)),
            Action::Tasks(step, t) => (
                format!("tasks of {}: {}", step.name, task_change(&t.change)),
                Some(step.name),
            ),
            _ => ("other".to_string(), None),
        };
        if step.is_some() && of_step.is_some() && of_step != step {
            continue;
        }
        kinds.extend(op.kinds.iter().copied());
        ops.push(PlanOp {
            action,
            kinds: op.kinds.iter().map(ToString::to_string).collect(),
        });
    }
    PlanDoc {
        ops,
        kinds: kinds.iter().map(ToString::to_string).collect(),
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
            format!(
                "keep {}, add {}, remove {}",
                d.kept.len(),
                d.appended_count(),
                d.removed_count()
            )
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
