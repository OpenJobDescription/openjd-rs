# openjd-sessions Crate Specifications

Design specifications for the `openjd-sessions` crate — the Rust runtime for executing
Open Job Description sessions (environment enter/exit, task execution, subprocess
management, and action monitoring).

This crate was inspired by the Python reference implementation
([openjd-sessions-for-python](https://github.com/OpenJobDescription/openjd-sessions-for-python))
but redesigned around Rust's async ecosystem (tokio) and ownership model. The Python
library uses threads, queues, and locks for non-blocking execution; the Rust crate
replaces all of that with async/await, channels, and cancellation tokens.

## Document Index

| Document | Description |
|----------|-------------|
| [architecture.md](architecture.md) | Crate structure, module layout, dependency graph, public API surface |
| [session.md](session.md) | Session struct, state machine, lifecycle, environment tracking, symbol table construction |
| [subprocess.md](subprocess.md) | Async subprocess execution, stdout streaming, signal delivery, process group isolation |
| [action-filter.md](action-filter.md) | Parsing `openjd_*` directives from stdout, redaction, malformed command detection |
| [action-messages.md](action-messages.md) | Real-time ActionMessage streaming via tokio mpsc channels |
| [action-status.md](action-status.md) | ActionStatus struct, callback lifecycle, field semantics |
| [runners.md](runners.md) | EnvironmentScriptRunner and StepScriptRunner, two-phase embedded file flow |
| [embedded-files.md](embedded-files.md) | Two-phase file materialization, let binding integration, cross-user permissions |
| [cross-user.md](cross-user.md) | Cross-user subprocess execution, signal delivery, file ownership (POSIX) |
| [cross-user-testing.md](cross-user-testing.md) | Docker-based test infrastructure for cross-user functionality |
| [tempdir.md](tempdir.md) | Secure temp directory creation, sticky bit validation, cleanup |
| [logging.md](logging.md) | LogContent bitflags, structured kv metadata, session_log! macro, banners |
| [error-handling.md](error-handling.md) | SessionError enum, error propagation patterns |
| [win32-locate.md](win32-locate.md) | Windows executable resolution (not yet integrated) |

## How the Worker Agent Uses Sessions

The primary consumer is the Deadline Cloud worker agent. Understanding its usage patterns
drove the API design:

1. Agent creates a `Session` with `SessionConfig` (session_id, job parameters, path mapping
   rules, user, callback, os_env_vars, session_root_directory, revision_extensions).
2. The callback fires on every action start, `openjd_*` directive, and action completion —
   the agent forwards these to the Deadline service as real-time progress updates.
3. Actions (`enter_environment`, `exit_environment`, `run_task`) are async — the agent
   awaits them or can cancel via `cancel_action()`.
4. During cleanup, environments are exited in reverse order, then `session.cleanup()`
   deletes the working directory.

## Relationship to the Python Library

The Rust crate mirrors the Python library's public API surface but diverges significantly
in implementation. See [architecture.md § Python-vs-Rust Design Comparison](architecture.md#python-vs-rust-design-comparison)
for the full comparison table.

## Normative References

- [2023-09-Template-Schemas](../../../openjd-specifications/wiki/2023-09-Template-Schemas.md) — Formal specification (sessions, environments, actions, embedded files)
- [How-Jobs-Are-Run](../../../openjd-specifications/wiki/How-Jobs-Are-Run.md) — Runtime behavior specification

## Specification Version Coverage

Currently implements `2023-09` with extensions:
- `TASK_CHUNKING` (RFC 0001)
- `REDACTED_ENV_VARS` (RFC 0003)
- `FEATURE_BUNDLE_1` (RFC 0004)
- `EXPR` (RFC 0005)
