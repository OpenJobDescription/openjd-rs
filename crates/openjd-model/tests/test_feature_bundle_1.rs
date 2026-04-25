// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test/openjd/model/v2023_09/test_feature_bundle_1.py
//!
//! Gold standard: failure tests assert the full error message including path.

use openjd_model::decode_job_template;
use openjd_model::CallerLimits;

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

/// Decode with FEATURE_BUNDLE_1 supported.
fn decode_fb1(s: &str) {
    let v = yaml_val(s);
    decode_job_template(v, Some(&["FEATURE_BUNDLE_1"]), &CallerLimits::default())
        .unwrap_or_else(|_| panic!("Expected success for: {s}"));
}

/// Decode without any extensions supported.
fn check_err_no_ext(s: &str, expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_job_template(v, Some(&[]), &CallerLimits::default())
        .expect_err(&format!("Expected error for: {s}"));
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

/// Decode with FEATURE_BUNDLE_1 supported, expect error.
fn check_err_fb1(s: &str, expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_job_template(v, Some(&["FEATURE_BUNDLE_1"]), &CallerLimits::default())
        .expect_err(&format!("Expected error for: {s}"));
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

// ══════════════════════════════════════════════════════════════
// Extension registration
// ══════════════════════════════════════════════════════════════

#[test]
fn extension_not_supported() {
    check_err_no_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
    }"#,
        &["Unknown or unsupported extension: FEATURE_BUNDLE_1"],
    );
}

// ══════════════════════════════════════════════════════════════
// Timeout format strings
// ══════════════════════════════════════════════════════════════

#[test]
fn timeout_format_string_without_extension_fails() {
    check_err_no_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo", "timeout": "{{Param.Timeout}}"}}}}]
    }"#,
        &["steps[0] -> script -> actions -> onRun:\n\tformat strings in timeout are not allowed."],
    );
}

#[test]
fn timeout_format_string_with_extension_succeeds() {
    decode_fb1(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "parameterDefinitions": [{"name": "Timeout", "type": "INT", "default": 60}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo", "timeout": "{{Param.Timeout}}"}}}}]
    }"#,
    );
}

// ══════════════════════════════════════════════════════════════
// NotifyPeriodInSeconds format strings
// ══════════════════════════════════════════════════════════════

#[test]
fn notify_period_format_string_without_extension_fails() {
    check_err_no_ext(r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo", "cancelation": {"mode": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": "{{Param.Period}}"}}}}}]
    }"#, &[
        "steps[0] -> script -> actions -> onRun:\n\tformat strings in notifyPeriodInSeconds are not allowed.",
    ]);
}

#[test]
fn notify_period_format_string_with_extension_succeeds() {
    decode_fb1(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "parameterDefinitions": [{"name": "Period", "type": "INT", "default": 30}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo", "cancelation": {"mode": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": "{{Param.Period}}"}}}}}]
    }"#,
    );
}

// ══════════════════════════════════════════════════════════════
// Amount min/max format strings
// ══════════════════════════════════════════════════════════════

#[test]
fn amount_min_format_string_without_extension_fails() {
    check_err_no_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}, "hostRequirements": {"amounts": [{"name": "amount.worker.vcpu", "min": "{{Param.CpuMin}}"}]}}]
    }"#,
        &["steps[0] -> hostRequirements -> amounts[0] -> min:\n\tformat strings are not allowed."],
    );
}

#[test]
fn amount_max_format_string_without_extension_fails() {
    check_err_no_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}, "hostRequirements": {"amounts": [{"name": "amount.worker.vcpu", "max": "{{Param.CpuMax}}"}]}}]
    }"#,
        &["steps[0] -> hostRequirements -> amounts[0] -> max:\n\tformat strings are not allowed."],
    );
}

#[test]
fn amount_min_format_string_with_extension_succeeds() {
    decode_fb1(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "parameterDefinitions": [{"name": "CpuMin", "type": "INT", "default": 1}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}, "hostRequirements": {"amounts": [{"name": "amount.worker.vcpu", "min": "{{Param.CpuMin}}"}]}}]
    }"#,
    );
}

#[test]
fn amount_max_format_string_with_extension_succeeds() {
    decode_fb1(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "parameterDefinitions": [{"name": "CpuMax", "type": "INT", "default": 8}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}, "hostRequirements": {"amounts": [{"name": "amount.worker.vcpu", "max": "{{Param.CpuMax}}"}]}}]
    }"#,
    );
}

// ══════════════════════════════════════════════════════════════
// endOfLine
// ══════════════════════════════════════════════════════════════

#[test]
fn end_of_line_without_extension_fails() {
    check_err_no_ext(r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "bash", "args": ["{{Task.File.run}}"]}}, "embeddedFiles": [{"name": "run", "type": "TEXT", "data": "echo hello", "endOfLine": "LF"}]}}]
    }"#, &[
        "steps[0] -> script -> embeddedFiles[0] -> endOfLine:\n\trequires the FEATURE_BUNDLE_1 extension.",
    ]);
}

#[test]
fn end_of_line_invalid_value_fails() {
    check_err_fb1(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "bash", "args": ["{{Task.File.run}}"]}}, "embeddedFiles": [{"name": "run", "type": "TEXT", "data": "echo hello", "endOfLine": "INVALID"}]}}]
    }"#,
        &["unknown variant `INVALID`, expected one of `LF`, `CRLF`, `AUTO`"],
    );
}

#[test]
fn end_of_line_lf_with_extension_succeeds() {
    decode_fb1(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "bash", "args": ["{{Task.File.run}}"]}}, "embeddedFiles": [{"name": "run", "type": "TEXT", "data": "echo hello", "endOfLine": "LF"}]}}]
    }"#,
    );
}

#[test]
fn end_of_line_crlf_with_extension_succeeds() {
    decode_fb1(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "bash", "args": ["{{Task.File.run}}"]}}, "embeddedFiles": [{"name": "run", "type": "TEXT", "data": "echo hello", "endOfLine": "CRLF"}]}}]
    }"#,
    );
}

#[test]
fn end_of_line_auto_with_extension_succeeds() {
    decode_fb1(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "bash", "args": ["{{Task.File.run}}"]}}, "embeddedFiles": [{"name": "run", "type": "TEXT", "data": "echo hello", "endOfLine": "AUTO"}]}}]
    }"#,
    );
}

// ══════════════════════════════════════════════════════════════
// Filename length limits
// ══════════════════════════════════════════════════════════════

#[test]
fn filename_65_chars_without_extension_fails() {
    let f = "f".repeat(65);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "bash", "args": ["{{{{Task.File.run}}}}"]}} }}, "embeddedFiles": [{{"name": "run", "type": "TEXT", "data": "echo", "filename": "{f}"}}]}}}}]
    }}"#
    );
    check_err_no_ext(
        &s,
        &["steps[0] -> script -> embeddedFiles[0] -> filename:\n\texceeds 64 characters."],
    );
}

#[test]
fn filename_257_chars_with_extension_fails() {
    let f = "f".repeat(257);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "bash", "args": ["{{{{Task.File.run}}}}"]}} }}, "embeddedFiles": [{{"name": "run", "type": "TEXT", "data": "echo", "filename": "{f}"}}]}}}}]
    }}"#
    );
    check_err_fb1(
        &s,
        &["steps[0] -> script -> embeddedFiles[0] -> filename:\n\texceeds 256 characters."],
    );
}

#[test]
fn filename_256_chars_with_extension_succeeds() {
    let f = "f".repeat(256);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "bash", "args": ["{{{{Task.File.run}}}}"]}} }}, "embeddedFiles": [{{"name": "run", "type": "TEXT", "data": "echo", "filename": "{f}"}}]}}}}]
    }}"#
    );
    decode_fb1(&s);
}

// ══════════════════════════════════════════════════════════════
// Parameter count limits
// ══════════════════════════════════════════════════════════════

#[test]
fn param_count_51_without_extension_fails() {
    let params: Vec<String> = (0..51)
        .map(|i| format!(r#"{{"name": "P{i}", "type": "INT"}}"#))
        .collect();
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}],
        "parameterDefinitions": [{}]
    }}"#,
        params.join(",")
    );
    check_err_no_ext(
        &s,
        &["parameterDefinitions:\n\tmust not contain more than 50 elements."],
    );
}

#[test]
fn param_count_201_with_extension_fails() {
    let params: Vec<String> = (0..201)
        .map(|i| format!(r#"{{"name": "P{i}", "type": "INT"}}"#))
        .collect();
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}],
        "parameterDefinitions": [{}]
    }}"#,
        params.join(",")
    );
    check_err_fb1(
        &s,
        &["parameterDefinitions:\n\tmust not contain more than 200 elements."],
    );
}

#[test]
fn param_count_51_with_extension_succeeds() {
    let params: Vec<String> = (0..51)
        .map(|i| format!(r#"{{"name": "P{i}", "type": "INT"}}"#))
        .collect();
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}],
        "parameterDefinitions": [{}]
    }}"#,
        params.join(",")
    );
    decode_fb1(&s);
}

#[test]
fn param_count_200_with_extension_succeeds() {
    let params: Vec<String> = (0..200)
        .map(|i| format!(r#"{{"name": "P{i}", "type": "INT"}}"#))
        .collect();
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}],
        "parameterDefinitions": [{}]
    }}"#,
        params.join(",")
    );
    decode_fb1(&s);
}

// ══════════════════════════════════════════════════════════════
// SimpleAction without extension
// ══════════════════════════════════════════════════════════════

#[test]
fn python_without_extension_fails() {
    check_err_no_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "python": {"script": "print('hello')"}}]
    }"#,
        &["steps[0] -> python:\n\t'python' requires the FEATURE_BUNDLE_1 extension."],
    );
}

#[test]
fn bash_without_extension_fails() {
    check_err_no_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "bash": {"script": "echo hello"}}]
    }"#,
        &["steps[0] -> bash:\n\t'bash' requires the FEATURE_BUNDLE_1 extension."],
    );
}

#[test]
fn cmd_without_extension_fails() {
    check_err_no_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "cmd": {"script": "echo hello"}}]
    }"#,
        &["steps[0] -> cmd:\n\t'cmd' requires the FEATURE_BUNDLE_1 extension."],
    );
}

#[test]
fn powershell_without_extension_fails() {
    check_err_no_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "powershell": {"script": "Write-Host 'hello'"}}]
    }"#,
        &["steps[0] -> powershell:\n\t'powershell' requires the FEATURE_BUNDLE_1 extension."],
    );
}

#[test]
fn node_without_extension_fails() {
    check_err_no_ext(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "node": {"script": "console.log('hello')"}}]
    }"#,
        &["steps[0] -> node:\n\t'node' requires the FEATURE_BUNDLE_1 extension."],
    );
}

// ══════════════════════════════════════════════════════════════
// Script + interpreter conflict
// ══════════════════════════════════════════════════════════════

#[test]
fn script_and_python_conflict_fails() {
    check_err_fb1(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}, "python": {"script": "print('hello')"}}]
    }"#,
        &["steps[0] -> python:\n\tcannot have both 'python' and 'script'."],
    );
}

// ══════════════════════════════════════════════════════════════
// No script or interpreter
// ══════════════════════════════════════════════════════════════

#[test]
fn no_script_or_interpreter_fails() {
    check_err_fb1(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{"name": "S"}]
    }"#,
        &["steps[0]:\n\tmust have 'script' or a simple action field."],
    );
}

// ══════════════════════════════════════════════════════════════
// Job name length limits
// ══════════════════════════════════════════════════════════════

#[test]
fn job_name_129_chars_without_extension_fails() {
    let name = "J".repeat(129);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "{name}",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}]
    }}"#
    );
    check_err_no_ext(&s, &["name:\n\texceeds 128 characters."]);
}

#[test]
fn job_name_513_chars_with_extension_fails() {
    let name = "J".repeat(513);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "{name}",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}]
    }}"#
    );
    check_err_fb1(&s, &["name:\n\texceeds 512 characters."]);
}

#[test]
fn job_name_128_chars_without_extension_succeeds() {
    let name = "J".repeat(128);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "{name}",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}]
    }}"#
    );
    let v = yaml_val(&s);
    decode_job_template(v, Some(&[]), &CallerLimits::default()).expect("Expected success");
}

#[test]
fn job_name_512_chars_with_extension_succeeds() {
    let name = "J".repeat(512);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "{name}",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}]
    }}"#
    );
    decode_fb1(&s);
}

// ══════════════════════════════════════════════════════════════
// Step name length limits
// ══════════════════════════════════════════════════════════════

#[test]
fn step_name_65_chars_without_extension_fails() {
    let name = "S".repeat(65);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{"name": "{name}", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}]
    }}"#
    );
    check_err_no_ext(&s, &["steps[0] -> name:\n\texceeds 64 characters."]);
}

#[test]
fn step_name_513_chars_with_extension_fails() {
    let name = "S".repeat(513);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{{"name": "{name}", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}]
    }}"#
    );
    check_err_fb1(&s, &["steps[0] -> name:\n\texceeds 512 characters."]);
}

#[test]
fn step_name_512_chars_with_extension_succeeds() {
    let name = "S".repeat(512);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{{"name": "{name}", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}]
    }}"#
    );
    decode_fb1(&s);
}

// ══════════════════════════════════════════════════════════════
// Environment name length limits
// ══════════════════════════════════════════════════════════════

#[test]
fn env_name_65_chars_without_extension_fails() {
    let name = "E".repeat(65);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}],
        "jobEnvironments": [{{"name": "{name}", "script": {{"actions": {{"onEnter": {{"command": "foo"}}}}}}}}]
    }}"#
    );
    check_err_no_ext(
        &s,
        &["jobEnvironments[0] -> name:\n\texceeds 64 characters."],
    );
}

#[test]
fn env_name_513_chars_with_extension_fails() {
    let name = "E".repeat(513);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}],
        "jobEnvironments": [{{"name": "{name}", "script": {{"actions": {{"onEnter": {{"command": "foo"}}}}}}}}]
    }}"#
    );
    check_err_fb1(
        &s,
        &["jobEnvironments[0] -> name:\n\texceeds 512 characters."],
    );
}

#[test]
fn env_name_512_chars_with_extension_succeeds() {
    let name = "E".repeat(512);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}],
        "jobEnvironments": [{{"name": "{name}", "script": {{"actions": {{"onEnter": {{"command": "foo"}}}}}}}}]
    }}"#
    );
    decode_fb1(&s);
}

// ══════════════════════════════════════════════════════════════
// Identifier (embedded file name) length limits
// ══════════════════════════════════════════════════════════════

#[test]
fn identifier_65_chars_without_extension_fails() {
    let name = "I".repeat(65);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "bash", "args": ["{{{{Task.File.{name}}}}}"]}} }}, "embeddedFiles": [{{"name": "{name}", "type": "TEXT", "data": "echo"}}]}}}}]
    }}"#
    );
    check_err_no_ext(
        &s,
        &["steps[0] -> script -> embeddedFiles[0] -> name:\n\texceeds 64 characters."],
    );
}

#[test]
fn identifier_513_chars_with_extension_fails() {
    let name = "I".repeat(513);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "bash", "args": ["{{{{Task.File.{name}}}}}"]}} }}, "embeddedFiles": [{{"name": "{name}", "type": "TEXT", "data": "echo"}}]}}}}]
    }}"#
    );
    check_err_fb1(
        &s,
        &["steps[0] -> script -> embeddedFiles[0] -> name:\n\texceeds 512 characters."],
    );
}

#[test]
fn identifier_512_chars_with_extension_succeeds() {
    let name = "I".repeat(512);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "bash", "args": ["{{{{Task.File.{name}}}}}"]}} }}, "embeddedFiles": [{{"name": "{name}", "type": "TEXT", "data": "echo"}}]}}}}]
    }}"#
    );
    decode_fb1(&s);
}

// ══════════════════════════════════════════════════════════════
// Extension enablement matrix (from Python TestExtensionFieldEnablement)
// ══════════════════════════════════════════════════════════════

#[test]
fn extension_declared_but_not_in_supported_fails() {
    // Template declares FEATURE_BUNDLE_1, but supported only has TASK_CHUNKING
    let params: Vec<String> = (0..51)
        .map(|i| format!(r#"{{"name": "P{i}", "type": "INT"}}"#))
        .collect();
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}],
        "parameterDefinitions": [{}]
    }}"#,
        params.join(",")
    );
    let v = yaml_val(&s);
    assert!(decode_job_template(v, Some(&["TASK_CHUNKING"]), &CallerLimits::default()).is_err());
}

#[test]
fn extension_not_declared_but_supported_uses_base_limits() {
    // Template omits extension, supported includes it — 51 params should fail (50 limit)
    let params: Vec<String> = (0..51)
        .map(|i| format!(r#"{{"name": "P{i}", "type": "INT"}}"#))
        .collect();
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}],
        "parameterDefinitions": [{}]
    }}"#,
        params.join(",")
    );
    let v = yaml_val(&s);
    assert!(decode_job_template(v, Some(&["FEATURE_BUNDLE_1"]), &CallerLimits::default()).is_err());
}

#[test]
fn extension_not_declared_50_params_succeeds() {
    // Template within default limit — passes regardless
    let params: Vec<String> = (0..50)
        .map(|i| format!(r#"{{"name": "P{i}", "type": "INT"}}"#))
        .collect();
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}],
        "parameterDefinitions": [{}]
    }}"#,
        params.join(",")
    );
    let v = yaml_val(&s);
    decode_job_template(v, Some(&["FEATURE_BUNDLE_1"]), &CallerLimits::default())
        .expect("Expected success");
}

// Note: Python tests for multiple interpreters conflict and resolve_syntax_sugar
// are not ported because those features are Python-specific (Pydantic model methods).

// ══════════════════════════════════════════════════════════════
// Timeout edge cases
// ══════════════════════════════════════════════════════════════

#[test]
fn timeout_int_string_without_extension_succeeds() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo", "timeout": "60"}}}}]
    }"#,
    );
    decode_job_template(v, Some(&[]), &CallerLimits::default()).expect("Expected success");
}

#[test]
fn timeout_int_with_extension_succeeds() {
    decode_fb1(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo", "timeout": 60}}}}]
    }"#,
    );
}

// ══════════════════════════════════════════════════════════════
// Amount min/max both format strings
// ══════════════════════════════════════════════════════════════

#[test]
fn amount_min_max_both_format_strings_with_extension_succeeds() {
    decode_fb1(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "parameterDefinitions": [
            {"name": "CpuMin", "type": "INT", "default": 2},
            {"name": "CpuMax", "type": "INT", "default": 8}
        ],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}, "hostRequirements": {"amounts": [{"name": "amount.worker.vcpu", "min": "{{Param.CpuMin}}", "max": "{{Param.CpuMax}}"}]}}]
    }"#,
    );
}

// ══════════════════════════════════════════════════════════════
// Amount decimal string without extension
// ══════════════════════════════════════════════════════════════

#[test]
fn amount_min_decimal_string_without_extension_succeeds() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}, "hostRequirements": {"amounts": [{"name": "amount.worker.vcpu", "min": "2.5"}]}}]
    }"#,
    );
    decode_job_template(v, Some(&[]), &CallerLimits::default()).expect("Expected success");
}

// ══════════════════════════════════════════════════════════════
// Notify period int string without extension
// ══════════════════════════════════════════════════════════════

#[test]
fn notify_period_int_string_without_extension_succeeds() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo", "cancelation": {"mode": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": "120"}}}}}]
    }"#,
    );
    decode_job_template(v, Some(&[]), &CallerLimits::default()).expect("Expected success");
}

// ══════════════════════════════════════════════════════════════
// Extension supported — success
// ══════════════════════════════════════════════════════════════

#[test]
fn extension_supported_succeeds() {
    decode_fb1(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test Job",
        "steps": [{"name": "step1", "script": {"actions": {"onRun": {"command": "echo"}}}}]
    }"#,
    );
}

// ══════════════════════════════════════════════════════════════
// Step name 64 chars without extension succeeds
// ══════════════════════════════════════════════════════════════

#[test]
fn step_name_64_chars_without_extension_succeeds() {
    let name = "S".repeat(64);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{"name": "{name}", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}]
    }}"#
    );
    let v = yaml_val(&s);
    decode_job_template(v, Some(&[]), &CallerLimits::default()).expect("Expected success");
}

// ══════════════════════════════════════════════════════════════
// Environment name 64 chars without extension succeeds
// ══════════════════════════════════════════════════════════════

#[test]
fn env_name_64_chars_without_extension_succeeds() {
    let name = "E".repeat(64);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}],
        "jobEnvironments": [{{"name": "{name}", "script": {{"actions": {{"onEnter": {{"command": "foo"}}}}}}}}]
    }}"#
    );
    let v = yaml_val(&s);
    decode_job_template(v, Some(&[]), &CallerLimits::default()).expect("Expected success");
}

// ══════════════════════════════════════════════════════════════
// Identifier 64 chars without extension succeeds
// ══════════════════════════════════════════════════════════════

#[test]
fn identifier_64_chars_without_extension_succeeds() {
    let name = "I".repeat(64);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "bash", "args": ["{{{{Task.File.{name}}}}}"]}} }}, "embeddedFiles": [{{"name": "{name}", "type": "TEXT", "data": "echo"}}]}}}}]
    }}"#
    );
    let v = yaml_val(&s);
    decode_job_template(v, Some(&[]), &CallerLimits::default()).expect("Expected success");
}

// ══════════════════════════════════════════════════════════════
// EnvironmentTemplate parameter count limits with FEATURE_BUNDLE_1
// ══════════════════════════════════════════════════════════════

#[test]
fn env_template_51_params_without_extension_fails() {
    let params: Vec<String> = (0..51)
        .map(|i| format!(r#"{{"name": "P{i}", "type": "INT"}}"#))
        .collect();
    let s = format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [{}],
        "environment": {{"name": "Env", "script": {{"actions": {{"onEnter": {{"command": "echo"}}}}}}}}
    }}"#,
        params.join(",")
    );
    let v = yaml_val(&s);
    assert!(openjd_model::decode_environment_template(v, Some(&[])).is_err());
}

#[test]
fn env_template_50_params_without_extension_succeeds() {
    let params: Vec<String> = (0..50)
        .map(|i| format!(r#"{{"name": "P{i}", "type": "INT"}}"#))
        .collect();
    let s = format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [{}],
        "environment": {{"name": "Env", "script": {{"actions": {{"onEnter": {{"command": "echo"}}}}}}}}
    }}"#,
        params.join(",")
    );
    let v = yaml_val(&s);
    openjd_model::decode_environment_template(v, Some(&[])).expect("Expected success");
}

// ══════════════════════════════════════════════════════════════
// Multiple SimpleAction fields are mutually exclusive
// ══════════════════════════════════════════════════════════════

#[test]
fn multiple_simple_action_fields_rejected() {
    check_err_fb1(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["FEATURE_BUNDLE_1"],
        "steps": [{"name": "S", "bash": {"script": "echo hi"}, "python": {"script": "print('hi')"}}]
    }"#,
        &["cannot have more than one simple action field."],
    );
}

#[test]
fn simple_action_malformed_format_string_behavior() {
    let exts = &["FEATURE_BUNDLE_1"];
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        extensions:
          - FEATURE_BUNDLE_1
        steps:
          - name: Step1
            bash: "echo '{{broken'"
    "#,
    );
    let result = decode_job_template(template, Some(exts), &CallerLimits::default());
    // Validation catches the malformed format string before
    // resolve_syntax_sugar runs, so the silent fallback is not reached.
    assert!(
        result.is_err(),
        "Validation should catch malformed format string"
    );
}
