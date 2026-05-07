// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests for the `PathParameterOptions` JS-binding wrapper.
//!
//! These tests exercise the wrapper from the Rust rlib target. They
//! verify the JS API surface by constructing the wrapper via the same
//! entry points `#[wasm_bindgen]` exposes to JS, then calling the
//! `as_rust()` bridge to confirm the options propagate correctly to
//! [`openjd_model::PathParameterOptions`].
//!
//! Rationale: the wasm32 build can't run on host, but the rlib build
//! can — and the wrapper's logic is the same on both. The `#[wasm_bindgen]`
//! attribute only affects the glue code, not the method bodies, so
//! testing via the rlib gives full coverage of the behavior we care
//! about. Complementary smoke tests in `js-tests/` exercise the JS↔WASM
//! boundary itself.

use openjd_for_js::expr::JsPathFormat;
use openjd_for_js::model::JsPathParameterOptions;

/// The JS constructor `new PathParameterOptions(jobTemplateDir, currentWorkingDir)`
/// must mirror `openjd_model::PathParameterOptions::new`: host path
/// format (Posix on wasm32), walk-up disabled, URI path values
/// disabled.
#[test]
fn constructor_mirrors_rust_defaults() {
    let opts = JsPathParameterOptions::new("/tmpl", "/cwd");

    assert_eq!(opts.job_template_dir(), "/tmpl");
    assert_eq!(opts.current_working_dir(), "/cwd");
    // On wasm32-unknown-unknown, PathFormat::host() returns Posix.
    // We hardcode Posix in the wrapper to make that deterministic
    // regardless of the host build target running this test.
    assert_eq!(opts.path_format(), JsPathFormat::Posix);
    assert!(!opts.allow_template_dir_walk_up());
    assert!(!opts.allow_uri_path_values());
}

/// Setters must update the stored value.
#[test]
fn setters_update_fields() {
    let mut opts = JsPathParameterOptions::new("/tmpl", "/cwd");

    opts.set_job_template_dir("/other".to_string());
    assert_eq!(opts.job_template_dir(), "/other");

    opts.set_current_working_dir("/wd".to_string());
    assert_eq!(opts.current_working_dir(), "/wd");

    opts.set_path_format(JsPathFormat::Windows);
    assert_eq!(opts.path_format(), JsPathFormat::Windows);

    opts.set_allow_template_dir_walk_up(true);
    assert!(opts.allow_template_dir_walk_up());

    opts.set_allow_uri_path_values(true);
    assert!(opts.allow_uri_path_values());
}

/// The internal `as_rust()` bridge must produce an
/// `openjd_model::PathParameterOptions` that carries the wrapper's
/// current field values unchanged.
#[test]
fn as_rust_propagates_all_fields() {
    let mut opts = JsPathParameterOptions::new("/tmpl", "/cwd");
    opts.set_path_format(JsPathFormat::Windows);
    opts.set_allow_template_dir_walk_up(true);
    opts.set_allow_uri_path_values(true);

    let rust = opts.as_rust();

    assert_eq!(rust.job_template_dir, "/tmpl");
    assert_eq!(rust.current_working_dir, "/cwd");
    assert_eq!(rust.path_format, openjd_expr::PathFormat::Windows);
    assert!(rust.allow_template_dir_walk_up);
    assert!(rust.allow_uri_path_values);
}

/// `JsPathFormat::Uri` must map to `openjd_expr::PathFormat::Uri`. This
/// variant closes the parity gap where the JS enum was missing `Uri`.
#[test]
fn path_format_uri_variant_round_trips() {
    let mut opts = JsPathParameterOptions::new("/tmpl", "/cwd");
    opts.set_path_format(JsPathFormat::Uri);
    assert_eq!(opts.path_format(), JsPathFormat::Uri);
    assert_eq!(opts.as_rust().path_format, openjd_expr::PathFormat::Uri);
}

/// Exercising `create_job` through the wrapper with a template that
/// has an absolute PATH default must now fail by default (because
/// the default of `allow_template_dir_walk_up` is `false`). This is
/// the behavior the original JS bindings silently allowed. See
/// `reports/openjd-for-js-security-review.md` finding F1.
#[test]
fn create_job_rejects_absolute_path_default_by_default() {
    use openjd_for_js::model::decode_job_template_str;

    let template_json = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "T",
        "parameterDefinitions": [
            {"name": "Out", "type": "PATH", "default": "/etc/passwd"}
        ],
        "steps": [
            {"name": "S", "script": {"actions": {"onRun": {"command": "x"}}}}
        ]
    }"#;

    let template = decode_job_template_str(template_json, None).expect("template decodes");
    let opts = JsPathParameterOptions::new("/tmpl", "/cwd");

    // Construct an empty params value as JsValue. On non-wasm we can't
    // build JsValue, so this test delegates to the internal helper that
    // accepts a `HashMap<String, String>` directly — see create_job_with_map.
    let params = std::collections::HashMap::<String, String>::new();
    let err = match openjd_for_js::model::create_job_with_map(&template, params, &opts) {
        Ok(_) => panic!("expected error, got a job"),
        Err(e) => e,
    };

    // Message comes from openjd_model::preprocess_job_parameters — the
    // assertion is a substring match on the spec-defined wording so
    // this test is stable against unrelated error-message edits.
    assert!(
        err.contains("absolute path") && err.contains("Out"),
        "unexpected error message: {err}"
    );
}

/// With `allow_template_dir_walk_up` enabled, the same template must
/// be accepted. Confirms the flag is plumbed correctly and mirrors the
/// Rust API's behavior.
#[test]
fn create_job_accepts_absolute_path_default_with_walk_up() {
    use openjd_for_js::model::{create_job_with_map, decode_job_template_str};

    let template_json = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "T",
        "parameterDefinitions": [
            {"name": "Out", "type": "PATH", "default": "/etc/passwd"}
        ],
        "steps": [
            {"name": "S", "script": {"actions": {"onRun": {"command": "x"}}}}
        ]
    }"#;

    let template = decode_job_template_str(template_json, None).expect("template decodes");
    let mut opts = JsPathParameterOptions::new("/tmpl", "/cwd");
    opts.set_allow_template_dir_walk_up(true);

    let params = std::collections::HashMap::<String, String>::new();
    create_job_with_map(&template, params, &opts).expect("job created");
}

/// URI values in PATH defaults are rejected when `allow_uri_path_values`
/// is false (the default) even with `EXPR` enabled.
#[test]
fn create_job_rejects_uri_path_default_by_default() {
    use openjd_for_js::model::{create_job_with_map, decode_job_template_str};

    let template_json = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "T",
        "extensions": ["EXPR"],
        "parameterDefinitions": [
            {"name": "Out", "type": "PATH", "default": "s3://bucket/key"}
        ],
        "steps": [
            {"name": "S", "script": {"actions": {"onRun": {"command": "x"}}}}
        ]
    }"#;

    let template = decode_job_template_str(template_json, None).expect("template decodes");
    let opts = JsPathParameterOptions::new("/tmpl", "/cwd");

    let params = std::collections::HashMap::<String, String>::new();
    let err = match create_job_with_map(&template, params, &opts) {
        Ok(_) => panic!("expected error, got a job"),
        Err(e) => e,
    };

    assert!(
        err.contains("URI path values are not permitted"),
        "unexpected error message: {err}"
    );
}

/// URI values are accepted when `allow_uri_path_values` is enabled
/// and EXPR is enabled on the template.
#[test]
fn create_job_accepts_uri_path_default_with_flag() {
    use openjd_for_js::model::{create_job_with_map, decode_job_template_str};

    let template_json = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "T",
        "extensions": ["EXPR"],
        "parameterDefinitions": [
            {"name": "Out", "type": "PATH", "default": "s3://bucket/key"}
        ],
        "steps": [
            {"name": "S", "script": {"actions": {"onRun": {"command": "x"}}}}
        ]
    }"#;

    let template = decode_job_template_str(template_json, None).expect("template decodes");
    let mut opts = JsPathParameterOptions::new("/tmpl", "/cwd");
    opts.set_allow_uri_path_values(true);

    let params = std::collections::HashMap::<String, String>::new();
    create_job_with_map(&template, params, &opts).expect("job created");
}

/// `preprocess_job_parameters` must apply the same options as
/// `create_job`. Repeating F1 coverage via this entry point
/// confirms both call sites were updated.
#[test]
fn preprocess_rejects_absolute_path_default_by_default() {
    use openjd_for_js::model::{decode_job_template_str, preprocess_job_parameters_with_map};

    let template_json = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "T",
        "parameterDefinitions": [
            {"name": "Out", "type": "PATH", "default": "/etc/passwd"}
        ],
        "steps": [
            {"name": "S", "script": {"actions": {"onRun": {"command": "x"}}}}
        ]
    }"#;

    let template = decode_job_template_str(template_json, None).expect("template decodes");
    let opts = JsPathParameterOptions::new("/tmpl", "/cwd");

    let params = std::collections::HashMap::<String, String>::new();
    let err = match preprocess_job_parameters_with_map(&template, params, &opts) {
        Ok(_) => panic!("expected error, got a result"),
        Err(e) => e,
    };

    assert!(
        err.contains("absolute path") && err.contains("Out"),
        "unexpected: {err}"
    );
}

/// Windows path format: a Posix-rooted default like `/posix/path`
/// normalizes outside of `C:\tmpl`, so with walk-up disabled the
/// default protection rejects it. This confirms `pathFormat`
/// genuinely controls path interpretation end-to-end — a regression
/// that silently treated the path as Posix would have produced a
/// different error message ("absolute path" vs. "outside of the
/// template directory").
#[test]
fn windows_path_format_rejects_escaping_default() {
    use openjd_for_js::model::{create_job_with_map, decode_job_template_str};

    let template_json = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "T",
        "parameterDefinitions": [
            {"name": "Out", "type": "PATH", "default": "/posix/path"}
        ],
        "steps": [
            {"name": "S", "script": {"actions": {"onRun": {"command": "x"}}}}
        ]
    }"#;

    let template = decode_job_template_str(template_json, None).expect("template decodes");
    let mut opts = JsPathParameterOptions::new(r"C:\tmpl", r"C:\cwd");
    opts.set_path_format(JsPathFormat::Windows);

    let params = std::collections::HashMap::<String, String>::new();
    let err = match create_job_with_map(&template, params, &opts) {
        Ok(_) => panic!("expected error, got a job"),
        Err(e) => e,
    };

    // The specific wording ("outside of the template directory")
    // confirms the Windows normalization actually ran — if pathFormat
    // had silently fallen back to Posix, we'd have seen a different
    // path-absolute-rejection wording or no error at all.
    assert!(
        err.contains("outside of the template directory"),
        "expected walk-up-escape rejection, got: {err}"
    );
}
