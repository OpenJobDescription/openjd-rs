// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests for `CallerLimits` — the host-configurable caps that protect
//! the WASM instance from over-large templates and over-broad fan-out.
//!
//! Mirrors [`openjd_model::CallerLimits`] field-for-field and plumbs
//! the limits through the decode and create-job paths. Resolves
//! finding F4 from the security review: without these knobs a
//! template author can submit arbitrarily large input and the host
//! has no way to cap memory before parsing begins.
//!
//! On the JS side `CallerLimits` is exposed as a plain structural
//! type (`{ maxTemplateSize: N, ... }`) rather than a wasm_bindgen
//! class, so callers can reuse the same options object across calls
//! without fighting ownership. These rlib tests exercise the Rust
//! struct directly.

use openjd_for_js::model::{
    create_job_with_map, decode_job_template_from_object, decode_job_template_str, JsCallerLimits,
    JsPathParameterOptions,
};

// ── Constructor + defaults ──────────────────────────────────────────

/// `JsCallerLimits::default()` (and the shim `::new()`) produces a
/// value equivalent to `openjd_model::CallerLimits::default()` —
/// all fields `None`.
#[test]
fn default_is_all_none() {
    let limits = JsCallerLimits::default();
    assert!(limits.max_step_count.is_none());
    assert!(limits.max_env_count.is_none());
    assert!(limits.max_task_count.is_none());
    assert!(limits.max_step_script_size.is_none());
    assert!(limits.max_environment_size.is_none());
    assert!(limits.max_template_size.is_none());

    let via_new = JsCallerLimits::new();
    assert!(via_new.max_step_count.is_none());
}

/// Struct fields are public. Callers set them directly; no setter
/// plumbing. Verifies round-trip.
#[test]
fn fields_round_trip() {
    let mut limits = JsCallerLimits::new();

    limits.max_step_count = Some(100);
    limits.max_env_count = Some(50);
    limits.max_task_count = Some(1_000_000);
    limits.max_step_script_size = Some(10_000);
    limits.max_environment_size = Some(5_000);
    limits.max_template_size = Some(1_000_000);

    assert_eq!(limits.max_step_count, Some(100));
    assert_eq!(limits.max_env_count, Some(50));
    assert_eq!(limits.max_task_count, Some(1_000_000));
    assert_eq!(limits.max_step_script_size, Some(10_000));
    assert_eq!(limits.max_environment_size, Some(5_000));
    assert_eq!(limits.max_template_size, Some(1_000_000));

    // Explicit None-clearing.
    limits.max_template_size = None;
    assert!(limits.max_template_size.is_none());
}

/// The internal `as_rust()` bridge must produce a
/// `openjd_model::CallerLimits` that carries the wrapper's current
/// field values unchanged.
#[test]
fn as_rust_propagates_all_fields() {
    let mut limits = JsCallerLimits::new();
    limits.max_step_count = Some(7);
    limits.max_env_count = Some(8);
    limits.max_task_count = Some(9);
    limits.max_step_script_size = Some(10);
    limits.max_environment_size = Some(11);
    limits.max_template_size = Some(12);

    let rust = limits.as_rust();

    assert_eq!(rust.max_step_count, Some(7));
    assert_eq!(rust.max_env_count, Some(8));
    assert_eq!(rust.max_task_count, Some(9));
    assert_eq!(rust.max_step_script_size, Some(10));
    assert_eq!(rust.max_environment_size, Some(11));
    assert_eq!(rust.max_template_size, Some(12));
}

/// Serde rename_all = "camelCase" is wired up, so JS-idiom camelCase
/// keys in an options literal (`{ maxTemplateSize: ... }`) land in
/// the Rust snake_case fields. Verified via a raw JSON
/// deserialization, which is the same path `serde_wasm_bindgen`
/// takes under the hood.
#[test]
fn deserializes_camel_case_keys() {
    let json = r#"{
        "maxStepCount": 1,
        "maxEnvCount": 2,
        "maxTaskCount": 3,
        "maxStepScriptSize": 4,
        "maxEnvironmentSize": 5,
        "maxTemplateSize": 6
    }"#;
    let limits: JsCallerLimits = serde_json::from_str(json).expect("camelCase keys deserialize");
    assert_eq!(limits.max_step_count, Some(1));
    assert_eq!(limits.max_env_count, Some(2));
    assert_eq!(limits.max_task_count, Some(3));
    assert_eq!(limits.max_step_script_size, Some(4));
    assert_eq!(limits.max_environment_size, Some(5));
    assert_eq!(limits.max_template_size, Some(6));
}

/// `deny_unknown_fields` is set on the struct. Via `serde_json` the
/// typo-protection is active and an unknown key fails.
///
/// Note: `serde_wasm_bindgen` does NOT enforce `deny_unknown_fields`
/// when deserializing from a JS object (it treats the object as a
/// map), so JS callers get only opt-in restrictions — a typo in a
/// field name silently skips that cap rather than erroring. This
/// rlib test documents the intent; a paired JS-side guard would
/// require manual key enumeration before handing off to serde.
#[test]
fn rejects_unknown_fields_via_serde_json() {
    let json = r#"{"maxStepCount": 1, "bogus": 99}"#;
    let err = serde_json::from_str::<JsCallerLimits>(json)
        .expect_err("unknown field must be rejected by serde_json");
    assert!(
        err.to_string().contains("bogus") || err.to_string().contains("unknown"),
        "expected unknown-field error, got: {err}"
    );
}

// ── F4 regression guards: limits enforced end-to-end ────────────────

const MINIMAL_TEMPLATE: &str = r#"{
    "specificationVersion": "jobtemplate-2023-09",
    "name": "T",
    "steps": [
        {"name": "S1", "script": {"actions": {"onRun": {"command": "x"}}}},
        {"name": "S2", "script": {"actions": {"onRun": {"command": "x"}}}}
    ]
}"#;

/// `max_template_size` is enforced by `document_string_to_object`
/// before parsing begins. A template whose byte length exceeds the
/// cap must be rejected with a size error, regardless of whether the
/// template itself is valid.
#[test]
fn decode_str_rejects_template_exceeding_max_template_size() {
    let mut limits = JsCallerLimits::new();
    // Template is ~200 bytes; cap at 50.
    limits.max_template_size = Some(50);

    let err = match decode_job_template_str(MINIMAL_TEMPLATE, None, Some(&limits), None) {
        Ok(_) => panic!("must reject oversized template"),
        Err(e) => e,
    };
    // Message comes from document_string_to_object — the wording
    // "exceeds caller limit" is stable and unique to this check.
    assert!(
        err.contains("exceeds caller limit"),
        "expected max_template_size error, got: {err}"
    );
}

/// With no caller-supplied cap (default), the same template decodes.
/// Confirms the limit is actually opt-in rather than always-on.
#[test]
fn decode_str_default_limits_does_not_reject_normal_template() {
    decode_job_template_str(MINIMAL_TEMPLATE, None, None, None)
        .expect("default limits must accept");
    decode_job_template_str(MINIMAL_TEMPLATE, None, Some(&JsCallerLimits::new()), None)
        .expect("explicit empty limits must accept");
}

/// `max_step_count` is enforced during template validation. A
/// template with more steps than the cap must be rejected.
#[test]
fn decode_str_rejects_template_exceeding_max_step_count() {
    let mut limits = JsCallerLimits::new();
    limits.max_step_count = Some(1);

    let err = match decode_job_template_str(MINIMAL_TEMPLATE, None, Some(&limits), None) {
        Ok(_) => panic!("must reject too-many-steps"),
        Err(e) => e,
    };
    // Don't pin the exact wording — openjd-model owns it. Just confirm
    // the rejection happened at validation, not parsing.
    assert!(
        err.contains("step") || err.contains("Step"),
        "expected step-count error, got: {err}"
    );
}

/// `decode_job_template_from_object` must apply the same limits as
/// the string-input path. The `max_template_size` check is skipped
/// here (no string to measure) but step-count/env-count/etc. still
/// apply.
#[test]
fn decode_from_object_respects_max_step_count() {
    let v: serde_json::Value = serde_json::from_str(MINIMAL_TEMPLATE).unwrap();
    let mut limits = JsCallerLimits::new();
    limits.max_step_count = Some(1);

    let err = match decode_job_template_from_object(v, Some(&limits), None) {
        Ok(_) => panic!("must reject too-many-steps"),
        Err(e) => e,
    };
    assert!(
        err.contains("step") || err.contains("Step"),
        "expected step-count error, got: {err}"
    );
}

/// `max_task_count` is enforced after parameter space expansion
/// during `create_job`. Confirm it threads through the createJob
/// path.
#[test]
fn create_job_rejects_template_exceeding_max_task_count() {
    // Template with a single step that has a 10-task parameter space.
    let template_json = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "T",
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "F", "type": "INT", "range": "1-10"}
                ]
            },
            "script": {"actions": {"onRun": {"command": "x"}}}
        }]
    }"#;
    let template = decode_job_template_str(template_json, None, None, None)
        .expect("template decodes with default limits");

    let path_opts = JsPathParameterOptions::new("/tmpl", "/cwd");
    let mut limits = JsCallerLimits::new();
    limits.max_task_count = Some(5);
    let params = std::collections::HashMap::<String, String>::new();

    let err = match create_job_with_map(&template, params, &path_opts, Some(&limits)) {
        Ok(_) => panic!("must reject when task count exceeds limit"),
        Err(e) => e,
    };
    assert!(
        err.contains("task") || err.contains("Task"),
        "expected task-count error, got: {err}"
    );
}

/// Under the default (no cap), the same 10-task template creates a
/// job successfully. Confirms the cap is opt-in.
#[test]
fn create_job_default_limits_accepts_ten_tasks() {
    let template_json = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "T",
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "F", "type": "INT", "range": "1-10"}
                ]
            },
            "script": {"actions": {"onRun": {"command": "x"}}}
        }]
    }"#;
    let template =
        decode_job_template_str(template_json, None, None, None).expect("template decodes");
    let path_opts = JsPathParameterOptions::new("/tmpl", "/cwd");
    let params = std::collections::HashMap::<String, String>::new();

    create_job_with_map(&template, params.clone(), &path_opts, None)
        .expect("10 tasks ok under default");
    create_job_with_map(&template, params, &path_opts, Some(&JsCallerLimits::new()))
        .expect("10 tasks ok under explicit empty limits");
}

/// Document the boundary at which `max_template_size` kicks in.
/// Matches the `openjd_model::document_string_to_object` invariant:
/// rejection happens when `document.len() > max`, not `>=`.
#[test]
fn decode_str_accepts_template_exactly_at_max_template_size() {
    let mut limits = JsCallerLimits::new();
    limits.max_template_size = Some(MINIMAL_TEMPLATE.len());
    // Boundary: `len == max` must succeed.
    decode_job_template_str(MINIMAL_TEMPLATE, None, Some(&limits), None)
        .expect("template exactly at the size cap must be accepted");

    // One byte under the cap: also success.
    let mut limits = JsCallerLimits::new();
    limits.max_template_size = Some(MINIMAL_TEMPLATE.len() + 1);
    decode_job_template_str(MINIMAL_TEMPLATE, None, Some(&limits), None)
        .expect("template under the cap must be accepted");
}
