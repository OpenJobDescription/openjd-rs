# Parameter Parsing

## Purpose

The CLI accepts parameters in several formats across multiple arguments. This document
covers all parameter parsing functions in the CLI, their input formats, error handling,
and the rationale for supporting each format.

## Job Parameters (`-p`, `--job-param`, `--parameter`)

`parse_cli_parameters()` in `run/params.rs` parses job parameters for `run`, `summary` and
`diff`. The short flag is `-p` in all three. The long flag is `--job-param` for `run` and
`summary`, with `--parameter` as a hidden alias. The long flag is `--parameter` for `diff`,
with `--job-param` as a hidden alias. `diff` also parses `--old-parameter` and
`--new-parameter` with it (see [diff.md](diff.md)). The values of each call are applied in
order, so a later value wins. It accepts three input formats and returns a
`HashMap<String, ExprValue>`.

A JSON or YAML null means the value is not given. That covers a null value, such as
`{"A": null}` inline or in a JSON file, and any YAML null (`null`, `Null`, `NULL`, `~`, or an
empty value) in a YAML file. A file
that is only `null` is rejected with `Job parameter file 'file://p.json' should contain a
dictionary.` This matches deadline-cloud's job bundle parameter handling
(`apply_job_parameters` in `job_bundle/parameters.py`), which skips a None value so the
template default applies. It does not match `deadline bundle submit -p`, which rejects a null
value with `Parameter A has no value.` (deadline-cloud mainline; release 0.60.11 accepts only
Key=Value). The null removes the parameter from the map, and it
still takes part in "later wins": `-p A=1 -p '{"A": null}'` leaves `A` unset, so the template
default applies. With no default, building the job fails with `Values missing for required
job parameters: A`. A null for a parameter the template does not define is ignored, with no
error. A non-null value for one fails with `Job parameter values provided for parameters that
are not defined in the template: Z`. In `openjd diff`, a null in `--new-parameter` or
`--old-parameter` overrides a `-p` value for that side. `A=null` is a Key=Value string, so it
stays the text `null`, as in `deadline`.

openjd-rs differs from the Python `openjd` CLI on purpose: that CLI keeps the None
(openjd-cli `_job_from_template.py`) and openjd-model turns it into the text `None`
(`_create_job.py`, `str(value)`), so `{"A": null}` runs with `A=None` even when `A` has
no default.

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
must contain a dictionary mapping parameter names to values. Each value goes through
`set_json_param()`, which removes the parameter for a null and converts any other value to
`ExprValue` with `json_val_to_expr()`:

- JSON strings → `ExprValue::String`
- null → not given (the parameter is removed, see above)
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
  This is unreachable through the CLI today. Inline JSON is only tried when the argument
  starts with `{` and ends with `}`, so any text that parses is an object; anything else
  gets the bad-format message.
- Oversized file: `"File '<path>' exceeds maximum size of <N> bytes."`

### File Size Limit

All `file://` inputs read by the `run` subcommand — job parameter files, task files
(`--tasks`), and path-mapping rule files (`--path-mapping-rules`) — are capped at
`common::MAX_FILE_INPUT_SIZE` (10 MiB). The cap is enforced by reading through
`Read::take(cap + 1)`, so allocation is bounded to `cap + 1` bytes regardless of
the underlying file size. The template itself has a separate limit enforced by
the model crate's `CallerLimits::max_template_size`.

### Value Coercion

`json_val_to_expr()` converts every non-null JSON value to `ExprValue::String`:

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
based on the parameter definition. The Python openjd CLI instead keeps typed values and
turns them into text later (`openjd-model _create_job.py str(value)`), so it gives `True`
and `None` where openjd-rs gives `true` and treats null as not given.

### Known type coercion bugs (shared by run, summary and diff)

Known bugs in the shared parser. Each was run with `openjd diff t.yaml
--old-parameter A=x --new-parameter <input>`, where `A` is a STRING.

- A JSON or YAML `true` or `false` gives the text `true` or `false`: `{"A": true}`, or
  `A: true` in a YAML file, gives `~ parameter A: x -> true`. The Python openjd CLI gives
  `True` and `False` (`openjd-model _create_job.py str(value)`).
- The parse location is dropped. A malformed file (`{"A": ` in `bad.json`, or
  `A: [unclosed` in `bad.yaml`) reports `Job parameter file 'file://bad.json' should
  contain a dictionary.`. Malformed inline JSON (`{"A": }`) reports the generic
  `Job parameter string ('{"A": }') not formatted correctly. ...`.
- A duplicate key in a YAML file (`A: a` then `A: b`) reports `Job parameter file
  'file://dup.yaml' should contain a dictionary.`.
- The `.json` extension is compared case-sensitively, so a `.JSON` file is read as YAML.
  `{"A": "a", "A": "b"}` gives `b` from `dupj.json` and `should contain a dictionary` from
  `dupj.JSON`.
- `file://file://x.json` strips the prefix twice and reads `x.json`.
- An integer too large for 64 bits loses digits: `{"A": 123456789012345678901234567890}`
  gives `1.2345678901234568e+29`, from inline JSON, a JSON file or a YAML file.
- A YAML file reads `007` and `010` as floats: `A: 007` gives `7.0`, and `A: 010` gives
  `10.0`. Python openjd and `deadline` read them as 7 and 8 (YAML 1.1).
- `--new-parameter =v` names no parameter in its error. It reports `Validation error: new
  parameters: Job parameter values provided for parameters that are not defined in the
  template: ` with an empty name. When `A` has no default, a second line follows:
  `Values missing for required job parameters: A`.

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

`parse_cli_parameters()` lives in `run/params.rs`, is re-exported by the public `run`
module, and is called by `summary.rs` and `diff.rs` via
`crate::run::parse_cli_parameters()`.

This cross-module dependency exists because `summary`, `run` and `diff` need to
parse `-p` arguments identically. The function could be extracted to a shared module.
