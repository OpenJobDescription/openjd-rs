// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Applying a job diff to a store, without knowing what the store is.
//!
//! **Unstable.** This API may change or be removed in any release without
//! notice. Don't depend on it outside openjd-rs.
//!
//! Three stages:
//!
//! 1. [`plan`] diffs the stored and requested jobs and turns the
//!    [`DiffTree`] into an ordered [`ApplyPlan`]. Every op is tagged with the [`MutationKind`]s it carries.
//!    Steps are addressed by index (`new.steps[j]`, or `old.steps[i]` for a
//!    removal). Adds and modifies run in topological order of the new step
//!    graph, so a step exists before anything depends on it. Removals run
//!    last, in reverse topological order of the old graph. A step's task
//!    change follows the step, as [`TaskChange`] boxes from
//!    [`task_delta`](crate::job::task_delta).
//! 2. [`validate`] asks a [`MutationPolicy`], then the store's own
//!    [`JobApplier::check`], about every op before anything is written.
//! 3. [`execute`] hands the ops, in order, to a [`JobApplier`].
//!
//! [`apply`] runs all three. See `specs/model/job-apply.md`.
//!
//! A host store plugs in by implementing [`JobApplier`], directly in Rust or
//! through an adaptor that forwards each call to another language. The
//! trait is object safe, every call names its step ([`StepRef`]), and every
//! payload has a plain-data form ([`Op::as_document`],
//! [`TaskOp::as_document`], [`PlannedOp::as_document`]), none needing an
//! argument.

use std::collections::{BTreeSet, HashSet};
use std::fmt;
use std::str::FromStr;

use serde::ser::{Error as _, SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};

use crate::error::ModelError;
use crate::job::diff::{
    diff_environments, diff_jobs, diff_steps, DiffNode, DiffNodeId, DiffNodeKind, DiffOptions,
    DiffStatus, DiffTree,
};
use crate::job::document::{ParameterDoc, SpaceDoc};
use crate::job::task_delta::{task_delta, task_parameter_set_document, ProductDelta, TaskDelta};
use crate::job::template_diff::{DocEnvironment, DocStep, JobDocument};
use crate::job::{Environment, Job, JobParameter, Step, StepParameterSpace};
use crate::step_dependency_graph::StepDependencyGraph;
use crate::step_param_space::StepParameterSpaceIterator;
use crate::types::TaskParameterSet;

/// Index of a step: into `new.steps`, or into `old.steps` for a removal.
pub type StepIndex = usize;

/// A step as an op names it: its name, and its index in `new.steps` (or
/// `old.steps` for a removal). A store keyed by name never needs the job to
/// resolve the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct StepRef<'a> {
    /// The step's name.
    pub name: &'a str,
    /// The step's index, as [`StepIndex`].
    pub index: StepIndex,
}

/// The object an op writes, owned, for messages and errors.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OpTarget {
    /// The job's own fields.
    Job,
    /// A job parameter, by name.
    Parameter(String),
    /// A job environment, by name.
    Environment(String),
    /// A step (its own fields or its tasks).
    Step {
        /// The step's name.
        name: String,
        /// The step's index, as in [`StepRef`].
        index: StepIndex,
    },
}

impl fmt::Display for OpTarget {
    /// `job`, `parameter Frames`, `job environment Conda`, `step Render`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Job => f.write_str("job"),
            Self::Parameter(n) => write!(f, "parameter {n}"),
            Self::Environment(n) => write!(f, "job environment {n}"),
            Self::Step { name, .. } => write!(f, "step {name}"),
        }
    }
}

/// A model type with a plain-data form: the form [`JobDocument`] writes it
/// in. `parameters` and environment `variables` are sorted by name, typed
/// values are in the host encoding, and `taskParameterDefinitions` is a list
/// in definition order, so no part of the document
/// depends on the order of an object's keys. See `specs/model/host-support.md`.
pub trait ToDocument {
    /// The document view. It borrows `self` and serializes on demand.
    fn as_document(&self) -> impl Serialize + '_;
}

impl ToDocument for Job {
    fn as_document(&self) -> impl Serialize + '_ {
        JobDocument(self)
    }
}

impl ToDocument for Step {
    fn as_document(&self) -> impl Serialize + '_ {
        DocStep::new(self)
    }
}

impl ToDocument for Environment {
    fn as_document(&self) -> impl Serialize + '_ {
        DocEnvironment::new(self)
    }
}

impl ToDocument for JobParameter {
    fn as_document(&self) -> impl Serialize + '_ {
        ParameterDoc::new(self)
    }
}

impl ToDocument for StepParameterSpace {
    fn as_document(&self) -> impl Serialize + '_ {
        SpaceDoc::new(self)
    }
}

/// A document as a `serde_json::Value`; a serialization failure becomes a
/// `DecodeValidation` error naming what was being written.
fn to_json_value(doc: impl Serialize, what: &str) -> Result<serde_json::Value, ModelError> {
    serde_json::to_value(doc)
        .map_err(|e| ModelError::DecodeValidation(format!("cannot write the {what} as data: {e}")))
}

/// A change to one object at one level. `T` is the model type at that level.
#[derive(Debug, PartialEq)]
pub enum Op<'a, T> {
    /// Create `new`.
    Add {
        /// The new object.
        new: &'a T,
    },
    /// Update `old` to `new`. `node` is the object's node in
    /// [`ApplyPlan::diff`], for stores that want the field-level detail.
    Modify {
        /// The object as stored.
        old: &'a T,
        /// The object as requested.
        new: &'a T,
        /// The object's node in the diff.
        node: DiffNodeId,
    },
    /// Delete `old`.
    Remove {
        /// The object as stored.
        old: &'a T,
    },
}

// Manual: a derive would require `T: Copy`, but every field is a reference.
impl<T> Clone for Op<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Op<'_, T> {}

impl<T> Op<'_, T> {
    /// `"add"`, `"modify"` or `"remove"`.
    #[must_use]
    pub fn verb(&self) -> &'static str {
        match self {
            Op::Add { .. } => "add",
            Op::Modify { .. } => "modify",
            Op::Remove { .. } => "remove",
        }
    }
}

impl<T: ToDocument> Op<'_, T> {
    /// The op as plain data: `{"change": "add", "new": …}`,
    /// `{"change": "modify", "old": …, "new": …}` or
    /// `{"change": "remove", "old": …}`, each object in its
    /// [`ToDocument`] form. `Modify::node` is left out: it indexes the
    /// plan's diff, which the host does not hold.
    pub fn as_document(&self) -> impl Serialize + '_ {
        OpDocument(*self)
    }

    /// [`as_document`](Self::as_document) as a `serde_json::Value`.
    pub fn to_value(&self) -> Result<serde_json::Value, ModelError> {
        to_json_value(self.as_document(), "op")
    }
}

struct OpDocument<'a, T>(Op<'a, T>);

impl<T: ToDocument> Serialize for OpDocument<'_, T> {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut m = ser.serialize_map(None)?;
        m.serialize_entry("change", self.0.verb())?;
        match self.0 {
            Op::Add { new } => m.serialize_entry("new", &new.as_document())?,
            Op::Modify { old, new, .. } => {
                m.serialize_entry("old", &old.as_document())?;
                m.serialize_entry("new", &new.as_document())?;
            }
            Op::Remove { old } => m.serialize_entry("old", &old.as_document())?,
        }
        m.end()
    }
}

/// How a step's tasks change.
///
/// A task is identified by its parameter values, never by a position: ids
/// are the host's. A host maps each task parameter set to its own task, and
/// gives each new task an id of its choosing. See "Task identity" in
/// `specs/model/job-apply.md`.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskChange {
    /// Product space: keep the tasks in `kept`, add those in `added`, delete
    /// those in `removed`, each named by its parameter set. The order of
    /// the additions is [`ProductDelta::appended`].
    Boxes(Box<ProductDelta>),
    /// A newly added step: every task of its space, from
    /// [`TaskOp::added_tasks`].
    All,
    /// No per-task mapping (association, chunking, a parameter added or
    /// removed, ...): the step's tasks become exactly those of its new
    /// space, from [`TaskOp::added_tasks`].
    Replace,
}

/// A step's task change with the step's new parameter space: what
/// [`JobApplier::tasks`] receives. Its [`added_tasks`](Self::added_tasks)
/// and [`as_document`](Self::as_document) need nothing else.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskOp<'a> {
    /// The step's new parameter space; `None` is one task with no
    /// parameters.
    pub space: Option<&'a StepParameterSpace>,
    /// How the step's tasks change.
    pub change: TaskChange,
}

impl<'a> TaskOp<'a> {
    /// The tasks this op adds, in the order a host creates them. For `All`
    /// and `Replace`, every task of the new space in `create_job`'s order:
    /// [`StepParameterSpaceIterator`] with the chunk override `create_job`
    /// counts tasks with (`Some(1)`: a `CHUNK[INT]` dimension gives one
    /// task per value, as CreateJob creates them), and one task with no
    /// parameters when `space` is `None`. For `Boxes`,
    /// [`ProductDelta::appended`]. A host never enumerates a space itself:
    /// a `combination` reorders dimensions, so definition order alone is
    /// not task order.
    pub fn added_tasks(
        &self,
    ) -> Result<impl Iterator<Item = TaskParameterSet> + use<'_, 'a>, ModelError> {
        let tasks: Box<dyn Iterator<Item = TaskParameterSet> + '_> =
            match (&self.change, self.space) {
                (TaskChange::Boxes(d), _) => Box::new(d.appended()?),
                (TaskChange::All | TaskChange::Replace, None) => {
                    Box::new(std::iter::once(TaskParameterSet::new()))
                }
                (TaskChange::All | TaskChange::Replace, Some(s)) => {
                    Box::new(StepParameterSpaceIterator::new_with_chunk_override(
                        s,
                        Some(CREATE_JOB_CHUNKS),
                    )?)
                }
            };
        Ok(tasks)
    }

    /// The number of tasks [`added_tasks`](Self::added_tasks) yields,
    /// without iterating.
    pub fn added_count(&self) -> Result<u64, ModelError> {
        Ok(match (&self.change, self.space) {
            (TaskChange::Boxes(d), _) => d.appended_count(),
            (TaskChange::All | TaskChange::Replace, None) => 1,
            (TaskChange::All | TaskChange::Replace, Some(s)) => {
                StepParameterSpaceIterator::new_with_chunk_override(s, Some(CREATE_JOB_CHUNKS))?
                    .len()
            }
        })
    }

    /// The op as plain data: `"space"`, the step's new parameter space as
    /// [`ToDocument`] writes it (`null` for no space), beside the task
    /// change, every task named by its parameter set
    /// ([`task_parameter_set_document`]). `All` and `Replace` are
    ///
    /// ```text
    /// {"space": …, "change": "all" | "replace", "taskCount": n,
    ///  "tasks": [<task parameter set>, …]}
    /// ```
    ///
    /// with `tasks` from [`added_tasks`](Self::added_tasks), so a host never
    /// enumerates the space. `Boxes` is
    ///
    /// ```text
    /// {"space": …, "change": "boxes", "oldCount": n, "keptCount": n,
    ///  "appendedCount": n, "appended": [<task parameter set>, …],
    ///  "removedCount": n, "removed": [<task parameter set>, …]}
    /// ```
    ///
    /// with `appended` in [`ProductDelta::appended`] order and `removed` in
    /// [`ProductDelta::removed`] order. The document holds no positions.
    /// Every listed task is written, so its size is linear in the number of
    /// tasks the change adds and removes (for `All` and `Replace`, the whole
    /// new space); the lists are written as they are iterated, without
    /// first being collected. A Rust adaptor that would rather stream the
    /// tasks calls [`added_tasks`](Self::added_tasks) itself.
    pub fn as_document(&self) -> impl Serialize + '_ {
        TaskOpDocument(self)
    }

    /// [`as_document`](Self::as_document) as a `serde_json::Value`.
    pub fn to_value(&self) -> Result<serde_json::Value, ModelError> {
        to_json_value(self.as_document(), "task change")
    }
}

/// The chunk override `create_job` counts a step's tasks with.
const CREATE_JOB_CHUNKS: u64 = 1;

struct TaskOpDocument<'a, 'b>(&'a TaskOp<'b>);

/// A list of task parameter sets, written as it is iterated.
struct TaskList<F>(F);

impl<F, I> Serialize for TaskList<F>
where
    F: Fn() -> Result<I, ModelError>,
    I: Iterator<Item = TaskParameterSet>,
{
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut seq = ser.serialize_seq(None)?;
        for t in (self.0)().map_err(S::Error::custom)? {
            seq.serialize_element(&task_parameter_set_document(&t))?;
        }
        seq.end()
    }
}

impl Serialize for TaskOpDocument<'_, '_> {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut m = ser.serialize_map(None)?;
        let op = self.0;
        m.serialize_entry("space", &op.space.map(SpaceDoc::new))?;
        let added = TaskList(|| op.added_tasks());
        match &op.change {
            TaskChange::All | TaskChange::Replace => {
                let name = if matches!(op.change, TaskChange::All) {
                    "all"
                } else {
                    "replace"
                };
                m.serialize_entry("change", name)?;
                let count = op.added_count().map_err(S::Error::custom)?;
                m.serialize_entry("taskCount", &count)?;
                m.serialize_entry("tasks", &added)?;
            }
            TaskChange::Boxes(d) => {
                m.serialize_entry("change", "boxes")?;
                m.serialize_entry("oldCount", &d.old.len())?;
                m.serialize_entry("keptCount", &d.kept.len())?;
                m.serialize_entry("appendedCount", &d.appended_count())?;
                m.serialize_entry("appended", &added)?;
                m.serialize_entry("removedCount", &d.removed_count())?;
                let removed = TaskList(|| Ok(d.removed()?.map(|(_, t)| t)));
                m.serialize_entry("removed", &removed)?;
            }
        }
        m.end()
    }
}

/// A kind of mutation, the unit a [`MutationPolicy`] allows or refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum MutationKind {
    /// Job `name` or `description`.
    JobMetadata,
    /// Job `extensions`.
    JobExtensions,
    /// A job parameter's value or type.
    ParameterValue,
    /// A job parameter added.
    ParameterAdd,
    /// A job parameter removed.
    ParameterRemove,
    /// An environment's `variables`.
    EnvVariables,
    /// An environment's script (actions, embedded files, let bindings).
    EnvScript,
    /// An environment's `name` or `description`.
    EnvMetadata,
    /// An environment's resolved symbol table: a parameter it reads changed.
    EnvSymbols,
    /// An environment added (job- or step-scoped).
    EnvAdd,
    /// An environment removed.
    EnvRemove,
    /// Environments reordered (they are entered in list order).
    EnvReorder,
    /// A step's `name` or `description`.
    StepMetadata,
    /// A step's script.
    StepScript,
    /// A step's host requirements.
    StepHostRequirements,
    /// A step's dependencies.
    StepDependencies,
    /// A step's resolved symbol table: a parameter it reads changed.
    StepSymbols,
    /// A step's parameter space changed but its tasks and their order did
    /// not (e.g. `1-2` rewritten as `[1, 2]`): only the definition is written.
    StepParameterSpace,
    /// A step added.
    StepAdd,
    /// A step removed.
    StepRemove,
    /// Steps reordered.
    StepReorder,
    /// Tasks appended to a step.
    TaskAppend,
    /// Tasks removed from a step.
    TaskRemove,
    /// Every task of a step replaced.
    TaskReplace,
}

impl MutationKind {
    /// Every kind, in declaration order.
    pub const ALL: &'static [MutationKind] = &[
        Self::JobMetadata,
        Self::JobExtensions,
        Self::ParameterValue,
        Self::ParameterAdd,
        Self::ParameterRemove,
        Self::EnvVariables,
        Self::EnvScript,
        Self::EnvMetadata,
        Self::EnvSymbols,
        Self::EnvAdd,
        Self::EnvRemove,
        Self::EnvReorder,
        Self::StepMetadata,
        Self::StepScript,
        Self::StepHostRequirements,
        Self::StepDependencies,
        Self::StepSymbols,
        Self::StepParameterSpace,
        Self::StepAdd,
        Self::StepRemove,
        Self::StepReorder,
        Self::TaskAppend,
        Self::TaskRemove,
        Self::TaskReplace,
    ];

    /// The kind's stable name: its variant name, such as `TaskAppend`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::JobMetadata => "JobMetadata",
            Self::JobExtensions => "JobExtensions",
            Self::ParameterValue => "ParameterValue",
            Self::ParameterAdd => "ParameterAdd",
            Self::ParameterRemove => "ParameterRemove",
            Self::EnvVariables => "EnvVariables",
            Self::EnvScript => "EnvScript",
            Self::EnvMetadata => "EnvMetadata",
            Self::EnvSymbols => "EnvSymbols",
            Self::EnvAdd => "EnvAdd",
            Self::EnvRemove => "EnvRemove",
            Self::EnvReorder => "EnvReorder",
            Self::StepMetadata => "StepMetadata",
            Self::StepScript => "StepScript",
            Self::StepHostRequirements => "StepHostRequirements",
            Self::StepDependencies => "StepDependencies",
            Self::StepSymbols => "StepSymbols",
            Self::StepParameterSpace => "StepParameterSpace",
            Self::StepAdd => "StepAdd",
            Self::StepRemove => "StepRemove",
            Self::StepReorder => "StepReorder",
            Self::TaskAppend => "TaskAppend",
            Self::TaskRemove => "TaskRemove",
            Self::TaskReplace => "TaskReplace",
        }
    }
}

impl fmt::Display for MutationKind {
    /// The [`name`](Self::name).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for MutationKind {
    type Err = UnknownMutationKind;

    /// The kind with this exact [`name`](Self::name); case-sensitive.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|k| k.name() == s)
            .ok_or_else(|| UnknownMutationKind(s.to_string()))
    }
}

/// A name that is not a [`MutationKind`]. Holds the name as given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownMutationKind(pub String);

impl fmt::Display for UnknownMutationKind {
    /// `unknown mutation kind 'Foo'`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown mutation kind '{}'", self.0)
    }
}

impl std::error::Error for UnknownMutationKind {}

/// What an op does.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Action<'a> {
    /// Job-level fields (`name`, `description`, `extensions`, orders).
    Job(Op<'a, Job>),
    /// A job parameter.
    Parameter(Op<'a, JobParameter>),
    /// A job environment and its index: the new list index for `Add` and
    /// `Modify`, the old one (informational) for `Remove`. A store matches
    /// `Modify` and `Remove` by name. Ops arrive as: any `Job` reorder of the
    /// shared environments, removes, then adds and modifies in ascending new
    /// index, so inserting each `Add` at its index rebuilds the new order.
    /// Step environments travel with their step.
    Environment(usize, Op<'a, Environment>),
    /// A step, including its step environments.
    Step(StepRef<'a>, Op<'a, Step>),
    /// A step's tasks: the step, in `new.steps`, and its [`TaskOp`].
    Tasks(StepRef<'a>, TaskOp<'a>),
}

/// One op in an [`ApplyPlan`].
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedOp<'a> {
    /// Every kind this op carries; a policy must allow all of them.
    pub kinds: BTreeSet<MutationKind>,
    /// What to do.
    pub action: Action<'a>,
}

fn op_name<'a, T>(op: &Op<'a, T>, name: impl Fn(&'a T) -> &'a String) -> String {
    match *op {
        Op::Add { new } | Op::Modify { new, .. } => name(new).clone(),
        Op::Remove { old } => name(old).clone(),
    }
}

impl PlannedOp<'_> {
    /// The object this op writes.
    #[must_use]
    pub fn target(&self) -> OpTarget {
        match &self.action {
            Action::Job(_) => OpTarget::Job,
            Action::Parameter(o) => OpTarget::Parameter(op_name(o, |p| &p.name)),
            Action::Environment(_, o) => OpTarget::Environment(op_name(o, |e| &e.name)),
            Action::Step(s, _) | Action::Tasks(s, _) => OpTarget::Step {
                name: s.name.to_string(),
                index: s.index,
            },
        }
    }

    /// The op as plain data, as [`JobApplier::check`] might hand it to a
    /// host: `{"kinds": [<kind name>, …], "action": "job" | "parameter" |
    /// "environment" | "step" | "tasks", …}`. `environment` adds `"index"`
    /// and `"op"`; `step` adds `"step"` (a [`StepRef`]) and `"op"`; `tasks`
    /// adds `"step"` and `"tasks"` ([`TaskOp::as_document`], which holds the
    /// step's new parameter space). `"op"` is [`Op::as_document`].
    pub fn as_document(&self) -> impl Serialize + '_ {
        PlannedOpDocument(self)
    }

    /// [`as_document`](Self::as_document) as a `serde_json::Value`.
    pub fn to_value(&self) -> Result<serde_json::Value, ModelError> {
        to_json_value(self.as_document(), "planned op")
    }
}

struct PlannedOpDocument<'a, 'b>(&'a PlannedOp<'b>);

impl Serialize for PlannedOpDocument<'_, '_> {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut m = ser.serialize_map(None)?;
        let kinds: Vec<&str> = self.0.kinds.iter().map(|k| k.name()).collect();
        m.serialize_entry("kinds", &kinds)?;
        match &self.0.action {
            Action::Job(o) => {
                m.serialize_entry("action", "job")?;
                m.serialize_entry("op", &o.as_document())?;
            }
            Action::Parameter(o) => {
                m.serialize_entry("action", "parameter")?;
                m.serialize_entry("op", &o.as_document())?;
            }
            Action::Environment(i, o) => {
                m.serialize_entry("action", "environment")?;
                m.serialize_entry("index", i)?;
                m.serialize_entry("op", &o.as_document())?;
            }
            Action::Step(s, o) => {
                m.serialize_entry("action", "step")?;
                m.serialize_entry("step", s)?;
                m.serialize_entry("op", &o.as_document())?;
            }
            Action::Tasks(step, op) => {
                m.serialize_entry("action", "tasks")?;
                m.serialize_entry("step", step)?;
                m.serialize_entry("tasks", &op.as_document())?;
            }
        }
        m.end()
    }
}

/// An ordered list of ops. Plain data: log it, dry-run it, or resume it.
#[derive(Debug, Clone, PartialEq)]
pub struct ApplyPlan<'a> {
    /// The ops, in the order they must run.
    pub ops: Vec<PlannedOp<'a>>,
    /// The full diff the plan was built from. `Op::Modify::node` indexes it.
    pub diff: DiffTree,
}

impl ApplyPlan<'_> {
    /// Every kind any op carries.
    #[must_use]
    pub fn kinds(&self) -> BTreeSet<MutationKind> {
        self.ops
            .iter()
            .flat_map(|o| o.kinds.iter().copied())
            .collect()
    }
}

/// Why a [`MutationPolicy`] refused an op: a kind that is not allowed yet,
/// whatever the store holds. A store's own veto, a kind that is not allowed
/// in the store's current state, is [`ApplyError::Vetoed`] instead. Built
/// with [`Refusal::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Refusal {
    /// The refused kind.
    pub kind: MutationKind,
    /// What the refused op writes.
    pub target: OpTarget,
    /// Why, as a sentence a host can show after the target.
    pub reason: String,
}

impl Refusal {
    /// A refusal of `kind` on `op`'s target.
    #[must_use]
    pub fn new(op: &PlannedOp<'_>, kind: MutationKind, reason: impl Into<String>) -> Self {
        Self {
            kind,
            target: op.target(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for Refusal {
    /// `{target}: {reason}`, e.g. `step Render: TaskReplace is not allowed`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.target, self.reason)
    }
}

impl std::error::Error for Refusal {}

/// Decides which kinds of mutation may run.
pub trait MutationPolicy {
    /// Ok to allow `op`, or the first kind it refuses.
    fn allows(&self, op: &PlannedOp<'_>) -> Result<(), Refusal>;
}

/// A policy that allows exactly a fixed set of kinds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AllowList(pub HashSet<MutationKind>);

impl AllowList {
    /// Allow these kinds.
    pub fn of(kinds: impl IntoIterator<Item = MutationKind>) -> Self {
        Self(kinds.into_iter().collect())
    }

    /// Allow the kinds with these [names](MutationKind::name), such as a
    /// host's configuration lists. The first name that is not a kind is the
    /// error.
    pub fn from_names<'a>(
        names: impl IntoIterator<Item = &'a str>,
    ) -> Result<Self, UnknownMutationKind> {
        names
            .into_iter()
            .map(MutationKind::from_str)
            .collect::<Result<_, _>>()
            .map(Self)
    }
}

impl MutationPolicy for AllowList {
    /// Refuses the first of the op's kinds, in [`MutationKind`] order, that
    /// is not in the list, with the reason `{kind} is not allowed`.
    fn allows(&self, op: &PlannedOp<'_>) -> Result<(), Refusal> {
        match op.kinds.iter().find(|k| !self.0.contains(k)) {
            Some(&kind) => Err(Refusal::new(op, kind, format!("{kind} is not allowed"))),
            None => Ok(()),
        }
    }
}

/// A store that applies ops. Every method receives ops in plan order.
///
/// The trait is object safe: `&mut dyn JobApplier<Error = E>` works with
/// [`validate`], [`execute`] and [`apply`], so an adaptor that forwards each
/// call to another language can be one concrete type behind a `dyn`.
pub trait JobApplier {
    /// The store's error. [`ApplyError`] needs only `Debug` and `Display`
    /// from it, so `Box<dyn std::error::Error + Send + Sync>` works.
    type Error;

    /// Store-side veto, called for every op before any write. Default: accept.
    fn check(&mut self, _op: &PlannedOp<'_>) -> Result<(), Self::Error> {
        Ok(())
    }
    /// Job-level fields changed.
    fn job(&mut self, op: Op<'_, Job>) -> Result<(), Self::Error>;
    /// A job parameter.
    fn parameter(&mut self, op: Op<'_, JobParameter>) -> Result<(), Self::Error>;
    /// A job environment at a list position (see [`Action::Environment`]).
    fn environment(&mut self, index: usize, op: Op<'_, Environment>) -> Result<(), Self::Error>;
    /// A step, including its step environments. `Modify` carries the whole
    /// new step; a step whose only change is its tasks gets no step op.
    fn step(&mut self, step: StepRef<'_>, op: Op<'_, Step>) -> Result<(), Self::Error>;
    /// A step's tasks. `op.change` names tasks by their parameter values,
    /// never by position; [`TaskOp::added_tasks`] gives the tasks it adds.
    /// The host maps them to its own task ids. A step whose only change is
    /// its tasks gets no step op, so the host writes `op.space` as the
    /// step's parameter space here.
    fn tasks(&mut self, step: StepRef<'_>, op: &TaskOp<'_>) -> Result<(), Self::Error>;
    /// Before the first write.
    fn begin(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
    /// After the last write.
    fn commit(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// How applying failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum ApplyError<E> {
    /// The policy refused op `op`. Nothing was written.
    Refused {
        /// Index of the refused op in the plan.
        op: usize,
        /// The refusal.
        refusal: Refusal,
    },
    /// The store vetoed op `op` in [`JobApplier::check`]. Nothing was
    /// written.
    Vetoed {
        /// Index of the op in the plan.
        op: usize,
        /// What the op writes.
        target: OpTarget,
        /// The store's error.
        error: E,
    },
    /// A write failed at op `op` (or in `begin`, reported at `resume_from`,
    /// or in `commit`, reported at the plan's length). Ops before `op` were
    /// applied.
    Store {
        /// Index of the op.
        op: usize,
        /// The store's error.
        error: E,
    },
    /// Planning failed, e.g. a step graph has a cycle.
    Model(ModelError),
}

impl<E: fmt::Display> fmt::Display for ApplyError<E> {
    /// `Refused` is the [`Refusal`]; `Vetoed` is `{target}: {error}`;
    /// `Store` is `write {op} failed: {error}`; `Model` is the model error.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused { refusal, .. } => refusal.fmt(f),
            Self::Vetoed { target, error, .. } => write!(f, "{target}: {error}"),
            Self::Store { op, error } => write!(f, "write {op} failed: {error}"),
            Self::Model(e) => e.fmt(f),
        }
    }
}

/// `source` is the [`Refusal`] or the [`ModelError`]. A store error is in
/// the message only: `E` need not implement `Error` (a boxed
/// `dyn Error` does not), so it cannot be returned as a source.
impl<E: fmt::Debug + fmt::Display> std::error::Error for ApplyError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Refused { refusal, .. } => Some(refusal),
            Self::Model(e) => Some(e),
            Self::Vetoed { .. } | Self::Store { .. } => None,
        }
    }
}

/// How far execution got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyReport {
    /// Ops applied, counting any skipped by `resume_from`. Pass it back as
    /// `resume_from` to continue after a failure.
    pub applied: usize,
}

/// Plan, validate and execute.
pub fn apply<A: JobApplier + ?Sized>(
    old: &Job,
    new: &Job,
    policy: &dyn MutationPolicy,
    store: &mut A,
) -> Result<ApplyReport, ApplyError<A::Error>> {
    let plan = plan(old, new).map_err(ApplyError::Model)?;
    validate(&plan, policy, store)?;
    execute(&plan, store, 0)
}

/// Check every op against the policy, then the store, before any write.
pub fn validate<A: JobApplier + ?Sized>(
    plan: &ApplyPlan<'_>,
    policy: &dyn MutationPolicy,
    store: &mut A,
) -> Result<(), ApplyError<A::Error>> {
    for (i, op) in plan.ops.iter().enumerate() {
        policy
            .allows(op)
            .map_err(|refusal| ApplyError::Refused { op: i, refusal })?;
    }
    for (i, op) in plan.ops.iter().enumerate() {
        store.check(op).map_err(|error| ApplyError::Vetoed {
            op: i,
            target: op.target(),
            error,
        })?;
    }
    Ok(())
}

/// Run the ops from `resume_from` on, in order. Does not validate.
pub fn execute<A: JobApplier + ?Sized>(
    plan: &ApplyPlan<'_>,
    store: &mut A,
    resume_from: usize,
) -> Result<ApplyReport, ApplyError<A::Error>> {
    let wrap = |op: usize| move |error| ApplyError::Store { op, error };
    store.begin().map_err(wrap(resume_from))?;
    for (i, op) in plan.ops.iter().enumerate().skip(resume_from) {
        match &op.action {
            Action::Job(o) => store.job(*o),
            Action::Parameter(o) => store.parameter(*o),
            Action::Environment(i, o) => store.environment(*i, *o),
            Action::Step(s, o) => store.step(*s, *o),
            Action::Tasks(step, op) => store.tasks(*step, op),
        }
        .map_err(wrap(i))?;
    }
    store.commit().map_err(wrap(plan.ops.len()))?;
    Ok(ApplyReport {
        applied: plan.ops.len(),
    })
}

// ─── planning ───────────────────────────────────────────────────────

fn op<'a, T>(
    n: &DiffNode,
    id: DiffNodeId,
    old: Option<&'a T>,
    new: Option<&'a T>,
) -> Option<Op<'a, T>> {
    match (n.status, old, new) {
        (DiffStatus::Added, _, Some(new)) => Some(Op::Add { new }),
        (DiffStatus::Removed, Some(old), _) => Some(Op::Remove { old }),
        (DiffStatus::Modified, Some(old), Some(new)) => Some(Op::Modify { old, new, node: id }),
        _ => None,
    }
}

fn kinds(k: impl IntoIterator<Item = MutationKind>) -> BTreeSet<MutationKind> {
    k.into_iter().collect()
}

/// Kinds of a modified environment, from a fresh full diff of the pair.
fn env_kinds(old: &Environment, new: &Environment) -> BTreeSet<MutationKind> {
    let t = diff_environments(old, new, &DiffOptions::default());
    let mut k = BTreeSet::new();
    for f in &t.root().field_changes {
        k.insert(match f.field.as_str() {
            "resolvedSymTab" => MutationKind::EnvSymbols,
            "name" | "description" => MutationKind::EnvMetadata,
            _ => MutationKind::EnvVariables,
        });
    }
    if t.child(0, "script")
        .is_some_and(|c| t.nodes()[c].is_changed())
    {
        k.insert(MutationKind::EnvScript);
    }
    k
}

/// Kinds of a modified step other than its tasks, from a fresh full diff,
/// and whether its parameter space changed.
fn step_kinds(old: &Step, new: &Step) -> (BTreeSet<MutationKind>, bool) {
    let t = diff_steps(old, new, &DiffOptions::default());
    let mut k = BTreeSet::new();
    for f in &t.root().field_changes {
        k.insert(match f.field.as_str() {
            "resolvedSymTab" => MutationKind::StepSymbols,
            "dependencies" | "dependencies.order" => MutationKind::StepDependencies,
            "stepEnvironments" | "stepEnvironments.order" => MutationKind::EnvReorder,
            _ => MutationKind::StepMetadata,
        });
    }
    let mut space_changed = false;
    for &c in &t.root().children {
        let n = &t.nodes()[c];
        if !n.is_changed() {
            continue;
        }
        match n.kind {
            DiffNodeKind::StepScript => {
                k.insert(MutationKind::StepScript);
            }
            DiffNodeKind::HostRequirements => {
                k.insert(MutationKind::StepHostRequirements);
            }
            DiffNodeKind::ParameterSpace => space_changed = true,
            DiffNodeKind::Environment => match n.status {
                DiffStatus::Added => {
                    k.insert(MutationKind::EnvAdd);
                }
                DiffStatus::Removed => {
                    k.insert(MutationKind::EnvRemove);
                }
                _ => {
                    let (o, w) = (
                        n.old_index
                            .and_then(|i| old.step_environments.as_ref()?.get(i)),
                        n.new_index
                            .and_then(|j| new.step_environments.as_ref()?.get(j)),
                    );
                    if let (Some(o), Some(w)) = (o, w) {
                        k.extend(env_kinds(o, w));
                    }
                }
            },
            _ => {}
        }
    }
    (k, space_changed)
}

fn task_kinds(change: &TaskChange) -> BTreeSet<MutationKind> {
    match change {
        TaskChange::Boxes(d) => {
            let mut k = BTreeSet::new();
            if !d.added.is_empty() {
                k.insert(MutationKind::TaskAppend);
            }
            if !d.removed.is_empty() {
                k.insert(MutationKind::TaskRemove);
            }
            k
        }
        TaskChange::All => kinds([MutationKind::TaskAppend]),
        TaskChange::Replace => kinds([MutationKind::TaskReplace]),
    }
}

/// Environments of one collection node's children, as ops.
fn env_ops<'a>(
    out: &mut Vec<PlannedOp<'a>>,
    diff: &DiffTree,
    parent: DiffNodeId,
    collection: &str,
    old: &'a [Environment],
    new: &'a [Environment],
) {
    let prefix = format!("{collection}[");
    let Some(p) = diff.node(parent) else { return };
    let mut removes = Vec::new();
    let mut writes = Vec::new();
    for &c in &p.children {
        let n = &diff.nodes()[c];
        if !n.segment.starts_with(&prefix) || !n.is_changed() {
            continue;
        }
        let (o, w) = (
            n.old_index.and_then(|i| old.get(i)),
            n.new_index.and_then(|j| new.get(j)),
        );
        let Some(action) = op(n, c, o, w) else {
            continue;
        };
        let (k, index) = match action {
            Op::Add { .. } => (kinds([MutationKind::EnvAdd]), n.new_index),
            Op::Remove { .. } => (kinds([MutationKind::EnvRemove]), n.old_index),
            Op::Modify { old, new, .. } => (env_kinds(old, new), n.new_index),
        };
        let Some(index) = index else { continue };
        let op = PlannedOp {
            kinds: k,
            action: Action::Environment(index, action),
        };
        if matches!(action, Op::Remove { .. }) {
            removes.push((index, op));
        } else {
            writes.push((index, op));
        }
    }
    // Removes (matched by name), then writes in ascending new index.
    writes.sort_by_key(|w| w.0);
    out.extend(removes.into_iter().chain(writes).map(|(_, op)| op));
}

/// Refuse a job that repeats a step, job-environment or step-environment
/// name. Template validation rejects these, so only a hand-built `Job` has
/// one; the diff reports the copies as one `<collection>.duplicates` field
/// change, which has no op and no kind of its own to gate.
fn check_unique_names(job: &Job) -> Result<(), ModelError> {
    use crate::error::{PathElement, ValidationErrors};
    use std::collections::HashSet;
    let mut errors = ValidationErrors::default();
    let mut repeats = |path: &[PathElement], names: &mut dyn Iterator<Item = &String>| {
        let mut seen = HashSet::new();
        for (i, n) in names.enumerate() {
            if !seen.insert(n) {
                let mut at = path.to_vec();
                at.extend([PathElement::Index(i), PathElement::Field("name".into())]);
                errors.add(
                    &at,
                    format!("repeated name '{n}': job::apply needs unique names"),
                );
            }
        }
    };
    let field = |f: &str| PathElement::Field(f.into());
    repeats(&[field("steps")], &mut job.steps.iter().map(|s| &s.name));
    repeats(
        &[field("jobEnvironments")],
        &mut job.job_environments.iter().flatten().map(|e| &e.name),
    );
    for (i, s) in job.steps.iter().enumerate() {
        repeats(
            &[
                field("steps"),
                PathElement::Index(i),
                field("stepEnvironments"),
            ],
            &mut s.step_environments.iter().flatten().map(|e| &e.name),
        );
    }
    errors.into_result("Job")
}

/// Build the ordered plan from the stored job `old` to the requested `new`.
///
/// The plan diffs the jobs itself, always with the default [`DiffOptions`]:
/// a caller's tree built with `max_depth` or `with_resolved_symtab(false)`
/// can mark a changed step `Unchanged`, and the plan must not miss writes.
/// The tree is kept in [`ApplyPlan::diff`]; `Op::Modify::node` indexes it.
pub fn plan<'a>(old: &'a Job, new: &'a Job) -> Result<ApplyPlan<'a>, ModelError> {
    check_unique_names(old)?;
    check_unique_names(new)?;
    let diff = diff_jobs(old, new, &DiffOptions::default());
    let mut ops = Vec::new();
    if !diff.is_changed() {
        return Ok(ApplyPlan { ops, diff });
    }
    let root = diff.root();

    // 1. Job-level fields.
    let mut jk = BTreeSet::new();
    for f in &root.field_changes {
        jk.insert(match f.field.as_str() {
            "extensions" => MutationKind::JobExtensions,
            "steps.order" => MutationKind::StepReorder,
            "jobEnvironments" | "jobEnvironments.order" => MutationKind::EnvReorder,
            _ => MutationKind::JobMetadata,
        });
    }
    if !jk.is_empty() {
        ops.push(PlannedOp {
            kinds: jk,
            action: Action::Job(Op::Modify { old, new, node: 0 }),
        });
    }

    // 2. Parameters: the old job's, in name order, then the ones only the new
    // job has, in name order. The diff orders parameter nodes this way, so the
    // order does not depend on how either job's map was filled, and a re-plan
    // resumes at the same op.
    for &c in &root.children {
        let n = &diff.nodes()[c];
        if n.kind != DiffNodeKind::JobParameter || !n.is_changed() {
            continue;
        }
        let key = n.key.as_deref().unwrap_or_default();
        if let Some(action) = op(n, c, old.parameters.get(key), new.parameters.get(key)) {
            let k = match action {
                Op::Add { .. } => MutationKind::ParameterAdd,
                Op::Remove { .. } => MutationKind::ParameterRemove,
                Op::Modify { .. } => MutationKind::ParameterValue,
            };
            ops.push(PlannedOp {
                kinds: kinds([k]),
                action: Action::Parameter(action),
            });
        }
    }

    // 3. Job environments.
    let empty: &[Environment] = &[];
    env_ops(
        &mut ops,
        &diff,
        0,
        "jobEnvironments",
        old.job_environments.as_deref().unwrap_or(empty),
        new.job_environments.as_deref().unwrap_or(empty),
    );

    // Step nodes by step_index.
    let mut by_old: Vec<Option<DiffNodeId>> = vec![None; old.steps.len()];
    let mut by_new: Vec<Option<DiffNodeId>> = vec![None; new.steps.len()];
    for &c in &root.children {
        let n = &diff.nodes()[c];
        let (Some(i), DiffNodeKind::Step) = (n.step_index, n.kind) else {
            continue;
        };
        let slot = if n.status == DiffStatus::Removed {
            by_old.get_mut(i)
        } else {
            by_new.get_mut(i)
        };
        if let Some(s) = slot {
            *s = Some(c);
        }
    }

    // 4. Adds and modifies, origins first.
    for j in StepDependencyGraph::new(new)?.topo_sorted()? {
        let Some(id) = by_new.get(j).copied().flatten() else {
            continue;
        };
        let n = &diff.nodes()[id];
        let s = &new.steps[j];
        let at = StepRef {
            name: &s.name,
            index: j,
        };
        match n.status {
            DiffStatus::Added => {
                let mut k = kinds([MutationKind::StepAdd]);
                if s.step_environments.as_ref().is_some_and(|e| !e.is_empty()) {
                    k.insert(MutationKind::EnvAdd);
                }
                ops.push(PlannedOp {
                    kinds: k,
                    action: Action::Step(at, Op::Add { new: s }),
                });
                ops.push(PlannedOp {
                    kinds: task_kinds(&TaskChange::All),
                    action: Action::Tasks(
                        at,
                        TaskOp {
                            space: s.parameter_space.as_ref(),
                            change: TaskChange::All,
                        },
                    ),
                });
            }
            DiffStatus::Modified => {
                let Some(o) = n.old_index.and_then(|i| old.steps.get(i)) else {
                    continue;
                };
                let (mut k, space_changed) = step_kinds(o, s);
                let change = if space_changed {
                    match task_delta(o.parameter_space.as_ref(), s.parameter_space.as_ref()) {
                        TaskDelta::Unchanged => None,
                        TaskDelta::Product(d) if d.added.is_empty() && d.removed.is_empty() => None,
                        TaskDelta::Product(d) => Some(TaskChange::Boxes(d)),
                        TaskDelta::Fallback { .. } => Some(TaskChange::Replace),
                    }
                } else {
                    None
                };
                // The space changed but no task did: the step write must carry
                // the new definition, since no Tasks op will.
                if space_changed && change.is_none() {
                    k.insert(MutationKind::StepParameterSpace);
                }
                if !k.is_empty() {
                    ops.push(PlannedOp {
                        kinds: k,
                        action: Action::Step(
                            at,
                            Op::Modify {
                                old: o,
                                new: s,
                                node: id,
                            },
                        ),
                    });
                }
                if let Some(change) = change {
                    ops.push(PlannedOp {
                        kinds: task_kinds(&change),
                        action: Action::Tasks(
                            at,
                            TaskOp {
                                space: s.parameter_space.as_ref(),
                                change,
                            },
                        ),
                    });
                }
            }
            _ => {}
        }
    }

    // 5. Removes, dependents first. No remaining step can depend on these.
    for i in StepDependencyGraph::new(old)?
        .topo_sorted()?
        .into_iter()
        .rev()
    {
        if by_old.get(i).copied().flatten().is_some() {
            let mut k = kinds([MutationKind::StepRemove]);
            if old.steps[i]
                .step_environments
                .as_ref()
                .is_some_and(|e| !e.is_empty())
            {
                k.insert(MutationKind::EnvRemove);
            }
            let gone = &old.steps[i];
            ops.push(PlannedOp {
                kinds: k,
                action: Action::Step(
                    StepRef {
                        name: &gone.name,
                        index: i,
                    },
                    Op::Remove { old: gone },
                ),
            });
        }
    }
    Ok(ApplyPlan { ops, diff })
}
