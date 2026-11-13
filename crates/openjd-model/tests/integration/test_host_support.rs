// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Host support functions in `job::template_diff` (unstable): template
//! fragments, the job-scope symbol table, the job digest, building from
//! preprocessed values, and the parameter merge. See
//! `specs/model/host-support.md`.

use openjd_expr::path_mapping::PathFormat;
use openjd_expr::ExprValue;
use openjd_model::job::apply::plan;
use openjd_model::job::diff::{diff_jobs, DiffOptions};
use openjd_model::job::template_diff::{
    build_job_symbol_table, diff_templates, job_digest, job_from_str, merge_parameters,
    template_job_environment_fragment, template_step_fragment, JobDocument, TemplateInput,
    TemplateSource,
};
use openjd_model::job::Job;
use openjd_model::{
    decode_job_template, preprocess_job_parameters, CallerLimits, JobParameterInputValues,
    JobParameterType, JobParameterValue, JobParameterValues, PathParameterOptions,
};
use serde_json::Value;

const V1: &str = include_str!("../../examples/job_diff/v1.yaml");
const V2: &str = include_str!("../../examples/job_diff/v2.yaml");
const CAMS1: &str = include_str!("../../examples/job_diff/cams_v1.yaml");
const CAMS2: &str = include_str!("../../examples/job_diff/cams_v2.yaml");

fn doc(yaml: &str) -> Value {
    serde_saphyr::from_str(yaml).unwrap()
}

/// Absolute on the host: preprocessing requires an absolute template dir.
const TEMPLATE_DIR: &str = if cfg!(windows) { r"C:\t" } else { "/t" };
const CWD: &str = if cfg!(windows) { r"C:\work" } else { "/work" };
const SEP: &str = if cfg!(windows) { r"\" } else { "/" };

fn text(yaml: &str) -> TemplateInput {
    TemplateInput::new(TemplateSource::Text(yaml.to_string()))
        .template_dir(TEMPLATE_DIR)
        .cwd(CWD)
}

// ─── template fragments ─────────────────────────────────────────────

#[test]
fn a_step_fragment_is_the_step_as_written() {
    let fragment = template_step_fragment(&doc(V2), "Denoise").unwrap();
    let want = doc(r#"
name: Denoise
dependencies: [{dependsOn: Render}]
stepEnvironments:
  - name: Gpu
    variables: {CUDA_VISIBLE_DEVICES: "0"}
script:
  actions:
    onRun: {command: oidn, args: ["frames"]}
"#);
    assert_eq!(fragment.step, want);
    assert_eq!(
        fragment.step.to_string(),
        r#"{"dependencies":[{"dependsOn":"Render"}],"name":"Denoise","script":{"actions":{"onRun":{"args":["frames"],"command":"oidn"}}},"stepEnvironments":[{"name":"Gpu","variables":{"CUDA_VISIBLE_DEVICES":"0"}}]}"#
    );
    assert_eq!(
        fragment.step_environments,
        vec![doc("{name: Gpu, variables: {CUDA_VISIBLE_DEVICES: \"0\"}}")]
    );
    // Format strings are unresolved.
    let render = template_step_fragment(&doc(V2), "Render").unwrap();
    assert_eq!(
        render.step["parameterSpace"]["taskParameterDefinitions"][0]["range"],
        "{{Param.Frames}}"
    );
    assert!(render.step_environments.is_empty());
}

#[test]
fn a_job_environment_fragment_is_the_environment_as_written() {
    assert_eq!(
        template_job_environment_fragment(&doc(V2), "Conda")
            .unwrap()
            .to_string(),
        r#"{"name":"Conda","variables":{"CONDA_PACKAGES":"blender=4.2","PYTHONUNBUFFERED":"1"}}"#
    );
}

#[test]
fn a_missing_fragment_is_an_error() {
    let cases = [
        (
            template_step_fragment(&doc(V2), "Nope").map(|f| f.step),
            "Validation error: the template has no step named 'Nope'",
        ),
        (
            template_step_fragment(&doc("name: X"), "Nope").map(|f| f.step),
            "Validation error: the template document has no 'steps' list",
        ),
        (
            template_step_fragment(&doc("steps: [{name: S, stepEnvironments: {name: E}}]"), "S")
                .map(|f| f.step),
            "Validation error: the 'stepEnvironments' of step 'S' is not a list",
        ),
        (
            template_job_environment_fragment(&doc(V2), "Nope"),
            "Validation error: the template has no job environment named 'Nope'",
        ),
        (
            template_job_environment_fragment(&doc("steps: []"), "E"),
            "Validation error: the template document has no 'jobEnvironments' list",
        ),
    ];
    for (got, want) in cases {
        assert_eq!(got.unwrap_err().to_string(), want);
    }
}

// ─── job-scope symbol table ─────────────────────────────────────────

const EVERY_TYPE: &str = r#"
specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: "Sym {{Param.S}}"
parameterDefinitions:
  - {name: S, type: STRING, default: s}
  - {name: I, type: INT, default: 2}
  - {name: F, type: FLOAT, default: "1.50"}
  - {name: P, type: PATH, default: in}
  - {name: B, type: BOOL, default: true}
  - {name: LS, type: "LIST[STRING]", default: [a, b]}
  - {name: LI, type: "LIST[INT]", default: [1, 2]}
  - {name: LF, type: "LIST[FLOAT]", default: [0.5]}
  - {name: LP, type: "LIST[PATH]", default: [x]}
  - {name: LB, type: "LIST[BOOL]", default: [false]}
  - {name: LL, type: "LIST[LIST[INT]]", default: [[1, 2], [3]]}
jobEnvironments:
  - name: All
    variables:
      JOB: "{{Job.Name}}"
      S: "{{Param.S}} {{RawParam.S}}"
      I: "{{Param.I}} {{RawParam.I}}"
      F: "{{Param.F}} {{RawParam.F}}"
      P: "{{RawParam.P}}"
      B: "{{Param.B}} {{RawParam.B}}"
      LS: "{{repr_json(Param.LS)}} {{repr_json(RawParam.LS)}}"
      LI: "{{repr_json(Param.LI)}} {{repr_json(RawParam.LI)}}"
      LF: "{{repr_json(Param.LF)}} {{repr_json(RawParam.LF)}}"
      LP: "{{repr_json(RawParam.LP)}}"
      LB: "{{repr_json(Param.LB)}} {{repr_json(RawParam.LB)}}"
      LL: "{{repr_json(Param.LL)}} {{repr_json(RawParam.LL)}}"
steps:
  - name: A
    script: {actions: {onRun: {command: a}}}
"#;

#[test]
fn the_job_symbol_table_holds_every_parameter_and_the_job_name() {
    let job = text(EVERY_TYPE).build().unwrap();
    let table = build_job_symbol_table(&job).unwrap();
    let under_t = |p: &str| serde_json::to_string(&format!("{TEMPLATE_DIR}{SEP}{p}")).unwrap();
    assert_eq!(
        serde_json::to_string(&table).unwrap(),
        format!(
            concat!(
                r#"[{{"name":"Job.Name","type":"string","value":"Sym s"}},"#,
                r#"{{"name":"Param.B","type":"bool","value":"true"}},"#,
                r#"{{"name":"Param.F","type":"float","value":"1.50"}},"#,
                r#"{{"name":"Param.I","type":"int","value":"2"}},"#,
                r#"{{"name":"Param.LB","type":"list[bool]","value":["false"]}},"#,
                r#"{{"name":"Param.LF","type":"list[float]","value":["0.5"]}},"#,
                r#"{{"name":"Param.LI","type":"list[int]","value":["1","2"]}},"#,
                r#"{{"name":"Param.LL","type":"list[list[int]]","value":[["1","2"],["3"]]}},"#,
                r#"{{"name":"Param.LS","type":"list[string]","value":["a","b"]}},"#,
                r#"{{"name":"Param.S","type":"string","value":"s"}},"#,
                r#"{{"name":"RawParam.B","type":"bool","value":"true"}},"#,
                r#"{{"name":"RawParam.F","type":"float","value":"1.50"}},"#,
                r#"{{"name":"RawParam.I","type":"int","value":"2"}},"#,
                r#"{{"name":"RawParam.LB","type":"list[bool]","value":["false"]}},"#,
                r#"{{"name":"RawParam.LF","type":"list[float]","value":["0.5"]}},"#,
                r#"{{"name":"RawParam.LI","type":"list[int]","value":["1","2"]}},"#,
                r#"{{"name":"RawParam.LL","type":"list[list[int]]","value":[["1","2"],["3"]]}},"#,
                r#"{{"name":"RawParam.LP","type":"list[string]","value":[{lp}]}},"#,
                r#"{{"name":"RawParam.LS","type":"list[string]","value":["a","b"]}},"#,
                r#"{{"name":"RawParam.P","type":"string","value":{p}}},"#,
                r#"{{"name":"RawParam.S","type":"string","value":"s"}}]"#,
            ),
            lp = under_t("x"),
            p = under_t("in")
        )
    );
    // An environment that reads every symbol gets exactly this table.
    let env = &job.job_environments.as_ref().unwrap()[0];
    assert_eq!(env.resolved_symtab.as_ref(), Some(&table));
}

#[test]
fn the_job_symbol_table_has_no_job_name_without_expr() {
    let job = text(V1).build().unwrap();
    assert_eq!(
        serde_json::to_string(&build_job_symbol_table(&job).unwrap()).unwrap(),
        concat!(
            r#"[{"name":"Param.Frames","type":"string","value":"1-10"},"#,
            r#"{"name":"Param.Scene","type":"string","value":"shot010.blend"},"#,
            r#"{"name":"RawParam.Frames","type":"string","value":"1-10"},"#,
            r#"{"name":"RawParam.Scene","type":"string","value":"shot010.blend"}]"#,
        )
    );
}

// ─── job digest ─────────────────────────────────────────────────────

#[test]
fn the_digest_ignores_map_order() {
    let a = text(EVERY_TYPE).build().unwrap();
    let mut b = text(EVERY_TYPE).build().unwrap();
    b.parameters.reverse();
    b.parameters.move_index(0, 3);
    let env = &mut b.job_environments.as_mut().unwrap()[0];
    let vars: Vec<_> = env.variables.take().unwrap().into_iter().collect();
    env.variables = Some(vars.into_iter().rev().collect());
    assert_eq!(job_digest(&a).unwrap(), job_digest(&b).unwrap());
    let d = job_digest(&a).unwrap();
    assert_eq!(d.len(), 64);
    assert!(d.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')), "{d}");
    // Pinned: a change here is a change of the document format.
    assert_eq!(
        job_digest(&text(V1).build().unwrap()).unwrap(),
        "236cb20c8f3f685c70ba07758c98365324bc00288b6c27a02b305758929dae70"
    );
}

#[test]
fn value_equal_jobs_have_the_same_digest() {
    // A list's cached size is not part of its value, nor of the document.
    let a = text(EVERY_TYPE).build().unwrap();
    let mut b = a.clone();
    b.parameters["LS"].value = ExprValue::ListString(vec!["a".into(), "b".into()], 0);
    assert_ne!(
        serde_json::to_string(&a.parameters["LS"].value).unwrap(),
        serde_json::to_string(&b.parameters["LS"].value).unwrap()
    );
    assert_eq!(a, b);
    assert_eq!(job_digest(&a).unwrap(), job_digest(&b).unwrap());
    // Every rebuild of the same inputs, and its document read back.
    let digests: std::collections::BTreeSet<String> = (0..50)
        .flat_map(|_| {
            let job = text(EVERY_TYPE).build().unwrap();
            let back = job_from_str(&serde_json::to_string(&JobDocument(&job)).unwrap()).unwrap();
            [job_digest(&job).unwrap(), job_digest(&back).unwrap()]
        })
        .collect();
    assert_eq!(digests.len(), 1, "{digests:?}");
}

#[test]
fn any_diff_changes_the_digest() {
    let pairs: Vec<(Job, Job)> = vec![
        (text(V1).build().unwrap(), text(V2).build().unwrap()),
        (
            text(V1).build().unwrap(),
            text(V1).param("Frames", "1-5").build().unwrap(),
        ),
        (
            text(V1).build().unwrap(),
            text(V1).param("Scene", "shot020.blend").build().unwrap(),
        ),
        (text(CAMS1).build().unwrap(), text(CAMS2).build().unwrap()),
        (
            text(EVERY_TYPE).build().unwrap(),
            text(EVERY_TYPE).param("F", "1.5").build().unwrap(),
        ),
    ];
    for (a, b) in &pairs {
        assert!(diff_jobs(a, b, &DiffOptions::default()).is_changed());
        assert_ne!(job_digest(a).unwrap(), job_digest(b).unwrap());
    }
}

// ─── building from stored values ────────────────────────────────────

const OUT: &str = r#"
specificationVersion: jobtemplate-2023-09
name: Out
parameterDefinitions:
  - {name: Out, type: PATH}
steps:
  - name: A
    script: {actions: {onRun: {command: a, args: ["{{RawParam.Out}}"]}}}
"#;

/// `Out` preprocessed as a Windows path, as a Windows host stores it.
fn stored_windows_out() -> JobParameterValues {
    let template = decode_job_template(doc(OUT), None, &CallerLimits::default()).unwrap();
    let input: JobParameterInputValues = [("Out".to_string(), ExprValue::String("out".into()))]
        .into_iter()
        .collect();
    preprocess_job_parameters(
        &template,
        &input,
        &[],
        &PathParameterOptions {
            job_template_dir: r"C:\t",
            current_working_dir: r"C:\work",
            path_format: PathFormat::Windows,
            allow_template_dir_walk_up: false,
            allow_uri_path_values: false,
        },
    )
    .unwrap()
}

fn out_value(job: &Job) -> String {
    job.parameters["Out"].value.to_display_string()
}

#[test]
fn preprocessed_values_are_built_as_given() {
    let stored = stored_windows_out();
    assert_eq!(stored["Out"].value.to_display_string(), r"C:\work\out");
    let job = text(OUT).preprocessed(stored.clone()).build().unwrap();
    assert_eq!(out_value(&job), r"C:\work\out");
    // A second preprocess resolves the value for this host. On a POSIX host
    // a Windows path is relative, so it is joined to the working directory.
    let again: JobParameterInputValues = stored
        .iter()
        .map(|(k, v)| (k.clone(), v.value.clone()))
        .collect();
    let rebuilt = text(OUT).params(again).build().unwrap();
    if cfg!(windows) {
        assert_eq!(out_value(&rebuilt), r"C:\work\out");
    } else {
        assert_eq!(out_value(&rebuilt), r"/work/C:\work\out");
    }
}

#[test]
fn preprocessed_values_replace_input_values_and_still_meet_constraints() {
    let stored = stored_windows_out();
    let job = text(OUT)
        .param("Out", "ignored")
        .preprocessed(stored.clone())
        .build()
        .unwrap();
    assert_eq!(out_value(&job), r"C:\work\out");
    // The same preprocessed values on both sides: nothing differs.
    let same = diff_templates(
        &text(OUT).preprocessed(stored.clone()),
        &text(OUT).preprocessed(stored),
        &DiffOptions::default(),
    )
    .unwrap();
    assert!(!same.is_changed());
    // `create_job` still checks constraints.
    let limited = OUT.replace("type: PATH}", "type: PATH, maxLength: 3}");
    let err = text(&limited)
        .preprocessed(stored_windows_out())
        .build()
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Validation error: Parameter 'Out': value length 11 exceeds maximum 3"
    );
}

// ─── parameter merge ────────────────────────────────────────────────

const OLD: &str = r#"
specificationVersion: jobtemplate-2023-09
name: M
parameterDefinitions:
  - {name: A, type: STRING, default: a}
  - {name: B, type: INT, default: 1}
  - {name: C, type: STRING, default: c}
  - {name: E, type: STRING, default: e}
steps:
  - name: S
    script: {actions: {onRun: {command: x}}}
"#;

const NEW: &str = r#"
specificationVersion: jobtemplate-2023-09
name: M
parameterDefinitions:
  - {name: A, type: STRING, default: a2}
  - {name: B, type: STRING, default: b2}
  - {name: C, type: STRING, default: c2}
  - {name: New, type: INT, default: 7}
steps:
  - name: S
    script: {actions: {onRun: {command: x}}}
"#;

/// Each value as `name=TYPE:value`, sorted.
fn sorted(values: &JobParameterValues) -> Vec<String> {
    let mut v: Vec<String> = values
        .iter()
        .map(|(k, v)| format!("{k}={}:{}", v.param_type, v.value.to_display_string()))
        .collect();
    v.sort();
    v
}

/// A built job's values, as [`TemplateInput::preprocessed`] takes them.
fn values_of(job: &Job) -> JobParameterValues {
    job.parameters
        .values()
        .map(|p| {
            (
                p.name.clone(),
                JobParameterValue {
                    param_type: p.param_type,
                    value: p.value.clone(),
                },
            )
        })
        .collect()
}

#[test]
fn merge_keeps_stored_values_and_preprocesses_the_rest() {
    let old = text(OLD)
        .param("A", "x")
        .param("B", "5")
        .param("C", "y")
        .param("E", "z")
        .build()
        .unwrap();
    let stored = values_of(&old);
    let requested: JobParameterInputValues = [("C".to_string(), ExprValue::String("req".into()))]
        .into_iter()
        .collect();
    let merged = merge_parameters(&stored, &requested, &text(NEW)).unwrap();
    // A kept (same type); B retyped: the new default; C requested; E no
    // longer declared: dropped; New: the new default.
    assert_eq!(
        sorted(&merged),
        vec!["A=STRING:x", "B=STRING:b2", "C=STRING:req", "New=INT:7"]
    );
    let job = text(NEW).preprocessed(merged.clone()).build().unwrap();
    assert_eq!(sorted(&values_of(&job)), sorted(&merged));
    assert_eq!(stored["B"].param_type, JobParameterType::Int);
}

#[test]
fn merge_reports_a_request_the_new_template_does_not_declare() {
    let requested: JobParameterInputValues =
        [("Z".to_string(), ExprValue::String("undeclared".into()))]
            .into_iter()
            .collect();
    let err = merge_parameters(&JobParameterValues::new(), &requested, &text(NEW)).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Validation error: Job parameter values provided for parameters that are not defined in the template: Z"
    );
}

#[test]
fn merge_reports_a_retyped_parameter_without_a_default() {
    let stored: JobParameterValues = [(
        "B".to_string(),
        JobParameterValue {
            param_type: JobParameterType::Int,
            value: ExprValue::Int(5),
        },
    )]
    .into_iter()
    .collect();
    let retyped = NEW.replace(
        "{name: B, type: STRING, default: b2}",
        "{name: B, type: STRING}",
    );
    let err =
        merge_parameters(&stored, &JobParameterInputValues::new(), &text(&retyped)).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Validation error: Values missing for required job parameters: B"
    );
}

#[test]
fn merge_keeps_a_stored_windows_path_so_a_no_op_update_plans_nothing() {
    let stored = stored_windows_out();
    let old = text(OUT).preprocessed(stored.clone()).build().unwrap();
    let merged = merge_parameters(&stored, &JobParameterInputValues::new(), &text(OUT)).unwrap();
    assert_eq!(merged["Out"].value, stored["Out"].value);
    let new = text(OUT).preprocessed(merged).build().unwrap();
    assert_eq!(out_value(&new), r"C:\work\out");
    let p = plan(&old, &new).unwrap();
    assert!(p.ops.is_empty(), "{:?}", p.kinds());
}

#[test]
fn merge_preprocesses_a_requested_path() {
    let requested: JobParameterInputValues = [("Out".to_string(), ExprValue::String("o".into()))]
        .into_iter()
        .collect();
    let merged = merge_parameters(&stored_windows_out(), &requested, &text(OUT)).unwrap();
    assert_eq!(
        merged["Out"].value.to_display_string(),
        format!("{CWD}{SEP}o")
    );
}

#[test]
fn two_preprocessed_sides_with_the_same_values_give_a_template_origin() {
    // Both sides hold the stored values, so only the template changed. On a
    // POSIX host the Windows path would change if preprocessed again, so a
    // rule that split the change would mark it `both`.
    let a = OUT.replace(
        r#"{command: a, args: ["{{RawParam.Out}}"]}"#,
        "{command: a}",
    );
    let b = OUT.replace(
        r#"{command: a, args: ["{{RawParam.Out}}"]}"#,
        r#"{command: "a {{RawParam.Out}}"}"#,
    );
    let d = diff_templates(
        &text(&a).preprocessed(stored_windows_out()),
        &text(&b).preprocessed(stored_windows_out()),
        &DiffOptions::default(),
    )
    .unwrap();
    let got: Vec<String> = d.changes.iter().map(ToString::to_string).collect();
    assert_eq!(
        got,
        [
            r"~ steps[A].resolvedSymTab: (none) -> RawParam.Out=C:\work\out  [template]",
            "~ steps[A].script.onRun.command: a -> a {{RawParam.Out}}  [template]",
        ]
    );
}

#[test]
fn the_job_symbol_table_has_no_job_name_with_feature_bundle_1_alone() {
    let t = V1.replace("\r\n", "\n").replace(
        "specificationVersion: jobtemplate-2023-09\n",
        "specificationVersion: jobtemplate-2023-09\nextensions: [FEATURE_BUNDLE_1]\n",
    );
    assert_ne!(t, V1);
    let job = text(&t).build().unwrap();
    let table = serde_json::to_string(&build_job_symbol_table(&job).unwrap()).unwrap();
    assert!(table.starts_with(r#"[{"name":"Param.Frames""#), "{table}");
    assert!(!table.contains("Job.Name"), "{table}");
}

// ─── kept values are not preprocessed again ─────────────────────────

/// `template`'s parameters preprocessed from `input` with `format`'s
/// directories, as a host on that platform stores them.
fn stored_as(template: &str, input: &[(&str, &str)], format: PathFormat) -> JobParameterValues {
    let template = decode_job_template(doc(template), None, &CallerLimits::default()).unwrap();
    let input: JobParameterInputValues = input
        .iter()
        .map(|(k, v)| (k.to_string(), ExprValue::String(v.to_string())))
        .collect();
    let (dir, cwd) = match format {
        PathFormat::Windows => (r"C:\t", r"C:\work"),
        _ => ("/t", "/work"),
    };
    preprocess_job_parameters(
        &template,
        &input,
        &[],
        &PathParameterOptions {
            job_template_dir: dir,
            current_working_dir: cwd,
            path_format: format,
            allow_template_dir_walk_up: false,
            allow_uri_path_values: false,
        },
    )
    .unwrap()
}

#[test]
fn merge_keeps_a_stored_path_without_checking_its_rewritten_form() {
    // (constraint, stored value): the stored value meets the constraint; on
    // a POSIX host its preprocessed form (`/work/C:\...`) does not.
    let cases = [
        (r#"allowedValues: ['C:\w\out']"#, r"C:\w\out"),
        ("maxLength: 12", r"C:\work\out"),
    ];
    for (constraint, value) in cases {
        let t = OUT.replace("type: PATH}", &format!("type: PATH, {constraint}}}"));
        assert_ne!(t, OUT);
        let stored = stored_as(&t, &[("Out", value)], PathFormat::Windows);
        let merged = merge_parameters(&stored, &JobParameterInputValues::new(), &text(&t))
            .unwrap_or_else(|e| panic!("{constraint}: {e}"));
        assert_eq!(sorted(&merged), sorted(&stored), "{constraint}");
        let job = text(&t).preprocessed(merged).build().unwrap();
        assert_eq!(out_value(&job), value, "{constraint}");
    }
}

#[test]
fn a_kept_value_that_breaks_a_narrowed_constraint_fails_when_the_job_is_built() {
    let stored = stored_windows_out();
    let narrowed = OUT.replace("type: PATH}", "type: PATH, maxLength: 3}");
    let merged =
        merge_parameters(&stored, &JobParameterInputValues::new(), &text(&narrowed)).unwrap();
    assert_eq!(sorted(&merged), sorted(&stored));
    let err = text(&narrowed).preprocessed(merged).build().unwrap_err();
    assert_eq!(
        err.to_string(),
        "Validation error: Parameter 'Out': value length 11 exceeds maximum 3"
    );
}

#[test]
fn merge_still_preprocesses_a_requested_value_and_a_new_default() {
    // A requested value is preprocessed, constraints included.
    let t = OUT.replace("type: PATH}", "type: PATH, maxLength: 3}");
    let requested: JobParameterInputValues = [("Out".to_string(), ExprValue::String("o".into()))]
        .into_iter()
        .collect();
    let err = merge_parameters(&stored_windows_out(), &requested, &text(&t)).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "Validation error: Parameter 'Out': value length {} exceeds maximum 3",
            CWD.len() + 2
        )
    );
    // A new PATH default is joined to the template directory, beside a kept
    // value.
    let with_default = OUT.replace("steps:", "  - {name: D, type: PATH, default: d}\nsteps:");
    let merged = merge_parameters(
        &stored_windows_out(),
        &JobParameterInputValues::new(),
        &text(&with_default),
    )
    .unwrap();
    assert_eq!(
        sorted(&merged),
        [
            format!("D=PATH:{TEMPLATE_DIR}{SEP}d"),
            r"Out=PATH:C:\work\out".to_string()
        ]
    );
}

const PN: &str = r#"
specificationVersion: jobtemplate-2023-09
name: PN
parameterDefinitions:
  - {name: P, type: PATH}
  - {name: Count, type: INT}
steps:
  - name: A
    script: {actions: {onRun: {command: a, args: ["{{RawParam.P}}", "{{Param.Count}}"]}}}
"#;

#[test]
fn the_origin_split_builds_its_middle_job_from_the_old_stored_values() {
    let new_template = PN.replace(r#""{{Param.Count}}"]"#, r#""{{Param.Count}}", x]"#);
    assert_ne!(new_template, PN);
    // (format the values were stored with, stored P)
    for (format, p) in [
        (PathFormat::Windows, r"C:\w\out"),
        (PathFormat::Posix, "/w/out"),
    ] {
        let old = stored_as(PN, &[("P", p), ("Count", "1")], format);
        let new = stored_as(PN, &[("P", p), ("Count", "2")], format);
        let d = diff_templates(
            &text(PN).preprocessed(old),
            &text(&new_template).preprocessed(new),
            &DiffOptions::default(),
        )
        .unwrap();
        let got: Vec<String> = d.changes.iter().map(ToString::to_string).collect();
        assert_eq!(
            got,
            [
                "~ parameters[Count].value: 1 -> 2  [parameters]".to_string(),
                format!(
                    "~ steps[A].resolvedSymTab: Param.Count=1, RawParam.P={p} -> \
                     Param.Count=2, RawParam.P={p}  [parameters]"
                ),
                "~ steps[A].script.onRun.args: [{{RawParam.P}}, {{Param.Count}}] -> \
                 [{{RawParam.P}}, {{Param.Count}}, x]  [template]"
                    .to_string(),
            ],
            "{p}"
        );
    }
}

// ─── range expressions in the host encoding ─────────────────────────

#[test]
fn a_range_flagged_contiguous_writes_its_canonical_text() {
    use openjd_expr::RangeExpr;
    use openjd_model::job::{JobParameter, StepParameterSpace, TaskParamRange, TaskParameter};
    let plain: RangeExpr = "5,6".parse().unwrap();
    let flagged = plain.clone().with_contiguous(true);
    assert_eq!(flagged.to_string(), "5-6");
    let job = |r: &RangeExpr| {
        let mut job = text(OUT)
            .preprocessed(stored_windows_out())
            .build()
            .unwrap();
        job.parameters.insert(
            "R".into(),
            JobParameter {
                name: "R".into(),
                param_type: JobParameterType::RangeExpr,
                value: ExprValue::RangeExpr(r.clone()),
            },
        );
        job.steps[0].parameter_space = Some(StepParameterSpace {
            task_parameter_definitions: [(
                "F".to_string(),
                TaskParameter::Int {
                    range: TaskParamRange::RangeExpr(r.clone()),
                    chunks: None,
                },
            )]
            .into_iter()
            .collect(),
            combination: None,
        });
        job
    };
    let (a, b) = (job(&plain), job(&flagged));
    let doc = serde_json::to_value(JobDocument(&b)).unwrap();
    assert_eq!(
        doc["parameters"]["R"].to_string(),
        r#"{"name":"R","type":"RANGE_EXPR","value":"5,6"}"#
    );
    assert_eq!(
        doc["steps"][0]["parameterSpace"].to_string(),
        r#"{"combination":null,"taskParameterDefinitions":[{"name":"F","range":"5,6","type":"INT"}]}"#
    );
    assert_eq!(doc, serde_json::to_value(JobDocument(&a)).unwrap());
    assert_eq!(job_digest(&a).unwrap(), job_digest(&b).unwrap());
}

const PNX: &str = r#"
specificationVersion: jobtemplate-2023-09
name: PN
parameterDefinitions:
  - {name: P, type: PATH}
  - {name: Count, type: INT}
  - {name: X, type: STRING, default: s}
steps:
  - name: A
    script: {actions: {onRun: {command: a, args: ["{{RawParam.P}}", "{{Param.Count}}"]}}}
"#;

#[test]
fn the_middle_job_is_built_as_merge_parameters_builds() {
    // The new template drops `X`, or retypes it with a default; the host
    // merges the stored values with `Count` requested, as an update does.
    // Before, the middle job preprocessed the stored `X` against the new
    // template, failed, and marked every job change `both`.
    let dropped = PNX.replace("  - {name: X, type: STRING, default: s}\n", "");
    let retyped = PNX.replace(
        "{name: X, type: STRING, default: s}",
        "{name: X, type: INT, default: 3}",
    );
    let count = "~ parameters[Count].value: 1 -> 2  [parameters]";
    let symtab = "~ steps[A].resolvedSymTab: Param.Count=1, RawParam.P=/w/out -> \
                  Param.Count=2, RawParam.P=/w/out  [parameters]";
    let cases = [
        (
            &dropped,
            vec![
                "- parameterDefinitions[X]: {default: s, type: STRING}  [template]",
                count,
                "- parameters[X]: s  [template]",
                symtab,
            ],
        ),
        (
            &retyped,
            vec![
                "~ parameterDefinitions[X].default: s -> 3  [template]",
                "~ parameterDefinitions[X].type: STRING -> INT  [template]",
                count,
                "~ parameters[X].type: STRING -> INT  [template]",
                "~ parameters[X].value: \"s\" (STRING) -> 3 (INT)  [template]",
                symtab,
            ],
        ),
    ];
    for (new_template, want) in cases {
        assert_ne!(new_template, PNX);
        let stored = stored_as(PNX, &[("P", "/w/out"), ("Count", "1")], PathFormat::Posix);
        let requested: JobParameterInputValues =
            [("Count".to_string(), ExprValue::String("2".into()))]
                .into_iter()
                .collect();
        let merged = merge_parameters(&stored, &requested, &text(new_template)).unwrap();
        let d = diff_templates(
            &text(PNX).preprocessed(stored),
            &text(new_template).preprocessed(merged),
            &DiffOptions::default(),
        )
        .unwrap();
        let got: Vec<String> = d.changes.iter().map(ToString::to_string).collect();
        assert_eq!(got, want, "{new_template}");
        assert!(d.notes.is_empty(), "{:?}", d.notes);
    }
}

#[test]
fn a_preprocessed_side_and_an_input_side_never_count_as_the_same_parameters() {
    // Both sides give `Out` the text `out`, but the input side preprocesses
    // it against the working directory, so the parameters did change.
    let stored: JobParameterValues = [(
        "Out".to_string(),
        JobParameterValue {
            param_type: JobParameterType::Path,
            value: ExprValue::String("out".into()),
        },
    )]
    .into_iter()
    .collect();
    let d = diff_templates(
        &text(OUT).param("Out", "out"),
        &text(OUT).preprocessed(stored),
        &DiffOptions::default(),
    )
    .unwrap();
    let got: Vec<String> = d.changes.iter().map(ToString::to_string).collect();
    assert_eq!(
        got,
        [
            format!("~ parameters[Out].value: {CWD}{SEP}out -> out  [parameters]"),
            format!("~ steps[A].resolvedSymTab: RawParam.Out={CWD}{SEP}out -> RawParam.Out=out  [parameters]"),
        ]
    );
}

const Q_JOB: &str = r#"
specificationVersion: jobtemplate-2023-09
name: Q
steps:
  - name: A
    script: {actions: {onRun: {command: a}}}
"#;

const Q_ENV: &str = r#"
specificationVersion: environment-2023-09
parameterDefinitions:
  - {name: Q, type: STRING, allowedValues: [a, b]}
environment:
  name: E
  variables: {Q: "{{Param.Q}}"}
"#;

#[test]
fn a_kept_value_only_an_environment_template_declares_is_checked() {
    let side = || text(Q_JOB).environment(TemplateSource::Text(Q_ENV.to_string()));
    let stored: JobParameterValues = [(
        "Q".to_string(),
        JobParameterValue {
            param_type: JobParameterType::String,
            value: ExprValue::String("c".into()),
        },
    )]
    .into_iter()
    .collect();
    let merged = merge_parameters(&stored, &JobParameterInputValues::new(), &side()).unwrap();
    assert_eq!(sorted(&merged), ["Q=STRING:c"]);
    let err = side().preprocessed(merged.clone()).build().unwrap_err();
    assert_eq!(
        err.to_string(),
        "Validation error: Parameter 'Q': value 'c' is not in allowed values"
    );
    let err = diff_templates(
        &side().preprocessed(merged.clone()),
        &side().preprocessed(merged),
        &DiffOptions::default(),
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Validation error: old template: Parameter 'Q': value 'c' is not in allowed values"
    );
    // A value the environment template allows builds.
    let mut ok = stored;
    ok.get_mut("Q").unwrap().value = ExprValue::String("b".into());
    assert!(side().preprocessed(ok).build().is_ok());
}

#[test]
fn a_null_step_environments_list_is_no_step_environments() {
    let document = doc("steps: [{name: S, stepEnvironments: null}]");
    let fragment = template_step_fragment(&document, "S").unwrap();
    assert!(fragment.step_environments.is_empty());
    assert_eq!(
        fragment.step.to_string(),
        r#"{"name":"S","stepEnvironments":null}"#
    );
}

#[test]
fn a_kept_value_the_job_declares_and_an_environment_constrains_is_checked() {
    let job = Q_JOB.replace(
        "name: Q\n",
        "name: Q\nparameterDefinitions:\n  - {name: Q, type: STRING}\n",
    );
    let side = || text(&job).environment(TemplateSource::Text(Q_ENV.to_string()));
    let stored = |v: &str| -> JobParameterValues {
        [(
            "Q".to_string(),
            JobParameterValue {
                param_type: JobParameterType::String,
                value: ExprValue::String(v.into()),
            },
        )]
        .into_iter()
        .collect()
    };
    let merged = merge_parameters(&stored("c"), &JobParameterInputValues::new(), &side()).unwrap();
    assert_eq!(sorted(&merged), ["Q=STRING:c"]);
    let err = side().preprocessed(merged).build().unwrap_err();
    assert_eq!(
        err.to_string(),
        "Validation error: Parameter 'Q': value 'c' is not in allowed values"
    );
    assert!(side().preprocessed(stored("b")).build().is_ok());
}
