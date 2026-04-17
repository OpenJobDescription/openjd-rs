# openjd-expr Crate Architecture

## Role in the Workspace

`openjd-expr` is the foundation crate in the openjd-rs workspace. It implements the
OpenJD Expression Language (the EXPR extension from RFC 0005) and the format string
interpolation mechanism from the base specification (§7.3).

```
openjd-cli
  ├── openjd-model
  │     └── openjd-expr    ← this crate
  └── openjd-sessions
        ├── openjd-model
        └── openjd-expr
```

Every other crate depends on `openjd-expr` either directly or transitively. This means
the expr crate must have zero dependencies on the model or sessions crates.

## Why FormatString Lives Here

Format strings (`{{Param.Name}}`, `{{Param.X + 3}}`) are the entry point to expression
evaluation. The `{{...}}` syntax dispatches to either simple dotted-name lookup (base spec)
or full expression evaluation (EXPR extension). Placing `FormatString` in `openjd-expr`
rather than `openjd-model` keeps the evaluation pipeline in one crate and avoids a circular
dependency — the model crate uses `FormatString` in its serde types via re-export.

The Python implementation splits this across `openjd.model._format_strings` and
`openjd.expr`, requiring cross-package imports. The Rust design is cleaner.

## Module Layout

```
src/
├── lib.rs                  Public API re-exports
├── types.rs                ExprType, TypeCode, signature matching
├── value.rs                ExprValue (typed list variants, Float64)
├── symbol_table.rs         Hierarchical SymbolTable
├── format_string.rs        FormatString parsing and resolution
├── range_expr.rs           RangeExpr parsing and indexing
├── path_mapping.rs         PathFormat, PathMappingRule
├── uri_path.rs             URI-aware path operations
├── error.rs                ExpressionError with caret formatting
├── edit_distance.rs        Levenshtein distance for "did you mean?" suggestions (see edit-distance.md)
├── default_library.rs      Default FunctionLibrary construction
├── function_library.rs     FunctionLibrary, FunctionEntry, dispatch
├── eval/
│   ├── mod.rs              evaluate_expression(), evaluate_expression_bounded()
│   ├── parse.rs            ruff_python_parser integration, AST validation
│   └── evaluator.rs        AST-walking Evaluator with resource bounds
└── functions/
    ├── mod.rs              Sub-library re-exports
    ├── arithmetic.rs       +, -, *, /, //, %, **, unary +/-
    ├── string.rs           String methods and operations
    ├── path.rs             Path properties and methods
    ├── path_parse.rs       Format-aware path parsing (sep, split, parts, etc.)
    ├── regex.rs            re_match, re_search, re_findall, re_sub, re_escape, re_split
    ├── repr.rs             repr_sh, repr_cmd, repr_pwsh, repr_py, repr_json
    ├── list.rs             List operations (sorted, reversed, unique, flatten, etc.)
    ├── math.rs             abs, min, max, sum, floor, ceil, round
    ├── misc.rs             len, fail, range, any, all
    └── conversion.rs       string(), int(), float(), bool(), path(), range_expr()
```

## Public API Surface

### Entry Points

```rust
/// Simplest evaluation — host-native path format, default library, default limits.
pub fn evaluate_expression(expr: &str, symtab: &SymbolTable) -> Result<ExprValue, ExpressionError>;

/// Evaluation with explicit resource limits, returns metrics.
pub fn evaluate_expression_bounded(
    expr: &str, symtab: &SymbolTable,
    memory_limit: usize, operation_limit: usize,
) -> Result<EvaluationResult, ExpressionError>;
```

For more control, parse once and evaluate with a builder:

```rust
let mut parsed = ParsedExpression::new("Param.Frame * 2 + 1")?;

// Inspect parse metadata
parsed.accessed_symbols  // {"Param.Frame"}
parsed.called_functions  // {}
parsed.local_bindings    // {}

// After evaluation, resource usage is recorded on the ParsedExpression:
parsed.peak_memory_usage  // peak bytes during the last evaluate() call
parsed.operation_count    // operations consumed during the last evaluate() call

// Simple evaluation
let value = parsed.evaluate(&symtab)?;

// Configured evaluation — custom library, limits, path format, path mapping
let value = parsed.evaluator(&[&job_params, &let_bindings])
    .with_library(&custom_lib)
    .with_memory_limit(50_000_000)
    .with_operation_limit(1_000_000)
    .with_path_format(PathFormat::Posix)
    .with_path_mapping_rules(&rules)
    .evaluate(&parsed.ast)?;
```

`ParsedExpression::new` parses once and exposes symbol/function metadata for validation.
The `evaluator()` builder covers the use cases that the Python implementation handles
via optional keyword arguments on `ParsedExpression.evaluate()` — library, limits, path
format, and path mapping rules are all configurable per-evaluation without re-parsing.

The `accessed_symbols` set also enables dependency analysis between expressions. By
comparing which symbols one expression writes (via let bindings or parameter definitions)
against which symbols another expression reads, callers can build a dependency graph
across a collection of expressions. This is the foundation for features like
[topological evaluation ordering](https://github.com/OpenJobDescription/openjd-specifications/discussions/42)
and further extensions such as incremental re-evaluation when a subset of inputs change.

### Core Types

| Type | Purpose |
|------|---------|
| `ExprType` | Type system — type codes, unions, generics, matching |
| `ExprValue` | Runtime values — typed list variants, coercion |
| `Float64` | f64 wrapper preserving original string representation |
| `SymbolTable` | Hierarchical variable bindings with dotted paths |
| `ParsedExpression` | Parsed AST with metadata, builder for evaluation |
| `Evaluator` | Builder-pattern evaluator with configurable limits |
| `EvaluationResult` | Value + peak_memory + operation_count |
| `FormatString` | `{{...}}` interpolation with serde integration |
| `FunctionLibrary` | Signature-based multiple dispatch registry |
| `FunctionEntry` | Single overload: signature + fn pointer |
| `EvalContext` | Trait for function impls to access evaluator state |
| `PathFormat` | Posix / Windows / Uri |
| `PathMappingRule` | Source→destination path transformation |
| `RangeExpr` | Parsed integer range expression |
| `ExpressionError` | Error with caret-annotated source display |

### Constants

```rust
pub const DEFAULT_MEMORY_LIMIT: usize = 100_000_000;   // 100 MB
pub const DEFAULT_OPERATION_LIMIT: usize = 10_000_000;  // 10M ops
```

## Key Dependencies

| Crate | Purpose | Source |
|-------|---------|--------|
| `ruff_python_parser` | Python expression parsing | Git (astral-sh/ruff, pinned commit) |
| `ruff_python_ast` | Python AST types | Git (same pin) |
| `regex` | Regular expression evaluation | crates.io |
| `xxhash-rust` | Fast hashing (internal) | crates.io |
| `serde` | Serialization (FormatString, PathMappingRule) | crates.io |

The ruff dependency is a git pin rather than a crates.io dependency because the ruff
team does not publish their parser crate. See [parser.md](parser.md) for the rationale
behind choosing ruff over rustpython-parser.

## Design Constraints (from the specification)

1. **No filesystem or network access** — expressions are pure computations
2. **Memory-bounded** — configurable limit, tracked per-value
3. **Operation-bounded** — configurable limit, proportional to work done
4. **Deterministic** — same inputs always produce same outputs
5. **No user-defined functions** — only the built-in library
6. **Backward compatible** — base spec format strings work unchanged
7. **Fail-fast errors** — type mismatches caught at validation time via unresolved values
8. **Reuse existing Python parsers** — ruff_python_parser for Rust
