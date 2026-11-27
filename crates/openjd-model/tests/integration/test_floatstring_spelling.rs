// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! A `<floatstring>` a template writes keeps the decimal places and exponent
//! it was written with, and loses the notation that is redundant to its value:
//! redundant leading zeros and a leading `+` (Template Schemas §7.5 rules 1
//! and 2, openjd-specifications#180). That holds for every place a template
//! writes one whose text reaches a Task: a FLOAT range element, written
//! directly or resolved from a format string, a FLOAT parameter's `default`,
//! and a `LIST[FLOAT]` default item. A value submitted for a FLOAT parameter
//! keeps its original form as a value (2026-02 Expression Language §1.3.4),
//! and is rendered as a range element's `<floatstring>` once it is one.

use openjd_expr::path_mapping::PathFormat;
use openjd_model::{
    create_job, decode_job_template, preprocess_job_parameters, CallerLimits,
    JobParameterInputValues, StepParameterSpaceIterator,
};

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn options() -> openjd_model::PathParameterOptions<'static> {
    openjd_model::PathParameterOptions {
        job_template_dir: "/tmp/template",
        current_working_dir: "/tmp/cwd",
        allow_template_dir_walk_up: true,
        path_format: PathFormat::host(),
        allow_uri_path_values: true,
    }
}

/// The job parameter values and the rendered values of task parameter `F`
/// for `template` and the submitted `inputs`.
fn run(template: &str, inputs: &[(&str, &str)]) -> (openjd_model::JobParameterValues, Vec<String>) {
    let jt = decode_job_template(
        yaml_val(template),
        Some(&["EXPR"]),
        &CallerLimits::default(),
    )
    .unwrap();
    let mut input = JobParameterInputValues::new();
    for (k, v) in inputs {
        input.insert(k.to_string(), openjd_expr::ExprValue::String(v.to_string()));
    }
    let params = preprocess_job_parameters(&jt, &input, &[], &options()).unwrap();
    let job = create_job(&jt, &params, &jt.default_validation_context()).unwrap();
    let tasks: Vec<String> = match &job.steps[0].parameter_space {
        Some(ps) => StepParameterSpaceIterator::new(ps)
            .unwrap()
            .map(|t| t["F"].value.to_display_string())
            .collect(),
        None => Vec::new(),
    };
    (params, tasks)
}

fn range_template(range: &str) -> String {
    format!(
        "specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: T
parameterDefinitions:
- {{name: S, type: STRING, default: '+04.25'}}
steps:
- name: Step
  parameterSpace:
    taskParameterDefinitions:
    - {{name: F, type: FLOAT, range: {range}}}
  script:
    actions:
      onRun: {{command: foo}}
"
    )
}

#[test]
fn a_range_element_loses_a_leading_plus_and_redundant_zeros() {
    let (_, tasks) = run(
        &range_template(
            "['+4.25', '+02.50', '-02.50', '+0.50', '2.50E+2', '+1.0e-3', '{{Param.S}}']",
        ),
        &[],
    );
    assert_eq!(
        tasks,
        ["4.25", "2.50", "-2.50", "0.50", "2.50E+2", "1.0e-3", "4.25"]
    );
}

fn defaults_template() -> &'static str {
    "specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: T
parameterDefinitions:
- {name: Plus, type: FLOAT, default: '+4.25'}
- {name: Zeros, type: FLOAT, default: ' 02.50 '}
- {name: Both, type: FLOAT, default: '+00.50'}
- {name: Neg, type: FLOAT, default: '-02.50'}
- {name: List, type: 'LIST[FLOAT]', default: ['+02.50', 1.5, '3.500']}
- {name: Submitted, type: FLOAT}
steps:
- name: Step
  script:
    actions:
      onRun: {command: foo}
"
}

#[test]
fn a_default_loses_a_leading_plus_and_redundant_zeros() {
    let (params, _) = run(defaults_template(), &[("Submitted", "1.0")]);
    let shown = |name: &str| params[name].value.to_display_string();
    assert_eq!(shown("Plus"), "4.25");
    assert_eq!(shown("Zeros"), "2.50");
    assert_eq!(shown("Both"), "0.50");
    assert_eq!(shown("Neg"), "-2.50");
    assert_eq!(shown("List"), "[2.50, 1.5, 3.500]");
}

#[test]
fn a_submitted_value_keeps_its_original_form() {
    let (params, _) = run(defaults_template(), &[("Submitted", " +03.10 ")]);
    assert_eq!(params["Submitted"].value.to_display_string(), "+03.10");
}

/// A submitted value keeps its original form as a value, and is rendered as a
/// range element's `<floatstring>` once it is one: through a format string
/// element and through a whole-field range expression alike, so the two forms
/// give the same tasks.
#[test]
fn a_submitted_value_in_a_range_is_rendered_as_a_range_element() {
    let template = "specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: T
parameterDefinitions:
- {name: P, type: FLOAT}
- {name: LF, type: 'LIST[FLOAT]'}
steps:
- name: Step
  parameterSpace:
    taskParameterDefinitions:
    - {name: F, type: FLOAT, range: ['{{Param.P}}', '{{ [Param.P] }}']}
  script:
    actions:
      onRun: {command: foo}
- name: Whole
  parameterSpace:
    taskParameterDefinitions:
    - {name: F, type: FLOAT, range: '{{ Param.LF }}'}
  script:
    actions:
      onRun: {command: foo}
";
    let (params, tasks) = run(
        template,
        &[("P", "+03.10"), ("LF", "[\"+03.10\", \"2.50\"]")],
    );
    assert_eq!(params["P"].value.to_display_string(), "+03.10");
    assert_eq!(tasks, ["3.10", "3.10"]);

    let jt = decode_job_template(
        yaml_val(template),
        Some(&["EXPR"]),
        &CallerLimits::default(),
    )
    .unwrap();
    let mut input = JobParameterInputValues::new();
    for (k, v) in [("P", "+03.10"), ("LF", "[\"+03.10\", \"2.50\"]")] {
        input.insert(k.to_string(), openjd_expr::ExprValue::String(v.to_string()));
    }
    let params = preprocess_job_parameters(&jt, &input, &[], &options()).unwrap();
    let job = create_job(&jt, &params, &jt.default_validation_context()).unwrap();
    let whole: Vec<String> =
        StepParameterSpaceIterator::new(job.steps[1].parameter_space.as_ref().unwrap())
            .unwrap()
            .map(|t| t["F"].value.to_display_string())
            .collect();
    assert_eq!(whole, ["3.10", "2.50"]);
}

/// The per-element cap on a FLOAT range element's text counts the text after
/// a leading `+` and redundant zeros are dropped (`numeric::floatstring_text`),
/// which is what lands on a command line except for a negative zero, whose
/// `-` is dropped later by `Float64::with_str` and so is counted. 1024
/// characters (the default `max_task_param_string_len`) are kept; 1025 keep
/// only the value.
#[test]
fn the_range_element_text_cap_counts_the_rendered_text() {
    let at_cap = format!("1.{}", "0".repeat(1022));
    let over_cap = format!("1.{}", "0".repeat(1023));
    assert_eq!(at_cap.len(), 1024);
    let template = range_template(&format!("['+{at_cap}', '+{over_cap}']"));
    let (_, tasks) = run(&template, &[]);
    assert_eq!(tasks, [at_cap, "1.0".to_string()]);
}
