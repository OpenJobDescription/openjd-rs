// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! CLI argument parsing and parameter coercion for the run command.

use crate::common::{read_input_file_with, MAX_FILE_INPUT_SIZE};
use openjd_sessions::path_mapping::PathMappingRule;
use std::collections::HashMap;
use std::path::PathBuf;

/// Parse job parameters from CLI `--job-param` / `-p` arguments.
///
/// Supports `Key=Value`, `file://path` (JSON/YAML), and inline JSON `{...}`.
pub fn parse_cli_parameters(
    args: &[String],
) -> Result<HashMap<String, openjd_expr::ExprValue>, Box<dyn std::error::Error>> {
    let mut params = HashMap::new();
    for arg in args {
        let arg = arg.trim();
        if arg.starts_with("file://") {
            let path = PathBuf::from(arg.trim_start_matches("file://"));
            let content = read_input_file_with(
                &path,
                Some(MAX_FILE_INPUT_SIZE),
                || {
                    format!(
                        "Provided parameter file '{}' does not exist.",
                        path.display()
                    )
                },
                || format!("Cannot read parameter file '{}'", path.display()),
            )?;
            let doc_type = if path.extension().and_then(|e| e.to_str()) == Some("json") {
                openjd_model::template::parse::DocumentType::Json
            } else {
                openjd_model::template::parse::DocumentType::Yaml
            };
            let value = openjd_model::template::parse::document_string_to_object(
                &content,
                doc_type,
                &openjd_model::CallerLimits::default(),
            )
            .map_err(|_| format!("Job parameter file '{}' should contain a dictionary.", arg))?;
            for (k, v) in value.as_object().unwrap() {
                params.insert(k.clone(), json_val_to_expr(v));
            }
        } else if arg.starts_with('{') && arg.ends_with('}') {
            let value: serde_json::Value = serde_json::from_str(arg)
                .map_err(|_| format!("Job parameter string ('{arg}') not formatted correctly. It must be key=value pairs, inline JSON, or a path to a JSON or YAML document prefixed with 'file://'."))?;
            if let Some(obj) = value.as_object() {
                for (k, v) in obj {
                    params.insert(k.clone(), json_val_to_expr(v));
                }
            } else {
                return Err(format!("Job parameter ('{arg}') must contain a dictionary mapping job parameters to their value.").into());
            }
        } else if let Some((key, value)) = arg.split_once('=') {
            params.insert(
                key.to_string(),
                openjd_expr::ExprValue::String(value.to_string()),
            );
        } else {
            return Err(format!("Job parameter string ('{arg}') not formatted correctly. It must be key=value pairs, inline JSON, or a path to a JSON or YAML document prefixed with 'file://'.").into());
        }
    }
    Ok(params)
}

fn json_val_to_expr(v: &serde_json::Value) -> openjd_expr::ExprValue {
    match v {
        serde_json::Value::String(s) => openjd_expr::ExprValue::String(s.clone()),
        other => openjd_expr::ExprValue::String(other.to_string()),
    }
}

/// Parse `--task-param` arguments into a single task parameter set.
pub fn parse_task_params(
    args: &[String],
) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let mut params = HashMap::new();
    let mut errors = Vec::new();
    for arg in args {
        let arg = arg.trim();
        if let Some((name, value)) = arg.split_once('=') {
            if !name.is_empty() && !value.is_empty() {
                if params.contains_key(name) {
                    errors.push(format!(
                        "Task parameter '{name}' has been defined more than once."
                    ));
                } else {
                    params.insert(name.to_string(), value.to_string());
                }
            } else {
                errors.push(format!(
                    "Task parameter '{arg}' defined incorrectly. Expected '<NAME>=<VALUE>' format."
                ));
            }
        } else {
            errors.push(format!(
                "Task parameter '{arg}' defined incorrectly. Expected '<NAME>=<VALUE>' format."
            ));
        }
    }
    if !errors.is_empty() {
        let msg = errors
            .iter()
            .map(|e| format!("\n - {e}"))
            .collect::<String>();
        return Err(format!("Found the following errors collecting Task parameters:{msg}").into());
    }
    Ok(params)
}

/// Parse `--tasks` argument into a list of task parameter sets.
pub fn parse_tasks_arg(
    arg: &str,
) -> Result<Vec<HashMap<String, String>>, Box<dyn std::error::Error>> {
    let arg = arg.trim();
    let raw: serde_json::Value = if arg.starts_with("file://") {
        let path = std::path::Path::new(arg.trim_start_matches("file://"));
        let content = read_input_file_with(
            path,
            Some(MAX_FILE_INPUT_SIZE),
            || format!("Provided tasks file '{}' does not exist.", path.display()),
            || format!("Cannot read tasks file '{}'", path.display()),
        )?;
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            serde_json::from_str(&content)?
        } else {
            let v: serde_json::Value = serde_saphyr::from_str(&content)?;
            v
        }
    } else {
        serde_json::from_str(arg)
            .map_err(|_| "--tasks argument must be a JSON encoded list of maps or a string with the file:// prefix.")?
    };

    let arr = raw
        .as_array()
        .ok_or("--tasks argument must be a list of maps from string to string when decoded.")?;
    let mut result = Vec::new();
    for item in arr {
        let obj = item
            .as_object()
            .ok_or("--tasks argument must be a list of maps from string to string when decoded.")?;
        let mut map = HashMap::new();
        for (k, v) in obj {
            let s = match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                _ => return Err(
                    "--tasks argument must be a list of maps from string to string when decoded."
                        .into(),
                ),
            };
            map.insert(k.clone(), s);
        }
        result.push(map);
    }
    Ok(result)
}

/// Coerce string task parameter values to the types declared in the parameter space.
pub fn coerce_task_params(
    params: &HashMap<String, String>,
    space: &openjd_model::job::StepParameterSpace,
) -> Result<openjd_model::types::TaskParameterSet, Box<dyn std::error::Error>> {
    use openjd_model::job::TaskParameter;
    use openjd_model::types::{TaskParameterType, TaskParameterValue};

    let mut result = openjd_model::types::TaskParameterSet::new();
    let mut errors = Vec::new();

    for name in params.keys() {
        if !space.task_parameter_definitions.contains_key(name) {
            errors.push(format!("Unknown task parameter '{name}'."));
        }
    }
    for name in space.task_parameter_definitions.keys() {
        if !params.contains_key(name) {
            errors.push(format!("Missing task parameter '{name}'."));
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("\n").into());
    }

    for (name, value) in params {
        let tp = &space.task_parameter_definitions[name];
        let (param_type, expr_value) = match tp {
            TaskParameter::Int { .. } => {
                let v: i64 = value
                    .parse()
                    .map_err(|_| format!("Task parameter '{name}': expected INT, got '{value}'"))?;
                (TaskParameterType::Int, openjd_expr::ExprValue::Int(v))
            }
            TaskParameter::Float { .. } => {
                let v: f64 = value.parse().map_err(|_| {
                    format!("Task parameter '{name}': expected FLOAT, got '{value}'")
                })?;
                if !v.is_finite() {
                    return Err(format!(
                        "Task parameter '{name}': value must be finite, got '{value}'"
                    )
                    .into());
                }
                let float = openjd_expr::value::Float64::new(v).map_err(|e| {
                    format!("Task parameter '{name}': invalid float '{value}': {e}")
                })?;
                (
                    TaskParameterType::Float,
                    openjd_expr::ExprValue::Float(float),
                )
            }
            TaskParameter::String { .. } => (
                TaskParameterType::String,
                openjd_expr::ExprValue::String(value.clone()),
            ),
            TaskParameter::Path { .. } => (
                TaskParameterType::Path,
                openjd_expr::ExprValue::new_path(value.clone(), openjd_expr::PathFormat::host()),
            ),
            TaskParameter::ChunkInt { .. } => {
                let r: openjd_expr::RangeExpr =
                    value.parse().map_err(|e: openjd_expr::ExpressionError| {
                        format!("Task parameter '{name}': invalid range expression '{value}': {e}")
                    })?;
                (
                    TaskParameterType::ChunkInt,
                    openjd_expr::ExprValue::RangeExpr(r),
                )
            }
        };
        result.insert(
            name.clone(),
            TaskParameterValue {
                param_type,
                value: expr_value,
            },
        );
    }
    Ok(result)
}

/// Load path mapping rules from a file or inline JSON string.
pub fn load_path_mapping_rules(
    path: &Option<String>,
) -> Result<Vec<PathMappingRule>, Box<dyn std::error::Error>> {
    let Some(path_str) = path else {
        return Ok(Vec::new());
    };
    let path_str = path_str.trim();

    let content = if path_str.starts_with("file://") {
        let file_path = std::path::Path::new(path_str.trim_start_matches("file://"));
        read_input_file_with(
            file_path,
            Some(MAX_FILE_INPUT_SIZE),
            || {
                format!(
                    "Path mapping rules file '{}' does not exist.",
                    file_path.display()
                )
            },
            || {
                format!(
                    "Cannot read path mapping rules file '{}'",
                    file_path.display()
                )
            },
        )?
    } else {
        path_str.to_string()
    };

    let doc: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Path mapping rules must be valid JSON: {e}"))?;

    if !doc.is_object() {
        return Err(
            "Path mapping rules must be an object with 'version' and 'path_mapping_rules' fields"
                .into(),
        );
    }
    match doc.get("version").and_then(|v| v.as_str()) {
        Some("pathmapping-1.0") => {}
        _ => {
            return Err(
                "Path mapping rules must have a 'version' value of 'pathmapping-1.0'".into(),
            )
        }
    }
    let rules = doc
        .get("path_mapping_rules")
        .and_then(|v| v.as_array())
        .ok_or("Path mapping rules must contain a list named 'path_mapping_rules'")?;
    rules
        .iter()
        .map(|r| serde_json::from_value(r.clone()).map_err(|e| e.into()))
        .collect()
}
