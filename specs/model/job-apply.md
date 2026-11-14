# Job Apply (unstable)

**Unstable.** This API may change or be removed in any release without notice. Don't
depend on it outside openjd-rs. Applies to `job::apply`.

`job::apply` turns a [job diff](job-diff.md) into an ordered, gated list of writes against
a store that it knows only through a trait.

## Stages

```rust
pub fn plan<'a>(old: &'a Job, new: &'a Job) -> Result<ApplyPlan<'a>, ModelError>;
pub fn validate<A: JobApplier + ?Sized>(plan: &ApplyPlan<'_>, policy: &dyn MutationPolicy, store: &mut A) -> Result<(), ApplyError<A::Error>>;
pub fn execute<A: JobApplier + ?Sized>(plan: &ApplyPlan<'_>, store: &mut A, resume_from: usize) -> Result<ApplyReport, ApplyError<A::Error>>;
pub fn apply<A: JobApplier + ?Sized>(old: &Job, new: &Job, policy: &dyn MutationPolicy, store: &mut A) -> Result<ApplyReport, ApplyError<A::Error>>;
```

`apply` is `plan`, then `validate`, then `execute(…, 0)`. `A: ?Sized` and an object-safe
`JobApplier` let a caller pass `&mut dyn JobApplier<Error = E>` (see
[Host adaptors](#host-adaptors)).

`plan` diffs the two jobs itself, always with the default `DiffOptions`, and keeps the
tree in `ApplyPlan::diff`. `Op::Modify::node` indexes that tree. A caller's tree built
with `max_depth` or `with_resolved_symtab(false)` can mark a changed step `Unchanged`, so
the plan never takes one.

`plan` refuses, with `ModelError::ModelValidation`, a job that repeats a step name, a job
environment name, or a step environment name within one step. Template validation rejects
these, so only a hand-built `Job` has one. The diff reports the copies as a single
`<collection>.duplicates` field change, which has no op and no kind of its own to gate.

## Plan order

1. `Action::Job`: one `Modify` when the root has its own field changes (name,
   description, extensions, step or job-environment order).
2. `Action::Parameter`: one op per changed parameter: the old job's parameters in name
   order, then the parameters only the new job has, in name order. The diff orders
   parameter nodes this way, so the order does not depend on how either job's map was
   filled (`create_job` fills it from a `HashMap`), and a plan of rebuilt jobs has the
   same ops at the same indices, which `execute(plan, store, op)` relies on to resume.
3. `Action::Environment(index, op)`: one op per changed **job** environment.
   - Removes come first. They are matched by name; the index is the old one and is
     informational.
   - Adds and modifies follow in ascending new index. Inserting each `Add` at its index
     after the removes, and after any `Job` op's `EnvReorder` of the shared environments,
     rebuilds the new list order.

   Step environments travel inside their step's op, because a step is one item in a
   store.
4. Steps in **topological order of the new step graph**, origins first:
   - `Added`: `Step(j, Add)`, then `Tasks(j, TaskOp { change: All, .. })`.
   - `Modified`: `Step(j, Modify)` unless the only change is the parameter space, then
     `Tasks(j, …)` when the task set or task order changed (`task_delta`: `Product` with
     added or removed boxes → `Boxes`, `Fallback` → `Replace`). A change of dimension
     order is a `Fallback`, because every kept task moves, and so is any change to a step
     with a `CHUNK[INT]` dimension. When the parameter space is the step's only change,
     there is no `Step` op: the host writes the step's new parameter space, `TaskOp::space`,
     from the `tasks` call. If the space changed but the
     tasks and their order didn't (e.g. `1-2` rewritten as `[1, 2]`), there is no
     `Tasks` op. The `Step(j, Modify)` is emitted with `StepParameterSpace`, so the store
     still writes the new definition.
5. Removed steps in **reverse topological order of the old step graph**, dependents first.

Every step op names its step with a `StepRef { name, index }`; `j` and `i` above are its
`index`. The name is the step's name in the job the index points into, so a store keyed by
name never resolves the index. The index in `Step(j, Add)` is the step's final position,
`new.steps[j]`. Adds arrive in
topological order, not index order, so `j` can be past the end of the store's list at the
time (`[A]` to `[B (dependsOn C), A, C]` adds C at 2, then B at 0). A store that keeps step
rows in order places the added steps by `j` once the plan is done, in ascending `j`, after
the removes and after any `Job` op's `StepReorder` of the shared steps. `Tasks(j, …)`
also indexes `new.steps`, and needs no `Step` op before it: a store resolves `j` from the
new job, as it does for `Step(j, …)`.

Adds and modifies come before removes. A step therefore exists before any step gains an
edge to it, and by the time a step is removed nothing still depends on it, because the
new job can't name it. `StepIndex` is `DiffNode::step_index`: `new.steps[j]` for steps 4,
and `old.steps[i]` for steps 5.

The planner classifies each modified step and job environment with a full
`diff_steps` / `diff_environments`.

A task-definition reorder under an explicit `combination` produces no op: the
combination fixes task order, and the diff reports such a step `Unchanged`. A store
that persists definition order verbatim keeps the old order. `combination: "*"` is not
explicit: it is the product in definition order, so there a reorder is a `Replace`.

A float parameter whose literal changes form only (`1.0` to `1.00`) compares equal, so it
plans no `ParameterValue` op and the store keeps the old spelling. A step or environment
that reads the parameter still gets its symbol-table kind (see [job diff](job-diff.md)).

## Mutation kinds

Each `PlannedOp` carries a `BTreeSet<MutationKind>`, and a `MutationPolicy` must allow
every one. `AllowList` is the default policy.

| kind | from |
|---|---|
| `JobMetadata`, `JobExtensions`, `StepReorder`, `EnvReorder` | root field changes; `EnvReorder` also from a step's `stepEnvironments` order |
| `ParameterValue`, `ParameterAdd`, `ParameterRemove` | parameter node status |
| `EnvVariables`, `EnvScript`, `EnvMetadata`, `EnvSymbols`, `EnvAdd`, `EnvRemove` | environment fields, `script` child, status (job or step scope) |
| `StepMetadata`, `StepScript`, `StepHostRequirements`, `StepDependencies`, `StepSymbols` | step fields and children |
| `StepParameterSpace` | the space changed with the same tasks in the same order |
| `StepAdd`, `StepRemove` | step status; also `EnvAdd` / `EnvRemove` when that step has step environments |
| `TaskAppend`, `TaskRemove`, `TaskReplace` | `TaskChange`: added boxes, removed boxes, or `Replace`; `All` is `TaskAppend` |

A kind's stable name is its variant name (`MutationKind::name`, also `Display`), and
`FromStr` reads it back, exactly and case-sensitively. `MutationKind::ALL` lists every kind.
`AllowList::from_names` builds a policy from names, such as a host's configuration, and
fails with `UnknownMutationKind` (`unknown mutation kind 'Foo'`) on the first name that is
not a kind.

"Render more frames" (a `Frames` parameter change) plans as `ParameterValue` +
`TaskAppend`, because the step's symbol table doesn't hold the symbols its parameter
space uses.

## Validation and execution

- `validate` asks the policy about every op first, then `JobApplier::check` about every
  op. The first refusal returns `ApplyError::Refused` (the policy: not allowed yet) or
  `ApplyError::Vetoed` (the store: not allowed in its current state) with the op's index,
  and nothing has been written.
- `execute` does not validate. `ApplyPlan::ops` is public, so a caller that edits a plan
  after `validate` must validate it again. `apply` runs both.
- `execute` calls `begin`, the trait method for each op from `resume_from` on, then
  `commit`. A failed write returns `ApplyError::Store { op }`. Ops before `op` were
  applied, so `execute(plan, store, op)` resumes, provided the store's writes are
  idempotent.

## Refusals and errors

A `Refusal` names the refused kind, the op's target (`OpTarget`: the job, a parameter or
job environment by name, or a step by name and index) and a reason. `Refusal::new` fills
the target from the op, for a custom policy; `Refusal`, `OpTarget` and `ApplyError` are
`#[non_exhaustive]`, so a later field or variant is not a breaking change. It displays as `{target}: {reason}`;
`AllowList`'s reason is `{kind} is not allowed`:

| op | message |
|---|---|
| a `Tasks` op of step `Render` | `step Render: TaskReplace is not allowed` |
| a `Parameter` op | `parameter Frames: ParameterValue is not allowed` |
| an `Environment` op | `job environment Conda: EnvVariables is not allowed` |
| a `Job` op | `job: JobMetadata is not allowed` |

`ApplyError<E>` implements `Display` when `E` does, and `std::error::Error` when `E` is
`Debug + Display`, so `Box<dyn Error + Send + Sync>` (which is not itself an `Error`)
works. `Refused` displays as its refusal, `Vetoed` as `{target}: {error}`, `Store` as
`write {op} failed: {error}`, and `Model` as the model error. `source` is the `Refusal`
or the `ModelError`; a store error appears in the message only, since `E` need not be an
`Error`.

## Task identity

A task is identified by its parameter values: its task parameter set, written as
`task_parameter_set_document` writes it (name to `{type, value}`). A task id is materialized
state, so it is the host's: openjd-rs never asks a host to compute a task's position or to
trust one. A host already stores each task's parameter set, so it maps a set to its own
task, and it gives each new task an id of its choosing.

A `TaskChange` names every task it touches by its set:

- `Boxes`: remove the tasks of `ProductDelta::removed`, then add those of
  `ProductDelta::appended`. Every other task is kept, with its id.
- `All` (a step add) and `Replace`: the step's tasks become exactly those of
  `TaskOp::added_tasks`. A host may keep a stored task whose set is in the list, with
  its id, and removes the rest.

`JobApplier::tasks` receives a `TaskOp { space, change }`: the change and the step's new
parameter space it is read against. `TaskOp::added_tasks()` yields the tasks the change
adds, in a deterministic order. For `All` and `Replace` it is every task of the step's new
space in `create_job`'s order: `StepParameterSpaceIterator` with the chunk override
`create_job` counts tasks with (`Some(1)`), or one task with no parameters for a step
without a space. A `CHUNK[INT]` dimension therefore gives one task per value, as CreateJob
creates them: `1-6` with `defaultTaskCount: 3` is six tasks, `"1-1"` to `"6-6"`, not two. A host never enumerates a space itself: a `combination` reorders dimensions
(`combination: "B * A"` with definitions `[A, B]` makes `B` the outer dimension), so
definition order alone is not task order. For `Boxes` it is `ProductDelta::appended`: the
`added` boxes in turn, each rightmost parameter fastest. Box `k` holds the new tasks whose
first value missing from the old space is in dimension `k` (in iteration order), so the
order groups tasks by that dimension and is not the new space's iteration order. For
`Frame` `1-2` × `Cam` `[a, b]` to `1-3` × `[b, c]`:

| order | task | box |
|---|---|---|
| 1 | `Frame=3, Cam=b` | 0 |
| 2 | `Frame=3, Cam=c` | 0 |
| 3 | `Frame=1, Cam=c` | 1 |
| 4 | `Frame=2, Cam=c` | 1 |

`ProductDelta::removed` yields each removed task in ascending order of its index in the old
space, merging the `removed` boxes as it goes, and returns that index beside it. The index
is only a hint: it is the task's index at job creation, so it locates the task only while
the stored job has never been updated. The task change document does not carry it. Both
orders depend only on the two parameter spaces, so every plan of the same jobs, in any
process, yields the same sequences. `appended_count`, `removed_count` and
`TaskOp::added_count` give the sizes without iterating.

## Host adaptors

A host store implements `JobApplier`. A store written in another language does so through
an adaptor: one Rust type that implements the trait by converting each call's arguments to
the host's values and calling the host. The trait is object safe, so the adaptor can be
used as `&mut dyn JobApplier<Error = E>`.

Every payload has a plain-data form that needs nothing outside these types:

| call | argument | plain data |
|---|---|---|
| `check` | `&PlannedOp` | `PlannedOp::as_document`: `{"kinds": [names], "action": "job" \| "parameter" \| "environment" \| "step" \| "tasks", …}` |
| `job`, `parameter`, `environment`, `step` | `Op<'_, T>` | `Op::as_document`: `{"change": "add" \| "modify" \| "remove", "old"?: …, "new"?: …}` |
| `step`, `tasks` | `StepRef` | `Serialize`: `{"name": …, "index": …}` |
| `environment` | `usize` | the index, a number: the new list index for `Add` and `Modify`, the old one for `Remove`; the environment's name is inside the op (`new.name`, `old.name`) |
| `tasks` | `&TaskOp` | `TaskOp::as_document()`: `{"space": …, "change": …, …}`, with `space` the step's new parameter space in its `JobDocument` form (`null` for none) |

No document needs an argument: each is a method of the value the call receives.

Objects inside an op (`Job`, `Step`, `Environment`, `JobParameter`, `StepParameterSpace`,
through `ToDocument`) are written as `JobDocument` writes them, in the
[host encoding](host-support.md#host-encoding): `parameters` and environment `variables`
sorted by name, typed values as `{type, value}`, and `taskParameterDefinitions` a list in
definition order. `Op::Modify::node` is left out,
because it indexes the plan's diff.

`TaskOp::as_document()` holds the step's new parameter space, so one call gives a host
everything the step's tasks need, and names every task by its set, written by
`task_parameter_set_document` (name to `{type, value}`, the value as text so a float keeps
its literal), and holds no positions (see [task identity](#task-identity)). `Frames` `1-2`
to `1-4` is (`space` left out here and below)

```text
{"change": "boxes", "oldCount": 2, "keptCount": 2,
 "appendedCount": 2, "appended": [{"Frame": {"type": "INT", "value": "3"}}, {"Frame": {"type": "INT", "value": "4"}}],
 "removedCount": 0, "removed": []}
```

and `1-4` to `3-6` removes `1` and `2` by value:

```text
{"change": "boxes", "oldCount": 4, "keptCount": 2,
 "appendedCount": 2, "appended": [{"Frame": {"type": "INT", "value": "5"}}, {"Frame": {"type": "INT", "value": "6"}}],
 "removedCount": 2, "removed": [{"Frame": {"type": "INT", "value": "1"}}, {"Frame": {"type": "INT", "value": "2"}}]}
```

`All` and `Replace` carry the step's whole new task list, from `TaskOp::added_tasks`:
`{"change": "all" | "replace", "taskCount": n, "tasks": [<task parameter set>, …]}`. For
`Frame` `CHUNK[INT]` `1-6` with `defaultTaskCount: 3` that is `"taskCount": 6` and
`{"F": {"type": "CHUNK[INT]", "value": "1-1"}}` to `"6-6"`. The `tasks` counts of
`openjd diff` and `TaskSummary` count what `StepParameterSpaceIterator` yields, which is
chunks (2 here); see [template-diff.md](template-diff.md#steps-and-tasks).

The document lists every task the change adds or removes, so for `All` and `Replace` it is
linear in the step's task count: the same size as the task rows the host writes for them.
The lists are serialized as they are iterated, not collected first, but a consumer that
parses the whole document (JSON text into Python, a `serde_json::Value`) holds it all at
once. That is the trade-off of a self-contained document: a host reads one value and never
calls back into openjd-rs. Measured for a single `INT` dimension, an All or Replace
document of 100,000 tasks is about 3.7 to 4.1 MB of JSON and takes about 0.1 to 0.25 s to
write; one of 1,000,000 tasks is about 38 to 42 MB and 1 to 3.3 s (the lower figures are
the task change document alone, in a release build). Any change to a step with a `CHUNK[INT]`
dimension takes this path, since its task delta is always a `Fallback` and so a `Replace`
of every task. An adaptor written in Rust that would rather stream a large step calls
`TaskOp::added_tasks` and `added_count` itself and hands the host the tasks in batches; the
order is the same.

Two orders a host must know. When only a step's tasks change, there is no `Step` op, so the
host writes the step's new parameter space (`TaskOp::space`, the document's `space`) from
the `tasks` call. A reorder
of the job environments both jobs share arrives as the `Job` modify op (kind `EnvReorder`),
before the environment ops; a reorder of a step's environments arrives in its `Step`
modify op.

`as_document` returns a `Serialize` view that borrows the plan; `to_value` is the same
document as a `serde_json::Value`. Every order in a document is a list order, so the two
agree, and a host may read either into maps that do not keep key order.

The reference adaptor is `DynStore` in `test_job_apply.rs`. It is used through
`&mut dyn JobApplier<Error = Box<dyn Error + Send + Sync>>`, appends one
`serde_json::Value` per call (`{"call": "step", "step": …, "op": …}`), and forwards the call
to an inner store. The tests replay the oracle cases through it, assert the whole log for
one case, resume after an injected write failure, and drive `ValueStore`, a store keyed by
task values that reads only the task change documents, through chained updates.

A PyO3 adaptor (in openjd-model-for-python, not here) has the same shape: a struct holding
a `Py<PyAny>`, the Python store object, whose trait methods take the GIL and call the
matching Python method. Documents cross the boundary as JSON text, read by Python's
`json.loads`; the `pythonize` crate, which serializes into Python objects directly, is the
alternative. The sketch below compiles against PyO3 0.25:

```rust
use openjd_model::job::apply::{
    apply, AllowList, ApplyError, ApplyReport, JobApplier, Op, PlannedOp, StepRef, TaskOp,
};
use openjd_model::job::{Environment, Job, JobParameter, Step};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// A `JobApplier` that forwards each call to a Python store object.
struct PyStore {
    store: Py<PyAny>,
}

/// A document as a Python value: JSON text, read by Python's `json`.
fn to_py<'py>(py: Python<'py>, doc: &impl serde::Serialize) -> PyResult<Bound<'py, PyAny>> {
    let text = serde_json::to_string(doc).map_err(|e| PyValueError::new_err(e.to_string()))?;
    PyModule::import(py, "json")?.call_method1("loads", (text,))
}

/// Object-safe `Serialize`, so `call` takes a list of mixed documents.
trait Doc {
    fn to_py<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>>;
}
impl<T: serde::Serialize> Doc for T {
    fn to_py<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, self)
    }
}

impl PyStore {
    /// `store.<method>(*args)`, each argument a document.
    fn call(&self, method: &str, args: &[&dyn Doc]) -> PyResult<()> {
        Python::with_gil(|py| {
            let args = args.iter().map(|a| a.to_py(py)).collect::<PyResult<Vec<_>>>()?;
            let args = pyo3::types::PyTuple::new(py, args)?;
            self.store.call_method1(py, method, args)?;
            Ok(())
        })
    }
}

impl JobApplier for PyStore {
    type Error = PyErr;

    fn check(&mut self, op: &PlannedOp<'_>) -> PyResult<()> {
        self.call("check", &[&op.as_document()])
    }
    fn job(&mut self, op: Op<'_, Job>) -> PyResult<()> {
        self.call("job", &[&op.as_document()])
    }
    fn parameter(&mut self, op: Op<'_, JobParameter>) -> PyResult<()> {
        self.call("parameter", &[&op.as_document()])
    }
    fn environment(&mut self, index: usize, op: Op<'_, Environment>) -> PyResult<()> {
        self.call("environment", &[&index, &op.as_document()])
    }
    fn step(&mut self, step: StepRef<'_>, op: Op<'_, Step>) -> PyResult<()> {
        self.call("step", &[&step, &op.as_document()])
    }
    fn tasks(&mut self, step: StepRef<'_>, op: &TaskOp<'_>) -> PyResult<()> {
        // The change lists its tasks, in create_job's order for All and
        // Replace, and the step's new space, so Python never enumerates it.
        self.call("tasks", &[&step, &op.as_document()])
    }
}

/// Apply through the adaptor as a `dyn JobApplier`.
fn apply_to(
    old: &Job,
    new: &Job,
    policy: &AllowList,
    store: Py<PyAny>,
) -> Result<ApplyReport, ApplyError<PyErr>> {
    let mut adaptor = PyStore { store };
    let dyn_store: &mut dyn JobApplier<Error = PyErr> = &mut adaptor;
    apply(old, new, policy, dyn_store)
}
```

A Python exception is the adaptor's `Error`. The binding maps `ApplyError` back to a Python
exception; the variants keep apart what a host treats differently (`Refused` and `Vetoed`
wrote nothing, `Store` wrote the ops before `op`). `ApplyError` is `#[non_exhaustive]`, so
the binding's `match` needs a final arm. `execute(plan, store, op)` resumes after a failed
write, as for any store.

## Tests

`crates/openjd-model/src/test_job_apply.rs`. `JobRebuilder` is an in-memory store that,
like a real adapter, maps new step indices to its own ids once. It gives each task a host
id and matches tasks by their values, never by position. It rejects:
- a step whose dependency is missing;
- the removal of a step that still has dependents;
- an environment insert past the end of the list;
- a modify at the wrong index;
- an added step placed past the end of the list. It places adds by index on `commit`;
- the removal of a task it does not hold, and the append of one it does.

The oracle: after `apply(a, b)` the store holds `b`. That covers parameters, steps by
name and in order, and job environments in order. It also covers each step's task-definition order
where that decides task order (no `combination`), and its task list. The task list is
compared in order, except after a `Boxes` op, where it is compared as a set.
It is run in both directions for the example templates, for parameter-only changes,
for job-environment inserts, moves and deletes, for a symbol-table-only step change,
for a definition reorder, and for step graphs where topological order differs from index
order (adds and removes). Other tests cover plan order and kinds, policy refusal with zero
writes, store veto, resuming after a failed write, repeated names, and `combination: "*"`.
Every kind is produced by some test, and for each one a policy that allows every other kind
in that plan refuses it with no writes.

`ValueStore` is a store keyed by task values that reads only the task change documents,
through `DynStore`. It and `JobRebuilder` apply chained updates to one stored job (append
then remove, remove then append, a product under `combination: "B * A"` changed three
times, and a chain of `Replace` updates: product to association, back, then a definition
reorder); after each update both hold exactly the new job's tasks, every kept task keeps its
id, and the tests assert how many were kept, so the `Replace` id-keeping branch of both
stores is exercised. The adaptor tests above cover the dyn path; others assert each refusal message in
full, every kind's name round trip, that `MutationKind::ALL` lists every variant (an
exhaustive match whose arms index a fixed list by constant, so a new variant fails to
compile, or fails the test, until `ALL` lists it), `AllowList::from_names`, that each op's
document equals the matching part of the `JobDocument`, one whole `check` document for a
job, an environment and a step op, a whole `Boxes` document with appends and removals, that
the `All` and `Replace` documents list `create_job`'s tasks (definitions `[Z, A]`, and
`combination: "B * A"` for a step add and a dimension reorder), the whole `Replace` and
`All` documents of a `CHUNK[INT]` step (`taskCount: 6`, `"1-1"` to `"6-6"`), that 20 plans of
independently rebuilt jobs are equal, and that resuming after a failed write with a plan of
rebuilt jobs leaves the store holding the new job.
`test_task_delta.rs` checks `appended` and `removed` against enumeration for every product
case, and `removed`'s old indices for three dimensions.
