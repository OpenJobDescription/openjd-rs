# Template Types

The `template` module contains types deserialized directly from YAML/JSON templates. These are
"unresolved" — `FormatString` fields have not been evaluated, parameter values have not been
substituted, and syntax sugar (like `SimpleAction`) has not been expanded.

All types use `#[serde(rename_all = "camelCase", deny_unknown_fields)]` for strict deserialization.
Section references (§) refer to the
[2023-09 Template Schemas](https://github.com/OpenJobDescription/openjd-specifications/wiki/2023-09-Template-Schemas).

## Constrained String Types (§7)

Three string types enforce spec constraints at deserialization time via custom `Deserialize` impls:

| Type | Pattern | Length | Usage |
|------|---------|--------|-------|
| `Identifier` | `[A-Za-z_][A-Za-z0-9_]*` | 1–512 | Parameter names, embedded file names |
| `Description` | Unicode except Cc control chars (allows `\n`, `\r`, `\t`) | 0–2048 | Description fields |
| `ExtensionName` | `[A-Z_0-9]{3,128}` | 3–128 | Extension names in `extensions` list |

These types implement `Deserialize`, `Serialize`, and `Display`. Validation happens during
deserialization — invalid values produce serde errors before the validation pipeline runs.

## Root Templates

### JobTemplate (§1.1)

```rust
pub struct JobTemplate {
    pub specification_version: String,
    pub schema: Option<String>,                              // $schema
    pub extensions: Option<Vec<ExtensionName>>,
    pub name: FormatString,
    pub description: Option<Description>,
    pub parameter_definitions: Option<Vec<JobParameterDefinition>>,
    pub job_environments: Option<Vec<Environment>>,
    pub steps: Vec<StepTemplate>,
}
```

Helper: `parameter_definitions_list()` returns `&[JobParameterDefinition]`, defaulting to
an empty slice when `parameter_definitions` is `None`.

### EnvironmentTemplate (§1.2)

```rust
pub struct EnvironmentTemplate {
    pub specification_version: String,
    pub extensions: Option<Vec<ExtensionName>>,
    pub parameter_definitions: Option<Vec<JobParameterDefinition>>,
    pub environment: Environment,
}
```

## StepTemplate (§3)

> **Note:** Step names are plain `String`, not `Identifier` or `FormatString`.
> They accept any Unicode except Cc control characters — unlike parameter names
> and environment names which are constrained to `[A-Za-z_][A-Za-z0-9_]*` via
> the `Identifier` type. This is per the OpenJD specification §3.1 `<StepName>`.

```rust
pub struct StepTemplate {
    pub name: FormatString,
    pub description: Option<Description>,
    pub let_bindings: Option<Vec<String>>,           // "let" field in YAML
    pub dependencies: Option<Vec<StepDependency>>,
    pub step_environments: Option<Vec<Environment>>,
    pub host_requirements: Option<HostRequirements>,
    pub parameter_space: Option<StepParameterSpaceDefinition>,
    pub script: Option<StepScript>,
    // SimpleAction syntax sugar (FEATURE_BUNDLE_1)
    pub bash: Option<SimpleAction>,
    pub python: Option<SimpleAction>,
    pub cmd: Option<SimpleAction>,
    pub powershell: Option<SimpleAction>,
    pub node: Option<SimpleAction>,
}
```

### SimpleAction (FEATURE_BUNDLE_1)

Syntax sugar that expands into a `StepScript` with an embedded file and `onRun` action.
The `resolve_syntax_sugar()` method performs this expansion. A step must have either `script`
or exactly one simple action field — never both.

```rust
pub struct SimpleAction {
    pub let_bindings: Option<Vec<String>>,
    pub script: String,
    pub args: Option<Vec<FormatString>>,
    pub timeout: Option<FormatString>,
    pub cancelation: Option<CancelationMode>,
}
```

### StepDependency (§3.2)

```rust
pub struct StepDependency {
    pub depends_on: String,
}
```

## Environment (§4)

```rust
pub struct Environment {
    pub name: String,
    pub description: Option<Description>,
    pub script: Option<EnvironmentScript>,
    pub variables: Option<HashMap<String, FormatString>>,
}
```

### EnvironmentScript (§4.1)

```rust
pub struct EnvironmentScript {
    pub let_bindings: Option<Vec<String>>,
    pub actions: EnvironmentActions,
    pub embedded_files: Option<Vec<EmbeddedFile>>,
}
```

### EmbeddedFile (§6)

```rust
pub struct EmbeddedFile {
    pub name: String,
    pub file_type: String,                    // "type" field; must be "TEXT"
    pub filename: Option<FormatString>,
    pub data: Option<FormatString>,
    pub runnable: Option<bool>,
    pub end_of_line: Option<String>,          // FEATURE_BUNDLE_1: "LF", "CRLF", "AUTO"
}
```

## Actions (§5)

```rust
pub struct Action {
    pub command: FormatString,
    pub args: Option<Vec<FormatString>>,
    pub cancelation: Option<CancelationMode>,
    pub timeout: Option<FormatString>,
}

pub struct StepActions {
    pub on_run: Action,
}

pub struct EnvironmentActions {
    pub on_enter: Option<Action>,
    pub on_exit: Option<Action>,
}
```

### CancelationMode

Discriminated union on the `mode` field, implemented as a Rust enum with a custom
`Deserialize` impl:

```rust
pub enum CancelationMode {
    Terminate,
    NotifyThenTerminate {
        notify_period_in_seconds: Option<FormatString>,
    },
}
```

The `Terminate` variant rejects any extra fields. The `NotifyThenTerminate` variant
accepts an optional `notifyPeriodInSeconds` field.

## StepScript (§3.5)

```rust
pub struct StepScript {
    pub let_bindings: Option<Vec<String>>,
    pub actions: StepActions,
    pub embedded_files: Option<Vec<EmbeddedFile>>,
}
```

## Host Requirements (§3.3)

```rust
pub struct HostRequirements {
    pub amounts: Option<Vec<AmountRequirement>>,
    pub attributes: Option<Vec<AttributeRequirement>>,
}

pub struct AmountRequirement {
    pub name: String,
    pub min: Option<FormatString>,
    pub max: Option<FormatString>,
}

pub struct AttributeRequirement {
    pub name: String,
    pub any_of: Option<Vec<FormatString>>,
    pub all_of: Option<Vec<FormatString>>,
}
```

## Task Parameter Space (§3.4)

### StepParameterSpaceDefinition

```rust
pub struct StepParameterSpaceDefinition {
    pub task_parameter_definitions: Vec<TaskParameterDefinition>,
    pub combination: Option<String>,
}
```

### TaskParameterDefinition

Discriminated union via `#[serde(tag = "type")]`. Variant names use SCREAMING_CASE to
match the serde tag values directly, with `#[serde(rename = "CHUNK[INT]")]` on `CHUNK_INT`
since brackets aren't valid in Rust identifiers:

| Variant | Type Field | Range Type | Extra Fields |
|---------|-----------|------------|-------------|
| `INT` | `"INT"` | `IntRange` | — |
| `FLOAT` | `"FLOAT"` | `FloatRange` | — |
| `STRING` | `"STRING"` | `StringRange` | — |
| `PATH` | `"PATH"` | `StringRange` | — |
| `CHUNK_INT` | `"CHUNK[INT]"` | `IntRange` | `chunks: ChunksDefinition` |

### Range Types

Ranges accept either a list of values or a range expression string:

```rust
pub enum IntRange {
    List(Vec<FlexInt>),
    Expression(FormatString),
}

pub enum StringRange {
    List(Vec<FormatString>),
    Expression(FormatString),
}

pub enum FloatRange {
    List(Vec<FloatRangeItem>),
    Expression(FormatString),
}
```

`FloatRange::List` uses `FloatRangeItem` — an enum that accepts either a plain `f64` or
a `FormatString` — to handle YAML float edge cases and format string interpolation in
float ranges:

```rust
pub enum FloatRangeItem {
    Float(f64),
    FormatString(FormatString),
}
```

### ChunksDefinition

```rust
pub struct ChunksDefinition {
    pub default_task_count: IntOrFormatString,
    pub target_runtime_seconds: Option<IntOrFormatString>,
    pub range_constraint: RangeConstraint,  // Required field
}

pub enum IntOrFormatString {
    Int(i64),
    FormatString(FormatString),
}

pub enum RangeConstraint {
    Contiguous,
    Noncontiguous,
}
```

## Flexible Deserialization Types

Several wrapper types handle YAML's flexible value representations:

| Type | Accepts | Rejects | Purpose |
|------|---------|---------|--------|
| `FlexInt(i64)` | Integers, floats with `.0`, strings of integers | Bools, nulls | INT parameter defaults/constraints |
| `FlexFloat(f64, Option<String>)` | Numbers, string representations | Bools, nulls | FLOAT parameter defaults/constraints |
| `FlexUint(u64)` | Non-negative integers, string representations | Negatives, bools | Timeout values |
| `BoolValue(bool)` | `true`/`false`, `0`/`1`, `"yes"`/`"no"`, `"on"`/`"off"` | Other strings | BOOL parameter defaults |
| `NullableVec<T>` | Absent field, list of T | Explicit `null` | INT/FLOAT `allowedValues` |

`FlexFloat` preserves the original string representation when parsed from a string, which
is needed for round-trip fidelity in constraint checking.

`NullableVec` exists because the spec distinguishes between an absent `allowedValues` field
(no constraint) and an explicit `null` (invalid). Serde's `Option<Vec<T>>` would accept both.
