# openjd-expr Crate Specifications

Design specifications for the `openjd-expr` crate — the OpenJD Expression Language
implementation in Rust.

## Specification Documents

| Document | Description |
|----------|-------------|
| [architecture.md](architecture.md) | Crate structure, module layout, dependency graph, public API surface |
| [public-api.md](public-api.md) | Authoritative reference for every public type, function, trait, and constant |
| [type-system.md](type-system.md) | ExprType, TypeCode, type matching, union normalization, type variables |
| [values.md](values.md) | ExprValue, typed list variants, Float64, memory sizing, coercion |
| [symbol-table.md](symbol-table.md) | Hierarchical symbol table with dotted path lookup |
| [parser.md](parser.md) | ruff_python_parser integration, AST validation, keyword renaming |
| [evaluator.md](evaluator.md) | AST-walking evaluator, resource bounding, dispatch flow |
| [function-library.md](function-library.md) | FunctionLibrary, signature dispatch, sub-library composition |
| [format-string.md](format-string.md) | FormatString parsing, resolution, serde integration |
| [error-formatting.md](error-formatting.md) | Caret error messages with smart positioning |
| [edit-distance.md](edit-distance.md) | Levenshtein "did you mean?" suggestions |
| [range-expr.md](range-expr.md) | RangeExpr parsing, indexing, iteration |
| [path-mapping.md](path-mapping.md) | PathFormat, PathMappingRule, URI path handling |
| [path-parse.md](path-parse.md) | Format-aware path parsing for PATH properties/methods |

## Normative References

- [2026-02-Expression-Language.md](../../../openjd-specifications/wiki/2026-02-Expression-Language.md) — Formal language specification
- [RFC 0005 — Expression Language](../../../openjd-specifications/rfcs/0005-expression-language.md) — Language design and rationale
- [RFC 0006 — Expression Function Library](../../../openjd-specifications/rfcs/0006-expression-function-library.md) — Operators and built-in functions
- [RFC 0007 — Extended Parameter Types](../../../openjd-specifications/rfcs/0007-extend-parameter-types.md) — BOOL, LIST[T], RANGE_EXPR parameter types

## Python Reference Implementation

The Rust crate mirrors the design of `openjd-model-for-python/src/openjd/expr/`, with
adaptations for Rust's type system and performance characteristics. Key divergences are
noted in each spec document.
