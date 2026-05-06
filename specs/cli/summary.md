# summary Command

## Purpose

`openjd-rs summary <path>` prints summary information about a job template: its parameters,
steps, task counts, environments, and dependencies. It can summarize the entire job or a
single step. The command instantiates the job (resolving parameters and creating the job
object) to produce accurate task counts that reflect parameter space expansion.

## Interface

```
openjd-rs summary <PATH> [--step <STEP>] [-p <KEY=VALUE>]... [--extensions <EXT>] [--output <FMT>]
```

### Arguments

| Argument | Type | Required | Description |
|----------|------|----------|-------------|
| `path` | `PathBuf` | Yes | Path to the job template file |
| `--step` | `Option<String>` | No | Print summary for this step only |
| `-p`, `--parameter` | `Vec<String>` | No | Job parameters (`Key=Value`, `file://path`, or inline JSON) |
| `--extensions` | `Option<String>` | No | Comma-separated extension names |
| `--output` | `String` | No | Output format: `human-readable` (default), `json`, `yaml` |

### SummaryArgs Struct

```rust
#[derive(Args)]
pub struct SummaryArgs {
    pub path: PathBuf,
    #[arg(long)]
    pub step: Option<String>,
    #[arg(short = 'p', long = "parameter")]
    pub parameters: Vec<String>,
    #[arg(long = "extensions")]
    pub extensions: Option<String>,
    #[arg(long = "output", value_parser = ["human-readable", "json", "yaml"], default_value = "human-readable")]
    pub output: String,
}
```

## Execution Pipeline

```
execute(args)
  │
  ├── Read and parse template (same pattern as check: common::read_input_file)
  ├── Resolve extensions list
  ├── decode_job_template()
  │
  ├── Preserve parameter definition order from template
  │   └── param_order: Vec<String> from job_template.parameter_definitions_list()
  │
  ├── Parse CLI parameters via run::parse_cli_parameters()
  ├── Resolve job_template_dir and current_working_dir
  ├── preprocess_job_parameters() → param_values
  ├── create_job() → Job
  │
  └── Dispatch on --step
      ├── Some(step_name) → output_step_summary(&job, step_name, output_format)
      └── None → output_job_summary(&job, &param_order, output_format)
```

## Parameter Definition Order

The summary preserves the order in which parameters are defined in the template file. This
is important for human-readable output — users expect to see parameters in the order they
wrote them, not sorted alphabetically or in hash map iteration order.

The implementation captures the order from `job_template.parameter_definitions_list()` before
job creation (which stores parameters in a `HashMap`), then uses that order vector when
iterating over `job.parameters` for display.

## Task Count Calculation

`step_total_tasks()` computes the total number of tasks for a step by constructing a
`StepParameterSpaceIterator` with a chunk override of 1. The chunk override ensures that
chunked parameters (CHUNK[INT]) are counted as individual tasks rather than chunks, giving
the true task count.

```rust
fn step_total_tasks(step: &job::Step) -> usize {
    match &step.parameter_space {
        None => 1,
        Some(ps) => {
            match StepParameterSpaceIterator::new_with_chunk_override(ps, Some(1)) {
                Ok(it) => it.len(),
                Err(_) => 1,
            }
        }
    }
}
```

Steps with no parameter space have exactly 1 task (the implicit single task).

## Job Summary Output

`output_job_summary()` collects and displays:

1. **Parameters** — Name, type, and resolved value for each job parameter, in template
   definition order.
2. **Totals** — Total steps, total tasks (sum across all steps), total environments
   (root + step environments).
3. **Per-step details** — Name, description, task count, task parameter definitions
   (name and type), environment count, dependency count.
4. **Environments** — Root (job-level) and step-level environments with names and parent
   context.

### Human-Readable Format

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

### JSON Format

The JSON output includes a `status` and `message` field for consistency with the Python
CLI's `OpenJDCliResult` pattern, plus structured data for all summary fields. Step
parameter definitions are arrays of `{"name", "type"}` objects. Optional fields
(`description`, `environments`, `dependencies`) are omitted when empty.

### YAML Format

The YAML output contains the same structured data as JSON, serialized via
`serde_yaml::to_string()`. Both formats share the same `serde_json::Value` construction
code, so they are always in sync.

## Step Summary Output

`output_step_summary()` displays details for a single step:

- Total tasks, total task parameters, total environments
- Dependencies (step names)
- Parameter definitions (name and type)
- Environments (name, parent step, description)

The step is looked up by name in `job.steps`. If the step name doesn't match any step,
an error is returned with the job name for context.

## Internal Types

Three private structs organize the collected summary data:

```rust
struct StepInfo {
    name: String,
    description: Option<String>,
    total_tasks: usize,
    task_params: Vec<(String, String)>,  // (name, type_name)
    envs: Vec<String>,
    deps: Vec<String>,
}

struct ParamInfo {
    name: String,
    param_type: String,
    value: String,
}

struct EnvInfo {
    name: String,
    description: Option<String>,
    parent: String,
}
```

These are display-oriented — they hold string representations ready for output, not the
original model types. Task parameters are sorted alphabetically by name for deterministic
output.

## Task Parameter Type Names

`task_param_type_name()` maps `job::TaskParameter` variants to their specification string
names:

| Variant | Display Name |
|---------|-------------|
| `Int` | `INT` |
| `Float` | `FLOAT` |
| `String` | `STRING` |
| `Path` | `PATH` |
| `ChunkInt` | `CHUNK[INT]` |

## Differences from Python CLI

| Aspect | Python | Rust |
|--------|--------|------|
| Result type | `OpenJDJobSummaryResult` / `OpenJDStepSummaryResult` dataclasses | Inline output in `output_*_summary()` functions |
| Output decorator | `@print_cli_result` handles format dispatch | Manual `match` on output format string |
| JSON/YAML parity | Identical structured output in both | Identical — same value, different serializer |
| Parameter display | Shows `description` field | Shows `value` field |

The Python CLI's summary shows parameter descriptions (from the template definition) while
the Rust CLI shows resolved parameter values. Both are useful — the Python approach helps
users understand what parameters mean, while the Rust approach shows what values will be
used for job execution.
