// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Structural diff of instantiated jobs, steps and environments.
//!
//! **Status: unstable.** This module may change or be removed in any
//! release without notice. [`diff_jobs`], [`diff_steps`] and
//! [`diff_environments`] return a [`DiffTree`] of what differs, where;
//! `specs/model/job-diff.md` gives the semantics.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use openjd_expr::symbol_table::SerializedSymbolTable;
use openjd_expr::value::Float64;
use serde::Serialize;
use serde_json::Value;

use crate::job::{
    Action, EmbeddedFile, Environment, EnvironmentActions, EnvironmentScript, HostRequirements,
    Job, JobParameter, ResolvedChunks, Step, StepActions, StepParameterSpace, StepScript,
    TaskParamRange, TaskParameter,
};
use crate::types::TaskParameterType;

/// How the two sides of a [`DiffNode`] relate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiffStatus {
    /// Both sides are present and equal.
    Unchanged,
    /// Only the new side is present.
    Added,
    /// Only the old side is present.
    Removed,
    /// Both sides are present and differ.
    Modified,
}

/// A field of a [`DiffNode`] whose value differs between the two sides.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FieldChange {
    /// The field's camelCase name, as the job serializes it. An entry of a
    /// keyed collection reads `variables[KEY]`, `amounts[NAME]`,
    /// `attributes[NAME]` or `resolvedSymTab[NAME]`; the relative order of
    /// a named list's shared elements reads `<collection>.order`.
    pub field: String,
    /// The old value as JSON; `null` when the field is absent.
    pub before: Value,
    /// The new value as JSON; `null` when the field is absent.
    pub after: Value,
}

/// One node of a [`DiffTree`]: the comparison of one pair of model values.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DiffNode {
    /// The path segment from the parent, such as `steps[Render]` or
    /// `script`. Empty for the root.
    pub segment: String,
    /// How the two sides relate.
    pub status: DiffStatus,
    /// The node's own fields that differ. Empty unless `Modified`.
    pub field_changes: Vec<FieldChange>,
    /// One node per pair of sub-values, unchanged ones included. Empty
    /// unless `Modified`.
    pub children: Vec<DiffNode>,
}

impl DiffNode {
    /// Whether the two sides differ.
    #[must_use]
    pub fn is_changed(&self) -> bool {
        self.status != DiffStatus::Unchanged
    }

    /// The child whose segment is `segment`.
    #[must_use]
    pub fn child(&self, segment: &str) -> Option<&DiffNode> {
        self.children.iter().find(|c| c.segment == segment)
    }

    fn new(segment: String, status: DiffStatus) -> Self {
        Self {
            segment,
            status,
            field_changes: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Record `name` when `a != b`, writing each side with `write`.
    fn field_with<T: PartialEq + ?Sized>(
        &mut self,
        name: &str,
        a: &T,
        b: &T,
        write: impl Fn(&T) -> Value,
    ) {
        if a != b {
            self.field_changes.push(FieldChange {
                field: name.to_string(),
                before: write(a),
                after: write(b),
            });
        }
    }

    fn field<T: PartialEq + Serialize + ?Sized>(&mut self, name: &str, a: &T, b: &T) {
        self.field_with(name, a, b, to_json);
    }

    /// Pair the elements of two optional named lists by name, old-side
    /// order first, then new-only elements. Records `<collection>.order`
    /// when the shared names are in a different relative order.
    fn match_names<'a, T>(
        &mut self,
        collection: &str,
        a: Option<&'a [T]>,
        b: Option<&'a [T]>,
        name: fn(&T) -> &str,
    ) -> Vec<(&'a str, Option<&'a T>, Option<&'a T>)> {
        let (a, b) = (a.unwrap_or_default(), b.unwrap_or_default());
        let a_by: HashMap<&str, &T> = a.iter().map(|v| (name(v), v)).collect();
        let b_by: HashMap<&str, &T> = b.iter().map(|v| (name(v), v)).collect();
        let shared = |list: &'a [T], other: &HashMap<&str, &T>| -> Vec<&'a str> {
            list.iter()
                .map(name)
                .filter(|k| other.contains_key(k))
                .collect()
        };
        let order = format!("{collection}.order");
        self.field(&order, &shared(a, &b_by), &shared(b, &a_by));
        a.iter()
            .map(|x| (name(x), Some(x), b_by.get(name(x)).copied()))
            .chain(
                b.iter()
                    .filter(|y| !a_by.contains_key(name(y)))
                    .map(|y| (name(y), None, Some(y))),
            )
            .collect()
    }

    /// One child per element of a named list.
    fn named_children<T: PartialEq>(
        &mut self,
        collection: &str,
        a: Option<&[T]>,
        b: Option<&[T]>,
        name: fn(&T) -> &str,
        expand: fn(&mut DiffNode, &T, &T),
    ) {
        for (key, x, y) in self.match_names(collection, a, b, name) {
            self.children
                .extend(pair(format!("{collection}[{key}]"), x, y, expand));
        }
    }

    /// One `<collection>[name]` field change per differing element of a
    /// named list whose elements get no node of their own.
    fn named_fields<T: PartialEq + Serialize>(
        &mut self,
        collection: &str,
        a: Option<&[T]>,
        b: Option<&[T]>,
        name: fn(&T) -> &str,
    ) {
        for (key, x, y) in self.match_names(collection, a, b, name) {
            self.field(&format!("{collection}[{key}]"), &x, &y);
        }
    }
}

/// The result of a diff: a tree of [`DiffNode`]s. The root has no segment,
/// so it prints no line of its own; a tree is what a caller diffs and displays.
///
/// `Display` writes the changed nodes as indented text, in the format
/// `specs/model/job-diff.md` "Display" gives.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffTree {
    root: DiffNode,
}

impl DiffTree {
    /// The root node, which compares the two values passed to the diff.
    #[must_use]
    pub fn root(&self) -> &DiffNode {
        &self.root
    }

    /// Whether the two values differ.
    #[must_use]
    pub fn is_changed(&self) -> bool {
        self.root.is_changed()
    }
}

impl fmt::Display for DiffTree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_node(f, &self.root, 0)
    }
}

fn write_node(f: &mut fmt::Formatter<'_>, node: &DiffNode, depth: usize) -> fmt::Result {
    let indent = "  ".repeat(depth);
    for c in &node.field_changes {
        writeln!(f, "{indent}{}: {} -> {}", c.field, c.before, c.after)?;
    }
    for child in node.children.iter().filter(|c| c.is_changed()) {
        let mark = match child.status {
            DiffStatus::Added => '+',
            DiffStatus::Removed => '-',
            DiffStatus::Unchanged | DiffStatus::Modified => '~',
        };
        writeln!(f, "{indent}{mark} {}", child.segment)?;
        write_node(f, child, depth + 1)?;
    }
    Ok(())
}

/// Diff two jobs.
#[must_use]
pub fn diff_jobs(old: &Job, new: &Job) -> DiffTree {
    DiffTree {
        root: compare(String::new(), old, new, expand_job),
    }
}

/// Diff two steps. The tree has the same shape as the step's subtree in
/// [`diff_jobs`].
#[must_use]
pub fn diff_steps(old: &Step, new: &Step) -> DiffTree {
    DiffTree {
        root: compare(String::new(), old, new, expand_step),
    }
}

/// Diff two environments. The tree has the same shape as an environment's
/// subtree in [`diff_jobs`].
#[must_use]
pub fn diff_environments(old: &Environment, new: &Environment) -> DiffTree {
    DiffTree {
        root: compare(String::new(), old, new, expand_environment),
    }
}

/// Model types always serialize, so the fallback is unreachable for model values.
fn to_json<T: Serialize + ?Sized>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or_else(|e| Value::String(format!("<unserializable: {e}>")))
}

/// A node for a pair that is present on both sides. Only a `Modified` node
/// is expanded.
fn compare<T: PartialEq>(
    segment: String,
    a: &T,
    b: &T,
    expand: fn(&mut DiffNode, &T, &T),
) -> DiffNode {
    if a == b {
        return DiffNode::new(segment, DiffStatus::Unchanged);
    }
    let mut node = DiffNode::new(segment, DiffStatus::Modified);
    expand(&mut node, a, b);
    node
}

fn pair<T: PartialEq>(
    segment: String,
    a: Option<&T>,
    b: Option<&T>,
    expand: fn(&mut DiffNode, &T, &T),
) -> Option<DiffNode> {
    match (a, b) {
        (Some(a), Some(b)) => Some(compare(segment, a, b, expand)),
        (Some(_), None) => Some(DiffNode::new(segment, DiffStatus::Removed)),
        (None, Some(_)) => Some(DiffNode::new(segment, DiffStatus::Added)),
        (None, None) => None,
    }
}

/// Pair two maps' entries by key, in key order.
fn by_key<'a, V>(
    a: impl IntoIterator<Item = (&'a String, &'a V)>,
    b: impl IntoIterator<Item = (&'a String, &'a V)>,
) -> BTreeMap<&'a str, (Option<&'a V>, Option<&'a V>)> {
    let mut pairs: BTreeMap<&str, (Option<&V>, Option<&V>)> = BTreeMap::new();
    for (k, v) in a {
        pairs.entry(k).or_default().0 = Some(v);
    }
    for (k, v) in b {
        pairs.entry(k).or_default().1 = Some(v);
    }
    pairs
}

/// Record one `resolvedSymTab[NAME]` field change per symbol whose entry
/// differs, each side the entry without its `name`, or `null` when absent;
/// or one whole `resolvedSymTab` change when a side is not an array of
/// named entries.
fn symtab_fields(
    n: &mut DiffNode,
    a: &Option<SerializedSymbolTable>,
    b: &Option<SerializedSymbolTable>,
) {
    match (symtab_entries(a), symtab_entries(b)) {
        (Some(x), Some(y)) => {
            for (key, (x, y)) in by_key(&x, &y) {
                n.field(&format!("resolvedSymTab[{key}]"), &x, &y);
            }
        }
        _ => n.field("resolvedSymTab", a, b),
    }
}

/// A symbol table's entries by name, each without its `name`. `None` unless
/// every entry is an object with a string `name`.
fn symtab_entries(t: &Option<SerializedSymbolTable>) -> Option<BTreeMap<String, Value>> {
    let Some(t) = t else {
        return Some(BTreeMap::new());
    };
    t.as_value()
        .as_array()?
        .iter()
        .map(|entry| {
            let mut entry = entry.as_object()?.clone();
            let Value::String(name) = entry.remove("name")? else {
                return None;
            };
            Some((name, Value::Object(entry)))
        })
        .collect()
}

// Each expand_* destructures its type without `..`, so a new model field
// fails to compile here until the diff handles it.

fn expand_job(n: &mut DiffNode, a: &Job, b: &Job) {
    let Job {
        name,
        description,
        extensions,
        parameters,
        steps,
        job_environments,
    } = a;
    n.field("name", name, &b.name);
    n.field("description", description, &b.description);
    n.field("extensions", extensions, &b.extensions);
    for (key, (x, y)) in by_key(parameters, &b.parameters) {
        n.children
            .extend(pair(format!("parameters[{key}]"), x, y, expand_parameter));
    }
    n.named_children(
        "jobEnvironments",
        job_environments.as_deref(),
        b.job_environments.as_deref(),
        |e| &e.name,
        expand_environment,
    );
    n.named_children(
        "steps",
        Some(steps),
        Some(&b.steps),
        |s| &s.name,
        expand_step,
    );
}

fn expand_parameter(n: &mut DiffNode, a: &JobParameter, b: &JobParameter) {
    let JobParameter {
        name,
        param_type,
        value,
    } = a;
    n.field("name", name, &b.name);
    n.field_with("type", param_type, &b.param_type, |t| {
        t.as_spec_str().into()
    });
    n.field_with("value", value, &b.value, |v| v.transport_value());
}

fn expand_step(n: &mut DiffNode, a: &Step, b: &Step) {
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
    n.field("name", name, &b.name);
    n.field("description", description, &b.description);
    n.field("dependencies", dependencies, &b.dependencies);
    symtab_fields(n, resolved_symtab, &b.resolved_symtab);
    n.children.push(compare(
        "script".into(),
        script,
        &b.script,
        expand_step_script,
    ));
    n.named_children(
        "stepEnvironments",
        step_environments.as_deref(),
        b.step_environments.as_deref(),
        |e| &e.name,
        expand_environment,
    );
    n.children.extend(pair(
        "parameterSpace".into(),
        parameter_space.as_ref(),
        b.parameter_space.as_ref(),
        expand_parameter_space,
    ));
    n.children.extend(pair(
        "hostRequirements".into(),
        host_requirements.as_ref(),
        b.host_requirements.as_ref(),
        expand_host_requirements,
    ));
}

fn expand_step_script(n: &mut DiffNode, a: &StepScript, b: &StepScript) {
    let StepScript {
        let_bindings,
        actions: StepActions { on_run },
        embedded_files,
    } = a;
    n.field("let", let_bindings, &b.let_bindings);
    n.children.push(compare(
        "onRun".into(),
        on_run,
        &b.actions.on_run,
        expand_action,
    ));
    embedded_file_children(n, embedded_files, &b.embedded_files);
}

fn expand_environment(n: &mut DiffNode, a: &Environment, b: &Environment) {
    let Environment {
        name,
        description,
        script,
        variables,
        resolved_symtab,
    } = a;
    n.field("name", name, &b.name);
    n.field("description", description, &b.description);
    for (key, (x, y)) in by_key(variables.iter().flatten(), b.variables.iter().flatten()) {
        n.field(&format!("variables[{key}]"), &x, &y);
    }
    symtab_fields(n, resolved_symtab, &b.resolved_symtab);
    n.children.extend(pair(
        "script".into(),
        script.as_ref(),
        b.script.as_ref(),
        expand_environment_script,
    ));
}

fn expand_environment_script(n: &mut DiffNode, a: &EnvironmentScript, b: &EnvironmentScript) {
    let EnvironmentScript {
        let_bindings,
        actions:
            EnvironmentActions {
                on_enter,
                on_wrap_env_enter,
                on_wrap_task_run,
                on_wrap_env_exit,
                on_exit,
            },
        embedded_files,
    } = a;
    n.field("let", let_bindings, &b.let_bindings);
    let bact = &b.actions;
    for (segment, x, y) in [
        ("onEnter", on_enter, &bact.on_enter),
        ("onWrapEnvEnter", on_wrap_env_enter, &bact.on_wrap_env_enter),
        ("onWrapTaskRun", on_wrap_task_run, &bact.on_wrap_task_run),
        ("onWrapEnvExit", on_wrap_env_exit, &bact.on_wrap_env_exit),
        ("onExit", on_exit, &bact.on_exit),
    ] {
        n.children
            .extend(pair(segment.into(), x.as_ref(), y.as_ref(), expand_action));
    }
    embedded_file_children(n, embedded_files, &b.embedded_files);
}

fn embedded_file_children(
    n: &mut DiffNode,
    a: &Option<Vec<EmbeddedFile>>,
    b: &Option<Vec<EmbeddedFile>>,
) {
    n.named_children(
        "embeddedFiles",
        a.as_deref(),
        b.as_deref(),
        |f| &f.name,
        expand_embedded_file,
    );
}

fn expand_action(n: &mut DiffNode, a: &Action, b: &Action) {
    let Action {
        command,
        args,
        timeout,
        cancelation,
    } = a;
    n.field("command", command, &b.command);
    n.field("args", args, &b.args);
    n.field("timeout", timeout, &b.timeout);
    n.field("cancelation", cancelation, &b.cancelation);
}

fn expand_embedded_file(n: &mut DiffNode, a: &EmbeddedFile, b: &EmbeddedFile) {
    let EmbeddedFile {
        name,
        file_type,
        filename,
        data,
        runnable,
        end_of_line,
    } = a;
    n.field("name", name, &b.name);
    n.field("type", file_type, &b.file_type);
    n.field("filename", filename, &b.filename);
    n.field("data", data, &b.data);
    n.field("runnable", runnable, &b.runnable);
    n.field("endOfLine", end_of_line, &b.end_of_line);
}

fn expand_parameter_space(n: &mut DiffNode, a: &StepParameterSpace, b: &StepParameterSpace) {
    let StepParameterSpace {
        task_parameter_definitions,
        combination,
    } = a;
    n.field("combination", combination, &b.combination);
    for (key, (x, y)) in by_key(task_parameter_definitions, &b.task_parameter_definitions) {
        n.children.extend(pair(
            format!("taskParameterDefinitions[{key}]"),
            x,
            y,
            expand_task_parameter,
        ));
    }
}

fn expand_task_parameter(n: &mut DiffNode, a: &TaskParameter, b: &TaskParameter) {
    let (a_type, a_range, a_chunks) = task_parameter_parts(a);
    let (b_type, b_range, b_chunks) = task_parameter_parts(b);
    n.field_with("type", &a_type, &b_type, |t| t.as_spec_str().into());
    n.field_with("range", &a_range, &b_range, Range::to_json);
    n.field("chunks", &a_chunks, &b_chunks);
}

/// A task parameter's range, compared by value. A range expression's
/// `PartialEq` ignores how it is displayed; its JSON is its display string.
#[derive(PartialEq)]
enum Range<'a> {
    Int(&'a TaskParamRange<i64>),
    Float(&'a [Float64]),
    Text(&'a [String]),
}

impl Range<'_> {
    fn to_json(&self) -> Value {
        match self {
            Range::Int(TaskParamRange::List(v)) => to_json(v),
            Range::Int(TaskParamRange::RangeExpr(r)) => Value::String(r.to_string()),
            Range::Float(v) => to_json(v),
            Range::Text(v) => to_json(v),
        }
    }
}

/// A task parameter as its type, its range and its chunks.
fn task_parameter_parts(
    p: &TaskParameter,
) -> (TaskParameterType, Range<'_>, Option<&ResolvedChunks>) {
    match p {
        TaskParameter::Int { range, chunks } => {
            (TaskParameterType::Int, Range::Int(range), chunks.as_ref())
        }
        TaskParameter::Float { range } => (TaskParameterType::Float, Range::Float(range), None),
        TaskParameter::String { range } => (TaskParameterType::String, Range::Text(range), None),
        TaskParameter::Path { range } => (TaskParameterType::Path, Range::Text(range), None),
        TaskParameter::ChunkInt { range, chunks } => {
            (TaskParameterType::ChunkInt, Range::Int(range), Some(chunks))
        }
    }
}

fn expand_host_requirements(n: &mut DiffNode, a: &HostRequirements, b: &HostRequirements) {
    let HostRequirements {
        amounts,
        attributes,
    } = a;
    n.named_fields("amounts", amounts.as_deref(), b.amounts.as_deref(), |r| {
        &r.name
    });
    n.named_fields(
        "attributes",
        attributes.as_deref(),
        b.attributes.as_deref(),
        |r| &r.name,
    );
}
