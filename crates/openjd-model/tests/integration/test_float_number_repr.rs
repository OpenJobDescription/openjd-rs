// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! A FLOAT given as a YAML or JSON number is written as CPython's `repr`
//! writes it, the same text as a computed float (Expression Language §1.3.4).
//! A FLOAT given as a string keeps its spelling (Template Schemas §7.5).
//!
//! The expected text comes from openjd-expr's CPython fixture
//! (`data/float_repr_cpython.txt`, see `scripts/generate_float_repr_fixture.py`).
//! Its one intended difference from `repr` is negative zero, which openjd
//! writes as `0.0`.

use openjd_expr::path_mapping::PathFormat;
use openjd_model::template::parse::{document_string_to_object, DocumentType};
use openjd_model::{
    decode_environment_template, decode_job_template, preprocess_job_parameters, CallerLimits,
    JobParameterInputValues, JobParameterValues, PathParameterOptions,
};

const FIXTURE: &str =
    include_str!("../../../openjd-expr/tests/integration/data/float_repr_cpython.txt");

/// `(repr, openjd text)` for each fixture value.
fn repr_cases() -> Vec<(&'static str, &'static str)> {
    FIXTURE
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let fields: Vec<&str> = line.split(' ').collect();
            match fields[..] {
                [_, repr] => (repr, repr),
                [_, repr, openjd] => (repr, openjd),
                _ => panic!("malformed fixture line: {line:?}"),
            }
        })
        .collect()
}

const STEPS: &str = r#"[{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}}]"#;

fn job_template_text(params: &[String]) -> String {
    let params = if params.is_empty() {
        String::new()
    } else {
        format!(r#""parameterDefinitions": [{}], "#, params.join(", "))
    };
    format!(
        r#"{{"specificationVersion": "jobtemplate-2023-09", "extensions": ["EXPR"], "name": "j",
        {params}"steps": {STEPS}}}"#
    )
}

/// Parse `text` as `doc_type`, the way `openjd run` reads a template file,
/// and resolve its parameters with no values given.
fn resolve(text: &str, doc_type: DocumentType) -> JobParameterValues {
    let value = document_string_to_object(text, doc_type, &CallerLimits::default()).unwrap();
    let template = decode_job_template(value, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let dirs = tempfile::TempDir::new().unwrap();
    let dir = dirs.path().to_str().unwrap();
    preprocess_job_parameters(
        &template,
        &JobParameterInputValues::new(),
        &[],
        &PathParameterOptions::new(dir, dir),
    )
    .unwrap()
}

/// Every fixture value as a FLOAT default written as a number, 50 parameters
/// per template (the 2023-09 limit), parsed from YAML and from JSON.
#[test]
fn float_default_given_as_a_number_writes_as_cpython_repr() {
    let cases = repr_cases();
    for doc_type in [DocumentType::Yaml, DocumentType::Json] {
        let mut mismatches = Vec::new();
        for chunk in cases.chunks(50) {
            let params: Vec<String> = chunk
                .iter()
                .enumerate()
                .map(|(i, (repr, _))| {
                    format!(r#"{{"name": "P{i}", "type": "FLOAT", "default": {repr}}}"#)
                })
                .collect();
            let values = resolve(&job_template_text(&params), doc_type);
            for (i, (repr, want)) in chunk.iter().enumerate() {
                let got = values[&format!("P{i}")].value.to_display_string();
                if got != *want {
                    mismatches.push(format!(
                        "{doc_type:?} default {repr}: got {got}, want {want}"
                    ));
                }
            }
        }
        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    }
}

/// Every fixture value as an item of one LIST[FLOAT] default written as
/// numbers, parsed from YAML and from JSON.
#[test]
fn list_float_default_items_given_as_numbers_write_as_cpython_repr() {
    let cases = repr_cases();
    let items: Vec<&str> = cases.iter().map(|(repr, _)| *repr).collect();
    let want = format!(
        "[{}]",
        cases
            .iter()
            .map(|(_, openjd)| *openjd)
            .collect::<Vec<_>>()
            .join(", ")
    );
    let param = format!(
        r#"{{"name": "L", "type": "LIST[FLOAT]", "default": [{}]}}"#,
        items.join(", ")
    );
    for doc_type in [DocumentType::Yaml, DocumentType::Json] {
        let values = resolve(&job_template_text(std::slice::from_ref(&param)), doc_type);
        let got = values["L"].value.to_display_string();
        if got != want {
            let got_items: Vec<&str> = got[1..got.len() - 1].split(", ").collect();
            let diffs: Vec<String> = got_items
                .iter()
                .zip(cases.iter())
                .filter(|(g, (_, w))| *g != w)
                .map(|(g, (r, w))| format!("{r}: got {g}, want {w}"))
                .collect();
            panic!("{doc_type:?}:\n{}", diffs.join("\n"));
        }
    }
}

#[test]
fn float_default_examples() {
    // The values from the original report, and a string default beside each:
    // a FLOAT given as a string keeps its spelling.
    let params: Vec<String> = [
        ("A", "2", "2.0"),
        ("B", "1e20", "1e+20"),
        ("C", "1e16", "1e+16"),
        ("D", "0.00001", "1e-05"),
        ("E", "1e15", "1000000000000000.0"),
        ("F", "-0.0", "0.0"),
        ("G", "1000000000000000.25", "1000000000000000.2"),
    ]
    .iter()
    .flat_map(|(name, number, _)| {
        [
            format!(r#"{{"name": "{name}", "type": "FLOAT", "default": {number}}}"#),
            format!(r#"{{"name": "{name}s", "type": "FLOAT", "default": "{number}"}}"#),
        ]
    })
    .collect();
    for doc_type in [DocumentType::Yaml, DocumentType::Json] {
        let values = resolve(&job_template_text(&params), doc_type);
        let text = |name: &str| values[name].value.to_display_string();
        for (name, number, want) in [
            ("A", "2", "2.0"),
            ("B", "1e20", "1e+20"),
            ("C", "1e16", "1e+16"),
            ("D", "0.00001", "1e-05"),
            ("E", "1e15", "1000000000000000.0"),
            ("F", "-0.0", "0.0"),
            ("G", "1000000000000000.25", "1000000000000000.2"),
        ] {
            assert_eq!(text(name), want, "{doc_type:?} {name}");
            // Zero has no sign, so a "-0.0" string is written "0.0" too.
            let spelling = if number == "-0.0" { "0.0" } else { number };
            assert_eq!(text(&format!("{name}s")), spelling, "{doc_type:?} {name}s");
        }
    }
}

#[test]
fn job_parameter_definition_default_value_writes_a_number_as_repr() {
    let text = job_template_text(&[
        r#"{"name": "N", "type": "FLOAT", "default": 2}"#.to_string(),
        r#"{"name": "S", "type": "FLOAT", "default": "2"}"#.to_string(),
    ]);
    let value =
        document_string_to_object(&text, DocumentType::Yaml, &CallerLimits::default()).unwrap();
    let template = decode_job_template(value, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let defaults: Vec<Option<String>> = template
        .parameter_definitions_list()
        .iter()
        .map(|p| p.default_value())
        .collect();
    assert_eq!(defaults, [Some("2.0".to_string()), Some("2".to_string())]);
}

#[test]
fn environment_template_float_default_given_as_a_number_writes_as_repr() {
    let env_text = r#"
specificationVersion: environment-2023-09
parameterDefinitions:
  - {name: E, type: FLOAT, default: 1e20}
  - {name: F, type: FLOAT, default: 2}
environment:
  name: env
  script:
    actions:
      onEnter: {command: echo}
"#;
    let env_value =
        document_string_to_object(env_text, DocumentType::Yaml, &CallerLimits::default()).unwrap();
    let env = decode_environment_template(env_value, None, &CallerLimits::default()).unwrap();
    let job_value = document_string_to_object(
        &job_template_text(&[]),
        DocumentType::Yaml,
        &CallerLimits::default(),
    )
    .unwrap();
    let template =
        decode_job_template(job_value, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let dirs = tempfile::TempDir::new().unwrap();
    let dir = dirs.path().to_str().unwrap();
    let mut options = PathParameterOptions::new(dir, dir);
    options.path_format = PathFormat::host();
    let values =
        preprocess_job_parameters(&template, &JobParameterInputValues::new(), &[env], &options)
            .unwrap();
    assert_eq!(values["E"].value.to_display_string(), "1e+20");
    assert_eq!(values["F"].value.to_display_string(), "2.0");
}
