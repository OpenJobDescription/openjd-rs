// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Template-level diff: build two jobs from templates and parameter sets,
//! diff them, and tag every change with where it came from.
//!
//! **Unstable.** This API may change or be removed in any release without
//! notice. Don't depend on it outside openjd-rs.
//!
//! [`diff_templates`] takes two [`TemplateInput`]s, builds each into a
//! [`Job`] with the same decode, parameter preprocessing and `create_job`
//! calls the CLI uses, and runs [`diff_jobs`] on the pair. On top of that
//! job layer it adds a *template layer* for what a built job cannot show
//! (parameter definitions, step-level `let` text, `extensions`,
//! `specificationVersion`). Each [`Change`] carries an [`Origin`] (template,
//! parameters or both) and a [`Stage`] (the earliest OpenJD stage it shows
//! at). Every step gets a [`StepSummary`] of its task change, from
//! [`task_delta`].
//!
//! The module also holds the host support functions a job store needs
//! beside a plan ([`template_step_fragment`], [`build_job_symbol_table`],
//! [`job_digest`], [`merge_parameters`], [`TemplateInput::preprocessed`]);
//! see `specs/model/host-support.md`.
//!
//! See `specs/model/template-diff.md`.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use indexmap::IndexMap;
use openjd_expr::format_string::FormatString;
use openjd_expr::path_mapping::PathFormat;
use openjd_expr::value::Float64;
use openjd_expr::{ExprType, ExprValue};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{json, Value};

use crate::create_job::{merge_job_parameter_definitions, preprocess_job_parameters_except};
use crate::error::ModelError;
use crate::job::apply::{plan, ApplyPlan};
use crate::job::diff::{diff_jobs, diff_steps, DiffNode, DiffOptions, DiffStatus, DiffTree};
use crate::job::document::{
    definition_from_document, parameter_from_document, space_from_document, ParameterDoc, SpaceDoc,
};
use crate::job::task_delta::{task_delta, TaskBox, TaskDelta};
use crate::job::TaskParameter;
use crate::job::{Environment, Job, JobParameter, Step, StepParameterSpace, TaskParamRange};
use crate::step_param_space::StepParameterSpaceIterator;
use crate::template::parse::{
    decode_environment_template, decode_job_template, document_string_to_object, DocumentType,
};
use crate::template::{EnvironmentTemplate, JobParameterDefinition, JobTemplate};
use crate::types::{CallerLimits, JobParameterInputValues, JobParameterValues, ModelExtension};
use crate::{create_job, PathParameterOptions};

mod host;
pub use host::{
    build_job_symbol_table, job_digest, merge_parameters, template_job_environment_fragment,
    template_step_fragment, TemplateStepFragment,
};

/// Longest [`Shown::text`] in characters before it is cut with `…`.
const MAX_TEXT_CHARS: usize = 120;

// ─── inputs ─────────────────────────────────────────────────────────

/// Where a template comes from.
#[derive(Debug, Clone)]
pub enum TemplateSource {
    /// A file. JSON when the extension is `.json`, YAML otherwise.
    Path(PathBuf),
    /// Template text. JSON when its first non-whitespace character is `{`,
    /// YAML otherwise.
    Text(String),
    /// An already parsed document.
    Value(serde_json::Value),
    /// An already decoded and validated job template. It is not decoded
    /// again, so the extension allowlist is not applied to it.
    Decoded(Box<JobTemplate>),
}

impl TemplateSource {
    /// [`Path`](Self::Path) when `s` names an existing file, otherwise
    /// [`Text`](Self::Text).
    #[must_use]
    pub fn guess(s: &str) -> Self {
        if Path::new(s).is_file() {
            Self::Path(PathBuf::from(s))
        } else {
            Self::Text(s.to_string())
        }
    }
}

/// One side of a template diff: everything `create_job` needs.
///
/// Built with [`TemplateInput::new`] and the builder methods. The defaults
/// are the ones `openjd check` uses: every supported extension, default
/// [`CallerLimits`], the template file's directory for relative PATH
/// defaults, and the process working directory for relative PATH values.
/// For a source other than a path, relative PATH defaults use the
/// [`cwd`](Self::cwd) directory when set, else the process working directory.
#[derive(Debug, Clone)]
pub struct TemplateInput {
    source: TemplateSource,
    parameters: JobParameterInputValues,
    preprocessed: Option<JobParameterValues>,
    environments: Vec<TemplateSource>,
    extensions: Vec<String>,
    limits: CallerLimits,
    template_dir: Option<PathBuf>,
    cwd: Option<PathBuf>,
}

impl TemplateInput {
    /// A side with no parameters and the defaults described on the type.
    #[must_use]
    pub fn new(source: TemplateSource) -> Self {
        Self {
            source,
            parameters: JobParameterInputValues::new(),
            preprocessed: None,
            environments: Vec::new(),
            extensions: ModelExtension::ALL
                .iter()
                .map(|e| e.as_str().to_string())
                .collect(),
            limits: CallerLimits::default(),
            template_dir: None,
            cwd: None,
        }
    }

    /// Set one job parameter, as a string to be coerced to its declared
    /// type. A later value for the same name wins.
    #[must_use]
    pub fn param(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.parameters
            .insert(name.into(), ExprValue::String(value.into()));
        self
    }

    /// Set several job parameters. A later value for the same name wins.
    #[must_use]
    pub fn params(mut self, values: impl IntoIterator<Item = (String, ExprValue)>) -> Self {
        self.parameters.extend(values);
        self
    }

    /// Add an environment template. Its parameter definitions are merged
    /// with the job template's, as `preprocess_job_parameters` does.
    #[must_use]
    pub fn environment(mut self, source: TemplateSource) -> Self {
        self.environments.push(source);
        self
    }

    /// Allow exactly these extensions (names such as `EXPR`).
    #[must_use]
    pub fn extensions(mut self, names: &[&str]) -> Self {
        self.extensions = names.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Allow no extensions.
    #[must_use]
    pub fn no_extensions(mut self) -> Self {
        self.extensions.clear();
        self
    }

    /// Caller limits for decode and job creation.
    #[must_use]
    pub fn limits(mut self, limits: CallerLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Directory that relative PATH defaults are joined to. It must be
    /// absolute.
    #[must_use]
    pub fn template_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.template_dir = Some(dir.into());
        self
    }

    /// Directory that relative PATH values are joined to.
    #[must_use]
    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    /// Build from parameter values that were already preprocessed, such as
    /// the values a host stored when it created the job, without
    /// preprocessing them again. A second pass resolves PATH values against
    /// this side's directories and the host's path format, so it can change
    /// a stored value; these values reach `create_job` as given. When the
    /// side is built, each is checked as given against every definition that
    /// declares it, in the job template and its environment templates.
    ///
    /// They replace the values set with [`param`](Self::param) and
    /// [`params`](Self::params), which the build then ignores. They include
    /// any environment templates' parameters, so the side's
    /// [`environment`](Self::environment)s supply no defaults, only the
    /// constraints the values are checked against. Unstable.
    #[must_use]
    pub fn preprocessed(mut self, values: JobParameterValues) -> Self {
        self.preprocessed = Some(values);
        self
    }

    /// The job parameter values set so far with [`param`](Self::param) and
    /// [`params`](Self::params). Values given with
    /// [`preprocessed`](Self::preprocessed) are not included.
    #[must_use]
    pub fn parameters(&self) -> &JobParameterInputValues {
        &self.parameters
    }

    /// Decode the template and build its job with this side's parameters.
    pub fn build(&self) -> Result<Job, ModelError> {
        let loaded = load(self)?;
        build(&loaded, self.values(), &self.limits)
    }

    fn values(&self) -> Values<'_> {
        match &self.preprocessed {
            Some(v) => Values::Preprocessed(v),
            None => Values::Input(&self.parameters),
        }
    }

    /// The side's values as inputs: the preprocessed values as their typed
    /// values when given, else the input values.
    fn inputs(&self) -> JobParameterInputValues {
        match &self.preprocessed {
            Some(v) => v
                .iter()
                .map(|(k, pv)| (k.clone(), pv.value.clone()))
                .collect(),
            None => self.parameters.clone(),
        }
    }
}

/// The parameter values a side builds with.
#[derive(Clone, Copy)]
enum Values<'a> {
    /// To be preprocessed against the side's template.
    Input(&'a JobParameterInputValues),
    /// Already preprocessed: passed to `create_job` as they are.
    Preprocessed(&'a JobParameterValues),
}

/// A side after decoding: the templates and the resolved directories.
struct Loaded {
    template: JobTemplate,
    environments: Vec<EnvironmentTemplate>,
    /// Everything except parameters and limits that decides the built job,
    /// for an equality check. `None` when the template was given decoded.
    fingerprint: Option<Value>,
    template_dir: String,
    cwd: String,
}

fn read_document(source: &TemplateSource, limits: &CallerLimits) -> Result<Value, ModelError> {
    match source {
        TemplateSource::Path(p) => {
            let text = std::fs::read_to_string(p).map_err(|e| {
                ModelError::DecodeValidation(format!("cannot read '{}': {e}", p.display()))
            })?;
            let kind = if p.extension().and_then(|e| e.to_str()) == Some("json") {
                DocumentType::Json
            } else {
                DocumentType::Yaml
            };
            document_string_to_object(&text, kind, limits)
        }
        TemplateSource::Text(t) => {
            let kind = if t.trim_start().starts_with('{') {
                DocumentType::Json
            } else {
                DocumentType::Yaml
            };
            document_string_to_object(t, kind, limits)
        }
        TemplateSource::Value(v) => Ok(v.clone()),
        TemplateSource::Decoded(_) => Err(ModelError::DecodeValidation(
            "a decoded job template has no document".into(),
        )),
    }
}

/// Windows `canonicalize` returns `\\?\C:\...`, which PATH handling rejects.
fn strip_extended_prefix(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => p.to_path_buf(),
    }
}

fn path_string(p: &Path, what: &str) -> Result<String, ModelError> {
    p.to_str().map(str::to_string).ok_or_else(|| {
        ModelError::DecodeValidation(format!("{what} '{}' is not valid UTF-8", p.display()))
    })
}

fn load(input: &TemplateInput) -> Result<Loaded, ModelError> {
    let allow: Vec<&str> = input.extensions.iter().map(String::as_str).collect();
    let (template, document) = match &input.source {
        TemplateSource::Decoded(t) => ((**t).clone(), None),
        other => {
            let doc = read_document(other, &input.limits)?;
            let t = decode_job_template(doc.clone(), Some(&allow), &input.limits)?;
            (t, Some(doc))
        }
    };
    let mut environments = Vec::new();
    let mut env_docs = Vec::new();
    for source in &input.environments {
        if matches!(source, TemplateSource::Decoded(_)) {
            return Err(ModelError::DecodeValidation(
                "an environment template cannot be given as a decoded job template".into(),
            ));
        }
        let doc = read_document(source, &input.limits)?;
        environments.push(decode_environment_template(
            doc.clone(),
            Some(&allow),
            &input.limits,
        )?);
        env_docs.push(doc);
    }
    let process_cwd = || {
        std::env::current_dir().map_err(|e| {
            ModelError::DecodeValidation(format!("cannot read the current directory: {e}"))
        })
    };
    let cwd = match &input.cwd {
        Some(c) => c.clone(),
        None => strip_extended_prefix(&process_cwd()?),
    };
    let template_dir = match (&input.template_dir, &input.source) {
        (Some(d), _) => d.clone(),
        (None, TemplateSource::Path(p)) => {
            let full = std::fs::canonicalize(p).map_err(|e| {
                ModelError::DecodeValidation(format!("cannot resolve '{}': {e}", p.display()))
            })?;
            strip_extended_prefix(full.parent().unwrap_or(Path::new(".")))
        }
        (None, _) => cwd.clone(),
    };
    let template_dir = path_string(&template_dir, "template directory")?;
    let cwd = path_string(&cwd, "working directory")?;
    let fingerprint = document.map(|doc| {
        json!({
            "template": doc,
            "environments": env_docs,
            "extensions": input.extensions,
            "templateDir": template_dir,
            "cwd": cwd,
        })
    });
    Ok(Loaded {
        template,
        environments,
        fingerprint,
        template_dir,
        cwd,
    })
}

fn build(loaded: &Loaded, values: Values<'_>, limits: &CallerLimits) -> Result<Job, ModelError> {
    let preprocessed;
    let values = match values {
        Values::Preprocessed(v) => {
            check_preprocessed(loaded, v)?;
            v
        }
        Values::Input(params) => {
            preprocessed = preprocess(loaded, params)?;
            &preprocessed
        }
    };
    let ctx = loaded
        .template
        .default_validation_context()
        .with_caller_limits(limits.clone());
    create_job(&loaded.template, values, &ctx)
}

/// Check preprocessed values against every definition that declares them,
/// in the job template and its environment templates (`create_job` checks
/// only the job template's). Values are checked as given, never
/// preprocessed again, so a stored Windows PATH is checked as stored.
fn check_preprocessed(loaded: &Loaded, values: &JobParameterValues) -> Result<(), ModelError> {
    for def in merge_job_parameter_definitions(&loaded.template, &loaded.environments)? {
        if let Some(v) = values.get(&def.name) {
            if v.param_type == def.param_type {
                def.check_constraints(&v.value)?;
            }
        }
    }
    Ok(())
}

/// `preprocess_job_parameters` for a side's template, environments and
/// directories.
fn preprocess(
    loaded: &Loaded,
    params: &JobParameterInputValues,
) -> Result<JobParameterValues, ModelError> {
    typed_values(loaded, JobParameterValues::new(), params)
}

/// The values to build `loaded` with: `kept` as given, and `inputs`
/// preprocessed against the side. A kept value is never preprocessed or
/// checked here, because a second pass can rewrite it (a Windows PATH is
/// relative on a POSIX host); `create_job` checks it when the job is built.
/// Kept parameters count as supplied, so they get no default and are not
/// missing. `kept` comes from [`keepable`].
fn typed_values(
    loaded: &Loaded,
    kept: JobParameterValues,
    inputs: &JobParameterInputValues,
) -> Result<JobParameterValues, ModelError> {
    let mut values = preprocess_job_parameters_except(
        &loaded.template,
        inputs,
        &loaded.environments,
        &PathParameterOptions {
            job_template_dir: &loaded.template_dir,
            current_working_dir: &loaded.cwd,
            path_format: PathFormat::host(),
            allow_template_dir_walk_up: false,
            allow_uri_path_values: true,
        },
        &kept,
    )?;
    values.extend(kept);
    Ok(values)
}

/// The values of `stored` that `loaded` keeps: those its template (with its
/// environments) declares with the stored type, and `inputs` does not set.
fn keepable(
    loaded: &Loaded,
    stored: &JobParameterValues,
    inputs: &JobParameterInputValues,
) -> Result<JobParameterValues, ModelError> {
    let mut kept = JobParameterValues::new();
    for def in merge_job_parameter_definitions(&loaded.template, &loaded.environments)? {
        if inputs.contains_key(&def.name) {
            continue;
        }
        if let Some(value) = stored.get(&def.name) {
            if value.param_type == def.param_type {
                kept.insert(def.name, value.clone());
            }
        }
    }
    Ok(kept)
}

/// The values of the origin split's middle job: the new template with the
/// old side's values. Old input values are preprocessed against the new
/// template. Old stored values are merged as [`host::merge_parameters`]
/// merges them with nothing requested: those the new template keeps are
/// used as stored, retyped and undeclared ones are left out, and the rest
/// take the new template's defaults.
fn middle_values(old: &TemplateInput, new: &Loaded) -> Result<JobParameterValues, ModelError> {
    let none = JobParameterInputValues::new();
    match &old.preprocessed {
        None => typed_values(new, JobParameterValues::new(), &old.parameters),
        Some(stored) => typed_values(new, keepable(new, stored, &none)?, &none),
    }
}

/// Prefix an error with the input that failed (`old template`, `new
/// parameters`), keeping the error's variant where it holds a message.
fn on_side(side: &str, e: ModelError) -> ModelError {
    match e {
        ModelError::DecodeValidation(m) => ModelError::DecodeValidation(format!("{side}: {m}")),
        ModelError::Compatibility(m) => ModelError::Compatibility(format!("{side}: {m}")),
        ModelError::UnsupportedSchema(m) => ModelError::UnsupportedSchema(format!("{side}: {m}")),
        other => ModelError::DecodeValidation(format!("{side}: {other}")),
    }
}

// ─── result types ───────────────────────────────────────────────────

/// Which comparison a [`Change`] comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Layer {
    /// The decoded templates, before anything is resolved.
    Template,
    /// The built jobs ([`diff_jobs`]).
    Job,
}

/// Where a change came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    /// The template changed; the parameter values alone would not change it.
    Template,
    /// The parameter values changed; the template alone would not change it.
    Parameters,
    /// Both, or the split could not be computed (see [`TemplateDiff::notes`]).
    Both,
    /// The inputs were two built jobs ([`diff_built_jobs`]), so the cause is
    /// not known.
    Job,
}

/// The earliest generic OpenJD stage at which a change shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    /// Template text only: definitions, step-level `let`, extensions.
    Template,
    /// Preprocessed job parameter values.
    Parameters,
    /// The built job.
    Job,
    /// A format string that only a worker can resolve: it references a
    /// `Task.` or `Session.` symbol.
    RunTime,
}

/// A value shown in a change: the JSON form and a rendering for people.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Shown {
    /// The value as JSON, in full.
    pub raw: Value,
    /// A compact rendering: ranges as `1-100`, typed values bare, symbol
    /// tables as `name=value` pairs. Cut at 120 characters with `…`.
    pub text: String,
}

impl Shown {
    /// `text` in full; [`finish_texts`] cuts it once every change is built.
    fn new(raw: Value, text: String) -> Self {
        Self { raw, text }
    }

    fn generic(raw: Value) -> Self {
        let text = render(&raw);
        Self::new(raw, text)
    }
}

impl fmt::Display for Shown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// One changed field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    /// The comparison it comes from.
    pub layer: Layer,
    /// [`DiffTree`] path of the node, e.g.
    /// `steps[Render].parameterSpace.taskParameterDefinitions[Eye]`. Empty
    /// for the root. Template-layer paths are `parameterDefinitions[P]`,
    /// `steps[S]` and the root.
    pub path: String,
    /// The name of the step the change is in, taken from the diff tree's
    /// step node (or the template step it was computed for); `None` for a
    /// job-level change. Serialized only when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// The field, as in [`FieldChange::field`](crate::job::diff::FieldChange::field).
    /// Empty when the whole node was added, removed or left unexpanded.
    pub field: String,
    /// The old value; `None` when the node or field was added.
    pub before: Option<Shown>,
    /// The new value; `None` when the node or field was removed.
    pub after: Option<Shown>,
    /// Where it came from.
    pub origin: Origin,
    /// The earliest stage it shows at.
    pub stage: Stage,
}

/// How a step's tasks change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TaskSummary {
    /// The same tasks in the same order.
    Unchanged {
        /// Number of tasks.
        count: u64,
    },
    /// An exact product decomposition ([`TaskDelta::Product`]).
    Product {
        /// Tasks on both sides.
        kept: BoxSummary,
        /// Tasks only on the new side, one box per product term.
        added: Vec<BoxSummary>,
        /// Tasks only on the old side.
        removed: Vec<BoxSummary>,
    },
    /// No per-task mapping ([`TaskDelta::Fallback`]): every task is replaced.
    Replaced {
        /// Old number of tasks.
        old_count: u64,
        /// New number of tasks.
        new_count: u64,
        /// Why there is no mapping.
        reason: String,
    },
    /// A new step.
    Added {
        /// Number of tasks.
        count: u64,
    },
    /// A removed step.
    Removed {
        /// Number of tasks.
        count: u64,
    },
}

/// One product box of tasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BoxSummary {
    /// Number of tasks in the box.
    pub count: u64,
    /// Each dimension's name and its values, readable (`1-100`, `left, right`).
    pub dims: Vec<(String, String)>,
}

/// One step of either job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepSummary {
    /// Step name, the identity both sides are matched by.
    pub name: String,
    /// How the two sides of the step relate, with the caller's options.
    #[serde(serialize_with = "ser_status")]
    pub status: DiffStatus,
    /// Position in the old job's steps.
    pub old_index: Option<usize>,
    /// Position in the new job's steps.
    pub new_index: Option<usize>,
    /// The task change.
    pub tasks: TaskSummary,
    /// Why the task count cannot be computed (`adaptive chunking`, or
    /// `cannot count tasks` when a parameter space does not iterate), with
    /// a note in [`TemplateDiff::notes`]. The counts in `tasks` are then 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks_unknown: Option<String>,
}

/// Counts over a [`TemplateDiff`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffStat {
    /// Steps on both sides that differ.
    pub steps_changed: u64,
    /// Steps only in the new job.
    pub steps_added: u64,
    /// Steps only in the old job.
    pub steps_removed: u64,
    /// Tasks that stay, in every step.
    pub tasks_kept: u64,
    /// Tasks appended to steps, including all tasks of added steps.
    pub tasks_added: u64,
    /// Tasks deleted from steps, including all tasks of removed steps.
    pub tasks_removed: u64,
    /// New tasks written by replacing every task of a step.
    pub tasks_replaced: u64,
}

/// The result of a template diff.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateDiff {
    /// Every changed field of both layers, sorted by path, field and layer.
    pub changes: Vec<Change>,
    /// Every step of either job: old order, then new-only steps.
    pub steps: Vec<StepSummary>,
    /// The job layer as [`diff_jobs`] returned it.
    #[serde(serialize_with = "ser_tree")]
    pub job: DiffTree,
    /// The old built job. Serialized as a [`JobDocument`].
    #[serde(serialize_with = "ser_job")]
    pub old_job: Job,
    /// The new built job. Serialized as a [`JobDocument`].
    #[serde(serialize_with = "ser_job")]
    pub new_job: Job,
    /// Anything the diff could not decide, e.g. why origins are `Both`.
    pub notes: Vec<Note>,
}

/// Something a [`TemplateDiff`] could not decide.
///
/// Displayed and serialized as one string: `steps[S]: text` for a step's
/// note, `text` for a job-level one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// The name of the step it is about; `None` for a job-level note.
    pub step: Option<String>,
    /// What could not be decided, and why.
    pub text: String,
}

impl Note {
    fn job(text: String) -> Self {
        Self { step: None, text }
    }

    fn step(name: &str, text: String) -> Self {
        Self {
            step: Some(name.to_string()),
            text,
        }
    }
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.step {
            Some(name) => write!(f, "steps[{name}]: {}", self.text),
            None => f.write_str(&self.text),
        }
    }
}

impl Serialize for Note {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.collect_str(self)
    }
}

impl TemplateDiff {
    /// Whether anything differs.
    #[must_use]
    pub fn is_changed(&self) -> bool {
        !self.changes.is_empty() || self.steps.iter().any(|s| s.status != DiffStatus::Unchanged)
    }

    /// Changes whose [`Change::path`] is `prefix` or below it, matched at
    /// segment boundaries: `steps[Render]` matches `steps[Render]` and
    /// `steps[Render].script`, not `steps[Render2]`; `steps` matches every
    /// step. The empty prefix matches everything.
    ///
    /// A step or entry name containing `]` or `.` can make a path prefix
    /// ambiguous: `steps[A]` also matches `steps[A].x].script`, a change in
    /// the step named `A].x`. Use [`changes_in_step`](Self::changes_in_step)
    /// to select a step's changes exactly.
    pub fn changes_at<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = &'a Change> + 'a {
        self.changes.iter().filter(move |c| under(&c.path, prefix))
    }

    /// The changes in the step named `name` ([`Change::step`]), matched
    /// exactly.
    pub fn changes_in_step<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Change> + 'a {
        self.changes
            .iter()
            .filter(move |c| c.step.as_deref() == Some(name))
    }

    /// The summary of the step named `name`.
    #[must_use]
    pub fn step(&self, name: &str) -> Option<&StepSummary> {
        self.steps.iter().find(|s| s.name == name)
    }

    /// The apply plan for the two jobs ([`job::apply::plan`](crate::job::apply::plan)).
    pub fn plan(&self) -> Result<ApplyPlan<'_>, ModelError> {
        plan(&self.old_job, &self.new_job)
    }

    /// Step and task counts. A step on both sides counts as changed when
    /// its job-layer status is modified or a template-layer change is under
    /// it (a step-level `let` that nothing reads). Counts saturate at
    /// `u64::MAX`.
    #[must_use]
    pub fn stat(&self) -> DiffStat {
        fn add(total: &mut u64, n: u64) {
            *total = total.saturating_add(n);
        }
        let mut s = DiffStat::default();
        for step in &self.steps {
            match step.status {
                DiffStatus::Added => add(&mut s.steps_added, 1),
                DiffStatus::Removed => add(&mut s.steps_removed, 1),
                _ if self.step_changed(step) => add(&mut s.steps_changed, 1),
                _ => {}
            }
            match &step.tasks {
                TaskSummary::Unchanged { count } => add(&mut s.tasks_kept, *count),
                TaskSummary::Product {
                    kept,
                    added,
                    removed,
                } => {
                    add(&mut s.tasks_kept, kept.count);
                    add(&mut s.tasks_added, total(added));
                    add(&mut s.tasks_removed, total(removed));
                }
                TaskSummary::Replaced { new_count, .. } => add(&mut s.tasks_replaced, *new_count),
                TaskSummary::Added { count } => add(&mut s.tasks_added, *count),
                TaskSummary::Removed { count } => add(&mut s.tasks_removed, *count),
            }
        }
        s
    }

    /// Whether `step` differs, or has any change in it.
    fn step_changed(&self, step: &StepSummary) -> bool {
        step.status != DiffStatus::Unchanged || self.changes_in_step(&step.name).next().is_some()
    }
}

/// The task count of a list of boxes, saturating.
fn total(boxes: &[BoxSummary]) -> u64 {
    boxes.iter().fold(0, |t, b| t.saturating_add(b.count))
}

/// Whether `path` is `prefix` or below it. A `[` continues only a prefix
/// that does not end in `]`, so `steps` matches `steps[A]` but `steps[A]`
/// does not match the step named `A][B`.
fn under(path: &str, prefix: &str) -> bool {
    prefix.is_empty()
        || path == prefix
        || path.strip_prefix(prefix).is_some_and(|rest| {
            rest.starts_with('.') || (rest.starts_with('[') && !prefix.ends_with(']'))
        })
}

// ─── entry points ───────────────────────────────────────────────────

/// Diff two templates, each with its own parameters and options.
///
/// Errors name the side that failed: `old template: …` or `new template: …`.
///
/// Unstable: see the module docs.
pub fn diff_templates(
    old: &TemplateInput,
    new: &TemplateInput,
    options: &DiffOptions,
) -> Result<TemplateDiff, ModelError> {
    let lo = load(old).map_err(|e| on_side("old template", e))?;
    let ln = load(new).map_err(|e| on_side("new template", e))?;
    let old_job = build(&lo, old.values(), &old.limits).map_err(|e| on_side("old template", e))?;
    let new_job = build(&ln, new.values(), &new.limits).map_err(|e| on_side("new template", e))?;
    let template = template_layer(&lo.template, &ln.template);
    let same_template = matches!((&lo.fingerprint, &ln.fingerprint), (Some(a), Some(b)) if a == b);
    let mut notes = Vec::new();
    let old_inputs = old.inputs();
    let rule =
        if old.preprocessed.is_none() == new.preprocessed.is_none() && old_inputs == new.inputs() {
            OriginRule::All(Origin::Template)
        } else if same_template {
            OriginRule::All(Origin::Parameters)
        } else {
            // `mid` is the new template with the old parameters.
            match middle_values(old, &ln)
                .and_then(|v| build(&ln, Values::Preprocessed(&v), &new.limits))
            {
                Ok(mid) => OriginRule::Split {
                    template: change_keys(&diff_jobs(&old_job, &mid, options)),
                    parameters: change_keys(&diff_jobs(&mid, &new_job, options)),
                },
                Err(e) => {
                    notes.push(Note::job(format!(
                        "origin: the new template does not build with the old parameters, so \
                     job-layer changes are marked both: {e}"
                    )));
                    OriginRule::All(Origin::Both)
                }
            }
        };
    Ok(assemble(old_job, new_job, template, &rule, notes, options))
}

/// Diff one template under two parameter sets. `old` and `new` are laid
/// over the template input's own parameters; a later value wins.
///
/// Errors name the build that failed: `old parameters: …` or
/// `new parameters: …`, or `the template: …` when the template itself
/// does not decode.
///
/// Unstable: see the module docs.
pub fn diff_job_template_with_parameter_sets(
    template: &TemplateInput,
    old: &JobParameterInputValues,
    new: &JobParameterInputValues,
    options: &DiffOptions,
) -> Result<TemplateDiff, ModelError> {
    let loaded = load(template).map_err(|e| on_side("the template", e))?;
    let overlay = |side: &JobParameterInputValues| {
        let mut p = template.inputs();
        p.extend(side.iter().map(|(k, v)| (k.clone(), v.clone())));
        p
    };
    let (old_params, new_params) = (overlay(old), overlay(new));
    let old_job = build(&loaded, Values::Input(&old_params), &template.limits)
        .map_err(|e| on_side("old parameters", e))?;
    let new_job = build(&loaded, Values::Input(&new_params), &template.limits)
        .map_err(|e| on_side("new parameters", e))?;
    let rule = OriginRule::All(Origin::Parameters);
    Ok(assemble(
        old_job,
        new_job,
        Vec::new(),
        &rule,
        Vec::new(),
        options,
    ))
}

/// Diff two jobs that are already built. There is no template layer, and
/// every change has origin [`Origin::Job`].
///
/// Unstable: see the module docs.
#[must_use]
pub fn diff_built_jobs(old: Job, new: Job, options: &DiffOptions) -> TemplateDiff {
    assemble(
        old,
        new,
        Vec::new(),
        &OriginRule::All(Origin::Job),
        Vec::new(),
        options,
    )
}

// ─── assembly ───────────────────────────────────────────────────────

/// A change's identity for the origin split: `(path, field, whole)`, where
/// `whole` is the status of a whole-node change (added, removed, or
/// modified and not expanded) and `None` for a field change. The status
/// keeps an added node apart from a node at the same path that the other
/// half leaves unexpanded, so a change keeps its origin at every depth at
/// which it appears.
type ChangeKey = (String, String, Option<DiffStatus>);

enum OriginRule {
    All(Origin),
    Split {
        template: HashSet<ChangeKey>,
        parameters: HashSet<ChangeKey>,
    },
}

impl OriginRule {
    fn classify(&self, key: &ChangeKey) -> Origin {
        match self {
            Self::All(o) => *o,
            Self::Split {
                template,
                parameters,
            } => match (template.contains(key), parameters.contains(key)) {
                (true, false) => Origin::Template,
                (false, true) => Origin::Parameters,
                _ => Origin::Both,
            },
        }
    }
}

/// The key of every change in a job tree.
fn change_keys(tree: &DiffTree) -> HashSet<ChangeKey> {
    let mut keys = HashSet::new();
    for (id, node) in tree.changed() {
        let path = tree.path(id);
        if node.status == DiffStatus::Modified && node.expanded {
            for fc in &node.field_changes {
                keys.insert((path.clone(), fc.field.clone(), None));
            }
        } else {
            keys.insert((path, String::new(), Some(node.status)));
        }
    }
    keys
}

fn assemble(
    mut old_job: Job,
    mut new_job: Job,
    template: Vec<Change>,
    rule: &OriginRule,
    mut notes: Vec<Note>,
    options: &DiffOptions,
) -> TemplateDiff {
    // `create_job` fills `parameters` from a `HashMap`, so their order
    // varies from run to run. Parameter order is not a change, so sort it to
    // make the tree and the result deterministic.
    old_job.parameters.sort_keys();
    new_job.parameters.sort_keys();
    let tree = diff_jobs(&old_job, &new_job, options);
    let (old_doc, new_doc) = (job_value(&old_job), job_value(&new_job));
    let mut pending: Vec<(Change, Written)> = template
        .into_iter()
        .map(|c| (c, Written::default()))
        .collect();
    for (id, node) in tree.changed() {
        let path = tree.path(id);
        let segments = segments(&tree, id);
        let step = owning_step(&tree, id);
        let mut push = |field: String, before: Option<NodeText>, after: Option<NodeText>| {
            let (before, written_before) = before.unzip();
            let (after, written_after) = after.unzip();
            let whole = field.is_empty().then_some(node.status);
            let origin = rule.classify(&(path.clone(), field.clone(), whole));
            let stage = job_stage(&path, &field, before.as_ref(), after.as_ref());
            let change = Change {
                layer: Layer::Job,
                path: path.clone(),
                step: step.clone(),
                field,
                before,
                after,
                origin,
                stage,
            };
            pending.push((change, (written_before, written_after)));
        };
        let side = |doc: &Value, job: &Job| {
            lookup(doc, &segments).map(|v| shown_node(node, "", v.clone(), job))
        };
        match node.status {
            DiffStatus::Added => push(String::new(), None, side(&new_doc, &new_job)),
            DiffStatus::Removed => push(String::new(), side(&old_doc, &old_job), None),
            DiffStatus::Modified if !node.expanded => push(
                String::new(),
                side(&old_doc, &old_job),
                side(&new_doc, &new_job),
            ),
            DiffStatus::Modified => {
                for fc in &node.field_changes {
                    let shown = |v: &Value, job: &Job| {
                        (!v.is_null()).then(|| shown_node(node, &fc.field, v.clone(), job))
                    };
                    push(
                        fc.field.clone(),
                        shown(&fc.before, &old_job),
                        shown(&fc.after, &new_job),
                    );
                }
            }
            DiffStatus::Unchanged => {}
        }
    }
    pending
        .sort_by(|(a, _), (b, _)| (&a.path, &a.field, a.layer).cmp(&(&b.path, &b.field, b.layer)));
    let changes = pending
        .into_iter()
        .map(|(mut c, written)| {
            finish_texts(&mut c, &written, &old_job, &new_job);
            c
        })
        .collect();
    let steps = step_summaries(&old_job, &new_job, options, &mut notes);
    TemplateDiff {
        changes,
        steps,
        job: tree,
        old_job,
        new_job,
        notes,
    }
}

/// A job-layer side: its [`Shown`], and its text with every task parameter
/// definition in it in its written form ([`written_text`]).
type NodeText = (Shown, String);

/// Each side's written-form text; `None` for a template-layer change or a
/// side that does not exist.
type Written = (Option<String>, Option<String>);

/// Cut each side's text to [`MAX_TEXT_CHARS`], once, after every rendering
/// rule has run. Two sides with different values never show the same text:
/// both sides are cut around their first difference, and when their full
/// texts are equal the cut is over their JSON instead.
fn finish_texts(c: &mut Change, written: &Written, old_job: &Job, new_job: &Job) {
    if let Some((bf, af)) = full_texts(c, written, old_job, new_job) {
        let (bt, at) = cut_pair(&bf, &af);
        if let (Some(b), Some(a)) = (&mut c.before, &mut c.after) {
            (b.text, a.text) = (bt, at);
            if b.raw != a.raw && b.text == a.text {
                (b.text, a.text) = around_difference(&b.raw.to_string(), &a.raw.to_string());
            }
        }
        return;
    }
    if let Some(s) = c.before.as_mut().or(c.after.as_mut()) {
        s.text = truncate(std::mem::take(&mut s.text));
    }
}

/// A side's text before the cut: `body` may be cut, `suffix` is always shown.
struct FullText {
    body: String,
    suffix: String,
}

impl FullText {
    fn plain(body: String) -> Self {
        Self {
            body,
            suffix: String::new(),
        }
    }
}

/// Both sides' full texts. A job parameter value whose type changes shows
/// its type (`"5" (STRING) -> 5 (INT)`); a pair whose texts are equal but
/// whose values differ shows each task parameter definition in it as it is
/// written (`range expression 1,2 -> list [1, 2]`), at any depth; anything
/// else keeps its text.
fn full_texts(
    c: &Change,
    written: &Written,
    old_job: &Job,
    new_job: &Job,
) -> Option<(FullText, FullText)> {
    let (Some(b), Some(a)) = (&c.before, &c.after) else {
        return None;
    };
    let plain = || {
        Some((
            FullText::plain(b.text.clone()),
            FullText::plain(a.text.clone()),
        ))
    };
    if c.layer != Layer::Job {
        return plain();
    }
    if matches!(c.field.as_str(), "" | "value") {
        let name = c
            .path
            .strip_prefix("parameters[")
            .and_then(|r| r.strip_suffix(']'));
        let types = name.map(|n| (old_job.parameters.get(n), new_job.parameters.get(n)));
        if let Some((Some(o), Some(n))) = types {
            if o.param_type != n.param_type {
                return Some((typed_text(b, o), typed_text(a, n)));
            }
        }
    }
    if let (true, (Some(wb), Some(wa))) = (b.text == a.text && b.raw != a.raw, written) {
        return Some((FullText::plain(wb.clone()), FullText::plain(wa.clone())));
    }
    plain()
}

/// A job parameter value with its type: `"5" (STRING)`, `4.5 (FLOAT)`.
/// Only `STRING` and `PATH` values are quoted.
fn typed_text(s: &Shown, p: &JobParameter) -> FullText {
    let body = match &p.value {
        ExprValue::String(v) | ExprValue::Path { value: v, .. } => quoted(v),
        _ => s.text.clone(),
    };
    FullText {
        body,
        suffix: format!(" ({})", p.param_type.as_spec_str()),
    }
}

/// Both texts cut as [`cut_around_difference`] cuts them, in the room the
/// longer suffix leaves, then each with its suffix.
fn cut_pair(b: &FullText, a: &FullText) -> (String, String) {
    let room = MAX_TEXT_CHARS - b.suffix.chars().count().max(a.suffix.chars().count());
    let (bt, at) = cut_around_difference(&b.body, &a.body, room);
    (bt + &b.suffix, at + &a.suffix)
}

/// A task parameter definition by how its range is written: an integer
/// range as `range expression 1,2` or `list [1, 2]`, any other type as its
/// type and list (`STRING [1, 2]`), then any chunk settings. A chunked
/// range names its type, `INT` or `CHUNK[INT]`, since both hold the same
/// range and chunks.
fn written_text(p: &TaskParameter) -> String {
    let int = |range: &TaskParamRange<i64>| match range {
        TaskParamRange::RangeExpr(e) => format!("range expression {e}"),
        TaskParamRange::List(v) => format!("list {}", list_text(v.iter().map(i64::to_string))),
    };
    match p {
        TaskParameter::Int { range, chunks } => match chunks {
            Some(c) => chunk_suffix(format!("INT {}", int(range)), c),
            None => int(range),
        },
        TaskParameter::ChunkInt { range, chunks } => {
            chunk_suffix(format!("CHUNK[INT] {}", int(range)), chunks)
        }
        TaskParameter::Float { .. } => format!("FLOAT {}", range_text(p)),
        TaskParameter::String { .. } => format!("STRING {}", range_text(p)),
        TaskParameter::Path { .. } => format!("PATH {}", range_text(p)),
    }
}

/// Both texts cut at [`MAX_TEXT_CHARS`]; when the first difference would
/// be cut off, both start a few characters before it (see
/// [`cut_around_difference`]).
fn around_difference(x: &str, y: &str) -> (String, String) {
    cut_around_difference(x, y, MAX_TEXT_CHARS)
}

/// [`around_difference`] with `max` characters in place of
/// [`MAX_TEXT_CHARS`]. `max` must be at least 64: the head, `…` and 40
/// characters of context before the difference. A list or object keeps up
/// to its first 20 characters, ending after its last `,` or `:` in them
/// when there is one, so a cut one still reads as a list or object; the
/// context starts at the first `/`, or after the first space, in its 40
/// characters when there is one: `{name: Demo,… …/renders/b]`.
fn cut_around_difference(x: &str, y: &str, max: usize) -> (String, String) {
    const CONTEXT: usize = 40;
    const HEAD: usize = 20;
    let first_diff = x.chars().zip(y.chars()).take_while(|(p, q)| p == q).count();
    if first_diff < max - 1 {
        return (
            truncate_to(x.to_string(), max),
            truncate_to(y.to_string(), max),
        );
    }
    // Both sides share their first `first_diff` characters, so the head and
    // the start of the context are the same for both.
    let chars: Vec<char> = x.chars().take(first_diff).collect();
    let head: String = if x.starts_with(['[', '{']) {
        let end = (1..HEAD)
            .rev()
            .find(|&i| matches!(chars[i - 1..=i], [',' | ':', ' ']))
            .unwrap_or(HEAD);
        chars[..end].iter().collect::<String>() + "… "
    } else {
        String::new()
    };
    let window = first_diff - CONTEXT;
    let start = (window..first_diff)
        .find_map(|i| match chars[i] {
            '/' => Some(i),
            ' ' if i + 1 < first_diff => Some(i + 1),
            _ => None,
        })
        .unwrap_or(window);
    let from = |s: &str| {
        let tail: String = s.chars().skip(start).collect();
        let room = max - head.chars().count();
        head.clone() + &truncate_to(format!("…{tail}"), room)
    };
    (from(x), from(y))
}

fn to_json<T: Serialize + ?Sized>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or_else(|e| Value::String(format!("<not serializable: {e}>")))
}

/// A job as a `Value` for whole-node [`Shown::raw`]s: its [`JobDocument`],
/// whose `taskParameterDefinitions` lists keep definition order.
fn job_value(job: &Job) -> Value {
    to_json(&JobDocument(job))
}

/// The key of the step node at or above `id`: the name of the step the
/// node is in, or `None` above step level.
fn owning_step(tree: &DiffTree, id: usize) -> Option<String> {
    use crate::job::diff::DiffNodeKind;
    let mut cur = tree.node(id);
    while let Some(n) = cur {
        if n.kind == DiffNodeKind::Step {
            return n.key.clone();
        }
        cur = n.parent.and_then(|p| tree.node(p));
    }
    None
}

/// Path segments of a node, root first.
fn segments(tree: &DiffTree, id: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = tree.node(id);
    while let Some(n) = cur {
        if !n.segment.is_empty() {
            out.push(n.segment.clone());
        }
        cur = n.parent.and_then(|p| tree.node(p));
    }
    out.reverse();
    out
}

/// Follow diff segments through a [`job_value`]: `name[key]` selects a map
/// entry, or the element of a list whose `name` is `key`.
///
/// Every diff segment is the serialized field name except an action's
/// (`onRun`, `onEnter`, …), which the diff hangs on the script directly but
/// the job holds under `script.actions`.
fn lookup<'v>(doc: &'v Value, segments: &[String]) -> Option<&'v Value> {
    segments
        .iter()
        .try_fold(doc, |v, seg| match seg.split_once('[') {
            None => v
                .get(seg)
                .or_else(|| v.get("actions").and_then(|a| a.get(seg))),
            Some((coll, rest)) => {
                let key = rest.strip_suffix(']')?;
                match v.get(coll)? {
                    Value::Object(m) => m.get(key),
                    Value::Array(a) => a
                        .iter()
                        .find(|e| e.get("name").and_then(Value::as_str) == Some(key)),
                    _ => None,
                }
            }
        })
}

fn job_stage(path: &str, field: &str, before: Option<&Shown>, after: Option<&Shown>) -> Stage {
    if path.starts_with("parameters[") {
        return Stage::Parameters;
    }
    let is_let = field == "let";
    let format_field = is_let
        || field.starts_with("variables.")
        || matches!(
            field,
            "command" | "args" | "timeout" | "cancelation" | "data"
        );
    let run_time = |s: Option<&Shown>| s.is_some_and(|s| refs_run_time(&s.raw, is_let));
    if format_field && (run_time(before) || run_time(after)) {
        Stage::RunTime
    } else {
        Stage::Job
    }
}

/// Whether any string in `v` references a `Task.` or `Session.` symbol:
/// inside a `{{ }}` interpolation, or anywhere in a `let` expression.
fn refs_run_time(v: &Value, is_let: bool) -> bool {
    match v {
        Value::String(s) if is_let => names_run_time_symbol(s),
        Value::String(s) => interpolations(s).any(names_run_time_symbol),
        Value::Array(a) => a.iter().any(|e| refs_run_time(e, is_let)),
        Value::Object(m) => m.values().any(|e| refs_run_time(e, is_let)),
        _ => false,
    }
}

/// The text inside each `{{ … }}` of a format string.
fn interpolations(s: &str) -> impl Iterator<Item = &str> {
    s.split("{{")
        .skip(1)
        .filter_map(|rest| rest.split_once("}}").map(|(inner, _)| inner))
}

/// Whether `expr` names a `Task.` or `Session.` symbol: the prefix starts
/// the text or follows a character that cannot be part of a name or an
/// attribute access, so `subTask.x` and `a.Task.x` do not count.
fn names_run_time_symbol(expr: &str) -> bool {
    ["Task.", "Session."].iter().any(|root| {
        expr.match_indices(root).any(|(i, _)| {
            expr[..i]
                .chars()
                .next_back()
                .is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '.'))
        })
    })
}

// ─── rendering ──────────────────────────────────────────────────────

fn truncate(s: String) -> String {
    truncate_to(s, MAX_TEXT_CHARS)
}

/// `s` cut to `max - 1` characters and `…` when it is longer than `max`.
fn truncate_to(s: String, max: usize) -> String {
    if s.chars().count() <= max {
        return s;
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

/// How a text writes a task parameter definition: [`range_text`], or
/// [`written_text`] when two sides' compact texts are equal.
type DefinitionText = fn(&TaskParameter) -> String;

/// A job-layer value and its written-form text ([`NodeText`]).
fn shown_node(node: &DiffNode, field: &str, raw: Value, job: &Job) -> NodeText {
    let text = node_text(node, field, &raw, job, range_text);
    let written = node_text(node, field, &raw, job, written_text);
    (Shown::new(raw, text), written)
}

/// A job-layer value, rendered by what the node holds. Every task parameter
/// definition in it, at any depth, is written by `def`.
fn node_text(node: &DiffNode, field: &str, raw: &Value, job: &Job, def: DefinitionText) -> String {
    use crate::job::diff::DiffNodeKind;
    match (node.kind, field) {
        (DiffNodeKind::TaskParameter, "definition" | "") => {
            definition(raw).map_or_else(|| render(raw), |p| def(&p))
        }
        (DiffNodeKind::JobParameter, "value" | "") => node
            .key
            .as_ref()
            .and_then(|k| job.parameters.get(k))
            .map_or_else(|| render(raw), parameter_text),
        (DiffNodeKind::JobParameter, "type") => node
            .key
            .as_ref()
            .and_then(|k| job.parameters.get(k))
            .map_or_else(|| render(raw), |p| p.param_type.as_spec_str().to_string()),
        (_, "resolvedSymTab") if raw.as_array().is_some_and(Vec::is_empty) => NO_SYMBOLS.into(),
        (_, "") => whole_text(&without_label(raw, node.key.as_deref()), def),
        _ => whole_text(&without_label(raw, field_key(field)), def),
    }
}

/// A task parameter definition from its host-encoding document, with or
/// without its `name`.
fn definition(raw: &Value) -> Option<TaskParameter> {
    definition_from_document(raw, "", raw.get("name").is_some())
        .ok()
        .map(|(_, p)| p)
}

/// A job parameter's value, bare: a `STRING` or `PATH` as [`string_text`]
/// writes it, any other type as its display form.
fn parameter_text(p: &JobParameter) -> String {
    match &p.value {
        ExprValue::String(s) | ExprValue::Path { value: s, .. } => string_text(s),
        v => escape_controls(&v.to_display_string()),
    }
}

/// A whole node, [`render`]ed, except that every parameter space in it is
/// its [`space_text`], every job parameter is written as its parameter line
/// writes it with its type (`Frames: 1-100 (STRING)`), and an empty
/// `resolvedSymTab` is left out at every depth.
fn whole_text(v: &Value, def: DefinitionText) -> String {
    render_with(&without_empty_symbols(v), &|e| {
        let m = e.as_object()?;
        if m.get("taskParameterDefinitions")
            .is_some_and(Value::is_array)
        {
            return Some(space_text(e, def));
        }
        let is_parameter =
            m.len() == 3 && ["name", "type", "value"].iter().all(|k| m.contains_key(*k));
        if !is_parameter {
            return None;
        }
        let name = m.get("name").and_then(Value::as_str)?;
        let p = parameter_from_document(name, e, "").ok()?;
        Some(format!(
            "{} ({})",
            parameter_text(&p),
            p.param_type.as_spec_str()
        ))
    })
}

/// `v` without any empty `resolvedSymTab`, at any depth.
fn without_empty_symbols(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(k, e)| {
                    !(k.as_str() == "resolvedSymTab" && e.as_array().is_some_and(Vec::is_empty))
                })
                .map(|(k, e)| (k.clone(), without_empty_symbols(e)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(without_empty_symbols).collect()),
        _ => v.clone(),
    }
}

/// The key of a keyed field, `vcpu` in `amounts[vcpu]`.
fn field_key(field: &str) -> Option<&str> {
    field.strip_suffix(']')?.split_once('[').map(|(_, k)| k)
}

/// A value without what its line's label already says: its `name` when
/// that is `key`, the key the label shows.
fn without_label(raw: &Value, key: Option<&str>) -> Value {
    let mut v = raw.clone();
    if let Some(m) = v.as_object_mut() {
        if key.is_some() && m.get("name").and_then(Value::as_str) == key {
            m.remove("name");
        }
    }
    v
}

/// How an empty symbol table is shown.
const NO_SYMBOLS: &str = "(none)";

/// A whole parameter space: each task parameter's range in definition
/// order, `{Frame: 1-10, Eye: [left]}`, then `, combination Frame * Eye`
/// when one is set. Each definition is written by `def`.
fn space_text(raw: &Value, def: DefinitionText) -> String {
    let Some(defs) = raw
        .get("taskParameterDefinitions")
        .and_then(Value::as_array)
    else {
        return render(raw);
    };
    let ranges = defs
        .iter()
        .map(|d| {
            let name = d.get("name").map_or_else(String::new, render);
            let range = definition(d).map_or_else(|| render(&without_label(d, None)), |p| def(&p));
            format!("{name}: {range}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    match raw.get("combination").filter(|c| !c.is_null()) {
        Some(c) => format!("{{{ranges}}}, combination {}", render(c)),
        None => format!("{{{ranges}}}"),
    }
}

/// A task parameter's range: integers as range expressions (`1-100`,
/// `1-9:2`, `1,3`; see [`int_range_text`]), other values as a list
/// (`[left, right]`). A chunked one adds its chunk settings.
fn range_text(p: &TaskParameter) -> String {
    match p {
        TaskParameter::Int { range, chunks } => match chunks {
            Some(c) => chunked_text(range, c),
            None => int_range_text(range),
        },
        TaskParameter::ChunkInt { range, chunks } => chunked_text(range, chunks),
        TaskParameter::Float { range } => list_text(range.iter().map(Float64::to_display_string)),
        TaskParameter::String { range } | TaskParameter::Path { range } => {
            list_text(range.iter().map(|s| string_text(s)))
        }
    }
}

/// `1-40 chunks of 10`, then `, target 5s` when `targetRuntimeSeconds` is
/// set and `, noncontiguous` for a noncontiguous range constraint.
fn chunked_text(range: &TaskParamRange<i64>, c: &crate::job::ResolvedChunks) -> String {
    chunk_suffix(int_range_text(range), c)
}

/// `range` followed by ` chunks of N` and the optional chunk settings.
fn chunk_suffix(range: String, c: &crate::job::ResolvedChunks) -> String {
    let mut s = format!("{range} chunks of {}", c.default_task_count);
    if let Some(t) = c.target_runtime_seconds {
        s.push_str(&format!(", target {t}s"));
    }
    if c.range_constraint == crate::template::RangeConstraint::Noncontiguous {
        s.push_str(", noncontiguous");
    }
    s
}

/// Integer task parameter values, the same text in a change and in a task
/// box: a range expression's canonical form, and a list as [`int_runs`],
/// which writes an increasing list in that same form.
fn int_range_text(r: &TaskParamRange<i64>) -> String {
    match r {
        TaskParamRange::RangeExpr(e) => e.to_string(),
        TaskParamRange::List(v) => int_runs(v),
    }
}

fn list_text(items: impl Iterator<Item = String>) -> String {
    format!("[{}]", items.collect::<Vec<_>>().join(", "))
}

/// Generic rendering: strings as [`string_text`], integral floats without
/// `.0`, lists as `[a, b]`, symbol tables as `name=value` pairs, step
/// dependencies as the names they depend on, objects as `{key: value}`
/// without their `null` fields.
fn render(v: &Value) -> String {
    render_with(v, &|_| None)
}

/// [`render`], with `special` asked first for every value at every depth;
/// a value it returns text for is written as that text.
fn render_with(v: &Value, special: &dyn Fn(&Value) -> Option<String>) -> String {
    if let Some(text) = special(v) {
        return text;
    }
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => number_text(n),
        Value::String(s) => string_text(s),
        Value::Array(a) if !a.is_empty() && a.iter().all(is_symbol) => a
            .iter()
            .map(|e| {
                format!(
                    "{}={}",
                    e.get("name").map_or_else(String::new, render),
                    e.get("value").map_or_else(String::new, render)
                )
            })
            .collect::<Vec<_>>()
            .join(", "),
        Value::Array(a) if !a.is_empty() && a.iter().all(is_dependency) => {
            list_text(a.iter().filter_map(|e| e.get("dependsOn")).map(render))
        }
        Value::Array(a) => list_text(a.iter().map(|e| render_with(e, special))),
        Value::Object(m) => format!(
            "{{{}}}",
            m.iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| format!("{k}: {}", render_with(v, special)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// A string bare, with its control characters escaped (`\n`, `\r`, `\t`,
/// `\u{1b}`). It is quoted, as `"…"` with `"` escaped, when bare text
/// would be ambiguous: it is empty, starts or ends with whitespace, or
/// contains ` -> `.
fn string_text(s: &str) -> String {
    let quote = s.is_empty() || s.trim() != s || s.contains(" -> ");
    match quote {
        true => quoted(s),
        false => escape_controls(s),
    }
}

/// `s` in double quotes, with its control characters and `"` escaped.
fn quoted(s: &str) -> String {
    format!("\"{}\"", escape_controls(s).replace('"', "\\\""))
}

/// `s` with each control character written as an escape: `\n`, `\r`, `\t`,
/// and any other as `\u{..}`.
fn escape_controls(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// 2^53: every whole number up to it in magnitude is exactly an `f64`.
const MAX_EXACT_INTEGER: f64 = 9_007_199_254_740_992.0;

/// A JSON number; a float whose value is a whole number of at most
/// [`MAX_EXACT_INTEGER`] in magnitude is written without its `.0` (`4`,
/// not `4.0`).
fn number_text(n: &serde_json::Number) -> String {
    match n.as_f64() {
        Some(f) if n.is_f64() && f.fract() == 0.0 && f.abs() <= MAX_EXACT_INTEGER => {
            format!("{f:.0}")
        }
        _ => n.to_string(),
    }
}

/// A step dependency: `{dependsOn}`.
fn is_dependency(v: &Value) -> bool {
    v.as_object()
        .is_some_and(|m| m.len() == 1 && m.get("dependsOn").is_some_and(Value::is_string))
}

/// A symbol table entry: `{name, type, value}`.
fn is_symbol(v: &Value) -> bool {
    v.as_object().is_some_and(|m| {
        m.len() == 3 && m.contains_key("name") && m.contains_key("type") && m.contains_key("value")
    })
}

// ─── template layer ─────────────────────────────────────────────────

fn template_change(
    path: String,
    step: Option<&str>,
    field: &str,
    before: Option<Value>,
    after: Option<Value>,
) -> Change {
    Change {
        layer: Layer::Template,
        path,
        step: step.map(str::to_string),
        field: field.to_string(),
        before: before.map(Shown::generic),
        after: after.map(Shown::generic),
        origin: Origin::Template,
        stage: Stage::Template,
    }
}

fn template_layer(a: &JobTemplate, b: &JobTemplate) -> Vec<Change> {
    let mut out = Vec::new();
    if a.specification_version != b.specification_version {
        out.push(template_change(
            String::new(),
            None,
            "specificationVersion",
            Some(json!(a.specification_version)),
            Some(json!(b.specification_version)),
        ));
    }
    let exts = |t: &JobTemplate| {
        t.extensions
            .as_ref()
            .map(|v| json!(v.iter().map(|e| e.as_str()).collect::<Vec<_>>()))
    };
    if exts(a) != exts(b) {
        out.push(template_change(
            String::new(),
            None,
            "extensions",
            exts(a),
            exts(b),
        ));
    }

    let defs = |t: &'_ JobTemplate| -> IndexMap<String, Vec<(String, Value)>> {
        t.parameter_definitions_list()
            .iter()
            .map(|d| (d.name().to_string(), definition_record(d)))
            .collect()
    };
    let (da, db) = (defs(a), defs(b));
    for (name, ra) in &da {
        let path = format!("parameterDefinitions[{name}]");
        let Some(rb) = db.get(name) else {
            out.push(template_change(path, None, "", Some(record_json(ra)), None));
            continue;
        };
        let fields = ra.iter().map(|(k, _)| k).chain(
            rb.iter()
                .map(|(k, _)| k)
                .filter(|k| !ra.iter().any(|(x, _)| x == *k)),
        );
        for field in fields {
            let get =
                |r: &[(String, Value)]| r.iter().find(|(k, _)| k == field).map(|(_, v)| v.clone());
            let (va, vb) = (get(ra), get(rb));
            if va != vb {
                out.push(template_change(path.clone(), None, field, va, vb));
            }
        }
    }
    for (name, rb) in db.iter().filter(|(n, _)| !da.contains_key(*n)) {
        out.push(template_change(
            format!("parameterDefinitions[{name}]"),
            None,
            "",
            None,
            Some(record_json(rb)),
        ));
    }

    for sa in &a.steps {
        let Some(sb) = b.steps.iter().find(|s| s.name == sa.name) else {
            continue;
        };
        if sa.let_bindings != sb.let_bindings {
            out.push(template_change(
                format!("steps[{}]", sa.name),
                Some(&sa.name),
                "let",
                sa.let_bindings.as_ref().map(|v| json!(v)),
                sb.let_bindings.as_ref().map(|v| json!(v)),
            ));
        }
    }
    out
}

fn record_json(r: &[(String, Value)]) -> Value {
    Value::Object(r.iter().cloned().collect())
}

/// Builds a parameter definition's comparable fields, in a fixed order.
#[derive(Default)]
struct Record(Vec<(String, Value)>);

impl Record {
    fn put(&mut self, field: &str, v: Option<Value>) {
        if let Some(v) = v {
            self.0.push((field.to_string(), v));
        }
    }

    fn ui(&mut self, control: &Option<String>, label: &Option<String>, group: &Option<String>) {
        self.put("userInterface.control", control.as_ref().map(|v| json!(v)));
        self.put("userInterface.label", label.as_ref().map(|v| json!(v)));
        self.put("userInterface.groupLabel", group.as_ref().map(|v| json!(v)));
    }
}

fn int_json(v: &crate::template::FlexInt) -> Value {
    json!(v.0)
}

fn float_json(v: &crate::template::FlexFloat) -> Value {
    json!(v.0)
}

fn filters_json(f: &[crate::template::FileFilter]) -> Value {
    Value::Array(f.iter().map(filter_json).collect())
}

fn filter_json(f: &crate::template::FileFilter) -> Value {
    json!({"label": f.label, "patterns": f.patterns})
}

/// Every field of a parameter definition that can differ, flattened:
/// `userInterface.label`, `item.minValue`, `item.item.maxValue`.
fn definition_record(d: &JobParameterDefinition) -> Vec<(String, Value)> {
    use JobParameterDefinition as D;
    let mut r = Record::default();
    r.put("type", Some(json!(d.type_name())));
    r.put("description", d.description().map(|s| json!(s)));
    let ints = |v: &[crate::template::FlexInt]| json!(v.iter().map(|i| i.0).collect::<Vec<_>>());
    let floats =
        |v: &[crate::template::FlexFloat]| json!(v.iter().map(|f| f.0).collect::<Vec<_>>());
    match d {
        D::STRING(p) => {
            r.put("default", p.default.as_ref().map(|v| json!(v)));
            r.put("allowedValues", p.allowed_values.as_ref().map(|v| json!(v)));
            r.put("minLength", p.min_length.map(|v| json!(v)));
            r.put("maxLength", p.max_length.map(|v| json!(v)));
            if let Some(u) = &p.user_interface {
                r.ui(&u.control, &u.label, &u.group_label);
            }
        }
        D::PATH(p) => {
            r.put("default", p.default.as_ref().map(|v| json!(v)));
            r.put("allowedValues", p.allowed_values.as_ref().map(|v| json!(v)));
            r.put("minLength", p.min_length.map(|v| json!(v)));
            r.put("maxLength", p.max_length.map(|v| json!(v)));
            r.put("objectType", p.object_type.map(|v| to_json(&v)));
            r.put("dataFlow", p.data_flow.map(|v| to_json(&v)));
            if let Some(u) = &p.user_interface {
                r.ui(&u.control, &u.label, &u.group_label);
                r.put(
                    "userInterface.fileFilters",
                    u.file_filters.as_deref().map(filters_json),
                );
                r.put(
                    "userInterface.fileFilterDefault",
                    u.file_filter_default.as_ref().map(filter_json),
                );
            }
        }
        D::INT(p) => {
            r.put("default", p.default.as_ref().map(int_json));
            r.put("allowedValues", p.allowed_values.as_ref().map(|v| ints(v)));
            r.put("minValue", p.min_value.as_ref().map(int_json));
            r.put("maxValue", p.max_value.as_ref().map(int_json));
            if let Some(u) = &p.user_interface {
                r.ui(&u.control, &u.label, &u.group_label);
                r.put(
                    "userInterface.singleStepDelta",
                    u.single_step_delta.as_ref().map(int_json),
                );
            }
        }
        D::FLOAT(p) => {
            r.put("default", p.default.as_ref().map(float_json));
            r.put(
                "allowedValues",
                p.allowed_values.as_ref().map(|v| floats(v)),
            );
            r.put("minValue", p.min_value.as_ref().map(float_json));
            r.put("maxValue", p.max_value.as_ref().map(float_json));
            if let Some(u) = &p.user_interface {
                r.ui(&u.control, &u.label, &u.group_label);
                r.put("userInterface.decimals", u.decimals.as_ref().map(int_json));
                r.put(
                    "userInterface.singleStepDelta",
                    u.single_step_delta.as_ref().map(float_json),
                );
            }
        }
        D::BOOL(p) => {
            r.put("default", p.default.as_ref().map(|v| json!(v.0)));
            if let Some(u) = &p.user_interface {
                r.ui(&u.control, &u.label, &u.group_label);
            }
        }
        D::RANGE_EXPR(p) => {
            r.put("default", p.default.as_ref().map(|v| json!(v)));
            r.put("minLength", p.min_length.map(|v| json!(v)));
            r.put("maxLength", p.max_length.map(|v| json!(v)));
            if let Some(u) = &p.user_interface {
                r.ui(&u.control, &u.label, &u.group_label);
            }
        }
        D::LIST_STRING(p) => {
            r.put("default", p.default.as_ref().map(|v| json!(v)));
            r.put("minLength", p.min_length.map(|v| json!(v)));
            r.put("maxLength", p.max_length.map(|v| json!(v)));
            if let Some(i) = &p.item {
                r.put(
                    "item.allowedValues",
                    i.allowed_values.as_ref().map(|v| json!(v)),
                );
                r.put("item.minLength", i.min_length.map(|v| json!(v)));
                r.put("item.maxLength", i.max_length.map(|v| json!(v)));
            }
            if let Some(u) = &p.user_interface {
                r.ui(&u.control, &u.label, &u.group_label);
            }
        }
        D::LIST_PATH(p) => {
            r.put("default", p.default.as_ref().map(|v| json!(v)));
            r.put("minLength", p.min_length.map(|v| json!(v)));
            r.put("maxLength", p.max_length.map(|v| json!(v)));
            r.put("objectType", p.object_type.map(|v| to_json(&v)));
            r.put("dataFlow", p.data_flow.map(|v| to_json(&v)));
            if let Some(i) = &p.item {
                r.put(
                    "item.allowedValues",
                    i.allowed_values.as_ref().map(|v| json!(v)),
                );
                r.put("item.minLength", i.min_length.map(|v| json!(v)));
                r.put("item.maxLength", i.max_length.map(|v| json!(v)));
            }
            if let Some(u) = &p.user_interface {
                r.ui(&u.control, &u.label, &u.group_label);
                r.put(
                    "userInterface.fileFilters",
                    u.file_filters.as_deref().map(filters_json),
                );
                r.put(
                    "userInterface.fileFilterDefault",
                    u.file_filter_default.as_ref().map(filter_json),
                );
            }
        }
        D::LIST_INT(p) => {
            r.put("default", p.default.as_deref().map(ints));
            r.put("minLength", p.min_length.map(|v| json!(v)));
            r.put("maxLength", p.max_length.map(|v| json!(v)));
            if let Some(i) = &p.item {
                r.put("item.allowedValues", i.allowed_values.as_deref().map(ints));
                r.put("item.minValue", i.min_value.as_ref().map(int_json));
                r.put("item.maxValue", i.max_value.as_ref().map(int_json));
            }
            if let Some(u) = &p.user_interface {
                r.ui(&u.control, &u.label, &u.group_label);
                r.put(
                    "userInterface.singleStepDelta",
                    u.single_step_delta.as_ref().map(int_json),
                );
            }
        }
        D::LIST_FLOAT(p) => {
            r.put("default", p.default.as_deref().map(floats));
            r.put("minLength", p.min_length.map(|v| json!(v)));
            r.put("maxLength", p.max_length.map(|v| json!(v)));
            if let Some(i) = &p.item {
                r.put(
                    "item.allowedValues",
                    i.allowed_values.as_deref().map(floats),
                );
                r.put("item.minValue", i.min_value.as_ref().map(float_json));
                r.put("item.maxValue", i.max_value.as_ref().map(float_json));
            }
            if let Some(u) = &p.user_interface {
                r.ui(&u.control, &u.label, &u.group_label);
                r.put("userInterface.decimals", u.decimals.as_ref().map(int_json));
                r.put(
                    "userInterface.singleStepDelta",
                    u.single_step_delta.as_ref().map(float_json),
                );
            }
        }
        D::LIST_BOOL(p) => {
            r.put(
                "default",
                p.default
                    .as_ref()
                    .map(|v| json!(v.iter().map(|b| b.0).collect::<Vec<_>>())),
            );
            r.put("minLength", p.min_length.map(|v| json!(v)));
            r.put("maxLength", p.max_length.map(|v| json!(v)));
            if let Some(u) = &p.user_interface {
                r.ui(&u.control, &u.label, &u.group_label);
            }
        }
        D::LIST_LIST_INT(p) => {
            r.put(
                "default",
                p.default.as_ref().map(|v| {
                    json!(v
                        .iter()
                        .map(|l| l.iter().map(|i| i.0).collect::<Vec<_>>())
                        .collect::<Vec<_>>())
                }),
            );
            r.put("minLength", p.min_length.map(|v| json!(v)));
            r.put("maxLength", p.max_length.map(|v| json!(v)));
            if let Some(i) = &p.item {
                r.put("item.minLength", i.min_length.map(|v| json!(v)));
                r.put("item.maxLength", i.max_length.map(|v| json!(v)));
                if let Some(ii) = &i.item {
                    r.put(
                        "item.item.allowedValues",
                        ii.allowed_values.as_deref().map(ints),
                    );
                    r.put("item.item.minValue", ii.min_value.as_ref().map(int_json));
                    r.put("item.item.maxValue", ii.max_value.as_ref().map(int_json));
                }
            }
            if let Some(u) = &p.user_interface {
                r.ui(&u.control, &u.label, &u.group_label);
            }
        }
    }
    r.0
}

// ─── steps and tasks ────────────────────────────────────────────────

/// A step's task count, or why it cannot be counted.
fn task_count(step: &Step) -> Result<u64, String> {
    let Some(space) = &step.parameter_space else {
        return Ok(1);
    };
    StepParameterSpaceIterator::new(space)
        .map(|it| it.len())
        .map_err(|e| format!("cannot count tasks: {e}"))
}

fn box_summary(b: &TaskBox) -> BoxSummary {
    BoxSummary {
        count: b.len(),
        dims: b
            .space()
            .task_parameter_definitions
            .iter()
            .map(|(name, p)| (name.clone(), values_text(p)))
            .collect(),
    }
}

/// A box dimension's values: integers as [`int_range_text`], other values
/// comma-separated.
fn values_text(p: &TaskParameter) -> String {
    match p {
        TaskParameter::Int { range, .. } | TaskParameter::ChunkInt { range, .. } => {
            int_range_text(range)
        }
        TaskParameter::Float { range } => range
            .iter()
            .map(Float64::to_display_string)
            .collect::<Vec<_>>()
            .join(", "),
        TaskParameter::String { range } | TaskParameter::Path { range } => range.join(", "),
    }
}

/// `[1,2,3,7,9,11,4,5]` -> `1-3,7-11:2,4,5`: runs of three or more values
/// with a positive step, as a range expression's canonical form writes them.
fn int_runs(v: &[i64]) -> String {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < v.len() {
        let step = v.get(i + 1).and_then(|n| n.checked_sub(v[i]));
        let mut j = i;
        if let Some(d) = step.filter(|d| *d > 0) {
            while j + 1 < v.len() && v[j + 1].checked_sub(v[j]) == Some(d) {
                j += 1;
            }
            if j - i >= 2 {
                parts.push(if d == 1 {
                    format!("{}-{}", v[i], v[j])
                } else {
                    format!("{}-{}:{d}", v[i], v[j])
                });
                i = j + 1;
                continue;
            }
        }
        parts.push(v[i].to_string());
        i += 1;
    }
    parts.join(",")
}

fn space_of(s: &Step) -> Option<&StepParameterSpace> {
    s.parameter_space.as_ref()
}

fn step_summaries(
    old: &Job,
    new: &Job,
    options: &DiffOptions,
    notes: &mut Vec<Note>,
) -> Vec<StepSummary> {
    /// The count, or 0 with the reason kept in `errors`.
    fn counted(step: &Step, errors: &mut Vec<String>) -> u64 {
        task_count(step).unwrap_or_else(|e| {
            errors.push(e);
            0
        })
    }
    let status_only = options.clone().with_max_depth(0);
    let mut names: Vec<&str> = old.steps.iter().map(|s| s.name.as_str()).collect();
    names.extend(
        new.steps
            .iter()
            .map(|s| s.name.as_str())
            .filter(|n| !old.steps.iter().any(|s| s.name == *n)),
    );
    let mut out = Vec::new();
    for name in names {
        let a = old.steps.iter().position(|s| s.name == name);
        let b = new.steps.iter().position(|s| s.name == name);
        let mut errors = Vec::new();
        let (status, tasks) = match (a.map(|i| &old.steps[i]), b.map(|j| &new.steps[j])) {
            (Some(x), Some(y)) => {
                let status = if diff_steps(x, y, &status_only).is_changed() {
                    DiffStatus::Modified
                } else {
                    DiffStatus::Unchanged
                };
                let tasks = match task_delta(space_of(x), space_of(y)) {
                    TaskDelta::Unchanged => TaskSummary::Unchanged {
                        count: counted(y, &mut errors),
                    },
                    TaskDelta::Product(d) => TaskSummary::Product {
                        kept: box_summary(&d.kept),
                        added: d.added.iter().map(box_summary).collect(),
                        removed: d.removed.iter().map(box_summary).collect(),
                    },
                    TaskDelta::Fallback { reason } => TaskSummary::Replaced {
                        old_count: counted(x, &mut errors),
                        new_count: counted(y, &mut errors),
                        reason,
                    },
                };
                (status, tasks)
            }
            (Some(x), None) => (
                DiffStatus::Removed,
                TaskSummary::Removed {
                    count: counted(x, &mut errors),
                },
            ),
            (None, Some(y)) => (
                DiffStatus::Added,
                TaskSummary::Added {
                    count: counted(y, &mut errors),
                },
            ),
            (None, None) => continue,
        };
        let adaptive = a
            .map(|i| &old.steps[i])
            .into_iter()
            .chain(b.map(|j| &new.steps[j]))
            .any(is_adaptive);
        notes.extend(errors.iter().map(|e| Note::step(name, e.clone())));
        let unknown = if adaptive {
            notes.push(Note::step(name, ADAPTIVE_NOTE.to_string()));
            Some("adaptive chunking")
        } else if !errors.is_empty() {
            Some("cannot count tasks")
        } else {
            None
        };
        let (tasks, tasks_unknown) = match unknown {
            Some(why) => {
                let zeroed = match tasks {
                    TaskSummary::Unchanged { .. } => TaskSummary::Unchanged { count: 0 },
                    TaskSummary::Added { .. } => TaskSummary::Added { count: 0 },
                    TaskSummary::Removed { .. } => TaskSummary::Removed { count: 0 },
                    TaskSummary::Product { .. } | TaskSummary::Replaced { .. } => {
                        TaskSummary::Replaced {
                            old_count: 0,
                            new_count: 0,
                            reason: why.into(),
                        }
                    }
                };
                (zeroed, Some(why.to_string()))
            }
            None => (tasks, None),
        };
        out.push(StepSummary {
            name: name.to_string(),
            status,
            old_index: a,
            new_index: b,
            tasks,
            tasks_unknown,
        });
    }
    out
}

const ADAPTIVE_NOTE: &str = "cannot count tasks: the chunk size is adaptive \
    (targetRuntimeSeconds), so the number of chunks is decided at run time";

/// Whether the step's chunk size adapts at run time, so its task count is
/// not known ahead of it.
fn is_adaptive(step: &Step) -> bool {
    space_of(step)
        .and_then(|s| StepParameterSpaceIterator::new(s).ok())
        .is_some_and(|it| it.chunks_adaptive())
}

// ─── built-job reader ───────────────────────────────────────────────

/// The serde form of a [`Job`] as [`JobDocument`] writes it, read field by
/// field.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BuiltJob {
    /// Checked by [`check_document_version`] before this is read.
    #[serde(rename = "documentVersion")]
    _document_version: serde::de::IgnoredAny,
    name: String,
    description: Option<String>,
    extensions: Option<Vec<String>>,
    // Not an `Option`: `openjd build` always writes the map, so a `null` is
    // an error like any other unexpected `null`.
    #[serde(default)]
    parameters: IndexMap<String, Value>,
    steps: Vec<BuiltStep>,
    job_environments: Option<Vec<Environment>>,
}

/// A [`Step`] as [`JobDocument`] writes it: its serde form, except that the
/// parameter space is in the host encoding, read by hand.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuiltStep {
    name: String,
    description: Option<String>,
    script: crate::job::StepScript,
    step_environments: Option<Vec<Environment>>,
    parameter_space: Option<Value>,
    host_requirements: Option<crate::job::HostRequirements>,
    dependencies: Option<Vec<crate::job::StepDependency>>,
    #[serde(rename = "resolvedSymTab")]
    resolved_symtab: Option<openjd_expr::symbol_table::SerializedSymbolTable>,
}

/// Read a [`Job`] back from the JSON or YAML that `openjd build` prints
/// (its [`JobDocument`]). JSON when the first non-whitespace character is
/// `{`, YAML otherwise. The document's `documentVersion` must be
/// [`JobDocument::VERSION`], the one [`JobDocument`] writes. Unknown fields
/// are rejected at every level, and so are a parameter or task parameter
/// definition that is not in the host encoding, a value not in the exact
/// form a built job writes (`"TRUE"` for a BOOL), a `resolvedSymTab` entry
/// that is not the `{name, type, value}` a `SymbolTable` serializes to, a
/// number or bool where the document writes a string (or a string where it
/// writes a bool or a number), and the forms
/// template validation rejects (empty names, unknown or repeated
/// dependencies, empty host requirements, ...). `specs/model/host-support.md`,
/// "Reading a built job", lists every check.
///
/// Unstable: see the module docs.
pub fn job_from_str(text: &str) -> Result<Job, ModelError> {
    let err = |m: String| ModelError::DecodeValidation(format!("not a built job: {m}"));
    let json = text.trim_start().starts_with('{');
    let input: Value = if json {
        serde_json::from_str(text).map_err(|e| err(e.to_string()))?
    } else {
        serde_saphyr::from_str(text).map_err(|e| err(e.to_string()))?
    };
    check_document_version(&input).map_err(err)?;
    let raw: BuiltJob = if json {
        serde_json::from_str(text).map_err(|e| err(e.to_string()))?
    } else {
        serde_saphyr::from_str(text).map_err(|e| err(e.to_string()))?
    };
    let job = job_from_built(raw).map_err(err)?;
    // Most job types ignore unknown fields when they deserialize, so a key
    // the parsed job does not write back is one that was dropped.
    let output = serde_json::to_value(JobDocument(&job)).map_err(|e| err(e.to_string()))?;
    if let Some(path) = dropped_key(&input, &output, String::new()) {
        return Err(err(format!("{path}: unknown field")));
    }
    if let Some((path, kind)) = coerced_scalar(&input, &output, String::new()) {
        return Err(err(format!("{path}: expected {kind}")));
    }
    check_symbol_tables(&job).map_err(err)?;
    check_created_forms(&job).map_err(err)?;
    Ok(job)
}

/// Check the document is an object whose `documentVersion` is
/// [`JobDocument::VERSION`].
fn check_document_version(input: &Value) -> Result<(), String> {
    let Some(m) = input.as_object() else {
        return Err("expected an object".into());
    };
    match m.get("documentVersion") {
        Some(v) if v.as_u64() == Some(JobDocument::VERSION) => Ok(()),
        Some(v) => Err(format!(
            "documentVersion: {v} is not supported; the supported version is {}",
            JobDocument::VERSION
        )),
        None => Err(format!(
            "missing field 'documentVersion'; the supported version is {}",
            JobDocument::VERSION
        )),
    }
}

/// Check every `resolvedSymTab` has the shape a [`SymbolTable`] serializes
/// to: a list of `{name, type, value}` whose `type` parses and whose
/// `value` has that type.
///
/// [`SymbolTable`]: openjd_expr::SymbolTable
fn check_symbol_tables(job: &Job) -> Result<(), String> {
    fn envs(envs: &Option<Vec<Environment>>, prefix: &str) -> Result<(), String> {
        for (j, e) in envs.iter().flatten().enumerate() {
            if let Some(t) = &e.resolved_symtab {
                check_symbol_table(t, &format!("{prefix}[{j}].resolvedSymTab"))?;
            }
        }
        Ok(())
    }
    envs(&job.job_environments, "jobEnvironments")?;
    for (i, step) in job.steps.iter().enumerate() {
        if let Some(t) = &step.resolved_symtab {
            check_symbol_table(t, &format!("steps[{i}].resolvedSymTab"))?;
        }
        envs(
            &step.step_environments,
            &format!("steps[{i}].stepEnvironments"),
        )?;
    }
    Ok(())
}

fn check_symbol_table(
    table: &openjd_expr::symbol_table::SerializedSymbolTable,
    path: &str,
) -> Result<(), String> {
    const KEYS: [&str; 3] = ["name", "type", "value"];
    let Some(entries) = table.as_value().as_array() else {
        return Err(format!("{path}: expected an array of symbol-table entries"));
    };
    let mut names = HashSet::new();
    for (i, entry) in entries.iter().enumerate() {
        let at = format!("{path}[{i}]");
        let Some(m) = entry.as_object() else {
            return Err(format!(
                "{at}: expected an object with 'name', 'type' and 'value'"
            ));
        };
        if let Some(k) = m.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(format!("{at}.{k}: unknown field"));
        }
        if let Some(k) = KEYS.iter().find(|k| !m.contains_key(**k)) {
            return Err(format!("{at}: missing field '{k}'"));
        }
        let string = |k: &str| {
            m[k].as_str()
                .ok_or_else(|| format!("{at}.{k}: expected a string"))
        };
        // The name's syntax is not checked, only that it is not repeated.
        let name = string("name")?;
        if !names.insert(name) {
            return Err(format!("{at}.name: '{name}' is repeated"));
        }
        let ty = ExprType::parse(string("type")?).map_err(|e| format!("{at}.type: {e}"))?;
        let value = ExprValue::from_transport_value(&m["value"], &ty, PathFormat::Posix)
            .map_err(|e| format!("{at}.value: {e}"))?;
        // Only the form a `SymbolTable` writes: `"TRUE"` is not a BOOL here.
        let written = value.transport_value();
        if m["value"] != written {
            return Err(format!(
                "{at}.value: {} is not in the form a built job writes ({written})",
                m["value"]
            ));
        }
    }
    table
        .to_symtab(PathFormat::Posix)
        .map(drop)
        .map_err(|e| format!("{path}: {e}"))
}

/// `Err("{path}: must not be empty")` for a list that is present and empty:
/// template validation rejects one, so `create_job` never writes it.
fn non_empty<T>(list: &Option<Vec<T>>, path: impl FnOnce() -> String) -> Result<(), String> {
    match list {
        Some(v) if v.is_empty() => Err(format!("{}: must not be empty", path())),
        _ => Ok(()),
    }
}

/// A literal `timeout` or `notifyPeriodInSeconds` must be a positive
/// integer, as template validation requires; a format string is resolved
/// on the worker and is not checked here.
fn positive_integer(
    value: &Option<FormatString>,
    path: impl FnOnce() -> String,
) -> Result<(), String> {
    match value {
        Some(v) if v.is_literal() && !v.raw().trim().parse::<i64>().is_ok_and(|n| n > 0) => {
            Err(format!("{}: must be a positive integer", path()))
        }
        _ => Ok(()),
    }
}

fn check_action(a: &crate::job::Action, path: &str) -> Result<(), String> {
    use crate::job::CancelationMode;
    non_empty(&a.args, || format!("{path}.args"))?;
    positive_integer(&a.timeout, || format!("{path}.timeout"))?;
    match &a.cancelation {
        Some(
            CancelationMode::NotifyThenTerminate {
                notify_period_in_seconds,
            }
            | CancelationMode::DeferredMode {
                notify_period_in_seconds,
                ..
            },
        ) => positive_integer(notify_period_in_seconds, || {
            format!("{path}.cancelation.notifyPeriodInSeconds")
        }),
        _ => Ok(()),
    }
}

fn check_environment(e: &Environment, path: &str, wrap_actions: bool) -> Result<(), String> {
    if e.variables.as_ref().is_some_and(|v| v.is_empty()) {
        return Err(format!("{path}.variables: must not be empty"));
    }
    if let Some(script) = &e.script {
        if !script.actions.has_any_action() {
            return Err(format!(
                "{path}.script.actions: {}",
                if wrap_actions {
                    "must define at least one of onEnter or onExit, or the complete set of wrap \
                     hooks (onWrapEnvEnter, onWrapTaskRun, and onWrapEnvExit together)"
                } else {
                    "must define at least one of onEnter or onExit"
                }
            ));
        }
        check_embedded_files(&script.embedded_files, &format!("{path}.script"))?;
        for (name, a) in script.actions.iter_named() {
            check_action(a, &format!("{path}.script.actions.{name}"))?;
        }
    }
    Ok(())
}

/// A script's `embeddedFiles`: non-empty, and every file has non-empty
/// `data`. `path` names the script.
fn check_embedded_files(
    files: &Option<Vec<crate::job::EmbeddedFile>>,
    path: &str,
) -> Result<(), String> {
    let at = format!("{path}.embeddedFiles");
    non_empty(files, || at.clone())?;
    for (k, f) in files.iter().flatten().enumerate() {
        match &f.data {
            None => {
                return Err(format!(
                    "{at}[{k}]: embedded file '{}' is missing 'data' field",
                    f.name
                ))
            }
            Some(d) if d.raw().is_empty() => {
                return Err(format!("{at}[{k}].data: must not be empty"))
            }
            Some(_) => {}
        }
    }
    Ok(())
}

fn check_environments(
    envs: &Option<Vec<Environment>>,
    path: &str,
    wrap_actions: bool,
) -> Result<(), String> {
    non_empty(envs, || path.to_string())?;
    for (j, e) in envs.iter().flatten().enumerate() {
        check_environment(e, &format!("{path}[{j}]"), wrap_actions)?;
    }
    Ok(())
}

/// The checks template validation makes on host requirements, with its
/// messages. `path` names the step's `hostRequirements`.
fn check_host_requirements(h: &crate::job::HostRequirements, path: &str) -> Result<(), String> {
    let some = |l: Option<usize>| l.is_some_and(|n| n > 0);
    if !some(h.amounts.as_ref().map(Vec::len)) && !some(h.attributes.as_ref().map(Vec::len)) {
        return Err(format!(
            "{path}: must have at least one of amounts or attributes"
        ));
    }
    for (k, a) in h.amounts.iter().flatten().enumerate() {
        let at = format!("{path}.amounts[{k}]");
        match (a.min, a.max) {
            (None, None) => return Err(format!("{at}: must have at least one of min or max")),
            (Some(min), _) if min < 0.0 => return Err(format!("{at}.min: must be non-negative")),
            (_, Some(max)) if max <= 0.0 => return Err(format!("{at}.max: must be positive")),
            (Some(min), Some(max)) if min > max => {
                return Err(format!("{at}: min ({min}) > max ({max})"))
            }
            _ => {}
        }
    }
    for (k, a) in h.attributes.iter().flatten().enumerate() {
        let at = format!("{path}.attributes[{k}]");
        if a.any_of.is_none() && a.all_of.is_none() {
            return Err(format!("{at}: must have at least one of anyOf or allOf"));
        }
        non_empty(&a.any_of, || format!("{at}.anyOf"))?;
        non_empty(&a.all_of, || format!("{at}.allOf"))?;
    }
    Ok(())
}

/// The job-level checks: a non-empty `name`, and an `extensions` list
/// that is non-empty with no repeats.
fn check_job_fields(job: &Job) -> Result<(), String> {
    if job.name.is_empty() {
        return Err("name: must not be empty".into());
    }
    if let Some(exts) = &job.extensions {
        if exts.is_empty() {
            return Err("extensions: if provided, must be a non-empty list".into());
        }
        let mut seen = HashSet::new();
        if let Some(e) = exts.iter().find(|e| !seen.insert(*e)) {
            return Err(format!(
                "extensions: Duplicate values for extension name are not allowed. \
                 Duplicate values: {}",
                e.as_str()
            ));
        }
    }
    Ok(())
}

/// Step names are non-empty and unique, and each step's dependencies name
/// another step of the job, once each.
fn check_steps_and_dependencies(job: &Job) -> Result<(), String> {
    let names: HashSet<&str> = job.steps.iter().map(|s| s.name.as_str()).collect();
    let mut seen = HashSet::new();
    for (i, s) in job.steps.iter().enumerate() {
        let at = format!("steps[{i}]");
        if s.name.is_empty() {
            return Err(format!("{at}.name: must not be empty"));
        }
        if !seen.insert(s.name.as_str()) {
            return Err(format!("{at}.name: duplicate step name: '{}'", s.name));
        }
        non_empty(&s.dependencies, || format!("{at}.dependencies"))?;
        let mut deps = HashSet::new();
        for (j, d) in s.dependencies.iter().flatten().enumerate() {
            let dep = format!("{at}.dependencies[{j}]");
            let on = d.depends_on.as_str();
            if on == s.name {
                return Err(format!("{dep}: cannot depend on itself"));
            }
            if !names.contains(on) {
                return Err(format!("{dep}: dependency '{on}' not found"));
            }
            if !deps.insert(on) {
                return Err(format!("{dep}: duplicate dependency '{on}'"));
            }
        }
    }
    Ok(())
}

/// Reject the forms template validation rules out, so `create_job` never
/// writes them. Each check, and its message, is listed in
/// `specs/model/host-support.md`, "Reading a built job".
fn check_created_forms(job: &Job) -> Result<(), String> {
    check_job_fields(job)?;
    if job.steps.is_empty() {
        return Err("steps: must not be empty".into());
    }
    check_steps_and_dependencies(job)?;
    let wrap_actions = job
        .extensions
        .iter()
        .flatten()
        .any(|e| *e == ModelExtension::WrapActions);
    check_environments(&job.job_environments, "jobEnvironments", wrap_actions)?;
    for (i, s) in job.steps.iter().enumerate() {
        let at = format!("steps[{i}]");
        check_environments(
            &s.step_environments,
            &format!("{at}.stepEnvironments"),
            wrap_actions,
        )?;
        check_embedded_files(&s.script.embedded_files, &format!("{at}.script"))?;
        check_action(
            &s.script.actions.on_run,
            &format!("{at}.script.actions.onRun"),
        )?;
        if let Some(h) = &s.host_requirements {
            check_host_requirements(h, &format!("{at}.hostRequirements"))?;
        }
    }
    Ok(())
}

/// The path of the first scalar in `input` whose kind differs from the
/// one `output` writes there, with the kind `output` writes, or `None`.
/// The job types read a number or a bool into a string field as its text
/// (`command: 7` as `"7"`), and the YAML reader reads a quoted scalar into
/// a bool or number field (`runnable: "TRUE"`, `min: "01"`). `create_job`
/// writes neither, so both are rejected.
fn coerced_scalar(input: &Value, output: &Value, path: String) -> Option<(String, &'static str)> {
    match (input, output) {
        (Value::Object(i), Value::Object(o)) => i.iter().find_map(|(k, v)| {
            let written = o.get(k).or_else(|| {
                ALIASES
                    .iter()
                    .find(|(alias, _)| alias == k)
                    .and_then(|(_, name)| o.get(*name))
            })?;
            let key_path = if path.is_empty() {
                k.clone()
            } else {
                format!("{path}.{k}")
            };
            coerced_scalar(v, written, key_path)
        }),
        (Value::Array(i), Value::Array(o)) => i
            .iter()
            .zip(o)
            .enumerate()
            .find_map(|(n, (iv, ov))| coerced_scalar(iv, ov, format!("{path}[{n}]"))),
        (Value::Number(_) | Value::Bool(_), Value::String(_)) => Some((path, "a string")),
        (Value::String(_), Value::Bool(_)) => Some((path, "a boolean")),
        (Value::String(_), Value::Number(_)) => Some((path, "a number")),
        _ => None,
    }
}

// Objects in a job document, as paths with each list index written `[]`
// and `*` for any one key.
const HAS_SYMTAB: &[&str] = &["steps[]", "jobEnvironments[]", "steps[].stepEnvironments[]"];
const ENV_ACTIONS: &[&str] = &[
    "jobEnvironments[].script.actions",
    "steps[].stepEnvironments[].script.actions",
];
const CANCELATIONS: &[&str] = &[
    "steps[].script.actions.onRun.cancelation",
    "jobEnvironments[].script.actions.*.cancelation",
    "steps[].stepEnvironments[].script.actions.*.cancelation",
];

/// Fields the job types skip when they are `None`, and the objects that
/// have them; there, `null` reads back as absent.
const SKIPPED_WHEN_NONE: [(&str, &[&str]); 5] = [
    ("resolvedSymTab", HAS_SYMTAB),
    ("onWrapEnvEnter", ENV_ACTIONS),
    ("onWrapTaskRun", ENV_ACTIONS),
    ("onWrapEnvExit", ENV_ACTIONS),
    ("notifyPeriodInSeconds", CANCELATIONS),
];

/// Serde aliases in the job types, and the key each one is written as.
/// They need no scope: only scripts write `let` and only embedded files
/// write `fileType`, so an alias elsewhere finds nothing and is rejected.
const ALIASES: [(&str, &str); 2] = [("letBindings", "let"), ("type", "fileType")];

/// Whether the object at `path` matches one of `patterns`.
fn at_object(path: &str, patterns: &[&str]) -> bool {
    let mut shape = String::with_capacity(path.len());
    let mut in_index = false;
    for c in path.chars() {
        match c {
            '[' => {
                in_index = true;
                shape.push_str("[]");
            }
            ']' => in_index = false,
            _ if in_index => {}
            _ => shape.push(c),
        }
    }
    patterns.iter().any(|p| {
        let (mut a, mut b) = (shape.split('.'), p.split('.'));
        loop {
            match (a.next(), b.next()) {
                (None, None) => return true,
                (Some(x), Some(y)) if y == "*" || x == y => {}
                _ => return false,
            }
        }
    })
}

/// The path of the first key in `input` that `output` does not have, or
/// `None`. Only object keys are compared, so values that are equal but
/// written differently (`2` and `2.0`) are not flagged.
fn dropped_key(input: &Value, output: &Value, path: String) -> Option<String> {
    match (input, output) {
        (Value::Object(i), Value::Object(o)) => i.iter().find_map(|(k, v)| {
            let key_path = if path.is_empty() {
                k.clone()
            } else {
                format!("{path}.{k}")
            };
            let written = o.get(k).or_else(|| {
                ALIASES
                    .iter()
                    .find(|(alias, _)| alias == k)
                    .and_then(|(_, name)| o.get(*name))
            });
            let skipped = || {
                SKIPPED_WHEN_NONE
                    .iter()
                    .any(|(name, objects)| name == k && at_object(&path, objects))
            };
            match written {
                Some(w) => dropped_key(v, w, key_path),
                None if v.is_null() && skipped() => None,
                None => Some(key_path),
            }
        }),
        (Value::Array(i), Value::Array(o)) => i
            .iter()
            .zip(o)
            .enumerate()
            .find_map(|(n, (iv, ov))| dropped_key(iv, ov, format!("{path}[{n}]"))),
        _ => None,
    }
}

/// The [`Job`] a [`BuiltJob`] describes; errors are bare messages.
fn job_from_built(raw: BuiltJob) -> Result<Job, String> {
    let extensions = raw
        .extensions
        .map(|v| {
            v.iter()
                .map(|e| ModelExtension::from_str(e))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()
        .map_err(|e| format!("extensions: {e}"))?;
    let mut parameters = IndexMap::new();
    for (k, v) in raw.parameters {
        let p = parameter_from_document(&k, &v, &format!("parameters[{k}]"))?;
        parameters.insert(k, p);
    }
    let steps = raw
        .steps
        .into_iter()
        .enumerate()
        .map(|(i, s)| step_from_built(s, i))
        .collect::<Result<_, _>>()?;
    Ok(Job {
        name: raw.name,
        description: raw.description,
        extensions,
        parameters,
        steps,
        job_environments: raw.job_environments,
    })
}

/// The [`Step`] a [`BuiltStep`] describes; `i` is its index, for errors.
fn step_from_built(s: BuiltStep, i: usize) -> Result<Step, String> {
    let BuiltStep {
        name,
        description,
        script,
        step_environments,
        parameter_space,
        host_requirements,
        dependencies,
        resolved_symtab,
    } = s;
    let parameter_space = match parameter_space {
        None | Some(Value::Null) => None,
        Some(v) => Some(space_from_document(
            &v,
            &format!("steps[{i}].parameterSpace"),
        )?),
    };
    // A struct literal: a new `Step` field fails to compile here until the
    // reader handles it.
    Ok(Step {
        name,
        description,
        script,
        step_environments,
        parameter_space,
        host_requirements,
        dependencies,
        resolved_symtab,
    })
}

// ─── serde helpers ──────────────────────────────────────────────────

/// A [`Job`] serialized as `openjd build` prints it, and as every host
/// document writes it: the job's serde form, except that
///
/// - it starts with `"documentVersion": 1`, the version of this format;
/// - `parameters` and each environment's `variables` are sorted by name
///   (`create_job` fills `Job::parameters` from a `HashMap`, and
///   `Environment::variables` is one);
/// - a parameter is `{"name", "type", "value"}` and a parameter space is
///   `{"taskParameterDefinitions": [{"name", "type", "range" | "list",
///   "chunks"?}, …], "combination"}`, the host encoding of
///   `specs/model/host-support.md`, with no cached sizes or Rust variant
///   names.
///
/// The definitions are a list in definition order, which the job keeps, so
/// the document means the same as a `serde_json::Value`. [`job_from_str`]
/// reads it back.
#[derive(Debug, Clone, Copy)]
pub struct JobDocument<'a>(pub &'a Job);

impl JobDocument<'_> {
    /// The `documentVersion` every job document writes, and the only one
    /// [`job_from_str`] reads. Digests compare only within one version.
    pub const VERSION: u64 = 1;
}

impl Serialize for JobDocument<'_> {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        // Exhaustive destructures: a new field fails to compile here until
        // the document writes it.
        let Job {
            name,
            description,
            extensions,
            parameters,
            steps,
            job_environments,
        } = self.0;
        DocJob {
            document_version: JobDocument::VERSION,
            name,
            description,
            extensions,
            parameters: parameters
                .iter()
                .map(|(k, p)| (k, ParameterDoc::new(p)))
                .collect(),
            steps: steps.iter().map(DocStep::new).collect(),
            job_environments: job_environments
                .as_ref()
                .map(|v| v.iter().map(DocEnvironment::new).collect()),
        }
        .serialize(ser)
    }
}

/// Field names and attributes mirror [`Job`], with `parameters` sorted,
/// after the format's `documentVersion`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DocJob<'a> {
    document_version: u64,
    name: &'a String,
    description: &'a Option<String>,
    extensions: &'a Option<Vec<ModelExtension>>,
    parameters: std::collections::BTreeMap<&'a String, ParameterDoc<'a>>,
    steps: Vec<DocStep<'a>>,
    job_environments: Option<Vec<DocEnvironment<'a>>>,
}

/// Field names and attributes mirror [`Step`].
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocStep<'a> {
    name: &'a String,
    description: &'a Option<String>,
    script: &'a crate::job::StepScript,
    step_environments: Option<Vec<DocEnvironment<'a>>>,
    parameter_space: Option<SpaceDoc<'a>>,
    host_requirements: &'a Option<crate::job::HostRequirements>,
    dependencies: &'a Option<Vec<crate::job::StepDependency>>,
    #[serde(rename = "resolvedSymTab", skip_serializing_if = "Option::is_none")]
    resolved_symtab: &'a Option<openjd_expr::symbol_table::SerializedSymbolTable>,
}

impl<'a> DocStep<'a> {
    pub(crate) fn new(step: &'a Step) -> Self {
        let Step {
            name,
            description,
            script,
            step_environments,
            parameter_space,
            host_requirements,
            dependencies,
            resolved_symtab,
        } = step;
        Self {
            name,
            description,
            script,
            step_environments: step_environments
                .as_ref()
                .map(|v| v.iter().map(DocEnvironment::new).collect()),
            parameter_space: parameter_space.as_ref().map(SpaceDoc::new),
            host_requirements,
            dependencies,
            resolved_symtab,
        }
    }
}

/// Field names and attributes mirror [`Environment`], with `variables`
/// sorted.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocEnvironment<'a> {
    name: &'a String,
    description: &'a Option<String>,
    script: &'a Option<crate::job::EnvironmentScript>,
    variables: Option<std::collections::BTreeMap<&'a String, &'a FormatString>>,
    #[serde(rename = "resolvedSymTab", skip_serializing_if = "Option::is_none")]
    resolved_symtab: &'a Option<openjd_expr::symbol_table::SerializedSymbolTable>,
}

impl<'a> DocEnvironment<'a> {
    pub(crate) fn new(env: &'a Environment) -> Self {
        let Environment {
            name,
            description,
            script,
            variables,
            resolved_symtab,
        } = env;
        Self {
            name,
            description,
            script,
            variables: variables.as_ref().map(|m| m.iter().collect()),
            resolved_symtab,
        }
    }
}

fn ser_job<S: Serializer>(job: &Job, ser: S) -> Result<S::Ok, S::Error> {
    JobDocument(job).serialize(ser)
}

fn status_str(s: DiffStatus) -> &'static str {
    match s {
        DiffStatus::Unchanged => "unchanged",
        DiffStatus::Added => "added",
        DiffStatus::Removed => "removed",
        DiffStatus::Modified => "modified",
    }
}

fn ser_status<S: Serializer>(s: &DiffStatus, ser: S) -> Result<S::Ok, S::Error> {
    ser.serialize_str(status_str(*s))
}

/// The tree as its changed nodes: `{path, status, expanded, fieldChanges}`.
fn ser_tree<S: Serializer>(tree: &DiffTree, ser: S) -> Result<S::Ok, S::Error> {
    let nodes: Vec<Value> = tree
        .changed()
        .map(|(id, n)| {
            json!({
                "path": tree.path(id),
                "status": status_str(n.status),
                "expanded": n.expanded,
                "fieldChanges": n.field_changes.iter().map(|c| json!({
                    "field": c.field, "before": c.before, "after": c.after,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    nodes.serialize(ser)
}

// ─── Display ────────────────────────────────────────────────────────

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Template => "template",
            Self::Job => "job",
        })
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Template => "template",
            Self::Parameters => "parameters",
            Self::Both => "both",
            Self::Job => "job",
        })
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Template => "template",
            Self::Parameters => "parameters",
            Self::Job => "job",
            Self::RunTime => "run time",
        })
    }
}

impl Change {
    fn mark(&self) -> char {
        match (&self.before, &self.after) {
            (None, Some(_)) => '+',
            (Some(_), None) => '-',
            _ => '~',
        }
    }

    fn value_text(&self) -> String {
        match (&self.before, &self.after) {
            (Some(b), Some(a)) => format!("{b} -> {a}"),
            (None, Some(a)) => a.text.clone(),
            (Some(b), None) => b.text.clone(),
            (None, None) => String::new(),
        }
    }

    /// `{mark} {label}: {values}  [{origin}]`.
    fn line(&self, label: &str) -> String {
        let values = self.value_text();
        if values.is_empty() {
            format!("{} {label}  [{}]", self.mark(), self.origin)
        } else {
            format!("{} {label}: {values}  [{}]", self.mark(), self.origin)
        }
    }

    /// The label in the job-level part of a [`TemplateDiff`] listing.
    fn job_label(&self) -> String {
        let keyed = |prefix: &str| {
            self.path
                .strip_prefix(prefix)
                .and_then(|r| r.strip_suffix(']'))
        };
        if let Some(name) = keyed("parameters[") {
            return match self.field.as_str() {
                "" | "value" => format!("parameter {name}"),
                f => format!("parameter {name}.{f}"),
            };
        }
        if let Some(name) = keyed("parameterDefinitions[") {
            return match self.field.as_str() {
                "" => format!("parameter definition {name}"),
                f => format!("parameter definition {name}.{f}"),
            };
        }
        join_label(&self.path, &self.field)
    }

    /// The label under a step, relative to `step_path` (`steps[S]`), the
    /// path of the step the change is in.
    fn step_label(&self, step_path: &str) -> String {
        let rel = self
            .path
            .strip_prefix(step_path)
            .map(|r| r.trim_start_matches('.'))
            .unwrap_or(&self.path);
        if let Some(name) = rel
            .strip_prefix("parameterSpace.taskParameterDefinitions[")
            .and_then(|r| r.strip_suffix(']'))
        {
            if matches!(self.field.as_str(), "" | "definition") {
                return name.to_string();
            }
        }
        join_label(rel, &self.field)
    }
}

fn join_label(path: &str, field: &str) -> String {
    match (path.is_empty(), field.is_empty()) {
        (true, true) => "<root>".into(),
        (true, false) => field.to_string(),
        (false, true) => path.to_string(),
        (false, false) => format!("{path}.{field}"),
    }
}

/// `~ steps[Render].script.onRun.args: [a] -> [b]  [template]`.
impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.line(&join_label(&self.path, &self.field)))
    }
}

/// `Frame 1-100 × Eye left`.
impl fmt::Display for BoxSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.dims.is_empty() {
            return f.write_str("(no parameters)");
        }
        let dims: Vec<String> = self.dims.iter().map(|(n, v)| format!("{n} {v}")).collect();
        f.write_str(&dims.join(" × "))
    }
}

impl TaskSummary {
    /// Old and new task counts.
    fn counts(&self) -> (u64, u64) {
        match self {
            Self::Unchanged { count } => (*count, *count),
            Self::Product {
                kept,
                added,
                removed,
            } => (
                kept.count.saturating_add(total(removed)),
                kept.count.saturating_add(total(added)),
            ),
            Self::Replaced {
                old_count,
                new_count,
                ..
            } => (*old_count, *new_count),
            Self::Added { count } => (0, *count),
            Self::Removed { count } => (*count, 0),
        }
    }
}

/// `tasks: 100 -> 400`, then one `keep`, `add` or `remove` line per box. A
/// product that adds and removes nothing prints `tasks: N unchanged`.
impl fmt::Display for TaskSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (old, new) = self.counts();
        match self {
            Self::Unchanged { count } => write!(f, "tasks: {count} unchanged"),
            Self::Added { count } => write!(f, "tasks: {count} added"),
            Self::Removed { count } => write!(f, "tasks: {count} removed"),
            Self::Replaced { reason, .. } => {
                write!(f, "tasks: {old} -> {new}, all replaced ({reason})")
            }
            Self::Product {
                kept,
                added,
                removed,
            } if added.is_empty() && removed.is_empty() => {
                write!(f, "tasks: {} unchanged", kept.count)
            }
            Self::Product {
                kept,
                added,
                removed,
            } => {
                write!(f, "tasks: {old} -> {new}")?;
                let boxes = std::iter::once(("keep", kept))
                    .filter(|(_, b)| b.count > 0)
                    .chain(added.iter().map(|b| ("add", b)))
                    .chain(removed.iter().map(|b| ("remove", b)));
                for (verb, b) in boxes {
                    write!(f, "\n  {verb:<6} {:<5} {b}", b.count)?;
                }
                Ok(())
            }
        }
    }
}

impl StepSummary {
    fn mark(&self) -> char {
        match self.status {
            DiffStatus::Added => '+',
            DiffStatus::Removed => '-',
            _ => '~',
        }
    }

    /// One line for `openjd diff --stat`:
    /// `~ Render  tasks 100 -> 400  (+300 -0)`, or
    /// `~ Render  tasks unknown (adaptive chunking)`.
    #[must_use]
    pub fn stat_line(&self) -> String {
        if let Some(why) = &self.tasks_unknown {
            return format!("{} {}  tasks unknown ({why})", self.mark(), self.name);
        }
        let (old, new) = self.tasks.counts();
        let delta = match &self.tasks {
            TaskSummary::Replaced { .. } => "replaced".to_string(),
            TaskSummary::Product { added, removed, .. } => {
                format!("+{} -{}", total(added), total(removed))
            }
            _ => format!("+{} -{}", new.saturating_sub(old), old.saturating_sub(new)),
        };
        format!(
            "{} {}  tasks {old} -> {new}  ({delta})",
            self.mark(),
            self.name
        )
    }

    /// The task summary, or `tasks: unknown (…)` when the count is unknown.
    fn tasks_text(&self) -> String {
        match &self.tasks_unknown {
            Some(why) => format!("tasks: unknown ({why})"),
            None => self.tasks.to_string(),
        }
    }
}

/// `~ step Render`, then the task summary indented by four spaces.
impl fmt::Display for StepSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} step {}", self.mark(), self.name)?;
        for line in self.tasks_text().lines() {
            write!(f, "\n    {line}")?;
        }
        Ok(())
    }
}

/// `1 step changed. Tasks: 100 kept, 300 added, 0 removed.`
impl fmt::Display for DiffStat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = if self.steps_changed == 1 { "" } else { "s" };
        write!(f, "{} step{s} changed", self.steps_changed)?;
        if self.steps_added > 0 {
            write!(f, ", {} added", self.steps_added)?;
        }
        if self.steps_removed > 0 {
            write!(f, ", {} removed", self.steps_removed)?;
        }
        write!(
            f,
            ". Tasks: {} kept, {} added, {} removed",
            self.tasks_kept, self.tasks_added, self.tasks_removed
        )?;
        if self.tasks_replaced > 0 {
            write!(f, ", {} replaced", self.tasks_replaced)?;
        }
        f.write_str(".")
    }
}

/// Job-level changes, then each changed step with its changes and task
/// summary, then the [`DiffStat`] line, then one `note: …` line per note.
/// `no changes` (and the notes) when nothing differs.
impl fmt::Display for TemplateDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.is_changed() {
            f.write_str("no changes")?;
            return self.write_notes(f);
        }
        let mut lines: Vec<String> = Vec::new();
        let job_level: Vec<&Change> = self
            .changes
            .iter()
            .filter(|c| c.step.is_none() && !is_repeated_extensions(c, &self.changes))
            .collect();
        for c in &job_level {
            if let Some(line) = folded_parameter_line(c, &job_level) {
                lines.push(line);
            } else if !is_folded_definition(c, &job_level) && !is_type_shown_by_value(c, &job_level)
            {
                lines.push(c.line(&c.job_label()));
            }
        }
        for s in &self.steps {
            if !self.step_changed(s) {
                continue;
            }
            let step_path = format!("steps[{}]", s.name);
            lines.push(format!("{} step {}", s.mark(), s.name));
            for c in self.step_changes(s, &step_path) {
                let line = if c.path == step_path && c.field.is_empty() {
                    // The step itself, left unexpanded by the depth limit.
                    format!("    ~ (not expanded)  [{}]", c.origin)
                } else {
                    format!("    {}", c.line(&c.step_label(&step_path)))
                };
                lines.push(line);
            }
            for line in s.tasks_text().lines() {
                lines.push(format!("    {line}"));
            }
        }
        lines.push(String::new());
        lines.push(self.stat().to_string());
        f.write_str(&lines.join("\n"))?;
        self.write_notes(f)
    }
}

impl TemplateDiff {
    /// `\nnote: …` for each note.
    fn write_notes(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for n in &self.notes {
            write!(f, "\nnote: {n}")?;
        }
        Ok(())
    }

    /// The changes in one step, task parameters in definition order. An
    /// added or removed step's own change is left out: its header says it.
    /// `step_path` is the step's own path, `steps[S]`, which every path in
    /// the step starts with.
    fn step_changes<'a>(&'a self, s: &'a StepSummary, step_path: &'a str) -> Vec<&'a Change> {
        let mut changes: Vec<&Change> = self
            .changes_in_step(&s.name)
            .filter(|c| !(c.path == step_path && c.field.is_empty() && c.mark() != '~'))
            .collect();
        let defs = format!("{step_path}.parameterSpace.taskParameterDefinitions[");
        let order: Vec<&String> = [self.new_step(s), self.old_step(s)]
            .into_iter()
            .flatten()
            .filter_map(|step| step.parameter_space.as_ref())
            .flat_map(|space| space.task_parameter_definitions.keys())
            .collect();
        let rank = |c: &Change| {
            c.path
                .strip_prefix(&defs)
                .and_then(|r| r.split(']').next())
                .and_then(|name| order.iter().position(|n| n.as_str() == name))
        };
        // The definitions are one block in path order; order it by rank.
        if let Some(start) = changes.iter().position(|c| rank(c).is_some()) {
            let end = start
                + changes[start..]
                    .iter()
                    .take_while(|c| rank(c).is_some())
                    .count();
            changes[start..end].sort_by_key(|c| rank(c));
        }
        changes
    }

    fn new_step(&self, s: &StepSummary) -> Option<&Step> {
        s.new_index.and_then(|i| self.new_job.steps.get(i))
    }

    fn old_step(&self, s: &StepSummary) -> Option<&Step> {
        s.old_index.and_then(|i| self.old_job.steps.get(i))
    }
}

/// The job layer's `extensions` change when the template layer reports one
/// too. The two say the same thing, so `Display` prints the template one.
fn is_repeated_extensions(c: &Change, changes: &[Change]) -> bool {
    let is_extensions =
        |x: &Change, layer: Layer| x.layer == layer && x.path.is_empty() && x.field == "extensions";
    is_extensions(c, Layer::Job) && changes.iter().any(|x| is_extensions(x, Layer::Template))
}

/// The template-layer definition change that a whole-parameter change folds
/// in: the same parameter, added or removed on the same side.
fn definition_for<'a>(c: &Change, job_level: &[&'a Change]) -> Option<&'a Change> {
    let name = c
        .path
        .strip_prefix("parameters[")
        .and_then(|r| r.strip_suffix(']'))?;
    if !c.field.is_empty() || c.mark() == '~' {
        return None;
    }
    let path = format!("parameterDefinitions[{name}]");
    job_level
        .iter()
        .copied()
        .find(|d| d.path == path && d.field.is_empty() && d.mark() == c.mark())
}

/// `- parameter Gone: g, defined as {default: g, type: STRING}  [template]`
/// for an added or removed parameter whose definition was added or removed.
fn folded_parameter_line(c: &Change, job_level: &[&Change]) -> Option<String> {
    let def = definition_for(c, job_level)?;
    let def_text = def.after.as_ref().or(def.before.as_ref())?;
    let label = c.job_label();
    Some(format!(
        "{} {label}: {}, defined as {def_text}  [{}]",
        c.mark(),
        c.value_text(),
        c.origin
    ))
}

/// Whether `d` is a definition change printed as part of its parameter's
/// line.
fn is_folded_definition(d: &Change, job_level: &[&Change]) -> bool {
    d.layer == Layer::Template
        && d.path.starts_with("parameterDefinitions[")
        && job_level
            .iter()
            .any(|c| definition_for(c, job_level).is_some_and(|x| std::ptr::eq(x, d)))
}

/// Whether `c` is a parameter's job-layer `type` change that its value line
/// already shows, as `"5" (STRING) -> 5 (INT)`.
fn is_type_shown_by_value(c: &Change, job_level: &[&Change]) -> bool {
    c.layer == Layer::Job
        && c.field == "type"
        && c.path.starts_with("parameters[")
        && job_level.iter().any(|v| {
            v.layer == Layer::Job
                && v.path == c.path
                && v.field == "value"
                && v.before.is_some()
                && v.after.is_some()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use openjd_expr::RangeExpr;

    #[test]
    fn run_time_symbols_need_a_name_boundary() {
        let cases = [
            ("Task.Param.F", true),
            ("x + Task.Param.F", true),
            ("(Session.WorkingDirectory)", true),
            ("subTask.x", false),
            ("a.Task.x", false),
            ("my_Session.x", false),
            ("Tasks.x", false),
            ("Param.P", false),
        ];
        for (expr, want) in cases {
            assert_eq!(names_run_time_symbol(expr), want, "{expr}");
        }
        let s = |t: &str| Value::String(t.into());
        assert!(refs_run_time(&s("a {{ Task.Param.F }} b"), false));
        assert!(!refs_run_time(&s("Task.log {{Param.P}}"), false));
        assert!(!refs_run_time(&s("{{Param.P}} }} Task.x {{"), false));
        assert!(refs_run_time(&s("Task.Param.F + 1"), true));
        assert!(!refs_run_time(&s("{{subTask.x}}"), false));
    }

    #[test]
    fn int_runs_of_an_increasing_list_is_the_canonical_range_expression() {
        // A small LCG, so the cases are the same on every run.
        let mut seed: u64 = 7;
        let mut next = |n: u64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            (seed >> 33) % n
        };
        for _ in 0..5000 {
            let len = next(12) as usize + 1;
            let mut v = vec![next(5) as i64 - 2];
            for _ in 1..len {
                let gap = [1, 1, 2, 2, 3, 5][next(6) as usize];
                v.push(v[v.len() - 1] + gap);
            }
            let canonical = RangeExpr::from_values(v.clone()).unwrap().to_string();
            assert_eq!(int_runs(&v), canonical, "{v:?}");
        }
        // A list that does not increase keeps its order.
        assert_eq!(int_runs(&[5, 1, 2, 3, 9, 7]), "5,1-3,9,7");
        // A repeated value is not a run: its step is 0.
        assert_eq!(int_runs(&[3, 3, 3]), "3,3,3");
    }

    #[test]
    fn a_symbol_has_exactly_name_type_and_value() {
        let yes = json!({"name": "Param.A", "type": "string", "value": "a"});
        assert!(is_symbol(&yes));
        for no in [
            json!({"name": "a", "type": "t", "other": 1}),
            json!({"name": "a", "other": 1, "value": 1}),
            json!({"other": "a", "type": "t", "value": 1}),
            json!({"name": "a", "type": "t", "value": 1, "x": 2}),
            json!({"name": "a", "min": 2, "max": 4}),
        ] {
            assert!(!is_symbol(&no), "{no}");
        }
    }

    #[test]
    fn around_difference_keeps_short_differences_and_shows_late_ones() {
        let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(around_difference("ab", "ac"), pair("ab", "ac"));
        // A difference at index 118 is the last one a plain cut keeps.
        let (x, y) = (
            "y".repeat(118) + "a" + &"z".repeat(10),
            "y".repeat(118) + "b",
        );
        assert_eq!(around_difference(&x, &y), (truncate(x.clone()), y.clone()));
        // At index 119 the cut would hide it, so both start near it.
        let (x, y) = ("y".repeat(119) + "a", "y".repeat(119) + "b");
        let want = |c: char| format!("…{}{c}", "y".repeat(40));
        assert_eq!(around_difference(&x, &y), (want('a'), want('b')));
    }

    #[test]
    fn whole_floats_drop_their_fraction_up_to_two_to_the_53() {
        let text = |f: f64| number_text(&serde_json::Number::from_f64(f).unwrap());
        assert_eq!(text(4.0), "4");
        assert_eq!(text(-4.0), "-4");
        assert_eq!(text(4.5), "4.5");
        assert_eq!(text(1e15), "1000000000000000");
        assert_eq!(text(MAX_EXACT_INTEGER), "9007199254740992");
        assert_eq!(text(-MAX_EXACT_INTEGER), "-9007199254740992");
        assert_eq!(text(MAX_EXACT_INTEGER + 2.0), "9007199254740994.0");
        assert_eq!(text(-(MAX_EXACT_INTEGER + 2.0)), "-9007199254740994.0");
        assert_eq!(number_text(&4.into()), "4");
    }

    #[test]
    fn a_list_of_single_key_objects_is_not_a_dependency_list() {
        assert_eq!(render(&json!([{"ListInt": [1]}])), "[{ListInt: [1]}]");
        assert_eq!(render(&json!([{"dependsOn": "A"}])), "[A]");
        // Only a one-key object whose value is a string is a dependency.
        assert_eq!(
            render(&json!([{"dependsOn": "A", "x": 1}])),
            "[{dependsOn: A, x: 1}]"
        );
        assert_eq!(render(&json!([{"dependsOn": 1}])), "[{dependsOn: 1}]");
    }

    #[test]
    fn a_cut_list_or_object_keeps_its_head() {
        let (x, y) = (
            format!("[{}a]", "y".repeat(150)),
            format!("[{}b]", "y".repeat(150)),
        );
        let want = |c: char| format!("[{}… …{}{c}]", "y".repeat(19), "y".repeat(40));
        assert_eq!(around_difference(&x, &y), (want('a'), want('b')));
        let (x, y) = (
            format!("{}a", "y".repeat(150)),
            format!("{}b", "y".repeat(150)),
        );
        let want = |c: char| format!("…{}{c}", "y".repeat(40));
        assert_eq!(around_difference(&x, &y), (want('a'), want('b')));
        // An object keeps its head too: the `{` branch of the test.
        let (x, y) = (
            format!("{{{}a}}", "y".repeat(150)),
            format!("{{{}b}}", "y".repeat(150)),
        );
        let want = |c: char| format!("{{{}… …{}{c}}}", "y".repeat(19), "y".repeat(40));
        assert_eq!(around_difference(&x, &y), (want('a'), want('b')));
    }

    #[test]
    fn a_cut_ends_its_head_and_starts_its_context_between_words() {
        let side = |v: &str| {
            format!(
                "{{name: J, parameters: {{Out: /mnt/{}/passes/{v}/out}}}}",
                "a".repeat(100)
            )
        };
        let (b, a) = around_difference(&side("v1"), &side("v2"));
        // The head ends after `J,`, not inside `parameters`; the context
        // starts at the `/` before `passes`, not inside the `a`s.
        assert_eq!(b, "{name: J,… …/passes/v1/out}}");
        assert_eq!(a, "{name: J,… …/passes/v2/out}}");
        // With no `/` or space in the context, it starts 40 characters
        // before the difference.
        let w = |c: char| format!("{}{c}", "x".repeat(150));
        assert_eq!(
            around_difference(&w('1'), &w('2')).0,
            format!("…{}1", "x".repeat(40))
        );
        // A space starts the context after it.
        let s = |c: char| format!("{} {}{c}", "x".repeat(100), "y".repeat(30));
        assert_eq!(
            around_difference(&s('1'), &s('2')).0,
            format!("…{}1", "y".repeat(30))
        );
    }

    #[test]
    fn strings_escape_control_characters_and_quote_when_ambiguous() {
        assert_eq!(string_text("a\nb"), "a\\nb");
        assert_eq!(string_text("a\r\nb\tc\u{1b}"), "a\\r\\nb\\tc\\u{1b}");
        assert_eq!(string_text(""), "\"\"");
        assert_eq!(string_text(" "), "\" \"");
        assert_eq!(string_text("a "), "\"a \"");
        assert_eq!(string_text("a\n"), "\"a\\n\"");
        assert_eq!(string_text("a -> b"), "\"a -> b\"");
        assert_eq!(string_text("say \"hi\" -> x"), "\"say \\\"hi\\\" -> x\"");
        assert_eq!(string_text("a->b c"), "a->b c");
        assert_eq!(render(&json!(["a\nb"])), "[a\\nb]");
        assert_eq!(render(&json!(["a\r\nb"])), "[a\\r\\nb]");
    }
}
