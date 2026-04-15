# Template Parsing

The `parse` module handles decoding YAML/JSON template documents into typed Rust structs.
Parsing is the entry point to the crate — all other operations work on the parsed types.

## Public API

```rust
pub fn decode_job_template(
    template: serde_yaml::Value,
    supported_extensions: Option<&[&str]>,
) -> Result<JobTemplate, OpenJdError>

pub fn decode_environment_template(
    template: serde_yaml::Value,
    supported_extensions: Option<&[&str]>,
) -> Result<EnvironmentTemplate, OpenJdError>

pub fn decode_template(
    template: serde_yaml::Value,
    supported_extensions: Option<&[&str]>,
) -> Result<DecodedTemplate, OpenJdError>

pub fn document_string_to_object(
    document: &str,
    doc_type: DocumentType,
) -> Result<serde_yaml::Value, OpenJdError>
```

### Types

```rust
pub enum DocumentType {
    Json,
    Yaml,
}

pub enum DecodedTemplate {
    Job(JobTemplate),
    Environment(EnvironmentTemplate),
}
```

## Pipeline Phases

Decoding proceeds through five phases:

### Phase 1: Raw Parsing

`document_string_to_object` parses a raw string into a `serde_yaml::Value` tree. The
`DocumentType` parameter selects JSON or YAML parsing. This phase catches syntax errors
(malformed YAML, invalid JSON).

Callers typically handle this phase themselves — the `decode_*` functions accept a
pre-parsed `serde_yaml::Value`.

### Phase 2: Version Dispatch

The `specificationVersion` field is read from the value tree and mapped to a
`TemplateSpecificationVersion` enum:

| String | Enum Variant |
|--------|-------------|
| `"jobtemplate-2023-09"` | `JobTemplate2023_09` |
| `"environment-2023-09"` | `Environment2023_09` |

Unrecognized versions produce `OpenJdError::UnsupportedSchema`.

`decode_template` auto-detects the template type from this field.

### Phase 3: Serde Deserialization

The value tree is deserialized into the appropriate struct via `serde_yaml::from_value`.
All template types use `#[serde(deny_unknown_fields)]`, so unexpected fields produce errors.

Custom deserializers handle:
- **`ExtensionName`** — Validates regex pattern during deserialization
- **`FormatString`** — Parses `{{...}}` interpolation syntax
- **`JobParameterDefinition`** — Case-insensitive `type` field matching, strips `type` before
  delegating to variant-specific deserialization
- **`TaskParameterDefinition`** — Uses serde's `#[serde(tag = "type")]`
- **`IntRange`/`StringRange`/`FloatRange`** — Distinguishes list vs expression string
- **`FlexInt`/`FlexFloat`** — Accepts multiple YAML value representations
- **`BoolValue`** — Accepts boolean, numeric, and string representations

### Phase 4: Extension Resolution

Each extension the template requests (via its `extensions` field) must be present in the
caller's `supported_extensions` list. If a requested extension is not supported, decoding
fails with `OpenJdError::DecodeValidation`. When `supported_extensions` is `None`, it
defaults to an empty set — no extensions are supported.

The resulting extension set is stored in a `ValidationContext`.

> **Empty extensions list asymmetry:** For environment templates, an empty `extensions: []`
> list is caught during parsing (Phase 4) and produces an immediate `DecodeValidation` error.
> For job templates, the same check is deferred to structural validation (Pass 3 in the
> validation pipeline). Both produce equivalent errors, but the detection point differs
> because the job template validation pipeline handles this as part of its accumulated
> error reporting, while the environment template parser checks it eagerly.

### Phase 5: Validation

The deserialized template is passed through the multi-pass validation pipeline
(see [validation.md](validation.md)). Validation errors are accumulated and returned
as a single `OpenJdError::DecodeValidation` or `OpenJdError::ModelValidation`.

## Design Decisions

### serde_yaml::Value as Input Type

The decode functions accept `serde_yaml::Value` rather than `&str` because:

1. Callers may need to inspect the raw value tree before decoding (e.g., to read
   `specificationVersion` for routing)
2. The same value tree can be used for both JSON and YAML sources
3. It separates syntax parsing (Phase 1) from semantic decoding (Phases 2–5)

### Comparison with Python

The Python library uses `parse_model()` which calls `model_validate()` (Pydantic v2) for
combined deserialization + validation. The Rust crate separates these because serde
deserialization is stateless and can't accumulate multiple validation errors. The multi-pass
pipeline runs after deserialization to provide comprehensive error reporting.
