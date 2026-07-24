// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Fuzz the full template → job instantiation pipeline.
//!
//! `model_decode` stops at `decode_job_template` (parse + validate). The two
//! documented model panics (finding #1 `json_to_expr_value`, finding #2 float
//! parameter-space construction) live one layer past that, in
//! `preprocess_job_parameters` + `create_job`: parameter-value coercion,
//! format-string evaluation, parameter-space iteration, and chunk-count
//! arithmetic. This target drives that pipeline exactly as `openjd run` does.
//!
//! Both stages MUST return `Ok` or `Err` for any decoded template and any
//! supplied parameter values — never panic, abort, or hang.

#![no_main]

use libfuzzer_sys::fuzz_target;
use openjd_expr::path_mapping::PathFormat;
use openjd_expr::ExprValue;
use openjd_model::template::parse::{decode_job_template, document_string_to_object, DocumentType};
use openjd_model::types::{CallerLimits, ModelExtension};
use openjd_model::{
    create_job, preprocess_job_parameters, JobParameterInputValues, PathParameterOptions,
};

/// Split the input at the first NUL: the head is an optional JSON object of
/// parameter name → value (fed to the job as user-supplied parameter values),
/// the tail is the template document text. Wiring parameter values in lets the
/// fuzzer reach coercion and format-string-resolution code that a parameterless
/// template never exercises.
fn split(data: &[u8]) -> (JobParameterInputValues, &[u8]) {
    match data.iter().position(|&b| b == 0) {
        Some(i) => {
            let mut values = JobParameterInputValues::new();
            if let Some(obj) = std::str::from_utf8(&data[..i])
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                .and_then(|v| v.as_object().cloned())
            {
                for (k, v) in obj {
                    // CLI callers pass everything as a string and let
                    // preprocess coerce; mirror that for string values, and
                    // pass through ints/bools directly for typed coverage.
                    let ev = match v {
                        serde_json::Value::String(s) => ExprValue::String(s),
                        serde_json::Value::Bool(b) => ExprValue::Bool(b),
                        serde_json::Value::Number(n) => match n.as_i64() {
                            Some(x) => ExprValue::Int(x),
                            None => continue,
                        },
                        _ => continue,
                    };
                    values.insert(k, ev);
                }
            }
            (values, &data[i + 1..])
        }
        None => (JobParameterInputValues::new(), data),
    }
}

fuzz_target!(|data: &[u8]| {
    let (input_values, doc_bytes) = split(data);
    let Ok(doc) = std::str::from_utf8(doc_bytes) else {
        return;
    };

    let limits = CallerLimits::default();
    let Ok(value) = document_string_to_object(doc, DocumentType::Yaml, &limits) else {
        return;
    };
    // Enable every recognized extension so the widest set of template features
    // (and thus the most instantiation code) is reachable. `supported_extensions`
    // is an allowlist: `None` / `Some(&[])` would reject any template declaring
    // an `extensions:` list, narrowing this target to extension-free templates.
    // Pass the full ModelExtension::ALL name list instead.
    let all_extensions: Vec<&str> = ModelExtension::ALL.iter().map(|e| e.as_str()).collect();
    let Ok(template) = decode_job_template(value, Some(&all_extensions), &limits) else {
        return;
    };

    // Absolute dirs + walk-up allowed so PATH-parameter handling doesn't reject
    // the fuzzer's synthetic paths before the interesting code runs.
    let path_options = PathParameterOptions {
        job_template_dir: "/tmpl",
        current_working_dir: "/cwd",
        path_format: PathFormat::Posix,
        allow_template_dir_walk_up: true,
        allow_uri_path_values: true,
    };

    let Ok(param_values) =
        preprocess_job_parameters(&template, &input_values, &[], &path_options)
    else {
        return;
    };

    let ctx = template.default_validation_context();
    let _ = create_job(&template, &param_values, &ctx);
});
