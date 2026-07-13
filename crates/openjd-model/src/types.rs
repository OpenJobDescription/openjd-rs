// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Core types shared across specification versions.
//!
//! Mirrors Python `_types.py`: SpecificationRevision, ParameterValueType,
//! ParameterValue, TemplateSpecificationVersion, etc.

use std::collections::HashMap;
use std::fmt;

use indexmap::IndexMap;
use openjd_expr::ExprType;
use serde::{Deserialize, Serialize};

// ── String-typed enums for compile-time safety ──

/// §6 Embedded file type.
///
/// Marked `#[non_exhaustive]` so that future revisions or extensions
/// can add new file types (for example, a `Binary` variant, which has
/// been reserved space in the spec since RFC 0001 discussion) without
/// a SemVer break for downstream crates that match on this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
pub enum FileType {
    Text,
}

impl fmt::Display for FileType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text => write!(f, "TEXT"),
        }
    }
}

/// End-of-line mode for embedded files (FEATURE_BUNDLE_1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EndOfLine {
    Lf,
    Crlf,
    Auto,
}

impl fmt::Display for EndOfLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lf => write!(f, "LF"),
            Self::Crlf => write!(f, "CRLF"),
            Self::Auto => write!(f, "AUTO"),
        }
    }
}

/// §2.2 PATH parameter objectType.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ObjectType {
    File,
    Directory,
}

impl fmt::Display for ObjectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File => write!(f, "FILE"),
            Self::Directory => write!(f, "DIRECTORY"),
        }
    }
}

/// §2.2 PATH parameter dataFlow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DataFlow {
    None,
    In,
    Out,
    Inout,
}

impl fmt::Display for DataFlow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "NONE"),
            Self::In => write!(f, "IN"),
            Self::Out => write!(f, "OUT"),
            Self::Inout => write!(f, "INOUT"),
        }
    }
}

/// Specification revision identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SpecificationRevision {
    V2023_09,
}

impl SpecificationRevision {
    /// The current revision. Equivalent to the most recent variant.
    pub const CURRENT: SpecificationRevision = SpecificationRevision::V2023_09;
}

impl Default for SpecificationRevision {
    fn default() -> Self {
        SpecificationRevision::CURRENT
    }
}

impl fmt::Display for SpecificationRevision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::V2023_09 => write!(f, "2023-09"),
        }
    }
}

/// Template specification version strings (the `specificationVersion` field value).
///
/// `#[non_exhaustive]` because future revisions will add new variants
/// (e.g., `JobTemplate2027_XX`, `Environment2027_XX`). Adding a variant
/// must not be a breaking change for downstream crates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TemplateSpecificationVersion {
    JobTemplate2023_09,
    Environment2023_09,
}

impl TemplateSpecificationVersion {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::JobTemplate2023_09 => "jobtemplate-2023-09",
            Self::Environment2023_09 => "environment-2023-09",
        }
    }

    pub fn is_job_template(&self) -> bool {
        matches!(self, Self::JobTemplate2023_09)
    }

    pub fn is_environment_template(&self) -> bool {
        matches!(self, Self::Environment2023_09)
    }

    pub fn revision(&self) -> SpecificationRevision {
        match self {
            Self::JobTemplate2023_09 | Self::Environment2023_09 => SpecificationRevision::V2023_09,
        }
    }
}

impl std::str::FromStr for TemplateSpecificationVersion {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "jobtemplate-2023-09" => Ok(Self::JobTemplate2023_09),
            "environment-2023-09" => Ok(Self::Environment2023_09),
            _ => Err(format!("unknown specification version: '{s}'")),
        }
    }
}

/// The type of a job parameter definition.
///
/// Marked `#[non_exhaustive]` so that future revisions and extensions can
/// add parameter types (as RFC 0007 already did for `Bool`, `RangeExpr`,
/// and the `List[…]` family) without a SemVer break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[non_exhaustive]
pub enum JobParameterType {
    String,
    Int,
    Float,
    Path,
    Bool,
    RangeExpr,
    ListString,
    ListInt,
    ListFloat,
    ListPath,
    ListBool,
    ListListInt,
}

impl JobParameterType {
    /// Parse from the spec string (case-insensitive).
    pub fn from_spec_str(s: &str) -> Option<Self> {
        let upper = s.to_ascii_uppercase();
        match upper.as_str() {
            "STRING" => Some(Self::String),
            "INT" => Some(Self::Int),
            "FLOAT" => Some(Self::Float),
            "PATH" => Some(Self::Path),
            "BOOL" => Some(Self::Bool),
            "RANGE_EXPR" => Some(Self::RangeExpr),
            "LIST[STRING]" => Some(Self::ListString),
            "LIST[INT]" => Some(Self::ListInt),
            "LIST[FLOAT]" => Some(Self::ListFloat),
            "LIST[PATH]" => Some(Self::ListPath),
            "LIST[BOOL]" => Some(Self::ListBool),
            "LIST[LIST[INT]]" => Some(Self::ListListInt),
            _ => None,
        }
    }

    /// Returns the canonical spec string.
    pub fn as_spec_str(&self) -> &'static str {
        match self {
            Self::String => "STRING",
            Self::Int => "INT",
            Self::Float => "FLOAT",
            Self::Path => "PATH",
            Self::Bool => "BOOL",
            Self::RangeExpr => "RANGE_EXPR",
            Self::ListString => "LIST[STRING]",
            Self::ListInt => "LIST[INT]",
            Self::ListFloat => "LIST[FLOAT]",
            Self::ListPath => "LIST[PATH]",
            Self::ListBool => "LIST[BOOL]",
            Self::ListListInt => "LIST[LIST[INT]]",
        }
    }

    /// Returns the `ExprType` this parameter produces in the symbol table.
    pub fn expr_type(&self) -> ExprType {
        match self {
            Self::String => ExprType::STRING,
            Self::Int => ExprType::INT,
            Self::Float => ExprType::FLOAT,
            Self::Path => ExprType::PATH,
            Self::Bool => ExprType::BOOL,
            Self::RangeExpr => ExprType::RANGE_EXPR,
            Self::ListString => ExprType::list(ExprType::STRING),
            Self::ListInt => ExprType::list(ExprType::INT),
            Self::ListFloat => ExprType::list(ExprType::FLOAT),
            Self::ListPath => ExprType::list(ExprType::PATH),
            Self::ListBool => ExprType::list(ExprType::BOOL),
            Self::ListListInt => ExprType::list(ExprType::list(ExprType::INT)),
        }
    }
}

impl fmt::Display for JobParameterType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_spec_str())
    }
}

/// The type of a task parameter definition.
///
/// Marked `#[non_exhaustive]` so that future revisions and extensions
/// can add task parameter types (for example, a list-typed task
/// parameter analogous to `JobParameterType::ListInt`, or additional
/// chunked variants) without a SemVer break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TaskParameterType {
    Int,
    Float,
    String,
    Path,
    ChunkInt,
}

impl TaskParameterType {
    /// Parse from the spec string (case-insensitive).
    pub fn from_spec_str(s: &str) -> Option<Self> {
        let upper = s.to_ascii_uppercase();
        match upper.as_str() {
            "INT" => Some(Self::Int),
            "FLOAT" => Some(Self::Float),
            "STRING" => Some(Self::String),
            "PATH" => Some(Self::Path),
            "CHUNK[INT]" => Some(Self::ChunkInt),
            _ => None,
        }
    }

    /// Returns the canonical spec string.
    pub fn as_spec_str(&self) -> &'static str {
        match self {
            Self::Int => "INT",
            Self::Float => "FLOAT",
            Self::String => "STRING",
            Self::Path => "PATH",
            Self::ChunkInt => "CHUNK[INT]",
        }
    }

    /// Returns the `ExprType` this parameter produces in the symbol table.
    pub fn expr_type(&self) -> ExprType {
        match self {
            Self::Int => ExprType::INT,
            Self::Float => ExprType::FLOAT,
            Self::String => ExprType::STRING,
            Self::Path => ExprType::PATH,
            Self::ChunkInt => ExprType::RANGE_EXPR,
        }
    }
}

impl fmt::Display for TaskParameterType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_spec_str())
    }
}

/// A processed job parameter value.
#[derive(Debug, Clone)]
pub struct JobParameterValue {
    pub param_type: JobParameterType,
    pub value: openjd_expr::ExprValue,
}

/// A processed task parameter value.
#[derive(Debug, Clone)]
pub struct TaskParameterValue {
    pub param_type: TaskParameterType,
    pub value: openjd_expr::ExprValue,
}

/// Input parameter values from the user (name → value).
///
/// Values are `ExprValue` so callers can pass native types directly:
/// - CLI callers pass `ExprValue::String("42".into())` for everything and
///   let `preprocess_job_parameters` coerce to the target type.
/// - Library callers can pass typed values like `ExprValue::Int(42)` or
///   `ExprValue::ListInt(vec![1, 2, 3])` directly.
pub type JobParameterInputValues = HashMap<String, openjd_expr::ExprValue>;

/// Processed job parameter values (name → typed value).
pub type JobParameterValues = HashMap<String, JobParameterValue>;

/// A single task's parameter values.
pub type TaskParameterSet = IndexMap<String, TaskParameterValue>;

/// Set of extensions enabled for a template.
pub type Extensions = std::collections::HashSet<ModelExtension>;

/// Extension variants recognized by `openjd-model` for the 2023-09
/// specification revision.
///
/// Template `extensions` lists are parsed into `ModelExtension`
/// values; unrecognized strings produce a `FromStr` error at the parse
/// boundary, so once an `Extensions` set has been constructed every
/// element is guaranteed to be a known extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ModelExtension {
    TaskChunking,
    RedactedEnvVars,
    FeatureBundle1,
    Expr,
    /// `WRAP_ACTIONS` — enables `onWrapEnvEnter`, `onWrapTaskRun`, and
    /// `onWrapEnvExit` on `<EnvironmentActions>`. See RFC 0008.
    WrapActions,
}

impl ModelExtension {
    /// All extension variants, in a stable order for iteration and
    /// for building default "enable all" allowlists.
    pub const ALL: &'static [ModelExtension] = &[
        Self::TaskChunking,
        Self::RedactedEnvVars,
        Self::FeatureBundle1,
        Self::Expr,
        Self::WrapActions,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TaskChunking => "TASK_CHUNKING",
            Self::RedactedEnvVars => "REDACTED_ENV_VARS",
            Self::FeatureBundle1 => "FEATURE_BUNDLE_1",
            Self::Expr => "EXPR",
            Self::WrapActions => "WRAP_ACTIONS",
        }
    }
}

impl std::str::FromStr for ModelExtension {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "TASK_CHUNKING" => Ok(Self::TaskChunking),
            "REDACTED_ENV_VARS" => Ok(Self::RedactedEnvVars),
            "FEATURE_BUNDLE_1" => Ok(Self::FeatureBundle1),
            "EXPR" => Ok(Self::Expr),
            "WRAP_ACTIONS" => Ok(Self::WrapActions),
            _ => Err(format!("Unknown extension: {s}")),
        }
    }
}

/// Serialize as the canonical UPPER_SNAKE_CASE extension name, so the
/// transport form matches what appears in template YAML/JSON and in
/// the Python implementation (e.g. `"EXPR"`, not `"Expr"`).
impl serde::Serialize for ModelExtension {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Caller-provided limits that layer on top of spec-defined limits.
///
/// These allow a service or application to impose additional restrictions
/// beyond what the OpenJD specification requires. All fields are optional —
/// `None` means "no additional restriction beyond the spec-defined limit."
///
/// Caller limits can only add restrictions, never relax spec-defined ones.
#[derive(Debug, Clone, Default)]
pub struct CallerLimits {
    /// Maximum number of steps in a job template.
    pub max_step_count: Option<usize>,
    /// Maximum number of environments (job + all step environments combined)
    /// in a job template.
    pub max_env_count: Option<usize>,
    /// Maximum total task count across all steps in a job template.
    /// Checked after parameter space ranges are resolved in `create_job`.
    pub max_task_count: Option<u64>,
    /// Maximum JSON-encoded size of a step script, in bytes.
    pub max_step_script_size: Option<usize>,
    /// Maximum JSON-encoded size of an environment, in bytes.
    pub max_environment_size: Option<usize>,
    /// Maximum total template document size, in bytes.
    pub max_template_size: Option<usize>,
}

/// Model-side profile: the specification revision plus the set of
/// enabled extensions that together describe what features a template
/// or job may use.
///
/// `ModelProfile` is the `openjd-model` counterpart of
/// [`openjd_expr::ExprProfile`]. Both crates share the pattern:
///
/// - `openjd-expr`: [`ExprProfile`](openjd_expr::ExprProfile) drives
///   [`FunctionLibrary::for_profile`](openjd_expr::FunctionLibrary::for_profile).
///   Its third axis is [`HostContext`](openjd_expr::HostContext) — host-supplied
///   runtime state (path mapping rules).
/// - `openjd-model`: `ModelProfile` drives template validation and
///   job creation. Caller policy is orthogonal and carried separately
///   in [`CallerLimits`]; the two are bundled into a
///   [`ValidationContext`] where both are needed together.
///
/// Profiles are small value types: clone them freely, store them on
/// sessions, pass them by reference into validators. `ModelProfile`
/// has no mutable operations other than builder-style `with_*`
/// methods that return a new profile.
///
/// Use [`ModelProfile::to_expr_profile`] to derive a matching
/// `ExprProfile` when calling into `openjd-expr`; the caller supplies
/// the appropriate `HostContext` for their situation.
#[derive(Debug, Clone)]
pub struct ModelProfile {
    revision: SpecificationRevision,
    extensions: Extensions,
}

impl ModelProfile {
    /// Build a profile for the given revision with no extensions enabled.
    pub fn new(revision: SpecificationRevision) -> Self {
        Self {
            revision,
            extensions: Extensions::new(),
        }
    }

    /// Shortcut for `ModelProfile::new(SpecificationRevision::CURRENT)`.
    ///
    /// Builds a profile with the current revision and *no* extensions. Use
    /// this when you want a stable baseline: future crate versions that ship
    /// a new revision will change what [`SpecificationRevision::CURRENT`]
    /// points to, but the extension set will remain explicitly empty.
    pub fn current() -> Self {
        Self::new(SpecificationRevision::CURRENT)
    }

    /// Build a profile at the current revision *with every known extension
    /// enabled*.
    ///
    /// **This profile is intentionally unstable across crate versions.** As
    /// new variants are added to [`ModelExtension::ALL`] and new revisions
    /// land at [`SpecificationRevision::CURRENT`], the set of enabled
    /// features grows. For behavior stable across crate versions, construct
    /// a profile with an explicit revision and extension set via
    /// [`ModelProfile::new`] or [`ModelProfile::current`].
    pub fn latest() -> Self {
        Self {
            revision: SpecificationRevision::CURRENT,
            extensions: ModelExtension::ALL.iter().copied().collect(),
        }
    }

    /// Set the enabled extensions (replaces any existing set).
    #[must_use]
    pub fn with_extensions(mut self, extensions: Extensions) -> Self {
        self.extensions = extensions;
        self
    }

    /// The specification revision this profile targets.
    pub fn revision(&self) -> SpecificationRevision {
        self.revision
    }

    /// The set of extensions this profile enables.
    pub fn extensions(&self) -> &Extensions {
        &self.extensions
    }

    /// True iff `ext` is enabled in this profile.
    pub fn has_extension(&self, ext: ModelExtension) -> bool {
        self.extensions.contains(&ext)
    }

    /// Derive an [`ExprProfile`](openjd_expr::ExprProfile) matching this
    /// profile's revision and extensions, with the caller-specified
    /// [`HostContext`](openjd_expr::HostContext).
    ///
    /// This is the bridge from `openjd-model` to `openjd-expr`: call
    /// this and pass the result to
    /// [`FunctionLibrary::for_profile`](openjd_expr::FunctionLibrary::for_profile).
    ///
    /// The `host_context` argument is a caller responsibility because
    /// the model has no opinion on it — template validation uses
    /// [`HostContext::Unresolved`](openjd_expr::HostContext::Unresolved),
    /// runtime session work uses
    /// [`HostContext::WithRules`](openjd_expr::HostContext::WithRules),
    /// and pure-template work (e.g. resolving the job name) uses
    /// [`HostContext::None`](openjd_expr::HostContext::None).
    pub fn to_expr_profile(
        &self,
        host_context: openjd_expr::HostContext,
    ) -> openjd_expr::ExprProfile {
        // Map this crate's SpecificationRevision onto openjd-expr's
        // ExprRevision. The mapping is total today because both enums
        // have a single variant each; future revisions will add arms
        // here as both sides grow. The match on revision mirrors the
        // pattern used in EffectiveLimits::from_context.
        let revision = match self.revision {
            SpecificationRevision::V2023_09 => openjd_expr::ExprRevision::V2026_02,
        };
        // `ExprExtension` is empty today — no expression-level
        // extensions exist yet. Model-side `ModelExtension` variants
        // gate *where* expressions are permitted in templates (EXPR,
        // FEATURE_BUNDLE_1, TASK_CHUNKING, REDACTED_ENV_VARS), not
        // which functions are registered once they are permitted, so
        // the expression-level extension set is always empty for now.
        let extensions = std::collections::HashSet::new();
        openjd_expr::ExprProfile::new(revision)
            .with_extensions(extensions)
            .with_host_context(host_context)
    }
}

impl Default for ModelProfile {
    fn default() -> Self {
        Self::current()
    }
}

/// Context for validation, carrying a [`ModelProfile`] and caller-policy
/// [`CallerLimits`] as a single bundle.
///
/// Use this type when a function needs both the profile (revision +
/// extensions) and the caller's policy overrides. When only the
/// profile is needed, take `&ModelProfile` directly.
#[derive(Debug, Clone)]
pub struct ValidationContext {
    pub profile: ModelProfile,
    pub caller_limits: CallerLimits,
}

impl ValidationContext {
    /// Build a context for the given revision with no extensions and
    /// default caller limits.
    pub fn new(revision: SpecificationRevision) -> Self {
        Self {
            profile: ModelProfile::new(revision),
            caller_limits: CallerLimits::default(),
        }
    }

    /// Build a context with the given revision + extensions and
    /// default caller limits.
    pub fn with_extensions(revision: SpecificationRevision, extensions: Extensions) -> Self {
        Self {
            profile: ModelProfile::new(revision).with_extensions(extensions),
            caller_limits: CallerLimits::default(),
        }
    }

    /// Build a context from an existing [`ModelProfile`], with default
    /// caller limits.
    pub fn from_profile(profile: ModelProfile) -> Self {
        Self {
            profile,
            caller_limits: CallerLimits::default(),
        }
    }

    /// Attach caller limits, consuming and returning `self`.
    #[must_use]
    pub fn with_caller_limits(mut self, caller_limits: CallerLimits) -> Self {
        self.caller_limits = caller_limits;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_VERSIONS: &[TemplateSpecificationVersion] = &[
        TemplateSpecificationVersion::JobTemplate2023_09,
        TemplateSpecificationVersion::Environment2023_09,
    ];

    fn job_template_versions() -> Vec<TemplateSpecificationVersion> {
        ALL_VERSIONS
            .iter()
            .copied()
            .filter(|v| v.is_job_template())
            .collect()
    }

    fn environment_template_versions() -> Vec<TemplateSpecificationVersion> {
        ALL_VERSIONS
            .iter()
            .copied()
            .filter(|v| v.is_environment_template())
            .collect()
    }

    #[test]
    fn test_all_values_classified() {
        let job_versions: std::collections::HashSet<_> =
            job_template_versions().into_iter().collect();
        let env_versions: std::collections::HashSet<_> =
            environment_template_versions().into_iter().collect();
        // No overlap
        assert!(job_versions.is_disjoint(&env_versions));
        // Together they cover all versions
        let all: std::collections::HashSet<_> = ALL_VERSIONS.iter().copied().collect();
        let union: std::collections::HashSet<_> =
            job_versions.union(&env_versions).copied().collect();
        assert_eq!(union, all);
    }

    #[test]
    fn test_job_template_versions() {
        for v in job_template_versions() {
            assert!(v.is_job_template(), "{:?} should be a job template", v);
        }
    }

    #[test]
    fn test_not_job_template_versions() {
        for v in ALL_VERSIONS {
            if !v.is_job_template() {
                assert!(v.is_environment_template());
            }
        }
    }

    #[test]
    fn test_environment_template_versions() {
        for v in environment_template_versions() {
            assert!(
                v.is_environment_template(),
                "{:?} should be an env template",
                v
            );
        }
    }

    #[test]
    fn test_not_environment_template_versions() {
        for v in ALL_VERSIONS {
            if !v.is_environment_template() {
                assert!(v.is_job_template());
            }
        }
    }

    #[test]
    fn test_from_str_roundtrip() {
        for v in ALL_VERSIONS {
            let s = v.as_str();
            let parsed: Result<TemplateSpecificationVersion, _> = s.parse();
            assert_eq!(parsed, Ok(*v));
        }
        assert!("unknown".parse::<TemplateSpecificationVersion>().is_err());
    }

    #[test]
    fn test_revision() {
        for v in ALL_VERSIONS {
            assert_eq!(v.revision(), SpecificationRevision::V2023_09);
        }
    }

    // ── JobParameterType tests ──

    const ALL_JOB_PARAM_TYPES: &[JobParameterType] = &[
        JobParameterType::String,
        JobParameterType::Int,
        JobParameterType::Float,
        JobParameterType::Path,
        JobParameterType::Bool,
        JobParameterType::RangeExpr,
        JobParameterType::ListString,
        JobParameterType::ListInt,
        JobParameterType::ListFloat,
        JobParameterType::ListPath,
        JobParameterType::ListBool,
        JobParameterType::ListListInt,
    ];

    #[test]
    fn test_job_param_type_roundtrip() {
        for &t in ALL_JOB_PARAM_TYPES {
            let s = t.as_spec_str();
            let parsed = JobParameterType::from_spec_str(s).unwrap();
            assert_eq!(parsed, t, "round-trip failed for {s}");
        }
    }

    #[test]
    fn test_job_param_type_case_insensitive() {
        assert_eq!(
            JobParameterType::from_spec_str("string"),
            Some(JobParameterType::String)
        );
        assert_eq!(
            JobParameterType::from_spec_str("Int"),
            Some(JobParameterType::Int)
        );
        assert_eq!(
            JobParameterType::from_spec_str("list[int]"),
            Some(JobParameterType::ListInt)
        );
        assert_eq!(
            JobParameterType::from_spec_str("List[List[Int]]"),
            Some(JobParameterType::ListListInt)
        );
        assert_eq!(
            JobParameterType::from_spec_str("range_expr"),
            Some(JobParameterType::RangeExpr)
        );
    }

    #[test]
    fn test_job_param_type_unknown() {
        assert_eq!(JobParameterType::from_spec_str("UNKNOWN"), None);
        assert_eq!(JobParameterType::from_spec_str(""), None);
        assert_eq!(JobParameterType::from_spec_str("LIST[UNKNOWN]"), None);
    }

    #[test]
    fn test_job_param_type_expr_type() {
        assert_eq!(JobParameterType::String.expr_type(), ExprType::STRING);
        assert_eq!(JobParameterType::Path.expr_type(), ExprType::PATH);
        assert_eq!(
            JobParameterType::ListInt.expr_type(),
            ExprType::list(ExprType::INT)
        );
        assert_eq!(
            JobParameterType::ListListInt.expr_type(),
            ExprType::list(ExprType::list(ExprType::INT))
        );
    }

    #[test]
    fn test_job_param_type_display() {
        assert_eq!(format!("{}", JobParameterType::String), "STRING");
        assert_eq!(format!("{}", JobParameterType::ListPath), "LIST[PATH]");
    }

    // ── TaskParameterType tests ──

    const ALL_TASK_PARAM_TYPES: &[TaskParameterType] = &[
        TaskParameterType::Int,
        TaskParameterType::Float,
        TaskParameterType::String,
        TaskParameterType::Path,
        TaskParameterType::ChunkInt,
    ];

    #[test]
    fn test_task_param_type_roundtrip() {
        for &t in ALL_TASK_PARAM_TYPES {
            let s = t.as_spec_str();
            let parsed = TaskParameterType::from_spec_str(s).unwrap();
            assert_eq!(parsed, t, "round-trip failed for {s}");
        }
    }

    #[test]
    fn test_task_param_type_unknown() {
        assert_eq!(TaskParameterType::from_spec_str("UNKNOWN"), None);
        assert_eq!(TaskParameterType::from_spec_str("BOOL"), None);
    }

    #[test]
    fn test_task_param_type_expr_type() {
        assert_eq!(TaskParameterType::String.expr_type(), ExprType::STRING);
        assert_eq!(TaskParameterType::Path.expr_type(), ExprType::PATH);
        assert_eq!(
            TaskParameterType::ChunkInt.expr_type(),
            ExprType::RANGE_EXPR
        );
    }

    #[test]
    fn test_task_param_type_display() {
        assert_eq!(format!("{}", TaskParameterType::ChunkInt), "CHUNK[INT]");
        assert_eq!(format!("{}", TaskParameterType::Int), "INT");
    }

    #[test]
    fn test_model_extension_serializes_as_canonical_string() {
        // ModelExtension must round-trip through the canonical
        // UPPER_SNAKE_CASE name rather than the Rust variant name, so
        // serialized Jobs match the form consumed by tools and the
        // Python implementation.
        assert_eq!(
            serde_json::to_string(&ModelExtension::Expr).unwrap(),
            "\"EXPR\""
        );
        assert_eq!(
            serde_json::to_string(&ModelExtension::TaskChunking).unwrap(),
            "\"TASK_CHUNKING\""
        );
        assert_eq!(
            serde_json::to_string(&ModelExtension::RedactedEnvVars).unwrap(),
            "\"REDACTED_ENV_VARS\""
        );
        assert_eq!(
            serde_json::to_string(&ModelExtension::FeatureBundle1).unwrap(),
            "\"FEATURE_BUNDLE_1\""
        );
        let v = vec![ModelExtension::Expr, ModelExtension::TaskChunking];
        assert_eq!(
            serde_json::to_string(&v).unwrap(),
            "[\"EXPR\",\"TASK_CHUNKING\"]"
        );
    }

    #[test]
    fn specification_revision_current_is_v2023_09() {
        assert_eq!(
            SpecificationRevision::CURRENT,
            SpecificationRevision::V2023_09
        );
    }

    #[test]
    fn model_profile_current_has_current_revision_and_no_extensions() {
        let p = ModelProfile::current();
        assert_eq!(p.revision(), SpecificationRevision::CURRENT);
        assert!(p.extensions().is_empty());
    }

    #[test]
    fn model_profile_latest_enables_all_extensions() {
        let p = ModelProfile::latest();
        assert_eq!(p.revision(), SpecificationRevision::CURRENT);
        for ext in ModelExtension::ALL {
            assert!(
                p.has_extension(*ext),
                "ModelProfile::latest() must enable every extension in ModelExtension::ALL; missing {ext:?}"
            );
        }
        assert_eq!(p.extensions().len(), ModelExtension::ALL.len());
    }

    #[test]
    fn model_profile_default_is_current() {
        let p = ModelProfile::default();
        assert_eq!(p.revision(), SpecificationRevision::CURRENT);
        assert!(p.extensions().is_empty());
    }
}
