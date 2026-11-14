// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! `openjd diff`, run as a binary on the samples in
//! `crates/openjd-model/examples/template_diff/` and on the multi-step
//! fixtures `tests/templates/diff_multi_v*.yaml`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn samples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../openjd-model/examples/template_diff")
}

fn templates() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/templates")
}

/// Run `openjd` in `dir`, with `stdin` piped in.
fn openjd_in(dir: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_openjd"))
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut pipe = child.stdin.take().unwrap();
    pipe.write_all(stdin.unwrap_or("").as_bytes()).unwrap();
    drop(pipe);
    child.wait_with_output().unwrap()
}

/// Run `openjd` in the samples directory, with `stdin` piped in.
fn openjd(args: &[&str], stdin: Option<&str>) -> Output {
    openjd_in(&samples(), args, stdin)
}

fn stdout(o: &Output) -> String {
    program_name_as_on_unix(String::from_utf8(o.stdout.clone()).unwrap())
}

fn stderr(o: &Output) -> String {
    program_name_as_on_unix(String::from_utf8(o.stderr.clone()).unwrap())
}

/// clap names the program by its file name, which is `openjd.exe` on Windows.
/// Only the `Usage:` lines carry it, so only those are rewritten.
fn program_name_as_on_unix(text: String) -> String {
    text.replace("Usage: openjd.exe ", "Usage: openjd ")
}

/// The line `openjd diff` prints to stderr before anything else.
const DIFF_WARNING: &str = "warning: 'openjd diff' is unstable; its flags and output may change.\n";

fn assert_ok(o: &Output, expected: &str) {
    assert_eq!(stderr(o), DIFF_WARNING);
    assert_eq!(stdout(o), expected);
    assert_eq!(o.status.code(), Some(0));
}

const CASE3: &[&str] = &[
    "diff",
    "render_v1.yaml",
    "render_v2.yaml",
    "--new-parameter",
    "Frames=1-200",
    "-p",
    "Scene=/s",
];

const V1_V2: &str = "render_v1.yaml -> render_v2.yaml\n";

const CASE3_OUT: &str = "\
~ parameter Frames: 1-100 -> 1-200  [parameters]
~ step Render
    ~ Frame: 1-100 -> 1-200  [parameters]
    ~ Eye: [left] -> [left, right]  [template]
    tasks: 100 -> 400
      keep   100   Frame 1-100 × Eye left
      add    200   Frame 101-200 × Eye left, right
      add    100   Frame 1-100 × Eye right

1 step changed. Tasks: 100 kept, 300 added, 0 removed.
";

fn with(base: &[&'static str], extra: &[&'static str]) -> Vec<&'static str> {
    base.iter().chain(extra).copied().collect()
}

#[test]
fn two_files() {
    let o = openjd(
        &["diff", "render_v1.yaml", "render_v2.yaml", "-p", "Scene=/s"],
        None,
    );
    assert_ok(
        &o,
        "\
render_v1.yaml -> render_v2.yaml
~ step Render
    ~ Eye: [left] -> [left, right]  [template]
    tasks: 100 -> 200
      keep   100   Frame 1-100 × Eye left
      add    100   Frame 1-100 × Eye right

1 step changed. Tasks: 100 kept, 100 added, 0 removed.
",
    );
}

#[test]
fn one_file_with_old_and_new_params() {
    let o = openjd(
        &[
            "diff",
            "render_v1.yaml",
            "--old-parameter",
            "Frames=1-100",
            "--new-parameter",
            r#"{"Frames": "1-200"}"#,
            "-p",
            "Scene=/s",
        ],
        None,
    );
    assert_ok(
        &o,
        "\
render_v1.yaml: old parameters -> new parameters
~ parameter Frames: 1-100 -> 1-200  [parameters]
~ step Render
    ~ Frame: 1-100 -> 1-200  [parameters]
    tasks: 100 -> 200
      keep   100   Frame 1-100 × Eye left
      add    100   Frame 101-200 × Eye left

1 step changed. Tasks: 100 kept, 100 added, 0 removed.
",
    );
}

#[test]
fn both_changes() {
    assert_ok(&openjd(CASE3, None), &format!("{V1_V2}{CASE3_OUT}"));
}

#[test]
fn stat() {
    assert_ok(
        &openjd(&with(CASE3, &["--stat"]), None),
        &format!("{V1_V2}~ Render  tasks 100 -> 400  (+300 -0)\n1 step changed. Tasks: 100 kept, 300 added, 0 removed.\n"),
    );
}

#[test]
fn tasks() {
    assert_ok(
        &openjd(&with(CASE3, &["--tasks"]), None),
        "\
render_v1.yaml -> render_v2.yaml
~ step Render
    tasks: 100 -> 400
      keep   100   Frame 1-100 × Eye left
      add    200   Frame 101-200 × Eye left, right
      add    100   Frame 1-100 × Eye right
",
    );
}

#[test]
fn plan() {
    let o = openjd(&with(CASE3, &["--plan"]), None);
    assert_ok(
        &o,
        &format!(
            "{V1_V2}{CASE3_OUT}
apply plan:
  1. parameter modify Frames  [ParameterValue]
  2. tasks of Render: keep 100, add 300, remove 0  [TaskAppend]
kinds: ParameterValue, TaskAppend
"
        ),
    );
}

#[test]
fn step_filter() {
    let o = openjd(&with(CASE3, &["--step", "Render", "--stat"]), None);
    assert_ok(
        &o,
        &format!("{V1_V2}~ Render  tasks 100 -> 400  (+300 -0)\n1 step changed. Tasks: 100 kept, 300 added, 0 removed.\n"),
    );
    let o = openjd(&with(CASE3, &["--step", "Nope"]), None);
    assert_eq!(
        stderr(&o),
        format!("{DIFF_WARNING}ERROR: Step 'Nope' does not exist in either job.\n")
    );
    assert_eq!(o.status.code(), Some(1));
}

#[test]
fn output_json_is_one_document() {
    let o = openjd(&with(CASE3, &["--output", "json", "--plan"]), None);
    assert_eq!(o.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["changed", "changes", "notes", "plan", "steps"]);
    assert_eq!(v["changed"], true);
    assert_eq!(v["changes"].as_array().unwrap().len(), 3);
    assert_eq!(v["changes"][1]["origin"], "template");
    assert_eq!(
        v["plan"]["kinds"],
        serde_json::json!(["ParameterValue", "TaskAppend"])
    );

    let o = openjd(&with(CASE3, &["--output", "json", "--verbose"]), None);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        ["changed", "changes", "job", "newJob", "notes", "oldJob", "steps"]
    );
    // The document is written in field order, `changed` first.
    assert!(stdout(&o).starts_with("{\n  \"changed\": true,\n  \"changes\": ["));

    let o = openjd(&with(CASE3, &["--output", "yaml"]), None);
    assert_eq!(o.status.code(), Some(0));
    assert!(
        stdout(&o).starts_with("changed: true\nchanges:\n"),
        "{}",
        stdout(&o)
    );

    let o = openjd(
        &[
            "diff",
            "render_v1.yaml",
            "render_v1.yaml",
            "-p",
            "Scene=/s",
            "--output",
            "json",
        ],
        None,
    );
    assert_eq!(
        stdout(&o),
        "{\n  \"changed\": false,\n  \"changes\": [],\n  \"steps\": [\n    {\n      \"name\": \"Render\",\n      \"status\": \"unchanged\",\n      \"oldIndex\": 0,\n      \"newIndex\": 0,\n      \"tasks\": {\n        \"kind\": \"unchanged\",\n        \"count\": 100\n      }\n    }\n  ],\n  \"notes\": []\n}\n"
    );
}

#[test]
fn exit_code() {
    let same = openjd(
        &[
            "diff",
            "render_v1.yaml",
            "render_v1.yaml",
            "-p",
            "Scene=/s",
            "--exit-code",
        ],
        None,
    );
    assert_eq!(
        (stdout(&same).as_str(), same.status.code()),
        ("render_v1.yaml -> render_v1.yaml\nno changes\n", Some(0))
    );
    let differ = openjd(&with(CASE3, &["--exit-code", "--stat"]), None);
    assert_eq!(differ.status.code(), Some(4));
    let error = openjd(
        &["diff", "render_v1.yaml", "render_v2.yaml", "--exit-code"],
        None,
    );
    assert_eq!(
        stderr(&error),
        format!("{DIFF_WARNING}ERROR: Validation error: old template: Values missing for required job parameters: Scene\n")
    );
    assert_eq!(error.status.code(), Some(1));
    // Without the flag, differences still exit 0.
    assert_eq!(openjd(CASE3, None).status.code(), Some(0));
}

#[test]
fn stdin() {
    let v2 = std::fs::read_to_string(samples().join("render_v2.yaml")).unwrap();
    let o = openjd(
        &["diff", "render_v1.yaml", "-", "-p", "Scene=/s", "--stat"],
        Some(&v2),
    );
    assert_ok(
        &o,
        "render_v1.yaml -> -\n~ Render  tasks 100 -> 200  (+100 -0)\n1 step changed. Tasks: 100 kept, 100 added, 0 removed.\n",
    );
    let o = openjd(&["diff", "-", "-"], Some(&v2));
    assert_eq!(
        stderr(&o),
        format!("{DIFF_WARNING}ERROR: Only one input can be read from stdin ('-').\n")
    );
    assert_eq!(o.status.code(), Some(1));
}

/// An environment template from stdin is read once and used by both sides.
#[test]
fn environment_from_stdin_with_two_templates() {
    let env = "\
specificationVersion: environment-2023-09
parameterDefinitions:
  - {name: Tool, type: STRING, default: t1}
environment:
  name: Env
  variables: {TOOL: \"{{Param.Tool}}\"}
";
    let o = openjd(
        &[
            "diff",
            "render_v1.yaml",
            "render_v2.yaml",
            "-p",
            "Scene=/s",
            "--environment",
            "-",
            "--new-parameter",
            "Tool=t2",
            "--stat",
        ],
        Some(env),
    );
    assert_ok(
        &o,
        &format!("{V1_V2}~ Render  tasks 100 -> 200  (+100 -0)\n1 step changed. Tasks: 100 kept, 100 added, 0 removed.\n"),
    );
}

#[test]
fn missing_file_names_the_path() {
    let o = openjd(&["diff", "nope.yaml", "render_v1.yaml"], None);
    assert_eq!(
        stderr(&o),
        format!("{DIFF_WARNING}ERROR: 'nope.yaml' does not exist.\n")
    );
    assert_eq!(stdout(&o), "");
    assert_eq!(o.status.code(), Some(1));
    let o = openjd(&["diff", "render_v1.yaml", "nope.yaml"], None);
    assert_eq!(
        stderr(&o),
        format!("{DIFF_WARNING}ERROR: 'nope.yaml' does not exist.\n")
    );
    assert_eq!(stdout(&o), "");
    assert_eq!(o.status.code(), Some(1));
}

#[test]
fn errors_name_the_parameter_set_or_the_template() {
    let o = openjd(
        &["diff", "render_v1.yaml", "--old-parameter", "Scene=/s"],
        None,
    );
    assert_eq!(
        stderr(&o),
        format!("{DIFF_WARNING}ERROR: Validation error: new parameters: Values missing for required job parameters: Scene\n")
    );
    assert_eq!(o.status.code(), Some(1));
    let o = openjd(
        &["diff", "render_v1.yaml", "--new-parameter", "Scene=/s"],
        None,
    );
    assert_eq!(
        stderr(&o),
        format!("{DIFF_WARNING}ERROR: Validation error: old parameters: Values missing for required job parameters: Scene\n")
    );
    let o = openjd(
        &[
            "diff",
            "render_v1.yaml",
            "render_v2.yaml",
            "--old-parameter",
            "Scene=/s",
        ],
        None,
    );
    assert_eq!(
        stderr(&o),
        format!("{DIFF_WARNING}ERROR: Validation error: new template: Values missing for required job parameters: Scene\n")
    );
}

#[test]
fn ignore_symbols_hides_resolved_symbol_tables() {
    let args = [
        "diff",
        "render_v1.yaml",
        "--old-parameter",
        "Scene=//srv/a",
        "--new-parameter",
        "Scene=//srv/b",
    ];
    assert_ok(
        &openjd(&args, None),
        "\
render_v1.yaml: old parameters -> new parameters
~ parameter Scene: //srv/a -> //srv/b  [parameters]
~ step Render
    ~ resolvedSymTab: RawParam.Scene=//srv/a -> RawParam.Scene=//srv/b  [parameters]
    tasks: 100 unchanged

1 step changed. Tasks: 100 kept, 0 added, 0 removed.
",
    );
    assert_ok(
        &openjd(&with(&args, &["--ignore-symbols"]), None),
        "\
render_v1.yaml: old parameters -> new parameters
~ parameter Scene: //srv/a -> //srv/b  [parameters]

0 steps changed. Tasks: 100 kept, 0 added, 0 removed.
",
    );
}

#[test]
fn verbose_human_output_appends_the_job_diff_tree() {
    let o = openjd(
        &[
            "diff",
            "render_v1.yaml",
            "--old-parameter",
            "Scene=//srv/a",
            "--new-parameter",
            "Scene=//srv/b",
            "--verbose",
        ],
        None,
    );
    assert_ok(
        &o,
        r#"render_v1.yaml: old parameters -> new parameters
~ parameter Scene: //srv/a -> //srv/b  [parameters]
~ step Render
    ~ resolvedSymTab: RawParam.Scene=//srv/a -> RawParam.Scene=//srv/b  [parameters]
    tasks: 100 unchanged

1 step changed. Tasks: 100 kept, 0 added, 0 removed.

job diff tree:
~ <root>
  ~ parameters[Scene]
      value: "//srv/a" -> "//srv/b"
  ~ steps[Render]
      resolvedSymTab: [{"name":"RawParam.Scene","type":"string","value":"//srv/a"}] -> [{"name":"RawParam.Scene","type":"string","value":"//srv/b"}]
"#,
    );
    let o = openjd(
        &[
            "diff",
            "render_v1.yaml",
            "render_v1.yaml",
            "-p",
            "Scene=/s",
            "--verbose",
        ],
        None,
    );
    assert_ok(
        &o,
        "render_v1.yaml -> render_v1.yaml\nno changes\n\njob diff tree:\nno changes\n",
    );
}

// ─── multi-step fixture ─────────────────────────────────────────────

const MULTI: &[&str] = &["diff", "diff_multi_v1.yaml", "diff_multi_v2.yaml"];
const MULTI_HEADER: &str = "diff_multi_v1.yaml -> diff_multi_v2.yaml\n";

fn multi(extra: &[&'static str]) -> Output {
    openjd_in(&templates(), &with(MULTI, extra), None)
}

const MULTI_JOB_LEVEL: &str = "\
~ description: first -> second  [template]
~ jobEnvironments[Tools].variables.TOOL: v1 -> v2  [template]
";

#[test]
fn multi_step_full_output() {
    assert_ok(
        &multi(&[]),
        &format!(
            "{MULTI_HEADER}{MULTI_JOB_LEVEL}\
~ step Shrink
    ~ N: 1-6 -> 1-3  [template]
    tasks: 6 -> 3
      keep   3     N 1-3
      remove 3     N 4-6
~ step Modify
    ~ script.onRun.command: old -> new  [template]
    tasks: 1 unchanged
~ step LetOnly
    ~ let: [unused = 1] -> [unused = 2]  [template]
    tasks: 1 unchanged
+ step Added
    tasks: 2 added

3 steps changed, 1 added. Tasks: 6 kept, 2 added, 3 removed.
"
        ),
    );
}

#[test]
fn multi_step_stat_and_tasks() {
    assert_ok(
        &multi(&["--stat"]),
        &format!(
            "{MULTI_HEADER}\
~ Shrink  tasks 6 -> 3  (+0 -3)
~ Modify  tasks 1 -> 1  (+0 -0)
~ LetOnly  tasks 1 -> 1  (+0 -0)
+ Added  tasks 0 -> 2  (+2 -0)
3 steps changed, 1 added. Tasks: 6 kept, 2 added, 3 removed.
"
        ),
    );
    assert_ok(
        &multi(&["--tasks"]),
        &format!(
            "{MULTI_HEADER}\
~ step Shrink
    tasks: 6 -> 3
      keep   3     N 1-3
      remove 3     N 4-6
~ step Modify
    tasks: 1 unchanged
~ step LetOnly
    tasks: 1 unchanged
+ step Added
    tasks: 2 added
"
        ),
    );
}

#[test]
fn multi_step_plan_has_every_op_kind() {
    let o = multi(&["--plan", "--stat"]);
    assert_ok(
        &o,
        &format!(
            "{MULTI_HEADER}\
~ Shrink  tasks 6 -> 3  (+0 -3)
~ Modify  tasks 1 -> 1  (+0 -0)
~ LetOnly  tasks 1 -> 1  (+0 -0)
+ Added  tasks 0 -> 2  (+2 -0)
3 steps changed, 1 added. Tasks: 6 kept, 2 added, 3 removed.

apply plan:
  1. job modify  [JobMetadata]
  2. job environment modify Tools  [EnvVariables]
  3. tasks of Shrink: keep 3, add 0, remove 3  [TaskRemove]
  4. step modify Modify  [StepScript]
  5. step add Added  [StepAdd]
  6. tasks of Added: all  [TaskAppend]
kinds: JobMetadata, EnvVariables, StepScript, StepAdd, TaskAppend, TaskRemove
"
        ),
    );
    // Reversed: a step removed, tasks appended.
    let o = openjd_in(
        &templates(),
        &[
            "diff",
            "diff_multi_v2.yaml",
            "diff_multi_v1.yaml",
            "--plan",
            "--stat",
        ],
        None,
    );
    let out = stdout(&o);
    let plan = out.split("apply plan:\n").nth(1).unwrap();
    assert_eq!(
        plan,
        "  1. job modify  [JobMetadata]
  2. job environment modify Tools  [EnvVariables]
  3. tasks of Shrink: keep 3, add 3, remove 0  [TaskAppend]
  4. step modify Modify  [StepScript]
  5. step remove Added  [StepRemove]
kinds: JobMetadata, EnvVariables, StepScript, StepRemove, TaskAppend
"
    );
}

#[test]
fn step_keeps_job_level_changes_and_its_own_plan_ops() {
    assert_ok(
        &multi(&["--step", "Modify", "--plan"]),
        &format!(
            "{MULTI_HEADER}{MULTI_JOB_LEVEL}\
~ step Modify
    ~ script.onRun.command: old -> new  [template]
    tasks: 1 unchanged

1 step changed. Tasks: 1 kept, 0 added, 0 removed.

apply plan:
  1. job modify  [JobMetadata]
  2. job environment modify Tools  [EnvVariables]
  3. step modify Modify  [StepScript]
kinds: JobMetadata, EnvVariables, StepScript
"
        ),
    );
    let o = multi(&["--step", "Shrink", "--plan", "--output", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(
        v["plan"],
        serde_json::json!({
            "ops": [
                {"action": "job modify", "kinds": ["JobMetadata"]},
                {"action": "job environment modify Tools", "kinds": ["EnvVariables"]},
                {"action": "tasks of Shrink: keep 3, add 0, remove 3", "kinds": ["TaskRemove"]},
            ],
            "kinds": ["JobMetadata", "EnvVariables", "TaskRemove"],
        })
    );
    let paths: Vec<&str> = v["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        [
            "",
            "jobEnvironments[Tools]",
            "steps[Shrink].parameterSpace.taskParameterDefinitions[N]"
        ]
    );
    let steps: Vec<&str> = v["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(steps, ["Shrink"]);
}

/// Only job-level changes: `--stat` and `--tasks` say so rather than
/// printing `no changes`.
#[test]
fn job_level_changes_only() {
    for (extra, body) in [
        (
            &["--step", "Same", "--stat"][..],
            "job-level changes only\n0 steps changed. Tasks: 1 kept, 0 added, 0 removed.\n",
        ),
        (
            &["--step", "Same", "--tasks"][..],
            "job-level changes only\n",
        ),
    ] {
        let mut args = MULTI.to_vec();
        args.extend(extra);
        assert_ok(
            &openjd_in(&templates(), &args, None),
            &format!("{MULTI_HEADER}{body}"),
        );
    }
    let o = openjd_in(
        &templates(),
        &[
            "diff",
            "wrap_simple_job.yaml",
            "wrap_job_with_inner_env.yaml",
            "--stat",
            "--exit-code",
        ],
        None,
    );
    assert_eq!(
        stdout(&o),
        "wrap_simple_job.yaml -> wrap_job_with_inner_env.yaml\njob-level changes only\n0 steps changed. Tasks: 1 kept, 0 added, 0 removed.\n"
    );
    assert_eq!(o.status.code(), Some(4));
}

// ─── audit round 2 ──────────────────────────────────────────────────

fn quality_template() -> String {
    std::fs::read_to_string(samples().join("render_v1.yaml"))
        .unwrap()
        .replace(
            "  - {name: Scene, type: PATH}",
            "  - {name: Scene, type: PATH}\n  - {name: Quality, type: INT}",
        )
}

const MID_NOTE: &str = "note: origin: the new template does not build with the old parameters, so job-layer changes are marked both: Validation error: Values missing for required job parameters: Quality\n";

#[test]
fn human_output_prints_the_notes() {
    let args = [
        "diff",
        "render_v1.yaml",
        "-",
        "-p",
        "Scene=/s",
        "--new-parameter",
        "Quality=3",
    ];
    let o = openjd(&args, Some(&quality_template()));
    assert_ok(
        &o,
        &format!(
            "render_v1.yaml -> -\n+ parameter Quality: 3, defined as {{type: INT}}  [both]\n\n0 steps changed. Tasks: 100 kept, 0 added, 0 removed.\n{MID_NOTE}"
        ),
    );
    let o = openjd(&with(&args, &["--stat"]), Some(&quality_template()));
    assert_ok(
        &o,
        &format!(
            "render_v1.yaml -> -\njob-level changes only\n0 steps changed. Tasks: 100 kept, 0 added, 0 removed.\n{MID_NOTE}"
        ),
    );
}

#[test]
fn task_parameter_reorder_at_depth_1() {
    // A CRLF checkout must still match the LF patterns below.
    let swapped = std::fs::read_to_string(samples().join("render_v1.yaml"))
        .unwrap()
        .replace("\r\n", "\n")
        .replace(
            "        - {name: Frame, type: INT, range: \"{{Param.Frames}}\"}\n        - {name: Eye, type: STRING, range: [left]}",
            "        - {name: Eye, type: STRING, range: [left]}\n        - {name: Frame, type: INT, range: \"{{Param.Frames}}\"}",
        );
    let args = [
        "diff",
        "render_v1.yaml",
        "-",
        "-p",
        "Scene=/s",
        "--depth",
        "1",
    ];
    let o = openjd(&args, Some(&swapped));
    assert_ok(
        &o,
        "\
render_v1.yaml -> -
~ step Render
    ~ (not expanded)  [template]
    tasks: 100 -> 100, all replaced (task parameter definition order changed, so task order changed)

1 step changed. Tasks: 0 kept, 0 added, 0 removed, 100 replaced.
",
    );
    let o = openjd(&with(&args, &["--output", "json"]), Some(&swapped));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let c = &v["changes"][0];
    let names = |side: &str| -> Vec<String> {
        c[side]["raw"]["parameterSpace"]["taskParameterDefinitions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["name"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(names("before"), ["Frame", "Eye"]);
    assert_eq!(names("after"), ["Eye", "Frame"]);
    assert_ne!(c["before"]["text"], c["after"]["text"]);
}

// ─── audit round 3 ──────────────────────────────────────────────────

/// `--exit-code` and `changed` count only what `--step` keeps.
#[test]
fn step_filter_decides_exit_code_and_changed() {
    let dir = tempfile::tempdir().unwrap();
    let t = |b: &str| {
        format!(
            "specificationVersion: jobtemplate-2023-09\nname: J\nsteps:\n  - name: A\n    script: {{actions: {{onRun: {{command: a}}}}}}\n  - name: B\n    script: {{actions: {{onRun: {{command: {b}}}}}}}\n"
        )
    };
    std::fs::write(dir.path().join("old.yaml"), t("b1")).unwrap();
    std::fs::write(dir.path().join("new.yaml"), t("b2")).unwrap();
    let base = ["diff", "old.yaml", "new.yaml", "--exit-code"];
    let run = |extra: &[&str]| {
        let mut args = base.to_vec();
        args.extend(extra);
        openjd_in(dir.path(), &args, None)
    };
    assert_ok(&run(&["--step", "A"]), "old.yaml -> new.yaml\nno changes\n");
    let o = run(&["--step", "A", "--output", "json"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["changed"], serde_json::json!(false));
    let o = run(&["--step", "B", "--output", "json"]);
    assert_eq!(o.status.code(), Some(4), "{}", stderr(&o));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["changed"], serde_json::json!(true));
}

// ─── audit round 4 ──────────────────────────────────────────────────

/// Write `old.yaml` and `new.yaml` with steps `Same` and `Modify`; only
/// `Modify`'s command changes.
fn same_modify_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let t = |cmd: &str| {
        format!(
            "specificationVersion: jobtemplate-2023-09\nname: J\nsteps:\n  - name: Same\n    script: {{actions: {{onRun: {{command: s}}}}}}\n  - name: Modify\n    script: {{actions: {{onRun: {{command: {cmd}}}}}}}\n"
        )
    };
    std::fs::write(dir.path().join("old.yaml"), t("m1")).unwrap();
    std::fs::write(dir.path().join("new.yaml"), t("m2")).unwrap();
    dir
}

#[test]
fn step_with_depth_0_is_a_usage_error() {
    let dir = same_modify_dir();
    let base = ["diff", "old.yaml", "new.yaml", "--step", "Same"];
    let run = |extra: &[&str]| {
        let mut args = base.to_vec();
        args.extend(extra);
        openjd_in(dir.path(), &args, None)
    };
    let o = run(&["--depth", "0", "--exit-code"]);
    assert_eq!(
        stderr(&o),
        "error: --step cannot be used with --depth 0: the job root is not expanded, so it cannot be split by step\n"
    );
    assert_eq!(stdout(&o), "");
    assert_eq!(o.status.code(), Some(2));
    assert_ok(
        &run(&["--depth", "1", "--exit-code"]),
        "old.yaml -> new.yaml\nno changes\n",
    );
}

#[test]
fn step_filter_drops_notes_about_other_steps() {
    let dir = tempfile::tempdir().unwrap();
    let t = |cmd: &str| {
        format!(
            "specificationVersion: jobtemplate-2023-09\nextensions: [TASK_CHUNKING]\nname: J\nsteps:\n  - name: Chunked\n    parameterSpace:\n      taskParameterDefinitions:\n        - name: F\n          type: CHUNK[INT]\n          range: \"1-100\"\n          chunks: {{defaultTaskCount: 10, targetRuntimeSeconds: 60, rangeConstraint: CONTIGUOUS}}\n    script: {{actions: {{onRun: {{command: {cmd}, args: [\"{{{{Task.Param.F}}}}\"]}}}}}}\n  - name: Plain\n    script: {{actions: {{onRun: {{command: {cmd}}}}}}}\n"
        )
    };
    std::fs::write(dir.path().join("old.yaml"), t("old")).unwrap();
    std::fs::write(dir.path().join("new.yaml"), t("new")).unwrap();
    let run = |step: &str| {
        openjd_in(
            dir.path(),
            &["diff", "old.yaml", "new.yaml", "--step", step],
            None,
        )
    };
    assert_ok(
        &run("Plain"),
        "\
old.yaml -> new.yaml
~ step Plain
    ~ script.onRun.command: old -> new  [template]
    tasks: 1 unchanged

1 step changed. Tasks: 1 kept, 0 added, 0 removed.
",
    );
    assert_ok(
        &run("Chunked"),
        "\
old.yaml -> new.yaml
~ step Chunked
    ~ script.onRun.command: old -> new  [template]
    tasks: unknown (adaptive chunking)

1 step changed. Tasks: 0 kept, 0 added, 0 removed.
note: steps[Chunked]: cannot count tasks: the chunk size is adaptive (targetRuntimeSeconds), so the number of chunks is decided at run time
",
    );
}

// ─── audit round 5 ──────────────────────────────────────────────────

#[test]
fn step_filter_matches_a_bracketed_step_name_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let t = |cmd: &str| {
        format!(
            "specificationVersion: jobtemplate-2023-09\nname: J\nsteps:\n  - name: \"A][B\"\n    script: {{actions: {{onRun: {{command: {cmd}}}}}}}\n  - name: A\n    script: {{actions: {{onRun: {{command: a}}}}}}\n"
        )
    };
    std::fs::write(dir.path().join("old.yaml"), t("x")).unwrap();
    std::fs::write(dir.path().join("new.yaml"), t("y")).unwrap();
    let run = |step: &str| {
        openjd_in(
            dir.path(),
            &[
                "diff",
                "old.yaml",
                "new.yaml",
                "--step",
                step,
                "--exit-code",
            ],
            None,
        )
    };
    assert_ok(&run("A"), "old.yaml -> new.yaml\nno changes\n");
    let o = run("A][B");
    assert_eq!(o.status.code(), Some(4), "{}", stderr(&o));
    assert_eq!(
        stdout(&o),
        "\
old.yaml -> new.yaml
~ step A][B
    ~ script.onRun.command: x -> y  [template]
    tasks: 1 unchanged

1 step changed. Tasks: 1 kept, 0 added, 0 removed.
"
    );
}

// ─── audit round 6 ──────────────────────────────────────────────────

#[test]
fn step_filter_matches_by_identity_not_by_path_prefix() {
    for other in ["A].x", "A][B"] {
        let dir = tempfile::tempdir().unwrap();
        let t = |cmd: &str| {
            format!(
                "specificationVersion: jobtemplate-2023-09\nname: J\nsteps:\n  - name: A\n    script: {{actions: {{onRun: {{command: a}}}}}}\n  - name: \"{other}\"\n    script: {{actions: {{onRun: {{command: {cmd}}}}}}}\n"
            )
        };
        std::fs::write(dir.path().join("old.yaml"), t("x")).unwrap();
        std::fs::write(dir.path().join("new.yaml"), t("y")).unwrap();
        let run = |extra: &[&str]| {
            let mut args = vec!["diff", "old.yaml", "new.yaml"];
            args.extend_from_slice(extra);
            openjd_in(dir.path(), &args, None)
        };
        assert_ok(
            &run(&["--step", "A", "--exit-code"]),
            "old.yaml -> new.yaml\nno changes\n",
        );
        assert_ok(
            &run(&["--step", "A", "--stat"]),
            "old.yaml -> new.yaml\nno changes\n",
        );
        let o = run(&["--step", other, "--exit-code"]);
        assert_eq!(o.status.code(), Some(4), "{other}: {}", stderr(&o));
        assert_eq!(
            stdout(&o),
            format!(
                "\
old.yaml -> new.yaml
~ step {other}
    ~ script.onRun.command: x -> y  [template]
    tasks: 1 unchanged

1 step changed. Tasks: 1 kept, 0 added, 0 removed.
"
            )
        );
        // Unfiltered, only the changed step is listed.
        assert_ok(
            &run(&["--stat"]),
            &format!(
                "old.yaml -> new.yaml\n~ {other}  tasks 1 -> 1  (+0 -0)\n1 step changed. Tasks: 2 kept, 0 added, 0 removed.\n"
            ),
        );
    }
}

#[test]
fn step_filter_keeps_the_steps_own_unexpanded_change() {
    let o = openjd(
        &[
            "diff",
            "render_v1.yaml",
            "render_v2.yaml",
            "-p",
            "Scene=/s",
            "--step",
            "Render",
            "--depth",
            "1",
        ],
        None,
    );
    assert_ok(
        &o,
        &format!(
            "{V1_V2}\
~ step Render
    ~ (not expanded)  [template]
    tasks: 100 -> 200
      keep   100   Frame 1-100 × Eye left
      add    100   Frame 1-100 × Eye right

1 step changed. Tasks: 100 kept, 100 added, 0 removed.
"
        ),
    );
}

#[test]
fn tasks_conflicts_with_stat() {
    let o = openjd(
        &[
            "diff",
            "render_v1.yaml",
            "render_v2.yaml",
            "--tasks",
            "--stat",
        ],
        None,
    );
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(
        stderr(&o),
        "error: the argument '--tasks' cannot be used with '--stat'\n\nUsage: openjd diff --tasks <TEMPLATE> <NEW_TEMPLATE>\n\nFor more information, try '--help'.\n"
    );
}

#[test]
fn env_and_job_param_are_aliases() {
    let env = "\
specificationVersion: environment-2023-09
parameterDefinitions:
  - {name: Tool, type: STRING, default: t1}
environment:
  name: Env
  variables: {TOOL: \"{{Param.Tool}}\"}
";
    let o = openjd(
        &[
            "diff",
            "render_v1.yaml",
            "render_v2.yaml",
            "--job-param",
            "Scene=/s",
            "--env",
            "-",
            "--new-param",
            "Tool=t2",
            "--stat",
        ],
        Some(env),
    );
    assert_ok(
        &o,
        &format!("{V1_V2}~ Render  tasks 100 -> 200  (+100 -0)\n1 step changed. Tasks: 100 kept, 100 added, 0 removed.\n"),
    );
}

#[test]
fn an_empty_plan_says_nothing_to_apply() {
    let o = openjd(
        &[
            "diff",
            "render_v1.yaml",
            "render_v1.yaml",
            "-p",
            "Scene=/s",
            "--plan",
        ],
        None,
    );
    assert_ok(
        &o,
        "render_v1.yaml -> render_v1.yaml\nno changes\n\napply plan:\n  nothing to apply\nkinds: none\n",
    );
}

// ─── audit round 7 ──────────────────────────────────────────────────

const PLAIN: &str = "specificationVersion: jobtemplate-2023-09\nname: J\nsteps:\n  - name: S\n    script: {actions: {onRun: {command: c}}}\n";

/// A template whose only arg resolves to 40000 characters: within the
/// library's defaults, past the CLI's `max_resolved_arg_len` of 32768.
const LONG_ARG: &str = "specificationVersion: jobtemplate-2023-09\nextensions: [EXPR]\nname: J\nparameterDefinitions:\n  - {name: N, type: INT, default: 40000}\nsteps:\n  - name: S\n    script: {actions: {onRun: {command: c, args: [\"{{ 'a' * Param.N }}\"]}}}\n";

const LONG_ARG_ERROR: &str = "Model validation error: 1 validation error for JobTemplate\nsteps[0] -> script -> actions -> onRun -> args[0]:\n\tresolves to at least 40000 characters, exceeding the maximum of 32768.\n";

#[test]
fn diff_applies_the_cli_caller_limits() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.yaml"), PLAIN).unwrap();
    std::fs::write(dir.path().join("long.yaml"), LONG_ARG).unwrap();
    let o = openjd_in(dir.path(), &["diff", "a.yaml", "long.yaml"], None);
    assert_eq!(stdout(&o), "");
    assert_eq!(
        stderr(&o),
        format!("{DIFF_WARNING}ERROR: Validation error: new template: {LONG_ARG_ERROR}")
    );
    assert_eq!(o.status.code(), Some(1));
}

#[test]
fn an_unexpanded_parameter_space_shows_each_definition_as_written() {
    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/diff");
    let o = openjd_in(
        &examples,
        &[
            "diff",
            "range_rewritten_v1.yaml",
            "range_rewritten_v2.yaml",
            "--depth",
            "2",
        ],
        None,
    );
    assert_ok(
        &o,
        "\
range_rewritten_v1.yaml -> range_rewritten_v2.yaml
~ step Render
    ~ parameterSpace: {Frame: range expression 1,2} -> {Frame: list [1, 2]}  [template]
    tasks: 2 unchanged

1 step changed. Tasks: 2 kept, 0 added, 0 removed.
",
    );
}

// ─── one diff command, deadline-style parameters ────────────────────

/// A template whose required `Frames` parameter sets the task range, and
/// `Frames=1-2` written as a JSON and a YAML parameter file.
fn frames_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("t.yaml"),
        "specificationVersion: jobtemplate-2023-09\nname: J\nparameterDefinitions:\n  - {name: Frames, type: STRING}\nsteps:\n  - name: S\n    parameterSpace:\n      taskParameterDefinitions:\n        - {name: Frame, type: INT, range: \"{{Param.Frames}}\"}\n    script: {actions: {onRun: {command: c, args: [\"{{Task.Param.Frame}}\"]}}}\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("p.json"), "{\"Frames\": \"1-2\"}\n").unwrap();
    std::fs::write(dir.path().join("p.yaml"), "Frames: 1-2\n").unwrap();
    dir
}

/// `Frames` 1-2 on the old side and 1-3 on the new.
const FRAMES_2_TO_3: &str = "\
~ parameter Frames: 1-2 -> 1-3  [parameters]
~ step S
    ~ Frame: 1,2 -> 1-3  [parameters]
    tasks: 2 -> 3
      keep   2     Frame 1,2
      add    1     Frame 3

1 step changed. Tasks: 2 kept, 1 added, 0 removed.
";

/// `Frames` 1-3 on the old side and 1-2 on the new.
const FRAMES_3_TO_2: &str = "\
~ parameter Frames: 1-3 -> 1-2  [parameters]
~ step S
    ~ Frame: 1-3 -> 1,2  [parameters]
    tasks: 3 -> 2
      keep   2     Frame 1,2
      remove 1     Frame 3

1 step changed. Tasks: 2 kept, 0 added, 1 removed.
";

const ONE_TEMPLATE: &str = "t.yaml: old parameters -> new parameters\n";

/// Each of the three parameter forms, on each of the three parameter flags.
#[test]
fn every_parameter_form_on_every_parameter_flag() {
    let dir = frames_dir();
    let forms = [
        "Frames=1-2",
        r#"{"Frames": "1-2"}"#,
        "file://p.json",
        "file://p.yaml",
    ];
    for form in forms {
        for (flags, expected) in [
            (["-p", form, "--new-parameter", "Frames=1-3"], FRAMES_2_TO_3),
            (
                ["--old-parameter", form, "--new-parameter", "Frames=1-3"],
                FRAMES_2_TO_3,
            ),
            (
                ["--new-parameter", form, "--old-parameter", "Frames=1-3"],
                FRAMES_3_TO_2,
            ),
        ] {
            let mut args = vec!["diff", "t.yaml"];
            args.extend_from_slice(&flags);
            let o = openjd_in(dir.path(), &args, None);
            assert_eq!(stderr(&o), DIFF_WARNING, "{args:?}");
            assert_eq!(stdout(&o), format!("{ONE_TEMPLATE}{expected}"), "{args:?}");
            assert_eq!(o.status.code(), Some(0), "{args:?}");
        }
    }
}

/// Within a flag a later value wins; a side flag wins over `-p` wherever
/// it appears on the command line; two templates each take their own side.
#[test]
fn later_values_win_and_a_side_flag_wins_over_p() {
    let dir = frames_dir();
    let run = |args: &[&str]| openjd_in(dir.path(), args, None);
    let one = format!("{ONE_TEMPLATE}{FRAMES_2_TO_3}");
    for args in [
        // The later -p wins.
        &[
            "diff",
            "t.yaml",
            "-p",
            "Frames=1-9",
            "-p",
            "file://p.json",
            "--new-parameter",
            "Frames=1-3",
        ][..],
        // The side flag wins over a -p that comes after it.
        &[
            "diff",
            "t.yaml",
            "--new-parameter",
            "Frames=1-3",
            "-p",
            "Frames=1-2",
        ][..],
        // The later --old-parameter wins, and both win over -p.
        &[
            "diff",
            "t.yaml",
            "-p",
            "Frames=1-9",
            "--old-parameter",
            "file://p.yaml",
            "--old-parameter",
            r#"{"Frames": "1-2"}"#,
            "--new-parameter",
            "Frames=1-9",
            "--new-parameter",
            "Frames=1-3",
        ][..],
    ] {
        assert_ok(&run(args), &one);
    }
    assert_ok(
        &run(&[
            "diff",
            "t.yaml",
            "t.yaml",
            "-p",
            "Frames=1-9",
            "--old-parameter",
            "file://p.yaml",
            "--new-parameter",
            "Frames=1-3",
        ]),
        &format!("t.yaml -> t.yaml\n{FRAMES_2_TO_3}"),
    );
}

/// Two templates, the new one from stdin, each side with its own value.
#[test]
fn two_templates_with_side_parameters_and_stdin() {
    let dir = frames_dir();
    let text = std::fs::read_to_string(dir.path().join("t.yaml")).unwrap();
    let o = openjd_in(
        dir.path(),
        &[
            "diff",
            "t.yaml",
            "-",
            "--old-parameter",
            "file://p.json",
            "--new-parameter",
            "Frames=1-3",
        ],
        Some(&text),
    );
    assert_ok(&o, &format!("t.yaml -> -\n{FRAMES_2_TO_3}"));
}

/// `--job-param` (the long name `run` and `summary` use) and the earlier
/// spellings `--old-param` and `--new-param` are accepted, and `--help` does
/// not list them.
#[test]
fn old_parameter_flag_names_are_hidden_aliases() {
    let dir = frames_dir();
    let o = openjd_in(
        dir.path(),
        &[
            "diff",
            "t.yaml",
            "--job-param",
            "Frames=1-9",
            "--old-param",
            "Frames=1-2",
            "--new-param",
            "Frames=1-3",
        ],
        None,
    );
    assert_ok(&o, &format!("{ONE_TEMPLATE}{FRAMES_2_TO_3}"));
    let help = stdout(&openjd(&["diff", "--help"], None));
    for hidden in ["--job-param", "--old-param ", "--new-param ", "alias"] {
        assert!(!help.contains(hidden), "{hidden}: {help}");
    }
}

/// The usage line and the parameter flags' help, which names each form
/// with an example.
#[test]
fn help_shows_one_or_two_templates_and_the_parameter_forms() {
    let help = stdout(&openjd(&["diff", "--help"], None));
    for line in [
        "Usage: openjd diff [OPTIONS] <TEMPLATE> [NEW_TEMPLATE]\n",
        "  <TEMPLATE>      Template file, or `-` for stdin. With only this template, the old and new sides differ only by --old-parameter and --new-parameter\n",
        "  [NEW_TEMPLATE]  New template file, or `-` for stdin; TEMPLATE is then the old side\n",
        "  -p, --parameter <PARAM>           Job parameter for both sides: Key=Value, a JSON or YAML file as file://path, or inline JSON. Repeatable; a later value wins. Examples: -p Frames=1-200, -p file://params.json, -p '{\"Frames\": \"1-200\"}'\n",
        "      --old-parameter <PARAM>       Job parameter for the old side only, in the same forms as -p; wins over -p. Example: --old-parameter Frames=1-100\n",
        "      --new-parameter <PARAM>       Job parameter for the new side only, in the same forms as -p; wins over -p. Example: --new-parameter Frames=1-200\n",
    ] {
        assert!(help.contains(line), "{line}{help}");
    }
}

#[test]
fn the_build_command_is_gone() {
    let o = openjd(&["build", "render_v1.yaml", "-p", "Scene=/s"], None);
    assert_eq!(
        stderr(&o),
        "error: unrecognized subcommand 'build'\n\nUsage: openjd <COMMAND>\n\nFor more information, try '--help'.\n"
    );
    assert_eq!(stdout(&o), "");
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn the_jobs_flag_is_gone() {
    let o = openjd(&["diff", "--jobs", "old.json", "new.json"], None);
    assert_eq!(
        stderr(&o),
        "error: unexpected argument '--jobs' found\n\n  tip: to pass '--jobs' as a value, use '-- --jobs'\n\nUsage: openjd diff [OPTIONS] <TEMPLATE> [NEW_TEMPLATE]\n\nFor more information, try '--help'.\n"
    );
    assert_eq!(stdout(&o), "");
    assert_eq!(o.status.code(), Some(2));
}

/// `--verbose` with JSON still includes both jobs, which are what a
/// caller that used to build them can read now.
#[test]
fn verbose_json_includes_both_jobs() {
    let dir = frames_dir();
    let o = openjd_in(
        dir.path(),
        &[
            "diff",
            "t.yaml",
            "-p",
            "Frames=1-2",
            "--new-parameter",
            "Frames=1-3",
            "--output",
            "json",
            "--verbose",
        ],
        None,
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(stderr(&o), DIFF_WARNING);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(
        (
            &v["oldJob"]["parameters"]["Frames"],
            &v["newJob"]["parameters"]["Frames"]
        ),
        (
            &serde_json::json!({"name": "Frames", "type": "STRING", "value": "1-2"}),
            &serde_json::json!({"name": "Frames", "type": "STRING", "value": "1-3"})
        )
    );
}

// ─── a null parameter value ─────────────────────────────────────────

/// Write `t.yaml`, whose STRING `A` has the default `dflt`, `nd.yaml`, whose
/// `A` has no default, and one parameter file per null spelling.
fn null_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let t = |default: &str| {
        format!(
            "specificationVersion: jobtemplate-2023-09\nname: J\nparameterDefinitions:\n  - {{name: A, type: STRING{default}}}\nsteps:\n  - name: S\n    script: {{actions: {{onRun: {{command: c, args: [\"{{{{Param.A}}}}\"]}}}}}}\n"
        )
    };
    std::fs::write(dir.path().join("t.yaml"), t(", default: dflt")).unwrap();
    std::fs::write(dir.path().join("nd.yaml"), t("")).unwrap();
    std::fs::write(dir.path().join("n.json"), "{\"A\": null}\n").unwrap();
    std::fs::write(dir.path().join("null.yaml"), "A: null\n").unwrap();
    std::fs::write(dir.path().join("tilde.yaml"), "A: ~\n").unwrap();
    std::fs::write(dir.path().join("empty.yaml"), "A:\n").unwrap();
    dir
}

/// Every parameter form that gives `A` a null.
const NULLS: &[&str] = &[
    r#"{"A": null}"#,
    "file://n.json",
    "file://null.yaml",
    "file://tilde.yaml",
    "file://empty.yaml",
];

fn a_changes(old: &str, new: &str) -> String {
    format!(
        "t.yaml: old parameters -> new parameters\n~ parameter A: {old} -> {new}  [parameters]\n~ step S\n    ~ resolvedSymTab: Param.A={old} -> Param.A={new}  [parameters]\n    tasks: 1 unchanged\n\n1 step changed. Tasks: 1 kept, 0 added, 0 removed.\n"
    )
}

/// A null means not given, so the template default applies.
#[test]
fn null_parameter_uses_the_default() {
    let dir = null_dir();
    for null in NULLS {
        let o = openjd_in(
            dir.path(),
            &[
                "diff",
                "t.yaml",
                "--old-parameter",
                "A=x",
                "--new-parameter",
                null,
            ],
            None,
        );
        assert_eq!(stderr(&o), DIFF_WARNING, "{null}");
        assert_eq!(stdout(&o), a_changes("x", "dflt"), "{null}");
        assert_eq!(o.status.code(), Some(0), "{null}");
    }
}

/// With a template from stdin, a new-side null overrides an old-side or `-p`
/// value, so the default applies.
#[test]
fn null_parameter_with_a_stdin_template_uses_the_default() {
    let dir = null_dir();
    let text = std::fs::read_to_string(dir.path().join("t.yaml")).unwrap();
    for (null, old_flag) in NULLS
        .iter()
        .flat_map(|null| [(null, "--old-parameter"), (null, "-p")])
    {
        let o = openjd_in(
            dir.path(),
            &["diff", "-", old_flag, "A=x", "--new-parameter", null],
            Some(&text),
        );
        assert_eq!(stderr(&o), DIFF_WARNING, "{old_flag} {null}");
        assert_eq!(
            stdout(&o),
            "-: old parameters -> new parameters\n~ parameter A: x -> dflt  [parameters]\n~ step S\n    ~ resolvedSymTab: Param.A=x -> Param.A=dflt  [parameters]\n    tasks: 1 unchanged\n\n1 step changed. Tasks: 1 kept, 0 added, 0 removed.\n",
            "{old_flag} {null}"
        );
        assert_eq!(o.status.code(), Some(0), "{old_flag} {null}");
    }
}

/// A null in the side flag overrides the `-p` value for that side only.
#[test]
fn side_flag_null_overrides_p() {
    let dir = null_dir();
    for null in NULLS {
        let o = openjd_in(
            dir.path(),
            &["diff", "t.yaml", "-p", "A=x", "--new-parameter", null],
            None,
        );
        assert_eq!(stderr(&o), DIFF_WARNING, "{null}");
        assert_eq!(stdout(&o), a_changes("x", "dflt"), "{null}");
        assert_eq!(o.status.code(), Some(0), "{null}");
    }
}

/// A null after a value in `-p` wins, as any later value does.
#[test]
fn null_after_a_value_uses_the_default() {
    let dir = null_dir();
    let o = openjd_in(
        dir.path(),
        &[
            "diff",
            "t.yaml",
            "-p",
            "A=1",
            "-p",
            r#"{"A": null}"#,
            "--new-parameter",
            "A=2",
        ],
        None,
    );
    assert_ok(&o, &a_changes("dflt", "2"));
}

/// A value after a null on one side wins, so the null does not stick.
#[test]
fn value_after_a_null_wins() {
    let dir = null_dir();
    let o = openjd_in(
        dir.path(),
        &[
            "diff",
            "t.yaml",
            "--old-parameter",
            "A=x",
            "--new-parameter",
            r#"{"A": null}"#,
            "--new-parameter",
            r#"{"A": "2"}"#,
        ],
        None,
    );
    assert_ok(&o, &a_changes("x", "2"));
}

#[test]
fn null_parameter_with_no_default_is_a_missing_value() {
    let dir = null_dir();
    for null in NULLS {
        let o = openjd_in(
            dir.path(),
            &[
                "diff",
                "nd.yaml",
                "--old-parameter",
                "A=x",
                "--new-parameter",
                null,
            ],
            None,
        );
        assert_eq!(
            stderr(&o),
            format!("{DIFF_WARNING}ERROR: Validation error: new parameters: Values missing for required job parameters: A\n"),
            "{null}"
        );
        assert_eq!(stdout(&o), "", "{null}");
        assert_eq!(o.status.code(), Some(1), "{null}");
    }
}

/// `A=null` is a Key=Value string, so it stays the text `null`, unlike a
/// JSON null.
#[test]
fn key_value_null_is_the_text_null() {
    let dir = null_dir();
    let o = openjd_in(
        dir.path(),
        &[
            "diff",
            "t.yaml",
            "--old-parameter",
            r#"{"A": null}"#,
            "--new-parameter",
            "A=null",
        ],
        None,
    );
    assert_ok(&o, &a_changes("dflt", "null"));
}

/// Write `t2.yaml`, whose STRING `A` has the default `dflt` and `B` the
/// default `bd`.
fn two_param_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("t2.yaml"),
        "specificationVersion: jobtemplate-2023-09\nname: J\nparameterDefinitions:\n  - {name: A, type: STRING, default: dflt}\n  - {name: B, type: STRING, default: bd}\nsteps:\n  - name: S\n    script: {actions: {onRun: {command: c, args: [\"{{Param.A}}\", \"{{Param.B}}\"]}}}\n",
    )
    .unwrap();
    dir
}

/// The diff when only `A` changes and `B` is `given` on both sides.
fn a_changes_b_given(old: &str, new: &str) -> String {
    format!(
        "t2.yaml: old parameters -> new parameters\n~ parameter A: {old} -> {new}  [parameters]\n~ step S\n    ~ resolvedSymTab: Param.A={old}, Param.B=given -> Param.A={new}, Param.B=given  [parameters]\n    tasks: 1 unchanged\n\n1 step changed. Tasks: 1 kept, 0 added, 0 removed.\n"
    )
}

/// A null in `--new-parameter` removes only `A`, so the new side keeps the
/// `B` that `-p` gave.
#[test]
fn new_side_null_leaves_other_parameters() {
    let dir = two_param_dir();
    let o = openjd_in(
        dir.path(),
        &[
            "diff",
            "t2.yaml",
            "-p",
            "A=x",
            "-p",
            "B=given",
            "--new-parameter",
            r#"{"A": null}"#,
        ],
        None,
    );
    assert_ok(&o, &a_changes_b_given("x", "dflt"));
}

/// A null in `--old-parameter` overrides `-p` on the old side only.
#[test]
fn old_side_null_overrides_p() {
    let dir = two_param_dir();
    let o = openjd_in(
        dir.path(),
        &[
            "diff",
            "t2.yaml",
            "-p",
            "A=x",
            "-p",
            "B=given",
            "--old-parameter",
            r#"{"A": null}"#,
        ],
        None,
    );
    assert_ok(&o, &a_changes_b_given("dflt", "x"));
}

/// A null for a parameter that only the environment template defines means
/// not given, so the environment's default applies.
#[test]
fn null_environment_parameter_uses_the_default() {
    let o = openjd_in(
        &templates(),
        &[
            "diff",
            "simple_with_j_param.yaml",
            "-p",
            "J=1",
            "--old-parameter",
            "EnvParam=x",
            "--new-parameter",
            r#"{"EnvParam": null}"#,
            "--environment",
            "env_with_param.yaml",
        ],
        None,
    );
    assert_ok(
        &o,
        "simple_with_j_param.yaml: old parameters -> new parameters\n~ parameter EnvParam: x -> DefaultForEnvParam  [parameters]\n\n0 steps changed. Tasks: 1 kept, 0 added, 0 removed.\n",
    );
}

/// A side-flag null for an environment parameter overrides the `-p` value
/// for that side, so the environment's default applies there.
#[test]
fn side_flag_null_overrides_p_for_an_environment_parameter() {
    let o = openjd_in(
        &templates(),
        &[
            "diff",
            "simple_with_j_param.yaml",
            "-p",
            "J=1",
            "-p",
            "EnvParam=x",
            "--new-parameter",
            r#"{"EnvParam": null}"#,
            "--environment",
            "env_with_param.yaml",
        ],
        None,
    );
    assert_ok(
        &o,
        "simple_with_j_param.yaml: old parameters -> new parameters\n~ parameter EnvParam: x -> DefaultForEnvParam  [parameters]\n\n0 steps changed. Tasks: 1 kept, 0 added, 0 removed.\n",
    );
}
