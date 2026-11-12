# Job Diff (unstable)

**Unstable.** This API may change or be removed in any release without notice. Don't
depend on it outside openjd-rs. Applies to `job::diff` and
`job::task_delta`.

`job::diff` compares two instantiated jobs, steps, or environments and returns a
`DiffTree`: an arena of `DiffNode`s in breadth-first order, node 0 the root.

## Entry points

```rust
pub fn diff_jobs(old: &Job, new: &Job, options: &DiffOptions) -> DiffTree;
pub fn diff_steps(old: &Step, new: &Step, options: &DiffOptions) -> DiffTree;
pub fn diff_environments(old: &Environment, new: &Environment, options: &DiffOptions) -> DiffTree;
```

Two environment templates are diffed by converting each with `convert_environment` and
calling `diff_environments`.

## Algorithm

1. `resolvedSymTab` is compared by default. Actions keep their format strings
   unresolved (`command` stays `{{Param.Scene}}`), so a job parameter's effect on a step
   is visible only in the step's table. `create_job` filters that table to the symbols
   the step and its step environments reference (`filter_symtab_for_step`), and each job
   environment's to the symbols it references, so a parameter change marks exactly the
   steps and job environments that read it. Step environments carry no table of their
   own, so a change they see lands on the step. `with_resolved_symtab(false)` clears the
   tables first, which hides parameter changes inside steps. A float parameter whose
   literal changes form only (`1.0` to `1.00`) shows `parameters[F]` as `Unchanged`, since
   `ExprValue` equality ignores the spelling, while the steps that read it are `Modified`
   through `resolvedSymTab`, which keeps the literal the worker renders.
2. Push the root pair. For each pair, the node's `DiffStatus` is decided **when the node is
   created**, by one `PartialEq` of the whole subtree: `Unchanged`, `Modified`, `Added`
   (new side only), or `Removed` (old side only). For a job, step or parameter space with
   no `combination` (or `*`), the check also compares task parameter definition order, which
   `IndexMap` equality ignores but which then fixes task iteration order and so task
   indices. With an explicit `combination`, that string decides the order instead.
3. Pop pairs from a FIFO queue. Expanding a `Modified` node records its own differing
   scalar fields as `FieldChange`s and creates one child node per child pair.
4. A child is queued only if it is `Modified` and its depth is below `max_depth`. An
   `Unchanged`, `Added` or `Removed` node is never expanded. A `Modified` node at the
   depth limit has `expanded == false`.

Because status is decided before expansion, the root answers "any delta?" in O(1) after
the walk, every step node answers "delta in this step?", and `with_max_depth(1)` reports
every step's status without expanding any of them.

Cost: each node's equality check walks its subtree, so the walk is O(N·D) for N model
nodes and depth D (D ≤ 5). Unchanged subtrees cost one equality check each and are never
descended.

## Matching

| collection | matched by | order recorded as |
|---|---|---|
| `steps`, `jobEnvironments`, `stepEnvironments`, `embeddedFiles` | `name` | `<collection>.order` field change |
| `taskParameterDefinitions` | map key | `taskParameterDefinitions.order` when there is no `combination`, or it is `*` (then it fixes task order) |
| `parameters` | map key | not a change |
| `variables` | map key | not a change; entries are `variables.<KEY>` |
| `hostRequirements.amounts` / `attributes` | `name` | `<collection>.order`; entries are `<collection>[<name>]` |
| `dependencies` | set of `dependsOn` | `dependencies.order` |

Children are ordered old-side first, then new-only entries. With duplicate names (not
possible in a validated job, but the fields are public) the first occurrence is matched
and the later copies are compared as a list. When they differ, a
`<collection>.duplicates` field change lists the repeated names, so the parent is never
an unattributed `Modified`.

A `None` versus `Some(empty)` difference in an optional list has no entry to attribute
it to, so it is recorded as a field change named after the collection
(`jobEnvironments`, `stepEnvironments`, `embeddedFiles`, `variables`, `amounts`,
`attributes`). Same-edge `dependencies` that differ only in order, repetition or
None-vs-empty are recorded as `dependencies.order`.

## Tree shape

| node kind | segment from parent | own fields | children |
|---|---|---|---|
| Job | (root) | `name`, `description`, `extensions`, `steps.order`, `steps.duplicates`, `jobEnvironments`, `jobEnvironments.order` | `parameters[K]`, `jobEnvironments[N]`, `steps[N]` |
| JobParameter | `parameters[K]` | `name`, `type`, `value` | — |
| Step | `steps[N]` | `name`, `description`, `dependencies`, `dependencies.order`, `stepEnvironments`, `stepEnvironments.order`, `resolvedSymTab` | `script`, `stepEnvironments[N]`, `parameterSpace`, `hostRequirements` |
| StepScript | `script` | `let`, `embeddedFiles`, `embeddedFiles.order` | `onRun`, `embeddedFiles[N]` |
| Environment | `jobEnvironments[N]` / `stepEnvironments[N]` | `name`, `description`, `variables.K`, `variables`, `resolvedSymTab` | `script` |
| EnvironmentScript | `script` | `let`, `embeddedFiles`, `embeddedFiles.order` | `onEnter`, `onWrapEnvEnter`, `onWrapTaskRun`, `onWrapEnvExit`, `onExit`, `embeddedFiles[N]` |
| Action | `onRun`, `onEnter`, ... | `command`, `args`, `timeout`, `cancelation` | — |
| EmbeddedFile | `embeddedFiles[N]` | `name`, `type`, `filename`, `data`, `runnable`, `endOfLine` | — |
| ParameterSpace | `parameterSpace` | `combination`, `taskParameterDefinitions.order` | `taskParameterDefinitions[K]` |
| TaskParameter | `taskParameterDefinitions[K]` | `definition` (whole value) | — |
| HostRequirements | `hostRequirements` | `amounts[N]`, `amounts.order`, `amounts`, `attributes[N]`, `attributes.order`, `attributes` | — |

## Identity and position

A node for an element of a named collection carries `key` (the name or map key),
`old_index` and `new_index` (its position in each side's collection; `None` on the side
where it is absent). For a step, the index is into `Job::steps`, which is the order
`StepDependencyGraph` uses for `step_index`. The name is the identity: inserting or
removing a step shifts the index of every later step, so a store maps `key` to its own
id and uses the indices only for ordering or display.

`step_index` is one int threaded through the whole step subtree: set on each step node
and inherited by every node below it. It indexes the new job's `steps` when the step
exists there, otherwise (a `Removed` step) the old job's. It is `None` above step level
and everywhere in a `diff_steps` tree.

## Task delta (`job::task_delta`)

`task_delta(old: Option<&StepParameterSpace>, new: Option<&StepParameterSpace>) ->
TaskDelta` gives the change in a step's task set without enumerating tasks, when both
sides are a plain Cartesian product over the same parameters (combination absent, `*`,
or every name exactly once joined by `*`; no chunking; no range that repeats a value,
since the algebra is over sets; a task count that fits `usize`; no parameter added,
removed or retyped). With
per-parameter value sets `O_k`, `N_k` and `I_k = O_k ∩ N_k`, in the new side's dimension
order:

```
kept      = I_0 × … × I_{D-1}
added_k   = I_0 × … × I_{k-1} × (N_k \ O_k) × N_{k+1} × … × N_{D-1}
removed_k = I_0 × … × I_{k-1} × (O_k \ N_k) × O_{k+1} × … × O_{D-1}
```

The `added_k` are disjoint and their union is `new \ kept`; likewise for `removed_k`. So
the delta is `TaskDelta::Product(ProductDelta { old, kept, added, removed })`: at most
`2D + 1` `TaskBox`es, each of which is a `StepParameterSpace` of list ranges that
`StepParameterSpaceIterator` iterates and `TaskBox::len` counts. Cost is linear in the
per-parameter range sizes, not their product. Each range value is keyed as a string and
hashed, so a 10^6-frame range takes seconds in a release build. Interval arithmetic on
`RangeExpr` would remove that and is left for later. `old` is the whole old space, and
`old.index_of(task)` is the row-major (rightmost fastest) position of a task at job
creation. Equal spaces are `Unchanged`. A different dimension order (a definition reorder with no `combination` or with `*`, or a `combination` that orders the dimensions differently), or a different relative order of the values both sides share in any range, is `Fallback`, because kept tasks move; anything else is `Fallback { reason }`, where
the caller enumerates and diffs by value. Tests: `src/test_task_delta.rs`, which checks
each decomposition against brute-force enumeration.

Each `expand_*` destructures its model type exhaustively, so adding a model field fails
to compile until the diff handles it.

Tasks are not enumerated. A step's tasks change exactly when its `parameterSpace` node
does.

`DiffTree::find` takes a dotted path of segments (`steps[Render].script.onRun`), so it
cannot address a name that contains `.`; walk with `DiffTree::child` instead.

Tests: `crates/openjd-model/src/test_job_diff.rs`.

Example CLI: `cargo run -p openjd-model --example job_diff -- OLD NEW [--max-depth N]
[--no-symtab] [--param K=V] [--old-param K=V] [--new-param K=V] [--plan]`, with sample
templates in `crates/openjd-model/examples/job_diff/`. `--plan` previews a step and task
apply order; it is example code, not part of the API. `openjd diff` ([cli/diff.md](../cli/diff.md))
is the supported command, built on [template-diff.md](template-diff.md).
