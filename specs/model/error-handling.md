# Error Handling

The `error` module defines the crate's error types and validation error infrastructure.

## Primary Error Type

```rust
#[non_exhaustive]
pub enum ModelError {
    DecodeValidation(String),
    ModelValidation(String),
    FormatStringError { message: String, input: Option<String>, start: Option<usize>, end: Option<usize> },
    Expression(String),
    Compatibility(String),
    UnsupportedSchema(String),
}
```

| Variant | When Used |
|---------|----------|
| `DecodeValidation` | Structural deserialization failure: bad YAML/JSON, missing required fields, wrong types, unsupported extensions |
| `ModelValidation` | Semantic validation failure: template parsed but violates spec rules (from validation pipeline) |
| `FormatStringError` | Format string interpolation error, with optional source position for diagnostics |
| `Expression` | Expression evaluation error (from `openjd-expr`) |
| `Compatibility` | Parameter merge conflicts between templates |
| `UnsupportedSchema` | Unknown `specificationVersion` value |

Marked `#[non_exhaustive]` to allow adding variants without breaking downstream code.

### From Conversions

- `openjd_expr::SymbolTableError` → `ModelError::Expression`
- `openjd_expr::FormatStringValidationError` → `ModelError::FormatStringError`

## Validation Error Infrastructure

### PathElement

Represents a location in the template tree for structured error paths:

```rust
pub enum PathElement {
    Field(String),    // Named field: "steps", "script", "actions"
    Index(usize),     // Array index: 0, 1, 2
}
```

### ValidationError

A single validation error with its location:

```rust
pub struct ValidationError {
    pub path: Vec<PathElement>,
    pub message: String,
}
```

### ValidationErrors

Accumulator for collecting multiple validation errors:

```rust
#[derive(Default)]
pub struct ValidationErrors {
    pub errors: Vec<ValidationError>,
}

impl ValidationErrors {
    pub fn add(&mut self, path: &[PathElement], msg: impl Into<String>)
    pub fn is_empty(&self) -> bool
    pub fn len(&self) -> usize
    pub fn into_result(self, model_name: &str) -> Result<(), ModelError>
    pub fn format(&self, model_name: &str) -> String
}
```

There is no `new()` constructor; use `ValidationErrors::default()` instead.

`into_result` converts the accumulated errors into `Ok(())` if empty, or
`Err(ModelError::ModelValidation(...))` with all errors formatted.

### Error Path Helpers

```rust
pub fn path_field(base: &[PathElement], field: &str) -> Vec<PathElement>
pub fn path_index(base: &[PathElement], index: usize) -> Vec<PathElement>
```

These build error paths incrementally as validation descends into the template tree.

## Error Formatting

Errors are formatted to match Pydantic's output format for consistency with the Python
implementation:

```
N validation error(s) for ModelName
steps[0] -> script -> actions -> onRun -> command:
	Format string references undefined variable 'Param.Foo'
```

The count line uses singular "error" when N=1 and plural "errors" otherwise, matching
Python Pydantic's behavior.

The path is rendered as `field[index] -> field -> field`, where `PathElement::Index`
attaches to the preceding field (e.g., `steps[0]` rather than `steps -> [0]`). The
message is indented below with a tab character. This format is important because:

1. Users familiar with the Python library see consistent error messages
2. Tooling that parses error output works with both implementations
3. The path format is human-readable and maps directly to the template structure

## Design Decisions

### Error Accumulation (vs Fail-Fast)

The validation pipeline accumulates all errors rather than failing on the first one.
This is a deliberate choice matching the Python library's behavior — users see all
problems at once rather than fixing one error only to discover the next.

The `ValidationErrors` collector makes this pattern ergonomic: each validation check
calls `errors.add(path, message)` and continues. At the end, `errors.into_result()`
converts to the appropriate Result.

### Pydantic-Compatible Paths

Despite not using Pydantic, the error path format matches Pydantic's output. This was
a deliberate choice for cross-implementation consistency. The `PathElement` enum and
formatting logic are simpler than Pydantic's internal representation but produce
identical output for the cases that matter.

### Non-Exhaustive Error Enum

`ModelError` is `#[non_exhaustive]` because new error variants may be needed as the
spec evolves (new extensions, new validation rules). Downstream code must use wildcard
matches, which is the correct default for error handling — most callers just display
the error message rather than matching on specific variants.

### FormatStringError Position Info

The `FormatStringError` variant carries optional `input`, `start`, and `end` fields
for source position information. This enables rich diagnostics that can point to the
exact location in a format string where an error occurred. The fields are optional
because not all format string errors have position information (e.g., type mismatches
discovered during evaluation).
