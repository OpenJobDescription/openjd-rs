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

use std::collections::{BTreeSet, HashSet};

use crate::error::ModelError;
use crate::job::diff::{
    diff_environments, diff_jobs, diff_steps, DiffNode, DiffNodeId, DiffNodeKind, DiffOptions,
    DiffStatus, DiffTree,
};
use crate::job::task_delta::{task_delta, ProductDelta, TaskDelta};
use crate::job::{Environment, Job, JobParameter, Step, StepParameterSpace};
use crate::step_dependency_graph::StepDependencyGraph;

/// Index of a step: into `new.steps`, or into `old.steps` for a removal.
pub type StepIndex = usize;

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

/// How a step's tasks change.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskChange {
    /// Product space: keep `kept`, append `added`, delete `removed`.
    Boxes(Box<ProductDelta>),
    /// A newly added step: all tasks of its space, store indices from 0.
    All,
    /// No per-task mapping (association, chunking, a parameter added or
    /// removed, ...): replace every task.
    Replace,
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
    Step(StepIndex, Op<'a, Step>),
    /// A step's tasks. `space` is the step's new parameter space.
    Tasks {
        /// The step, in `new.steps`.
        step: StepIndex,
        /// The new parameter space; `None` is one task with no parameters.
        space: Option<&'a StepParameterSpace>,
        /// The change.
        change: TaskChange,
    },
}

/// One op in an [`ApplyPlan`].
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedOp<'a> {
    /// Every kind this op carries; a policy must allow all of them.
    pub kinds: BTreeSet<MutationKind>,
    /// What to do.
    pub action: Action<'a>,
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

/// Why a [`MutationPolicy`] refused an op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The refused kind.
    pub kind: MutationKind,
    /// Why.
    pub reason: String,
}

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
}

impl MutationPolicy for AllowList {
    fn allows(&self, op: &PlannedOp<'_>) -> Result<(), Refusal> {
        match op.kinds.iter().find(|k| !self.0.contains(k)) {
            Some(&kind) => Err(Refusal {
                kind,
                reason: format!("{kind:?} is not allowed"),
            }),
            None => Ok(()),
        }
    }
}

/// A store that applies ops. Every method receives ops in plan order.
pub trait JobApplier {
    /// The store's error.
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
    fn step(&mut self, step: StepIndex, op: Op<'_, Step>) -> Result<(), Self::Error>;
    /// A step's tasks.
    fn tasks(
        &mut self,
        step: StepIndex,
        space: Option<&StepParameterSpace>,
        change: &TaskChange,
    ) -> Result<(), Self::Error>;
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
pub enum ApplyError<E> {
    /// The policy refused op `op`. Nothing was written.
    Refused {
        /// Index of the refused op in the plan.
        op: usize,
        /// The refusal.
        refusal: Refusal,
    },
    /// The store vetoed in `check` (nothing written) or failed a write at op
    /// `op` (ops before it were applied).
    Store {
        /// Index of the op.
        op: usize,
        /// The store's error.
        error: E,
    },
    /// Planning failed, e.g. a step graph has a cycle.
    Model(ModelError),
}

/// How far execution got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyReport {
    /// Ops applied, counting any skipped by `resume_from`. Pass it back as
    /// `resume_from` to continue after a failure.
    pub applied: usize,
}

/// Plan, validate and execute.
pub fn apply<A: JobApplier>(
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
pub fn validate<A: JobApplier>(
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
        store
            .check(op)
            .map_err(|error| ApplyError::Store { op: i, error })?;
    }
    Ok(())
}

/// Run the ops from `resume_from` on, in order. Does not validate.
pub fn execute<A: JobApplier>(
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
            Action::Step(j, o) => store.step(*j, *o),
            Action::Tasks {
                step,
                space,
                change,
            } => store.tasks(*step, *space, change),
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

    // 2. Parameters.
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
        match n.status {
            DiffStatus::Added => {
                let mut k = kinds([MutationKind::StepAdd]);
                if s.step_environments.as_ref().is_some_and(|e| !e.is_empty()) {
                    k.insert(MutationKind::EnvAdd);
                }
                ops.push(PlannedOp {
                    kinds: k,
                    action: Action::Step(j, Op::Add { new: s }),
                });
                ops.push(PlannedOp {
                    kinds: task_kinds(&TaskChange::All),
                    action: Action::Tasks {
                        step: j,
                        space: s.parameter_space.as_ref(),
                        change: TaskChange::All,
                    },
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
                            j,
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
                        action: Action::Tasks {
                            step: j,
                            space: s.parameter_space.as_ref(),
                            change,
                        },
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
            ops.push(PlannedOp {
                kinds: k,
                action: Action::Step(i, Op::Remove { old: &old.steps[i] }),
            });
        }
    }
    Ok(ApplyPlan { ops, diff })
}
