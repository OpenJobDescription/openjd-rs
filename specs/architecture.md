# openjd-rs Architecture

## Overview

A Rust implementation of the Open Job Description (OpenJD) model library, expression
language, sessions runtime, and CLI, targeting conformance with the
[2023-09 Template Schemas](https://github.com/OpenJobDescription/openjd-specifications/wiki/2023-09-Template-Schemas)
and the [Expression Language](https://github.com/OpenJobDescription/openjd-specifications/wiki/2026-02-Expression-Language) extension.
The workspace also holds a job attachments snapshots library and WebAssembly bindings,
both experimental (see [Crate Status](#crate-status)).

## Project Structure

The tree shows each crate's source directories. Comments say what a directory holds
rather than listing its files. Each crate also has a `tests/` directory of integration
tests, not shown. The crate spec directories under `specs/` describe the modules in
detail; see [Detailed Specifications](#detailed-specifications).

```
openjd-rs/
├── Cargo.toml                  # Workspace root: members, shared metadata, lints, dependency versions
├── crates/
│   ├── openjd-expr/            # Expression language
│   │   └── src/                # lib.rs plus one module per area: types, values, symbol table, format strings,
│   │       │                   #   function library and profiles, range expressions, path mapping and
│   │       │                   #   URI-aware paths, errors, and their helpers
│   │       ├── eval/           # Expression parsing (ruff_python_parser) and the bounded AST-walking evaluator
│   │       └── functions/      # Built-in function implementations, one file per function family
│   ├── openjd-model/           # Template parsing, validation, job creation
│   │   └── src/                # lib.rs, error and shared types, capabilities, in-crate test modules
│   │       ├── template/       # The 2023-09 template model types and parsing (parse.rs)
│   │       │   ├── validation/         # Revision-neutral validation entry points, limits and rules types
│   │       │   └── validate_v2023_09/  # The 2023-09 validation passes
│   │       └── job/            # Instantiated job types, parameter space iteration, step dependency graph;
│   │           │               #   job diff, task delta, template diff and apply plan (unstable)
│   │           └── create_job/ # create_job: parameter resolution and template instantiation
│   ├── openjd-sessions/        # Session runtime
│   │   ├── build.rs            # Builds the cross-user helper and embeds it in the crate
│   │   └── src/                # lib.rs plus modules for the session, actions and their status, subprocesses,
│   │       │                   #   let bindings, limits, logging and action filtering, embedded files,
│   │       │                   #   temp dirs, session users, the cross-user client, and Windows support
│   │       ├── runner/         # Environment and step script runners
│   │       └── helper/         # Cross-user helper binary: a separate Cargo project (Cargo.bundled.toml)
│   │           ├── src/        # Helper protocol and output framing, Unix and Windows runners, process trees
│   │           └── tests/      # Helper protocol tests
│   ├── openjd-cli/             # The `openjd` binary
│   │   └── src/                # main.rs (subcommand dispatch), check, summary, diff (unstable), help,
│   │       │                   #   shared helpers
│   │       └── run/            # `openjd run`: parameters, execution, result reporting
│   ├── openjd-snapshots/       # Job attachments snapshots (experimental)
│   │   └── src/                # lib.rs plus manifest types, the codec (v2023 and v2025 encode and decode),
│   │       │                   #   hashing, caches, and S3 helpers
│   │       ├── codec/          # Codec internals: a lenient typed manifest reader and a streaming v2023 decoder
│   │       ├── ops/            # Snapshot operations: collect, hash, filter, diff, upload, download, ...
│   │       └── bin/            # Benchmark binary
│   └── openjd-for-js/          # WebAssembly bindings (experimental), with npm package files
│       ├── src/                # wasm-bindgen bindings for openjd-model and openjd-expr
│       └── js-tests/           # Vitest tests of the built package
├── .github/                    # CI workflows (authoritative for the CI checks), PR and issue templates
├── skills/                     # Agent skills, including implement-change
├── fuzz/                       # Fuzz harness: a separate cargo workspace (see fuzz/README.md)
├── scripts/                    # Dev and CI scripts: public API, copyright header and fuzz coverage checks, coverage,
│                               #   cross-user tests, generated-table and fixture generators
├── docs/
│   └── dev/                    # Contributor docs, including quality-criteria.md
├── reports/                    # Crate quality evaluation and other engineering reports
├── samples/                    # Sample templates
├── testing_containers/         # Docker environments for the cross-user tests
└── specs/                      # Design specs
    ├── architecture.md
    ├── expr/                   # openjd-expr crate specs
    ├── model/                  # openjd-model crate specs
    ├── sessions/               # openjd-sessions crate specs
    ├── cli/                    # openjd-cli crate specs
    ├── snapshots/              # openjd-snapshots crate specs
    ├── openjd-for-js/          # openjd-for-js crate spec
    ├── job-attachments-snapshots.md   # Cross-cutting design docs
    ├── snapshots-benchmarking-plan.md
    ├── windows-cross-user-helper.md
    └── rust-port-agent-method.md      # How the crates were ported from Python
```

## Crate Dependency Graph

Normal (non-dev) dependencies between workspace crates, from `cargo metadata`:

```
openjd-cli
  ├── openjd-sessions
  │     ├── openjd-model
  │     │     └── openjd-expr
  │     └── openjd-expr
  ├── openjd-model
  └── openjd-expr

openjd-for-js
  ├── openjd-model
  └── openjd-expr

openjd-snapshots            (no workspace dependencies)
```

The cross-user helper in `crates/openjd-sessions/src/helper/` is not a workspace
member and depends on no workspace crate; `openjd-sessions`' build script builds it.

## Crate Responsibilities

### openjd-expr
Expression language implementation. No dependency on any other workspace crate. Provides:
- Type system (`ExprType`, `TypeCode`) matching the EXPR extension spec
- Runtime values (`ExprValue`) with comparison and memory tracking
- Hierarchical symbol table for expression evaluation
- Format string parsing and resolution (`{{Param.Name}}`, `FormatString`)
- Expression parsing via `ruff_python_parser` (see `specs/expr/parser.md`)
- AST-walking evaluator with memory and operation bounds
- Function library with signature-based dispatch, the default built-in library, and
  expression profiles (revision, extensions, host context)
- Integer range expression parsing (`1-10:2,20-30`)
- Path format and mapping rules

### openjd-model
Core template library. Depends on openjd-expr. Provides:
- YAML/JSON template parsing and deserialization (serde)
- Schema validation (structural + cross-field constraints, including format string checks)
- Job creation (`create_job`): parameter resolution and format string evaluation
- Parameter space iteration and the step dependency graph
- Capability definitions for host requirements
- All v2023-09 model types
- Re-exports of `format_string` and `symbol_table` from openjd-expr

### openjd-sessions
Runtime library. Depends on openjd-model and openjd-expr. Provides:
- Session lifecycle management
- Action execution (subprocess management, process tree termination)
- Running actions as another OS user through an embedded helper binary
- Embedded files, session temp directories, and action output filtering and logging
- Re-exports path mapping from openjd-expr

### openjd-cli
Binary crate (`openjd`). Depends on openjd-sessions, openjd-model, and openjd-expr.
Commands: `check` (validate templates), `summary` (job/step summary), `run` (execute jobs locally),
`diff` (diff two templates or two parameter sets; unstable).

### openjd-snapshots
Job attachments library (experimental, see [Crate Status](#crate-status)). No dependency
on any other workspace crate. Provides content-addressed directory tree snapshots:
manifest types and their v2023 and v2025 on-disk formats, hashing and caches, and
operations such as collect, diff, upload to S3, and download.

### openjd-for-js
WebAssembly bindings (experimental, see [Crate Status](#crate-status)). Depends on
openjd-model and openjd-expr. Compiles to `wasm32-unknown-unknown` and is packaged for
npm with `wasm-bindgen`.

## Key Design Decisions

- **Parser**: `ruff_python_parser` via crates.io (see `specs/expr/parser.md`)
- **Serialization**: serde, with serde-saphyr for YAML and serde_json for JSON
- **Validation**: Post-deserialization validation pass (not inline with serde)
- **Error handling**: `thiserror`-based enums in the library crates
- **Workspace layout**: `crates/` directory (rattler convention)
- **Fuzzing**: `fuzz/` is a separate cargo workspace (cargo-fuzz + libFuzzer)
  with property oracles over the library crates and a CI gate that every
  public function is executed by its corpus or excluded with a reason
  (`openjd-for-js` is not gated). See [`fuzz/README.md`](../fuzz/README.md).

## Crate Status

The beta crates — `openjd-expr`, `openjd-model`, `openjd-sessions`,
and `openjd-cli` — are released to crates.io via
[release-plz](https://release-plz.dev). They are still on the 0.x
version line, so breaking changes may land in minor-version bumps per
Cargo's [pre-1.0 semver rules][pre-1.0].

[pre-1.0]: https://doc.rust-lang.org/cargo/reference/semver.html

Two crates are **experimental**, meaning their public APIs may change
without notice:

- **`openjd-snapshots`** — job attachments library. Its v2023 on-disk
  manifest format is stable and is the format AWS Deadline Cloud uses;
  the v2025 format is an **experimental draft** whose wire format is
  expected to change. It is released to crates.io by release-plz with the
  beta crates.
- **`openjd-for-js`** — WebAssembly bindings for `openjd-model` and
  `openjd-expr`. Built as an npm package; not published to crates.io
  (`publish = false`) or to npm. Both the Rust side and the npm package layout
  are subject to change.

## Detailed Specifications

Detailed design specifications for individual crates:

- [model/](model/) — `openjd-model` crate: template types, validation, job creation, parameter space iteration
- [expr/](expr/) — `openjd-expr` crate: expression language, format strings, symbol tables
- [sessions/](sessions/) — `openjd-sessions` crate: session runtime, subprocess management, action monitoring
- [snapshots/](snapshots/) — `openjd-snapshots` crate (**experimental**): job attachment snapshot operations
- [cli/](cli/) — `openjd-cli` crate: CLI binary, command implementations, output formatting, context-aware help
- [openjd-for-js/spec.md](openjd-for-js/spec.md) — `openjd-for-js` crate (**experimental**): ECMAScript/WebAssembly bindings