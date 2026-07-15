# Instantiated Job Types

The `job` module contains types produced by `create_job()`. These represent a fully instantiated
job where template-scope format strings have been resolved to concrete values. Session-scope and
task-scope fields remain as `FormatString` for evaluation at runtime.

All types derive `Debug, Clone, Serialize` with `#[serde(rename_all = "camelCase")]`.

## Resolution Scopes

The OpenJD specification defines three resolution scopes that determine when format strings
are evaluated:

| Scope | When Resolved | Examples |
|-------|--------------|----------|
| TEMPLATE | At `create_job()` time | Job name, step names, host requirement values, parameter space ranges, step-level let bindings |
| SESSION | At session start | Environment variables, environment script commands, embedded file data |
| TASK | At task execution | Step script commands/args, step embedded file data |

Template-scope fields become concrete `String`/`f64`/`i64` values in the `job::*` types.
Session and task-scope fields remain as `FormatString` because they depend on runtime context
(e.g., `Session.WorkingDirectory`, `Task.Param.*`).

One exception separates symbol scope from resolution time: action `timeout` and
`notifyPeriodInSeconds` validate in TEMPLATE scope (only job-creation-stage symbols may be
referenced — no `Session.*`/`Task.*`/`Env.File.*`), yet they remain `FormatString` in the
`job::*` types and resolve on the worker alongside the host-context fields.

## Type Definitions

### Job

```rust
pub struct Job {
    pub name: String,                                    // Resolved from FormatString
    pub description: Option<String>,
    pub extensions: Option<Vec<String>>,
    pub parameters: IndexMap<String, JobParameter>,      // Insertion-ordered
    pub steps: Vec<Step>,
    pub job_environments: Option<Vec<Environment>>,
}
```

`parameters` uses `IndexMap` (not `HashMap`) to preserve insertion order for deterministic
output.

### JobParameter

```rust
pub struct JobParameter {
    pub name: String,
    pub param_type: JobParameterType,
    pub value: ExprValue,
}
```

Stores the final typed value as an `ExprValue` (from `openjd-expr`), preserving the full
type information. PATH values are stored as `ExprValue::String` because the source path
format may differ from the host path format. During the TEMPLATE scope, `Param.X` is not
accessible for PATH types — only `RawParam.X` is available (as a string). Later, in
SESSION and TASK scope, the session applies path mapping rules and `Param.X` becomes
available as `ExprValue::Path`.

### Step

```rust
pub struct Step {
    pub name: String,                                    // Resolved
    pub description: Option<String>,
    pub script: StepScript,
    pub step_environments: Option<Vec<Environment>>,
    pub parameter_space: Option<StepParameterSpace>,
    pub host_requirements: Option<HostRequirements>,
    pub dependencies: Option<Vec<StepDependency>>,
    pub resolved_symtab: Option<SerializedSymbolTable>,
}
```

`resolved_symtab` exists to transport symbol values across the network to the worker host
that runs the job. The worker evaluates the format strings that remain unresolved after job
creation, so `resolved_symtab` is filtered to contain exactly the symbols referenced by
those format strings. For a Step, these are: the step script's actions (command, args,
timeout, cancelation), embedded files, script-level let bindings, step-level let bindings,
and any step-scoped environments' variables, actions, and embedded files. Most of these are
host-context (SESSION and TASK scope); timeout and notifyPeriodInSeconds are template scope
(validation rejects `Session.*`/`Task.*`/`Env.File.*` in them) but still resolve on the
worker, referencing only the job-creation-stage symbols carried in `resolved_symtab`.

Contents include `RawParam.*`, non-PATH `Param.*` values, `Job.Name`, `Step.Name`, and
let bindings. PATH-typed `Param.*` entries and any `apply_path_mapping` results are excluded
because path mapping rules aren't available until session time. The session layers these
plus `Session.*` and `Task.*` values on top at runtime.

The type is `SerializedSymbolTable` (not `SymbolTable`) — a wire-format type serialized as
`[{"name": str, "value": ..., "type": str}]` for cross-host transfer in a Python-compatible
format.

### StepScript, StepActions, Action

```rust
pub struct StepScript {
    pub let_bindings: Option<Vec<String>>,
    pub actions: StepActions,
    pub embedded_files: Option<Vec<EmbeddedFile>>,
}

pub struct StepActions {
    pub on_run: Action,
}

pub struct Action {
    pub command: FormatString,                           // Task-scope, unresolved
    pub args: Option<Vec<FormatString>>,                 // Task-scope, unresolved
    pub timeout: Option<FormatString>,                   // Template-scope symbols only, unresolved
    pub cancelation: Option<CancelationMode>,
}
```

`command` and `args` remain as `FormatString` because they may reference `Task.Param.*`
variables that are only available at task execution time. `timeout` (and
`notifyPeriodInSeconds` inside `cancelation`) also travel unresolved and are resolved by
the session runtime, but validation restricts them to template-scope symbols (they are
plain `@fmtstring` in the spec — job-creation stage), so no `Session.*`, `Task.*`, or
`Env.File.*` references can appear in them.

### Environment, EnvironmentScript, EnvironmentActions

```rust
pub struct Environment {
    pub name: String,
    pub description: Option<String>,
    pub script: Option<EnvironmentScript>,
    pub variables: Option<HashMap<String, FormatString>>,  // Session-scope
    pub resolved_symtab: Option<SerializedSymbolTable>,
}

pub struct EnvironmentScript {
    pub let_bindings: Option<Vec<String>>,
    pub actions: EnvironmentActions,
    pub embedded_files: Option<Vec<EmbeddedFile>>,
}

pub struct EnvironmentActions {
    pub on_enter: Option<Action>,
    pub on_exit: Option<Action>,
}
```

`resolved_symtab` on `Environment` serves the same purpose as on `Step`: transporting
symbol values to the worker host. It is filtered to contain only the symbols referenced
by this environment's worker-resolved format strings — its variables, script actions
(including their timeouts), embedded files, and script-level let bindings. The same `RawParam` fallback applies:
if a format string references `Param.X` for a PATH-typed parameter, `RawParam.X` is
included instead.

### EmbeddedFile, CancelationMode

```rust
pub struct EmbeddedFile {
    pub name: String,
    pub file_type: FileType,                             // Typed enum, not String
    pub filename: Option<String>,                        // Plain string (not @fmtstring)
    pub data: Option<FormatString>,                      // Session/task-scope
    pub runnable: Option<bool>,
    pub end_of_line: Option<EndOfLine>,                  // Typed enum, not String
}
```

`file_type` is `FileType` (an enum) and `end_of_line` is `Option<EndOfLine>` (an enum),
not raw strings.

```rust
pub enum CancelationMode {
    Terminate,
    NotifyThenTerminate {
        notify_period_in_seconds: Option<FormatString>,
    },
}
```

`CancelationMode` is an enum (same pattern as the template side), serialized with
`#[serde(tag = "mode", rename_all = "SCREAMING_SNAKE_CASE")]`.

### StepParameterSpace

```rust
pub struct StepParameterSpace {
    pub task_parameter_definitions: IndexMap<String, TaskParameter>,  // Insertion-ordered
    pub combination: Option<String>,
}
```

`task_parameter_definitions` uses `IndexMap` (not `HashMap`) to preserve definition order,
which matters for the default product combination.

### TaskParameter

Fully resolved task parameter with concrete range values:

```rust
pub enum TaskParameter {
    Int { range: TaskParamRange<i64>, chunks: Option<ResolvedChunks> },
    Float { range: Vec<f64> },
    String { range: Vec<String> },
    Path { range: Vec<String> },
    ChunkInt { range: TaskParamRange<i64>, chunks: ResolvedChunks },
}
```

`Int` and `ChunkInt` ranges may be either a materialized list or a `RangeExpr` (from
`openjd-expr`) for compact representation of large integer sequences. `Float`, `String`,
and `Path` ranges are always materialized lists because they don't have a compact
representation.

### TaskParamRange, ResolvedChunks

```rust
pub enum TaskParamRange<T> {
    List(Vec<T>),
    RangeExpr(RangeExpr),
}

pub struct ResolvedChunks {
    pub default_task_count: usize,
    pub target_runtime_seconds: Option<usize>,
    pub range_constraint: RangeConstraint,
}
```

### Host Requirements (Resolved)

```rust
pub struct HostRequirements {
    pub amounts: Option<Vec<AmountRequirement>>,
    pub attributes: Option<Vec<AttributeRequirement>>,
}

pub struct AmountRequirement {
    pub name: String,
    pub min: Option<f64>,                                // Resolved from FormatString
    pub max: Option<f64>,                                // Resolved from FormatString
}

pub struct AttributeRequirement {
    pub name: String,
    pub any_of: Option<Vec<String>>,                     // Resolved from FormatString
    pub all_of: Option<Vec<String>>,                     // Resolved from FormatString
}
```

### StepDependency

```rust
pub struct StepDependency {
    pub depends_on: String,
}
```

## Template → Job Type Mapping

| Template Type | Job Type | Key Differences |
|--------------|----------|----------------|
| `template::JobTemplate` | `job::Job` | `name` is `String` not `FormatString`; parameters carry resolved values; `parameters` is `IndexMap` |
| `template::StepTemplate` | `job::Step` | `name` resolved; `host_requirements` values resolved; carries `resolved_symtab: Option<SerializedSymbolTable>` |
| `template::StepScript` | `job::StepScript` | Structurally identical; action fields remain `FormatString` |
| `template::Environment` | `job::Environment` | `variables` values remain `FormatString` (session-scope); adds `resolved_symtab` |
| `template::HostRequirements` | `job::HostRequirements` | `min`/`max` are `f64`; `any_of`/`all_of` are `Vec<String>` |
| `template::EmbeddedFile` | `job::EmbeddedFile` | `file_type` is `FileType` enum; `end_of_line` is `Option<EndOfLine>` enum |
| `template::CancelationMode` | `job::CancelationMode` | Both are enums with `Terminate` and `NotifyThenTerminate` variants |
| `template::StepParameterSpaceDefinition` | `job::StepParameterSpace` | Ranges resolved to concrete values; definitions keyed by name in `IndexMap` |
| `template::TaskParameterDefinition` | `job::TaskParameter` | Enum with resolved ranges and optional chunks |
