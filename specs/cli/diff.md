# diff Command (unstable)

**Unstable.** The flags and output of `openjd diff` may change in any release. The
command prints `warning: 'openjd diff' is unstable; its flags and output may change.` to
stderr when it starts, after argument parsing, so a usage error prints only the usage
error. Stdout is only the diff or the document. `openjd diff --help` ends its summary
line with ` (unstable)`.

## Purpose

`openjd diff` reports what changes between two jobs and where each change came from. It
compares two job templates, one template under two parameter sets, or two jobs that
`openjd build` printed. It is the command-line form of `openjd_model::job::template_diff`
([model/template-diff.md](../model/template-diff.md)), which builds both jobs the same way
`summary` and `run` do and diffs them with `job::diff`.

## Interface

```
openjd diff <OLD> [NEW] [-p <PARAM>]... [--old-param <PARAM>]... [--new-param <PARAM>]...
            [--environment <FILE>]... [--extensions <EXT>] [--jobs]
            [--step <NAME>] [--tasks | --stat] [--plan] [--depth <N>] [--ignore-symbols]
            [--output human-readable|json|yaml] [--verbose] [--exit-code]
```

| Argument | Description |
|----------|-------------|
| `OLD`, `NEW` | Template files, or `-` for stdin (at most one input of the command, counting `--environment`). With only `OLD`, both sides use that template and differ in `--old-param` / `--new-param` |
| `-p`, `--job-param`, `--parameter` | Job parameter for both sides. Same parser as `run` ([parameter-parsing.md](parameter-parsing.md)): `Key=Value`, `file://path`, inline JSON; repeatable, later wins |
| `--old-param`, `--new-param` | Job parameter for one side, same parser; applied after `-p`, so it wins |
| `--environment`, `--env` | Environment template file for both sides, or `-` for stdin; repeatable. Each is read once and used by both sides |
| `--extensions` | Extension allowlist, parsed as for `check` and `summary`; default every supported extension |
| `--jobs` | `OLD` and `NEW` are built jobs (JSON or YAML from `openjd build`). Requires `NEW` (a usage error without it). Conflicts with `-p`, `--old-param`, `--new-param`, `--environment` and `--extensions` (exit 2) |
| `--step` | Keep only this step and the job-level changes, and with `--plan` only this step's ops and the job-level ops (job fields, parameters, job environments). A change and a note belong to the step their `step` field names (`Change::step`, `Note::step`), so the name is matched exactly and never as a path prefix: `--step A` keeps neither the step `A][B` nor the step `A].x`. The step's own unexpanded change (`~ (not expanded)`, at `--depth 1`) is kept. Notes about other steps are dropped; job-level notes are kept. A step in neither job is an error. `--step` with `--depth 0` is a usage error (exit 2): at depth 0 the only change is the unexpanded job root, which cannot be split by step. `--exit-code` and `changed` see only what is kept, so a change in another step alone gives exit 0 and `"changed": false`. `--verbose` is not filtered: the job diff tree, and `oldJob` and `newJob`, are the whole jobs |
| `--tasks` | Print only each shown step's task summary. Conflicts with `--stat` (exit 2) |
| `--stat` | Print one line per shown step, then the totals |
| `--plan` | Also print `job::apply::plan` on the two jobs: one op per line with its mutation kinds. The plan always compares the whole jobs, so with `--ignore-symbols` it can list a `StepSymbols` op for a step the diff shows as unchanged (`0 steps changed`) |
| `--depth` | `DiffOptions::with_max_depth` |
| `--ignore-symbols` | `DiffOptions::with_resolved_symtab(false)` |
| `--output` | `human-readable` (default), `json`, `yaml` |
| `--verbose` | Human-readable: also print the job diff tree (`DiffTree`'s `Display`) after the summary. `json` or `yaml`: also include `oldJob`, `newJob` and the `job` tree |
| `--exit-code` | Exit 0 with no differences, 4 with differences. With `--step`, only the differences it keeps count |

Which library call runs:

| inputs | call |
|---|---|
| `OLD` only | `diff_job_template_with_parameter_sets(OLD, -p + --old-param, -p + --new-param)` |
| `OLD NEW` | `diff_templates(OLD with -p + --old-param, NEW with -p + --new-param)` |
| `--jobs OLD NEW` | `diff_built_jobs(job_from_str(OLD), job_from_str(NEW))` |

Each template side gets the CLI's caller limits (`common::caller_limits`), as `summary`
and `run` do. A template is read as a file (the format from its extension) and its
directory sets relative PATH defaults. Stdin text is JSON when its first non-whitespace
character is `{` and YAML otherwise, with the working directory as its template directory. Inline template text is
not accepted on the command line, because YAML is hard to quote in a shell argument.

`--help` ends with examples of each form that use the shipped files in
`crates/openjd-cli/examples/diff/` and `crates/openjd-model/examples/template_diff/`, and a
line pointing to the full gallery, [diff-examples.md](diff-examples.md).

## Output

Human-readable output starts with a header naming the two sides, then `TemplateDiff`'s
`Display` ([model/template-diff.md](../model/template-diff.md#rendering)). The header is
`OLD -> NEW` for two templates, `OLD: old parameters -> new parameters` for one template,
and `jobs: OLD -> NEW` with `--jobs`, each argument as given (`-` for stdin).

```
$ openjd diff render_v1.yaml render_v2.yaml --new-param Frames=1-200 -p Scene=/proj/shot010.blend
render_v1.yaml -> render_v2.yaml
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

`--stat`:

```
render_v1.yaml -> render_v2.yaml
~ Render  tasks 100 -> 400  (+300 -0)
1 step changed. Tasks: 100 kept, 300 added, 0 removed.
```

`--plan` appends:

```

apply plan:
  1. parameter modify Frames  [ParameterValue]
  2. tasks of Render: keep 100, add 300, remove 0  [TaskAppend]
kinds: ParameterValue, TaskAppend
```

With `--step`, the plan keeps that step's `step` and `tasks of` ops and every job-level op,
and `kinds` is the union over the ops kept. A plan with no ops prints `nothing to apply`
and `kinds: none`.

A step is shown when its status is not `unchanged` or when a change is in it (a
step-level `let` that nothing reads, for example). `--tasks` and `--stat` print
`no changes` only when nothing differs. When only job-level changes differ (a job rename,
an added job environment, a parameter that no step reads), they print
`job-level changes only` in place of the step lines, and `--stat` still prints the totals:

```
$ openjd diff wrap_simple_job.yaml wrap_job_with_inner_env.yaml --stat
wrap_simple_job.yaml -> wrap_job_with_inner_env.yaml
job-level changes only
0 steps changed. Tasks: 1 kept, 0 added, 0 removed.
```

`--verbose` appends a blank line, `job diff tree:`, and the job layer's `DiffTree`
listing, or `no changes`.

Every human-readable form ends its diff with one `note: …` line per entry of
`TemplateDiff::notes`: after the totals line, after the `--tasks` lines, or after
`no changes`. A note says why job-layer origins are `[both]` (the new template does not
build with the old parameters) or why a step's tasks are unknown (adaptive chunking, or a parameter space that does not
iterate):

```
$ openjd diff render_v1.yaml quality.yaml -p Scene=/s --new-param Quality=3
render_v1.yaml -> quality.yaml
+ parameter Quality: 3, defined as {type: INT}  [both]

0 steps changed. Tasks: 100 kept, 0 added, 0 removed.
note: origin: the new template does not build with the old parameters, so job-layer changes are marked both: Validation error: Values missing for required job parameters: Quality
```

`--output json` and `yaml` print one document and nothing else on stdout, with no header:
`changed` (whether anything differs, the same test as `--exit-code`, so with `--step` only
what that step and the job level show), then the
`TemplateDiff` serde form with `changes`, `steps` and `notes`, plus `plan` with `--plan`
(`{ops: [{action, kinds}], kinds}`, filtered as above with `--step`). `--verbose` adds
`oldJob`, `newJob` (each a `JobDocument`, so parameters and environment variables are
sorted) and `job`.
Each `Shown` keeps `raw`, the job value in its document's host encoding, next to `text`. `--tasks` and `--stat` affect only human-readable output. Like
`build`, the document is data rather than a command result: it has no `status` or
`message`, and its keys are camelCase (see
[output-formatting.md](output-formatting.md#status-convention)).

## Exit codes

| case | exit |
|---|---|
| success, without `--exit-code` | 0, whether or not the inputs differ |
| `--exit-code`, no differences | 0 |
| `--exit-code`, differences | 4 |
| any error (missing file, bad parameter, a template that does not build) | 1, with `ERROR: …` on stderr |
| usage error (clap), including `--jobs` without `NEW` | 2 |

4 keeps "the inputs differ" apart from 1, which `openjd` already uses for every error,
and from clap's 2. Errors name the path (`'nope.yaml' does not exist.`) or the input that
failed: `old template: …` / `new template: …` for two templates, `old parameters: …` /
`new parameters: …` for one template under two parameter sets, and `the template: …` when
that one template does not decode.

## Not implemented

`diff-step` (a step template in the context of its job), `diff-space` (two parameter
spaces), `--origin` as a switch (the origin tag is always printed), and a render check.
See [model/template-diff.md](../model/template-diff.md#the-template-layer).

## Examples

[diff-examples.md](diff-examples.md) has one case per field the diff can report: the
command, its output, and for some cases its `--plan` output and JSON document. The test
`crates/openjd-cli/tests/diff_examples.rs` runs every case and fails when the file is out
of date; `UPDATE_DIFF_EXAMPLES=1 cargo test -p openjd-cli --test diff_examples` rewrites
it, and the two built jobs its `--jobs` case reads.

Tests: `crates/openjd-cli/tests/diff_tests.rs` and `diff_examples.rs`.
