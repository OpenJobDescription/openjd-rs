// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Job parameter definitions per spec §2.

use super::constrained_strings::{Description, Identifier};
use crate::types::{DataFlow, ObjectType};
use serde::Deserialize;

/// Wrapper for `Option<Vec<T>>` that rejects explicit YAML `null`.
///
/// With `#[serde(default)]`, absent fields get `Default::default()` (inner None)
/// without calling the deserializer. Present-but-null fields DO call the
/// deserializer, where we reject them. This lets us distinguish:
/// - absent → `NullableVec(None)` via Default
/// - `null` → deserialization error
/// - `[]` → `NullableVec(Some([]))`
/// - `[1,2]` → `NullableVec(Some([1,2]))`
#[derive(Debug, Clone)]
pub struct NullableVec<T>(pub Option<Vec<T>>);

impl<T> Default for NullableVec<T> {
    fn default() -> Self {
        NullableVec(None)
    }
}

impl<T> NullableVec<T> {
    pub fn as_ref(&self) -> Option<&Vec<T>> {
        self.0.as_ref()
    }
    pub fn is_some(&self) -> bool {
        self.0.is_some()
    }
    pub fn is_none(&self) -> bool {
        self.0.is_none()
    }
}

impl<'de, T: serde::de::DeserializeOwned> Deserialize<'de> for NullableVec<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match Option::<Vec<T>>::deserialize(deserializer)? {
            None => Err(serde::de::Error::custom(
                "null is not allowed for this field",
            )),
            Some(vec) => Ok(NullableVec(Some(vec))),
        }
    }
}

/// §2 JobParameterDefinition — discriminated union on `type` field.
///
/// With the EXPR extension, type names are case-insensitive and additional
/// types are available (BOOL, RANGE_EXPR, `LIST[*]`).
#[derive(Debug, Clone)]
#[allow(non_camel_case_types)]
pub enum JobParameterDefinition {
    STRING(JobStringParameterDefinition),
    INT(JobIntParameterDefinition),
    FLOAT(JobFloatParameterDefinition),
    PATH(JobPathParameterDefinition),
    // EXPR extension types
    BOOL(super::expr_parameters::JobBoolParameterDefinition),
    RANGE_EXPR(super::expr_parameters::JobRangeExprParameterDefinition),
    LIST_STRING(super::expr_parameters::JobListStringParameterDefinition),
    LIST_PATH(super::expr_parameters::JobListPathParameterDefinition),
    LIST_INT(super::expr_parameters::JobListIntParameterDefinition),
    LIST_FLOAT(super::expr_parameters::JobListFloatParameterDefinition),
    LIST_BOOL(super::expr_parameters::JobListBoolParameterDefinition),
    LIST_LIST_INT(super::expr_parameters::JobListListIntParameterDefinition),
}

/// Remove the `type` field from a JSON object before deserializing into a struct.
fn strip_type_field(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = value.as_object_mut() {
        obj.remove("type");
    }
    value
}

impl<'de> serde::Deserialize<'de> for JobParameterDefinition {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let type_str = value
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                serde::de::Error::custom("missing 'type' field in parameter definition")
            })?
            .to_string();

        let normalized = type_str.to_uppercase();
        let stripped = strip_type_field(value);

        match normalized.as_str() {
            "STRING" => serde_json::from_value(stripped)
                .map(Self::STRING)
                .map_err(serde::de::Error::custom),
            "INT" => serde_json::from_value(stripped)
                .map(Self::INT)
                .map_err(serde::de::Error::custom),
            "FLOAT" => serde_json::from_value(stripped)
                .map(Self::FLOAT)
                .map_err(serde::de::Error::custom),
            "PATH" => serde_json::from_value(stripped)
                .map(Self::PATH)
                .map_err(serde::de::Error::custom),
            "BOOL" => serde_json::from_value(stripped)
                .map(Self::BOOL)
                .map_err(serde::de::Error::custom),
            "RANGE_EXPR" => serde_json::from_value(stripped)
                .map(Self::RANGE_EXPR)
                .map_err(serde::de::Error::custom),
            "LIST[STRING]" => serde_json::from_value(stripped)
                .map(Self::LIST_STRING)
                .map_err(serde::de::Error::custom),
            "LIST[PATH]" => serde_json::from_value(stripped)
                .map(Self::LIST_PATH)
                .map_err(serde::de::Error::custom),
            "LIST[INT]" => serde_json::from_value(stripped)
                .map(Self::LIST_INT)
                .map_err(serde::de::Error::custom),
            "LIST[FLOAT]" => serde_json::from_value(stripped)
                .map(Self::LIST_FLOAT)
                .map_err(serde::de::Error::custom),
            "LIST[BOOL]" => serde_json::from_value(stripped)
                .map(Self::LIST_BOOL)
                .map_err(serde::de::Error::custom),
            "LIST[LIST[INT]]" => serde_json::from_value(stripped)
                .map(Self::LIST_LIST_INT)
                .map_err(serde::de::Error::custom),
            _ => Err(serde::de::Error::custom(format!(
                "unknown parameter type: '{type_str}'"
            ))),
        }
    }
}

impl JobParameterDefinition {
    pub fn job_param_type(&self) -> crate::types::JobParameterType {
        use crate::types::JobParameterType;
        match self {
            Self::STRING(_) => JobParameterType::String,
            Self::INT(_) => JobParameterType::Int,
            Self::FLOAT(_) => JobParameterType::Float,
            Self::PATH(_) => JobParameterType::Path,
            Self::BOOL(_) => JobParameterType::Bool,
            Self::RANGE_EXPR(_) => JobParameterType::RangeExpr,
            Self::LIST_STRING(_) => JobParameterType::ListString,
            Self::LIST_PATH(_) => JobParameterType::ListPath,
            Self::LIST_INT(_) => JobParameterType::ListInt,
            Self::LIST_FLOAT(_) => JobParameterType::ListFloat,
            Self::LIST_BOOL(_) => JobParameterType::ListBool,
            Self::LIST_LIST_INT(_) => JobParameterType::ListListInt,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::STRING(p) => p.name.as_str(),
            Self::INT(p) => p.name.as_str(),
            Self::FLOAT(p) => p.name.as_str(),
            Self::PATH(p) => p.name.as_str(),
            Self::BOOL(p) => p.name.as_str(),
            Self::RANGE_EXPR(p) => p.name.as_str(),
            Self::LIST_STRING(p) => p.name.as_str(),
            Self::LIST_PATH(p) => p.name.as_str(),
            Self::LIST_INT(p) => p.name.as_str(),
            Self::LIST_FLOAT(p) => p.name.as_str(),
            Self::LIST_BOOL(p) => p.name.as_str(),
            Self::LIST_LIST_INT(p) => p.name.as_str(),
        }
    }

    pub fn description(&self) -> Option<&str> {
        match self {
            Self::STRING(p) => p.description.as_ref().map(|d| d.0.as_str()),
            Self::INT(p) => p.description.as_ref().map(|d| d.0.as_str()),
            Self::FLOAT(p) => p.description.as_ref().map(|d| d.0.as_str()),
            Self::PATH(p) => p.description.as_ref().map(|d| d.0.as_str()),
            Self::BOOL(p) => p.description.as_ref().map(|d| d.0.as_str()),
            Self::RANGE_EXPR(p) => p.description.as_ref().map(|d| d.0.as_str()),
            Self::LIST_STRING(p) => p.description.as_ref().map(|d| d.0.as_str()),
            Self::LIST_PATH(p) => p.description.as_ref().map(|d| d.0.as_str()),
            Self::LIST_INT(p) => p.description.as_ref().map(|d| d.0.as_str()),
            Self::LIST_FLOAT(p) => p.description.as_ref().map(|d| d.0.as_str()),
            Self::LIST_BOOL(p) => p.description.as_ref().map(|d| d.0.as_str()),
            Self::LIST_LIST_INT(p) => p.description.as_ref().map(|d| d.0.as_str()),
        }
    }

    pub fn type_name(&self) -> &str {
        match self {
            Self::STRING(_) => "STRING",
            Self::INT(_) => "INT",
            Self::FLOAT(_) => "FLOAT",
            Self::PATH(_) => "PATH",
            Self::BOOL(_) => "BOOL",
            Self::RANGE_EXPR(_) => "RANGE_EXPR",
            Self::LIST_STRING(_) => "LIST[STRING]",
            Self::LIST_PATH(_) => "LIST[PATH]",
            Self::LIST_INT(_) => "LIST[INT]",
            Self::LIST_FLOAT(_) => "LIST[FLOAT]",
            Self::LIST_BOOL(_) => "LIST[BOOL]",
            Self::LIST_LIST_INT(_) => "LIST[LIST[INT]]",
        }
    }

    pub fn path_properties(&self) -> (Option<ObjectType>, Option<DataFlow>) {
        match self {
            Self::PATH(p) => (p.object_type, p.data_flow),
            Self::LIST_PATH(p) => (p.object_type, p.data_flow),
            _ => (None, None),
        }
    }

    pub fn default_value(&self) -> Option<String> {
        match self {
            Self::STRING(p) => p.default.clone(),
            Self::INT(p) => p.default.as_ref().map(|v| v.to_string()),
            Self::FLOAT(p) => p
                .default
                .as_ref()
                .map(|v| v.1.clone().unwrap_or_else(|| v.0.to_string())),
            Self::PATH(p) => p.default.clone(),
            Self::BOOL(p) => p.default.as_ref().map(|v| v.0.to_string()),
            Self::RANGE_EXPR(p) => p.default.clone(),
            Self::LIST_STRING(p) => p
                .default
                .as_ref()
                .map(|v| serde_json::to_string(v).unwrap_or_default()),
            Self::LIST_PATH(p) => p
                .default
                .as_ref()
                .map(|v| serde_json::to_string(v).unwrap_or_default()),
            Self::LIST_INT(p) => p.default.as_ref().map(|v| {
                let ints: Vec<i64> = v.iter().map(|i| i.0).collect();
                serde_json::to_string(&ints).unwrap_or_default()
            }),
            Self::LIST_FLOAT(p) => p.default.as_ref().map(|v| {
                let floats: Vec<f64> = v.iter().map(|f| f.0).collect();
                serde_json::to_string(&floats).unwrap_or_default()
            }),
            Self::LIST_BOOL(p) => p.default.as_ref().map(|v| {
                let bools: Vec<bool> = v.iter().map(|b| b.0).collect();
                serde_json::to_string(&bools).unwrap_or_default()
            }),
            Self::LIST_LIST_INT(p) => p.default.as_ref().map(|v| {
                let lists: Vec<Vec<i64>> = v
                    .iter()
                    .map(|inner| inner.iter().map(|i| i.0).collect())
                    .collect();
                serde_json::to_string(&lists).unwrap_or_default()
            }),
        }
    }

    pub fn min_value_i64(&self) -> Option<i64> {
        match self {
            Self::INT(p) => p.min_value.as_ref().map(|v| v.0),
            _ => None,
        }
    }

    pub fn max_value_i64(&self) -> Option<i64> {
        match self {
            Self::INT(p) => p.max_value.as_ref().map(|v| v.0),
            _ => None,
        }
    }

    pub fn allowed_values_i64(&self) -> Option<Vec<i64>> {
        match self {
            Self::INT(p) => p
                .allowed_values
                .as_ref()
                .map(|v| v.iter().map(|i| i.0).collect()),
            _ => None,
        }
    }

    pub fn allowed_values_f64(&self) -> Option<Vec<f64>> {
        match self {
            Self::FLOAT(p) => p
                .allowed_values
                .as_ref()
                .map(|v| v.iter().map(|f| f.0).collect()),
            _ => None,
        }
    }

    pub fn allowed_values_strings(&self) -> Option<Vec<String>> {
        match self {
            Self::STRING(p) => p.allowed_values.clone(),
            Self::PATH(p) => p.allowed_values.clone(),
            _ => None,
        }
    }

    pub fn min_length(&self) -> Option<usize> {
        match self {
            Self::STRING(p) => p.min_length,
            Self::PATH(p) => p.min_length,
            Self::RANGE_EXPR(p) => p.min_length,
            _ => None,
        }
    }

    pub fn max_length(&self) -> Option<usize> {
        match self {
            Self::STRING(p) => p.max_length,
            Self::PATH(p) => p.max_length,
            Self::RANGE_EXPR(p) => p.max_length,
            _ => None,
        }
    }

    pub fn min_value_f64(&self) -> Option<f64> {
        match self {
            Self::FLOAT(p) => p.min_value.as_ref().map(|v| v.0),
            _ => None,
        }
    }

    pub fn max_value_f64(&self) -> Option<f64> {
        match self {
            Self::FLOAT(p) => p.max_value.as_ref().map(|v| v.0),
            _ => None,
        }
    }

    pub fn check_constraints(&self, value: &openjd_expr::ExprValue) -> Result<(), String> {
        // Extract string representation for base type constraint checking
        let s;
        let str_val = match value {
            openjd_expr::ExprValue::String(v) => v.as_str(),
            openjd_expr::ExprValue::Int(v) => {
                s = v.to_string();
                &s
            }
            openjd_expr::ExprValue::Float(v) => {
                s = v.to_string();
                &s
            }
            openjd_expr::ExprValue::Bool(v) => {
                s = v.to_string();
                &s
            }
            openjd_expr::ExprValue::Path { value: v, .. } => v.as_str(),
            _ => "", // List types — base constraint checking doesn't apply
        };
        match self {
            Self::STRING(p) => p.check_constraints(str_val),
            Self::INT(p) => p.check_constraints(str_val),
            Self::FLOAT(p) => p.check_constraints(str_val),
            Self::PATH(p) => p.check_constraints(str_val),
            Self::BOOL(p) => p.check_value_constraints(value),
            Self::RANGE_EXPR(p) => p.check_value_constraints(value),
            Self::LIST_STRING(p) => p.check_value_constraints(value),
            Self::LIST_PATH(p) => p.check_value_constraints(value),
            Self::LIST_INT(p) => p.check_value_constraints(value),
            Self::LIST_FLOAT(p) => p.check_value_constraints(value),
            Self::LIST_BOOL(p) => p.check_value_constraints(value),
            Self::LIST_LIST_INT(p) => p.check_value_constraints(value),
        }
    }

    pub fn validate_definition(
        &self,
        limits: &super::validate_v2023_09::EffectiveLimits,
    ) -> Result<(), Vec<String>> {
        match self {
            Self::STRING(p) => p.validate_definition(limits),
            Self::INT(p) => p.validate_definition(),
            Self::FLOAT(p) => p.validate_definition(),
            Self::PATH(p) => p.validate_definition(limits),
            Self::BOOL(p) => p.validate_definition(),
            Self::RANGE_EXPR(p) => p.validate_definition(),
            Self::LIST_STRING(p) => p.validate_definition(),
            Self::LIST_PATH(p) => p.validate_definition(),
            Self::LIST_INT(p) => p.validate_definition(),
            Self::LIST_FLOAT(p) => p.validate_definition(),
            Self::LIST_BOOL(p) => p.validate_definition(),
            Self::LIST_LIST_INT(p) => p.validate_definition(),
        }
    }
}

/// User interface definition for STRING parameters.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StringUserInterface {
    pub control: Option<String>,
    pub label: Option<String>,
    pub group_label: Option<String>,
}

/// User interface definition for INT parameters.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IntUserInterface {
    pub control: Option<String>,
    pub label: Option<String>,
    pub group_label: Option<String>,
    pub single_step_delta: Option<FlexInt>,
}

/// User interface definition for FLOAT parameters.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FloatUserInterface {
    pub control: Option<String>,
    pub label: Option<String>,
    pub group_label: Option<String>,
    pub decimals: Option<FlexInt>,
    pub single_step_delta: Option<FlexFloat>,
}

/// User interface definition for PATH parameters.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PathUserInterface {
    pub control: Option<String>,
    pub label: Option<String>,
    pub group_label: Option<String>,
    pub file_filters: Option<Vec<FileFilter>>,
    pub file_filter_default: Option<FileFilter>,
}

/// §2.7 JobPathParameterFileFilter
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileFilter {
    pub label: String,
    pub patterns: Vec<String>,
}

pub(crate) fn validate_ui_label(
    label: &Option<String>,
    field_name: &str,
    param_name: &str,
) -> Vec<String> {
    let mut errors = Vec::new();
    if let Some(l) = label {
        if l.is_empty() {
            errors.push(format!(
                "Parameter '{param_name}': {field_name} must not be empty."
            ));
        }
        if l.chars().count() > 64 {
            errors.push(format!(
                "Parameter '{param_name}': {field_name} exceeds 64 characters."
            ));
        }
        if l.chars().any(|c| c.is_control()) {
            errors.push(format!(
                "Parameter '{param_name}': {field_name} contains control characters."
            ));
        }
    }
    errors
}

/// §2.1 JobStringParameterDefinition
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobStringParameterDefinition {
    pub name: Identifier,
    pub description: Option<Description>,
    pub default: Option<String>,
    pub allowed_values: Option<Vec<String>>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub user_interface: Option<StringUserInterface>,
}

impl JobStringParameterDefinition {
    pub fn check_constraints(&self, value: &str) -> Result<(), String> {
        let char_len = value.chars().count();
        if let Some(min) = self.min_length {
            if char_len < min {
                return Err(format!(
                    "Parameter '{}': value length {} is less than minimum {min}",
                    self.name, char_len
                ));
            }
        }
        if let Some(max) = self.max_length {
            if char_len > max {
                return Err(format!(
                    "Parameter '{}': value length {} exceeds maximum {max}",
                    self.name, char_len
                ));
            }
        }
        if let Some(allowed) = self.allowed_values.as_ref() {
            if !allowed.iter().any(|a| a == value) {
                return Err(format!(
                    "Parameter '{}': value '{value}' is not in allowed values",
                    self.name
                ));
            }
        }
        Ok(())
    }

    pub fn validate_definition(
        &self,
        limits: &super::validate_v2023_09::EffectiveLimits,
    ) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        // §2.5: allowed values max length
        if let Some(allowed) = self.allowed_values.as_ref() {
            if allowed.is_empty() {
                errors.push(format!(
                    "Parameter '{}': allowedValues must not be empty.",
                    self.name
                ));
            }
            for (i, v) in allowed.iter().enumerate() {
                let vlen = v.chars().count();
                if vlen > limits.max_job_param_string_len {
                    errors.push(format!(
                        "Parameter '{}': allowedValues[{i}] exceeds {} characters.",
                        self.name, limits.max_job_param_string_len
                    ));
                }
                if let Some(min) = self.min_length {
                    if vlen < min {
                        errors.push(format!("Parameter '{}': allowedValues[{i}] length {vlen} is less than minLength {min}.", self.name));
                    }
                }
                if let Some(max) = self.max_length {
                    if vlen > max {
                        errors.push(format!(
                            "Parameter '{}': allowedValues[{i}] length {vlen} exceeds maxLength {max}.",
                            self.name,
                        ));
                    }
                }
            }
        }

        // min/max length consistency
        if let (Some(min), Some(max)) = (self.min_length, self.max_length) {
            if min > max {
                errors.push(format!(
                    "Parameter '{}': minLength ({min}) > maxLength ({max}).",
                    self.name
                ));
            }
        }
        if let Some(max) = self.max_length {
            if max == 0 {
                errors.push(format!("Parameter '{}': maxLength must be > 0.", self.name));
            }
        }

        // Default must satisfy constraints
        if let Some(default) = &self.default {
            let dlen = default.chars().count();
            if dlen > limits.max_job_param_string_len {
                errors.push(format!(
                    "Parameter '{}': default exceeds {} characters.",
                    self.name, limits.max_job_param_string_len
                ));
            }
            if let Some(min) = self.min_length {
                if dlen < min {
                    errors.push(format!(
                        "Parameter '{}': default length {dlen} is less than minLength {min}.",
                        self.name,
                    ));
                }
            }
            if let Some(max) = self.max_length {
                if dlen > max {
                    errors.push(format!(
                        "Parameter '{}': default length {dlen} exceeds maxLength {max}.",
                        self.name,
                    ));
                }
            }
            if let Some(allowed) = self.allowed_values.as_ref() {
                if !allowed.contains(default) {
                    errors.push(format!(
                        "Parameter '{}': default '{}' is not in allowedValues.",
                        self.name, default
                    ));
                }
            }
        }

        // UI validation
        if let Some(ui) = &self.user_interface {
            errors.extend(validate_ui_label(&ui.label, "label", self.name.as_str()));
            errors.extend(validate_ui_label(
                &ui.group_label,
                "groupLabel",
                self.name.as_str(),
            ));

            if let Some(control) = &ui.control {
                match control.as_str() {
                    "LINE_EDIT" | "MULTILINE_EDIT" => {
                        if self.allowed_values.is_some() {
                            errors.push(format!("Parameter '{}': control '{control}' cannot be used with allowedValues.", self.name));
                        }
                    }
                    "DROPDOWN_LIST" => {
                        if self.allowed_values.is_none() {
                            errors.push(format!(
                                "Parameter '{}': DROPDOWN_LIST requires allowedValues.",
                                self.name
                            ));
                        }
                    }
                    "CHECK_BOX" => {
                        if let Some(allowed) = self.allowed_values.as_ref() {
                            if allowed.len() != 2 {
                                errors.push(format!(
                                    "Parameter '{}': CHECK_BOX requires exactly 2 allowedValues.",
                                    self.name
                                ));
                            } else {
                                let pair: Vec<String> =
                                    allowed.iter().map(|s| s.to_lowercase()).collect();
                                let valid_pairs = [
                                    vec!["true", "false"],
                                    vec!["false", "true"],
                                    vec!["yes", "no"],
                                    vec!["no", "yes"],
                                    vec!["on", "off"],
                                    vec!["off", "on"],
                                    vec!["1", "0"],
                                    vec!["0", "1"],
                                ];
                                if !valid_pairs.iter().any(|vp| vp == &pair) {
                                    errors.push(format!("Parameter '{}': CHECK_BOX allowedValues must be a valid boolean pair.", self.name));
                                }
                            }
                        } else {
                            errors.push(format!(
                                "Parameter '{}': CHECK_BOX requires allowedValues.",
                                self.name
                            ));
                        }
                    }
                    "HIDDEN" => {}
                    _ => {
                        errors.push(format!(
                            "Parameter '{}': unknown control '{control}'.",
                            self.name
                        ));
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// An `i64` that deserializes from YAML integers, integer-valued floats (e.g. `42.0`), or
/// numeric strings (e.g. `"42"`). Rejects booleans, nulls, and non-integer floats.
#[derive(Debug, Clone)]
pub struct FlexInt(pub i64);

impl<'de> Deserialize<'de> for FlexInt {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let val = serde_json::Value::deserialize(deserializer)?;
        match &val {
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(FlexInt(i))
                } else if let Some(f) = n.as_f64() {
                    if f.fract() == 0.0 {
                        Ok(FlexInt(f as i64))
                    } else {
                        Err(serde::de::Error::custom(format!(
                            "Expected integer, got float: {f}"
                        )))
                    }
                } else {
                    Err(serde::de::Error::custom("Invalid number"))
                }
            }
            serde_json::Value::String(s) => s
                .trim()
                .parse::<i64>()
                .map(FlexInt)
                .map_err(|_| serde::de::Error::custom(format!("Cannot parse '{s}' as integer"))),
            serde_json::Value::Bool(_) => {
                Err(serde::de::Error::custom("Expected integer, got boolean"))
            }
            serde_json::Value::Null => Err(serde::de::Error::custom("Expected integer, got null")),
            _ => Err(serde::de::Error::custom("Expected integer or string")),
        }
    }
}

impl std::fmt::Display for FlexInt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An `f64` that deserializes from YAML numbers or numeric strings (e.g. `"3.14"`).
/// Preserves the original string representation when parsed from a string, which is needed
/// for round-trip fidelity in constraint checking. Rejects NaN, Infinity, booleans, and nulls.
#[derive(Debug, Clone)]
pub struct FlexFloat(pub f64, pub Option<String>);

/// Reject NaN and Infinity float values.
pub(crate) fn reject_nan_inf(f: f64) -> Result<(), String> {
    if f.is_nan() {
        Err("NaN is not a valid float value".to_string())
    } else if f.is_infinite() {
        Err("Infinity is not a valid float value".to_string())
    } else {
        Ok(())
    }
}

impl<'de> Deserialize<'de> for FlexFloat {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let val = serde_json::Value::deserialize(deserializer)?;
        match &val {
            serde_json::Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    reject_nan_inf(f).map_err(serde::de::Error::custom)?;
                    Ok(FlexFloat(f, None))
                } else {
                    Err(serde::de::Error::custom("Invalid number"))
                }
            }
            serde_json::Value::String(s) => {
                let f = s.trim().parse::<f64>().map_err(|_| {
                    serde::de::Error::custom(format!("Cannot parse '{s}' as float"))
                })?;
                reject_nan_inf(f).map_err(serde::de::Error::custom)?;
                Ok(FlexFloat(f, Some(s.trim().to_string())))
            }
            serde_json::Value::Bool(_) => {
                Err(serde::de::Error::custom("Expected number, got boolean"))
            }
            serde_json::Value::Null => Err(serde::de::Error::custom("Expected number, got null")),
            _ => Err(serde::de::Error::custom("Expected number or string")),
        }
    }
}

impl std::fmt::Display for FlexFloat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Note: strict `<` on the upper bound because i64::MAX is not exactly representable
        // as f64 — `i64::MAX as f64` rounds up to 9223372036854775808.0, and casting that
        // back via `as i64` would saturate to i64::MAX, producing a wrong display value.
        if self.0.fract() == 0.0 && self.0 >= i64::MIN as f64 && self.0 < i64::MAX as f64 {
            write!(f, "{}", self.0 as i64)
        } else {
            write!(f, "{}", self.0)
        }
    }
}

/// §2.3 JobIntParameterDefinition
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobIntParameterDefinition {
    pub name: Identifier,
    pub description: Option<Description>,
    pub default: Option<FlexInt>,

    #[serde(default)]
    pub allowed_values: NullableVec<FlexInt>,
    pub min_value: Option<FlexInt>,
    pub max_value: Option<FlexInt>,
    pub user_interface: Option<IntUserInterface>,
}

impl JobIntParameterDefinition {
    pub fn check_constraints(&self, value: &str) -> Result<(), String> {
        let parsed: i64 = value.parse().map_err(|_| {
            format!(
                "Parameter '{}': value '{value}' is not a valid integer",
                self.name
            )
        })?;
        if let Some(min) = &self.min_value {
            if parsed < min.0 {
                return Err(format!(
                    "Parameter '{}': value {parsed} is less than minimum {}",
                    self.name, min.0
                ));
            }
        }
        if let Some(max) = &self.max_value {
            if parsed > max.0 {
                return Err(format!(
                    "Parameter '{}': value {parsed} exceeds maximum {}",
                    self.name, max.0
                ));
            }
        }
        if let Some(allowed) = self.allowed_values.as_ref() {
            if !allowed.iter().any(|a| a.0 == parsed) {
                return Err(format!(
                    "Parameter '{}': value {parsed} is not in allowed values",
                    self.name
                ));
            }
        }
        Ok(())
    }

    pub fn validate_definition(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if let Some(allowed) = self.allowed_values.as_ref() {
            if allowed.is_empty() {
                errors.push(format!(
                    "Parameter '{}': allowedValues must not be empty.",
                    self.name
                ));
            }
        }
        if let (Some(min), Some(max)) = (&self.min_value, &self.max_value) {
            if min.0 > max.0 {
                errors.push(format!(
                    "Parameter '{}': minValue ({}) > maxValue ({}).",
                    self.name, min.0, max.0
                ));
            }
        }
        if let Some(default) = &self.default {
            if let Err(e) = self.check_constraints(&default.0.to_string()) {
                errors.push(e);
            }
        }
        if let Some(ui) = &self.user_interface {
            errors.extend(validate_ui_label(&ui.label, "label", self.name.as_str()));
            errors.extend(validate_ui_label(
                &ui.group_label,
                "groupLabel",
                self.name.as_str(),
            ));
            let control = ui
                .control
                .as_deref()
                .unwrap_or(if self.allowed_values.is_some() {
                    "DROPDOWN_LIST"
                } else {
                    "SPIN_BOX"
                });
            match control {
                "SPIN_BOX" => {
                    if self.allowed_values.is_some() {
                        errors.push(format!(
                            "Parameter '{}': SPIN_BOX cannot be used with allowedValues.",
                            self.name
                        ));
                    }
                }
                "DROPDOWN_LIST" => {
                    if self.allowed_values.is_none() {
                        errors.push(format!(
                            "Parameter '{}': DROPDOWN_LIST requires allowedValues.",
                            self.name
                        ));
                    }
                    if ui.single_step_delta.is_some() {
                        errors.push(format!(
                            "Parameter '{}': singleStepDelta is only valid with SPIN_BOX.",
                            self.name
                        ));
                    }
                }
                "HIDDEN" => {
                    if ui.single_step_delta.is_some() {
                        errors.push(format!(
                            "Parameter '{}': singleStepDelta is not valid with HIDDEN.",
                            self.name
                        ));
                    }
                }
                _ => errors.push(format!(
                    "Parameter '{}': unknown control '{control}'.",
                    self.name
                )),
            }
            // singleStepDelta must be positive integer
            if let Some(delta) = &ui.single_step_delta {
                if delta.0 <= 0 {
                    errors.push(format!(
                        "Parameter '{}': singleStepDelta must be positive.",
                        self.name
                    ));
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// §2.4 JobFloatParameterDefinition
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobFloatParameterDefinition {
    pub name: Identifier,
    pub description: Option<Description>,
    pub default: Option<FlexFloat>,

    #[serde(default)]
    pub allowed_values: NullableVec<FlexFloat>,
    pub min_value: Option<FlexFloat>,
    pub max_value: Option<FlexFloat>,
    pub user_interface: Option<FloatUserInterface>,
}

impl JobFloatParameterDefinition {
    pub fn check_constraints(&self, value: &str) -> Result<(), String> {
        let parsed: f64 = value.parse().map_err(|_| {
            format!(
                "Parameter '{}': value '{value}' is not a valid float",
                self.name
            )
        })?;
        reject_nan_inf(parsed).map_err(|e| format!("Parameter '{}': {e}", self.name))?;
        if let Some(min) = &self.min_value {
            if parsed < min.0 {
                return Err(format!(
                    "Parameter '{}': value {parsed} is less than minimum {}",
                    self.name, min.0
                ));
            }
        }
        if let Some(max) = &self.max_value {
            if parsed > max.0 {
                return Err(format!(
                    "Parameter '{}': value {parsed} exceeds maximum {}",
                    self.name, max.0
                ));
            }
        }
        if let Some(allowed) = self.allowed_values.as_ref() {
            if !allowed.iter().any(|a| a.0 == parsed) {
                return Err(format!(
                    "Parameter '{}': value {parsed} is not in allowed values",
                    self.name
                ));
            }
        }
        Ok(())
    }

    pub fn validate_definition(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if let Some(allowed) = self.allowed_values.as_ref() {
            if allowed.is_empty() {
                errors.push(format!(
                    "Parameter '{}': allowedValues must not be empty.",
                    self.name
                ));
            }
            // allowedValues must satisfy min/max constraints
            for (i, a) in allowed.iter().enumerate() {
                if let Some(min) = &self.min_value {
                    if a.0 < min.0 {
                        errors.push(format!(
                            "Parameter '{}': allowedValues[{i}] ({}) is less than minValue ({}).",
                            self.name, a.0, min.0
                        ));
                    }
                }
                if let Some(max) = &self.max_value {
                    if a.0 > max.0 {
                        errors.push(format!(
                            "Parameter '{}': allowedValues[{i}] ({}) exceeds maxValue ({}).",
                            self.name, a.0, max.0
                        ));
                    }
                }
            }
        }
        if let (Some(min), Some(max)) = (&self.min_value, &self.max_value) {
            if min.0 > max.0 {
                errors.push(format!(
                    "Parameter '{}': minValue ({}) > maxValue ({}).",
                    self.name, min.0, max.0
                ));
            }
        }
        // Default must satisfy constraints
        if let Some(default) = &self.default {
            if let Err(e) = self.check_constraints(&default.0.to_string()) {
                errors.push(e);
            }
        }
        if let Some(ui) = &self.user_interface {
            errors.extend(validate_ui_label(&ui.label, "label", self.name.as_str()));
            errors.extend(validate_ui_label(
                &ui.group_label,
                "groupLabel",
                self.name.as_str(),
            ));
            let control = ui
                .control
                .as_deref()
                .unwrap_or(if self.allowed_values.is_some() {
                    "DROPDOWN_LIST"
                } else {
                    "SPIN_BOX"
                });
            match control {
                "SPIN_BOX" => {
                    if self.allowed_values.is_some() {
                        errors.push(format!(
                            "Parameter '{}': SPIN_BOX cannot be used with allowedValues.",
                            self.name
                        ));
                    }
                }
                "DROPDOWN_LIST" => {
                    if self.allowed_values.is_none() {
                        errors.push(format!(
                            "Parameter '{}': DROPDOWN_LIST requires allowedValues.",
                            self.name
                        ));
                    }
                    if ui.decimals.is_some() {
                        errors.push(format!(
                            "Parameter '{}': decimals is only valid with SPIN_BOX.",
                            self.name
                        ));
                    }
                    if ui.single_step_delta.is_some() {
                        errors.push(format!(
                            "Parameter '{}': singleStepDelta is only valid with SPIN_BOX.",
                            self.name
                        ));
                    }
                }
                "HIDDEN" => {
                    if ui.decimals.is_some() {
                        errors.push(format!(
                            "Parameter '{}': decimals is not valid with HIDDEN.",
                            self.name
                        ));
                    }
                    if ui.single_step_delta.is_some() {
                        errors.push(format!(
                            "Parameter '{}': singleStepDelta is not valid with HIDDEN.",
                            self.name
                        ));
                    }
                }
                _ => errors.push(format!(
                    "Parameter '{}': unknown control '{control}'.",
                    self.name
                )),
            }
            if let Some(delta) = &ui.single_step_delta {
                if delta.0 <= 0.0 {
                    errors.push(format!(
                        "Parameter '{}': singleStepDelta must be positive.",
                        self.name
                    ));
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// §2.2 JobPathParameterDefinition
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobPathParameterDefinition {
    pub name: Identifier,
    pub description: Option<Description>,
    pub default: Option<String>,
    pub allowed_values: Option<Vec<String>>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub object_type: Option<ObjectType>,
    pub data_flow: Option<DataFlow>,
    pub user_interface: Option<PathUserInterface>,
}

impl JobPathParameterDefinition {
    pub fn check_constraints(&self, value: &str) -> Result<(), String> {
        let char_len = value.chars().count();
        if let Some(min) = self.min_length {
            if char_len < min {
                return Err(format!(
                    "Parameter '{}': value length {} is less than minimum {min}",
                    self.name, char_len
                ));
            }
        }
        if let Some(max) = self.max_length {
            if char_len > max {
                return Err(format!(
                    "Parameter '{}': value length {} exceeds maximum {max}",
                    self.name, char_len
                ));
            }
        }
        if let Some(allowed) = self.allowed_values.as_ref() {
            if !allowed.iter().any(|a| a == value) {
                return Err(format!(
                    "Parameter '{}': value '{value}' is not in allowed values",
                    self.name
                ));
            }
        }
        Ok(())
    }

    pub fn validate_definition(
        &self,
        limits: &super::validate_v2023_09::EffectiveLimits,
    ) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        let object_type = self.object_type.unwrap_or(ObjectType::Directory);
        if let Some(allowed) = self.allowed_values.as_ref() {
            if allowed.is_empty() {
                errors.push(format!(
                    "Parameter '{}': allowedValues must not be empty.",
                    self.name
                ));
            }
            for (i, v) in allowed.iter().enumerate() {
                let vlen = v.chars().count();
                if vlen > limits.max_job_param_string_len {
                    errors.push(format!(
                        "Parameter '{}': allowedValues[{i}] exceeds {} characters.",
                        self.name, limits.max_job_param_string_len
                    ));
                }
                if let Some(min) = self.min_length {
                    if vlen < min {
                        errors.push(format!(
                            "Parameter '{}': allowedValues[{i}] length {vlen} < minLength {min}.",
                            self.name,
                        ));
                    }
                }
                if let Some(max) = self.max_length {
                    if vlen > max {
                        errors.push(format!(
                            "Parameter '{}': allowedValues[{i}] length {vlen} > maxLength {max}.",
                            self.name,
                        ));
                    }
                }
            }
        }
        if let (Some(min), Some(max)) = (self.min_length, self.max_length) {
            if min > max {
                errors.push(format!(
                    "Parameter '{}': minLength ({min}) > maxLength ({max}).",
                    self.name
                ));
            }
        }
        if let Some(default) = &self.default {
            let dlen = default.chars().count();
            if dlen > limits.max_job_param_string_len {
                errors.push(format!(
                    "Parameter '{}': default exceeds {} characters.",
                    self.name, limits.max_job_param_string_len
                ));
            }
            if let Some(min) = self.min_length {
                if dlen < min {
                    errors.push(format!(
                        "Parameter '{}': default length {dlen} < minLength {min}.",
                        self.name,
                    ));
                }
            }
            if let Some(max) = self.max_length {
                if dlen > max {
                    errors.push(format!(
                        "Parameter '{}': default length {dlen} > maxLength {max}.",
                        self.name,
                    ));
                }
            }
            if let Some(allowed) = self.allowed_values.as_ref() {
                if !allowed.contains(default) {
                    errors.push(format!(
                        "Parameter '{}': default '{}' is not in allowedValues.",
                        self.name, default
                    ));
                }
            }
        }
        if let Some(ui) = &self.user_interface {
            errors.extend(validate_ui_label(&ui.label, "label", self.name.as_str()));
            errors.extend(validate_ui_label(
                &ui.group_label,
                "groupLabel",
                self.name.as_str(),
            ));
            let control = ui
                .control
                .as_deref()
                .unwrap_or(if self.allowed_values.is_some() {
                    "DROPDOWN_LIST"
                } else if object_type == ObjectType::File {
                    if self.data_flow == Some(DataFlow::Out) {
                        "CHOOSE_OUTPUT_FILE"
                    } else {
                        "CHOOSE_INPUT_FILE"
                    }
                } else {
                    "CHOOSE_DIRECTORY"
                });
            match control {
                "CHOOSE_INPUT_FILE" | "CHOOSE_OUTPUT_FILE" => {
                    if self.allowed_values.is_some() {
                        errors.push(format!(
                            "Parameter '{}': {control} cannot be used with allowedValues.",
                            self.name
                        ));
                    }
                    if object_type == ObjectType::Directory {
                        errors.push(format!(
                            "Parameter '{}': {control} requires objectType FILE.",
                            self.name
                        ));
                    }
                }
                "CHOOSE_DIRECTORY" => {
                    if self.allowed_values.is_some() {
                        errors.push(format!(
                            "Parameter '{}': CHOOSE_DIRECTORY cannot be used with allowedValues.",
                            self.name
                        ));
                    }
                    if object_type == ObjectType::File {
                        errors.push(format!(
                            "Parameter '{}': CHOOSE_DIRECTORY requires objectType DIRECTORY.",
                            self.name
                        ));
                    }
                }
                "DROPDOWN_LIST" => {
                    if self.allowed_values.is_none() {
                        errors.push(format!(
                            "Parameter '{}': DROPDOWN_LIST requires allowedValues.",
                            self.name
                        ));
                    }
                }
                "HIDDEN" => {}
                _ => errors.push(format!(
                    "Parameter '{}': unknown control '{control}'.",
                    self.name
                )),
            }
            // fileFilters only valid with CHOOSE_INPUT_FILE/CHOOSE_OUTPUT_FILE
            let is_file_chooser = control == "CHOOSE_INPUT_FILE" || control == "CHOOSE_OUTPUT_FILE";
            if let Some(filters) = &ui.file_filters {
                if !is_file_chooser {
                    errors.push(format!(
                        "Parameter '{}': fileFilters only valid with file chooser controls.",
                        self.name
                    ));
                }
                if filters.len() > 20 {
                    errors.push(format!(
                        "Parameter '{}': fileFilters exceeds 20 elements.",
                        self.name
                    ));
                }
                for filter in filters {
                    if filter.patterns.is_empty() {
                        errors.push(format!(
                            "Parameter '{}': fileFilter patterns must not be empty.",
                            self.name
                        ));
                    }
                    for pattern in &filter.patterns {
                        validate_file_filter_pattern(pattern, self.name.as_str(), &mut errors);
                    }
                }
            }
            if let Some(filter) = &ui.file_filter_default {
                if !is_file_chooser {
                    errors.push(format!(
                        "Parameter '{}': fileFilterDefault only valid with file chooser controls.",
                        self.name
                    ));
                }
                for pattern in &filter.patterns {
                    validate_file_filter_pattern(pattern, self.name.as_str(), &mut errors);
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn validate_file_filter_pattern(pattern: &str, param_name: &str, errors: &mut Vec<String>) {
    if pattern.is_empty() || pattern.len() > 20 {
        errors.push(format!(
            "Parameter '{param_name}': file filter pattern must be 1..=20 characters."
        ));
        return;
    }
    if pattern == "*" || pattern == "*.*" {
        return;
    }
    if !pattern.starts_with("*.") {
        errors.push(format!("Parameter '{param_name}': file filter pattern '{pattern}' must be '*', '*.*', or '*.ext'."));
        return;
    }
    let ext = &pattern[2..];
    if ext.is_empty() {
        errors.push(format!(
            "Parameter '{param_name}': file filter pattern '{pattern}' has empty extension."
        ));
        return;
    }
    let disallowed = [
        '\\', '/', '*', '?', '[', ']', '#', '%', '&', '{', '}', '<', '>', '$', '!', '\'', '"', ':',
        '@', '`', '|', '=',
    ];
    for ch in ext.chars() {
        if ch.is_control() || disallowed.contains(&ch) {
            errors.push(format!("Parameter '{param_name}': file filter pattern '{pattern}' contains disallowed character '{ch}'."));
            return;
        }
    }
}
