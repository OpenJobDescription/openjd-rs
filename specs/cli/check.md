# check Command

## Purpose

`openjd check <path>` validates a job template or environment template file against the
OpenJD 2023-09 specification. It reports whether the template passes all validation checks
or prints the first error encountered.

This is the simplest command — it parses, detects the template type, validates, and prints
a success or error message. No job instantiation or parameter processing occurs.

## Interface

```
openjd check <PATH> [--extensions <EXTENSIONS>]
```

### Arguments

| Argument | Type | Required | Description |
|----------|------|----------|-------------|
| `path` | `PathBuf` | Yes | Path to the template file (YAML or JSON) |
| `--extensions` | `Option<String>` | No | Comma-separated extension names. Defaults to all supported. Empty string disables all. |

### CheckArgs Struct

```rust
#[derive(Args)]
pub struct CheckArgs {
    pub path: PathBuf,
    #[arg(long = "extensions")]
    pub extensions: Option<String>,
}
```

The struct uses clap's `Args` derive. `path` is a positional argument. `--extensions` is
optional with no default value at the clap level — the default is applied in `execute()`.

## Execution Pipeline

```
execute(args)
  │
  ├── Read file content via common::read_input_file()
  │   (single read that maps errors to:
  │    "does not exist" / "is not a file" / "Cannot read ...")
  │
  ├── Detect document type from file extension
  │   └── .json → DocumentType::Json, anything else → DocumentType::Yaml
  │
  ├── Parse to serde_json::Value via parse::document_string_to_object()
  │
  ├── Extract specificationVersion string from the parsed value
  │
  ├── Resolve extensions list
  │   ├── --extensions "" → empty vec (no extensions)
  │   ├── --extensions "A,B" → vec!["A", "B"]
  │   └── (not provided) → vec!["TASK_CHUNKING", "REDACTED_ENV_VARS", "FEATURE_BUNDLE_1", "EXPR"]
  │
  ├── Parse specificationVersion into TemplateSpecificationVersion
  │
  ├── Dispatch on template type
  │   ├── Job template → parse::decode_job_template(value, extensions)
  │   └── Environment template → parse::decode_environment_template(value, extensions)
  │
  └── Print "Template at '<path>' passes validation checks."
```

## Template Type Detection

The command determines whether the file is a job template or environment template by
parsing the `specificationVersion` field and calling `is_job_template()` or
`is_environment_template()` on the parsed version enum. This mirrors the Python CLI's
approach of inspecting the version string prefix (`jobtemplate-` vs `environment-`).

If the version string doesn't match any known pattern, the command reports an error with
the unrecognized version string.

## Error Handling

The function returns `Result<(), Box<dyn std::error::Error>>`. Errors propagate to `main()`
which prints them to stderr and exits with code 1. Error sources:

- File not found or not a regular file (checked explicitly before reading)
- File read failure (I/O error from `read_to_string`)
- Parse failure (malformed YAML/JSON)
- Missing `specificationVersion` field
- Unknown specification version
- Validation errors from `decode_job_template` or `decode_environment_template`

Validation errors from the model crate include structured paths (e.g.,
`steps[0] -> script -> actions -> onRun -> command`) that help users locate the problem
in their template file.

## Differences from Python CLI

| Aspect | Python | Rust |
|--------|--------|------|
| Output formatting | `@print_cli_result` decorator, supports `--output` (human/json/yaml) | Plain `println!`, no `--output` flag |
| Return type | `OpenJDCliResult` dataclass with status/message | `Result<(), Box<dyn Error>>` |
| Common args | Uses `CommonArgument.PATH` for shared path argument | Defines `path` directly in `CheckArgs` |

The Rust `check` command does not support `--output` for structured output. The Python CLI
wraps the result in an `OpenJDCliResult` dataclass and formats it according to `--output`,
but for a simple pass/fail validation command, the structured output adds little value.
Adding `--output` support would be straightforward if needed for tooling integration.
