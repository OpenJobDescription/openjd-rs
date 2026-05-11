# openjd-rs Architecture

## Overview

A Rust implementation of the Open Job Description (OpenJD) model library, expression
language, sessions runtime, and CLI, targeting conformance with the
[2023-09 Template Schemas](../../openjd-specifications/wiki/2023-09-Template-Schemas.md)
and the [Expression Language](../../openjd-specifications/wiki/2026-02-Expression-Language.md) extension.

## Project Structure

```
openjd-rs/
├── Cargo.toml                  # Workspace root
├── crates/
│   ├── openjd-expr/            # Expression language: types, parser, evaluator, path mapping
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── types.rs        # Type system (ExprType, TypeCode)
│   │       ├── value.rs        # Runtime values (ExprValue)
│   │       ├── symbol_table.rs # Hierarchical symbol table
│   │       ├── range_expr.rs   # RangeExpr parsing
│   │       ├── path_mapping.rs # Path format and mapping rules
│   │       ├── error.rs        # Expression error types
│   │       └── eval/           # Expression parsing and evaluation
│   │           ├── mod.rs
│   │           ├── parse.rs    # ruff_python_parser integration
│   │           └── evaluator.rs # AST-walking evaluator with resource bounds
│   ├── openjd-model/           # Core library: template parsing, validation
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── error.rs
│   │       ├── types.rs
│   │       ├── parse.rs
│   │       ├── symbol_table.rs # Simple string-based symtab (base spec)
│   │       ├── format_string.rs
│   │       ├── validate.rs
│   │       ├── template/        # Template model types (revision-independent)
│   │       └── job/             # Instantiated job types (result of create_job)
│   ├── openjd-sessions/        # Session runtime
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── action.rs
│   │       ├── session.rs
│   │       └── embedded_files.rs
│   └── openjd-cli/             # CLI binary
│       └── src/
│           ├── main.rs
│           ├── check.rs
│           ├── summary.rs
│           ├── run.rs
│           └── help.rs
└── specs/                      # Design specs
    ├── architecture.md
    ├── model/                  # openjd-model crate specs
    ├── expr/                   # openjd-expr crate specs
    ├── sessions/               # openjd-sessions crate specs
    ├── snapshots/              # openjd-snapshots crate specs
    └── cli/                    # openjd-cli crate specs
```

## Crate Dependency Graph

```
openjd-cli
  ├── openjd-model
  │     └── openjd-expr
  └── openjd-sessions
        ├── openjd-model
        └── openjd-expr
```

## Crate Responsibilities

### openjd-expr
Expression language implementation. No dependency on model or sessions. Provides:
- Type system (`ExprType`, `TypeCode`) matching the EXPR extension spec
- Runtime values (`ExprValue`) with truthiness, comparison, memory tracking
- Hierarchical symbol table for expression evaluation
- Expression parsing via `ruff_python_parser` (see `specs/expr/parser.md`)
- AST-walking evaluator with memory and operation bounds
- Integer range expression parsing (`1-10:2,20-30`)
- Path format and mapping rules

### openjd-model
Core template library. Depends on openjd-expr. Provides:
- YAML/JSON template parsing and deserialization (serde)
- Schema validation (structural + cross-field constraints)
- Format string parsing and resolution
- All v2023-09 model types

### openjd-sessions
Runtime library. Depends on openjd-model and openjd-expr. Provides:
- Session lifecycle management
- Action execution (subprocess management)
- Re-exports path mapping from openjd-expr

### openjd-cli
Binary crate. Depends on all three libraries.
Commands: `check` (validate templates), `summary` (job/step summary), `run` (execute jobs locally).

## Key Design Decisions

- **Parser**: `ruff_python_parser` via crates.io (see `specs/expr/parser.md`)
- **Serialization**: serde + serde_yaml for template deserialization
- **Validation**: Post-deserialization validation pass (not inline with serde)
- **Error handling**: `thiserror`-based enums throughout
- **Workspace layout**: `crates/` directory (rattler convention)

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
  expected to change.
- **`openjd-for-js`** — WebAssembly bindings for `openjd-model` and
  `openjd-expr`. Built as an npm package; not published to crates.io
  (`publish = false`). Both the Rust side and the npm package layout
  are subject to change.

## Detailed Specifications

Detailed design specifications for individual crates:

- [model/](model/) — `openjd-model` crate: template types, validation, job creation, parameter space iteration
- [expr/](expr/) — `openjd-expr` crate: expression language, format strings, symbol tables
- [sessions/](sessions/) — `openjd-sessions` crate: session runtime, subprocess management, action monitoring
- [snapshots/](snapshots/) — `openjd-snapshots` crate (**experimental**): job attachment snapshot operations
- [cli/](cli/) — `openjd-cli` crate: CLI binary, command implementations, output formatting, context-aware help
- [openjd-for-js/spec.md](openjd-for-js/spec.md) — `openjd-for-js` crate (**experimental**): ECMAScript/WebAssembly bindings