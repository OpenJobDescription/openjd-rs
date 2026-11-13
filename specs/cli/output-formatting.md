# Output Formatting

## Purpose

All commands that produce structured output support an `--output` flag with three modes:
`human-readable` (default), `json`, and `yaml`. This allows both interactive use (readable
terminal output) and programmatic consumption (parseable structured output).

## Supported Commands

| Command | `--output` support | Default |
|---------|-------------------|---------|
| `check` | No | Plain text only |
| `summary` | Yes | `human-readable` |
| `run` | Yes | `human-readable` |
| `diff` | Yes | `human-readable` |

The `check` command produces only a single success/error message and doesn't benefit from
structured output. Adding `--output` support would be straightforward but low value.

## Output Structure

### Status Convention

JSON and YAML outputs include `status` and `message` fields, following the Python CLI's
`OpenJDCliResult` pattern:

- `status`: `"success"` or `"error"`
- `message`: Human-readable summary of the result

This convention enables simple programmatic checks (`jq .status`) without parsing the
full output.

`diff` is an exception. Its document is data, not a command result: it prints a
`TemplateDiff`, and has no `status` or `message`. A failure prints `ERROR: …` on stderr and
exits 1, as for every command, and `diff` puts `changed` first in its document. Its keys
are camelCase (`oldJob`, `oldIndex`), because they are the serde form of the openjd-model
types, while `summary` and `run` use snake_case (`total_environments`). See
[diff.md](diff.md).

### Summary Command Output

**Human-readable:**
```
--- Summary for 'MyJob' ---

Parameters:
  - Frames (INT): 1-100
  - OutputDir (PATH): /output

Total steps: 2
Total tasks: 100
Total environments: 1

--- Steps in 'MyJob' ---

1. 'Render' (100 total Tasks)
  Task parameters:
    - Frame (INT)
  1 environments

2. 'Encode' (1 total Tasks)
  1 dependencies
```

**JSON:**
```json
{
  "status": "success",
  "message": "Summary for 'MyJob'",
  "name": "MyJob",
  "parameter_definitions": [
    {"name": "Frames", "type": "INT", "value": "1-100"}
  ],
  "total_steps": 2,
  "total_tasks": 100,
  "total_environments": 1,
  "steps": [
    {
      "name": "Render",
      "total_tasks": 100,
      "parameter_definitions": [{"name": "Frame", "type": "INT"}],
      "environments": 1
    }
  ]
}
```

**YAML:** Same structured data as JSON, serialized via `serde_yaml::to_string()`:
```yaml
status: success
message: Summary for 'MyJob'
name: MyJob
parameter_definitions:
- name: Frames
  type: INT
  value: 1-100
total_steps: 2
total_tasks: 100
total_environments: 1
steps:
- name: Render
  total_tasks: 100
  parameter_definitions:
  - name: Frame
    type: INT
  environments: 1
```

### Run Command Output

**Human-readable:**
```
--- Results of local session ---

Session ended successfully

Job: MyJob
Step: Render
Duration: 42.123 seconds
Tasks run: 100
```

**JSON:**
```json
{
  "status": "success",
  "message": "Session ended successfully",
  "job_name": "MyJob",
  "step_name": "Render",
  "duration": 42.123,
  "tasks_run": 100
}
```

**YAML:**
```yaml
status: success
message: Session ended successfully
job_name: MyJob
step_name: Render
duration: 42.123
tasks_run: 100
```

## Implementation Pattern
JSON and YAML share the same code path — a `serde_json::Value` object is built once, then
serialized with either `serde_json::to_string_pretty()` or `serde_yaml::to_string()`:

```rust
match args.output.as_str() {
    "json" | "yaml" => {
        let val = /* build serde_json::Value */;
        if output_format == "json" {
            println!("{}", serde_json::to_string_pretty(&val)?);
        } else {
            print!("{}", serde_yaml::to_string(&val)?);
        }
    }
    _ => { /* human-readable println! */ }
}
```

This ensures JSON and YAML outputs contain identical structured data — the only difference
is the serialization format.

`diff` is an exception: it serializes its top-level struct (the `TemplateDiff` document,
whose `oldJob` and `newJob` are `JobDocument`s) directly with `serde_json::to_string_pretty`
or `serde_saphyr::to_string`, not through one `serde_json::Value`, so its fields print in the
structs' order rather than sorted.
Inside the `diff` document, each `job` tree node and each change's `raw` is a `Value`, so
their keys are sorted. Neither depends on key order: `taskParameterDefinitions` is a list
in definition order in every job document (see `specs/model/host-support.md`, "Host
encoding").

### JSON/YAML Construction

Output values are built using `serde_json::Map` and `serde_json::json!()` macro rather than
serializing a dedicated struct. This avoids defining output-specific structs but means the
output schema is implicit in the code rather than enforced by types. `diff` is the
exception above: its schema is the serde form of the openjd-model types.

The Python CLI uses dataclass-based result types (`OpenJDCliResult`, `OpenJDJobSummaryResult`,
etc.) that are serialized via the `@print_cli_result` decorator. This is more type-safe
but requires maintaining parallel type hierarchies (domain types + output types).

## Conditional Fields

Both JSON and human-readable formats omit empty optional fields:

- `description` — omitted if the step/environment has no description
- `environments` — omitted if the step has no environments
- `dependencies` — omitted if the step has no dependencies
- `total_environments` — omitted if zero (in JSON summary output)
- `step_name` — omitted if no specific step was selected (in run output)

## Differences from Python CLI

| Aspect | Python | Rust |
|--------|--------|------|
| Output dispatch | `@print_cli_result` decorator | Inline `match` per command |
| Result types | Dedicated dataclasses per command | No output types; inline construction |
| JSON/YAML parity | Full structured output in both formats | Identical — same value, different serializer |
| Error output | Structured error in chosen format | Plain text to stderr |
| Check command | Supports `--output` | No `--output` support |

The Python CLI's decorator pattern is more DRY — output formatting logic is written once
and applied to all commands. The Rust CLI's inline approach is simpler but requires each
command to handle formatting independently. If more commands are added, extracting a shared
output formatting utility would reduce duplication.
