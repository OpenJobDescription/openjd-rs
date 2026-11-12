// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! The `openjd diff` example gallery, `specs/cli/diff-examples.md`.
//!
//! Every case runs the real binary from the repository root on the files in
//! `crates/openjd-cli/examples/diff/` (and the model's
//! `examples/template_diff/`), and the gallery is compared with the checked-in
//! file. `UPDATE_DIFF_EXAMPLES=1` rewrites it, and the two built jobs the
//! `--jobs` case reads, instead of comparing.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

const GALLERY: &str = "specs/cli/diff-examples.md";
const UPDATE: &str = "UPDATE_DIFF_EXAMPLES";
const WARNING: &str = "warning: 'openjd diff' is unstable; its flags and output may change.\n";
const BUILD_WARNING: &str =
    "warning: 'openjd build' is unstable; its flags and output may change.\n";

/// The two jobs of the `--jobs` case, and the `openjd build` arguments that
/// print them.
const BUILT_JOBS: &[(&str, &str)] = &[
    (
        "crates/openjd-cli/examples/diff/frames_1_100.json",
        "build crates/openjd-cli/examples/diff/frames.yaml -p Frames=1-100",
    ),
    (
        "crates/openjd-cli/examples/diff/frames_1_200.json",
        "build crates/openjd-cli/examples/diff/frames.yaml -p Frames=1-200",
    ),
];

struct Case {
    group: &'static str,
    id: &'static str,
    /// The changed field, for the coverage table.
    field: &'static str,
    /// One sentence saying what changed.
    what: &'static str,
    /// Arguments after `openjd`, split on spaces. `@` stands for
    /// `crates/openjd-cli/examples/diff/` and `%` for
    /// `crates/openjd-model/examples/template_diff/`.
    command: &'static str,
    /// Text that picks the coverage table's "shown as" line from the output.
    shown: &'static str,
    /// Also show the `--plan` output and the `--output json` document.
    rich: bool,
}

const fn case(
    group: &'static str,
    id: &'static str,
    field: &'static str,
    what: &'static str,
    command: &'static str,
    shown: &'static str,
) -> Case {
    Case {
        group,
        id,
        field,
        what,
        command,
        shown,
        rich: false,
    }
}

const fn rich(c: Case) -> Case {
    Case { rich: true, ..c }
}

const JOB: &str = "Job";
const DEFS: &str = "Parameter definitions";
const VALUES: &str = "Parameter values";
const ENVS: &str = "Job environments";
const STEPS: &str = "Steps";
const TASKS: &str = "Tasks";
const STAGE: &str = "Stage";
const ORIGIN: &str = "Origin";
const FORMS: &str = "Other inputs and filters";

const CASES: &[Case] = &[
    case(JOB, "job_name", "job `name`", "The job is renamed.",
        "diff @job_name_v1.yaml @job_name_v2.yaml", "~ name:"),
    case(JOB, "job_description", "job `description`", "The job description changes.",
        "diff @job_description_v1.yaml @job_description_v2.yaml", "~ description:"),
    rich(case(JOB, "extension_added", "`extensions` added", "The template starts using the `EXPR` extension.",
        "diff @extensions_v1.yaml @extensions_v2.yaml", "+ extensions:")),
    rich(case(JOB, "extension_removed", "`extensions` removed", "The template stops using the `EXPR` extension.",
        "diff @extensions_v2.yaml @extensions_v1.yaml", "- extensions:")),
    case(JOB, "extension_reordered", "`extensions` order", "The same two extensions are listed in the other order.",
        "diff @extensions_order_v1.yaml @extensions_order_v2.yaml", "~ extensions:"),
    case(DEFS, "param_def_added", "parameter definition added", "A parameter with a default is added; no step reads it.",
        "diff @param_added_v1.yaml @param_added_v2.yaml", "+ parameter Quality"),
    case(DEFS, "param_def_removed", "parameter definition removed", "The parameter is removed again.",
        "diff @param_added_v2.yaml @param_added_v1.yaml", "- parameter Quality"),
    case(DEFS, "param_def_type", "parameter `type`", "`Count` changes from `STRING` to `INT`, and its default from `\"5\"` to `5`.",
        "diff @param_type_v1.yaml @param_type_v2.yaml", "Count.type"),
    case(DEFS, "param_def_description", "parameter `description`", "The parameter description changes; the job does not.",
        "diff @param_description_v1.yaml @param_description_v2.yaml", "Quality.description"),
    case(DEFS, "param_def_default", "parameter `default`, not supplied",
        "The `Frames` default changes and no value is supplied, so the job changes and the template is the cause.",
        "diff %render_v1.yaml %render_default_1_200.yaml -p Scene=//srv/s", "Frames.default"),
    case(DEFS, "param_def_default_supplied", "parameter `default`, supplied",
        "The same default change with `Frames` supplied on both sides: only the definition differs.",
        "diff %render_v1.yaml %render_default_1_200.yaml -p Scene=//srv/s -p Frames=1-100", "Frames.default"),
    case(DEFS, "param_def_allowed_values", "parameter `allowedValues`", "A value is added to `allowedValues`.",
        "diff @allowed_values_v1.yaml @allowed_values_v2.yaml", "allowedValues"),
    case(DEFS, "param_def_min_max_value", "parameter `minValue`, `maxValue`", "Both bounds of an `INT` parameter change.",
        "diff @min_max_value_v1.yaml @min_max_value_v2.yaml", "minValue"),
    case(DEFS, "param_def_min_max_length", "parameter `minLength`, `maxLength`", "Both length bounds of a `STRING` parameter change.",
        "diff @min_max_length_v1.yaml @min_max_length_v2.yaml", "minLength"),
    case(DEFS, "param_def_user_interface", "parameter `userInterface`", "The label changes and a group label is added.",
        "diff @user_interface_v1.yaml @user_interface_v2.yaml", "userInterface.label"),
    case(DEFS, "param_def_path_kind", "parameter `objectType`, `dataFlow`", "A `PATH` parameter changes from an input file to an output directory.",
        "diff @path_kind_v1.yaml @path_kind_v2.yaml", "objectType"),
    case(DEFS, "param_def_list_item", "parameter `item.*` (LIST)", "A `LIST[STRING]` parameter allows one more item value.",
        "diff @list_item_v1.yaml @list_item_v2.yaml", "item.allowedValues"),
    case(DEFS, "param_def_list_list_item", "parameter `item.item.*` (LIST[LIST[INT]])", "The inner lists of a `LIST[LIST[INT]]` parameter allow larger values.",
        "diff @list_list_item_v1.yaml @list_list_item_v2.yaml", "item.item.maxValue"),
    rich(case(VALUES, "param_value", "parameter value", "One template, `Frames` 1-100 on the old side and 1-200 on the new.",
        "diff @frames.yaml --old-param Frames=1-100 --new-param Frames=1-200", "~ parameter Frames")),
    case(VALUES, "param_value_path", "`PATH` parameter value",
        "A `PATH` parameter changes; the step sees it as `RawParam.Scene` in its resolved symbol table.",
        "diff %render_v1.yaml --old-param Scene=//srv/a --new-param Scene=//srv/b", "RawParam.Scene"),
    case(VALUES, "param_value_env", "parameter read by a job environment",
        "A job environment's variable reads the parameter, so the environment's symbol table changes.",
        "diff @env_param.yaml --old-param Tool=v1 --new-param Tool=v2", "jobEnvironments[Tools].resolvedSymTab"),
    case(ENVS, "env_added", "job environment added", "A job environment is added.",
        "diff @env_added_v1.yaml @env_added_v2.yaml", "+ jobEnvironments[Tools]"),
    case(ENVS, "env_removed", "job environment removed", "The job environment is removed.",
        "diff @env_added_v2.yaml @env_added_v1.yaml", "- jobEnvironments[Tools]"),
    case(ENVS, "env_reordered", "job environment order", "Two job environments swap places, which changes the order they are entered in.",
        "diff @env_order_v1.yaml @env_order_v2.yaml", "jobEnvironments.order"),
    case(ENVS, "env_description", "environment `description`", "The job environment's description changes.",
        "diff @env_description_v1.yaml @env_description_v2.yaml", "jobEnvironments[Tools].description"),
    case(ENVS, "env_script_added", "environment `script` added", "A job environment gains a script.",
        "diff @env_script_added_v1.yaml @env_script_added_v2.yaml", "jobEnvironments[Tools].script"),
    case(ENVS, "env_variables", "environment `variables`", "One variable changes and one is added.",
        "diff @env_variables_v1.yaml @env_variables_v2.yaml", "variables.TOOL"),
    case(ENVS, "env_command", "environment action `command`", "The `onEnter` command changes.",
        "diff @env_command_v1.yaml @env_command_v2.yaml", "onEnter.command"),
    case(ENVS, "env_let", "environment script `let` (EXPR)", "The environment script's `let` binding changes.",
        "diff @env_let_v1.yaml @env_let_v2.yaml", "script.let"),
    case(ENVS, "env_embedded_file", "environment embedded file `data`", "The environment's embedded file changes.",
        "diff @env_file_v1.yaml @env_file_v2.yaml", "embeddedFiles[Setup].data"),
    case(STEPS, "step_added", "step added", "A second step is added.",
        "diff @step_added_v1.yaml @step_added_v2.yaml", "+ step Composite"),
    case(STEPS, "step_removed", "step removed", "The second step is removed.",
        "diff @step_added_v2.yaml @step_added_v1.yaml", "- step Composite"),
    case(STEPS, "step_renamed", "step `name`", "The step is renamed. Steps are matched by name, so this is a remove and an add.",
        "diff @step_renamed_v1.yaml @step_renamed_v2.yaml", "+ step Beauty"),
    case(STEPS, "step_reordered", "step order", "Two independent steps swap places.",
        "diff @step_order_v1.yaml @step_order_v2.yaml", "steps.order"),
    case(STEPS, "step_description", "step `description`", "The step description changes.",
        "diff @step_description_v1.yaml @step_description_v2.yaml", "~ description:"),
    case(STEPS, "step_dependencies", "step `dependencies`", "`Composite` starts depending on `Render`.",
        "diff @step_dependencies_v1.yaml @step_dependencies_v2.yaml", "dependencies"),
    case(STEPS, "step_dependencies_reordered", "step `dependencies` order", "`Composite` lists the same two dependencies in the other order.",
        "diff @dependencies_order_v1.yaml @dependencies_order_v2.yaml", "dependencies.order"),
    case(STEPS, "host_added", "`hostRequirements` added", "A step that had no host requirements gets one.",
        "diff @host_added_v1.yaml @host_added_v2.yaml", "+ hostRequirements"),
    case(STEPS, "host_amount", "host requirement amount", "The minimum vCPU count changes.",
        "diff @host_amount_v1.yaml @host_amount_v2.yaml", "amounts["),
    case(STEPS, "host_attribute", "host requirement attribute", "Windows hosts are allowed as well as Linux.",
        "diff @host_attribute_v1.yaml @host_attribute_v2.yaml", "attributes["),
    case(STEPS, "host_reordered", "host requirement `amounts`, `attributes` order", "The same amounts and attributes are listed in the other order.",
        "diff @host_order_v1.yaml @host_order_v2.yaml", "amounts.order"),
    case(STEPS, "host_amounts_empty", "host requirement `amounts: []` added", "An empty `amounts` list is added beside the attributes, which template validation accepts.",
        "diff @host_amounts_empty_v1.yaml @host_amounts_empty_v2.yaml", "+ hostRequirements.amounts"),
    case(STEPS, "script_command", "action `command`", "The step's `onRun` command changes.",
        "diff @script_command_v1.yaml @script_command_v2.yaml", "onRun.command"),
    case(STEPS, "script_args", "action `args`", "An argument is added.",
        "diff @script_args_v1.yaml @script_args_v2.yaml", "onRun.args"),
    case(STEPS, "script_timeout", "action `timeout` (FEATURE_BUNDLE_1)", "The action timeout doubles.",
        "diff @script_timeout_v1.yaml @script_timeout_v2.yaml", "onRun.timeout"),
    case(STEPS, "script_cancelation", "action `cancelation` (FEATURE_BUNDLE_1)", "The notify period before termination grows.",
        "diff @cancelation_v1.yaml @cancelation_v2.yaml", "onRun.cancelation"),
    case(STEPS, "embedded_file_data", "embedded file `data`", "The step's embedded script changes.",
        "diff @file_data_v1.yaml @file_data_v2.yaml", ".data:"),
    case(STEPS, "embedded_file_data_multiline", "embedded file `data`, multi-line", "A line of the step's multi-line embedded script changes; the value stays on one line.",
        "diff @file_multiline_v1.yaml @file_multiline_v2.yaml", ".data:"),
    case(STEPS, "embedded_file_filename", "embedded file `filename`", "The embedded file is written under another name.",
        "diff @file_name_v1.yaml @file_name_v2.yaml", ".filename:"),
    case(STEPS, "embedded_file_runnable", "embedded file `runnable`", "The embedded file becomes runnable.",
        "diff @file_runnable_v1.yaml @file_runnable_v2.yaml", ".runnable:"),
    case(STEPS, "embedded_file_reordered", "`embeddedFiles` order", "The step's two embedded files swap places.",
        "diff @file_order_v1.yaml @file_order_v2.yaml", "embeddedFiles.order"),
    case(STEPS, "embedded_file_added", "embedded file added", "The step gains a second embedded file.",
        "diff @file_added_v1.yaml @file_added_v2.yaml", "+ script.embeddedFiles[Notes]"),
    case(STEPS, "embedded_file_removed", "embedded file removed", "The second embedded file is removed again.",
        "diff @file_added_v2.yaml @file_added_v1.yaml", "- script.embeddedFiles[Notes]"),
    rich(case(STEPS, "let_script", "step script `let` (EXPR)", "The step script's `let` binding, which an argument reads, changes.",
        "diff @let_script_v1.yaml @let_script_v2.yaml", "script.let")),
    rich(case(STEPS, "let_step_used", "step `let` added, read by an arg (EXPR)",
        "A step-level `let` binding is added and an argument reads it, so the step's symbol table changes too.",
        "diff @let_step_used_v1.yaml @let_step_used_v2.yaml", "+ let:")),
    case(STEPS, "let_step_unused", "step `let`, read by nothing (EXPR)",
        "A step-level `let` that nothing reads changes; only the template layer can see it.",
        "diff @let_step_unused_v1.yaml @let_step_unused_v2.yaml", "~ let:"),
    case(STEPS, "step_env_added", "step environment added", "A step environment is added.",
        "diff @step_env_added_v1.yaml @step_env_added_v2.yaml", "+ stepEnvironments[Cache]"),
    case(STEPS, "step_env_removed", "step environment removed", "The step environment is removed.",
        "diff @step_env_added_v2.yaml @step_env_added_v1.yaml", "- stepEnvironments[Cache]"),
    case(STEPS, "step_env_reordered", "step environment order", "Two step environments swap places.",
        "diff @step_env_order_v1.yaml @step_env_order_v2.yaml", "stepEnvironments.order"),
    case(STEPS, "step_env_changed", "step environment `variables`", "A step environment's variable changes.",
        "diff @step_env_changed_v1.yaml @step_env_changed_v2.yaml", "stepEnvironments[Cache].variables"),
    case(TASKS, "frames_extended", "task range extended", "The frame range grows from 1-10 to 1-20 in the template; the new tasks are appended.",
        "diff @frames_extended_v1.yaml @frames_extended_v2.yaml", "~ Frame:"),
    case(TASKS, "frames_shrunk", "task range shrunk", "The frame range shrinks from 1-20 to 1-10; the tasks past 10 are dropped.",
        "diff @frames_shrunk_v1.yaml @frames_shrunk_v2.yaml", "~ Frame:"),
    rich(case(TASKS, "eye_added", "task range value added to one dimension",
        "`Eye` gains `right` in a `Frame × Eye` product: the old tasks are kept and the right eye is added.",
        "diff %render_v1.yaml %render_v2.yaml -p Scene=//srv/s", "~ Eye:")),
    rich(case(TASKS, "new_dimension", "task parameter added (new dimension)",
        "A step gains an `Eye` dimension that no argument reads, so only the tasks change: every task is replaced.",
        "diff @new_dimension_v1.yaml @new_dimension_v2.yaml", "+ Eye:")),
    case(TASKS, "new_dimension_with_arg", "task parameter added, and the arg that reads it",
        "A step gains an `Eye` dimension and an argument that reads it, so the script changes too.",
        "diff %render_no_eye.yaml %render_v1.yaml -p Scene=//srv/s", "+ Eye:"),
    case(TASKS, "space_added", "`parameterSpace` added", "A step with one task gets a parameter space, so its task is replaced.",
        "diff @space_added_v1.yaml @space_added_v2.yaml", "+ parameterSpace"),
    case(TASKS, "range_rewritten", "task range rewritten, same values",
        "The range `\"1-2\"` is written as the list `[1, 2]`: the tasks are the same, and the parameter space is rewritten in place.",
        "diff @range_rewritten_v1.yaml @range_rewritten_v2.yaml", "~ Frame:"),
    case(TASKS, "range_rewritten_depth_2", "task range rewritten, parameter space not expanded",
        "The `range_rewritten` case with `--depth 2`: the parameter space is shown whole, and each definition in it as it is written.",
        "diff @range_rewritten_v1.yaml @range_rewritten_v2.yaml --depth 2", "~ parameterSpace:"),
    case(TASKS, "dimension_reordered", "task parameter definition order",
        "`Frame` and `Eye` swap places; with no `combination` that changes task order, so every task is replaced.",
        "diff @dimension_order_v1.yaml @dimension_order_v2.yaml", "taskParameterDefinitions.order"),
    case(TASKS, "combination", "`combination`", "A product becomes an association.",
        "diff @combination_v1.yaml @combination_v2.yaml", "combination"),
    case(TASKS, "chunk_size", "chunking (CHUNK[INT])", "The chunk size doubles, so the chunks are replaced.",
        "diff @chunk_size_v1.yaml @chunk_size_v2.yaml", "~ Frame:"),
    case(TASKS, "chunk_adaptive", "adaptive chunking", "A target runtime is set, so the chunk count is decided at run time.",
        "diff @chunk_adaptive_v1.yaml @chunk_adaptive_v2.yaml", "~ Frame:"),
    case(STAGE, "runtime_task_param", "`{{Task.Param.X}}` reference", "An argument that names a task parameter changes; a worker resolves it.",
        "diff @runtime_task_param_v1.yaml @runtime_task_param_v2.yaml", "onRun.args"),
    case(STAGE, "runtime_session_dir", "`{{Session.WorkingDirectory}}` reference", "An argument under the session directory changes.",
        "diff @runtime_session_dir_v1.yaml @runtime_session_dir_v2.yaml", "onRun.args"),
    case(ORIGIN, "origin_both", "origin `both`",
        "The template appends frame 50 to the range and the new parameters extend `Frames`, so the range change has both causes.",
        "diff @origin_both_v1.yaml @origin_both_v2.yaml --new-param Frames=1-20", "[both]"),
    case(ORIGIN, "origin_mid_fails", "origin `both`, with a note",
        "The new template needs a parameter the old side does not supply, so the split cannot be computed.",
        "diff @origin_mid_fails_v1.yaml @origin_mid_fails_v2.yaml --new-param Quality=3", "+ parameter Quality"),
    case(FORMS, "jobs", "built jobs (`--jobs`)",
        "Two jobs that `openjd build` printed from `frames.yaml` with `Frames=1-100` and `Frames=1-200`. Every origin is `job`.",
        "diff --jobs @frames_1_100.json @frames_1_200.json", "~ parameter Frames"),
    case(FORMS, "step_filter", "`--step`", "Two steps change; `--step Composite` keeps one of them.",
        "diff @two_steps_v1.yaml @two_steps_v2.yaml --step Composite", "onRun.command"),
    case(FORMS, "stat", "`--stat`", "The `eye_added` case, one line per step.",
        "diff %render_v1.yaml %render_v2.yaml -p Scene=//srv/s --stat", "~ Render"),
];

/// Fields the diff can report that no case shows, and why.
const NOT_COVERED: &[(&str, &str)] = &[
    (
        "`specificationVersion`",
        "`jobtemplate-2023-09` is the only job template version, so it cannot change.",
    ),
    (
        "parameter, environment or embedded file `name`",
        "Entries are matched by name, so a rename is a remove and an add, as `step_renamed` shows.",
    ),
    (
        "`<collection>.duplicates`",
        "Template validation rejects repeated names; only a hand-edited built job has them, and `--plan` rejects them.",
    ),
    (
        "an empty list versus none, other than `hostRequirements.amounts` and `attributes`",
        "Template validation rejects an empty `parameterDefinitions`, `jobEnvironments`, `stepEnvironments`, `dependencies`, `taskParameterDefinitions`, `embeddedFiles` or `args`, so only a hand-edited built job has one. An empty `amounts` or `attributes` is accepted beside the other, as `host_amounts_empty` shows.",
    ),
    (
        "embedded file `type`",
        "`TEXT` is the only embedded file type, so it cannot change.",
    ),
    (
        "embedded file `endOfLine`; other actions (`onExit`, `onWrap*`)",
        "Shown like the fields above: one line per field, under the same path.",
    ),
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn args(command: &str) -> Vec<String> {
    command
        .split(' ')
        .map(|a| {
            a.replace('@', "crates/openjd-cli/examples/diff/")
                .replace('%', "crates/openjd-model/examples/template_diff/")
        })
        .collect()
}

/// Run `openjd` from the repository root; return stdout, checking that the
/// only stderr is the unstable warning and that it exits 0.
fn run(args: &[String], warning: &str) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_openjd"))
        .args(args)
        .current_dir(root())
        .output()
        .unwrap();
    let stderr = String::from_utf8(o.stderr).unwrap();
    assert_eq!(stderr, warning, "openjd {}", args.join(" "));
    assert_eq!(o.status.code(), Some(0), "openjd {}", args.join(" "));
    String::from_utf8(o.stdout).unwrap()
}

fn with(base: &[String], extra: &[&str]) -> Vec<String> {
    let mut v = base.to_vec();
    v.extend(extra.iter().map(|s| s.to_string()));
    v
}

fn block(out: &mut String, lang: &str, args: &[String], stdout: &str) {
    let _ = writeln!(out, "```{lang}");
    if lang != "json" {
        let _ = writeln!(out, "$ openjd {}", args.join(" "));
    }
    out.push_str(stdout);
    if !stdout.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("```\n\n");
}

fn joined(values: &BTreeSet<String>) -> String {
    if values.is_empty() {
        "—".to_string()
    } else {
        values.iter().cloned().collect::<Vec<_>>().join(", ")
    }
}

/// The case's `--plan --output json` document.
fn document(args: &[String]) -> serde_json::Value {
    let json = with(args, &["--plan", "--output", "json"]);
    let stdout = run(&json, WARNING);
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "openjd {}: stdout is not one JSON document: {e}\n{stdout}",
            json.join(" ")
        )
    })
}

/// The coverage row's origin, stage and plan kinds, from the JSON document.
fn meta(doc: &serde_json::Value) -> (String, String, String) {
    let mut origins = BTreeSet::new();
    let mut stages = BTreeSet::new();
    for c in doc["changes"].as_array().unwrap() {
        origins.insert(c["origin"].as_str().unwrap().to_string());
        stages.insert(c["stage"].as_str().unwrap().to_string());
    }
    let kinds: BTreeSet<String> = doc["plan"]["kinds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_str().unwrap().to_string())
        .collect();
    (joined(&origins), joined(&stages), joined(&kinds))
}

/// The coverage table's "shown as" text. A line under a step header is
/// relative to the step, so it is rewritten with the full path of the change
/// it shows (`~ steps[Render].description: a -> b  [template]`), as
/// `Change` alone prints it; any other line is kept as printed.
fn shown_as(id: &str, human: &str, pick: &str, doc: &serde_json::Value) -> String {
    let lines: Vec<&str> = human.lines().skip(1).collect(); // the header names the inputs
    let at = lines
        .iter()
        .position(|l| l.contains(pick))
        .unwrap_or_else(|| panic!("{id}: no output line contains {pick:?}:\n{human}"));
    let line = lines[at];
    let Some(rel) = line.strip_prefix("    ") else {
        return line.trim().to_string();
    };
    let step = lines[..at]
        .iter()
        .rev()
        .find_map(|l| l.get(2..).and_then(|r| r.strip_prefix("step ")))
        .unwrap_or_else(|| panic!("{id}: no step header above {line:?}"));
    let (mark, rest) = rel.split_at(1);
    let (label, tail) = rest
        .trim_start()
        .split_once(": ")
        .unwrap_or_else(|| panic!("{id}: {line:?} has no value"));
    let text = |side: &serde_json::Value| side["text"].as_str().map(str::to_string);
    let full = doc["changes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["step"].as_str() == Some(step))
        .find_map(|c| {
            let values = match (text(&c["before"]), text(&c["after"])) {
                (Some(b), Some(a)) => format!("{b} -> {a}"),
                (Some(v), None) | (None, Some(v)) => v,
                (None, None) => return None,
            };
            let path = c["path"].as_str()?;
            let field = c["field"].as_str()?;
            let full = if field.is_empty() {
                path.to_string()
            } else {
                format!("{path}.{field}")
            };
            let ours =
                full.ends_with(&format!(".{label}")) || path.ends_with(&format!("[{label}]"));
            let tail_matches = tail == format!("{values}  [{}]", c["origin"].as_str()?);
            (ours && tail_matches).then_some(full)
        })
        .unwrap_or_else(|| panic!("{id}: no change in the JSON document prints as {line:?}"));
    format!("{mark} {full}: {tail}")
}

/// Every file in `examples/diff/` except the README is read by a case or
/// is one of [`BUILT_JOBS`], so a file nothing uses is noticed.
fn assert_every_example_is_used() {
    let dir = root().join("crates/openjd-cli/examples/diff");
    let mut used = BTreeSet::new();
    let commands = CASES
        .iter()
        .map(|c| c.command)
        .chain(BUILT_JOBS.iter().map(|(_, command)| *command));
    for command in commands {
        for a in args(command) {
            if let Some(name) = a.strip_prefix("crates/openjd-cli/examples/diff/") {
                used.insert(name.to_string());
            }
        }
    }
    for (file, _) in BUILT_JOBS {
        if let Some(name) = file.strip_prefix("crates/openjd-cli/examples/diff/") {
            used.insert(name.to_string());
        }
    }
    let unused: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name != "README.md" && !used.contains(name))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    assert!(
        unused.is_empty(),
        "files in {} that no case or BUILT_JOBS entry uses: {unused:?}",
        dir.display()
    );
}

fn gallery() -> String {
    let mut table = String::new();
    let mut body = String::new();
    let mut group = "";
    for c in CASES {
        let a = args(c.command);
        let human = run(&a, WARNING);
        let doc = document(&a);
        let shown = shown_as(c.id, &human, c.shown, &doc);
        let (origin, stage, kinds) = meta(&doc);
        let _ = writeln!(
            table,
            "| {} | [{}](#{}) | `{}` | {origin} | {stage} | {kinds} |",
            c.field,
            c.id,
            c.id,
            shown.replace('|', "\\|")
        );
        if c.group != group {
            group = c.group;
            let _ = writeln!(body, "## {group}\n");
        }
        let _ = writeln!(body, "### {}\n\n{}\n", c.id, c.what);
        block(&mut body, "console", &a, &human);
        if c.rich {
            let plan = with(&a, &["--plan"]);
            block(&mut body, "console", &plan, &run(&plan, WARNING));
            let json = with(&a, &["--output", "json"]);
            let _ = writeln!(body, "`openjd {}`:\n", json.join(" "));
            block(&mut body, "json", &json, &run(&json, WARNING));
        }
    }
    let mut not_covered = String::new();
    for (field, why) in NOT_COVERED {
        let _ = writeln!(not_covered, "| {field} | {why} |");
    }
    format!(
        "\
# `openjd diff` examples

<!-- Generated by crates/openjd-cli/tests/diff_examples.rs. Do not edit; run
     UPDATE_DIFF_EXAMPLES=1 cargo test -p openjd-cli --test diff_examples -->

**Unstable.** `openjd diff` may change its flags and output in any release. Every command
also prints `{}` to stderr, which is left out here.

One case per field the diff can report ([diff.md](diff.md),
[model/template-diff.md](../model/template-diff.md)). Commands run from the repository
root, on the templates in `crates/openjd-cli/examples/diff/` and
`crates/openjd-model/examples/template_diff/`. The output below is the real output of
each command, checked by a test.

## Coverage

`shown as` is the main output line. A line printed under a step header is relative to the
step, so here it carries the change's full path, as a change prints on its own
(`~ steps[Render].description: …`). `origin` and `stage` are the values over the case's
changes, and `plan kinds` the mutation kinds of `--plan`.

| field | example | shown as | origin | stage | plan kinds |
|---|---|---|---|---|---|
{table}
Not shown:

| field | why |
|---|---|
{not_covered}
{body}",
        WARNING.trim_end()
    )
    .trim_end()
    .to_string()
        + "\n"
}

fn check_or_update(path: &Path, actual: &str, update: bool) {
    if update {
        std::fs::write(path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        expected == actual,
        "{} is out of date. Rerun with {UPDATE}=1 to regenerate it:\n  \
         {UPDATE}=1 cargo test -p openjd-cli --test diff_examples",
        path.display()
    );
}

#[test]
fn diff_examples_gallery_is_up_to_date() {
    let update = std::env::var(UPDATE).is_ok_and(|v| v == "1");
    assert_every_example_is_used();
    // The built jobs first: the `--jobs` case reads them.
    for (file, command) in BUILT_JOBS {
        let job = run(&args(command), BUILD_WARNING);
        check_or_update(&root().join(file), &job, update);
    }
    check_or_update(&root().join(GALLERY), &gallery(), update);
}

/// The examples `openjd diff --help` ends with.
const HELP_EXAMPLES: &str = "\
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

Every field the diff reports, with its output: specs/cli/diff-examples.md
";

/// `--help` ends with the examples, the `about` says the command is
/// unstable, and every example that names only shipped files runs.
#[test]
fn help_examples_use_the_shipped_files() {
    let help = |cmd: &str| {
        let o = Command::new(env!("CARGO_BIN_EXE_openjd"))
            .args([cmd, "--help"])
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(0));
        String::from_utf8(o.stdout).unwrap()
    };
    let diff = help("diff");
    assert!(
        diff.starts_with(
            "Diff two templates, one template under two parameter sets, or two built jobs (unstable)\n"
        ),
        "{diff}"
    );
    assert!(diff.ends_with(&format!("\n\n{HELP_EXAMPLES}")), "{diff}");
    let build = help("build");
    assert!(
        build.starts_with("Print the job a template builds into, as JSON or YAML (unstable)\n"),
        "{build}"
    );
    let mut ran = 0;
    for line in HELP_EXAMPLES
        .lines()
        .filter_map(|l| l.strip_prefix("  openjd "))
    {
        if line.contains("old.") {
            continue;
        }
        let command = line.replace("$E/", "@").replace("$M/", "%");
        run(&args(&command), WARNING);
        ran += 1;
    }
    assert_eq!(ran, 5);
}
