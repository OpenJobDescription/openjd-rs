# build Command (unstable)

**Unstable.** The flags and output of `openjd build` may change in any release. The
command prints `warning: 'openjd build' is unstable; its flags and output may change.` to
stderr when it starts. Stdout is only the job document, so `openjd build … > job.json`
still writes a job that `openjd diff --jobs` reads. `openjd build --help` ends its summary
line with ` (unstable)`.

## Purpose

`openjd build <path>` prints the job a template builds into: the output of `create_job`,
including each step's resolved symbol table. It shows what a template turns into, and its
output is the input of `openjd diff --jobs` ([diff.md](diff.md)).

## Interface

```
openjd build <PATH> [-p <PARAM>]... [--environment <FILE>]... [--extensions <EXT>] [--output json|yaml]
```

| Argument | Description |
|----------|-------------|
| `PATH` | Job template file |
| `-p`, `--job-param`, `--parameter` | Job parameters, as for `run` ([parameter-parsing.md](parameter-parsing.md)) |
| `--environment`, `--env` | Environment template files; their parameter definitions are merged |
| `--extensions` | Extension allowlist, as for `check`; default every supported extension |
| `--output` | `json` (default) or `yaml` |

The job is built by `TemplateInput::build` (`openjd_model::job::template_diff`) with the
CLI's caller limits: the same decode, parameter preprocessing and `create_job` calls as
`summary` and `run`.

## Output

The job as a `JobDocument` (`openjd_model::job::template_diff`), pretty JSON or YAML: a
`"documentVersion": 1` field first, then the `Job`'s serde form with `parameters` and each environment's `variables` sorted by name, so
the output is the same on every run (`create_job` fills `parameters` from a `HashMap`, and
`Environment::variables` is a `HashMap`), and parameters and parameter spaces in the host
encoding of [`specs/model/host-support.md`](../model/host-support.md#host-encoding): a
parameter is `{"name": "Frames", "type": "STRING", "value": "1-100"}`, and
`taskParameterDefinitions` is a list in definition order. It is serialized directly rather
than through `serde_json::Value`, so fields print in the structs' order.
`job_from_str` reads either format back, and `openjd diff --jobs` on two outputs of
`openjd build` for the same inputs prints `no changes`. The document is data, so it has no
`status` or `message` field ([output-formatting.md](output-formatting.md#status-convention)).

Errors print `ERROR: …` after the warning and exit 1, as for every command.

Tests: `crates/openjd-cli/tests/diff_tests.rs` (`jobs_round_trip_through_build`,
`build_errors_and_bad_jobs`, `build_defaults_to_json_and_sorts_environment_variables`,
`build_prints_the_same_job_on_every_run`,
`build_merges_environment_parameters_under_both_spellings`,
`build_accepts_parameter_as_an_alias_for_job_param`, `build_applies_the_cli_caller_limits`).
