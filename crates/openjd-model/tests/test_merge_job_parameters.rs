// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test_merge_job_parameters.py
//!
//! Tests merge_job_parameter_definitions and constraint validation
//! when merging parameters from multiple environment templates and a job template.

use openjd_expr::path_mapping::PathFormat;
use openjd_model::{
    decode_environment_template, decode_job_template, merge_job_parameter_definitions,
    preprocess_job_parameters, CallerLimits, JobParameterInputValues, JobParameterType,
};

struct TestDirs {
    _root: tempfile::TempDir,
    dir: std::path::PathBuf,
}
impl TestDirs {
    fn new() -> Self {
        let root = tempfile::TempDir::new().unwrap();
        let dir = root.path().to_path_buf();
        Self { _root: root, dir }
    }
    fn path(&self) -> &str {
        self.dir.to_str().unwrap()
    }
}

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn job_template(params: &str) -> serde_json::Value {
    yaml_val(&format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "parameterDefinitions": [{params}],
        "steps": [{{"name": "Test", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}]
    }}"#
    ))
}

fn job_template_no_params() -> serde_json::Value {
    yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{"name": "Test", "script": {"actions": {"onRun": {"command": "foo"}}}}]
    }"#,
    )
}

fn env_template(name: &str, params: &str) -> serde_json::Value {
    yaml_val(&format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [{params}],
        "environment": {{"name": "{name}", "script": {{"actions": {{"onEnter": {{"command": "bar"}}}}}}}}
    }}"#
    ))
}

// ══════════════════════════════════════════════════════════════
// merge_job_parameter_definitions — basic merge tests
// ══════════════════════════════════════════════════════════════

#[test]
fn merge_simple_int() {
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "INT"}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template("Env", r#"{"name": "foo", "type": "INT"}"#),
        None,
    )
    .unwrap();
    let merged = merge_job_parameter_definitions(&jt, &[et]).unwrap();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].name, "foo");
    assert_eq!(merged[0].param_type, JobParameterType::Int);
}

#[test]
fn merge_simple_float() {
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "FLOAT"}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template("Env", r#"{"name": "foo", "type": "FLOAT"}"#),
        None,
    )
    .unwrap();
    let merged = merge_job_parameter_definitions(&jt, &[et]).unwrap();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].param_type, JobParameterType::Float);
}

#[test]
fn merge_simple_string() {
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "STRING"}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template("Env", r#"{"name": "foo", "type": "STRING"}"#),
        None,
    )
    .unwrap();
    let merged = merge_job_parameter_definitions(&jt, &[et]).unwrap();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].param_type, JobParameterType::String);
}

#[test]
fn merge_simple_path() {
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "PATH"}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template("Env", r#"{"name": "foo", "type": "PATH"}"#),
        None,
    )
    .unwrap();
    let merged = merge_job_parameter_definitions(&jt, &[et]).unwrap();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].param_type, JobParameterType::Path);
}

// ══════════════════════════════════════════════════════════════
// merge — type conflicts
// ══════════════════════════════════════════════════════════════

#[test]
fn merge_type_conflict_int_float() {
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "FLOAT"}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template("Env", r#"{"name": "foo", "type": "INT"}"#),
        None,
    )
    .unwrap();
    let err = merge_job_parameter_definitions(&jt, &[et]).unwrap_err();
    assert!(err.to_string().contains("conflicting types"), "got: {err}");
}

// ══════════════════════════════════════════════════════════════
// merge — default precedence (job template wins)
// ══════════════════════════════════════════════════════════════

#[test]
fn merge_job_template_default_wins() {
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "INT", "default": "10"}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template("Env", r#"{"name": "foo", "type": "INT", "default": "5"}"#),
        None,
    )
    .unwrap();
    let merged = merge_job_parameter_definitions(&jt, &[et]).unwrap();
    assert_eq!(merged[0].default.as_deref(), Some("10"));
}

#[test]
fn merge_env_default_used_when_job_has_none() {
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "INT"}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template("Env", r#"{"name": "foo", "type": "INT", "default": "8"}"#),
        None,
    )
    .unwrap();
    let merged = merge_job_parameter_definitions(&jt, &[et]).unwrap();
    assert_eq!(merged[0].default.as_deref(), Some("8"));
}

// ══════════════════════════════════════════════════════════════
// merge — only job template (no environments)
// ══════════════════════════════════════════════════════════════

#[test]
fn merge_only_job_template() {
    let jt = decode_job_template(
        job_template(r#"{"name": "Foo", "type": "INT", "maxValue": 50}, {"name": "Bar", "type": "STRING", "minLength": 1}"#),
        None,
        &CallerLimits::default(),
    ).unwrap();
    let merged = merge_job_parameter_definitions(&jt, &[]).unwrap();
    assert_eq!(merged.len(), 2);
    assert!(merged.iter().any(|p| p.name == "Foo"));
    assert!(merged.iter().any(|p| p.name == "Bar"));
}

// ══════════════════════════════════════════════════════════════
// merge — two environments, no job template params
// ══════════════════════════════════════════════════════════════

#[test]
fn merge_two_environments() {
    let jt = decode_job_template(job_template_no_params(), None, &CallerLimits::default()).unwrap();
    let et1 = decode_environment_template(
        env_template(
            "Env1",
            r#"{"name": "Foo", "type": "INT"}, {"name": "Bar", "type": "STRING"}"#,
        ),
        None,
    )
    .unwrap();
    let et2 = decode_environment_template(
        env_template(
            "Env2",
            r#"{"name": "Foo", "type": "INT"}, {"name": "Bar", "type": "STRING"}"#,
        ),
        None,
    )
    .unwrap();
    let merged = merge_job_parameter_definitions(&jt, &[et1, et2]).unwrap();
    assert_eq!(merged.len(), 2);
}

// ══════════════════════════════════════════════════════════════
// merge — environments + job template with constraint merging
// ══════════════════════════════════════════════════════════════

#[test]
fn merge_env_and_job_constraints_correct_order() {
    let jt = decode_job_template(
        job_template(
            r#"{"name": "Foo", "type": "INT", "minValue": 5, "maxValue": 10, "default": "8"}"#,
        ),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et1 = decode_environment_template(
        env_template(
            "Env1",
            r#"{"name": "Foo", "type": "INT", "minValue": 1, "default": "3"}"#,
        ),
        None,
    )
    .unwrap();
    let et2 = decode_environment_template(
        env_template("Env2", r#"{"name": "Foo", "type": "INT", "maxValue": 20}"#),
        None,
    )
    .unwrap();
    // Job template is processed last, so its default wins
    let merged = merge_job_parameter_definitions(&jt, &[et1, et2]).unwrap();
    assert_eq!(merged[0].default.as_deref(), Some("8"));
}

// ══════════════════════════════════════════════════════════════
// Constraint validation via preprocess (which calls validate_merged_constraints)
// ══════════════════════════════════════════════════════════════

#[test]
fn constraint_non_compatible_int_value_range() {
    let td = TestDirs::new();
    // Env: minValue=10, maxValue=20; Job: minValue=5, maxValue=8
    // Merged: min=10, max=8 → not satisfiable
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "INT", "minValue": 5, "maxValue": 8}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template(
            "Env",
            r#"{"name": "foo", "type": "INT", "minValue": 10, "maxValue": 20}"#,
        ),
        None,
    )
    .unwrap();
    let input = JobParameterInputValues::new();
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("no valid range"), "got: {err}");
}

#[test]
fn constraint_non_compatible_string_length() {
    let td = TestDirs::new();
    // Env: minLength=10, maxLength=20; Job: minLength=5, maxLength=8
    // Merged: min=10, max=8 → not satisfiable
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "STRING", "minLength": 5, "maxLength": 8}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template(
            "Env",
            r#"{"name": "foo", "type": "STRING", "minLength": 10, "maxLength": 20}"#,
        ),
        None,
    )
    .unwrap();
    let input = JobParameterInputValues::new();
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("no valid length"), "got: {err}");
}

#[test]
fn constraint_non_compatible_path_object_type() {
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "PATH", "objectType": "FILE"}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template(
            "Env",
            r#"{"name": "foo", "type": "PATH", "objectType": "DIRECTORY"}"#,
        ),
        None,
    )
    .unwrap();
    let err = merge_job_parameter_definitions(&jt, &[et]).unwrap_err();
    assert!(err.to_string().contains("objectType"), "got: {err}");
}

#[test]
fn constraint_non_compatible_path_data_flow() {
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "PATH", "dataFlow": "OUT"}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template(
            "Env",
            r#"{"name": "foo", "type": "PATH", "dataFlow": "IN"}"#,
        ),
        None,
    )
    .unwrap();
    let err = merge_job_parameter_definitions(&jt, &[et]).unwrap_err();
    assert!(err.to_string().contains("dataFlow"), "got: {err}");
}

#[test]
fn constraint_compatible_path_same_object_type() {
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "PATH", "objectType": "FILE", "dataFlow": "IN"}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template(
            "Env",
            r#"{"name": "foo", "type": "PATH", "objectType": "FILE", "dataFlow": "IN"}"#,
        ),
        None,
    )
    .unwrap();
    let merged = merge_job_parameter_definitions(&jt, &[et]).unwrap();
    assert_eq!(merged.len(), 1);
    assert_eq!(
        merged[0].object_type,
        Some(openjd_model::types::ObjectType::File)
    );
    assert_eq!(merged[0].data_flow, Some(openjd_model::types::DataFlow::In));
}

// ══════════════════════════════════════════════════════════════
// Category A: env-env merge conflicts per §1.2.1
// ══════════════════════════════════════════════════════════════

#[test]
fn merge_env_env_type_conflict() {
    // Two env templates define same param with different types → conflict
    let jt = decode_job_template(job_template_no_params(), None, &CallerLimits::default()).unwrap();
    let et1 = decode_environment_template(
        env_template("Env1", r#"{"name": "foo", "type": "INT"}"#),
        None,
    )
    .unwrap();
    let et2 = decode_environment_template(
        env_template("Env2", r#"{"name": "foo", "type": "STRING"}"#),
        None,
    )
    .unwrap();
    let err = merge_job_parameter_definitions(&jt, &[et1, et2]).unwrap_err();
    assert!(err.to_string().contains("conflicting types"), "got: {err}");
}

#[test]
fn merge_env_env_object_type_conflict() {
    // Two env templates define same PATH param with different objectType → conflict
    let jt = decode_job_template(job_template_no_params(), None, &CallerLimits::default()).unwrap();
    let et1 = decode_environment_template(
        env_template(
            "Env1",
            r#"{"name": "foo", "type": "PATH", "objectType": "FILE"}"#,
        ),
        None,
    )
    .unwrap();
    let et2 = decode_environment_template(
        env_template(
            "Env2",
            r#"{"name": "foo", "type": "PATH", "objectType": "DIRECTORY"}"#,
        ),
        None,
    )
    .unwrap();
    let err = merge_job_parameter_definitions(&jt, &[et1, et2]).unwrap_err();
    assert!(err.to_string().contains("objectType"), "got: {err}");
}

#[test]
fn merge_env_env_data_flow_conflict() {
    // Two env templates define same PATH param with different dataFlow → conflict
    let jt = decode_job_template(job_template_no_params(), None, &CallerLimits::default()).unwrap();
    let et1 = decode_environment_template(
        env_template(
            "Env1",
            r#"{"name": "foo", "type": "PATH", "dataFlow": "IN"}"#,
        ),
        None,
    )
    .unwrap();
    let et2 = decode_environment_template(
        env_template(
            "Env2",
            r#"{"name": "foo", "type": "PATH", "dataFlow": "OUT"}"#,
        ),
        None,
    )
    .unwrap();
    let err = merge_job_parameter_definitions(&jt, &[et1, et2]).unwrap_err();
    assert!(err.to_string().contains("dataFlow"), "got: {err}");
}

#[test]
fn merge_env_env_same_object_type_ok() {
    // Two env templates with same objectType → no conflict
    let jt = decode_job_template(job_template_no_params(), None, &CallerLimits::default()).unwrap();
    let et1 = decode_environment_template(
        env_template(
            "Env1",
            r#"{"name": "foo", "type": "PATH", "objectType": "FILE"}"#,
        ),
        None,
    )
    .unwrap();
    let et2 = decode_environment_template(
        env_template(
            "Env2",
            r#"{"name": "foo", "type": "PATH", "objectType": "FILE"}"#,
        ),
        None,
    )
    .unwrap();
    let merged = merge_job_parameter_definitions(&jt, &[et1, et2]).unwrap();
    assert_eq!(
        merged[0].object_type,
        Some(openjd_model::types::ObjectType::File)
    );
}

#[test]
fn merge_env_env_same_data_flow_ok() {
    let _td = TestDirs::new();
    let jt = decode_job_template(job_template_no_params(), None, &CallerLimits::default()).unwrap();
    let et1 = decode_environment_template(
        env_template(
            "Env1",
            r#"{"name": "foo", "type": "PATH", "dataFlow": "IN"}"#,
        ),
        None,
    )
    .unwrap();
    let et2 = decode_environment_template(
        env_template(
            "Env2",
            r#"{"name": "foo", "type": "PATH", "dataFlow": "IN"}"#,
        ),
        None,
    )
    .unwrap();
    let merged = merge_job_parameter_definitions(&jt, &[et1, et2]).unwrap();
    assert_eq!(merged[0].data_flow, Some(openjd_model::types::DataFlow::In));
}

// ══════════════════════════════════════════════════════════════
// Category D: validate_merged_constraints — FLOAT range
// ══════════════════════════════════════════════════════════════

#[test]
fn constraint_float_incompatible_range() {
    let td = TestDirs::new();
    // Env: minValue=10.0, maxValue=20.0; Job: minValue=1.0, maxValue=5.0
    // Merged: min=10.0, max=5.0 → not satisfiable
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "FLOAT", "minValue": 1.0, "maxValue": 5.0}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template(
            "Env",
            r#"{"name": "foo", "type": "FLOAT", "minValue": 10.0, "maxValue": 20.0}"#,
        ),
        None,
    )
    .unwrap();
    let input = JobParameterInputValues::new();
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("no valid range"), "got: {err}");
}

#[test]
fn constraint_float_compatible_range() {
    let td = TestDirs::new();
    // Env: minValue=1.0, maxValue=20.0; Job: minValue=5.0, maxValue=15.0
    // Merged: min=5.0, max=15.0 → satisfiable
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "FLOAT", "minValue": 5.0, "maxValue": 15.0, "default": "10.0"}"#), None,
        &CallerLimits::default(),
    ).unwrap();
    let et = decode_environment_template(
        env_template(
            "Env",
            r#"{"name": "foo", "type": "FLOAT", "minValue": 1.0, "maxValue": 20.0}"#,
        ),
        None,
    )
    .unwrap();
    let input = JobParameterInputValues::new();
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    );
    assert!(result.is_ok(), "got: {}", result.unwrap_err());
}

#[test]
fn constraint_float_boundary_equal_range() {
    let td = TestDirs::new();
    // Merged: min=10.0, max=10.0 → satisfiable (single valid value)
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "FLOAT", "minValue": 10.0, "default": "10.0"}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template(
            "Env",
            r#"{"name": "foo", "type": "FLOAT", "maxValue": 10.0}"#,
        ),
        None,
    )
    .unwrap();
    let input = JobParameterInputValues::new();
    assert!(preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .is_ok());
}

// ══════════════════════════════════════════════════════════════
// Category D: validate_merged_constraints — STRING/PATH allowedValues
// ══════════════════════════════════════════════════════════════

#[test]
fn constraint_string_no_common_allowed_values() {
    let td = TestDirs::new();
    // Env: allowedValues=["a","b"]; Job: allowedValues=["c","d"]
    // Intersection is empty → error
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "STRING", "allowedValues": ["c", "d"]}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template(
            "Env",
            r#"{"name": "foo", "type": "STRING", "allowedValues": ["a", "b"]}"#,
        ),
        None,
    )
    .unwrap();
    let input = JobParameterInputValues::new();
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("no common values"), "got: {err}");
}

#[test]
fn constraint_string_common_allowed_values_ok() {
    let td = TestDirs::new();
    // Env: allowedValues=["a","b","c"]; Job: allowedValues=["b","c","d"]
    // Intersection is ["b","c"] → ok
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "STRING", "allowedValues": ["b", "c", "d"], "default": "b"}"#), None,
        &CallerLimits::default(),
    ).unwrap();
    let et = decode_environment_template(
        env_template(
            "Env",
            r#"{"name": "foo", "type": "STRING", "allowedValues": ["a", "b", "c"]}"#,
        ),
        None,
    )
    .unwrap();
    let input = JobParameterInputValues::new();
    assert!(preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .is_ok());
}

#[test]
fn constraint_string_default_not_in_merged_allowed() {
    let td = TestDirs::new();
    // Env: allowedValues=["a","b"]; Job: allowedValues=["b","c"], default="c"
    // Intersection is ["b"], but default "c" is not in intersection → error
    let jt = decode_job_template(
        job_template(
            r#"{"name": "foo", "type": "STRING", "allowedValues": ["b", "c"], "default": "c"}"#,
        ),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template(
            "Env",
            r#"{"name": "foo", "type": "STRING", "allowedValues": ["a", "b"]}"#,
        ),
        None,
    )
    .unwrap();
    let input = JobParameterInputValues::new();
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("not in merged allowedValues"),
        "got: {err}"
    );
}

#[test]
fn constraint_string_default_in_merged_allowed_ok() {
    let td = TestDirs::new();
    // Env: allowedValues=["a","b"]; Job: allowedValues=["b","c"], default="b"
    // Intersection is ["b"], default "b" is in intersection → ok
    let jt = decode_job_template(
        job_template(
            r#"{"name": "foo", "type": "STRING", "allowedValues": ["b", "c"], "default": "b"}"#,
        ),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template(
            "Env",
            r#"{"name": "foo", "type": "STRING", "allowedValues": ["a", "b"]}"#,
        ),
        None,
    )
    .unwrap();
    let input = JobParameterInputValues::new();
    assert!(preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .is_ok());
}

// ══════════════════════════════════════════════════════════════
// Category D: validate_merged_constraints — INT min-only / max-only
// ══════════════════════════════════════════════════════════════

#[test]
fn constraint_int_min_only_max_only_compatible() {
    let td = TestDirs::new();
    // Env: minValue=5; Job: maxValue=10 → merged: [5,10] → ok
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "INT", "maxValue": 10, "default": "7"}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template("Env", r#"{"name": "foo", "type": "INT", "minValue": 5}"#),
        None,
    )
    .unwrap();
    let input = JobParameterInputValues::new();
    assert!(preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .is_ok());
}

#[test]
fn constraint_int_min_only_max_only_incompatible() {
    let td = TestDirs::new();
    // Env: minValue=15; Job: maxValue=10 → merged: min=15 > max=10 → error
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "INT", "maxValue": 10}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template("Env", r#"{"name": "foo", "type": "INT", "minValue": 15}"#),
        None,
    )
    .unwrap();
    let input = JobParameterInputValues::new();
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("no valid range"), "got: {err}");
}

// ══════════════════════════════════════════════════════════════
// Finding #1: User-provided values must be validated against
// ALL templates' constraints, not just the job template's.
// ══════════════════════════════════════════════════════════════

#[test]
fn input_value_rejected_by_env_int_allowed_values() {
    // Env allows [1,2,3], Job allows [1,2,3,4,5]. User provides 4.
    // Merged allowedValues = [1,2,3], so 4 must be rejected.
    let td = TestDirs::new();
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "INT", "allowedValues": [1,2,3,4,5]}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template(
            "Env",
            r#"{"name": "foo", "type": "INT", "allowedValues": [1,2,3]}"#,
        ),
        None,
    )
    .unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("foo".into(), openjd_expr::ExprValue::Int(4));
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("foo"), "got: {err}");
}

#[test]
fn input_value_rejected_by_env_int_min_value() {
    // Env: minValue=10; Job: minValue=1. User provides 5.
    // Merged min=10, so 5 must be rejected.
    let td = TestDirs::new();
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "INT", "minValue": 1}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template("Env", r#"{"name": "foo", "type": "INT", "minValue": 10}"#),
        None,
    )
    .unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("foo".into(), openjd_expr::ExprValue::Int(5));
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("foo"), "got: {err}");
}

#[test]
fn input_value_rejected_by_env_int_max_value() {
    // Env: maxValue=10; Job: maxValue=20. User provides 15.
    // Merged max=10, so 15 must be rejected.
    let td = TestDirs::new();
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "INT", "maxValue": 20}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template("Env", r#"{"name": "foo", "type": "INT", "maxValue": 10}"#),
        None,
    )
    .unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("foo".into(), openjd_expr::ExprValue::Int(15));
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("foo"), "got: {err}");
}

#[test]
fn input_value_rejected_by_env_float_min_value() {
    // Env: minValue=10.0; Job: minValue=1.0. User provides 5.0.
    let td = TestDirs::new();
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "FLOAT", "minValue": 1.0}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template(
            "Env",
            r#"{"name": "foo", "type": "FLOAT", "minValue": 10.0}"#,
        ),
        None,
    )
    .unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "foo".into(),
        openjd_expr::ExprValue::Float(openjd_expr::value::Float64::new(5.0).unwrap()),
    );
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("foo"), "got: {err}");
}

#[test]
fn input_value_rejected_by_env_string_allowed_values() {
    // Env allows ["a","b"], Job allows ["a","b","c"]. User provides "c".
    // Merged = ["a","b"], so "c" must be rejected.
    let td = TestDirs::new();
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "STRING", "allowedValues": ["a","b","c"]}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template(
            "Env",
            r#"{"name": "foo", "type": "STRING", "allowedValues": ["a","b"]}"#,
        ),
        None,
    )
    .unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("foo".into(), openjd_expr::ExprValue::String("c".into()));
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("foo"), "got: {err}");
}

#[test]
fn input_value_rejected_by_env_string_max_length() {
    // Env: maxLength=5; Job: maxLength=10. User provides "abcdefgh" (len 8).
    let td = TestDirs::new();
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "STRING", "maxLength": 10}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template(
            "Env",
            r#"{"name": "foo", "type": "STRING", "maxLength": 5}"#,
        ),
        None,
    )
    .unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "foo".into(),
        openjd_expr::ExprValue::String("abcdefgh".into()),
    );
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("foo"), "got: {err}");
}

#[test]
fn input_value_rejected_by_env_string_min_length() {
    // Env: minLength=5; Job: minLength=1. User provides "ab" (len 2).
    let td = TestDirs::new();
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "STRING", "minLength": 1}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template(
            "Env",
            r#"{"name": "foo", "type": "STRING", "minLength": 5}"#,
        ),
        None,
    )
    .unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("foo".into(), openjd_expr::ExprValue::String("ab".into()));
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("foo"), "got: {err}");
}

#[test]
fn input_value_accepted_when_within_all_constraints() {
    // Env allows [1,2,3], Job allows [1,2,3,4,5]. User provides 2.
    // Merged = [1,2,3], 2 is in merged → accepted.
    let td = TestDirs::new();
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "INT", "allowedValues": [1,2,3,4,5]}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template(
            "Env",
            r#"{"name": "foo", "type": "INT", "allowedValues": [1,2,3]}"#,
        ),
        None,
    )
    .unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("foo".into(), openjd_expr::ExprValue::Int(2));
    assert!(preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .is_ok());
}

#[test]
fn input_value_rejected_by_env_only_constraint_no_job_constraint() {
    // Env: maxValue=10; Job: no constraints. User provides 15.
    // Merged max=10, so 15 must be rejected.
    let td = TestDirs::new();
    let jt = decode_job_template(
        job_template(r#"{"name": "foo", "type": "INT"}"#),
        None,
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        env_template("Env", r#"{"name": "foo", "type": "INT", "maxValue": 10}"#),
        None,
    )
    .unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("foo".into(), openjd_expr::ExprValue::Int(15));
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.path(),
            current_working_dir: td.path(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("foo"), "got: {err}");
}

// ══════════════════════════════════════════════════════════════
// merge — LIST and BOOL parameter types (requires EXPR extension)
// ══════════════════════════════════════════════════════════════

fn expr_job_template(params: &str) -> serde_json::Value {
    yaml_val(&format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "extensions": ["EXPR"],
        "parameterDefinitions": [{params}],
        "steps": [{{"name": "Test", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}]
    }}"#
    ))
}

fn expr_env_template(name: &str, params: &str) -> serde_json::Value {
    yaml_val(&format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [{params}],
        "environment": {{"name": "{name}", "script": {{"actions": {{"onEnter": {{"command": "bar"}}}}}}}}
    }}"#
    ))
}

#[test]
fn merge_simple_bool() {
    let jt = decode_job_template(
        expr_job_template(r#"{"name": "foo", "type": "BOOL"}"#),
        Some(&["EXPR"]),
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        expr_env_template("Env", r#"{"name": "foo", "type": "BOOL"}"#),
        Some(&["EXPR"]),
    )
    .unwrap();
    let merged = merge_job_parameter_definitions(&jt, &[et]).unwrap();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].param_type, JobParameterType::Bool);
}

#[test]
fn merge_simple_list_string() {
    let jt = decode_job_template(
        expr_job_template(r#"{"name": "foo", "type": "LIST[STRING]"}"#),
        Some(&["EXPR"]),
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        expr_env_template("Env", r#"{"name": "foo", "type": "LIST[STRING]"}"#),
        Some(&["EXPR"]),
    )
    .unwrap();
    let merged = merge_job_parameter_definitions(&jt, &[et]).unwrap();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].param_type, JobParameterType::ListString);
}

#[test]
fn merge_simple_list_int() {
    let jt = decode_job_template(
        expr_job_template(r#"{"name": "foo", "type": "LIST[INT]"}"#),
        Some(&["EXPR"]),
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        expr_env_template("Env", r#"{"name": "foo", "type": "LIST[INT]"}"#),
        Some(&["EXPR"]),
    )
    .unwrap();
    let merged = merge_job_parameter_definitions(&jt, &[et]).unwrap();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].param_type, JobParameterType::ListInt);
}

#[test]
fn merge_type_conflict_bool_int() {
    let jt = decode_job_template(
        expr_job_template(r#"{"name": "foo", "type": "BOOL"}"#),
        Some(&["EXPR"]),
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        expr_env_template("Env", r#"{"name": "foo", "type": "INT"}"#),
        Some(&["EXPR"]),
    )
    .unwrap();
    let err = merge_job_parameter_definitions(&jt, &[et]).unwrap_err();
    assert!(err.to_string().contains("conflicting types"), "got: {err}");
}

#[test]
fn merge_type_conflict_list_string_string() {
    let jt = decode_job_template(
        expr_job_template(r#"{"name": "foo", "type": "LIST[STRING]"}"#),
        Some(&["EXPR"]),
        &CallerLimits::default(),
    )
    .unwrap();
    let et = decode_environment_template(
        expr_env_template("Env", r#"{"name": "foo", "type": "STRING"}"#),
        Some(&["EXPR"]),
    )
    .unwrap();
    let err = merge_job_parameter_definitions(&jt, &[et]).unwrap_err();
    assert!(err.to_string().contains("conflicting types"), "got: {err}");
}
