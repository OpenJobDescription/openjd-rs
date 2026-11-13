// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Structural diff of instantiated jobs, steps and environments.
//!
//! **Unstable.** This API may change or be removed in any release without
//! notice. Don't depend on it outside openjd-rs.
//!
//! The diff is built breadth-first into an arena ([`DiffTree`]). Each node's
//! [`DiffStatus`] is decided with one `PartialEq` comparison of the whole
//! subtree *before* the node is expanded, so a parent always knows whether
//! anything below it changed: the root answers "is there any delta?", each
//! step answers "is there a delta in this step?", and an
//! [`Unchanged`](DiffStatus::Unchanged) node is never descended into.
//! [`DiffOptions::with_max_depth`] stops the walk at a level, which is the
//! early exit: depth 1 reports the status of every step and job environment
//! without expanding any of them.
//!
//! Named collections (`steps`, environments, parameters, embedded files) are
//! matched by name, not by position, so a reorder is reported once as an
//! `<collection>.order` field change rather than as every element changing.
//!
//! See `specs/model/job-diff.md`.

use std::borrow::Cow;
use std::collections::{BTreeSet, VecDeque};
use std::fmt;

use indexmap::IndexMap;
use serde::Serialize;

use crate::job::document;
use crate::job::task_delta::definitions_fix_task_order;

use crate::job::{
    Action, EmbeddedFile, Environment, EnvironmentActions, EnvironmentScript, HostRequirements,
    Job, JobParameter, Step, StepActions, StepParameterSpace, StepScript, TaskParameter,
};

/// Index of a node in a [`DiffTree`].
pub type DiffNodeId = usize;

/// What a [`DiffNode`] compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DiffNodeKind {
    /// A [`Job`].
    Job,
    /// A [`JobParameter`].
    JobParameter,
    /// An [`Environment`], at job or step scope.
    Environment,
    /// A [`Step`].
    Step,
    /// A [`StepScript`].
    StepScript,
    /// An [`EnvironmentScript`].
    EnvironmentScript,
    /// An [`Action`].
    Action,
    /// An [`EmbeddedFile`].
    EmbeddedFile,
    /// A [`StepParameterSpace`].
    ParameterSpace,
    /// A [`TaskParameter`] definition.
    TaskParameter,
    /// A [`HostRequirements`].
    HostRequirements,
}

/// How the two sides of a [`DiffNode`] relate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffStatus {
    /// Both sides are present and equal, including everything below.
    Unchanged,
    /// Only the new side is present.
    Added,
    /// Only the old side is present.
    Removed,
    /// Both sides are present and differ somewhere in the subtree.
    Modified,
}

/// A changed scalar field on a [`DiffNode`].
///
/// `before` and `after` are the field's JSON serialization, as the job's
/// document writes it (a parameter's `type` and `value` and a task parameter
/// `definition` in the host encoding); an absent optional field is `null`.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldChange {
    /// Field name, camelCase as serialized. Keyed entries read
    /// `variables.FOO` or `amounts[amount.worker.vcpu]`; a reorder of a
    /// named collection reads `<collection>.order`.
    pub field: String,
    /// The old value.
    pub before: serde_json::Value,
    /// The new value.
    pub after: serde_json::Value,
}

/// One node of a [`DiffTree`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DiffNode {
    /// What this node compares.
    pub kind: DiffNodeKind,
    /// Path segment from the parent, e.g. `steps[Render]` or `script`.
    /// Empty for the root.
    pub segment: String,
    /// How the two sides relate. Decided before expansion.
    pub status: DiffStatus,
    /// Distance from the root, which is depth 0.
    pub depth: usize,
    /// Parent node, `None` for the root.
    pub parent: Option<DiffNodeId>,
    /// Child nodes, in old-side order followed by new-only entries.
    /// Empty unless the node was expanded.
    pub children: Vec<DiffNodeId>,
    /// Scalar fields that differ on this node itself.
    pub field_changes: Vec<FieldChange>,
    /// Whether this node was expanded. A `Modified` node at
    /// [`DiffOptions::with_max_depth`] is not, so its children and field
    /// changes are unknown; diff the pair again to see them.
    pub expanded: bool,
    /// For an element of a named collection (`steps`, environments,
    /// parameters, embedded files, task parameter definitions): its name.
    /// This is the stable identity to map to a store's own id.
    pub key: Option<String>,
    /// For an element of a named collection: its position in the old side's
    /// collection, e.g. the index into the old `Job::steps`. Positions shift
    /// when elements are inserted or removed, so they are not an identity.
    pub old_index: Option<usize>,
    /// As [`old_index`](Self::old_index), on the new side.
    pub new_index: Option<usize>,
    /// Index of the step this node is in, or is: into the new job's `steps`
    /// when the step exists there, otherwise (a removed step) into the old
    /// job's. Set on step nodes and inherited by every node below them.
    /// `None` above step level and in [`diff_steps`] trees.
    pub step_index: Option<usize>,
}

impl DiffNode {
    /// Whether anything at or below this node differs.
    #[must_use]
    pub fn is_changed(&self) -> bool {
        self.status != DiffStatus::Unchanged
    }
}

/// Options for [`diff_jobs`], [`diff_steps`] and [`diff_environments`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DiffOptions {
    max_depth: Option<usize>,
    include_resolved_symtab: bool,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            max_depth: None,
            include_resolved_symtab: true,
        }
    }
}

impl DiffOptions {
    /// Do not expand nodes at `depth` or deeper. Their status is still
    /// reported. `0` reports only the root's status.
    #[must_use]
    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = Some(depth);
        self
    }

    /// Whether to compare `resolvedSymTab`. On by default, and it must stay
    /// on to see job parameter changes in a step: actions keep their format
    /// strings unresolved, and the table, filtered to the symbols the step or
    /// environment references, is the only place a parameter's value shows.
    #[must_use]
    pub fn with_resolved_symtab(mut self, include: bool) -> Self {
        self.include_resolved_symtab = include;
        self
    }
}

/// A diff, stored as an arena in breadth-first order. Node 0 is the root.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffTree {
    nodes: Vec<DiffNode>,
}

impl DiffTree {
    /// The root node.
    #[must_use]
    pub fn root(&self) -> &DiffNode {
        // The builder always pushes the root first and never removes nodes.
        &self.nodes[0]
    }

    /// Whether the two sides differ at all. O(1).
    #[must_use]
    pub fn is_changed(&self) -> bool {
        self.root().is_changed()
    }

    /// A node by id.
    #[must_use]
    pub fn node(&self, id: DiffNodeId) -> Option<&DiffNode> {
        self.nodes.get(id)
    }

    /// All nodes, in breadth-first order.
    #[must_use]
    pub fn nodes(&self) -> &[DiffNode] {
        &self.nodes
    }

    /// The child of `id` whose segment is `segment`.
    #[must_use]
    pub fn child(&self, id: DiffNodeId, segment: &str) -> Option<DiffNodeId> {
        self.nodes
            .get(id)?
            .children
            .iter()
            .copied()
            .find(|&c| self.nodes[c].segment == segment)
    }

    /// The node at a dotted path of segments, e.g.
    /// `steps[Render].script.onRun`. The empty path is the root.
    /// Segments must not themselves contain `.`; walk with [`child`](Self::child)
    /// for names that do.
    #[must_use]
    pub fn find(&self, path: &str) -> Option<DiffNodeId> {
        if path.is_empty() {
            return Some(0);
        }
        path.split('.').try_fold(0, |id, seg| self.child(id, seg))
    }

    /// The dotted path of `id` from the root.
    #[must_use]
    pub fn path(&self, id: DiffNodeId) -> String {
        let mut segments = Vec::new();
        let mut cur = self.nodes.get(id);
        while let Some(n) = cur {
            if !n.segment.is_empty() {
                segments.push(n.segment.as_str());
            }
            cur = n.parent.and_then(|p| self.nodes.get(p));
        }
        segments.reverse();
        segments.join(".")
    }

    /// Changed nodes, in breadth-first order.
    pub fn changed(&self) -> impl Iterator<Item = (DiffNodeId, &DiffNode)> {
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.is_changed())
    }

    fn fmt_node(&self, f: &mut fmt::Formatter<'_>, id: DiffNodeId) -> fmt::Result {
        let n = &self.nodes[id];
        let mark = match n.status {
            DiffStatus::Unchanged => return Ok(()),
            DiffStatus::Added => '+',
            DiffStatus::Removed => '-',
            DiffStatus::Modified => '~',
        };
        let indent = "  ".repeat(n.depth);
        let label = if n.segment.is_empty() {
            "<root>"
        } else {
            &n.segment
        };
        let more = if n.status == DiffStatus::Modified && !n.expanded {
            " (not expanded)"
        } else {
            ""
        };
        writeln!(f, "{indent}{mark} {label}{more}")?;
        for c in &n.field_changes {
            writeln!(f, "{indent}    {}: {} -> {}", c.field, c.before, c.after)?;
        }
        for &child in &n.children {
            self.fmt_node(f, child)?;
        }
        Ok(())
    }
}

/// Indented listing of the changed nodes and their field changes.
impl fmt::Display for DiffTree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_node(f, 0)
    }
}

/// Diff two jobs.
#[must_use]
pub fn diff_jobs(old: &Job, new: &Job, options: &DiffOptions) -> DiffTree {
    let (old, new) = if options.include_resolved_symtab {
        (Cow::Borrowed(old), Cow::Borrowed(new))
    } else {
        (Cow::Owned(strip_job(old)), Cow::Owned(strip_job(new)))
    };
    build(options, Subject::Job(&old), Subject::Job(&new))
}

/// Diff two steps, e.g. a pair left unexpanded by `with_max_depth`.
#[must_use]
pub fn diff_steps(old: &Step, new: &Step, options: &DiffOptions) -> DiffTree {
    let (old, new) = if options.include_resolved_symtab {
        (Cow::Borrowed(old), Cow::Borrowed(new))
    } else {
        (Cow::Owned(strip_step(old)), Cow::Owned(strip_step(new)))
    };
    build(options, Subject::Step(&old), Subject::Step(&new))
}

/// Diff two environments. For two environment templates, convert each with
/// [`convert_environment`](crate::convert_environment) first.
#[must_use]
pub fn diff_environments(old: &Environment, new: &Environment, options: &DiffOptions) -> DiffTree {
    let (old, new) = if options.include_resolved_symtab {
        (Cow::Borrowed(old), Cow::Borrowed(new))
    } else {
        (Cow::Owned(strip_env(old)), Cow::Owned(strip_env(new)))
    };
    build(
        options,
        Subject::Environment(&old),
        Subject::Environment(&new),
    )
}

// ─── symtab stripping ───────────────────────────────────────────────

fn strip_env(e: &Environment) -> Environment {
    Environment {
        resolved_symtab: None,
        ..e.clone()
    }
}

fn strip_step(s: &Step) -> Step {
    Step {
        resolved_symtab: None,
        step_environments: s
            .step_environments
            .as_ref()
            .map(|v| v.iter().map(strip_env).collect()),
        ..s.clone()
    }
}

fn strip_job(j: &Job) -> Job {
    Job {
        steps: j.steps.iter().map(strip_step).collect(),
        job_environments: j
            .job_environments
            .as_ref()
            .map(|v| v.iter().map(strip_env).collect()),
        ..j.clone()
    }
}

// ─── BFS builder ────────────────────────────────────────────────────

/// A borrowed model value that can be one side of a node.
#[derive(Clone, Copy)]
enum Subject<'a> {
    Job(&'a Job),
    JobParameter(&'a JobParameter),
    Environment(&'a Environment),
    Step(&'a Step),
    StepScript(&'a StepScript),
    EnvironmentScript(&'a EnvironmentScript),
    Action(&'a Action),
    EmbeddedFile(&'a EmbeddedFile),
    ParameterSpace(&'a StepParameterSpace),
    TaskParameter(&'a TaskParameter),
    HostRequirements(&'a HostRequirements),
}

impl Subject<'_> {
    fn kind(self) -> DiffNodeKind {
        match self {
            Self::Job(_) => DiffNodeKind::Job,
            Self::JobParameter(_) => DiffNodeKind::JobParameter,
            Self::Environment(_) => DiffNodeKind::Environment,
            Self::Step(_) => DiffNodeKind::Step,
            Self::StepScript(_) => DiffNodeKind::StepScript,
            Self::EnvironmentScript(_) => DiffNodeKind::EnvironmentScript,
            Self::Action(_) => DiffNodeKind::Action,
            Self::EmbeddedFile(_) => DiffNodeKind::EmbeddedFile,
            Self::ParameterSpace(_) => DiffNodeKind::ParameterSpace,
            Self::TaskParameter(_) => DiffNodeKind::TaskParameter,
            Self::HostRequirements(_) => DiffNodeKind::HostRequirements,
        }
    }

    /// Whole-subtree equality. Sides of different kinds never occur: both
    /// sides of a node come from the same field.
    ///
    /// `==` on `IndexMap` ignores order, but task parameter definition order
    /// fixes task iteration order (and so task indices), so it is checked
    /// on top for every node that contains a parameter space.
    fn subtree_eq(self, other: Self) -> bool {
        match (self, other) {
            (Self::Job(a), Self::Job(b)) => {
                a == b
                    && a.steps.iter().zip(&b.steps).all(|(x, y)| {
                        space_order_eq(x.parameter_space.as_ref(), y.parameter_space.as_ref())
                    })
            }
            (Self::JobParameter(a), Self::JobParameter(b)) => a == b,
            (Self::Environment(a), Self::Environment(b)) => a == b,
            (Self::Step(a), Self::Step(b)) => {
                a == b && space_order_eq(a.parameter_space.as_ref(), b.parameter_space.as_ref())
            }
            (Self::ParameterSpace(a), Self::ParameterSpace(b)) => {
                a == b && space_order_eq(Some(a), Some(b))
            }
            (Self::StepScript(a), Self::StepScript(b)) => a == b,
            (Self::EnvironmentScript(a), Self::EnvironmentScript(b)) => a == b,
            (Self::Action(a), Self::Action(b)) => a == b,
            (Self::EmbeddedFile(a), Self::EmbeddedFile(b)) => a == b,
            (Self::TaskParameter(a), Self::TaskParameter(b)) => a == b,
            (Self::HostRequirements(a), Self::HostRequirements(b)) => a == b,
            _ => false,
        }
    }
}

/// Definition order decides task order only without an explicit
/// `combination` (one decides it instead, and `==` already compares it).
fn space_order_eq(a: Option<&StepParameterSpace>, b: Option<&StepParameterSpace>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) if definitions_fix_task_order(a) && definitions_fix_task_order(b) => a
            .task_parameter_definitions
            .keys()
            .eq(b.task_parameter_definitions.keys()),
        _ => true,
    }
}

struct Builder<'a> {
    nodes: Vec<DiffNode>,
    queue: VecDeque<(DiffNodeId, Subject<'a>, Subject<'a>)>,
    max_depth: Option<usize>,
}

fn build<'a>(options: &DiffOptions, old: Subject<'a>, new: Subject<'a>) -> DiffTree {
    let mut b = Builder {
        nodes: Vec::new(),
        queue: VecDeque::new(),
        max_depth: options.max_depth,
    };
    b.add(None, String::new(), Some(old), Some(new));
    while let Some((id, old, new)) = b.queue.pop_front() {
        b.expand(id, old, new);
    }
    DiffTree { nodes: b.nodes }
}

fn to_json<T: Serialize + ?Sized>(v: &T) -> serde_json::Value {
    serde_json::to_value(v)
        .unwrap_or_else(|e| serde_json::Value::String(format!("<not serializable: {e}>")))
}

impl<'a> Builder<'a> {
    /// Push a node for a pair, decide its status, and queue it for expansion
    /// when it is modified and above the depth limit.
    fn add(
        &mut self,
        parent: Option<DiffNodeId>,
        segment: String,
        old: Option<Subject<'a>>,
        new: Option<Subject<'a>>,
    ) -> Option<DiffNodeId> {
        let (status, kind) = match (old, new) {
            (Some(o), Some(n)) if o.subtree_eq(n) => (DiffStatus::Unchanged, o.kind()),
            (Some(o), Some(_)) => (DiffStatus::Modified, o.kind()),
            (Some(o), None) => (DiffStatus::Removed, o.kind()),
            (None, Some(n)) => (DiffStatus::Added, n.kind()),
            (None, None) => return None,
        };
        let depth = parent.map_or(0, |p| self.nodes[p].depth + 1);
        let step_index = parent.and_then(|p| self.nodes[p].step_index);
        let id = self.nodes.len();
        let expanded = status == DiffStatus::Modified && self.max_depth.is_none_or(|m| depth < m);
        self.nodes.push(DiffNode {
            kind,
            segment,
            status,
            depth,
            parent,
            children: Vec::new(),
            field_changes: Vec::new(),
            expanded,
            key: None,
            old_index: None,
            new_index: None,
            step_index,
        });
        if let Some(p) = parent {
            self.nodes[p].children.push(id);
        }
        if let (true, Some(o), Some(n)) = (expanded, old, new) {
            self.queue.push_back((id, o, n));
        }
        Some(id)
    }

    fn field<T: PartialEq + Serialize + ?Sized>(
        &mut self,
        id: DiffNodeId,
        name: &str,
        a: &T,
        b: &T,
    ) {
        if a != b {
            self.nodes[id].field_changes.push(FieldChange {
                field: name.to_string(),
                before: to_json(a),
                after: to_json(b),
            });
        }
    }

    /// [`field`](Self::field), with each side written by `write`.
    fn field_as<T: PartialEq + ?Sized>(
        &mut self,
        id: DiffNodeId,
        name: &str,
        a: &T,
        b: &T,
        write: impl Fn(&T) -> serde_json::Value,
    ) {
        if a != b {
            self.nodes[id].field_changes.push(FieldChange {
                field: name.to_string(),
                before: write(a),
                after: write(b),
            });
        }
    }

    /// Match two named collections by key and add a child per key. When
    /// `ordered`, a different relative order of the shared keys is recorded
    /// as `<collection>.order`.
    fn keyed<T: PartialEq + 'a>(
        &mut self,
        id: DiffNodeId,
        collection: &str,
        ordered: bool,
        a: impl IntoIterator<Item = (&'a str, &'a T)>,
        b: impl IntoIterator<Item = (&'a str, &'a T)>,
        wrap: fn(&'a T) -> Subject<'a>,
    ) {
        // First occurrence wins; names are unique in a validated job. A
        // hand-built job can repeat one, so later copies are reported as a
        // `<collection>.duplicates` field change rather than dropped silently.
        let mut a_map: IndexMap<&str, (usize, &T)> = IndexMap::new();
        let mut a_dups: Vec<(&str, &T)> = Vec::new();
        for (i, (k, v)) in a.into_iter().enumerate() {
            if a_map.contains_key(k) {
                a_dups.push((k, v));
            } else {
                a_map.insert(k, (i, v));
            }
        }
        let mut b_map: IndexMap<&str, (usize, &T)> = IndexMap::new();
        let mut b_dups: Vec<(&str, &T)> = Vec::new();
        for (i, (k, v)) in b.into_iter().enumerate() {
            if b_map.contains_key(k) {
                b_dups.push((k, v));
            } else {
                b_map.insert(k, (i, v));
            }
        }
        // Compare the copies, not just their names: two sides can repeat the
        // same name with different content.
        if a_dups != b_dups {
            let names = |d: &[(&'a str, &T)]| d.iter().map(|(k, _)| *k).collect::<Vec<_>>();
            let (an, bn) = (names(&a_dups), names(&b_dups));
            let field = format!("{collection}.duplicates");
            if an == bn {
                // Same names, different content: record it so the change is attributed.
                self.nodes[id].field_changes.push(FieldChange {
                    field,
                    before: to_json(&an),
                    after: to_json(&format!("{} changed", bn.join(", "))),
                });
            } else {
                self.field(id, &field, &an, &bn);
            }
        }
        if ordered {
            let a_order: Vec<&str> = a_map
                .keys()
                .copied()
                .filter(|k| b_map.contains_key(k))
                .collect();
            let b_order: Vec<&str> = b_map
                .keys()
                .copied()
                .filter(|k| a_map.contains_key(k))
                .collect();
            self.field(id, &format!("{collection}.order"), &a_order, &b_order);
        }
        let keys: Vec<&str> = a_map
            .keys()
            .chain(b_map.keys().filter(|k| !a_map.contains_key(*k)))
            .copied()
            .collect();
        for k in keys {
            let (ai, bi) = (a_map.get(k), b_map.get(k));
            let child = self.add(
                Some(id),
                format!("{collection}[{k}]"),
                ai.map(|&(_, v)| wrap(v)),
                bi.map(|&(_, v)| wrap(v)),
            );
            if let Some(c) = child {
                let n = &mut self.nodes[c];
                n.key = Some(k.to_string());
                n.old_index = ai.map(|&(i, _)| i);
                n.new_index = bi.map(|&(i, _)| i);
                if collection == "steps" {
                    n.step_index = n.new_index.or(n.old_index);
                }
            }
        }
    }

    /// Record per-entry changes of a named list as `<collection>[key]` field
    /// changes (for leaf lists that do not get nodes of their own).
    fn keyed_fields<T: PartialEq + Serialize>(
        &mut self,
        id: DiffNodeId,
        collection: &str,
        a: Option<&[T]>,
        b: Option<&[T]>,
        key: fn(&T) -> &str,
    ) {
        let a_map: IndexMap<&str, &T> = a.unwrap_or_default().iter().map(|v| (key(v), v)).collect();
        let b_map: IndexMap<&str, &T> = b.unwrap_or_default().iter().map(|v| (key(v), v)).collect();
        let before = self.nodes[id].field_changes.len();
        let a_order: Vec<&str> = a_map
            .keys()
            .copied()
            .filter(|k| b_map.contains_key(k))
            .collect();
        let b_order: Vec<&str> = b_map
            .keys()
            .copied()
            .filter(|k| a_map.contains_key(k))
            .collect();
        self.field(id, &format!("{collection}.order"), &a_order, &b_order);
        let keys: Vec<&str> = a_map
            .keys()
            .chain(b_map.keys().filter(|k| !a_map.contains_key(*k)))
            .copied()
            .collect();
        for k in keys {
            self.field(
                id,
                &format!("{collection}[{k}]"),
                &a_map.get(k),
                &b_map.get(k),
            );
        }
        // Nothing attributable (None vs empty, or repeated names): record the whole list.
        if self.nodes[id].field_changes.len() == before {
            self.field(id, collection, &a, &b);
        }
    }

    fn expand(&mut self, id: DiffNodeId, old: Subject<'a>, new: Subject<'a>) {
        match (old, new) {
            (Subject::Job(a), Subject::Job(b)) => self.expand_job(id, a, b),
            (Subject::JobParameter(a), Subject::JobParameter(b)) => {
                let JobParameter {
                    name,
                    param_type,
                    value,
                } = a;
                self.field(id, "name", name, &b.name);
                // The type and value as the host encoding writes them.
                self.field_as(id, "type", param_type, &b.param_type, |t| {
                    t.as_spec_str().into()
                });
                self.field_as(id, "value", value, &b.value, document::parameter_value);
            }
            (Subject::Environment(a), Subject::Environment(b)) => self.expand_env(id, a, b),
            (Subject::Step(a), Subject::Step(b)) => self.expand_step(id, a, b),
            (Subject::StepScript(a), Subject::StepScript(b)) => {
                let StepScript {
                    let_bindings,
                    actions: StepActions { on_run },
                    embedded_files,
                } = a;
                self.field(id, "let", let_bindings, &b.let_bindings);
                self.add(
                    Some(id),
                    "onRun".into(),
                    Some(Subject::Action(on_run)),
                    Some(Subject::Action(&b.actions.on_run)),
                );
                self.embedded_files(id, embedded_files, &b.embedded_files);
            }
            (Subject::EnvironmentScript(a), Subject::EnvironmentScript(b)) => {
                let EnvironmentScript {
                    let_bindings,
                    actions,
                    embedded_files,
                } = a;
                self.field(id, "let", let_bindings, &b.let_bindings);
                let EnvironmentActions {
                    on_enter,
                    on_wrap_env_enter,
                    on_wrap_task_run,
                    on_wrap_env_exit,
                    on_exit,
                } = actions;
                let bact = &b.actions;
                for (seg, x, y) in [
                    ("onEnter", on_enter, &bact.on_enter),
                    ("onWrapEnvEnter", on_wrap_env_enter, &bact.on_wrap_env_enter),
                    ("onWrapTaskRun", on_wrap_task_run, &bact.on_wrap_task_run),
                    ("onWrapEnvExit", on_wrap_env_exit, &bact.on_wrap_env_exit),
                    ("onExit", on_exit, &bact.on_exit),
                ] {
                    self.add(
                        Some(id),
                        seg.into(),
                        x.as_ref().map(Subject::Action),
                        y.as_ref().map(Subject::Action),
                    );
                }
                self.embedded_files(id, embedded_files, &b.embedded_files);
            }
            (Subject::Action(a), Subject::Action(b)) => {
                let Action {
                    command,
                    args,
                    timeout,
                    cancelation,
                } = a;
                self.field(id, "command", command, &b.command);
                self.field(id, "args", args, &b.args);
                self.field(id, "timeout", timeout, &b.timeout);
                self.field(id, "cancelation", cancelation, &b.cancelation);
            }
            (Subject::EmbeddedFile(a), Subject::EmbeddedFile(b)) => {
                let EmbeddedFile {
                    name,
                    file_type,
                    filename,
                    data,
                    runnable,
                    end_of_line,
                } = a;
                self.field(id, "name", name, &b.name);
                self.field(id, "type", file_type, &b.file_type);
                self.field(id, "filename", filename, &b.filename);
                self.field(id, "data", data, &b.data);
                self.field(id, "runnable", runnable, &b.runnable);
                self.field(id, "endOfLine", end_of_line, &b.end_of_line);
            }
            (Subject::ParameterSpace(a), Subject::ParameterSpace(b)) => {
                let StepParameterSpace {
                    task_parameter_definitions,
                    combination,
                } = a;
                self.field(id, "combination", combination, &b.combination);
                // Without an explicit combination, definition order fixes task order.
                self.keyed(
                    id,
                    "taskParameterDefinitions",
                    definitions_fix_task_order(a) && definitions_fix_task_order(b),
                    task_parameter_definitions
                        .iter()
                        .map(|(k, v)| (k.as_str(), v)),
                    b.task_parameter_definitions
                        .iter()
                        .map(|(k, v)| (k.as_str(), v)),
                    Subject::TaskParameter,
                );
            }
            (Subject::TaskParameter(a), Subject::TaskParameter(b)) => {
                self.field_as(id, "definition", a, b, |p| {
                    document::to_value(document::DefinitionDoc::new(None, p))
                });
            }
            (Subject::HostRequirements(a), Subject::HostRequirements(b)) => {
                let HostRequirements {
                    amounts,
                    attributes,
                } = a;
                self.keyed_fields(
                    id,
                    "amounts",
                    amounts.as_deref(),
                    b.amounts.as_deref(),
                    |r| &r.name,
                );
                self.keyed_fields(
                    id,
                    "attributes",
                    attributes.as_deref(),
                    b.attributes.as_deref(),
                    |r| &r.name,
                );
            }
            // Both sides of a node always come from the same field.
            _ => {}
        }
    }

    fn expand_job(&mut self, id: DiffNodeId, a: &'a Job, b: &'a Job) {
        // Exhaustive destructure: a new Job field fails to compile here
        // until the diff handles it.
        let Job {
            name,
            description,
            extensions,
            parameters,
            steps,
            job_environments,
        } = a;
        self.field(id, "name", name, &b.name);
        self.field(id, "description", description, &b.description);
        self.field(id, "extensions", extensions, &b.extensions);
        // In name order: parameter order is not a change, and a map filled
        // from a `HashMap` would otherwise order the nodes differently from
        // run to run.
        let sorted = |m: &'a IndexMap<String, JobParameter>| {
            let mut v: Vec<(&'a str, &'a JobParameter)> =
                m.iter().map(|(k, v)| (k.as_str(), v)).collect();
            v.sort_unstable_by_key(|(k, _)| *k);
            v
        };
        self.keyed(
            id,
            "parameters",
            false,
            sorted(parameters),
            sorted(&b.parameters),
            Subject::JobParameter,
        );
        self.environments(id, "jobEnvironments", job_environments, &b.job_environments);
        self.keyed(
            id,
            "steps",
            true,
            steps.iter().map(|s| (s.name.as_str(), s)),
            b.steps.iter().map(|s| (s.name.as_str(), s)),
            Subject::Step,
        );
    }

    fn expand_step(&mut self, id: DiffNodeId, a: &'a Step, b: &'a Step) {
        let Step {
            name,
            description,
            script,
            step_environments,
            parameter_space,
            host_requirements,
            dependencies,
            resolved_symtab,
        } = a;
        self.field(id, "name", name, &b.name);
        self.field(id, "description", description, &b.description);
        let deps = |d: &'a Option<Vec<crate::job::StepDependency>>| -> BTreeSet<&'a str> {
            d.iter().flatten().map(|d| d.depends_on.as_str()).collect()
        };
        let (da, db) = (deps(dependencies), deps(&b.dependencies));
        if da == db {
            // Same edges; only order or None-vs-empty can differ.
            self.field(id, "dependencies.order", dependencies, &b.dependencies);
        } else {
            self.field(id, "dependencies", &da, &db);
        }
        self.field(id, "resolvedSymTab", resolved_symtab, &b.resolved_symtab);
        self.add(
            Some(id),
            "script".into(),
            Some(Subject::StepScript(script)),
            Some(Subject::StepScript(&b.script)),
        );
        self.environments(
            id,
            "stepEnvironments",
            step_environments,
            &b.step_environments,
        );
        self.add(
            Some(id),
            "parameterSpace".into(),
            parameter_space.as_ref().map(Subject::ParameterSpace),
            b.parameter_space.as_ref().map(Subject::ParameterSpace),
        );
        self.add(
            Some(id),
            "hostRequirements".into(),
            host_requirements.as_ref().map(Subject::HostRequirements),
            b.host_requirements.as_ref().map(Subject::HostRequirements),
        );
    }

    fn expand_env(&mut self, id: DiffNodeId, a: &'a Environment, b: &'a Environment) {
        let Environment {
            name,
            description,
            script,
            variables,
            resolved_symtab,
        } = a;
        self.field(id, "name", name, &b.name);
        self.field(id, "description", description, &b.description);
        let before = self.nodes[id].field_changes.len();
        let keys: BTreeSet<&String> = variables
            .iter()
            .flatten()
            .chain(b.variables.iter().flatten())
            .map(|(k, _)| k)
            .collect();
        for k in keys {
            let x = variables.as_ref().and_then(|m| m.get(k));
            let y = b.variables.as_ref().and_then(|m| m.get(k));
            self.field(id, &format!("variables.{k}"), &x, &y);
        }
        if self.nodes[id].field_changes.len() == before {
            // Same entries; only None-vs-empty can differ.
            self.field(id, "variables", variables, &b.variables);
        }
        self.field(id, "resolvedSymTab", resolved_symtab, &b.resolved_symtab);
        self.add(
            Some(id),
            "script".into(),
            script.as_ref().map(Subject::EnvironmentScript),
            b.script.as_ref().map(Subject::EnvironmentScript),
        );
    }

    fn environments(
        &mut self,
        id: DiffNodeId,
        collection: &str,
        a: &'a Option<Vec<Environment>>,
        b: &'a Option<Vec<Environment>>,
    ) {
        if a.is_some() != b.is_some() && a.iter().chain(b).all(Vec::is_empty) {
            self.field(id, collection, a, b);
        }
        // Order matters: environments are entered in list order.
        self.keyed(
            id,
            collection,
            true,
            a.iter().flatten().map(|e| (e.name.as_str(), e)),
            b.iter().flatten().map(|e| (e.name.as_str(), e)),
            Subject::Environment,
        );
    }

    fn embedded_files(
        &mut self,
        id: DiffNodeId,
        a: &'a Option<Vec<EmbeddedFile>>,
        b: &'a Option<Vec<EmbeddedFile>>,
    ) {
        if a.is_some() != b.is_some() && a.iter().chain(b).all(Vec::is_empty) {
            self.field(id, "embeddedFiles", a, b);
        }
        self.keyed(
            id,
            "embeddedFiles",
            true,
            a.iter().flatten().map(|f| (f.name.as_str(), f)),
            b.iter().flatten().map(|f| (f.name.as_str(), f)),
            Subject::EmbeddedFile,
        );
    }
}
