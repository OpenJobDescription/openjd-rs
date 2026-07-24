# Parameter Parsing

## Purpose

The CLI accepts parameters in several formats across multiple arguments. This document
covers all parameter parsing functions in the CLI, their input formats, error handling,
and the rationale for supporting each format.

## Job Parameters (`-p` / `--parameter`)

`parse_cli_parameters()` in `run/params.rs` handles the `-p`/`--parameter` argument, which is
used by both the `summary` and `run` commands. It accepts three input formats and returns
a `HashMap<String, ExprValue>`.

### Format 1: Key=Value Pairs

```
openjd run template.yaml -p Frames=1-100 -p OutputDir=/output
```

The string is split on the first `=`. The key becomes the parameter name, the value
becomes an `ExprValue::String`. No type coercion is performed — the model crate's
`preprocess_job_parameters()` handles type validation and coercion later.

### Format 2: File Reference

```
openjd run template.yaml -p file://params.json
openjd run template.yaml -p file://params.yaml
```

The file is read and parsed as JSON (if `.json` extension) or YAML (otherwise). The file
must contain a dictionary mapping parameter names to values. Each value is converted to
`ExprValue` via `json_val_to_expr()`:

- JSON strings → `ExprValue::String`
- All other JSON types → `ExprValue::String(value.to_string())` (stringified)

### Format 3: Inline JSON

```
openjd run template.yaml -p '{"Frames": "1-100", "OutputDir": "/output"}'
```

Detected by checking if the string starts with `{` and ends with `}`. Parsed as JSON and
processed identically to file-based JSON.

### Error Messages

Error messages match the Python CLI's format for consistency:

- Missing file: `"Provided parameter file '<path>' does not exist."`
- Bad format: `"Job parameter string ('<arg>') not formatted correctly. It must be key=value pairs, inline JSON, or a path to a JSON or YAML document prefixed with 'file://'."`
- Non-dict file: `"Job parameter file '<arg>' should contain a dictionary."`
- Non-dict JSON: `"Job parameter ('<arg>') must contain a dictionary mapping job parameters to their value."`
- Oversized file: `"File '<path>' exceeds maximum size of <N> bytes."`

### File Size Limit

All `file://` inputs read by the `run` subcommand — job parameter files, task files
(`--tasks`), and path-mapping rule files (`--path-mapping-rules`) — are capped at
`common::MAX_FILE_INPUT_SIZE` (10 MiB). The cap is enforced by reading through
`Read::take(cap + 1)`, so allocation is bounded to `cap + 1` bytes regardless of
the underlying file size. The template itself has a separate limit enforced by
the model crate's `CallerLimits::max_template_size`.

### Value Coercion

`json_val_to_expr()` converts all JSON values to `ExprValue::String`:

```rust
fn json_val_to_expr(v: &serde_json::Value) -> ExprValue {
    match v {
        serde_json::Value::String(s) => ExprValue::String(s.clone()),
        other => ExprValue::String(other.to_string()),
    }
}
```

This means numeric JSON values like `{"Frames": 100}` become the string `"100"`. The
model crate's parameter preprocessing handles parsing this string into the correct type
based on the parameter definition. This matches the Python CLI's behavior where all
parameter values pass through as strings initially.

## Task Parameters (`-t` / `--task-param`)

`parse_task_params()` handles the `-t`/`--task-param` argument for the `run` command. It
accepts only `NAME=VALUE` format and returns a `HashMap<String, String>`.

```
openjd run template.yaml -t Frame=42 -t Quality=high
```

### Validation Rules

- Each argument must contain exactly one `=` with non-empty name and value
- Duplicate parameter names are errors
- All errors are collected and reported together (not fail-fast)

### Error Accumulation

Unlike `parse_cli_parameters()` which fails on the first error, `parse_task_params()`
collects all errors and reports them in a single message:

```
Found the following errors collecting Task parameters:
 - Task parameter 'bad' defined incorrectly. Expected '<NAME>=<VALUE>' format.
 - Task parameter 'Frame' has been defined more than once.
```

This matches the Python CLI's error accumulation pattern for task parameters.

## Task Sets (`--tasks`)

`parse_tasks_arg()` handles the `--tasks` argument, which specifies multiple complete
task parameter sets. It returns a `Vec<HashMap<String, String>>`.

### Format: Inline JSON Array

```
openjd run template.yaml --tasks '[{"Frame": "1"}, {"Frame": "2"}, {"Frame": "3"}]'
```

### Format: File Reference

```
openjd run template.yaml --tasks file://tasks.json
openjd run template.yaml --tasks file://tasks.yaml
```

The file or inline string must parse to a JSON array of objects. Each object maps parameter
names to string or numeric values. Numeric values are stringified.

### Validation

- Must be a JSON array (not an object or scalar)
- Each element must be an object (not an array or scalar)
- Each object value must be a string or number (not an object, array, bool, or null)

Error messages are terse: `"--tasks argument must be a list of maps from string to string when decoded."`

## Path Mapping Rules (`--path-mapping-rules`)

`load_path_mapping_rules()` handles the `--path-mapping-rules` argument. It returns a
`Vec<PathMappingRule>`.

### Format: Inline JSON

```
openjd run template.yaml --path-mapping-rules '{"version": "pathmapping-1.0", "path_mapping_rules": [...]}'
```

### Format: File Reference

```
openjd run template.yaml --path-mapping-rules file://rules.json
```

### Validation

The JSON document must:
1. Be an object (not an array or scalar)
2. Have a `version` field with value `"pathmapping-1.0"`
3. Have a `path_mapping_rules` field that is an array
4. Each rule must deserialize into `PathMappingRule` via serde

### Why JSON Only

Path mapping rules are JSON-only (no YAML support) because the `pathmapping-1.0` schema
is defined as a JSON format in the OpenJD specification. The Python CLI also only accepts
JSON for path mapping rules.

## Extension Parsing

Extension parsing is duplicated across all three commands that accept `--extensions`. The
logic is identical:

```rust
let default_exts = vec!["TASK_CHUNKING", "REDACTED_ENV_VARS", "FEATURE_BUNDLE_1", "EXPR"];
let supported: Vec<&str> = match &args.extensions {
    Some(ext_str) if ext_str.is_empty() => vec![],
    Some(ext_str) => ext_str.split(',').map(|s| s.trim()).collect(),
    None => default_exts,
};
```

Three cases:
- `--extensions ""` → No extensions (strict base spec)
- `--extensions "A,B"` → Only the listed extensions
- (not provided) → All supported extensions

The CLI validates extension names against `ModelExtension::ALL` before template decoding.

## Cross-Module Dependency

`parse_cli_parameters()` lives in `run/params.rs`, is re-exported by the public `run`
module, and is called by `summary.rs` via `crate::run::parse_cli_parameters()`.

This cross-module dependency exists because both `summary` and `run` need to parse `-p`
arguments identically. The function could be extracted to a shared module, but with only
two callers the current arrangement is simpler.
