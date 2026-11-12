// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for `job::apply`. The oracle: applying `diff(a, b)` to a store
//! holding `a` leaves it holding `b`.

use std::collections::BTreeMap;

use crate::error::ModelError;
use crate::job::apply::{
    apply, execute, plan, validate, Action, AllowList, ApplyError, JobApplier, MutationKind, Op,
    TaskChange,
};
use crate::job::{Environment, Job, JobParameter, Step, StepParameterSpace};
use crate::step_param_space::StepParameterSpaceIterator;
use crate::types::TaskParameterSet;
use crate::{
    create_job, decode_job_template, preprocess_job_parameters, CallerLimits,
    JobParameterInputValues,
};
use openjd_expr::ExprValue;

fn job(yaml: &str, params: &[(&str, &str)]) -> Job {
    let v: serde_json::Value = serde_saphyr::from_str(yaml).unwrap();
    let exts: &[&str] = &["TASK_CHUNKING"];
    let jt = decode_job_template(v, Some(exts), &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    for (k, val) in params {
        input.insert((*k).into(), ExprValue::String((*val).into()));
    }
    let values = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &crate::PathParameterOptions {
            job_template_dir: "/tmp",
            current_working_dir: "/tmp",
            path_format: openjd_expr::path_mapping::PathFormat::Posix,
            allow_template_dir_walk_up: true,
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    create_job(&jt, &values, &jt.default_validation_context()).unwrap()
}

fn key(t: &TaskParameterSet) -> String {
    let mut kv: Vec<String> = t
        .iter()
        .map(|(k, v)| format!("{k}={}", serde_json::to_string(&v.value).unwrap()))
        .collect();
    kv.sort();
    kv.join(",")
}

fn tasks(space: Option<&StepParameterSpace>) -> Vec<String> {
    match space {
        None => vec![String::new()],
        Some(s) => StepParameterSpaceIterator::new(s)
            .unwrap()
            .map(|t| key(&t))
            .collect(),
    }
}

/// An in-memory store. Like a real adapter it maps the new job's step
/// indices to its own ids (here: names) once, up front.
#[derive(Debug)]
struct JobRebuilder {
    name: String,
    description: Option<String>,
    extensions: Option<Vec<crate::types::ModelExtension>>,
    params: BTreeMap<String, JobParameter>,
    envs: Vec<Environment>,
    steps: BTreeMap<String, Step>,
    tasks: BTreeMap<String, Vec<String>>,
    /// Steps whose tasks were appended to or removed from: their store order
    /// is allowed to differ from the new iterator order.
    boxed: std::collections::BTreeSet<String>,
    new_names: Vec<String>,
    /// Step rows in store order.
    order: Vec<String>,
    /// Added steps, placed at their final index on commit: adds arrive in
    /// topological order, so an index can be past the current end.
    pending: Vec<(usize, String)>,
}

impl JobRebuilder {
    fn holding(old: &Job, new_names: Vec<String>) -> Self {
        Self {
            name: old.name.clone(),
            description: old.description.clone(),
            extensions: old.extensions.clone(),
            params: old
                .parameters
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            envs: old.job_environments.iter().flatten().cloned().collect(),
            steps: old
                .steps
                .iter()
                .map(|s| (s.name.clone(), s.clone()))
                .collect(),
            tasks: old
                .steps
                .iter()
                .map(|s| (s.name.clone(), tasks(s.parameter_space.as_ref())))
                .collect(),
            boxed: Default::default(),
            new_names,
            order: old.steps.iter().map(|s| s.name.clone()).collect(),
            pending: Vec::new(),
        }
    }

    fn assert_holds(&self, new: &Job) {
        assert_eq!(self.name, new.name);
        assert_eq!(self.description, new.description);
        assert_eq!(self.extensions, new.extensions);
        let params: BTreeMap<_, _> = new
            .parameters
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        assert_eq!(self.params, params, "parameters");
        let envs: Vec<Environment> = new.job_environments.iter().flatten().cloned().collect();
        assert_eq!(self.envs, envs, "job environments, in order");
        let steps: BTreeMap<_, _> = new
            .steps
            .iter()
            .map(|s| (s.name.clone(), s.clone()))
            .collect();
        assert_eq!(self.steps, steps, "steps");
        assert_eq!(self.order, self.new_names, "step order");
        for s in &new.steps {
            // `Step ==` ignores definition order; it decides task order only
            // without a combination.
            let order = |s: &Step| {
                s.parameter_space
                    .as_ref()
                    .filter(|p| crate::job::task_delta::definitions_fix_task_order(p))
                    .map(|p| {
                        p.task_parameter_definitions
                            .keys()
                            .cloned()
                            .collect::<Vec<_>>()
                    })
            };
            assert_eq!(
                order(&self.steps[&s.name]),
                order(s),
                "definition order of {}",
                s.name
            );
            let have = self.tasks[&s.name].clone();
            let want = tasks(s.parameter_space.as_ref());
            if self.boxed.contains(&s.name) {
                let (mut h, mut w) = (have, want);
                h.sort();
                w.sort();
                assert_eq!(h, w, "task set of {}", s.name);
            } else {
                assert_eq!(have, want, "tasks of {}, in order", s.name);
            }
        }
        assert_eq!(
            self.tasks.len(),
            new.steps.len(),
            "no tasks left for removed steps"
        );
    }
}

impl JobApplier for JobRebuilder {
    type Error = String;

    fn job(&mut self, op: Op<'_, Job>) -> Result<(), String> {
        let Op::Modify { new, .. } = op else {
            return Err("job op must be Modify".into());
        };
        self.name = new.name.clone();
        self.description = new.description.clone();
        self.extensions = new.extensions.clone();
        // A reorder: put the environments both sides share in the new order.
        let order: Vec<&String> = new
            .job_environments
            .iter()
            .flatten()
            .map(|e| &e.name)
            .collect();
        self.envs.sort_by_key(|e| {
            order
                .iter()
                .position(|n| **n == e.name)
                .unwrap_or(usize::MAX)
        });
        // Likewise the shared steps.
        let names = &self.new_names;
        self.order
            .sort_by_key(|s| names.iter().position(|n| n == s).unwrap_or(usize::MAX));
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
    fn parameter(&mut self, op: Op<'_, JobParameter>) -> Result<(), String> {
        match op {
            Op::Add { new } | Op::Modify { new, .. } => {
                self.params.insert(new.name.clone(), new.clone());
            }
            Op::Remove { old } => {
                self.params.remove(&old.name);
            }
        }
        Ok(())
    }
    fn environment(&mut self, i: usize, op: Op<'_, Environment>) -> Result<(), String> {
        match op {
            Op::Add { new } => {
                if i > self.envs.len() {
                    return Err(format!("insert {} at {i} past the end", new.name));
                }
                self.envs.insert(i, new.clone());
            }
            Op::Modify { new, .. } => {
                let at = self
                    .envs
                    .iter()
                    .position(|e| e.name == new.name)
                    .ok_or("modify a missing env")?;
                if at != i {
                    return Err(format!("{} is at {at}, op says {i}", new.name));
                }
                self.envs[at] = new.clone();
            }
            Op::Remove { old } => {
                let at = self
                    .envs
                    .iter()
                    .position(|e| e.name == old.name)
                    .ok_or_else(|| format!("remove missing {}", old.name))?;
                self.envs.remove(at);
            }
        }
        Ok(())
    }
    fn step(&mut self, j: usize, op: Op<'_, Step>) -> Result<(), String> {
        match op {
            Op::Add { new } | Op::Modify { new, .. } => {
                if self.new_names.get(j) != Some(&new.name) {
                    return Err(format!("step index {j} is not {}", new.name));
                }
                for d in new.dependencies.iter().flatten() {
                    if !self.steps.contains_key(&d.depends_on) {
                        return Err(format!("{} depends on missing {}", new.name, d.depends_on));
                    }
                }
                if matches!(op, Op::Add { .. }) {
                    self.pending.push((j, new.name.clone()));
                }
                self.steps.insert(new.name.clone(), new.clone());
            }
            Op::Remove { old } => {
                let dependents: Vec<&String> = self
                    .steps
                    .values()
                    .filter(|s| {
                        s.dependencies
                            .iter()
                            .flatten()
                            .any(|d| d.depends_on == old.name)
                    })
                    .map(|s| &s.name)
                    .collect();
                if !dependents.is_empty() {
                    return Err(format!("{} still has dependents {dependents:?}", old.name));
                }
                self.steps.remove(&old.name);
                self.tasks.remove(&old.name);
                self.order.retain(|n| *n != old.name);
            }
        }
        Ok(())
    }
    fn tasks(
        &mut self,
        j: usize,
        space: Option<&StepParameterSpace>,
        change: &TaskChange,
    ) -> Result<(), String> {
        let name = self.new_names.get(j).ok_or("bad step index")?.clone();
        let step = self.steps.get_mut(&name).ok_or("tasks before their step")?;
        step.parameter_space = space.cloned();
        if matches!(change, TaskChange::Boxes(_)) {
            self.boxed.insert(name.clone());
        }
        let list = self.tasks.entry(name).or_default();
        match change {
            TaskChange::Boxes(d) => {
                for b in &d.removed {
                    for t in b.tasks().map_err(|e| e.to_string())? {
                        let k = key(&t);
                        let at = list
                            .iter()
                            .position(|x| *x == k)
                            .ok_or("remove a missing task")?;
                        list.remove(at);
                    }
                }
                for b in &d.added {
                    for t in b.tasks().map_err(|e| e.to_string())? {
                        list.push(key(&t));
                    }
                }
            }
            TaskChange::All | TaskChange::Replace => *list = tasks(space),
        }
        Ok(())
    }
}

fn roundtrip(a: &Job, b: &Job) {
    let mut r = JobRebuilder::holding(a, b.steps.iter().map(|s| s.name.clone()).collect());
    let all = AllowList::of(plan(a, b).unwrap().kinds());
    apply(a, b, &all, &mut r).unwrap();
    r.assert_holds(b);
}

const V1: &str = include_str!("../examples/job_diff/v1.yaml");
const V2: &str = include_str!("../examples/job_diff/v2.yaml");
const CAMS1: &str = include_str!("../examples/job_diff/cams_v1.yaml");
const CAMS2: &str = include_str!("../examples/job_diff/cams_v2.yaml");

#[test]
fn oracle_shot_render_v1_to_v2_and_back() {
    let (a, b) = (job(V1, &[]), job(V2, &[]));
    roundtrip(&a, &b);
    roundtrip(&b, &a);
}

#[test]
fn oracle_parameter_only_changes() {
    let a = job(V1, &[]);
    roundtrip(&a, &job(V1, &[("Frames", "0-10")]));
    roundtrip(&a, &job(V1, &[("Frames", "1-5")]));
    roundtrip(&a, &job(V1, &[("Scene", "shot020.blend")]));
    roundtrip(&a, &a.clone());
}

#[test]
fn oracle_two_dimensional_tasks() {
    let (a, b) = (job(CAMS1, &[]), job(CAMS2, &[]));
    roundtrip(&a, &b);
    roundtrip(&b, &a);
}

#[test]
fn no_change_is_an_empty_plan() {
    let a = job(V1, &[]);
    let p = plan(&a, &a).unwrap();
    assert!(p.ops.is_empty());
    assert!(!p.diff.is_changed());
}

fn summary(p: &crate::job::apply::ApplyPlan<'_>, old: &Job, new: &Job) -> Vec<String> {
    p.ops
        .iter()
        .map(|o| match &o.action {
            Action::Job(_) => "job".to_string(),
            Action::Parameter(op) => format!("param {}", name_of(op, |p| &p.name)),
            Action::Environment(_, op) => format!("env {}", name_of(op, |e| &e.name)),
            Action::Step(i, op) => {
                let verb = match op {
                    Op::Add { .. } => "add",
                    Op::Modify { .. } => "modify",
                    Op::Remove { .. } => "remove",
                };
                let n = match op {
                    Op::Remove { .. } => &old.steps[*i].name,
                    _ => &new.steps[*i].name,
                };
                format!("step {verb} {n}")
            }
            Action::Tasks { step, change, .. } => {
                let c = match change {
                    TaskChange::Boxes(d) => format!(
                        "+{} -{}",
                        d.added.iter().map(|b| b.len()).sum::<u64>(),
                        d.removed.iter().map(|b| b.len()).sum::<u64>()
                    ),
                    TaskChange::All => "all".into(),
                    TaskChange::Replace => "replace".into(),
                };
                format!("tasks {} {c}", new.steps[*step].name)
            }
        })
        .collect()
}

fn name_of<'a, T>(op: &Op<'a, T>, f: impl Fn(&'a T) -> &'a String) -> String {
    match *op {
        Op::Add { new } | Op::Modify { new, .. } => f(new).clone(),
        Op::Remove { old } => f(old).clone(),
    }
}

#[test]
fn plan_order_and_kinds_for_shot_render() {
    let (a, b) = (job(V1, &[]), job(V2, &[]));
    let p = plan(&a, &b).unwrap();
    assert_eq!(
        summary(&p, &a, &b),
        vec![
            "param Frames",
            "env Conda",
            "tasks Render +2 -0",
            "step add Denoise",
            "tasks Denoise all",
            "step modify Encode",
            "step remove Cleanup",
        ]
    );
    use MutationKind::*;
    let kinds: Vec<Vec<MutationKind>> = p
        .ops
        .iter()
        .map(|o| o.kinds.iter().copied().collect())
        .collect();
    assert_eq!(
        kinds,
        vec![
            vec![ParameterValue],
            vec![EnvVariables],
            vec![TaskAppend],
            vec![EnvAdd, StepAdd],
            vec![TaskAppend],
            vec![StepScript, StepDependencies],
            vec![StepRemove],
        ]
    );
}

#[test]
fn render_more_frames_needs_only_parameter_value_and_task_append() {
    let (a, b) = (job(V1, &[]), job(V1, &[("Frames", "1-12")]));
    let p = plan(&a, &b).unwrap();
    assert_eq!(
        p.kinds().into_iter().collect::<Vec<_>>(),
        vec![MutationKind::ParameterValue, MutationKind::TaskAppend]
    );
    let phase1 = AllowList::of([
        MutationKind::ParameterValue,
        MutationKind::EnvVariables,
        MutationKind::TaskAppend,
    ]);
    let mut r = JobRebuilder::holding(&a, b.steps.iter().map(|s| s.name.clone()).collect());
    apply(&a, &b, &phase1, &mut r).unwrap();
    r.assert_holds(&b);
}

/// Records writes; never fails unless told to.
#[derive(Default)]
struct Recorder {
    writes: usize,
    veto: bool,
    fail_at: Option<usize>,
}

impl JobApplier for Recorder {
    type Error = String;
    fn check(&mut self, _op: &crate::job::apply::PlannedOp<'_>) -> Result<(), String> {
        if self.veto {
            Err("vetoed".into())
        } else {
            Ok(())
        }
    }
    fn job(&mut self, _: Op<'_, Job>) -> Result<(), String> {
        self.write()
    }
    fn parameter(&mut self, _: Op<'_, JobParameter>) -> Result<(), String> {
        self.write()
    }
    fn environment(&mut self, _: usize, _: Op<'_, Environment>) -> Result<(), String> {
        self.write()
    }
    fn step(&mut self, _: usize, _: Op<'_, Step>) -> Result<(), String> {
        self.write()
    }
    fn tasks(
        &mut self,
        _: usize,
        _: Option<&StepParameterSpace>,
        _: &TaskChange,
    ) -> Result<(), String> {
        self.write()
    }
}

impl Recorder {
    fn write(&mut self) -> Result<(), String> {
        if self.fail_at == Some(self.writes) {
            self.fail_at = None;
            return Err("transient".into());
        }
        self.writes += 1;
        Ok(())
    }
}

#[test]
fn a_refused_kind_writes_nothing_and_names_the_kind() {
    let (a, b) = (job(V1, &[]), job(V2, &[]));
    let phase1 = AllowList::of([
        MutationKind::ParameterValue,
        MutationKind::EnvVariables,
        MutationKind::TaskAppend,
    ]);
    let mut r = Recorder::default();
    match apply(&a, &b, &phase1, &mut r) {
        Err(ApplyError::Refused { op, refusal }) => {
            assert_eq!(op, 3);
            assert_eq!(refusal.kind, MutationKind::EnvAdd);
        }
        other => panic!("expected Refused, got {other:?}"),
    }
    assert_eq!(r.writes, 0);
}

#[test]
fn a_store_veto_writes_nothing() {
    let (a, b) = (job(V1, &[]), job(V2, &[]));
    let p = plan(&a, &b).unwrap();
    let mut r = Recorder {
        veto: true,
        ..Recorder::default()
    };
    assert!(matches!(
        validate(&p, &AllowList::of(p.kinds()), &mut r),
        Err(ApplyError::Store { op: 0, .. })
    ));
    assert_eq!(r.writes, 0);
}

#[test]
fn execution_resumes_after_a_failed_write() {
    let (a, b) = (job(V1, &[]), job(V2, &[]));
    let p = plan(&a, &b).unwrap();
    let mut r = Recorder {
        fail_at: Some(4),
        ..Recorder::default()
    };
    let at = match execute(&p, &mut r, 0) {
        Err(ApplyError::Store { op, .. }) => op,
        other => panic!("expected a store error, got {other:?}"),
    };
    assert_eq!((at, r.writes), (4, 4));
    let report = execute(&p, &mut r, at).unwrap();
    assert_eq!(report.applied, p.ops.len());
    assert_eq!(r.writes, p.ops.len());
}

// ─── audit round 1 regressions ──────────────────────────────────────

const ENVS: &str = r#"
specificationVersion: jobtemplate-2023-09
name: Envs
parameterDefinitions:
  - {name: S, type: STRING, default: a}
jobEnvironments:
  - {name: E1, variables: {X: "1"}}
  - {name: E2, variables: {X: "2"}}
  - {name: E3, variables: {X: "3"}}
steps:
  - name: R
    parameterSpace:
      taskParameterDefinitions:
        - {name: F, type: INT, range: "1-2"}
        - {name: C, type: STRING, range: [l, r]}
    script:
      actions:
        onRun: {command: "{{Param.S}}"}
"#;

fn envs(list: &str) -> String {
    let mut out = String::new();
    for e in list.split(',').filter(|e| !e.is_empty()) {
        let (n, x) = e.split_once('=').unwrap_or((e, e));
        out.push_str(&format!("  - {{name: {n}, variables: {{X: \"{x}\"}}}}\n"));
    }
    let head = ENVS.split("jobEnvironments:\n").next().unwrap();
    let tail = &ENVS[ENVS.index_of("steps:")..];
    format!("{head}jobEnvironments:\n{out}{tail}")
}

trait IndexOf {
    fn index_of(&self, pat: &str) -> usize;
}
impl IndexOf for str {
    fn index_of(&self, pat: &str) -> usize {
        self.find(pat).unwrap()
    }
}

#[test]
fn job_environment_positions_are_rebuilt_exactly() {
    let cases = [
        ("E1", "E2,E1"),
        ("E1", "E1,E2"),
        ("E1,E2,E3", "E3,E1"),
        ("E1,E2,E3", "E4,E2,E5"),
        ("E1,E2", "E2,E1,E3"),
        ("E1,E2,E3", ""),
    ];
    for (from, to) in cases {
        let (a, b) = (job(&envs(from), &[]), job(&envs(to), &[]));
        roundtrip(&a, &b);
        roundtrip(&b, &a);
    }
    // The two inserts are distinguishable by index.
    let a = job(&envs("E1"), &[]);
    let (b1, b2) = (job(&envs("E2,E1"), &[]), job(&envs("E1,E2"), &[]));
    let p1 = plan(&a, &b1).unwrap();
    let p2 = plan(&a, &b2).unwrap();
    assert_ne!(p1.ops, p2.ops);
}

#[test]
fn a_parameter_seen_only_through_the_symtab_is_still_written() {
    let (a, b) = (job(ENVS, &[]), job(ENVS, &[("S", "b")]));
    let p = plan(&a, &b).unwrap();
    assert!(
        p.kinds().contains(&MutationKind::StepSymbols),
        "{:?}",
        p.kinds()
    );
    roundtrip(&a, &b);
}

#[test]
fn a_definition_reorder_replaces_the_tasks() {
    let swapped = ENVS.replace(
        "        - {name: F, type: INT, range: \"1-2\"}\n        - {name: C, type: STRING, range: [l, r]}\n",
        "        - {name: C, type: STRING, range: [l, r]}\n        - {name: F, type: INT, range: \"1-2\"}\n",
    );
    let (a, b) = (job(ENVS, &[]), job(&swapped, &[]));
    let p = plan(&a, &b).unwrap();
    assert_eq!(
        p.kinds().into_iter().collect::<Vec<_>>(),
        vec![MutationKind::TaskReplace]
    );
    roundtrip(&a, &b);
    each_kind_refused(&a, &b);
}

#[test]
fn every_op_carries_at_least_one_kind() {
    let pairs = [
        (job(V1, &[]), job(V2, &[])),
        (job(CAMS1, &[]), job(CAMS2, &[])),
        (job(ENVS, &[]), job(&envs("E3,E9"), &[("S", "z")])),
    ];
    for (a, b) in &pairs {
        for p in [plan(a, b).unwrap(), plan(b, a).unwrap()] {
            assert!(p.ops.iter().all(|o| !o.kinds.is_empty()), "{:?}", p.ops);
        }
    }
}

// ─── audit round 2 regressions ──────────────────────────────────────

fn one_step(space: &str) -> String {
    format!(
        r#"
specificationVersion: jobtemplate-2023-09
name: One
steps:
  - name: S
    parameterSpace:
{space}
    script:
      actions:
        onRun: {{command: run}}
"#
    )
}

const FG: &str = "      taskParameterDefinitions:\n        - {name: F, type: INT, range: \"1-2\"}\n        - {name: G, type: STRING, range: [a, b]}\n";

#[test]
fn a_combination_that_reorders_dimensions_replaces_the_tasks() {
    let none = one_step(FG);
    let gf = one_step(&format!("{FG}      combination: \"G * F\"\n"));
    let fg = one_step(&format!("{FG}      combination: \"F * G\"\n"));
    for (x, y) in [(&none, &gf), (&fg, &gf)] {
        let (a, b) = (job(x, &[]), job(y, &[]));
        let p = plan(&a, &b).unwrap();
        assert!(
            p.kinds().contains(&MutationKind::TaskReplace),
            "{:?}",
            p.kinds()
        );
        roundtrip(&a, &b);
        roundtrip(&b, &a);
    }
}

#[test]
fn a_space_rewrite_with_the_same_tasks_still_writes_the_step() {
    let range = one_step(FG);
    let list = one_step(&FG.replace("range: \"1-2\"", "range: [1, 2]"));
    let (a, b) = (job(&range, &[]), job(&list, &[]));
    let p = plan(&a, &b).unwrap();
    assert_eq!(
        p.kinds().into_iter().collect::<Vec<_>>(),
        vec![MutationKind::StepParameterSpace]
    );
    roundtrip(&a, &b);
    roundtrip(&b, &a);
    each_kind_refused(&a, &b);
    // `F * G` written out is the default order: same tasks, same order.
    let fg = one_step(&format!("{FG}      combination: \"F * G\"\n"));
    roundtrip(&a, &job(&fg, &[]));
}

#[test]
fn adding_or_removing_a_step_with_step_environments_carries_env_kinds() {
    let (a, b) = (job(V1, &[]), job(V2, &[]));
    // Denoise has a step environment.
    let p = plan(&a, &b).unwrap();
    let add = p
        .ops
        .iter()
        .find(|o| matches!(o.action, Action::Step(_, Op::Add { .. })))
        .unwrap();
    assert!(add.kinds.contains(&MutationKind::EnvAdd), "{:?}", add.kinds);
    let p = plan(&b, &a).unwrap();
    let rm = p
        .ops
        .iter()
        .find(|o| matches!(o.action, Action::Step(_, Op::Remove { .. })))
        .unwrap();
    assert!(
        rm.kinds.contains(&MutationKind::EnvRemove),
        "{:?}",
        rm.kinds
    );
}

// ─── kind classification (one edit per row) ─────────────────────────

const KINDS: &str = r#"
specificationVersion: jobtemplate-2023-09
name: Kinds
parameterDefinitions:
  - {name: P, type: STRING, default: a}
jobEnvironments:
  - {name: J1, description: d, variables: {V: "{{Param.P}}"}}
  - {name: J2, variables: {V: "2"}}
steps:
  - name: A
    stepEnvironments:
      - {name: SE1, variables: {V: "1"}}
      - {name: SE2, variables: {V: "2"}}
    hostRequirements:
      amounts:
        - {name: amount.worker.vcpu, min: 2}
    script:
      actions:
        onRun: {command: a}
  - name: B
    script:
      actions:
        onRun: {command: b}
"#;

fn kinds_for(edit: impl Fn(&str) -> String, params: &[(&str, &str)]) -> Vec<MutationKind> {
    let (a, b) = (job(KINDS, &[]), job(&edit(KINDS), params));
    roundtrip(&a, &b);
    each_kind_refused(&a, &b).into_iter().collect()
}

/// For every kind in `plan(a, b)`, a policy allowing all the others refuses
/// that kind with no writes. Returns the plan's kinds.
fn each_kind_refused(a: &Job, b: &Job) -> std::collections::BTreeSet<MutationKind> {
    let kinds = plan(a, b).unwrap().kinds();
    for k in &kinds {
        let others = AllowList::of(kinds.iter().copied().filter(|o| o != k));
        let mut r = Recorder::default();
        match apply(a, b, &others, &mut r) {
            Err(ApplyError::Refused { refusal, .. }) => assert_eq!(refusal.kind, *k),
            other => panic!("{k:?} not refused: {other:?}"),
        }
        assert_eq!(r.writes, 0);
    }
    kinds
}

#[test]
fn each_kind_comes_from_its_own_edit() {
    use MutationKind::*;
    let same = |t: &str| t.to_string();
    let cases: Vec<(&str, Vec<MutationKind>, Vec<MutationKind>)> = vec![
        (
            "job env reads P",
            kinds_for(same, &[("P", "b")]),
            vec![ParameterValue, EnvSymbols],
        ),
        (
            "job env description",
            kinds_for(|t| t.replace("description: d", "description: e"), &[]),
            vec![EnvMetadata],
        ),
        (
            "job envs reordered",
            kinds_for(
                |t| {
                    t.replace(
                        "  - {name: J1, description: d, variables: {V: \"{{Param.P}}\"}}\n  - {name: J2, variables: {V: \"2\"}}\n",
                        "  - {name: J2, variables: {V: \"2\"}}\n  - {name: J1, description: d, variables: {V: \"{{Param.P}}\"}}\n",
                    )
                },
                &[],
            ),
            vec![EnvReorder],
        ),
        (
            "steps reordered",
            kinds_for(
                |t| {
                    let (head, rest) = t.split_once("  - name: A\n").unwrap();
                    let (a, b) = rest.split_once("  - name: B\n").unwrap();
                    format!("{head}  - name: B\n{b}  - name: A\n{a}")
                },
                &[],
            ),
            vec![StepReorder],
        ),
        (
            "extensions",
            kinds_for(
                |t| {
                    t.replace(
                        "name: Kinds\n",
                        "name: Kinds\nextensions: [TASK_CHUNKING]\n",
                    )
                },
                &[],
            ),
            vec![JobExtensions],
        ),
        (
            "host requirements",
            kinds_for(|t| t.replace("min: 2}", "min: 4}"), &[]),
            vec![StepHostRequirements],
        ),
        (
            "step envs reordered",
            kinds_for(
                |t| {
                    t.replace(
                        "      - {name: SE1, variables: {V: \"1\"}}\n      - {name: SE2, variables: {V: \"2\"}}\n",
                        "      - {name: SE2, variables: {V: \"2\"}}\n      - {name: SE1, variables: {V: \"1\"}}\n",
                    )
                },
                &[],
            ),
            vec![EnvReorder],
        ),
        (
            "step env added",
            kinds_for(
                |t| {
                    t.replace(
                        "      - {name: SE2, variables: {V: \"2\"}}\n",
                        "      - {name: SE2, variables: {V: \"2\"}}\n      - {name: SE3, variables: {V: \"3\"}}\n",
                    )
                },
                &[],
            ),
            vec![EnvAdd],
        ),
        (
            "step env removed",
            kinds_for(
                |t| t.replace("      - {name: SE2, variables: {V: \"2\"}}\n", ""),
                &[],
            ),
            vec![EnvRemove],
        ),
        (
            "step env variable",
            kinds_for(
                |t| {
                    t.replace(
                        "{name: SE1, variables: {V: \"1\"}}",
                        "{name: SE1, variables: {V: \"9\"}}",
                    )
                },
                &[],
            ),
            vec![EnvVariables],
        ),
        (
            "job name",
            kinds_for(|t| t.replace("name: Kinds", "name: Kinds2"), &[]),
            vec![JobMetadata],
        ),
        (
            "job env gains a script",
            kinds_for(
                |t| {
                    t.replace(
                        "{name: J2, variables: {V: \"2\"}}",
                        "{name: J2, variables: {V: \"2\"}, script: {actions: {onEnter: {command: e}}}}",
                    )
                },
                &[],
            ),
            vec![EnvScript],
        ),
        (
            "step description",
            kinds_for(
                |t| t.replace("  - name: B\n", "  - name: B\n    description: x\n"),
                &[],
            ),
            vec![StepMetadata],
        ),
        (
            "parameter added",
            kinds_for(
                |t| {
                    t.replace(
                        "  - {name: P, type: STRING, default: a}\n",
                        "  - {name: P, type: STRING, default: a}\n  - {name: Q, type: STRING, default: q}\n",
                    )
                },
                &[],
            ),
            vec![ParameterAdd],
        ),
    ];
    for (what, got, want) in cases {
        assert_eq!(got, want, "{what}");
    }
    // The reverse of the add.
    let q = KINDS.replace(
        "  - {name: P, type: STRING, default: a}\n",
        "  - {name: P, type: STRING, default: a}\n  - {name: Q, type: STRING, default: q}\n",
    );
    let removed = each_kind_refused(&job(&q, &[]), &job(KINDS, &[]));
    assert_eq!(
        removed.into_iter().collect::<Vec<_>>(),
        vec![ParameterRemove]
    );
}

#[test]
fn a_step_named_like_a_parameter_is_not_a_parameter_change() {
    let t = KINDS.replace("  - name: B\n", "  - name: P\n");
    let (a, b) = (
        job(&t, &[]),
        job(&t.replace("command: b}", "command: b2}"), &[]),
    );
    let p = plan(&a, &b).unwrap();
    assert_eq!(
        p.kinds().into_iter().collect::<Vec<_>>(),
        vec![MutationKind::StepScript]
    );
    roundtrip(&a, &b);
}

// ─── audit round 4 regressions ──────────────────────────────────────

/// Steps from `"A>B,B,C@2"`: `A` depends on `B`; `@2` changes `C`'s command.
fn graph(steps: &str, envs: &str) -> String {
    let mut out = String::from("specificationVersion: jobtemplate-2023-09\nname: G\n");
    if !envs.is_empty() {
        out.push_str("jobEnvironments:\n");
        for e in envs.split(',') {
            out.push_str(&format!("  - {{name: {e}, variables: {{X: \"1\"}}}}\n"));
        }
    }
    out.push_str("steps:\n");
    for s in steps.split(',') {
        let (s, cmd) = s.split_once('@').unwrap_or((s, ""));
        let mut parts = s.split('>');
        let name = parts.next().unwrap();
        out.push_str(&format!("  - name: {name}\n"));
        let deps: Vec<&str> = parts.collect();
        if !deps.is_empty() {
            out.push_str("    dependencies:\n");
            for d in deps {
                out.push_str(&format!("      - {{dependsOn: {d}}}\n"));
            }
        }
        out.push_str(&format!(
            "    script:\n      actions:\n        onRun: {{command: run{cmd}}}\n"
        ));
    }
    out
}

#[test]
fn adds_follow_dependencies_not_step_index() {
    // A depends on B, which comes after it: index order would add A first.
    // Reversed, the removes must go dependents first (A before B).
    for (from, to) in [("C", "A>B,B,C"), ("A", "B>C,A,C"), ("A,B>A,C", "C")] {
        let (a, b) = (job(&graph(from, ""), &[]), job(&graph(to, ""), &[]));
        roundtrip(&a, &b);
        roundtrip(&b, &a);
    }
}

#[test]
fn step_adds_carry_their_final_index() {
    // The add of C (index 2) is planned before B (index 0), while the store
    // holds one step: a store places adds by index once the plan is done.
    let (a, b) = (job(&graph("A", ""), &[]), job(&graph("B>C,A,C", ""), &[]));
    let p = plan(&a, &b).unwrap();
    let adds: Vec<usize> = p
        .ops
        .iter()
        .filter_map(|o| match o.action {
            Action::Step(j, Op::Add { .. }) => Some(j),
            _ => None,
        })
        .collect();
    assert_eq!(adds, vec![2, 0]);
    roundtrip(&a, &b);
}

#[test]
fn job_environment_writes_go_in_new_index_order() {
    // The diff lists A (shared) before Z (added); Z goes in front of A, so
    // modifying A first would use an index A does not have yet.
    for (from, to) in [("A=1", "Z=1,A=2"), ("A=1,B=1", "Z=1,A=2,Q=1,B=2")] {
        let (a, b) = (job(&envs(from), &[]), job(&envs(to), &[]));
        roundtrip(&a, &b);
        roundtrip(&b, &a);
    }
}

#[test]
fn a_step_change_is_not_an_environment_op() {
    // Step B and job environment E2 are both at index 1.
    let a = job(&graph("A,B", "E1,E2"), &[]);
    let b = job(&graph("A,B@2", "E1,E2"), &[]);
    let p = plan(&a, &b).unwrap();
    assert!(
        p.ops
            .iter()
            .all(|o| !matches!(o.action, Action::Environment(..))),
        "{:?}",
        p.ops
    );
    assert_eq!(
        p.kinds().into_iter().collect::<Vec<_>>(),
        vec![MutationKind::StepScript]
    );
    roundtrip(&a, &b);
}

#[test]
fn repeated_names_are_refused() {
    let a = job(&graph("A,B", "E1"), &[]);
    let mut env = a.clone();
    let mut e = env.job_environments.as_ref().unwrap()[0].clone();
    e.variables = None;
    env.job_environments.as_mut().unwrap().push(e);
    let mut step = a.clone();
    let mut s = step.steps[1].clone();
    s.script = a.steps[0].script.clone();
    step.steps.push(s);
    let mut step_env = a.clone();
    let se = env.job_environments.as_ref().unwrap()[0].clone();
    step_env.steps[0].step_environments = Some(vec![se.clone(), se]);
    for bad in [&env, &step, &step_env] {
        assert!(matches!(plan(&a, bad), Err(ModelError::ModelValidation(_))));
        assert!(matches!(plan(bad, &a), Err(ModelError::ModelValidation(_))));
    }
}

#[test]
fn a_star_combination_keeps_definition_order() {
    // `*` is the plain product in definition order, so a reorder moves
    // every task. Templates reject a bare `*`; only a built space has one.
    let a = job(&one_step(FG), &[]);
    let mut x = a.clone();
    let mut y = a.clone();
    let star = |j: &mut Job, rev: bool| {
        let s = j.steps[0].parameter_space.as_mut().unwrap();
        s.combination = Some("*".into());
        if rev {
            s.task_parameter_definitions.reverse();
        }
    };
    star(&mut x, false);
    star(&mut y, true);
    let p = plan(&x, &y).unwrap();
    assert!(
        p.kinds().contains(&MutationKind::TaskReplace),
        "{:?}",
        p.kinds()
    );
    roundtrip(&x, &y);
}

#[test]
fn every_kind_is_refused_on_its_own() {
    let pairs = [
        (job(V1, &[]), job(V2, &[])),
        (job(CAMS1, &[]), job(CAMS2, &[])),
        (job(ENVS, &[]), job(&envs("E3,E9"), &[("S", "z")])),
        (
            job(&graph("A,B>A", "E1"), &[]),
            job(&graph("B,C>B@2", "E2"), &[]),
        ),
    ];
    let mut seen = std::collections::BTreeSet::new();
    for (a, b) in &pairs {
        for (x, y) in [(a, b), (b, a)] {
            seen.extend(each_kind_refused(x, y));
        }
    }
    assert!(seen.len() >= 11, "{seen:?}");
}
