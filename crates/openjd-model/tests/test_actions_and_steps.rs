// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test_action.py, test_step_template.py, and test_scripts.py
//!
//! Gold standard: failure tests assert the full error message including path.

use openjd_model::decode_job_template;

fn yaml_val(s: &str) -> serde_yaml::Value {
    serde_yaml::from_str(s).unwrap()
}

/// Wrap an action in a minimal job template for validation
fn job_with_action(action_json: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {action_json}}}}}}}]
    }}"#
    )
}

fn job_with_step(step_json: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{step_json}]
    }}"#
    )
}

fn decode_ok(s: &str) {
    let v = yaml_val(s);
    decode_job_template(v, None).expect("Expected success");
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

// === Action success cases ===

#[test]
fn test_action_command_min_len() {
    decode_ok(&job_with_action(r#"{"command": "1"}"#));
}

#[test]
fn test_action_with_args() {
    decode_ok(&job_with_action(r#"{"command": "foo", "args": ["bar"]}"#));
}

#[test]
fn test_action_with_timeout() {
    decode_ok(&job_with_action(r#"{"command": "foo", "timeout": 1}"#));
}

#[test]
fn test_action_with_timeout_string() {
    decode_ok(&job_with_action(r#"{"command": "foo", "timeout": "1"}"#));
}

#[test]
fn test_action_cancel_terminate() {
    decode_ok(&job_with_action(
        r#"{"command": "foo", "cancelation": {"mode": "TERMINATE"}}"#,
    ));
}

#[test]
fn test_action_cancel_notify() {
    decode_ok(&job_with_action(
        r#"{"command": "foo", "cancelation": {"mode": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": 1}}"#,
    ));
}

#[test]
fn test_action_cancel_notify_as_string() {
    decode_ok(&job_with_action(
        r#"{"command": "foo", "cancelation": {"mode": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": "1"}}"#,
    ));
}

#[test]
fn test_action_arg_empty_string() {
    decode_ok(&job_with_action(r#"{"command": "foo", "args": [""]}"#));
}

#[test]
fn test_action_command_format_string() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Foo", "type": "STRING"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "{{ Param.Foo }}"}}}}]
    }"#;
    decode_ok(s);
}

#[test]
fn test_action_arg_format_string() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Foo", "type": "STRING"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo", "args": ["{{ Param.Foo }}"]}}}}]
    }"#;
    decode_ok(s);
}

#[test]
fn test_action_timeout_max_value() {
    decode_ok(&job_with_action(r#"{"command": "foo", "timeout": 600}"#));
}

// === Action failure cases ===

#[test]
fn test_action_empty_command() {
    check_err(
        &job_with_action(r#"{"command": ""}"#),
        &["steps[0] -> script -> actions -> onRun -> command:\n\tmust not be empty."],
    );
}

#[test]
fn test_action_empty_args() {
    check_err(
        &job_with_action(r#"{"command": "1", "args": []}"#),
        &["steps[0] -> script -> actions -> onRun -> args:\n\tif provided, must not be empty."],
    );
}

#[test]
fn test_action_timeout_zero() {
    check_err(
        &job_with_action(r#"{"command": "1", "timeout": 0}"#),
        &["steps[0] -> script -> actions -> onRun:\n\ttimeout must be > 0."],
    );
}

#[test]
fn test_action_cancel_notify_zero() {
    check_err(
        &job_with_action(
            r#"{"command": "1", "cancelation": {"mode": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": 0}}"#,
        ),
        &["steps[0] -> script -> actions -> onRun:\n\tnotifyPeriodInSeconds must be > 0."],
    );
}

#[test]
fn test_action_empty_object() {
    check_err(&job_with_action(r#"{}"#), &["missing field `command`"]);
}

#[test]
fn test_action_unknown_key() {
    check_err(
        &job_with_action(r#"{"command": "foo", "extra": 12}"#),
        &["unknown field `extra`, expected one of `command`, `args`, `cancelation`, `timeout`"],
    );
}

#[test]
fn test_action_timeout_not_int() {
    check_err(
        &job_with_action(r#"{"command": "1", "timeout": 0.5}"#),
        &["steps[0] -> script -> actions -> onRun:\n\ttimeout must be a positive integer."],
    );
}

#[test]
fn test_action_timeout_not_intstring() {
    check_err(
        &job_with_action(r#"{"command": "1", "timeout": "0.5"}"#),
        &["steps[0] -> script -> actions -> onRun:\n\ttimeout must be a positive integer."],
    );
}

#[test]
fn test_action_cancelation_not_obj() {
    check_err(
        &job_with_action(r#"{"command": "1", "cancelation": "TERMINATE"}"#),
        &["invalid type: string"],
    );
}

#[test]
fn test_action_cancelation_unknown_mode() {
    check_err(
        &job_with_action(r#"{"command": "1", "cancelation": {"mode": "UNKNOWN"}}"#),
        &["unknown variant `UNKNOWN`, expected `TERMINATE` or `NOTIFY_THEN_TERMINATE`"],
    );
}

#[test]
fn test_action_cancelation_terminate_lowercase() {
    check_err(
        &job_with_action(r#"{"command": "1", "cancelation": {"mode": "terminate"}}"#),
        &["unknown variant `terminate`, expected `TERMINATE` or `NOTIFY_THEN_TERMINATE`"],
    );
}

#[test]
fn test_action_cancelation_notify_terminate_lowercase() {
    check_err(&job_with_action(r#"{"command": "1", "cancelation": {"mode": "notify_then_terminate"}}"#), &[
        "unknown variant `notify_then_terminate`, expected `TERMINATE` or `NOTIFY_THEN_TERMINATE`",
    ]);
}

#[test]
fn test_action_cancelation_terminate_rejects_notify_period() {
    check_err(
        &job_with_action(
            r#"{"command": "1", "cancelation": {"mode": "TERMINATE", "notifyPeriodInSeconds": 30}}"#,
        ),
        &["unknown field `notifyPeriodInSeconds`"],
    );
}

#[test]
fn test_action_cancelation_terminate_no_extra_fields() {
    decode_ok(&job_with_action(
        r#"{"command": "foo", "cancelation": {"mode": "TERMINATE"}}"#,
    ));
}

#[test]
fn test_action_cancelation_notify_is_float() {
    check_err(&job_with_action(r#"{"command": "1", "cancelation": {"mode": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": 0.5}}"#), &[
        "steps[0] -> script -> actions -> onRun:\n\tnotifyPeriodInSeconds must be a positive integer.",
    ]);
}

#[test]
fn test_action_cancelation_notify_not_intstring() {
    check_err(&job_with_action(r#"{"command": "1", "cancelation": {"mode": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": "0.5"}}"#), &[
        "steps[0] -> script -> actions -> onRun:\n\tnotifyPeriodInSeconds must be a positive integer.",
    ]);
}

// === StepActions failure cases ===

#[test]
fn test_step_actions_empty() {
    check_err(
        &job_with_step(r#"{"name": "S", "script": {"actions": {}}}"#),
        &["missing field `onRun`"],
    );
}

#[test]
fn test_step_actions_unknown_field() {
    check_err(
        &job_with_step(
            r#"{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}, "onUnknown": "blah"}}}"#,
        ),
        &["unknown field `onUnknown`, expected `onRun`"],
    );
}

// === EnvironmentActions failure cases ===

#[test]
fn test_env_actions_empty() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{"name": "E", "script": {"actions": {}}}]
    }"#;
    check_err(
        s,
        &["jobEnvironments[0] -> script -> actions:\n\tonEnter is required."],
    );
}

#[test]
fn test_env_actions_unknown_field() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{"name": "E", "script": {"actions": {"onEnter": {"command": "foo"}, "onUnknown": "blah"}}}]
    }"#;
    check_err(
        s,
        &["unknown field `onUnknown`, expected `onEnter` or `onExit`"],
    );
}

// === Step template success cases ===

#[test]
fn test_step_minimum() {
    decode_ok(&job_with_step(
        r#"{"name": "Foo", "script": {"actions": {"onRun": {"command": "foo"}}}}"#,
    ));
}

#[test]
fn test_step_with_description() {
    decode_ok(&job_with_step(
        r#"{"name": "Foo", "script": {"actions": {"onRun": {"command": "foo"}}}, "description": "some text"}"#,
    ));
}

#[test]
fn test_step_with_environment() {
    decode_ok(&job_with_step(
        r#"{"name": "Foo", "script": {"actions": {"onRun": {"command": "foo"}}}, "stepEnvironments": [{"name": "Env1", "script": {"actions": {"onEnter": {"command": "foo"}}}}]}"#,
    ));
}

#[test]
fn test_step_with_dependency() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [
            {"name": "Bar", "script": {"actions": {"onRun": {"command": "foo"}}}},
            {"name": "Foo", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "Bar"}]}
        ]
    }"#;
    decode_ok(s);
}

#[test]
fn test_step_with_multiple_dependencies() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [
            {"name": "Bar", "script": {"actions": {"onRun": {"command": "foo"}}}},
            {"name": "Fuz", "script": {"actions": {"onRun": {"command": "foo"}}}},
            {"name": "Foo", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "Bar"}, {"dependsOn": "Fuz"}]}
        ]
    }"#;
    decode_ok(s);
}

#[test]
fn test_step_with_host_requirements() {
    decode_ok(&job_with_step(
        r#"{"name": "Foo", "script": {"actions": {"onRun": {"command": "foo"}}}, "hostRequirements": {"amounts": [{"name": "amount.custom", "min": 1}], "attributes": [{"name": "attr.custom", "anyOf": ["foo"]}]}}"#,
    ));
}

#[test]
fn test_step_different_env_names() {
    decode_ok(&job_with_step(
        r#"{"name": "Foo", "script": {"actions": {"onRun": {"command": "foo"}}}, "stepEnvironments": [{"name": "E0", "script": {"actions": {"onEnter": {"command": "foo"}}}}, {"name": "E1", "script": {"actions": {"onEnter": {"command": "foo"}}}}]}"#,
    ));
}

// === Step template failure cases ===

#[test]
fn test_step_missing_name() {
    check_err(
        &job_with_step(r#"{"script": {"actions": {"onRun": {"command": "foo"}}}}"#),
        &["missing field `name`"],
    );
}

#[test]
fn test_step_missing_script() {
    check_err(
        &job_with_step(r#"{"name": "Foo"}"#),
        &["steps[0]:\n\tmust have 'script' or a simple action field."],
    );
}

#[test]
fn test_step_empty_environments() {
    check_err(
        &job_with_step(
            r#"{"name": "Foo", "script": {"actions": {"onRun": {"command": "foo"}}}, "stepEnvironments": []}"#,
        ),
        &["steps[0] -> stepEnvironments:\n\tmust not be empty."],
    );
}

#[test]
fn test_step_self_dependency() {
    check_err(
        &job_with_step(
            r#"{"name": "Foo", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "Foo"}]}"#,
        ),
        &["steps[0] -> dependencies[0]:\n\tcannot depend on itself."],
    );
}

#[test]
fn test_step_duplicate_dependency() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [
            {"name": "Bar", "script": {"actions": {"onRun": {"command": "foo"}}}},
            {"name": "Foo", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": [{"dependsOn": "Bar"}, {"dependsOn": "Bar"}]}
        ]
    }"#;
    check_err(
        s,
        &["steps[1] -> dependencies[1]:\n\tduplicate dependency 'Bar'."],
    );
}

#[test]
fn test_step_duplicate_env_names() {
    check_err(
        &job_with_step(
            r#"{"name": "Foo", "script": {"actions": {"onRun": {"command": "foo"}}}, "stepEnvironments": [{"name": "E", "script": {"actions": {"onEnter": {"command": "foo"}}}}, {"name": "E", "script": {"actions": {"onEnter": {"command": "foo"}}}}]}"#,
        ),
        &["steps[0] -> stepEnvironments[1]:\n\tduplicate environment name: 'E'"],
    );
}

#[test]
fn test_step_unknown_key() {
    check_err(
        &job_with_step(
            r#"{"name": "Foo", "script": {"actions": {"onRun": {"command": "foo"}}}, "unresolved": "key"}"#,
        ),
        &["unknown field `unresolved`"],
    );
}

#[test]
fn test_step_empty_object() {
    check_err(&job_with_step(r#"{}"#), &["missing field `name`"]);
}

#[test]
fn test_step_script_empty() {
    check_err(
        &job_with_step(r#"{"name": "Foo", "script": {}}"#),
        &["missing field `actions`"],
    );
}

#[test]
fn test_step_script_not_object() {
    check_err(
        &job_with_step(r#"{"name": "Foo", "script": 12}"#),
        &["invalid type: integer `12`, expected struct StepScript"],
    );
}

#[test]
fn test_step_description_not_string() {
    check_err(
        &job_with_step(
            r#"{"name": "Foo", "script": {"actions": {"onRun": {"command": "foo"}}}, "description": 12}"#,
        ),
        &["invalid type: integer `12`, expected a string"],
    );
}

#[test]
fn test_step_parameter_space_empty() {
    check_err(
        &job_with_step(
            r#"{"name": "Foo", "script": {"actions": {"onRun": {"command": "foo"}}}, "parameterSpace": {}}"#,
        ),
        &["missing field `taskParameterDefinitions`"],
    );
}

#[test]
fn test_step_empty_dependencies() {
    check_err(
        &job_with_step(
            r#"{"name": "Foo", "script": {"actions": {"onRun": {"command": "foo"}}}, "dependencies": []}"#,
        ),
        &["steps[0] -> dependencies:\n\tmust not be empty."],
    );
}

// === StepScript failure cases ===

#[test]
fn test_step_script_unknown_key() {
    check_err(
        &job_with_step(
            r#"{"name": "Foo", "script": {"actions": {"onRun": {"command": "foo"}}, "unresolved": "name"}}"#,
        ),
        &["unknown field `unresolved`, expected one of `let`, `actions`, `embeddedFiles`"],
    );
}

#[test]
fn test_step_script_embedded_files_empty() {
    check_err(
        &job_with_step(
            r#"{"name": "Foo", "script": {"actions": {"onRun": {"command": "foo"}}, "embeddedFiles": []}}"#,
        ),
        &["steps[0] -> script -> embeddedFiles:\n\tmust not be empty."],
    );
}

#[test]
fn test_step_script_embedded_files_duplicate_names() {
    check_err(
        &job_with_step(
            r#"{"name": "Foo", "script": {"actions": {"onRun": {"command": "foo"}}, "embeddedFiles": [{"name": "Name", "type": "TEXT", "data": "data"}, {"name": "Name", "type": "TEXT", "data": "data"}]}}"#,
        ),
        &["steps[0] -> script -> embeddedFiles[1]:\n\tduplicate embedded file name 'Name'."],
    );
}

// === EnvironmentScript failure cases ===

#[test]
fn test_env_script_unknown_key() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{"name": "E", "script": {"actions": {"onEnter": {"command": "foo"}}, "unresolved": "name"}}]
    }"#;
    check_err(
        s,
        &["unknown field `unresolved`, expected one of `let`, `actions`, `embeddedFiles`"],
    );
}

#[test]
fn test_env_script_embedded_files_empty() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{"name": "E", "script": {"actions": {"onEnter": {"command": "foo"}}, "embeddedFiles": []}}]
    }"#;
    check_err(
        s,
        &["jobEnvironments[0] -> script -> embeddedFiles:\n\tmust not be empty."],
    );
}

#[test]
fn test_env_script_embedded_files_duplicate_names() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{"name": "E", "script": {"actions": {"onEnter": {"command": "foo"}}, "embeddedFiles": [{"name": "Name", "type": "TEXT", "data": "data"}, {"name": "Name", "type": "TEXT", "data": "data"}]}}]
    }"#;
    check_err(s, &[
        "jobEnvironments[0] -> script -> embeddedFiles[1]:\n\tduplicate embedded file name 'Name'.",
    ]);
}

// === Environment actions success cases ===

#[test]
fn test_env_action_on_enter() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{"name": "E", "script": {"actions": {"onEnter": {"command": "foo"}}}}]
    }"#;
    decode_ok(s);
}

#[test]
fn test_env_action_on_exit() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{"name": "E", "script": {"actions": {"onExit": {"command": "foo"}}}}]
    }"#;
    check_err(
        s,
        &["jobEnvironments[0] -> script -> actions:\n\tonEnter is required."],
    );
}

#[test]
fn test_env_action_both() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{"name": "E", "script": {"actions": {"onEnter": {"command": "foo"}, "onExit": {"command": "bar"}}}}]
    }"#;
    decode_ok(s);
}

// === StepScript success cases ===

#[test]
fn test_step_script_with_embedded_files() {
    decode_ok(&job_with_step(
        r#"{"name": "Foo", "script": {"actions": {"onRun": {"command": "foo"}}, "embeddedFiles": [{"name": "Foo", "type": "TEXT", "data": "data"}]}}"#,
    ));
}

#[test]
fn test_step_script_max_embedded_files() {
    let files: Vec<String> = (0..5)
        .map(|i| format!(r#"{{"name": "Name{i}", "type": "TEXT", "data": "data"}}"#))
        .collect();
    let s = job_with_step(&format!(
        r#"{{"name": "Foo", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}, "embeddedFiles": [{}]}}}}"#,
        files.join(",")
    ));
    decode_ok(&s);
}

// === EnvironmentScript success cases ===

#[test]
fn test_env_script_with_embedded_files() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{"name": "E", "script": {"actions": {"onEnter": {"command": "foo"}}, "embeddedFiles": [{"name": "Foo", "type": "TEXT", "data": "data"}]}}]
    }"#;
    decode_ok(s);
}

// === StepName is a plain string, not a format string ===

#[test]
fn test_step_name_rejects_format_string() {
    check_err(
        &job_with_step(
            r#"{"name": "{{Param.Value}}", "script": {"actions": {"onRun": {"command": "foo"}}}}"#,
        ),
        &[
            "steps[0] -> name:",
            "must not contain format string expressions",
        ],
    );
}

#[test]
fn test_step_name_rejects_embedded_expression() {
    check_err(
        &job_with_step(
            r#"{"name": "Step-{{Param.X}}", "script": {"actions": {"onRun": {"command": "foo"}}}}"#,
        ),
        &[
            "steps[0] -> name:",
            "must not contain format string expressions",
        ],
    );
}

#[test]
fn test_step_name_allows_single_braces() {
    // Single braces are fine — they're not format string syntax
    decode_ok(&job_with_step(
        r#"{"name": "Step {1}", "script": {"actions": {"onRun": {"command": "foo"}}}}"#,
    ));
}

#[test]
fn test_step_name_allows_plain_string() {
    decode_ok(&job_with_step(
        r#"{"name": "My Step", "script": {"actions": {"onRun": {"command": "foo"}}}}"#,
    ));
}
