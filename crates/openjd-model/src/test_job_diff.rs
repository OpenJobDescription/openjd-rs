// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for `job::diff`.

use crate::job::diff::{
    diff_environments, diff_jobs, diff_steps, DiffNodeKind, DiffOptions, DiffStatus, DiffTree,
};
use crate::job::Job;
use crate::{
    convert_environment, create_job, decode_environment_template, decode_job_template,
    preprocess_job_parameters, CallerLimits, JobParameterInputValues,
};
use openjd_expr::ExprValue;

const BASE: &str = r#"
specificationVersion: jobtemplate-2023-09
name: Demo
parameterDefinitions:
  - {name: Frames, type: STRING, default: "1-10"}
  - {name: Scene, type: STRING, default: a.blend}
jobEnvironments:
  - name: Conda
    variables: {CONDA_PACKAGES: blender, PYTHONUNBUFFERED: "1"}
steps:
  - name: Prep
    script:
      actions: {onRun: {command: prep, args: ["{{Param.Scene}}"]}}
  - name: Render
    dependencies: [{dependsOn: Prep}]
    parameterSpace:
      taskParameterDefinitions:
        - {name: Frame, type: INT, range: "{{Param.Frames}}"}
    script:
      actions: {onRun: {command: render, args: ["{{Task.Param.Frame}}"]}}
"#;

fn job(yaml: &str, params: &[(&str, &str)]) -> Job {
    let v: serde_json::Value = serde_saphyr::from_str(yaml).unwrap();
    let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    for (k, val) in params {
        input.insert((*k).into(), ExprValue::String((*val).into()));
    }
    let values = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &crate::PathParameterOptions {
            job_template_dir: "/tmp",
            current_working_dir: "/tmp",
            path_format: openjd_expr::path_mapping::PathFormat::Posix,
            allow_template_dir_walk_up: true,
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    create_job(&jt, &values, &jt.default_validation_context()).unwrap()
}

fn status(t: &DiffTree, path: &str) -> DiffStatus {
    let id = t
        .find(path)
        .unwrap_or_else(|| panic!("no node at {path}\n{t}"));
    t.node(id).unwrap().status
}

fn fields(t: &DiffTree, path: &str) -> Vec<String> {
    let id = t.find(path).unwrap();
    t.node(id)
        .unwrap()
        .field_changes
        .iter()
        .map(|c| c.field.clone())
        .collect()
}

/// Every expanded modified node must explain itself: a field change or a
/// changed child. Otherwise a model field is missing from the diff.
fn assert_attributed(t: &DiffTree) {
    for (id, n) in t.changed() {
        if n.status == DiffStatus::Modified && n.expanded {
            let child_changed = n.children.iter().any(|&c| t.node(c).unwrap().is_changed());
            assert!(
                !n.field_changes.is_empty() || child_changed,
                "unattributed change at {}\n{t}",
                t.path(id)
            );
        }
    }
}

#[test]
fn identical_jobs_are_unchanged_with_a_single_root_node() {
    let t = diff_jobs(&job(BASE, &[]), &job(BASE, &[]), &DiffOptions::default());
    assert!(!t.is_changed());
    assert_eq!(t.nodes().len(), 1);
    assert_eq!(t.root().kind, DiffNodeKind::Job);
    assert!(!t.root().expanded);
}

#[test]
fn command_change_is_localized_to_one_action() {
    let new = BASE.replace("command: render", "command: render2");
    let t = diff_jobs(&job(BASE, &[]), &job(&new, &[]), &DiffOptions::default());
    assert!(t.is_changed());
    assert_eq!(status(&t, "steps[Prep]"), DiffStatus::Unchanged);
    assert_eq!(status(&t, "jobEnvironments[Conda]"), DiffStatus::Unchanged);
    assert_eq!(status(&t, "steps[Render]"), DiffStatus::Modified);
    assert_eq!(status(&t, "steps[Render].script"), DiffStatus::Modified);
    assert_eq!(fields(&t, "steps[Render].script.onRun"), vec!["command"]);
    // The unchanged step was not descended into.
    let prep = t.find("steps[Prep]").unwrap();
    assert!(t.node(prep).unwrap().children.is_empty());
    assert_attributed(&t);
}

#[test]
fn max_depth_one_reports_step_status_without_expanding() {
    let new = BASE.replace("command: render", "command: render2");
    let opts = DiffOptions::default().with_max_depth(1);
    let t = diff_jobs(&job(BASE, &[]), &job(&new, &[]), &opts);
    let render = t.node(t.find("steps[Render]").unwrap()).unwrap();
    assert_eq!(render.status, DiffStatus::Modified);
    assert!(!render.expanded);
    assert!(render.children.is_empty());
    assert!(t.nodes().iter().all(|n| n.depth <= 1));
}

#[test]
fn max_depth_zero_reports_only_the_root() {
    let new = BASE.replace("command: render", "command: render2");
    let opts = DiffOptions::default().with_max_depth(0);
    let t = diff_jobs(&job(BASE, &[]), &job(&new, &[]), &opts);
    assert!(t.is_changed());
    assert_eq!(t.nodes().len(), 1);
}

#[test]
fn unexpanded_step_can_be_diffed_on_its_own() {
    let (a, b) = (
        job(BASE, &[]),
        job(&BASE.replace("command: render", "command: render2"), &[]),
    );
    let t = diff_steps(&a.steps[1], &b.steps[1], &DiffOptions::default());
    assert_eq!(fields(&t, "script.onRun"), vec!["command"]);
}

#[test]
fn added_and_removed_steps() {
    let new = BASE
        .replace("  - name: Prep\n", "  - name: Setup\n")
        .replace("dependsOn: Prep", "dependsOn: Setup");
    let t = diff_jobs(&job(BASE, &[]), &job(&new, &[]), &DiffOptions::default());
    assert_eq!(status(&t, "steps[Prep]"), DiffStatus::Removed);
    assert_eq!(status(&t, "steps[Setup]"), DiffStatus::Added);
    assert_eq!(fields(&t, "steps[Render]"), vec!["dependencies"]);
    assert_attributed(&t);
}

#[test]
fn job_parameter_change_marks_exactly_the_steps_that_read_it() {
    // Prep reads Param.Scene; Render reads only Param.Frames.
    let (a, b) = (job(BASE, &[]), job(BASE, &[("Scene", "b.blend")]));
    let t = diff_jobs(&a, &b, &DiffOptions::default());
    assert_eq!(fields(&t, "parameters[Scene]"), vec!["value"]);
    assert_eq!(fields(&t, "steps[Prep]"), vec!["resolvedSymTab"]);
    assert_eq!(status(&t, "steps[Render]"), DiffStatus::Unchanged);
    assert_eq!(status(&t, "jobEnvironments[Conda]"), DiffStatus::Unchanged);
    assert_attributed(&t);

    // Opting out hides the change in Prep: its command is still the
    // unresolved "{{Param.Scene}}" on both sides.
    let t = diff_jobs(&a, &b, &DiffOptions::default().with_resolved_symtab(false));
    assert_eq!(status(&t, "steps[Prep]"), DiffStatus::Unchanged);
}

#[test]
fn task_range_change_names_the_task_parameter() {
    let (a, b) = (job(BASE, &[]), job(BASE, &[("Frames", "1-20")]));
    let t = diff_jobs(&a, &b, &DiffOptions::default());
    assert_eq!(
        fields(
            &t,
            "steps[Render].parameterSpace.taskParameterDefinitions[Frame]"
        ),
        vec!["definition"]
    );
    assert_attributed(&t);
}

#[test]
fn step_reorder_is_one_order_change_not_n_modifications() {
    let new = r#"
specificationVersion: jobtemplate-2023-09
name: Demo
steps:
  - name: B
    script: {actions: {onRun: {command: b}}}
  - name: A
    script: {actions: {onRun: {command: a}}}
"#;
    let old = r#"
specificationVersion: jobtemplate-2023-09
name: Demo
steps:
  - name: A
    script: {actions: {onRun: {command: a}}}
  - name: B
    script: {actions: {onRun: {command: b}}}
"#;
    let t = diff_jobs(&job(old, &[]), &job(new, &[]), &DiffOptions::default());
    assert_eq!(fields(&t, ""), vec!["steps.order"]);
    assert_eq!(status(&t, "steps[A]"), DiffStatus::Unchanged);
    assert_eq!(status(&t, "steps[B]"), DiffStatus::Unchanged);
}

#[test]
fn environment_variable_change_is_keyed() {
    let new = BASE.replace("CONDA_PACKAGES: blender", "CONDA_PACKAGES: blender=4.2");
    let t = diff_jobs(&job(BASE, &[]), &job(&new, &[]), &DiffOptions::default());
    assert_eq!(
        fields(&t, "jobEnvironments[Conda]"),
        vec!["variables.CONDA_PACKAGES"]
    );
    assert_eq!(status(&t, "steps[Render]"), DiffStatus::Unchanged);
    assert_attributed(&t);
}

#[test]
fn environment_templates_diff_through_convert_environment() {
    let tpl = |cmd: &str| {
        let yaml = format!(
            r#"
specificationVersion: environment-2023-09
environment:
  name: Env
  script:
    actions:
      onEnter: {{command: {cmd}}}
"#
        );
        let v: serde_json::Value = serde_saphyr::from_str(&yaml).unwrap();
        let et = decode_environment_template(v, None, &CallerLimits::default()).unwrap();
        convert_environment(et.environment())
    };
    let t = diff_environments(&tpl("start"), &tpl("start2"), &DiffOptions::default());
    assert_eq!(t.root().kind, DiffNodeKind::Environment);
    assert_eq!(fields(&t, "script.onEnter"), vec!["command"]);
    assert!(t.find("script.onExit").is_none(), "absent on both sides");
    assert_attributed(&t);
}

#[test]
fn step_nodes_carry_name_and_old_and_new_positions() {
    // Insert a step before Render: Render moves from #1 to #2.
    let new = BASE.replace(
        "  - name: Render\n",
        "  - name: Lint\n    script:\n      actions: {onRun: {command: lint}}\n  - name: Render\n",
    );
    let t = diff_jobs(&job(BASE, &[]), &job(&new, &[]), &DiffOptions::default());
    let at = |p: &str| t.node(t.find(p).unwrap()).unwrap();
    let render = at("steps[Render]");
    assert_eq!(render.key.as_deref(), Some("Render"));
    assert_eq!((render.old_index, render.new_index), (Some(1), Some(2)));
    assert_eq!(render.status, DiffStatus::Unchanged);
    let lint = at("steps[Lint]");
    assert_eq!((lint.old_index, lint.new_index), (None, Some(1)));
    // Non-collection nodes have no key or position.
    assert_eq!(t.root().key, None);
    assert_eq!(t.root().old_index, None);
    assert_eq!(t.root().step_index, None);
}

#[test]
fn step_index_is_threaded_to_every_node_in_the_step() {
    // Prep removed (old#0), Render modified (new#0), Lint added (new#1).
    let new = BASE
        .replace(
            "  - name: Prep\n    script:\n      actions: {onRun: {command: prep, args: [\"{{Param.Scene}}\"]}}\n",
            "",
        )
        .replace("    dependencies: [{dependsOn: Prep}]\n", "")
        .replace("command: render", "command: render2")
        + "  - name: Lint\n    script:\n      actions: {onRun: {command: lint}}\n";
    let t = diff_jobs(&job(BASE, &[]), &job(&new, &[]), &DiffOptions::default());
    let at = |p: &str| t.node(t.find(p).unwrap()).unwrap().step_index;
    assert_eq!(at("steps[Prep]"), Some(0), "removed: index into old.steps");
    assert_eq!(
        at("steps[Render]"),
        Some(0),
        "present: index into new.steps"
    );
    assert_eq!(at("steps[Render].script"), Some(0));
    assert_eq!(at("steps[Render].script.onRun"), Some(0));
    assert_eq!(at("steps[Lint]"), Some(1));
    assert_eq!(at("jobEnvironments[Conda]"), None);
    // Every node under a step node agrees with it.
    for (id, n) in t.nodes().iter().enumerate() {
        if let Some(p) = n.parent {
            let parent = t.node(p).unwrap();
            if parent.step_index.is_some() {
                assert_eq!(n.step_index, parent.step_index, "{}", t.path(id));
            }
        }
    }
}

#[test]
fn display_lists_only_changes() {
    let new = BASE.replace("command: render", "command: render2");
    let t = diff_jobs(&job(BASE, &[]), &job(&new, &[]), &DiffOptions::default());
    let s = t.to_string();
    assert!(s.contains("~ steps[Render]"), "{s}");
    assert!(s.contains("command: \"render\" -> \"render2\""), "{s}");
    assert!(!s.contains("Prep"), "{s}");
}

// ─── coverage for every node kind (also kills subtree_eq/expand mutants) ───

const RICH: &str = r#"
specificationVersion: jobtemplate-2023-09
name: Rich
parameterDefinitions:
  - {name: Pkg, type: STRING, default: blender}
jobEnvironments:
  - name: Env
    variables: {PKG: "{{Param.Pkg}}"}
    script:
      actions:
        onEnter: {command: enter}
        onExit: {command: exit}
      embeddedFiles:
        - {name: Rc, type: TEXT, data: "rc"}
steps:
  - name: S
    stepEnvironments:
      - name: StepEnv
        variables: {PKG: "{{Param.Pkg}}"}
    parameterSpace:
      taskParameterDefinitions:
        - {name: Frame, type: INT, range: "1-3"}
        - {name: Cam, type: STRING, range: [left, right]}
    hostRequirements:
      amounts:
        - {name: amount.worker.vcpu, min: 2}
        - {name: amount.worker.memory, min: 1024}
      attributes:
        - {name: attr.worker.os.family, anyOf: [linux]}
    script:
      actions:
        onRun: {command: run}
      embeddedFiles:
        - {name: A, type: TEXT, data: "a"}
        - {name: B, type: TEXT, data: "b"}
"#;

fn rich(edit: impl Fn(&str) -> String) -> (Job, Job) {
    (job(RICH, &[]), job(&edit(RICH), &[]))
}

#[test]
fn unchanged_siblings_of_every_kind_stay_unchanged() {
    // One leaf change per subtree; its siblings of each node kind must stay Unchanged.
    let (a, b) = rich(|t| {
        t.replace("command: enter", "command: enter2")
            .replace("data: \"a\"", "data: \"a2\"")
            .replace("range: \"1-3\"", "range: \"1-4\"")
            .replace("command: run", "command: run2")
    });
    let t = diff_jobs(&a, &b, &DiffOptions::default());
    assert_eq!(
        fields(&t, "jobEnvironments[Env].script.onEnter"),
        vec!["command"]
    );
    assert_eq!(
        status(&t, "jobEnvironments[Env].script.onExit"),
        DiffStatus::Unchanged
    );
    assert_eq!(
        status(&t, "jobEnvironments[Env].script.embeddedFiles[Rc]"),
        DiffStatus::Unchanged
    );
    assert_eq!(fields(&t, "steps[S].script.embeddedFiles[A]"), vec!["data"]);
    assert_eq!(
        status(&t, "steps[S].script.embeddedFiles[B]"),
        DiffStatus::Unchanged
    );
    assert_eq!(
        status(&t, "steps[S].parameterSpace.taskParameterDefinitions[Cam]"),
        DiffStatus::Unchanged
    );
    assert_eq!(
        status(&t, "steps[S].hostRequirements"),
        DiffStatus::Unchanged
    );
    assert_eq!(
        status(&t, "steps[S].stepEnvironments[StepEnv]"),
        DiffStatus::Unchanged
    );
    assert_attributed(&t);

    // Environment variables change but its script does not.
    let (a, b) = rich(|t| t.replacen("{PKG: \"{{Param.Pkg}}\"}", "{PKG: x}", 1));
    let t = diff_jobs(&a, &b, &DiffOptions::default());
    assert!(fields(&t, "jobEnvironments[Env]").contains(&"variables.PKG".to_string()));
    assert_eq!(
        status(&t, "jobEnvironments[Env].script"),
        DiffStatus::Unchanged
    );
    assert_attributed(&t);
}

#[test]
fn host_requirements_entries_and_order() {
    let (a, b) = rich(|t| t.replace("min: 2}", "min: 3}"));
    let t = diff_jobs(&a, &b, &DiffOptions::default());
    assert_eq!(
        fields(&t, "steps[S].hostRequirements"),
        vec!["amounts[amount.worker.vcpu]"]
    );
    assert_eq!(status(&t, "steps[S].script"), DiffStatus::Unchanged);

    let (a, b) = rich(|t| t.replace("anyOf: [linux]", "anyOf: [windows]"));
    let t = diff_jobs(&a, &b, &DiffOptions::default());
    assert_eq!(
        fields(&t, "steps[S].hostRequirements"),
        vec!["attributes[attr.worker.os.family]"]
    );

    // Swap the two amounts and change one: both the order and the entry are reported.
    let (a, b) = rich(|t| {
        t.replace(
            "        - {name: amount.worker.vcpu, min: 2}\n        - {name: amount.worker.memory, min: 1024}\n",
            "        - {name: amount.worker.memory, min: 1024}\n        - {name: amount.worker.vcpu, min: 3}\n",
        )
    });
    let t = diff_jobs(&a, &b, &DiffOptions::default());
    assert_eq!(
        fields(&t, "steps[S].hostRequirements"),
        vec!["amounts.order", "amounts[amount.worker.vcpu]"]
    );
}

#[test]
fn task_parameter_definition_order_is_a_change() {
    // Same tasks, different iteration order (and so different task indices).
    let (a, b) = rich(|t| {
        t.replace(
            "        - {name: Frame, type: INT, range: \"1-3\"}\n        - {name: Cam, type: STRING, range: [left, right]}\n",
            "        - {name: Cam, type: STRING, range: [left, right]}\n        - {name: Frame, type: INT, range: \"1-3\"}\n",
        )
    });
    assert_eq!(
        a.steps[0].parameter_space, b.steps[0].parameter_space,
        "== ignores order"
    );
    let t = diff_jobs(&a, &b, &DiffOptions::default());
    assert_eq!(status(&t, "steps[S]"), DiffStatus::Modified);
    assert_eq!(
        fields(&t, "steps[S].parameterSpace"),
        vec!["taskParameterDefinitions.order"]
    );
    let t = diff_steps(&a.steps[0], &b.steps[0], &DiffOptions::default());
    assert!(t.is_changed());
}

#[test]
fn parameter_change_reaches_environments_through_their_symtab() {
    let (a, b) = (job(RICH, &[]), job(RICH, &[("Pkg", "maya")]));
    let t = diff_jobs(&a, &b, &DiffOptions::default());
    assert_eq!(fields(&t, "jobEnvironments[Env]"), vec!["resolvedSymTab"]);
    // Step environments carry no table of their own; the step's table holds
    // the symbols they reference, so the change lands on the step.
    assert_eq!(fields(&t, "steps[S]"), vec!["resolvedSymTab"]);
    assert_eq!(
        status(&t, "steps[S].stepEnvironments[StepEnv]"),
        DiffStatus::Unchanged
    );
    assert_attributed(&t);
    // Opting out strips job- and step-environment tables as well as the step's.
    let t = diff_jobs(&a, &b, &DiffOptions::default().with_resolved_symtab(false));
    assert_eq!(status(&t, "jobEnvironments[Env]"), DiffStatus::Unchanged);
    assert_eq!(status(&t, "steps[S]"), DiffStatus::Unchanged);
    let t = diff_steps(
        &a.steps[0],
        &b.steps[0],
        &DiffOptions::default().with_resolved_symtab(false),
    );
    assert!(!t.is_changed());
    let (ea, eb) = (
        &a.job_environments.as_ref().unwrap()[0],
        &b.job_environments.as_ref().unwrap()[0],
    );
    assert!(diff_environments(ea, eb, &DiffOptions::default()).is_changed());
    assert!(
        !diff_environments(ea, eb, &DiffOptions::default().with_resolved_symtab(false))
            .is_changed()
    );
}

#[test]
fn none_versus_empty_collections_are_attributed() {
    let a = job(RICH, &[]);
    let mut b = a.clone();
    b.steps[0].step_environments = Some(vec![]);
    b.steps[0].script.embedded_files = Some(vec![]);
    let mut a2 = a.clone();
    a2.steps[0].step_environments = None;
    a2.steps[0].script.embedded_files = None;
    let t = diff_jobs(&a2, &b, &DiffOptions::default());
    assert_eq!(fields(&t, "steps[S]"), vec!["stepEnvironments"]);
    assert_eq!(fields(&t, "steps[S].script"), vec!["embeddedFiles"]);
    assert_attributed(&t);

    let mut c = a.clone();
    c.job_environments = None;
    let mut d = a.clone();
    d.job_environments = Some(vec![]);
    let t = diff_jobs(&c, &d, &DiffOptions::default());
    assert_eq!(fields(&t, ""), vec!["jobEnvironments"]);
}

#[test]
fn duplicate_names_are_reported_not_dropped() {
    let a = job(RICH, &[]);
    let mut b = a.clone();
    let mut dup = b.steps[0].clone();
    dup.script.actions.on_run.command = a.steps[0].script.actions.on_run.command.clone();
    dup.description = Some("second copy".into());
    b.steps.push(dup);
    let t = diff_jobs(&a, &b, &DiffOptions::default());
    assert_eq!(fields(&t, ""), vec!["steps.duplicates"]);
    assert_attributed(&t);
}

#[test]
fn path_find_and_changed_agree() {
    let (a, b) = rich(|t| t.replace("command: run", "command: run2"));
    let t = diff_jobs(&a, &b, &DiffOptions::default());
    let id = t.find("steps[S].script.onRun").unwrap();
    assert_eq!(t.path(id), "steps[S].script.onRun");
    assert_eq!(t.path(0), "");
    for (i, _) in t.nodes().iter().enumerate() {
        assert_eq!(t.find(&t.path(i)), Some(i));
    }
    let changed: Vec<String> = t.changed().map(|(i, _)| t.path(i)).collect();
    assert_eq!(
        changed,
        vec!["", "steps[S]", "steps[S].script", "steps[S].script.onRun"]
    );
}

#[test]
fn display_marks_unexpanded_nodes_and_the_root() {
    let (a, b) = rich(|t| t.replace("command: run", "command: run2"));
    let s = diff_jobs(&a, &b, &DiffOptions::default().with_max_depth(1)).to_string();
    assert_eq!(s, "~ <root>\n  ~ steps[S] (not expanded)\n");
    let s = diff_jobs(&a, &b, &DiffOptions::default()).to_string();
    assert!(!s.contains("not expanded"), "{s}");
    let mut c = b.clone();
    c.steps.clear();
    let s = diff_jobs(&a, &c, &DiffOptions::default()).to_string();
    assert!(s.contains("- steps[S]\n"), "{s}");
    let s = diff_jobs(&c, &a, &DiffOptions::default()).to_string();
    assert!(s.contains("+ steps[S]\n"), "{s}");
}

#[test]
fn duplicate_names_on_both_sides_with_different_copies_are_attributed() {
    let a = job(RICH, &[]);
    let mut dup = a.steps[0].clone();
    dup.description = Some("copy one".into());
    let (mut x, mut y) = (a.clone(), a.clone());
    x.steps.push(dup.clone());
    dup.description = Some("copy two".into());
    y.steps.push(dup);
    let t = diff_jobs(&x, &y, &DiffOptions::default());
    assert!(t.is_changed());
    assert_eq!(fields(&t, ""), vec!["steps.duplicates"]);
    assert_attributed(&t);
    // Identical duplicates on both sides: no change at all.
    assert!(!diff_jobs(&x, &x.clone(), &DiffOptions::default()).is_changed());
}

#[test]
fn definition_reorder_under_an_explicit_combination_is_not_a_change() {
    let with_combo = RICH.replace(
        "        - {name: Cam, type: STRING, range: [left, right]}\n",
        "        - {name: Cam, type: STRING, range: [left, right]}\n      combination: \"Frame * Cam\"\n",
    );
    let swapped = with_combo.replace(
        "        - {name: Frame, type: INT, range: \"1-3\"}\n        - {name: Cam, type: STRING, range: [left, right]}\n",
        "        - {name: Cam, type: STRING, range: [left, right]}\n        - {name: Frame, type: INT, range: \"1-3\"}\n",
    );
    assert_ne!(with_combo, swapped);
    let t = diff_jobs(
        &job(&with_combo, &[]),
        &job(&swapped, &[]),
        &DiffOptions::default(),
    );
    assert!(!t.is_changed(), "{t}");
}

#[test]
fn none_versus_non_empty_is_only_an_added_child() {
    let a = job(RICH, &[]);
    let mut b = a.clone();
    b.job_environments = None;
    b.steps[0].script.embedded_files = None;
    let t = diff_jobs(&b, &a, &DiffOptions::default());
    assert_eq!(status(&t, "jobEnvironments[Env]"), DiffStatus::Added);
    assert!(fields(&t, "").is_empty(), "{t}");
    assert_eq!(
        status(&t, "steps[S].script.embeddedFiles[A]"),
        DiffStatus::Added
    );
    assert!(fields(&t, "steps[S].script").is_empty(), "{t}");
}

#[test]
fn opting_out_strips_a_hand_set_step_environment_table() {
    let a = job(RICH, &[]);
    let mut b = a.clone();
    let table = a.job_environments.as_ref().unwrap()[0]
        .resolved_symtab
        .clone();
    assert!(table.is_some());
    b.steps[0].step_environments.as_mut().unwrap()[0].resolved_symtab = table;
    let off = DiffOptions::default().with_resolved_symtab(false);
    assert!(!diff_jobs(&a, &b, &off).is_changed());
    assert!(!diff_steps(&a.steps[0], &b.steps[0], &off).is_changed());
    assert!(diff_jobs(&a, &b, &DiffOptions::default()).is_changed());
}

#[test]
fn adding_a_combination_reports_only_the_combination() {
    let with_combo = RICH.replace(
        "        - {name: Cam, type: STRING, range: [left, right]}\n",
        "        - {name: Cam, type: STRING, range: [left, right]}\n      combination: \"Frame * Cam\"\n",
    );
    let swapped = with_combo.replace(
        "        - {name: Frame, type: INT, range: \"1-3\"}\n        - {name: Cam, type: STRING, range: [left, right]}\n",
        "        - {name: Cam, type: STRING, range: [left, right]}\n        - {name: Frame, type: INT, range: \"1-3\"}\n",
    );
    let t = diff_jobs(
        &job(RICH, &[]),
        &job(&swapped, &[]),
        &DiffOptions::default(),
    );
    assert_eq!(fields(&t, "steps[S].parameterSpace"), vec!["combination"]);
}
