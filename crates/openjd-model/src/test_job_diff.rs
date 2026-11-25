// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for `job::diff`. A few golden tests pin the `Display` text of
//! whole trees; `single_edits` checks that each one-field edit lands on the
//! node and field the spec names.

use crate::job::diff::{diff_environments, diff_jobs, diff_steps, DiffNode, DiffStatus};
use crate::job::{Job, ResolvedChunks, TaskParamRange, TaskParameter};
use crate::template::RangeConstraint;
use crate::types::{EndOfLine, JobParameterType, ModelExtension};
use crate::{create_job, decode_job_template, preprocess_job_parameters, CallerLimits};
use crate::{JobParameterInputValues, PathParameterOptions};
use openjd_expr::symbol_table::SerializedSymbolTable;
use openjd_expr::RangeExpr;

const BASE: &str = r#"
specificationVersion: jobtemplate-2023-09
name: Demo
parameterDefinitions:
  - {name: Scene, type: STRING, default: a.blend}
  - {name: Quality, type: STRING, default: low}
  - {name: Last, type: INT, default: 10}
jobEnvironments:
  - name: Conda
    variables: {PACKAGES: blender, QUALITY: "{{Param.Quality}}"}
    script:
      actions: {onEnter: {command: activate}}
      embeddedFiles: [{name: Init, type: TEXT, data: x}]
  - name: Log
    variables: {LEVEL: info}
steps:
  - name: Prep
    script:
      actions: {onRun: {command: prep, args: ["{{Param.Scene}}"]}}
      embeddedFiles: [{name: F, type: TEXT, data: one}, {name: G, type: TEXT, data: two}]
  - name: Render
    dependencies: [{dependsOn: Prep}]
    hostRequirements:
      amounts: [{name: amount.worker.vcpu, min: 2}, {name: amount.worker.gpu, min: 1}]
      attributes: [{name: attr.worker.os.family, anyOf: [linux]}, {name: attr.worker.cpu.arch, anyOf: [x86_64]}]
    parameterSpace:
      taskParameterDefinitions:
        - {name: Frame, type: INT, range: "1-{{Param.Last}}"}
        - {name: Eye, type: STRING, range: [left, right]}
    script:
      actions: {onRun: {command: render, args: ["{{Task.Param.Frame}}"]}}
"#;

fn job_with(yaml: &str, params: &[(&str, &str)]) -> Job {
    let v: serde_json::Value = serde_saphyr::from_str(yaml).unwrap();
    let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    for (k, val) in params {
        input.insert((*k).into(), openjd_expr::ExprValue::String((*val).into()));
    }
    let options = PathParameterOptions {
        job_template_dir: "/tmp",
        current_working_dir: "/tmp",
        path_format: openjd_expr::path_mapping::PathFormat::Posix,
        allow_template_dir_walk_up: true,
        allow_uri_path_values: true,
    };
    let values = preprocess_job_parameters(&jt, &input, &[], &options).unwrap();
    create_job(&jt, &values, &jt.default_validation_context()).unwrap()
}

fn job(yaml: &str) -> Job {
    job_with(yaml, &[])
}

fn edited(yaml: &str, from: &str, to: &str) -> String {
    assert_eq!(
        yaml.matches(from).count(),
        1,
        "`{from}` is not in the template exactly once"
    );
    yaml.replace(from, to)
}

/// Every modified node explains itself with a field change or a changed
/// child; otherwise a model field is missing from the diff.
fn assert_attributed(node: &DiffNode) {
    if node.status == DiffStatus::Modified {
        assert!(
            !node.field_changes.is_empty() || node.children.iter().any(DiffNode::is_changed),
            "unattributed change at `{}`",
            node.segment
        );
    }
    node.children.iter().for_each(assert_attributed);
}

/// The diff's text, after checking every modified node is attributed.
fn diff_text(old: &Job, new: &Job) -> String {
    let tree = diff_jobs(old, new);
    assert_attributed(tree.root());
    tree.to_string()
}

// Golden `Display` tests.

#[test]
fn identical_jobs_are_unchanged() {
    let tree = diff_jobs(&job(BASE), &job(BASE));
    assert_eq!(tree.root().status, DiffStatus::Unchanged);
    assert!(tree.root().children.is_empty());
    assert_eq!(tree.to_string(), "");
}

const PUBLISH: &str =
    "  - name: Publish\n    script:\n      actions: {onRun: {command: publish}}\n";

#[test]
fn added_step_is_one_added_node() {
    let new = job(&format!("{BASE}{PUBLISH}"));
    assert_eq!(diff_text(&job(BASE), &new), "+ steps[Publish]\n");
}

#[test]
fn removed_step_is_one_removed_node() {
    let old = job(&format!("{BASE}{PUBLISH}"));
    assert!(diff_jobs(&old, &job(BASE)).is_changed());
    assert_eq!(diff_text(&old, &job(BASE)), "- steps[Publish]\n");
}

/// `Last` only feeds a task range, so it shows there and on no step's
/// `resolvedSymTab`.
#[test]
fn task_range_change_is_on_its_task_parameter() {
    assert_eq!(
        diff_text(&job(BASE), &job_with(BASE, &[("Last", "20")])),
        "~ parameters[Last]\n  value: \"10\" -> \"20\"\n~ steps[Render]\n  ~ parameterSpace\n    \
         ~ taskParameterDefinitions[Frame]\n      range: \"1-10\" -> \"1-20\"\n"
    );
}

#[test]
fn job_parameter_change_marks_only_the_step_that_reads_it() {
    assert_eq!(
        diff_text(&job(BASE), &job_with(BASE, &[("Scene", "b.blend")])),
        concat!(
            "~ parameters[Scene]\n",
            r#"  value: "a.blend" -> "b.blend""#,
            "\n~ steps[Prep]\n",
            r#"  resolvedSymTab[Param.Scene]: {"type":"string","value":"a.blend"} -> {"type":"string","value":"b.blend"}"#,
            "\n",
        )
    );
}

/// Map entries print in key order whatever the template order, and a
/// named list prints its old-side elements before its new-only ones.
#[test]
fn children_print_in_key_order_then_old_side_first() {
    let new = edited(
        &edited(
            BASE,
            "  - {name: Scene",
            "  - {name: Zeta, type: STRING, default: z}\n  - {name: Alpha, type: STRING, default: a}\n  - {name: Scene",
        ),
        PREP,
        "  - name: Setup\n    script:\n      actions: {onRun: {command: setup}}\n  - name: Prep\n",
    );
    let new = edited(&new, "command: render", "command: render2");
    assert_eq!(
        diff_text(&job(BASE), &job(&new)),
        concat!(
            "+ parameters[Alpha]\n+ parameters[Zeta]\n",
            "~ steps[Render]\n  ~ script\n    ~ onRun\n",
            r#"      command: "render" -> "render2""#,
            "\n+ steps[Setup]\n",
        )
    );
}

/// A three-step reorder is one field change with both sides' orders, and
/// every step is unchanged.
#[test]
fn step_reorder_is_one_order_change() {
    let old = job(&format!("{BASE}{PUBLISH}"));
    let mut new = old.clone();
    new.steps.rotate_left(1);
    let tree = diff_jobs(&old, &new);
    let root = tree.root();
    assert_eq!(
        field_lines(root),
        r#"steps.order: ["Prep","Render","Publish"] -> ["Render","Publish","Prep"]"#
    );
    let children: Vec<_> = root
        .children
        .iter()
        .map(|c| (c.segment.as_str(), c.status))
        .collect();
    let unchanged = DiffStatus::Unchanged;
    assert_eq!(
        children,
        [
            ("parameters[Last]", unchanged),
            ("parameters[Quality]", unchanged),
            ("parameters[Scene]", unchanged),
            ("jobEnvironments[Conda]", unchanged),
            ("jobEnvironments[Log]", unchanged),
            ("steps[Prep]", unchanged),
            ("steps[Render]", unchanged),
            ("steps[Publish]", unchanged),
        ]
    );
}

// Single edits: each row changes one thing and names the node it lands on.

enum Edit {
    /// Replace text in `BASE` for the new job.
    Text(&'static str, &'static str),
    /// Give a job parameter a value for the new job.
    Param(&'static str, &'static str),
    /// Edit both created jobs, for what validation does not allow in a template.
    Jobs(fn(&mut Job, &mut Job)),
}

enum Expect {
    /// The node's field changes, exactly, one `field: before -> after` per line.
    Fields(&'static str),
    Child(&'static str, DiffStatus),
}

use DiffStatus::{Added, Removed};
use Edit::{Jobs, Param, Text};
use Expect::{Child, Fields};

const PREP: &str = "  - name: Prep\n";
const FRAME: &str = "        - {name: Frame";
const RENDER_RUN: &str = "{command: render,";
const RENDER: &str = "steps[Render]";
const ON_RUN: &[&str] = &[RENDER, "script", "onRun"];
const FILE_F: &[&str] = &["steps[Prep]", "script", "embeddedFiles[F]"];
const SPACE: &[&str] = &[RENDER, "parameterSpace"];
const FRAME_NODE: &[&str] = &[RENDER, "parameterSpace", "taskParameterDefinitions[Frame]"];
const CONDA: &str = "jobEnvironments[Conda]";
const LOG: &str = "jobEnvironments[Log]";

fn prep_files(j: &mut Job) -> &mut Vec<crate::job::EmbeddedFile> {
    j.steps[0].script.embedded_files.as_mut().unwrap()
}

fn host(j: &mut Job) -> &mut crate::job::HostRequirements {
    j.steps[1].host_requirements.as_mut().unwrap()
}

fn frame(j: &mut Job) -> &mut TaskParameter {
    let space = j.steps[1].parameter_space.as_mut().unwrap();
    space.task_parameter_definitions.get_mut("Frame").unwrap()
}

fn two_chunks() -> ResolvedChunks {
    ResolvedChunks {
        default_task_count: 2,
        target_runtime_seconds: None,
        range_constraint: RangeConstraint::Contiguous,
    }
}

fn five_six(contiguous: bool) -> TaskParamRange<i64> {
    TaskParamRange::RangeExpr(
        RangeExpr::from_values(vec![5, 6])
            .unwrap()
            .with_contiguous(contiguous),
    )
}

fn conda_script(j: &mut Job) -> &mut crate::job::EnvironmentScript {
    j.job_environments.as_mut().unwrap()[0]
        .script
        .as_mut()
        .unwrap()
}

#[rustfmt::skip]
const ROWS: &[(&str, Edit, &[&str], Expect)] = &[
    ("job name", Text("name: Demo\n", "name: Demo2\n"), &[], Fields(r#"name: "Demo" -> "Demo2""#)),
    ("job description", Text("name: Demo\n", "name: Demo\ndescription: d\n"), &[], Fields(r#"description: null -> "d""#)),
    ("job extensions", Jobs(|_, b| b.extensions = Some(vec![ModelExtension::TaskChunking])),
        &[], Fields(r#"extensions: null -> ["TASK_CHUNKING"]"#)),
    ("parameter added",
        Text("  - {name: Last", "  - {name: Extra, type: STRING, default: e}\n  - {name: Last"),
        &[], Child("parameters[Extra]", Added)),
    ("parameter removed", Jobs(|_, b| { b.parameters.shift_remove("Quality"); }),
        &[], Child("parameters[Quality]", Removed)),
    ("parameter name", Jobs(|_, b| b.parameters["Scene"].name = "Other".into()),
        &["parameters[Scene]"], Fields(r#"name: "Scene" -> "Other""#)),
    ("parameter type", Jobs(|_, b| b.parameters["Scene"].param_type = JobParameterType::Path),
        &["parameters[Scene]"], Fields(r#"type: "STRING" -> "PATH""#)),
    ("parameter value", Param("Scene", "b.blend"), &["parameters[Scene]"], Fields(r#"value: "a.blend" -> "b.blend""#)),
    ("jobEnvironments order", Jobs(|_, b| b.job_environments.as_mut().unwrap().reverse()),
        &[], Fields(r#"jobEnvironments.order: ["Conda","Log"] -> ["Log","Conda"]"#)),
    ("jobEnvironments absent versus empty",
        Jobs(|a, b| { a.job_environments = None; b.job_environments = Some(Vec::new()); }),
        &[], Fields(r#"jobEnvironments: null -> []"#)),
    ("step name with a bracket is written unescaped", Jobs(|_, b| b.steps[0].name = "P]rep".into()),
        &[], Child("steps[P]rep]", Added)),
    ("steps order", Jobs(|_, b| b.steps.reverse()), &[], Fields(r#"steps.order: ["Prep","Render"] -> ["Render","Prep"]"#)),
    ("step description", Text(PREP, "  - name: Prep\n    description: d\n"),
        &["steps[Prep]"], Fields(r#"description: null -> "d""#)),
    ("step dependencies", Text("    dependencies: [{dependsOn: Prep}]\n", ""),
        &[RENDER], Fields(r#"dependencies: [{"dependsOn":"Prep"}] -> null"#)),
    ("step resolvedSymTab", Param("Scene", "b.blend"), &["steps[Prep]"], Fields(r#"resolvedSymTab[Param.Scene]: {"type":"string","value":"a.blend"} -> {"type":"string","value":"b.blend"}"#)),
    ("step resolvedSymTab symbol added",
        Text("args: [\"{{Param.Scene}}\"]", "args: [\"{{Param.Scene}}\", \"{{Param.Quality}}\"]"),
        &["steps[Prep]"], Fields(r#"resolvedSymTab[Param.Quality]: null -> {"type":"string","value":"low"}"#)),
    ("step resolvedSymTab not an array",
        Jobs(|_, b| b.steps[0].resolved_symtab = Some(SerializedSymbolTable::from_value(serde_json::json!({})))),
        &["steps[Prep]"], Fields(r#"resolvedSymTab: [{"name":"Param.Scene","type":"string","value":"a.blend"}] -> {}"#)),
    ("step resolvedSymTab absent versus empty", Jobs(|a, b| {
            a.steps[0].resolved_symtab = None;
            b.steps[0].resolved_symtab = Some(SerializedSymbolTable::from_value(serde_json::json!([])));
        }), &["steps[Prep]"], Fields(r#"resolvedSymTab: null -> []"#)),
    ("action command", Text("command: render", "command: render2"), ON_RUN, Fields(r#"command: "render" -> "render2""#)),
    ("action args", Text("[\"{{Task.Param.Frame}}\"]", "[\"{{Task.Param.Frame}}\", x]"),
        ON_RUN, Fields(r#"args: ["{{Task.Param.Frame}}"] -> ["{{Task.Param.Frame}}","x"]"#)),
    ("action timeout", Text(RENDER_RUN, "{command: render, timeout: 60,"), ON_RUN, Fields(r#"timeout: null -> "60""#)),
    ("action cancelation", Text(RENDER_RUN, "{command: render, cancelation: {mode: TERMINATE},"),
        ON_RUN, Fields(r#"cancelation: null -> {"mode":"TERMINATE"}"#)),
    ("step script let", Jobs(|_, b| b.steps[0].script.let_bindings = Some(vec!["x = 1".into()])),
        &["steps[Prep]", "script"], Fields(r#"let: null -> ["x = 1"]"#)),
    ("embeddedFiles order", Jobs(|_, b| prep_files(b).reverse()),
        &["steps[Prep]", "script"], Fields(r#"embeddedFiles.order: ["F","G"] -> ["G","F"]"#)),
    ("embedded file data", Text("data: one", "data: uno"), FILE_F, Fields(r#"data: "one" -> "uno""#)),
    ("embedded file filename", Text("{name: F, type: TEXT,", "{name: F, type: TEXT, filename: f.txt,"),
        FILE_F, Fields(r#"filename: null -> "f.txt""#)),
    ("embedded file runnable", Text("{name: F, type: TEXT,", "{name: F, type: TEXT, runnable: true,"),
        FILE_F, Fields(r#"runnable: null -> true"#)),
    ("embedded file endOfLine", Jobs(|_, b| prep_files(b)[0].end_of_line = Some(EndOfLine::Crlf)),
        FILE_F, Fields(r#"endOfLine: null -> "CRLF""#)),
    ("stepEnvironments order", Jobs(|a, b| {
            a.steps[0].step_environments = a.job_environments.clone();
            b.steps[0].step_environments = b.job_environments.clone().map(|mut e| { e.reverse(); e });
        }), &["steps[Prep]"], Fields(r#"stepEnvironments.order: ["Conda","Log"] -> ["Log","Conda"]"#)),
    ("step environment added",
        Text(PREP, "  - name: Prep\n    stepEnvironments: [{name: Tmp, variables: {DIR: /tmp}}]\n"),
        &["steps[Prep]"], Child("stepEnvironments[Tmp]", Added)),
    ("parameterSpace added",
        Text(PREP, "  - name: Prep\n    parameterSpace:\n      taskParameterDefinitions: [{name: Shot, type: INT, range: [1]}]\n"),
        &["steps[Prep]"], Child("parameterSpace", Added)),
    ("parameterSpace removed", Jobs(|_, b| b.steps[1].parameter_space = None),
        &[RENDER], Child("parameterSpace", Removed)),
    ("hostRequirements added",
        Text(PREP, "  - name: Prep\n    hostRequirements: {attributes: [{name: attr.worker.os.family, anyOf: [linux]}]}\n"),
        &["steps[Prep]"], Child("hostRequirements", Added)),
    ("hostRequirements removed", Jobs(|_, b| b.steps[1].host_requirements = None),
        &[RENDER], Child("hostRequirements", Removed)),
    ("combination", Text("    parameterSpace:\n", "    parameterSpace:\n      combination: \"Frame * Eye\"\n"),
        SPACE, Fields(r#"combination: null -> "Frame * Eye""#)),
    ("task parameter added", Text(FRAME, "        - {name: Pass, type: INT, range: [1]}\n        - {name: Frame"),
        SPACE, Child("taskParameterDefinitions[Pass]", Added)),
    ("task parameter removed", Text("        - {name: Eye, type: STRING, range: [left, right]}\n", ""),
        SPACE, Child("taskParameterDefinitions[Eye]", Removed)),
    ("task parameter type", Text("{name: Eye, type: STRING", "{name: Eye, type: PATH"),
        &[RENDER, "parameterSpace", "taskParameterDefinitions[Eye]"], Fields(r#"type: "STRING" -> "PATH""#)),
    ("task parameter range", Param("Last", "20"), FRAME_NODE, Fields(r#"range: "1-10" -> "1-20""#)),
    ("task parameter chunks", Jobs(|_, b| {
            if let TaskParameter::Int { chunks, .. } = frame(b) {
                *chunks = Some(two_chunks());
            }
        }), FRAME_NODE, Fields(r#"chunks: null -> {"defaultTaskCount":2,"rangeConstraint":"CONTIGUOUS","targetRuntimeSeconds":null}"#)),
    ("task parameter CHUNK[INT] with a list range",
        Jobs(|_, b| *frame(b) = TaskParameter::ChunkInt { range: TaskParamRange::List(vec![1, 2]), chunks: two_chunks() }),
        FRAME_NODE, Fields(concat!(
            r#"type: "INT" -> "CHUNK[INT]""#, "\n",
            r#"range: "1-10" -> [1,2]"#, "\n",
            r#"chunks: null -> {"defaultTaskCount":2,"rangeConstraint":"CONTIGUOUS","targetRuntimeSeconds":null}"#))),
    ("task parameter range differing only in display",
        Jobs(|a, b| {
            *frame(a) = TaskParameter::Int { range: five_six(false), chunks: None };
            *frame(b) = TaskParameter::Int { range: five_six(true), chunks: Some(two_chunks()) };
        }), FRAME_NODE, Fields(r#"chunks: null -> {"defaultTaskCount":2,"rangeConstraint":"CONTIGUOUS","targetRuntimeSeconds":null}"#)),
    ("amount", Text("min: 2}", "min: 4}"), &[RENDER, "hostRequirements"], Fields(r#"amounts[amount.worker.vcpu]: {"max":null,"min":2.0,"name":"amount.worker.vcpu"} -> {"max":null,"min":4.0,"name":"amount.worker.vcpu"}"#)),
    ("attribute", Text("anyOf: [linux]}", "anyOf: [linux, macos]}"),
        &[RENDER, "hostRequirements"], Fields(r#"attributes[attr.worker.os.family]: {"allOf":null,"anyOf":["linux"],"name":"attr.worker.os.family"} -> {"allOf":null,"anyOf":["linux","macos"],"name":"attr.worker.os.family"}"#)),
    ("amounts order", Jobs(|_, b| host(b).amounts.as_mut().unwrap().reverse()),
        &[RENDER, "hostRequirements"], Fields(r#"amounts.order: ["amount.worker.vcpu","amount.worker.gpu"] -> ["amount.worker.gpu","amount.worker.vcpu"]"#)),
    ("attributes order", Jobs(|_, b| host(b).attributes.as_mut().unwrap().reverse()),
        &[RENDER, "hostRequirements"], Fields(r#"attributes.order: ["attr.worker.os.family","attr.worker.cpu.arch"] -> ["attr.worker.cpu.arch","attr.worker.os.family"]"#)),
    ("environment description", Text("  - name: Log\n", "  - name: Log\n    description: d\n"),
        &[LOG], Fields(r#"description: null -> "d""#)),
    ("environment variable", Text("{LEVEL: info}", "{LEVEL: debug}"), &[LOG], Fields(r#"variables[LEVEL]: "info" -> "debug""#)),
    ("environment variables absent versus empty", Jobs(|a, b| {
            a.job_environments.as_mut().unwrap()[1].variables = Some(Default::default());
            b.job_environments.as_mut().unwrap()[1].variables = None;
        }), &[LOG], Fields(r#"variables: {} -> null"#)),
    ("environment resolvedSymTab", Param("Quality", "high"), &[CONDA], Fields(r#"resolvedSymTab[Param.Quality]: {"type":"string","value":"low"} -> {"type":"string","value":"high"}"#)),
    ("environment script added",
        Text("    variables: {LEVEL: info}\n", "    variables: {LEVEL: info}\n    script:\n      actions: {onExit: {command: flush}}\n"),
        &[LOG], Child("script", Added)),
    ("environment action", Text("{command: activate}", "{command: activate2}"),
        &[CONDA, "script", "onEnter"], Fields(r#"command: "activate" -> "activate2""#)),
    ("environment action added",
        Text("{onEnter: {command: activate}}", "{onEnter: {command: activate}, onExit: {command: stop}}"),
        &[CONDA, "script"], Child("onExit", Added)),
    ("environment onWrapEnvEnter added", Jobs(|_, b| {
            let s = conda_script(b);
            s.actions.on_wrap_env_enter = s.actions.on_enter.clone();
        }), &[CONDA, "script"], Child("onWrapEnvEnter", Added)),
    ("environment onWrapEnvExit added", Jobs(|_, b| {
            let s = conda_script(b);
            s.actions.on_wrap_env_exit = s.actions.on_enter.clone();
        }), &[CONDA, "script"], Child("onWrapEnvExit", Added)),
    ("environment script let", Jobs(|_, b| conda_script(b).let_bindings = Some(vec!["x = 1".into()])),
        &[CONDA, "script"], Fields(r#"let: null -> ["x = 1"]"#)),
    ("environment embedded file", Text("data: x}", "data: z}"),
        &[CONDA, "script", "embeddedFiles[Init]"], Fields(r#"data: "x" -> "z""#)),
];

#[test]
fn single_edits() {
    for (what, edit, path, expect) in ROWS {
        let mut old = job(BASE);
        let mut new = match edit {
            Text(from, to) => job(&edited(BASE, from, to)),
            Param(k, v) => job_with(BASE, &[(k, v)]),
            Jobs(_) => old.clone(),
        };
        if let Jobs(f) = edit {
            f(&mut old, &mut new);
        }
        let tree = diff_jobs(&old, &new);
        assert_attributed(tree.root());
        let mut node = tree.root();
        for seg in *path {
            assert_eq!(
                node.status,
                DiffStatus::Modified,
                "{what}: at `{}`",
                node.segment
            );
            node = node
                .child(seg)
                .unwrap_or_else(|| panic!("{what}: no `{seg}` in\n{tree}"));
        }
        assert_eq!(node.status, DiffStatus::Modified, "{what}:\n{tree}");
        match expect {
            Fields(want) => assert_eq!(field_lines(node), *want, "{what}:\n{tree}"),
            Child(seg, status) => assert_eq!(
                node.child(seg).map(|c| c.status),
                Some(*status),
                "{what}:\n{tree}"
            ),
        }
    }
}

fn field_lines(node: &DiffNode) -> String {
    let lines: Vec<_> = node
        .field_changes
        .iter()
        .map(|c| format!("{}: {} -> {}", c.field, c.before, c.after))
        .collect();
    lines.join("\n")
}

// Other claims of the spec.

/// Parameters and task parameter definitions are maps: their order is not
/// part of the job's equality, so a reorder is no change.
#[test]
fn reordered_map_entries_are_unchanged() {
    let params = edited(
        BASE,
        "  - {name: Scene, type: STRING, default: a.blend}\n  - {name: Quality, type: STRING, default: low}",
        "  - {name: Quality, type: STRING, default: low}\n  - {name: Scene, type: STRING, default: a.blend}",
    );
    let tasks = edited(
        &params,
        "        - {name: Frame, type: INT, range: \"1-{{Param.Last}}\"}\n        - {name: Eye, type: STRING, range: [left, right]}",
        "        - {name: Eye, type: STRING, range: [left, right]}\n        - {name: Frame, type: INT, range: \"1-{{Param.Last}}\"}",
    );
    assert!(!diff_jobs(&job(BASE), &job(&tasks)).is_changed());
}

/// A step environment has no `resolvedSymTab`; the parameters it reads are
/// in its step's.
#[test]
fn step_environment_reads_show_on_its_step() {
    let yaml = edited(
        BASE,
        PREP,
        "  - name: Prep\n    stepEnvironments: [{name: Tmp, variables: {Q: \"{{Param.Quality}}\"}}]\n",
    );
    let tree = diff_jobs(&job(&yaml), &job_with(&yaml, &[("Quality", "high")]));
    let prep = tree.root().child("steps[Prep]").unwrap();
    let fields: Vec<_> = prep
        .field_changes
        .iter()
        .map(|c| c.field.as_str())
        .collect();
    assert_eq!(fields, ["resolvedSymTab[Param.Quality]"]);
    assert_eq!(
        prep.child("stepEnvironments[Tmp]").unwrap().status,
        DiffStatus::Unchanged
    );
}

/// Value equality ignores a float's spelling, so `parameters[K]` stays
/// unchanged while the readers' `resolvedSymTab` records the new text.
#[test]
fn float_respelling_shows_only_on_readers() {
    let yaml = edited(
        &edited(
            BASE,
            "  - {name: Last",
            "  - {name: Scale, type: FLOAT, default: 2.5}\n  - {name: Last",
        ),
        "args: [\"{{Param.Scene}}\"]",
        "args: [\"{{Param.Scale}}\"]",
    );
    let tree = diff_jobs(&job(&yaml), &job_with(&yaml, &[("Scale", "2.50")]));
    let root = tree.root();
    assert_eq!(
        root.child("parameters[Scale]").unwrap().status,
        DiffStatus::Unchanged
    );
    assert_eq!(
        field_lines(root.child("steps[Prep]").unwrap()),
        r#"resolvedSymTab[Param.Scale]: {"type":"float","value":"2.5"} -> {"type":"float","value":"2.50"}"#
    );
}

#[test]
fn diff_steps_equals_the_job_diffs_step_subtree() {
    let (old, new) = (
        job(BASE),
        job(&edited(BASE, "command: render", "command: render2")),
    );
    let job_tree = diff_jobs(&old, &new);
    let alone = diff_steps(&old.steps[1], &new.steps[1]);
    assert_eq!(
        alone.root(),
        &with_segment(job_tree.root().child(RENDER), "")
    );
}

#[test]
fn diff_environments_equals_the_job_diffs_environment_subtree() {
    let (old, new) = (
        job(BASE),
        job(&edited(BASE, "{LEVEL: info}", "{LEVEL: debug}")),
    );
    let job_tree = diff_jobs(&old, &new);
    let envs = |j: &Job| j.job_environments.clone().unwrap();
    let alone = diff_environments(&envs(&old)[1], &envs(&new)[1]);
    assert_eq!(alone.root(), &with_segment(job_tree.root().child(LOG), ""));
}

fn with_segment(node: Option<&DiffNode>, segment: &str) -> DiffNode {
    let mut node = node.unwrap().clone();
    node.segment = segment.into();
    node
}

/// Called directly, `diff_steps` and `diff_environments` compare names,
/// which `diff_jobs` uses to match elements.
#[test]
fn diff_steps_and_diff_environments_compare_names() {
    let j = job(BASE);
    let mut step = j.steps[0].clone();
    step.name = "Other".into();
    assert_eq!(
        diff_steps(&j.steps[0], &step).to_string(),
        "name: \"Prep\" -> \"Other\"\n"
    );
    let env = &j.job_environments.as_ref().unwrap()[1];
    let mut renamed = env.clone();
    renamed.name = "Other".into();
    assert_eq!(
        diff_environments(env, &renamed).to_string(),
        "name: \"Log\" -> \"Other\"\n"
    );
}
