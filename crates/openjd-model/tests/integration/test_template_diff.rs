// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! `job::template_diff`: the cases in `specs/model/template-diff.md` § Examples,
//! with the sample templates in `examples/template_diff/`.

use std::path::PathBuf;

use openjd_expr::ExprValue;
use openjd_model::job::apply::MutationKind;
use openjd_model::job::diff::{DiffOptions, DiffStatus};
use openjd_model::job::template_diff::*;
use openjd_model::JobParameterInputValues;

fn sample_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/template_diff")
        .join(name)
}

fn sample(name: &str) -> TemplateSource {
    TemplateSource::Path(sample_path(name))
}

/// A sample with `Scene` set; it has no default.
fn input(name: &str) -> TemplateInput {
    TemplateInput::new(sample(name)).param("Scene", "shot.blend")
}

fn text(s: &str) -> TemplateInput {
    TemplateInput::new(TemplateSource::Text(s.to_string()))
}

fn params(pairs: &[(&str, &str)]) -> JobParameterInputValues {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), ExprValue::String(v.to_string())))
        .collect()
}

fn opts() -> DiffOptions {
    DiffOptions::default()
}

fn lines(c: &[Change]) -> Vec<String> {
    c.iter().map(ToString::to_string).collect()
}

const CASE1: &str = "\
~ parameter Frames: 1-100 -> 1-200  [parameters]
~ step Render
    ~ Frame: 1-100 -> 1-200  [parameters]
    tasks: 100 -> 200
      keep   100   Frame 1-100 × Eye left
      add    100   Frame 101-200 × Eye left

1 step changed. Tasks: 100 kept, 100 added, 0 removed.";

// ─── the five cases ─────────────────────────────────────────────────

#[test]
fn case1_frames_parameter_set_appends_100_tasks() {
    let d = diff_job_template_with_parameter_sets(
        &input("render_v1.yaml"),
        &params(&[("Frames", "1-100")]),
        &params(&[("Frames", "1-200")]),
        &opts(),
    )
    .unwrap();
    assert_eq!(d.to_string(), CASE1);
    assert!(d.changes.iter().all(|c| c.origin == Origin::Parameters));
    let stages: Vec<Stage> = d.changes.iter().map(|c| c.stage).collect();
    assert_eq!(stages, [Stage::Parameters, Stage::Job]);

    // The same through diff_templates with one file on both sides.
    let same = diff_templates(
        &input("render_v1.yaml"),
        &input("render_v1.yaml").param("Frames", "1-200"),
        &opts(),
    )
    .unwrap();
    assert_eq!(same.to_string(), CASE1);
    assert!(same.changes.iter().all(|c| c.origin == Origin::Parameters));
}

#[test]
fn case2_template_adds_right_eye() {
    let d = diff_templates(&input("render_v1.yaml"), &input("render_v2.yaml"), &opts()).unwrap();
    assert_eq!(
        d.to_string(),
        "\
~ step Render
    ~ Eye: [left] -> [left, right]  [template]
    tasks: 100 -> 200
      keep   100   Frame 1-100 × Eye left
      add    100   Frame 1-100 × Eye right

1 step changed. Tasks: 100 kept, 100 added, 0 removed."
    );
    assert_eq!(d.changes[0].origin, Origin::Template);
    assert_eq!(d.changes[0].stage, Stage::Job);
}

const CASE3: &str = "\
~ parameter Frames: 1-100 -> 1-200  [parameters]
~ step Render
    ~ Frame: 1-100 -> 1-200  [parameters]
    ~ Eye: [left] -> [left, right]  [template]
    tasks: 100 -> 400
      keep   100   Frame 1-100 × Eye left
      add    200   Frame 101-200 × Eye left, right
      add    100   Frame 1-100 × Eye right

1 step changed. Tasks: 100 kept, 300 added, 0 removed.";

fn case3() -> TemplateDiff {
    diff_templates(
        &input("render_v1.yaml"),
        &input("render_v2.yaml").param("Frames", "1-200"),
        &opts(),
    )
    .unwrap()
}

#[test]
fn case3_both_changes_have_mixed_origins() {
    let d = case3();
    assert_eq!(d.to_string(), CASE3);
    let origins: Vec<(&str, Origin)> = d
        .changes
        .iter()
        .map(|c| (c.path.as_str(), c.origin))
        .collect();
    assert_eq!(
        origins,
        [
            ("parameters[Frames]", Origin::Parameters),
            (
                "steps[Render].parameterSpace.taskParameterDefinitions[Eye]",
                Origin::Template
            ),
            (
                "steps[Render].parameterSpace.taskParameterDefinitions[Frame]",
                Origin::Parameters
            ),
        ]
    );
    assert_eq!(
        d.stat(),
        DiffStat {
            steps_changed: 1,
            tasks_kept: 100,
            tasks_added: 300,
            ..DiffStat::default()
        }
    );
    assert!(d.notes.is_empty());
}

#[test]
fn case4_new_dimension_replaces_every_task() {
    let d = diff_templates(
        &input("render_no_eye.yaml"),
        &input("render_v1.yaml"),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        d.to_string(),
        "\
~ step Render
    + Eye: [left]  [template]
    ~ script.onRun.args: [{{Param.Scene}}, {{Task.Param.Frame}}] -> [{{Param.Scene}}, {{Task.Param.Frame}}, {{Task.Param.Eye}}]  [template]
    tasks: 100 -> 100, all replaced (task parameter added or removed)

1 step changed. Tasks: 0 kept, 0 added, 0 removed, 100 replaced."
    );
    assert_eq!(
        d.step("Render").map(|s| &s.tasks),
        Some(&TaskSummary::Replaced {
            old_count: 100,
            new_count: 100,
            reason: "task parameter added or removed".into(),
        })
    );
    // The args reference `Task.` symbols, so only a worker resolves them.
    assert_eq!(d.changes[1].field, "args");
    assert_eq!(d.changes[1].stage, Stage::RunTime);
}

#[test]
fn case5_scene_path_changes_the_raw_param_symbol() {
    let (a, b) = if cfg!(windows) {
        (r"C:\proj\a.blend", r"C:\proj\b.blend")
    } else {
        ("/proj/a.blend", "/proj/b.blend")
    };
    let d = diff_job_template_with_parameter_sets(
        &input("render_v1.yaml"),
        &params(&[("Scene", a)]),
        &params(&[("Scene", b)]),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        d.to_string(),
        format!(
            "\
~ parameter Scene: {a} -> {b}  [parameters]
~ step Render
    ~ resolvedSymTab: RawParam.Scene={a} -> RawParam.Scene={b}  [parameters]
    tasks: 100 unchanged

1 step changed. Tasks: 100 kept, 0 added, 0 removed."
        )
    );
    let stages: Vec<(&str, Stage)> = d
        .changes
        .iter()
        .map(|c| (c.field.as_str(), c.stage))
        .collect();
    assert_eq!(
        stages,
        [("value", Stage::Parameters), ("resolvedSymTab", Stage::Job)]
    );
}

// ─── template layer ─────────────────────────────────────────────────

#[test]
fn template_default_change_with_parameter_unsupplied_is_template_origin() {
    let d = diff_templates(
        &input("render_v1.yaml"),
        &input("render_default_1_200.yaml"),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        d.to_string(),
        "\
~ parameter definition Frames.default: 1-100 -> 1-200  [template]
~ parameter Frames: 1-100 -> 1-200  [template]
~ step Render
    ~ Frame: 1-100 -> 1-200  [template]
    tasks: 100 -> 200
      keep   100   Frame 1-100 × Eye left
      add    100   Frame 101-200 × Eye left

1 step changed. Tasks: 100 kept, 100 added, 0 removed."
    );
    assert_eq!(d.changes[0].layer, Layer::Template);
    assert_eq!(d.changes[0].stage, Stage::Template);
}

const PARAMS_OLD: &str = r#"
specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: P
parameterDefinitions:
  - {name: S, type: STRING, description: a, default: x, allowedValues: [x, y], minLength: 1, maxLength: 5, userInterface: {control: DROPDOWN_LIST, label: L}}
  - {name: I, type: INT, default: 5, minValue: 1, maxValue: 10}
  - {name: F, type: FLOAT, default: 1.5, minValue: 0, userInterface: {label: old}}
  - {name: LI, type: "LIST[INT]", default: [1], userInterface: {label: old}}
  - {name: LF, type: "LIST[FLOAT]", default: [1.5], userInterface: {label: old}}
  - {name: T, type: STRING, default: "3"}
  - {name: Gone, type: STRING, default: g}
steps:
  - name: Render
    script:
      actions:
        onRun: {command: echo}
"#;

const PARAMS_NEW: &str = r#"
specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: P
parameterDefinitions:
  - {name: S, type: STRING, description: b, default: x, allowedValues: [x, y, z], minLength: 0, maxLength: 9, userInterface: {control: DROPDOWN_LIST, label: M}}
  - {name: I, type: INT, default: 5, minValue: 2, maxValue: 20}
  - {name: F, type: FLOAT, default: 1.5, maxValue: 9.5, userInterface: {label: new}}
  - {name: LI, type: "LIST[INT]", default: [1], userInterface: {label: new}}
  - {name: LF, type: "LIST[FLOAT]", default: [1.5], userInterface: {label: new}}
  - {name: T, type: INT, default: 3}
  - {name: New, type: STRING, default: n}
steps:
  - name: Render
    script:
      actions:
        onRun: {command: echo}
"#;

#[test]
fn parameter_definition_changes_are_compared_field_by_field() {
    let d = diff_templates(&text(PARAMS_OLD), &text(PARAMS_NEW), &opts()).unwrap();
    assert_eq!(
        d.to_string(),
        r#"+ parameter definition F.maxValue: 9.5  [template]
- parameter definition F.minValue: 0  [template]
~ parameter definition F.userInterface.label: old -> new  [template]
~ parameter definition I.maxValue: 10 -> 20  [template]
~ parameter definition I.minValue: 1 -> 2  [template]
~ parameter definition LF.userInterface.label: old -> new  [template]
~ parameter definition LI.userInterface.label: old -> new  [template]
~ parameter definition S.allowedValues: [x, y] -> [x, y, z]  [template]
~ parameter definition S.description: a -> b  [template]
~ parameter definition S.maxLength: 5 -> 9  [template]
~ parameter definition S.minLength: 1 -> 0  [template]
~ parameter definition S.userInterface.label: L -> M  [template]
~ parameter definition T.default: "3" -> 3  [template]
~ parameter definition T.type: STRING -> INT  [template]
- parameter Gone: g, defined as {default: g, type: STRING}  [template]
+ parameter New: n, defined as {default: n, type: STRING}  [template]
~ parameter T: "3" (STRING) -> 3 (INT)  [template]

0 steps changed. Tasks: 1 kept, 0 added, 0 removed."#
    );
    let all = lines(&d.changes);
    for p in ["F", "LI", "LF"] {
        let ui: Vec<String> = all
            .iter()
            .filter(|l| l.contains(&format!("[{p}].userInterface")))
            .cloned()
            .collect();
        assert_eq!(
            ui,
            [format!(
                "~ parameterDefinitions[{p}].userInterface.label: old -> new  [template]"
            )]
        );
    }
    let defs: Vec<&Change> = d.changes_at("parameterDefinitions[S]").collect();
    assert_eq!(defs.len(), 5);
    assert!(defs
        .iter()
        .all(|c| c.layer == Layer::Template && c.stage == Stage::Template));
}

const LET_OLD: &str = r#"
specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: L
steps:
  - name: Render
    let: ["unused = 1"]
    script:
      actions:
        onRun: {command: echo}
"#;

#[test]
fn step_let_nothing_reads_shows_only_in_the_template_layer() {
    let d = diff_templates(
        &text(LET_OLD),
        &text(&LET_OLD.replace("unused = 1", "unused = 2")),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        d.to_string(),
        "\
~ step Render
    ~ let: [unused = 1] -> [unused = 2]  [template]
    tasks: 1 unchanged

1 step changed. Tasks: 1 kept, 0 added, 0 removed."
    );
    assert!(!d.job.is_changed());
    assert_eq!(
        lines(&d.changes),
        ["~ steps[Render].let: [unused = 1] -> [unused = 2]  [template]"]
    );
}

#[test]
fn task_param_reference_change_is_run_time() {
    let old = std::fs::read_to_string(sample_path("render_v1.yaml")).unwrap();
    let new = old.replace("{{Task.Param.Frame}}", "{{Task.RawParam.Frame}}");
    let d = diff_templates(
        &text(&old).param("Scene", "s"),
        &text(&new).param("Scene", "s"),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        lines(&d.changes),
        ["~ steps[Render].script.onRun.args: [{{Param.Scene}}, {{Task.Param.Frame}}, {{Task.Param.Eye}}] -> [{{Param.Scene}}, {{Task.RawParam.Frame}}, {{Task.Param.Eye}}]  [template]"]
    );
    assert_eq!(d.changes[0].stage, Stage::RunTime);
}

const NO_EXT: &str = r#"
specificationVersion: jobtemplate-2023-09
name: E
steps:
  - name: Render
    script:
      actions:
        onRun: {command: echo}
"#;

#[test]
fn extensions_change_is_reported_by_both_layers_and_listed_once() {
    let with = NO_EXT.replace("name: E", "extensions: [EXPR]\nname: E");
    let d = diff_templates(&text(NO_EXT), &text(&with), &opts()).unwrap();
    assert_eq!(
        d.to_string(),
        "\
+ extensions: [EXPR]  [template]

0 steps changed. Tasks: 1 kept, 0 added, 0 removed."
    );
    let layers: Vec<(Layer, &str)> = d
        .changes
        .iter()
        .map(|c| (c.layer, c.field.as_str()))
        .collect();
    assert_eq!(
        layers,
        [(Layer::Template, "extensions"), (Layer::Job, "extensions")]
    );
}

// ─── identity, equality, serde ──────────────────────────────────────

#[test]
fn identical_inputs_have_no_changes() {
    let d = diff_templates(&input("render_v1.yaml"), &input("render_v1.yaml"), &opts()).unwrap();
    assert!(!d.is_changed());
    assert_eq!(d.to_string(), "no changes");
    assert!(d.changes.is_empty());
    assert_eq!(d.stat().tasks_kept, 100);
    assert_eq!(d.plan().unwrap().ops.len(), 0);
}

#[test]
fn two_diffs_of_the_same_inputs_are_equal() {
    assert_eq!(case3(), case3());
    assert_ne!(
        case3(),
        diff_templates(&input("render_v1.yaml"), &input("render_v2.yaml"), &opts()).unwrap()
    );
}

#[test]
fn serde_shape_is_pinned() {
    let d = diff_templates(&input("render_v1.yaml"), &input("render_v2.yaml"), &opts()).unwrap();
    let mut v = serde_json::to_value(&d).unwrap();
    let obj = v.as_object_mut().unwrap();
    assert!(obj.remove("oldJob").is_some());
    assert!(obj.remove("newJob").is_some());
    assert_eq!(
        v,
        serde_json::json!({
            "changes": [{
                "layer": "job",
                "path": "steps[Render].parameterSpace.taskParameterDefinitions[Eye]",
                "step": "Render",
                "field": "definition",
                "before": {"raw": {"string": {"range": ["left"]}}, "text": "[left]"},
                "after": {"raw": {"string": {"range": ["left", "right"]}}, "text": "[left, right]"},
                "origin": "template",
                "stage": "job",
            }],
            "steps": [{
                "name": "Render",
                "status": "modified",
                "oldIndex": 0,
                "newIndex": 0,
                "tasks": {
                    "kind": "product",
                    "kept": {"count": 100, "dims": [["Frame", "1-100"], ["Eye", "left"]]},
                    "added": [{"count": 100, "dims": [["Frame", "1-100"], ["Eye", "right"]]}],
                    "removed": [],
                },
            }],
            "job": [
                {"path": "", "status": "modified", "expanded": true, "fieldChanges": []},
                {"path": "steps[Render]", "status": "modified", "expanded": true, "fieldChanges": []},
                {"path": "steps[Render].parameterSpace", "status": "modified", "expanded": true, "fieldChanges": []},
                {"path": "steps[Render].parameterSpace.taskParameterDefinitions[Eye]", "status": "modified",
                 "expanded": true, "fieldChanges": [{
                    "field": "definition",
                    "before": {"string": {"range": ["left"]}},
                    "after": {"string": {"range": ["left", "right"]}},
                 }]},
            ],
            "notes": [],
        })
    );
}

#[test]
fn replaced_task_summary_serializes_camel_case() {
    let v = serde_json::to_value(TaskSummary::Replaced {
        old_count: 1,
        new_count: 2,
        reason: "r".into(),
    })
    .unwrap();
    assert_eq!(
        v,
        serde_json::json!({"kind": "replaced", "oldCount": 1, "newCount": 2, "reason": "r"})
    );
}

// ─── origin fallbacks and errors ────────────────────────────────────

#[test]
fn mid_build_failure_marks_job_changes_both_with_a_note() {
    let old = std::fs::read_to_string(sample_path("render_v1.yaml")).unwrap();
    let new = old.replace(
        "  - {name: Scene, type: PATH}",
        "  - {name: Scene, type: PATH}\n  - {name: Quality, type: INT}",
    );
    let d = diff_templates(
        &text(&old).param("Scene", "s"),
        &text(&new).param("Scene", "s").param("Quality", "3"),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        d.notes,
        [Note {
            step: None,
            text: MID_NOTE.to_string()
        }]
    );
    assert_eq!(
        lines(&d.changes),
        [
            "+ parameterDefinitions[Quality]: {type: INT}  [template]",
            "+ parameters[Quality]: 3  [both]",
        ]
    );
}

const MID_NOTE: &str = "origin: the new template does not build with the old parameters, so job-layer changes are marked both: Validation error: Values missing for required job parameters: Quality";

#[test]
fn errors_name_the_side() {
    let old = std::fs::read_to_string(sample_path("render_v1.yaml")).unwrap();
    let err = diff_templates(&text(&old).param("Scene", "s"), &text(&old), &opts()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Validation error: new template: Values missing for required job parameters: Scene"
    );
    let err = diff_templates(&text("name: x"), &text(&old), &opts()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Validation error: old template: Template is missing Open Job Description schema version key: specificationVersion"
    );
    let err = diff_job_template_with_parameter_sets(
        &text(&old),
        &params(&[("Scene", "s")]),
        &params(&[]),
        &opts(),
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Validation error: new parameters: Values missing for required job parameters: Scene"
    );
}

#[test]
fn extension_allowlist_applies() {
    let err = diff_templates(&text(LET_OLD).no_extensions(), &text(LET_OLD), &opts()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Validation error: old template: Model validation error: 1 validation error for JobTemplate\nextensions:\n\tUnsupported extension names: EXPR"
    );
    assert!(diff_templates(
        &text(LET_OLD).extensions(&["EXPR"]),
        &text(LET_OLD),
        &opts()
    )
    .is_ok());
}

// ─── sources, built jobs, helpers ───────────────────────────────────

#[test]
fn guess_picks_a_path_only_for_an_existing_file() {
    let p = sample_path("render_v1.yaml");
    let s = p.to_str().unwrap();
    assert!(matches!(TemplateSource::guess(s), TemplateSource::Path(x) if x == p));
    assert!(matches!(TemplateSource::guess("name: x"), TemplateSource::Text(t) if t == "name: x"));
}

#[test]
fn decoded_and_value_sources_give_the_same_diff() {
    let doc: serde_json::Value =
        serde_saphyr::from_str(&std::fs::read_to_string(sample_path("render_v1.yaml")).unwrap())
            .unwrap();
    let jt = openjd_model::decode_job_template(doc.clone(), None, &Default::default()).unwrap();
    let decoded =
        TemplateInput::new(TemplateSource::Decoded(Box::new(jt))).param("Scene", "shot.blend");
    let value = TemplateInput::new(TemplateSource::Value(doc)).param("Scene", "shot.blend");
    // A decoded template has no document to compare, so the origin comes
    // from building the `mid` job; it agrees with the shortcut.
    for side in [decoded, value] {
        let d = diff_templates(&side, &side.clone().param("Frames", "1-200"), &opts()).unwrap();
        assert_eq!(d.to_string(), CASE1);
    }
}

#[test]
fn json_text_source_is_parsed_as_json() {
    let json = r#"{"specificationVersion": "jobtemplate-2023-09", "name": "J",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "a"}}}}]}"#;
    let d = diff_templates(&text(json), &text(&json.replace("\"a\"", "\"b\"")), &opts()).unwrap();
    assert_eq!(
        lines(&d.changes),
        ["~ steps[S].script.onRun.command: a -> b  [template]"]
    );
}

#[test]
fn environment_template_parameters_are_merged() {
    let env = r#"
specificationVersion: environment-2023-09
parameterDefinitions:
  - {name: Tool, type: STRING, default: t1}
environment:
  name: Env
  variables: {TOOL: "{{Param.Tool}}"}
"#;
    let job = TemplateInput::new(sample("render_v1.yaml"))
        .param("Scene", "s")
        .environment(TemplateSource::Text(env.into()));
    let d = diff_job_template_with_parameter_sets(
        &job,
        &params(&[]),
        &params(&[("Tool", "t2")]),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        lines(&d.changes),
        ["~ parameters[Tool].value: t1 -> t2  [parameters]"]
    );
}

#[test]
fn built_jobs_round_trip_and_diff_with_job_origin() {
    let old = input("render_v1.yaml").build().unwrap();
    let new = input("render_v2.yaml")
        .param("Frames", "1-200")
        .build()
        .unwrap();
    let back = job_from_str(&serde_json::to_string(&old).unwrap()).unwrap();
    // `==` ignores task parameter definition order; the diff does not.
    assert!(!openjd_model::job::diff::diff_jobs(&back, &old, &opts()).is_changed());
    let d = diff_built_jobs(back, new, &opts());
    assert_eq!(
        d.to_string(),
        CASE3
            .replace("[parameters]", "[job]")
            .replace("[template]", "[job]")
    );
    assert!(d.changes.iter().all(|c| c.origin == Origin::Job));
}

#[test]
fn job_from_value_reads_every_parameter_type() {
    let t = r#"
specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: All
parameterDefinitions:
  - {name: S, type: STRING, default: s}
  - {name: I, type: INT, default: 3}
  - {name: F, type: FLOAT, default: "1.50"}
  - {name: P, type: PATH, default: p}
  - {name: B, type: BOOL, default: true}
  - {name: R, type: RANGE_EXPR, default: "1-3"}
  - {name: LS, type: "LIST[STRING]", default: [a, b]}
  - {name: LE, type: "LIST[STRING]", default: []}
  - {name: LI, type: "LIST[INT]", default: [1, 2]}
  - {name: LF, type: "LIST[FLOAT]", default: [1.5]}
  - {name: LP, type: "LIST[PATH]", default: [x]}
  - {name: LB, type: "LIST[BOOL]", default: [false]}
  - {name: LL, type: "LIST[LIST[INT]]", default: [[1], [2, 3]]}
steps:
  - name: Render
    script:
      actions:
        onRun: {command: echo}
"#;
    let job = text(t).build().unwrap();
    for text in [
        serde_json::to_string(&job).unwrap(),
        serde_saphyr::to_string(&job).unwrap(),
    ] {
        let back = job_from_str(&text).unwrap();
        assert_eq!(back, job);
        assert!(!openjd_model::job::diff::diff_jobs(&back, &job, &opts()).is_changed());
    }
    let err = job_from_str(r#"{"name": "x", "steps": [], "bogus": 1}"#).unwrap_err();
    assert_eq!(err.to_string(), "Validation error: not a built job: unknown field `bogus`, expected one of `name`, `description`, `extensions`, `parameters`, `steps`, `jobEnvironments` at line 1 column 34");
}

#[test]
fn helpers_find_changes_steps_and_plan() {
    let d = case3();
    assert_eq!(d.changes_at("steps[Render]").count(), 2);
    assert_eq!(d.changes_at("steps[Rend]").count(), 0);
    assert_eq!(d.changes_at("").count(), 3);
    let s = d.step("Render").unwrap();
    assert_eq!(
        (s.status, s.old_index, s.new_index),
        (DiffStatus::Modified, Some(0), Some(0))
    );
    assert!(d.step("Nope").is_none());
    assert_eq!(s.stat_line(), "~ Render  tasks 100 -> 400  (+300 -0)");
    let kinds: Vec<MutationKind> = d.plan().unwrap().kinds().into_iter().collect();
    assert_eq!(
        kinds,
        [MutationKind::ParameterValue, MutationKind::TaskAppend]
    );
}

#[test]
fn max_depth_reports_unexpanded_nodes_whole() {
    let d = diff_templates(
        &input("render_v1.yaml"),
        &input("render_v2.yaml").param("Frames", "1-200"),
        &DiffOptions::default().with_max_depth(1),
    )
    .unwrap();
    // The step itself is not expanded; its tasks come from the jobs, not
    // the tree, so they still show.
    assert_eq!(
        d.to_string(),
        "\
~ parameter Frames: 1-100 -> 1-200  [parameters]
~ step Render
    ~ (not expanded)  [both]
    tasks: 100 -> 400
      keep   100   Frame 1-100 × Eye left
      add    200   Frame 101-200 × Eye left, right
      add    100   Frame 1-100 × Eye right

1 step changed. Tasks: 100 kept, 300 added, 0 removed."
    );
    let fields: Vec<(&str, &str, Origin)> = d
        .changes
        .iter()
        .map(|c| (c.path.as_str(), c.field.as_str(), c.origin))
        .collect();
    assert_eq!(
        fields,
        [
            ("parameters[Frames]", "", Origin::Parameters),
            ("steps[Render]", "", Origin::Both)
        ]
    );
}

#[test]
fn integer_boxes_render_as_range_expressions_and_long_text_is_cut() {
    let base = r#"
specificationVersion: jobtemplate-2023-09
name: R
steps:
  - name: S
    parameterSpace:
      taskParameterDefinitions:
        - {name: N, type: INT, range: [1, 3, 5, 7]}
    script:
      actions:
        onRun: {command: echo, args: ["ARG"]}
"#;
    let long = "x".repeat(200);
    let new = base
        .replace("[1, 3, 5, 7]", "[1, 3, 5, 7, 8, 20]")
        .replace("ARG", &long);
    let d = diff_templates(&text(base), &text(&new), &opts()).unwrap();
    let args = d.changes.iter().find(|c| c.field == "args").unwrap();
    let after = args.after.as_ref().unwrap();
    assert_eq!(after.raw, serde_json::json!([long]));
    assert_eq!(after.text.chars().count(), 120);
    assert!(after.text.ends_with('…'));
    assert_eq!(
        d.step("S").unwrap().tasks.to_string(),
        "tasks: 4 -> 6\n  keep   4     N 1-7:2\n  add    2     N 8,20"
    );
}

#[test]
fn added_and_removed_steps() {
    let one = r#"
specificationVersion: jobtemplate-2023-09
name: J
steps:
  - name: A
    script: {actions: {onRun: {command: a}}}
"#;
    let two = r#"
specificationVersion: jobtemplate-2023-09
name: J
steps:
  - name: B
    parameterSpace:
      taskParameterDefinitions:
        - {name: N, type: INT, range: "1-3"}
    script: {actions: {onRun: {command: b}}}
"#;
    let d = diff_templates(&text(one), &text(two), &opts()).unwrap();
    assert_eq!(
        d.to_string(),
        "\
- step A
    tasks: 1 removed
+ step B
    tasks: 3 added

0 steps changed, 1 added, 1 removed. Tasks: 0 kept, 3 added, 1 removed."
    );
    let stat: Vec<String> = d.steps.iter().map(StepSummary::stat_line).collect();
    assert_eq!(
        stat,
        ["- A  tasks 1 -> 0  (+0 -1)", "+ B  tasks 0 -> 3  (+3 -0)"]
    );
    assert_eq!(d.steps[1].to_string(), "+ step B\n    tasks: 3 added");
}

// ─── audit round 1 regressions ──────────────────────────────────────

const ENV_JOB: &str = r#"
specificationVersion: jobtemplate-2023-09
name: J
jobEnvironments:
  - name: E
    script:
      actions:
        onEnter: {command: hi}
steps:
  - name: S
    script:
      actions:
        onRun: {command: a}
"#;

#[test]
fn whole_action_added_or_removed_carries_its_side() {
    let new = ENV_JOB.replace(
        "onEnter: {command: hi}",
        "onEnter: {command: hi}\n        onExit: {command: bye}",
    );
    let d = diff_templates(&text(ENV_JOB), &text(&new), &opts()).unwrap();
    assert_eq!(
        lines(&d.changes),
        ["+ jobEnvironments[E].script.onExit: {command: bye}  [template]"]
    );
    let d = diff_templates(&text(&new), &text(ENV_JOB), &opts()).unwrap();
    assert_eq!(
        lines(&d.changes),
        ["- jobEnvironments[E].script.onExit: {command: bye}  [template]"]
    );
    // `onRun` cut off by the depth limit is reported whole, with both sides.
    let d = diff_templates(
        &text(ENV_JOB),
        &text(&ENV_JOB.replace("command: a", "command: b")),
        &DiffOptions::default().with_max_depth(3),
    )
    .unwrap();
    assert_eq!(
        lines(&d.changes),
        ["~ steps[S].script.onRun: {command: a} -> {command: b}  [template]"]
    );
}

const COUNT_OVERFLOW: &str = r#"
specificationVersion: jobtemplate-2023-09
name: Big
steps:
  - name: A
    parameterSpace:
      taskParameterDefinitions:
        - {name: F, type: INT, range: "1-4000000000"}
        - {name: G, type: INT, range: "1-4000000000"}
    script: {actions: {onRun: {command: a}}}
  - name: B
    parameterSpace:
      taskParameterDefinitions:
        - {name: F, type: INT, range: "1-4000000000"}
        - {name: G, type: INT, range: "1-4000000000"}
    script: {actions: {onRun: {command: a}}}
"#;

#[test]
fn task_counts_saturate_instead_of_overflowing() {
    let d = diff_templates(
        &text(COUNT_OVERFLOW),
        &text(&COUNT_OVERFLOW.replace("command: a", "command: b")),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        d.stat(),
        DiffStat {
            steps_changed: 2,
            tasks_kept: u64::MAX,
            ..DiffStat::default()
        }
    );
    assert_eq!(
        d.stat().to_string(),
        "2 steps changed. Tasks: 18446744073709551615 kept, 0 added, 0 removed."
    );
}

const ADAPTIVE: &str = r#"
specificationVersion: jobtemplate-2023-09
extensions: [TASK_CHUNKING]
name: C
steps:
  - name: Chunked
    parameterSpace:
      taskParameterDefinitions:
        - name: F
          type: CHUNK[INT]
          range: "1-100"
          chunks: {defaultTaskCount: 10, targetRuntimeSeconds: 60, rangeConstraint: CONTIGUOUS}
    script: {actions: {onRun: {command: old, args: ["{{Task.Param.F}}"]}}}
"#;

#[test]
fn adaptive_chunking_task_count_is_unknown_with_a_note() {
    let d = diff_templates(
        &text(ADAPTIVE),
        &text(&ADAPTIVE.replace("command: old", "command: new")),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        d.to_string(),
        "\
~ step Chunked
    ~ script.onRun.command: old -> new  [template]
    tasks: unknown (adaptive chunking)

1 step changed. Tasks: 0 kept, 0 added, 0 removed.
note: steps[Chunked]: cannot count tasks: the chunk size is adaptive (targetRuntimeSeconds), so the number of chunks is decided at run time"
    );
    assert_eq!(
        d.notes,
        [Note {
            step: Some("Chunked".to_string()),
            text: "cannot count tasks: the chunk size is adaptive (targetRuntimeSeconds), so the number of chunks is decided at run time".to_string()
        }]
    );
    let s = d.step("Chunked").unwrap();
    assert_eq!(s.tasks, TaskSummary::Unchanged { count: 0 });
    assert_eq!(
        s.stat_line(),
        "~ Chunked  tasks unknown (adaptive chunking)"
    );
}

fn env_vars_job(n: usize) -> String {
    let vars: Vec<String> = (0..n).map(|i| format!("V{i}: v{i}")).collect();
    format!(
        r#"
specificationVersion: jobtemplate-2023-09
name: J
jobEnvironments:
  - name: E
    variables: {{{}}}
steps:
  - name: S
    script: {{actions: {{onRun: {{command: a}}}}}}
"#,
        vars.join(", ")
    )
}

#[test]
fn serialized_diff_is_deterministic_with_many_environment_variables() {
    let old = env_vars_job(10);
    let new = old.replace("command: a", "command: b");
    let runs: Vec<String> = (0..20)
        .map(|_| {
            let d = diff_templates(&text(&old), &text(&new), &opts()).unwrap();
            serde_json::to_string(&d).unwrap()
        })
        .collect();
    assert!(runs.iter().all(|r| r == &runs[0]));
    // The variables are written sorted by name.
    let v: serde_json::Value = serde_json::from_str(&runs[0]).unwrap();
    let names: Vec<&String> = v["oldJob"]["jobEnvironments"][0]["variables"]
        .as_object()
        .unwrap()
        .keys()
        .collect();
    assert_eq!(
        names,
        ["V0", "V1", "V2", "V3", "V4", "V5", "V6", "V7", "V8", "V9"]
    );
}

#[test]

fn job_document_is_deterministic_and_reads_back() {
    let old = env_vars_job(10);
    let job = text(&old).build().unwrap();
    let docs: Vec<String> = (0..20)
        .map(|_| serde_json::to_string(&JobDocument(&text(&old).build().unwrap())).unwrap())
        .collect();
    assert!(docs.iter().all(|r| r == &docs[0]));
    assert_eq!(job_from_str(&docs[0]).unwrap(), job);
}

#[test]
fn long_values_that_truncate_alike_show_where_they_differ() {
    let base = format!("{}a", "x".repeat(200));
    let t = |arg: &str| {
        format!(
            "specificationVersion: jobtemplate-2023-09\nname: J\nsteps:\n  - name: S\n    script: {{actions: {{onRun: {{command: c, args: [\"{arg}\"]}}}}}}\n"
        )
    };
    let d = diff_templates(
        &text(&t(&base)),
        &text(&t(&base.replace('a', "b"))),
        &opts(),
    )
    .unwrap();
    let c = &d.changes[0];
    let (b, a) = (c.before.as_ref().unwrap(), c.after.as_ref().unwrap());
    assert_ne!(b.text, a.text);
    // A cut list keeps its first 20 characters, so it still reads as one.
    let (head, tail) = ("x".repeat(19), "x".repeat(40));
    assert_eq!(b.text, format!("[{head}… …{tail}a]"));
    assert_eq!(a.text, format!("[{head}… …{tail}b]"));
}

#[test]
fn unexpanded_step_change_is_shown_under_the_step() {
    let d = diff_templates(
        &input("render_v1.yaml"),
        &input("render_v2.yaml"),
        &DiffOptions::default().with_max_depth(1),
    )
    .unwrap();
    assert_eq!(
        d.to_string(),
        "\
~ step Render
    ~ (not expanded)  [template]
    tasks: 100 -> 200
      keep   100   Frame 1-100 × Eye left
      add    100   Frame 1-100 × Eye right

1 step changed. Tasks: 100 kept, 100 added, 0 removed."
    );
}

/// A job whose one step's `onRun` args are `args`, with an EXPR script
/// `let` and a step environment variable `V`.
fn stage_job(args: &str, script_let: &str, var: &str) -> String {
    format!(
        r#"
specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: J
parameterDefinitions:
  - {{name: Scene, type: STRING, default: s}}
  - {{name: P, type: STRING, default: p}}
steps:
  - name: S
    parameterSpace:
      taskParameterDefinitions:
        - {{name: Frame, type: INT, range: "1-2"}}
    stepEnvironments:
      - name: E
        variables: {{V: "{var}"}}
    script:
      let: ["{script_let}"]
      actions:
        onRun: {{command: c, args: ["{args}"]}}
"#
    )
}

/// `(args, script let, variable V)` of one side.
type Fields<'a> = (&'a str, &'a str, &'a str);

#[test]
fn stage_is_run_time_only_for_task_or_session_references_in_format_strings() {
    let (a, l, v) = ("{{Param.Scene}} a", "f = 1", "x");
    let cases: &[(Fields, Fields, &str, Stage)] = &[
        // a command change with no run-time reference
        ((a, l, v), ("{{Param.Scene}} b", l, v), "args", Stage::Job),
        // `Task.` outside `{{ }}` is literal text
        (
            ("{{Param.P}} Task.log a", l, v),
            ("{{Param.P}} Task.log b", l, v),
            "args",
            Stage::Job,
        ),
        (
            ("Session.x a", l, v),
            ("Session.x b", l, v),
            "args",
            Stage::Job,
        ),
        // a run-time reference on the new side only, and on the old side only
        (
            (a, l, v),
            ("{{Session.WorkingDirectory}}", l, v),
            "args",
            Stage::RunTime,
        ),
        (
            ("{{Session.WorkingDirectory}}", l, v),
            (a, l, v),
            "args",
            Stage::RunTime,
        ),
        (
            ("{{Task.Param.Frame}} a", l, v),
            ("{{Task.Param.Frame}} b", l, v),
            "args",
            Stage::RunTime,
        ),
        // a script `let` references symbols without braces
        (
            (a, "f = Task.Param.Frame", v),
            (a, "f = Task.Param.Frame + 1", v),
            "let",
            Stage::RunTime,
        ),
        ((a, "f = 1", v), (a, "f = 2", v), "let", Stage::Job),
        // an environment variable
        (
            (a, l, "{{Session.WorkingDirectory}}/a"),
            (a, l, "{{Session.WorkingDirectory}}/b"),
            "variables.V",
            Stage::RunTime,
        ),
        ((a, l, "x"), (a, l, "y"), "variables.V", Stage::Job),
    ];
    for ((oa, ol, ov), (na, nl, nv), field, stage) in cases {
        let d = diff_templates(
            &text(&stage_job(oa, ol, ov)),
            &text(&stage_job(na, nl, nv)),
            &opts(),
        )
        .unwrap();
        let c = d
            .changes
            .iter()
            .find(|c| c.layer == Layer::Job && c.field == *field)
            .unwrap_or_else(|| panic!("no {field} change: {d}"));
        assert_eq!(c.stage, *stage, "{c}");
    }
}

#[test]
fn errors_name_the_parameter_set_in_a_parameter_set_diff() {
    let old = std::fs::read_to_string(sample_path("render_v1.yaml")).unwrap();
    let err = diff_job_template_with_parameter_sets(
        &text(&old),
        &params(&[]),
        &params(&[("Scene", "s")]),
        &opts(),
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Validation error: old parameters: Values missing for required job parameters: Scene"
    );
    let err = diff_job_template_with_parameter_sets(
        &text(&old),
        &params(&[("Scene", "s")]),
        &params(&[]),
        &opts(),
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Validation error: new parameters: Values missing for required job parameters: Scene"
    );
    let err = diff_job_template_with_parameter_sets(
        &text("name: x"),
        &params(&[]),
        &params(&[]),
        &opts(),
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Validation error: the template: Template is missing Open Job Description schema version key: specificationVersion"
    );
}

/// Every field the template layer records, for every parameter type that
/// has it, changed one at a time.
#[test]
fn every_parameter_definition_field_is_compared() {
    let cases: &[(&str, &str, &str, &str)] = &[
        ("STRING", "description: a", "description: b", "description"),
        ("STRING", "default: a", "default: b", "default"),
        ("STRING", "allowedValues: [a]", "allowedValues: [a, b]", "allowedValues"),
        ("STRING", "minLength: 0", "minLength: 1", "minLength"),
        ("STRING", "maxLength: 5", "maxLength: 6", "maxLength"),
        ("STRING", "userInterface: {control: LINE_EDIT}", "userInterface: {control: MULTILINE_EDIT}", "userInterface.control"),
        ("STRING", "userInterface: {label: a}", "userInterface: {label: b}", "userInterface.label"),
        ("STRING", "userInterface: {groupLabel: a}", "userInterface: {groupLabel: b}", "userInterface.groupLabel"),
        ("PATH", "default: a", "default: b", "default"),
        ("PATH", "allowedValues: ['ABS']", "allowedValues: ['ABS', b]", "allowedValues"),
        ("PATH", "minLength: 0", "minLength: 1", "minLength"),
        ("PATH", "maxLength: 100", "maxLength: 101", "maxLength"),
        ("PATH", "objectType: FILE", "objectType: DIRECTORY", "objectType"),
        ("PATH", "dataFlow: IN", "dataFlow: OUT", "dataFlow"),
        ("PATH", "objectType: FILE, userInterface: {control: CHOOSE_INPUT_FILE}", "objectType: FILE, userInterface: {control: CHOOSE_OUTPUT_FILE}", "userInterface.control"),
        ("PATH", "objectType: FILE, userInterface: {control: CHOOSE_INPUT_FILE, fileFilters: [{label: a, patterns: ['*']}]}", "objectType: FILE, userInterface: {control: CHOOSE_INPUT_FILE, fileFilters: [{label: b, patterns: ['*']}]}", "userInterface.fileFilters"),
        ("PATH", "objectType: FILE, userInterface: {control: CHOOSE_INPUT_FILE, fileFilterDefault: {label: a, patterns: ['*']}}", "objectType: FILE, userInterface: {control: CHOOSE_INPUT_FILE, fileFilterDefault: {label: b, patterns: ['*']}}", "userInterface.fileFilterDefault"),
        ("INT", "default: 1", "default: 2", "default"),
        ("INT", "allowedValues: [1]", "allowedValues: [1, 2]", "allowedValues"),
        ("INT", "minValue: 1", "minValue: 0", "minValue"),
        ("INT", "maxValue: 5", "maxValue: 6", "maxValue"),
        ("INT", "userInterface: {control: SPIN_BOX, singleStepDelta: 1}", "userInterface: {control: SPIN_BOX, singleStepDelta: 2}", "userInterface.singleStepDelta"),
        ("INT", "userInterface: {control: SPIN_BOX}", "userInterface: {control: HIDDEN}", "userInterface.control"),
        ("FLOAT", "default: 1.5", "default: 2.5", "default"),
        ("FLOAT", "allowedValues: [1.5]", "allowedValues: [1.5, 2]", "allowedValues"),
        ("FLOAT", "minValue: 1", "minValue: 0", "minValue"),
        ("FLOAT", "maxValue: 5", "maxValue: 6", "maxValue"),
        ("FLOAT", "userInterface: {control: SPIN_BOX, decimals: 1}", "userInterface: {control: SPIN_BOX, decimals: 2}", "userInterface.decimals"),
        ("FLOAT", "userInterface: {control: SPIN_BOX, singleStepDelta: 0.5}", "userInterface: {control: SPIN_BOX, singleStepDelta: 1.5}", "userInterface.singleStepDelta"),
        ("BOOL", "default: true", "default: false", "default"),
        ("BOOL", "userInterface: {control: CHECK_BOX}", "userInterface: {control: HIDDEN}", "userInterface.control"),
        ("RANGE_EXPR", "default: 1-2", "default: 1-3", "default"),
        ("RANGE_EXPR", "minLength: 1", "minLength: 2", "minLength"),
        ("RANGE_EXPR", "maxLength: 5", "maxLength: 6", "maxLength"),
        ("RANGE_EXPR", "userInterface: {control: LINE_EDIT}", "userInterface: {control: HIDDEN}", "userInterface.control"),
        ("LIST[STRING]", "default: [a]", "default: [b]", "default"),
        ("LIST[STRING]", "minLength: 1", "minLength: 0", "minLength"),
        ("LIST[STRING]", "maxLength: 5", "maxLength: 6", "maxLength"),
        ("LIST[STRING]", "item: {allowedValues: [a]}", "item: {allowedValues: [a, b]}", "item.allowedValues"),
        ("LIST[STRING]", "item: {minLength: 0}", "item: {minLength: 1}", "item.minLength"),
        ("LIST[STRING]", "item: {maxLength: 5}", "item: {maxLength: 6}", "item.maxLength"),
        ("LIST[STRING]", "userInterface: {control: LINE_EDIT_LIST}", "userInterface: {control: HIDDEN}", "userInterface.control"),
        ("LIST[PATH]", "default: [a]", "default: [b]", "default"),
        ("LIST[PATH]", "minLength: 1", "minLength: 0", "minLength"),
        ("LIST[PATH]", "maxLength: 5", "maxLength: 6", "maxLength"),
        ("LIST[PATH]", "objectType: FILE", "objectType: DIRECTORY", "objectType"),
        ("LIST[PATH]", "dataFlow: IN", "dataFlow: OUT", "dataFlow"),
        ("LIST[PATH]", "item: {allowedValues: ['ABS']}", "item: {allowedValues: ['ABS', b]}", "item.allowedValues"),
        ("LIST[PATH]", "item: {minLength: 1}", "item: {minLength: 2}", "item.minLength"),
        ("LIST[PATH]", "item: {maxLength: 100}", "item: {maxLength: 101}", "item.maxLength"),
        ("LIST[PATH]", "objectType: FILE, userInterface: {control: CHOOSE_INPUT_FILE_LIST}", "objectType: FILE, userInterface: {control: HIDDEN}", "userInterface.control"),
        ("LIST[PATH]", "objectType: FILE, userInterface: {control: CHOOSE_INPUT_FILE_LIST, fileFilters: [{label: a, patterns: ['*']}]}", "objectType: FILE, userInterface: {control: CHOOSE_INPUT_FILE_LIST, fileFilters: [{label: b, patterns: ['*']}]}", "userInterface.fileFilters"),
        ("LIST[PATH]", "objectType: FILE, userInterface: {control: CHOOSE_INPUT_FILE_LIST, fileFilterDefault: {label: a, patterns: ['*']}}", "objectType: FILE, userInterface: {control: CHOOSE_INPUT_FILE_LIST, fileFilterDefault: {label: b, patterns: ['*']}}", "userInterface.fileFilterDefault"),
        ("LIST[INT]", "default: [1]", "default: [2]", "default"),
        ("LIST[INT]", "minLength: 1", "minLength: 0", "minLength"),
        ("LIST[INT]", "maxLength: 5", "maxLength: 6", "maxLength"),
        ("LIST[INT]", "item: {allowedValues: [1, 2]}", "item: {allowedValues: [1, 2, 3]}", "item.allowedValues"),
        ("LIST[INT]", "item: {minValue: 0}", "item: {minValue: -1}", "item.minValue"),
        ("LIST[INT]", "item: {maxValue: 5}", "item: {maxValue: 6}", "item.maxValue"),
        ("LIST[INT]", "userInterface: {control: SPIN_BOX_LIST, singleStepDelta: 1}", "userInterface: {control: SPIN_BOX_LIST, singleStepDelta: 2}", "userInterface.singleStepDelta"),
        ("LIST[FLOAT]", "default: [1.5]", "default: [2.5]", "default"),
        ("LIST[FLOAT]", "minLength: 1", "minLength: 0", "minLength"),
        ("LIST[FLOAT]", "maxLength: 5", "maxLength: 6", "maxLength"),
        ("LIST[FLOAT]", "item: {allowedValues: [1.5, 2]}", "item: {allowedValues: [1.5, 2, 3]}", "item.allowedValues"),
        ("LIST[FLOAT]", "item: {minValue: 0}", "item: {minValue: -1}", "item.minValue"),
        ("LIST[FLOAT]", "item: {maxValue: 5}", "item: {maxValue: 6}", "item.maxValue"),
        ("LIST[FLOAT]", "userInterface: {control: SPIN_BOX_LIST, decimals: 1}", "userInterface: {control: SPIN_BOX_LIST, decimals: 2}", "userInterface.decimals"),
        ("LIST[FLOAT]", "userInterface: {control: SPIN_BOX_LIST, singleStepDelta: 0.5}", "userInterface: {control: SPIN_BOX_LIST, singleStepDelta: 1.5}", "userInterface.singleStepDelta"),
        ("LIST[BOOL]", "default: [true]", "default: [false]", "default"),
        ("LIST[BOOL]", "minLength: 1", "minLength: 0", "minLength"),
        ("LIST[BOOL]", "maxLength: 5", "maxLength: 6", "maxLength"),
        ("LIST[BOOL]", "userInterface: {control: CHECK_BOX_LIST}", "userInterface: {control: HIDDEN}", "userInterface.control"),
        ("LIST[LIST[INT]]", "default: [[1]]", "default: [[2]]", "default"),
        ("LIST[LIST[INT]]", "minLength: 1", "minLength: 0", "minLength"),
        ("LIST[LIST[INT]]", "maxLength: 5", "maxLength: 6", "maxLength"),
        ("LIST[LIST[INT]]", "item: {minLength: 1}", "item: {minLength: 0}", "item.minLength"),
        ("LIST[LIST[INT]]", "item: {maxLength: 5}", "item: {maxLength: 6}", "item.maxLength"),
        ("LIST[LIST[INT]]", "item: {item: {allowedValues: [1, 2]}}", "item: {item: {allowedValues: [1, 2, 3]}}", "item.item.allowedValues"),
        ("LIST[LIST[INT]]", "item: {item: {minValue: 0}}", "item: {item: {minValue: -1}}", "item.item.minValue"),
        ("LIST[LIST[INT]]", "item: {item: {maxValue: 5}}", "item: {item: {maxValue: 6}}", "item.item.maxValue"),
        ("LIST[LIST[INT]]", "userInterface: {label: a}", "userInterface: {control: HIDDEN, label: a}", "userInterface.control"),
    ];
    let t = |ty: &str, extra: &str| {
        format!(
            "specificationVersion: jobtemplate-2023-09\nextensions: [EXPR]\nname: J\nparameterDefinitions:\n  - {{name: X, type: \"{ty}\", {extra}}}\nsteps:\n  - name: S\n    script: {{actions: {{onRun: {{command: c}}}}}}\n"
        )
    };
    let mut failures = Vec::new();
    for (ty, old, new, field) in cases {
        let (a, b) = (
            t(ty, &old.replace("ABS", ABS)),
            t(ty, &new.replace("ABS", ABS)),
        );
        let side = |s: &str| text(s).param("X", default_for(ty));
        match diff_templates(&side(&a), &side(&b), &opts()) {
            Ok(d) => {
                let fields: Vec<&str> = d
                    .changes_at("parameterDefinitions[X]")
                    .map(|c| c.field.as_str())
                    .collect();
                if fields != [*field] {
                    failures.push(format!("{ty} {old} -> {new}: {fields:?}"));
                }
            }
            Err(e) => failures.push(format!("{ty} {old} -> {new}: {e}")),
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// An absolute PATH value on this platform.
const ABS: &str = if cfg!(windows) { r"C:\a" } else { "/a" };

/// A value of `ty` that satisfies every constraint in the table above.
fn default_for(ty: &str) -> String {
    match ty {
        "STRING" => "a".into(),
        "PATH" => ABS.into(),
        "INT" => "1".into(),
        "FLOAT" => "1.5".into(),
        "BOOL" => "true".into(),
        "RANGE_EXPR" => "1-2".into(),
        "LIST[STRING]" => r#"["a"]"#.into(),
        "LIST[PATH]" => serde_json::json!([ABS]).to_string(),
        "LIST[INT]" => "[1]".into(),
        "LIST[FLOAT]" => "[1.5]".into(),
        "LIST[BOOL]" => "[true]".into(),
        _ => "[[1]]".into(),
    }
}

const TASKS_6: &str = r#"
specificationVersion: jobtemplate-2023-09
name: T
steps:
  - name: Render
    parameterSpace:
      taskParameterDefinitions:
        - {name: N, type: INT, range: "1-6"}
    script: {actions: {onRun: {command: c}}}
"#;

#[test]
fn removed_tasks_are_counted_and_listed() {
    let d = diff_templates(
        &text(TASKS_6),
        &text(&TASKS_6.replace("1-6", "1-3")),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        d.to_string(),
        "\
~ step Render
    ~ N: 1-6 -> 1-3  [template]
    tasks: 6 -> 3
      keep   3     N 1-3
      remove 3     N 4-6

1 step changed. Tasks: 3 kept, 0 added, 3 removed."
    );
    assert_eq!(d.steps[0].stat_line(), "~ Render  tasks 6 -> 3  (+0 -3)");
}

#[test]
fn added_and_removed_boxes_both_show_in_the_stat_line() {
    let old = TASKS_6.replace("\"1-6\"", "[1, 2, 3]");
    let new = TASKS_6.replace("\"1-6\"", "[1, 2, 4]");
    let d = diff_templates(&text(&old), &text(&new), &opts()).unwrap();
    assert_eq!(d.steps[0].stat_line(), "~ Render  tasks 3 -> 3  (+1 -1)");
    assert_eq!(
        d.steps[0].tasks.to_string(),
        "tasks: 3 -> 3\n  keep   2     N 1,2\n  add    1     N 4\n  remove 1     N 3"
    );
}

#[test]
fn replaced_tasks_stat_line_and_totals() {
    let new = TASKS_6.replace(
        "        - {name: N, type: INT, range: \"1-6\"}",
        "        - {name: N, type: INT, range: \"1-6\"}\n        - {name: M, type: INT, range: \"1-2\"}",
    );
    let d = diff_templates(&text(TASKS_6), &text(&new), &opts()).unwrap();
    assert_eq!(
        d.steps[0].stat_line(),
        "~ Render  tasks 6 -> 12  (replaced)"
    );
    assert_eq!(
        d.stat().to_string(),
        "1 step changed. Tasks: 0 kept, 0 added, 0 removed, 12 replaced."
    );
}

#[test]
fn integer_lists_render_as_runs_only_with_three_values() {
    let d = |old: &str, new: &str| {
        diff_templates(
            &text(&TASKS_6.replace("\"1-6\"", old)),
            &text(&TASKS_6.replace("\"1-6\"", new)),
            &opts(),
        )
        .unwrap()
    };
    assert_eq!(
        d("[1, 3]", "[1, 3, 5]").steps[0].tasks.to_string(),
        "tasks: 2 -> 3\n  keep   2     N 1,3\n  add    1     N 5"
    );
    assert_eq!(
        d("[1, 3, 5]", "[1, 3, 5, 9]").steps[0].tasks.to_string(),
        "tasks: 3 -> 4\n  keep   3     N 1-5:2\n  add    1     N 9"
    );
    assert_eq!(
        d("[2]", "[1, 5, 7]").steps[0].tasks.to_string(),
        "tasks: 1 -> 3\n  add    3     N 1,5,7\n  remove 1     N 2"
    );
}

#[test]
fn job_from_str_round_trips_empty_lists() {
    let t = r#"
specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: Empty
parameterDefinitions:
  - {name: LS, type: "LIST[STRING]", default: []}
  - {name: LI, type: "LIST[INT]", default: []}
  - {name: LF, type: "LIST[FLOAT]", default: []}
  - {name: LP, type: "LIST[PATH]", default: []}
  - {name: LB, type: "LIST[BOOL]", default: []}
  - {name: LL, type: "LIST[LIST[INT]]", default: []}
  - {name: LLE, type: "LIST[LIST[INT]]", default: [[]]}
steps:
  - name: Render
    script:
      actions:
        onRun: {command: echo}
"#;
    let job = text(t).build().unwrap();
    for text in [
        serde_json::to_string(&job).unwrap(),
        serde_saphyr::to_string(&job).unwrap(),
    ] {
        let back = job_from_str(&text).unwrap();
        assert_eq!(back, job);
        for (k, p) in &job.parameters {
            assert_eq!(
                back.parameters[k].value.expr_type(),
                p.value.expr_type(),
                "{k}"
            );
        }
    }
}

#[test]
fn parameter_order_is_deterministic_with_many_parameters() {
    let defs: Vec<String> = (0..10)
        .map(|i| format!("  - {{name: P{i}, type: STRING, default: a}}"))
        .collect();
    let t = format!(
        "specificationVersion: jobtemplate-2023-09\nname: J\nparameterDefinitions:\n{}\nsteps:\n  - name: S\n    script: {{actions: {{onRun: {{command: c}}}}}}\n",
        defs.join("\n")
    );
    let all_b: Vec<(String, String)> = (0..10).map(|i| (format!("P{i}"), "b".into())).collect();
    let run = || {
        let d = diff_job_template_with_parameter_sets(
            &text(&t),
            &params(&[]),
            &params(
                &all_b
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect::<Vec<_>>(),
            ),
            &opts(),
        )
        .unwrap();
        (
            d.to_string(),
            d.job.to_string(),
            serde_json::to_string(&d).unwrap(),
        )
    };
    let first = run();
    assert!(first.0.starts_with(
        "~ parameter P0: a -> b  [parameters]\n~ parameter P1: a -> b  [parameters]\n"
    ));
    for _ in 0..20 {
        assert_eq!(run(), first);
    }
}

#[test]
fn layer_and_stage_display_names() {
    assert_eq!(Layer::Template.to_string(), "template");
    assert_eq!(Layer::Job.to_string(), "job");
    let stages = [
        Stage::Template,
        Stage::Parameters,
        Stage::Job,
        Stage::RunTime,
    ];
    let names: Vec<String> = stages.iter().map(ToString::to_string).collect();
    assert_eq!(names, ["template", "parameters", "job", "run time"]);
    let origins = [
        Origin::Template,
        Origin::Parameters,
        Origin::Both,
        Origin::Job,
    ];
    let names: Vec<String> = origins.iter().map(ToString::to_string).collect();
    assert_eq!(names, ["template", "parameters", "both", "job"]);
}

#[test]
fn template_input_parameters_are_the_values_set() {
    let i = TemplateInput::new(sample("render_v1.yaml"))
        .param("Scene", "a")
        .params([("Frames".to_string(), ExprValue::String("1-2".into()))])
        .param("Scene", "b");
    assert_eq!(
        i.parameters(),
        &params(&[("Scene", "b"), ("Frames", "1-2")])
    );
}

const KINDS_OLD: &str = r#"
specificationVersion: jobtemplate-2023-09
name: K
parameterDefinitions:
  - {name: Gone, type: STRING, default: g}
jobEnvironments:
  - name: Kept
    script:
      actions:
        onEnter: {command: a}
  - name: GoneEnv
    variables: {A: a}
steps:
  - name: S
    stepEnvironments:
      - name: StepEnv
        variables: {A: a}
    parameterSpace:
      taskParameterDefinitions:
        - {name: N, type: INT, range: "1-2"}
    hostRequirements:
      amounts: [{name: amount.worker.vcpu, min: 1}]
    script:
      actions:
        onRun: {command: a}
      embeddedFiles:
        - {name: F, type: TEXT, data: x}
  - name: GoneStep
    script: {actions: {onRun: {command: a}}}
"#;

const KINDS_NEW: &str = r#"
specificationVersion: jobtemplate-2023-09
name: K
parameterDefinitions:
  - {name: New, type: STRING, default: n}
jobEnvironments:
  - name: Kept
    script:
      actions:
        onEnter: {command: b}
        onExit: {command: bye}
  - name: NewEnv
    script:
      actions:
        onEnter: {command: a}
steps:
  - name: S
    parameterSpace:
      taskParameterDefinitions:
        - {name: N, type: INT, range: "1-2"}
        - {name: M, type: STRING, range: [x]}
    script:
      actions:
        onRun: {command: b}
      embeddedFiles:
        - {name: G, type: TEXT, data: y}
  - name: NewStep
    script: {actions: {onRun: {command: a}}}
"#;

/// For every node kind and every depth, a whole-node change carries the
/// sides that exist: an added node its new value, a removed node its old
/// value, an unexpanded node both.
#[test]
fn every_whole_node_change_carries_the_sides_that_exist() {
    let mut seen = std::collections::BTreeSet::new();
    for depth in [None, Some(1), Some(2), Some(3), Some(4), Some(5)] {
        let mut o = opts();
        if let Some(n) = depth {
            o = o.with_max_depth(n);
        }
        let d = diff_templates(&text(KINDS_OLD), &text(KINDS_NEW), &o).unwrap();
        for c in d
            .changes
            .iter()
            .filter(|c| c.layer == Layer::Job && c.field.is_empty())
        {
            let id = d.job.find(&c.path).unwrap();
            let node = d.job.node(id).unwrap();
            seen.insert(format!("{:?} {:?}", node.kind, node.status));
            let (b, a) = (c.before.is_some(), c.after.is_some());
            let want = match node.status {
                DiffStatus::Added => (false, true),
                DiffStatus::Removed => (true, false),
                _ => (true, true),
            };
            assert_eq!((b, a), want, "depth {depth:?}: {c}");
        }
    }
    let seen: Vec<String> = seen.into_iter().collect();
    assert_eq!(
        seen,
        [
            "Action Added",
            "Action Modified",
            "EmbeddedFile Added",
            "EmbeddedFile Removed",
            "Environment Added",
            "Environment Modified",
            "Environment Removed",
            "EnvironmentScript Modified",
            "HostRequirements Removed",
            "JobParameter Added",
            "JobParameter Removed",
            "ParameterSpace Modified",
            "Step Added",
            "Step Modified",
            "Step Removed",
            "StepScript Modified",
            "TaskParameter Added",
        ]
    );
}

#[test]
fn job_document_matches_the_job_serde_form_up_to_map_order() {
    let job = input("render_v2.yaml")
        .environment(TemplateSource::Text(
            "specificationVersion: environment-2023-09\nenvironment:\n  name: Env\n  variables: {A: a}\n  script: {actions: {onEnter: {command: x}}}\n".into(),
        ))
        .build()
        .unwrap();
    let mut job = job;
    // A step environment, so every DocEnvironment path is written.
    job.steps[0].step_environments = job.job_environments.clone();
    // The document writes parameters sorted; one variable has one order.
    job.parameters.sort_keys();
    assert_eq!(
        serde_json::to_string(&JobDocument(&job)).unwrap(),
        serde_json::to_string(&job).unwrap()
    );
    assert_eq!(
        serde_saphyr::to_string(&JobDocument(&job)).unwrap(),
        serde_saphyr::to_string(&job).unwrap()
    );
}

#[test]
fn product_counts_saturate() {
    let b = |count| BoxSummary {
        count,
        dims: vec![],
    };
    let s = StepSummary {
        name: "S".into(),
        status: DiffStatus::Modified,
        old_index: Some(0),
        new_index: Some(0),
        tasks: TaskSummary::Product {
            kept: b(u64::MAX),
            added: vec![b(1), b(u64::MAX)],
            removed: vec![b(2)],
        },
        tasks_unknown: None,
    };
    assert_eq!(
        s.stat_line(),
        "~ S  tasks 18446744073709551615 -> 18446744073709551615  (+18446744073709551615 -2)"
    );
}

// ─── audit round 2 ──────────────────────────────────────────────────

/// A template with 10 job parameters and 8 job environment variables.
fn many_params_job() -> String {
    let defs: Vec<String> = (0..10)
        .map(|i| format!("  - {{name: P{i}, type: STRING, default: v{i}}}"))
        .collect();
    let vars: Vec<String> = (0..8).map(|i| format!("V{i}: x{i}")).collect();
    format!(
        "specificationVersion: jobtemplate-2023-09\nname: J\nparameterDefinitions:\n{}\njobEnvironments:\n  - name: E\n    variables: {{{}}}\nsteps:\n  - name: S\n    script: {{actions: {{onRun: {{command: c}}}}}}\n",
        defs.join("\n"),
        vars.join(", ")
    )
}

#[test]
fn job_document_sorts_parameters_so_every_build_serializes_the_same() {
    let t = many_params_job();
    let json: std::collections::BTreeSet<String> = (0..20)
        .map(|_| serde_json::to_string(&JobDocument(&text(&t).build().unwrap())).unwrap())
        .collect();
    assert_eq!(json.len(), 1, "{json:#?}");
    let yaml: std::collections::BTreeSet<String> = (0..20)
        .map(|_| serde_saphyr::to_string(&JobDocument(&text(&t).build().unwrap())).unwrap())
        .collect();
    assert_eq!(yaml.len(), 1);
    let doc = json.into_iter().next().unwrap();
    let names: Vec<String> = doc
        .match_indices("\"paramType\"")
        .map(|(i, _)| {
            let name_at = doc[..i].rfind("\"name\":\"").unwrap() + 8;
            doc[name_at..].split('"').next().unwrap().to_string()
        })
        .collect();
    assert_eq!(
        names,
        ["P0", "P1", "P2", "P3", "P4", "P5", "P6", "P7", "P8", "P9"]
    );
    assert_eq!(job_from_str(&doc).unwrap(), text(&t).build().unwrap());
}

const ORIGIN_OLD: &str = r#"
specificationVersion: jobtemplate-2023-09
name: J
parameterDefinitions:
  - {name: A, type: STRING, default: a}
steps:
  - name: S
    script: {actions: {onRun: {command: "c {{Param.A}}"}}}
"#;

#[test]
fn origin_of_an_added_node_does_not_depend_on_the_depth() {
    let new = ORIGIN_OLD.replace(
        "steps:",
        "  - {name: B, type: INT, default: 7}\nsteps:\n  - name: T\n    script: {actions: {onRun: {command: \"t {{Param.A}}\"}}}",
    );
    for depth in [None, Some(0), Some(1), Some(2), Some(3), Some(4)] {
        let o = depth.map_or_else(DiffOptions::default, |n| opts().with_max_depth(n));
        let d = diff_templates(
            &text(ORIGIN_OLD).param("A", "x"),
            &text(&new).param("A", "y").param("B", "8"),
            &o,
        )
        .unwrap();
        let added: Vec<(&str, Origin)> = d
            .changes
            .iter()
            .filter(|c| {
                c.layer == Layer::Job && (c.path == "parameters[B]" || c.path == "steps[T]")
            })
            .map(|c| (c.path.as_str(), c.origin))
            .collect();
        let want: &[(&str, Origin)] = if depth == Some(0) {
            &[]
        } else {
            &[
                ("parameters[B]", Origin::Template),
                ("steps[T]", Origin::Template),
            ]
        };
        assert_eq!(added, want, "depth {depth:?}");
    }
}

const REORDER_OLD: &str = r#"
specificationVersion: jobtemplate-2023-09
name: J
steps:
  - name: S
    parameterSpace:
      taskParameterDefinitions:
        - {name: X, type: INT, range: "1-3"}
        - {name: Y, type: STRING, range: [p, q]}
    script: {actions: {onRun: {command: c}}}
"#;

fn reorder_new() -> String {
    REORDER_OLD
        .replace("        - {name: X, type: INT, range: \"1-3\"}\n", "")
        .replace(
            "range: [p, q]}\n",
            "range: [p, q]}\n        - {name: X, type: INT, range: \"1-3\"}\n",
        )
}

#[test]
fn task_parameter_reorder_shows_in_whole_node_values_at_every_depth() {
    let new = reorder_new();
    let mut seen = Vec::new();
    for depth in [0, 1, 2] {
        let d = diff_templates(
            &text(REORDER_OLD),
            &text(&new),
            &opts().with_max_depth(depth),
        )
        .unwrap();
        assert_eq!(d.changes.len(), 1, "depth {depth}");
        let c = &d.changes[0];
        let (b, a) = (c.before.as_ref().unwrap(), c.after.as_ref().unwrap());
        assert_ne!(b.raw, a.raw, "depth {depth}");
        assert_ne!(b.text, a.text, "depth {depth}");
        let space = |raw: &serde_json::Value| {
            let steps = raw.get("steps").and_then(|s| s.get(0));
            let step = steps.unwrap_or(raw);
            let space = step.get("parameterSpace").unwrap_or(raw);
            space["taskParameterDefinitions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["name"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(space(&b.raw), ["X", "Y"], "depth {depth}");
        assert_eq!(space(&a.raw), ["Y", "X"], "depth {depth}");
        seen.push(format!("{} | {}", b.text, a.text));
    }
    assert_eq!(seen, REORDER_TEXTS);
}

const REORDER_TEXTS: [&str; 3] = [
    "{name: J, parameters: {}, steps: [{name: S, parameterSpace: {X: 1-3, Y: [p, q]}, script: {actions: {onRun: {command: c}… | {name: J, parameters: {}, steps: [{name: S, parameterSpace: {Y: [p, q], X: 1-3}, script: {actions: {onRun: {command: c}…",
    "{parameterSpace: {X: 1-3, Y: [p, q]}, script: {actions: {onRun: {command: c}}}} | {parameterSpace: {Y: [p, q], X: 1-3}, script: {actions: {onRun: {command: c}}}}",
    "{X: 1-3, Y: [p, q]} | {Y: [p, q], X: 1-3}",
];

#[test]
fn display_prints_the_notes_after_the_totals() {
    let old = std::fs::read_to_string(sample_path("render_v1.yaml")).unwrap();
    let new = old.replace(
        "  - {name: Scene, type: PATH}",
        "  - {name: Scene, type: PATH}\n  - {name: Quality, type: INT}",
    );
    let d = diff_templates(
        &text(&old).param("Scene", "s"),
        &text(&new).param("Scene", "s").param("Quality", "3"),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        d.to_string(),
        format!(
            "\
+ parameter Quality: 3, defined as {{type: INT}}  [both]

0 steps changed. Tasks: 100 kept, 0 added, 0 removed.
note: {MID_NOTE}"
        )
    );
    let d = diff_templates(
        &text(ADAPTIVE),
        &text(&ADAPTIVE.replace("command: old", "command: new")),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        d.to_string(),
        "\
~ step Chunked
    ~ script.onRun.command: old -> new  [template]
    tasks: unknown (adaptive chunking)

1 step changed. Tasks: 0 kept, 0 added, 0 removed.
note: steps[Chunked]: cannot count tasks: the chunk size is adaptive (targetRuntimeSeconds), so the number of chunks is decided at run time"
    );
}

const DEFS_A: &str = r#"
specificationVersion: jobtemplate-2023-09
name: J
steps:
  - name: S
    parameterSpace:
      taskParameterDefinitions:
        - {name: A, type: INT, range: "1-2"}
    script: {actions: {onRun: {command: c}}}
"#;

#[test]
fn step_changes_follow_the_new_definition_order_then_the_old() {
    let new = DEFS_A.replace(
        "        - {name: A, type: INT, range: \"1-2\"}",
        "        - {name: Z, type: INT, range: \"1-2\"}\n        - {name: A, type: INT, range: \"1-3\"}",
    );
    let d = diff_templates(&text(DEFS_A), &text(&new), &opts()).unwrap();
    assert_eq!(d.to_string(), DEFS_ADDED);
    let old = DEFS_A.replace(
        "        - {name: A, type: INT, range: \"1-2\"}",
        "        - {name: A, type: INT, range: \"1-2\"}\n        - {name: Z, type: INT, range: \"1-2\"}\n        - {name: Y, type: INT, range: \"1-2\"}",
    );
    let new = DEFS_A.replace("\"1-2\"", "\"1-3\"");
    let d = diff_templates(&text(&old), &text(&new), &opts()).unwrap();
    assert_eq!(d.to_string(), DEFS_REMOVED);
}

const DEFS_ADDED: &str = "\
~ step S
    + Z: 1,2  [template]
    ~ A: 1,2 -> 1-3  [template]
    tasks: 2 -> 6, all replaced (task parameter added or removed)

1 step changed. Tasks: 0 kept, 0 added, 0 removed, 6 replaced.";
const DEFS_REMOVED: &str = "\
~ step S
    ~ A: 1,2 -> 1-3  [template]
    - Z: 1,2  [template]
    - Y: 1,2  [template]
    tasks: 8 -> 3, all replaced (task parameter added or removed)

1 step changed. Tasks: 0 kept, 0 added, 0 removed, 3 replaced.";

#[test]
fn each_added_parameter_folds_in_its_own_definition() {
    let t = "specificationVersion: jobtemplate-2023-09\nname: J\nsteps:\n  - name: S\n    script: {actions: {onRun: {command: c}}}\n";
    let new = t.replace(
        "steps:",
        "parameterDefinitions:\n  - {name: A, type: STRING, default: a}\n  - {name: B, type: INT, default: 7}\nsteps:",
    );
    let d = diff_templates(&text(t), &text(&new), &opts()).unwrap();
    assert_eq!(
        d.to_string(),
        "\
+ parameter A: a, defined as {default: a, type: STRING}  [template]
+ parameter B: 7, defined as {default: 7, type: INT}  [template]

0 steps changed. Tasks: 1 kept, 0 added, 0 removed."
    );
}

#[test]
fn an_object_with_a_name_is_not_a_symbol_unless_it_has_exactly_name_type_value() {
    let t = "specificationVersion: jobtemplate-2023-09\nname: J\nsteps:\n  - name: S\n    script: {actions: {onRun: {command: c}}}\n";
    let new = format!(
        "{t}  - name: T\n    hostRequirements:\n      amounts: [{{name: amount.worker.vcpu, min: 2, max: 4}}]\n    script: {{actions: {{onRun: {{command: c}}}}}}\n"
    );
    let d = diff_templates(
        &text(t),
        &text(&new),
        &DiffOptions::default().with_max_depth(2),
    )
    .unwrap();
    let c = d.changes.iter().find(|c| c.path == "steps[T]").unwrap();
    assert_eq!(c.after.as_ref().unwrap().text, HOST_REQ_TEXT);
}

const HOST_REQ_TEXT: &str = "{hostRequirements: {amounts: [{max: 4, min: 2, name: amount.worker.vcpu}]}, script: {actions: {onRun: {command: c}}}}";

#[test]
fn built_job_path_and_list_path_parameters_read_back() {
    use openjd_expr::path_mapping::PathFormat;
    let mut job = input("render_v1.yaml").build().unwrap();
    let p = job.parameters.get_mut("Scene").unwrap();
    p.value = ExprValue::new_path("/a", PathFormat::Posix);
    let mut lp = p.clone();
    lp.name = "LP".into();
    lp.param_type = openjd_model::types::JobParameterType::ListPath;
    lp.value = ExprValue::make_list(
        vec![
            ExprValue::new_path("/x", PathFormat::Posix),
            ExprValue::new_path("/y", PathFormat::Posix),
        ],
        openjd_expr::ExprType::PATH,
    )
    .unwrap();
    job.parameters.insert("LP".into(), lp);
    for doc in [
        serde_json::to_string(&JobDocument(&job)).unwrap(),
        serde_saphyr::to_string(&JobDocument(&job)).unwrap(),
    ] {
        let back = job_from_str(&doc).unwrap();
        assert_eq!(back, job);
        assert_eq!(
            back.parameters["Scene"].value,
            job.parameters["Scene"].value
        );
        assert_eq!(back.parameters["LP"].value, job.parameters["LP"].value);
    }
}

const CHUNKED: &str = r#"
specificationVersion: jobtemplate-2023-09
extensions: [TASK_CHUNKING]
name: C
steps:
  - name: S
    parameterSpace:
      taskParameterDefinitions:
        - name: F
          type: CHUNK[INT]
          range: "1-40"
          chunks: {defaultTaskCount: 10, targetRuntimeSeconds: 0, rangeConstraint: CONTIGUOUS}
    script: {actions: {onRun: {command: c, args: ["{{Task.Param.F}}"]}}}
"#;

#[test]
fn a_chunk_settings_change_renders_the_chunk_settings() {
    let new = CHUNKED
        .replace("targetRuntimeSeconds: 0", "targetRuntimeSeconds: 5")
        .replace("CONTIGUOUS}", "NONCONTIGUOUS}");
    let d = diff_templates(&text(CHUNKED), &text(&new), &opts()).unwrap();
    let c = d
        .changes
        .iter()
        .find(|c| c.path.ends_with("taskParameterDefinitions[F]"))
        .unwrap();
    assert_eq!(
        c.to_string(),
        "~ steps[S].parameterSpace.taskParameterDefinitions[F].definition: 1-40 chunks of 10, target 0s -> 1-40 chunks of 10, target 5s, noncontiguous  [template]"
    );
}

#[test]
fn a_two_value_range_renders_the_same_in_the_change_and_the_boxes() {
    let new = TASKS_6.replace("\"1-6\"", "\"1-2\"");
    let d = diff_templates(&text(TASKS_6), &text(&new), &opts()).unwrap();
    assert_eq!(
        d.to_string(),
        "\
~ step Render
    ~ N: 1-6 -> 1,2  [template]
    tasks: 6 -> 2
      keep   2     N 1,2
      remove 4     N 3-6

1 step changed. Tasks: 2 kept, 0 added, 4 removed."
    );
    let new = TASKS_6.replace("\"1-6\"", "[1, 2, 7]");
    let d = diff_templates(&text(TASKS_6), &text(&new), &opts()).unwrap();
    assert_eq!(
        d.to_string(),
        "\
~ step Render
    ~ N: 1-6 -> 1,2,7  [template]
    tasks: 6 -> 3
      keep   2     N 1,2
      add    1     N 7
      remove 4     N 3-6

1 step changed. Tasks: 2 kept, 1 added, 4 removed."
    );
}

#[test]
fn a_run_time_reference_inside_a_cancelation_object_is_run_time() {
    let t = "specificationVersion: jobtemplate-2023-09\nname: J\nsteps:\n  - name: S\n    script:\n      actions:\n        onRun: {command: c, cancelation: {mode: NOTIFY_THEN_TERMINATE, notifyPeriodInSeconds: 5}}\n";
    let old = text(t).build().unwrap();
    let doc = serde_json::to_string(&JobDocument(&old)).unwrap();
    assert!(doc.contains("\"notifyPeriodInSeconds\":\"5\""), "{doc}");
    let new = job_from_str(&doc.replace(
        "\"notifyPeriodInSeconds\":\"5\"",
        "\"notifyPeriodInSeconds\":\"{{Task.Param.N}}\"",
    ))
    .unwrap();
    let d = diff_built_jobs(old, new, &opts());
    let got: Vec<(String, Stage)> = d.changes.iter().map(|c| (c.to_string(), c.stage)).collect();
    assert_eq!(
        got,
        [(
            "~ steps[S].script.onRun.cancelation: {mode: NOTIFY_THEN_TERMINATE, notifyPeriodInSeconds: 5} -> {mode: NOTIFY_THEN_TERMINATE, notifyPeriodInSeconds: {{Task.Param.N}}}  [job]".to_string(),
            Stage::RunTime
        )]
    );
}

// ─── audit round 3 ──────────────────────────────────────────────────

fn f_list_diff(old: &str, new: &str) -> String {
    let t = |range: &str| {
        TASKS_6
            .replace("name: N", "name: F")
            .replace("\"1-6\"", range)
    };
    diff_templates(&text(&t(old)), &text(&t(new)), &opts())
        .unwrap()
        .to_string()
}

#[test]
fn a_list_that_does_not_increase_renders_in_its_own_order() {
    assert_eq!(
        f_list_diff("[5, 1, 2, 3]", "[5, 1, 2, 3, 4]"),
        "\
~ step Render
    ~ F: 5,1-3 -> 5,1-4  [template]
    tasks: 4 -> 5
      keep   4     F 5,1-3
      add    1     F 4

1 step changed. Tasks: 4 kept, 1 added, 0 removed."
    );
}

#[test]
fn repeated_integer_values_are_not_a_run() {
    assert_eq!(
        f_list_diff("[3, 3, 3]", "[3, 3, 3, 4]"),
        "\
~ step Render
    ~ F: 3,3,3 -> 3,3,3,4  [template]
    tasks: 3 -> 4, all replaced (not an exact plain product: association, chunking, a repeated range value or name, or a task count overflow)

1 step changed. Tasks: 0 kept, 0 added, 0 removed, 4 replaced."
    );
}

#[test]
fn job_from_str_names_each_malformed_parameter() {
    let job =
        |param: &str| format!(r#"{{"name": "x", "steps": [], "parameters": {{"LI": {param}}}}}"#);
    let cases = [
        (
            r#"{"paramType": "ListInt", "value": {"ListInt": []}}"#,
            "'name' must be a string",
        ),
        (
            r#"{"name": "LI", "value": {"ListInt": []}}"#,
            "'paramType' must be a string",
        ),
        (
            r#"{"name": "LI", "paramType": "Matrix", "value": {"ListInt": []}}"#,
            "unknown paramType 'Matrix'",
        ),
        (
            r#"{"name": "LI", "paramType": "ListInt"}"#,
            "missing 'value'",
        ),
        (
            r#"{"name": "LI", "paramType": "ListInt", "value": {"ListInt": [[1, 2]]}}"#,
            r#"unrecognized value {"ListInt":[[1,2]]}"#,
        ),
        (
            r#"{"name": "LI", "paramType": "ListBool", "value": {"ListBool": [[true]]}}"#,
            r#"unrecognized value {"ListBool":[[true]]}"#,
        ),
        (
            r#"{"name": "LI", "paramType": "ListFloat", "value": {"ListFloat": [["1.5"]]}}"#,
            "Expected a float or a float string",
        ),
        (
            r#"{"name": "LI", "paramType": "Int", "value": {"Int": "3"}}"#,
            r#"unrecognized value {"Int":"3"}"#,
        ),
        (
            r#"{"name": "LI", "paramType": "Int", "value": {"Int": 3, "Bool": true}}"#,
            r#"unrecognized value {"Bool":true,"Int":3}"#,
        ),
    ];
    for (param, message) in cases {
        let err = job_from_str(&job(param)).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("Validation error: not a built job: parameters[LI]: {message}"),
            "{param}"
        );
    }
}

#[test]
fn job_from_str_rejects_a_value_that_is_not_of_the_parameter_type() {
    let job = |ty: &str, value: &str| {
        format!(
            r#"{{"name": "x", "steps": [], "parameters": {{"P": {{"name": "P", "paramType": "{ty}", "value": {value}}}}}}}"#
        )
    };
    let cases = [
        ("ListInt", r#"{"ListList": [[], 48]}"#, "ListList"),
        ("ListBool", r#"{"ListInt": [1]}"#, "ListInt"),
        ("ListListInt", r#"{"ListInt": [1]}"#, "ListInt"),
        ("String", r#"{"Int": 3}"#, "Int"),
        ("Int", r#""Null""#, "Null"),
    ];
    for (ty, value, tag) in cases {
        let err = job_from_str(&job(ty, value)).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "Validation error: not a built job: parameters[P]: paramType '{ty}' cannot hold a '{tag}' value"
            ),
            "{ty} {value}"
        );
    }
}

#[test]
fn extensions_print_once_when_the_job_layer_origin_is_both() {
    // A CRLF checkout must still match the LF patterns below.
    let old = std::fs::read_to_string(sample_path("render_v1.yaml"))
        .unwrap()
        .replace("\r\n", "\n");
    let new = old
        .replace(
            "name: Render\nparameterDefinitions",
            "extensions: [EXPR]\nname: Render\nparameterDefinitions",
        )
        .replace(
            "  - {name: Scene, type: PATH}",
            "  - {name: Scene, type: PATH}\n  - {name: Quality, type: INT}",
        );
    let d = diff_templates(
        &text(&old).param("Scene", "s"),
        &text(&new).param("Scene", "s").param("Quality", "3"),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        d.to_string(),
        format!(
            "\
+ extensions: [EXPR]  [template]
+ parameter Quality: 3, defined as {{type: INT}}  [both]

0 steps changed. Tasks: 100 kept, 0 added, 0 removed.
note: {MID_NOTE}"
        )
    );
}

// ─── audit round 4 ──────────────────────────────────────────────────

#[test]
fn job_from_str_rejects_a_list_list_element_that_is_not_a_list_int() {
    let doc = r#"{"name": "x", "steps": [], "parameters": {"P": {"name": "P", "paramType": "ListListInt", "value": {"ListList": [[{"ListString": [["a"]]}]]}}}}"#;
    assert_eq!(
        job_from_str(doc).unwrap_err().to_string(),
        "Validation error: not a built job: parameters[P]: a ListList element must be a 'ListInt' value, not a 'ListString' value"
    );
}

#[test]
fn job_from_str_rejects_extra_parameter_keys_and_a_name_that_is_not_the_key() {
    let job =
        |param: &str| format!(r#"{{"name": "x", "steps": [], "parameters": {{"P": {param}}}}}"#);
    let cases = [
        (
            r#"{"name": "P", "paramType": "Int", "value": {"Int": 3}, "bogus": 1}"#,
            "unknown field 'bogus'",
        ),
        (
            r#"{"name": "Q", "paramType": "Int", "value": {"Int": 3}}"#,
            "'name' is 'Q', but the parameter's key is 'P'",
        ),
    ];
    for (param, message) in cases {
        assert_eq!(
            job_from_str(&job(param)).unwrap_err().to_string(),
            format!("Validation error: not a built job: parameters[P]: {message}"),
            "{param}"
        );
    }
}

/// A built job with a job environment and a step parameter space, as JSON.
fn built_job_value() -> serde_json::Value {
    let job = text(
        "specificationVersion: jobtemplate-2023-09\nname: J\njobEnvironments:\n  - name: E\n    variables: {A: a}\n    script: {actions: {onEnter: {command: e}}}\nsteps:\n  - name: S\n    parameterSpace:\n      taskParameterDefinitions:\n        - {name: F, type: INT, range: \"1-3\"}\n    script: {actions: {onRun: {command: c, args: [\"{{Task.Param.F}}\"]}}}\n",
    )
    .build()
    .unwrap();
    serde_json::to_value(JobDocument(&job)).unwrap()
}

#[test]
fn job_from_str_rejects_unknown_fields_at_every_level() {
    // (error path, JSON pointer)
    let cases = [
        ("steps[0]", "/steps/0"),
        ("jobEnvironments[0]", "/jobEnvironments/0"),
        (
            "steps[0].script.actions.onRun",
            "/steps/0/script/actions/onRun",
        ),
        ("steps[0].parameterSpace", "/steps/0/parameterSpace"),
        (
            "jobEnvironments[0].script.actions.onEnter",
            "/jobEnvironments/0/script/actions/onEnter",
        ),
    ];
    for (path, pointer) in cases {
        let mut v = built_job_value();
        v.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("bogus".into(), serde_json::json!(1));
        let doc = serde_json::to_string(&v).unwrap();
        assert_eq!(
            job_from_str(&doc).unwrap_err().to_string(),
            format!("Validation error: not a built job: {path}.bogus: unknown field"),
            "{path}"
        );
        let yaml = serde_saphyr::to_string(&v).unwrap();
        assert_eq!(
            job_from_str(&yaml).unwrap_err().to_string(),
            format!("Validation error: not a built job: {path}.bogus: unknown field"),
            "{path} (YAML)"
        );
    }
    // A null value does not hide an unknown key.
    let mut v = built_job_value();
    v["steps"][0]["bogus"] = serde_json::Value::Null;
    assert_eq!(
        job_from_str(&serde_json::to_string(&v).unwrap())
            .unwrap_err()
            .to_string(),
        "Validation error: not a built job: steps[0].bogus: unknown field"
    );
}

#[test]
fn job_from_str_accepts_known_fields_written_differently() {
    let mut v = built_job_value();
    // A float written as an integer, a skipped field written as null, and
    // the `letBindings` alias of `let`.
    let step = &mut v["steps"][0];
    step["resolvedSymTab"] = serde_json::Value::Null;
    let script = step["script"].as_object_mut().unwrap();
    script.remove("let");
    script.insert("letBindings".into(), serde_json::json!(["x = 1"]));
    step["hostRequirements"] =
        serde_json::json!({"amounts": [{"name": "amount.worker.vcpu", "min": 2}]});
    v["jobEnvironments"][0]["script"]["actions"]["onWrapTaskRun"] = serde_json::Value::Null;
    let job = job_from_str(&serde_json::to_string(&v).unwrap()).unwrap();
    assert_eq!(
        job.steps[0].script.let_bindings,
        Some(vec!["x = 1".to_string()])
    );
}

#[test]
fn changes_at_a_collection_matches_every_entry_in_it() {
    let t = |cmd: &str| {
        format!(
            "specificationVersion: jobtemplate-2023-09\nname: J\nparameterDefinitions:\n  - {{name: P, type: STRING, default: {cmd}}}\nsteps:\n  - name: Render\n    script: {{actions: {{onRun: {{command: \"{{{{Param.P}}}}\"}}}}}}\n  - name: Render2\n    script: {{actions: {{onRun: {{command: {cmd}}}}}}}\n"
        )
    };
    let d = diff_templates(&text(&t("a")), &text(&t("b")), &opts()).unwrap();
    let paths =
        |prefix: &str| -> Vec<String> { d.changes_at(prefix).map(|c| c.path.clone()).collect() };
    assert_eq!(paths("steps[Render]"), ["steps[Render]"]);
    assert_eq!(
        paths("steps"),
        ["steps[Render2].script.onRun", "steps[Render]"]
    );
    assert_eq!(paths("parameters"), ["parameters[P]"]);
    assert_eq!(paths("step"), Vec::<String>::new());
    // `[` does not continue a prefix that ends in `]`: the step `A][B` is
    // not under `steps[A]`.
    let t = |cmd: &str| {
        format!(
            "specificationVersion: jobtemplate-2023-09\nname: J\nsteps:\n  - name: \"A][B\"\n    script: {{actions: {{onRun: {{command: {cmd}}}}}}}\n  - name: A\n    script: {{actions: {{onRun: {{command: a}}}}}}\n"
        )
    };
    let d = diff_templates(&text(&t("x")), &text(&t("y")), &opts()).unwrap();
    assert_eq!(d.changes_at("steps[A]").count(), 0);
    assert_eq!(d.changes_at("steps[A][B]").count(), 1);
}

// ─── audit round 5 ──────────────────────────────────────────────────

/// A step with task parameter `F`, an `onRun` command, and an embedded
/// file `E` with a `filename` and `data`.
fn file_step(command: &str, filename: &str, data: &str) -> String {
    format!(
        "specificationVersion: jobtemplate-2023-09\nname: J\nsteps:\n  - name: S\n    parameterSpace:\n      taskParameterDefinitions:\n        - {{name: F, type: INT, range: \"1-2\"}}\n    script:\n      embeddedFiles:\n        - {{name: E, type: TEXT, filename: \"{filename}\", data: \"{data}\"}}\n      actions: {{onRun: {{command: \"{command}\"}}}}\n"
    )
}

#[test]
fn stage_of_command_data_and_filename_changes() {
    let f = "{{Task.Param.F}}";
    // (old (command, filename, data), new, change, stage)
    let cases = [
        (
            (format!("{f} a"), "f.txt".to_string(), "d".to_string()),
            (format!("{f} b"), "f.txt".to_string(), "d".to_string()),
            "~ steps[S].script.onRun.command: {{Task.Param.F}} a -> {{Task.Param.F}} b  [template]",
            Stage::RunTime,
        ),
        (
            ("c".to_string(), "f.txt".to_string(), format!("{f} a")),
            ("c".to_string(), "f.txt".to_string(), format!("{f} b")),
            "~ steps[S].script.embeddedFiles[E].data: {{Task.Param.F}} a -> {{Task.Param.F}} b  [template]",
            Stage::RunTime,
        ),
        // `filename` is a plain string: the worker writes `{{` literally.
        (
            ("c".to_string(), format!("f{f}.txt"), "d".to_string()),
            ("c".to_string(), format!("g{f}.txt"), "d".to_string()),
            "~ steps[S].script.embeddedFiles[E].filename: f{{Task.Param.F}}.txt -> g{{Task.Param.F}}.txt  [template]",
            Stage::Job,
        ),
    ];
    for ((oc, of, od), (nc, nf, nd), change, stage) in cases {
        let d = diff_templates(
            &text(&file_step(&oc, &of, &od)),
            &text(&file_step(&nc, &nf, &nd)),
            &opts(),
        )
        .unwrap();
        let got: Vec<(String, Stage)> =
            d.changes.iter().map(|c| (c.to_string(), c.stage)).collect();
        assert_eq!(got, [(change.to_string(), stage)], "{change}");
    }
}

#[test]
fn a_timeout_naming_a_session_symbol_is_run_time() {
    // Templates reject `Task.` and `Session.` in `timeout`, so edit a built job.
    let t = "specificationVersion: jobtemplate-2023-09\nname: J\nsteps:\n  - name: S\n    script:\n      actions:\n        onRun: {command: c, timeout: 60}\n";
    let old = text(t).build().unwrap();
    let doc = serde_json::to_string(&JobDocument(&old)).unwrap();
    assert!(doc.contains("\"timeout\":\"60\""), "{doc}");
    let new =
        job_from_str(&doc.replace("\"timeout\":\"60\"", "\"timeout\":\"{{Session.T}}\"")).unwrap();
    let d = diff_built_jobs(old, new, &opts());
    let got: Vec<(String, Stage)> = d.changes.iter().map(|c| (c.to_string(), c.stage)).collect();
    assert_eq!(
        got,
        [(
            "~ steps[S].script.onRun.timeout: 60 -> {{Session.T}}  [job]".to_string(),
            Stage::RunTime
        )]
    );
}

/// Old template: `P` defaults to `a`. New template: `P` defaults to `b`,
/// and the new parameters set `P` back to `a` and change `Q`.
fn undone_by_a_parameter(depth: Option<usize>) -> String {
    let t = |p: &str| {
        format!(
            "specificationVersion: jobtemplate-2023-09\nname: J\nparameterDefinitions:\n  - {{name: P, type: STRING, default: {p}}}\n  - {{name: Q, type: STRING}}\nsteps:\n  - name: S\n    parameterSpace:\n      taskParameterDefinitions:\n        - {{name: A, type: STRING, range: [\"{{{{Param.P}}}}\"]}}\n        - {{name: B, type: STRING, range: [\"{{{{Param.Q}}}}\"]}}\n    script: {{actions: {{onRun: {{command: c}}}}}}\n"
        )
    };
    let mut o = opts();
    if let Some(n) = depth {
        o = o.with_max_depth(n);
    }
    diff_templates(
        &text(&t("a")).param("Q", "q1"),
        &text(&t("b")).param("P", "a").param("Q", "q2"),
        &o,
    )
    .unwrap()
    .to_string()
}

#[test]
fn an_unexpanded_node_differing_in_both_halves_is_both() {
    // Full depth: the only job-layer change below the step is `B`, from the
    // parameters. Depth 1: the step is whole, and it differs in both halves
    // (old -> mid changes `A`, mid -> new changes it back and changes `B`),
    // so it is `both`.
    assert_eq!(
        undone_by_a_parameter(None),
        "\
~ parameter definition P.default: a -> b  [template]
~ parameter Q: q1 -> q2  [parameters]
~ step S
    ~ B: [q1] -> [q2]  [parameters]
    tasks: 1 -> 1
      add    1     A a × B q2
      remove 1     A a × B q1

1 step changed. Tasks: 0 kept, 1 added, 1 removed."
    );
    assert_eq!(
        undone_by_a_parameter(Some(1)),
        "\
~ parameter definition P.default: a -> b  [template]
~ parameter Q: q1 -> q2  [parameters]
~ step S
    ~ (not expanded)  [both]
    tasks: 1 -> 1
      add    1     A a × B q2
      remove 1     A a × B q1

1 step changed. Tasks: 0 kept, 1 added, 1 removed."
    );
}

/// A built job with a job environment, a step environment, an embedded
/// file and `NOTIFY_THEN_TERMINATE` cancelations, as JSON.
fn rich_built_job_value() -> serde_json::Value {
    let job = text(
        "specificationVersion: jobtemplate-2023-09\nname: J\njobEnvironments:\n  - name: E\n    script: {actions: {onEnter: {command: e, cancelation: {mode: NOTIFY_THEN_TERMINATE}}}}\nsteps:\n  - name: S\n    stepEnvironments:\n      - name: SE\n        script: {actions: {onEnter: {command: se}}}\n    script:\n      embeddedFiles: [{name: EF, type: TEXT, data: d}]\n      actions: {onRun: {command: c, cancelation: {mode: NOTIFY_THEN_TERMINATE}}}\n",
    )
    .build()
    .unwrap();
    serde_json::to_value(JobDocument(&job)).unwrap()
}

#[test]
fn job_from_str_accepts_a_null_skipped_field_only_where_the_field_exists() {
    // (parent pointer, key, the error path when the placement is rejected)
    let mut cases: Vec<(&str, &str, Option<String>)> = vec![
        ("/steps/0", "resolvedSymTab", None),
        ("/jobEnvironments/0", "resolvedSymTab", None),
        ("/steps/0/stepEnvironments/0", "resolvedSymTab", None),
        (
            "/steps/0/script/actions/onRun",
            "resolvedSymTab",
            Some("steps[0].script.actions.onRun.resolvedSymTab".into()),
        ),
        (
            "/steps/0/script/actions/onRun/cancelation",
            "notifyPeriodInSeconds",
            None,
        ),
        (
            "/jobEnvironments/0/script/actions/onEnter/cancelation",
            "notifyPeriodInSeconds",
            None,
        ),
        (
            "/steps/0/script/actions/onRun",
            "notifyPeriodInSeconds",
            Some("steps[0].script.actions.onRun.notifyPeriodInSeconds".into()),
        ),
    ];
    for key in ["onWrapEnvEnter", "onWrapTaskRun", "onWrapEnvExit"] {
        cases.push(("/jobEnvironments/0/script/actions", key, None));
        cases.push(("/steps/0/stepEnvironments/0/script/actions", key, None));
        cases.push((
            "/jobEnvironments/0/script/actions/onEnter",
            key,
            Some(format!("jobEnvironments[0].script.actions.onEnter.{key}")),
        ));
    }
    for (pointer, key, rejected) in cases {
        let mut v = rich_built_job_value();
        v.pointer_mut(pointer)
            .unwrap_or_else(|| panic!("{pointer}"))
            .as_object_mut()
            .unwrap()
            .insert(key.into(), serde_json::Value::Null);
        let got = job_from_str(&serde_json::to_string(&v).unwrap()).map(drop);
        let want = rejected
            .map(|path| format!("Validation error: not a built job: {path}: unknown field"));
        assert_eq!(
            got.map_err(|e| e.to_string()),
            want.map_or(Ok(()), Err),
            "{pointer} {key}"
        );
    }
}

#[test]
fn job_from_str_accepts_an_alias_only_where_its_field_exists() {
    // (parent pointer, written key, alias, the error path when rejected)
    let cases = [
        ("/steps/0/script", "let", "letBindings", None),
        ("/jobEnvironments/0/script", "let", "letBindings", None),
        ("/steps/0/script/embeddedFiles/0", "fileType", "type", None),
        (
            "/steps/0/script/actions/onRun",
            "",
            "letBindings",
            Some("steps[0].script.actions.onRun.letBindings"),
        ),
        (
            "/steps/0/script/actions/onRun",
            "",
            "type",
            Some("steps[0].script.actions.onRun.type"),
        ),
    ];
    for (pointer, written, alias, rejected) in cases {
        let mut v = rich_built_job_value();
        let obj = v.pointer_mut(pointer).unwrap().as_object_mut().unwrap();
        let value = match obj.remove(written) {
            Some(serde_json::Value::Null) | None if alias == "letBindings" => {
                serde_json::json!(["x = 1"])
            }
            Some(w) => w,
            None => serde_json::json!("TEXT"),
        };
        obj.insert(alias.into(), value.clone());
        let got = job_from_str(&serde_json::to_string(&v).unwrap());
        match rejected {
            Some(path) => assert_eq!(
                got.unwrap_err().to_string(),
                format!("Validation error: not a built job: {path}: unknown field"),
                "{pointer} {alias}"
            ),
            None => {
                let back = serde_json::to_value(JobDocument(&got.unwrap())).unwrap();
                assert_eq!(
                    back.pointer(&format!("{pointer}/{written}")),
                    Some(&value),
                    "{pointer} {alias}"
                );
            }
        }
    }
}

#[test]
fn job_from_str_checks_symbol_table_entries() {
    let entry = |e: serde_json::Value| {
        let mut v = rich_built_job_value();
        v["steps"][0]["stepEnvironments"][0]["resolvedSymTab"] = serde_json::json!([e]);
        job_from_str(&serde_json::to_string(&v).unwrap())
            .map(drop)
            .map_err(|e| e.to_string())
    };
    let at = "Validation error: not a built job: steps[0].stepEnvironments[0].resolvedSymTab";
    assert_eq!(
        entry(serde_json::json!({"name": "Param.Z", "type": "string", "value": "z"})),
        Ok(())
    );
    let cases = [
        (
            serde_json::json!({"name": "Param.Z", "type": "string", "value": "z", "zzBogus": 1}),
            format!("{at}[0].zzBogus: unknown field"),
        ),
        (
            serde_json::json!({"name": "Param.Z", "type": "string"}),
            format!("{at}[0]: missing field 'value'"),
        ),
        (
            serde_json::json!({"name": 1, "type": "string", "value": "z"}),
            format!("{at}[0].name: expected a string"),
        ),
        (
            serde_json::json!({"name": "Param.Z", "type": "int", "value": "z"}),
            format!("{at}[0].value: Cannot convert 'z' to int: invalid digit found in string"),
        ),
        (
            serde_json::json!("Param.Z"),
            format!("{at}[0]: expected an object with 'name', 'type' and 'value'"),
        ),
    ];
    for (e, message) in cases {
        assert_eq!(entry(e.clone()), Err(message), "{e}");
    }
    let mut v = rich_built_job_value();
    v["jobEnvironments"][0]["resolvedSymTab"] = serde_json::json!({"name": "Param.Z"});
    assert_eq!(
        job_from_str(&serde_json::to_string(&v).unwrap()).unwrap_err().to_string(),
        "Validation error: not a built job: jobEnvironments[0].resolvedSymTab: expected an array of symbol-table entries"
    );
}

#[test]
fn job_from_str_names_an_unknown_field_in_a_task_parameter_definition() {
    let def = "/steps/0/parameterSpace/taskParameterDefinitions/F";
    let at = "steps[0].parameterSpace.taskParameterDefinitions.F";
    // (below the definition: JSON pointer, error path)
    let cases = [
        ("", ""),
        ("/int", ".int"),
        ("/int/range", ".int.range"),
        ("/int/range/rangeExpr", ".int.range.rangeExpr"),
        (
            "/int/range/rangeExpr/ranges/0",
            ".int.range.rangeExpr.ranges[0]",
        ),
    ];
    for (pointer, path) in cases {
        let mut v = built_job_value();
        v.pointer_mut(&format!("{def}{pointer}"))
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("bogus".into(), serde_json::json!(1));
        let want = format!("Validation error: not a built job: {at}{path}.bogus: unknown field");
        let doc = serde_json::to_string(&v).unwrap();
        assert_eq!(
            job_from_str(&doc).unwrap_err().to_string(),
            want,
            "{pointer}"
        );
        let yaml = serde_saphyr::to_string(&v).unwrap();
        assert_eq!(
            job_from_str(&yaml).unwrap_err().to_string(),
            want,
            "{pointer} (YAML)"
        );
    }
}

// ─── audit round 6 ──────────────────────────────────────────────────

/// Steps `A` and `other`, with `let` text and `other`'s command as given.
fn two_steps(other: &str, let_text: &str, command: &str) -> TemplateInput {
    text(&format!(
        "specificationVersion: jobtemplate-2023-09\nextensions: [EXPR]\nname: J\nsteps:\n  - name: A\n    script: {{actions: {{onRun: {{command: a}}}}}}\n  - name: \"{other}\"\n    let: [\"{let_text}\"]\n    script: {{actions: {{onRun: {{command: {command}}}}}}}\n"
    ))
}

#[test]
fn a_change_belongs_to_its_step_by_identity_not_by_path_prefix() {
    for other in ["A].x", "A][B"] {
        let d = diff_templates(
            &two_steps(other, "u = 1", "x"),
            &two_steps(other, "u = 2", "y"),
            &opts(),
        )
        .unwrap();
        assert_eq!(
            d.to_string(),
            format!(
                "\
~ step {other}
    ~ let: [u = 1] -> [u = 2]  [template]
    ~ script.onRun.command: x -> y  [template]
    tasks: 1 unchanged

1 step changed. Tasks: 2 kept, 0 added, 0 removed."
            ),
            "{other}"
        );
        // Both layers carry the owning step.
        let owners: Vec<(Layer, Option<&str>)> = d
            .changes
            .iter()
            .map(|c| (c.layer, c.step.as_deref()))
            .collect();
        assert_eq!(
            owners,
            [(Layer::Template, Some(other)), (Layer::Job, Some(other))],
            "{other}"
        );
        assert_eq!(d.changes_in_step("A").count(), 0, "{other}");
        assert_eq!(d.changes_in_step(other).count(), 2, "{other}");
        assert_eq!(d.stat().steps_changed, 1, "{other}");
        // A path prefix cannot tell `A` from `A].x`, which is why
        // `changes_in_step` exists.
        let at_a = if other == "A].x" { 2 } else { 0 };
        assert_eq!(d.changes_at("steps[A]").count(), at_a, "{other}");
    }
}

#[test]
fn job_level_changes_have_no_step() {
    let d = diff_job_template_with_parameter_sets(
        &input("render_v1.yaml"),
        &params(&[("Frames", "1-100")]),
        &params(&[("Frames", "1-200")]),
        &opts(),
    )
    .unwrap();
    let owners: Vec<(String, Option<&str>)> = d
        .changes
        .iter()
        .map(|c| (c.to_string(), c.step.as_deref()))
        .collect();
    assert_eq!(
        owners,
        [
            (
                "~ parameters[Frames].value: 1-100 -> 1-200  [parameters]".to_string(),
                None
            ),
            (
                "~ steps[Render].parameterSpace.taskParameterDefinitions[Frame].definition: 1-100 -> 1-200  [parameters]".to_string(),
                Some("Render")
            ),
        ]
    );
    let json = serde_json::to_value(&d.changes).unwrap();
    assert_eq!(json[0].get("step"), None);
    assert_eq!(json[1]["step"], "Render");
}

#[test]
fn mid_is_built_with_the_new_sides_limits() {
    // `mid` (the new template with the old Frames=1-100) has 200 tasks; the
    // new side allows 150. With the old side's limits `mid` would build and
    // the origins would split.
    let limits = openjd_model::CallerLimits {
        max_task_count: Some(150),
        ..Default::default()
    };
    let d = diff_templates(
        &input("render_v1.yaml").param("Frames", "1-100"),
        &input("render_v2.yaml")
            .param("Frames", "1-50")
            .limits(limits),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        d.to_string(),
        "\
~ parameter Frames: 1-100 -> 1-50  [both]
~ step Render
    ~ Frame: 1-100 -> 1-50  [both]
    ~ Eye: [left] -> [left, right]  [both]
    tasks: 100 -> 100
      keep   50    Frame 1-50 × Eye left
      add    50    Frame 1-50 × Eye right
      remove 50    Frame 51-100 × Eye left

1 step changed. Tasks: 50 kept, 50 added, 50 removed.
note: origin: the new template does not build with the old parameters, so job-layer changes are marked both: Model validation error: 1 validation error for Template
Template: Total task count (200) exceeds caller limit of 150."
    );
}

/// A one-step template whose single task parameter is `def`.
fn one_parameter(extensions: &str, def: &str) -> String {
    format!(
        "specificationVersion: jobtemplate-2023-09\n{extensions}name: J\nsteps:\n  - name: S\n    parameterSpace:\n      taskParameterDefinitions:\n        - {def}\n    script: {{actions: {{onRun: {{command: c}}}}}}\n"
    )
}

#[test]
fn job_from_str_reads_back_every_task_parameter_and_range_kind() {
    let cases = [
        ("", "{name: F, type: FLOAT, range: [1.5, 2.5]}"),
        ("", "{name: P, type: PATH, range: [/a, /b]}"),
        ("", "{name: N, type: INT, range: [3, 1, 2]}"),
        (
            "extensions: [TASK_CHUNKING]\n",
            "{name: C, type: \"CHUNK[INT]\", range: \"1-10\", chunks: {defaultTaskCount: 5, rangeConstraint: CONTIGUOUS}}",
        ),
    ];
    for (extensions, def) in cases {
        let job = text(&one_parameter(extensions, def)).build().unwrap();
        for doc in [
            serde_json::to_string(&JobDocument(&job)).unwrap(),
            serde_saphyr::to_string(&JobDocument(&job)).unwrap(),
        ] {
            let back = job_from_str(&doc).unwrap_or_else(|e| panic!("{def}: {e}"));
            assert_eq!(back, job, "{def}");
        }
    }
}

#[test]
fn job_from_str_accepts_a_null_notify_period_in_a_step_environment() {
    let mut v = rich_built_job_value();
    v["steps"][0]["stepEnvironments"][0]["script"]["actions"]["onEnter"]["cancelation"] =
        serde_json::json!({"mode": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": null});
    let job = job_from_str(&serde_json::to_string(&v).unwrap()).unwrap();
    let back = serde_json::to_value(JobDocument(&job)).unwrap();
    assert_eq!(
        back.pointer("/steps/0/stepEnvironments/0/script/actions/onEnter/cancelation"),
        Some(&serde_json::json!({"mode": "NOTIFY_THEN_TERMINATE"}))
    );
}

#[test]
fn a_decreasing_integer_list_is_not_a_run() {
    let d = diff_templates(
        &text(&one_parameter("", "{name: N, type: INT, range: [3, 2, 1]}")),
        &text(&one_parameter(
            "",
            "{name: N, type: INT, range: [3, 2, 1, 0]}",
        )),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        d.to_string(),
        "\
~ step S
    ~ N: 3,2,1 -> 3,2,1,0  [template]
    tasks: 3 -> 4
      keep   3     N 3,2,1
      add    1     N 0

1 step changed. Tasks: 3 kept, 1 added, 0 removed."
    );
}

#[test]
fn job_from_str_rejects_null_parameters() {
    let mut v = built_job_value();
    v["parameters"] = serde_json::Value::Null;
    let doc = serde_json::to_string(&v).unwrap();
    assert_eq!(
        job_from_str(&doc).unwrap_err().to_string(),
        "Validation error: not a built job: invalid type: null, expected a map at line 1 column 301"
    );
}

#[test]
fn job_from_str_rejects_a_repeated_symbol_name() {
    let mut v = rich_built_job_value();
    v["steps"][0]["resolvedSymTab"] = serde_json::json!([
        {"name": "Param.Z", "type": "string", "value": "a"},
        {"name": "Param.Z", "type": "string", "value": "b"},
    ]);
    assert_eq!(
        job_from_str(&serde_json::to_string(&v).unwrap())
            .unwrap_err()
            .to_string(),
        "Validation error: not a built job: steps[0].resolvedSymTab[1].name: 'Param.Z' is repeated"
    );
}

#[test]
fn a_step_whose_space_does_not_iterate_has_an_unknown_task_count() {
    let t = one_parameter("", "{name: F, type: INT, range: \"1-2\"}");
    let old = text(&t).build().unwrap();
    let doc = serde_json::to_string(&JobDocument(&old)).unwrap();
    assert!(doc.contains("\"combination\":null"), "{doc}");
    let new = job_from_str(&doc.replace("\"combination\":null", "\"combination\":\"F * Bogus\""))
        .unwrap();
    let d = diff_built_jobs(old, new, &opts());
    let s = d.step("S").unwrap();
    assert_eq!(s.tasks_unknown.as_deref(), Some("cannot count tasks"));
    assert_eq!(
        s.tasks,
        TaskSummary::Replaced {
            old_count: 0,
            new_count: 0,
            reason: "cannot count tasks".into()
        }
    );
    assert_eq!(s.stat_line(), "~ S  tasks unknown (cannot count tasks)");
    assert_eq!(
        d.notes,
        [Note {
            step: Some("S".into()),
            text: "cannot count tasks: Validation error: Unknown parameter 'Bogus' in combination expression".into()
        }]
    );
}

// ─── audit round 7 ──────────────────────────────────────────────────

#[test]
fn a_text_source_joins_relative_path_defaults_to_the_cwd_override() {
    let t = "specificationVersion: jobtemplate-2023-09\nname: J\nparameterDefinitions:\n  - {name: P, type: PATH, default: rel}\nsteps:\n  - name: S\n    script: {actions: {onRun: {command: c}}}\n";
    let dir = tempfile::tempdir().unwrap();
    let elsewhere = dir.path().join("somewhere").join("else");
    assert_ne!(std::env::current_dir().unwrap(), elsewhere);
    let job = text(t).cwd(&elsewhere).build().unwrap();
    assert_eq!(
        job.parameters["P"].value.to_display_string(),
        elsewhere.join("rel").to_str().unwrap()
    );
}

// ─── diff gallery audit round 1: rendering ──────────────────────────

/// One step whose only task parameter is `def`.
fn one_param_job(def: &str) -> String {
    format!(
        "\
specificationVersion: jobtemplate-2023-09
name: J
steps:
  - name: S
    parameterSpace:
      taskParameterDefinitions:
        - {def}
    script: {{actions: {{onRun: {{command: a, args: [\"{{{{Task.Param.P}}}}\"]}}}}}}
"
    )
}

#[test]
fn a_range_rewritten_with_the_same_values_shows_how_each_side_is_written() {
    let d = diff_templates(
        &text(&one_param_job(r#"{name: P, type: INT, range: "1-2"}"#)),
        &text(&one_param_job("{name: P, type: INT, range: [1, 2]}")),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        d.to_string(),
        "\
~ step S
    ~ P: range expression 1,2 -> list [1, 2]  [template]
    tasks: 2 unchanged

1 step changed. Tasks: 2 kept, 0 added, 0 removed."
    );
    assert_eq!(
        d.plan().unwrap().kinds(),
        std::collections::BTreeSet::from([MutationKind::StepParameterSpace])
    );
}

#[test]
fn a_task_parameter_whose_type_changes_but_renders_alike_shows_its_type() {
    let d = diff_templates(
        &text(&one_param_job("{name: P, type: FLOAT, range: [1.5, 2.5]}")),
        &text(&one_param_job(
            r#"{name: P, type: STRING, range: ["1.5", "2.5"]}"#,
        )),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        lines(&d.changes),
        ["~ steps[S].parameterSpace.taskParameterDefinitions[P].definition: FLOAT [1.5, 2.5] -> STRING [1.5, 2.5]  [template]"]
    );
}

#[test]
fn a_parameter_value_whose_type_changes_shows_both_types() {
    let job = |def: &str| {
        format!(
            "\
specificationVersion: jobtemplate-2023-09
name: J
parameterDefinitions:
  - {def}
steps:
  - name: S
    script: {{actions: {{onRun: {{command: a}}}}}}
"
        )
    };
    let d = diff_templates(
        &text(&job("{name: P, type: STRING, default: abc}")),
        &text(&job("{name: P, type: INT, default: 7}")),
        &opts(),
    )
    .unwrap();
    let value = d
        .changes
        .iter()
        .find(|c| c.path == "parameters[P]" && c.field == "value")
        .unwrap();
    assert_eq!(
        value.to_string(),
        r#"~ parameters[P].value: "abc" (STRING) -> 7 (INT)  [template]"#
    );
}

#[test]
fn environment_dumps_leave_out_nulls_the_name_and_an_empty_symbol_table() {
    let old = "\
specificationVersion: jobtemplate-2023-09
name: J
steps:
  - name: S
    script: {actions: {onRun: {command: a}}}
";
    let new = "\
specificationVersion: jobtemplate-2023-09
name: J
jobEnvironments:
  - name: Tools
    variables: {TOOL: v1}
steps:
  - name: S
    stepEnvironments:
      - name: Cache
        description: scratch space
        script: {actions: {onEnter: {command: mk}}}
    script: {actions: {onRun: {command: a}}}
";
    let d = diff_templates(&text(old), &text(new), &opts()).unwrap();
    assert_eq!(
        d.to_string(),
        "\
+ jobEnvironments[Tools]: {variables: {TOOL: v1}}  [template]
~ step S
    + stepEnvironments[Cache]: {description: scratch space, script: {actions: {onEnter: {command: mk}}}}  [template]
    tasks: 1 unchanged

1 step changed. Tasks: 1 kept, 0 added, 0 removed."
    );
}

#[test]
fn an_empty_symbol_table_is_shown_as_none() {
    let old = "\
specificationVersion: jobtemplate-2023-09
name: J
extensions: [EXPR]
steps:
  - name: S
    script: {actions: {onRun: {command: a}}}
";
    let new = old
        .replace("  - name: S\n", "  - name: S\n    let: [\"m = 'x'\"]\n")
        .replace("command: a}", "command: a, args: [\"{{m}}\"]}");
    let d = diff_templates(&text(old), &text(&new), &opts()).unwrap();
    let symtab = d
        .changes
        .iter()
        .find(|c| c.field == "resolvedSymTab")
        .unwrap();
    assert_eq!(
        symtab.to_string(),
        "~ steps[S].resolvedSymTab: (none) -> m=x  [template]"
    );
}

#[test]
fn a_whole_parameter_space_shows_each_range_and_the_combination() {
    let old = "\
specificationVersion: jobtemplate-2023-09
name: J
steps:
  - name: S
    script: {actions: {onRun: {command: a}}}
";
    let new = "\
specificationVersion: jobtemplate-2023-09
name: J
steps:
  - name: S
    parameterSpace:
      taskParameterDefinitions:
        - {name: F, type: INT, range: \"1-3\"}
        - {name: E, type: STRING, range: [l, r, c]}
      combination: \"(F, E)\"
    script: {actions: {onRun: {command: a}}}
";
    let d = diff_templates(&text(old), &text(new), &opts()).unwrap();
    assert_eq!(
        lines(&d.changes),
        ["+ steps[S].parameterSpace: {F: 1-3, E: [l, r, c]}, combination (F, E)  [template]"]
    );
}

#[test]
fn a_long_parameter_value_whose_type_changes_shows_the_difference_and_both_types() {
    let long = "a".repeat(200);
    let job = |ty: &str, v: &str| {
        let t = format!(
            "\
specificationVersion: jobtemplate-2023-09
name: J
parameterDefinitions:
  - {{name: X, type: {ty}}}
steps:
  - name: S
    script: {{actions: {{onRun: {{command: a}}}}}}
"
        );
        text(&t).param("X", format!("//srv/{long}{v}"))
    };
    let d = diff_templates(&job("STRING", "1"), &job("PATH", "2"), &opts()).unwrap();
    let value = d
        .changes
        .iter()
        .find(|c| c.path == "parameters[X]" && c.field == "value")
        .unwrap();
    let (b, a) = (
        value.before.as_ref().unwrap(),
        value.after.as_ref().unwrap(),
    );
    let tail = "a".repeat(40);
    assert_eq!(b.text, format!("…{tail}1\" (STRING)"));
    assert_eq!(a.text, format!("…{tail}2\" (PATH)"));
}

/// A job with `files` embedded in a job environment's script and a step's.
fn embedded_file_job(env_files: &str, step: &str, step_files: &str) -> String {
    format!(
        "\
specificationVersion: jobtemplate-2023-09
name: J
jobEnvironments:
  - name: E
    script:
      actions: {{onEnter: {{command: a}}}}
      embeddedFiles: [{env_files}]
steps:
  - name: \"{step}\"
    script:
      actions: {{onRun: {{command: a}}}}
      embeddedFiles: [{step_files}]
"
    )
}

#[test]
fn whole_values_drop_nulls_and_the_name_the_label_shows_wherever_they_are() {
    const KEEP: &str = "{name: K, type: TEXT, data: k}";
    const F: &str = "{name: K, type: TEXT, data: k}, {name: F, type: TEXT, data: hi}";
    for step in ["S", "MyEnvironments[1]"] {
        let d = diff_templates(
            &text(&embedded_file_job(KEEP, step, KEEP)),
            &text(&embedded_file_job(F, step, F)),
            &opts(),
        )
        .unwrap();
        assert_eq!(
            lines(&d.changes),
            [
                "+ jobEnvironments[E].script.embeddedFiles[F]: {data: hi, fileType: TEXT}  [template]"
                    .to_string(),
                format!("+ steps[{step}].script.embeddedFiles[F]: {{data: hi, fileType: TEXT}}  [template]"),
            ],
            "step {step}"
        );
    }
    // A step at the depth limit, by any name, is shown the same way.
    let shown = |step: &str| {
        let d = diff_templates(
            &text(&embedded_file_job(KEEP, step, KEEP)),
            &text(&embedded_file_job(KEEP, step, F)),
            &opts().with_max_depth(1),
        )
        .unwrap();
        d.changes[0].after.as_ref().unwrap().text.clone()
    };
    assert_eq!(shown("S"), shown("MyEnvironments[1]"));
    assert!(
        shown("S").starts_with(
            "{script: {actions: {onRun: {command: a}}, embeddedFiles: [{data: k, fileType: TEXT, name: K}"
        ),
        "{}",
        shown("S")
    );
}

#[test]
fn a_host_requirement_shows_no_nulls_and_whole_numbers_without_a_fraction() {
    let job = |host: &str| {
        format!(
            "\
specificationVersion: jobtemplate-2023-09
name: J
steps:
  - name: S
{host}    script: {{actions: {{onRun: {{command: a}}}}}}
"
        )
    };
    let d = diff_templates(
        &text(&job("")),
        &text(&job(
            "    hostRequirements: {amounts: [{name: amount.worker.vcpu, min: 4}]}\n",
        )),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        lines(&d.changes),
        ["+ steps[S].hostRequirements: {amounts: [{min: 4, name: amount.worker.vcpu}]}  [template]"]
    );
}

#[test]
fn reordered_dependencies_show_the_step_names() {
    let job = |deps: &str| {
        format!(
            "\
specificationVersion: jobtemplate-2023-09
name: J
steps:
  - {{name: A, script: {{actions: {{onRun: {{command: a}}}}}}}}
  - {{name: B, script: {{actions: {{onRun: {{command: a}}}}}}}}
  - name: C
    dependencies: {deps}
    script: {{actions: {{onRun: {{command: a}}}}}}
"
        )
    };
    let d = diff_templates(
        &text(&job("[{dependsOn: A}, {dependsOn: B}]")),
        &text(&job("[{dependsOn: B}, {dependsOn: A}]")),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        lines(&d.changes),
        ["~ steps[C].dependencies.order: [A, B] -> [B, A]  [template]"]
    );
}

#[test]
fn a_string_range_rewritten_as_a_path_range_shows_both_types() {
    let d = diff_templates(
        &text(&one_param_job("{name: P, type: STRING, range: [a]}")),
        &text(&one_param_job("{name: P, type: PATH, range: [a]}")),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        lines(&d.changes),
        ["~ steps[S].parameterSpace.taskParameterDefinitions[P].definition: STRING [a] -> PATH [a]  [template]"]
    );
}

/// One step whose only task parameter is a `CHUNK[INT]` with `range`.
fn chunked_job(range: &str) -> String {
    format!(
        "\
specificationVersion: jobtemplate-2023-09
extensions: [TASK_CHUNKING]
name: J
steps:
  - name: S
    parameterSpace:
      taskParameterDefinitions:
        - name: P
          type: CHUNK[INT]
          range: {range}
          chunks: {{defaultTaskCount: 1, rangeConstraint: CONTIGUOUS}}
    script: {{actions: {{onRun: {{command: a, args: [\"{{{{Task.Param.P}}}}\"]}}}}}}
"
    )
}

#[test]
fn a_chunked_range_rewritten_with_the_same_values_shows_its_chunk_settings() {
    let d = diff_templates(
        &text(&chunked_job("\"1-2\"")),
        &text(&chunked_job("[1, 2]")),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        lines(&d.changes),
        ["~ steps[S].parameterSpace.taskParameterDefinitions[P].definition: CHUNK[INT] range expression 1,2 chunks of 1 -> CHUNK[INT] list [1, 2] chunks of 1  [template]"]
    );
}

#[test]
fn an_int_with_chunks_and_the_chunk_int_it_mirrors_name_their_types() {
    // `create_job` never builds an `INT` with `chunks`, but `job_from_str`
    // reads one, and its range and chunks are the `CHUNK[INT]`'s.
    let old = TemplateInput::new(TemplateSource::Text(chunked_job("\"1-20\"")))
        .build()
        .unwrap();
    let doc = serde_json::to_string(&JobDocument(&old)).unwrap();
    assert!(doc.contains("\"chunkInt\""), "{doc}");
    let new = job_from_str(&doc.replace("\"chunkInt\"", "\"int\"")).unwrap();
    let d = diff_built_jobs(old, new, &opts());
    let c = d
        .changes
        .iter()
        .find(|c| c.path.ends_with("taskParameterDefinitions[P]"))
        .unwrap();
    assert_eq!(
        c.to_string(),
        "~ steps[S].parameterSpace.taskParameterDefinitions[P].definition: CHUNK[INT] range expression 1-20 chunks of 1 -> INT range expression 1-20 chunks of 1  [job]"
    );
}

// ─── diff gallery audit round 3: rendering ──────────────────────────

/// A job whose only parameter `X` has type `ty` and the value `v`.
fn typed_param_job(ty: &str, v: &str) -> TemplateInput {
    let t = format!(
        "\
specificationVersion: jobtemplate-2023-09
name: J
parameterDefinitions:
  - {{name: X, type: {ty}}}
steps:
  - name: S
    script: {{actions: {{onRun: {{command: a}}}}}}
"
    );
    text(&t).param("X", v)
}

#[test]
fn a_task_parameter_differing_past_the_cut_shows_where_it_differs() {
    let q = "q".repeat(150);
    let d = diff_templates(
        &text(&one_param_job(&format!(
            "{{name: P, type: STRING, range: [{q}1]}}"
        ))),
        &text(&one_param_job(&format!(
            "{{name: P, type: STRING, range: [{q}2]}}"
        ))),
        &opts(),
    )
    .unwrap();
    let (head, tail) = ("q".repeat(19), "q".repeat(40));
    assert_eq!(
        lines(&d.changes),
        [format!(
            "~ steps[S].parameterSpace.taskParameterDefinitions[P].definition: [{head}… …{tail}1] -> [{head}… …{tail}2]  [template]"
        )]
    );
}

#[test]
fn an_unexpanded_parameter_whose_type_changes_shows_its_value_and_type() {
    let d = diff_templates(
        &typed_param_job("STRING", "//x/1"),
        &typed_param_job("PATH", "//x/2"),
        &opts().with_max_depth(1),
    )
    .unwrap();
    assert_eq!(
        d.to_string(),
        "\
~ parameter definition X.type: STRING -> PATH  [template]
~ parameter X: \"//x/1\" (STRING) -> \"//x/2\" (PATH)  [both]

0 steps changed. Tasks: 1 kept, 0 added, 0 removed."
    );
}

#[test]
fn a_long_value_whose_type_changes_is_cut_short_enough_to_keep_its_type() {
    let long = "a".repeat(200);
    let d = diff_templates(
        &typed_param_job("STRING", &format!("//sr1/{long}")),
        &typed_param_job("PATH", &format!("//sr2/{long}")),
        &opts(),
    )
    .unwrap();
    let value = d
        .changes
        .iter()
        .find(|c| c.path == "parameters[X]" && c.field == "value")
        .unwrap();
    let (b, a) = (
        value.before.as_ref().unwrap(),
        value.after.as_ref().unwrap(),
    );
    for (s, ty) in [(b, " (STRING)"), (a, " (PATH)")] {
        assert!(s.text.chars().count() <= 120, "{}", s.text);
        assert!(s.text.ends_with(&format!("…{ty}")), "{}", s.text);
    }
    assert!(b.text.starts_with("\"//sr1/aaa"), "{}", b.text);
    assert!(a.text.starts_with("\"//sr2/aaa"), "{}", a.text);
}

#[test]
fn only_string_and_path_values_are_quoted_and_the_type_line_is_folded() {
    let d = diff_templates(
        &typed_param_job("INT", "4"),
        &typed_param_job("FLOAT", "4.5"),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        d.to_string(),
        "\
~ parameter definition X.type: INT -> FLOAT  [template]
~ parameter X: 4 (INT) -> 4.5 (FLOAT)  [parameters]

0 steps changed. Tasks: 1 kept, 0 added, 0 removed."
    );
    // The job layer still reports the type change, as plain type names.
    let ty = d
        .changes
        .iter()
        .find(|c| c.path == "parameters[X]" && c.field == "type")
        .unwrap();
    assert_eq!(
        ty.to_string(),
        "~ parameters[X].type: INT -> FLOAT  [template]"
    );
}

/// The `range: "1-60"` and `range: [1, ..., 60]` pair.
fn sixty_written_two_ways() -> (TemplateInput, TemplateInput) {
    let list = (1..=60)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    (
        text(&one_param_job(r#"{name: P, type: INT, range: "1-60"}"#)),
        text(&one_param_job(&format!(
            "{{name: P, type: INT, range: [{list}]}}"
        ))),
    )
}

const SIXTY_LIST: &str = "list [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31…";

#[test]
fn a_long_range_rewrite_label_is_cut() {
    let (old, new) = sixty_written_two_ways();
    let d = diff_templates(&old, &new, &opts()).unwrap();
    assert_eq!(
        lines(&d.changes),
        [format!("~ steps[S].parameterSpace.taskParameterDefinitions[P].definition: range expression 1-60 -> {SIXTY_LIST}  [template]")]
    );
    assert_eq!(SIXTY_LIST.chars().count(), 120);
}

#[test]
fn an_unexpanded_definition_node_shows_how_each_range_is_written() {
    let (old, new) = sixty_written_two_ways();
    let d = diff_templates(&old, &new, &opts().with_max_depth(3)).unwrap();
    assert_eq!(
        d.to_string(),
        format!(
            "\
~ step S
    ~ P: range expression 1-60 -> {SIXTY_LIST}  [template]
    tasks: 60 unchanged

1 step changed. Tasks: 60 kept, 0 added, 0 removed."
        )
    );
    assert_eq!(d.changes[0].field, "");
}

#[test]
fn diff_modules_carry_the_unstable_banner_and_entry_points_end_with_it() {
    const BANNER: &str = "//! **Unstable.** This API may change or be removed in any release without\n//! notice. Don't depend on it outside openjd-rs.";
    let src = |m: &str| {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("src/job/{m}.rs"));
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
    };
    for m in ["template_diff", "diff", "task_delta", "apply"] {
        let s = src(m);
        let docs: Vec<&str> = s
            .lines()
            .skip_while(|l| !l.starts_with("//!"))
            .take_while(|l| l.starts_with("//!"))
            .collect();
        let docs = docs.join("\n");
        // The banner is the second paragraph: rustdoc lists the first.
        let paragraphs: Vec<&str> = docs.split("\n//!\n").collect();
        assert_eq!(paragraphs.get(1), Some(&BANNER), "{m}");
    }
    let s = src("template_diff");
    let lines: Vec<&str> = s.lines().collect();
    let mut entry_points = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if let Some(rest) = line.strip_prefix("pub fn ") {
            let last_doc = lines[..i].iter().rev().find(|l| !l.starts_with("#["));
            assert_eq!(
                last_doc,
                Some(&"/// Unstable: see the module docs."),
                "{line}"
            );
            entry_points.push(rest.split('(').next().unwrap_or_default());
        }
    }
    assert_eq!(
        entry_points,
        [
            "diff_templates",
            "diff_job_template_with_parameter_sets",
            "diff_built_jobs",
            "job_from_str"
        ]
    );
}

#[test]
fn a_type_change_with_the_same_value_keeps_its_type_line() {
    let d = diff_templates(
        &typed_param_job("STRING", "//x/1"),
        &typed_param_job("PATH", "//x/1"),
        &opts(),
    )
    .unwrap();
    assert_eq!(
        d.to_string(),
        "\
~ parameter definition X.type: STRING -> PATH  [template]
~ parameter X.type: STRING -> PATH  [template]

0 steps changed. Tasks: 1 kept, 0 added, 0 removed."
    );
}

// ─── diff gallery audit round 4 ─────────────────────────────────────

/// A template with a job parameter, a job environment, and a step with
/// host requirements, a parameter space and an embedded file. Each `@NAME@`
/// is filled from one side of a [`SWEEP_PAIRS`] entry or its default.
const SWEEP_TEMPLATE: &str = "\
specificationVersion: jobtemplate-2023-09
extensions: [EXPR, TASK_CHUNKING]
name: J
parameterDefinitions:
  - {name: Count, type: @COUNT_TYPE@, default: @COUNT@}
  - {name: Tiles, type: \"LIST[LIST[INT]]\", default: @TILES@}
jobEnvironments:
  - name: Tools
    variables: {TOOL: \"@TOOL@\"}
steps:
  - name: S
    hostRequirements:
      attributes: [{name: attr.worker.os.family, anyOf: [linux]}]
    parameterSpace:
      taskParameterDefinitions:
        - {name: F, type: INT, range: @F@}
        - {name: C, type: \"CHUNK[INT]\", range: @C@, chunks: {defaultTaskCount: 1, rangeConstraint: CONTIGUOUS}}
        - {name: E, type: @E_TYPE@, range: @E@}
    script:
      actions: {onRun: {command: a, args: [\"{{Task.File.Run}}\", \"{{Task.Param.F}}\", \"{{Task.Param.C}}\", \"{{Task.Param.E}}\"]}}
      embeddedFiles:
        - {name: Run, type: TEXT, data: \"@DATA@\"}
";

/// The placeholders of [`SWEEP_TEMPLATE`] and their unchanged values.
const SWEEP_DEFAULTS: &[(&str, &str)] = &[
    ("@COUNT_TYPE@", "STRING"),
    ("@COUNT@", "\"5\""),
    ("@TILES@", "[[1, 2]]"),
    ("@TOOL@", "t"),
    ("@F@", "\"1-2\""),
    ("@C@", "\"1-2\""),
    ("@E_TYPE@", "STRING"),
    ("@E@", "[a, b]"),
    ("@DATA@", "d"),
];

/// One placeholder of [`SWEEP_TEMPLATE`], with its old and new value.
type SweepEdit = (&'static str, String, String);

/// Change pairs, each named and made of one or more edits.
fn sweep_pairs() -> Vec<(&'static str, Vec<SweepEdit>)> {
    let long = |end: &str| format!("{}{end}", "p".repeat(150));
    let s = |v: &str| v.to_string();
    vec![
        ("range rewrite", vec![("@F@", s("\"1-2\""), s("[1, 2]"))]),
        ("chunk rewrite", vec![("@C@", s("\"1-2\""), s("[1, 2]"))]),
        (
            "type change",
            vec![
                ("@E_TYPE@", s("STRING"), s("PATH")),
                ("@COUNT_TYPE@", s("STRING"), s("INT")),
                ("@COUNT@", s("\"5\""), s("5")),
            ],
        ),
        (
            "long-prefix string",
            vec![
                ("@TOOL@", long("1"), long("2")),
                ("@DATA@", long("1"), long("2")),
                (
                    "@E@",
                    format!("[{}]", long("1")),
                    format!("[{}]", long("2")),
                ),
            ],
        ),
        (
            "list element",
            vec![
                ("@E@", s("[a, b]"), s("[a, c]")),
                ("@TILES@", s("[[1, 2]]"), s("[[1, 3]]")),
            ],
        ),
    ]
}

fn sweep_template(edits: &[(&str, &str)]) -> String {
    SWEEP_DEFAULTS
        .iter()
        .fold(SWEEP_TEMPLATE.to_string(), |t, (k, default)| {
            let v = edits
                .iter()
                .find(|(e, _)| e == k)
                .map_or(*default, |(_, v)| v);
            t.replace(k, v)
        })
}

/// Text that only a value's serde form, never a rendering for people,
/// contains, as `render` writes it (keys unquoted), plus the JSON form's
/// tags.
pub(crate) const SERDE_INTERNALS: [&str; 15] = [
    "{String:",
    "{Int:",
    "{Float:",
    "{Bool:",
    "{ListInt",
    "{ListString",
    "{ListFloat",
    "{ListList",
    "{RangeExpr",
    "cumulative_lengths",
    "paramType",
    "resolvedSymTab: []",
    "\"rangeExpr\"",
    "\"chunkInt\"",
    "{\"String\"",
];

/// Each [`SERDE_INTERNALS`] pattern matches text that a whole node showed
/// before job parameters were written as their parameter lines, so the
/// sweep below can fail.
#[test]
fn every_serde_internal_pattern_can_match() {
    let samples = [
        "{name: Demo, parameters: {Frames: {name: Frames, paramType: String, value: {String: 1-100}}}}",
        "{N: {name: N, paramType: Int, value: {Int: 4}}, X: {name: X, paramType: Float, value: {Float: 4.5}}}",
        "{B: {name: B, paramType: Bool, value: {Bool: true}}}",
        "{value: {ListInt: [1, 2]}}, {value: {ListString: [[a], 48]}}, {value: {ListFloat: [1.5]}}",
        "{value: {ListList: [[{ListInt: [1, 2]}], LIST[INT], 72]}}",
        "{value: {RangeExpr: {cumulative_lengths: [1], ranges: [{start: 1}]}}}",
        "steps: [{name: S, resolvedSymTab: [], script: {}}]",
        "{\"definition\":{\"rangeExpr\":\"1-2\"}}, {\"chunkInt\":{}}, {\"String\":\"a\"}",
    ];
    for bad in SERDE_INTERNALS {
        assert!(samples.iter().any(|s| s.contains(bad)), "{bad}");
    }
}

#[test]
fn whole_nodes_at_every_depth_show_no_serde_internals() {
    let mut checked = 0;
    for (what, pair) in sweep_pairs() {
        let old: Vec<(&str, &str)> = pair.iter().map(|(k, o, _)| (*k, o.as_str())).collect();
        let new: Vec<(&str, &str)> = pair.iter().map(|(k, _, n)| (*k, n.as_str())).collect();
        for depth in [None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(5)] {
            let options = depth.map_or_else(opts, |d| opts().with_max_depth(d));
            let d = diff_templates(
                &text(&sweep_template(&old)),
                &text(&sweep_template(&new)),
                &options,
            )
            .unwrap_or_else(|e| panic!("{what}, depth {depth:?}: {e}"));
            assert!(d.is_changed(), "{what}, depth {depth:?}");
            for c in &d.changes {
                let sides = [c.before.as_ref(), c.after.as_ref()];
                for s in sides.into_iter().flatten() {
                    for bad in SERDE_INTERNALS {
                        assert!(!s.text.contains(bad), "{what}, depth {depth:?}: {c}");
                    }
                    checked += 1;
                }
                if let [Some(b), Some(a)] = sides {
                    if b.raw != a.raw {
                        assert_ne!(b.text, a.text, "{what}, depth {depth:?}: {c}");
                    }
                }
            }
        }
    }
    assert!(checked > 100, "{checked}");
}

#[test]
fn a_list_list_int_change_at_depth_0_shows_its_values() {
    let pair = |tiles: &str| text(&sweep_template(&[("@TILES@", tiles)]));
    let d = diff_templates(
        &pair("[[1, 2]]"),
        &pair("[[1, 3]]"),
        &opts().with_max_depth(0),
    )
    .unwrap();
    let root = d
        .changes
        .iter()
        .find(|c| c.path.is_empty() && c.layer == Layer::Job)
        .unwrap();
    let (b, a) = (root.before.as_ref().unwrap(), root.after.as_ref().unwrap());
    assert!(
        b.text.contains("Tiles: [[1, 2]] (LIST[LIST[INT]])"),
        "{}",
        b.text
    );
    assert!(
        a.text.contains("Tiles: [[1, 3]] (LIST[LIST[INT]])"),
        "{}",
        a.text
    );
}

// ─── diff gallery audit round 5 ─────────────────────────────────────

/// A step whose args and embedded file `data` are filled in by the caller.
fn strings_template(args: &str, data: &str) -> TemplateInput {
    text(&format!(
        "specificationVersion: jobtemplate-2023-09
name: J
steps:
  - name: S
    script:
      actions: {{onRun: {{command: a, args: {args}}}}}
      embeddedFiles:
        - name: Run
          type: TEXT
          data: {data}
"
    ))
}

/// The text of the one change at `field` under step `S`.
fn change_line(d: &TemplateDiff, field: &str) -> String {
    let c = d
        .changes
        .iter()
        .find(|c| c.field == field || c.path.ends_with(field))
        .unwrap_or_else(|| panic!("no {field} in\n{d}"));
    let (b, a) = (c.before.as_ref().unwrap(), c.after.as_ref().unwrap());
    format!("{b} -> {a}")
}

#[test]
fn a_multi_line_data_change_prints_on_one_line() {
    let d = diff_templates(
        &strings_template("[x]", "|\n            set -e\n            render --fast"),
        &strings_template("[x]", "|\n            set -e\n            render --slow"),
        &opts(),
    )
    .unwrap();
    let line = d
        .to_string()
        .lines()
        .find(|l| l.contains(".data:"))
        .unwrap()
        .to_string();
    assert_eq!(
        line,
        "    ~ script.embeddedFiles[Run].data: \"set -e\\nrender --fast\\n\" -> \"set -e\\nrender --slow\\n\"  [template]"
    );
}

#[test]
fn a_carriage_return_is_shown() {
    let d = diff_templates(
        &strings_template("[\"a\\nb\"]", "d"),
        &strings_template("[\"a\\r\\nb\"]", "d"),
        &opts(),
    )
    .unwrap();
    assert_eq!(change_line(&d, "args"), "[a\\nb] -> [a\\r\\nb]");
}

#[test]
fn an_ambiguous_string_is_quoted() {
    let pair = |b: &str, a: &str| {
        let d = diff_templates(
            &strings_template("[x]", b),
            &strings_template("[x]", a),
            &opts(),
        )
        .unwrap();
        change_line(&d, "data")
    };
    assert_eq!(pair("a", "\"a -> b\""), "a -> \"a -> b\"");
    assert_eq!(pair("\" a\"", "a"), "\" a\" -> a");
    assert_eq!(pair("\"x \"", "\" \""), "\"x \" -> \" \"");
}

#[test]
fn a_whole_job_writes_each_parameter_as_its_line_with_its_type() {
    let pair = |frames: &str| {
        text(&format!(
            "specificationVersion: jobtemplate-2023-09
name: Demo
parameterDefinitions:
  - {{name: Frames, type: STRING, default: \"{frames}\"}}
  - {{name: N, type: INT, default: 4}}
steps:
  - name: Render
    parameterSpace:
      taskParameterDefinitions:
        - {{name: Frame, type: INT, range: \"{{{{Param.Frames}}}}\"}}
    script: {{actions: {{onRun: {{command: render}}}}}}
"
        ))
    };
    let d = diff_templates(&pair("1-100"), &pair("1-200"), &opts().with_max_depth(0)).unwrap();
    let root = d
        .changes
        .iter()
        .find(|c| c.path.is_empty() && c.layer == Layer::Job)
        .unwrap();
    let (b, a) = (root.before.as_ref().unwrap(), root.after.as_ref().unwrap());
    assert_eq!(
        b.text,
        "{name: Demo, parameters: {Frames: 1-100 (STRING), N: 4 (INT)}, steps: [{name: Render, parameterSpace: {Frame: 1-100}, s…"
    );
    assert!(a.text.contains("Frames: 1-200 (STRING)"), "{}", a.text);
    for t in [&b.text, &a.text] {
        assert!(!t.contains("resolvedSymTab"), "{t}");
    }
}
