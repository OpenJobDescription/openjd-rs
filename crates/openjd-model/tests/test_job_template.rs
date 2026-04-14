// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test/openjd/model/v2023_09/test_job_template.py
//!
//! Gold standard: failure tests assert the full error message including path.

use openjd_model::decode_job_template;

fn yaml_val(s: &str) -> serde_yaml::Value {
    serde_yaml::from_str(s).unwrap()
}

fn decode_ok(s: &str) {
    let v = yaml_val(s);
    decode_job_template(v, None).unwrap_or_else(|_| panic!("Expected success for: {s}"));
}

fn check_err(s: &str, expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_job_template(v, None).expect_err(&format!("Expected error for: {s}"));
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

// === Success cases ===

#[test]
fn test_minimum_required() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [{"name": "StepName", "script": {"actions": {"onRun": {"command": "foo"}}}}]
    }"#,
    );
}

#[test]
fn test_with_description() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [{"name": "StepName", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "description": "some text"
    }"#,
    );
}

#[test]
fn test_with_parameters() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [{"name": "StepName", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "parameterDefinitions": [{"name": "P", "type": "INT"}]
    }"#,
    );
}

#[test]
fn test_with_environments() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [{"name": "StepName", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{"name": "Env1", "script": {"actions": {"onEnter": {"command": "foo"}}}}]
    }"#,
    );
}

#[test]
fn test_with_schema() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [{"name": "StepName", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "$schema": "some text"
    }"#,
    );
}

#[test]
fn test_with_step_dependencies() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [
            {"name": "StepOne", "script": {"actions": {"onRun": {"command": "foo"}}}},
            {"name": "StepTwo", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "StepOne"}]}
        ]
    }"#,
    );
}

#[test]
fn test_with_step_dependencies_reverse_order() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [
            {"name": "StepOne", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "StepTwo"}]},
            {"name": "StepTwo", "script": {"actions": {"onRun": {"command": "foo"}}}}
        ]
    }"#,
    );
}

#[test]
fn test_job_and_step_env_names_differ() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "jobEnvironments": [{"name": "JobEnv", "script": {"actions": {"onEnter": {"command": "foo"}}}}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}},
                   "stepEnvironments": [{"name": "StepEnv", "script": {"actions": {"onEnter": {"command": "foo"}}}}]}]
    }"#,
    );
}

// === Failure cases ===

#[test]
fn test_empty_steps() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": []
    }"#,
        &[
            "1 validation errors for JobTemplate\n",
            "JobTemplate: must have at least one step.",
        ],
    );
}

#[test]
fn test_missing_steps() {
    // serde rejects missing required field
    let v = yaml_val(r#"{"specificationVersion": "jobtemplate-2023-09", "name": "Foo"}"#);
    assert!(decode_job_template(v, None).is_err());
}

#[test]
fn test_unknown_key() {
    // serde deny_unknown_fields
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "unresolved": "key"
    }"#,
    );
    assert!(decode_job_template(v, None).is_err());
}

#[test]
fn test_duplicate_step_names() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [
            {"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}},
            {"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}
        ]
    }"#,
        &["steps[1] -> name:\n\tduplicate step name: 'S'"],
    );
}

#[test]
fn test_empty_parameters() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "parameterDefinitions": []
    }"#,
        &["parameterDefinitions:\n\tif provided, must contain at least one element."],
    );
}

#[test]
fn test_duplicate_parameter_names() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "parameterDefinitions": [{"name": "P", "type": "INT"}, {"name": "P", "type": "INT"}]
    }"#,
        &["parameterDefinitions[1]:\n\tduplicate parameter name: 'P'"],
    );
}

#[test]
fn test_empty_environments() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": []
    }"#,
        &["jobEnvironments:\n\tmust not be empty."],
    );
}

#[test]
fn test_duplicate_environment_names() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [
            {"name": "E", "script": {"actions": {"onEnter": {"command": "foo"}}}},
            {"name": "E", "script": {"actions": {"onEnter": {"command": "foo"}}}}
        ]
    }"#,
        &["jobEnvironments[1]:\n\tduplicate environment name: 'E'"],
    );
}

#[test]
fn test_step_dependency_cycle() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [
            {"name": "A", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "C"}]},
            {"name": "B", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "A"}]},
            {"name": "C", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "B"}]}
        ]
    }"#,
        &["step dependencies contain a cycle."],
    );
}

#[test]
fn test_step_dependency_unknown_step() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [
            {"name": "A", "script": {"actions": {"onRun": {"command": "foo"}}}},
            {"name": "B", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "Unknown"}]}
        ]
    }"#,
        &["steps[1] -> dependencies[0]:\n\tdependency 'Unknown' not found."],
    );
}

#[test]
fn test_name_with_control_char() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job\u001fName",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
    }"#,
        &["name:\n\tcontains control characters."],
    );
}

#[test]
fn test_step_env_name_duplicates_job_env() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "jobEnvironments": [{"name": "Shared", "script": {"actions": {"onEnter": {"command": "foo"}}}}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}},
                   "stepEnvironments": [{"name": "Shared", "script": {"actions": {"onEnter": {"command": "foo"}}}}]}]
    }"#,
        &["duplicate environment name: 'Shared'"],
    );
}

#[test]
fn test_too_many_parameters() {
    let params: Vec<String> = (0..51)
        .map(|i| format!(r#"{{"name": "P{i}", "type": "INT"}}"#))
        .collect();
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}],
        "parameterDefinitions": [{}]
    }}"#,
        params.join(",")
    );
    check_err(
        &s,
        &["parameterDefinitions:\n\tmust not contain more than 50 elements."],
    );
}

#[test]
fn test_self_dependency() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [
            {"name": "A", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "A"}]}
        ]
    }"#,
        &["steps[0] -> dependencies[0]:\n\tcannot depend on itself."],
    );
}

#[test]
fn test_duplicate_dependency() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [
            {"name": "A", "script": {"actions": {"onRun": {"command": "foo"}}}},
            {"name": "B", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "A"}, {"dependsOn": "A"}]}
        ]
    }"#,
        &["steps[1] -> dependencies[1]:\n\tduplicate dependency 'A'."],
    );
}

#[test]
fn test_empty_dependencies() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Foo",
        "steps": [
            {"name": "A", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": []}
        ]
    }"#,
        &["steps[0] -> dependencies:\n\tmust not be empty."],
    );
}
