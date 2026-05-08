// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests for the `supportedExtensions` parameter on decode entry
//! points, plus the `getSupportedExtensions` helper.
//!
//! Resolves F8: every decode call previously used a JS-binding-local
//! hardcoded list of supported extensions. That list diverged from
//! the CLI's list and silently lagged `openjd-model`'s
//! `ModelExtension` authority. Hosts could not tighten the set —
//! e.g. a template-viewer extension had no way to disable EXPR.
//!
//! The new surface:
//!
//! * Every decode entry point takes an optional trailing
//!   `supportedExtensions: Option<Vec<String>>` argument.
//! * Omitted / `None` means "use the full default from
//!   `ModelExtension::ALL`" — which is now the one authoritative
//!   source in `openjd-model`.
//! * An empty slice `[]` disables every extension.
//! * An unknown extension name in the list is rejected by the
//!   underlying `decode_*` with a clear error.
//! * `get_supported_extensions()` returns the full default list for
//!   callers that want to start from it and subtract.

use openjd_for_js::model::{
    decode_environment_template_str, decode_job_template_from_object, decode_job_template_str,
    get_supported_extensions,
};

/// Minimal job template that uses the EXPR extension — a
/// `let:` binding in format-string syntax is only valid when EXPR
/// is active.
const JOB_TEMPLATE_USING_EXPR: &str = r#"{
    "specificationVersion": "jobtemplate-2023-09",
    "$schema": "",
    "name": "T",
    "extensions": ["EXPR"],
    "steps": [
        {"name": "S", "script": {"actions": {"onRun": {"command": "x"}}}}
    ]
}"#;

/// Minimal job template that does NOT require any extension.
const JOB_TEMPLATE_NO_EXTENSIONS: &str = r#"{
    "specificationVersion": "jobtemplate-2023-09",
    "name": "T",
    "steps": [
        {"name": "S", "script": {"actions": {"onRun": {"command": "x"}}}}
    ]
}"#;

/// Minimal environment template.
const ENV_TEMPLATE: &str = r#"{
    "specificationVersion": "environment-2023-09",
    "environment": {"name": "E", "variables": {"FOO": "bar"}}
}"#;

// ── get_supported_extensions ────────────────────────────────────────

/// The helper returns the full authoritative default list,
/// sourced from `openjd_model::ModelExtension::ALL`. Must include
/// every currently-known extension.
#[test]
fn get_supported_extensions_returns_full_default_list() {
    let exts = get_supported_extensions();
    // Exactly the four currently-known extensions. If a new one is
    // added upstream, this test will fail and prompt the JS
    // binding author to decide whether to update the default.
    assert_eq!(exts.len(), 4);
    assert!(exts.contains(&"TASK_CHUNKING".to_string()));
    assert!(exts.contains(&"REDACTED_ENV_VARS".to_string()));
    assert!(exts.contains(&"FEATURE_BUNDLE_1".to_string()));
    assert!(exts.contains(&"EXPR".to_string()));
}

// ── Default behavior (supportedExtensions omitted) ──────────────────

/// Omitting `supportedExtensions` uses the full default, which
/// includes EXPR. An EXPR-using template decodes cleanly.
#[test]
fn decode_with_default_extensions_accepts_expr_template() {
    decode_job_template_str(JOB_TEMPLATE_USING_EXPR, None, None, None)
        .expect("default extension set includes EXPR");
}

/// A template that doesn't declare any extensions decodes under the
/// full default as expected.
#[test]
fn decode_with_default_extensions_accepts_plain_template() {
    decode_job_template_str(JOB_TEMPLATE_NO_EXTENSIONS, None, None, None)
        .expect("default extension set accepts plain template");
}

// ── F8 regression guards: host can disable EXPR ─────────────────────

/// When the host passes a `supportedExtensions` list that excludes
/// EXPR, a template that declares EXPR must fail validation. This is
/// the attack scenario from the review: a browser template-viewer
/// that has no reason to evaluate EXPR on untrusted input can shut
/// off the attack surface.
#[test]
fn decode_without_expr_rejects_template_that_requires_expr() {
    let allowlist = vec!["TASK_CHUNKING".to_string()];
    let err = match decode_job_template_str(
        JOB_TEMPLATE_USING_EXPR,
        None,
        None,
        Some(allowlist.as_slice()),
    ) {
        Ok(_) => panic!("must reject EXPR template under restricted allowlist"),
        Err(e) => e,
    };
    assert!(
        err.contains("EXPR") || err.to_lowercase().contains("extension"),
        "expected unsupported-extension error, got: {err}"
    );
}

/// An empty allowlist disables every extension. The plain
/// no-extension template still decodes (it doesn't require any).
#[test]
fn decode_with_empty_allowlist_accepts_plain_template() {
    let empty: Vec<String> = vec![];
    decode_job_template_str(
        JOB_TEMPLATE_NO_EXTENSIONS,
        None,
        None,
        Some(empty.as_slice()),
    )
    .expect("empty allowlist is fine for a template that uses no extensions");
}

/// An empty allowlist rejects a template that requires an extension.
#[test]
fn decode_with_empty_allowlist_rejects_expr_template() {
    let empty: Vec<String> = vec![];
    let err = match decode_job_template_str(
        JOB_TEMPLATE_USING_EXPR,
        None,
        None,
        Some(empty.as_slice()),
    ) {
        Ok(_) => panic!("empty allowlist must reject EXPR template"),
        Err(e) => e,
    };
    assert!(
        err.contains("EXPR") || err.to_lowercase().contains("extension"),
        "expected unsupported-extension error, got: {err}"
    );
}

/// A `supportedExtensions` list that includes EXPR decodes the EXPR
/// template. Confirms the opt-in path round-trips.
#[test]
fn decode_with_explicit_expr_allowlist_accepts_expr_template() {
    let only_expr = vec!["EXPR".to_string()];
    decode_job_template_str(
        JOB_TEMPLATE_USING_EXPR,
        None,
        None,
        Some(only_expr.as_slice()),
    )
    .expect("EXPR in allowlist accepts EXPR template");
}

/// An unknown extension name in the allowlist is rejected — the
/// underlying model layer emits an error on any unsupported name.
#[test]
fn decode_with_unknown_extension_name_rejects_cleanly() {
    let bogus = vec!["NOT_A_REAL_EXTENSION".to_string()];
    // Use a template that declares the bogus extension so the model
    // actually exercises the unsupported-extension path. Declaring
    // an extension in the allowlist but not in the template is a
    // valid no-op.
    let template = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "T",
        "extensions": ["NOT_A_REAL_EXTENSION"],
        "steps": [
            {"name": "S", "script": {"actions": {"onRun": {"command": "x"}}}}
        ]
    }"#;
    let err = match decode_job_template_str(template, None, None, Some(bogus.as_slice())) {
        Ok(_) => panic!("must reject template with unknown extension"),
        Err(e) => e,
    };
    assert!(
        err.to_lowercase().contains("extension") || err.contains("NOT_A_REAL_EXTENSION"),
        "expected unknown-extension error, got: {err}"
    );
}

// ── Parity across all decode entry points ───────────────────────────

/// `decodeJobTemplateFromObject` must honor `supportedExtensions`
/// the same way as the string-input path — it flows through the
/// same `openjd_model::decode_job_template` call.
#[test]
fn decode_from_object_respects_supported_extensions() {
    let v: serde_json::Value = serde_json::from_str(JOB_TEMPLATE_USING_EXPR).unwrap();
    let allowlist = vec!["TASK_CHUNKING".to_string()];
    let err = match decode_job_template_from_object(v, None, Some(allowlist.as_slice())) {
        Ok(_) => panic!("must reject EXPR template under restricted allowlist"),
        Err(e) => e,
    };
    assert!(
        err.contains("EXPR") || err.to_lowercase().contains("extension"),
        "expected unsupported-extension error, got: {err}"
    );
}

/// Environment templates also take `supportedExtensions`. Any
/// extension-requiring env template must be rejected when the
/// extension is excluded.
#[test]
fn decode_environment_default_extensions_accepts_plain_template() {
    decode_environment_template_str(ENV_TEMPLATE, None, None, None)
        .expect("default extension set accepts plain env template");
}

#[test]
fn decode_environment_empty_allowlist_accepts_plain_template() {
    let empty: Vec<String> = vec![];
    decode_environment_template_str(ENV_TEMPLATE, None, None, Some(empty.as_slice()))
        .expect("empty allowlist fine when env template uses no extensions");
}
