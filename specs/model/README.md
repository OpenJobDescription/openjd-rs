# openjd-model Crate Specifications

Design specifications for the `openjd-model` crate — the Rust implementation of the
Open Job Description template model.

This crate implements parsing, validation, and instantiation of OpenJD job and environment
templates per the [2023-09 Template Schemas](https://github.com/OpenJobDescription/openjd-specifications/wiki/2023-09-Template-Schemas)
specification. It was inspired by the Python reference implementation
([openjd-model-for-python](https://github.com/OpenJobDescription/openjd-model-for-python))
but redesigned for Rust's type system and performance characteristics.

## Document Index

| Document | Description |
|----------|-------------|
| [architecture.md](architecture.md) | Crate structure, module layout, dependency graph, and key design decisions |
| [public-api.md](public-api.md) | Authoritative reference for every public type, function, and re-export |
| [template-types.md](template-types.md) | Unresolved template types (deserialized from YAML, format strings unevaluated) |
| [job-types.md](job-types.md) | Instantiated job types (fully resolved, output of `create_job`) |
| [parameters.md](parameters.md) | Job and task parameter type systems, value coercion, constraint checking |
| [parsing.md](parsing.md) | Template decoding pipeline: YAML/JSON → version dispatch → serde → validation |
| [validation.md](validation.md) | Multi-pass validation pipeline: limits, structure, extensions, format strings |
| [job-creation.md](job-creation.md) | Job creation pipeline: parameter merging, preprocessing, symbol table, instantiation |
| [parameter-space.md](parameter-space.md) | Lazy parameter space iteration: node tree, combination expressions, chunking |
| [step-dependencies.md](step-dependencies.md) | Step dependency graph: construction, topological sort, cycle detection |
| [capabilities.md](capabilities.md) | Standard capability constants, name validation functions, regex patterns |
| [error-handling.md](error-handling.md) | Error types, structured error paths, validation error accumulation |

## Two-Phase Type System

The crate's central architectural pattern is a two-phase type system that mirrors the two
stages of the job lifecycle:

1. **`template::*` types** — A template file (JSON or YAML) is parsed and validated according
   to the specification revision and extensions it declares. The result is a typed template
   struct with `FormatString` fields still unevaluated. This phase catches structural errors,
   constraint violations, and invalid variable references before any parameter values are involved.

2. **`job::*` types** — A validated template is combined with specific job parameter values
   via `create_job()` to produce a runnable job. Template-scope format strings are resolved
   to concrete values; session/task-scope strings remain as `FormatString` for evaluation
   at runtime when the execution environment is known.

This separation reflects the job lifecycle: a template is a reusable artifact that is validated
once, then instantiated into many jobs with different parameter values. The two type phases
exist because a template definition and a concrete job instance are fundamentally different
things.

## Relationship to the Python Library

The Rust crate mirrors the Python library's public API surface but diverges in implementation:

- **Validation**: Python uses Pydantic model validators; Rust uses a multi-pass pipeline with
  explicit `ValidationErrors` accumulation after serde deserialization.
- **Type dispatch**: Python uses Union types over version-specific Pydantic models; Rust uses
  enums and direct struct types with `#[serde(deny_unknown_fields)]`.
- **Parameter space**: Python's `StepParameterSpaceIterator` mutates a passed-in dict to avoid
  allocation; Rust's version uses index arithmetic on a node tree for zero-allocation random access.
- **Error formatting**: Both produce Pydantic-compatible error paths for consistency with existing
  tooling and error message expectations.

## Specification Version Coverage

Currently implements `2023-09` with extensions:
- `TASK_CHUNKING` (RFC 0001)
- `REDACTED_ENV_VARS` (RFC 0003)
- `FEATURE_BUNDLE_1` (RFC 0004)
- `EXPR` (RFC 0005)
- `WRAP_ACTIONS` (RFC 0008) — full schema and runtime support. Wrap-action
  routing tests live in `crates/openjd-sessions/tests/integration/test_wrap_actions.rs`.
  Re-materialization of the wrap environment's embedded files on each task
  run (needed to resolve `Env.File.*` inside `onWrapTaskRun` scripts) is a
  follow-up.
