# Job Apply (unstable)

**Unstable.** This API may change or be removed in any release without notice. Don't
depend on it outside openjd-rs. Applies to `job::apply`.

`job::apply` turns a [job diff](job-diff.md) into an ordered, gated list of writes against
a store that it knows only through a trait.

## Stages

```rust
pub fn plan<'a>(old: &'a Job, new: &'a Job) -> Result<ApplyPlan<'a>, ModelError>;
pub fn validate<A: JobApplier>(plan: &ApplyPlan<'_>, policy: &dyn MutationPolicy, store: &mut A) -> Result<(), ApplyError<A::Error>>;
pub fn execute<A: JobApplier>(plan: &ApplyPlan<'_>, store: &mut A, resume_from: usize) -> Result<ApplyReport, ApplyError<A::Error>>;
pub fn apply<A: JobApplier>(old: &Job, new: &Job, policy: &dyn MutationPolicy, store: &mut A) -> Result<ApplyReport, ApplyError<A::Error>>;
```

`apply` is `plan`, then `validate`, then `execute(…, 0)`.

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
2. `Action::Parameter`: one op per changed parameter.
3. `Action::Environment(index, op)`: one op per changed **job** environment.
   - Removes come first. They are matched by name; the index is the old one and is
     informational.
   - Adds and modifies follow in ascending new index. Inserting each `Add` at its index
     after the removes, and after any `Job` op's `EnvReorder` of the shared environments,
     rebuilds the new list order.

   Step environments travel inside their step's op, because a step is one item in a
   store.
4. Steps in **topological order of the new step graph**, origins first:
   - `Added`: `Step(j, Add)`, then `Tasks { change: All }`.
   - `Modified`: `Step(j, Modify)` unless the only change is the parameter space, then
     `Tasks` when the task set or task order changed (`task_delta`: `Product` with
     added or removed boxes → `Boxes`, `Fallback` → `Replace`). A change of dimension
     order is a `Fallback`, because every kept task moves. If the space changed but the
     tasks and their order didn't (e.g. `1-2` rewritten as `[1, 2]`), there is no
     `Tasks` op. The `Step(j, Modify)` is emitted with `StepParameterSpace`, so the store
     still writes the new definition.
5. Removed steps in **reverse topological order of the old step graph**, dependents first.

The index in `Step(j, Add)` is the step's final position, `new.steps[j]`. Adds arrive in
topological order, not index order, so `j` can be past the end of the store's list at the
time (`[A]` to `[B (dependsOn C), A, C]` adds C at 2, then B at 0). A store that keeps step
rows in order places the added steps by `j` once the plan is done, in ascending `j`, after
the removes and after any `Job` op's `StepReorder` of the shared steps. `Tasks { step: j }`
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

"Render more frames" (a `Frames` parameter change) plans as `ParameterValue` +
`TaskAppend`, because the step's symbol table doesn't hold the symbols its parameter
space uses.

## Validation and execution

- `validate` asks the policy about every op first, then `JobApplier::check` about every
  op. The first refusal returns `ApplyError::Refused` or `ApplyError::Store` with the
  op's index, and nothing has been written.
- `execute` does not validate. `ApplyPlan::ops` is public, so a caller that edits a plan
  after `validate` must validate it again. `apply` runs both.
- `execute` calls `begin`, the trait method for each op from `resume_from` on, then
  `commit`. A failed write returns `ApplyError::Store { op }`. Ops before `op` were
  applied, so `execute(plan, store, op)` resumes, provided the store's writes are
  idempotent.

## Tests

`crates/openjd-model/src/test_job_apply.rs`. `JobRebuilder` is an in-memory store that,
like a real adapter, maps new step indices to its own ids once. It rejects:
- a step whose dependency is missing;
- the removal of a step that still has dependents;
- an environment insert past the end of the list;
- a modify at the wrong index;
- an added step placed past the end of the list. It places adds by index on `commit`.

The oracle: after `apply(a, b)` the store holds `b`. That covers parameters, steps by
name and in order, and job environments in order. It also covers each step's task-definition order
where that decides task order (no `combination`), and its task list. The task list is
compared in order, except after a `Boxes` op, where appends are expected to land after
the old tasks.
It is run in both directions for the example templates, for parameter-only changes,
for job-environment inserts, moves and deletes, for a symbol-table-only step change,
for a definition reorder, and for step graphs where topological order differs from index
order (adds and removes). Other tests cover plan order and kinds, policy refusal with zero
writes, store veto, resuming after a failed write, repeated names, and `combination: "*"`.
Every kind is produced by some test, and for each one a policy that allows every other kind
in that plan refuses it with no writes.
