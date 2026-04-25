// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test/openjd/model/v2023_09/test_step_host_requirements.py
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
