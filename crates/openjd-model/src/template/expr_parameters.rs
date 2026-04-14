// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! EXPR extension parameter types (§2.9-2.16).
//!
//! These types are only available when the EXPR extension is enabled.

use super::constrained_strings::{Description, Identifier};
use super::parameters::{validate_ui_label, FileFilter, FlexFloat, FlexInt};
use crate::types::{DataFlow, ObjectType};
use openjd_expr::ExprValue;
use serde::Deserialize;

/// User interface definition for BOOL parameters.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoolUserInterface {
    pub control: Option<String>,
    pub label: Option<String>,
    pub group_label: Option<String>,
}

/// User interface definition for RANGE_EXPR parameters.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RangeExprUserInterface {
    pub control: Option<String>,
    pub label: Option<String>,
    pub group_label: Option<String>,
}

/// User interface definition for `LIST[STRING]` and `LIST[BOOL]` parameters.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListSimpleUserInterface {
    pub control: Option<String>,
    pub label: Option<String>,
    pub group_label: Option<String>,
}

/// User interface definition for `LIST[PATH]` parameters.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListPathUserInterface {
    pub control: Option<String>,
    pub label: Option<String>,
    pub group_label: Option<String>,
    pub file_filters: Option<Vec<FileFilter>>,
    pub file_filter_default: Option<FileFilter>,
}

/// User interface definition for `LIST[INT]` parameters.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListIntUserInterface {
    pub control: Option<String>,
    pub label: Option<String>,
    pub group_label: Option<String>,
    pub single_step_delta: Option<FlexInt>,
}

/// User interface definition for `LIST[FLOAT]` parameters.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListFloatUserInterface {
    pub control: Option<String>,
    pub label: Option<String>,
    pub group_label: Option<String>,
    pub decimals: Option<FlexInt>,
    pub single_step_delta: Option<FlexFloat>,
}

/// User interface definition for `LIST[LIST[INT]]` parameters (HIDDEN only).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HiddenOnlyUserInterface {
    pub control: Option<String>,
    pub label: Option<String>,
    pub group_label: Option<String>,
}

/// §2.9 JobBoolParameterDefinition
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobBoolParameterDefinition {
    pub name: Identifier,
    pub description: Option<Description>,
    pub default: Option<BoolValue>,
    pub user_interface: Option<BoolUserInterface>,
}

/// A bool value that accepts: true/false, 0/1, 0.0/1.0, "true"/"false"/"yes"/"no"/"on"/"off"/"1"/"0"
#[derive(Debug, Clone)]
pub struct BoolValue(pub bool);

impl<'de> Deserialize<'de> for BoolValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let val = serde_yaml::Value::deserialize(deserializer)?;
        match &val {
            serde_yaml::Value::Bool(b) => Ok(BoolValue(*b)),
            serde_yaml::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    match i {
                        0 => Ok(BoolValue(false)),
                        1 => Ok(BoolValue(true)),
                        _ => Err(serde::de::Error::custom(format!("Invalid bool value: {i}"))),
                    }
                } else if let Some(f) = n.as_f64() {
                    if f == 0.0 {
                        Ok(BoolValue(false))
                    } else if f == 1.0 {
                        Ok(BoolValue(true))
                    } else {
                        Err(serde::de::Error::custom(format!("Invalid bool value: {f}")))
                    }
                } else {
                    Err(serde::de::Error::custom("Invalid bool value"))
                }
            }
            serde_yaml::Value::String(s) => match s.to_lowercase().as_str() {
                "true" | "yes" | "on" | "1" => Ok(BoolValue(true)),
                "false" | "no" | "off" | "0" => Ok(BoolValue(false)),
                _ => Err(serde::de::Error::custom(format!(
                    "Invalid bool value: '{s}'"
                ))),
            },
            _ => Err(serde::de::Error::custom("Invalid bool value")),
        }
    }
}

impl JobBoolParameterDefinition {
    pub fn check_value_constraints(&self, value: &ExprValue) -> Result<(), String> {
        match value {
            ExprValue::Bool(_) => Ok(()),
            ExprValue::Int(0) | ExprValue::Int(1) => Ok(()),
            ExprValue::String(s) => match s.to_lowercase().as_str() {
                "true" | "false" | "yes" | "no" | "on" | "off" | "1" | "0" => Ok(()),
                _ => Err(format!(
                    "Parameter '{}': value '{}' is not a valid bool",
                    self.name, s
                )),
            },
            _ => Err(format!(
                "Parameter '{}': expected bool, got {}",
                self.name,
                value.type_name()
            )),
        }
    }

    pub fn validate_definition(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if let Some(ui) = &self.user_interface {
            validate_ui(
                self.name.as_str(),
                &ui.label,
                &ui.group_label,
                &ui.control,
                &["CHECK_BOX", "HIDDEN"],
                &mut errors,
            );
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// §2.10 JobRangeExprParameterDefinition
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobRangeExprParameterDefinition {
    pub name: Identifier,
    pub description: Option<Description>,
    pub default: Option<String>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub user_interface: Option<RangeExprUserInterface>,
}

impl JobRangeExprParameterDefinition {
    pub fn check_value_constraints(&self, value: &ExprValue) -> Result<(), String> {
        let s = match value {
            ExprValue::RangeExpr(r) => r.to_string(),
            ExprValue::String(s) => {
                s.parse::<openjd_expr::RangeExpr>().map_err(|_| {
                    format!(
                        "Parameter '{}': value '{}' is not a valid range expression",
                        self.name, s
                    )
                })?;
                s.clone()
            }
            _ => {
                return Err(format!(
                    "Parameter '{}': expected range_expr, got {}",
                    self.name,
                    value.type_name()
                ))
            }
        };
        if let Some(min) = self.min_length {
            let char_len = s.chars().count();
            if char_len < min {
                return Err(format!(
                    "Parameter '{}': value length {} is less than minimum {min}",
                    self.name, char_len
                ));
            }
        }
        if let Some(max) = self.max_length {
            let char_len = s.chars().count();
            if char_len > max {
                return Err(format!(
                    "Parameter '{}': value length {} exceeds maximum {max}",
                    self.name, char_len
                ));
            }
        }
        Ok(())
    }

    pub fn validate_definition(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if let Some(default) = &self.default {
            if default.parse::<openjd_expr::RangeExpr>().is_err() {
                errors.push(format!(
                    "Parameter '{}': default '{}' is not a valid range expression.",
                    self.name, default
                ));
            }
        }
        if let Some(ui) = &self.user_interface {
            validate_ui(
                self.name.as_str(),
                &ui.label,
                &ui.group_label,
                &ui.control,
                &["LINE_EDIT", "HIDDEN"],
                &mut errors,
            );
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// §2.11 JobListStringParameterDefinition
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobListStringParameterDefinition {
    pub name: Identifier,
    pub description: Option<Description>,
    pub default: Option<Vec<String>>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub item: Option<ListStringItemConstraints>,
    pub user_interface: Option<ListSimpleUserInterface>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListStringItemConstraints {
    pub allowed_values: Option<Vec<String>>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
}

impl JobListStringParameterDefinition {
    pub fn check_value_constraints(&self, value: &ExprValue) -> Result<(), String> {
        let items = match value {
            ExprValue::ListString(v, _) | ExprValue::ListPath(v, _, _) => v,
            _ => {
                return Err(format!(
                    "Parameter '{}': expected list[string], got {}",
                    self.name,
                    value.type_name()
                ))
            }
        };
        check_list_length(&self.name, items.len(), self.min_length, self.max_length)?;
        if let Some(item) = &self.item {
            check_string_items(&self.name, items, item)?;
        }
        Ok(())
    }

    pub fn validate_definition(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        validate_list_length(
            &self.name,
            &self.default,
            self.min_length,
            self.max_length,
            &mut errors,
        );
        if let (Some(default), Some(item)) = (&self.default, &self.item) {
            validate_string_item_defaults(&self.name, default, item, &mut errors);
        }
        if let Some(ui) = &self.user_interface {
            validate_ui(
                self.name.as_str(),
                &ui.label,
                &ui.group_label,
                &ui.control,
                &["LINE_EDIT_LIST", "HIDDEN"],
                &mut errors,
            );
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// §2.12 JobListPathParameterDefinition
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobListPathParameterDefinition {
    pub name: Identifier,
    pub description: Option<Description>,
    pub object_type: Option<ObjectType>,
    pub data_flow: Option<DataFlow>,
    pub default: Option<Vec<String>>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub item: Option<ListStringItemConstraints>,
    pub user_interface: Option<ListPathUserInterface>,
}

impl JobListPathParameterDefinition {
    pub fn check_value_constraints(&self, value: &ExprValue) -> Result<(), String> {
        let items = match value {
            ExprValue::ListString(v, _) | ExprValue::ListPath(v, _, _) => v,
            _ => {
                return Err(format!(
                    "Parameter '{}': expected list[path], got {}",
                    self.name,
                    value.type_name()
                ))
            }
        };
        check_list_length(&self.name, items.len(), self.min_length, self.max_length)?;
        if let Some(item) = &self.item {
            check_string_items(&self.name, items, item)?;
        }
        Ok(())
    }

    pub fn validate_definition(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        validate_list_length(
            &self.name,
            &self.default,
            self.min_length,
            self.max_length,
            &mut errors,
        );
        if let (Some(default), Some(item)) = (&self.default, &self.item) {
            validate_string_item_defaults(&self.name, default, item, &mut errors);
        }
        if let Some(ui) = &self.user_interface {
            validate_ui(
                self.name.as_str(),
                &ui.label,
                &ui.group_label,
                &ui.control,
                &[
                    "CHOOSE_INPUT_FILE_LIST",
                    "CHOOSE_OUTPUT_FILE_LIST",
                    "CHOOSE_DIRECTORY_LIST",
                    "HIDDEN",
                ],
                &mut errors,
            );
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// §2.13 JobListIntParameterDefinition
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobListIntParameterDefinition {
    pub name: Identifier,
    pub description: Option<Description>,
    pub default: Option<Vec<FlexInt>>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub item: Option<ListIntItemConstraints>,
    pub user_interface: Option<ListIntUserInterface>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListIntItemConstraints {
    pub allowed_values: Option<Vec<FlexInt>>,
    pub min_value: Option<FlexInt>,
    pub max_value: Option<FlexInt>,
}

impl JobListIntParameterDefinition {
    pub fn check_value_constraints(&self, value: &ExprValue) -> Result<(), String> {
        let items = match value {
            ExprValue::ListInt(v) => v,
            _ => {
                return Err(format!(
                    "Parameter '{}': expected list[int], got {}",
                    self.name,
                    value.type_name()
                ))
            }
        };
        check_list_length(&self.name, items.len(), self.min_length, self.max_length)?;
        if let Some(item) = &self.item {
            check_int_items(&self.name, items, item, "item")?;
        }
        Ok(())
    }

    pub fn validate_definition(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        validate_list_length(
            &self.name,
            &self.default,
            self.min_length,
            self.max_length,
            &mut errors,
        );
        if let (Some(default), Some(item)) = (&self.default, &self.item) {
            validate_int_item_defaults(&self.name, default, item, "default", "item", &mut errors);
        }
        if let Some(ui) = &self.user_interface {
            validate_ui(
                self.name.as_str(),
                &ui.label,
                &ui.group_label,
                &ui.control,
                &["SPIN_BOX_LIST", "HIDDEN"],
                &mut errors,
            );
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// §2.14 JobListFloatParameterDefinition
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobListFloatParameterDefinition {
    pub name: Identifier,
    pub description: Option<Description>,
    pub default: Option<Vec<super::parameters::FlexFloat>>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub item: Option<ListFloatItemConstraints>,
    pub user_interface: Option<ListFloatUserInterface>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListFloatItemConstraints {
    pub allowed_values: Option<Vec<super::parameters::FlexFloat>>,
    pub min_value: Option<super::parameters::FlexFloat>,
    pub max_value: Option<super::parameters::FlexFloat>,
}

impl JobListFloatParameterDefinition {
    pub fn check_value_constraints(&self, value: &ExprValue) -> Result<(), String> {
        let items = match value {
            ExprValue::ListFloat(v) => v,
            _ => {
                return Err(format!(
                    "Parameter '{}': expected list[float], got {}",
                    self.name,
                    value.type_name()
                ))
            }
        };
        check_list_length(&self.name, items.len(), self.min_length, self.max_length)?;
        if let Some(item) = &self.item {
            for (i, v) in items.iter().enumerate() {
                if let Some(min) = &item.min_value {
                    if v.0 < min.0 {
                        return Err(format!(
                            "Parameter '{}': item[{i}] {} is less than minimum {}",
                            self.name, v, min.0
                        ));
                    }
                }
                if let Some(max) = &item.max_value {
                    if v.0 > max.0 {
                        return Err(format!(
                            "Parameter '{}': item[{i}] {} exceeds maximum {}",
                            self.name, v, max.0
                        ));
                    }
                }
                if let Some(allowed) = &item.allowed_values {
                    if !allowed.iter().any(|a| a.0 == v.0) {
                        return Err(format!(
                            "Parameter '{}': item[{i}] {} is not in allowed values",
                            self.name, v
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn validate_definition(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        validate_list_length(
            &self.name,
            &self.default,
            self.min_length,
            self.max_length,
            &mut errors,
        );
        if let (Some(default), Some(item)) = (&self.default, &self.item) {
            for (i, v) in default.iter().enumerate() {
                if let Some(min) = &item.min_value {
                    if v.0 < min.0 {
                        errors.push(format!(
                            "Parameter '{}': default[{i}] {} < item minValue {}.",
                            self.name, v.0, min.0
                        ));
                    }
                }
                if let Some(max) = &item.max_value {
                    if v.0 > max.0 {
                        errors.push(format!(
                            "Parameter '{}': default[{i}] {} > item maxValue {}.",
                            self.name, v.0, max.0
                        ));
                    }
                }
            }
        }
        if let Some(ui) = &self.user_interface {
            validate_ui(
                self.name.as_str(),
                &ui.label,
                &ui.group_label,
                &ui.control,
                &["SPIN_BOX_LIST", "HIDDEN"],
                &mut errors,
            );
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// §2.15 JobListBoolParameterDefinition
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobListBoolParameterDefinition {
    pub name: Identifier,
    pub description: Option<Description>,
    pub default: Option<Vec<BoolValue>>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub user_interface: Option<ListSimpleUserInterface>,
}

impl JobListBoolParameterDefinition {
    pub fn check_value_constraints(&self, value: &ExprValue) -> Result<(), String> {
        let items = match value {
            ExprValue::ListBool(v) => v,
            _ => {
                return Err(format!(
                    "Parameter '{}': expected list[bool], got {}",
                    self.name,
                    value.type_name()
                ))
            }
        };
        check_list_length(&self.name, items.len(), self.min_length, self.max_length)?;
        Ok(())
    }

    pub fn validate_definition(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        validate_list_length(
            &self.name,
            &self.default,
            self.min_length,
            self.max_length,
            &mut errors,
        );
        if let Some(ui) = &self.user_interface {
            validate_ui(
                self.name.as_str(),
                &ui.label,
                &ui.group_label,
                &ui.control,
                &["CHECK_BOX_LIST", "HIDDEN"],
                &mut errors,
            );
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// §2.16 JobListListIntParameterDefinition
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobListListIntParameterDefinition {
    pub name: Identifier,
    pub description: Option<Description>,
    pub default: Option<Vec<Vec<FlexInt>>>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub item: Option<ListListIntItemConstraints>,
    pub user_interface: Option<HiddenOnlyUserInterface>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListListIntItemConstraints {
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub item: Option<ListIntItemConstraints>,
}

impl JobListListIntParameterDefinition {
    pub fn check_value_constraints(&self, value: &ExprValue) -> Result<(), String> {
        let items = match value {
            ExprValue::ListList(v, _, _) => v,
            _ => {
                return Err(format!(
                    "Parameter '{}': expected list[list[int]], got {}",
                    self.name,
                    value.type_name()
                ))
            }
        };
        check_list_length(&self.name, items.len(), self.min_length, self.max_length)?;
        if let Some(item) = &self.item {
            for (i, inner) in items.iter().enumerate() {
                let ints = match inner {
                    ExprValue::ListInt(v) => v,
                    _ => {
                        return Err(format!(
                            "Parameter '{}': item[{i}] expected list[int], got {}",
                            self.name,
                            inner.type_name()
                        ))
                    }
                };
                if let Some(min) = item.min_length {
                    if ints.len() < min {
                        return Err(format!(
                            "Parameter '{}': item[{i}] length {} is less than minimum {min}",
                            self.name,
                            ints.len()
                        ));
                    }
                }
                if let Some(max) = item.max_length {
                    if ints.len() > max {
                        return Err(format!(
                            "Parameter '{}': item[{i}] length {} exceeds maximum {max}",
                            self.name,
                            ints.len()
                        ));
                    }
                }
                if let Some(inner_item) = &item.item {
                    check_int_items(&self.name, ints, inner_item, &format!("item[{i}]"))?;
                }
            }
        }
        Ok(())
    }

    pub fn validate_definition(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        validate_list_length(
            &self.name,
            &self.default,
            self.min_length,
            self.max_length,
            &mut errors,
        );
        if let (Some(default), Some(item)) = (&self.default, &self.item) {
            for (i, inner) in default.iter().enumerate() {
                if let Some(min) = item.min_length {
                    if inner.len() < min {
                        errors.push(format!("Parameter '{}': default[{i}] inner list length {} < item minLength {min}.", self.name, inner.len()));
                    }
                }
                if let Some(max) = item.max_length {
                    if inner.len() > max {
                        errors.push(format!("Parameter '{}': default[{i}] inner list length {} > item maxLength {max}.", self.name, inner.len()));
                    }
                }
                if let Some(inner_item) = &item.item {
                    validate_int_item_defaults(
                        &self.name,
                        inner,
                        inner_item,
                        &format!("default[{i}]"),
                        "item.item",
                        &mut errors,
                    );
                }
            }
        }
        if let Some(ui) = &self.user_interface {
            validate_ui(
                self.name.as_str(),
                &ui.label,
                &ui.group_label,
                &ui.control,
                &["HIDDEN"],
                &mut errors,
            );
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Helper: check list length against minLength/maxLength for runtime constraint checking.
fn check_list_length(
    param_name: &Identifier,
    len: usize,
    min_length: Option<usize>,
    max_length: Option<usize>,
) -> Result<(), String> {
    if let Some(min) = min_length {
        if len < min {
            return Err(format!(
                "Parameter '{param_name}': list length {len} is less than minimum {min}"
            ));
        }
    }
    if let Some(max) = max_length {
        if len > max {
            return Err(format!(
                "Parameter '{param_name}': list length {len} exceeds maximum {max}"
            ));
        }
    }
    Ok(())
}

/// Helper: validate list default length against minLength/maxLength.
fn validate_list_length<T>(
    param_name: &Identifier,
    default: &Option<Vec<T>>,
    min_length: Option<usize>,
    max_length: Option<usize>,
    errors: &mut Vec<String>,
) {
    if let Some(default) = default {
        if let Some(min) = min_length {
            if default.len() < min {
                errors.push(format!(
                    "Parameter '{param_name}': default list length {} < minLength {min}.",
                    default.len()
                ));
            }
        }
        if let Some(max) = max_length {
            if default.len() > max {
                errors.push(format!(
                    "Parameter '{param_name}': default list length {} > maxLength {max}.",
                    default.len()
                ));
            }
        }
    }
}

/// Shared UI validation: labels + control allowlist.
fn validate_ui(
    name: &str,
    label: &Option<String>,
    group_label: &Option<String>,
    control: &Option<String>,
    allowed_controls: &[&str],
    errors: &mut Vec<String>,
) {
    errors.extend(validate_ui_label(label, "label", name));
    errors.extend(validate_ui_label(group_label, "groupLabel", name));
    if let Some(c) = control {
        if !allowed_controls.contains(&c.as_str()) {
            errors.push(format!("Parameter '{name}': unknown control '{c}'."));
        }
    }
}

/// Helper: check string items against item constraints at runtime.
fn check_string_items(
    name: &Identifier,
    items: &[String],
    item: &ListStringItemConstraints,
) -> Result<(), String> {
    for (i, s) in items.iter().enumerate() {
        if let Some(min) = item.min_length {
            let char_len = s.chars().count();
            if char_len < min {
                return Err(format!(
                    "Parameter '{name}': item[{i}] length {} is less than minimum {min}",
                    char_len
                ));
            }
        }
        if let Some(max) = item.max_length {
            let char_len = s.chars().count();
            if char_len > max {
                return Err(format!(
                    "Parameter '{name}': item[{i}] length {} exceeds maximum {max}",
                    char_len
                ));
            }
        }
        if let Some(allowed) = &item.allowed_values {
            if !allowed.contains(s) {
                return Err(format!(
                    "Parameter '{name}': item[{i}] '{s}' is not in allowed values"
                ));
            }
        }
    }
    Ok(())
}

/// Helper: validate string item defaults against item constraints.
fn validate_string_item_defaults(
    name: &Identifier,
    default: &[String],
    item: &ListStringItemConstraints,
    errors: &mut Vec<String>,
) {
    for (i, v) in default.iter().enumerate() {
        if let Some(allowed) = &item.allowed_values {
            if !allowed.contains(v) {
                errors.push(format!(
                    "Parameter '{name}': default[{i}] '{v}' not in item allowedValues."
                ));
            }
        }
        if let Some(min) = item.min_length {
            let char_len = v.chars().count();
            if char_len < min {
                errors.push(format!(
                    "Parameter '{name}': default[{i}] length {} < item minLength {min}.",
                    char_len
                ));
            }
        }
        if let Some(max) = item.max_length {
            let char_len = v.chars().count();
            if char_len > max {
                errors.push(format!(
                    "Parameter '{name}': default[{i}] length {} > item maxLength {max}.",
                    char_len
                ));
            }
        }
    }
}

/// Helper: check int items against item constraints at runtime.
fn check_int_items(
    name: &Identifier,
    items: &[i64],
    item: &ListIntItemConstraints,
    prefix: &str,
) -> Result<(), String> {
    for (i, v) in items.iter().enumerate() {
        if let Some(min) = &item.min_value {
            if *v < min.0 {
                return Err(format!(
                    "Parameter '{name}': {prefix}[{i}] {v} is less than minimum {}",
                    min.0
                ));
            }
        }
        if let Some(max) = &item.max_value {
            if *v > max.0 {
                return Err(format!(
                    "Parameter '{name}': {prefix}[{i}] {v} exceeds maximum {}",
                    max.0
                ));
            }
        }
        if let Some(allowed) = &item.allowed_values {
            if !allowed.iter().any(|a| a.0 == *v) {
                return Err(format!(
                    "Parameter '{name}': {prefix}[{i}] {v} is not in allowed values"
                ));
            }
        }
    }
    Ok(())
}

/// Helper: validate int item defaults against item constraints.
fn validate_int_item_defaults(
    name: &Identifier,
    defaults: &[FlexInt],
    item: &ListIntItemConstraints,
    prefix: &str,
    constraint_label: &str,
    errors: &mut Vec<String>,
) {
    for (i, v) in defaults.iter().enumerate() {
        if let Some(min) = &item.min_value {
            if v.0 < min.0 {
                errors.push(format!(
                    "Parameter '{name}': {prefix}[{i}] {} < {constraint_label} minValue {}.",
                    v.0, min.0
                ));
            }
        }
        if let Some(max) = &item.max_value {
            if v.0 > max.0 {
                errors.push(format!(
                    "Parameter '{name}': {prefix}[{i}] {} > {constraint_label} maxValue {}.",
                    v.0, max.0
                ));
            }
        }
        if let Some(allowed) = &item.allowed_values {
            if !allowed.iter().any(|a| a.0 == v.0) {
                errors.push(format!(
                    "Parameter '{name}': {prefix}[{i}] {} not in {constraint_label} allowedValues.",
                    v.0
                ));
            }
        }
    }
}
