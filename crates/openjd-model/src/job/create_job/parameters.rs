// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Parameter merging, preprocessing, and coercion.
//!
//! Mirrors Python `_merge_job_parameter.py` and the parameter preprocessing
//! portion of `_create_job.py`.

use indexmap::IndexMap;
use openjd_expr::symbol_table::SymbolTable;

use crate::error::OpenJdError;
use crate::template::{EnvironmentTemplate, JobParameterDefinition, JobTemplate};
use crate::types::{
    DataFlow, JobParameterInputValues, JobParameterType, JobParameterValue, JobParameterValues,
    ObjectType,
};

/// Merge parameter definitions from environment templates and the job template.
///
/// Per §1.2.1: environment templates are processed in order, job template last.
/// Type conflicts are errors. Defaults come from the last template that defines one.
pub fn merge_job_parameter_definitions(
    job_template: &JobTemplate,
    environment_templates: &[EnvironmentTemplate],
) -> Result<Vec<MergedParameterDefinition>, OpenJdError> {
    let mut merged: IndexMap<String, MergedParameterDefinition> = IndexMap::new();

    // Helper: process one parameter definition into the merged map.
    let mut process_param = |p: &JobParameterDefinition, source: &str| -> Result<(), OpenJdError> {
        let name = p.name().to_string();
        if let Some(existing) = merged.get(&name) {
            if existing.param_type != p.job_param_type() {
                return Err(OpenJdError::Compatibility(format!(
                    "Parameter '{name}' has conflicting types: '{}' in {} and '{}' in {source}",
                    existing.param_type,
                    existing.source,
                    p.type_name()
                )));
            }
            if existing.param_type == JobParameterType::Path {
                let (new_ot, new_df) = p.path_properties();
                if let Some(ot) = new_ot {
                    if let Some(eot) = existing.object_type {
                        if eot != ot {
                            return Err(OpenJdError::Compatibility(format!(
                                "Parameter '{name}' has conflicting objectType: '{eot}' in {} and '{ot}' in {source}",
                                existing.source
                            )));
                        }
                    }
                }
                if let Some(df) = new_df {
                    if let Some(edf) = existing.data_flow {
                        if edf != df {
                            return Err(OpenJdError::Compatibility(format!(
                                "Parameter '{name}' has conflicting dataFlow: '{edf}' in {} and '{df}' in {source}",
                                existing.source
                            )));
                        }
                    }
                }
            }
        }
        let default = p.default_value();
        let (ot, df) = p.path_properties();
        let src = source.to_string();
        merged
            .entry(name.clone())
            .and_modify(|m| {
                if let Some(d) = &default {
                    m.default = Some(d.clone());
                }
                if let Some(v) = ot {
                    m.object_type = Some(v);
                }
                if let Some(v) = df {
                    m.data_flow = Some(v);
                }
                m.source = src.clone();
                m.merge_constraints(p);
            })
            .or_insert_with(|| {
                let mut m = MergedParameterDefinition {
                    name: name.clone(),
                    param_type: p.job_param_type(),
                    default,
                    object_type: ot,
                    data_flow: df,
                    source: src,
                    min_value_i64: None,
                    max_value_i64: None,
                    min_value_f64: None,
                    max_value_f64: None,
                    min_length: None,
                    max_length: None,
                    allowed_values_int: None,
                    allowed_values_float: None,
                    allowed_values_str: None,
                };
                m.merge_constraints(p);
                m
            });
        Ok(())
    };

    // Process env templates first (in order), job template last
    for et in environment_templates {
        let source = format!("EnvironmentTemplate '{}'", et.environment.name);
        if let Some(params) = &et.parameter_definitions {
            for p in params {
                process_param(p, &source)?;
            }
        }
    }
    let source = "JobTemplate".to_string();
    for p in job_template.parameter_definitions_list() {
        process_param(p, &source)?;
    }

    Ok(merged.into_values().collect())
}

/// A merged parameter definition from multiple templates.
///
/// Constraints are tightened per §1.2.1: allowedValues are intersected,
/// min values take the maximum, max values take the minimum.
#[derive(Debug, Clone)]
pub struct MergedParameterDefinition {
    pub name: String,
    pub param_type: JobParameterType,
    pub default: Option<String>,
    pub object_type: Option<ObjectType>,
    pub data_flow: Option<DataFlow>,
    /// Which template last defined/contributed to this parameter.
    pub source: String,
    // Merged constraints (tightened across all templates)
    pub(crate) min_value_i64: Option<i64>,
    pub(crate) max_value_i64: Option<i64>,
    pub(crate) min_value_f64: Option<f64>,
    pub(crate) max_value_f64: Option<f64>,
    pub(crate) min_length: Option<usize>,
    pub(crate) max_length: Option<usize>,
    pub(crate) allowed_values_int: Option<Vec<i64>>,
    pub(crate) allowed_values_float: Option<Vec<f64>>,
    pub(crate) allowed_values_str: Option<Vec<String>>,
}

impl MergedParameterDefinition {
    /// Merge constraints from a parameter definition into this merged definition.
    fn merge_constraints(&mut self, def: &JobParameterDefinition) {
        if let Some(v) = def.min_value_i64() {
            self.min_value_i64 = Some(self.min_value_i64.map_or(v, |cur| cur.max(v)));
        }
        if let Some(v) = def.max_value_i64() {
            self.max_value_i64 = Some(self.max_value_i64.map_or(v, |cur| cur.min(v)));
        }
        if let Some(v) = def.min_value_f64() {
            self.min_value_f64 = Some(self.min_value_f64.map_or(v, |cur| cur.max(v)));
        }
        if let Some(v) = def.max_value_f64() {
            self.max_value_f64 = Some(self.max_value_f64.map_or(v, |cur| cur.min(v)));
        }
        if let Some(v) = def.min_length() {
            self.min_length = Some(self.min_length.map_or(v, |cur| cur.max(v)));
        }
        if let Some(v) = def.max_length() {
            self.max_length = Some(self.max_length.map_or(v, |cur| cur.min(v)));
        }
        // allowedValues: intersect
        if let Some(new_vals) = def.allowed_values_strings() {
            let new_set: std::collections::HashSet<String> = new_vals.into_iter().collect();
            self.allowed_values_str = Some(match self.allowed_values_str.take() {
                Some(cur) => cur.into_iter().filter(|v| new_set.contains(v)).collect(),
                None => new_set.into_iter().collect(),
            });
        }
        if let Some(new_vals) = def.allowed_values_i64() {
            let new_set: std::collections::HashSet<i64> = new_vals.into_iter().collect();
            self.allowed_values_int = Some(match self.allowed_values_int.take() {
                Some(cur) => cur.into_iter().filter(|v| new_set.contains(v)).collect(),
                None => new_set.into_iter().collect(),
            });
        }
        if let Some(new_vals) = def.allowed_values_f64() {
            let new_bits: std::collections::HashSet<u64> =
                new_vals.iter().map(|f| f.to_bits()).collect();
            self.allowed_values_float = Some(match self.allowed_values_float.take() {
                Some(cur) => cur
                    .into_iter()
                    .filter(|v| new_bits.contains(&v.to_bits()))
                    .collect(),
                None => new_vals,
            });
        }
    }

    /// Validate that the merged constraints are satisfiable (§1.2.1).
    pub fn validate_satisfiable(&self) -> Result<(), OpenJdError> {
        if let (Some(min), Some(max)) = (self.min_value_i64, self.max_value_i64) {
            if min > max {
                return Err(OpenJdError::Compatibility(format!(
                    "Parameter '{}': merged INT constraints have no valid range (min {min} > max {max})", self.name)));
            }
        }
        if let (Some(min), Some(max)) = (self.min_value_f64, self.max_value_f64) {
            if min > max {
                return Err(OpenJdError::Compatibility(format!(
                    "Parameter '{}': merged FLOAT constraints have no valid range (min {min} > max {max})", self.name)));
            }
        }
        if let (Some(min), Some(max)) = (self.min_length, self.max_length) {
            if min > max {
                return Err(OpenJdError::Compatibility(format!(
                    "Parameter '{}': merged {} constraints have no valid length (minLength {min} > maxLength {max})",
                    self.name, self.param_type)));
            }
        }
        if let Some(allowed) = &self.allowed_values_str {
            if allowed.is_empty() {
                return Err(OpenJdError::Compatibility(format!(
                    "Parameter '{}': merged {} allowedValues have no common values",
                    self.name, self.param_type
                )));
            }
            if let Some(def) = &self.default {
                if !allowed.iter().any(|a| a == def) {
                    return Err(OpenJdError::Compatibility(format!(
                        "Parameter '{}': default '{}' not in merged allowedValues",
                        self.name, def
                    )));
                }
            }
        }
        if let Some(allowed) = &self.allowed_values_int {
            if allowed.is_empty() {
                return Err(OpenJdError::Compatibility(format!(
                    "Parameter '{}': merged INT allowedValues have no common values",
                    self.name
                )));
            }
        }
        if let Some(allowed) = &self.allowed_values_float {
            if allowed.is_empty() {
                return Err(OpenJdError::Compatibility(format!(
                    "Parameter '{}': merged FLOAT allowedValues have no common values",
                    self.name
                )));
            }
        }
        Ok(())
    }

    /// Check a coerced value against the merged constraints.
    pub fn check_constraints(&self, value: &openjd_expr::ExprValue) -> Result<(), OpenJdError> {
        match value {
            openjd_expr::ExprValue::Int(v) => {
                if let Some(min) = self.min_value_i64 {
                    if *v < min {
                        return Err(OpenJdError::DecodeValidation(format!(
                            "Parameter '{}': value {v} is less than minimum {min}",
                            self.name
                        )));
                    }
                }
                if let Some(max) = self.max_value_i64 {
                    if *v > max {
                        return Err(OpenJdError::DecodeValidation(format!(
                            "Parameter '{}': value {v} exceeds maximum {max}",
                            self.name
                        )));
                    }
                }
                if let Some(allowed) = &self.allowed_values_int {
                    if !allowed.contains(v) {
                        return Err(OpenJdError::DecodeValidation(format!(
                            "Parameter '{}': value {v} is not in allowed values",
                            self.name
                        )));
                    }
                }
            }
            openjd_expr::ExprValue::Float(v) => {
                let f = v.value();
                if let Some(min) = self.min_value_f64 {
                    if f < min {
                        return Err(OpenJdError::DecodeValidation(format!(
                            "Parameter '{}': value {f} is less than minimum {min}",
                            self.name
                        )));
                    }
                }
                if let Some(max) = self.max_value_f64 {
                    if f > max {
                        return Err(OpenJdError::DecodeValidation(format!(
                            "Parameter '{}': value {f} exceeds maximum {max}",
                            self.name
                        )));
                    }
                }
                if let Some(allowed) = &self.allowed_values_float {
                    if !allowed.contains(&f) {
                        return Err(OpenJdError::DecodeValidation(format!(
                            "Parameter '{}': value {f} is not in allowed values",
                            self.name
                        )));
                    }
                }
            }
            openjd_expr::ExprValue::String(v) | openjd_expr::ExprValue::Path { value: v, .. } => {
                if let Some(min) = self.min_length {
                    if v.len() < min {
                        return Err(OpenJdError::DecodeValidation(format!(
                            "Parameter '{}': value length {} is less than minimum {min}",
                            self.name,
                            v.len()
                        )));
                    }
                }
                if let Some(max) = self.max_length {
                    if v.len() > max {
                        return Err(OpenJdError::DecodeValidation(format!(
                            "Parameter '{}': value length {} exceeds maximum {max}",
                            self.name,
                            v.len()
                        )));
                    }
                }
                if let Some(allowed) = &self.allowed_values_str {
                    if !allowed.iter().any(|a| a == v) {
                        return Err(OpenJdError::DecodeValidation(format!(
                            "Parameter '{}': value '{}' is not in allowed values",
                            self.name, v
                        )));
                    }
                }
            }
            _ => {} // BOOL, RANGE_EXPR, LIST types — no cross-template mergeable constraints
        }
        Ok(())
    }
}

/// Coerce an `ExprValue` to the target `JobParameterType`.
pub(super) fn coerce_to_type(
    value: &openjd_expr::ExprValue,
    param_type: JobParameterType,
) -> Result<openjd_expr::ExprValue, String> {
    use openjd_expr::ExprValue;

    if value_matches_type(value, param_type) {
        return Ok(value.clone());
    }

    match (value, param_type) {
        (ExprValue::Int(i), JobParameterType::Float) => {
            return Ok(ExprValue::Float(
                openjd_expr::value::Float64::new(*i as f64).unwrap(),
            ));
        }
        (ExprValue::Float(f), JobParameterType::Int) => {
            let v = f.value();
            if v.fract() == 0.0 {
                return Ok(ExprValue::Int(v as i64));
            }
        }
        _ => {}
    }

    let s = match value {
        ExprValue::String(s) => s.as_str(),
        ExprValue::Int(i) => {
            return coerce_from_str(&i.to_string(), param_type);
        }
        ExprValue::Float(f) => {
            return coerce_from_str(&f.to_string(), param_type);
        }
        ExprValue::Bool(b) => {
            return coerce_from_str(if *b { "true" } else { "false" }, param_type);
        }
        other => {
            return Err(format!(
                "Cannot coerce {} to {}",
                other.type_name(),
                param_type.as_spec_str()
            ));
        }
    };
    coerce_from_str(s, param_type)
}

/// Coerce a string value to the target type.
pub(super) fn coerce_from_str(
    s: &str,
    param_type: JobParameterType,
) -> Result<openjd_expr::ExprValue, String> {
    use openjd_expr::ExprValue;
    Ok(match param_type {
        JobParameterType::Int => s
            .parse::<i64>()
            .map(ExprValue::Int)
            .map_err(|_| format!("Value '{s}' is not a valid integer or integer string."))?,
        JobParameterType::Float => s
            .parse::<f64>()
            .map(|f| {
                ExprValue::Float(openjd_expr::value::Float64::with_str(f, s.to_string()).unwrap())
            })
            .map_err(|_| format!("Value '{s}' is not a valid float."))?,
        JobParameterType::Bool => match s.to_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => ExprValue::Bool(true),
            "false" | "no" | "off" | "0" => ExprValue::Bool(false),
            _ => {
                return Err(format!(
                    "Value '{}' is not a valid boolean. Accepted: true/false, 1/0, yes/no, on/off.",
                    s
                ))
            }
        },
        JobParameterType::RangeExpr => match s.parse::<openjd_expr::RangeExpr>() {
            Ok(r) => ExprValue::RangeExpr(r),
            Err(e) => return Err(format!("Value '{s}' is not a valid range expression: {e}")),
        },
        JobParameterType::Path | JobParameterType::String => ExprValue::String(s.to_string()),
        JobParameterType::ListString
        | JobParameterType::ListInt
        | JobParameterType::ListFloat
        | JobParameterType::ListPath
        | JobParameterType::ListBool
        | JobParameterType::ListListInt => {
            // Try parsing the string as JSON for list parameter coercion.
            if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(s) {
                json_to_expr_value(&json_val)
            } else {
                return Err(format!(
                    "Value '{s}' is not valid JSON for a list parameter."
                ));
            }
        }
    })
}

fn value_matches_type(value: &openjd_expr::ExprValue, param_type: JobParameterType) -> bool {
    use openjd_expr::ExprValue;
    matches!(
        (value, param_type),
        (
            ExprValue::String(_),
            JobParameterType::String | JobParameterType::Path
        ) | (ExprValue::Int(_), JobParameterType::Int)
            | (ExprValue::Float(_), JobParameterType::Float)
            | (ExprValue::Bool(_), JobParameterType::Bool)
            | (ExprValue::RangeExpr(_), JobParameterType::RangeExpr)
            | (
                ExprValue::ListString(_, _),
                JobParameterType::ListString | JobParameterType::ListPath
            )
            | (ExprValue::ListInt(_), JobParameterType::ListInt)
            | (ExprValue::ListFloat(_), JobParameterType::ListFloat)
            | (ExprValue::ListBool(_), JobParameterType::ListBool)
            | (ExprValue::ListList(_, _, _), JobParameterType::ListListInt)
    )
}

/// Options controlling how PATH parameters are resolved in [`preprocess_job_parameters`].
pub struct PathParameterOptions<'a> {
    /// Directory containing the job template. Relative PATH defaults are joined to this.
    pub job_template_dir: &'a std::path::Path,
    /// Current working directory. Relative PATH user values are joined to this.
    pub current_working_dir: &'a std::path::Path,
    /// How path strings are interpreted for absolute/relative checks.
    /// Use `PathFormat::host()` for local filesystem paths, or `PathFormat::Posix` /
    /// `PathFormat::Windows` when paths originate from a known platform.
    pub path_format: openjd_expr::path_mapping::PathFormat,
    /// If `false`, PATH defaults must be relative and within `job_template_dir`.
    /// If `true`, absolute defaults and `..` walk-up are permitted.
    pub allow_template_dir_walk_up: bool,
    /// If `true`, URI values (`scheme://...`) in PATH parameters are preserved as-is
    /// (requires EXPR extension). If `false` with EXPR, URIs are rejected with an error.
    /// Without EXPR, this flag is ignored — URIs are treated as opaque relative strings.
    pub allow_uri_path_values: bool,
}

impl<'a> PathParameterOptions<'a> {
    /// Create options with sensible defaults: host path format, no walk-up, no URIs.
    pub fn new(
        job_template_dir: &'a std::path::Path,
        current_working_dir: &'a std::path::Path,
    ) -> Self {
        Self {
            job_template_dir,
            current_working_dir,
            path_format: openjd_expr::path_mapping::PathFormat::host(),
            allow_template_dir_walk_up: false,
            allow_uri_path_values: false,
        }
    }
}

/// Preprocess job parameters: validate inputs, fill defaults, check constraints.
pub fn preprocess_job_parameters(
    job_template: &JobTemplate,
    input_values: &JobParameterInputValues,
    environment_templates: &[EnvironmentTemplate],
    path_options: &PathParameterOptions<'_>,
) -> Result<JobParameterValues, OpenJdError> {
    let job_template_dir = path_options.job_template_dir;
    let current_working_dir = path_options.current_working_dir;
    let path_format = path_options.path_format;
    let allow_job_template_dir_walk_up = path_options.allow_template_dir_walk_up;
    let allow_uri_path_values = path_options.allow_uri_path_values;

    if !allow_job_template_dir_walk_up
        && !is_absolute_for_format(&job_template_dir.to_string_lossy(), path_format)
    {
        return Err(OpenJdError::DecodeValidation(format!(
            "The value supplied for the job template dir, {}, is not an absolute path. \
             It must be absolute to enforce that PATH parameter defaults are always inside the job template dir.",
            job_template_dir.display()
        )));
    }

    let merged = merge_job_parameter_definitions(job_template, environment_templates)?;

    for param in &merged {
        param.validate_satisfiable()?;
    }

    for key in input_values.keys() {
        if !merged.iter().any(|p| p.name == *key) {
            return Err(OpenJdError::DecodeValidation(format!(
                "Job parameter values provided for parameters that are not defined in the template: {key}"
            )));
        }
    }

    let mut result = JobParameterValues::new();
    let mut missing = Vec::new();

    let has_expr = job_template
        .extensions
        .as_ref()
        .is_some_and(|exts| exts.iter().any(|e| e.as_str() == "EXPR"));

    for param in &merged {
        let param_type = param.param_type;
        if let Some(input_val) = input_values.get(&param.name) {
            let coerced = if param.param_type == JobParameterType::Path {
                let s = input_val.as_str_repr();
                if !s.is_empty() && has_expr && openjd_expr::uri_path::is_uri(&s) {
                    // EXPR extension: URI handling depends on allow_uri_path_values
                    if !allow_uri_path_values {
                        return Err(OpenJdError::DecodeValidation(format!(
                            "Parameter '{}': URI path values are not permitted. Got '{}'",
                            param.name, s
                        )));
                    }
                    input_val.clone()
                } else if !(s.is_empty() || is_absolute_for_format_no_uri(&s, path_format)) {
                    // We already know s is not absolute (URI-unaware check).
                    // Use join_for_format for root-relative handling, but the value
                    // won't be recognized as a URI by join since left (cwd) isn't a URI
                    // and right was already checked. However, path::join's is_absolute
                    // still recognizes scheme:// in right. So we do a direct concat
                    // using the format-appropriate separator.
                    let cwd_str = current_working_dir.to_string_lossy();
                    openjd_expr::ExprValue::String(openjd_expr::functions::path::non_uri_join(
                        &cwd_str,
                        &s,
                        path_format,
                    ))
                } else {
                    input_val.clone()
                }
            } else {
                input_val.clone()
            };
            let expr_value = coerce_to_type(&coerced, param_type).map_err(|e| {
                OpenJdError::DecodeValidation(format!("Parameter '{}': {e}", param.name))
            })?;
            param.check_constraints(&expr_value)?;
            result.insert(
                param.name.clone(),
                JobParameterValue {
                    param_type,
                    value: expr_value,
                },
            );
        } else if let Some(default) = &param.default {
            let value_str = if param.param_type == JobParameterType::Path && !default.is_empty() {
                if has_expr && allow_uri_path_values && openjd_expr::uri_path::is_uri(default) {
                    // EXPR + allow: URI preserved as-is
                    default.clone()
                } else if has_expr
                    && !allow_uri_path_values
                    && openjd_expr::uri_path::is_uri(default)
                {
                    return Err(OpenJdError::DecodeValidation(format!(
                        "Parameter '{}': URI path values are not permitted in defaults. Got '{}'",
                        param.name, default
                    )));
                } else if is_absolute_for_format_no_uri(default, path_format) {
                    if !allow_job_template_dir_walk_up {
                        return Err(OpenJdError::DecodeValidation(format!(
                            "The default value of PATH parameter {} is an absolute path. Default paths must be relative, and are joined to the job template's directory.",
                            param.name
                        )));
                    }
                    default.clone()
                } else if !allow_job_template_dir_walk_up
                    && is_absolute_for_format(&job_template_dir.to_string_lossy(), path_format)
                {
                    let joined =
                        join_for_format(&job_template_dir.to_string_lossy(), default, path_format);
                    let normalized = normalize_path_str(&joined, path_format);
                    let normalized_dir =
                        normalize_path_str(&job_template_dir.to_string_lossy(), path_format);
                    if !normalized.starts_with(&normalized_dir) {
                        return Err(OpenJdError::DecodeValidation(format!(
                            "The default value of PATH parameter {} references a path outside of the template directory. Walking up from the template directory is not permitted.",
                            param.name
                        )));
                    }
                    normalized
                } else if is_absolute_for_format(&job_template_dir.to_string_lossy(), path_format) {
                    join_for_format(&job_template_dir.to_string_lossy(), default, path_format)
                } else {
                    default.clone()
                }
            } else {
                default.clone()
            };
            let expr_value = coerce_from_str(&value_str, param_type).map_err(|e| {
                OpenJdError::DecodeValidation(format!("Parameter '{}': {e}", param.name))
            })?;
            result.insert(
                param.name.clone(),
                JobParameterValue {
                    param_type,
                    value: expr_value,
                },
            );
        } else {
            missing.push(param.name.clone());
        }
    }

    if !missing.is_empty() {
        missing.sort();
        return Err(OpenJdError::DecodeValidation(format!(
            "Values missing for required job parameters: {}",
            missing.join(", ")
        )));
    }

    Ok(result)
}

/// Check whether a path string is absolute according to the given path format.
///
/// Delegates to `openjd_expr::functions::path::is_absolute` which handles
/// URIs (`scheme://...`), UNC paths (`\\server`), POSIX (`/...`), and
/// Windows drive letters (`C:\...`).
fn is_absolute_for_format(s: &str, format: openjd_expr::path_mapping::PathFormat) -> bool {
    openjd_expr::functions::path::is_absolute(s, format)
}

/// Like `is_absolute_for_format` but does NOT recognize URIs as absolute.
/// Used for PATH parameter values when EXPR is not enabled — URIs should be
/// treated as opaque relative strings.
fn is_absolute_for_format_no_uri(s: &str, format: openjd_expr::path_mapping::PathFormat) -> bool {
    if openjd_expr::uri_path::is_uri(s) {
        return false;
    }
    openjd_expr::functions::path::is_absolute(s, format)
}

/// Join two path strings using the separator and absoluteness rules for `format`.
fn join_for_format(
    base: &str,
    relative: &str,
    format: openjd_expr::path_mapping::PathFormat,
) -> String {
    openjd_expr::functions::path::join(base, relative, format)
}

/// Normalize a path string by resolving `.` and `..` components.
/// Uses string-based logic so it works correctly regardless of host OS.
fn normalize_path_str(path: &str, format: openjd_expr::path_mapping::PathFormat) -> String {
    use openjd_expr::path_mapping::PathFormat;
    let sep = match format {
        PathFormat::Windows => '\\',
        PathFormat::Posix | PathFormat::Uri => '/',
    };

    // Detect and preserve the root prefix
    let (root, rest) = if path.len() >= 3
        && path.as_bytes()[0].is_ascii_alphabetic()
        && path.as_bytes()[1] == b':'
        && (path.as_bytes()[2] == b'\\' || path.as_bytes()[2] == b'/')
    {
        // Windows drive root: "C:\" or "C:/"
        let root = format!("{}:{sep}", path.chars().next().unwrap());
        (root, &path[3..])
    } else if path.starts_with("\\\\") || path.starts_with("//") {
        // UNC path
        (format!("{sep}{sep}"), &path[2..])
    } else if path.starts_with('/') || path.starts_with('\\') {
        (sep.to_string(), &path[1..])
    } else {
        (String::new(), path)
    };

    let mut components: Vec<&str> = Vec::new();
    for part in rest.split(['/', '\\']) {
        match part {
            ".." => {
                components.pop();
            }
            "." | "" => {}
            _ => components.push(part),
        }
    }
    format!("{root}{}", components.join(&sep.to_string()))
}

/// Convert a serde_json::Value to an ExprValue.
pub(super) fn json_to_expr_value(val: &serde_json::Value) -> openjd_expr::ExprValue {
    match val {
        serde_json::Value::Null => openjd_expr::ExprValue::Null,
        serde_json::Value::Bool(b) => openjd_expr::ExprValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                openjd_expr::ExprValue::Int(i)
            } else if let Some(f) = n.as_f64() {
                openjd_expr::ExprValue::Float(openjd_expr::value::Float64::new(f).unwrap())
            } else {
                openjd_expr::ExprValue::String(n.to_string())
            }
        }
        serde_json::Value::String(s) => openjd_expr::ExprValue::String(s.clone()),
        serde_json::Value::Array(arr) => {
            let elements: Vec<openjd_expr::ExprValue> =
                arr.iter().map(json_to_expr_value).collect();
            // hint_type is only used by make_list for empty lists; for non-empty
            // lists it infers the type from elements (with int→float promotion etc.)
            openjd_expr::ExprValue::make_list(elements, openjd_expr::ExprType::NULLTYPE).unwrap()
        }
        serde_json::Value::Object(_) => openjd_expr::ExprValue::String(val.to_string()),
    }
}

/// Build a symbol table from processed job parameter values.
pub fn build_symbol_table(params: &JobParameterValues) -> Result<SymbolTable, OpenJdError> {
    let mut symtab = SymbolTable::new();
    for (name, pv) in params {
        // PATH and LIST[PATH] Param.* are excluded from the template-scope symtab.
        // They are host-context only (concrete values depend on session-time path mapping).
        let is_path = matches!(
            pv.param_type,
            JobParameterType::Path | JobParameterType::ListPath
        );
        if !is_path {
            symtab.set(&format!("Param.{name}"), pv.value.clone())?;
        }

        let raw_value = match pv.param_type {
            JobParameterType::Path => match &pv.value {
                openjd_expr::ExprValue::String(s) => openjd_expr::ExprValue::String(s.clone()),
                openjd_expr::ExprValue::Path { value, .. } => {
                    openjd_expr::ExprValue::String(value.clone())
                }
                _ => pv.value.clone(),
            },
            JobParameterType::ListPath => {
                if let openjd_expr::ExprValue::ListString(ref elements, _) = pv.value {
                    openjd_expr::ExprValue::ListString(elements.clone(), 0)
                } else if let openjd_expr::ExprValue::ListPath(ref elements, _, _) = pv.value {
                    openjd_expr::ExprValue::ListString(elements.clone(), 0)
                } else {
                    pv.value.clone()
                }
            }
            _ => pv.value.clone(),
        };
        symtab.set(&format!("RawParam.{name}"), raw_value)?;
    }
    Ok(symtab)
}
