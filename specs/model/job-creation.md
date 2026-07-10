# Job Creation Pipeline

The `create_job` module transforms parsed templates into instantiated jobs. This is the core
workflow: templates + user-provided parameter values → a `job::Job` ready for session execution.

## Public API

### merge_job_parameter_definitions

```rust
pub fn merge_job_parameter_definitions(
    job_template: &JobTemplate,
    environment_templates: &[EnvironmentTemplate],
) -> Result<Vec<MergedParameterDefinition>, ModelError>
```

Merges parameter definitions from environment templates (processed in order) then the job
template (last), per §1.2.1.

Environment templates and job templates share the same parameter namespace. This allows an
environment template to accept a parameter that defines, for example, which software
installation to provide, while the job template defines that parameter's default value for
the software the job needs. The merge rules accommodate this and similar use cases where
multiple templates collaborate on the same parameters.

Returns a list of `MergedParameterDefinition` entries, each containing the merged definition
and its source template.

**Merge rules:**
- All definitions of the same parameter must have the same type
- `allowedValues`: intersection (must be non-empty after intersection)
- `minLength`/`minValue`: takes the maximum (most restrictive)
- `maxLength`/`maxValue`: takes the minimum (most restrictive)
- PATH-specific: `objectType` and `dataFlow` must be identical across definitions
- Default value: last template to define one wins

Conflicts produce `ModelError::Compatibility` with details about which templates conflict.

### preprocess_job_parameters

```rust
pub fn preprocess_job_parameters(
    job_template: &JobTemplate,
    input_values: &JobParameterInputValues,
    environment_templates: &[EnvironmentTemplate],
    path_options: &PathParameterOptions<'_>,
) -> Result<JobParameterValues, ModelError>
```

Validates and coerces user-provided parameter values against merged definitions.

`PathParameterOptions` consolidates path-related options:

```rust
pub struct PathParameterOptions<'a> {
    pub job_template_dir: &'a Path,
    pub current_working_dir: &'a Path,
    pub allow_template_dir_walk_up: bool,
    pub path_format: PathFormat,
    pub allow_uri_path_values: bool,
}
```

**Pipeline:**
1. Merge parameter definitions from all templates
2. Check for extra (undefined) parameters in input
3. Fill defaults for missing parameters
4. Coerce values to target types
5. Validate PATH parameters (relative path resolution, URI handling)
6. Check constraints (allowedValues, min/max, length bounds)
7. Validate merged constraints across multiple definitions
8. Error on still-missing required parameters

**PATH handling:**
- User-provided relative paths joined to `current_working_dir`
- Default relative paths joined to `job_template_dir`
- URI paths (`s3://`, `https://`) preserved as-is when EXPR extension is enabled
- `allow_template_dir_walk_up` controls whether paths can traverse above `job_template_dir`

**Value coercion:**
- `coerce_from_str` — Parses string input (CLI): numeric parsing, boolean aliases
  (`yes`/`no`/`on`/`off`/`1`/`0`), JSON list parsing for list types
- `coerce_to_type` — Validates typed input (library): type compatibility, numeric
  widening (int → float), list element type validation

### build_symbol_table

```rust
pub fn build_symbol_table(
    params: &JobParameterValues,
) -> Result<SymbolTable, ModelError>
```

Builds a `SymbolTable` with `Param.*` and `RawParam.*` entries from processed parameter
values. Returns `Result` because symbol table insertion can fail.

PATH types are stored as `Unresolved(PATH)` since the source path format may differ
from the host path format — the value must be preserved exactly as a string until path
mapping is applied at session time. `RawParam.*` for PATH types is forced to STRING.

### create_job

```rust
pub fn create_job(
    job_template: &JobTemplate,
    job_parameter_values: &JobParameterValues,
) -> Result<job::Job, ModelError>
```

Full template instantiation pipeline. Takes 2 arguments — environment templates should
already be merged into `job_parameter_values` via `preprocess_job_parameters` before
calling this function.

1. Build symbol table from parameter values
2. Resolve template-scope fields:
   - Job name (evaluate FormatString)
   - Step names
   - Host requirement values (amounts min/max, attribute values)
   - Parameter space ranges (evaluate range expressions, resolve FormatString ranges)
   - Step-level let bindings
3. Carry forward session/task-scope fields as FormatString (plus action
   `timeout`/`notifyPeriodInSeconds`, which validate in template scope but
   resolve on the worker)
4. With EXPR extension: inject `Job.Name` and `Step.Name` into symbol table
5. Convert environments from template to job types
6. Build step dependency list
7. Attach resolved symbol table to each step

### convert_environment

```rust
pub fn convert_environment(env: &template::Environment) -> job::Environment
```

Converts a template environment to a resolved job environment. Takes 1 argument and is
infallible. Environment variables and script fields remain as FormatString (session-scope).

A separate `convert_environment_with_symtab` function accepts an optional `&SymbolTable`
to filter the symbol table to only symbols referenced by the environment's format strings.

### evaluate_let_bindings

```rust
pub fn evaluate_let_bindings(
    bindings: &[String],
    symtab: &SymbolTable,
    library: Option<&FunctionLibrary>,
    path_format: PathFormat,
) -> Result<SymbolTable, ModelError>
```

Evaluates `"name = expression"` bindings sequentially. Each binding sees the results of
all prior bindings in the same block. Returns a new symbol table with the binding results
added.

The `library` parameter is optional (pass `None` for template-scope bindings that don't
need host functions). `path_format` controls path construction behavior.

## Design Decisions

### Explicit Instantiation (vs Generic Traversal)

The Python library uses `instantiate_model()` which generically traverses Pydantic models
via metadata to find and resolve FormatString fields. The Rust crate uses explicit conversion
methods on each type instead. This is more verbose but:

- Makes template-scope vs session-scope distinction explicit and compiler-verified
- Avoids runtime reflection or trait object overhead
- Makes it clear which fields are resolved at which phase
- Allows different resolution strategies per field (e.g., host requirements resolve
  FormatStrings to f64, while step names resolve to String)

### Merged Constraint Validation

When multiple templates define the same parameter, constraints are merged (intersection for
allowedValues, most restrictive for ranges). The merged constraints are validated for
consistency — e.g., if the intersection of allowedValues is empty, or if the merged
min > merged max, that's an error. This catches conflicts that individual template
validation wouldn't find.

### Path Normalization Without Filesystem Access

`normalize_path` performs pure path normalization (resolving `.` and `..` components)
without filesystem access. This is important because:
- Templates may reference paths that don't exist yet
- The crate should be usable in environments without filesystem access
- Path mapping (for cross-platform execution) happens at session time, not job creation time
