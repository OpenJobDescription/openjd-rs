// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_environments.py, test_definitions.py, test_embedded.py,
//! test_redacted_env_vars.py, test_template_variables.py
//!
//! Gold standard: failure tests assert the full error message including path.

use openjd_model::decode_job_template;
use openjd_model::CallerLimits;

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn decode_ok(s: &str) {
    let v = yaml_val(s);
    decode_job_template(v, None, &CallerLimits::default()).expect("Expected success");
}

fn check_err(s: &str, expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_job_template(v, None, &CallerLimits::default())
        .expect_err(&format!("Expected error for: {s}"));
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

// === Environment tests ===

#[test]
fn test_env_with_script_only() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{"name": "E", "script": {"actions": {"onEnter": {"command": "foo"}}}}]
    }"#,
    );
}

#[test]
fn test_env_with_variables_only() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{"name": "E", "variables": {"FOO": "bar"}}]
    }"#,
    );
}

#[test]
fn test_env_with_both() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{"name": "E", "script": {"actions": {"onEnter": {"command": "foo"}}}, "variables": {"FOO": "bar"}}]
    }"#,
    );
}

#[test]
fn test_env_empty_variables() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{"name": "E", "script": {"actions": {"onEnter": {"command": "foo"}}}, "variables": {}}]
    }"#,
        &["jobEnvironments[0] -> variables:\n\tif provided, must not be empty."],
    );
}

// === Embedded files ===

#[test]
fn test_embedded_text_file() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {
            "embeddedFiles": [{"name": "MyFile", "type": "TEXT", "data": "hello world"}],
            "actions": {"onRun": {"command": "foo"}}
        }}]
    }"#,
    );
}

#[test]
fn test_embedded_file_with_format_string() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "P", "type": "STRING", "default": "val"}],
        "steps": [{"name": "S", "script": {
            "embeddedFiles": [{"name": "MyFile", "type": "TEXT", "data": "value is {{Param.P}}"}],
            "actions": {"onRun": {"command": "foo"}}
        }}]
    }"#,
    );
}

// === Template variables (format string resolution) ===

#[test]
fn test_template_variable_in_name() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job-{{Param.Name}}",
        "parameterDefinitions": [{"name": "Name", "type": "STRING", "default": "test"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
    }"#,
    );
}

#[test]
fn test_template_variable_in_command() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Cmd", "type": "STRING", "default": "echo"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "{{Param.Cmd}}"}}}}]
    }"#,
    );
}

// === Step dependency graph ===

#[test]
fn test_dependency_graph_linear() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [
            {"name": "A", "script": {"actions": {"onRun": {"command": "foo"}}}},
            {"name": "B", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "A"}]},
            {"name": "C", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "B"}]}
        ]
    }"#,
    );
}

#[test]
fn test_dependency_graph_diamond() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [
            {"name": "A", "script": {"actions": {"onRun": {"command": "foo"}}}},
            {"name": "B", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "A"}]},
            {"name": "C", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "A"}]},
            {"name": "D", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "B"}, {"dependsOn": "C"}]}
        ]
    }"#,
    );
}

#[test]
fn test_dependency_graph_cycle() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
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
fn test_dependency_unknown_step() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [
            {"name": "A", "script": {"actions": {"onRun": {"command": "foo"}}}},
            {"name": "B", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "Unknown"}]}
        ]
    }"#,
        &["steps[1] -> dependencies[0]:\n\tdependency 'Unknown' not found."],
    );
}

#[test]
fn test_dependency_self_reference() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [
            {"name": "A", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "A"}]}
        ]
    }"#,
        &["steps[0] -> dependencies[0]:\n\tcannot depend on itself."],
    );
}

// === Env variable name validation ===

#[test]
fn test_env_var_name_valid() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{"name": "E", "variables": {"MY_VAR": "value", "PATH": "/usr/bin"}}]
    }"#,
    );
}

#[test]
fn test_env_var_name_starts_with_digit() {
    check_err(r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{"name": "E", "variables": {"1VAR": "value"}}]
    }"#, &[
        "jobEnvironments[0] -> variables -> 1VAR:\n\tvariable name '1VAR' cannot start with a digit.",
    ]);
}

#[test]
fn test_env_var_name_with_special_chars() {
    check_err(r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{"name": "E", "variables": {"MY-VAR": "value"}}]
    }"#, &[
        "jobEnvironments[0] -> variables -> MY-VAR:\n\tvariable name 'MY-VAR' contains invalid character '-'.",
    ]);
}

// === Template variable in env variable value ===

#[test]
fn test_template_variable_in_env_variable() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Val", "type": "STRING", "default": "test"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{"name": "E", "variables": {"MY_VAR": "{{Param.Val}}"}}]
    }"#,
    );
}

// === Template variable in args ===

#[test]
fn test_template_variable_in_args() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Arg", "type": "STRING", "default": "val"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo", "args": ["{{Param.Arg}}"]}}}}]
    }"#,
    );
}

// === Env variable name with underscore ===

#[test]
fn test_env_var_name_underscore_prefix() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{"name": "E", "variables": {"_MY_VAR": "value"}}]
    }"#,
    );
}

// === Dependency graph — diamond (misc) ===

#[test]
fn test_dependency_graph_diamond_misc() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [
            {"name": "A", "script": {"actions": {"onRun": {"command": "foo"}}}},
            {"name": "B", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "A"}]},
            {"name": "C", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "A"}]},
            {"name": "D", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "B"}, {"dependsOn": "C"}]}
        ]
    }"#,
    );
}
