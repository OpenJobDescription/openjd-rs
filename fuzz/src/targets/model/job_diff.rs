// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! `model_job_diff`: the unstable job diff, apply plan, task delta and
//! template diff (`openjd_model::job::{diff, apply, task_delta,
//! template_diff}`) on two jobs built from a generated template: the same
//! template under two parameter sets, an edited copy of it, or a second
//! generated template.
//!
//! Oracles beyond "never panics":
//! - **Diff identity**: `diff_jobs(a, a)` (and `diff_steps`,
//!   `diff_environments`) has no changed node, and `diff_jobs(a, b)` is
//!   changed when `a != b` and, for `==` jobs, only where their
//!   `JobDocument`s differ (`==` ignores task parameter definition order).
//!   Depth and symbol-table options agree with the full diff. Every node is
//!   found again by its path and its segment.
//! - **Apply rebuilds**: applying `plan(a, b)` to a store that holds
//!   `JobDocument(a)` and keys tasks by value, reading only the op
//!   documents, leaves a document that reads back as `b` and the task sets
//!   `create_job` builds for `b`. A refused kind, a store veto and a failed
//!   write behave as documented, and execution resumes after the failure.
//! - **Task delta**: kept plus added is the new step's task set, kept plus
//!   removed the old one's, and every count and index agrees with
//!   iteration.
//! - **Job documents**: `job_from_str(JobDocument(a)) == a` from JSON and
//!   YAML, with the same `job_digest`.
//! - **Template diff**: its jobs are the jobs the sides build, origins follow
//!   the split rules for equal parameters and equal templates, and
//!   `diff_built_jobs` reports the same job-layer changes with origin `Job`.

use super::{all_extensions, inputs, limits};
use crate::gen::template::{environment_template, job_template};
use crate::gen::{one_in, upto};
use crate::oracle;
use arbitrary::Unstructured;
use openjd_expr::{ExprValue, PathFormat};
use openjd_model::job::apply::{
    self as ap, Action, AllowList, ApplyError, JobApplier, MutationKind, MutationPolicy, Op,
    PlannedOp, Refusal, StepRef, TaskOp, ToDocument,
};
use openjd_model::job::diff::{
    diff_environments, diff_jobs, diff_steps, DiffOptions, DiffStatus, DiffTree,
};
use openjd_model::job::task_delta::{task_delta, task_parameter_set_document, TaskDelta};
use openjd_model::job::template_diff::JobDocument;
use openjd_model::job::template_diff::{
    build_job_symbol_table, diff_built_jobs, diff_job_template_with_parameter_sets, diff_templates,
    job_digest, job_from_str, merge_parameters, template_job_environment_fragment,
    template_step_fragment, Layer, Note, Origin, TaskSummary, TemplateDiff, TemplateInput,
    TemplateSource,
};
use openjd_model::job::{Environment, Job, JobParameter, Step, StepParameterSpace};
use openjd_model::{
    decode_job_template, JobParameterInputValues, JobParameterType, JobParameterValue,
    JobParameterValues, StepParameterSpaceIterator, TaskParameterSet,
};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error as _;

/// Task documents and how many tasks hold each.
type Tasks = BTreeMap<String, usize>;

fn counted(docs: &[Value]) -> Tasks {
    let mut held = Tasks::new();
    for d in docs {
        *held.entry(d.to_string()).or_default() += 1;
    }
    held
}

/// Above this many tasks in either job, the store and task-delta oracles
/// are skipped: they list every task.
const MAX_TASKS: u64 = 2_000;

/// Absolute directories for the build, on any host.
fn dirs() -> (&'static str, &'static str) {
    if cfg!(windows) {
        ("C:\\tmpl", "C:\\cwd")
    } else {
        ("/tmpl", "/cwd")
    }
}

/// The tasks of a space as `create_job` builds them (one per chunked
/// value), as task documents. A step with no space has one task.
fn create_job_tasks(space: Option<&StepParameterSpace>) -> Option<Vec<Value>> {
    match space {
        None => Some(vec![json!({})]),
        Some(s) => {
            let it = StepParameterSpaceIterator::new_with_chunk_override(s, Some(1)).ok()?;
            Some(it.map(|t| task_parameter_set_document(&t)).collect())
        }
    }
}

fn task_count(job: &Job) -> u64 {
    job.steps
        .iter()
        .map(|s| match &s.parameter_space {
            None => 1,
            Some(p) => StepParameterSpaceIterator::new_with_chunk_override(p, Some(1))
                .map_or(u64::MAX, |it| it.len()),
        })
        .fold(0, u64::saturating_add)
}

fn doc_value(job: &Job) -> Value {
    match serde_json::to_value(JobDocument(job)) {
        Ok(v) => v,
        Err(e) => {
            oracle!(false, "a JobDocument does not serialize: {e}");
            Value::Null
        }
    }
}

/// Clone through the `Clone` impl (an `Op` is also `Copy`).
fn dup<T: Clone>(x: &T) -> T {
    x.clone()
}

fn name_of(v: &Value) -> String {
    v.get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// A host store that holds a job as `JobDocument` pieces and its tasks as
/// counts of task documents, and reads every op only through its document,
/// as a store in another language would.
#[derive(Clone, PartialEq)]
struct DocStore {
    /// Every root key except `parameters`, `steps` and `jobEnvironments`.
    meta: Map<String, Value>,
    params: BTreeMap<String, Value>,
    envs: Vec<Value>,
    steps: BTreeMap<String, Value>,
    order: Vec<String>,
    /// Added steps and their final index, placed on commit.
    pending: Vec<(usize, String)>,
    /// Per step, how many tasks hold each task document. A count above one
    /// is a repeated range value: tasks with equal parameter sets.
    tasks: BTreeMap<String, Tasks>,
    writes: usize,
    checks: usize,
    /// Fail the write with this number once, before applying it.
    fail_write: Option<usize>,
    /// Veto the check with this number.
    veto_check: Option<usize>,
}

impl DocStore {
    fn holding(job: &Job) -> Option<Self> {
        let Value::Object(mut meta) = doc_value(job) else {
            return None;
        };
        let params = match meta.remove("parameters") {
            Some(Value::Object(m)) => m.into_iter().collect(),
            _ => BTreeMap::new(),
        };
        let list = |v: Option<Value>| match v {
            Some(Value::Array(a)) => a,
            _ => Vec::new(),
        };
        let envs = list(meta.remove("jobEnvironments"));
        let step_docs = list(meta.remove("steps"));
        let mut tasks = BTreeMap::new();
        for s in &job.steps {
            let held = counted(&create_job_tasks(s.parameter_space.as_ref())?);
            tasks.insert(s.name.clone(), held);
        }
        Some(Self {
            meta,
            params,
            envs,
            order: step_docs.iter().map(name_of).collect(),
            steps: step_docs.into_iter().map(|s| (name_of(&s), s)).collect(),
            pending: Vec::new(),
            tasks,
            writes: 0,
            checks: 0,
            fail_write: None,
            veto_check: None,
        })
    }

    /// The job document the store holds.
    fn document(&self) -> Value {
        let mut doc = self.meta.clone();
        doc.insert(
            "parameters".into(),
            Value::Object(self.params.clone().into_iter().collect()),
        );
        let steps = self
            .order
            .iter()
            .filter_map(|n| self.steps.get(n).cloned())
            .collect();
        doc.insert("steps".into(), Value::Array(steps));
        let envs = if self.envs.is_empty() {
            Value::Null
        } else {
            Value::Array(self.envs.clone())
        };
        doc.insert("jobEnvironments".into(), envs);
        Value::Object(doc)
    }

    /// The store holds `job`: its document reads back as `job`, and each
    /// step holds exactly the tasks `create_job` builds.
    fn check_holds(&self, job: &Job, what: &str) {
        let doc = self.document();
        let back = job_from_str(&doc.to_string());
        oracle!(
            back.as_ref().is_ok_and(|j| j == job),
            "{what}: the store's document does not read back as the new job: {:?}\n{doc}",
            back.err()
        );
        let mut want = BTreeMap::new();
        for s in &job.steps {
            let held = counted(&create_job_tasks(s.parameter_space.as_ref()).unwrap_or_default());
            want.insert(s.name.clone(), held);
        }
        oracle!(
            self.tasks == want,
            "{what}: the store's tasks are not the new job's:\n{:?}\n{want:?}",
            self.tasks
        );
    }

    fn write(&mut self) -> Result<(), String> {
        if self.fail_write == Some(self.writes) {
            self.fail_write = None;
            return Err("transient".into());
        }
        self.writes += 1;
        Ok(())
    }

    /// An op's document, checked against the op itself.
    fn op_doc<T: ToDocument>(op: Op<'_, T>) -> Result<Value, String> {
        let v = op.to_value().map_err(|e| e.to_string())?;
        let again = dup(&op).to_value().map_err(|e| e.to_string())?;
        oracle!(v == again, "a cloned op writes another document");
        oracle!(
            v["change"] == op.verb(),
            "op document {v} does not name its verb {}",
            op.verb()
        );
        Ok(v)
    }
}

impl JobApplier for DocStore {
    type Error = String;

    fn check(&mut self, _op: &PlannedOp<'_>) -> Result<(), String> {
        let n = self.checks;
        self.checks += 1;
        if self.veto_check == Some(n) {
            return Err(format!("veto {n}"));
        }
        Ok(())
    }

    fn job(&mut self, op: Op<'_, Job>) -> Result<(), String> {
        self.write()?;
        let d = Self::op_doc(op)?;
        if d["change"] != "modify" {
            return Err(format!("job op must be modify: {d}"));
        }
        let Value::Object(mut new) = d["new"].clone() else {
            return Err("job op without a new job".into());
        };
        let names = |v: Option<Value>| -> Vec<String> {
            match v {
                Some(Value::Array(a)) => a.iter().map(name_of).collect(),
                _ => Vec::new(),
            }
        };
        let env_order = names(new.remove("jobEnvironments"));
        let step_order = names(new.remove("steps"));
        new.remove("parameters");
        self.meta = new;
        // Reorders of what both jobs share.
        let at =
            |order: &[String], n: &str| order.iter().position(|x| x == n).unwrap_or(usize::MAX);
        self.envs.sort_by_key(|e| at(&env_order, &name_of(e)));
        self.order.sort_by_key(|s| at(&step_order, s));
        Ok(())
    }

    fn parameter(&mut self, op: Op<'_, JobParameter>) -> Result<(), String> {
        self.write()?;
        let d = Self::op_doc(op)?;
        match d["change"].as_str() {
            Some("add" | "modify") => {
                self.params.insert(name_of(&d["new"]), d["new"].clone());
            }
            Some("remove") => {
                self.params
                    .remove(&name_of(&d["old"]))
                    .ok_or("remove a missing parameter")?;
            }
            _ => return Err(format!("bad parameter op {d}")),
        }
        Ok(())
    }

    fn environment(&mut self, i: usize, op: Op<'_, Environment>) -> Result<(), String> {
        self.write()?;
        let d = Self::op_doc(op)?;
        let at = |envs: &[Value], n: &str| envs.iter().position(|e| name_of(e) == n);
        match d["change"].as_str() {
            Some("add") => {
                if i > self.envs.len() {
                    return Err(format!("environment add at {i} is past the end"));
                }
                self.envs.insert(i, d["new"].clone());
            }
            Some("modify") => {
                let n = name_of(&d["new"]);
                let j = at(&self.envs, &n).ok_or("modify a missing environment")?;
                if j != i {
                    return Err(format!("environment {n} is at {j}, op says {i}"));
                }
                self.envs[j] = d["new"].clone();
            }
            Some("remove") => {
                let j =
                    at(&self.envs, &name_of(&d["old"])).ok_or("remove a missing environment")?;
                self.envs.remove(j);
            }
            _ => return Err(format!("bad environment op {d}")),
        }
        Ok(())
    }

    fn step(&mut self, step: StepRef<'_>, op: Op<'_, Step>) -> Result<(), String> {
        self.write()?;
        let d = Self::op_doc(op)?;
        let side = if d["change"] == "remove" {
            "old"
        } else {
            "new"
        };
        let named = name_of(&d[side]);
        if named != step.name {
            return Err(format!("step ref {} names {named}", step.name));
        }
        match d["change"].as_str() {
            Some("add") => {
                self.pending.push((step.index, named.clone()));
                self.steps.insert(named, d["new"].clone());
            }
            Some("modify") => {
                if !self.steps.contains_key(&named) {
                    return Err(format!("modify missing step {named}"));
                }
                self.steps.insert(named, d["new"].clone());
            }
            Some("remove") => {
                self.steps.remove(&named).ok_or("remove a missing step")?;
                self.tasks.remove(&named);
                self.order.retain(|n| *n != named);
            }
            _ => return Err(format!("bad step op {d}")),
        }
        Ok(())
    }

    fn tasks(&mut self, step: StepRef<'_>, op: &TaskOp<'_>) -> Result<(), String> {
        self.write()?;
        let d = op.to_value().map_err(|e| e.to_string())?;
        let doc_view = serde_json::to_value(op.as_document()).map_err(|e| e.to_string())?;
        oracle!(d == doc_view, "TaskOp::to_value differs from as_document");
        if let Some(space) = op.space {
            let s =
                serde_json::to_value(ToDocument::as_document(space)).map_err(|e| e.to_string())?;
            oracle!(
                d["space"] == s,
                "the task document's space is not the space's document"
            );
        }
        let list = |key: &str| -> Vec<Value> { d[key].as_array().cloned().unwrap_or_default() };
        let added: Vec<Value> = op
            .added_tasks()
            .map_err(|e| e.to_string())?
            .map(|t| task_parameter_set_document(&t))
            .collect();
        let count = op.added_count().map_err(|e| e.to_string())?;
        oracle!(
            count == added.len() as u64,
            "added_count {count} but added_tasks yields {}",
            added.len()
        );
        let row = self
            .steps
            .get_mut(step.name)
            .ok_or_else(|| format!("tasks of missing step {}", step.name))?;
        row["parameterSpace"] = d["space"].clone();
        let held = self.tasks.entry(step.name.to_string()).or_default();
        match d["change"].as_str() {
            Some("boxes") => {
                oracle!(
                    list("appended") == added,
                    "appended documents are not added_tasks"
                );
                for t in list("removed") {
                    if held.remove(&t.to_string()).is_none() {
                        return Err(format!("{}: no task {t} to remove", step.name));
                    }
                }
                for t in list("appended") {
                    if held.insert(t.to_string(), 1).is_some() {
                        return Err(format!("{}: task {t} already exists", step.name));
                    }
                }
            }
            Some("all" | "replace") => {
                oracle!(list("tasks") == added, "task documents are not added_tasks");
                *held = counted(&list("tasks"));
            }
            other => return Err(format!("unknown task change {other:?}")),
        }
        Ok(())
    }

    fn commit(&mut self) -> Result<(), String> {
        self.pending.sort();
        for (j, name) in std::mem::take(&mut self.pending) {
            if j > self.order.len() {
                return Err(format!("step {name} at {j} is past the end"));
            }
            self.order.insert(j, name);
        }
        Ok(())
    }
}

/// Refuses every op on a step, with [`Refusal::new`].
struct FrozenSteps;

impl MutationPolicy for FrozenSteps {
    fn allows(&self, op: &PlannedOp<'_>) -> Result<(), Refusal> {
        match (&op.action, op.kinds.first()) {
            (Action::Step(..) | Action::Tasks(..), Some(k)) => {
                Err(Refusal::new(op, *k, "steps are frozen"))
            }
            _ => Ok(()),
        }
    }
}

/// A diff's `changed` agrees with `==` and with the document: unequal
/// values differ, and equal ones differ only where their documents do (a
/// task parameter definition reorder, which `==` ignores).
fn agrees(changed: bool, equal: bool, same_document: impl FnOnce() -> bool) -> bool {
    match (changed, equal) {
        (false, true) | (true, false) => true,
        (false, false) => false,
        (true, true) => !same_document(),
    }
}

/// The tree's nodes agree with its accessors, and it has no changed node
/// when `unchanged`.
fn check_tree(tree: &DiffTree, unchanged: bool, what: &str) {
    let nodes = tree.nodes();
    oracle!(
        std::ptr::eq(tree.root(), &nodes[0]),
        "{what}: root is not node 0"
    );
    oracle!(
        tree.is_changed() == tree.root().is_changed(),
        "{what}: is_changed disagrees with the root"
    );
    for (id, n) in nodes.iter().enumerate() {
        oracle!(
            tree.node(id).is_some_and(|m| std::ptr::eq(m, n)),
            "{what}: node({id}) is not nodes()[{id}]"
        );
        oracle!(
            n.is_changed() == (n.status != DiffStatus::Unchanged),
            "{what}: node {id} is_changed disagrees with its status"
        );
        if let Some(p) = n.parent {
            oracle!(
                tree.child(p, &n.segment) == Some(id),
                "{what}: node {id} ({}) is not its parent's child",
                n.segment
            );
            oracle!(
                tree.node(p).is_some_and(|m| m.depth + 1 == n.depth),
                "{what}: node {id} depth is not its parent's plus one"
            );
        }
        if !n.segment.contains('.') {
            let path = tree.path(id);
            let found = tree.find(&path);
            oracle!(
                found.is_some_and(|f| tree.path(f) == path),
                "{what}: find({path:?}) = {found:?}"
            );
        }
    }
    let changed: Vec<usize> = tree.changed().map(|(id, _)| id).collect();
    let want: Vec<usize> = (0..nodes.len())
        .filter(|&i| nodes[i].is_changed())
        .collect();
    oracle!(
        changed == want,
        "{what}: changed() is not the changed nodes"
    );
    let text = tree.to_string();
    oracle!(
        text.is_empty() != tree.is_changed(),
        "{what}: listing {text:?} for is_changed {}",
        tree.is_changed()
    );
    if unchanged {
        oracle!(
            changed.is_empty(),
            "{what}: a diff with itself has changed nodes:\n{text}"
        );
    }
}

/// The job document reads back as the job, from JSON and YAML, with the
/// same digest; the job symbol table has every parameter.
fn check_document(job: &Job) {
    let text = match serde_json::to_string(&JobDocument(job)) {
        Ok(t) => t,
        Err(e) => return oracle!(false, "JobDocument does not serialize: {e}"),
    };
    oracle!(
        text.contains(&format!("\"documentVersion\":{}", JobDocument::VERSION)),
        "the document has no documentVersion"
    );
    let back = job_from_str(&text);
    oracle!(
        back.as_ref().is_ok_and(|j| j == job),
        "job_from_str(JobDocument(job)) is not the job: {:?}\n{text}",
        back.as_ref().err()
    );
    if let Ok(yaml) = serde_saphyr::to_string(&JobDocument(job)) {
        let y = job_from_str(&yaml);
        oracle!(
            y.as_ref().is_ok_and(|j| j == job),
            "job_from_str(YAML JobDocument) is not the job: {:?}\n{yaml}",
            y.err()
        );
    }
    match (job_digest(job), back.as_ref().map(job_digest)) {
        (Ok(d), Ok(Ok(d2))) => {
            oracle!(
                d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()),
                "job_digest {d:?} is not sha256 hex"
            );
            oracle!(
                d == d2,
                "a job read back from its document digests differently"
            );
        }
        (d, d2) => oracle!(false, "job_digest failed: {d:?} {d2:?}"),
    }
    match build_job_symbol_table(job).map(|t| t.to_symtab(PathFormat::Posix)) {
        Ok(Ok(st)) => {
            for (name, p) in &job.parameters {
                let path = matches!(
                    p.param_type,
                    JobParameterType::Path | JobParameterType::ListPath
                );
                oracle!(
                    st.contains(&format!("RawParam.{name}"))
                        && st.contains(&format!("Param.{name}")) != path,
                    "build_job_symbol_table: wrong entries for {name} ({:?})",
                    p.param_type
                );
            }
        }
        other => oracle!(false, "build_job_symbol_table failed: {other:?}"),
    }
}

/// `task_delta` between two steps' spaces agrees with iteration.
fn check_task_delta(old: &Step, new: &Step) {
    let (o, n) = (old.parameter_space.as_ref(), new.parameter_space.as_ref());
    oracle!(
        matches!(task_delta(n, n), TaskDelta::Unchanged),
        "task_delta of a space with itself is not Unchanged"
    );
    let TaskDelta::Product(d) = task_delta(o, n) else {
        return;
    };
    let set = |s: Option<&StepParameterSpace>| -> BTreeSet<String> {
        create_job_tasks(s)
            .unwrap_or_default()
            .iter()
            .map(Value::to_string)
            .collect()
    };
    let docs = |it: &mut dyn Iterator<Item = TaskParameterSet>| -> Vec<String> {
        it.map(|t| task_parameter_set_document(&t).to_string())
            .collect()
    };
    let mut boxes = vec![&d.old, &d.kept];
    boxes.extend(&d.added);
    boxes.extend(&d.removed);
    for b in boxes {
        let Ok(it) = b.tasks() else {
            return oracle!(false, "a task box does not iterate");
        };
        let tasks: Vec<TaskParameterSet> = it.collect();
        oracle!(
            b.len() == tasks.len() as u64 && b.is_empty() == tasks.is_empty(),
            "TaskBox::len {} but it yields {}",
            b.len(),
            tasks.len()
        );
        for (i, t) in tasks.iter().enumerate() {
            oracle!(
                b.index_of(t) == Some(i as u64),
                "TaskBox::index_of of task {i} is {:?}",
                b.index_of(t)
            );
        }
        let space_tasks = create_job_tasks(Some(b.space())).unwrap_or_default();
        oracle!(
            space_tasks.len() == tasks.len(),
            "a box's space iterates {} tasks, the box {}",
            space_tasks.len(),
            tasks.len()
        );
    }
    let kept = docs(&mut d.kept.tasks().into_iter().flatten());
    let Ok(mut appended) = d.appended() else {
        return oracle!(false, "ProductDelta::appended fails");
    };
    let appended = docs(&mut appended);
    let Ok(removed) = d.removed() else {
        return oracle!(false, "ProductDelta::removed fails");
    };
    let removed: Vec<(u64, TaskParameterSet)> = removed.collect();
    oracle!(
        d.appended_count() == appended.len() as u64 && d.removed_count() == removed.len() as u64,
        "ProductDelta counts disagree with iteration"
    );
    oracle!(
        removed.windows(2).all(|w| w[0].0 < w[1].0),
        "ProductDelta::removed is not in ascending index order"
    );
    for (i, t) in &removed {
        oracle!(
            d.old.index_of(t) == Some(*i),
            "removed task at old index {i} has index_of {:?}",
            d.old.index_of(t)
        );
    }
    let kept_set: BTreeSet<String> = kept.iter().cloned().collect();
    let mut new_set = kept_set.clone();
    new_set.extend(appended.iter().cloned());
    oracle!(
        new_set == set(n) && new_set.len() == kept.len() + appended.len(),
        "kept + added is not the new step's tasks"
    );
    let mut old_set = kept_set;
    old_set.extend(
        removed
            .iter()
            .map(|(_, t)| task_parameter_set_document(t).to_string()),
    );
    oracle!(
        old_set == set(o) && old_set.len() == kept.len() + removed.len(),
        "kept + removed is not the old step's tasks"
    );
}

/// Plan `a -> b`, apply it to stores holding `a`, and check the refusal,
/// veto and resume paths.
fn check_apply(a: &Job, b: &Job, tree: &DiffTree, u: &mut Unstructured<'_>) {
    let Ok(p) = ap::plan(a, b) else {
        return oracle!(false, "plan fails on two built jobs");
    };
    oracle!(
        p.diff == *tree,
        "the plan's diff is not diff_jobs with default options"
    );
    oracle!(
        p.ops.is_empty() || tree.is_changed(),
        "an unchanged diff plans ops"
    );
    let mut union = BTreeSet::new();
    for op in &p.ops {
        oracle!(!op.kinds.is_empty(), "an op carries no mutation kind");
        union.extend(op.kinds.iter().copied());
        let v = op.to_value();
        let view = serde_json::to_value(op.as_document());
        oracle!(
            matches!((&v, &view), (Ok(x), Ok(y)) if x == y),
            "PlannedOp::to_value differs from as_document"
        );
        oracle!(
            !op.target().to_string().is_empty(),
            "an op target displays empty"
        );
    }
    oracle!(
        p.kinds() == union,
        "ApplyPlan::kinds is not the union of its ops' kinds"
    );
    let all = AllowList::of(p.kinds());
    let named = AllowList::from_names(p.kinds().iter().map(|k| k.name()));
    oracle!(
        named.as_ref() == Ok(&all),
        "AllowList::from_names differs from of"
    );

    let Some(fresh) = DocStore::holding(a) else {
        return;
    };
    // Plain apply.
    let mut store = fresh.clone();
    match ap::apply(a, b, &all, &mut store) {
        Ok(r) => oracle!(
            r.applied == p.ops.len(),
            "applied {} of {}",
            r.applied,
            p.ops.len()
        ),
        Err(e) => return oracle!(false, "apply with every kind allowed fails: {e}"),
    }
    store.check_holds(b, "apply");
    if p.ops.is_empty() {
        return;
    }

    // A refused kind: the first op that carries it is refused, nothing is written.
    let i = upto(u, p.ops.len() - 1);
    let kinds: Vec<MutationKind> = p.ops[i].kinds.iter().copied().collect();
    let k = kinds[upto(u, kinds.len() - 1)];
    let policy = AllowList::of(p.kinds().into_iter().filter(|x| *x != k));
    let first = p.ops.iter().position(|op| op.kinds.contains(&k));
    let mut store = fresh.clone();
    match ap::apply(a, b, &policy, &mut store) {
        Err(ref e @ ApplyError::Refused { op, ref refusal }) => {
            oracle!(Some(op) == first, "refused op {op}, expected {first:?}");
            oracle!(
                refusal.kind == k && refusal.target == p.ops[op].target(),
                "refusal {refusal:?} for kind {k}"
            );
            let want = format!("{}: {k} is not allowed", refusal.target);
            oracle!(
                e.to_string() == want && refusal.to_string() == want,
                "refusal displays {e}, expected {want}"
            );
            oracle!(e.source().is_some(), "a refusal has no source");
        }
        other => oracle!(false, "a policy without {k} gives {other:?}"),
    }
    oracle!(store == fresh, "a refused plan wrote to the store");

    // A custom policy built with Refusal::new.
    let mut store = fresh.clone();
    let frozen = p
        .ops
        .iter()
        .position(|op| matches!(op.action, Action::Step(..) | Action::Tasks(..)));
    match (ap::apply(a, b, &FrozenSteps, &mut store), frozen) {
        (Ok(_), None) => {}
        (Err(ApplyError::Refused { op, refusal }), Some(f)) => oracle!(
            op == f && refusal.reason == "steps are frozen",
            "FrozenSteps refused op {op} ({refusal}), expected {f}"
        ),
        (other, f) => oracle!(false, "FrozenSteps gives {other:?}, first step op {f:?}"),
    }
    oracle!(
        store.writes == 0 || frozen.is_none(),
        "a refused plan wrote to the store"
    );

    // A store veto: nothing is written.
    let v = upto(u, p.ops.len() - 1);
    let mut store = fresh.clone();
    store.veto_check = Some(v);
    match ap::validate(&p, &all, &mut store) {
        Err(ref e @ ApplyError::Vetoed { op, ref target, .. }) => {
            oracle!(
                op == v && *target == p.ops[v].target(),
                "vetoed op {op} ({target}), expected {v}"
            );
            oracle!(
                e.to_string() == format!("{target}: veto {v}") && e.source().is_none(),
                "a veto displays {e}"
            );
        }
        other => oracle!(false, "a veto at {v} gives {other:?}"),
    }
    oracle!(store.writes == 0, "a vetoed plan wrote to the store");

    // A failed write, then a resume from it.
    let w = upto(u, p.ops.len() - 1);
    let mut store = fresh;
    store.fail_write = Some(w);
    oracle!(
        ap::validate(&p, &all, &mut store).is_ok(),
        "validate fails with every kind allowed"
    );
    match ap::execute(&p, &mut store, 0) {
        Err(ref e @ ApplyError::Store { op, .. }) => {
            oracle!(op == w, "write {op} failed, expected {w}");
            oracle!(
                e.to_string() == format!("write {w} failed: transient") && e.source().is_none(),
                "a failed write displays {e}"
            );
            match ap::execute(&p, &mut store, op) {
                Ok(r) => oracle!(r.applied == p.ops.len(), "resume applied {}", r.applied),
                Err(e) => oracle!(false, "resume after a failed write fails: {e}"),
            }
            store.check_holds(b, "resume");
        }
        other => oracle!(false, "a failed write {w} gives {other:?}"),
    }
}

/// A template diff's accessors and listings agree with its fields.
fn check_template_diff(td: &TemplateDiff, what: &str) {
    let _ = td.to_string();
    oracle!(
        serde_json::to_value(td).is_ok(),
        "{what}: TemplateDiff does not serialize"
    );
    oracle!(
        td.changes_at("").count() == td.changes.len(),
        "{what}: changes_at(\"\") is not every change"
    );
    for c in &td.changes {
        oracle!(
            c.to_string().ends_with(&format!("  [{}]", c.origin)),
            "{what}: change {c} does not end with its origin"
        );
        oracle!(
            !c.layer.to_string().is_empty() && !c.stage.to_string().is_empty(),
            "{what}: a layer or stage displays empty"
        );
        for s in c.before.iter().chain(&c.after) {
            oracle!(
                s.to_string() == s.text,
                "{what}: Shown displays other than its text"
            );
        }
        oracle!(
            td.changes_at(&c.path).any(|x| x == c),
            "{what}: changes_at({:?}) misses its own change",
            c.path
        );
    }
    for n in &td.notes {
        check_note(n);
    }
    let stat = td.stat();
    let _ = stat.to_string();
    let count = |st: DiffStatus| td.steps.iter().filter(|s| s.status == st).count() as u64;
    oracle!(
        stat.steps_added == count(DiffStatus::Added)
            && stat.steps_removed == count(DiffStatus::Removed),
        "{what}: DiffStat {stat:?} disagrees with the step statuses"
    );
    for s in &td.steps {
        oracle!(
            td.step(&s.name) == Some(s),
            "{what}: step({:?}) is not its summary",
            s.name
        );
        let mine = td
            .changes
            .iter()
            .filter(|c| c.step.as_deref() == Some(&*s.name));
        oracle!(
            td.changes_in_step(&s.name).eq(mine),
            "{what}: changes_in_step({:?}) is not the step's changes",
            s.name
        );
        let _ = (s.stat_line(), s.to_string(), s.tasks.to_string());
        if let TaskSummary::Product {
            kept,
            added,
            removed,
        } = &s.tasks
        {
            for b in std::iter::once(kept).chain(added).chain(removed) {
                let _ = b.to_string();
            }
        }
    }
    if !td.is_changed() {
        oracle!(td.changes.is_empty(), "{what}: unchanged with changes");
    }
    match (td.plan(), ap::plan(&td.old_job, &td.new_job)) {
        (Ok(x), Ok(y)) => oracle!(x == y, "{what}: TemplateDiff::plan is not plan(old, new)"),
        (x, y) => oracle!(false, "{what}: plans fail: {:?} {:?}", x.err(), y.err()),
    }
}

/// A note displays as `steps[S]: text` or `text` and serializes as that.
fn check_note(n: &Note) {
    let want = match &n.step {
        Some(s) => format!("steps[{s}]: {}", n.text),
        None => n.text.clone(),
    };
    oracle!(n.to_string() == want, "note displays {n}, expected {want}");
    oracle!(
        serde_json::to_value(n).ok() == Some(Value::String(want)),
        "a note does not serialize as its text"
    );
}

/// A value not in the list `r`, of the type of its elements.
fn fresh_value(r: &[Value]) -> Option<Value> {
    let int = |v: &Value| match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse::<i64>().ok(),
        _ => None,
    };
    let ints: Option<Vec<i64>> = r.iter().map(int).collect();
    match (r.first(), ints) {
        (Some(Value::Number(_)), Some(i)) => i
            .iter()
            .max()
            .and_then(|m| m.checked_add(1))
            .map(Value::from),
        (Some(Value::String(_)), Some(i)) => i
            .iter()
            .max()
            .and_then(|m| m.checked_add(1))
            .map(|m| json!(m.to_string())),
        (Some(Value::String(f)), None) => Some(json!(format!("{f}_{}", r.len()))),
        _ => None,
    }
}

/// Make every parameter space of `doc` a plain product: no `combination`
/// and no repeated list value, so a change to it can be a task delta
/// `Product`.
fn plain(doc: &mut Value) {
    let Some(Value::Array(steps)) = doc.get_mut("steps") else {
        return;
    };
    for space in steps.iter_mut().filter_map(|s| s.get_mut("parameterSpace")) {
        if let Some(m) = space.as_object_mut() {
            m.remove("combination");
        }
        let defs = space.get_mut("taskParameterDefinitions");
        for d in defs.and_then(Value::as_array_mut).into_iter().flatten() {
            if let Some(Value::Array(r)) = d.get_mut("range") {
                let mut seen = Vec::new();
                r.retain(|v| {
                    let new = !seen.contains(v);
                    if new {
                        seen.push(v.clone());
                    }
                    new
                });
            }
        }
    }
}

/// An edit of a template document. Returns the edited document and, when
/// the edit adds a required parameter, its name and value.
fn edit(doc: &Value, u: &mut Unstructured<'_>) -> (Value, Option<(String, ExprValue)>) {
    let mut doc = doc.clone();
    let mut extra = None;
    for _ in 0..=upto(u, 2) {
        let Some(o) = doc.as_object_mut() else {
            break;
        };
        match upto(u, 9) {
            0 => {
                let name = o.get("name").and_then(Value::as_str).unwrap_or("Job");
                let renamed = format!("{name} v2");
                o.insert("name".into(), json!(renamed));
            }
            1 => {
                if let Some(Value::Array(steps)) = o.get_mut("steps") {
                    steps.reverse();
                }
            }
            2 => {
                // Drop the last step and every dependency on it.
                if let Some(Value::Array(steps)) = o.get_mut("steps") {
                    if steps.len() > 1 {
                        let gone = steps.pop().map(|s| name_of(&s)).unwrap_or_default();
                        for m in steps.iter_mut().filter_map(Value::as_object_mut) {
                            if let Some(Value::Array(deps)) = m.get_mut("dependencies") {
                                deps.retain(|d| d.get("dependsOn") != Some(&json!(gone)));
                                if deps.is_empty() {
                                    m.remove("dependencies");
                                }
                            }
                        }
                    }
                }
            }
            3 => {
                // A copy of the first step under a new name.
                if let Some(Value::Array(steps)) = o.get_mut("steps") {
                    let copy = format!("Copy{}", steps.len());
                    if let Some(Value::Object(mut s)) = steps.first().cloned() {
                        s.insert("name".into(), json!(copy));
                        steps.push(Value::Object(s));
                    }
                }
            }
            4 => {
                if let Some(Value::Array(steps)) = o.get_mut("steps") {
                    let n = steps.len();
                    let s = steps.get_mut(upto(u, n.saturating_sub(1)));
                    if let Some(c) = s.and_then(|s| s.pointer_mut("/script/actions/onRun/command"))
                    {
                        *c = json!("edited");
                    }
                }
            }
            5 | 6 => {
                // Reorder, shorten or extend a step's task parameters.
                if let Some(Value::Array(steps)) = o.get_mut("steps") {
                    let n = steps.len();
                    let s = steps.get_mut(upto(u, n.saturating_sub(1)));
                    let defs =
                        s.and_then(|s| s.pointer_mut("/parameterSpace/taskParameterDefinitions"));
                    if let Some(Value::Array(defs)) = defs {
                        match upto(u, 2) {
                            0 => defs.reverse(),
                            1 => {
                                for d in defs.iter_mut() {
                                    if let Some(Value::Array(r)) = d.get_mut("range") {
                                        if r.len() > 1 {
                                            r.pop();
                                        }
                                    }
                                }
                            }
                            _ => {
                                for d in defs.iter_mut() {
                                    if let Some(Value::Array(r)) = d.get_mut("range") {
                                        if let Some(v) = fresh_value(r) {
                                            r.push(v);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            7 => {
                if let Some(Value::Array(envs)) = o.get_mut("jobEnvironments") {
                    if one_in(u, 2) && envs.len() > 1 {
                        envs.reverse();
                    } else {
                        envs.pop();
                    }
                    if envs.is_empty() {
                        o.remove("jobEnvironments");
                    }
                }
            }
            8 => {
                // A required parameter the old parameter set lacks.
                let defs = o
                    .entry("parameterDefinitions")
                    .or_insert_with(|| Value::Array(Vec::new()));
                if let Value::Array(defs) = defs {
                    if defs.iter().all(|d| d["name"] != "Added") {
                        defs.push(json!({"name": "Added", "type": "INT"}));
                        extra = Some(("Added".to_string(), ExprValue::Int(upto(u, 9) as i64)));
                    }
                }
            }
            _ => {
                o.insert("description".into(), json!("edited"));
            }
        }
    }
    (doc, extra)
}

/// Two jobs from a generated template, and every oracle in the module docs.
pub fn job_diff(data: &[u8]) {
    let mut u = Unstructured::new(data);
    let mut g = job_template(&mut u);
    if one_in(&mut u, 2) {
        plain(&mut g.doc);
    }
    let exts = all_extensions();
    let env = one_in(&mut u, 4).then(|| environment_template(&mut u).doc);
    let (new_doc, extra) = match upto(&mut u, 3) {
        0 => (g.doc.clone(), None),
        1 | 2 => edit(&g.doc, &mut u),
        _ => (job_template(&mut u).doc, None),
    };
    let in_old = inputs(&mut u, &g.params);
    let mut in_new = if one_in(&mut u, 2) {
        in_old.clone()
    } else {
        inputs(&mut u, &g.params)
    };
    in_new.extend(extra);
    let (tdir, cwd) = dirs();
    let side = |source: TemplateSource, input: &JobParameterInputValues| {
        let s = TemplateInput::new(source)
            .extensions(&exts)
            .limits(limits())
            .template_dir(tdir)
            .cwd(cwd)
            .params(input.clone());
        match &env {
            Some(e) => s.environment(TemplateSource::Value(e.clone())),
            None => s,
        }
    };
    let opts = DiffOptions::default();
    // Each template is decoded once here and given `Decoded`: validation is
    // most of a build's cost, and the target builds each side several times.
    let lim = limits();
    let decoded = |doc: &Value| {
        decode_job_template(doc.clone(), Some(&exts), &lim)
            .ok()
            .map(|t| TemplateSource::Decoded(Box::new(t)))
    };
    let Some(old_source) = decoded(&g.doc) else {
        return;
    };
    let new_source = if new_doc == g.doc {
        old_source.clone()
    } else {
        match decoded(&new_doc) {
            Some(s) => s,
            None => {
                // The failing side is decoded again: a document, not `Decoded`.
                let new_side = side(TemplateSource::Value(new_doc.clone()), &in_new);
                oracle!(
                    diff_templates(&side(old_source, &in_old), &new_side, &opts).is_err(),
                    "diff_templates succeeds though the new template does not decode"
                );
                return;
            }
        }
    };
    // The template alone, for diff_job_template_with_parameter_sets, which
    // lays each set over the template input's own parameters.
    let template = side(old_source.clone(), &JobParameterInputValues::new());
    let old_side = side(old_source, &in_old);
    let new_side = side(new_source, &in_new);
    oracle!(
        old_side.parameters() == &in_old,
        "TemplateInput::parameters is not what params set"
    );
    let (a, b) = match (old_side.build(), new_side.build()) {
        (Ok(a), Ok(b)) => (a, b),
        _ => {
            oracle!(
                diff_templates(&old_side, &new_side, &opts).is_err(),
                "diff_templates succeeds though a side does not build"
            );
            return;
        }
    };

    // Other ways to build the old side give the same job.
    let p = crate::gen::ident(&mut u);
    let with = old_side.clone().param(p.clone(), "1");
    oracle!(
        with.parameters().get(&p) == Some(&ExprValue::String("1".into())),
        "TemplateInput::param does not set {p}"
    );
    let same = |s: TemplateInput, what: &str| {
        let j = s.build();
        oracle!(
            j.as_ref().is_ok_and(|j| *j == a),
            "{what} builds another job: {:?}",
            j.err()
        );
    };
    let yaml = serde_saphyr::to_string(&g.doc).ok();
    let text = yaml.as_ref().map(|y| (y, TemplateSource::guess(y)));
    match &text {
        Some((y, TemplateSource::Text(t))) => {
            oracle!(t == *y, "TemplateSource::guess changed the text")
        }
        Some((y, _)) => oracle!(
            std::path::Path::new(y).is_file(),
            "TemplateSource::guess gives a path for text that names no file"
        ),
        None => {}
    }
    let declares_extensions = |d: &Value| d.get("extensions").is_some();
    match upto(&mut u, 2) {
        0 => same(
            side(TemplateSource::Value(g.doc.clone()), &in_old),
            "the template document",
        ),
        1 => {
            if let Some((_, t @ TemplateSource::Text(_))) = text {
                same(side(t, &in_old), "the template's YAML text");
            }
        }
        _ => {
            if !declares_extensions(&g.doc) && !env.as_ref().is_some_and(declares_extensions) {
                let s = side(TemplateSource::Value(g.doc.clone()), &in_old).no_extensions();
                same(s, "a template with no extensions, without the allowlist,");
            }
        }
    }
    let stored: JobParameterValues = a
        .parameters
        .iter()
        .map(|(k, p)| {
            let v = JobParameterValue {
                param_type: p.param_type,
                value: p.value.clone(),
            };
            (k.clone(), v)
        })
        .collect();
    match merge_parameters(&stored, &JobParameterInputValues::new(), &old_side) {
        Ok(m) => same(
            old_side.clone().preprocessed(m),
            "a job rebuilt from its own stored values",
        ),
        Err(e) => oracle!(false, "merge_parameters of a job's own values fails: {e}"),
    }

    // Host fragments.
    for s in &a.steps {
        match template_step_fragment(&g.doc, &s.name) {
            Ok(f) => oracle!(
                name_of(&f.step) == s.name
                    && f.step_environments.len()
                        == s.step_environments.as_ref().map_or(0, Vec::len),
                "template_step_fragment({:?}) is another step",
                s.name
            ),
            Err(e) => oracle!(false, "template_step_fragment({:?}) fails: {e}", s.name),
        }
    }
    let missing = format!(
        "{}x",
        a.steps.iter().map(|s| s.name.as_str()).collect::<String>()
    );
    oracle!(
        template_step_fragment(&g.doc, &missing).is_err()
            && template_job_environment_fragment(&g.doc, &missing).is_err(),
        "a fragment for a name the template does not have"
    );
    for e in a.job_environments.iter().flatten() {
        let f = template_job_environment_fragment(&g.doc, &e.name);
        oracle!(
            f.as_ref().is_ok_and(|f| name_of(f) == e.name),
            "template_job_environment_fragment({:?}): {:?}",
            e.name,
            f.err()
        );
    }

    // Documents.
    check_document(&a);
    if b != a {
        check_document(&b);
    }

    // Job diff.
    let tree = diff_jobs(&a, &b, &opts);
    // `==` ignores task parameter definition order, which without an
    // explicit `combination` is the task order and a change; the document
    // keeps it. So: unequal jobs differ, and a reported change is in the
    // document.
    oracle!(
        agrees(tree.is_changed(), a == b, || doc_value(&a) == doc_value(&b)),
        "diff_jobs is_changed {} but a == b is {}:\n{tree}",
        tree.is_changed(),
        a == b
    );
    check_tree(&tree, false, "diff_jobs(a, b)");
    check_tree(&diff_jobs(&a, &a, &opts), true, "diff_jobs(a, a)");
    let depth = upto(&mut u, 4);
    let shallow = diff_jobs(&a, &b, &DiffOptions::default().with_max_depth(depth));
    oracle!(
        shallow.is_changed() == tree.is_changed()
            && shallow.nodes().iter().all(|n| n.depth <= depth),
        "diff at depth {depth} disagrees with the full diff"
    );
    check_tree(&shallow, false, "diff_jobs at a depth");
    let no_symtab = diff_jobs(&a, &b, &DiffOptions::default().with_resolved_symtab(false));
    oracle!(
        tree.is_changed() || !no_symtab.is_changed(),
        "ignoring symbol tables finds a change the full diff does not"
    );
    check_tree(&no_symtab, false, "diff_jobs without symbol tables");
    for s in &a.steps {
        check_tree(&diff_steps(s, s, &opts), true, "diff_steps(s, s)");
        if let Some(t) = b.steps.iter().find(|t| t.name == s.name) {
            let d = diff_steps(s, t, &opts);
            let doc = |x: &Step| serde_json::to_value(ToDocument::as_document(x)).ok();
            oracle!(
                agrees(d.is_changed(), s == t, || doc(s) == doc(t)),
                "diff_steps is_changed {} for steps {}",
                d.is_changed(),
                s.name
            );
            check_tree(&d, false, "diff_steps");
        }
    }
    for e in a.job_environments.iter().flatten() {
        check_tree(
            &diff_environments(e, e, &opts),
            true,
            "diff_environments(e, e)",
        );
        if let Some(f) = b
            .job_environments
            .iter()
            .flatten()
            .find(|f| f.name == e.name)
        {
            let d = diff_environments(e, f, &opts);
            oracle!(
                d.is_changed() == (e != f),
                "diff_environments is_changed {} for {}",
                d.is_changed(),
                e.name
            );
        }
    }

    // Plan and apply.
    let small = task_count(&a) <= MAX_TASKS && task_count(&b) <= MAX_TASKS;
    match ap::plan(&a, &a) {
        Ok(p) => oracle!(p.ops.is_empty(), "plan(a, a) has {} ops", p.ops.len()),
        Err(e) => oracle!(false, "plan(a, a) fails: {e}"),
    }
    if small {
        for s in &a.steps {
            if let Some(t) = b.steps.iter().find(|t| t.name == s.name) {
                check_task_delta(s, t);
            }
        }
        check_apply(&a, &b, &tree, &mut u);
    }

    // Template diff.
    // Documents take the equal-template shortcut; decoded sources never do.
    let (dt_old, dt_new) = if one_in(&mut u, 4) {
        (
            side(TemplateSource::Value(g.doc.clone()), &in_old),
            side(TemplateSource::Value(new_doc.clone()), &in_new),
        )
    } else {
        (old_side.clone(), new_side.clone())
    };
    match diff_templates(&dt_old, &dt_new, &opts) {
        Ok(td) => {
            oracle!(
                td.old_job == a && td.new_job == b && td.job == tree,
                "diff_templates' jobs are not the jobs the sides build"
            );
            let job_layer = || td.changes.iter().filter(|c| c.layer == Layer::Job);
            if in_old == in_new {
                oracle!(
                    job_layer().all(|c| c.origin == Origin::Template),
                    "equal parameters, but a job-layer change is not from the template"
                );
            } else if new_doc == g.doc {
                oracle!(
                    td.changes
                        .iter()
                        .all(|c| c.layer == Layer::Job && c.origin == Origin::Parameters),
                    "one template, but a change is not from the parameters"
                );
            }
            if small {
                check_template_diff(&td, "diff_templates");
            }
            let built = diff_built_jobs(a.clone(), b.clone(), &opts);
            oracle!(
                built
                    .changes
                    .iter()
                    .all(|c| c.layer == Layer::Job && c.origin == Origin::Job),
                "diff_built_jobs has a change that is not a job change"
            );
            // Rendered text may use template knowledge; the raw values may not.
            let key = |c: &openjd_model::job::template_diff::Change| {
                let raw = |s: &Option<openjd_model::job::template_diff::Shown>| {
                    s.as_ref().map(|s| s.raw.clone())
                };
                (
                    c.path.clone(),
                    c.field.clone(),
                    raw(&c.before),
                    raw(&c.after),
                )
            };
            let from_templates: Vec<_> = job_layer().map(key).collect();
            let from_jobs: Vec<_> = built.changes.iter().map(key).collect();
            oracle!(
                from_templates == from_jobs,
                "diff_built_jobs and diff_templates report other job changes"
            );
            if small {
                check_template_diff(&built, "diff_built_jobs");
            }
        }
        Err(e) => oracle!(false, "diff_templates fails though both sides build: {e}"),
    }
    match diff_job_template_with_parameter_sets(&template, &in_old, &in_old, &opts) {
        Ok(td) => oracle!(
            !td.is_changed() && td.old_job == a && td.new_job == a,
            "one template under equal parameter sets differs"
        ),
        Err(e) => oracle!(false, "diff_job_template_with_parameter_sets fails: {e}"),
    }
    if new_doc == g.doc {
        match diff_job_template_with_parameter_sets(&template, &in_old, &in_new, &opts) {
            Ok(td) => oracle!(
                td.old_job == a && td.new_job == b,
                "diff_job_template_with_parameter_sets' jobs are not the sides' jobs"
            ),
            Err(e) => oracle!(false, "diff_job_template_with_parameter_sets fails: {e}"),
        }
    }

    // Mutation kind names.
    for &k in MutationKind::ALL {
        oracle!(
            k.name().parse::<MutationKind>() == Ok(k) && k.to_string() == k.name(),
            "MutationKind {k:?} does not round-trip through its name"
        );
    }
    let text: String = u.arbitrary().unwrap_or_default();
    match text.parse::<MutationKind>() {
        Ok(k) => oracle!(k.name() == text, "{text:?} parses as {k}"),
        Err(e) => oracle!(
            e.0 == text && e.to_string() == format!("unknown mutation kind '{text}'"),
            "UnknownMutationKind displays {e}"
        ),
    }
    check_note(&Note {
        step: one_in(&mut u, 2).then(|| text.clone()),
        text: format!("note {text}"),
    });
}
