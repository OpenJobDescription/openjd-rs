// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Model bindings: template decode, job creation, step dependency graph,
//! parameter space iteration.

use std::collections::HashMap;

use crate::errors::*;
use crate::expr::JsSymbolTable;
use wasm_bindgen::prelude::*;

// ── Template wrappers ──────────────────────────────────────────────

/// A decoded job template.
#[wasm_bindgen(js_name = "JobTemplate")]
pub struct JsJobTemplate {
    pub(crate) inner: openjd_model::template::JobTemplate,
}

#[wasm_bindgen(js_class = "JobTemplate")]
impl JsJobTemplate {
    #[wasm_bindgen(getter)]
    pub fn name(&self) -> String {
        self.inner.name.raw().to_string()
    }

    #[wasm_bindgen(getter, js_name = "specificationVersion")]
    pub fn specification_version(&self) -> String {
        self.inner.specification_version.clone()
    }

    /// Number of steps.
    #[wasm_bindgen(getter, js_name = "stepCount")]
    pub fn step_count(&self) -> usize {
        self.inner.steps.len()
    }

    /// Number of parameter definitions.
    #[wasm_bindgen(getter, js_name = "parameterDefinitionCount")]
    pub fn parameter_definition_count(&self) -> usize {
        self.inner.parameter_definitions_list().len()
    }
}

/// A decoded environment template.
#[wasm_bindgen(js_name = "EnvironmentTemplate")]
pub struct JsEnvironmentTemplate {
    pub(crate) inner: openjd_model::template::EnvironmentTemplate,
}

#[wasm_bindgen(js_class = "EnvironmentTemplate")]
impl JsEnvironmentTemplate {
    #[wasm_bindgen(getter, js_name = "specificationVersion")]
    pub fn specification_version(&self) -> String {
        self.inner.specification_version.clone()
    }
}

// ── PathParameterOptions ───────────────────────────────────────────

/// Options controlling how `PATH`-typed job parameters are resolved.
///
/// Mirrors [`openjd_model::PathParameterOptions`] field-for-field.
/// Construct with `new(jobTemplateDir, currentWorkingDir)` — the
/// remaining fields default to the same values as
/// `PathParameterOptions::new` in Rust:
/// - `pathFormat`: Posix (equivalent to `PathFormat::host()` on wasm32,
///   which always evaluates to Posix since `target_os` is not `windows`),
/// - `allowTemplateDirWalkUp`: `false`,
/// - `allowUriPathValues`: `false`.
///
/// Tune fields as needed via setters before passing the options to
/// `createJob` or `preprocessJobParameters`.
#[wasm_bindgen(js_name = "PathParameterOptions")]
#[derive(Clone, Debug)]
pub struct JsPathParameterOptions {
    job_template_dir: String,
    current_working_dir: String,
    path_format: crate::expr::JsPathFormat,
    allow_template_dir_walk_up: bool,
    allow_uri_path_values: bool,
}

#[wasm_bindgen(js_class = "PathParameterOptions")]
impl JsPathParameterOptions {
    /// Construct options with the same safe defaults as
    /// `openjd_model::PathParameterOptions::new` in Rust.
    #[wasm_bindgen(constructor)]
    pub fn new(job_template_dir: &str, current_working_dir: &str) -> JsPathParameterOptions {
        JsPathParameterOptions {
            job_template_dir: job_template_dir.to_string(),
            current_working_dir: current_working_dir.to_string(),
            // `PathFormat::host()` returns `Posix` on `wasm32-unknown-unknown`
            // because `cfg!(windows)` is false there. We hardcode `Posix`
            // to make the WASM default deterministic and to match what
            // `PathFormat::host()` would return anyway.
            path_format: crate::expr::JsPathFormat::Posix,
            allow_template_dir_walk_up: false,
            allow_uri_path_values: false,
        }
    }

    #[wasm_bindgen(getter, js_name = "jobTemplateDir")]
    pub fn job_template_dir(&self) -> String {
        self.job_template_dir.clone()
    }

    #[wasm_bindgen(setter, js_name = "jobTemplateDir")]
    pub fn set_job_template_dir(&mut self, v: String) {
        self.job_template_dir = v;
    }

    #[wasm_bindgen(getter, js_name = "currentWorkingDir")]
    pub fn current_working_dir(&self) -> String {
        self.current_working_dir.clone()
    }

    #[wasm_bindgen(setter, js_name = "currentWorkingDir")]
    pub fn set_current_working_dir(&mut self, v: String) {
        self.current_working_dir = v;
    }

    #[wasm_bindgen(getter, js_name = "pathFormat")]
    pub fn path_format(&self) -> crate::expr::JsPathFormat {
        self.path_format
    }

    #[wasm_bindgen(setter, js_name = "pathFormat")]
    pub fn set_path_format(&mut self, v: crate::expr::JsPathFormat) {
        self.path_format = v;
    }

    #[wasm_bindgen(getter, js_name = "allowTemplateDirWalkUp")]
    pub fn allow_template_dir_walk_up(&self) -> bool {
        self.allow_template_dir_walk_up
    }

    #[wasm_bindgen(setter, js_name = "allowTemplateDirWalkUp")]
    pub fn set_allow_template_dir_walk_up(&mut self, v: bool) {
        self.allow_template_dir_walk_up = v;
    }

    #[wasm_bindgen(getter, js_name = "allowUriPathValues")]
    pub fn allow_uri_path_values(&self) -> bool {
        self.allow_uri_path_values
    }

    #[wasm_bindgen(setter, js_name = "allowUriPathValues")]
    pub fn set_allow_uri_path_values(&mut self, v: bool) {
        self.allow_uri_path_values = v;
    }
}

impl JsPathParameterOptions {
    /// Borrow as the Rust-side options struct for a call into
    /// `openjd_model`. The returned struct borrows `&self`'s strings,
    /// so the returned value cannot outlive `self`.
    pub fn as_rust(&self) -> openjd_model::PathParameterOptions<'_> {
        openjd_model::PathParameterOptions {
            job_template_dir: &self.job_template_dir,
            current_working_dir: &self.current_working_dir,
            path_format: self.path_format.into_inner(),
            allow_template_dir_walk_up: self.allow_template_dir_walk_up,
            allow_uri_path_values: self.allow_uri_path_values,
        }
    }
}

// ── CallerLimits ───────────────────────────────────────────────────

/// Optional caps that a host can apply on top of the spec-defined
/// limits. Mirrors [`openjd_model::CallerLimits`] field-for-field.
///
/// Exposed to JS as a plain structural type rather than an opaque
/// handle. Every field is optional; `undefined` (or omitted) means
/// "no additional cap beyond the spec-defined limit." Caller
/// limits can only add restrictions, never relax spec-defined ones.
///
/// On the JS side, construct as a plain object with camelCase
/// keys matching this struct's fields:
///
/// ```ignore
/// mod.decodeJobTemplate(doc, undefined, {
///     maxTemplateSize: 10_000_000,   // 10 MB
///     maxTaskCount: 1_000_000n,      // u64 — pass as BigInt
/// });
/// ```
///
/// Plain-object shape avoids the ownership footgun of passing an
/// exported wasm_bindgen class by value (which invalidates the JS
/// handle after one use). Callers can reuse the same options object
/// across multiple calls without issue.
///
/// Resolves finding F4 from the security review: without these
/// knobs a template author could submit arbitrarily large input
/// and the host had no way to cap memory before parsing begins.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JsCallerLimits {
    #[serde(default)]
    pub max_step_count: Option<usize>,
    #[serde(default)]
    pub max_env_count: Option<usize>,
    #[serde(default)]
    pub max_task_count: Option<u64>,
    #[serde(default)]
    pub max_step_script_size: Option<usize>,
    #[serde(default)]
    pub max_environment_size: Option<usize>,
    #[serde(default)]
    pub max_template_size: Option<usize>,
}

impl JsCallerLimits {
    /// Factory used by rlib tests to build an empty limits value.
    /// JS callers use the plain-object literal form instead.
    pub fn new() -> JsCallerLimits {
        JsCallerLimits::default()
    }

    /// Convert to the Rust-side struct for a call into
    /// `openjd_model`. Cheap — copies six `Option` scalars.
    pub fn as_rust(&self) -> openjd_model::CallerLimits {
        openjd_model::CallerLimits {
            max_step_count: self.max_step_count,
            max_env_count: self.max_env_count,
            max_task_count: self.max_task_count,
            max_step_script_size: self.max_step_script_size,
            max_environment_size: self.max_environment_size,
            max_template_size: self.max_template_size,
        }
    }

    /// Deserialize a JS value into `JsCallerLimits`. Returns `None`
    /// when the caller passes `undefined`/`null` (the "no caps"
    /// sentinel); returns `Err` when the value is present but
    /// malformed (unknown key, wrong type, etc.).
    pub fn from_js_value(value: JsValue) -> Result<Option<JsCallerLimits>, JsError> {
        if value.is_undefined() || value.is_null() {
            return Ok(None);
        }
        let limits: JsCallerLimits =
            serde_wasm_bindgen::from_value(value).map_err(serde_wasm_to_js_error)?;
        Ok(Some(limits))
    }
}

// ── Job wrappers ───────────────────────────────────────────────────

/// A fully instantiated job with all format strings resolved.
#[wasm_bindgen(js_name = "Job")]
pub struct JsJob {
    pub(crate) inner: openjd_model::job::Job,
}

#[wasm_bindgen(js_class = "Job")]
impl JsJob {
    #[wasm_bindgen(getter)]
    pub fn name(&self) -> String {
        self.inner.name.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn description(&self) -> Option<String> {
        self.inner.description.clone()
    }

    /// Get the full job as a JS object via serde.
    #[wasm_bindgen(js_name = "toJSON")]
    pub fn to_json(&self) -> Result<JsValue, JsError> {
        serde_wasm_bindgen::to_value(&self.inner).map_err(serde_wasm_to_js_error)
    }

    /// Number of steps.
    #[wasm_bindgen(getter, js_name = "stepCount")]
    pub fn step_count(&self) -> usize {
        self.inner.steps.len()
    }

    /// Get step names.
    #[wasm_bindgen(getter, js_name = "stepNames")]
    pub fn step_names(&self) -> Vec<String> {
        self.inner.steps.iter().map(|s| s.name.clone()).collect()
    }
}

// ── StepDependencyGraph ────────────────────────────────────────────

/// Step dependency graph for analyzing execution order.
#[wasm_bindgen(js_name = "StepDependencyGraph")]
pub struct JsStepDependencyGraph {
    inner: openjd_model::StepDependencyGraph,
}

#[wasm_bindgen(js_class = "StepDependencyGraph")]
impl JsStepDependencyGraph {
    /// Create a dependency graph from a Job.
    #[wasm_bindgen(constructor)]
    pub fn new(job: &JsJob) -> Result<JsStepDependencyGraph, JsError> {
        let graph = openjd_model::StepDependencyGraph::new(&job.inner).map_err(to_js_error)?;
        Ok(JsStepDependencyGraph { inner: graph })
    }

    /// Get step names in topological (dependency) order.
    #[wasm_bindgen(js_name = "topologicalOrder")]
    pub fn topological_order(&self) -> Result<Vec<String>, JsError> {
        self.inner.topo_sorted_names().map_err(to_js_error)
    }
}

// ── StepParameterSpaceIterator ─────────────────────────────────────

/// Iterator over task parameter sets in a step's parameter space.
#[wasm_bindgen(js_name = "StepParameterSpaceIterator")]
pub struct JsStepParameterSpaceIterator {
    inner: openjd_model::StepParameterSpaceIterator,
}

#[wasm_bindgen(js_class = "StepParameterSpaceIterator")]
impl JsStepParameterSpaceIterator {
    /// Total number of tasks.
    #[wasm_bindgen(getter)]
    pub fn count(&self) -> usize {
        self.inner.len()
    }

    /// Get a specific task's parameter set as a JS object.
    pub fn get(&self, index: usize) -> Result<JsValue, JsError> {
        match self.inner.get(index) {
            Some(params) => {
                // Convert TaskParameterSet to a simple {name: value_string} object
                let map: HashMap<String, String> = params
                    .into_iter()
                    .map(|(k, v)| (k, v.value.to_display_string()))
                    .collect();
                serde_wasm_bindgen::to_value(&map).map_err(serde_wasm_to_js_error)
            }
            None => Err(JsError::new(&format!("Index {index} out of range"))),
        }
    }

    /// Get parameter names.
    #[wasm_bindgen(getter)]
    pub fn names(&self) -> Vec<String> {
        self.inner.names().iter().cloned().collect()
    }
}

// ── Decode functions ───────────────────────────────────────────────

/// Build a `Vec<&str>` covering the full default extension list,
/// sourced from [`openjd_model::ModelExtension::ALL`]. Use this as
/// the fallback when the JS caller omits `supportedExtensions`.
///
/// Centralizing on the upstream enum resolves the F8 drift problem:
/// the JS binding, the CLI, and Python all see the same authoritative
/// list without hand-maintained local copies.
fn default_supported_extensions() -> Vec<&'static str> {
    openjd_model::ModelExtension::ALL
        .iter()
        .map(|e| e.as_str())
        .collect()
}

/// Resolve the caller-provided `supported_extensions` slice to a
/// `Vec<&str>` suitable for passing to `openjd_model` decode
/// functions. `None` means "use the default"; `Some(&[])` means
/// "no extensions at all."
fn resolve_supported_extensions(supported_extensions: Option<&[String]>) -> Vec<&str> {
    match supported_extensions {
        Some(list) => list.iter().map(|s| s.as_str()).collect(),
        None => default_supported_extensions(),
    }
}

/// Return the full default list of supported extensions — the same
/// set used when `supportedExtensions` is omitted on a decode call.
///
/// Exposed as a helper (rather than a constant) because wasm-bindgen
/// does not support exporting Rust `const` / `static` items to JS.
/// Callers that want to start from the default and subtract a
/// specific extension can call this once and filter the result.
#[wasm_bindgen(js_name = "getSupportedExtensions")]
pub fn get_supported_extensions() -> Vec<String> {
    default_supported_extensions()
        .into_iter()
        .map(String::from)
        .collect()
}

/// Document format for template string input.
///
/// Mirrors [`openjd_model::template::parse::DocumentType`] and the `DocumentType`
/// enum exported by the Python bindings (`openjd._openjd_rs.DocumentType`).
/// `Yaml` is the safe default for untrusted input because YAML is a
/// strict superset of JSON — a caller that doesn't know which format
/// they received can pass `Yaml` and get the correct answer either way.
#[wasm_bindgen(js_name = "DocumentType")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JsDocumentType {
    Yaml = 0,
    Json = 1,
}

impl JsDocumentType {
    /// Convert to the Rust-side enum. Public so the rlib tests can
    /// verify the mapping.
    pub fn into_inner(self) -> openjd_model::template::parse::DocumentType {
        match self {
            JsDocumentType::Yaml => openjd_model::template::parse::DocumentType::Yaml,
            JsDocumentType::Json => openjd_model::template::parse::DocumentType::Json,
        }
    }
}

/// Decode and validate a job template from a string.
///
/// `format` selects JSON or YAML parsing. If omitted, defaults to
/// Decode and validate a job template from a string.
///
/// `format` selects JSON or YAML parsing. If omitted, defaults to
/// `DocumentType.Yaml` — which also accepts JSON, since JSON is a
/// subset of YAML. Mirrors the Python binding
/// `decode_job_template_str(document, format=DocumentType.YAML)`.
///
/// `limits` lets the host impose additional caps (template size,
/// step count, environment count, etc.) on top of the spec-defined
/// limits. Pass `undefined` for spec-only limits; pass a
/// `CallerLimits` instance to apply additional restrictions.
#[wasm_bindgen(js_name = "decodeJobTemplate")]
pub fn decode_job_template(
    input: &str,
    format: Option<JsDocumentType>,
    limits: JsValue,
    supported_extensions: Option<Vec<String>>,
) -> Result<JsJobTemplate, JsError> {
    let parsed = JsCallerLimits::from_js_value(limits)?;
    decode_job_template_str(
        input,
        format,
        parsed.as_ref(),
        supported_extensions.as_deref(),
    )
    .map_err(|e| JsError::new(&e))
}

/// Rust-native helper for [`decode_job_template`].
///
/// Exposed as a plain Rust function so rlib-target tests can exercise
/// the behavior without the `JsError` wrapping layer.
///
/// `supported_extensions`: `None` uses the full default from
/// [`openjd_model::ModelExtension::ALL`]; `Some(&[])` disables every
/// extension; `Some(&["EXPR"])` allows only EXPR. See
/// [`get_supported_extensions`] for the default list.
pub fn decode_job_template_str(
    input: &str,
    format: Option<JsDocumentType>,
    limits: Option<&JsCallerLimits>,
    supported_extensions: Option<&[String]>,
) -> Result<JsJobTemplate, String> {
    let doc_type = format.unwrap_or(JsDocumentType::Yaml).into_inner();
    let rust_limits = limits.map(|l| l.as_rust()).unwrap_or_default();
    let exts = resolve_supported_extensions(supported_extensions);
    let value =
        openjd_model::template::parse::document_string_to_object(input, doc_type, &rust_limits)
            .map_err(|e| e.to_string())?;
    let template = openjd_model::decode_job_template(value, Some(&exts), &rust_limits)
        .map_err(|e| e.to_string())?;
    Ok(JsJobTemplate { inner: template })
}

/// Decode and validate a job template from a pre-parsed JS object.
///
/// Mirrors the Python binding `decode_job_template_dict(template)`.
/// Use this when the caller already has a parsed JSON/YAML object on
/// the JS side; it skips the string-parsing step entirely.
///
/// `limits` applies to validation (step count, env count, etc.). The
/// `maxTemplateSize` cap is not meaningful here since the byte size
/// of the source string has already been discarded.
///
/// `supportedExtensions` behaves the same as on
/// [`decode_job_template`].
#[wasm_bindgen(js_name = "decodeJobTemplateFromObject")]
pub fn decode_job_template_from_object_js(
    obj: JsValue,
    limits: JsValue,
    supported_extensions: Option<Vec<String>>,
) -> Result<JsJobTemplate, JsError> {
    let value: serde_json::Value =
        serde_wasm_bindgen::from_value(obj).map_err(serde_wasm_to_js_error)?;
    let parsed = JsCallerLimits::from_js_value(limits)?;
    decode_job_template_from_object(value, parsed.as_ref(), supported_extensions.as_deref())
        .map_err(|e| JsError::new(&e))
}

/// Rust-native helper for [`decode_job_template_from_object_js`].
pub fn decode_job_template_from_object(
    value: serde_json::Value,
    limits: Option<&JsCallerLimits>,
    supported_extensions: Option<&[String]>,
) -> Result<JsJobTemplate, String> {
    if !value.is_object() {
        return Err("Template must be a JSON/YAML object".to_string());
    }
    let rust_limits = limits.map(|l| l.as_rust()).unwrap_or_default();
    let exts = resolve_supported_extensions(supported_extensions);
    let template = openjd_model::decode_job_template(value, Some(&exts), &rust_limits)
        .map_err(|e| e.to_string())?;
    Ok(JsJobTemplate { inner: template })
}

/// Decode and validate an environment template from a string.
///
/// Mirrors the Python binding
/// `decode_environment_template_str(document, format=DocumentType.YAML)`.
///
/// `limits` applies only the `maxTemplateSize` cap here, since
/// `openjd_model::decode_environment_template` does not accept
/// `CallerLimits` at the validation step.
///
/// `supportedExtensions` behaves the same as on
/// [`decode_job_template`].
#[wasm_bindgen(js_name = "decodeEnvironmentTemplate")]
pub fn decode_environment_template(
    input: &str,
    format: Option<JsDocumentType>,
    limits: JsValue,
    supported_extensions: Option<Vec<String>>,
) -> Result<JsEnvironmentTemplate, JsError> {
    let parsed = JsCallerLimits::from_js_value(limits)?;
    decode_environment_template_str(
        input,
        format,
        parsed.as_ref(),
        supported_extensions.as_deref(),
    )
    .map_err(|e| JsError::new(&e))
}

/// Rust-native helper for [`decode_environment_template`].
pub fn decode_environment_template_str(
    input: &str,
    format: Option<JsDocumentType>,
    limits: Option<&JsCallerLimits>,
    supported_extensions: Option<&[String]>,
) -> Result<JsEnvironmentTemplate, String> {
    let doc_type = format.unwrap_or(JsDocumentType::Yaml).into_inner();
    let rust_limits = limits.map(|l| l.as_rust()).unwrap_or_default();
    let exts = resolve_supported_extensions(supported_extensions);
    let value =
        openjd_model::template::parse::document_string_to_object(input, doc_type, &rust_limits)
            .map_err(|e| e.to_string())?;
    let template =
        openjd_model::decode_environment_template(value, Some(&exts)).map_err(|e| e.to_string())?;
    Ok(JsEnvironmentTemplate { inner: template })
}

/// Decode and validate an environment template from a pre-parsed JS object.
///
/// Mirrors the Python binding `decode_environment_template_dict(template)`.
/// `limits` is accepted for API uniformity with the other decode
/// entry points, but no cap in `CallerLimits` currently applies to
/// the object path for environment templates.
#[wasm_bindgen(js_name = "decodeEnvironmentTemplateFromObject")]
pub fn decode_environment_template_from_object_js(
    obj: JsValue,
    limits: JsValue,
    supported_extensions: Option<Vec<String>>,
) -> Result<JsEnvironmentTemplate, JsError> {
    let value: serde_json::Value =
        serde_wasm_bindgen::from_value(obj).map_err(serde_wasm_to_js_error)?;
    let parsed = JsCallerLimits::from_js_value(limits)?;
    decode_environment_template_from_object(value, parsed.as_ref(), supported_extensions.as_deref())
        .map_err(|e| JsError::new(&e))
}

/// Rust-native helper for [`decode_environment_template_from_object_js`].
pub fn decode_environment_template_from_object(
    value: serde_json::Value,
    _limits: Option<&JsCallerLimits>,
    supported_extensions: Option<&[String]>,
) -> Result<JsEnvironmentTemplate, String> {
    if !value.is_object() {
        return Err("Template must be a JSON/YAML object".to_string());
    }
    let exts = resolve_supported_extensions(supported_extensions);
    let template =
        openjd_model::decode_environment_template(value, Some(&exts)).map_err(|e| e.to_string())?;
    Ok(JsEnvironmentTemplate { inner: template })
}

// ── Job creation ───────────────────────────────────────────────────

/// Create a fully resolved Job from a template and parameter values.
///
/// `params` is a JS object mapping parameter names to string values.
/// `pathOptions` controls how `PATH` parameters are resolved. Construct
/// with `new PathParameterOptions(jobTemplateDir, currentWorkingDir)`.
///
/// `limits` lets the host cap task count and other resource limits.
/// `maxTaskCount` is checked after parameter space expansion.
#[wasm_bindgen(js_name = "createJob")]
pub fn create_job(
    template: &JsJobTemplate,
    params: JsValue,
    path_options: &JsPathParameterOptions,
    limits: JsValue,
) -> Result<JsJob, JsError> {
    let raw_params: HashMap<String, String> =
        serde_wasm_bindgen::from_value(params).map_err(serde_wasm_to_js_error)?;
    let parsed = JsCallerLimits::from_js_value(limits)?;
    create_job_with_map(template, raw_params, path_options, parsed.as_ref())
        .map_err(|e| JsError::new(&e))
}

/// Rust-native helper for [`create_job`].
///
/// Exposed as a plain Rust function so that rlib-target integration
/// tests can exercise the same behavior without going through the
/// `JsValue` boundary. The `JsError` returned from [`create_job`] is
/// constructed from the `String` this helper returns.
pub fn create_job_with_map(
    template: &JsJobTemplate,
    raw_params: HashMap<String, String>,
    path_options: &JsPathParameterOptions,
    limits: Option<&JsCallerLimits>,
) -> Result<JsJob, String> {
    let input_values: openjd_model::JobParameterInputValues = raw_params
        .into_iter()
        .map(|(k, v)| (k, openjd_expr::ExprValue::String(v)))
        .collect();

    let rust_opts = path_options.as_rust();
    let rust_limits = limits.map(|l| l.as_rust()).unwrap_or_default();
    let param_values =
        openjd_model::preprocess_job_parameters(&template.inner, &input_values, &[], &rust_opts)
            .map_err(|e| e.to_string())?;

    // Build a ValidationContext from the template's declared extensions
    // and the caller-supplied limits.
    let ctx = {
        let mut exts = std::collections::HashSet::new();
        if let Some(ext_list) = &template.inner.extensions {
            exts.extend(
                ext_list
                    .iter()
                    .filter_map(|s| s.as_str().parse::<openjd_model::ModelExtension>().ok()),
            );
        }
        openjd_model::ValidationContext::with_extensions(
            openjd_model::SpecificationRevision::V2023_09,
            exts,
        )
        .with_caller_limits(rust_limits)
    };

    let job = openjd_model::create_job(&template.inner, &param_values, &ctx)
        .map_err(|e| e.to_string())?;
    Ok(JsJob { inner: job })
}

/// Preprocess raw parameter values into typed values.
#[wasm_bindgen(js_name = "preprocessJobParameters")]
pub fn preprocess_job_parameters(
    template: &JsJobTemplate,
    raw_values: JsValue,
    path_options: &JsPathParameterOptions,
) -> Result<JsValue, JsError> {
    let raw_params: HashMap<String, String> =
        serde_wasm_bindgen::from_value(raw_values).map_err(serde_wasm_to_js_error)?;
    let map = preprocess_job_parameters_with_map(template, raw_params, path_options)
        .map_err(|e| JsError::new(&e))?;
    serde_wasm_bindgen::to_value(&map).map_err(serde_wasm_to_js_error)
}

/// Rust-native helper for [`preprocess_job_parameters`].
///
/// Returns the `{name: {type, value}}` map directly so rlib-target
/// tests can exercise the behavior without `JsValue` round-tripping.
pub fn preprocess_job_parameters_with_map(
    template: &JsJobTemplate,
    raw_params: HashMap<String, String>,
    path_options: &JsPathParameterOptions,
) -> Result<HashMap<String, serde_json::Value>, String> {
    let input_values: openjd_model::JobParameterInputValues = raw_params
        .into_iter()
        .map(|(k, v)| (k, openjd_expr::ExprValue::String(v)))
        .collect();

    let rust_opts = path_options.as_rust();
    let param_values =
        openjd_model::preprocess_job_parameters(&template.inner, &input_values, &[], &rust_opts)
            .map_err(|e| e.to_string())?;

    Ok(param_values
        .into_iter()
        .map(|(k, v)| {
            (
                k,
                serde_json::json!({
                    "type": format!("{:?}", v.param_type),
                    "value": v.value.to_display_string(),
                }),
            )
        })
        .collect())
}

/// Merge parameter definitions from job and environment templates.
#[wasm_bindgen(js_name = "mergeJobParameterDefinitions")]
pub fn merge_job_parameter_definitions(template: &JsJobTemplate) -> Result<JsValue, JsError> {
    let merged =
        openjd_model::merge_job_parameter_definitions(&template.inner, &[]).map_err(to_js_error)?;

    // Return as array of {name, type} objects
    let result: Vec<serde_json::Value> = merged
        .iter()
        .map(|m| {
            serde_json::json!({
                "name": m.name,
                "type": format!("{:?}", m.param_type),
            })
        })
        .collect();
    serde_wasm_bindgen::to_value(&result).map_err(serde_wasm_to_js_error)
}

/// Evaluate let bindings and return an updated symbol table.
#[wasm_bindgen(js_name = "evaluateLetBindings")]
pub fn evaluate_let_bindings(
    bindings: Vec<String>,
    symbols: &JsSymbolTable,
) -> Result<JsSymbolTable, JsError> {
    let result = openjd_model::evaluate_let_bindings(
        &bindings,
        &symbols.inner,
        None,
        openjd_expr::PathFormat::Posix,
    )
    .map_err(to_js_error)?;
    Ok(JsSymbolTable { inner: result })
}
