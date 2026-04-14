# openjd-expr Crate Quality Evaluation Report

**Date:** 2026-04-13
**Evaluator:** AI-assisted review
**Crate version:** 0.1.0
**Scope:** Specifications, implementation source, and tests

---

## Executive Summary

The `openjd-expr` crate is a well-engineered Rust implementation of the OpenJD Expression
Language. It has 2,819 tests across 34 test binaries, all passing with zero warnings.
The specifications are comprehensive and mostly aligned with the implementation, though
several areas of spec drift were identified. One confirmed bug was found (symbol table
conflict detection), along with two confirmed spec violations (URI case sensitivity,
operation limit error types) and one performance issue (format string re-parsing).

**Overall assessment:** High quality, with targeted improvements needed.

---

## 1. Build and Test Results

- **Build:** Compiles cleanly with `cargo build --package openjd-expr` — zero errors, zero warnings.
- **Tests:** 2,819 tests across 34 test binaries — all pass.
  - 119 unit tests (in-crate `#[cfg(test)]` modules)
  - 2,696 integration tests (32 files in `tests/`)
  - 4 doc-tests

---

## 2. Specification Review

### 2.1 Specification Documents

The `specs/expr/` directory contains 11 well-structured documents:

| Document | Size | Quality |
|----------|------|---------|
| README.md | Overview | ✅ Good — clear index with normative references |
| architecture.md | 7.6KB | ✅ Good — accurate module layout, dependency graph, public API |
| parser.md | 5.4KB | ✅ Good — faithful description of parsing pipeline |
| type-system.md | 5.3KB | ✅ Good — updated, non-existent constants removed |
| values.md | 6.2KB | ✅ Updated — enum definition, hint_type, promotion rules, coercions |
| symbol-table.md | 3.4KB | ✅ Good — minor naming discrepancy |
| evaluator.md | 10.9KB | ✅ Good — thorough evaluator description |
| function-library.md | 8.4KB | ✅ Updated — call() signature, EvalContext return types, regex cache |
| format-string.md | 4.1KB | ✅ Good — pre-parsing now implemented to match spec |
| range-expr.md | 4.9KB | ✅ Updated — descending ranges require negative step |
| path-mapping.md | 5.2KB | ✅ Good — minor return type difference |
| error-formatting.md | 3.8KB | ✅ Good — accurate description |

### 2.2 Spec-Implementation Discrepancies

#### Critical

| # | Area | Issue | Status |
|---|------|-------|--------|
| S1 | format-string.md | Spec says expressions are "pre-parsed `ParsedExpression` objects, avoiding re-parsing on each resolution call" but the implementation stores expressions as strings and re-parses on every `resolve` call. | ✅ Fixed — implementation now pre-parses at `FormatString::new()` time |
| S2 | range-expr.md | Spec shows `"10-1"` as valid (producing `[10,9,...,1]`) but code rejects descending ranges without a negative step. Code comment says "invalid per spec" — contradicts the spec document. | ✅ Fixed — spec updated: descending ranges require negative step |
| S3 | function-library.md | Spec says `library.call(name, &arg_types, &args, eval_context)` but implementation is `library.call(name, &args, ctx)` — arg types are derived internally. | ✅ Fixed — spec updated to match implementation |

#### Medium

| # | Area | Issue | Status |
|---|------|-------|--------|
| S4 | values.md | Enum definition missing `usize` cached memory size fields on `ListString`, `ListPath`, `ListList`, and `ExprType` field on `ListList`. | ✅ Fixed — spec updated with cached size fields |
| S5 | values.md | Spec says empty list defaults to `ListInt`; implementation defaults to `ListString` for null/unknown hints. | ✅ Fixed — both updated: empty lists are `list[nulltype]` |
| S6 | values.md | Spec says `make_list` "validates max 2 nesting levels" but implementation does no depth validation. | ✅ Fixed — `make_list` now validates nesting depth (TDD) |
| S7 | values.md | Spec doesn't document `hint_type` parameter on `make_list` or nested list promotion rules. | ✅ Fixed — spec updated with hint_type and promotion rules |
| S8 | values.md | Spec lists `RANGE_EXPR → STRING` and `RANGE_EXPR → LIST[INT]` target type coercions not present in `coerce()`. | ✅ Fixed — coercions added to implementation |
| S9 | function-library.md | `EvalContext` trait methods return `Result<(), ExpressionError>` but spec shows them returning `()`. | ✅ Fixed — spec updated with correct return types |
| S10 | function-library.md | `EvalContext` has `get_or_compile_regex()` method not mentioned in spec. | ✅ Fixed — documented in spec as internal optimization |
| S11 | type-system.md | Spec defines `LIST_INT`, `LIST_FLOAT`, `LIST_STRING`, `LIST_PATH`, `LIST_BOOL`, `LIST_LIST_INT`, `EMPTY_LIST` constants not present in implementation. | ✅ Fixed — removed from spec (not wanted) |

#### Low

| # | Area | Issue | Status |
|---|------|-------|--------|
| S12 | type-system.md | Spec uses `ExprType::NULLTYPE`, implementation uses `ExprType::NULL`. | ✅ Fixed — implementation renamed to `NULLTYPE` |
| S13 | symbol-table.md | Spec says `SymbolEntry`, code uses `SymbolTableEntry`. | ✅ Fixed — spec updated to `SymbolTableEntry` |
| S14 | values.md | Spec references `Float64::from_str()` constructor; actual is `Float64::with_str()`. | ✅ Fixed — spec updated to `with_str` with correct signature |
| S15 | format-string.md | `copy_used_symtab_values`, `resolve_typed`, `resolve_typed_with_format`, `FormatStringValidationError` not documented. | ✅ Fixed — all four documented in spec |
| S16 | evaluator.md | Regex cache not mentioned in spec. | ✅ Fixed — documented as per-evaluation cache |

---

## 3. Implementation Review

### 3.1 Confirmed Bugs

#### ✅ BUG-1: Symbol Table Conflict Detection is Asymmetric — FIXED

**Status:** Already fixed in the codebase. The `set_value()` method checks for
`SymbolTableEntry::Table` before inserting a value at both single-key and multi-part
key paths.

#### ✅ BUG-2: Operation Limit Error Type Inconsistency — FIXED

**Status:** Resolved. The private `Evaluator::count_op()` now uses
`ExpressionErrorKind::OperationLimitExceeded` matching the `EvalContext` trait impl.

#### ✅ BUG-3: URI Path Mapping Case Sensitivity Violation — FIXED

**Status:** Resolved. `apply_uri()` now splits URIs at the path boundary, compares
scheme+authority case-insensitively and path case-sensitively per RFC 3986.

### 3.2 Performance Issue

#### ✅ PERF-1: Format String Re-Parses Expressions on Every Resolve — FIXED

**Status:** Resolved (S1)

`Segment::Expression` now stores a pre-parsed `ParsedExpression`. Parsing happens once
at `FormatString::new()` time. Structural validation errors (bitwise ops, dict literals,
etc.) are now caught at parse time rather than resolve time — strictly better fail-fast
behavior.

### 3.3 Code Quality Assessment

#### Naming and Consistency

- **Good:** Method names follow consistent conventions (`eval_name`, `eval_attribute`, etc.)
- **Good:** Error constructors are descriptive (`ExpressionError::undefined_variable`, etc.)
- **Good:** Builder pattern with `#[must_use]` on all builder methods
- **Minor:** `SymbolTableEntry` vs spec's `SymbolEntry` — pick one and update the other

#### Error Messages

Error messages are consistently high quality:
- Caret-annotated source display (3-line format: message, expression, caret)
- Smart caret positioning (at operator for BinOp, at attribute name for Attribute, etc.)
- "Did you mean?" suggestions via Levenshtein distance for unknown functions/variables
- Available-types hints for property access on wrong types
- Full error message assertion in tests (per AGENTS.md requirement)

#### Performance

- **Good:** `LazyLock` caching of the default function library
- **Good:** Typed list variants for memory efficiency (97% savings for `list[bool]`)
- **Good:** O(log n) indexing and containment for `RangeExpr` via binary search
- **Good:** Regex cache in the evaluator for repeated pattern compilation
- **Concern:** `ast::Expr` cloning for error context — clones entire AST subtrees
- **Concern:** `build_dotted_name` allocates `Vec<&str>` on every attribute access
- **Concern:** List comprehension creates new `SymbolTable` + `Evaluator` per iteration

#### Algorithmic Complexity

No O(N²) or worse algorithms were found. Key operations:
- Expression parsing: O(N) via ruff parser
- Symbol table lookup: O(K) where K is path depth (typically 2-3)
- Function dispatch: O(S) where S is number of overloads (typically <10)
- List operations: O(N) for most, O(N log N) for sort
- RangeExpr indexing: O(log R) where R is number of ranges
- Memory tracking: O(1) per operation

#### Rust Best Practices

- **Good:** `#[non_exhaustive]` on `ExpressionErrorKind` for forward compatibility
- **Good:** `thiserror` for error derivation
- **Good:** `fn` pointers (not closures) in `FunctionEntry` for `Clone + Send + Sync`
- **Good:** `EvalContext` trait boundary prevents function impls from calling evaluator methods
- **Good:** `saturating_sub` for memory release to prevent underflow
- **Minor:** Some `#[allow(dead_code)]` in test helpers that should be cleaned up

---

## 4. Test Review

### 4.1 Coverage and Organization

| Aspect | Assessment |
|--------|-----------|
| Total tests | 2,819 — comprehensive |
| Test files | 34 — well-organized by feature area |
| Happy path coverage | Excellent — all operators, functions, types covered |
| Edge case coverage | Excellent — overflow, empty collections, Unicode, boundary values |
| Error message assertions | Strong — most error tests assert full 3-line caret messages |
| RFC example tests | Present (26 tests) — validates spec compliance |

### 4.2 Test Quality Issues

| # | File | Issue | Status |
|---|------|-------|--------|
| T1 | test_strings.rs | `eval_fails` helper defined with `#[allow(dead_code)]` — never used | ✅ Removed |
| T2 | test_strings.rs | Some `split`/`rsplit` tests only assert `is_list()` without checking contents | ✅ 10 redundant removed, 2 strengthened |
| T3 | test_types.rs | 7 `p_err()` calls only check `is_err()` without asserting error messages | ✅ All 7 now assert exact error messages |
| T4 | test_rfc_examples.rs | Some assertions use `contains()` instead of exact value comparison | ✅ Both assertions now exact |
| T5 | format_string.rs | 8 test functions outside `mod tests` block (at module scope) | ✅ All 9 moved inside `#[cfg(test)] mod tests` |
| T6 | path_mapping.rs | No tests for actual path mapping application logic — only serde tests | ✅ Report was inaccurate — extensive apply tests already exist |

### 4.3 Exploratory Test Results

27 exploratory edge-case tests were written and executed:

- **26 passed** — confirming correct behavior for: integer overflow, float edge cases,
  division by zero, empty collections, Unicode string length, truthiness semantics,
  chained comparisons, negative indexing, slicing, power operator, null handling,
  keyword-as-attribute, format strings, ternary expressions.
- **1 failed** — `symbol_table_table_then_value_conflict` (BUG-1 above).

---

## 5. Recommendations

### Priority 1 — Bug Fixes

1. ~~**Fix symbol table conflict detection** (BUG-1)~~ ✅ Already fixed in codebase.

2. ~~**Unify operation limit error types** (BUG-2)~~ ✅ Done — private `count_op` now
   uses `OperationLimitExceeded`.

3. ~~**Fix URI case sensitivity** (BUG-3)~~ ✅ Done — scheme+authority compared
   case-insensitively per RFC 3986.

### Priority 2 — Performance

4. ~~**Pre-parse format string expressions** (PERF-1)~~ ✅ Done (S1).

### Priority 3 — Spec Updates

5. ~~**Update values.md**~~ ✅ Done (S4, S5, S7).

6. ~~**Update function-library.md**~~ ✅ Done (S3, S9, S10).

7. ~~**Update format-string.md**~~ ✅ Done — pre-parsing implemented (S1), spec now accurate.

8. ~~**Resolve range-expr.md conflict**~~ ✅ Done (S2) — spec updated to require negative step.

9. ~~**Update evaluator.md**~~ ✅ Mostly done — regex cache documented (S16), unresolved
   BoolOp handling was already documented. Minor: `eval_attribute` operation counting
   remains undocumented.

### Priority 4 — Test Improvements

10. ~~**Add path mapping application tests**~~ ✅ Report was inaccurate — extensive
    `apply_unit` tests already exist covering POSIX, Windows, URI, trailing slashes,
    case sensitivity, `apply_rules`, and `apply_with_format`.

11. ~~**Strengthen weak assertions**~~ ✅ Done — 10 redundant `is_list()` split/rsplit
    tests removed, 2 strengthened; 7 `p_err()` calls now assert exact error messages;
    2 `contains()` assertions replaced with exact values.

12. ~~**Clean up test organization**~~ ✅ Done — dead `eval_fails` removed; 9 tests
    moved inside `#[cfg(test)] mod tests` in format_string.rs.

### Priority 5 — Code Quality

13. **Extract `eval_listcomp`**: The list comprehension handling is ~80 lines inline in
    `evaluate_inner`. Extract to a separate method for readability and testability.

14. **Reduce AST cloning for error context**: Consider using `Cow` or span references
    instead of cloning entire AST subtrees for error context attachment.

15. ~~**Add missing type constants**~~ — Removed from spec (S11). Not wanted.

---

## 6. Summary Scorecard

| Dimension | Score | Notes |
|-----------|-------|-------|
| Spec completeness | 7/10 | Good coverage but several stale sections |
| Spec accuracy | 6/10 | Multiple discrepancies with implementation |
| Implementation correctness | 8/10 | 1 confirmed bug, 2 spec violations |
| Implementation performance | 8/10 | Good overall, format string re-parsing is the main issue |
| Error message quality | 10/10 | Excellent caret formatting, suggestions, context |
| Test coverage | 9/10 | 2,819 tests, comprehensive edge cases, missing path mapping tests |
| Test organization | 8/10 | Well-structured, minor cleanup needed |
| Rust best practices | 9/10 | Good use of type system, traits, error handling |
| Code readability | 8/10 | Clear naming, some large methods could be extracted |
| API ergonomics | 9/10 | Builder pattern, convenience functions, macro support |
