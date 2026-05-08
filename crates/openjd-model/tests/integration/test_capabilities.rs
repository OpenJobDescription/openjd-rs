// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test_capabilities.py
//!
//! Tests capability name validation (amount and attribute) via the regex patterns
//! and reserved scope checking used in host requirements validation.

use openjd_model::decode_job_template;
use openjd_model::CallerLimits;

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn job_with_amount(name: &str) -> serde_json::Value {
    yaml_val(&format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{
            "name": "S",
            "hostRequirements": {{
                "amounts": [{{"name": "{name}", "min": 1}}]
            }},
            "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}
        }}]
    }}"#
    ))
}

fn job_with_attr(name: &str, value: &str) -> serde_json::Value {
    yaml_val(&format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{
            "name": "S",
            "hostRequirements": {{
                "attributes": [{{"name": "{name}", "anyOf": ["{value}"]}}]
            }},
            "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}
        }}]
    }}"#
    ))
}

fn amount_ok(name: &str) {
    decode_job_template(job_with_amount(name), None, &CallerLimits::default()).unwrap();
}

fn amount_err(name: &str) {
    let err = decode_job_template(job_with_amount(name), None, &CallerLimits::default())
        .expect_err(&format!("expected error for amount name: {name}"));
    let msg = err.to_string();
    assert!(
        msg.contains("amounts[0]") || msg.contains("amounts"),
        "Expected amounts error path for {name}, got: {msg}"
    );
}

fn attr_ok(name: &str, value: &str) {
    decode_job_template(job_with_attr(name, value), None, &CallerLimits::default()).unwrap();
}

fn attr_err(name: &str) {
    let err = decode_job_template(
        job_with_attr(name, "somevalue"),
        None,
        &CallerLimits::default(),
    )
    .expect_err(&format!("expected error for attr name: {name}"));
    let msg = err.to_string();
    assert!(
        msg.contains("attributes[0]") || msg.contains("attributes"),
        "Expected attributes error path for {name}, got: {msg}"
    );
}

// ══════════════════════════════════════════════════════════════
// Amount capability name — success cases
// ══════════════════════════════════════════════════════════════

#[test]
fn amount_builtin_worker_vcpu() {
    amount_ok("amount.worker.vcpu");
}
#[test]
fn amount_builtin_memory() {
    amount_ok("amount.worker.memory");
}
#[test]
fn amount_builtin_gpu() {
    amount_ok("amount.worker.gpu");
}
#[test]
fn amount_builtin_gpu_memory() {
    amount_ok("amount.worker.gpu.memory");
}
#[test]
fn amount_builtin_disk_scratch() {
    amount_ok("amount.worker.disk.scratch");
}
#[test]
fn amount_customer_defined() {
    amount_ok("amount.custom");
}
#[test]
fn amount_vendor_defined() {
    amount_ok("vendor:amount.custom");
}
#[test]
fn amount_caps() {
    amount_ok("AMOUNT.WORKER.VCPU");
}
#[test]
fn amount_caps_vendor() {
    amount_ok("VENDOR:AMOUNT.CUSTOM");
}
#[test]
fn amount_vendor_starts_underscore() {
    amount_ok("_az09_:amount.custom");
}
#[test]
fn amount_vendor_starts_letter() {
    amount_ok("aaz09_:amount.custom");
}
#[test]
fn amount_segment_starts_underscore() {
    amount_ok("amount._az09_");
}
#[test]
fn amount_segment_starts_letter() {
    amount_ok("amount.aaz09_");
}
#[test]
fn amount_second_segment_starts_underscore() {
    amount_ok("amount.segment._az09_");
}
#[test]
fn amount_second_segment_starts_letter() {
    amount_ok("amount.segment.aaz09_");
}

// ══════════════════════════════════════════════════════════════
// Amount capability name — error cases
// ══════════════════════════════════════════════════════════════

#[test]
fn amount_wrong_prefix() {
    amount_err("attr.worker.foo");
}
#[test]
fn amount_reserved_worker_scope() {
    amount_err("amount.worker.notreserved");
}
#[test]
fn amount_reserved_job_scope() {
    amount_err("amount.job.notreserved");
}
#[test]
fn amount_reserved_step_scope() {
    amount_err("amount.step.notreserved");
}
#[test]
fn amount_reserved_task_scope() {
    amount_err("amount.task.notreserved");
}
#[test]
fn amount_bad_prefix() {
    amount_err("foo.custom");
}
#[test]
fn amount_vendor_start_digit() {
    amount_err("0:amount.custom");
}
#[test]
fn amount_vendor_start_dot() {
    amount_err(".:amount.custom");
}
#[test]
fn amount_vendor_contains_dot() {
    amount_err("v.:amount.custom");
}
#[test]
fn amount_name_start_digit() {
    amount_err("amount.0");
}
#[test]
fn amount_name_start_dot() {
    amount_err("amount..");
}
#[test]
fn amount_name_contains_space() {
    amount_err("amount.v ");
}
#[test]
fn amount_ends_in_newline() {
    amount_err("amount.worker.vcpu\n");
}

// ══════════════════════════════════════════════════════════════
// Attribute capability name — success cases
// ══════════════════════════════════════════════════════════════

#[test]
fn attr_builtin_os_family() {
    attr_ok("attr.worker.os.family", "linux");
}
#[test]
fn attr_builtin_cpu_arch() {
    attr_ok("attr.worker.cpu.arch", "x86_64");
}
#[test]
fn attr_customer_defined() {
    attr_ok("attr.custom", "somevalue");
}
#[test]
fn attr_vendor_defined() {
    attr_ok("vendor:attr.custom", "somevalue");
}
#[test]
fn attr_caps() {
    attr_ok("ATTR.WORKER.OS.FAMILY", "linux");
}
#[test]
fn attr_caps_vendor() {
    attr_ok("VENDOR:ATTR.CUSTOM", "somevalue");
}
#[test]
fn attr_vendor_starts_underscore() {
    attr_ok("_az09_:attr.custom", "somevalue");
}
#[test]
fn attr_vendor_starts_letter() {
    attr_ok("aaz09_:attr.custom", "somevalue");
}
#[test]
fn attr_segment_starts_underscore() {
    attr_ok("attr._az09_", "somevalue");
}
#[test]
fn attr_segment_starts_letter() {
    attr_ok("attr.aaz09_", "somevalue");
}
#[test]
fn attr_second_segment_starts_underscore() {
    attr_ok("attr.segment._az09_", "somevalue");
}
#[test]
fn attr_second_segment_starts_letter() {
    attr_ok("attr.segment.aaz09_", "somevalue");
}

// ══════════════════════════════════════════════════════════════
// Attribute capability name — error cases
// ══════════════════════════════════════════════════════════════

#[test]
fn attr_wrong_prefix() {
    attr_err("amount.worker.foo");
}
#[test]
fn attr_reserved_worker_scope() {
    attr_err("attr.worker.notreserved");
}
#[test]
fn attr_reserved_job_scope() {
    attr_err("attr.job.notreserved");
}
#[test]
fn attr_reserved_step_scope() {
    attr_err("attr.step.notreserved");
}
#[test]
fn attr_reserved_task_scope() {
    attr_err("attr.task.notreserved");
}
#[test]
fn attr_bad_prefix() {
    attr_err("foo.custom");
}
#[test]
fn attr_vendor_start_digit() {
    attr_err("0:attr.custom");
}
#[test]
fn attr_vendor_start_dot() {
    attr_err(".:attr.custom");
}
#[test]
fn attr_vendor_contains_dot() {
    attr_err("v.:attr.custom");
}
#[test]
fn attr_name_start_digit() {
    attr_err("attr.0");
}
#[test]
fn attr_name_start_dot() {
    attr_err("attr..");
}
#[test]
fn attr_name_contains_space() {
    attr_err("attr.v ");
}
#[test]
fn attr_ends_in_newline() {
    attr_err("attr.worker.os.family\n");
}

// ══════════════════════════════════════════════════════════════
// Standard attribute value validation
// ══════════════════════════════════════════════════════════════

#[test]
fn attr_os_family_invalid_value() {
    let err = decode_job_template(
        job_with_attr("attr.worker.os.family", "invalid"),
        None,
        &CallerLimits::default(),
    )
    .expect_err("invalid os.family value should be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("attr.worker.os.family"),
        "Expected os.family error, got: {msg}"
    );
}

#[test]
fn attr_cpu_arch_invalid_value() {
    let err = decode_job_template(
        job_with_attr("attr.worker.cpu.arch", "invalid"),
        None,
        &CallerLimits::default(),
    )
    .expect_err("invalid cpu.arch value should be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("attr.worker.cpu.arch"),
        "Expected cpu.arch error, got: {msg}"
    );
}
