// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test/openjd/model/v2023_09/test_step_host_requirements.py
//!
//! The final section, "Resolved `<AttributeCapabilityValue>` constraints", is
//! not a port. It is a job-creation suite covering the §3.3.2.2 value check
//! that decode defers when an `anyOf`/`allOf` element is a format string.
//!
//! Gold standard: failure tests assert the full error message including path.

use openjd_model::decode_job_template;
use openjd_model::CallerLimits;

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn job_with_host_req(hr_json: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}, "hostRequirements": {hr_json}}}]
    }}"#
    )
}

fn decode_ok(s: &str) {
    let v = yaml_val(s);
    decode_job_template(v, None, &CallerLimits::default())
        .unwrap_or_else(|_| panic!("Expected success for: {s}"));
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

// ══════════════════════════════════════════════════════════════
// Attribute requirements — success cases
// ══════════════════════════════════════════════════════════════

#[test]
fn test_attr_os_family_any_of() {
    decode_ok(&job_with_host_req(
        r#"{"attributes": [{"name": "attr.worker.os.family", "anyOf": ["linux"]}]}"#,
    ));
}

#[test]
fn test_attr_os_family_any_of_multiple() {
    decode_ok(&job_with_host_req(
        r#"{"attributes": [{"name": "attr.worker.os.family", "anyOf": ["linux", "windows"]}]}"#,
    ));
}

#[test]
fn test_attr_os_family_all_of_single() {
    decode_ok(&job_with_host_req(
        r#"{"attributes": [{"name": "attr.worker.os.family", "allOf": ["linux"]}]}"#,
    ));
}

#[test]
fn test_attr_cpu_arch_any_of() {
    decode_ok(&job_with_host_req(
        r#"{"attributes": [{"name": "attr.worker.cpu.arch", "anyOf": ["x86_64", "arm64"]}]}"#,
    ));
}

#[test]
fn test_attr_cpu_arch_all_of_single() {
    decode_ok(&job_with_host_req(
        r#"{"attributes": [{"name": "attr.worker.cpu.arch", "allOf": ["x86_64"]}]}"#,
    ));
}

#[test]
fn test_attr_user_defined() {
    decode_ok(&job_with_host_req(
        r#"{"attributes": [{"name": "attr.mycapability", "anyOf": ["somevalue"]}]}"#,
    ));
}

#[test]
fn test_attr_user_defined_all_of() {
    decode_ok(&job_with_host_req(
        r#"{"attributes": [{"name": "attr.mycapability", "allOf": ["somevalue"]}]}"#,
    ));
}

#[test]
fn test_attr_both_any_and_all() {
    decode_ok(&job_with_host_req(
        r#"{"attributes": [{"name": "attr.mycapability", "allOf": ["foo"], "anyOf": ["bar"]}]}"#,
    ));
}

#[test]
fn test_attr_any_of_format_string() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Foo", "type": "STRING", "default": "x86_64"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}, "hostRequirements": {"attributes": [{"name": "attr.worker.cpu.arch", "anyOf": ["{{ Param.Foo }}"]}]}}]
    }"#,
    );
    decode_job_template(v, None, &CallerLimits::default()).expect("Expected success");
}

#[test]
fn test_attr_all_of_format_string() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Foo", "type": "STRING", "default": "x86_64"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}, "hostRequirements": {"attributes": [{"name": "attr.worker.cpu.arch", "allOf": ["{{ Param.Foo }}"]}]}}]
    }"#,
    );
    decode_job_template(v, None, &CallerLimits::default()).expect("Expected success");
}

#[test]
fn test_attr_any_of_max_elements() {
    let vals: Vec<String> = (0..50).map(|i| format!("\"value{i}\"")).collect();
    let s = job_with_host_req(&format!(
        r#"{{"attributes": [{{"name": "attr.mycapability", "anyOf": [{}]}}]}}"#,
        vals.join(",")
    ));
    decode_ok(&s);
}

#[test]
fn test_attr_all_of_max_elements() {
    let vals: Vec<String> = (0..50).map(|i| format!("\"value{i}\"")).collect();
    let s = job_with_host_req(&format!(
        r#"{{"attributes": [{{"name": "attr.mycapability", "allOf": [{}]}}]}}"#,
        vals.join(",")
    ));
    decode_ok(&s);
}

// ══════════════════════════════════════════════════════════════
// Attribute requirements — failure cases
// ══════════════════════════════════════════════════════════════

#[test]
fn test_attr_missing_any_and_all() {
    check_err(&job_with_host_req(r#"{"attributes": [{"name": "attr.mycapability"}]}"#), &[
        "steps[0] -> hostRequirements -> attributes[0]:\n\tmust have at least one of anyOf or allOf.",
    ]);
}

#[test]
fn test_attr_empty_any_of() {
    check_err(
        &job_with_host_req(r#"{"attributes": [{"name": "attr.mycapability", "anyOf": []}]}"#),
        &["steps[0] -> hostRequirements -> attributes[0] -> anyOf:\n\tmust not be empty."],
    );
}

#[test]
fn test_attr_empty_all_of() {
    check_err(
        &job_with_host_req(r#"{"attributes": [{"name": "attr.mycapability", "allOf": []}]}"#),
        &["steps[0] -> hostRequirements -> attributes[0] -> allOf:\n\tmust not be empty."],
    );
}

#[test]
fn test_attr_any_of_too_many() {
    let vals: Vec<String> = (0..51).map(|i| format!("\"value{i}\"")).collect();
    let s = job_with_host_req(&format!(
        r#"{{"attributes": [{{"name": "attr.mycapability", "anyOf": [{}]}}]}}"#,
        vals.join(",")
    ));
    check_err(
        &s,
        &["steps[0] -> hostRequirements -> attributes[0] -> anyOf:\n\texceeds 50 elements."],
    );
}

#[test]
fn test_attr_all_of_too_many() {
    let vals: Vec<String> = (0..51).map(|i| format!("\"value{i}\"")).collect();
    let s = job_with_host_req(&format!(
        r#"{{"attributes": [{{"name": "attr.mycapability", "allOf": [{}]}}]}}"#,
        vals.join(",")
    ));
    check_err(
        &s,
        &["steps[0] -> hostRequirements -> attributes[0] -> allOf:\n\texceeds 50 elements."],
    );
}

#[test]
fn test_attr_reserved_scope() {
    check_err(&job_with_host_req(r#"{"attributes": [{"name": "attr.worker.custom", "anyOf": ["foo"]}]}"#), &[
        "steps[0] -> hostRequirements -> attributes[0]:\n\tcapability 'attr.worker.custom' uses reserved scope 'worker'. Only spec-defined capabilities may use this scope.",
    ]);
}

#[test]
fn test_attr_os_family_missing_any_all() {
    check_err(&job_with_host_req(r#"{"attributes": [{"name": "attr.worker.os.family"}]}"#), &[
        "steps[0] -> hostRequirements -> attributes[0]:\n\tmust have at least one of anyOf or allOf.",
    ]);
}

#[test]
fn test_attr_os_family_invalid_value() {
    check_err(&job_with_host_req(r#"{"attributes": [{"name": "attr.worker.os.family", "anyOf": ["personalos"]}]}"#), &[
        "steps[0] -> hostRequirements -> attributes[0] -> anyOf:\n\tvalue 'personalos' is not valid for attr.worker.os.family.",
    ]);
}

#[test]
fn test_attr_os_family_empty_any_of() {
    check_err(
        &job_with_host_req(r#"{"attributes": [{"name": "attr.worker.os.family", "anyOf": []}]}"#),
        &["steps[0] -> hostRequirements -> attributes[0] -> anyOf:\n\tmust not be empty."],
    );
}

#[test]
fn test_attr_os_family_all_of_multiple() {
    check_err(&job_with_host_req(r#"{"attributes": [{"name": "attr.worker.os.family", "allOf": ["linux", "windows"]}]}"#), &[
        "steps[0] -> hostRequirements -> attributes[0] -> allOf:\n\tsingle-valued attribute cannot have more than 1 element.",
    ]);
}

#[test]
fn test_attr_cpu_arch_invalid_value() {
    check_err(&job_with_host_req(r#"{"attributes": [{"name": "attr.worker.cpu.arch", "allOf": ["x86_128"]}]}"#), &[
        "steps[0] -> hostRequirements -> attributes[0] -> allOf:\n\tvalue 'x86_128' is not valid for attr.worker.cpu.arch.",
    ]);
}

#[test]
fn test_attr_cpu_arch_empty_all_of() {
    check_err(
        &job_with_host_req(r#"{"attributes": [{"name": "attr.worker.cpu.arch", "allOf": []}]}"#),
        &["steps[0] -> hostRequirements -> attributes[0] -> allOf:\n\tmust not be empty."],
    );
}

#[test]
fn test_attr_cpu_arch_all_of_multiple() {
    check_err(&job_with_host_req(r#"{"attributes": [{"name": "attr.worker.cpu.arch", "allOf": ["x86_64", "arm64"]}]}"#), &[
        "steps[0] -> hostRequirements -> attributes[0] -> allOf:\n\tsingle-valued attribute cannot have more than 1 element.",
    ]);
}

#[test]
fn test_vendor_attr_missing_any_all() {
    check_err(&job_with_host_req(r#"{"attributes": [{"name": "vendor:attr.somecapability"}]}"#), &[
        "steps[0] -> hostRequirements -> attributes[0]:\n\tmust have at least one of anyOf or allOf.",
    ]);
}

#[test]
fn test_vendor_attr_empty_any_of() {
    check_err(
        &job_with_host_req(
            r#"{"attributes": [{"name": "vendor:attr.somecapability", "anyOf": []}]}"#,
        ),
        &["steps[0] -> hostRequirements -> attributes[0] -> anyOf:\n\tmust not be empty."],
    );
}

#[test]
fn test_vendor_attr_empty_all_of() {
    check_err(
        &job_with_host_req(
            r#"{"attributes": [{"name": "vendor:attr.somecapability", "allOf": []}]}"#,
        ),
        &["steps[0] -> hostRequirements -> attributes[0] -> allOf:\n\tmust not be empty."],
    );
}

// ══════════════════════════════════════════════════════════════
// Attribute value string constraints
// ══════════════════════════════════════════════════════════════

#[test]
fn test_attr_value_anyof_empty_string() {
    check_err(
        &job_with_host_req(r#"{"attributes": [{"name": "attr.custom", "anyOf": [""]}]}"#),
        &["steps[0] -> hostRequirements -> attributes[0] -> anyOf[0]:\n\tmust not be empty."],
    );
}

#[test]
fn test_attr_value_anyof_too_long() {
    let val = "a".repeat(101);
    let s = job_with_host_req(&format!(
        r#"{{"attributes": [{{"name": "attr.custom", "anyOf": ["{val}"]}}]}}"#
    ));
    check_err(
        &s,
        &["steps[0] -> hostRequirements -> attributes[0] -> anyOf[0]:\n\texceeds 100 characters."],
    );
}

#[test]
fn test_attr_value_anyof_starts_with_digit() {
    check_err(&job_with_host_req(r#"{"attributes": [{"name": "attr.custom", "anyOf": ["0abc"]}]}"#), &[
        "steps[0] -> hostRequirements -> attributes[0] -> anyOf[0]:\n\tvalue '0abc' contains invalid characters.",
    ]);
}

#[test]
fn test_attr_value_anyof_invalid_char() {
    check_err(&job_with_host_req(r#"{"attributes": [{"name": "attr.custom", "anyOf": ["A!"]}]}"#), &[
        "steps[0] -> hostRequirements -> attributes[0] -> anyOf[0]:\n\tvalue 'A!' contains invalid characters.",
    ]);
}

#[test]
fn test_attr_value_allof_empty_string() {
    check_err(
        &job_with_host_req(r#"{"attributes": [{"name": "attr.custom", "allOf": [""]}]}"#),
        &["steps[0] -> hostRequirements -> attributes[0] -> allOf[0]:\n\tmust not be empty."],
    );
}

#[test]
fn test_attr_value_allof_too_long() {
    let val = "a".repeat(101);
    let s = job_with_host_req(&format!(
        r#"{{"attributes": [{{"name": "attr.custom", "allOf": ["{val}"]}}]}}"#
    ));
    check_err(
        &s,
        &["steps[0] -> hostRequirements -> attributes[0] -> allOf[0]:\n\texceeds 100 characters."],
    );
}

/// The element length and emptiness checks measure the value, not the template text,
/// so they are gated on the element being a literal exactly as the pattern check is.
/// A long format string resolving to a short legal value is accepted at decode; the
/// resolved value is checked at job creation instead. The reference implementation
/// does the same: its `_validate_attribute_list` skips any format string carrying
/// expressions, and `AttributeCapabilityValue` declares no maximum length at all.
#[test]
fn test_attr_value_long_format_string_resolving_short_is_accepted_at_decode() {
    let long_name = format!("P{}", "a".repeat(54));
    let element = format!("{{{{Param.{long_name}}}}}{{{{Param.{long_name}}}}}");
    assert!(element.len() > 100, "the raw element must exceed the limit");
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{{"name": "{long_name}", "type": "STRING", "default": "short"}}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}},
            "hostRequirements": {{"attributes": [{{"name": "attr.custom", "anyOf": ["{element}"]}}]}}}}]
    }}"#
    );
    decode_ok(&s);
}

#[test]
fn test_attr_value_allof_starts_with_digit() {
    check_err(&job_with_host_req(r#"{"attributes": [{"name": "attr.custom", "allOf": ["0abc"]}]}"#), &[
        "steps[0] -> hostRequirements -> attributes[0] -> allOf[0]:\n\tvalue '0abc' contains invalid characters.",
    ]);
}

// ══════════════════════════════════════════════════════════════
// Amount requirements — success cases
// ══════════════════════════════════════════════════════════════

#[test]
fn test_amount_vcpu() {
    decode_ok(&job_with_host_req(
        r#"{"amounts": [{"name": "amount.worker.vcpu", "min": 1}]}"#,
    ));
}

#[test]
fn test_amount_memory() {
    decode_ok(&job_with_host_req(
        r#"{"amounts": [{"name": "amount.worker.memory", "min": 1024}]}"#,
    ));
}

#[test]
fn test_amount_gpu() {
    decode_ok(&job_with_host_req(
        r#"{"amounts": [{"name": "amount.worker.gpu", "min": 2}]}"#,
    ));
}

#[test]
fn test_amount_gpu_memory_float() {
    decode_ok(&job_with_host_req(
        r#"{"amounts": [{"name": "amount.worker.gpu.memory", "min": 2.25}]}"#,
    ));
}

#[test]
fn test_amount_disk_scratch_min_max() {
    decode_ok(&job_with_host_req(
        r#"{"amounts": [{"name": "amount.worker.disk.scratch", "min": 10, "max": 50}]}"#,
    ));
}

#[test]
fn test_amount_user_defined() {
    decode_ok(&job_with_host_req(
        r#"{"amounts": [{"name": "amount.custom", "min": 1}]}"#,
    ));
}

#[test]
fn test_amount_with_min_and_max() {
    decode_ok(&job_with_host_req(
        r#"{"amounts": [{"name": "amount.custom", "min": 1, "max": 10}]}"#,
    ));
}

#[test]
fn test_amount_user_min_max_float() {
    decode_ok(&job_with_host_req(
        r#"{"amounts": [{"name": "amount.mycapability", "min": 0.5, "max": 2.9}]}"#,
    ));
}

#[test]
fn test_amount_user_max_only_int() {
    decode_ok(&job_with_host_req(
        r#"{"amounts": [{"name": "amount.mycapability", "max": 1000}]}"#,
    ));
}

#[test]
fn test_amount_user_max_only_float() {
    decode_ok(&job_with_host_req(
        r#"{"amounts": [{"name": "amount.mycapability", "max": 10.79}]}"#,
    ));
}

#[test]
fn test_amount_vendor_min_max_float() {
    decode_ok(&job_with_host_req(
        r#"{"amounts": [{"name": "vendor:amount.capability", "min": 0.5, "max": 2.9}]}"#,
    ));
}

#[test]
fn test_amount_vendor_min_max_equal() {
    decode_ok(&job_with_host_req(
        r#"{"amounts": [{"name": "vendor:amount.capability", "min": 6, "max": 6}]}"#,
    ));
}

// ══════════════════════════════════════════════════════════════
// Amount requirements — failure cases
// ══════════════════════════════════════════════════════════════

#[test]
fn test_amount_missing_min_and_max() {
    check_err(
        &job_with_host_req(r#"{"amounts": [{"name": "amount.custom"}]}"#),
        &["steps[0] -> hostRequirements -> amounts[0]:\n\tmust have at least one of min or max."],
    );
}

#[test]
fn test_amount_min_greater_than_max() {
    check_err(
        &job_with_host_req(r#"{"amounts": [{"name": "amount.custom", "min": 10, "max": 1}]}"#),
        &["steps[0] -> hostRequirements -> amounts[0]:\n\tmin (10) > max (1)."],
    );
}

#[test]
fn test_amount_min_greater_than_max_int() {
    check_err(
        &job_with_host_req(r#"{"amounts": [{"name": "amount.mycap", "min": 3, "max": 2}]}"#),
        &["steps[0] -> hostRequirements -> amounts[0]:\n\tmin (3) > max (2)."],
    );
}

#[test]
fn test_amount_min_greater_than_max_float_close() {
    check_err(
        &job_with_host_req(r#"{"amounts": [{"name": "amount.mycap", "min": 0.3, "max": 0.29}]}"#),
        &["steps[0] -> hostRequirements -> amounts[0]:\n\tmin (0.3) > max (0.29)."],
    );
}

#[test]
fn test_amount_negative_min() {
    check_err(
        &job_with_host_req(r#"{"amounts": [{"name": "amount.custom", "min": -1}]}"#),
        &["steps[0] -> hostRequirements -> amounts[0] -> min:\n\tmust be non-negative."],
    );
}

#[test]
fn test_amount_negative_min_float() {
    check_err(
        &job_with_host_req(r#"{"amounts": [{"name": "amount.worker.disk.scratch", "min": -1.5}]}"#),
        &["steps[0] -> hostRequirements -> amounts[0] -> min:\n\tmust be non-negative."],
    );
}

#[test]
fn test_amount_max_zero() {
    check_err(
        &job_with_host_req(r#"{"amounts": [{"name": "amount.mycap", "max": 0}]}"#),
        &["steps[0] -> hostRequirements -> amounts[0] -> max:\n\tmust be positive."],
    );
}

#[test]
fn test_amount_non_finite_literals() {
    for value in ["nan", "NaN", "inf", "infinity", "-inf"] {
        let requirement =
            format!(r#"{{"amounts": [{{"name": "amount.custom", "min": "{value}"}}]}}"#);
        check_err(
            &job_with_host_req(&requirement),
            &["steps[0] -> hostRequirements -> amounts[0] -> min:\n\tmust be a finite number."],
        );
    }
}

#[test]
fn test_amount_invalid_number_literals() {
    for value in [".nan", ".inf", "-.inf", "not-a-number"] {
        let requirement =
            format!(r#"{{"amounts": [{{"name": "amount.custom", "max": "{value}"}}]}}"#);
        check_err(
            &job_with_host_req(&requirement),
            &["steps[0] -> hostRequirements -> amounts[0] -> max:\n\tmust be a number."],
        );
    }
}

#[test]
fn test_amount_reserved_scope() {
    check_err(&job_with_host_req(r#"{"amounts": [{"name": "amount.worker.custom", "min": 1}]}"#), &[
        "steps[0] -> hostRequirements -> amounts[0]:\n\tcapability 'amount.worker.custom' uses reserved scope 'worker'. Only spec-defined capabilities may use this scope.",
    ]);
}

// ══════════════════════════════════════════════════════════════
// HostRequirements — success cases
// ══════════════════════════════════════════════════════════════

#[test]
fn test_both_amounts_and_attributes() {
    decode_ok(&job_with_host_req(
        r#"{"amounts": [{"name": "amount.custom", "min": 1}], "attributes": [{"name": "attr.custom", "anyOf": ["foo"]}]}"#,
    ));
}

#[test]
fn test_max_amounts_only() {
    let amounts: Vec<String> = (0..50)
        .map(|i| format!(r#"{{"name": "amount.mycap{i}", "min": 1}}"#))
        .collect();
    decode_ok(&job_with_host_req(&format!(
        r#"{{"amounts": [{}]}}"#,
        amounts.join(",")
    )));
}

#[test]
fn test_max_attributes_only() {
    let attrs: Vec<String> = (0..50)
        .map(|i| format!(r#"{{"name": "attr.mycap{i}", "anyOf": ["foo"]}}"#))
        .collect();
    decode_ok(&job_with_host_req(&format!(
        r#"{{"attributes": [{}]}}"#,
        attrs.join(",")
    )));
}

#[test]
fn test_max_combination() {
    let amounts: Vec<String> = (0..25)
        .map(|i| format!(r#"{{"name": "amount.mycap{i}", "min": 1}}"#))
        .collect();
    let attrs: Vec<String> = (0..25)
        .map(|i| format!(r#"{{"name": "attr.mycap{i}", "anyOf": ["foo"]}}"#))
        .collect();
    decode_ok(&job_with_host_req(&format!(
        r#"{{"amounts": [{}], "attributes": [{}]}}"#,
        amounts.join(","),
        attrs.join(",")
    )));
}

// ══════════════════════════════════════════════════════════════
// HostRequirements — failure cases
// ══════════════════════════════════════════════════════════════

#[test]
fn test_empty_host_requirements() {
    check_err(
        &job_with_host_req(r#"{}"#),
        &["steps[0] -> hostRequirements:\n\tmust have at least one of amounts or attributes."],
    );
}

#[test]
fn test_empty_amounts_list() {
    check_err(
        &job_with_host_req(r#"{"amounts": []}"#),
        &["steps[0] -> hostRequirements:\n\tmust have at least one of amounts or attributes."],
    );
}

#[test]
fn test_empty_attributes_list() {
    check_err(
        &job_with_host_req(r#"{"attributes": []}"#),
        &["steps[0] -> hostRequirements:\n\tmust have at least one of amounts or attributes."],
    );
}

#[test]
fn test_too_many_amounts() {
    let amounts: Vec<String> = (0..51)
        .map(|i| format!(r#"{{"name": "amount.mycap{i}", "min": 1}}"#))
        .collect();
    let s = job_with_host_req(&format!(r#"{{"amounts": [{}]}}"#, amounts.join(",")));
    check_err(
        &s,
        &["steps[0] -> hostRequirements:\n\ttotal amounts + attributes must not exceed 50."],
    );
}

#[test]
fn test_too_many_attributes() {
    let attrs: Vec<String> = (0..51)
        .map(|i| format!(r#"{{"name": "attr.mycap{i}", "anyOf": ["foo"]}}"#))
        .collect();
    let s = job_with_host_req(&format!(r#"{{"attributes": [{}]}}"#, attrs.join(",")));
    check_err(
        &s,
        &["steps[0] -> hostRequirements:\n\ttotal amounts + attributes must not exceed 50."],
    );
}

#[test]
fn test_too_many_combination() {
    let amounts: Vec<String> = (0..26)
        .map(|i| format!(r#"{{"name": "amount.mycap{i}", "min": 1}}"#))
        .collect();
    let attrs: Vec<String> = (0..25)
        .map(|i| format!(r#"{{"name": "attr.mycap{i}", "anyOf": ["foo"]}}"#))
        .collect();
    let s = job_with_host_req(&format!(
        r#"{{"amounts": [{}], "attributes": [{}]}}"#,
        amounts.join(","),
        attrs.join(",")
    ));
    check_err(
        &s,
        &["steps[0] -> hostRequirements:\n\ttotal amounts + attributes must not exceed 50."],
    );
}

#[test]
fn test_duplicate_amount_names() {
    check_err(
        &job_with_host_req(
            r#"{"amounts": [{"name": "amount.custom", "min": 1}, {"name": "amount.custom", "min": 2}]}"#,
        ),
        &["steps[0] -> hostRequirements -> amounts[1]:\n\tduplicate amount name 'amount.custom'."],
    );
}

#[test]
fn test_duplicate_amount_names_case_insensitive() {
    check_err(&job_with_host_req(r#"{"amounts": [{"name": "amount.worker.vcpu", "min": 1}, {"name": "AMOUNT.WORKER.VCPU", "min": 2}]}"#), &[
        "steps[0] -> hostRequirements -> amounts[1]:\n\tduplicate amount name 'AMOUNT.WORKER.VCPU'.",
    ]);
}

#[test]
fn test_duplicate_attribute_names() {
    check_err(&job_with_host_req(r#"{"attributes": [{"name": "attr.custom", "anyOf": ["a"]}, {"name": "attr.custom", "anyOf": ["b"]}]}"#), &[
        "steps[0] -> hostRequirements -> attributes[1]:\n\tduplicate attribute name 'attr.custom'.",
    ]);
}

#[test]
fn test_duplicate_attribute_names_case_insensitive() {
    check_err(&job_with_host_req(r#"{"attributes": [{"name": "attr.worker.os.family", "anyOf": ["linux"]}, {"name": "ATTR.WORKER.OS.FAMILY", "anyOf": ["windows"]}]}"#), &[
        "steps[0] -> hostRequirements -> attributes[1]:\n\tduplicate attribute name 'ATTR.WORKER.OS.FAMILY'.",
    ]);
}

// ══════════════════════════════════════════════════════════════
// Vendor-prefixed capabilities — success cases
// ══════════════════════════════════════════════════════════════

#[test]
fn test_vendor_attr_any_of() {
    decode_ok(&job_with_host_req(
        r#"{"attributes": [{"name": "vendor:attr.somecapability", "anyOf": ["foo"]}]}"#,
    ));
}

#[test]
fn test_vendor_attr_all_of() {
    decode_ok(&job_with_host_req(
        r#"{"attributes": [{"name": "vendor:attr.somecapability", "allOf": ["foo"]}]}"#,
    ));
}

#[test]
fn test_vendor_amount_min() {
    decode_ok(&job_with_host_req(
        r#"{"amounts": [{"name": "vendor:amount.capability", "min": 1}]}"#,
    ));
}

// ══════════════════════════════════════════════════════════════
// Capability name validation — success cases
// ══════════════════════════════════════════════════════════════

#[test]
fn test_attr_name_with_dots() {
    decode_ok(&job_with_host_req(
        r#"{"attributes": [{"name": "attr.my.deep.capability", "anyOf": ["foo"]}]}"#,
    ));
}

#[test]
fn test_amount_name_with_dots() {
    decode_ok(&job_with_host_req(
        r#"{"amounts": [{"name": "amount.my.deep.capability", "min": 1}]}"#,
    ));
}

// ══════════════════════════════════════════════════════════════
// Amount min=0 succeeds
// ══════════════════════════════════════════════════════════════

#[test]
fn test_amount_min_zero_with_max() {
    decode_ok(&job_with_host_req(
        r#"{"amounts": [{"name": "amount.custom", "min": 0, "max": 1}]}"#,
    ));
}

// ══════════════════════════════════════════════════════════════
// Amount min=max succeeds
// ══════════════════════════════════════════════════════════════

#[test]
fn test_amount_min_equals_max() {
    decode_ok(&job_with_host_req(
        r#"{"amounts": [{"name": "amount.custom", "min": 5, "max": 5}]}"#,
    ));
}

// ══════════════════════════════════════════════════════════════
// Resolved <AttributeCapabilityValue> constraints (§3.3.2.2)
//
// `attributes[].anyOf` / `.allOf` elements are @fmtstring in base
// 2023-09, so an element written as a format string has an unknown
// value at decode and its constraints cannot be applied there.
// Job creation resolves the value, so the deferred check resumes
// there. These tests exercise job CREATION, not decode.
// ══════════════════════════════════════════════════════════════

use openjd_expr::path_mapping::PathFormat;
use openjd_model::{create_job, job, preprocess_job_parameters};

/// Decode `template_json` with `extensions` supported, preprocess `params` as
/// STRING inputs, and create the job.
///
/// Returns the rendered error text on failure at either stage so callers can
/// assert on the full message the way `check_err` does for decode.
fn create_job_from_with_extensions(
    template_json: &str,
    params: &[(&str, &str)],
    extensions: Option<&[&str]>,
) -> Result<job::Job, String> {
    let root = tempfile::TempDir::new().unwrap();
    let dir = root.path().to_str().unwrap();
    let jt = decode_job_template(
        yaml_val(template_json),
        extensions,
        &CallerLimits::default(),
    )
    .map_err(|e| e.to_string())?;
    let input: std::collections::HashMap<String, openjd_expr::ExprValue> = params
        .iter()
        .map(|(k, v)| (k.to_string(), openjd_expr::ExprValue::String(v.to_string())))
        .collect();
    let processed = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: dir,
            current_working_dir: dir,
            allow_template_dir_walk_up: true,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .map_err(|e| e.to_string())?;
    create_job(&jt, &processed, &jt.default_validation_context()).map_err(|e| e.to_string())
}

/// As [`create_job_from_with_extensions`], with no extensions enabled.
fn create_job_from(template_json: &str, params: &[(&str, &str)]) -> Result<job::Job, String> {
    create_job_from_with_extensions(template_json, params, None)
}

fn create_ok(template_json: &str, params: &[(&str, &str)]) -> job::Job {
    create_job_from(template_json, params)
        .unwrap_or_else(|e| panic!("Expected create_job success, got:\n{e}"))
}

fn create_err(template_json: &str, params: &[(&str, &str)], expected: &[&str]) -> String {
    assert_create_err(create_job_from(template_json, params), expected)
}

/// Assert `result` failed and that every line of `expected` appears in the
/// rendered error. Returns the rendered error so callers can also assert on
/// what is *absent*.
fn assert_create_err(result: Result<job::Job, String>, expected: &[&str]) -> String {
    let msg = result.expect_err("Expected create_job to fail");
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
    msg
}

/// A single-step job template whose `hostRequirements.attributes` is `attributes`
/// and which declares one unconstrained STRING parameter named `Value`.
fn job_with_attrs(attributes: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{{"name": "Value", "type": "STRING"}}],
        "steps": [{{
            "name": "S",
            "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}},
            "hostRequirements": {{"attributes": {attributes}}}
        }}]
    }}"#
    )
}

/// As [`job_with_attrs`], but declaring two STRING parameters, `Value` and `Other`,
/// so a single `anyOf`/`allOf` list can hold two independently resolved values.
fn job_with_attrs_two_params(attributes: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [
            {{"name": "Value", "type": "STRING"}},
            {{"name": "Other", "type": "STRING"}}
        ],
        "steps": [{{
            "name": "S",
            "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}},
            "hostRequirements": {{"attributes": {attributes}}}
        }}]
    }}"#
    )
}

/// A legal `<AttributeCapabilityValue>` of exactly `len` characters.
fn identifier_of_len(len: usize) -> String {
    "a".repeat(len)
}

// ── Non-standard capability: pattern and length constraints ──

#[test]
fn test_resolved_attr_any_of_invalid_chars_rejected() {
    create_err(
        &job_with_attrs(r#"[{"name": "attr.custom.software", "anyOf": ["{{Param.Value}}"]}]"#),
        &[("Value", "not valid!")],
        &[
            "steps[0] -> hostRequirements -> attributes[0] -> anyOf[0]:\n\tvalue 'not valid!' contains invalid characters.",
        ],
    );
}

#[test]
fn test_resolved_attr_all_of_invalid_chars_rejected() {
    create_err(
        &job_with_attrs(r#"[{"name": "attr.custom.software", "allOf": ["{{Param.Value}}"]}]"#),
        &[("Value", "not valid!")],
        &[
            "steps[0] -> hostRequirements -> attributes[0] -> allOf[0]:\n\tvalue 'not valid!' contains invalid characters.",
        ],
    );
}

#[test]
fn test_resolved_attr_value_over_100_chars_rejected() {
    let long = identifier_of_len(101);
    create_err(
        &job_with_attrs(r#"[{"name": "attr.custom.software", "anyOf": ["{{Param.Value}}"]}]"#),
        &[("Value", &long)],
        &[&format!(
            "steps[0] -> hostRequirements -> attributes[0] -> anyOf[0]:\n\tvalue '{long}' exceeds 100 characters."
        )],
    );
}

#[test]
fn test_resolved_attr_value_empty_rejected() {
    create_err(
        &job_with_attrs(r#"[{"name": "attr.custom.software", "anyOf": ["{{Param.Value}}"]}]"#),
        &[("Value", "")],
        &["steps[0] -> hostRequirements -> attributes[0] -> anyOf[0]:\n\tmust not be empty."],
    );
}

// ── Standard capability: fixed value set, compared case-insensitively ──

#[test]
fn test_resolved_attr_standard_capability_unknown_value_rejected() {
    create_err(
        &job_with_attrs(r#"[{"name": "attr.worker.os.family", "anyOf": ["{{Param.Value}}"]}]"#),
        &[("Value", "solaris")],
        &[
            "steps[0] -> hostRequirements -> attributes[0] -> anyOf[0]:\n\tvalue 'solaris' is not valid for attr.worker.os.family.",
        ],
    );
}

#[test]
fn test_resolved_attr_standard_capability_value_case_insensitive_ok() {
    let job = create_ok(
        &job_with_attrs(r#"[{"name": "attr.worker.os.family", "anyOf": ["{{Param.Value}}"]}]"#),
        &[("Value", "LINUX")],
    );
    let attrs = job.steps[0]
        .host_requirements
        .as_ref()
        .unwrap()
        .attributes
        .as_ref()
        .unwrap();
    assert_eq!(attrs[0].any_of.as_ref().unwrap(), &["LINUX".to_string()]);
}

/// A capability NAME is matched against the standard table case-insensitively, so
/// `ATTR.WORKER.OS.FAMILY` is still constrained to the OS family value set. The
/// message names the lowercased table entry, not the name as written.
#[test]
fn test_resolved_attr_standard_capability_name_matched_case_insensitively() {
    create_err(
        &job_with_attrs(r#"[{"name": "ATTR.WORKER.OS.FAMILY", "anyOf": ["{{Param.Value}}"]}]"#),
        &[("Value", "solaris")],
        &[
            "steps[0] -> hostRequirements -> attributes[0] -> anyOf[0]:\n\tvalue 'solaris' is not valid for attr.worker.os.family.",
        ],
    );
}

/// Every bad value in one list is reported, not just the first.
#[test]
fn test_resolved_attr_all_invalid_values_in_one_list_reported() {
    create_err(
        &job_with_attrs_two_params(
            r#"[{"name": "attr.custom.software", "anyOf": ["{{Param.Value}}", "{{Param.Other}}"]}]"#,
        ),
        &[("Value", "first bad!"), ("Other", "second bad!")],
        &[
            "2 validation errors for JobTemplate",
            "steps[0] -> hostRequirements -> attributes[0] -> anyOf[0]:\n\tvalue 'first bad!' contains invalid characters.",
            "steps[0] -> hostRequirements -> attributes[0] -> anyOf[1]:\n\tvalue 'second bad!' contains invalid characters.",
        ],
    );
}

// ── Error path indices point at the offending step and attribute ──

#[test]
fn test_resolved_attr_error_reports_second_step_index() {
    let msg = create_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Value", "type": "STRING"}],
        "steps": [
            {
                "name": "Good",
                "script": {"actions": {"onRun": {"command": "foo"}}},
                "hostRequirements": {"attributes": [{"name": "attr.custom.software", "anyOf": ["blender"]}]}
            },
            {
                "name": "Bad",
                "script": {"actions": {"onRun": {"command": "foo"}}},
                "hostRequirements": {"attributes": [{"name": "attr.custom.software", "anyOf": ["{{Param.Value}}"]}]}
            }
        ]
    }"#,
        &[("Value", "not valid!")],
        &[
            "steps[1] -> hostRequirements -> attributes[0] -> anyOf[0]:\n\tvalue 'not valid!' contains invalid characters.",
        ],
    );
    assert!(
        !msg.contains("steps[0] -> hostRequirements"),
        "Error blamed the wrong step:\n{msg}"
    );
}

#[test]
fn test_resolved_attr_error_reports_second_attribute_index() {
    let msg = create_err(
        &job_with_attrs(
            r#"[
                {"name": "attr.custom.first", "anyOf": ["blender"]},
                {"name": "attr.custom.second", "anyOf": ["{{Param.Value}}"]}
            ]"#,
        ),
        &[("Value", "not valid!")],
        &[
            "steps[0] -> hostRequirements -> attributes[1] -> anyOf[0]:\n\tvalue 'not valid!' contains invalid characters.",
        ],
    );
    assert!(
        !msg.contains("attributes[0] -> anyOf"),
        "Error blamed the wrong attribute:\n{msg}"
    );
}

#[test]
fn test_resolved_attr_error_reports_second_value_index() {
    let msg = create_err(
        &job_with_attrs(
            r#"[{"name": "attr.custom.software", "anyOf": ["blender", "{{Param.Value}}"]}]"#,
        ),
        &[("Value", "not valid!")],
        &[
            "steps[0] -> hostRequirements -> attributes[0] -> anyOf[1]:\n\tvalue 'not valid!' contains invalid characters.",
        ],
    );
    assert!(
        !msg.contains("anyOf[0]"),
        "Error blamed the wrong value:\n{msg}"
    );
}

// ── Negative controls: legal values must keep creating jobs ──

#[test]
fn test_resolved_attr_legal_identifier_accepted() {
    let job = create_ok(
        &job_with_attrs(r#"[{"name": "attr.custom.software", "anyOf": ["{{Param.Value}}"]}]"#),
        &[("Value", "blender-4_2")],
    );
    let attrs = job.steps[0]
        .host_requirements
        .as_ref()
        .unwrap()
        .attributes
        .as_ref()
        .unwrap();
    assert_eq!(
        attrs[0].any_of.as_ref().unwrap(),
        &["blender-4_2".to_string()]
    );
}

/// Literal values are unaffected by the resolved-value check, on both the `anyOf`
/// branch (a standard capability, "run on Linux or macOS") and the `allOf` branch
/// (a custom capability, "the worker must carry both licenses"). The two live on
/// separate attributes because `anyOf: [linux]` with `allOf: [windows]` on one
/// attribute is unsatisfiable and would mislead a reader.
#[test]
fn test_literal_attr_legal_value_still_accepted() {
    let job = create_ok(
        &job_with_attrs(
            r#"[
                {"name": "attr.worker.os.family", "anyOf": ["linux", "macos"]},
                {"name": "attr.custom.licenses", "allOf": ["maya", "nuke"]}
            ]"#,
        ),
        &[("Value", "unused")],
    );
    let attrs = job.steps[0]
        .host_requirements
        .as_ref()
        .unwrap()
        .attributes
        .as_ref()
        .unwrap();
    assert_eq!(
        attrs[0].any_of.as_ref().unwrap(),
        &["linux".to_string(), "macos".to_string()]
    );
    assert_eq!(
        attrs[1].all_of.as_ref().unwrap(),
        &["maya".to_string(), "nuke".to_string()]
    );
}

#[test]
fn test_resolved_attr_value_exactly_100_chars_accepted() {
    let boundary = identifier_of_len(100);
    let job = create_ok(
        &job_with_attrs(r#"[{"name": "attr.custom.software", "anyOf": ["{{Param.Value}}"]}]"#),
        &[("Value", &boundary)],
    );
    let attrs = job.steps[0]
        .host_requirements
        .as_ref()
        .unwrap()
        .attributes
        .as_ref()
        .unwrap();
    assert_eq!(attrs[0].any_of.as_ref().unwrap(), &[boundary]);
}
// ══════════════════════════════════════════════════════════════
// Resolved amounts[].min / .max bounds (§3.3.1)
//
// Under FEATURE_BUNDLE_1 both fields may be format strings, so
// their bounds have an unknown value at decode and cannot be
// applied there — `validate_v2023_09::structure`'s
// `parse_literal_amount` returns `None` for a non-literal, which
// skips all three checks. Job creation resolves the values, so the
// deferred checks resume there. These tests exercise job CREATION,
// not decode.
//
// `min` is `<nonnegativefloat>` and `max` is `<positivefloat>`, so
// `0` is legal for one and not the other.
// ══════════════════════════════════════════════════════════════

/// Extensions needed for `amounts[].min` / `.max` to accept a format string.
const FB1: Option<&[&str]> = Some(&["FEATURE_BUNDLE_1"]);

/// A single-step FEATURE_BUNDLE_1 job template whose `hostRequirements.amounts`
/// is `amounts`, declaring two unconstrained STRING parameters, `Min` and `Max`,
/// so either bound can be supplied as a resolved format string.
fn job_with_amounts(amounts: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "parameterDefinitions": [
            {{"name": "Min", "type": "STRING"}},
            {{"name": "Max", "type": "STRING"}}
        ],
        "steps": [{{
            "name": "S",
            "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}},
            "hostRequirements": {{"amounts": {amounts}}}
        }}]
    }}"#
    )
}

fn create_amounts_ok(amounts: &str, params: &[(&str, &str)]) -> job::Job {
    create_job_from_with_extensions(&job_with_amounts(amounts), params, FB1)
        .unwrap_or_else(|e| panic!("Expected create_job success, got:\n{e}"))
}

fn create_amounts_err(amounts: &str, params: &[(&str, &str)], expected: &[&str]) -> String {
    assert_create_err(
        create_job_from_with_extensions(&job_with_amounts(amounts), params, FB1),
        expected,
    )
}

/// The resolved `amounts` of the job's only step.
fn amounts_of(job: &job::Job) -> &[job::AmountRequirement] {
    job.steps[0]
        .host_requirements
        .as_ref()
        .unwrap()
        .amounts
        .as_ref()
        .unwrap()
}

// ── Each bound is re-checked on the resolved value ──

/// `max` is `<positivefloat>`, so a resolved `0` is out of range. This is the
/// case the old code accepted: it only checked that the text parsed as a finite
/// number.
#[test]
fn test_resolved_amount_max_zero_rejected() {
    create_amounts_err(
        r#"[{"name": "amount.custom.thing", "max": "{{Param.Max}}"}]"#,
        &[("Min", "1"), ("Max", "0")],
        &["steps[0] -> hostRequirements -> amounts[0] -> max:\n\tmust be positive."],
    );
}

#[test]
fn test_resolved_amount_min_negative_rejected() {
    create_amounts_err(
        r#"[{"name": "amount.custom.thing", "min": "{{Param.Min}}"}]"#,
        &[("Min", "-1"), ("Max", "1")],
        &["steps[0] -> hostRequirements -> amounts[0] -> min:\n\tmust be non-negative."],
    );
}

/// The cross-field check is reported on the amount itself, not on either bound.
#[test]
fn test_resolved_amount_min_greater_than_max_rejected() {
    create_amounts_err(
        r#"[{"name": "amount.custom.thing", "min": "{{Param.Min}}", "max": "{{Param.Max}}"}]"#,
        &[("Min", "10"), ("Max", "2")],
        &["steps[0] -> hostRequirements -> amounts[0]:\n\tmin (10) > max (2)."],
    );
}

/// `min` is `<nonnegativefloat>` while `max` is `<positivefloat>`, so `0` is
/// legal for `min` and not for `max`. This is the test that distinguishes the
/// two fields: a check written as `min <= 0.0` would reject this job.
#[test]
fn test_resolved_amount_min_zero_accepted() {
    let job = create_amounts_ok(
        r#"[{"name": "amount.custom.thing", "min": "{{Param.Min}}"}]"#,
        &[("Min", "0"), ("Max", "1")],
    );
    assert_eq!(amounts_of(&job)[0].min, Some(0.0));
}

// ── Error path indices point at the offending amount ──

/// Two amounts where only the second is out of range. The index in the path is
/// the amount's own index, not a hardcoded 0.
#[test]
fn test_resolved_amount_error_reports_second_amount_index() {
    let msg = create_amounts_err(
        r#"[
            {"name": "amount.custom.first", "min": "1", "max": "4"},
            {"name": "amount.custom.second", "max": "{{Param.Max}}"}
        ]"#,
        &[("Min", "1"), ("Max", "0")],
        &["steps[0] -> hostRequirements -> amounts[1] -> max:\n\tmust be positive."],
    );
    assert!(
        !msg.contains("amounts[0]"),
        "Error blamed the wrong amount:\n{msg}"
    );
}

// ── Negative control: in-range resolved bounds must keep creating jobs ──

#[test]
fn test_resolved_amount_ordinary_bounds_accepted() {
    let job = create_amounts_ok(
        r#"[{"name": "amount.custom.thing", "min": "{{Param.Min}}", "max": "{{Param.Max}}"}]"#,
        &[("Min", "2"), ("Max", "8")],
    );
    let amounts = amounts_of(&job);
    assert_eq!(amounts[0].min, Some(2.0));
    assert_eq!(amounts[0].max, Some(8.0));
}
