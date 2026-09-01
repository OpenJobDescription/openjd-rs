// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Instantiated job types — the result of `create_job()`.
//!
//! These types represent a fully resolved job where all format strings
//! have been evaluated and template variables substituted. Contrast with
//! `crate::template` which holds the unresolved template types.
//!
//! # Equality and hashing
//!
//! All types in this module implement `PartialEq` and `Hash` with the
//! invariant `a == b ⇒ hash(a) == hash(b)`. Equality is structural on
//! the *created job*, not the source template: derived state such as
//! `resolved_symtab` participates, and since its transport format
//! preserves original float literals, jobs created from `1.0` vs `1.00`
//! parameter values compare unequal. Map-typed fields (`IndexMap`,
//! `HashMap`) compare order-insensitively, so their `Hash` impls are
//! written by hand to hash entries sorted by key. `f64` fields hash via
//! `to_bits()` after normalizing `-0.0` to `0.0`, consistent with
//! `-0.0 == 0.0`. Types with `f64` fields implement `PartialEq` but not
//! `Eq`.

pub mod create_job;
pub mod step_dependency_graph;
pub mod step_param_space;

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use indexmap::IndexMap;
use openjd_expr::format_string::FormatString;
use openjd_expr::symbol_table::SerializedSymbolTable;
use openjd_expr::ExprValue;
use openjd_expr::RangeExpr;
use serde::{Deserialize, Serialize};

use crate::types::{EndOfLine, FileType};

use crate::template::RangeConstraint;
use crate::types::JobParameterType;

/// Hash the entries of a string-keyed map sorted by key, so that maps
/// that compare equal (order-insensitively) hash identically regardless
/// of insertion order.
fn hash_map_entries<K: AsRef<str>, V: Hash, H: Hasher>(
    entries: impl Iterator<Item = (K, V)>,
    state: &mut H,
) {
    let mut entries: Vec<_> = entries.collect();
    entries.sort_unstable_by(|(a, _), (b, _)| a.as_ref().cmp(b.as_ref()));
    entries.len().hash(state);
    for (k, v) in entries {
        k.as_ref().hash(state);
        v.hash(state);
    }
}

/// Hash an `f64` via its bit pattern, normalizing `-0.0` to `0.0` so the
/// hash is consistent with `-0.0 == 0.0` under `PartialEq`.
fn hash_f64<H: Hasher>(v: f64, state: &mut H) {
    let v = if v == 0.0 { 0.0 } else { v };
    v.to_bits().hash(state);
}

/// A fully instantiated job — all format strings resolved, parameters bound.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub name: String,
    pub description: Option<String>,
    pub extensions: Option<Vec<crate::types::ModelExtension>>,
    pub parameters: IndexMap<String, JobParameter>,
    pub steps: Vec<Step>,
    pub job_environments: Option<Vec<Environment>>,
}

/// Manual because `IndexMap` has no `Hash`; parameters hash as
/// key-sorted entries to match `IndexMap`'s order-insensitive equality.
impl Hash for Job {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.description.hash(state);
        self.extensions.hash(state);
        hash_map_entries(self.parameters.iter(), state);
        self.steps.hash(state);
        self.job_environments.hash(state);
    }
}

/// A resolved job parameter (name + type + bound value).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobParameter {
    pub name: String,
    pub param_type: JobParameterType,
    pub value: ExprValue,
}

/// A fully instantiated step.
#[derive(Debug, Clone, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub name: String,
    pub description: Option<String>,
    pub script: StepScript,
    pub step_environments: Option<Vec<Environment>>,
    pub parameter_space: Option<StepParameterSpace>,
    pub host_requirements: Option<HostRequirements>,
    pub dependencies: Option<Vec<StepDependency>>,
    /// Complete symbol table at step scope in JSON transport format.
    /// Contains Param.*, RawParam.*, Job.Name, Step.Name, and step-level let bindings.
    /// The session deserializes this with PathFormat::host() and layers
    /// Session.* and Task.* values on top at runtime.
    #[serde(rename = "resolvedSymTab", skip_serializing_if = "Option::is_none")]
    pub resolved_symtab: Option<SerializedSymbolTable>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepScript {
    #[serde(rename = "let", alias = "letBindings")]
    pub let_bindings: Option<Vec<String>>,
    pub actions: StepActions,
    pub embedded_files: Option<Vec<EmbeddedFile>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepActions {
    pub on_run: Action,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub command: FormatString,
    pub args: Option<Vec<FormatString>>,
    pub timeout: Option<FormatString>,
    pub cancelation: Option<CancelationMode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub name: String,
    pub description: Option<String>,
    pub script: Option<EnvironmentScript>,
    pub variables: Option<HashMap<String, FormatString>>,
    /// Filtered symbol table containing only symbols referenced by this
    /// environment's format strings (variables, actions, embedded files, let bindings).
    #[serde(rename = "resolvedSymTab", skip_serializing_if = "Option::is_none")]
    pub resolved_symtab: Option<SerializedSymbolTable>,
}

/// Manual because `HashMap` has no `Hash`; `variables` hashes as
/// key-sorted entries to match `HashMap`'s order-insensitive equality.
impl Hash for Environment {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.description.hash(state);
        self.script.hash(state);
        match &self.variables {
            None => false.hash(state),
            Some(vars) => {
                true.hash(state);
                hash_map_entries(vars.iter(), state);
            }
        }
        self.resolved_symtab.hash(state);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvironmentScript {
    #[serde(rename = "let", alias = "letBindings")]
    pub let_bindings: Option<Vec<String>>,
    pub actions: EnvironmentActions,
    pub embedded_files: Option<Vec<EmbeddedFile>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentActions {
    pub on_enter: Option<Action>,
    /// RFC 0008 — wraps inner environments' `onEnter` actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_wrap_env_enter: Option<Action>,
    /// RFC 0008 — wraps tasks' `onRun` actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_wrap_task_run: Option<Action>,
    /// RFC 0008 — wraps inner environments' `onExit` actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_wrap_env_exit: Option<Action>,
    pub on_exit: Option<Action>,
}

crate::template::impl_environment_actions_helpers!(EnvironmentActions, Action);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedFile {
    pub name: String,
    #[serde(alias = "type")]
    pub file_type: FileType,
    pub filename: Option<String>,
    pub data: Option<FormatString>,
    pub runnable: Option<bool>,
    pub end_of_line: Option<EndOfLine>,
}

/// §5.3 CancelationMethod — discriminated union on `mode`.
///
/// `DeferredMode` carries a format-string `mode` (FEATURE_BUNDLE_1) whose
/// TERMINATE-vs-NOTIFY_THEN_TERMINATE decision is made at run time, right
/// before the action launches (in short: `mode` is the schema selector,
/// so it normally must be known at parse time, but a forwarded value like
/// `{{WrappedAction.Cancelation.Mode}}` only exists at run time — see
/// `specs/model/template-types.md` § CancelationMode for the full design
/// rationale). A `null` resolution (whole-field expressions only) means
/// the whole cancelation object is treated as never declared.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CancelationMode {
    Terminate,
    NotifyThenTerminate {
        notify_period_in_seconds: Option<FormatString>,
    },
    DeferredMode {
        mode: FormatString,
        notify_period_in_seconds: Option<FormatString>,
    },
}

// Manual serde impls: the wire shape is `{"mode": <string>, ...}` where a
// DeferredMode's `mode` is the raw format string. A serde `tag = "mode"`
// representation cannot express that (the tag would collide with the
// variant's own `mode` field), so both directions are hand-written.
impl Serialize for CancelationMode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        match self {
            CancelationMode::Terminate => {
                map.serialize_entry("mode", "TERMINATE")?;
            }
            CancelationMode::NotifyThenTerminate {
                notify_period_in_seconds,
            } => {
                map.serialize_entry("mode", "NOTIFY_THEN_TERMINATE")?;
                if let Some(n) = notify_period_in_seconds {
                    map.serialize_entry("notifyPeriodInSeconds", n)?;
                }
            }
            CancelationMode::DeferredMode {
                mode,
                notify_period_in_seconds,
            } => {
                map.serialize_entry("mode", mode)?;
                if let Some(n) = notify_period_in_seconds {
                    map.serialize_entry("notifyPeriodInSeconds", n)?;
                }
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for CancelationMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use std::collections::HashMap;
        let map = HashMap::<String, serde_json::Value>::deserialize(deserializer)?;
        let mode_value = map
            .get("mode")
            .ok_or_else(|| serde::de::Error::missing_field("mode"))?;
        let mode = mode_value
            .as_str()
            .ok_or_else(|| serde::de::Error::custom("`mode` must be a string"))?;
        let deny_extra = |allowed: &[&str]| -> Result<(), D::Error> {
            if let Some(extra) = map.keys().find(|k| !allowed.contains(&k.as_str())) {
                return Err(serde::de::Error::custom(format!("unknown field `{extra}`")));
            }
            Ok(())
        };
        // An explicit null is treated as "not provided": the previous
        // derived impl serialized an unset period as
        // `"notifyPeriodInSeconds": null`, so documents written by released
        // versions must read back as None rather than failing.
        let notify = || -> Result<Option<FormatString>, D::Error> {
            map.get("notifyPeriodInSeconds")
                .filter(|v| !v.is_null())
                .map(|v| FormatString::deserialize(v.clone()))
                .transpose()
                .map_err(serde::de::Error::custom)
        };
        match mode {
            "TERMINATE" => {
                deny_extra(&["mode"])?;
                Ok(CancelationMode::Terminate)
            }
            "NOTIFY_THEN_TERMINATE" => {
                deny_extra(&["mode", "notifyPeriodInSeconds"])?;
                Ok(CancelationMode::NotifyThenTerminate {
                    notify_period_in_seconds: notify()?,
                })
            }
            other if other.contains("{{") => {
                deny_extra(&["mode", "notifyPeriodInSeconds"])?;
                let mode = FormatString::deserialize(mode_value.clone())
                    .map_err(serde::de::Error::custom)?;
                Ok(CancelationMode::DeferredMode {
                    mode,
                    notify_period_in_seconds: notify()?,
                })
            }
            other => Err(serde::de::Error::custom(format!(
                "unknown variant `{other}`, expected `TERMINATE` or `NOTIFY_THEN_TERMINATE`"
            ))),
        }
    }
}

/// Resolved parameter space with concrete ranges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepParameterSpace {
    pub task_parameter_definitions: IndexMap<String, TaskParameter>,
    pub combination: Option<String>,
}

/// Manual because `IndexMap` has no `Hash`; definitions hash as
/// key-sorted entries to match `IndexMap`'s order-insensitive equality.
impl Hash for StepParameterSpace {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_map_entries(self.task_parameter_definitions.iter(), state);
        self.combination.hash(state);
    }
}

/// A resolved `<FloatRangeList>` element.
///
/// Template Schemas §7.5 keeps the decimal places a `<floatstring>` was written
/// with, and `2.50_f64` cannot carry them. A `<float>` literal makes no such
/// request and stores `None`, rendering through
/// `openjd_expr::value::format_float`.
///
/// `text` is the rendered form, not the source: it is trimmed and has its
/// redundant leading zeros stripped before being stored, so `'02.50'` arrives as
/// `2.50`. Construct through `new`/`with_text`, which hold that invariant.
#[derive(Debug, Clone)]
pub struct FloatRangeValue {
    /// The number the element denotes.
    pub value: f64,
    /// How to render it, when the element was written as a `<floatstring>`.
    pub text: Option<String>,
}

impl FloatRangeValue {
    /// A value with no rendering of its own — a `<float>` literal.
    pub fn new(value: f64) -> Self {
        Self {
            value: normalize_zero(value),
            text: None,
        }
    }

    /// A value that renders as `text` — a `<floatstring>`.
    ///
    /// `text` is dropped when it would not render the value: zero has no sign, so
    /// `"-0.00"` is stored as `"0.00"` and `"1e-400"`, which parses to zero by
    /// underflow, keeps no text at all. Both would otherwise render a string that
    /// disagrees with `value`.
    pub fn with_text(value: f64, text: impl Into<String>) -> Self {
        let value = normalize_zero(value);
        let text = text.into();
        if value != 0.0 {
            return Self {
                value,
                text: Some(text),
            };
        }
        let unsigned = text.strip_prefix(['+', '-']).unwrap_or(&text);
        if unsigned.is_empty() || !unsigned.bytes().all(|b| b == b'0' || b == b'.') {
            return Self::new(value);
        }
        Self {
            value,
            text: Some(unsigned.to_string()),
        }
    }

    /// The text this element renders as.
    pub fn rendered(&self) -> std::borrow::Cow<'_, str> {
        self.text.as_deref().map_or_else(
            || std::borrow::Cow::Owned(openjd_expr::value::format_float(self.value)),
            std::borrow::Cow::Borrowed,
        )
    }
}

/// `-0.0` and `0.0` are the same number, and `openjd_expr::value::Float64` holds
/// the same invariant so that `Hash` and `PartialEq` agree.
fn normalize_zero(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

/// Compares the rendering rather than the stored text, so two elements that put
/// the same string on a command line are the same element however they were
/// written. `FloatRangeValue::new(2.5)` and `with_text(2.5, "2.5")` are equal;
/// `with_text(2.5, "2.50")` is not.
impl PartialEq for FloatRangeValue {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value && self.rendered() == other.rendered()
    }
}

/// Hand-written so the wire shape stays the `<float> | <floatstring>` union the
/// template uses, rather than a `{"value": .., "text": ..}` object. Round-trips.
impl Serialize for FloatRangeValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.text {
            Some(text) => serializer.serialize_str(text),
            None => serializer.serialize_f64(self.value),
        }
    }
}

impl<'de> Deserialize<'de> for FloatRangeValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Applies the same checks the resolver does, so a deserialized Job cannot
        // hold state `create_job` would have rejected: NaN, which would break the
        // reflexive equality `PartialEq` and `Hash` rely on, and un-normalized
        // text, which is what reaches a command line.
        let finite = |v: f64, src: &str| {
            if v.is_finite() {
                Ok(v)
            } else {
                Err(serde::de::Error::custom(format!(
                    "Float range value '{src}' is not finite"
                )))
            }
        };
        match serde_json::Value::deserialize(deserializer)? {
            serde_json::Value::Number(n) => {
                let v = n
                    .as_f64()
                    .ok_or_else(|| serde::de::Error::custom("Invalid float range value"))?;
                Ok(Self::new(finite(v, &n.to_string())?))
            }
            serde_json::Value::String(s) => {
                let trimmed = s.trim();
                let value = trimmed.parse::<f64>().map_err(|_| {
                    serde::de::Error::custom(format!("Cannot parse '{s}' as float"))
                })?;
                Ok(Self::with_text(
                    finite(value, &s)?,
                    crate::job::create_job::strip_redundant_leading_zeros(trimmed),
                ))
            }
            _ => Err(serde::de::Error::custom(
                "Expected a float or a float string",
            )),
        }
    }
}

/// A resolved task parameter with concrete range values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskParameter {
    Int {
        range: TaskParamRange<i64>,
        chunks: Option<ResolvedChunks>,
    },
    Float {
        range: Vec<FloatRangeValue>,
    },
    String {
        range: Vec<String>,
    },
    Path {
        range: Vec<String>,
    },
    ChunkInt {
        range: TaskParamRange<i64>,
        chunks: ResolvedChunks,
    },
}

/// Manual because `f64` has no `Hash`; those hash via `hash_f64`. The rendering
/// text is hashed too, since `PartialEq` compares it: same number, different
/// rendering means a different command line, so not the same job.
impl Hash for TaskParameter {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Int { range, chunks } => {
                range.hash(state);
                chunks.hash(state);
            }
            Self::Float { range } => {
                range.len().hash(state);
                for elem in range {
                    hash_f64(elem.value, state);
                    elem.rendered().hash(state);
                }
            }
            Self::String { range } | Self::Path { range } => range.hash(state),
            Self::ChunkInt { range, chunks } => {
                range.hash(state);
                chunks.hash(state);
            }
        }
    }
}

/// A resolved range — either a concrete list or a RangeExpr.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    bound(deserialize = "T: serde::de::DeserializeOwned")
)]
pub enum TaskParamRange<T: Serialize> {
    List(Vec<T>),
    RangeExpr(RangeExpr),
}

/// Chunks config with all format strings resolved.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedChunks {
    pub default_task_count: usize,
    pub target_runtime_seconds: Option<usize>,
    pub range_constraint: RangeConstraint,
}

/// Resolved host requirements — no FormatStrings.
#[derive(Debug, Clone, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostRequirements {
    pub amounts: Option<Vec<AmountRequirement>>,
    pub attributes: Option<Vec<AttributeRequirement>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AmountRequirement {
    pub name: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

/// Manual because `f64` has no `Hash`; `min`/`max` hash via `hash_f64`.
impl Hash for AmountRequirement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        for bound in [self.min, self.max] {
            match bound {
                None => false.hash(state),
                Some(v) => {
                    true.hash(state);
                    hash_f64(v, state);
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributeRequirement {
    pub name: String,
    pub any_of: Option<Vec<String>>,
    pub all_of: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepDependency {
    pub depends_on: String,
}
