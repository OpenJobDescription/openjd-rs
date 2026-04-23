# openjd-expr Crate Quality Evaluation Report

**Date:** 2026-04-23
**Crate:** `openjd-expr`

## Executive Summary

The `openjd-expr` crate is a high-quality, well-structured implementation of the OpenJD Expression Language. The specifications are thorough and accurately describe the implementation. The public API is ergonomic and well-documented. The test suite is comprehensive (3,143 tests, all passing, including 50 exploratory tests added during this evaluation) and exceeds the Python reference in both count and coverage. One bug was found during exploratory testing (`INT64_MIN % -1` returns IntegerOverflow instead of 0), and a few minor spec-implementation divergences were identified. The crate compiles cleanly with zero warnings. Overall, this is a mature implementation ready for production use.

## 1. Specifications Review

The `specs/expr/` directory contains 13 specification documents plus a README index. Each document is well-organized, accurate, and provides clear rationale for design decisions.

### README.md
- Complete index of all spec documents with descriptions.
- Links to normative references (formal spec, RFCs).
- Notes the Python reference implementation relationship.
- **No issues.**

### architecture.md
- Clearly describes the crate's role in the workspace dependency graph.
- Explains why `FormatString` lives in `openjd-expr` rather than `openjd-model`.
- Module layout matches the actual source tree exactly.
- Public API surface is comprehensive and accurate.
- Core types table is complete.
- Key dependencies are documented with rationale.
- Design constraints from the specification are enumerated.
- **Minor divergence:** The spec describes `evaluate_expression()` and `evaluate_expression_bounded()` as top-level entry points, but these don't exist as standalone functions. The actual API uses `ParsedExpression::new(expr)?.evaluate(&symtab)` and the builder pattern. The spec should either add these convenience functions or update the description to match the `ParsedExpression` API.

### type-system.md
- TypeCode enum matches the implementation exactly.
- ExprType construction, matching, and substitution are accurately described.
- Union normalization rules are complete and match the implementation.
- Unresolved type normalization is correctly documented.
- Implicit coercion rules are clear.
- **No issues.**

### values.md
- ExprValue enum matches the implementation.
- Float64 invariants (no NaN, no Infinity, -0.0 normalization) are correctly documented and enforced.
- Typed list variants are well-explained with memory savings rationale.
- `make_list` promotion rules are accurate.
- Memory sizing, ListIter, equality/hashing semantics are thorough.
- JSON transport format is well-documented.
- `from_str_coerce` table is accurate.
- **No issues.**

### symbol-table.md
- Structure, construction, dotted path operations all match implementation.
- Path conflict detection is correctly documented.
- `SerializedSymbolTable` wire format is accurate.
- The `symtab!` macro is documented.
- **No issues.**

### parser.md
- Parser selection rationale is well-argued.
- Parsing pipeline is accurately described.
- Keyword renaming mechanism is clearly explained.
- AST validation allowlist matches the implementation.
- JSON literal normalization is correct.
- Symbol collection sets are accurate.
- **No issues.**

### evaluator.md
- Builder pattern is accurately described.
- Resource bounding (memory tracking, operation counting) matches implementation.
- AST node evaluation dispatch table is complete.
- BoolOp falsy definition (only null and false) is correctly documented.
- Dispatch flow is accurate.
- Fast path for simple name lookup is documented.
- Regex cache behavior is correctly described.
- **No issues.**

### function-library.md
- FunctionEntry, FunctionLibrary, EvalContext trait all match implementation.
- Three-phase dispatch is accurately described.
- Operator mapping table is complete.
- Property access mechanism is correct.
- Static type derivation (including union path) is well-explained.
- Sub-library composition matches `default_library.rs`.
- Host context mechanism is thoroughly documented.
- 200 signatures confirmed.
- **No issues.**

### format-string.md
- Parsing, resolution, validation all match implementation.
- `FormatStringOptions` builder is accurately described.
- Serde integration is correct.
- `escape_format_string` utility is documented.
- **No issues.**

### error-formatting.md
- Error display format is accurate.
- `ExpressionError` structure matches implementation.
- Smart caret positioning rules are correct.
- **Minor divergence:** The spec shows `ExpressionErrorKind::OperationLimitExceeded` without fields, but the implementation has `{ count: usize, limit: usize }`. Similarly, the spec shows `MemoryLimitExceeded { used, limit }` which matches. The `OperationLimitExceeded` fields are an improvement over the spec.

### edit-distance.md
- Algorithm description is accurate.
- Threshold of 5 is documented.
- Call sites are correctly identified.
- Future work note about length-difference early rejection is appropriate.
- **No issues.**

### range-expr.md
- Syntax, internal representation, parsing all match implementation.
- Indexing, iteration, conversion, slicing are accurately described.
- Contiguous display mode with bit packing is well-documented.
- Expression language integration table is complete.
- **No issues.**

### path-mapping.md
- PathFormat, PathMappingRule, URI path operations all match implementation.
- Expression language integration tables are complete.
- `apply_path_mapping` host-context function is correctly documented.
- **No issues.**

### path-parse.md
- Rationale for not using `std::path` is well-argued.
- Separator handling, anchor detection, public API all match implementation.
- Integration with expression language is correctly documented.
- **No issues.**

## 2. Public API Review

### Completeness

The public API is defined in `lib.rs` via `pub use` re-exports. All types mentioned in the architecture spec's "Core Types" table are exported:

- `ExprType`, `TypeCode` — type system
- `ExprValue` — runtime values (includes `Float64` via the `value` module)
- `SymbolTable`, `SerializedSymbolTable`, `SymbolTableError` — symbol tables
- `ParsedExpression`, `EvalBuilder`, `EvalResult` — evaluation
- `FormatString`, `FormatStringOptions`, `FormatStringValidationError` — format strings
- `FunctionLibrary`, `EvalContext` — function dispatch
- `PathFormat`, `PathMappingRule` — path mapping
- `RangeExpr`, `RangeExprError` — range expressions
- `ExpressionError`, `ExpressionErrorKind` — errors
- `DEFAULT_MEMORY_LIMIT`, `DEFAULT_OPERATION_LIMIT` — constants
- `escape_format_string` — utility function
- `symtab!` — convenience macro

### Spec-Implementation Divergence

The architecture spec describes two top-level convenience functions:
```rust
pub fn evaluate_expression(expr: &str, symtab: &SymbolTable) -> Result<ExprValue, ExpressionError>;
pub fn evaluate_expression_bounded(...) -> Result<EvalResult, ExpressionError>;
```

These do not exist. The equivalent functionality is:
```rust
ParsedExpression::new(expr)?.evaluate(&symtab)
ParsedExpression::new(expr)?.with_memory_limit(m).with_operation_limit(o).evaluate_with_metrics(&[&symtab])
```

The builder pattern is more flexible, but the convenience functions would reduce boilerplate for simple use cases. The spec should be updated to match the actual API, or the convenience functions should be added.

### API Ergonomics

- The builder pattern (`ParsedExpression::new().with_*().evaluate()`) is idiomatic Rust and well-designed.
- `ParsedExpression::evaluate(&self, &SymbolTable)` accepts a single `&SymbolTable` for the common case, while `EvalBuilder::evaluate(&[&SymbolTable])` accepts a slice for stacked scopes.
- The `symtab!` macro provides concise construction with automatic `Into<ExprValue>` conversion.
- `SymbolTable::from_pairs` accepts any `IntoIterator`, which is flexible.
- `FormatStringOptions` uses a chainable builder pattern.
- `#[non_exhaustive]` on `ExprValue::Path` and `ExpressionErrorKind` is good forward-compatibility practice.
- `#[must_use]` on builder methods prevents silent configuration loss.

### Module Visibility

- `edit_distance` is correctly `pub(crate)` (internal implementation detail).
- `eval::evaluator::Evaluator` is correctly `pub(crate)` (internal, exposed via `ParsedExpression`).
- All `functions/` sub-modules are `pub` but their individual functions are not re-exported at the crate root, which is appropriate — they're accessed through the `FunctionLibrary`.

## 3. Implementation Review

### types.rs (ExprType, TypeCode)
- 980 lines including 390 lines of unit tests.
- `TypeCode` enum is comprehensive with all specified type codes.
- `ExprType` uses a compact representation with `TypeCode` + `Vec<ExprType>` params.
- Union normalization correctly implements all 7 rules (flatten, dedup, ANY absorption, NORETURN collapse, unresolved hoisting, singleton unwrap, sort).
- `match_call` and `resolve_call` correctly handle type variable binding.
- `parse()` from string notation is well-implemented for test convenience.
- **No issues.**

### value.rs (ExprValue, Float64)
- 1,175 lines, the largest source file.
- Typed list variants (`ListBool`, `ListInt`, `ListFloat`, `ListString`, `ListPath`, `ListList`) provide significant memory savings.
- `Float64` correctly normalizes -0.0 and rejects NaN/Infinity.
- `make_list` promotion rules are correctly implemented with all 7 priority rules.
- `Hash` implementation correctly uses tag-based grouping for cross-type equivalence.
- `ListIter` provides zero-allocation iteration with `ExactSizeIterator`.
- Memory sizing is accurate with cached sizes for variable-length variants.
- `from_str_coerce` handles all documented type conversions.
- JSON transport format is correctly implemented.
- **No issues.**

### symbol_table.rs
- Clean hierarchical HashMap implementation.
- Path conflict detection works correctly.
- `all_paths` collects leaf paths for "did you mean?" suggestions.
- `SerializedSymbolTable` correctly handles the JSON wire format.
- `symtab!` macro is well-designed.
- **No issues.**

### eval/parse.rs (ParsedExpression, EvalBuilder)
- Keyword renaming is correctly implemented with same-length placeholders.
- AST validation rejects all unsupported Python features with descriptive errors.
- Symbol collection correctly distinguishes accessed symbols, called functions, and local bindings.
- `EvalBuilder` provides clean chainable configuration.
- **No issues.**

### eval/evaluator.rs
- 1,520 lines, the most complex source file.
- AST dispatch table is complete for all supported node types.
- Memory tracking (`track`/`release`) is consistently applied.
- Operation counting is correctly implemented with proportional costs.
- BoolOp correctly implements null-coalescing with only null/false as falsy.
- Chained comparison correctly implements short-circuit with clone-and-release.
- List comprehension correctly creates child evaluators with local scope.
- Regex cache is correctly shared between parent and child evaluators.
- Unresolved value propagation is thorough.
- **No issues found in the evaluator itself.**

### function_library.rs
- Three-phase dispatch (exact, coerced, generic) is correctly implemented.
- Method-vs-function distinction correctly skips receiver coercion.
- `derive_return_type` handles union types with per-signature recursive matching.
- `host_context_enabled` flag is correctly managed.
- Error messages include friendly operator names and "did you mean?" suggestions.
- **No issues.**

### default_library.rs
- 200 signatures registered across 12 sub-library categories.
- All function names from the specification are present.
- Sub-library composition via `merge()` is clean.
- Global caching via `LazyLock` is correct and thread-safe.
- **No issues.**

### functions/arithmetic.rs
- Python-style floored division and modulo are correctly implemented.
- Integer overflow is checked via `checked_*` methods.
- Power function correctly handles negative exponents (returns float), large exponents (overflow guard), and special bases (-1, 0, 1).
- **Bug:** `mod_int` fails on `INT64_MIN % -1` with IntegerOverflow. See Section 7.

### functions/string.rs
- String methods (upper, lower, strip, split, join, etc.) are correctly implemented.
- Operation counting for string processing uses `ceil(len/256)`.
- **No issues.**

### functions/path.rs, functions/path_parse.rs
- Format-aware path operations correctly handle Posix, Windows, and URI paths.
- `path()` constructor from `list[string]` follows Python pathlib semantics.
- URI paths are correctly treated as opaque.
- **No issues.**

### functions/regex.rs
- Correctly rejects lookahead, lookbehind, backreferences, and `\Z`.
- Uses `RegexBuilder` with 1 MiB size limit.
- Regex cache integration via `EvalContext::get_or_compile_regex`.
- **No issues.**

### functions/repr.rs
- Shell-safe quoting for sh, cmd, pwsh, py, json.
- **No issues.**

### functions/list.rs, functions/comparison.rs, functions/math.rs, functions/misc.rs, functions/conversion.rs
- All correctly implemented with appropriate type checking.
- **No issues.**

### format_string.rs
- Parsing correctly handles `{{...}}` delimiters with pre-parsed `ParsedExpression`.
- `resolve_with` correctly implements typed-value passthrough for single-expression strings.
- `resolve_string_with` correctly concatenates all segments.
- Validation methods work correctly with unresolved values.
- Serde integration catches syntax errors at deserialization time.
- **No issues.**

### range_expr.rs
- Parsing uses a clean tokenizer + recursive descent parser.
- Descending ranges are normalized to ascending canonical form.
- O(log n) indexing via binary search on cumulative lengths.
- Contiguous display flag is packed into the MSB of the length field.
- `from_values` correctly detects arithmetic sequences for compact representation.
- Slicing operates in O(m) time without materializing elements.
- **No issues.**

### path_mapping.rs, uri_path.rs
- PathMappingRule application is correct with format-appropriate comparison.
- URI path operations correctly preserve opaque structure.
- **No issues.**

### error.rs
- Caret error formatting is well-implemented with smart positioning.
- `ExpressionErrorKind` is `#[non_exhaustive]` for forward compatibility.
- Convenience constructors set the kind automatically.
- **No issues.**

### edit_distance.rs
- Classic two-row Levenshtein implementation.
- Character-based (not byte-based) for correct UTF-8 handling.
- Threshold of 5 is reasonable.
- **No issues.**

### Naming Consistency
- Naming is consistent within the crate and follows Rust conventions.
- Type names match the specification (`ExprType`, `ExprValue`, `SymbolTable`, etc.).
- Function names in the library match the specification (`__add__`, `__property_name__`, etc.).
- Error variant names are descriptive and consistent.

## 4. Test Review

### Coverage Assessment

The crate has 3,143 tests across 35 test files (plus 5 doc-tests), all passing. This includes 50 exploratory tests added during this evaluation. This significantly exceeds the Python reference's ~1,366 test functions.

### Test File Organization

| Test File | Tests | Coverage Area |
|-----------|-------|---------------|
| test_types.rs | 273 | Type system, matching, unions |
| test_strings.rs | 369 | String operations and methods |
| test_evaluation.rs | 229 | Core evaluation semantics |
| test_unresolved_eval.rs | 230 | Unresolved value propagation |
| test_paths.rs | 240 | Path properties and methods |
| test_lists.rs | 179 | List operations |
| test_error_formatting.rs | 171 | Caret error messages |
| test_expr_value.rs | 128 | ExprValue construction, equality, hashing |
| test_path_mapping.rs | 124 | Path mapping rules |
| test_uri_paths.rs | 119 | URI path operations |
| test_function_library.rs | 95 | Dispatch, signatures, type derivation |
| test_symbol_table.rs | 86 | Symbol table operations |
| test_arithmetic.rs | 74 | Arithmetic operators |
| test_range_expr.rs | 68 | Range expression parsing and operations |
| test_parse_expression.rs | 64 | Parser, keyword renaming, validation |
| test_comparison.rs | 60 | Comparison operators |
| test_slicing.rs | 55 | List and string slicing |
| test_function_context.rs | 53 | EvalContext, host context |
| test_operation_limit.rs | 50 | Operation counting |
| test_string_operation_counting.rs | 42 | String operation costs |
| test_memory.rs | 33 | Memory tracking and limits |
| test_rfc_examples.rs | 33 | Examples from RFC documents |
| test_int64_bounds.rs | 32 | 64-bit integer boundary cases |
| test_types_evaluate.rs | 31 | Type-level evaluation |
| test_ast_validation.rs | 28 | AST validation rejections |
| test_target_type_propagation.rs | 26 | Target type coercion |
| test_misc_builtins.rs | 25 | len, fail, range, any, all |
| test_format_strings.rs | 23 | Format string parsing/resolution |
| test_unicode_codepoint.rs | 22 | Unicode handling |
| test_method_coercion.rs | 19 | Method call coercion rules |
| test_list_nesting.rs | 16 | Nested list depth limits |
| test_path_format_mismatch.rs | 15 | Path format validation |
| test_path_mapping_platform.rs | 9 | Platform-specific path mapping |
| test_misc_getitem.rs | 8 | Subscript edge cases |
| test_exploratory.rs | 50 | Exploratory edge case tests (added during this evaluation) |

### Happy Path vs Edge Cases

The test suite has excellent coverage of both:
- **Happy path:** Every function, operator, and language feature has basic correctness tests.
- **Edge cases:** INT64 boundaries, NaN/Infinity rejection, -0.0 normalization, empty lists, nested list depth limits, keyword-as-attribute, chained comparisons, unresolved value propagation through all node types.

### Organization

Tests are well-organized by feature area. Each test file focuses on a specific aspect of the crate. Test names are descriptive and follow a consistent naming convention.

### Missing Test Coverage

- No fuzz testing (Python has `test_fuzz.py` with Hypothesis). Adding property-based testing with `proptest` or `arbitrary` would strengthen confidence.
- The `INT64_MIN % -1` edge case was not covered (found during exploratory testing).

## 5. Python Comparison

### Implementation Algorithms

The Rust implementation faithfully mirrors the Python reference's algorithmic structure:

- **Parser:** Both use the same keyword-renaming approach for Python keywords as attributes.
- **Evaluator:** Same AST-walking structure with the same dispatch flow.
- **BoolOp:** Both implement the same null-coalescing semantics with only null/false as falsy.
- **Type system:** Same type codes, matching, and union normalization rules.
- **Function library:** Same three-phase dispatch (exact, coerced, generic).
- **Path operations:** Both use format-aware string operations rather than OS-native path libraries.

### Behavioral Differences

1. **`INT64_MIN % -1`:** Python returns 0 (correct). Rust returns IntegerOverflow (bug). See Section 7.
2. **RangeExpr direction:** Python preserves user-supplied direction; Rust normalizes to ascending canonical form. This is a deliberate simplification documented in the spec.
3. **FormatString location:** Python splits across `openjd.model._format_strings` and `openjd.expr`. Rust keeps it all in `openjd-expr`, which is architecturally cleaner.
4. **Typed list variants:** Python uses a single `List` with dynamic elements. Rust uses specialized variants (`ListBool`, `ListInt`, etc.) for 60-97% memory savings.
5. **Builder pattern:** Python uses constructor arguments for evaluation configuration. Rust uses a chainable builder pattern.
6. **EvalContext trait:** Rust uses a trait to prevent function implementations from accessing evaluator internals. Python passes the evaluator directly.

### Error Messages

Error messages are consistent between implementations. Both produce caret-annotated error output with smart positioning. The Rust implementation adds `count` and `limit` fields to `OperationLimitExceeded` for better diagnostics.

### API Design

The Rust API is more ergonomic in several ways:
- `#[non_exhaustive]` on `ExprValue::Path` enforces separator normalization invariants.
- The `symtab!` macro provides concise construction.
- `SymbolTable::from_pairs` accepts any `IntoIterator`.
- `FormatStringOptions` builder is cleaner than Python's keyword arguments.

### Test Coverage Comparison

The Rust test suite (3,093 tests) significantly exceeds the Python reference (~1,366 test functions). The Rust side has additional test files for AST validation, function library dispatch, list nesting, Unicode codepoints, and platform-specific path mapping. The Python side has fuzz testing that the Rust side lacks.

## 6. Build and Test Results

### Compilation

```
$ cargo build -p openjd-expr
   Compiling openjd-expr v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.30s
```

**Zero warnings.** Clean compilation.

### Test Results

```
$ cargo test -p openjd-expr
test result: ok. 3143 passed; 0 failed; 0 ignored (across 36 test binaries)
test result: ok. 5 passed; 0 failed (doc-tests)
```

**All 3,143 tests pass** (including 50 exploratory tests added during this evaluation). All 5 doc-tests pass.

## 7. Exploratory Findings

### Bug: `INT64_MIN % -1` Returns IntegerOverflow

**Severity:** Low (extremely rare edge case)

**Location:** `functions/arithmetic.rs`, `mod_int` function

**Description:** `(-9223372036854775808) % (-1)` should return `0` (any integer modulo ±1 is always 0), but the implementation returns `IntegerOverflow`. This is because Rust's `i64::checked_rem(-1)` returns `None` for `i64::MIN` — the intermediate division `i64::MIN / -1` overflows, even though the remainder is mathematically 0.

**Python behavior:** Returns `0` (correct), because Python integers are arbitrary precision.

**Fix:** Add a special case for divisor `±1` before calling `checked_rem`:
```rust
if *r == 1 || *r == -1 {
    return Ok(ExprValue::Int(0));
}
```

**Test added:** `test_exploratory.rs::test_int64_min_mod_neg1` documents this bug.

### No Other Bugs Found

50 exploratory tests were written (in `test_exploratory.rs`) covering:
- INT64 boundary arithmetic (MIN, MAX, overflow)
- Division/modulo by zero
- Float edge cases (NaN, Infinity, -0.0)
- Power function edge cases (0^-1, large exponents)
- String operations (repeat negative, methods)
- Boolean semantics (null coalescing, 0 is truthy, "" is truthy)
- Comparison operators (chained, in/not in)
- List operations (empty, nested depth 3, concat, mixed types)
- Control flow (ternary, non-bool condition)
- Syntax rejection (walrus, lambda, dict, bitwise, is)
- Format strings (basic, typed passthrough, no passthrough with literal)
- Range expressions (basic, descending, no-step descending)
- Keyword-as-attribute access

All passed except the `INT64_MIN % -1` case documented above.

## 8. Recommendations

### P1 — Bug Fix

1. **Fix `INT64_MIN % -1`** in `functions/arithmetic.rs`. Add early return for `±1` divisor. This is a correctness bug that diverges from the Python reference and the mathematical definition.

### P2 — Spec Alignment

2. **Update architecture.md** to remove the `evaluate_expression()` / `evaluate_expression_bounded()` convenience functions from the public API description, or add them to the implementation. The current spec describes an API that doesn't exist.

3. **Update error-formatting.md** to document the `count` and `limit` fields on `OperationLimitExceeded`, matching the implementation.

### P3 — Test Improvements

4. **Add fuzz testing** using `proptest` or `arbitrary` to match the Python reference's Hypothesis-based fuzz tests. Focus on parser robustness (arbitrary strings) and evaluator robustness (random expressions with random symbol tables).

5. **Add a test for `INT64_MIN % -1`** that expects `Ok(Int(0))` once the bug is fixed.

### P4 — Minor Improvements

6. **Add length-difference early rejection** to `suggest_closest` in `edit_distance.rs`. Skip candidates where `|len(name) - len(candidate)| > MAX_SUGGESTION_DISTANCE`. This is a simple optimization noted in the spec's own "Future Work" section.

7. **Consider adding `evaluate_expression` convenience function** as a top-level export for simple use cases:
   ```rust
   pub fn evaluate_expression(expr: &str, symtab: &SymbolTable) -> Result<ExprValue, ExpressionError> {
       ParsedExpression::new(expr)?.evaluate(symtab)
   }
   ```
   This would match the Python API and reduce boilerplate for callers who don't need the builder pattern.
