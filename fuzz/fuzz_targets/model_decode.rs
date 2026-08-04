// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Fuzz job/environment template decoding.
//!
//! `document_string_to_object` + `decode_job_template` / `decode_environment_template`
//! is exactly the path `openjd check` runs on an untrusted YAML/JSON template
//! file. It parses YAML (via serde-saphyr, with a depth budget), then validates
//! structure, extensions, format strings, and parameter spaces. This is where
//! the `json_to_expr_value` panic and float-parameter-space panic lived. Decode
//! MUST reject malformed templates with an `Err`, never panic or abort.

#![no_main]

use libfuzzer_sys::fuzz_target;
use openjd_model::template::parse::{
    decode_environment_template, decode_job_template, document_string_to_object, DocumentType,
};
use openjd_model::types::CallerLimits;

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = std::str::from_utf8(data) else {
        return;
    };

    let limits = CallerLimits::default();

    // Try both document types. YAML is the common case; JSON exercises the
    // serde_json branch. Each returns a serde_json::Value on success.
    for doc_type in [DocumentType::Yaml, DocumentType::Json] {
        let Ok(value) = document_string_to_object(doc, doc_type, &limits) else {
            continue;
        };

        // Feed the decoded value into both decoders. `None` for supported
        // extensions means "no extensions"; the decoders still parse the
        // structure and report unknown-extension / validation errors as `Err`.
        // Clone because each decoder takes the value by move.
        let _ = decode_job_template(value.clone(), None, &limits);
        let _ = decode_environment_template(value, None);
    }
});
