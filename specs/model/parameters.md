# Parameter Type System

The crate has two distinct parameter type systems: job parameters (user-provided inputs to a
template) and task parameters (values that vary per task within a step's parameter space).

## Job Parameter Types

### JobParameterType Enum

Maps to the `type` field in `parameterDefinitions` (§2). Variants use PascalCase (Rust
convention) with `as_spec_str()` returning the wire-format name:

| Variant | Spec Name | Extension | ExprType |
|---------|-----------|-----------|----------|
| `String` | `STRING` | Base | `string` |
| `Int` | `INT` | Base | `int` |
| `Float` | `FLOAT` | Base | `float` |
| `Path` | `PATH` | Base | `path` |
| `Bool` | `BOOL` | EXPR | `bool` |
| `RangeExpr` | `RANGE_EXPR` | EXPR | `range_expr` |
| `ListString` | `LIST[STRING]` | EXPR | `list[string]` |
| `ListInt` | `LIST[INT]` | EXPR | `list[int]` |
| `ListFloat` | `LIST[FLOAT]` | EXPR | `list[float]` |
| `ListPath` | `LIST[PATH]` | EXPR | `list[path]` |
| `ListBool` | `LIST[BOOL]` | EXPR | `list[bool]` |
| `ListListInt` | `LIST[LIST[INT]]` | EXPR | `list[list[int]]` |

Methods:
- `from_spec_str(s) -> Option<Self>` — Case-insensitive parse from spec string
- `as_spec_str() -> &str` — Canonical spec string
- `expr_type() -> ExprType` — Corresponding expression type for symbol table

### JobParameterDefinition

Discriminated union deserialized from the `type` field with a custom `Deserialize` impl
that performs case-insensitive type matching and strips the `type` field before delegating
to variant-specific deserialization. Variant names use SCREAMING_SNAKE_CASE to match the
serde tag values (e.g., `STRING`, `INT`, `LIST_STRING`).

Base type variants (§2.1–2.4):

| Variant | Key Constraints |
|---------|----------------|
| `STRING` | `allowedValues`, `minLength`, `maxLength`, `userInterface` (LINE_EDIT, MULTILINE_EDIT, DROPDOWN_LIST, CHECK_BOX, HIDDEN) |
| `INT` | `allowedValues` (NullableVec<FlexInt>), `minValue`, `maxValue`, `userInterface` (SPIN_BOX, DROPDOWN_LIST, HIDDEN) |
| `FLOAT` | `allowedValues` (NullableVec<FlexFloat>), `minValue`, `maxValue`, `userInterface` (SPIN_BOX with decimals, DROPDOWN_LIST, HIDDEN) |
| `PATH` | `allowedValues`, `minLength`, `maxLength`, `objectType` (FILE/DIRECTORY), `dataFlow` (NONE/IN/OUT/INOUT), `userInterface` (CHOOSE_INPUT_FILE, CHOOSE_OUTPUT_FILE, CHOOSE_DIRECTORY, DROPDOWN_LIST, HIDDEN) with `fileFilters` |

EXPR extension variants (§2.9–2.16):

| Variant | Key Constraints |
|---------|----------------|
| `BOOL` | `default` (BoolValue) |
| `RANGE_EXPR` | `default`, `minLength`, `maxLength` |
| `LIST_STRING` | `default`, `minLength`, `maxLength`, `item` constraints |
| `LIST_PATH` | `default`, `objectType`, `dataFlow`, `minLength`, `maxLength`, `item` constraints |
| `LIST_INT` | `default`, `minLength`, `maxLength`, `item` constraints (allowedValues, min/maxValue) |
| `LIST_FLOAT` | `default`, `minLength`, `maxLength`, `item` constraints |
| `LIST_BOOL` | `default`, `minLength`, `maxLength` |
| `LIST_LIST_INT` | `default`, `minLength`, `maxLength`, nested `item` constraints |

Common methods on all variants:
- `name() -> &str`
- `job_param_type() -> JobParameterType`
- `default_value() -> Option<String>` — Returns the default as a string representation
- `check_constraints(&ExprValue) -> Result<(), String>` — Runtime value validation
- `validate_definition(&EffectiveLimits) -> Result<(), Vec<String>>` — Template-level consistency (accumulates errors)

### Constraint Checking vs Definition Validation

> **Length measurement:** All string length checks (minLength, maxLength, allowedValues
> length validation) use **Unicode scalar value count** (Rust's `.chars().count()`), not
> byte length. For example, the string "aéb" has length 3 (three characters), not 4
> (its UTF-8 byte length). This applies to both `check_constraints` (runtime) and
> `validate_definition` (template-time) checks for STRING and PATH parameters.

These are two distinct operations:

- **`check_constraints`** runs at `preprocess_job_parameters` time against user-provided values.
  It checks: value within min/max range, value in allowedValues, string length within bounds.

- **`validate_definition`** runs during template validation (Pass 3). It checks: min ≤ max,
  minLength ≤ maxLength, default satisfies own constraints, allowedValues entries satisfy
  constraints, UI control is compatible with other fields.

## Task Parameter Types

### TaskParameterType Enum

Variants use PascalCase (Rust convention):

| Variant | Spec Name | Extension |
|---------|-----------|----------|
| `Int` | `INT` | Base |
| `Float` | `FLOAT` | Base |
| `String` | `STRING` | Base |
| `Path` | `PATH` | Base |
| `ChunkInt` | `CHUNK[INT]` | TASK_CHUNKING |

## Parameter Values

### ExprValue as Universal Runtime Type

Rather than using strings as the universal value type, the crate
uses `ExprValue` from `openjd-expr`. This avoids string round-tripping and preserves type
information throughout the pipeline.

```rust
pub struct JobParameterValue {
    pub param_type: JobParameterType,
    pub value: ExprValue,
}

pub struct TaskParameterValue {
    pub param_type: TaskParameterType,
    pub value: ExprValue,
}
```

### Type Aliases

```rust
pub type JobParameterInputValues = HashMap<String, ExprValue>;   // Raw user input
pub type JobParameterValues = HashMap<String, JobParameterValue>; // Processed/typed
pub type TaskParameterSet = HashMap<String, TaskParameterValue>;  // Single task's params
pub type Extensions = HashSet<KnownExtension>;                    // Enabled extensions
```

## PATH Parameter Handling

PATH parameters have special handling throughout the pipeline:

1. **Raw form and RawParam**: PATH values are stored as `ExprValue::String` in their raw
   form and in `RawParam.X`, preserving the original path string as provided.

2. **Session/task scope**: In the session and task template contexts, `Param.X` for PATH
   types becomes `ExprValue::Path` (which carries the host path format) after the session
   applies path mapping rules. The `apply_path_mapping` expression function also produces
   `ExprValue::Path`.

3. **Relative path resolution**: User-provided relative paths are joined to `current_working_dir`;
   default relative paths are joined to `job_template_dir`. URI paths (`s3://`, `https://`)
   are preserved as-is when the EXPR extension is enabled.

4. **LIST[PATH]**: `RawParam.X` for `LIST[PATH]` is `list(STRING)`, not `list(PATH)`.

## Value Coercion

`preprocess_job_parameters` coerces input values to target types. The coercion rules handle
two input modes:

- **CLI input** (string values): Parsed via `coerce_from_str` — handles numeric parsing,
  boolean aliases (`yes`/`no`/`on`/`off`/`1`/`0`), JSON list parsing for list types.

- **Library input** (typed ExprValue): Validated via `coerce_to_type` — checks type
  compatibility, performs numeric widening (int → float), validates list element types.

## Validation Context and Extension Effects

### ValidationContext

```rust
pub struct ValidationContext {
    pub revision: SpecificationRevision,
    pub extensions: Extensions,
}
```

Constructed during template decoding from the intersection of template-requested extensions
and caller-supported extensions.

### EffectiveLimits

Numeric limits derived from context. FEATURE_BUNDLE_1 raises many limits:

| Limit | Base | With FEATURE_BUNDLE_1 |
|-------|------|----------------------|
| `max_identifier_len` | 64 | 512 |
| `max_job_name_len` | 128 | 512 |
| `max_step_name_len` | 64 | 512 |
| `max_env_name_len` | 64 | 512 |
| `max_param_count` | 50 | 200 |
| `max_filename_len` | 64 | 256 |
| `max_task_param_range_len` | 1024 | 1024 |
| `max_task_param_string_len` | 1024 | 1024 |
| `max_job_param_string_len` | 1024 | 1024 |
| `max_command_len` | 1024 | 1024 |
| `max_description_len` | 2048 | 2048 |

### EffectiveRules

Structural rules derived from context:

```rust
pub struct EffectiveRules {
    pub allowed_job_param_types: HashSet<JobParameterType>,
    pub allowed_task_param_types: HashSet<TaskParameterType>,
    pub allow_fmtstring_in_numeric_fields: bool,
}
```

The EXPR extension adds `Bool`, `RangeExpr`, and `List*` types to `allowed_job_param_types`.
The TASK_CHUNKING extension adds `ChunkInt` to `allowed_task_param_types`.
FEATURE_BUNDLE_1 sets `allow_fmtstring_in_numeric_fields` to `true`.
