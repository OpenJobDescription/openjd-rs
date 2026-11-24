// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for `job::diff`. Each builds both jobs from small templates with
//! `create_job` and asserts the whole `DiffTree` text.

use crate::job::diff::{diff_environments, diff_jobs, diff_steps, DiffNode, DiffStatus};
use crate::job::Job;
use crate::{create_job, decode_job_template, preprocess_job_parameters, CallerLimits};
use crate::{JobParameterInputValues, PathParameterOptions};

const BASE: &str = r#"
specificationVersion: jobtemplate-2023-09
name: Demo
parameterDefinitions:
  - {name: Scene, type: STRING, default: a.blend}
  - {name: Quality, type: STRING, default: low}
jobEnvironments:
  - name: Conda
    variables: {PACKAGES: blender}
steps:
  - name: Prep
    script:
      actions: {onRun: {command: prep, args: ["{{Param.Scene}}"]}}
  - name: Render
    dependencies: [{dependsOn: Prep}]
    parameterSpace:
      taskParameterDefinitions:
        - {name: Frame, type: INT, range: "1-10"}
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

fn edited(from: &str, to: &str) -> Job {
    assert!(BASE.contains(from), "BASE has no `{from}`");
    job(&BASE.replace(from, to))
}

#[test]
fn identical_jobs_are_unchanged() {
    let tree = diff_jobs(&job(BASE), &job(BASE));
    assert!(!tree.is_changed());
    assert_eq!(tree.root().status, DiffStatus::Unchanged);
    assert!(tree.root().children.is_empty());
    assert_eq!(tree.to_string(), "");
}

#[test]
fn added_step_is_one_added_node() {
    let new = job(&format!(
        "{BASE}  - name: Publish\n    script:\n      actions: {{onRun: {{command: publish}}}}\n"
    ));
    assert_eq!(diff_text(&job(BASE), &new), "+ steps[Publish]\n");
}

#[test]
fn removed_step_is_one_removed_node() {
    let old = job(&format!(
        "{BASE}  - name: Publish\n    script:\n      actions: {{onRun: {{command: publish}}}}\n"
    ));
    let tree = diff_jobs(&old, &job(BASE));
    assert!(tree.is_changed());
    assert_eq!(tree.to_string(), "- steps[Publish]\n");
    let removed = tree.root().child("steps[Publish]").unwrap();
    assert_eq!(removed.status, DiffStatus::Removed);
    assert!(removed.children.is_empty());
}

#[test]
fn task_range_change_is_on_its_task_parameter() {
    assert_eq!(
        diff_text(&job(BASE), &edited("range: \"1-10\"", "range: \"1-20\"")),
        "~ steps[Render]\n  ~ parameterSpace\n    ~ taskParameterDefinitions[Frame]\n      \
         range: \"1-10\" -> \"1-20\"\n"
    );
}

#[test]
fn job_parameter_change_marks_only_the_steps_that_read_it() {
    let old = job(BASE);
    let new = job_with(BASE, &[("Scene", "b.blend")]);
    assert_eq!(
        diff_text(&old, &new),
        "~ parameters[Scene]\n  value: \"a.blend\" -> \"b.blend\"\n~ steps[Prep]\n  \
         resolvedSymTab: [{\"name\":\"Param.Scene\",\"type\":\"string\",\"value\":\"a.blend\"}] \
         -> [{\"name\":\"Param.Scene\",\"type\":\"string\",\"value\":\"b.blend\"}]\n"
    );
    let tree = diff_jobs(&old, &new);
    let root = tree.root();
    for unchanged in [
        "parameters[Quality]",
        "jobEnvironments[Conda]",
        "steps[Render]",
    ] {
        assert_eq!(root.child(unchanged).unwrap().status, DiffStatus::Unchanged);
    }
}

#[test]
fn on_run_command_change_is_on_its_action() {
    assert_eq!(
        diff_text(&job(BASE), &edited("command: render", "command: render2")),
        "~ steps[Render]\n  ~ script\n    ~ onRun\n      command: \"render\" -> \"render2\"\n"
    );
}

#[test]
fn job_environment_variable_change_is_keyed() {
    assert_eq!(
        diff_text(
            &job(BASE),
            &edited("{PACKAGES: blender}", "{PACKAGES: maya, MODE: x}")
        ),
        "~ jobEnvironments[Conda]\n  variables[MODE]: null -> \"x\"\n  \
         variables[PACKAGES]: \"blender\" -> \"maya\"\n"
    );
}

#[test]
fn reordered_steps_record_order_and_leave_each_step_unchanged() {
    let old = job(BASE);
    let mut new = old.clone();
    new.steps.reverse();
    let tree = diff_jobs(&old, &new);
    assert_eq!(
        tree.to_string(),
        "steps.order: [\"Prep\",\"Render\"] -> [\"Render\",\"Prep\"]\n"
    );
    for step in ["steps[Prep]", "steps[Render]"] {
        assert_eq!(
            tree.root().child(step).unwrap().status,
            DiffStatus::Unchanged
        );
    }
}

/// Parameters and task parameter definitions are maps: their order is not
/// part of the job's equality, so a reorder is no change.
#[test]
fn reordered_map_entries_are_unchanged() {
    let reordered = BASE
        .replace(
            "  - {name: Scene, type: STRING, default: a.blend}\n  - {name: Quality, type: STRING, default: low}",
            "  - {name: Quality, type: STRING, default: low}\n  - {name: Scene, type: STRING, default: a.blend}",
        )
        .replace(
            "        - {name: Frame, type: INT, range: \"1-10\"}\n        - {name: Eye, type: STRING, range: [left, right]}",
            "        - {name: Eye, type: STRING, range: [left, right]}\n        - {name: Frame, type: INT, range: \"1-10\"}",
        );
    assert_ne!(reordered, BASE);
    assert!(!diff_jobs(&job(BASE), &job(&reordered)).is_changed());
}

#[test]
fn diff_steps_equals_the_job_diffs_step_subtree() {
    let (old, new) = (job(BASE), edited("command: render", "command: render2"));
    let job_tree = diff_jobs(&old, &new);
    let in_job = job_tree.root().child("steps[Render]").unwrap();
    let alone = diff_steps(&old.steps[1], &new.steps[1]);
    assert_eq!(alone.root().status, in_job.status);
    assert_eq!(alone.root().field_changes, in_job.field_changes);
    assert_eq!(alone.root().children, in_job.children);
}

#[test]
fn diff_environments_and_absent_versus_empty_collections() {
    let (old, new) = (job(BASE), edited("{PACKAGES: blender}", "{PACKAGES: maya}"));
    let (a, b) = (
        &old.job_environments.as_ref().unwrap()[0],
        &new.job_environments.as_ref().unwrap()[0],
    );
    let alone = diff_environments(a, b);
    assert_eq!(
        alone.to_string(),
        "variables[PACKAGES]: \"blender\" -> \"maya\"\n"
    );
    let job_tree = diff_jobs(&old, &new);
    assert_eq!(
        job_tree
            .root()
            .child("jobEnvironments[Conda]")
            .unwrap()
            .children,
        alone.root().children
    );

    // create_job never writes an empty list or map, but the fields are public.
    let mut empty = old.clone();
    empty.steps[0].step_environments = Some(Vec::new());
    let envs = empty.job_environments.as_mut().unwrap();
    envs[0].variables = Some(Default::default());
    assert_eq!(
        diff_text(&old, &empty),
        "~ jobEnvironments[Conda]\n  variables[PACKAGES]: \"blender\" -> null\n\
         ~ steps[Prep]\n  stepEnvironments: null -> []\n"
    );
    let mut none = old.clone();
    none.job_environments.as_mut().unwrap()[0].variables = Some(Default::default());
    let mut absent = old.clone();
    absent.job_environments.as_mut().unwrap()[0].variables = None;
    assert_eq!(
        diff_text(&none, &absent),
        "~ jobEnvironments[Conda]\n  variables: {} -> null\n"
    );
}

#[test]
fn job_fields_and_environment_actions() {
    let new = job(&BASE.replace("name: Demo", "name: Demo2\ndescription: d").replace(
        "    variables: {PACKAGES: blender}",
        "    variables: {PACKAGES: blender}\n    script:\n      actions: {onEnter: {command: activate}}",
    ));
    assert_eq!(
        diff_text(&job(BASE), &new),
        "name: \"Demo\" -> \"Demo2\"\ndescription: null -> \"d\"\n~ jobEnvironments[Conda]\n  + script\n"
    );
}

#[test]
fn host_requirements_and_embedded_files_are_matched_by_name() {
    const STEP: &str = r#"
specificationVersion: jobtemplate-2023-09
name: Demo
steps:
  - name: S
    hostRequirements:
      amounts: [{name: amount.worker.vcpu, min: 2}, {name: amount.worker.gpu, min: 1}]
      attributes: [{name: attr.worker.os.family, anyOf: [linux]}]
    script:
      embeddedFiles: [{name: F, type: TEXT, data: one}, {name: G, type: TEXT, data: g}]
      actions: {onRun: {command: "{{Task.File.F}}"}}
"#;
    let new = STEP
        .replace(
            "{name: amount.worker.vcpu, min: 2}, {name: amount.worker.gpu, min: 1}",
            "{name: amount.worker.gpu, min: 1}, {name: amount.worker.vcpu, min: 4}",
        )
        .replace("anyOf: [linux]", "anyOf: [linux, macos]")
        .replace("data: one", "data: two");
    assert_eq!(
        diff_text(&job(STEP), &job(&new)),
        "~ steps[S]\n  ~ script\n    ~ embeddedFiles[F]\n      data: \"one\" -> \"two\"\n  ~ hostRequirements\n    \
         amounts.order: [\"amount.worker.vcpu\",\"amount.worker.gpu\"] -> [\"amount.worker.gpu\",\"amount.worker.vcpu\"]\n    \
         amounts[amount.worker.vcpu]: {\"max\":null,\"min\":2.0,\"name\":\"amount.worker.vcpu\"} -> {\"max\":null,\"min\":4.0,\"name\":\"amount.worker.vcpu\"}\n    \
         attributes[attr.worker.os.family]: {\"allOf\":null,\"anyOf\":[\"linux\"],\"name\":\"attr.worker.os.family\"} \
         -> {\"allOf\":null,\"anyOf\":[\"linux\",\"macos\"],\"name\":\"attr.worker.os.family\"}\n"
    );
}

#[test]
fn environment_script_actions_are_children() {
    let env = |actions: &str| {
        edited(
            "    variables: {PACKAGES: blender}",
            &format!("    variables: {{PACKAGES: blender}}\n    script:\n      actions: {actions}"),
        )
    };
    let old = env("{onEnter: {command: activate}}");
    let new = env("{onEnter: {command: activate2}, onExit: {command: deactivate}}");
    assert_eq!(
        diff_text(&old, &new),
        "~ jobEnvironments[Conda]\n  ~ script\n    ~ onEnter\n      \
         command: \"activate\" -> \"activate2\"\n    + onExit\n"
    );
}

#[test]
fn task_parameter_type_change_is_its_own_field() {
    assert_eq!(
        diff_text(
            &job(BASE),
            &edited("{name: Eye, type: STRING", "{name: Eye, type: PATH")
        ),
        "~ steps[Render]\n  ~ parameterSpace\n    ~ taskParameterDefinitions[Eye]\n      \
         type: \"STRING\" -> \"PATH\"\n"
    );
}
