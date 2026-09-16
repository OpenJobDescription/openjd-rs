// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Resolved-value constraint tests for template validation: the fields
//! whose spec constraints apply to the value a format string resolves
//! to — "after the format string has been resolved" in the spec's
//! wording (see the Spec-Mandated Resolved-Value Constraints section of
//! `specs/model/validation.md`).
//!
//! Every such field carries a spec-mandated constraint on the value its
//! format string resolves to. Template validation (`openjd check`) must
//! fail when the violation is statically knowable:
//!
//! - the field is **fully static** and resolves to a value that is too
//!   big / out of range / not in the allowed set, or
//! - the field is partially unresolved but `min_resolved_string_len`
//!   already exceeds the limit — no run-time resolution can conform.
//!
//! Each field also has passing controls: an under-limit static value and a
//! fully-unresolved value. Failure tests assert the full path + message
//! per the repo's error-message test standard.

use openjd_model::CallerLimits;
use openjd_model::{decode_environment_template, decode_job_template};

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

const ALL_EXTS: &[&str] = &["EXPR", "FEATURE_BUNDLE_1", "TASK_CHUNKING"];

fn check_err(s: &str, expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_job_template(v, Some(ALL_EXTS), &CallerLimits::default())
        .expect_err("Expected validation error");
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

/// Like [`check_err`], but asserts the *entire* error output. Use when a
/// test must also pin the error count — e.g., that one violation is
/// reported exactly once.
fn check_err_exact(s: &str, expected: &str) {
    let v = yaml_val(s);
    let err = decode_job_template(v, Some(ALL_EXTS), &CallerLimits::default())
        .expect_err("Expected validation error");
    assert_eq!(err.to_string(), expected);
}

fn check_ok(s: &str) {
    let v = yaml_val(s);
    if let Err(e) = decode_job_template(v, Some(ALL_EXTS), &CallerLimits::default()) {
        panic!("Expected template to validate, got:\n{e}");
    }
}

/// Minimal job template with one STRING parameter `X` and the given
/// name field value. Declares FEATURE_BUNDLE_1, so the name limit is 512.
fn job_with_name(name: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR", "FEATURE_BUNDLE_1"],
        "name": "{name}",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}}}]
    }}"#
    )
}

// ══════════════════════════════════════════════════════════════
// §1.1.1 Job name — ≤ 512 chars (FEATURE_BUNDLE_1), no Cc chars
// ══════════════════════════════════════════════════════════════

#[test]
fn job_name_static_too_long_fails() {
    check_err(
        &job_with_name("{{ 'A' * 600 }}"),
        &["name:\n\tresolves to at least 600 characters, exceeding the maximum of 512."],
    );
}

#[test]
fn job_name_bound_too_long_fails_with_unresolved_part() {
    // Param.X is unknown at validation (contributes 0), but the static
    // segment alone already exceeds the limit.
    check_err(
        &job_with_name("{{ Param.X }}{{ 'A' * 600 }}"),
        &["name:\n\tresolves to at least 600 characters, exceeding the maximum of 512."],
    );
}

#[test]
fn job_name_static_control_chars_fail() {
    // `\\n` in the JSON source is `\n` in the format string, which the
    // expression parser reads as a Python-style newline escape: the
    // resolved value contains a Cc character.
    check_err(
        &job_with_name("{{ 'a' + '\\\\n' + 'b' }}"),
        &["name:\n\tcontains control characters."],
    );
}

#[test]
fn job_name_literal_control_char_fails_even_when_interpolated() {
    // A control character in the name's *literal* text appears verbatim
    // in every possible resolution, so decode rejects it even though the
    // interpolated part is unknowable (`\t` here is a JSON escape — a
    // real tab in the literal segment).
    check_err(
        &job_with_name("render-{{ Param.X }}\t"),
        &["name:\n\tcontains control characters."],
    );
}

#[test]
fn job_name_control_char_in_expression_source_passes_when_resolution_clean() {
    // A control character inside expression *source* text never appears
    // in a resolved value — only what the expression evaluates to does.
    // This name's expression source contains a real tab (JSON `\t`), but
    // it statically resolves to "xa": legal, and previously
    // over-rejected when the check ran on the whole raw text.
    check_ok(&job_with_name("x{{ 'a' if 1 == 1 else '\t' }}"));
}

#[test]
fn job_name_under_limit_and_unresolved_pass() {
    check_ok(&job_with_name("{{ 'A' * 500 }}"));
    check_ok(&job_with_name("{{ Param.X }}{{ 'A' * 100 }}"));
    check_ok(&job_with_name("{{ Param.X }}"));
}

// ══════════════════════════════════════════════════════════════
// Whole-field null in required string fields
// ══════════════════════════════════════════════════════════════
//
// Required string fields resolve whole-field expressions with target
// type `string` (Expression Language §1.3.2), and null does not coerce
// to string — so `check` rejects it with the same diagnostic resolution
// produces. Null *inside* a larger string is ordinary interpolation
// (renders empty) and stays valid.

#[test]
fn job_name_whole_field_null_fails() {
    check_err(
        &job_with_name("{{ null }}"),
        &["name:\n\tFailed to parse interpolation expression at [0, 10]. Cannot coerce nulltype to string"],
    );
}

#[test]
fn job_name_null_inside_text_passes() {
    check_ok(&job_with_name("job{{ null }}name"));
}

#[test]
fn job_name_static_empty_fails() {
    // §1.1.1 minimum length 1 applies to the resolved value.
    check_err(
        &job_with_name("{{ '' }}"),
        &["name:\n\tmust not resolve to an empty string."],
    );
}

#[test]
fn env_var_value_static_empty_passes() {
    // §4.4.2's minimum length is 0: empty environment variable values
    // are legal.
    check_ok(&job_with_env_var("{{ '' }}"));
}

#[test]
fn string_range_item_static_empty_passes() {
    // §3.4.2 sets a minimum length of 1, but the reference implementation
    // enforces it only for PATH elements — empty STRING elements are
    // accepted end to end. Matched for compatibility.
    check_ok(&job_with_range_item("STRING", "{{ '' }}"));
    check_ok(&job_with_range_item("STRING", "{{ ['ok', ''] }}"));
}

#[test]
fn path_range_item_static_empty_fails() {
    // §3.4.2 minimum length 1, PATH only: an empty string is not a valid
    // path on any OS.
    check_err(
        &job_with_range_item("PATH", "{{ '' }}"),
        &["steps[0] -> parameterSpace -> taskParameterDefinitions[0] -> range[0]:\n\tmust not resolve to an empty string."],
    );
}

#[test]
fn path_range_item_static_list_with_empty_element_fails() {
    check_err(
        &job_with_range_item("PATH", "{{ ['ok', ''] }}"),
        &["steps[0] -> parameterSpace -> taskParameterDefinitions[0] -> range[0]:\n\tlist element 1 must not resolve to an empty string."],
    );
}

#[test]
fn job_name_whole_field_list_fails() {
    // No list → string coercion exists: a required string field rejects
    // list-valued whole-field expressions.
    check_err(
        &job_with_name("{{ ['a', 'b'] }}"),
        &["name:\n\tFailed to parse interpolation expression at [0, 16]. Cannot coerce list[string] to string"],
    );
}

#[test]
fn env_var_value_whole_field_list_fails() {
    check_err(
        &job_with_env_var("{{ ['a', 'b'] }}"),
        &["jobEnvironments[0] -> variables -> FOO:\n\tFailed to parse interpolation expression at [0, 16]. Cannot coerce list[string] to string"],
    );
}

#[test]
fn job_name_list_inside_text_passes() {
    // Inside surrounding text a list renders its display form — ordinary
    // string interpolation per §1.3.2.
    check_ok(&job_with_name("job {{ ['a', 'b'] }} name"));
}

#[test]
fn attr_value_whole_field_null_skips_element() {
    // Attribute values are list items: a whole-field null skips the
    // element at validation time (job creation rejects the requirement
    // if every element skips away).
    check_ok(&job_with_attr("attr.custom.tag", "{{ null }}"));
}

#[test]
fn attr_value_static_list_flattens_and_checks_each_element() {
    check_ok(&job_with_attr(
        "attr.custom.tag",
        "{{ ['tag_a', 'tag_b'] }}",
    ));
    // Each flattened element gets the §3.3.2.2 charset check.
    check_err(
        &job_with_attr("attr.custom.tag", "{{ ['tag_a', 'has space'] }}"),
        &["steps[0] -> hostRequirements -> attributes[0] -> anyOf[0]:\n\tvalue 'has space' contains invalid characters."],
    );
}

#[test]
fn string_range_item_whole_field_null_skips_element() {
    // Range elements are list items: a whole-field null skips the
    // element at validation time (job creation rejects the range if
    // every element skips away).
    check_ok(&job_with_range_item("STRING", "{{ null }}"));
}

#[test]
fn string_range_item_static_list_checks_each_element() {
    check_ok(&job_with_range_item("STRING", "{{ ['a', 'b'] }}"));
    // The 1024-character limit applies to each flattened element, not to
    // the list's display form.
    check_err(
        &job_with_range_item("STRING", "{{ ['ok', 'x' * 1100] }}"),
        &["steps[0] -> parameterSpace -> taskParameterDefinitions[0] -> range[0]:\n\tlist element 1 resolves to 1100 characters, exceeding the maximum of 1024."],
    );
}

#[test]
fn env_var_value_whole_field_null_fails() {
    check_err(
        &job_with_env_var("{{ null }}"),
        &["jobEnvironments[0] -> variables -> FOO:\n\tFailed to parse interpolation expression at [0, 10]. Cannot coerce nulltype to string"],
    );
}

// ══════════════════════════════════════════════════════════════
// §3.3.2.2 Attribute capability values — ≤ 100 chars / allowed set
// ══════════════════════════════════════════════════════════════

fn job_with_attr(name: &str, value: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "T",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "steps": [{{
            "name": "S",
            "hostRequirements": {{"attributes": [{{"name": "{name}", "anyOf": ["{value}"]}}]}},
            "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}
        }}]
    }}"#
    )
}

#[test]
fn attr_value_static_too_long_fails() {
    check_err(
        &job_with_attr("attr.custom.tag", "{{ 'a' * 150 }}"),
        &["steps[0] -> hostRequirements -> attributes[0] -> anyOf[0]:\n\tresolves to at least 150 characters, exceeding the maximum of 100."],
    );
}

#[test]
fn attr_value_bound_too_long_fails_with_unresolved_part() {
    check_err(
        &job_with_attr("attr.custom.tag", "{{ Param.X }}{{ 'a' * 150 }}"),
        &["steps[0] -> hostRequirements -> attributes[0] -> anyOf[0]:\n\tresolves to at least 150 characters, exceeding the maximum of 100."],
    );
}

#[test]
fn attr_value_static_invalid_charset_fails() {
    check_err(
        &job_with_attr("attr.custom.tag", "{{ 'has space' }}"),
        &["steps[0] -> hostRequirements -> attributes[0] -> anyOf[0]:\n\tvalue 'has space' contains invalid characters."],
    );
}

#[test]
fn attr_value_standard_capability_static_invalid_fails() {
    check_err(
        &job_with_attr("attr.worker.os.family", "{{ 'lin' + 'uxx' }}"),
        &["steps[0] -> hostRequirements -> attributes[0] -> anyOf[0]:\n\tvalue 'linuxx' is not valid for attr.worker.os.family."],
    );
}

#[test]
fn attr_value_standard_capability_bound_too_long_fails() {
    // No allowed value for attr.worker.os.family is longer than 7 chars
    // ("windows"), so a bound past that can never conform.
    check_err(
        &job_with_attr("attr.worker.os.family", "{{ Param.X }}{{ 'a' * 10 }}"),
        &["steps[0] -> hostRequirements -> attributes[0] -> anyOf[0]:\n\tresolves to at least 10 characters; no valid value for attr.worker.os.family is longer than 7 characters."],
    );
}

#[test]
fn attr_value_under_limit_and_unresolved_pass() {
    check_ok(&job_with_attr("attr.custom.tag", "{{ 'a' * 100 }}"));
    check_ok(&job_with_attr("attr.custom.tag", "{{ Param.X }}"));
    check_ok(&job_with_attr("attr.worker.os.family", "{{ 'linux' }}"));
}

// ══════════════════════════════════════════════════════════════
// §3.4.2 Task parameter STRING/PATH range values — ≤ 1024 chars
// ══════════════════════════════════════════════════════════════

fn job_with_range_item(ptype: &str, item: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "T",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "steps": [{{
            "name": "S",
            "parameterSpace": {{"taskParameterDefinitions": [
                {{"name": "P", "type": "{ptype}", "range": ["{item}"]}}
            ]}},
            "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}
        }}]
    }}"#
    )
}

#[test]
fn string_range_item_static_too_long_fails() {
    // Exact-output assertion: the violation must be reported exactly once
    // (the bound check and the per-element check must not both fire for
    // the same fully static string).
    check_err_exact(
        &job_with_range_item("STRING", "{{ 'A' * 1100 }}"),
        "Model validation error: 1 validation error for JobTemplate\nsteps[0] -> parameterSpace -> taskParameterDefinitions[0] -> range[0]:\n\tresolves to at least 1100 characters, exceeding the maximum of 1024.",
    );
}

#[test]
fn path_range_item_static_too_long_fails() {
    check_err(
        &job_with_range_item("PATH", "{{ 'A' * 1100 }}"),
        &["steps[0] -> parameterSpace -> taskParameterDefinitions[0] -> range[0]:\n\tresolves to at least 1100 characters, exceeding the maximum of 1024."],
    );
}

#[test]
fn string_range_item_bound_too_long_fails_with_unresolved_part() {
    check_err(
        &job_with_range_item("STRING", "{{ Param.X }}{{ 'A' * 1100 }}"),
        &["steps[0] -> parameterSpace -> taskParameterDefinitions[0] -> range[0]:\n\tresolves to at least 1100 characters, exceeding the maximum of 1024."],
    );
}

#[test]
fn range_item_under_limit_and_unresolved_pass() {
    check_ok(&job_with_range_item("STRING", "{{ 'A' * 1024 }}"));
    check_ok(&job_with_range_item("STRING", "{{ Param.X }}"));
    // Limits count characters, not bytes (spec §3.4.2; the reference
    // implementation's len() counts characters): 1024 two-byte chars is
    // 2048 bytes but exactly at the limit.
    check_ok(&job_with_range_item("STRING", "{{ 'é' * 1024 }}"));
}

#[test]
fn range_item_char_count_over_limit_fails() {
    check_err(
        &job_with_range_item("STRING", "{{ 'é' * 1025 }}"),
        &["steps[0] -> parameterSpace -> taskParameterDefinitions[0] -> range[0]:\n\tresolves to at least 1025 characters, exceeding the maximum of 1024."],
    );
}

// ══════════════════════════════════════════════════════════════
// §4.4.2 Environment variable values — ≤ 2048 chars (resolved)
// ══════════════════════════════════════════════════════════════

fn job_with_env_var(value: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "T",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "jobEnvironments": [{{"name": "E", "variables": {{"FOO": "{value}"}}}}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}}}]
    }}"#
    )
}

#[test]
fn env_var_value_static_too_long_fails() {
    check_err(
        &job_with_env_var("{{ 'A' * 3000 }}"),
        &["jobEnvironments[0] -> variables -> FOO:\n\tresolves to at least 3000 characters, exceeding the maximum of 2048."],
    );
}

#[test]
fn env_var_value_bound_too_long_fails_with_unresolved_part() {
    // Session.WorkingDirectory only resolves on the worker (contributes
    // 0), but the static segment alone exceeds the limit.
    check_err(
        &job_with_env_var("{{ Session.WorkingDirectory }}{{ 'A' * 3000 }}"),
        &["jobEnvironments[0] -> variables -> FOO:\n\tresolves to at least 3000 characters, exceeding the maximum of 2048."],
    );
}

#[test]
fn env_var_value_under_limit_and_unresolved_pass() {
    check_ok(&job_with_env_var("{{ 'A' * 2048 }}"));
    check_ok(&job_with_env_var("{{ Session.WorkingDirectory }}"));
}

#[test]
fn environment_template_env_var_value_static_too_long_fails() {
    let tmpl = r#"{
        "specificationVersion": "environment-2023-09",
        "extensions": ["EXPR"],
        "environment": {"name": "E", "variables": {"FOO": "{{ 'A' * 3000 }}"}}
    }"#;
    let err = decode_environment_template(
        yaml_val(tmpl),
        Some(&["EXPR", "FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    )
    .expect_err("Expected validation error");
    let msg = err.to_string();
    assert!(
        msg.contains("variables -> FOO:\n\tresolves to at least 3000 characters, exceeding the maximum of 2048."),
        "Got:\n{msg}"
    );
}

// ══════════════════════════════════════════════════════════════
// §5.3.2 notifyPeriodInSeconds — positive integer ≤ 600
// ══════════════════════════════════════════════════════════════

fn job_with_notify(period: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR", "FEATURE_BUNDLE_1"],
        "name": "T",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{
            "command": "echo",
            "cancelation": {{"mode": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": "{period}"}}
        }}}}}}}}]
    }}"#
    )
}

#[test]
fn notify_period_static_too_big_fails() {
    check_err(
        &job_with_notify("{{ 300 + 400 }}"),
        &["steps[0] -> script -> actions -> onRun -> cancelation:\n\tnotifyPeriodInSeconds must not exceed 600."],
    );
}

#[test]
fn notify_period_static_zero_fails() {
    check_err(
        &job_with_notify("{{ 100 - 100 }}"),
        &["steps[0] -> script -> actions -> onRun -> cancelation:\n\tnotifyPeriodInSeconds must be > 0."],
    );
}

#[test]
fn notify_period_static_non_integer_fails() {
    // Whole-field expressions resolve with target type `int?` (§5.3.2),
    // so an uncoercible value fails with the same diagnostic resolution
    // itself produces.
    check_err(
        &job_with_notify("{{ 'abc' }}"),
        &["steps[0] -> script -> actions -> onRun -> cancelation:\n\tFailed to parse interpolation expression at [0, 11]. Cannot coerce string to int?"],
    );
}

#[test]
fn notify_period_bound_unreasonably_long_fails() {
    // Leading zeros make any exact limit impossible, but a resolution of
    // at least 200 characters cannot be a reasonable integer.
    check_err(
        &job_with_notify("{{ Param.X }}{{ '0' * 200 }}"),
        &["steps[0] -> script -> actions -> onRun -> cancelation:\n\tresolves to at least 200 characters, which cannot be a reasonable integer value."],
    );
}

#[test]
fn notify_period_valid_and_unresolved_pass() {
    check_ok(&job_with_notify("{{ 300 + 300 }}"));
    check_ok(&job_with_notify("{{ null }}"));
    check_ok(&job_with_notify("{{ Param.X }}"));
    // Leading zeros parse under the `int?` target's string→int
    // conversion, matching resolution.
    check_ok(&job_with_notify("{{ '0600' }}"));
}

// ══════════════════════════════════════════════════════════════
// §5 action timeout — positive integer
// ══════════════════════════════════════════════════════════════

fn job_with_timeout(timeout: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR", "FEATURE_BUNDLE_1"],
        "name": "T",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{
            "command": "echo",
            "timeout": "{timeout}"
        }}}}}}}}]
    }}"#
    )
}

#[test]
fn timeout_static_zero_fails() {
    check_err(
        &job_with_timeout("{{ 5 - 5 }}"),
        &["steps[0] -> script -> actions -> onRun -> timeout:\n\ttimeout must be > 0."],
    );
}

#[test]
fn timeout_static_non_integral_float_fails() {
    // Whole-field expressions resolve with target type `int?` (§5): a
    // non-integral float cannot coerce, matching resolution behavior.
    check_err(
        &job_with_timeout("{{ 1.5 }}"),
        &["steps[0] -> script -> actions -> onRun -> timeout:\n\tFailed to parse interpolation expression at [0, 9]. Cannot coerce float to int?"],
    );
}

#[test]
fn timeout_bound_unreasonably_long_fails() {
    check_err(
        &job_with_timeout("{{ Param.X }}{{ '1' * 200 }}"),
        &["steps[0] -> script -> actions -> onRun -> timeout:\n\tresolves to at least 200 characters, which cannot be a reasonable integer value."],
    );
}

#[test]
fn timeout_valid_and_unresolved_pass() {
    check_ok(&job_with_timeout("{{ 60 * 60 }}"));
    check_ok(&job_with_timeout("{{ Param.X }}"));
    check_ok(&job_with_timeout("{{ null }}"));
    // Whole-number floats and numeric strings coerce under the `int?`
    // target, exactly as resolution coerces them.
    check_ok(&job_with_timeout("{{ 120.0 }}"));
    check_ok(&job_with_timeout("{{ '120' }}"));
}

// ══════════════════════════════════════════════════════════════
// Cancelation mode — TERMINATE | NOTIFY_THEN_TERMINATE | null
// ══════════════════════════════════════════════════════════════

fn job_with_mode(mode: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR", "FEATURE_BUNDLE_1"],
        "name": "T",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{
            "command": "echo",
            "cancelation": {{"mode": "{mode}"}}
        }}}}}}}}]
    }}"#
    )
}

#[test]
fn cancelation_mode_static_invalid_fails() {
    check_err(
        &job_with_mode("{{ 'TERMINATE' + 'X' }}"),
        &["steps[0] -> script -> actions -> onRun -> cancelation:\n\tmode must resolve to TERMINATE or NOTIFY_THEN_TERMINATE, got 'TERMINATEX'."],
    );
}

#[test]
fn cancelation_mode_bound_too_long_fails() {
    // Longer than any valid mode value (21 chars).
    check_err(
        &job_with_mode("{{ Param.X }}{{ 'X' * 30 }}"),
        &["steps[0] -> script -> actions -> onRun -> cancelation:\n\tmode resolves to at least 30 characters; valid values are TERMINATE and NOTIFY_THEN_TERMINATE."],
    );
}

#[test]
fn cancelation_mode_valid_and_unresolved_pass() {
    check_ok(&job_with_mode("{{ 'TERMINATE' }}"));
    check_ok(&job_with_mode("{{ 'NOTIFY_THEN' + '_TERMINATE' }}"));
    check_ok(&job_with_mode("{{ null }}"));
    check_ok(&job_with_mode("{{ Param.X }}"));
}

// ══════════════════════════════════════════════════════════════
// TASK_CHUNKING chunks — defaultTaskCount ≥ 1, targetRuntimeSeconds ≥ 0
// ══════════════════════════════════════════════════════════════

fn job_with_chunks(dtc: &str, target: Option<&str>) -> String {
    let target_field = target
        .map(|t| format!(r#", "targetRuntimeSeconds": "{t}""#))
        .unwrap_or_default();
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR", "TASK_CHUNKING"],
        "name": "T",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "steps": [{{
            "name": "S",
            "parameterSpace": {{"taskParameterDefinitions": [
                {{"name": "C", "type": "CHUNK[INT]", "range": "1-100",
                  "chunks": {{"defaultTaskCount": "{dtc}"{target_field}, "rangeConstraint": "CONTIGUOUS"}}}}
            ]}},
            "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}
        }}]
    }}"#
    )
}

#[test]
fn chunks_default_task_count_static_zero_fails() {
    check_err(
        &job_with_chunks("{{ 1 - 1 }}", None),
        &["steps[0] -> parameterSpace -> taskParameterDefinitions[0] -> chunks -> defaultTaskCount:\n\tdefaultTaskCount must be >= 1."],
    );
}

#[test]
fn chunks_target_runtime_static_negative_fails() {
    check_err(
        &job_with_chunks("{{ 5 }}", Some("{{ 0 - 10 }}")),
        &["steps[0] -> parameterSpace -> taskParameterDefinitions[0] -> chunks -> targetRuntimeSeconds:\n\ttargetRuntimeSeconds must be >= 0."],
    );
}

#[test]
fn chunks_default_task_count_bound_unreasonably_long_fails() {
    check_err(
        &job_with_chunks("{{ Param.X }}{{ '1' * 200 }}", None),
        &["steps[0] -> parameterSpace -> taskParameterDefinitions[0] -> chunks -> defaultTaskCount:\n\tresolves to at least 200 characters, which cannot be a reasonable integer value."],
    );
}

#[test]
fn chunks_valid_and_unresolved_pass() {
    check_ok(&job_with_chunks("{{ 5 }}", Some("{{ 30 * 2 }}")));
    check_ok(&job_with_chunks("{{ Param.X }}", None));
    // Whole-field expressions resolve with target type `int` (Expression
    // Language §1.2.3): whole-number floats and numeric strings coerce.
    check_ok(&job_with_chunks("{{ 4.0 }}", None));
    check_ok(&job_with_chunks("{{ '4' }}", None));
    // targetRuntimeSeconds is optional, so its target is `int?`: a
    // whole-field null means "field omitted".
    check_ok(&job_with_chunks("{{ 5 }}", Some("{{ null }}")));
}

#[test]
fn chunks_static_uncoercible_fails() {
    // No null semantics for chunks: the target is plain `int`, so an
    // uncoercible value fails with the resolution diagnostic.
    check_err(
        &job_with_chunks("{{ 'abc' }}", None),
        &["steps[0] -> parameterSpace -> taskParameterDefinitions[0] -> chunks -> defaultTaskCount:\n\tFailed to parse interpolation expression at [0, 11]. Cannot convert 'abc' to int: invalid digit found in string"],
    );
}

// ══════════════════════════════════════════════════════════════
// FEATURE_BUNDLE_1 amount capabilities — min ≥ 0, max > 0
// ══════════════════════════════════════════════════════════════

fn job_with_amount(field: &str, value: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR", "FEATURE_BUNDLE_1"],
        "name": "T",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "steps": [{{
            "name": "S",
            "hostRequirements": {{"amounts": [{{"name": "amount.worker.gpu", "{field}": "{value}"}}]}},
            "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}
        }}]
    }}"#
    )
}

#[test]
fn amount_min_static_negative_fails() {
    check_err(
        &job_with_amount("min", "{{ 0.0 - 1.5 }}"),
        &["steps[0] -> hostRequirements -> amounts[0] -> min:\n\tmust be non-negative."],
    );
}

#[test]
fn amount_max_static_zero_fails() {
    check_err(
        &job_with_amount("max", "{{ 0.0 }}"),
        &["steps[0] -> hostRequirements -> amounts[0] -> max:\n\tmust be positive."],
    );
}

#[test]
fn amount_min_bound_unreasonably_long_fails() {
    check_err(
        &job_with_amount("min", "{{ Param.X }}{{ '9' * 200 }}"),
        &["steps[0] -> hostRequirements -> amounts[0] -> min:\n\tresolves to at least 200 characters, which cannot be a reasonable number."],
    );
}

#[test]
fn amount_valid_and_unresolved_pass() {
    check_ok(&job_with_amount("min", "{{ 1.5 }}"));
    check_ok(&job_with_amount("max", "{{ 4.0 * 2 }}"));
    check_ok(&job_with_amount("min", "{{ Param.X }}"));
}

// ══════════════════════════════════════════════════════════════
// Opt-in caller caps (Group B): action command/args (§5.1/§5.2)
// and embedded-file data (§6.1.2) under CallerLimits
// ══════════════════════════════════════════════════════════════
//
// The spec sets no maximum on these fields, so the caps are opt-in —
// `CallerLimits::max_resolved_arg_len` / `max_resolved_data_len` —
// and the default (`None`) imposes nothing. When set, template
// validation fails as soon as the guaranteed lower bound on any
// possible resolution exceeds the cap.

fn check_err_limits(s: &str, limits: &CallerLimits, expected: &[&str]) {
    let v = yaml_val(s);
    let err =
        decode_job_template(v, Some(ALL_EXTS), limits).expect_err("Expected validation error");
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

fn check_ok_limits(s: &str, limits: &CallerLimits) {
    let v = yaml_val(s);
    if let Err(e) = decode_job_template(v, Some(ALL_EXTS), limits) {
        panic!("Expected template to validate, got:\n{e}");
    }
}

fn arg_cap(n: usize) -> CallerLimits {
    CallerLimits {
        max_resolved_arg_len: Some(n),
        ..Default::default()
    }
}

/// Minimal job template with one STRING parameter `X` and the given
/// (pre-escaped JSON) `args` array element.
fn job_with_arg(arg: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "echo", "args": ["{arg}"]}}}}}}}}]
    }}"#
    )
}

fn job_with_command(command: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "{command}"}}}}}}}}]
    }}"#
    )
}

fn job_with_data(data: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "steps": [{{"name": "S", "script": {{
            "actions": {{"onRun": {{"command": "echo"}}}},
            "embeddedFiles": [{{"name": "F", "type": "TEXT", "data": "{data}"}}]
        }}}}]
    }}"#
    )
}

#[test]
fn arg_static_over_cap_fails() {
    check_err_limits(
        &job_with_arg("{{ 'A' * 200 }}"),
        &arg_cap(100),
        &["steps[0] -> script -> actions -> onRun -> args[0]:\n\tresolves to at least 200 characters, exceeding the maximum of 100."],
    );
}

#[test]
fn arg_bound_with_unresolved_part_fails() {
    // The Session.WorkingDirectory segment only
    // resolves on the worker (contributes 0 to the bound), but the
    // static segment alone already exceeds the cap — no possible
    // resolution conforms.
    check_err_limits(
        &job_with_arg("{{ Session.WorkingDirectory }}/{{ 'A' * 200 }}"),
        &arg_cap(100),
        &["steps[0] -> script -> actions -> onRun -> args[0]:\n\tresolves to at least 201 characters, exceeding the maximum of 100."],
    );
}

#[test]
fn arg_unresolved_and_under_cap_pass() {
    check_ok_limits(&job_with_arg("{{ Param.X }}"), &arg_cap(100));
    check_ok_limits(&job_with_arg("{{ 'A' * 100 }}"), &arg_cap(100));
}

#[test]
fn arg_over_cap_passes_without_opt_in() {
    // The library default imposes no cap (§5.2 sets no maximum).
    check_ok_limits(&job_with_arg("{{ 'A' * 200 }}"), &CallerLimits::default());
}

#[test]
fn literal_arg_over_cap_fails() {
    // A purely-literal element never reaches expression validation, but
    // its resolved value is known exactly — the raw text — so the opt-in
    // cap applies to it directly (no other pass length-checks args).
    check_err_limits(
        &job_with_arg(&"A".repeat(200)),
        &arg_cap(100),
        &["steps[0] -> script -> actions -> onRun -> args[0]:\n\tis 200 characters, exceeding the maximum of 100."],
    );
    check_ok_limits(&job_with_arg(&"A".repeat(100)), &arg_cap(100));
}

#[test]
fn literal_command_over_cap_fails() {
    // The raw-text pass caps command text at 1024; an opted-in cap below
    // that still applies to a literal command.
    check_err_limits(
        &job_with_command(&"A".repeat(150)),
        &arg_cap(100),
        &["steps[0] -> script -> actions -> onRun -> command:\n\tis 150 characters, exceeding the maximum of 100."],
    );
}

#[test]
fn literal_data_over_cap_fails() {
    let limits = CallerLimits {
        max_resolved_data_len: Some(100),
        ..Default::default()
    };
    check_err_limits(
        &job_with_data(&"A".repeat(200)),
        &limits,
        &["steps[0] -> script -> embeddedFiles[0] -> data:\n\tis 200 characters, exceeding the maximum of 100."],
    );
    check_ok_limits(&job_with_data(&"A".repeat(200)), &CallerLimits::default());
}

#[test]
fn arg_list_flatten_element_over_cap_fails() {
    // A list-valued args element flattens into one argv entry per
    // member: the cap applies to each entry, not to the list's display
    // form.
    check_err_limits(
        &job_with_arg("{{ ['A' * 150] * 2 }}"),
        &arg_cap(100),
        &[
            "steps[0] -> script -> actions -> onRun -> args[0]:\n\tlist element 0 resolves to 150 characters, exceeding the maximum of 100.",
            "list element 1 resolves to 150 characters, exceeding the maximum of 100.",
        ],
    );
}

#[test]
fn arg_list_flatten_elements_under_cap_pass() {
    // The list's display form is over the cap, but each flattened argv
    // entry is under it — the whole-string bound must not apply to a
    // list-valued resolution.
    check_ok_limits(&job_with_arg("{{ ['A' * 90] * 3 }}"), &arg_cap(100));
}

#[test]
fn command_static_over_cap_fails() {
    check_err_limits(
        &job_with_command("{{ 'A' * 200 }}"),
        &arg_cap(100),
        &["steps[0] -> script -> actions -> onRun -> command:\n\tresolves to at least 200 characters, exceeding the maximum of 100."],
    );
}

#[test]
fn data_static_over_cap_fails() {
    let limits = CallerLimits {
        max_resolved_data_len: Some(100),
        ..Default::default()
    };
    check_err_limits(
        &job_with_data("{{ 'A' * 200 }}"),
        &limits,
        &["steps[0] -> script -> embeddedFiles[0] -> data:\n\tresolves to at least 200 characters, exceeding the maximum of 100."],
    );
}

#[test]
fn data_over_cap_passes_without_opt_in() {
    check_ok_limits(&job_with_data("{{ 'A' * 200 }}"), &CallerLimits::default());
}

#[test]
fn env_template_arg_over_cap_fails() {
    // decode_environment_template applies the same caps (it now carries
    // CallerLimits like decode_job_template).
    let v = yaml_val(
        r#"{
        "specificationVersion": "environment-2023-09",
        "extensions": ["EXPR"],
        "environment": {"name": "E", "script": {"actions": {"onEnter": {"command": "echo", "args": ["{{ 'A' * 200 }}"]}}}}
    }"#,
    );
    let err = decode_environment_template(v, Some(ALL_EXTS), &arg_cap(100))
        .expect_err("Expected validation error");
    assert!(
        err.to_string().contains(
            "environment -> script -> actions -> onEnter -> args[0]:\n\tresolves to at least 200 characters, exceeding the maximum of 100."
        ),
        "Got:\n{err}"
    );
}

// ══════════════════════════════════════════════════════════════
// Evaluation budgets (Expression Language "Memory-bounded
// evaluation"): CallerLimits::max_eval_memory_bytes / max_eval_operations
// ══════════════════════════════════════════════════════════════

#[test]
fn lowered_memory_budget_fails_static_blowup_at_validation() {
    // With no cap opted in, a lowered evaluation
    // memory budget — the spec's own lever against `'A' * N` blowups —
    // fails the template at validation time.
    let limits = CallerLimits {
        max_eval_memory_bytes: Some(1000),
        ..Default::default()
    };
    check_err_limits(
        &job_with_arg("{{ 'A' * 100000 }}"),
        &limits,
        &[
            "steps[0] -> script -> actions -> onRun -> args[0]:\n\tFailed to parse interpolation expression at [0, 18].",
            "exceeded limit (1000 bytes)",
        ],
    );
}

#[test]
fn lowered_operation_budget_fails_at_validation() {
    let limits = CallerLimits {
        max_eval_operations: Some(50),
        ..Default::default()
    };
    check_err_limits(
        &job_with_arg("{{ sum([1] * 1000) }}"),
        &limits,
        &[
            "steps[0] -> script -> actions -> onRun -> args[0]:",
            "exceeded limit (50)",
        ],
    );
}

#[test]
fn default_budgets_pass_static_blowup() {
    // Under the spec-default 100 MB budget a 100 KB string is fine, and
    // with no cap opted in nothing else rejects it.
    check_ok_limits(
        &job_with_arg("{{ 'A' * 100000 }}"),
        &CallerLimits::default(),
    );
}

#[test]
fn lowered_memory_budget_bounds_let_bindings_at_validation() {
    // Let-binding evaluation goes through the same budgets as
    // format-string expressions — the budgets are the spec's lever
    // against exactly this shape, so a `let` must not bypass them.
    let limits = CallerLimits {
        max_eval_memory_bytes: Some(1000),
        ..Default::default()
    };
    let template = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "script": {
                "let": ["x = 'a' * 100000"],
                "actions": {"onRun": {"command": "echo"}}
            }
        }]
    }"#;
    check_err_limits(
        template,
        &limits,
        &[
            "Invalid expression in let binding 'x'",
            "exceeded limit (1000 bytes)",
        ],
    );
    // Same template under default budgets validates fine.
    check_ok_limits(template, &CallerLimits::default());
}
