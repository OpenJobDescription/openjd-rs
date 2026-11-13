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

The whole argument is trimmed, then split on the first `=`. The key becomes the parameter
name, the value becomes an `ExprValue::String`. No type coercion is performed — the model
crate's `preprocess_job_parameters()` handles type validation and coercion later. Because of
the trim, `-p "N= 7x "` reports the value as `' 7x'`; a value in inline JSON or a parameter
file keeps white space at both ends.

### Format 2: File Reference

```
openjd run template.yaml -p file://params.json
openjd run template.yaml -p file://params.yaml
```

The file is read and parsed as JSON (if `.json` extension) or YAML (otherwise). The file
must contain a dictionary mapping parameter names to values. Each value is converted to
`ExprValue` via `json_val_to_expr()`:

- JSON strings → `ExprValue::String`
- JSON numbers → see [Value Coercion](#value-coercion)
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

`json_val_to_expr()` converts a JSON or YAML value to an `ExprValue`. The model crate's
parameter preprocessing then coerces it to the parameter's declared type. The caller passes
the names of the FLOAT parameters the job and environment templates define
(`float_parameter_names`), because a number given to a FLOAT is read differently:

- A string is an `ExprValue::String`.
- Any number given to a FLOAT parameter is an `ExprValue::Float` with no spelling, so it is
  written the way a computed float is, CPython's `repr` (Expression Language §1.3.4):
  `{"F": 2}` is `2.0`, `{"F": 1e20}` is `1e+20`, `{"F": 0.00001}` is `1e-05`, and
  `{"F": 18446744073709551615}` is `1.8446744073709552e+19`. Zero is `0.0`.
- For any other parameter, an integer in the 64-bit signed range is an `ExprValue::Int`,
  which every type reads as it reads the integer's text. serde_json reads `-0` as a float,
  so it is the text `-0.0` (an INT refuses it; Python accepts it as `0`).
- For any other parameter, a float is an `ExprValue::String` holding its text as Python
  writes it (`repr(float)`, which is also `str(float)`): `format_float`'s text, but `-0.0`
  for negative zero. A STRING gets that text, as Python's `str` gives it. An INT or BOOL
  reads the text, so `{"I": 3.0}` is refused, as `-p I=3.0` is.
- For any other parameter, an integer from `i64::MAX + 1` to `u64::MAX` (serde_json holds
  it as a `u64`) is a string of its digits, so a STRING keeps them and an INT error quotes
  them. serde_json reads an integer below `i64::MIN` or above `u64::MAX` as a float, so it
  is written as one (`-9.223372036854776e+18`) and an INT error quotes that text.
- Anything else (a boolean, `null`, a list, an object) is a string of its JSON text. A list
  parameter reads that text as JSON, so a number item of a `LIST[FLOAT]` is a float with
  no spelling too.

**Divergence from openjd-cli / openjd-model-for-python.** The Python CLI passes the decoded
value itself, so the differences include:

- A FLOAT value given as a JSON integer is written as the integer (`{"F": 2}` is `2`, not
  `2.0`); a float is written as `str(float)`, which is `repr`.
- A STRING given `true` is `True` in Python and `true` here, and one given an integer below
  `i64::MIN` or above `u64::MAX` keeps its digits in Python.
- Python refuses a number given to a BOOL, RANGE_EXPR or PATH parameter, a number item of a
  `LIST[STRING]`, and a float item of a `LIST[INT]`; this CLI reads each as its text.
- Python reads a YAML parameter file as YAML 1.1, so `F: 1e20` is the string `1e20` there and
  the number `1e20` here.

A value given as `Key=Value` is always a string: `-p F=2` is the `<floatstring>` `2`, which
keeps its spelling (Template Schemas §7.5) and is written `2`.

## Task Parameters (`-t` / `--task-param`)

`parse_task_params()` handles the `-t`/`--task-param` argument for the `run` command. It
accepts only `NAME=VALUE` format and returns a `HashMap<String, String>`.

```
openjd run template.yaml -t Frame=42 -t Quality=high
```

### Validation Rules

- Each argument must contain exactly one `=` with non-empty name and value
- Duplicate parameter names are errors
- The whole argument is trimmed before it is split, so `-t "Frame=7 "` gives the value `7`.
  Coercing the value to its declared type is a separate step
  ([run.md](run.md#explicit-task-parameters)).
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
names to string or numeric values. Numeric values are stringified. YAML task files resolve
booleans, nulls and non-finite floats by the YAML 1.2 Core schema with strict booleans, as
templates do (see [parsing.md](../model/parsing.md#pass-1-raw-parsing)). Only `true`, `True`, `TRUE`, `false`,
`False` and `FALSE` are booleans, and a boolean value is rejected. Shorthand words such as
`yes`, `no`, `on`, and `off`, and other casings such as `tRUE`, remain strings.

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
5. Each rule's `source_path` must be absolute for its `source_path_format`,
   and neither path may be longer than 32 KiB; and the rules' trie must fit
   the default 30 MiB limit
   (checked with `PathMapper::new`, see
   [expr path-mapping.md](../expr/path-mapping.md#source-paths-must-be-absolute)).
   The error is `Invalid path mapping rules: path mapping rule <i>: <reason>`,
   where `<i>` is the rule's position in `path_mapping_rules`.

The rules keep the order given. The session's `PathMapper` decides which rule
applies to a path: the one whose source path has the most names, and between
rules for the same location, the one given last.

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

`parse_cli_parameters()` and `float_parameter_names()` live in `run/params.rs`, are
re-exported by the public `run` module, and are called by `summary.rs` via
`crate::run::parse_cli_parameters()` and `crate::run::float_parameter_names()`.

This cross-module dependency exists because both `summary` and `run` need to parse `-p`
arguments identically. The function could be extracted to a shared module, but with only
two callers the current arrangement is simpler.
