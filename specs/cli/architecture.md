# openjd-cli Architecture

## Crate Position in the Workspace

The `openjd-cli` crate is the binary entry point that depends on all three library crates:

```
openjd-cli
├── openjd-model      # Template parsing, validation, job creation, parameter spaces
├── openjd-sessions   # Session runtime, subprocess execution, environment lifecycle
└── openjd-expr       # Expression evaluation, symbol tables (used directly for run-time eval)
```

The CLI is a thin orchestration layer. It parses user arguments, delegates to the library
crates for all domain logic, and formats output. No template parsing, validation, or
session management logic lives in the CLI crate itself.

## Module Layout

```
src/
├── main.rs       # Entry point: Cli/Commands structs, SessionLogger, tokio::main
├── check.rs      # CheckArgs + execute(): template validation
├── summary.rs    # SummaryArgs + execute(): job/step summary output
├── run.rs        # RunArgs + execute(): full session lifecycle, helper functions
└── help.rs       # Context-aware help: template-driven --help for run command
```

Each command module exports an `Args` struct (clap derive) and an `execute()` function.
`main.rs` dispatches to the appropriate `execute()` based on the parsed subcommand.

## Dependencies

| Dependency | Purpose |
|------------|---------|
| `openjd-model` | `parse::*` (template decoding), `create_job`, `preprocess_job_parameters`, `StepParameterSpaceIterator`, job/template types |
| `openjd-sessions` | `Session`, `SessionConfig`, `ActionState`, `PathMappingRule`, `embedded_files::write_embedded_file` |
| `openjd-expr` | `ExprValue`, `SymbolTable`, `Evaluator`, `ParsedExpression`, `FunctionLibrary`, `path_mapping::apply_rules`, `default_library` |
| `clap` | Argument parsing via derive macros (`Parser`, `Subcommand`, `Args`) |
| `serde_json` | JSON output formatting, inline JSON parameter parsing, path mapping rule loading |
| `serde_yaml` | YAML output formatting, YAML parameter file loading |
| `tokio` | Async runtime (`rt-multi-thread`, `macros`) for session execution |
| `log` | Logging facade; custom `Log` impl for subprocess output |
| `chrono` | Timestamp formatting (local, UTC) for session logs |

## Public API Surface

The crate is a binary (`[[bin]] name = "openjd"`), so it has no library API. However,
`run.rs` is `pub mod` to allow `summary.rs` to call `parse_cli_parameters()` — the only
cross-module dependency within the crate.

**Exported from `run.rs` (crate-internal):**
- `parse_cli_parameters(&[String]) -> Result<HashMap<String, ExprValue>>` — Parses `-p`/`--parameter` arguments

## Entry Point Flow

```
main()
  │
  ├── Initialize SessionLogger (log::set_boxed_logger)
  ├── Set log level to Info
  │
  ├── Intercept: help::try_context_aware_help(&args)
  │   └── If `run <path> --help`, parse template and print parameter help, exit
  │
  ├── Cli::parse() (clap)
  │
  └── match cli.command
      ├── Commands::Check(args)   → check::execute(args)
      ├── Commands::Summary(args) → summary::execute(args)
      └── Commands::Run(args)     → run::execute(args).await
```

The help interception happens before clap parsing because clap would consume `-h`/`--help`
and print its own generic help text. By intercepting first, the CLI can parse the template
file and generate help that includes job-specific parameter definitions.

## Key Design Decisions

### Clap Derive vs Argparse

The Python CLI uses argparse with manual `add_argument()` calls and a `SubparserGroup`
abstraction for modular subcommand registration. The Rust CLI uses clap's derive macros,
which generate the parser from struct definitions. This eliminates the manual wiring code
and makes the argument structure self-documenting in the type system.

The tradeoff is that clap derive is less flexible for dynamic argument generation. The
Python CLI's `@print_cli_result` decorator pattern (which wraps command functions to handle
output formatting uniformly) has no direct equivalent — each Rust command handles its own
output formatting inline.

### Direct Session API vs LocalSession Wrapper

The Python CLI wraps the sessions library in a `LocalSession` context manager that handles
signal registration, environment enter/exit ordering, and cleanup. The Rust CLI calls the
`Session` API directly in `run::execute()`, managing the lifecycle inline.

This was a deliberate simplification. The Python `LocalSession` exists partly to manage
Python's signal handling complexity (registering/restoring `SIGINT`/`SIGTERM` handlers,
coordinating cancellation across threads). Rust's tokio handles cancellation more naturally
through async task dropping, and the CLI's single-threaded command flow doesn't need the
abstraction layer.

The cost is that `run.rs` is the largest file in the crate (~580 lines) because it contains
the full session lifecycle inline. If the CLI grows more commands that need session
management, extracting a `LocalSession` equivalent would be warranted.

### Async Main

The `#[tokio::main]` attribute creates a multi-thread tokio runtime. Only the `run` command
is async (it awaits session actions), but the runtime is created unconditionally because
tokio's runtime initialization is cheap and having a consistent async context simplifies
future additions.

`check` and `summary` are synchronous functions called from the async main — tokio handles
this transparently.

### Global State for Logging

`SESSION_START` and `TIMESTAMP_FORMAT` are `OnceLock<T>` globals initialized by the `run`
command. The `SessionLogger` reads these to format timestamps on subprocess output lines.

This is global state, which is generally undesirable, but it's the pragmatic choice given
`log::Log`'s interface constraint: the `log()` method receives only a `&Record` with no
way to pass additional context. The Python CLI solves this differently — its
`LocalSessionLogHandler` is a class instance that stores the start time and format as
fields. Rust's `log` crate doesn't support stateful loggers as cleanly, so the globals
are the simplest correct approach.

### No Schema Command

The Python CLI's `schema` command uses Pydantic's `model_json_schema()` to generate JSON
Schema from the template model classes. The Rust crate uses serde for deserialization, which
doesn't have a built-in schema generation facility. Adding this would require either:

- Manual schema construction (error-prone, hard to keep in sync)
- Integration with `schemars` crate (would require `JsonSchema` derives on all template types)

This is deferred until there's demand for it.

### Extension Default List

All commands that accept `--extensions` default to enabling all supported extensions:
`["TASK_CHUNKING", "REDACTED_ENV_VARS", "FEATURE_BUNDLE_1", "EXPR"]`. This matches the
Python CLI's `SUPPORTED_EXTENSIONS` list. The default is hardcoded in each command's
`execute()` function rather than centralized — a minor duplication that could be extracted
to a shared constant if more commands are added.

An empty string (`--extensions ""`) disables all extensions, which is useful for testing
strict base-spec compliance.
