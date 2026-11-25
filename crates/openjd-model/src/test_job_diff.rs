// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for `job::diff`. A few golden tests pin the `Display` text of
//! whole trees; `single_edits` checks that each one-field edit lands on the
//! node and field the spec names.

use crate::job::diff::{diff_environments, diff_jobs, diff_steps, DiffNode, DiffStatus};
use crate::job::{Job, ResolvedChunks, TaskParamRange, TaskParameter};
use crate::template::RangeConstraint;
use crate::types::ModelExtension;
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
    stepEnvironments: [{name: A, variables: {X: a}}, {name: B, variables: {X: b}}]
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

/// The `Param.Scene` entry's change when `Scene` goes from `a.blend` to `b.blend`.
macro_rules! scene_change {
    () => {
        concat!(
            r#"resolvedSymTab[Param.Scene]: {"type":"string","value":"a.blend"}"#,
            r#" -> {"type":"string","value":"b.blend"}"#
        )
    };
}

/// A contiguous `chunks` with a default task count of `$n`, as JSON.
macro_rules! chunks {
    ($n:literal) => {
        concat!(
            r#"{"defaultTaskCount":"#,
            $n,
            r#","rangeConstraint":"CONTIGUOUS","targetRuntimeSeconds":null}"#
        )
    };
}

fn job_with(yaml: &str, params: &[(&str, &str)]) -> Job {
    let v: serde_json::Value = serde_saphyr::from_str(yaml).unwrap();
    let extensions: Vec<_> = ModelExtension::ALL
        .iter()
        .map(ModelExtension::as_str)
        .collect();
    let jt = decode_job_template(v, Some(&extensions), &CallerLimits::default()).unwrap();
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

/// The segment of the first modified node that explains itself with no
/// field change and no changed child; such a node means a model field is
/// missing from the diff.
fn unattributed(node: &DiffNode) -> Option<&str> {
    if node.status == DiffStatus::Modified
        && node.field_changes.is_empty()
        && !node.children.iter().any(DiffNode::is_changed)
    {
        return Some(&node.segment);
    }
    node.children.iter().find_map(unattributed)
}

fn assert_attributed(node: &DiffNode) {
    if let Some(at) = unattributed(node) {
        panic!("unattributed change at `{at}`");
    }
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
            "  ",
            scene_change!(),
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

/// The use the spec states: the changed steps' names, read from segments.
#[test]
fn changed_step_names_read_from_segments() {
    let new = format!(
        "{}{PUBLISH}",
        edited(BASE, "command: render", "command: render2")
    );
    let tree = diff_jobs(&job(BASE), &job(&new));
    let names: Vec<_> = tree
        .root()
        .children
        .iter()
        .filter(|c| c.is_changed())
        .filter_map(|c| c.segment.strip_prefix("steps[")?.strip_suffix(']'))
        .collect();
    assert_eq!(names, ["Render", "Publish"]);
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
    /// Replace text in `BASE` for the old job; the new job is `BASE`.
    Old(&'static str, &'static str),
    /// Rewrite `BASE` for the old job and for the new job.
    Templates(fn(&str) -> String, fn(&str) -> String),
    /// Give a job parameter a value for the new job.
    Param(&'static str, &'static str),
    /// Edit both created jobs, for a pair no template pair produces.
    Jobs(fn(&mut Job, &mut Job)),
}

enum Expect {
    /// The node's field changes, exactly, one `field: before -> after` per line.
    Fields(&'static str),
    Child(&'static str, DiffStatus),
}

use DiffStatus::{Added, Removed};
use Edit::{Jobs, Old, Param, Templates, Text};
use Expect::{Child, Fields};

const PREP: &str = "  - name: Prep\n";
const PREP_STEP: &str = "  - name: Prep\n    script:\n      \
    actions: {onRun: {command: prep, args: [\"{{Param.Scene}}\"]}}\n      \
    embeddedFiles: [{name: F, type: TEXT, data: one}, {name: G, type: TEXT, data: two}]\n";
const PREP_SPACE: &str = "  - name: Prep\n    parameterSpace:\n      \
    taskParameterDefinitions: [{name: Shot, type: INT, range: [1]}]\n";
const PREP_ATTRS: &str =
    "  - name: Prep\n    hostRequirements: {attributes: [{name: attr.worker.os.family, anyOf: [linux]}]}\n";
const PREP_AMOUNTS: &str =
    "  - name: Prep\n    hostRequirements: {amounts: [{name: amount.worker.vcpu, min: 2}]}\n";
const LOG_ENV: &str = "  - name: Log\n    variables: {LEVEL: info}\n";
const LAST: &str = "  - {name: Last";
const EXTRA_THEN_LAST: &str = "  - {name: Extra, type: STRING, default: e}\n  - {name: Last";
const FRAME: &str = "        - {name: Frame";
const FRAME_DEF: &str = "{name: Frame, type: INT, range: \"1-{{Param.Last}}\"}";
const RENDER_RUN: &str = "{command: render,";
const RENDER: &str = "steps[Render]";
const ON_RUN: &[&str] = &[RENDER, "script", "onRun"];
const FILE_F: &[&str] = &["steps[Prep]", "script", "embeddedFiles[F]"];
const SPACE: &[&str] = &[RENDER, "parameterSpace"];
const FRAME_NODE: &[&str] = &[RENDER, "parameterSpace", "taskParameterDefinitions[Frame]"];
const EYE_NODE: &[&str] = &[RENDER, "parameterSpace", "taskParameterDefinitions[Eye]"];
const HOST: &[&str] = &[RENDER, "hostRequirements"];
const PREP_HOST: &[&str] = &["steps[Prep]", "hostRequirements"];
const CONDA: &str = "jobEnvironments[Conda]";
const LOG: &str = "jobEnvironments[Log]";

fn same(t: &str) -> String {
    t.into()
}

fn with_extensions(t: &str, extensions: &str) -> String {
    edited(
        t,
        "name: Demo\n",
        &format!("name: Demo\nextensions: [{extensions}]\n"),
    )
}

/// `Frame` as a CHUNK[INT] with `range` and a default task count of `count`.
fn chunked(t: &str, range: &str, count: u32) -> String {
    let def = format!(
        "{{name: Frame, type: \"CHUNK[INT]\", range: {range}, \
         chunks: {{defaultTaskCount: {count}, rangeConstraint: CONTIGUOUS}}}}"
    );
    edited(&with_extensions(t, "TASK_CHUNKING"), FRAME_DEF, &def)
}

/// The Conda environment with the three wrap actions, which come together.
fn wrapped(t: &str) -> String {
    let wraps = "{onEnter: {command: activate}, onWrapEnvEnter: {command: wrap-enter}, \
                 onWrapTaskRun: {command: wrap-task}, onWrapEnvExit: {command: wrap-exit}}";
    let t = with_extensions(t, "WRAP_ACTIONS, EXPR");
    edited(&t, "{onEnter: {command: activate}}", wraps)
}

/// A `let` binding in the script whose actions line starts `actions`.
fn with_let(t: &str, actions: &str) -> String {
    let binding = format!("      let: [\"x = 1\"]\n{actions}");
    edited(&with_extensions(t, "EXPR"), actions, &binding)
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

#[rustfmt::skip]
const ROWS: &[(&str, Edit, &[&str], Expect)] = &[
    ("job name", Text("name: Demo\n", "name: Demo2\n"), &[], Fields(r#"name: "Demo" -> "Demo2""#)),
    ("job description", Text("name: Demo\n", "name: Demo\ndescription: d\n"),
        &[], Fields(r#"description: null -> "d""#)),
    ("job extensions", Templates(same, |t| with_extensions(t, "TASK_CHUNKING")),
        &[], Fields(r#"extensions: null -> ["TASK_CHUNKING"]"#)),
    ("parameter added", Text(LAST, EXTRA_THEN_LAST), &[], Child("parameters[Extra]", Added)),
    ("parameter removed", Old(LAST, EXTRA_THEN_LAST), &[], Child("parameters[Extra]", Removed)),
    ("parameter type",
        Templates(|t| edited(t, LAST, "  - {name: Root, type: STRING, default: /r}\n  - {name: Last"),
                  |t| edited(t, LAST, "  - {name: Root, type: PATH, default: /r}\n  - {name: Last")),
        &["parameters[Root]"], Fields(r#"type: "STRING" -> "PATH""#)),
    ("parameter value", Param("Scene", "b.blend"), &["parameters[Scene]"], Fields(r#"value: "a.blend" -> "b.blend""#)),
    ("jobEnvironments order",
        Templates(same, |t| {
            edited(&edited(t, LOG_ENV, ""), "  - name: Conda\n", &format!("{LOG_ENV}  - name: Conda\n"))
        }),
        &[], Fields(r#"jobEnvironments.order: ["Conda","Log"] -> ["Log","Conda"]"#)),
    ("step name with a bracket is written unescaped",
        Templates(same, |t| {
            edited(&edited(t, PREP, "  - name: \"P]rep\"\n"), "dependsOn: Prep", "dependsOn: \"P]rep\"")
        }),
        &[], Child("steps[P]rep]", Added)),
    ("steps order", Templates(same, |t| format!("{}{PREP_STEP}", edited(t, PREP_STEP, ""))),
        &[], Fields(r#"steps.order: ["Prep","Render"] -> ["Render","Prep"]"#)),
    ("step description", Text(PREP, "  - name: Prep\n    description: d\n"),
        &["steps[Prep]"], Fields(r#"description: null -> "d""#)),
    ("step dependencies", Text("    dependencies: [{dependsOn: Prep}]\n", ""),
        &[RENDER], Fields(r#"dependencies: [{"dependsOn":"Prep"}] -> null"#)),
    ("step resolvedSymTab", Param("Scene", "b.blend"), &["steps[Prep]"], Fields(scene_change!())),
    ("step resolvedSymTab symbol added",
        Text("args: [\"{{Param.Scene}}\"]", "args: [\"{{Param.Scene}}\", \"{{Param.Quality}}\"]"),
        &["steps[Prep]"], Fields(r#"resolvedSymTab[Param.Quality]: null -> {"type":"string","value":"low"}"#)),
    ("step resolvedSymTab entries in name order",
        Templates(same, |t| {
            let t = edited(t, "args: [\"{{Param.Scene}}\"]", "args: [\"{{Param.Scene}}\", \"{{Param.Quality}}\"]");
            edited(&t, "default: a.blend", "default: b.blend")
        }),
        &["steps[Prep]"], Fields(concat!(
            r#"resolvedSymTab[Param.Quality]: null -> {"type":"string","value":"low"}"#, "\n",
            scene_change!()))),
    // `create_job` always writes a symbol table as an array.
    ("step resolvedSymTab not an array",
        Jobs(|_, b| b.steps[0].resolved_symtab = Some(SerializedSymbolTable::from_value(serde_json::json!({})))),
        &["steps[Prep]"],
        Fields(r#"resolvedSymTab: [{"name":"Param.Scene","type":"string","value":"a.blend"}] -> {}"#)),
    ("action command", Text("command: render", "command: render2"),
        ON_RUN, Fields(r#"command: "render" -> "render2""#)),
    ("action args", Text("[\"{{Task.Param.Frame}}\"]", "[\"{{Task.Param.Frame}}\", x]"),
        ON_RUN, Fields(r#"args: ["{{Task.Param.Frame}}"] -> ["{{Task.Param.Frame}}","x"]"#)),
    ("action timeout", Text(RENDER_RUN, "{command: render, timeout: 60,"), ON_RUN, Fields(r#"timeout: null -> "60""#)),
    ("action cancelation", Text(RENDER_RUN, "{command: render, cancelation: {mode: TERMINATE},"),
        ON_RUN, Fields(r#"cancelation: null -> {"mode":"TERMINATE"}"#)),
    ("step script let", Templates(same, |t| with_let(t, "      actions: {onRun: {command: prep")),
        &["steps[Prep]", "script"], Fields(r#"let: null -> ["x = 1"]"#)),
    ("embeddedFiles order",
        Text("{name: F, type: TEXT, data: one}, {name: G, type: TEXT, data: two}",
             "{name: G, type: TEXT, data: two}, {name: F, type: TEXT, data: one}"),
        &["steps[Prep]", "script"], Fields(r#"embeddedFiles.order: ["F","G"] -> ["G","F"]"#)),
    ("embedded file added", Text("data: two}]", "data: two}, {name: H, type: TEXT, data: three}]"),
        &["steps[Prep]", "script"], Child("embeddedFiles[H]", Added)),
    ("embedded file data", Text("data: one", "data: uno"), FILE_F, Fields(r#"data: "one" -> "uno""#)),
    ("embedded file filename", Text("{name: F, type: TEXT,", "{name: F, type: TEXT, filename: f.txt,"),
        FILE_F, Fields(r#"filename: null -> "f.txt""#)),
    ("embedded file runnable", Text("{name: F, type: TEXT,", "{name: F, type: TEXT, runnable: true,"),
        FILE_F, Fields(r#"runnable: null -> true"#)),
    ("embedded file endOfLine",
        Templates(same, |t| {
            let t = with_extensions(t, "FEATURE_BUNDLE_1");
            edited(&t, "{name: F, type: TEXT,", "{name: F, type: TEXT, endOfLine: CRLF,")
        }),
        FILE_F, Fields(r#"endOfLine: null -> "CRLF""#)),
    ("stepEnvironments order",
        Text("{name: A, variables: {X: a}}, {name: B, variables: {X: b}}",
             "{name: B, variables: {X: b}}, {name: A, variables: {X: a}}"),
        &[RENDER], Fields(r#"stepEnvironments.order: ["A","B"] -> ["B","A"]"#)),
    ("step environment added",
        Text(PREP, "  - name: Prep\n    stepEnvironments: [{name: Tmp, variables: {DIR: /tmp}}]\n"),
        &["steps[Prep]"], Child("stepEnvironments[Tmp]", Added)),
    ("parameterSpace added", Text(PREP, PREP_SPACE), &["steps[Prep]"], Child("parameterSpace", Added)),
    ("parameterSpace removed", Old(PREP, PREP_SPACE), &["steps[Prep]"], Child("parameterSpace", Removed)),
    ("hostRequirements added", Text(PREP, PREP_ATTRS), &["steps[Prep]"], Child("hostRequirements", Added)),
    ("hostRequirements removed", Old(PREP, PREP_ATTRS), &["steps[Prep]"], Child("hostRequirements", Removed)),
    ("combination", Text("    parameterSpace:\n", "    parameterSpace:\n      combination: \"Frame * Eye\"\n"),
        SPACE, Fields(r#"combination: null -> "Frame * Eye""#)),
    ("task parameter added", Text(FRAME, "        - {name: Pass, type: INT, range: [1]}\n        - {name: Frame"),
        SPACE, Child("taskParameterDefinitions[Pass]", Added)),
    ("task parameter removed", Text("        - {name: Eye, type: STRING, range: [left, right]}\n", ""),
        SPACE, Child("taskParameterDefinitions[Eye]", Removed)),
    ("task parameter type", Text("{name: Eye, type: STRING", "{name: Eye, type: PATH"),
        EYE_NODE, Fields(r#"type: "STRING" -> "PATH""#)),
    ("task parameter range", Param("Last", "20"), FRAME_NODE, Fields(r#"range: "1-10" -> "1-20""#)),
    ("task parameter FLOAT", Text(FRAME_DEF, "{name: Frame, type: FLOAT, range: [1.5]}"),
        FRAME_NODE, Fields(concat!(r#"type: "INT" -> "FLOAT""#, "\n", r#"range: "1-10" -> [1.5]"#))),
    ("task parameter STRING range", Text("range: [left, right]", "range: [left, centre]"),
        EYE_NODE, Fields(r#"range: ["left","right"] -> ["left","centre"]"#)),
    ("task parameter PATH range",
        Templates(|t| edited(t, "type: STRING, range: [left, right]", "type: PATH, range: [left, right]"),
                  |t| edited(t, "type: STRING, range: [left, right]", "type: PATH, range: [left, centre]")),
        EYE_NODE, Fields(r#"range: ["left","right"] -> ["left","centre"]"#)),
    ("task parameter chunks", Templates(|t| chunked(t, "[1, 2]", 2), |t| chunked(t, "[1, 2]", 3)),
        FRAME_NODE, Fields(concat!("chunks: ", chunks!(2), " -> ", chunks!(3)))),
    ("task parameter CHUNK[INT] with a list range", Templates(same, |t| chunked(t, "[1, 2]", 2)),
        FRAME_NODE, Fields(concat!(
            r#"type: "INT" -> "CHUNK[INT]""#, "\n",
            r#"range: "1-10" -> [1,2]"#, "\n",
            "chunks: null -> ", chunks!(2)))),
    // `create_job` writes every range expression in canonical form, so only
    // hand-built ranges differ in display alone.
    ("task parameter range differing only in display",
        Jobs(|a, b| {
            *frame(a) = TaskParameter::Int { range: five_six(false), chunks: None };
            *frame(b) = TaskParameter::Int { range: five_six(true), chunks: Some(two_chunks()) };
        }), FRAME_NODE, Fields(concat!("chunks: null -> ", chunks!(2)))),
    ("amount", Text("min: 2}", "min: 4}"),
        HOST, Fields(r#"amounts[amount.worker.vcpu]: {"max":null,"min":2.0} -> {"max":null,"min":4.0}"#)),
    ("amount added",
        Text("{name: amount.worker.gpu, min: 1}",
             "{name: amount.worker.gpu, min: 1}, {name: amount.worker.memory, min: 1024}"),
        HOST, Fields(r#"amounts[amount.worker.memory]: null -> {"max":null,"min":1024.0}"#)),
    ("attribute removed", Text(", {name: attr.worker.cpu.arch, anyOf: [x86_64]}", ""),
        HOST, Fields(r#"attributes[attr.worker.cpu.arch]: {"allOf":null,"anyOf":["x86_64"]} -> null"#)),
    ("attribute", Text("anyOf: [linux]}", "anyOf: [linux, macos]}"),
        HOST, Fields(concat!(
            r#"attributes[attr.worker.os.family]: {"allOf":null,"anyOf":["linux"]}"#,
            r#" -> {"allOf":null,"anyOf":["linux","macos"]}"#))),
    ("amounts order",
        Text("{name: amount.worker.vcpu, min: 2}, {name: amount.worker.gpu, min: 1}",
             "{name: amount.worker.gpu, min: 1}, {name: amount.worker.vcpu, min: 2}"),
        HOST, Fields(concat!(
            r#"amounts.order: ["amount.worker.vcpu","amount.worker.gpu"]"#,
            r#" -> ["amount.worker.gpu","amount.worker.vcpu"]"#))),
    ("attributes order",
        Text("{name: attr.worker.os.family, anyOf: [linux]}, {name: attr.worker.cpu.arch, anyOf: [x86_64]}",
             "{name: attr.worker.cpu.arch, anyOf: [x86_64]}, {name: attr.worker.os.family, anyOf: [linux]}"),
        HOST, Fields(concat!(
            r#"attributes.order: ["attr.worker.os.family","attr.worker.cpu.arch"]"#,
            r#" -> ["attr.worker.cpu.arch","attr.worker.os.family"]"#))),
    ("amounts empty",
        Templates(|t| edited(t, PREP, PREP_ATTRS),
                  |t| edited(t, PREP, &PREP_ATTRS.replace("{attributes:", "{amounts: [], attributes:"))),
        PREP_HOST, Fields("amounts: null -> []")),
    ("amounts added with an entry",
        Templates(|t| edited(t, PREP, PREP_ATTRS), |t| {
            let amounts = "{amounts: [{name: amount.worker.vcpu, min: 2}], attributes:";
            edited(t, PREP, &PREP_ATTRS.replace("{attributes:", amounts))
        }),
        PREP_HOST, Fields(r#"amounts[amount.worker.vcpu]: null -> {"max":null,"min":2.0}"#)),
    ("attributes empty",
        Templates(|t| edited(t, PREP, PREP_AMOUNTS),
                  |t| edited(t, PREP, &PREP_AMOUNTS.replace("min: 2}]}", "min: 2}], attributes: []}"))),
        PREP_HOST, Fields("attributes: null -> []")),
    ("environment description", Text("  - name: Log\n", "  - name: Log\n    description: d\n"),
        &[LOG], Fields(r#"description: null -> "d""#)),
    ("environment variable", Text("{LEVEL: info}", "{LEVEL: debug}"),
        &[LOG], Fields(r#"variables[LEVEL]: "info" -> "debug""#)),
    ("environment variable added", Text("{LEVEL: info}", "{LEVEL: info, COLOR: auto}"),
        &[LOG], Fields(r#"variables[COLOR]: null -> "auto""#)),
    ("environment variables in key order", Text("{LEVEL: info}", "{LEVEL: debug, COLOR: auto}"),
        &[LOG], Fields(concat!(r#"variables[COLOR]: null -> "auto""#, "\n", r#"variables[LEVEL]: "info" -> "debug""#))),
    ("environment resolvedSymTab", Param("Quality", "high"), &[CONDA], Fields(concat!(
        r#"resolvedSymTab[Param.Quality]: {"type":"string","value":"low"}"#,
        r#" -> {"type":"string","value":"high"}"#))),
    ("environment script added",
        Text("    variables: {LEVEL: info}\n",
             "    variables: {LEVEL: info}\n    script:\n      actions: {onExit: {command: flush}}\n"),
        &[LOG], Child("script", Added)),
    ("environment action", Text("{command: activate}", "{command: activate2}"),
        &[CONDA, "script", "onEnter"], Fields(r#"command: "activate" -> "activate2""#)),
    ("environment action added",
        Text("{onEnter: {command: activate}}", "{onEnter: {command: activate}, onExit: {command: stop}}"),
        &[CONDA, "script"], Child("onExit", Added)),
    ("environment onWrapEnvEnter", Templates(wrapped, |t| edited(&wrapped(t), "wrap-enter}", "wrap-enter2}")),
        &[CONDA, "script", "onWrapEnvEnter"], Fields(r#"command: "wrap-enter" -> "wrap-enter2""#)),
    ("environment onWrapTaskRun", Templates(wrapped, |t| edited(&wrapped(t), "wrap-task}", "wrap-task2}")),
        &[CONDA, "script", "onWrapTaskRun"], Fields(r#"command: "wrap-task" -> "wrap-task2""#)),
    ("environment onWrapEnvExit", Templates(wrapped, |t| edited(&wrapped(t), "wrap-exit}", "wrap-exit2}")),
        &[CONDA, "script", "onWrapEnvExit"], Fields(r#"command: "wrap-exit" -> "wrap-exit2""#)),
    ("environment script let", Templates(same, |t| with_let(t, "      actions: {onEnter")),
        &[CONDA, "script"], Fields(r#"let: null -> ["x = 1"]"#)),
    ("environment embedded file", Text("data: x}", "data: z}"),
        &[CONDA, "script", "embeddedFiles[Init]"], Fields(r#"data: "x" -> "z""#)),
];

#[test]
fn single_edits() {
    let failures: Vec<String> = ROWS.iter().filter_map(single_edit).collect();
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// The row's failure message, or `None` when the edit lands where it says.
fn single_edit((what, edit, path, expect): &(&str, Edit, &[&str], Expect)) -> Option<String> {
    let (mut old, mut new) = match edit {
        Text(from, to) => (job(BASE), job(&edited(BASE, from, to))),
        Old(from, to) => (job(&edited(BASE, from, to)), job(BASE)),
        Templates(a, b) => (job(&a(BASE)), job(&b(BASE))),
        Param(k, v) => (job(BASE), job_with(BASE, &[(k, v)])),
        Jobs(_) => (job(BASE), job(BASE)),
    };
    if let Jobs(f) = edit {
        f(&mut old, &mut new);
    }
    let tree = diff_jobs(&old, &new);
    if let Some(at) = unattributed(tree.root()) {
        return Some(format!("{what}: unattributed change at `{at}`"));
    }
    let mut node = tree.root();
    for seg in *path {
        if node.status != DiffStatus::Modified {
            return Some(format!(
                "{what}: `{}` is {:?}\n{tree}",
                node.segment, node.status
            ));
        }
        let Some(child) = node.child(seg) else {
            return Some(format!("{what}: no `{seg}` in\n{tree}"));
        };
        node = child;
    }
    let ok = node.status == DiffStatus::Modified
        && match expect {
            Fields(want) => field_lines(node) == *want,
            Child(seg, status) => node.child(seg).map(|c| c.status) == Some(*status),
        };
    (!ok).then(|| format!("{what}:\n{tree}"))
}

/// Must match the field-line format of `DiffTree`'s `Display`.
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
        "        - {name: Frame, type: INT, range: \"1-{{Param.Last}}\"}\n        \
         - {name: Eye, type: STRING, range: [left, right]}",
        "        - {name: Eye, type: STRING, range: [left, right]}\n        \
         - {name: Frame, type: INT, range: \"1-{{Param.Last}}\"}",
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

const ROLES: &str = r#"
specificationVersion: jobtemplate-2023-09
name: Roles
parameterDefinitions:
  - {name: P, type: STRING, default: p}
jobEnvironments:
  - {name: J, variables: {X: VALUE}}
steps:
  - name: S
    stepEnvironments: [{name: A, variables: {X: VALUE}}]
    script:
      actions: {onRun: {command: run}}
"#;

/// A job environment and a step environment with the same content, both
/// from `create_job`. Only the job environment carries a symbol table, so the
/// table is the one difference.
#[test]
fn job_environment_against_step_environment() {
    for (value, want) in [
        ("a", "resolvedSymTab: [] -> null\n"),
        (
            "\"{{Param.P}}\"",
            "resolvedSymTab[Param.P]: {\"type\":\"string\",\"value\":\"p\"} -> null\n",
        ),
    ] {
        let roles = ROLES.replace("VALUE", value);
        let step_side = job(&roles);
        let job_side = job(&edited(
            &edited(&roles, "{name: J,", "{name: A,"),
            "[{name: A,",
            "[{name: Z,",
        ));
        let job_env = &job_side.job_environments.as_ref().unwrap()[0];
        let step_env = &step_side.steps[0].step_environments.as_ref().unwrap()[0];
        let tree = diff_environments(job_env, step_env);
        assert_attributed(tree.root());
        assert_eq!(tree.to_string(), want, "variable {value}");
    }
    // The whole-table change still shows when another field changed too.
    let j = job(&ROLES.replace("VALUE", "a"));
    let job_env = &j.job_environments.as_ref().unwrap()[0];
    let step_env = &j.steps[0].step_environments.as_ref().unwrap()[0];
    assert_eq!(
        diff_environments(job_env, step_env).to_string(),
        "name: \"J\" -> \"A\"\nresolvedSymTab: [] -> null\n"
    );
}
