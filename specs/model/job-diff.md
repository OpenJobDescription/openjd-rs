# Job Diff

**Status: unstable.** `job::diff` may change or be removed in any release
without notice.

`job::diff` compares two instantiated jobs, steps or environments and says
what differs, where. A caller that holds a job created earlier and a job
created from an edited template or new parameter values uses it to find the
steps and environments that changed.

```rust
pub fn diff_jobs(old: &Job, new: &Job) -> DiffTree;
pub fn diff_steps(old: &Step, new: &Step) -> DiffTree;
pub fn diff_environments(old: &Environment, new: &Environment) -> DiffTree;
```

`diff_steps` and `diff_environments` return the same tree that `diff_jobs`
builds below that step or environment.

## Nodes

A `DiffTree` is a tree of `DiffNode`s. The root compares the two arguments.
Each node has a `segment` (its path step from the parent, such as
`steps[Render]` or `script`; empty for the root), a `DiffStatus`, its own
`field_changes`, and its `children`.

| status | meaning | expanded |
|---|---|---|
| `Unchanged` | both sides present and equal under the model's `PartialEq` | no |
| `Modified` | both sides present and not equal | yes |
| `Added` | only the new side present | no |
| `Removed` | only the old side present | no |

Expanding a `Modified` node records each of its own fields that differs as a
`FieldChange` and adds one child node per pair of sub-values, unchanged ones
included. The status comes from one `PartialEq` call, so an unchanged subtree
costs one comparison and is not walked. Every field of a model type is either
compared as a field or descended into as a child, so a `Modified` node has a
field change or a changed child. The code destructures each model type without
`..`, so a new model field fails to compile until the diff handles it.

## Identity

Elements of collections are matched by identity, not position.

| collection | kind | matched by | children |
|---|---|---|---|
| `steps`, `jobEnvironments`, `stepEnvironments`, `embeddedFiles` | list | `name` | old-side order, then new-only |
| `hostRequirements.amounts`, `.attributes` | list | `name` | field changes, same order |
| `parameters`, `taskParameterDefinitions` | map | key | key order |
| `variables` | map | key | field changes, key order |
| `resolvedSymTab` | list | `name` | field changes, name order |

Names are unique within a collection. `create_job` guarantees this, and the
diff assumes it. An element's segment is `collection[name]`, with the name
written as is, unescaped.

## Order

A list's `PartialEq` depends on order, so a list whose shared names are in a
different relative order records `<collection>.order`, with the shared names
in each side's order. A pure reorder is then a `Modified` parent with that one
field change and unchanged children. A map's `PartialEq` ignores order, so
maps never record an order change and reordered map entries are `Unchanged`.

`Unchanged` means equal under `PartialEq`, which does not imply the same task
order. A reordered task parameter definition list with no `combination` is
`Unchanged` although it iterates tasks in another order (see "Out of scope").

An optional list or map that is absent on one side and empty on the other is
recorded as one field change named after the collection (`null -> []`). The
model types allow it, though `create_job` writes neither empty form.

## Resolved symbol tables

A step's or environment's actions keep their format strings unresolved:
`command` stays `{{Param.Scene}}`. The value a parameter contributes is in
`resolvedSymTab`, which `create_job` filters to the symbols that step or
environment references. Each symbol whose entry differs is one field change
`resolvedSymTab[NAME]`, the way `variables[K]` works: `before` and `after`
are that entry without its `name` (`{"type":...,"value":...}`), or `null`
when the symbol is absent on that side. When a side is not an array of
uniquely named entries, or no entry differs (an absent table against an empty
one, or the same entries in another order), the change is one whole-value
`resolvedSymTab` field. So a job parameter change shows on its
`parameters[K]` node and on exactly the job environments and steps that read
it. A step environment
has no `resolvedSymTab` of its own, so its reads show on its step. A change
that value equality ignores, such as a float's spelling (`2.5` to `2.50`),
shows only on the readers, and `parameters[K]` stays `Unchanged`. A
parameter that only feeds a template-scope field, such as a task parameter
range, shows on that field instead.

## Values

`before` and `after` are JSON, and `null` means absent. Most fields are their
derived `Serialize` output. A job parameter's `type` is its spec string and its
`value` is `ExprValue::transport_value`. A task parameter definition has three
fields: `type` (its spec string, such as `CHUNK[INT]`), `range` (the range
expression string, or the list) and `chunks`. Each is compared by its typed
value, and JSON is only how a side is written, so a range expression that
differs only in how it displays (`5,6` and `5-6`) is not a `range` change.

## Display

`Display` writes a tree as indented text, two spaces per level. Each changed
child is a line with `~` (modified), `+` (added) or `-` (removed) and its
segment; each field change is a line `field: before -> after`, values as
compact JSON, under its node. The root has no line, and an unchanged tree
writes nothing.

```
~ parameters[Scene]
  value: "a.blend" -> "b.blend"
~ steps[Prep]
  resolvedSymTab[Param.Scene]: {"type":"string","value":"a.blend"} -> {"type":"string","value":"b.blend"}
```

## Tree shape

| node | segment | own fields | children |
|---|---|---|---|
| Job | (root) | `name`, `description`, `extensions`, `steps.order`, `jobEnvironments`, `jobEnvironments.order` | `parameters[K]`, `jobEnvironments[N]`, `steps[N]` |
| JobParameter | `parameters[K]` | `name`, `type`, `value` | none |
| Step | `steps[N]` | `name`, `description`, `dependencies`, `resolvedSymTab[NAME]` (or `resolvedSymTab`), `stepEnvironments`, `stepEnvironments.order` | `script`, `stepEnvironments[N]`, `parameterSpace`, `hostRequirements` |
| Environment | `jobEnvironments[N]`, `stepEnvironments[N]` | `name`, `description`, `variables`, `variables[K]`, `resolvedSymTab[NAME]` (or `resolvedSymTab`) | `script` |
| StepScript | `script` | `let`, `embeddedFiles`, `embeddedFiles.order` | `onRun`, `embeddedFiles[N]` |
| EnvironmentScript | `script` | `let`, `embeddedFiles`, `embeddedFiles.order` | `onEnter`, `onWrapEnvEnter`, `onWrapTaskRun`, `onWrapEnvExit`, `onExit`, `embeddedFiles[N]` |
| Action | `onRun`, `onEnter`, ... | `command`, `args`, `timeout`, `cancelation` | none |
| EmbeddedFile | `embeddedFiles[N]` | `name`, `type`, `filename`, `data`, `runnable`, `endOfLine` | none |
| ParameterSpace | `parameterSpace` | `combination` | `taskParameterDefinitions[K]` |
| TaskParameter | `taskParameterDefinitions[K]` | `type`, `range`, `chunks` | none |
| HostRequirements | `hostRequirements` | `amounts`, `amounts.order`, `amounts[N]`, and the same for `attributes` | none |

## Out of scope

- Templates. The diff compares created jobs only.
- Tasks. Task parameter definitions are a map, so their order is not a
  change, although it fixes task iteration order. Which tasks a change adds
  or removes is not computed.
- Applying a diff, and any command-line interface. Segments are the only
  identity a node carries; a structured identity for applying a diff comes
  with that work.
- Options such as a depth limit. `diff_*` always expands every `Modified`
  node.

Tests: `crates/openjd-model/src/test_job_diff.rs`.
