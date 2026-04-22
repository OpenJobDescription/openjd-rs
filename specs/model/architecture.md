# Architecture

## Crate Position in the Workspace

The `openjd-model` crate sits at the center of the `openjd-rs` workspace dependency graph:

```
openjd-cli ──► openjd-sessions ──► openjd-model ──► openjd-expr
```

- **openjd-expr** — Expression language parser and evaluator, format string interpolation,
  symbol tables, and the `ExprValue`/`ExprType` runtime value system.
- **openjd-model** — Template parsing, validation, and job instantiation. This crate.
- **openjd-sessions** — Session runtime that executes instantiated jobs.
- **openjd-cli** — CLI frontend (`openjd check`, `openjd run`, etc.).

## Module Layout

```
src/
├── lib.rs                    # Public API re-exports
├── error.rs                  # ModelError, ValidationErrors, PathElement
├── types.rs                  # Shared types: ValidationContext, parameter types, limits, rules
├── capabilities.rs           # Standard capability constants and validation functions
├── template/                 # Unresolved template types (phase 1)
│   ├── mod.rs
│   ├── parse.rs              # YAML/JSON decoding, version dispatch
│   ├── job_template.rs       # JobTemplate (§1.1)
│   ├── environment_template.rs # EnvironmentTemplate (§1.2)
│   ├── parameters.rs         # Job parameter definitions (§2.1-2.4)
│   ├── expr_parameters.rs    # EXPR extension parameter types (§2.9-2.16)
│   ├── task_parameters.rs    # Task parameter definitions (§3.4)
│   ├── step.rs               # StepTemplate, SimpleAction (§3)
│   ├── environment.rs        # Environment, EmbeddedFile (§4, §6)
│   ├── actions.rs            # Action, CancelationMode (§5)
│   ├── host_requirements.rs  # HostRequirements (§3.3)
│   ├── constrained_strings.rs # Identifier, Description, ExtensionName (§7)
│   └── validate_v2023_09/    # Validation pipeline for 2023-09 revision
│       ├── mod.rs            # Orchestrator
│       ├── limits.rs         # Pass 2: EffectiveLimits enforcement
│       ├── structure.rs      # Pass 3: Structural validation via EffectiveRules
│       ├── feature_bundle_1.rs # Pass 4: FEATURE_BUNDLE_1 gating
│       ├── format_strings.rs # Pass 5: Format string reference validation
│       ├── task_chunking.rs  # Pass 6: TASK_CHUNKING gating
│       └── helpers.rs        # Shared regex patterns, constants, utilities
└── job/                      # Instantiated job types (phase 2)
    ├── mod.rs                # Job, Step, StepScript, Environment, etc.
    ├── create_job/           # Job creation pipeline
    │   ├── mod.rs            # create_job() entry point
    │   ├── parameters.rs     # Parameter merging, preprocessing, symbol table
    │   ├── instantiate.rs    # Template → job type conversion
    │   └── ranges.rs         # Range expression evaluation
    ├── step_param_space.rs   # Lazy parameter space iteration
    └── step_dependency_graph.rs # Step dependency graph
```

## Public API Surface

The crate re-exports a curated public API from `lib.rs`:

**Functions:**
- `decode_job_template`, `decode_environment_template`, `decode_template` — Template parsing
- `create_job` — Full job instantiation pipeline
- `preprocess_job_parameters` — Parameter validation and coercion
- `merge_job_parameter_definitions` — Cross-template parameter merging
- `build_symbol_table` — Symbol table construction from parameter values
- `convert_environment` — Template environment to resolved environment
- `evaluate_let_bindings` — Let binding expression evaluation

**Types:**
- `DecodedTemplate`, `DocumentType` — Parse output types
- `StepParameterSpaceIterator` — Lazy parameter space iteration
- `StepDependencyGraph` — Step dependency graph
- `TaskParameterDefinition` — Task parameter definition (from template module)
- `MergedParameterDefinition` — Merged parameter from multiple templates
- `PathParameterOptions` — Options for PATH parameter resolution
- `FormatString`, `SymbolTable` — Re-exported from `openjd-expr`
- `format_string`, `symbol_table` — Modules re-exported wholesale from `openjd-expr`
- From `types` module: `DataFlow`, `EndOfLine`, `Extensions`, `FileType`,
  `JobParameterInputValues`, `JobParameterType`, `JobParameterValue`, `JobParameterValues`,
  `KnownExtension`, `ObjectType`, `SpecificationRevision`, `TaskParameterSet`,
  `TaskParameterType`, `TaskParameterValue`, `TemplateSpecificationVersion`,
  `ValidationContext`

**Error types:**
- `ModelError` — Primary error enum

## Key Dependencies

| Dependency | Purpose |
|------------|--------|
| `openjd-expr` | Expression evaluation, format strings, symbol tables, ExprValue/ExprType |
| `serde` + `serde_yaml` + `serde_json` | YAML/JSON deserialization with `deny_unknown_fields` |
| `indexmap` | Insertion-ordered maps for deterministic output |
| `thiserror` | Ergonomic error type derivation |
| `regex` | Capability name validation, let binding self-reference detection |

## Design Decisions

### Post-Deserialization Validation (vs Pydantic)

The Python library uses Pydantic model validators that run during deserialization. Rust's serde
doesn't support this pattern well — serde deserializers are stateless and can't accumulate
multiple errors. Instead, the Rust crate:

1. Deserializes with serde (catching structural errors like missing fields, wrong types)
2. Runs a multi-pass validation pipeline on the deserialized structs

This separation has advantages: validation passes can be ordered by dependency (limits before
structure before format strings), each pass has access to the full template tree, and errors
from all passes are accumulated into a single `ValidationErrors` collection.

### Extension-Aware Validation via Context

Rather than branching on extension names throughout the code, the crate computes
`EffectiveLimits` and `EffectiveRules` from the `ValidationContext` at the start of validation.
All subsequent checks reference these computed values, so extension effects are centralized
in `types.rs` and the validation code itself is extension-agnostic.

For example, `FEATURE_BUNDLE_1` raises `max_identifier_len` from 64 to 512 and
`max_param_count` from 50 to 200. The limits pass just checks against `limits.max_identifier_len`
without knowing which extension set that value.

### Explicit Type Conversion (vs Generic Traversal)

The Python library uses `instantiate_model()` which generically traverses Pydantic models,
finding `FormatString` fields via metadata and resolving them. The Rust crate instead has
explicit `instantiate()` or conversion methods on each type. This is more verbose but:

- Makes the template-scope vs session-scope distinction explicit in code
- Allows the compiler to verify all fields are handled
- Avoids runtime reflection or trait object overhead
- Makes it clear which fields are resolved at which phase

### Pydantic-Compatible Error Paths

Despite not using Pydantic, the Rust crate formats validation errors to match Pydantic's
output format (e.g., `steps[0] -> script -> actions -> onRun -> command`). This ensures
error messages are consistent between the Python and Rust implementations, which matters
for users and tooling that parse error output.

### Re-exports from openjd-expr

`FormatString` and `SymbolTable` are re-exported because they appear in the public API
(in `job::*` types and function signatures). The `format_string` and `symbol_table` modules
are also re-exported wholesale so consumers of `openjd-model` don't need to depend on
`openjd-expr` directly for common operations.
