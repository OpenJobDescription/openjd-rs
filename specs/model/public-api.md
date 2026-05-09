# Public API

[README](README.md) · Public API

This document is the authoritative reference for the `openjd-model` crate's public
API. All public types, functions, and re-exports are listed here with their
signatures, organized by where they are visible from.

The crate implements the [2023-09 Template Schemas][spec-2023-09] specification,
a data-interchange format for describing renderable jobs in a
worker-host-agnostic way. An `openjd-model` caller's typical flow is:

1. Read a YAML or JSON template document.
2. Call [`decode_job_template`] or [`decode_environment_template`] to parse
   and validate it against the schema + declared extensions.
3. Build a [`ModelProfile`] and a [`JobParameterInputValues`] from user input.
4. Call [`preprocess_job_parameters`] to coerce inputs against the
   merged parameter definitions.
5. Call [`create_job`] with the preprocessed values and the
   [`ValidationContext`]. The result is an owned [`job::Job`] ready for
   the session runtime.

Beyond that core flow, the crate exposes low-level building blocks —
symbol-table construction, step dependency graphs, lazy parameter-space
iteration, capability lookup, and the full template/job type hierarchy —
so applications (submitters, validators, schedulers) can compose exactly
what they need without re-implementing the schema.

[spec-2023-09]: https://github.com/OpenJobDescription/openjd-specifications/wiki/2023-09-Template-Schemas

## Module Structure

```
openjd_model              — crate root re-exports, the main public surface
├── capabilities          — standard capability names + name validators
├── error                 — ModelError + structured validation errors
├── format_string         — re-exported from openjd_expr
├── job                   — instantiated (resolved) job types
│   ├── create_job        — pipeline functions + MergedParameterDefinition
│   ├── step_param_space  — StepParameterSpaceIterator
│   └── step_dependency_graph — StepDependencyGraph + friends
├── parse                 — re-exported from the (private) template module
├── symbol_table          — re-exported from openjd_expr
└── types                 — enums, profile, validation context, parameter values
```

The `template::*` types (unresolved/parsed template structs) are *not* re-exported
at the crate root beyond [`JobTemplate`], [`EnvironmentTemplate`],
[`JobParameterDefinition`], and [`TaskParameterDefinition`]. The `template`
module itself is crate-private; consequently, `template::StepTemplate`,
`template::Environment`, and the other nested template types are not
nameable in external code — callers can **read** `JobTemplate.steps[i].name`,
`JobTemplate.job_environments.unwrap()[i].variables`, etc. through the
field accessors on those types, but cannot write function signatures or
`impl` blocks that take a `&StepTemplate`. This is a deliberate
decision: the resolved `job::*` types are what most consumers need
(format strings evaluated, parameters bound), and restricting the
template-side surface keeps the template/job boundary explicit. When a
legitimate use case arises for direct access to `template::StepTemplate`
or `template::Environment`, additional re-exports can be added
SemVer-compatibly.

## Entry Points at the Crate Root

### Parsing + Validation

```rust
pub fn decode_job_template(
    template: serde_json::Value,
    supported_extensions: Option<&[&str]>,
    caller_limits: &CallerLimits,
) -> Result<JobTemplate, ModelError>;

pub fn decode_environment_template(
    template: serde_json::Value,
    supported_extensions: Option<&[&str]>,
) -> Result<EnvironmentTemplate, ModelError>;

pub fn decode_template(
    template: serde_json::Value,
    supported_extensions: Option<&[&str]>,
    caller_limits: &CallerLimits,
) -> Result<DecodedTemplate, ModelError>;
```

Each function takes a generic JSON value (typically produced from YAML by
[`parse::document_string_to_object`]) and returns the parsed template struct
on success. `supported_extensions` is an allowlist of extension names the
application is willing to honor — template extensions not in this list are
rejected. `caller_limits` constrains document size beyond any spec-defined
limits; it's an optional per-deployment policy hook, not something defined
by the spec.

[`decode_template`] auto-detects the template kind from
`specificationVersion` and dispatches to the matching decoder.

### Job Instantiation

```rust
pub fn create_job(
    job_template: &JobTemplate,
    job_parameter_values: &JobParameterValues,
    ctx: &ValidationContext,
) -> Result<job::Job, ModelError>;

pub fn preprocess_job_parameters(
    job_template: &JobTemplate,
    input_values: &JobParameterInputValues,
    environment_templates: &[EnvironmentTemplate],
    path_options: &PathParameterOptions<'_>,
) -> Result<JobParameterValues, ModelError>;

pub fn merge_job_parameter_definitions(
    job_template: &JobTemplate,
    environment_templates: &[EnvironmentTemplate],
) -> Result<Vec<MergedParameterDefinition>, ModelError>;

pub fn build_symbol_table(
    params: &JobParameterValues,
) -> Result<SymbolTable, ModelError>;

pub fn evaluate_let_bindings(
    bindings: &[String],
    base: &SymbolTable,
    library: Option<&openjd_expr::FunctionLibrary>,
    path_format: openjd_expr::path_mapping::PathFormat,
) -> Result<SymbolTable, ModelError>;

pub fn convert_environment(env: &template::Environment) -> job::Environment;
```

[`create_job`] is the high-level entry point: it resolves the job name
(template scope), instantiates every step, runs the final task-count
limit from [`CallerLimits`], and returns the complete [`job::Job`]. The
`ctx` it takes is a full [`ValidationContext`] — i.e. revision +
extensions + caller limits — and callers commonly get one from
[`JobTemplate::default_validation_context`].

[`preprocess_job_parameters`] implements the parameter coercion and
default-value pipeline from spec §2: type coercion, constraint checks,
PATH resolution relative to the template directory and the current
working directory, merging of env-template parameters per §1.2.1.

[`merge_job_parameter_definitions`] exposes just the merge step —
useful for UIs that need to display the merged constraints before
collecting user input.

[`build_symbol_table`] and [`evaluate_let_bindings`] let callers
assemble symbol tables outside the `create_job` flow — for example,
to resolve a single format string during template editing.

[`convert_environment`] maps a template-time [`template::Environment`]
to its resolved [`job::Environment`] counterpart without running the
full `create_job` pipeline. This is used by the session runtime when
it needs a resolved environment shape for an environment template
that's being entered directly without a parent job.

## Template Types (Unresolved)

These types represent a *parsed, validated* template where format strings
are still unevaluated. They're the output of `decode_*_template` and the
input to [`create_job`] / [`preprocess_job_parameters`].

```rust
pub struct JobTemplate {
    pub specification_version: String,
    pub schema: Option<String>,
    pub extensions: Option<Vec<ExtensionName>>,
    pub name: FormatString,
    pub description: Option<Description>,
    pub parameter_definitions: Option<Vec<JobParameterDefinition>>,
    pub job_environments: Option<Vec<template::Environment>>,
    pub steps: Vec<template::StepTemplate>,
}
```

Methods on `JobTemplate`:

```rust
impl JobTemplate {
    pub fn name(&self) -> &FormatString;
    pub fn description(&self) -> Option<&str>;
    pub fn parameter_definitions_list(&self) -> &[JobParameterDefinition];

    /// Build a ModelProfile from the template's declared
    /// specificationVersion + extensions. Entries in `extensions` that
    /// don't parse as a known `ModelExtension` are silently skipped.
    pub fn profile(&self) -> ModelProfile;

    /// Convenience: `ValidationContext::from_profile(self.profile())`.
    /// The context callers want when they just want to `create_job`
    /// whatever the template says.
    pub fn default_validation_context(&self) -> ValidationContext;
}
```

```rust
pub struct EnvironmentTemplate {
    pub specification_version: String,
    pub extensions: Option<Vec<ExtensionName>>,
    pub parameter_definitions: Option<Vec<JobParameterDefinition>>,
    pub environment: template::Environment,
}

impl EnvironmentTemplate {
    pub fn environment(&self) -> &template::Environment;
}
```

`ExtensionName` and `Description` are constrained string newtypes
defined in the (crate-private) `template::constrained_strings` module.
They reach the public surface only as field types on these structs;
callers observe them as `AsRef<str>`-style wrappers.

### Job Parameter Definitions

```rust
/// Discriminated enum over every parameter type the spec allows. The
/// variants in CamelCase upper case reflect the spec's
/// UPPER_SNAKE_CASE `type` discriminator verbatim.
pub enum JobParameterDefinition {
    STRING(JobStringParameterDefinition),
    INT(JobIntParameterDefinition),
    FLOAT(JobFloatParameterDefinition),
    PATH(JobPathParameterDefinition),
    BOOL(JobBoolParameterDefinition),           // EXPR extension
    RANGE_EXPR(JobRangeExprParameterDefinition), // EXPR extension
    LIST_STRING(JobListStringParameterDefinition), // EXPR extension
    LIST_PATH(JobListPathParameterDefinition),   // EXPR extension
    LIST_INT(JobListIntParameterDefinition),     // EXPR extension
    LIST_FLOAT(JobListFloatParameterDefinition), // EXPR extension
    LIST_BOOL(JobListBoolParameterDefinition),   // EXPR extension
    LIST_LIST_INT(JobListListIntParameterDefinition), // EXPR extension
}
```

The inner struct types are **not** re-exported from the crate root
(they live inside the private `template` module). Callers access
parameter fields through the API below:

```rust
impl JobParameterDefinition {
    pub fn job_param_type(&self) -> JobParameterType;
    pub fn name(&self) -> &str;
    pub fn description(&self) -> Option<&str>;
    pub fn type_name(&self) -> &str;
    pub fn path_properties(&self) -> (Option<ObjectType>, Option<DataFlow>);
    pub fn default_value(&self) -> Option<String>;

    // Numeric constraints — None if not set on this definition or not
    // applicable to the parameter type.
    pub fn min_value_i64(&self) -> Option<i64>;
    pub fn max_value_i64(&self) -> Option<i64>;
    pub fn min_value_f64(&self) -> Option<f64>;
    pub fn max_value_f64(&self) -> Option<f64>;

    // Length constraints (STRING / LIST_*).
    pub fn min_length(&self) -> Option<usize>;
    pub fn max_length(&self) -> Option<usize>;

    // Allowed-values (STRING / INT / FLOAT).
    pub fn allowed_values_i64(&self) -> Option<Vec<i64>>;
    pub fn allowed_values_f64(&self) -> Option<Vec<f64>>;
    pub fn allowed_values_strings(&self) -> Option<Vec<String>>;

    /// Validate an already-coerced ExprValue against this definition's
    /// constraints. Used by `preprocess_job_parameters`.
    pub fn check_constraints(&self, value: &openjd_expr::ExprValue) -> Result<(), String>;
}
```

### Task Parameter Definitions

```rust
/// Discriminated enum on `type`. Again, variant names mirror the spec's
/// UPPER_SNAKE_CASE strings verbatim.
pub enum TaskParameterDefinition {
    INT(IntTaskParameterDefinition),
    FLOAT(FloatTaskParameterDefinition),
    STRING(StringTaskParameterDefinition),
    PATH(PathTaskParameterDefinition),
    CHUNK_INT(ChunkIntTaskParameterDefinition), // TASK_CHUNKING extension
}

impl TaskParameterDefinition {
    pub fn task_param_type(&self) -> TaskParameterType;
    pub fn name(&self) -> &str;
}
```

### Range + Parameter-Space Shape Types

Available via the `template::*` path when needed (e.g. `openjd_model::template::IntRange`).
These appear as fields on the `TaskParameterDefinition::*` variants:

```rust
pub enum IntRange {
    List(Vec<FlexInt>),       // a concrete list of integers
    Expression(FormatString), // a range expression string to evaluate
}

pub enum StringRange {
    List(Vec<FormatString>),
    Expression(FormatString),
}

pub struct FloatRange(pub Vec<FloatRangeItem>);
pub struct FloatRangeItem(pub FormatString);

pub enum IntOrFormatString {
    Int(FlexInt),
    FormatString(FormatString),
}

pub struct RangeConstraint {
    pub min: Option<IntOrFormatString>,
    pub max: Option<IntOrFormatString>,
}

pub struct StepParameterSpaceDefinition {
    pub task_parameter_definitions: Vec<TaskParameterDefinition>,
    pub combination: Option<String>,
}
```

## Instantiated Job Types

These types, re-exported via the `job::` path, are the output of
[`create_job`]. They have no `FormatString` fields at the template-scope
level — all template-scope strings have been resolved to concrete values.
Session- and task-scope strings (in `script.actions`, `variables`,
embedded file contents, etc.) remain as [`FormatString`] for the session
runtime to resolve when worker state is available.

```rust
pub struct job::Job {
    pub name: String,
    pub description: Option<String>,
    pub extensions: Option<Vec<ModelExtension>>,
    pub parameters: IndexMap<String, JobParameter>,
    pub steps: Vec<Step>,
    pub job_environments: Option<Vec<Environment>>,
}

pub struct job::JobParameter {
    pub name: String,
    pub param_type: JobParameterType,
    pub value: openjd_expr::ExprValue,
}

pub struct job::Step {
    pub name: String,
    pub description: Option<String>,
    pub script: StepScript,
    pub step_environments: Option<Vec<Environment>>,
    pub parameter_space: Option<StepParameterSpace>,
    pub host_requirements: Option<HostRequirements>,
    pub dependencies: Option<Vec<StepDependency>>,
    /// Complete symbol table at step scope in JSON transport format.
    /// Contains Param.*, RawParam.*, Job.Name, Step.Name, and step-level
    /// let bindings. The session deserializes this with `PathFormat::host()`
    /// and layers Session.* and Task.* values on top at runtime.
    pub resolved_symtab: Option<SerializedSymbolTable>,
}

pub struct job::StepScript {
    pub let_bindings: Option<Vec<String>>,
    pub actions: StepActions,
    pub embedded_files: Option<Vec<EmbeddedFile>>,
}

pub struct job::StepActions {
    pub on_run: Action,
}

pub struct job::Action {
    pub command: FormatString,
    pub args: Option<Vec<FormatString>>,
    pub timeout: Option<FormatString>,
    pub cancelation: Option<CancelationMode>,
    /// RFC 0008 — if `Some(true)`, this action runs directly on the host
    /// and is not intercepted by any active wrap hook. Requires the
    /// `WRAP_ACTIONS` extension at template-validation time. Serialized
    /// only when non-`None`.
    pub run_on_host: Option<bool>,
}

pub struct job::Environment {
    pub name: String,
    pub description: Option<String>,
    pub script: Option<EnvironmentScript>,
    pub variables: Option<HashMap<String, FormatString>>,
    /// Filtered symbol table with only the symbols this environment references.
    pub resolved_symtab: Option<SerializedSymbolTable>,
}

pub struct job::EnvironmentScript {
    pub let_bindings: Option<Vec<String>>,
    pub actions: EnvironmentActions,
    pub embedded_files: Option<Vec<EmbeddedFile>>,
}

pub struct job::EnvironmentActions {
    pub on_enter: Option<Action>,
    /// RFC 0008 — wraps inner environments' `onEnter` actions. Requires
    /// the `WRAP_ACTIONS` extension at template-validation time.
    pub on_wrap_enter: Option<Action>,
    /// RFC 0008 — wraps tasks' `onRun` actions. Requires the
    /// `WRAP_ACTIONS` extension at template-validation time.
    pub on_wrap_task_run: Option<Action>,
    /// RFC 0008 — wraps inner environments' `onExit` actions. Requires
    /// the `WRAP_ACTIONS` extension at template-validation time.
    pub on_wrap_exit: Option<Action>,
    pub on_exit: Option<Action>,
}

pub struct job::EmbeddedFile {
    pub name: String,
    pub file_type: FileType,
    pub filename: Option<FormatString>,
    pub data: Option<FormatString>,
    pub runnable: Option<bool>,
    pub end_of_line: Option<EndOfLine>,
}

pub enum job::CancelationMode {
    Terminate,
    NotifyThenTerminate { notify_period_in_seconds: Option<FormatString> },
}
```

### Parameter Space (Resolved)

```rust
pub struct job::StepParameterSpace {
    pub task_parameter_definitions: IndexMap<String, TaskParameter>,
    pub combination: Option<String>,
}

pub enum job::TaskParameter {
    Int { range: TaskParamRange<i64>, chunks: Option<ResolvedChunks> },
    Float { range: Vec<f64> },
    String { range: Vec<String> },
    Path { range: Vec<String> },
    ChunkInt { range: TaskParamRange<i64>, chunks: ResolvedChunks },
}

pub enum job::TaskParamRange<T> {
    List(Vec<T>),
    RangeExpr(RangeExpr),
}

pub struct job::ResolvedChunks {
    pub default_task_count: usize,
    pub target_runtime_seconds: Option<usize>,
    pub range_constraint: RangeConstraint,
}
```

### Host Requirements (Resolved)

Host requirements in the resolved form have their `min`/`max` format
strings evaluated to concrete `f64`s:

```rust
pub struct job::HostRequirements {
    pub amounts: Option<Vec<AmountRequirement>>,
    pub attributes: Option<Vec<AttributeRequirement>>,
}

pub struct job::AmountRequirement {
    pub name: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

pub struct job::AttributeRequirement {
    pub name: String,
    pub any_of: Option<Vec<String>>,
    pub all_of: Option<Vec<String>>,
}

pub struct job::StepDependency {
    pub depends_on: String,
}
```

## Shared Types

Everything in this section is re-exported at the crate root from the
`types` module.

### Profile + Validation Context

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SpecificationRevision {
    V2023_09,
}

#[non_exhaustive]
pub enum ModelExtension {
    TaskChunking,      // RFC 0001 — "TASK_CHUNKING"
    RedactedEnvVars,   // RFC 0003 — "REDACTED_ENV_VARS"
    FeatureBundle1,    // RFC 0004 — "FEATURE_BUNDLE_1"
    Expr,              // RFC 0005 — "EXPR"
    WrapActions,       // RFC 0008 — "WRAP_ACTIONS"
}

impl ModelExtension {
    pub const ALL: &'static [ModelExtension];
    pub fn as_str(&self) -> &'static str;
}

pub type Extensions = HashSet<ModelExtension>;

/// Model-side profile: spec revision + enabled extensions.
///
/// `ModelProfile` is the bridge to `openjd-expr`: call
/// `to_expr_profile(host_context)` to get an
/// `openjd_expr::ExprProfile` whose `FunctionLibrary::for_profile` will
/// match this template's declared rules.
#[derive(Debug, Clone)]
pub struct ModelProfile { /* private fields */ }

impl ModelProfile {
    pub fn new(revision: SpecificationRevision) -> Self;
    #[must_use] pub fn with_extensions(self, extensions: Extensions) -> Self;
    pub fn revision(&self) -> SpecificationRevision;
    pub fn extensions(&self) -> &Extensions;
    pub fn has_extension(&self, ext: ModelExtension) -> bool;

    /// Derive an `ExprProfile` for the given host context. Pass
    /// `HostContext::None` for template-scope work, `HostContext::Unresolved`
    /// for template-validation type checking, and
    /// `HostContext::WithRules(..)` for runtime evaluation with real path
    /// mapping.
    pub fn to_expr_profile(
        &self,
        host_context: openjd_expr::HostContext,
    ) -> openjd_expr::ExprProfile;
}

impl Default for ModelProfile { /* ... */ }
```

```rust
/// Caller-supplied limits beyond what the spec defines.
///
/// These tighten the spec-defined limits but can never loosen them.
/// Every field is optional; `None` means "no additional restriction."
#[derive(Debug, Clone, Default)]
pub struct CallerLimits {
    pub max_step_count: Option<usize>,
    pub max_env_count: Option<usize>,
    pub max_task_count: Option<u64>,
    pub max_step_script_size: Option<usize>,
    pub max_environment_size: Option<usize>,
    pub max_template_size: Option<usize>,
}

/// The thing every validation and instantiation function takes — a
/// `ModelProfile` plus `CallerLimits`. Most callers construct one via
/// `JobTemplate::default_validation_context` or
/// `ValidationContext::new(revision)`.
#[derive(Debug, Clone)]
pub struct ValidationContext {
    pub profile: ModelProfile,
    pub caller_limits: CallerLimits,
}

impl ValidationContext {
    pub fn new(revision: SpecificationRevision) -> Self;
    pub fn with_extensions(revision: SpecificationRevision, extensions: Extensions) -> Self;
    pub fn from_profile(profile: ModelProfile) -> Self;
    #[must_use] pub fn with_caller_limits(self, caller_limits: CallerLimits) -> Self;
}
```

### Parameter Types

```rust
/// Job parameter type — the `type` field on a job parameter definition,
/// after `UPPER_SNAKE_CASE` → Rust-identifier conversion.
#[non_exhaustive]
pub enum JobParameterType {
    String, Int, Float, Path, Bool, RangeExpr,
    ListString, ListInt, ListFloat, ListPath, ListBool, ListListInt,
}

impl JobParameterType {
    pub fn from_spec_str(s: &str) -> Option<Self>;  // case-insensitive
    pub fn as_spec_str(&self) -> &'static str;
    pub fn expr_type(&self) -> openjd_expr::ExprType;
}

pub enum TaskParameterType {
    Int, Float, String, Path, ChunkInt,
}

impl TaskParameterType {
    pub fn from_spec_str(s: &str) -> Option<Self>;
    pub fn as_spec_str(&self) -> &'static str;
    pub fn expr_type(&self) -> openjd_expr::ExprType;
}
```

### Parameter Values

```rust
pub struct JobParameterValue {
    pub param_type: JobParameterType,
    pub value: openjd_expr::ExprValue,
}

pub struct TaskParameterValue {
    pub param_type: TaskParameterType,
    pub value: openjd_expr::ExprValue,
}

/// Input parameter values from the user. CLI callers typically pass
/// every value as `ExprValue::String(..)` and let
/// `preprocess_job_parameters` coerce to the target type; library
/// callers may pass typed values directly.
pub type JobParameterInputValues = HashMap<String, openjd_expr::ExprValue>;

/// Processed job parameter values (name → typed value).
pub type JobParameterValues = HashMap<String, JobParameterValue>;

/// A single task's parameter values, in insertion order.
pub type TaskParameterSet = IndexMap<String, TaskParameterValue>;
```

### Spec-String Enums

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FileType {
    Text,
}

#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EndOfLine {
    Lf, Crlf, Auto,
}

#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ObjectType {
    File, Directory,
}

#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DataFlow {
    None, In, Out, Inout,
}

pub enum TemplateSpecificationVersion {
    JobTemplate2023_09,      // "jobtemplate-2023-09"
    Environment2023_09,      // "environment-2023-09"
}

impl TemplateSpecificationVersion {
    pub fn as_str(&self) -> &'static str;
    pub fn is_job_template(&self) -> bool;
    pub fn is_environment_template(&self) -> bool;
    pub fn revision(&self) -> SpecificationRevision;
}

impl std::str::FromStr for TemplateSpecificationVersion { /* ... */ }
```

All implement `Display`. None (except `SpecificationRevision` and
`JobParameterType`) are currently `#[non_exhaustive]`; see the
future-revision-readiness report for the rationale and the list of
enums that should be marked before stable release.

## Parsing Module

```rust
pub mod parse {
    /// Document format.
    pub enum DocumentType { Json, Yaml }

    /// Maximum structural nesting depth for template documents.
    /// Matches serde_json's hardcoded recursion limit so YAML and JSON
    /// behave identically on deeply nested input.
    pub const MAX_DOCUMENT_DEPTH: usize = 128;

    /// Parse a document string into a generic JSON value, enforcing the
    /// caller-configured maximum document size (if any) before parsing.
    pub fn document_string_to_object(
        document: &str,
        doc_type: DocumentType,
        caller_limits: &CallerLimits,
    ) -> Result<serde_json::Value, ModelError>;

    // decode_* functions re-exported at the crate root (see above).

    /// Result of `decode_template`.
    pub enum DecodedTemplate {
        Job(JobTemplate),
        Environment(EnvironmentTemplate),
    }
}
```

## Error Types

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ModelError {
    /// Structural deserialization failure (bad YAML/JSON, missing
    /// fields, wrong types).
    DecodeValidation(String),

    /// Semantic validation failure — a template parsed but violates
    /// spec rules. The embedded `ValidationErrors` carries a per-field
    /// path for each problem.
    ModelValidation(ValidationErrors),

    /// Format string interpolation error with optional source position.
    FormatStringError {
        message: String,
        input: Option<String>,
        start: Option<usize>,
        end: Option<usize>,
    },

    /// Expression evaluation or symbol table error. Preserves the
    /// full `ExpressionError` with its kind and source context.
    Expression(openjd_expr::ExpressionError),

    /// Incompatible env-template parameter merges (§1.2.1).
    Compatibility(String),

    /// The `specificationVersion` field named a revision this library
    /// does not support.
    UnsupportedSchema(String),
}
```

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathElement {
    Field(String),
    Index(usize),
}

#[derive(Debug, Clone)]
pub struct ValidationError {
    pub path: Vec<PathElement>,
    pub message: String,
    pub detail: Option<ErrorDetail>,
}

#[derive(Debug, Clone)]
pub struct ErrorDetail {
    pub summary: String,
    pub spans: Vec<DiagnosticSpan>,
}

#[derive(Debug, Clone)]
pub struct DiagnosticSpan {
    pub summary: String,
    pub source: String,
    pub start: usize,
    pub end: usize,
    pub caret: usize,
}

#[derive(Debug, Default)]
pub struct ValidationErrors {
    pub errors: Vec<ValidationError>,
    // model_name is private (set by into_result)
}

impl ValidationErrors {
    pub fn single(msg: impl Into<String>) -> Self;
    pub fn add(&mut self, path: &[PathElement], msg: impl Into<String>);
    pub fn add_with_detail(
        &mut self,
        path: &[PathElement],
        msg: impl Into<String>,
        detail: ErrorDetail,
    );
    pub fn is_empty(&self) -> bool;
    pub fn len(&self) -> usize;
    pub fn into_result(self, model_name: &str) -> Result<(), ModelError>;
    pub fn format(&self, model_name: &str) -> String;
}

impl std::fmt::Display for ValidationErrors { /* ... */ }

// Path-construction helpers
pub fn path_field(base: &[PathElement], field: &str) -> Vec<PathElement>;
pub fn path_index(base: &[PathElement], index: usize) -> Vec<PathElement>;
```

Errors format to match Python's Pydantic output: `steps[0] -> script ->
actions -> onRun -> command:\n\t<message>`. The format is part of the
stability contract — tools that consume CLI output depend on it
across both the Python and Rust reference implementations.

## Job Creation Module

### `PathParameterOptions`

Used by [`preprocess_job_parameters`] to control how PATH parameters
are anchored and what sources are allowed.

```rust
pub struct PathParameterOptions<'a> {
    /// Directory containing the job template. Relative PATH defaults
    /// are joined to this.
    pub job_template_dir: &'a str,

    /// Current working directory. Relative PATH user values are joined
    /// to this.
    pub current_working_dir: &'a str,

    /// How path strings are interpreted. `PathFormat::host()` for
    /// local filesystem paths; `Posix` or `Windows` when paths
    /// originate from a known platform (e.g. cross-platform render
    /// farms).
    pub path_format: openjd_expr::path_mapping::PathFormat,

    /// If false, PATH defaults must be relative and within
    /// `job_template_dir`. If true, absolute defaults and `..`
    /// walk-up are permitted.
    pub allow_template_dir_walk_up: bool,

    /// If true, URI values (`scheme://...`) in PATH parameters are
    /// preserved as-is (requires EXPR). If false with EXPR, URIs are
    /// rejected. Without EXPR, the flag is ignored.
    pub allow_uri_path_values: bool,
}

impl<'a> PathParameterOptions<'a> {
    pub fn new(job_template_dir: &'a str, current_working_dir: &'a str) -> Self;
}
```

### `MergedParameterDefinition`

The output of [`merge_job_parameter_definitions`]. Constraints from a
job template and its environment templates are tightened per §1.2.1
(allowed-values intersected, min taking the max, max taking the min).

```rust
#[derive(Debug, Clone)]
pub struct MergedParameterDefinition {
    pub name: String,
    pub param_type: JobParameterType,
    pub default: Option<String>,
    pub object_type: Option<ObjectType>,
    pub data_flow: Option<DataFlow>,
    /// Name of the template that last defined/contributed to this parameter.
    pub source: String,
    // Merged constraint fields are crate-private; access via
    // `check_constraints`.
}

impl MergedParameterDefinition {
    /// Verify that merge produced a satisfiable set (§1.2.1 "No template
    /// may narrow another's constraints to the empty set").
    pub fn validate_satisfiable(&self) -> Result<(), ModelError>;

    pub fn check_constraints(&self, value: &ExprValue) -> Result<(), ModelError>;
    pub fn min_value_i64(&self) -> Option<i64>;
    pub fn max_value_i64(&self) -> Option<i64>;
    pub fn min_value_f64(&self) -> Option<f64>;
    pub fn max_value_f64(&self) -> Option<f64>;
    pub fn min_length(&self) -> Option<usize>;
    pub fn max_length(&self) -> Option<usize>;
    pub fn allowed_values_int(&self) -> Option<&[i64]>;
    pub fn allowed_values_float(&self) -> Option<&[f64]>;
    pub fn allowed_values_str(&self) -> Option<&[String]>;
}
```

### `convert_environment_with_symtab`

Available via the `create_job::` module path:

```rust
/// Convert a template Environment to a job Environment, optionally
/// filtering the symbol table to only the symbols this environment
/// references. When `symtab` is `Some`, the returned Environment's
/// `resolved_symtab` field carries the filtered table so that the
/// session side can reconstruct this environment's context without
/// the whole job's state.
pub fn convert_environment_with_symtab(
    env: &template::Environment,
    symtab: Option<&SymbolTable>,
) -> job::Environment;
```

## Parameter Space Iteration

```rust
/// Lazy iterator over a resolved step parameter space.
///
/// Supports random access (`get(index)`) for non-adaptive spaces and
/// sequential iteration via `Iterator`. Construct from a
/// `job::StepParameterSpace` that `create_job` produced.
pub struct StepParameterSpaceIterator { /* private fields */ }

impl StepParameterSpaceIterator {
    pub fn new(space: &job::StepParameterSpace) -> Result<Self, ModelError>;

    /// Build with a one-task-per-chunk override. `Some(1)` disables
    /// adaptive chunking and lets the iterator count individual tasks.
    pub fn new_with_chunk_override(
        space: &job::StepParameterSpace,
        override_count: Option<usize>,
    ) -> Result<Self, ModelError>;

    pub fn names(&self) -> &HashSet<String>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;

    /// Random access. Returns `None` for out-of-bounds and for
    /// sequential-only spaces (adaptive chunking, contiguous chunking).
    pub fn get(&self, index: usize) -> Option<TaskParameterSet>;

    pub fn contains(&self, params: &TaskParameterSet) -> bool;
    pub fn validate_containment(&self, params: &TaskParameterSet) -> Result<(), String>;

    /// Adaptive chunking (TASK_CHUNKING with `targetRuntimeSeconds`).
    pub fn chunks_adaptive(&self) -> bool;
    pub fn chunks_parameter_name(&self) -> Option<&str>;
    pub fn chunks_default_task_count(&self) -> Option<usize>;
    pub fn set_chunks_default_task_count(&mut self, value: usize);

    /// Rewind the iterator so a fresh `Iterator::next` walk yields the
    /// same elements again. Preserves the adaptive chunk size set via
    /// `set_chunks_default_task_count`.
    pub fn reset(&mut self);
}

impl Iterator for StepParameterSpaceIterator {
    type Item = TaskParameterSet;
    fn next(&mut self) -> Option<TaskParameterSet>;
    fn size_hint(&self) -> (usize, Option<usize>);
}
```

Random-access indexing uses `O(1)` arithmetic on a product-of-factors
representation — submitters that want to shard a large parameter
space across workers can compute per-worker index slices without
iterating the whole space. Adaptive chunking (TASK_CHUNKING §4 /
RFC 0001) forces sequential iteration because chunk size depends on
runtime feedback; for that case, callers mutate
`set_chunks_default_task_count` while iterating to reshape chunks
dynamically.

## Step Dependency Graph

```rust
#[derive(Debug)]
pub struct StepDependencyEdge {
    pub origin: usize,     // index of the depended-upon step
    pub dependent: usize,  // index of the depending step
}

#[derive(Debug)]
pub struct StepDependencyNode {
    pub step_index: usize,
    pub name: String,
    pub in_edges: Vec<usize>,   // indices into the edges vector
    pub out_edges: Vec<usize>,
}

/// Directed acyclic graph over an instantiated job's steps. Built from
/// `job::Job.steps[].dependencies`.
#[derive(Debug)]
pub struct StepDependencyGraph { /* private fields */ }

impl StepDependencyGraph {
    pub fn new(job: &job::Job) -> Result<Self, ModelError>;
    pub fn node_count(&self) -> usize;
    pub fn step_node(&self, name: &str) -> Option<&StepDependencyNode>;
    pub fn node(&self, index: usize) -> Option<&StepDependencyNode>;
    pub fn edge(&self, index: usize) -> Option<&StepDependencyEdge>;
    pub fn max_indegree(&self) -> usize;
    pub fn max_outdegree(&self) -> usize;

    /// Stable topological sort matching the Python reference
    /// implementation: DFS-based, template order is the tiebreaker.
    /// Returns step indices. Fails with a descriptive cycle path if
    /// the graph is cyclic.
    pub fn topo_sorted(&self) -> Result<Vec<usize>, ModelError>;

    /// Convenience wrapper returning step names instead of indices.
    pub fn topo_sorted_names(&self) -> Result<Vec<String>, ModelError>;
}
```

## Capabilities

Standard capability names are tied to `(revision, extensions)` because
a future revision could introduce capabilities, and the built-in
`STANDARD_*` tables are per-revision. All accessors return a `Result`
so the function signature is forward-compatible for "this revision
doesn't have this capability" outcomes.

```rust
pub mod capabilities {
    /// Amount capability names. Today (2023-09, no extensions):
    /// `amount.worker.vcpu`, `amount.worker.memory`, `amount.worker.gpu`,
    /// `amount.worker.gpu.memory`, `amount.worker.disk.scratch`.
    pub fn standard_amount_capability_names(
        revision: SpecificationRevision,
        extensions: &Extensions,
    ) -> Result<&'static [&'static str], ModelError>;

    /// Attribute capability names (just the names).
    pub fn standard_attribute_capability_names(
        revision: SpecificationRevision,
        extensions: &Extensions,
    ) -> Result<Vec<&'static str>, ModelError>;

    /// Attribute capability names paired with their allowed value sets.
    /// Today: `("attr.worker.os.family", ["linux", "windows", "macos"])`
    /// and `("attr.worker.cpu.arch", ["x86_64", "arm64"])`.
    pub fn standard_attribute_capabilities(
        revision: SpecificationRevision,
        extensions: &Extensions,
    ) -> Result<&'static [(&'static str, &'static [&'static str])], ModelError>;

    /// Check that a string matches the grammar for an amount capability
    /// name. Does not check whether the name is a *standard* capability —
    /// user-defined capabilities are allowed.
    pub fn validate_amount_capability_name(name: &str) -> Result<(), String>;

    /// Same, for attribute capability names.
    pub fn validate_attribute_capability_name(name: &str) -> Result<(), String>;
}
```

## Re-exports from `openjd-expr`

These appear in the crate's public API because they're used as field
types on template / job structs (`FormatString`) or as inputs to
instantiation functions (`SymbolTable`). Re-exporting them here means
downstream callers don't need to depend on `openjd-expr` directly for
common operations.

```rust
pub use openjd_expr::format_string;          // module
pub use openjd_expr::format_string::FormatString;
pub use openjd_expr::symbol_table;           // module
pub use openjd_expr::symbol_table::SymbolTable;
```

## Versioning and Stability Conventions

The crate targets the 2023-09 specification revision exclusively at
present. Per the future-revision-readiness report, the plumbing for a
second revision exists (`EffectiveLimits::from_context` dispatches on
revision, `validation::validate_*` dispatches on revision,
`decode_job_template` wraps its `from_value` call in a revision
match), but no second revision has been defined.

Enums that are marked `#[non_exhaustive]` today:

- `SpecificationRevision`
- `JobParameterType`
- `TemplateSpecificationVersion`
- `ModelExtension`
- `TaskParameterType`
- `FileType`
- `ModelError`

`EndOfLine`, `ObjectType`, and `DataFlow` are intentionally closed:
they represent decidable logical concepts (newline mode, filesystem
entity kind, data direction) whose sets of variants are not expected
to change.
