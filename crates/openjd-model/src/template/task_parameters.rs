// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Task parameter definitions per spec §3.4.

use super::constrained_strings::Identifier;
use super::parameters::FlexInt;
use crate::format_string::FormatString;
use serde::Deserialize;

/// §3.4.1 TaskParameterDefinition — discriminated union on `type`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[allow(non_camel_case_types)]
pub enum TaskParameterDefinition {
    INT(IntTaskParameterDefinition),
    FLOAT(FloatTaskParameterDefinition),
    STRING(StringTaskParameterDefinition),
    PATH(PathTaskParameterDefinition),
    #[serde(rename = "CHUNK[INT]")]
    CHUNK_INT(ChunkIntTaskParameterDefinition),
}

impl TaskParameterDefinition {
    pub fn task_param_type(&self) -> crate::types::TaskParameterType {
        use crate::types::TaskParameterType;
        match self {
            Self::INT(_) => TaskParameterType::Int,
            Self::FLOAT(_) => TaskParameterType::Float,
            Self::STRING(_) => TaskParameterType::String,
            Self::PATH(_) => TaskParameterType::Path,
            Self::CHUNK_INT(_) => TaskParameterType::ChunkInt,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::INT(p) => p.name.as_str(),
            Self::FLOAT(p) => p.name.as_str(),
            Self::STRING(p) => p.name.as_str(),
            Self::PATH(p) => p.name.as_str(),
            Self::CHUNK_INT(p) => p.name.as_str(),
        }
    }
}

/// Int range: either a list of values or a range expression string.
#[derive(Debug, Clone)]
pub enum IntRange {
    List(Vec<FlexInt>),
    Expression(FormatString),
}

impl<'de> Deserialize<'de> for IntRange {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let val = serde_json::Value::deserialize(deserializer)?;
        match val {
            serde_json::Value::Array(seq) => {
                let items: Result<Vec<FlexInt>, _> = seq
                    .into_iter()
                    .map(|v| serde_json::from_value(v).map_err(serde::de::Error::custom))
                    .collect();
                Ok(IntRange::List(items?))
            }
            serde_json::Value::String(s) => FormatString::new(&s)
                .map(IntRange::Expression)
                .map_err(serde::de::Error::custom),
            _ => Err(serde::de::Error::custom(
                "Expected list or string for range",
            )),
        }
    }
}

/// Range that can be a list or a single expression string (EXPR extension).
/// Concrete types to avoid derive conflicts with FormatString.

#[derive(Debug, Clone)]
pub enum StringRange {
    List(Vec<FormatString>),
    Expression(FormatString),
}

impl<'de> Deserialize<'de> for StringRange {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let val = serde_json::Value::deserialize(deserializer)?;
        match &val {
            serde_json::Value::Array(_) => {
                let items: Vec<FormatString> =
                    serde_json::from_value(val).map_err(serde::de::Error::custom)?;
                Ok(StringRange::List(items))
            }
            serde_json::Value::String(s) => FormatString::new(s)
                .map(StringRange::Expression)
                .map_err(serde::de::Error::custom),
            _ => Err(serde::de::Error::custom(
                "Expected list or string for range",
            )),
        }
    }
}

/// A float range list item: either a literal float or a format string.
#[derive(Debug, Clone)]
pub enum FloatRangeItem {
    Float(f64),
    FormatString(FormatString),
}

impl<'de> Deserialize<'de> for FloatRangeItem {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let val = serde_json::Value::deserialize(deserializer)?;
        match &val {
            serde_json::Value::Number(n) => {
                let f = n
                    .as_f64()
                    .ok_or_else(|| serde::de::Error::custom("Invalid number in float range"))?;
                super::parameters::reject_nan_inf(f).map_err(serde::de::Error::custom)?;
                Ok(FloatRangeItem::Float(f))
            }
            serde_json::Value::String(s) => FormatString::new(s)
                .map(FloatRangeItem::FormatString)
                .map_err(serde::de::Error::custom),
            _ => Err(serde::de::Error::custom(
                "Expected number or string in float range",
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub enum FloatRange {
    List(Vec<FloatRangeItem>),
    Expression(FormatString),
}

impl<'de> Deserialize<'de> for FloatRange {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let val = serde_json::Value::deserialize(deserializer)?;
        match &val {
            serde_json::Value::Array(_) => {
                let items: Vec<FloatRangeItem> =
                    serde_json::from_value(val).map_err(serde::de::Error::custom)?;
                Ok(FloatRange::List(items))
            }
            serde_json::Value::String(s) => FormatString::new(s)
                .map(FloatRange::Expression)
                .map_err(serde::de::Error::custom),
            _ => Err(serde::de::Error::custom(
                "Expected list or string for range",
            )),
        }
    }
}

/// §3.4.1.1 IntTaskParameterDefinition
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IntTaskParameterDefinition {
    pub name: Identifier,
    pub range: IntRange,
}

/// §3.4.1.2 FloatTaskParameterDefinition
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FloatTaskParameterDefinition {
    pub name: Identifier,
    pub range: FloatRange,
}

/// §3.4.1.3 StringTaskParameterDefinition
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StringTaskParameterDefinition {
    pub name: Identifier,
    pub range: StringRange,
}

/// §3.4.1.4 PathTaskParameterDefinition
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PathTaskParameterDefinition {
    pub name: Identifier,
    pub range: StringRange,
}

/// §3.4.1.5 ChunkIntTaskParameterDefinition (TASK_CHUNKING extension)
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChunkIntTaskParameterDefinition {
    pub name: Identifier,
    pub range: IntRange,
    pub chunks: ChunksDefinition,
}

/// An integer value or a format string (e.g. `{{Param.ChunkSize}}`).
///
/// Accepts:
/// - YAML integer → `IntOrFormatString::Int(n)`
/// - String that parses as i64 → `IntOrFormatString::Int(n)`
/// - String containing `{{…}}` → `IntOrFormatString::FormatString(fs)`
/// - Boolean/null → error
#[derive(Debug, Clone)]
pub enum IntOrFormatString {
    Int(i64),
    FormatString(FormatString),
}

impl IntOrFormatString {
    /// Return the integer value if this is a literal, or `None` if it's a format string.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(n) => Some(*n),
            Self::FormatString(_) => None,
        }
    }
}

impl<'de> Deserialize<'de> for IntOrFormatString {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let val = serde_json::Value::deserialize(deserializer)?;
        match &val {
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(Self::Int(i))
                } else if let Some(f) = n.as_f64() {
                    if f.fract() == 0.0 {
                        Ok(Self::Int(f as i64))
                    } else {
                        Err(serde::de::Error::custom(format!(
                            "Expected integer, got float: {f}"
                        )))
                    }
                } else {
                    Err(serde::de::Error::custom("Invalid number"))
                }
            }
            serde_json::Value::String(s) => {
                // If it contains format string interpolation, treat as FormatString
                if s.contains("{{") {
                    FormatString::new(s)
                        .map(Self::FormatString)
                        .map_err(serde::de::Error::custom)
                } else {
                    // Try parsing as integer
                    s.trim().parse::<i64>().map(Self::Int).map_err(|_| {
                        serde::de::Error::custom(format!("Cannot parse '{s}' as integer"))
                    })
                }
            }
            serde_json::Value::Bool(_) => {
                Err(serde::de::Error::custom("Expected integer, got boolean"))
            }
            serde_json::Value::Null => Err(serde::de::Error::custom("Expected integer, got null")),
            _ => Err(serde::de::Error::custom("Expected integer or string")),
        }
    }
}

/// Chunks configuration for `CHUNK[INT]` parameters.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChunksDefinition {
    pub default_task_count: IntOrFormatString,
    pub target_runtime_seconds: Option<IntOrFormatString>,
    pub range_constraint: RangeConstraint,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RangeConstraint {
    Contiguous,
    Noncontiguous,
}

/// §3.4 StepParameterSpaceDefinition
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepParameterSpaceDefinition {
    pub task_parameter_definitions: Vec<TaskParameterDefinition>,
    pub combination: Option<String>,
}
