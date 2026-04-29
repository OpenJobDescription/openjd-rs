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
    pub(crate) inner: openjd_model::JobTemplate,
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

    /// Get the full template as a JS object via JSON serialization.
    #[wasm_bindgen(js_name = "toJSON")]
    pub fn to_json(&self) -> Result<JsValue, JsError> {
        // Serialize the YAML value back to JSON for JS consumption
        let yaml_str = serde_yaml::to_string(
            &serde_yaml::to_value(&self.inner.specification_version)
                .map_err(|e| JsError::new(&e.to_string()))?,
        )
        .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(JsValue::from_str(&yaml_str))
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
    pub(crate) inner: openjd_model::EnvironmentTemplate,
}

#[wasm_bindgen(js_class = "EnvironmentTemplate")]
impl JsEnvironmentTemplate {
    #[wasm_bindgen(getter, js_name = "specificationVersion")]
    pub fn specification_version(&self) -> String {
        self.inner.specification_version.clone()
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

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "EXPR",
    "TASK_CHUNKING",
    "REDACTED_ENV_VARS",
    "FEATURE_BUNDLE_1",
];

/// Decode and validate a job template from a JSON/YAML string.
#[wasm_bindgen(js_name = "decodeJobTemplate")]
pub fn decode_job_template(input: &str) -> Result<JsJobTemplate, JsError> {
    let yaml_value: serde_json::Value = serde_yaml::from_str(input).map_err(yaml_to_js_error)?;
    let template = openjd_model::decode_job_template(
        yaml_value,
        Some(SUPPORTED_EXTENSIONS),
        &openjd_model::CallerLimits::default(),
    )
    .map_err(to_js_error)?;
    Ok(JsJobTemplate { inner: template })
}

/// Decode and validate an environment template from a JSON/YAML string.
#[wasm_bindgen(js_name = "decodeEnvironmentTemplate")]
pub fn decode_environment_template(input: &str) -> Result<JsEnvironmentTemplate, JsError> {
    let yaml_value: serde_json::Value = serde_yaml::from_str(input).map_err(yaml_to_js_error)?;
    let template =
        openjd_model::decode_environment_template(yaml_value, Some(SUPPORTED_EXTENSIONS))
            .map_err(to_js_error)?;
    Ok(JsEnvironmentTemplate { inner: template })
}

/// Validate a template string. Returns an array of structured error objects (empty = valid).
/// Each error has `path` (array of {type, value} elements), `message`, and `severity` fields.
#[wasm_bindgen(js_name = "validateTemplate")]
pub fn validate_template(input: &str) -> Result<JsValue, JsError> {
    let yaml_value: serde_json::Value = serde_yaml::from_str(input).map_err(yaml_to_js_error)?;
    match openjd_model::decode_template(
        yaml_value,
        Some(SUPPORTED_EXTENSIONS),
        &openjd_model::CallerLimits::default(),
    ) {
        Ok(_) => serde_wasm_bindgen::to_value(&Vec::<serde_json::Value>::new())
            .map_err(serde_wasm_to_js_error),
        Err(openjd_model::ModelError::ModelValidation(errors)) => {
            let js_errors: Vec<serde_json::Value> = errors
                .errors
                .iter()
                .map(|e| {
                    let path_elements: Vec<serde_json::Value> = e
                        .path
                        .iter()
                        .map(|p| match p {
                            openjd_model::PathElement::Field(name) => {
                                serde_json::json!({"type": "field", "value": name})
                            }
                            openjd_model::PathElement::Index(i) => {
                                serde_json::json!({"type": "index", "value": i})
                            }
                        })
                        .collect();
                    serde_json::json!({
                        "path": path_elements,
                        "message": e.message,
                        "severity": "error",
                    })
                })
                .collect();
            serde_wasm_bindgen::to_value(&js_errors).map_err(serde_wasm_to_js_error)
        }
        Err(e) => {
            // Fatal decode error — return as a single root-level error
            let js_errors = vec![serde_json::json!({
                "path": [],
                "message": e.to_string(),
                "severity": "error",
            })];
            serde_wasm_bindgen::to_value(&js_errors).map_err(serde_wasm_to_js_error)
        }
    }
}

/// Check if a template string is a job template (true), environment template (false), or invalid (throws).
#[wasm_bindgen(js_name = "isJobTemplate")]
pub fn is_job_template(input: &str) -> Result<bool, JsError> {
    let yaml_value: serde_json::Value = serde_yaml::from_str(input).map_err(yaml_to_js_error)?;
    match openjd_model::decode_template(
        yaml_value,
        Some(SUPPORTED_EXTENSIONS),
        &openjd_model::CallerLimits::default(),
    ) {
        Ok(openjd_model::DecodedTemplate::Job(_)) => Ok(true),
        Ok(openjd_model::DecodedTemplate::Environment(_)) => Ok(false),
        Err(e) => Err(to_js_error(e)),
    }
}

/// Get the specification version from a template string.
#[wasm_bindgen(js_name = "getSpecVersion")]
pub fn get_spec_version(input: &str) -> Result<String, JsError> {
    let yaml_value: serde_json::Value = serde_yaml::from_str(input).map_err(yaml_to_js_error)?;
    let decoded = openjd_model::decode_template(
        yaml_value,
        Some(SUPPORTED_EXTENSIONS),
        &openjd_model::CallerLimits::default(),
    )
    .map_err(to_js_error)?;
    match decoded {
        openjd_model::DecodedTemplate::Job(t) => Ok(t.specification_version.to_string()),
        openjd_model::DecodedTemplate::Environment(t) => Ok(t.specification_version.to_string()),
    }
}

// ── Job creation ───────────────────────────────────────────────────

/// Create a fully resolved Job from a template and parameter values.
///
/// `params` is a JS object mapping parameter names to string values.
#[wasm_bindgen(js_name = "createJob")]
pub fn create_job(template: &JsJobTemplate, params: JsValue) -> Result<JsJob, JsError> {
    // Convert JS object to HashMap<String, String>
    let raw_params: HashMap<String, String> =
        serde_wasm_bindgen::from_value(params).map_err(serde_wasm_to_js_error)?;

    // Convert to JobParameterInputValues (HashMap<String, ExprValue>)
    let input_values: openjd_model::JobParameterInputValues = raw_params
        .into_iter()
        .map(|(k, v)| (k, openjd_expr::ExprValue::String(v)))
        .collect();

    // Preprocess parameters
    let path_options = openjd_model::PathParameterOptions {
        job_template_dir: "/",
        current_working_dir: "/",
        path_format: openjd_expr::PathFormat::Posix,
        allow_template_dir_walk_up: true,
        allow_uri_path_values: true,
    };
    let param_values =
        openjd_model::preprocess_job_parameters(&template.inner, &input_values, &[], &path_options)
            .map_err(to_js_error)?;

    // Create the job
    let job = openjd_model::create_job(
        &template.inner,
        &param_values,
        &openjd_model::CallerLimits::default(),
    )
    .map_err(to_js_error)?;
    Ok(JsJob { inner: job })
}

/// Preprocess raw parameter values into typed values.
#[wasm_bindgen(js_name = "preprocessJobParameters")]
pub fn preprocess_job_parameters(
    template: &JsJobTemplate,
    raw_values: JsValue,
) -> Result<JsValue, JsError> {
    let raw_params: HashMap<String, String> =
        serde_wasm_bindgen::from_value(raw_values).map_err(serde_wasm_to_js_error)?;

    let input_values: openjd_model::JobParameterInputValues = raw_params
        .into_iter()
        .map(|(k, v)| (k, openjd_expr::ExprValue::String(v)))
        .collect();

    let path_options = openjd_model::PathParameterOptions {
        job_template_dir: "/",
        current_working_dir: "/",
        path_format: openjd_expr::PathFormat::Posix,
        allow_template_dir_walk_up: true,
        allow_uri_path_values: true,
    };

    let param_values =
        openjd_model::preprocess_job_parameters(&template.inner, &input_values, &[], &path_options)
            .map_err(to_js_error)?;

    // Convert to {name: {type, value}} since JobParameterValue doesn't impl Serialize
    let map: HashMap<String, serde_json::Value> = param_values
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
        .collect();
    serde_wasm_bindgen::to_value(&map).map_err(serde_wasm_to_js_error)
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

/// Parse a YAML or JSON string into a JS object.
#[wasm_bindgen(js_name = "parseYaml")]
pub fn parse_yaml(input: &str) -> Result<JsValue, JsError> {
    let value: serde_yaml::Value = serde_yaml::from_str(input).map_err(yaml_to_js_error)?;
    serde_wasm_bindgen::to_value(&value).map_err(serde_wasm_to_js_error)
}
