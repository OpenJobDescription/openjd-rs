# Template Diff (unstable)

**Unstable.** This API may change or be removed in any release without notice. Don't
depend on it outside openjd-rs. Applies to `job::template_diff`; each
of its entry points says so in its doc comment.

In code, the banner is the second paragraph of the module docs of `job::template_diff`,
`job::diff`, `job::task_delta` and `job::apply`, after the one-line summary, because
rustdoc shows the first paragraph in module lists. Each public function of
`job::template_diff` ends its docs with `Unstable: see the module docs.` A test checks both.

`job::template_diff` diffs two job templates, or one template under two parameter sets.
It builds both jobs, runs [`diff_jobs`](job-diff.md) on them, adds a *template layer* for
what a built job cannot show, and tags every change with its origin and stage. The result
is one `TemplateDiff`, which can be printed, serialized, compared, or turned into an
[apply plan](job-apply.md). Design: SuperDaveDocs `docs/dag-diff/03-template-diff.md`.
The command-line form is [`openjd diff`](../cli/diff.md).

## Entry points

```rust
pub fn diff_templates(old: &TemplateInput, new: &TemplateInput, options: &DiffOptions) -> Result<TemplateDiff, ModelError>;
pub fn diff_job_template_with_parameter_sets(template: &TemplateInput, old: &JobParameterInputValues, new: &JobParameterInputValues, options: &DiffOptions) -> Result<TemplateDiff, ModelError>;
pub fn diff_built_jobs(old: Job, new: Job, options: &DiffOptions) -> TemplateDiff;
pub fn job_from_str(text: &str) -> Result<Job, ModelError>;
```

- `diff_job_template_with_parameter_sets` lays `old` and `new` over the template input's
  own parameters (a later value wins), decodes the template once and builds it twice. It has no template
  layer, and every change has origin `Parameters`.
- `diff_built_jobs` is for callers that already hold two jobs. It has no template layer,
  and every change has origin `Job`, because the cause is not known.
- `job_from_str` reads a job back from its serde form (JSON when the first
  non-whitespace character is `{`, YAML otherwise), as `openjd build` prints it. It takes text rather than a
  `serde_json::Value` because a `Value` object sorts its keys, which would reorder task
  parameter definitions and so change task order. Unknown fields are rejected at every
  level: after parsing, every key of the input must appear in the parsed job's
  `JobDocument` form, so a key a job type ignored is an error naming its path,
  `steps[0].bogus: unknown field`. Only keys are compared, so `2` for `2.0` passes, and so
  do the `letBindings` and `type` aliases. A `null` passes for a field the document omits
  when unset, but only in the objects that have that field: `resolvedSymTab` in a step or
  an environment, the `onWrap*` actions in an environment's `actions`, and
  `notifyPeriodInSeconds` in a `cancelation`. An extra key beside the tag of a task
  parameter definition or of its `range` is reported the same way
  (`steps[0].parameterSpace.taskParameterDefinitions.F.bogus: unknown field`). Each
  `resolvedSymTab` entry must have exactly the keys `name`, `type` and `value`, a string
  `name` and `type`, a `type` that parses, and a `value` of that type, as `SymbolTable`
  serializes it (`steps[0].resolvedSymTab[0].zzBogus: unknown field`), and no two entries
  of one table may have the same `name` (`steps[0].resolvedSymTab[1].name: 'Param.Z' is
  repeated`). The name's syntax is not checked. `parameters` may be absent (an empty
  map), but a `null` is rejected like any other unexpected `null`, with serde's message
  (`invalid type: null, expected a map at line 1 column N`).
  A parameter must have exactly the keys `name`, `paramType` and `value`, and its `name`
  must be its key. A parameter whose value does not have its `paramType`'s type is
  rejected, with `parameters[P]: paramType 'ListInt' cannot hold a 'ListList' value`, and
  so is a `ListList` element that is not a `ListInt`. A `Path` parameter
  holds a `String` value and a `ListPath` one a `ListString`, as `create_job` stores them;
  a `Path` or `ListPath` value is accepted too.

## Inputs

`TemplateSource` is owned: `Path(PathBuf)`, `Text(String)`, `Value(serde_json::Value)`,
or `Decoded(Box<JobTemplate>)`. A path is JSON when its extension is `.json` and YAML
otherwise, as in the CLI. Text is JSON when its first non-whitespace character
is `{`; the leading whitespace is skipped only for this check. Both go through
`document_string_to_object`. `TemplateSource::guess(s)` is `Path` when `s` names an
existing file and `Text` otherwise; the library never guesses on its own.

`TemplateInput` is one side: the source, its parameter values, environment templates, the
extension allowlist, `CallerLimits`, and the two directories for PATH parameters. Its
defaults match `openjd check`: every extension in `ModelExtension::ALL`, default limits,
the template file's directory for relative PATH defaults, and the process working
directory for relative PATH values. For a source other than `Path`, relative PATH
defaults use the PATH-value directory: the `cwd` override when one is set, else the
process working directory.

Each side is built exactly as `openjd summary` and `openjd run` build a job:

1. `decode_job_template` with the allowlist and limits (skipped for `Decoded`, so the
   allowlist is not applied to it), and `decode_environment_template` for each
   environment template;
2. `preprocess_job_parameters` with host path format, no template-directory walk-up, and
   URI path values allowed;
3. `create_job` with `default_validation_context().with_caller_limits(limits)`.

`TemplateInput::build` runs these for one side. A failure names the input that failed. In
`diff_templates` it is `old template: …` or `new template: …`. In
`diff_job_template_with_parameter_sets` the template is shared, so a build failure is `old parameters: …` or `new parameters: …`,
and `the template: …` when the template does not decode. The error keeps its variant when
it carries a message (`DecodeValidation`, `Compatibility`, `UnsupportedSchema`); any other
error becomes `DecodeValidation` with its display text.

## The job layer

`diff_jobs(old_job, new_job, options)` runs unchanged and is kept as `TemplateDiff::job`.
Every changed node becomes `Change`s:

| node | changes |
|---|---|
| `Added` / `Removed` | one change with an empty `field`; the side that exists is the node's serialized value, the other is `None` |
| `Modified`, not expanded (depth limit) | one change with an empty `field` and both serialized values |
| `Modified`, expanded | one change per `FieldChange`; a `null` side is `None` (field added or removed) |

A whole node's value is found by following its `DiffTree` segments through the serialized
job. Every segment is the serialized field name except an action's (`onRun`, `onEnter`,
…), which the tree hangs on the script but the job holds under `script.actions`. The
serialized job is the `JobDocument` form as a `serde_json::Value`, except that each step's
`taskParameterDefinitions` is a list of `{name, definition}` in definition order: a `Value`
object sorts its keys, and definition order is task order, so a map would hide a reorder.

Before diffing, both jobs' `parameters` are sorted by name. `create_job` fills them from a
`HashMap`, so their order varies between runs, and parameter order is not a change.
Without the sort, two diffs of the same inputs would build trees with nodes in different
orders.

## The template layer

What the built job cannot show, compared on the decoded templates:

| path | field | compared |
|---|---|---|
| (root) | `specificationVersion` | the string |
| (root) | `extensions` | the list as written |
| `parameterDefinitions[P]` | `type`, `description`, `default`, `allowedValues`, `minValue`, `maxValue`, `minLength`, `maxLength`, `objectType`, `dataFlow`, `item.*`, `item.item.*`, `userInterface.*` | field by field; numbers by value, so `default: 5` and `default: "5"` are equal |
| `parameterDefinitions[P]` | (empty) | a definition added or removed, shown as its fields |
| `steps[S]` | `let` | the step-level `let` text, for steps on both sides |

Definitions are matched by name. `extensions` is also a field of the built job, so both
layers report it; `Display` prints only the template layer's line, whatever the job
layer's origin.

An extension added, removed or reordered is therefore printed once (`+ extensions: [EXPR]
[template]`) and kept twice in `changes`, once per layer. `job::apply::plan` turns the job
layer's change into the root `Job` modify op with kind `JobExtensions`, and a store applies
it through `JobApplier::job`. The gallery's `extension_added` and `extension_removed`
cases show both ([cli/diff-examples.md](../cli/diff-examples.md)).

**Deferred**, and recorded here so they are not mistaken for gaps:

- sugar form (`bash:` versus `script`);
- the source format strings of resolved fields (`name`, host requirements, ranges);
- a render check that says whether a changed format string renders differently;
- `diff-step` (a step template in the context of its job) and `diff-space` (two
  parameter spaces alone).

## Origin

Each change says where it came from:

| origin | meaning |
|---|---|
| `Template` | the template changed; the parameter values alone would not change it |
| `Parameters` | the parameter values changed; the template alone would not change it |
| `Both` | both, or the split could not be computed |
| `Job` | the inputs were built jobs (`diff_built_jobs`) |

Template-layer changes are always `Template`. For job-layer changes in `diff_templates`:

1. Parameter inputs equal (`==` on the two `JobParameterInputValues`): every change is
   `Template`.
2. Otherwise, templates equal: every change is `Parameters`. "Equal" compares the parsed
   documents of the template and its environment templates, the allowlist, and the two
   PATH directories. A `Decoded` source has no document, so it never takes this shortcut;
   step 3 gives the same answer at the cost of one more build.
3. Otherwise build `mid`, the new template with the old parameters and the new side's
   limits. A change whose key
   appears in `diff_jobs(old, mid)` only is `Template`, in `diff_jobs(mid, new)` only is
   `Parameters`, and in both or neither is `Both`. The key of a field change is
   `(path, field)`; the key of a whole-node change is `(path, status)`, the status being
   added, removed, or modified and not expanded. So a node one half adds is not confused
   with the same node the other half leaves unexpanded at the depth limit. A change keeps
   its origin at every depth at which it appears. A whole (unexpanded) node is classified
   by whether its value differs in each half (old to `mid`, `mid` to new), so it can be
   `Both` even when the full-depth changes below it have one origin: for example, when the
   template changes a value and a parameter changes it back. The two extra diffs use the
   caller's options, so the keys line up with the main diff.
4. If `mid` does not build (the new template needs a parameter the old set lacks, for
   example), every job-layer change is `Both` and `TemplateDiff::notes` says why.

Comparing parameter values alone would get a template default change wrong: if `Frames`
is not supplied and its `default` changes, the value changes but the cause is the template
(rule 1 gives `Template`).

The split compares keys, not values. A whole node the template adds (an environment, a
step, a task parameter) is therefore `Template` even when its contents also depend on the
parameter values.

## Stage

The earliest generic OpenJD stage a change shows at. openjd-rs has no knowledge of any
store; a service maps the stage and path to its own records.

| stage | rule |
|---|---|
| `Template` | the change is in the template layer |
| `Parameters` | the path starts with `parameters[` |
| `RunTime` | the field is a format string (`command`, `args`, `timeout`, `cancelation`, `data`, `let`, `variables.*`) and its before or after value names a `Task.` or `Session.` symbol |
| `Job` | everything else |

A format string names a run-time symbol when the text inside one of its `{{ }}`
interpolations contains `Task.` or `Session.` at a name boundary: at the start, or after a
character that is not a letter, digit, `_` or `.`. So `{{Task.Param.Frame}}` and
`{{ x + Session.WorkingDirectory }}` count, and `Task.log` outside braces, `subTask.x` and
`a.Task.x` do not. A `let` binding is an expression with no braces, so the same boundary
rule applies to the whole binding.

An embedded file's `filename` is a plain string, not a format string: the worker uses it
as written, so a change to it is stage `Job` even when it contains `{{Task.Param.F}}`.

A PATH parameter change shows in `resolvedSymTab` as `RawParam.P` and is stage `Job`; PATH
mapping itself happens on the worker.

## Steps and tasks

`TemplateDiff::steps` has one `StepSummary` per step of either job, in old order and then
new-only steps. `status` is the step's status under the caller's options (one
`diff_steps` at depth 0), so it does not depend on `max_depth`. `tasks` comes from
`task_delta` on the two parameter spaces:

| `TaskDelta` | `TaskSummary` |
|---|---|
| `Unchanged` | `Unchanged { count }` |
| `Product(d)` | `Product { kept, added, removed }`, each a `BoxSummary` |
| `Fallback { reason }` | `Replaced { old_count, new_count, reason }` |
| step added or removed | `Added { count }` / `Removed { count }` |

Counts are `u64` and count what `StepParameterSpaceIterator` yields (chunks, for a chunked
step). Sums saturate at `u64::MAX` rather than overflow. When either side's parameter space
does not iterate (reachable only through an edited built job, for example a `combination`
naming an unknown parameter), the step's `tasks_unknown` is `Some("cannot count tasks")`,
its counts are 0 (`Replaced` with reason `cannot count tasks` when the space changed),
`notes` has `steps[S]: cannot count tasks: <the iterator's error>`, and `Display` prints
`tasks: unknown (cannot count tasks)`; adaptive chunking, below, takes precedence. A `BoxSummary` has each dimension's name and its values: integers as the
same text a change shows (see [Rendering](#rendering)), other values comma-separated.

A step with adaptive chunking (`CHUNK[INT]` with `targetRuntimeSeconds` above 0) on either
side decides its chunk count at run time. Its `tasks_unknown` is `Some("adaptive
chunking")`, its counts are 0 (`Replaced` with reason `adaptive chunking` when the space
changed), `notes` has `steps[S]: cannot count tasks: the chunk size is adaptive
(targetRuntimeSeconds), so the number of chunks is decided at run time`, and `Display`
prints `tasks: unknown (adaptive chunking)`.

`DiffStat` (`TemplateDiff::stat`) counts steps changed, added and removed, and tasks kept,
added, removed and replaced over all steps. A step on both sides is changed when its status
is `Modified` or a template-layer change is in it (a step-level `let` that nothing
reads), so the totals line agrees with the steps `Display` lists. Kept tasks include
unchanged steps' tasks. `tasks_replaced` counts the new tasks of `Replaced` steps; their
old tasks are not counted as removed.

## Result types

```rust
pub struct TemplateDiff { pub changes: Vec<Change>, pub steps: Vec<StepSummary>, pub job: DiffTree,
                          pub old_job: Job, pub new_job: Job, pub notes: Vec<Note> }
pub struct Change { pub layer: Layer, pub path: String, pub step: Option<String>, pub field: String,
                    pub before: Option<Shown>, pub after: Option<Shown>, pub origin: Origin, pub stage: Stage }
pub struct Shown { pub raw: serde_json::Value, pub text: String }
pub struct Note { pub step: Option<String>, pub text: String }
pub struct JobDocument<'a>(pub &'a Job);
```

`changes` is sorted by path, field and layer. `path` uses `DiffTree` segments
(`steps[Render].parameterSpace.taskParameterDefinitions[Eye]`); `field` is empty for a
whole node.

`step` is the name of the step a change is in, and `None` for a job-level change. A
job-layer change takes it from the key of the `DiffTree` step node at or above its node; a
template-layer step `let` change from the template step it was computed for. Every
grouping by step uses it, never the path: `Display`, `stat`, `changes_in_step`, and
`openjd diff --step`. A path cannot be split by step reliably, because a step name may
contain `]` and `.`: the step `A].x` has paths starting `steps[A].x]`, which also start
with `steps[A]` followed by a `.`.

Helpers: `is_changed`; `changes_in_step(name)`, the changes whose `step` is `name`,
exactly; `changes_at(prefix)`, the changes whose path is the prefix or below it at a
segment boundary (not `steps[Render2]` for `steps[Render]`; `steps` and `parameters` match
every entry), which a name containing `]` or `.` can make ambiguous (`steps[A]` matches
the step `A].x`), so it is not for selecting a step; `step(name)`; `plan()`
(`job::apply::plan` on the kept jobs); and `stat()`.

A `Note` is something the diff could not decide. `step` names the step it is about (a task
count that is unknown), and is `None` for a job-level note (the origin split). It displays
and serializes as one string, `steps[S]: text` or `text`.

Every type derives `Debug`, `Clone`, `PartialEq` (and `Eq` where its fields allow; not
`TemplateDiff`, whose jobs hold floats) and `Serialize` with camelCase field names. Enums
serialize as lowercase camelCase strings (`template`, `parameters`, `both`, `job`,
`runTime`); `TaskSummary` is tagged with `kind`; `StepSummary::status` is `unchanged`,
`added`, `removed` or `modified`, and `tasksUnknown` is written only when set;
`Change::step` is written only when set, so a job-level change has no `step` key;
`TemplateDiff::job` serializes as its changed nodes, `{path, status, expanded,
fieldChanges}`.

`JobDocument` serializes a `Job` as its serde form with `parameters` and each
environment's `variables` sorted by name. `create_job` fills `Job::parameters` from a
`HashMap` and `Environment::variables` is a `HashMap`, so the plain serde form of a job
with more than one of either differs from run to run. Task parameter definitions keep their
order, which is task order. Every other map in a `Job` is already ordered: resolved symbol
tables are lists sorted by name. `TemplateDiff` writes `oldJob` and `newJob` as
`JobDocument`s, so its serde form is deterministic. `openjd build` prints the same form,
and `job_from_str` reads it back.

### Rendering

`Shown::raw` is the value as the job serializes it (for a job parameter,
`{"String":"1-200"}`), in full; it is meant for programs. A whole node's `raw` writes task
parameter definitions as a list of `{name, definition}` in definition order (see [The job
layer](#the-job-layer)). `Shown::text` is for people:

- an integer task parameter range is a range expression's canonical form (`1-100`,
  `1-9:2`, `1,2`, `1,3`: a run needs three values), whether the job holds a range
  expression or a list; a list whose values do not increase is written as runs in its own
  order (`5,1-3`). The same text is used in task boxes;
- other task parameter ranges are lists (`[left, right]`);
- a chunked task parameter adds `chunks of N`, then `, target Ts` when
  `targetRuntimeSeconds` is set and `, noncontiguous` for a noncontiguous range
  constraint (`1-40 chunks of 10, target 5s, noncontiguous`);
- a job parameter value is bare (`1-200`, not `{"String":"1-200"}`), and its type is the
  spec name (`INT`). When the parameter's type differs between the two sides, each value
  is followed by its type, and only `STRING` and `PATH` values are quoted:
  `~ parameter Count: "5" (STRING) -> 5 (INT)`, `~ parameter X: 4 (INT) -> 4.5 (FLOAT)`.
  A whole parameter left unexpanded by the depth limit is shown the same way. A long
  value is cut as below, and its type is always shown: `…aaaa1" (STRING) -> …aaaa2"
  (PATH)`. A whole job writes each job parameter in it as its bare value and its type,
  whether or not the type changes: `parameters: {Frames: 1-100 (STRING), N: 4 (INT)}`;
- a symbol table is `name=value` pairs, and an empty one is `(none)`;
- a whole parameter space is each task parameter's range in definition order, then the
  `combination` when one is set: `{Frame: 1-10, Eye: [left, right]}, combination (Frame, Eye)`.
  A whole step or job shows each parameter space in it the same way
  (`parameterSpace: {Frame: 1-10}`);
- step dependencies are the names of the steps depended on: `[Prep, Render]`;
- an object leaves out its `null` fields. A value whose line already names it (a whole
  step, environment or embedded file, a host requirement entry such as
  `amounts[amount.worker.vcpu]`) leaves out its `name`, and a whole node leaves out an
  empty `resolvedSymTab` at every depth. This depends only on the value and the node, so a value looks
  the same wherever it is: `+ jobEnvironments[Tools].script.embeddedFiles[F]: {data: hi,
  fileType: TEXT}` and `+ steps[S].script.embeddedFiles[F]: {data: hi, fileType: TEXT}`;
- a float that is a whole number of at most 2^53 in magnitude (every such number is
  exactly an `f64`) is written without its fraction (`min: 4`, not `4.0`); a larger one
  keeps it (`9007199254740994.0`). `raw` keeps the number as the job holds it;
- other strings are bare, lists are `[a, b]`, objects are `{key: value}`;
- a string's control characters are escaped, so a value is always one line and a CR
  shows: `\n`, `\r`, `\t`, and any other as `\u{1b}` (`[a\nb] -> [a\r\nb]`). A string
  that would be ambiguous bare is quoted, with `"` escaped: one that is empty, starts or
  ends with whitespace, or contains ` -> ` (`a -> "a -> b"`, `"" -> " "`). A multi-line
  embedded file ends with a newline, so it is quoted:
  `"set -e\nrender --fast\n" -> "set -e\nrender --quality high\n"`;
- when both sides' full texts are equal but their values differ, every task parameter
  definition in them, whether the change is the definition, a parameter space, a step or
  the job, shows how its range is written: an integer range as `range expression 1,2` or
  `list [1, 2]`, any other type as its type and list (`FLOAT [1.5, 2.5] -> STRING
  [1.5, 2.5]`), then its chunk settings. A chunked range names its type, so a hand-made
  `--jobs` pair with an `INT` with `chunks` against the `CHUNK[INT]` it mirrors shows
  `CHUNK[INT] range expression 1-20 chunks of 5 -> INT range expression 1-20 chunks of 5`.
  The built job keeps only a range expression's canonical form, so `"1-2"` shows as
  `range expression 1,2`, and at `--depth 2` a step's parameter space shows as
  `{Frame: range expression 1,2} -> {Frame: list [1, 2]}`.
- text longer than 120 characters is cut to 119 and ends with `…`. The cut happens once,
  after every rule above, and a type suffix such as ` (STRING)` is never cut. **Two sides
  with different values never show the same text**: when the first difference between
  the two full texts would be cut off, both start with `…` up to 40 characters before it,
  at the first `/` or after the first space in those 40 when there is one, so the cut
  does not split a word. A list or object keeps up to its first 20 characters before
  that, ending after the last `,` or `:` in them when there is one, so it still reads
  as one: `[s3://bucket/projects… …/shot_010/renders/a]`, `{name: J,… …/passes/v1/out}}`. When the full texts are still equal
  (`"3"` and `3` as generic values), both sides show as their JSON, cut the same way.

`Display` is deterministic, so tests assert whole outputs:

```text
~ parameter Frames: 1-100 -> 1-200  [parameters]
~ step Render
    ~ Frame: 1-100 -> 1-200  [parameters]
    ~ Eye: [left] -> [left, right]  [template]
    tasks: 100 -> 400
      keep   100   Frame 1-100 × Eye left
      add    200   Frame 101-200 × Eye left, right
      add    100   Frame 1-100 × Eye right

1 step changed. Tasks: 100 kept, 300 added, 0 removed.
```

- Job-level changes (`step` is `None`) come first, then each step that changed or has a
  change in it (`step` is its name),
  with its changes relative to the step and its task summary, then the `DiffStat` line,
  then one `note: …` line per entry of `notes` (why origins are `both`, why a task count
  is unknown). Nothing changed prints `no changes`, followed by the notes.
- Under a step, task parameter changes follow the step's definition order (new side, then
  definitions only on the old side), not path order.
- `~` is changed, `+` added, `-` removed. Every line ends with the origin tag. The tags are
  not aligned into a column, because the value text varies too much in width.
- Labels: `parameter P` (`parameters[P]`), `parameter definition P.field`, a task
  parameter definition by its name, other paths relative to the step.
- A parameter whose type changes prints its value line with both types and leaves out its
  job-layer `parameter P.type` line, which says the same; `changes` keeps that change.
- A parameter added or removed together with its definition prints once:
  `- parameter Gone: g, defined as {default: g, type: STRING}  [template]`, with the
  parameter change's origin. A definition added or removed without the parameter (an
  environment template still defines it) prints on its own line.
- A step left unexpanded by the depth limit prints `~ (not expanded)  [origin]` under its
  header. An added or removed step's own change is not repeated under its `+`/`-` header.
- `Change` alone prints `~ path.field: before -> after  [origin]`.
- A task summary prints `tasks: 100 -> 400`, then one `keep`, `add` or `remove` line per
  box. A product that adds and removes nothing prints `tasks: N unchanged`, as unchanged
  tasks do; other summaries print `tasks: N added`, `tasks: N removed` or
  `tasks: 1 -> 2, all replaced (reason)`.
- `StepSummary` prints the step header and its task summary; `stat_line` prints
  `~ Render  tasks 100 -> 400  (+300 -0)`, or `~ Render  tasks unknown (adaptive
  chunking)`.
- `DiffStat` prints `N step(s) changed[, A added][, R removed]. Tasks: K kept, A added,
  R removed[, P replaced].`

## Examples

The samples are in `crates/openjd-model/examples/template_diff/` and are the cases of the
design's §7.4. [cli/diff-examples.md](../cli/diff-examples.md) has one generated example per
field the diff reports, with its output:

| case | inputs | result |
|---|---|---|
| 1 | `render_v1.yaml`, `Frames` 1-100 → 1-200 | `parameters[Frames]` and `Frame` [parameters]; keep 100, add 100 |
| 2 | `render_v1.yaml` → `render_v2.yaml` (`Eye` gains `right`) | `Eye` [template]; keep 100, add 100 |
| 3 | both | mixed origins; keep 100, add 300 |
| 4 | `render_no_eye.yaml` → `render_v1.yaml` | `+ Eye`, `args` (stage run time); every task replaced |
| 5 | `Scene` //srv/a → //srv/b | `parameters[Scene]` and `resolvedSymTab` `RawParam.Scene` (stage job) |
| A4 | `render_v1.yaml` → `render_default_1_200.yaml`, `Frames` not supplied | `parameterDefinitions[Frames].default` and the job changes, all [template] |

Tests: `crates/openjd-model/tests/integration/test_template_diff.rs`.
