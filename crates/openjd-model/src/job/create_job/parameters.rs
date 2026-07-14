// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Parameter merging, preprocessing, and coercion.
//!
//! Mirrors Python `_merge_job_parameter.py` and the parameter preprocessing
//! portion of `_create_job.py`.

use indexmap::IndexMap;
use openjd_expr::symbol_table::SymbolTable;

use crate::error::ModelError;
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
) -> Result<Vec<MergedParameterDefinition>, ModelError> {
    let mut merged: IndexMap<String, MergedParameterDefinition> = IndexMap::new();

    // Helper: process one parameter definition into the merged map.
    let mut process_param = |p: &JobParameterDefinition, source: &str| -> Result<(), ModelError> {
        let name = p.name().to_string();
        if let Some(existing) = merged.get(&name) {
            if existing.param_type != p.job_param_type() {
                return Err(ModelError::Compatibility(format!(
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
                            return Err(ModelError::Compatibility(format!(
                                "Parameter '{name}' has conflicting objectType: '{eot}' in {} and '{ot}' in {source}",
                                existing.source
                            )));
                        }
                    }
                }
                if let Some(df) = new_df {
                    if let Some(edf) = existing.data_flow {
                        if edf != df {
                            return Err(ModelError::Compatibility(format!(
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
                    item_min_value_i64: None,
                    item_max_value_i64: None,
                    item_allowed_values_int: None,
                    item_min_value_f64: None,
                    item_max_value_f64: None,
                    item_allowed_values_float: None,
                    item_min_length: None,
                    item_max_length: None,
                    item_allowed_values_str: None,
                    item_item_min_value_i64: None,
                    item_item_max_value_i64: None,
                    item_item_allowed_values_int: None,
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
    // List per-item constraints
    pub(crate) item_min_value_i64: Option<i64>,
    pub(crate) item_max_value_i64: Option<i64>,
    pub(crate) item_allowed_values_int: Option<Vec<i64>>,
    pub(crate) item_min_value_f64: Option<f64>,
    pub(crate) item_max_value_f64: Option<f64>,
    pub(crate) item_allowed_values_float: Option<Vec<f64>>,
    pub(crate) item_min_length: Option<usize>,
    pub(crate) item_max_length: Option<usize>,
    pub(crate) item_allowed_values_str: Option<Vec<String>>,
    pub(crate) item_item_min_value_i64: Option<i64>,
    pub(crate) item_item_max_value_i64: Option<i64>,
    pub(crate) item_item_allowed_values_int: Option<Vec<i64>>,
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

        // Note: list count constraints are merged via min_length()/max_length()
        // above, which now dispatches to list types as well as string types.
        if let Some(v) = def.item_min_value_i64() {
            self.item_min_value_i64 = Some(self.item_min_value_i64.map_or(v, |cur| cur.max(v)));
        }
        if let Some(v) = def.item_max_value_i64() {
            self.item_max_value_i64 = Some(self.item_max_value_i64.map_or(v, |cur| cur.min(v)));
        }
        if let Some(new_vals) = def.item_allowed_values_i64() {
            let new_set: std::collections::HashSet<i64> = new_vals.into_iter().collect();
            self.item_allowed_values_int = Some(match self.item_allowed_values_int.take() {
                Some(cur) => cur.into_iter().filter(|v| new_set.contains(v)).collect(),
                None => new_set.into_iter().collect(),
            });
        }
        if let Some(v) = def.item_min_value_f64() {
            self.item_min_value_f64 = Some(self.item_min_value_f64.map_or(v, |cur| cur.max(v)));
        }
        if let Some(v) = def.item_max_value_f64() {
            self.item_max_value_f64 = Some(self.item_max_value_f64.map_or(v, |cur| cur.min(v)));
        }
        if let Some(new_vals) = def.item_allowed_values_f64() {
            let new_bits: std::collections::HashSet<u64> =
                new_vals.iter().map(|f| f.to_bits()).collect();
            self.item_allowed_values_float = Some(match self.item_allowed_values_float.take() {
                Some(cur) => cur
                    .into_iter()
                    .filter(|v| new_bits.contains(&v.to_bits()))
                    .collect(),
                None => new_vals,
            });
        }
        if let Some(v) = def.item_min_length() {
            self.item_min_length = Some(self.item_min_length.map_or(v, |cur| cur.max(v)));
        }
        if let Some(v) = def.item_max_length() {
            self.item_max_length = Some(self.item_max_length.map_or(v, |cur| cur.min(v)));
        }
        if let Some(new_vals) = def.item_allowed_values_strings() {
            let new_set: std::collections::HashSet<String> = new_vals.into_iter().collect();
            self.item_allowed_values_str = Some(match self.item_allowed_values_str.take() {
                Some(cur) => cur.into_iter().filter(|v| new_set.contains(v)).collect(),
                None => new_set.into_iter().collect(),
            });
        }
        if let Some(v) = def.item_item_min_value_i64() {
            self.item_item_min_value_i64 =
                Some(self.item_item_min_value_i64.map_or(v, |cur| cur.max(v)));
        }
        if let Some(v) = def.item_item_max_value_i64() {
            self.item_item_max_value_i64 =
                Some(self.item_item_max_value_i64.map_or(v, |cur| cur.min(v)));
        }
        if let Some(new_vals) = def.item_item_allowed_values_i64() {
            let new_set: std::collections::HashSet<i64> = new_vals.into_iter().collect();
            self.item_item_allowed_values_int =
                Some(match self.item_item_allowed_values_int.take() {
                    Some(cur) => cur.into_iter().filter(|v| new_set.contains(v)).collect(),
                    None => new_set.into_iter().collect(),
                });
        }
    }

    /// Validate that the merged constraints are satisfiable (§1.2.1).
    pub fn validate_satisfiable(&self) -> Result<(), ModelError> {
        if let (Some(min), Some(max)) = (self.min_value_i64, self.max_value_i64) {
            if min > max {
                return Err(ModelError::Compatibility(format!(
                    "Parameter '{}': merged INT constraints have no valid range (min {min} > max {max})", self.name)));
            }
        }
        if let (Some(min), Some(max)) = (self.min_value_f64, self.max_value_f64) {
            if min > max {
                return Err(ModelError::Compatibility(format!(
                    "Parameter '{}': merged FLOAT constraints have no valid range (min {min} > max {max})", self.name)));
            }
        }
        if let (Some(min), Some(max)) = (self.min_length, self.max_length) {
            if min > max {
                return Err(ModelError::Compatibility(format!(
                    "Parameter '{}': merged {} constraints have no valid length (minLength {min} > maxLength {max})",
                    self.name, self.param_type)));
            }
        }
        if let Some(allowed) = &self.allowed_values_str {
            if allowed.is_empty() {
                return Err(ModelError::Compatibility(format!(
                    "Parameter '{}': merged {} allowedValues have no common values",
                    self.name, self.param_type
                )));
            }
            if let Some(def) = &self.default {
                if !allowed.iter().any(|a| a == def) {
                    return Err(ModelError::Compatibility(format!(
                        "Parameter '{}': default '{}' not in merged allowedValues",
                        self.name, def
                    )));
                }
            }
        }
        if let Some(allowed) = &self.allowed_values_int {
            if allowed.is_empty() {
                return Err(ModelError::Compatibility(format!(
                    "Parameter '{}': merged INT allowedValues have no common values",
                    self.name
                )));
            }
        }
        if let Some(allowed) = &self.allowed_values_float {
            if allowed.is_empty() {
                return Err(ModelError::Compatibility(format!(
                    "Parameter '{}': merged FLOAT allowedValues have no common values",
                    self.name
                )));
            }
        }
        Ok(())
    }

    /// Check a coerced value against the merged constraints.
    pub fn check_constraints(&self, value: &openjd_expr::ExprValue) -> Result<(), ModelError> {
        match value {
            openjd_expr::ExprValue::Int(v) => {
                if let Some(min) = self.min_value_i64 {
                    if *v < min {
                        return Err(ModelError::DecodeValidation(format!(
                            "Parameter '{}': value {v} is less than minimum {min}",
                            self.name
                        )));
                    }
                }
                if let Some(max) = self.max_value_i64 {
                    if *v > max {
                        return Err(ModelError::DecodeValidation(format!(
                            "Parameter '{}': value {v} exceeds maximum {max}",
                            self.name
                        )));
                    }
                }
                if let Some(allowed) = &self.allowed_values_int {
                    if !allowed.contains(v) {
                        return Err(ModelError::DecodeValidation(format!(
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
                        return Err(ModelError::DecodeValidation(format!(
                            "Parameter '{}': value {f} is less than minimum {min}",
                            self.name
                        )));
                    }
                }
                if let Some(max) = self.max_value_f64 {
                    if f > max {
                        return Err(ModelError::DecodeValidation(format!(
                            "Parameter '{}': value {f} exceeds maximum {max}",
                            self.name
                        )));
                    }
                }
                if let Some(allowed) = &self.allowed_values_float {
                    if !allowed.contains(&f) {
                        return Err(ModelError::DecodeValidation(format!(
                            "Parameter '{}': value {f} is not in allowed values",
                            self.name
                        )));
                    }
                }
            }
            openjd_expr::ExprValue::String(v) | openjd_expr::ExprValue::Path { value: v, .. } => {
                if let Some(min) = self.min_length {
                    if v.len() < min {
                        return Err(ModelError::DecodeValidation(format!(
                            "Parameter '{}': value length {} is less than minimum {min}",
                            self.name,
                            v.len()
                        )));
                    }
                }
                if let Some(max) = self.max_length {
                    if v.len() > max {
                        return Err(ModelError::DecodeValidation(format!(
                            "Parameter '{}': value length {} exceeds maximum {max}",
                            self.name,
                            v.len()
                        )));
                    }
                }
                if let Some(allowed) = &self.allowed_values_str {
                    if !allowed.iter().any(|a| a == v) {
                        return Err(ModelError::DecodeValidation(format!(
                            "Parameter '{}': value '{}' is not in allowed values",
                            self.name, v
                        )));
                    }
                }
            }
            openjd_expr::ExprValue::ListBool(items) => {
                if let Some(min) = self.min_length {
                    if items.len() < min {
                        return Err(ModelError::DecodeValidation(format!(
                            "Parameter '{}': list length {} is less than minimum {min}",
                            self.name,
                            items.len()
                        )));
                    }
                }
                if let Some(max) = self.max_length {
                    if items.len() > max {
                        return Err(ModelError::DecodeValidation(format!(
                            "Parameter '{}': list length {} exceeds maximum {max}",
                            self.name,
                            items.len()
                        )));
                    }
                }
            }
            openjd_expr::ExprValue::ListInt(items) => {
                if let Some(min) = self.min_length {
                    if items.len() < min {
                        return Err(ModelError::DecodeValidation(format!(
                            "Parameter '{}': list length {} is less than minimum {min}",
                            self.name,
                            items.len()
                        )));
                    }
                }
                if let Some(max) = self.max_length {
                    if items.len() > max {
                        return Err(ModelError::DecodeValidation(format!(
                            "Parameter '{}': list length {} exceeds maximum {max}",
                            self.name,
                            items.len()
                        )));
                    }
                }
                for (i, v) in items.iter().enumerate() {
                    if let Some(min) = self.item_min_value_i64 {
                        if *v < min {
                            return Err(ModelError::DecodeValidation(format!(
                                "Parameter '{}': item[{i}] value {v} is less than minimum {min}",
                                self.name
                            )));
                        }
                    }
                    if let Some(max) = self.item_max_value_i64 {
                        if *v > max {
                            return Err(ModelError::DecodeValidation(format!(
                                "Parameter '{}': item[{i}] value {v} exceeds maximum {max}",
                                self.name
                            )));
                        }
                    }
                    if let Some(allowed) = &self.item_allowed_values_int {
                        if !allowed.contains(v) {
                            return Err(ModelError::DecodeValidation(format!(
                                "Parameter '{}': item[{i}] value {v} is not in allowed values",
                                self.name
                            )));
                        }
                    }
                }
            }
            openjd_expr::ExprValue::ListFloat(items) => {
                if let Some(min) = self.min_length {
                    if items.len() < min {
                        return Err(ModelError::DecodeValidation(format!(
                            "Parameter '{}': list length {} is less than minimum {min}",
                            self.name,
                            items.len()
                        )));
                    }
                }
                if let Some(max) = self.max_length {
                    if items.len() > max {
                        return Err(ModelError::DecodeValidation(format!(
                            "Parameter '{}': list length {} exceeds maximum {max}",
                            self.name,
                            items.len()
                        )));
                    }
                }
                for (i, v) in items.iter().enumerate() {
                    let f = v.value();
                    if let Some(min) = self.item_min_value_f64 {
                        if f < min {
                            return Err(ModelError::DecodeValidation(format!(
                                "Parameter '{}': item[{i}] value {f} is less than minimum {min}",
                                self.name
                            )));
                        }
                    }
                    if let Some(max) = self.item_max_value_f64 {
                        if f > max {
                            return Err(ModelError::DecodeValidation(format!(
                                "Parameter '{}': item[{i}] value {f} exceeds maximum {max}",
                                self.name
                            )));
                        }
                    }
                    if let Some(allowed) = &self.item_allowed_values_float {
                        if !allowed.contains(&f) {
                            return Err(ModelError::DecodeValidation(format!(
                                "Parameter '{}': item[{i}] value {f} is not in allowed values",
                                self.name
                            )));
                        }
                    }
                }
            }
            openjd_expr::ExprValue::ListString(items, _)
            | openjd_expr::ExprValue::ListPath(items, _, _) => {
                if let Some(min) = self.min_length {
                    if items.len() < min {
                        return Err(ModelError::DecodeValidation(format!(
                            "Parameter '{}': list length {} is less than minimum {min}",
                            self.name,
                            items.len()
                        )));
                    }
                }
                if let Some(max) = self.max_length {
                    if items.len() > max {
                        return Err(ModelError::DecodeValidation(format!(
                            "Parameter '{}': list length {} exceeds maximum {max}",
                            self.name,
                            items.len()
                        )));
                    }
                }
                for (i, s) in items.iter().enumerate() {
                    let char_len = s.chars().count();
                    if let Some(min) = self.item_min_length {
                        if char_len < min {
                            return Err(ModelError::DecodeValidation(format!(
                                "Parameter '{}': item[{i}] length {char_len} is less than minimum {min}",
                                self.name
                            )));
                        }
                    }
                    if let Some(max) = self.item_max_length {
                        if char_len > max {
                            return Err(ModelError::DecodeValidation(format!(
                                "Parameter '{}': item[{i}] length {char_len} exceeds maximum {max}",
                                self.name
                            )));
                        }
                    }
                    if let Some(allowed) = &self.item_allowed_values_str {
                        if !allowed.iter().any(|a| a == s) {
                            return Err(ModelError::DecodeValidation(format!(
                                "Parameter '{}': item[{i}] value '{}' is not in allowed values",
                                self.name, s
                            )));
                        }
                    }
                }
            }
            openjd_expr::ExprValue::ListList(items, _, _) => {
                if let Some(min) = self.min_length {
                    if items.len() < min {
                        return Err(ModelError::DecodeValidation(format!(
                            "Parameter '{}': list length {} is less than minimum {min}",
                            self.name,
                            items.len()
                        )));
                    }
                }
                if let Some(max) = self.max_length {
                    if items.len() > max {
                        return Err(ModelError::DecodeValidation(format!(
                            "Parameter '{}': list length {} exceeds maximum {max}",
                            self.name,
                            items.len()
                        )));
                    }
                }
                for (i, inner) in items.iter().enumerate() {
                    if let openjd_expr::ExprValue::ListInt(ints) = inner {
                        if let Some(min) = self.item_min_length {
                            if ints.len() < min {
                                return Err(ModelError::DecodeValidation(format!(
                                    "Parameter '{}': item[{i}] length {} is less than minimum {min}",
                                    self.name,
                                    ints.len()
                                )));
                            }
                        }
                        if let Some(max) = self.item_max_length {
                            if ints.len() > max {
                                return Err(ModelError::DecodeValidation(format!(
                                    "Parameter '{}': item[{i}] length {} exceeds maximum {max}",
                                    self.name,
                                    ints.len()
                                )));
                            }
                        }
                        for (j, v) in ints.iter().enumerate() {
                            if let Some(min) = self.item_item_min_value_i64 {
                                if *v < min {
                                    return Err(ModelError::DecodeValidation(format!(
                                        "Parameter '{}': item[{i}][{j}] value {v} is less than minimum {min}",
                                        self.name
                                    )));
                                }
                            }
                            if let Some(max) = self.item_item_max_value_i64 {
                                if *v > max {
                                    return Err(ModelError::DecodeValidation(format!(
                                        "Parameter '{}': item[{i}][{j}] value {v} exceeds maximum {max}",
                                        self.name
                                    )));
                                }
                            }
                            if let Some(allowed) = &self.item_item_allowed_values_int {
                                if !allowed.contains(v) {
                                    return Err(ModelError::DecodeValidation(format!(
                                        "Parameter '{}': item[{i}][{j}] value {v} is not in allowed values",
                                        self.name
                                    )));
                                }
                            }
                        }
                    }
                }
            }
            _ => {} // BOOL, RANGE_EXPR — no cross-template mergeable constraints
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
                openjd_expr::value::Float64::new(*i as f64)
                    .map_err(|_| format!("Cannot represent integer {i} as a finite float"))?,
            ));
        }
        (ExprValue::Float(f), JobParameterType::Int) => {
            let v = f.value();
            if v.fract() == 0.0 && v >= i64::MIN as f64 && v < i64::MAX as f64 {
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
        JobParameterType::Float => {
            let f = s
                .parse::<f64>()
                .map_err(|_| format!("Value '{s}' is not a valid float."))?;
            ExprValue::Float(
                openjd_expr::value::Float64::with_str(f, s.to_string())
                    .map_err(|_| format!("Value '{s}' is not a valid float."))?,
            )
        }
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
                json_to_expr_value(&json_val)?
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
    pub job_template_dir: &'a str,
    /// Current working directory. Relative PATH user values are joined to this.
    pub current_working_dir: &'a str,
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
    pub fn new(job_template_dir: &'a str, current_working_dir: &'a str) -> Self {
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
///
/// Errors are accumulated rather than fail-fast: a single bad parameter
/// won't mask later problems. The returned `ModelError::DecodeValidation`
/// contains all collected messages joined by newlines, in this order:
///
/// 1. Per-parameter satisfiability errors (constraint conflicts after merging).
/// 2. Per-parameter input handling errors (bad coercion, constraint violations,
///    forbidden URI paths) and per-parameter default handling errors.
/// 3. A single "extra parameters" line for inputs that don't match any defined
///    parameter (Pythonish behavior: one line listing them all).
/// 4. A single "missing required" line for parameters with no value or default.
///
/// Preconditions that prevent any per-parameter work — a non-absolute
/// `job_template_dir` and structural failures from
/// [`merge_job_parameter_definitions`] — still fail fast.
pub fn preprocess_job_parameters(
    job_template: &JobTemplate,
    input_values: &JobParameterInputValues,
    environment_templates: &[EnvironmentTemplate],
    path_options: &PathParameterOptions<'_>,
) -> Result<JobParameterValues, ModelError> {
    let job_template_dir = path_options.job_template_dir;
    let current_working_dir = path_options.current_working_dir;
    let path_format = path_options.path_format;
    let allow_job_template_dir_walk_up = path_options.allow_template_dir_walk_up;
    let allow_uri_path_values = path_options.allow_uri_path_values;

    if !allow_job_template_dir_walk_up && !is_absolute_for_format(job_template_dir, path_format) {
        return Err(ModelError::DecodeValidation(format!(
            "The value supplied for the job template dir, {job_template_dir}, is not an absolute path. \
             It must be absolute to enforce that PATH parameter defaults are always inside the job template dir.",
        )));
    }

    let merged = merge_job_parameter_definitions(job_template, environment_templates)?;

    let mut errors: Vec<String> = Vec::new();

    for param in &merged {
        if let Err(e) = param.validate_satisfiable() {
            errors.push(model_err_message(e));
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
            let coerced_opt: Option<openjd_expr::ExprValue> =
                if param.param_type == JobParameterType::Path {
                    let s = input_val.as_str_repr();
                    if !s.is_empty() && has_expr && openjd_expr::uri_path::is_uri(&s) {
                        // EXPR extension: URI handling depends on allow_uri_path_values
                        if !allow_uri_path_values {
                            errors.push(format!(
                                "Parameter '{}': URI path values are not permitted. Got '{}'",
                                param.name, s
                            ));
                            None
                        } else {
                            Some(input_val.clone())
                        }
                    } else if !(s.is_empty() || is_absolute_for_format_no_uri(&s, path_format)) {
                        // Relative path: join with current_working_dir if non-empty.
                        if current_working_dir.is_empty() {
                            Some(input_val.clone())
                        } else {
                            Some(openjd_expr::ExprValue::String(
                                openjd_expr::functions::path::non_uri_join(
                                    current_working_dir,
                                    &s,
                                    path_format,
                                ),
                            ))
                        }
                    } else {
                        Some(input_val.clone())
                    }
                } else {
                    Some(input_val.clone())
                };
            let Some(coerced) = coerced_opt else { continue };
            match coerce_to_type(&coerced, param_type) {
                Ok(expr_value) => {
                    if let Err(e) = param.check_constraints(&expr_value) {
                        errors.push(model_err_message(e));
                    } else {
                        result.insert(
                            param.name.clone(),
                            JobParameterValue {
                                param_type,
                                value: expr_value,
                            },
                        );
                    }
                }
                Err(e) => {
                    errors.push(format!("Parameter '{}': {e}", param.name));
                }
            }
        } else if let Some(default) = &param.default {
            let value_str_opt: Option<String> = if param.param_type == JobParameterType::Path
                && !default.is_empty()
            {
                if has_expr && allow_uri_path_values && openjd_expr::uri_path::is_uri(default) {
                    // EXPR + allow: URI preserved as-is
                    Some(default.clone())
                } else if has_expr
                    && !allow_uri_path_values
                    && openjd_expr::uri_path::is_uri(default)
                {
                    errors.push(format!(
                        "Parameter '{}': URI path values are not permitted in defaults. Got '{}'",
                        param.name, default
                    ));
                    None
                } else if is_absolute_for_format_no_uri(default, path_format) {
                    if !allow_job_template_dir_walk_up {
                        errors.push(format!(
                            "The default value of PATH parameter {} is an absolute path. Default paths must be relative, and are joined to the job template's directory.",
                            param.name
                        ));
                        None
                    } else {
                        Some(default.clone())
                    }
                } else if !allow_job_template_dir_walk_up
                    && is_absolute_for_format(job_template_dir, path_format)
                {
                    let joined = join_for_format(job_template_dir, default, path_format);
                    let normalized = normalize_path_str(&joined, path_format);
                    let normalized_dir = normalize_path_str(job_template_dir, path_format);
                    if !path_is_within(&normalized, &normalized_dir, path_format) {
                        errors.push(format!(
                            "The default value of PATH parameter {} references a path outside of the template directory. Walking up from the template directory is not permitted.",
                            param.name
                        ));
                        None
                    } else {
                        Some(normalized)
                    }
                } else if is_absolute_for_format(job_template_dir, path_format) {
                    let joined = join_for_format(job_template_dir, default, path_format);
                    Some(normalize_path_str(&joined, path_format))
                } else {
                    Some(default.clone())
                }
            } else {
                Some(default.clone())
            };
            let Some(value_str) = value_str_opt else {
                continue;
            };
            match coerce_from_str(&value_str, param_type) {
                Ok(expr_value) => {
                    result.insert(
                        param.name.clone(),
                        JobParameterValue {
                            param_type,
                            value: expr_value,
                        },
                    );
                }
                Err(e) => errors.push(format!("Parameter '{}': {e}", param.name)),
            }
        } else {
            missing.push(param.name.clone());
        }
    }

    let mut extras: Vec<&str> = input_values
        .keys()
        .filter(|k| !merged.iter().any(|p| p.name == **k))
        .map(String::as_str)
        .collect();
    if !extras.is_empty() {
        extras.sort();
        errors.push(format!(
            "Job parameter values provided for parameters that are not defined in the template: {}",
            extras.join(", ")
        ));
    }

    if !missing.is_empty() {
        missing.sort();
        errors.push(format!(
            "Values missing for required job parameters: {}",
            missing.join(", ")
        ));
    }

    if !errors.is_empty() {
        return Err(ModelError::DecodeValidation(errors.join("\n")));
    }

    Ok(result)
}

/// Extract a flat string from a `ModelError`.
///
/// `validate_satisfiable` and `check_constraints` may return either
/// `DecodeValidation` or `Compatibility` variants; both carry their
/// human-readable text directly. For other variants we fall back to
/// `Display`, which is fine because those cases don't occur in this code
/// path today.
fn model_err_message(e: ModelError) -> String {
    match e {
        ModelError::DecodeValidation(msg) | ModelError::Compatibility(msg) => msg,
        other => other.to_string(),
    }
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
///
/// # Limitations
///
/// This is a **string-level** normalization only. It does **not** resolve
/// symlinks, so a path like `./symlink_to_parent/../secret` may normalize
/// to a location that appears to be within the base directory but actually
/// escapes it via the symlink target. Callers that use the result for
/// access-control checks (e.g. the `starts_with(normalized_dir)` guard in
/// `preprocess_job_parameters`) should be aware that filesystem-level
/// canonicalization (e.g. `std::fs::canonicalize`) is needed downstream
/// before performing actual file I/O to prevent symlink-based traversal.
fn normalize_path_str(path: &str, format: openjd_expr::path_mapping::PathFormat) -> String {
    use openjd_expr::path_mapping::PathFormat;
    let sep = match format {
        PathFormat::Windows => '\\',
        PathFormat::Posix | PathFormat::Uri => '/',
    };

    // Detect and preserve the root prefix
    let (root, rest, min_components) = if path.len() >= 3
        && path.as_bytes()[0].is_ascii_alphabetic()
        && path.as_bytes()[1] == b':'
        && (path.as_bytes()[2] == b'\\' || path.as_bytes()[2] == b'/')
    {
        // Windows drive root: "C:\" or "C:/"
        let root = format!("{}:{sep}", path.chars().next().unwrap());
        (root, &path[3..], 0)
    } else if path.starts_with("\\\\") || path.starts_with("//") {
        // UNC path — server and share components must be preserved
        (format!("{sep}{sep}"), &path[2..], 2)
    } else if path.starts_with('/') || path.starts_with('\\') {
        (sep.to_string(), &path[1..], 0)
    } else {
        (String::new(), path, 0)
    };

    let mut components: Vec<&str> = Vec::new();
    for part in rest.split(['/', '\\']) {
        match part {
            ".." => {
                if components.len() > min_components {
                    components.pop();
                }
            }
            "." | "" => {}
            _ => components.push(part),
        }
    }
    format!("{root}{}", components.join(&sep.to_string()))
}

/// Return `true` if `path` is `base` itself or a descendant of `base`.
///
/// Both arguments must already be normalized (see [`normalize_path_str`]).
/// This is a **path-component-aware** containment check, not a raw string
/// prefix check: `/a/job1` contains `/a/job1/sub` but does **not** contain the
/// sibling `/a/job10`. It matches the Python reference's use of
/// `Path.is_relative_to` and is what enforces the `allow_template_dir_walk_up`
/// restriction, so a plain `str::starts_with` here would let a default like
/// `../job1-sibling` escape into a sibling directory.
fn path_is_within(path: &str, base: &str, format: openjd_expr::path_mapping::PathFormat) -> bool {
    use openjd_expr::path_mapping::PathFormat;
    let sep = match format {
        PathFormat::Windows => '\\',
        PathFormat::Posix | PathFormat::Uri => '/',
    };
    // Exact match — the path resolves to the base directory itself.
    if path == base {
        return true;
    }
    // Descendant — `path` must start with `base` followed by a separator so
    // that only whole-component boundaries match. A `base` that already ends
    // in a separator (e.g. a filesystem root like "/" or "C:\") needs no extra
    // separator inserted.
    if base.ends_with(sep) {
        path.starts_with(base)
    } else {
        path.starts_with(&format!("{base}{sep}"))
    }
}

/// Convert a serde_json::Value to an ExprValue.
pub(super) fn json_to_expr_value(
    val: &serde_json::Value,
) -> Result<openjd_expr::ExprValue, String> {
    match val {
        serde_json::Value::Null => {
            Err("Unexpected null in parameter value. List elements must be strings, integers, floats, or booleans.".to_string())
        }
        serde_json::Value::Bool(b) => Ok(openjd_expr::ExprValue::Bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(openjd_expr::ExprValue::Int(i))
            } else if let Some(f) = n.as_f64() {
                openjd_expr::value::Float64::new(f)
                    .map(openjd_expr::ExprValue::Float)
                    .map_err(|_| format!("Float value {f} is not finite"))
            } else {
                Ok(openjd_expr::ExprValue::String(n.to_string()))
            }
        }
        serde_json::Value::String(s) => Ok(openjd_expr::ExprValue::String(s.clone())),
        serde_json::Value::Array(arr) => {
            let elements: Vec<openjd_expr::ExprValue> = arr
                .iter()
                .map(json_to_expr_value)
                .collect::<Result<_, _>>()?;
            openjd_expr::ExprValue::make_list(elements, openjd_expr::ExprType::NULLTYPE)
                .map_err(|e| format!("Invalid list value: {e}"))
        }
        serde_json::Value::Object(_) => {
            Err("Unexpected JSON object in parameter value. List elements must be strings, integers, floats, or booleans.".to_string())
        }
    }
}

/// Build a symbol table from processed job parameter values.
pub fn build_symbol_table(params: &JobParameterValues) -> Result<SymbolTable, ModelError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use openjd_expr::path_mapping::PathFormat;

    #[test]
    fn normalize_unc_dotdot_preserves_server_share() {
        // \\server\share\.. should not pop below the server\share components
        let result = normalize_path_str(r"\\server\share\..", PathFormat::Windows);
        assert_eq!(
            result, r"\\server\share",
            "UNC path should preserve server\\share: got {result}"
        );
    }

    #[test]
    fn normalize_unc_double_dotdot_preserves_server_share() {
        let result = normalize_path_str(r"\\server\share\a\..\..", PathFormat::Windows);
        assert_eq!(result, r"\\server\share", "got {result}");
    }

    #[test]
    fn normalize_unc_excessive_dotdot_preserves_server_share() {
        // More .. than components should clamp at server\share
        let result = normalize_path_str(r"\\server\share\..\..\..\..", PathFormat::Windows);
        assert_eq!(result, r"\\server\share", "got {result}");
    }

    #[test]
    fn path_is_within_accepts_descendant_and_self() {
        assert!(path_is_within("/a/job1", "/a/job1", PathFormat::Posix));
        assert!(path_is_within("/a/job1/sub", "/a/job1", PathFormat::Posix));
        assert!(path_is_within(
            "/a/job1/sub/deep",
            "/a/job1",
            PathFormat::Posix
        ));
    }

    #[test]
    fn path_is_within_rejects_prefix_sibling() {
        // Sibling whose name extends the base name must NOT be considered inside.
        assert!(!path_is_within("/a/job10", "/a/job1", PathFormat::Posix));
        assert!(!path_is_within(
            "/tmp/templates/job1-sibling",
            "/tmp/templates/job1",
            PathFormat::Posix
        ));
    }

    #[test]
    fn path_is_within_root_base() {
        // A filesystem root already ends in a separator.
        assert!(path_is_within("/sub", "/", PathFormat::Posix));
        assert!(path_is_within("/", "/", PathFormat::Posix));
    }

    #[test]
    fn path_is_within_windows() {
        assert!(path_is_within(
            r"C:\templates\job1\sub",
            r"C:\templates\job1",
            PathFormat::Windows
        ));
        assert!(!path_is_within(
            r"C:\templates\job1-sibling",
            r"C:\templates\job1",
            PathFormat::Windows
        ));
    }

    #[test]
    fn coerce_int_to_float_returns_ok() {
        let val = openjd_expr::ExprValue::Int(42);
        let result = coerce_to_type(&val, JobParameterType::Float);
        assert!(result.is_ok(), "int-to-float coercion should succeed");
        match result.unwrap() {
            openjd_expr::ExprValue::Float(f) => assert_eq!(f.value(), 42.0),
            other => panic!("expected Float, got {other:?}"),
        }
    }

    #[test]
    fn coerce_large_int_to_float_returns_ok() {
        // Large i64 that loses precision as f64 but is still finite
        let val = openjd_expr::ExprValue::Int(i64::MAX);
        let result = coerce_to_type(&val, JobParameterType::Float);
        assert!(result.is_ok(), "large int-to-float coercion should succeed");
    }
}
