# Context-Aware Help

## Purpose

When a user runs `openjd run <template> --help`, the CLI generates help text that
includes the job template's parameter definitions — their names, types, defaults,
constraints, and descriptions. This is more useful than generic clap help because it
tells the user exactly what parameters the specific template expects.

This feature is called "context-aware help" because the help output depends on the
context (the template file), not just the command's static argument definitions.

## Interface

Context-aware help is triggered automatically when:
1. The second CLI argument is `run`
2. Any argument is `-h` or `--help`
3. A positional argument (the template path) is present after `run`

There is no separate command or flag — it intercepts the standard help mechanism.

## Implementation Strategy

### Why Interception Before Clap

Clap's built-in help handling consumes `-h`/`--help` during parsing and prints static
help text derived from the struct definitions. There is no hook to inject dynamic content
into clap's help output after parsing.

The Python CLI solves this with a custom argparse `Action` subclass (`JobTemplateHelpAction`)
that runs when `-h` is encountered. Clap doesn't have an equivalent mechanism, so the
Rust CLI intercepts the raw `std::env::args()` before clap parsing begins.

```rust
// In main(), before Cli::parse():
let args: Vec<String> = std::env::args().collect();
if help::try_context_aware_help(&args) {
    return;
}
```

If the interception succeeds (template is valid, help is printed), the function calls
`std::process::exit(0)` and main never reaches clap. If it fails (template not found or
invalid), it calls `std::process::exit(1)` with an error message.

### Argument Detection

`try_context_aware_help()` performs minimal argument inspection:

1. Checks `args.len() >= 3` (binary, "run", and at least one more arg)
2. Checks `args[1] == "run"`
3. Checks any arg is `-h` or `--help`
4. Finds the template path: first arg after "run" that doesn't start with `-`
5. Finds `--extensions` value if present (looks for `--extensions` followed by a value)

This is intentionally simple — it doesn't try to fully parse the argument list. Edge cases
(e.g., `--extensions` as the last arg with no value) are handled by falling through to
clap's normal error reporting.

## Help Generation Pipeline

```
try_context_aware_help(&args)
  │
  ├── Detect "run" + "--help" + template path
  │
  └── generate_template_help(path, extensions)
      │
      ├── Read and parse template file
      ├── Resolve extensions (same logic as check/summary/run)
      ├── decode_job_template()
      │
      └── format_help(&template, &path)
          │
          ├── Usage line: "usage: openjd run <path> [arguments]"
          ├── Job name
          ├── Job description (if present)
          ├── Parameter definitions (if any)
          │   └── For each parameter: format_parameter()
          └── Standard options list (static)
```

## Parameter Formatting

`format_parameter()` produces a multi-line description for each job parameter:

```
  Frames (INT) [default: 100] (range: 1 to 10000)
    The frame range to render
```

### Components

1. **Name and type**: `Frames (INT)`
2. **Default or required**: `[default: 100]` or `[required]`
3. **Constraints**: Parenthesized, comma-separated list
4. **Description**: Indented on the next line (if present)

### Default Value Display

| Condition | Display |
|-----------|---------|
| No default | `[required]` |
| STRING/PATH with newlines | `[default: see below]` + multi-line block at end |
| STRING/PATH without newlines | `[default: 'value']` (quoted) |
| Other types | `[default: value]` (unquoted) |

Multi-line defaults (common for script content in PATH parameters) are shown after the
description to avoid breaking the compact single-line format.

### Constraint Display

Constraints are collected into a list and joined with commas:

| Constraint | Format |
|------------|--------|
| INT range | `range: 1 to 100` |
| INT min only | `minimum: 1` |
| INT max only | `maximum: 100` |
| FLOAT range | `range: 0.0 to 1.0` |
| String length range | `length: 1 to 256 characters` |
| String min length | `minimum length: 1 characters` |
| String max length | `maximum length: 256 characters` |
| Allowed values (STRING/PATH) | `allowed: 'low', 'medium', 'high'` |
| Allowed values (other) | `allowed: 1, 2, 3` |

### Description Extraction

`get_description()` pattern-matches on `JobParameterDefinition` variants to extract the
`description` field. It handles STRING, INT, FLOAT, PATH, BOOL, and RANGE_EXPR variants.
List parameter types return `None` (they don't have a description field in the current
model).

## Standard Options Block

After the parameter definitions, a short block lists the most commonly needed `run`
command options. This is deliberately abbreviated compared to the full option set — the
context-aware help is focused on the job template (its name, description, and parameters),
so showing every option would bury the template-specific information that the user asked
for.

The hint on `--help` tells users how to see the full option list:

```
Standard Options:
  -p, --parameter <KEY=VALUE> Job parameters (Key=Value, file://path, or inline JSON)
  --environment <PATH>       Environment template files
  --verbose                  Enable verbose logging
  -h, --help                 Print help (leave out template to list all options)
```

Running `openjd run --help` (without a template path) bypasses the context-aware
interception and falls through to clap's standard help, which lists every option with
full descriptions. This two-tier approach keeps the template-focused help clean while
still making the complete option reference discoverable.

## Error Handling

If the template file doesn't exist or can't be parsed, `generate_template_help()` returns
an error. `try_context_aware_help()` prints the error to stderr and exits with code 1.
It does not fall back to clap's generic help — the user asked for help on a specific
template, so a parse error is the correct response.

## Differences from Python CLI

| Aspect | Python | Rust |
|--------|--------|------|
| Mechanism | `JobTemplateHelpAction` (argparse Action subclass) | Pre-clap argument interception |
| Trigger | argparse calls action when `-h` is encountered | Manual arg scanning in `main()` |
| Fallback | Falls back to standard argparse help on parse error | Exits with error on parse error |
| Integration | Integrated into argparse's help system | Completely separate from clap |

The Python approach is more elegant because it's integrated into the argument parser's
help system. The Rust approach is a workaround for clap's lack of custom help actions,
but it produces equivalent output.
