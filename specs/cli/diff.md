# diff Command (unstable)

**Unstable.** The flags and output of `openjd diff` may change in any release. The
command prints `warning: 'openjd diff' is unstable; its flags and output may change.` to
stderr when it starts, after argument parsing, so a usage error prints only the usage
error. Stdout is only the diff or the document. `openjd diff --help` ends its summary
line with ` (unstable)`.

## Purpose

`openjd diff` reports what changes between two jobs and where each change came from. It
is the one diff command: it compares one template under two parameter sets, or two job
templates, each side with its own parameters. It is the command-line form of `openjd_model::job::template_diff`
([model/template-diff.md](../model/template-diff.md)), which builds both jobs the same way
`summary` and `run` do and diffs them with `job::diff`.

## Interface

```
openjd diff [OPTIONS] <TEMPLATE> [NEW_TEMPLATE]

openjd diff <TEMPLATE> [NEW_TEMPLATE] [-p <PARAM>]... [--old-parameter <PARAM>]...
            [--new-parameter <PARAM>]... [--environment <FILE>]... [--extensions <EXT>]
            [--step <NAME>] [--tasks | --stat] [--plan] [--depth <N>] [--ignore-symbols]
            [--output human-readable|json|yaml] [--verbose] [--exit-code]
```

With one template, the old and new sides are that template and differ only by their
parameters. With two, `TEMPLATE` is the old side and `NEW_TEMPLATE` the new one.

| Argument | Description |
|----------|-------------|
| `TEMPLATE`, `NEW_TEMPLATE` | Template files, or `-` for stdin (at most one input of the command, counting `--environment`). With only `TEMPLATE`, both sides use that template and differ in `--old-parameter` / `--new-parameter` |
| `-p`, `--parameter` | Job parameter for both sides. Hidden alias `--job-param` |
| `--old-parameter`, `--new-parameter` | Job parameter for one side; applied after `-p`, so it wins. Hidden aliases `--old-param`, `--new-param` |
| `--environment`, `--env` | Environment template file for both sides, or `-` for stdin; repeatable. Each is read once and used by both sides |
| `--extensions` | Extension allowlist, parsed as for `check` and `summary`; default every supported extension |
| `--step` | Keep only this step and the job-level changes, and with `--plan` only this step's ops and the job-level ops (job fields, parameters, job environments). A change and a note belong to the step their `step` field names (`Change::step`, `Note::step`), so the name is matched exactly and never as a path prefix: `--step A` keeps neither the step `A][B` nor the step `A].x`. The step's own unexpanded change (`~ (not expanded)`, at `--depth 1`) is kept. Notes about other steps are dropped; job-level notes are kept. A step in neither job is an error. `--step` with `--depth 0` is a usage error (exit 2): at depth 0 the only change is the unexpanded job root, which cannot be split by step. `--exit-code` and `changed` see only what is kept, so a change in another step alone gives exit 0 and `"changed": false`. `--verbose` is not filtered: the job diff tree, and `oldJob` and `newJob`, are the whole jobs |
| `--tasks` | Print only each shown step's task summary. Conflicts with `--stat` (exit 2) |
| `--stat` | Print one line per shown step, then the totals |
| `--plan` | Also print `job::apply::plan` on the two jobs: one op per line with its mutation kinds. The plan always compares the whole jobs, so with `--ignore-symbols` it can list a `StepSymbols` op for a step the diff shows as unchanged (`0 steps changed`) |
| `--depth` | `DiffOptions::with_max_depth` |
| `--ignore-symbols` | `DiffOptions::with_resolved_symtab(false)` |
| `--output` | `human-readable` (default), `json`, `yaml` |
| `--verbose` | Human-readable: also print the job diff tree (`DiffTree`'s `Display`) after the summary. `json` or `yaml`: also include `oldJob`, `newJob` and the `job` tree |
| `--exit-code` | Exit 0 with no differences, 4 with differences. With `--step`, only the differences it keeps count |

### Parameters

`openjd diff` parses parameters exactly as `openjd run -p` does. The rules live in one
place, [parameter-parsing.md](parameter-parsing.md). A parameter takes one of three forms:

| form | example |
|---|---|
| `Key=Value` | `-p Frames=1-200` |
| a JSON or YAML file of `{Key: Value}`, as `file://path` | `-p file://params.json` |
| inline JSON | `-p '{"Frames": "1-200"}'` |

Each flag is repeatable, and a later value wins. A side's parameters are its `-p` values
followed by its `--old-parameter` or `--new-parameter` values. So the side flag wins over
`-p`, wherever it appears on the command line.

`--job-param` is a hidden alias of `-p`, because it is the long name `run` and `summary`
use. `--old-param` and `--new-param` are hidden aliases of the side flags, because they
are the earlier spellings. `--help` lists only `--parameter`, `--old-parameter` and
`--new-parameter`. The `-p` help names the three forms with one example each. The
`--old-parameter` and `--new-parameter` help says `in the same forms as -p` and gives one
`Key=Value` example.

The syntax follows `deadline bundle submit -p` on deadline-cloud mainline (release 0.60.11
accepts only Key=Value), but the values it produces are not guaranteed to be identical.

#### Known differences

This list is not exhaustive. Each entry was run against `openjd diff` with a template
whose parameter `A` is a STRING.

- `file://~/p.json` does not expand `~`: `Provided parameter file '~/p.json' does not exist.`
- A null value, such as `{"A": null}` inline or in a JSON file, means not given, so the
  template default applies. `deadline bundle submit -p '{"A": null}'` instead fails with
  `Parameter A has no value.` (deadline-cloud mainline; release 0.60.11 accepts only
  Key=Value). openjd-rs follows deadline's job bundle parameter handling here
  (see [parameter-parsing.md](parameter-parsing.md)). A file that is only `null` fails with
  `Job parameter file 'file://p.json' should contain a dictionary.`
- Every value other than null is passed as text. JSON `{"A": true}` gives `true`.
- For an INT parameter `N`, a YAML file with `N: 007` fails with `Parameter 'N': Value
  '7.0' is not a valid integer or integer string.`, where `deadline` gives 7.
- A malformed parameter file reports `Job parameter file 'file://bad.yaml' should contain
  a dictionary.`, without the parse location.
- The parser accepts `-p =v`; building the job then fails with an error that names no
  parameter: `Job parameter values provided for parameters that are not defined in the
  template: `.

Which library call runs:

| inputs | call |
|---|---|
| `TEMPLATE` only | `diff_job_template_with_parameter_sets(TEMPLATE, -p + --old-parameter, -p + --new-parameter)` |
| `TEMPLATE NEW_TEMPLATE` | `diff_templates(TEMPLATE with -p + --old-parameter, NEW_TEMPLATE with -p + --new-parameter)` |

To diff two jobs that are already built, call the library's `diff_built_jobs` on two
`job_from_str` results; the CLI has no form for it.

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
`TEMPLATE -> NEW_TEMPLATE` for two templates and `TEMPLATE: old parameters -> new parameters`
for one, each argument as given (`-` for stdin).

```
$ openjd diff render_v1.yaml render_v2.yaml --new-parameter Frames=1-200 -p Scene=/proj/shot010.blend
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
`no changes`. A note says why job-layer origins are `[both]`: the new template does not
build with the old parameters. A note also says why a step's tasks are unknown. Through
the CLI that happens only with adaptive chunking. The other cause, a parameter space that
does not iterate, needs an edited built job, which only the library's `diff_built_jobs`
takes ([model/template-diff.md](../model/template-diff.md#steps-and-tasks)).

```
$ openjd diff render_v1.yaml quality.yaml -p Scene=/s --new-parameter Quality=3
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
Each `Shown` keeps `raw`, the job value in its document's host encoding, next to `text`. `--tasks` and `--stat` affect only human-readable output. The
document is data rather than a command result: it has no `status` or
`message`, and its keys are camelCase (see
[output-formatting.md](output-formatting.md#status-convention)).

## Exit codes

| case | exit |
|---|---|
| success, without `--exit-code` | 0, whether or not the inputs differ |
| `--exit-code`, no differences | 0 |
| `--exit-code`, differences | 4 |
| any error (missing file, bad parameter, a template that does not build) | 1, with `ERROR: …` on stderr |
| usage error (clap), including an unknown flag | 2 |

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
it.

Tests: `crates/openjd-cli/tests/diff_tests.rs` and `diff_examples.rs`.
