# openjd-cli Crate Specifications

Design specifications for the `openjd-cli` crate — the Rust CLI binary for validating,
summarizing, and running Open Job Description templates locally.

This crate is the user-facing entry point to the `openjd-rs` workspace. It was inspired
by the Python reference implementation
([openjd-cli](https://github.com/OpenJobDescription/openjd-cli)) but redesigned around
Rust's clap derive API, async runtime (tokio), and the structured logging facade.

## Document Index

| Document | Description |
|----------|-------------|
| [architecture.md](architecture.md) | Crate structure, module layout, dependency graph, public API surface, key design decisions |
| [check.md](check.md) | `check` command: template validation pipeline, extension handling, error reporting |
| [summary.md](summary.md) | `summary` command: job/step summarization, output formatting, parameter display |
| [run.md](run.md) | `run` command: full session lifecycle, task iteration, adaptive chunking, environment management |
| [help.md](help.md) | Context-aware help: template-driven `--help` output for the `run` command |
| [logging.md](logging.md) | Session logger: timestamp formatting, `openjd_log_content` filtering, log level control |
| [output-formatting.md](output-formatting.md) | Structured output: human-readable, JSON, and YAML output modes across all commands |
| [parameter-parsing.md](parameter-parsing.md) | CLI parameter parsing: `Key=Value`, `file://`, inline JSON, task params, path mapping rules |

## Relationship to the Python CLI

The Rust CLI mirrors the Python CLI's command surface and output format but diverges in
implementation:

| Aspect | Python | Rust |
|--------|--------|------|
| Argument parsing | argparse with manual subparser wiring | clap derive macros |
| Output formatting | `@print_cli_result` decorator on dataclass results | Inline match on `--output` format string |
| Session management | `LocalSession` context manager with signal handlers | Direct `Session` API calls in async `execute()` |
| Async runtime | None (synchronous, threads in sessions layer) | tokio multi-thread runtime |
| Logging | `LocalSessionLogHandler` (Python `logging.Handler`) | Custom `log::Log` impl filtering on kv metadata |
| Context-aware help | `JobTemplateHelpAction` (argparse `Action` subclass) | `try_context_aware_help()` intercepting args before clap |
| Schema command | `openjd schema --version` (Pydantic `model_json_schema`) | Not yet implemented |

## Commands

| Command | Description |
|---------|-------------|
| `openjd check <path>` | Validate a job or environment template file |
| `openjd summary <path>` | Print summary information about a job template |
| `openjd run <path>` | Run a job template locally in an OpenJD session |

## Not Yet Implemented

The following Python CLI features are not yet ported:

- **`schema` command** — Returns JSON Schema for a template model version. The Python CLI
  uses Pydantic's `model_json_schema()` which has no direct Rust equivalent; a manual schema
  generator or schemars integration would be needed.
