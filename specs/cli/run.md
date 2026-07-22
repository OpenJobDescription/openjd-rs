# run Command

## Purpose

`openjd run <path>` executes a job template locally in an OpenJD session. It is the
most complex command in the CLI, orchestrating the full session lifecycle: template parsing,
parameter resolution, job creation, environment enter/exit, task iteration with adaptive
chunking, embedded file materialization, let binding evaluation, and structured result output.

## Interface

```
openjd run <PATH> [--step <STEP>] [-p <KEY=VALUE>]... [-t <KEY=VALUE>]...
    [--tasks <JSON|file://path>] [--maximum-tasks <N>]
    [--environment <PATH>]... [--path-mapping-rules <JSON|file://path>]
    [--run-dependencies] [--no-run-dependencies]
    [--extensions <EXT>] [--preserve] [--verbose]
    [--timestamp-format <FMT>] [--output <FMT>]
```

### Arguments

| Argument | Type | Required | Default | Description |
|----------|------|----------|---------|-------------|
| `path` | `PathBuf` | Yes | — | Path to the job template file |
| `--step` | `Option<String>` | No | — | Step to run (auto-selects if single step; runs all if omitted with multi-step job) |
| `-p`, `--parameter` | `Vec<String>` | No | — | Job parameters |
| `-t`, `--task-param` | `Vec<String>` | No | — | Explicit task parameter values for a single task |
| `--tasks` | `Option<String>` | No | — | JSON array or file of task parameter sets |
| `--maximum-tasks` | `i64` | No | `-1` | Max tasks to run (-1 = all) |
| `--environment` | `Vec<PathBuf>` | No | — | Additional environment template files |
| `--path-mapping-rules` | `Option<String>` | No | — | Path mapping rules (JSON or file://) |
| `--run-dependencies` | `bool` | No | `false` | Run transitive dependency steps first |
| `--no-run-dependencies` | `bool` | No | — | Explicit opt-out (default behavior) |
| `--extensions` | `Option<String>` | No | all | Comma-separated extension names |
| `--preserve` | `bool` | No | `false` | Keep session working directory after completion |
| `--verbose` | `bool` | No | `false` | Enable DEBUG-level logging |
| `--timestamp-format` | `String` | No | `relative` | Log timestamp format: `relative`, `local`, `utc` |
| `--output` | `String` | No | `human-readable` | Output format: `human-readable`, `json`, `yaml` |

### Mutual Exclusivity

Three task selection modes are mutually exclusive (enforced by clap `conflicts_with_all`):

- `--task-param` — Run a single task with explicit parameter values
- `--tasks` — Run specific tasks from a JSON array
- `--maximum-tasks` — Run up to N tasks from the parameter space

### RunArgs Struct

```rust
#[derive(Args)]
pub struct RunArgs {
    pub path: PathBuf,
    #[arg(long)]
    pub step: Option<String>,
    #[arg(short = 'p', long = "parameter")]
    pub parameters: Vec<String>,
    #[arg(long = "task-param", short = 't', action = clap::ArgAction::Append,
          conflicts_with_all = ["tasks", "maximum_tasks"])]
    pub task_params: Vec<String>,
    #[arg(long = "tasks", conflicts_with_all = ["task_params", "maximum_tasks"])]
    pub tasks: Option<String>,
    #[arg(long = "environment", alias = "env")]
    pub environments: Vec<PathBuf>,
    #[arg(long = "path-mapping-rules")]
    pub path_mapping_rules: Option<String>,
    #[arg(long = "run-dependencies")]
    pub run_dependencies: bool,
    #[arg(long = "no-run-dependencies")]
    pub no_run_dependencies: bool,
    #[arg(long = "maximum-tasks", default_value = "-1")]
    pub maximum_tasks: i64,
    #[arg(long = "extensions")]
    pub extensions: Option<String>,
    #[arg(long)]
    pub preserve: bool,
    #[arg(long)]
    pub verbose: bool,
    #[arg(long = "timestamp-format", value_parser = ["relative", "local", "utc"],
          default_value = "relative")]
    pub timestamp_format: String,
    #[arg(long = "output", value_parser = ["human-readable", "json", "yaml"],
          default_value = "human-readable")]
    pub output: String,
}
```

## Execution Pipeline

The `execute()` function is async and runs the full session lifecycle:

```
execute(args).await
  │
  ├── 1. SETUP
  │   ├── Enable verbose logging if --verbose
  │   ├── Initialize SESSION_START and TIMESTAMP_FORMAT globals
  │   ├── Read and parse job template
  │   ├── Read and parse environment templates (--environment)
  │   ├── Parse CLI parameters → input_values
  │   ├── Load path mapping rules
  │   ├── preprocess_job_parameters() → param_values
  │   └── create_job() → Job
  │
  ├── 2. SESSION CREATION
  │   ├── Build host-context function library (with path mapping)
  │   ├── Build ValidationContext from job's declared extensions
  │   ├── Create SessionConfig
  │   └── Session::with_config() + with_library()
  │
  ├── 3. ENVIRONMENT ENTRY
  │   ├── Enter environment template environments (--environment files)
  │   └── Enter job environments (from template's jobEnvironments)
  │
  ├── 4. STEP EXECUTION
  │   ├── Resolve step selection (explicit / auto-select / all)
  │   ├── Parse explicit task params if provided
  │   ├── Determine step execution order (with dependency resolution if requested)
  │   └── For each step:
  │       ├── Enter step environments
  │       ├── Prepare embedded file paths
  │       ├── Execute tasks (explicit, lazy iteration, or no-param single task)
  │       └── Exit step environments (reverse order)
  │
  ├── 5. ENVIRONMENT EXIT
  │   └── Exit every entered environment (LIFO), including any whose
  │       enter action failed (cleanup guarantee)
  │
  └── 6. RESULTS
      ├── Print session summary (format depends on --output)
      ├── Cleanup session (unless --preserve)
      └── Exit with code 1 if any action failed
```

## Step Selection Logic

The step selection has three modes:

1. **Explicit `--step`** — Run the named step. Error if the name doesn't match any step.
2. **Auto-select** — If the job has exactly one step and no `--step` is provided, that
   step is selected automatically.
3. **All steps** — If the job has multiple steps and no `--step` is provided, all steps
   run in index order. Explicit task params (`--task-param`, `--tasks`) are not allowed
   in this mode because they're ambiguous across steps.

When `--run-dependencies` is set with an explicit step, `resolve_step_dependencies()`
computes the transitive closure of dependencies and returns them in topological order
(dependencies before dependents) via recursive DFS.

## Task Execution Modes

Within a selected step, tasks execute in one of three modes:

### No Parameter Space

If the step has no `parameterSpace`, a single task runs with no task parameters. Let
bindings and embedded files are evaluated against the step's symbol table only.

### Explicit Task Parameters

When `--task-param` or `--tasks` is provided, the CLI runs exactly the specified tasks.
Task parameter values are injected as `Task.Param.<name>` and `Task.RawParam.<name>` in
the symbol table. The parameter type is set to `String` for `--task-param` values (the
CLI doesn't have type information for explicit params).

### Lazy Parameter Space Iteration

The default mode iterates the step's parameter space lazily via
`StepParameterSpaceIterator`. The iterator is never collected into a `Vec` — tasks are
consumed one at a time, which is critical for large parameter spaces (e.g., 100,000 frames)
that would exhaust memory if materialized.

For each task:
1. Get the next parameter set from the iterator
2. Build a task symbol table with `Task.Param.*` and `Task.RawParam.*` entries
3. Apply path mapping to PATH-typed parameters
4. Evaluate let bindings against the task symbol table
5. Write embedded files with resolved content
6. Call `session.run_task()`
7. Check result; stop on failure

`--maximum-tasks` limits the iteration count. A value of -1 (default) means no limit.

## Adaptive Chunking

When a step uses `CHUNK[INT]` task parameters (from the TASK_CHUNKING extension), the
iterator produces chunks of varying size. The CLI implements adaptive chunking to
dynamically adjust chunk sizes toward a target runtime:

```
For each completed task:
  1. Count items in the chunk (from the RangeExpr value)
  2. Accumulate total items and total duration
  3. Compute duration_per_task = total_duration / total_items
  4. Compute ideal_chunk_size = target_runtime_seconds / duration_per_task
  5. For the first 10 tasks, blend: 75% current + 25% ideal (conservative ramp)
  6. Clamp to minimum of 1
  7. Update iterator's default task count if changed
```

The blending in step 5 prevents wild oscillation when early tasks have atypical durations.
After 10 tasks, the estimate stabilizes and the ideal size is used directly.

`target_runtime_seconds` comes from the step's CHUNK[INT] parameter definition. If not
specified or zero, the model layer's `StepParameterSpaceIterator` sets `chunks_adaptive()`
to `false` (it requires `target_runtime_seconds > 0`), so `is_adaptive` is `false` in the
CLI and the entire adaptive adjustment block is skipped. The iterator uses its
`defaultTaskCount` as-is throughout execution.

## Environment Lifecycle

Environments are entered and exited in a strict order:

```
Enter:  env_templates[0], env_templates[1], ..., job_envs[0], job_envs[1], ...
  For each step:
    Enter:  step_envs[0], step_envs[1], ...
    (run tasks)
    Exit:   step_envs[N], ..., step_envs[1], step_envs[0]
Exit:   job_envs[N], ..., job_envs[0], env_templates[N], ..., env_templates[0]
```

Environment template environments (from `--environment` files) are converted from
`template::Environment` to `job::Environment` via
`openjd_model::convert_environment_with_symtab()`, passing a symbol table built from
the preprocessed parameter values (`openjd_model::build_symbol_table(&param_values)`).
This freezes the environment template's own `Param.*`/`RawParam.*` values into its
`resolved_symtab`, which the session's RFC 0008 wrap-hook dispatch merges into hook
scope — a wrap hook resolved against a step's symbol table could not otherwise see
the environment template's own parameters.

Step environments receive the step's symbol table (`step_symtab`) for format string
resolution. Job and template environments receive `None` for the step symbol table.

**Single-wrap-layer preflight (RFC 0008).** Before entering any environment,
the CLI validates every session stack this run will build — external
environment templates + job environments + each selected step's step
environments — and rejects the run if more than one environment in any stack
defines a wrap hook. The RFC requires rejection "before entering any
Environment", so this happens before the first wrapper's own `onEnter` can
run. The session's enter-time check (`SessionError::MultipleWrapEnvironments`)
remains as defense in depth for library callers.

Every entered environment — template, job, and step scoped — is tracked in an
`entered_envs` list of `(identifier, name, step_symtab)` tuples as it is
entered. A failed enter action marks the session failed but still records the
environment (the session keeps a failed-enter environment on its stack),
prints the failing action's `Process exited with code: N` line, and skips
entering further environments and running tasks. Step environments are
unwound to the pre-step baseline at the end of each step (or after a step-env
enter failure); final cleanup then exits every remaining recorded environment
in LIFO order. This is the OpenJD cleanup guarantee (extended to wrapped
exits by RFC 0008 "Lifecycle and cleanup guarantees"): every environment
entered or attempted has its `onExit` (or substituted `onWrapEnvExit`) run
before the session ends. An environment rejected before entry is not recorded
and not exited.

## Embedded File Handling

For each step, embedded file paths are pre-computed before task iteration:

1. Filenames are plain strings per the 2023-09 schema (not `@fmtstring`) and
   are used literally
2. File paths are constructed relative to the session working directory
3. For each task, file contents are resolved against the task symbol table and written
   via `openjd_sessions::embedded_files::write_embedded_file()`
4. If `end_of_line` is `"CRLF"`, newlines are converted after resolution

Files are re-written for each task because their content may contain task parameter
references that change per-task.

## Let Binding Evaluation

Script-level let bindings are evaluated per-task using `eval_with_lib()`, a helper that
creates a `ParsedExpression`, constructs an `Evaluator` with the host library, and
evaluates the AST. Results are inserted into the task symbol table for use by subsequent
bindings, embedded files, and action argument resolution.

```rust
fn eval_with_lib(
    expr: &str,
    symtab: &SymbolTable,
    lib: &FunctionLibrary,
) -> Result<ExprValue, ExpressionError>
```

This function exists in `run.rs` because the CLI needs to evaluate let bindings with the
host-context library (which includes path mapping functions). The model crate's
`evaluate_let_bindings()` doesn't accept a custom library, so the CLI reimplements the
evaluation loop.

## Host-Context Function Library

The `run` command builds a function library that extends the default library with
host-context functions (path mapping). This library is passed to:

- `eval_with_lib()` for let binding evaluation
- `FormatString::resolve_string()` for embedded file content resolution
- `Session::with_library()` for action argument resolution during subprocess execution

The library is obtained from
`openjd_expr::FunctionLibrary::for_profile(&profile)`, where `profile` is
an `ExprProfile` carrying `HostContext::with_rules(path_mapping_rules)`.

## Session Configuration

The `SessionConfig` struct is populated with:

| Field | Source |
|-------|--------|
| `session_id` | `"cli-{pid}"` where pid is the current process ID |
| `job_parameter_values` | From `preprocess_job_parameters()` |
| `path_mapping_rules` | From `--path-mapping-rules` (None if empty) |
| `retain_working_dir` | From `--preserve` |
| `callback` | `None` (CLI doesn't use action callbacks) |
| `os_env_vars` | `None` (inherit current environment) |
| `session_root_directory` | `None` (use system temp) |
| `user` | `None` (run as current user) |
| `revision_extensions` | `ValidationContext` built from job's declared extensions |

## Result Output

After all steps complete (or on failure), the command prints a summary:

### Human-Readable

```
--- Results of local session ---

Session ended successfully
Working directory preserved at: /tmp/openjd-cli-12345

Job: MyJob
Step: Render
Duration: 42.123 seconds
Tasks run: 100
```

### JSON

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

### YAML

```yaml
status: success
message: Session ended successfully
job_name: MyJob
step_name: Render
duration: 42.123
tasks_run: 100
```

## Error Handling and Exit Codes

- Exit code 0: All actions completed successfully
- Exit code 1: Any action failed, or a setup error occurred

On action failure, the session continues to exit environments (cleanup) but skips
remaining tasks. The `session_failed` flag tracks whether any action returned a
non-`Success` state.

Setup errors (file not found, parse failure, invalid parameters) return immediately
via `Result::Err`, which `main()` prints to stderr before exiting with code 1.

## Differences from Python CLI

| Aspect | Python | Rust |
|--------|--------|------|
| Session wrapper | `LocalSession` context manager | Direct `Session` API calls |
| Signal handling | SIGINT/SIGTERM handlers with `cancel()` | Not implemented (tokio task dropping) |
| Adaptive chunking | In `LocalSession._run_tasks_adaptive_chunking()` | Inline in task iteration loop |
| Step ordering | `StepDependencyGraph` topological sort for all-steps mode | Index order (0..N) for all-steps mode |
| Environment conversion | Implicit (Python sessions accept template types) | Explicit `convert_environment()` call |
| Callback | `LocalSession._action_callback()` handles states | No callback; result checked after each await |
