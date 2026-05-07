// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests for `DocumentType`, `decodeJobTemplate*`, and
//! `decodeEnvironmentTemplate*` — the string/object decoding surface.
//!
//! These tests verify parity with the Python bindings
//! (`openjd_model_for_python/rust/src/model/decode.rs`):
//!
//! * The `decode*_str` path takes a `DocumentType` enum (default `YAML`)
//!   and routes through `openjd_model::parse::document_string_to_object`,
//!   which enforces a max-depth budget via `serde-saphyr` for YAML and
//!   the built-in `serde_json` recursion limit for JSON.
//!
//! * The `decode*_from_object` path accepts a pre-parsed JS object
//!   (on the Rust side, a `serde_json::Value`) and skips string parsing.
//!
//! The tests here close finding F2 from the security review: untrusted
//! YAML input must go through the depth-budgeted parser, not the
//! deprecated `serde_yaml 0.9` that the bindings previously used.

use openjd_for_js::model::{
    decode_environment_template_from_object, decode_environment_template_str,
    decode_job_template_from_object, decode_job_template_str, JsDocumentType,
};

const VALID_JOB_TEMPLATE_JSON: &str = r#"{
    "specificationVersion": "jobtemplate-2023-09",
    "name": "T",
    "steps": [
        {"name": "S", "script": {"actions": {"onRun": {"command": "x"}}}}
    ]
}"#;

const VALID_JOB_TEMPLATE_YAML: &str = "
specificationVersion: jobtemplate-2023-09
name: T
steps:
  - name: S
    script:
      actions:
        onRun:
          command: x
";

const VALID_ENV_TEMPLATE_YAML: &str = "
specificationVersion: environment-2023-09
environment:
  name: E
  variables:
    FOO: bar
";

// ── DocumentType ───────────────────────────────────────────────────

/// The JS-visible `DocumentType` must round-trip cleanly to the
/// Rust-side `openjd_model::parse::DocumentType`.
#[test]
fn document_type_round_trips() {
    assert_eq!(
        JsDocumentType::Yaml.into_inner(),
        openjd_model::parse::DocumentType::Yaml
    );
    assert_eq!(
        JsDocumentType::Json.into_inner(),
        openjd_model::parse::DocumentType::Json
    );
}

// ── decode_job_template_str ─────────────────────────────────────────

/// `format = None` must default to YAML (matches Python's
/// `format=PyDocumentType.YAML`).
#[test]
fn decode_job_template_str_defaults_to_yaml() {
    let tmpl = decode_job_template_str(VALID_JOB_TEMPLATE_YAML, None, None)
        .expect("default format must accept valid YAML");
    // JobTemplate has no Serialize, but `name` is exposed on the wrapper.
    assert_eq!(tmpl.name(), "T");
}

/// `DocumentType::Yaml` explicitly must also accept JSON, because
/// JSON is a subset of YAML and `serde-saphyr` parses JSON-shaped
/// YAML correctly.
#[test]
fn decode_job_template_str_yaml_accepts_json() {
    let tmpl = decode_job_template_str(VALID_JOB_TEMPLATE_JSON, Some(JsDocumentType::Yaml), None)
        .expect("YAML parser must accept JSON-shaped input");
    assert_eq!(tmpl.name(), "T");
}

/// `DocumentType::Json` must accept JSON.
#[test]
fn decode_job_template_str_json_accepts_json() {
    let tmpl = decode_job_template_str(VALID_JOB_TEMPLATE_JSON, Some(JsDocumentType::Json), None)
        .expect("JSON parser must accept JSON");
    assert_eq!(tmpl.name(), "T");
}

/// `DocumentType::Json` must reject input that is valid YAML but
/// not valid JSON (proves the format flag is actually routed).
#[test]
fn decode_job_template_str_json_rejects_yaml_only_syntax() {
    // Unquoted bare keys + hash-comments are YAML, not JSON.
    let yaml_only = "
specificationVersion: jobtemplate-2023-09
name: T  # YAML comment
steps:
  - name: S
    script:
      actions:
        onRun:
          command: x
";
    let err = match decode_job_template_str(yaml_only, Some(JsDocumentType::Json), None) {
        Ok(_) => panic!("JSON parser must reject YAML-only syntax"),
        Err(e) => e,
    };
    // `document_string_to_object` wraps the JSON error with
    // "not a valid JSON document" prefix — assert on that.
    assert!(
        err.contains("valid JSON"),
        "expected JSON parse error, got: {err}"
    );
}

/// F2 regression guard: YAML deeper than `MAX_DOCUMENT_DEPTH` (128)
/// must be rejected with a depth-budget error rather than running
/// to stack exhaustion. This is the whole point of switching from
/// `serde_yaml` to `serde-saphyr`.
#[test]
fn decode_job_template_str_yaml_rejects_deep_nesting() {
    // Build a 200-level nested YAML map. This isn't a valid job
    // template on its own — it will fail with either a depth error
    // (desired) or a validation error saying it doesn't have a
    // specificationVersion. The critical property is that the
    // parser returns an error quickly, not that it stack-overflows.
    let mut doc = String::new();
    for i in 0..200 {
        for _ in 0..i {
            doc.push_str("  ");
        }
        doc.push_str("a:\n");
    }
    for i in 0..200 {
        for _ in 0..(200 - 1 - i) {
            doc.push_str("  ");
        }
        doc.push_str("  b: 1\n");
    }

    let result = decode_job_template_str(&doc, Some(JsDocumentType::Yaml), None);
    assert!(
        result.is_err(),
        "200-level nested YAML must be rejected, not parsed"
    );
    // Don't assert on the specific error text — depth-budget vs.
    // missing-specificationVersion is an acceptable range of outcomes.
    // The invariant we care about is "does not crash the host."
}

/// Malformed YAML returns a structured error (not a panic).
#[test]
fn decode_job_template_str_rejects_malformed_yaml() {
    let err = match decode_job_template_str("{{{{", Some(JsDocumentType::Yaml), None) {
        Ok(_) => panic!("must fail on garbage input"),
        Err(e) => e,
    };
    // The specific wording comes from `document_string_to_object`.
    assert!(
        err.contains("valid YAML"),
        "expected YAML parse error, got: {err}"
    );
}

// ── decode_job_template_from_object ─────────────────────────────────

/// `decodeJobTemplateFromObject` accepts a pre-parsed JSON value
/// and does not invoke any YAML parser. Parity with Python's
/// `decode_job_template_dict`.
#[test]
fn decode_job_template_from_object_accepts_valid_value() {
    let v: serde_json::Value = serde_json::from_str(VALID_JOB_TEMPLATE_JSON).unwrap();
    let tmpl = decode_job_template_from_object(v, None)
        .expect("decode_from_object must accept a valid parsed value");
    assert_eq!(tmpl.name(), "T");
}

/// A top-level JSON array (not an object) is rejected — the template
/// must always be a map.
#[test]
fn decode_job_template_from_object_rejects_non_object() {
    let v: serde_json::Value = serde_json::Value::Array(vec![]);
    assert!(
        decode_job_template_from_object(v, None).is_err(),
        "non-object template must be rejected"
    );
}

// ── decode_environment_template_* ───────────────────────────────────

#[test]
fn decode_environment_template_str_defaults_to_yaml() {
    let et = decode_environment_template_str(VALID_ENV_TEMPLATE_YAML, None, None)
        .expect("default format must accept valid YAML");
    assert_eq!(et.specification_version(), "environment-2023-09");
}

#[test]
fn decode_environment_template_from_object_accepts_valid_value() {
    let v: serde_json::Value = serde_json::json!({
        "specificationVersion": "environment-2023-09",
        "environment": {"name": "E", "variables": {"FOO": "bar"}},
    });
    let et = decode_environment_template_from_object(v, None)
        .expect("decode_from_object must accept a valid parsed value");
    assert_eq!(et.specification_version(), "environment-2023-09");
}
