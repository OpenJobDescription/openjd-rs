# `openjd diff` examples

`openjd diff` compares two job templates, or one template under two sets of job
parameters. It tells you what changed in the job and what happens to its tasks: which are
kept, which are added, and which are replaced.

The command is unstable. Its flags and output may change in any release, and it prints a
warning saying so to stderr, which is left out below.

Run the commands from this directory. The output shown is the real output, checked by a
test.

## One template, more frames

`frame_range.yaml` takes its frame range from the `Frames` parameter. Give the old side
1-3 and the new side 1-5. `--plan` adds the steps a job store would take to apply the
change.

```console
$ openjd diff frame_range.yaml --old-parameter Frames=1-3 --new-parameter Frames=1-5 --plan
frame_range.yaml: old parameters -> new parameters
~ parameter Frames: 1-3 -> 1-5  [parameters]
~ step Render
    ~ Frame: 1-3 -> 1-5  [parameters]
    tasks: 3 -> 5
      keep   3     Frame 1-3
      add    2     Frame 4,5

1 step changed. Tasks: 3 kept, 2 added, 0 removed.

apply plan:
  1. parameter modify Frames  [ParameterValue]
  2. tasks of Render: keep 3, add 2, remove 0  [TaskAppend]
kinds: ParameterValue, TaskAppend
```

What to look for:

- `[parameters]` at the end of a line says the parameter values caused the change.
- The three frames that already exist are kept, and frames 4 and 5 are added.

## A template edit and a parameter change

`render_v2.yaml` adds the right eye to `render_v1.yaml`. The new side also asks for one
more frame.

```console
$ openjd diff render_v1.yaml render_v2.yaml --new-parameter Frames=1-4
render_v1.yaml -> render_v2.yaml
~ parameter Frames: 1-3 -> 1-4  [parameters]
~ step Render
    ~ Frame: 1-3 -> 1-4  [parameters]
    ~ Eye: [left] -> [left, right]  [template]
    tasks: 3 -> 8
      keep   3     Frame 1-3 × Eye left
      add    2     Frame 4 × Eye left, right
      add    3     Frame 1-3 × Eye right

1 step changed. Tasks: 3 kept, 5 added, 0 removed.
```

What to look for:

- Each change says what caused it: `[template]` for the edit, `[parameters]` for the
  values.
- The task space is a product of `Frame` and `Eye`, so it grows in both directions. Every
  existing task is kept.

## Two different templates

`pipeline_v2.yaml` renames the job, adds a `Resolution` parameter, changes the render
command, and adds a `Composite` step that depends on `Render`.

Parameters come from three places here:

- `-p file://shot.json` gives both sides `Frames` and `Quality`.
- `--new-parameter file://new_side.yaml` gives the new side `Resolution`.
- `--new-parameter '{"Quality": null}'` takes `Quality` away from the new side again, so
  the template's default, `draft`, applies.

```console
$ openjd diff pipeline_v1.yaml pipeline_v2.yaml -p file://shot.json --new-parameter file://new_side.yaml --new-parameter '{"Quality": null}'
pipeline_v1.yaml -> pipeline_v2.yaml
~ name: Shot010 -> Shot010-final  [template]
~ parameter Quality: final -> draft  [parameters]
+ parameter Resolution: 2160, defined as {default: 1080, type: INT}  [template]
~ step Render
    ~ resolvedSymTab: Param.Quality=final -> Param.Quality=draft, Param.Resolution=2160  [both]
    ~ script.onRun.args: [--quality, {{Param.Quality}}, --frame, {{Task.Param.Frame}}] -> [--quality, {{Param.Quality}}, --height, {{Param.Resolution}}, --frame, {{Task.Param.Frame}}]  [template]
    ~ script.onRun.command: render -> render-gpu  [template]
    tasks: 2 unchanged
+ step Composite
    tasks: 1 added

1 step changed, 1 added. Tasks: 2 kept, 1 added, 0 removed.
```

What to look for:

- `resolvedSymTab` is the set of values the step's commands see. It changed for two
  reasons, so it says `[both]`.
- The `Render` command changed, but its tasks are the same frames, so they are kept.

## What is kept and what is replaced

`tasks_v2.yaml` gives `Render` a new task parameter, `Eye`. It also changes a `let` in
`Composite` that no command reads.

```console
$ openjd diff tasks_v1.yaml tasks_v2.yaml --plan
tasks_v1.yaml -> tasks_v2.yaml
~ step Render
    + Eye: [left, right]  [template]
    tasks: 3 -> 6, all replaced (task parameter added or removed)
~ step Composite
    ~ let: [fps = 24] -> [fps = 30]  [template]
    tasks: 1 unchanged

2 steps changed. Tasks: 1 kept, 0 added, 0 removed, 6 replaced.

apply plan:
  1. tasks of Render: replace all  [TaskReplace]
kinds: TaskReplace
```

What to look for:

- A new task parameter changes what a task is, so every `Render` task is replaced.
- The `Composite` change shows in the template, but the job it builds is the same. The
  plan has nothing to do for it, and its task is kept.

## More

`openjd diff --help` lists every flag. [`specs/cli/diff-cases.md`](../../../../specs/cli/diff-cases.md)
is the test suite's output for every field the diff can report.

The test `crates/openjd-cli/tests/diff_cases.rs` runs every command on this page and fails
when its output differs from what is shown. After changing the diff output, rewrite the
page with `UPDATE_DIFF_CASES=1 cargo test -p openjd-cli --test diff_cases`.
