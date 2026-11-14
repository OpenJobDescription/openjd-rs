// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! The host encoding: how every host document writes a job parameter, a
//! task parameter definition and a parameter space, and how
//! [`job_from_str`](crate::job::template_diff::job_from_str) reads them back.
//! See `specs/model/host-support.md`, "Host encoding".
//!
//! A typed value is `{"type": <spec type name>, "value": <transport value>}`:
//! scalars as text, lists as arrays, as a symbol table writes them. Nothing
//! internal to the Rust types (cached sizes, type descriptors, variant
//! names) is written, so value-equal jobs write the same document.

use indexmap::IndexMap;
use openjd_expr::path_mapping::PathFormat;
use openjd_expr::value::Float64;
use openjd_expr::{ExprType, ExprValue, RangeExpr};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::job::step_param_space::tokenize;
use crate::job::{JobParameter, ResolvedChunks, StepParameterSpace, TaskParamRange, TaskParameter};
use crate::template::RangeConstraint;
use crate::types::JobParameterType;

/// A value's transport form: text for a scalar, an array for a list. A range
/// expression is written in canonical form.
fn transport(value: &ExprValue) -> Value {
    match value {
        ExprValue::RangeExpr(r) => Value::String(canonical(r)),
        v => v.transport_value(),
    }
}

/// The canonical text of a range expression, whatever its display flag.
fn canonical(r: &RangeExpr) -> String {
    r.clone().with_contiguous(false).to_string()
}

/// A job parameter: `{"name", "type", "value"}`.
#[derive(Serialize)]
pub(crate) struct ParameterDoc<'a> {
    name: &'a str,
    #[serde(rename = "type")]
    ty: &'static str,
    value: Value,
}

impl<'a> ParameterDoc<'a> {
    pub(crate) fn new(p: &'a JobParameter) -> Self {
        let JobParameter {
            name,
            param_type,
            value,
        } = p;
        Self {
            name,
            ty: param_type.as_spec_str(),
            value: transport(value),
        }
    }
}

/// A job parameter's value alone, as its document's `value` holds it.
pub(crate) fn parameter_value(value: &ExprValue) -> Value {
    transport(value)
}

/// A task parameter definition: `{"name"?, "type", "range" | "list",
/// "chunks"?}`.
#[derive(Serialize)]
pub(crate) struct DefinitionDoc<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(rename = "type")]
    ty: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    range: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    list: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    chunks: Option<ChunksDoc>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChunksDoc {
    default_task_count: u64,
    target_runtime_seconds: Option<u64>,
    range_constraint: RangeConstraint,
}

impl ChunksDoc {
    fn new(c: &ResolvedChunks) -> Self {
        let ResolvedChunks {
            default_task_count,
            target_runtime_seconds,
            range_constraint,
        } = c;
        Self {
            default_task_count: *default_task_count,
            target_runtime_seconds: *target_runtime_seconds,
            range_constraint: range_constraint.clone(),
        }
    }
}

impl<'a> DefinitionDoc<'a> {
    /// The definition, with `name` first when given.
    pub(crate) fn new(name: Option<&'a str>, p: &'a TaskParameter) -> Self {
        let ints = |r: &TaskParamRange<i64>| match r {
            TaskParamRange::RangeExpr(r) => (Some(canonical(r)), None),
            TaskParamRange::List(v) => (None, Some(v.iter().map(i64::to_string).collect())),
        };
        let (ty, (range, list), chunks) = match p {
            TaskParameter::Int { range, chunks } => {
                ("INT", ints(range), chunks.as_ref().map(ChunksDoc::new))
            }
            TaskParameter::ChunkInt { range, chunks } => {
                ("CHUNK[INT]", ints(range), Some(ChunksDoc::new(chunks)))
            }
            TaskParameter::Float { range } => (
                "FLOAT",
                (
                    None,
                    Some(range.iter().map(Float64::to_display_string).collect()),
                ),
                None,
            ),
            TaskParameter::String { range } => ("STRING", (None, Some(range.clone())), None),
            TaskParameter::Path { range } => ("PATH", (None, Some(range.clone())), None),
        };
        Self {
            name,
            ty,
            range,
            list,
            chunks,
        }
    }
}

/// A parameter space: `{"taskParameterDefinitions": [<definition with
/// name>, …], "combination"}`, the definitions in definition order.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpaceDoc<'a> {
    task_parameter_definitions: Vec<DefinitionDoc<'a>>,
    combination: &'a Option<String>,
}

impl<'a> SpaceDoc<'a> {
    pub(crate) fn new(space: &'a StepParameterSpace) -> Self {
        let StepParameterSpace {
            task_parameter_definitions,
            combination,
        } = space;
        Self {
            task_parameter_definitions: task_parameter_definitions
                .iter()
                .map(|(n, p)| DefinitionDoc::new(Some(n), p))
                .collect(),
            combination,
        }
    }
}

/// A [`Value`] of a document, for the diff's field changes.
pub(crate) fn to_value(doc: impl Serialize) -> Value {
    serde_json::to_value(doc).unwrap_or_else(|e| Value::String(format!("<not serializable: {e}>")))
}

// ─── reading ────────────────────────────────────────────────────────

fn object<'v>(v: &'v Value, path: &str) -> Result<&'v Map<String, Value>, String> {
    v.as_object()
        .ok_or_else(|| format!("{path}: expected an object"))
}

/// Reject a key not in `allowed`, and a missing one in `required`.
fn keys(
    m: &Map<String, Value>,
    path: &str,
    allowed: &[&str],
    required: &[&str],
) -> Result<(), String> {
    if let Some(k) = m.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(format!("{path}.{k}: unknown field"));
    }
    if let Some(k) = required.iter().find(|k| !m.contains_key(**k)) {
        return Err(format!("{path}: missing field '{k}'"));
    }
    Ok(())
}

fn string<'v>(m: &'v Map<String, Value>, key: &str, path: &str) -> Result<&'v str, String> {
    m[key]
        .as_str()
        .ok_or_else(|| format!("{path}.{key}: expected a string"))
}

/// The type a stored value of `t` has: `create_job` stores a PATH as a
/// STRING and a LIST[PATH] as a LIST[STRING].
fn storage_type(t: JobParameterType) -> ExprType {
    match t {
        JobParameterType::Path => ExprType::STRING,
        JobParameterType::ListPath => ExprType::list(ExprType::STRING),
        other => other.expr_type(),
    }
}

/// `Err` unless `given` is exactly `written`, the form a host document
/// writes the value it was read as: the reader takes only that form, so
/// `"TRUE"` is not read as the BOOL `true`, nor `" 1-4 "` as the range
/// `1-4`.
fn canonical_form(given: &Value, written: &Value, path: &str) -> Result<(), String> {
    if given == written {
        return Ok(());
    }
    Err(format!(
        "{path}: {given} is not in the form a built job writes ({written})"
    ))
}

/// The parameter stored under `key`, from its document. `path` names it in
/// errors.
pub(crate) fn parameter_from_document(
    key: &str,
    v: &Value,
    path: &str,
) -> Result<JobParameter, String> {
    let m = object(v, path)?;
    keys(
        m,
        path,
        &["name", "type", "value"],
        &["name", "type", "value"],
    )?;
    let name = string(m, "name", path)?;
    if name != key {
        return Err(format!(
            "{path}.name: '{name}', but the parameter's key is '{key}'"
        ));
    }
    let type_name = string(m, "type", path)?;
    let param_type = JobParameterType::from_spec_str(type_name)
        .filter(|t| t.as_spec_str() == type_name)
        .ok_or_else(|| format!("{path}.type: unknown parameter type '{type_name}'"))?;
    let value =
        ExprValue::from_transport_value(&m["value"], &storage_type(param_type), PathFormat::Posix)
            .map_err(|e| format!("{path}.value: {e}"))?;
    canonical_form(&m["value"], &transport(&value), &format!("{path}.value"))?;
    Ok(JobParameter {
        name: name.to_string(),
        param_type,
        value,
    })
}

fn texts<'v>(v: &'v Value, path: &str) -> Result<Vec<&'v str>, String> {
    v.as_array()
        .ok_or_else(|| format!("{path}: expected a list"))?
        .iter()
        .enumerate()
        .map(|(i, e)| {
            e.as_str()
                .ok_or_else(|| format!("{path}[{i}]: expected a string"))
        })
        .collect()
}

fn int_range(m: &Map<String, Value>, path: &str) -> Result<TaskParamRange<i64>, String> {
    if let Some(r) = m.get("range") {
        let text = r
            .as_str()
            .ok_or_else(|| format!("{path}.range: expected a string"))?;
        let range = text
            .parse::<RangeExpr>()
            .map_err(|e| format!("{path}.range: {e}"))?;
        canonical_form(
            r,
            &Value::String(canonical(&range)),
            &format!("{path}.range"),
        )?;
        return Ok(TaskParamRange::RangeExpr(range));
    }
    texts(&m["list"], &format!("{path}.list"))?
        .into_iter()
        .enumerate()
        .map(|(i, s)| {
            let at = format!("{path}.list[{i}]");
            let n = s
                .parse::<i64>()
                .map_err(|_| format!("{at}: '{s}' is not an INT"))?;
            canonical_form(&Value::from(s), &Value::String(n.to_string()), &at)?;
            Ok(n)
        })
        .collect::<Result<_, String>>()
        .map(TaskParamRange::List)
}

fn chunks(v: &Value, path: &str) -> Result<ResolvedChunks, String> {
    let m = object(v, path)?;
    let all = [
        "defaultTaskCount",
        "targetRuntimeSeconds",
        "rangeConstraint",
    ];
    keys(m, path, &all, &["defaultTaskCount", "rangeConstraint"])?;
    let count = |k: &str| {
        m[k].as_u64()
            .ok_or_else(|| format!("{path}.{k}: expected a non-negative integer"))
    };
    let default_task_count = count("defaultTaskCount")?;
    if default_task_count == 0 {
        return Err(format!("{path}.defaultTaskCount: must be at least 1"));
    }
    let target_runtime_seconds = match m.get("targetRuntimeSeconds") {
        None | Some(Value::Null) => None,
        Some(_) => Some(count("targetRuntimeSeconds")?),
    };
    let range_constraint = RangeConstraint::deserialize(&m["rangeConstraint"])
        .map_err(|e| format!("{path}.rangeConstraint: {e}"))?;
    Ok(ResolvedChunks {
        default_task_count,
        target_runtime_seconds,
        range_constraint,
    })
}

/// A task parameter definition, from its document. `with_name` says
/// whether the document holds the definition's `name` (an element of a
/// space's list), which is returned.
pub(crate) fn definition_from_document(
    v: &Value,
    path: &str,
    with_name: bool,
) -> Result<(Option<String>, TaskParameter), String> {
    let m = object(v, path)?;
    let type_name = m
        .get("type")
        .ok_or_else(|| format!("{path}: missing field 'type'"))?
        .as_str()
        .ok_or_else(|| format!("{path}.type: expected a string"))?;
    let ranged = matches!(type_name, "INT" | "CHUNK[INT]");
    let mut allowed = vec!["type"];
    allowed.push(if ranged && m.contains_key("range") {
        "range"
    } else {
        "list"
    });
    if ranged {
        allowed.push("chunks");
    }
    if with_name {
        allowed.push("name");
    }
    let mut required = vec!["type", allowed[1]];
    if type_name == "CHUNK[INT]" {
        required.push("chunks");
    }
    if with_name {
        required.push("name");
    }
    if !matches!(
        type_name,
        "INT" | "CHUNK[INT]" | "FLOAT" | "STRING" | "PATH"
    ) {
        return Err(format!(
            "{path}.type: unknown task parameter type '{type_name}'"
        ));
    }
    keys(m, path, &allowed, &required)?;
    if m.get("list")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
    {
        return Err(format!("{path}.list: must not be empty"));
    }
    let name = if with_name {
        Some(string(m, "name", path)?.to_string())
    } else {
        None
    };
    let list = |p: &str| texts(&m["list"], p);
    let at = format!("{path}.list");
    let p = match type_name {
        "INT" => TaskParameter::Int {
            range: int_range(m, path)?,
            chunks: m
                .get("chunks")
                .filter(|c| !c.is_null())
                .map(|c| chunks(c, &format!("{path}.chunks")))
                .transpose()?,
        },
        "CHUNK[INT]" => TaskParameter::ChunkInt {
            range: int_range(m, path)?,
            chunks: chunks(&m["chunks"], &format!("{path}.chunks"))?,
        },
        "FLOAT" => TaskParameter::Float {
            range: list(&at)?
                .into_iter()
                .enumerate()
                .map(|(i, s)| {
                    match ExprValue::from_transport_value(
                        &Value::String(s.to_string()),
                        &ExprType::FLOAT,
                        PathFormat::Posix,
                    ) {
                        Ok(ExprValue::Float(f)) => {
                            canonical_form(
                                &Value::from(s),
                                &Value::String(f.to_display_string()),
                                &format!("{at}[{i}]"),
                            )?;
                            Ok(f)
                        }
                        _ => Err(format!("{at}[{i}]: '{s}' is not a FLOAT")),
                    }
                })
                .collect::<Result<_, _>>()?,
        },
        "STRING" => TaskParameter::String {
            range: list(&at)?.into_iter().map(str::to_string).collect(),
        },
        _ => TaskParameter::Path {
            range: list(&at)?.into_iter().map(str::to_string).collect(),
        },
    };
    Ok((name, p))
}

/// A parameter space, from its document. `path` names it in errors.
pub(crate) fn space_from_document(v: &Value, path: &str) -> Result<StepParameterSpace, String> {
    let m = object(v, path)?;
    let all = ["taskParameterDefinitions", "combination"];
    keys(m, path, &all, &["taskParameterDefinitions"])?;
    let defs_path = format!("{path}.taskParameterDefinitions");
    let defs = m["taskParameterDefinitions"]
        .as_array()
        .ok_or_else(|| format!("{defs_path}: expected a list"))?;
    if defs.is_empty() {
        return Err(format!("{defs_path}: must not be empty"));
    }
    let mut task_parameter_definitions = IndexMap::new();
    for (i, d) in defs.iter().enumerate() {
        let at = format!("{defs_path}[{i}]");
        let (name, p) = definition_from_document(d, &at, true)?;
        let name = name.unwrap_or_default();
        if task_parameter_definitions.contains_key(&name) {
            return Err(format!("{at}.name: '{name}' is repeated"));
        }
        task_parameter_definitions.insert(name, p);
    }
    let combination = match m.get("combination") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => return Err(format!("{path}.combination: expected a string")),
    };
    // Only the names are checked: `create_job` writes a combination the
    // template validated, so a name it does not define is not one it wrote.
    let operator = |t: &str| matches!(t, "*" | "(" | ")" | ",");
    if let Some(unknown) = combination
        .as_deref()
        .map(tokenize)
        .into_iter()
        .flatten()
        .find(|t| !operator(t) && !task_parameter_definitions.contains_key(t))
    {
        return Err(format!(
            "{path}.combination: '{unknown}' is not a task parameter of the step"
        ));
    }
    if let Some(c) = &combination {
        let names = task_parameter_definitions.keys().cloned().collect();
        if let Some(e) = crate::template::validate_v2023_09::combination_error(c, &names) {
            return Err(format!("{path}.combination: {e}"));
        }
    }
    Ok(StepParameterSpace {
        task_parameter_definitions,
        combination,
    })
}
