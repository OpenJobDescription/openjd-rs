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

// ══════════════════════════════════════════════════════════════
// Format-string capability names (§3.3.1.1 / §3.3.2.1)
// ══════════════════════════════════════════════════════════════
//
// `name` is `@fmtstring`. A name containing expressions is resolved at job
// creation, and the §3.3.1.1 / §3.3.2.1 constraints and the §3.3 uniqueness
// constraints apply to the resolved name.

/// A template with two `STRING` parameters, `A` and `B`, and one step whose
/// `hostRequirements` is the given JSON object.
fn job_with_host_requirements(host_requirements: &str) -> serde_json::Value {
    yaml_val(&format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [
            {{"name": "A", "type": "STRING"}},
            {{"name": "B", "type": "STRING"}}
        ],
        "steps": [{{
            "name": "S",
            "hostRequirements": {host_requirements},
            "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}
        }}]
    }}"#
    ))
}

/// Decode `template` and create a job with `A` and `B` bound to `a` and `b`.
fn create_job_with(
    template: serde_json::Value,
    a: &str,
    b: &str,
) -> Result<openjd_model::job::Job, openjd_model::ModelError> {
    let jt = decode_job_template(template, None, &CallerLimits::default())?;
    let mut input = openjd_model::JobParameterInputValues::new();
    input.insert("A".into(), openjd_expr::ExprValue::String(a.into()));
    input.insert("B".into(), openjd_expr::ExprValue::String(b.into()));
    let dir = tempfile::TempDir::new().unwrap();
    let dir = dir.path().to_str().unwrap();
    let processed = openjd_model::preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: dir,
            current_working_dir: dir,
            allow_template_dir_walk_up: false,
            path_format: openjd_expr::path_mapping::PathFormat::host(),
            allow_uri_path_values: true,
        },
    )?;
    openjd_model::create_job(&jt, &processed, &jt.default_validation_context())
}

fn resolved_amount_names(job: &openjd_model::job::Job) -> Vec<String> {
    let hr = job.steps[0].host_requirements.as_ref().unwrap();
    hr.amounts
        .as_ref()
        .unwrap()
        .iter()
        .map(|a| a.name.clone())
        .collect()
}

fn resolved_attribute_names(job: &openjd_model::job::Job) -> Vec<String> {
    let hr = job.steps[0].host_requirements.as_ref().unwrap();
    hr.attributes
        .as_ref()
        .unwrap()
        .iter()
        .map(|a| a.name.clone())
        .collect()
}

/// The full message of a template-validation or job-creation error that
/// reports `message` at `path`.
fn full_error(path: &str, message: &str) -> String {
    format!("Model validation error: 1 validation error for JobTemplate\n{path}:\n\t{message}")
}

fn assert_create_err(
    result: Result<openjd_model::job::Job, openjd_model::ModelError>,
    path: &str,
    message: &str,
) {
    let msg = result
        .expect_err("expected job creation to fail")
        .to_string();
    assert_eq!(msg, full_error(path, message));
}
#[test]
fn format_string_names_pass_decode() {
    // Neither name is a capability name until it is resolved, so decode
    // must not apply the capability name checks to the raw text.
    for host_requirements in [
        r#"{"amounts": [{"name": "{{Param.A}}", "min": 1}]}"#,
        r#"{"attributes": [{"name": "{{Param.A}}", "anyOf": ["v"]}]}"#,
        r#"{"amounts": [{"name": "amount.custom.{{Param.A}}", "min": 1}]}"#,
        r#"{"attributes": [{"name": "attr.custom.{{Param.A}}", "anyOf": ["v"]}]}"#,
    ] {
        decode_job_template(
            job_with_host_requirements(host_requirements),
            None,
            &CallerLimits::default(),
        )
        .unwrap_or_else(|e| panic!("{host_requirements} should decode: {e}"));
    }
}

#[test]
fn format_string_name_rejects_undefined_parameter() {
    // The name's expressions are checked like any other format string.
    let err = decode_job_template(
        job_with_amount("{{Param.Undefined}}"),
        None,
        &CallerLimits::default(),
    )
    .expect_err("an undefined parameter in a capability name should be rejected");
    assert_eq!(
        err.to_string(),
        full_error(
            "steps[0] -> hostRequirements -> amounts[0] -> name",
            "Failed to parse interpolation expression at [0, 19]. Undefined variable: 'Param.Undefined'.\n  \
             Param.Undefined\n  \
             ~~~~~~^~~~~~~~~",
        )
    );
}

#[test]
fn format_string_names_with_identical_raw_text_pass_decode() {
    // Uniqueness is on the resolved names, which decode cannot know. Job
    // creation rejects these because both resolve to the same name.
    let template = job_with_host_requirements(
        r#"{"attributes": [
            {"name": "{{Param.A}}", "anyOf": ["v1"]},
            {"name": "{{Param.A}}", "anyOf": ["v2"]}
        ]}"#,
    );
    decode_job_template(template.clone(), None, &CallerLimits::default()).unwrap();
    assert_create_err(
        create_job_with(template, "attr.custom.x", "unused"),
        "steps[0] -> hostRequirements -> attributes[1]",
        "duplicate attribute name 'attr.custom.x'.",
    );
}

#[test]
fn format_string_name_rejects_symbols_unavailable_at_job_creation() {
    // The name is resolved at job creation, so it may only reference
    // values that stage knows. Task parameters are only known per task.
    let template = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{
            "name": "S",
            "parameterSpace": {"taskParameterDefinitions": [
                {"name": "F", "type": "STRING", "range": ["attr.custom.x"]}
            ]},
            "hostRequirements": {
                "attributes": [{"name": "{{Task.Param.F}}", "anyOf": ["v"]}]
            },
            "script": {"actions": {"onRun": {"command": "foo"}}}
        }]
    }"#,
    );
    let err = decode_job_template(template, None, &CallerLimits::default())
        .expect_err("Task.Param in a capability name should be rejected");
    assert_eq!(
        err.to_string(),
        full_error(
            "steps[0] -> hostRequirements -> attributes[0] -> name",
            "Failed to parse interpolation expression at [0, 16]. Undefined variable: 'Task.Param.F'.\n  \
             Task.Param.F\n  \
             ~~~~~~~~~~~^",
        )
    );
}

#[test]
fn literal_names_still_checked_at_decode() {
    // Gating the name checks on format strings must not skip them for
    // literal names.
    amount_err("attr.custom.x");
    attr_err("amount.custom.x");
    amount_err("amount.worker.custom");
    attr_err("attr.worker.custom");
    let long = format!("attr.custom.{}", "a".repeat(89));
    attr_err(&long);
}

#[test]
fn format_string_names_resolve() {
    let job = create_job_with(
        job_with_host_requirements(
            r#"{
            "amounts": [{"name": "{{Param.A}}", "min": 1}],
            "attributes": [{"name": "attr.custom.{{Param.B}}", "anyOf": ["v"]}]
        }"#,
        ),
        "amount.custom.licenses",
        "gpu_type",
    )
    .unwrap();
    assert_eq!(resolved_amount_names(&job), ["amount.custom.licenses"]);
    assert_eq!(resolved_attribute_names(&job), ["attr.custom.gpu_type"]);
}

#[test]
fn format_string_name_resolving_to_standard_capability_is_allowed() {
    // A standard name is exempt from the reserved-scope rule whether it is
    // written literally or produced by a format string.
    let job = create_job_with(
        job_with_host_requirements(
            r#"{
            "amounts": [{"name": "{{Param.A}}", "min": 2}],
            "attributes": [{"name": "{{Param.B}}", "anyOf": ["linux"]}]
        }"#,
        ),
        "amount.worker.vcpu",
        "attr.worker.os.family",
    )
    .unwrap();
    assert_eq!(resolved_amount_names(&job), ["amount.worker.vcpu"]);
    assert_eq!(resolved_attribute_names(&job), ["attr.worker.os.family"]);
}

#[test]
fn format_string_name_resolving_to_invalid_pattern_fails() {
    let amount = job_with_host_requirements(r#"{"amounts": [{"name": "{{Param.A}}", "min": 1}]}"#);
    assert_create_err(
        create_job_with(amount, "attr.custom.x", "unused"),
        "steps[0] -> hostRequirements -> amounts[0]",
        "name 'attr.custom.x' does not match capability name pattern.",
    );
    let attr =
        job_with_host_requirements(r#"{"attributes": [{"name": "{{Param.A}}", "anyOf": ["v"]}]}"#);
    assert_create_err(
        create_job_with(attr, "amount.custom.x", "unused"),
        "steps[0] -> hostRequirements -> attributes[0]",
        "name 'amount.custom.x' does not match capability name pattern.",
    );
}

#[test]
fn format_string_name_resolving_to_reserved_scope_fails() {
    let amount = job_with_host_requirements(r#"{"amounts": [{"name": "{{Param.A}}", "min": 1}]}"#);
    assert_create_err(
        create_job_with(amount, "amount.worker.licenses", "unused"),
        "steps[0] -> hostRequirements -> amounts[0]",
        "capability 'amount.worker.licenses' uses reserved scope 'worker'. Only spec-defined capabilities may use this scope.",
    );
    let attr =
        job_with_host_requirements(r#"{"attributes": [{"name": "{{Param.A}}", "anyOf": ["v"]}]}"#);
    assert_create_err(
        create_job_with(attr, "attr.job.custom", "unused"),
        "steps[0] -> hostRequirements -> attributes[0]",
        "capability 'attr.job.custom' uses reserved scope 'job'. Only spec-defined capabilities may use this scope.",
    );
}

#[test]
fn format_string_name_length_limit_applies_to_resolved_name() {
    let attr = || {
        job_with_host_requirements(r#"{"attributes": [{"name": "{{Param.A}}", "anyOf": ["v"]}]}"#)
    };
    let at_limit = format!("attr.custom.{}", "a".repeat(88));
    assert_eq!(at_limit.chars().count(), 100);
    create_job_with(attr(), &at_limit, "unused").unwrap();
    let over_limit = format!("attr.custom.{}", "a".repeat(89));
    assert_create_err(
        create_job_with(attr(), &over_limit, "unused"),
        "steps[0] -> hostRequirements -> attributes[0]",
        &format!("name '{over_limit}' exceeds 100 characters."),
    );
}

#[test]
fn format_string_name_resolving_to_standard_capability_checks_values() {
    let attr = job_with_host_requirements(
        r#"{"attributes": [{"name": "{{Param.A}}", "anyOf": ["plan9"]}]}"#,
    );
    assert_create_err(
        create_job_with(attr, "attr.worker.os.family", "unused"),
        "steps[0] -> hostRequirements -> attributes[0] -> anyOf[0]",
        "value 'plan9' is not valid for attr.worker.os.family.",
    );
}

#[test]
fn format_string_names_resolving_to_duplicates_fail() {
    let amounts = job_with_host_requirements(
        r#"{"amounts": [
            {"name": "{{Param.A}}", "min": 1},
            {"name": "{{Param.B}}", "min": 1}
        ]}"#,
    );
    assert_create_err(
        create_job_with(amounts, "amount.custom.x", "amount.custom.x"),
        "steps[0] -> hostRequirements -> amounts[1]",
        "duplicate amount name 'amount.custom.x'.",
    );
    // Uniqueness is case-insensitive (§3.3.2.1), and a format-string name
    // can collide with a literal one.
    let attrs = job_with_host_requirements(
        r#"{"attributes": [
            {"name": "{{Param.A}}", "anyOf": ["v1"]},
            {"name": "attr.custom.x", "anyOf": ["v2"]}
        ]}"#,
    );
    assert_create_err(
        create_job_with(attrs, "ATTR.CUSTOM.X", "unused"),
        "steps[0] -> hostRequirements -> attributes[1]",
        "duplicate attribute name 'attr.custom.x'.",
    );
}

#[test]
fn format_string_names_resolving_to_distinct_names_pass() {
    let job = create_job_with(
        job_with_host_requirements(
            r#"{"attributes": [
                {"name": "{{Param.A}}", "anyOf": ["v1"]},
                {"name": "{{Param.B}}", "anyOf": ["v2"]}
            ]}"#,
        ),
        "attr.custom.x",
        "attr.custom.y",
    )
    .unwrap();
    assert_eq!(
        resolved_attribute_names(&job),
        ["attr.custom.x", "attr.custom.y"]
    );
}

// ══════════════════════════════════════════════════════════════
// Fully static capability names are checked at template validation
// ══════════════════════════════════════════════════════════════
//
// A name whose value is known at template validation — a literal, or an
// expression that depends only on literals and `let` bindings — gets the
// checks job creation would run on it, at template validation.

/// An `EXPR` template with two `STRING` parameters, `A` and `B`, optional
/// step `let` bindings, and one step whose `hostRequirements` is the given
/// JSON object.
fn expr_job_with_host_requirements(
    let_bindings: &[&str],
    host_requirements: &str,
) -> serde_json::Value {
    let let_json = if let_bindings.is_empty() {
        String::new()
    } else {
        format!("\"let\": {},", serde_json::to_string(let_bindings).unwrap())
    };
    yaml_val(&format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [
            {{"name": "A", "type": "STRING"}},
            {{"name": "B", "type": "STRING"}}
        ],
        "steps": [{{
            "name": "S",
            {let_json}
            "hostRequirements": {host_requirements},
            "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}
        }}]
    }}"#
    ))
}

fn decode_expr(let_bindings: &[&str], host_requirements: &str) -> Result<(), String> {
    let supported = ["EXPR"];
    decode_job_template(
        expr_job_with_host_requirements(let_bindings, host_requirements),
        Some(&supported),
        &CallerLimits::default(),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

fn assert_decode_err(let_bindings: &[&str], host_requirements: &str, path: &str, message: &str) {
    let msg = decode_expr(let_bindings, host_requirements)
        .expect_err("expected template validation to fail");
    assert_eq!(msg, full_error(path, message));
}
#[test]
fn static_name_reserved_scope_fails_decode() {
    assert_decode_err(
        &[],
        r#"{"attributes": [{"name": "{{ 'attr.worker.custom' }}", "anyOf": ["v"]}]}"#,
        "steps[0] -> hostRequirements -> attributes[0] -> name",
        "capability 'attr.worker.custom' uses reserved scope 'worker'. Only spec-defined capabilities may use this scope.",
    );
    assert_decode_err(
        &[],
        r#"{"amounts": [{"name": "{{ 'amount.task.custom' }}", "min": 1}]}"#,
        "steps[0] -> hostRequirements -> amounts[0] -> name",
        "capability 'amount.task.custom' uses reserved scope 'task'. Only spec-defined capabilities may use this scope.",
    );
}

#[test]
fn static_partial_name_bad_pattern_fails_decode() {
    assert_decode_err(
        &[],
        r#"{"amounts": [{"name": "amount.custom.{{ 'bad name' }}", "min": 1}]}"#,
        "steps[0] -> hostRequirements -> amounts[0] -> name",
        "name 'amount.custom.bad name' does not match capability name pattern.",
    );
}

#[test]
fn static_name_from_let_binding_fails_decode() {
    assert_decode_err(
        &["cap = 'attr.job.custom'"],
        r#"{"attributes": [{"name": "{{ cap }}", "anyOf": ["v"]}]}"#,
        "steps[0] -> hostRequirements -> attributes[0] -> name",
        "capability 'attr.job.custom' uses reserved scope 'job'. Only spec-defined capabilities may use this scope.",
    );
}

#[test]
fn static_name_too_long_fails_decode() {
    let name = format!("attr.custom.{}", "a".repeat(89));
    assert_decode_err(
        &[],
        &format!(r#"{{"attributes": [{{"name": "{{{{ '{name}' }}}}", "anyOf": ["v"]}}]}}"#),
        "steps[0] -> hostRequirements -> attributes[0] -> name",
        &format!("name '{name}' exceeds 100 characters."),
    );
}

#[test]
fn static_empty_name_fails_decode() {
    assert_decode_err(
        &[],
        r#"{"attributes": [{"name": "{{ '' }}", "anyOf": ["v"]}]}"#,
        "steps[0] -> hostRequirements -> attributes[0] -> name",
        "name '' does not match capability name pattern.",
    );
}

#[test]
fn static_name_duplicating_literal_fails_decode() {
    assert_decode_err(
        &[],
        r#"{"attributes": [
            {"name": "{{ 'ATTR.CUSTOM.X' }}", "anyOf": ["v1"]},
            {"name": "attr.custom.x", "anyOf": ["v2"]}
        ]}"#,
        "steps[0] -> hostRequirements -> attributes[1]",
        "duplicate attribute name 'attr.custom.x'.",
    );
    assert_decode_err(
        &[],
        r#"{"amounts": [
            {"name": "{{ 'amount.custom.x' }}", "min": 1},
            {"name": "{{ 'amount.custom.x' }}", "min": 1}
        ]}"#,
        "steps[0] -> hostRequirements -> amounts[1]",
        "duplicate amount name 'amount.custom.x'.",
    );
}

#[test]
fn literal_duplicate_names_reported_once() {
    // Structure reports duplicates between literal names. The static-name
    // check must not report the same pair again.
    let msg = decode_expr(
        &[],
        r#"{"attributes": [
            {"name": "attr.custom.x", "anyOf": ["v1"]},
            {"name": "attr.custom.x", "anyOf": ["v2"]}
        ]}"#,
    )
    .expect_err("literal duplicate names should be rejected");
    assert_eq!(
        msg,
        full_error(
            "steps[0] -> hostRequirements -> attributes[1]",
            "duplicate attribute name 'attr.custom.x'.",
        )
    );
}

#[test]
fn static_standard_name_checks_literal_values_at_decode() {
    assert_decode_err(
        &[],
        r#"{"attributes": [{"name": "{{ 'attr.worker.os.family' }}", "anyOf": ["plan9"]}]}"#,
        "steps[0] -> hostRequirements -> attributes[0] -> anyOf[0]",
        "value 'plan9' is not valid for attr.worker.os.family.",
    );
    decode_expr(
        &[],
        r#"{"attributes": [{"name": "{{ 'attr.worker.os.family' }}", "anyOf": ["linux"]}]}"#,
    )
    .unwrap();
}

#[test]
fn static_standard_name_checks_single_valued_all_of_at_decode() {
    assert_decode_err(
        &[],
        r#"{"attributes": [{"name": "{{ 'attr.worker.cpu.arch' }}", "allOf": ["x86_64", "arm64"]}]}"#,
        "steps[0] -> hostRequirements -> attributes[0] -> allOf",
        "single-valued attribute cannot have more than 1 element.",
    );
}

#[test]
fn static_valid_names_pass_decode() {
    decode_expr(
        &["kind = 'licenses'"],
        r#"{
            "amounts": [{"name": "amount.custom.{{ kind }}", "min": 1}],
            "attributes": [
                {"name": "{{ 'attr.worker.os.family' }}", "anyOf": ["linux"]},
                {"name": "{{ 'attr.custom.' + 'gpu' }}", "anyOf": ["v"]}
            ]
        }"#,
    )
    .unwrap();
}

#[test]
fn parameter_dependent_names_are_deferred_to_job_creation() {
    // A name that depends on a job parameter is not known at template
    // validation, so neither its own checks nor uniqueness apply yet.
    decode_expr(
        &[],
        r#"{"attributes": [
            {"name": "{{ Param.A }}", "anyOf": ["plan9"]},
            {"name": "{{ Param.A }}", "anyOf": ["v"]},
            {"name": "attr.worker.{{ Param.B }}", "anyOf": ["v"]}
        ]}"#,
    )
    .unwrap();
}

#[test]
fn partially_static_name_over_length_bound_fails_decode() {
    // The literal part alone is longer than 100 characters, so every
    // resolution is too long.
    let prefix = format!("attr.custom.{}", "a".repeat(89));
    assert_decode_err(
        &[],
        &format!(r#"{{"attributes": [{{"name": "{prefix}{{{{ Param.A }}}}", "anyOf": ["v"]}}]}}"#),
        "steps[0] -> hostRequirements -> attributes[0] -> name",
        "resolves to at least 101 characters, exceeding the maximum of 100.",
    );
}
