// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test/openjd/model/v2023_09/test_environment_template.py,
//! test_environments.py, and test_embedded.py.
//!
//! Gold standard: failure tests assert the full error message including path.

use openjd_model::decode_environment_template;

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn decode_ok(s: &str) {
    let v = yaml_val(s);
    decode_environment_template(v, None).unwrap_or_else(|_| panic!("Expected success for: {s}"));
}

fn check_env_err(s: &str, expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_environment_template(v, None).expect_err(&format!("Expected error for: {s}"));
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

// ══════════════════════════════════════════════════════════════
// Success cases — EnvironmentTemplate
// ══════════════════════════════════════════════════════════════

#[test]
fn test_minimum_required() {
    decode_ok(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": "foo"}}}}
    }"#,
    );
}

#[test]
fn test_with_parameters() {
    decode_ok(
        r#"{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [{"name": "P", "type": "INT"}],
        "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": "foo"}}}}
    }"#,
    );
}

#[test]
fn test_with_most_parameters() {
    let params: Vec<String> = (0..50)
        .map(|i| format!(r#"{{"name": "P{i}", "type": "INT"}}"#))
        .collect();
    let s = format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [{}],
        "environment": {{"name": "Foo", "script": {{"actions": {{"onEnter": {{"command": "foo"}}}}}}}}
    }}"#,
        params.join(",")
    );
    decode_ok(&s);
}

#[test]
fn test_with_parameter_references() {
    decode_ok(
        r#"{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [{"name": "P", "type": "INT"}],
        "environment": {
            "name": "AnEnv",
            "script": {
                "embeddedFiles": [{"name": "Enter", "type": "TEXT", "data": "testing {{Param.P}}"}],
                "actions": {
                    "onEnter": {"command": "{{Param.P}}", "args": ["{{Param.P}}"]},
                    "onExit": {"command": "{{Param.P}}", "args": ["{{Param.P}}"]}
                }
            },
            "variables": {"Foo": "{{Param.P}}"}
        }
    }"#,
    );
}

// ══════════════════════════════════════════════════════════════
// Success cases — Environment (within env template)
// ══════════════════════════════════════════════════════════════

#[test]
fn test_env_with_script_only() {
    decode_ok(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": "foo"}}}}
    }"#,
    );
}

#[test]
fn test_env_with_variables_only() {
    decode_ok(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "variables": {"FOO": "bar"}}
    }"#,
    );
}

#[test]
fn test_env_with_description() {
    decode_ok(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "description": "text", "script": {"actions": {"onEnter": {"command": "foo"}}}}
    }"#,
    );
}

#[test]
fn test_env_with_both_script_and_variables() {
    decode_ok(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": "foo"}}}, "variables": {"FOO": "bar"}}
    }"#,
    );
}

// ══════════════════════════════════════════════════════════════
// Success cases — Embedded files
// ══════════════════════════════════════════════════════════════

#[test]
fn test_embedded_text_file() {
    decode_ok(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "script": {
            "embeddedFiles": [{"name": "MyFile", "type": "TEXT", "data": "hello world"}],
            "actions": {"onEnter": {"command": "foo"}}
        }}
    }"#,
    );
}

#[test]
fn test_embedded_file_with_filename() {
    decode_ok(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "script": {
            "embeddedFiles": [{"name": "MyFile", "type": "TEXT", "data": "hello", "filename": "out.txt"}],
            "actions": {"onEnter": {"command": "foo"}}
        }}
    }"#,
    );
}

#[test]
fn test_embedded_file_with_runnable() {
    decode_ok(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "script": {
            "embeddedFiles": [{"name": "MyFile", "type": "TEXT", "data": "hello", "runnable": true}],
            "actions": {"onEnter": {"command": "foo"}}
        }}
    }"#,
    );
}

// ══════════════════════════════════════════════════════════════
// Failure cases — EnvironmentTemplate parse/serde errors
// ══════════════════════════════════════════════════════════════

#[test]
fn test_empty_object() {
    check_env_err(
        "{}",
        &["missing Open Job Description schema version key: specificationVersion"],
    );
}

#[test]
fn test_unknown_key() {
    check_env_err(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": "foo"}}}},
        "unresolved": "key"
    }"#,
        &["unknown field `unresolved`"],
    );
}

#[test]
fn test_missing_spec_ver() {
    check_env_err(
        r#"{
        "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": "foo"}}}}
    }"#,
        &["missing Open Job Description schema version key: specificationVersion"],
    );
}

#[test]
fn test_incorrect_spec_ver() {
    check_env_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": "foo"}}}}
    }"#,
        &["is not an Environment Template version"],
    );
}

#[test]
fn test_environment_is_none() {
    check_env_err(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": null
    }"#,
        &["invalid type: null, expected struct Environment"],
    );
}

#[test]
fn test_discriminator_missing() {
    check_env_err(
        r#"{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [{"name": "foo"}],
        "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": "foo"}}}}
    }"#,
        &["missing 'type' field in parameter definition"],
    );
}

#[test]
fn test_discriminator_works() {
    check_env_err(
        r#"{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [{"name": "foo", "type": "INT", "default": "nine"}],
        "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": "foo"}}}}
    }"#,
        &["Cannot parse 'nine' as integer"],
    );
}

// ══════════════════════════════════════════════════════════════
// Failure cases — EnvironmentTemplate validation errors
// ══════════════════════════════════════════════════════════════

#[test]
fn test_empty_parameters() {
    check_env_err(
        r#"{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [],
        "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": "foo"}}}}
    }"#,
        &[
            "1 validation error for EnvironmentTemplate\n",
            "parameterDefinitions, if provided, must contain at least one element.",
        ],
    );
}

#[test]
fn test_too_many_parameters() {
    let params: Vec<String> = (0..51)
        .map(|i| format!(r#"{{"name": "P{i}", "type": "INT"}}"#))
        .collect();
    let s = format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [{}],
        "environment": {{"name": "Foo", "script": {{"actions": {{"onEnter": {{"command": "foo"}}}}}}}}
    }}"#,
        params.join(",")
    );
    check_env_err(
        &s,
        &[
            "1 validation error for EnvironmentTemplate\n",
            "parameterDefinitions must not contain more than 50 elements.",
        ],
    );
}

#[test]
fn test_duplicate_parameter_names() {
    check_env_err(
        r#"{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [{"name": "P", "type": "INT"}, {"name": "P", "type": "INT"}],
        "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": "foo"}}}}
    }"#,
        &[
            "1 validation error for EnvironmentTemplate\n",
            "Duplicate parameter name: 'P'",
        ],
    );
}

// ══════════════════════════════════════════════════════════════
// Failure cases — Environment validation errors
// ══════════════════════════════════════════════════════════════

#[test]
fn test_env_missing_script_and_variables() {
    check_env_err(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo"}
    }"#,
        &[
            "validation errors for EnvironmentTemplate\n",
            "environment:\n\tmust have at least one of 'script' or 'variables'.",
        ],
    );
}

#[test]
fn test_env_empty_variables() {
    check_env_err(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": "foo"}}}, "variables": {}}
    }"#,
        &[
            "1 validation error for EnvironmentTemplate\n",
            "environment -> variables:\n\tif provided, must not be empty.",
        ],
    );
}

#[test]
fn test_env_variable_name_starts_with_digit() {
    check_env_err(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "variables": {"2FOO": "BAR"}}
    }"#,
        &["environment -> variables -> 2FOO:\n\tvariable name '2FOO' cannot start with a digit."],
    );
}

#[test]
fn test_env_name_too_long() {
    let long_name = "A".repeat(65);
    let s = format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "environment": {{"name": "{long_name}", "variables": {{"X": "1"}}}}
    }}"#
    );
    check_env_err(
        &s,
        &[
            "1 validation error for EnvironmentTemplate\n",
            "environment -> name:\n\texceeds 64 characters.",
        ],
    );
}

// ══════════════════════════════════════════════════════════════
// Failure cases — Embedded file validation errors
// ══════════════════════════════════════════════════════════════

#[test]
fn test_embedded_empty_data() {
    check_env_err(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "script": {
            "embeddedFiles": [{"name": "MyFile", "type": "TEXT", "data": ""}],
            "actions": {"onEnter": {"command": "foo"}}
        }}
    }"#,
        &["environment -> script -> embeddedFiles[0] -> data:\n\tmust not be empty."],
    );
}

#[test]
fn test_embedded_unknown_type() {
    check_env_err(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "script": {
            "embeddedFiles": [{"name": "MyFile", "type": "text", "data": "hello"}],
            "actions": {"onEnter": {"command": "foo"}}
        }}
    }"#,
        &["unknown variant `text`, expected `TEXT`"],
    );
}

#[test]
fn test_embedded_filename_empty() {
    check_env_err(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "script": {
            "embeddedFiles": [{"name": "MyFile", "type": "TEXT", "data": "hello", "filename": ""}],
            "actions": {"onEnter": {"command": "foo"}}
        }}
    }"#,
        &["environment -> script -> embeddedFiles[0] -> filename:\n\tmust not be empty."],
    );
}

#[test]
fn test_embedded_filename_forward_slash() {
    check_env_err(r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "script": {
            "embeddedFiles": [{"name": "MyFile", "type": "TEXT", "data": "hello", "filename": "dir/file.txt"}],
            "actions": {"onEnter": {"command": "foo"}}
        }}
    }"#, &[
        "environment -> script -> embeddedFiles[0] -> filename:\n\tmust not contain path separators.",
    ]);
}

#[test]
fn test_embedded_filename_backslash() {
    check_env_err(r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "script": {
            "embeddedFiles": [{"name": "MyFile", "type": "TEXT", "data": "hello", "filename": "dir\\file.txt"}],
            "actions": {"onEnter": {"command": "foo"}}
        }}
    }"#, &[
        "environment -> script -> embeddedFiles[0] -> filename:\n\tmust not contain path separators.",
    ]);
}

#[test]
fn test_embedded_duplicate_names() {
    check_env_err(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "script": {
            "embeddedFiles": [
                {"name": "MyFile", "type": "TEXT", "data": "hello"},
                {"name": "MyFile", "type": "TEXT", "data": "world"}
            ],
            "actions": {"onEnter": {"command": "foo"}}
        }}
    }"#,
        &["environment -> script -> embeddedFiles[1]:\n\tduplicate embedded file name 'MyFile'."],
    );
}

// ══════════════════════════════════════════════════════════════
// Extensions on EnvironmentTemplate
// ══════════════════════════════════════════════════════════════

fn decode_with_exts(s: &str, exts: &[&str]) {
    let v = yaml_val(s);
    decode_environment_template(v, Some(exts))
        .unwrap_or_else(|_| panic!("Expected success for: {s}"));
}

fn check_env_err_with_exts(s: &str, exts: &[&str], expected: &[&str]) {
    let v = yaml_val(s);
    let err =
        decode_environment_template(v, Some(exts)).expect_err(&format!("Expected error for: {s}"));
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

const MINIMAL_ENV: &str = r#"{
    "specificationVersion": "environment-2023-09",
    "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": "foo"}}}}
}"#;

#[test]
fn test_env_template_with_extensions_field() {
    // Environment template with a valid extensions field should parse
    decode_with_exts(
        r#"{
        "specificationVersion": "environment-2023-09",
        "extensions": ["EXPR"],
        "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": "foo"}}}}
    }"#,
        &["EXPR"],
    );
}

#[test]
fn test_env_template_extensions_unsupported() {
    // Extension not in supported list should fail
    check_env_err_with_exts(
        r#"{
        "specificationVersion": "environment-2023-09",
        "extensions": ["EXPR"],
        "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": "foo"}}}}
    }"#,
        &[],
        &["unsupported extension"],
    );
}

#[test]
fn test_env_template_extensions_unknown() {
    // Completely unknown extension name should fail
    check_env_err_with_exts(
        r#"{
        "specificationVersion": "environment-2023-09",
        "extensions": ["NOT_A_REAL_EXTENSION"],
        "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": "foo"}}}}
    }"#,
        &["NOT_A_REAL_EXTENSION"],
        &["Unknown or unsupported extension"],
    );
}

#[test]
fn test_env_template_extensions_empty_list() {
    // Empty extensions list is rejected early in parsing.
    check_env_err_with_exts(
        r#"{
        "specificationVersion": "environment-2023-09",
        "extensions": [],
        "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": "foo"}}}}
    }"#,
        &["EXPR"],
        &["extensions"],
    );
}

#[test]
fn test_env_template_no_extensions_field_still_works() {
    // Omitting extensions entirely should still work (backward compat)
    decode_with_exts(MINIMAL_ENV, &["EXPR"]);
}

#[test]
fn test_env_template_extensions_enables_validation_context() {
    // EXPR extension should allow expression syntax in environment template format strings
    decode_with_exts(
        r#"{
        "specificationVersion": "environment-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "parameterDefinitions": [{"name": "P", "type": "INT"}],
        "environment": {
            "name": "Foo",
            "script": {
                "actions": {
                    "onEnter": {"command": "echo", "args": ["{{ Param.P + 1 }}"]}
                }
            }
        }
    }"#,
        &["FEATURE_BUNDLE_1", "EXPR"],
    );
}

#[test]
fn test_env_template_multiple_extensions() {
    decode_with_exts(
        r#"{
        "specificationVersion": "environment-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": "foo"}}}}
    }"#,
        &["FEATURE_BUNDLE_1", "EXPR"],
    );
}

// ══════════════════════════════════════════════════════════════
// Environment template extension handling — ported from Python
// test_parse.py::test_template_extensions_list and
// test_feature_bundle_1.py::TestParameterDefinitionsCount,
// TestEnvironmentNameLength
// ══════════════════════════════════════════════════════════════

#[test]
fn test_env_template_duplicate_extensions() {
    // Ported from Python: duplicate extension names should fail
    check_env_err_with_exts(
        r#"{
        "specificationVersion": "environment-2023-09",
        "extensions": ["EXPR", "EXPR"],
        "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": "foo"}}}}
    }"#,
        &["EXPR"],
        &["Duplicate"],
    );
}

#[test]
fn test_env_template_51_params_without_extension_fails() {
    // Ported from Python: 51 params without FEATURE_BUNDLE_1 → error
    let params: Vec<String> = (0..51)
        .map(|i| format!(r#"{{"name": "P{i}", "type": "INT"}}"#))
        .collect();
    let s = format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [{}],
        "environment": {{"name": "Foo", "script": {{"actions": {{"onEnter": {{"command": "foo"}}}}}}}}
    }}"#,
        params.join(",")
    );
    check_env_err(&s, &["50"]);
}

#[test]
fn test_env_template_50_params_without_extension_succeeds() {
    // Ported from Python: 50 params without extension → succeeds
    let params: Vec<String> = (0..50)
        .map(|i| format!(r#"{{"name": "P{i}", "type": "INT"}}"#))
        .collect();
    let s = format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [{}],
        "environment": {{"name": "Foo", "script": {{"actions": {{"onEnter": {{"command": "foo"}}}}}}}}
    }}"#,
        params.join(",")
    );
    decode_ok(&s);
}

#[test]
fn test_env_template_50_params_with_extension_succeeds() {
    // Ported from Python: 50 params with FEATURE_BUNDLE_1 → succeeds
    let params: Vec<String> = (0..50)
        .map(|i| format!(r#"{{"name": "P{i}", "type": "INT"}}"#))
        .collect();
    let s = format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "parameterDefinitions": [{}],
        "environment": {{"name": "Foo", "script": {{"actions": {{"onEnter": {{"command": "foo"}}}}}}}}
    }}"#,
        params.join(",")
    );
    decode_with_exts(&s, &["FEATURE_BUNDLE_1"]);
}

#[test]
fn test_env_template_51_params_with_extension_still_fails() {
    // Ported from Python: 51 params with FEATURE_BUNDLE_1 → STILL fails
    // Environment templates are always capped at 50 parameters, even with FEATURE_BUNDLE_1
    let params: Vec<String> = (0..51)
        .map(|i| format!(r#"{{"name": "P{i}", "type": "INT"}}"#))
        .collect();
    let s = format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "parameterDefinitions": [{}],
        "environment": {{"name": "Foo", "script": {{"actions": {{"onEnter": {{"command": "foo"}}}}}}}}
    }}"#,
        params.join(",")
    );
    check_env_err_with_exts(&s, &["FEATURE_BUNDLE_1"], &["50"]);
}

#[test]
fn test_env_name_65_chars_without_extension_fails() {
    // Ported from Python: environment name > 64 chars without extension → error
    let long_name = "A".repeat(65);
    let s = format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "environment": {{"name": "{long_name}", "variables": {{"X": "1"}}}}
    }}"#
    );
    check_env_err(&s, &["64"]);
}

#[test]
fn test_env_name_64_chars_without_extension_succeeds() {
    // Ported from Python: environment name exactly 64 chars → succeeds
    let name = "A".repeat(64);
    let s = format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "environment": {{"name": "{name}", "variables": {{"X": "1"}}}}
    }}"#
    );
    decode_ok(&s);
}

#[test]
fn test_env_name_512_chars_with_extension_succeeds() {
    // Ported from Python: 512-char name with FEATURE_BUNDLE_1 → succeeds
    let name = "A".repeat(512);
    let s = format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "environment": {{"name": "{name}", "variables": {{"X": "1"}}}}
    }}"#
    );
    decode_with_exts(&s, &["FEATURE_BUNDLE_1"]);
}

#[test]
fn test_env_name_513_chars_with_extension_fails() {
    // Ported from Python: 513-char name with FEATURE_BUNDLE_1 → error
    let name = "A".repeat(513);
    let s = format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "environment": {{"name": "{name}", "variables": {{"X": "1"}}}}
    }}"#
    );
    check_env_err_with_exts(&s, &["FEATURE_BUNDLE_1"], &["512"]);
}

// ══════════════════════════════════════════════════════════════
// Bug fix: onEnter is required per spec §4.3
// ══════════════════════════════════════════════════════════════

#[test]
fn test_env_actions_on_enter_required() {
    // onExit alone should fail — onEnter is required
    check_env_err(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "script": {"actions": {"onExit": {"command": "cleanup"}}}}
    }"#,
        &["environment -> script -> actions:\n\tonEnter is required."],
    );
}

// ══════════════════════════════════════════════════════════════
// Bug fix: environment actions must be validated through validate_action
// ══════════════════════════════════════════════════════════════

#[test]
fn test_env_on_enter_empty_command_validated() {
    check_env_err(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "script": {"actions": {"onEnter": {"command": ""}}}}
    }"#,
        &["environment -> script -> actions -> onEnter -> command:\n\tmust not be empty."],
    );
}

#[test]
fn test_env_on_exit_empty_command_validated() {
    check_env_err(
        r#"{
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "Foo", "script": {"actions": {
            "onEnter": {"command": "setup"},
            "onExit": {"command": ""}
        }}}
    }"#,
        &["environment -> script -> actions -> onExit -> command:\n\tmust not be empty."],
    );
}

// ══════════════════════════════════════════════════════════════
// BUG-2: Case-sensitivity consistency — parameter names should be
// case-sensitive (matching job template behavior)
// ══════════════════════════════════════════════════════════════

#[test]
fn env_template_case_different_params_accepted() {
    // "Foo" and "foo" are different names — should be accepted (case-sensitive)
    decode_ok(
        r#"{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [
            {"name": "Foo", "type": "INT"},
            {"name": "foo", "type": "INT"}
        ],
        "environment": {"name": "Env", "script": {"actions": {"onEnter": {"command": "bar"}}}}
    }"#,
    );
}
