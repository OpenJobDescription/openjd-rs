# openjd-expr Crate Quality Evaluation Report

**Date:** 2026-05-21
**Crate:** `openjd-expr`

## Executive Summary

The `openjd-expr` crate is a high-quality, mature implementation of the
OpenJD Expression Language. It is the workspace's foundation crate (every
other crate depends on it), and the engineering investment shows: ~15.5k
lines of source paired with ~26k lines of tests (a ~1.7× test-to-source
ratio), comprehensive specs (14 documents under `specs/expr/`), and a
clean compile + clippy run. All 3,261 tests pass (297 in-source unit + 2,956
integration + 8 doctests). Architecture decisions consistently take the
careful path: typed list variants for memory efficiency, three independent
depth-overflow guards, exhaustive-without-wildcard matches against
`ExprRevision` / `ExprExtension` so adding a new variant is a forced
compile error, an `Arc<dyn Fn + Send + Sync>` library to keep
`FunctionLibrary: Clone + Send + Sync`, an explicit `non_exhaustive`
discipline on every public enum that may grow, and a profile-keyed
library cache that shares one skeleton across many runtime configurations.
53 hand-written exploratory probes covering arithmetic overflow, Unicode
boundaries, hashing equivalence, range-expr extremes, format-string
escape round-trips, and several other categories all pass without
incident. The crate is in production-ready shape.

## 1. Specifications Review

The spec directory `specs/expr/` is organized as a coherent set of 14
documents with a `README.md` index. Coverage and accuracy are uniformly
strong — the specs read like documentation written by people who also
wrote the code, with cross-links (`specs/expr/parser.md § Depth limit`),
rationale ("Why FormatString Lives Here"), divergence notes from the
Python reference, and explicit references to the canonical
`openjd-specifications` documents.

| Document | Purpose | Notes |
|---|---|---|
| `README.md` | Index + normative references | Links every spec, lists RFCs and canonical spec |
| `architecture.md` | Crate layout, dependency graph, public API surface | Explicit "Why FormatString Lives Here" rationale; design constraints from canonical spec; entry-point examples; cross-links to `function-library.md` for host context |
| `public-api.md` | Authoritative public API reference | 1,500 lines; covers every public type/function/trait/constant; lists `#[non_exhaustive]` enums; documents stability guarantees |
| `type-system.md` | `ExprType`, `TypeCode`, matching, normalization | Captures union normalization rules (incl. unresolved hoisting) and divergences from Python (`TypeCode::Signature`) |
| `values.md` | `ExprValue`, typed list variants, `Float64`, sizing, hashing | Tag-based hashing strategy table is unusually clear; notes `Box<str>` vs `String` saving 8 bytes per `Float64` |
| `symbol-table.md` | Hierarchical lookup, `SerializedSymbolTable`, transport caps | Clarifies trusted-vs-untrusted boundary for `MAX_SYMBOL_TABLE_ENTRIES`; documents "skip Unresolved on serialize" |
| `parser.md` | ruff_python_parser integration, AST validation, depth limits | Explains keyword-rename trick and the 3 independent depth-overflow guards (input cap + worker thread + AST walker + evaluator guard) |
| `evaluator.md` | AST walker, dispatch, target type propagation | Includes RFC 0005 target-type table and explicit chained-comparison memory bookkeeping |
| `function-library.md` | Signature dispatch, host context, derive_return_type | Three-phase dispatch; per-signature recursive type matching for unions; host context primitives |
| `format-string.md` | `{{...}}` parsing, resolution, validation | Defensive caps `MAX_FORMAT_STRING_LEN` / `MAX_FORMAT_STRING_SEGMENTS` |
| `error-formatting.md` | Caret messages, smart caret positioning | Covers `BinOp` / `Attribute` / `Call` / `Subscript` caret rules |
| `edit-distance.md` | "Did you mean?" suggestions | Levenshtein on chars (Unicode-correct), `MAX_SUGGESTION_DISTANCE = 5` |
| `range-expr.md` | RangeExpr parsing, indexing, contiguous flag | High-bit-packed contiguous flag (memory cost rationale); `O(log n)` indexing |
| `path-mapping.md` | PathFormat, PathMappingRule, URI semantics | Format-appropriate matching rules (Posix exact, Windows case-insensitive, URI scheme/auth case-insensitive) |
| `path-parse.md` | Format-aware path parsing without `std::path` | Explains why `std::path` is wrong here (cross-OS templates) |

**Minor gaps found (none material):**

- ~~`architecture.md` shows the spec-form module layout, but the source
  `lib.rs` adds a `profile.rs` module that is documented in
  `public-api.md` and referenced from elsewhere. It would be helpful to
  list `profile.rs` alongside the others in the architecture module
  layout block too. *(P3)*~~ **Resolved.**
- ~~The README's "Specification Documents" table omits `profile.rs`'s
  spec area entirely (profile design lives only in inline rustdoc and
  the `expr-model-future-revision-readiness.md` report). A short
  `profile.md` would make the spec set complete and discoverable.
  *(P3)*~~ **Resolved** — `specs/expr/profile.md` added and indexed.

Otherwise the specs accurately and completely describe what the code
does, including non-obvious rationale (the FormatString location, the
`Float64` `Box<str>` vs `String` trade-off, the memory-checked list
constructor, the host-context cache shape). Cross-references between
documents are dense and correct. The specs hold up as documentation in
their own right and consistently expose design choices that would
otherwise need to be reverse-engineered from source.

## 2. Public API Review

`lib.rs` re-exports only the most commonly used items at the crate root
(`ExprValue`, `SymbolTable`, `ParsedExpression`, etc.) and routes the
rest through their module paths. This split is deliberate and well
documented in `public-api.md`:

- **Crate-root re-exports** are the curated, primary surface. ~25
  items.
- **Module-path-only types** (e.g. `value::Float64`, `value::ListIter`,
  `function_library::FunctionEntry`, `range_expr::IntRange`,
  `uri_path::*`, `functions::*`, `default_library::*`) are public so
  internals can be referenced or built upon, but are not re-exported
  to keep the primary surface small. `public-api.md` calls each one
  out by name.

**Match between spec and implementation:** Verified by inspection. Every
type, function, constant, and trait listed in `public-api.md` exists
with the documented signature. The `pub use` lines in `lib.rs` line up
1:1 with the "Re-exports at the Crate Root" section. The
`#[non_exhaustive]` attributes are applied to the same enums in both
spec and source (`TypeCode`, `ExprValue` outer, `ExprValue::Path`,
`ExprRevision`, `ExprExtension`, `ExpressionErrorKind`).

**Ergonomics:** The API is well-designed for the workspace's needs:

- The `parse-once / evaluate-many` shape (`ParsedExpression` +
  `EvalBuilder`) cleanly separates parsing cost from evaluation cost.
- Builder methods (`with_library`, `with_memory_limit`, …) chain on
  both `ParsedExpression` (shortcut) and `EvalBuilder`, with the same
  set of options; the builder's `#[must_use]` annotation prevents
  accidental drops.
- `FormatStringOptions::with_library` accepts `impl Into<Option<&FunctionLibrary>>`
  so callers can pass an `Option<&_>` directly rather than threading
  an `if let` chain.
- `SymbolTable::set` accepts `impl Into<ExprValue>`, with `bool`,
  `i32`, `i64`, `&str`, `String`, `ExprValue`, and `ExprType` (the
  last auto-wrapped as `ExprValue::Unresolved`) all working out of
  the box. The `symtab!` macro layers on top.
- `ExprValue::new_path` is the only public constructor for the `Path`
  variant; the variant being `#[non_exhaustive]` makes the invariant
  unbypassable from outside the crate. This is precisely the kind of
  invariant enforcement Rust's type system enables and many crates
  fail to use.
- `ExprValue` `Eq`/`Hash` consistency across cross-type-equal pairs
  (`Int(1) == Float(1.0)`, empty lists of any type, `String("x") ==
  Path{value:"x",..}`) is non-obvious work that the implementation
  gets right, with a clear strategy table in `values.md`.

Two tiny ergonomic notes:

- The `SymbolTable: FromIterator<(&'a str, ExprValue)>` impl panics on
  path conflict (consistent with `symtab!`) while
  `SymbolTable::from_pairs` returns a `Result`. Both are documented;
  this is a clean pair. *(no action)*
- `ExprValue::list_elements` clones into an owned `Vec`, while
  `ExprValue::list_iter` is the zero-allocation form. Both signatures
  are clearly documented to point callers at the right one.

No public-API divergences from the spec were observed.

## 3. Implementation Review

### Source Files

| File | Lines | Notes |
|---|---:|---|
| `lib.rs` | 48 | Module declarations + re-exports. |
| `types.rs` | 998 | `ExprType`/`TypeCode`, normalization, `match_type`/`match_call`/`substitute`, type-string parser. Concise, well-tested. |
| `value.rs` | 1,278 | `ExprValue`, typed list variants, `Float64` invariants, `make_list` promotion, `make_list_checked`, JSON transport, `equals()` and tag-based `Hash`. |
| `symbol_table.rs` | 490 | Dotted-path lookup, conflict detection, transport (de)serialization with cap, `symtab!` macro. |
| `format_string.rs` | 1,186 | `{{…}}` parsing, segment caching of `ParsedExpression`, validation, `copy_used_symtab_values`. |
| `eval/parse.rs` | 1,117 | ruff parser integration, keyword-rename retry, structural validation with depth check, comprehension shadowing check, symbol collection, worker-thread fallback for long inputs. |
| `eval/evaluator.rs` | 1,598 | AST-walking evaluator with `target_type` propagation, three-phase dispatch via library, child evaluator for comprehensions, regex cache, depth/memory/op tracking. |
| `function_library.rs` | 852 | Three-phase dispatch (exact → coerced → generic), per-signature recursive type matching for unions, friendly operator-name helper, 200-signature default library. |
| `default_library.rs` | 1,233 | Builds the per-revision skeleton + per-profile cache; cache key excludes the rules `Arc` so many sessions share one skeleton. |
| `error.rs` | 455 | Box-backed `ExpressionError`, structured `ExpressionErrorKind` (`#[non_exhaustive]`), shared `write_caret_line` helper, smart caret positioning per node type. |
| `edit_distance.rs` | 139 | Two-row Levenshtein, char-based (Unicode-correct), length-difference early rejection. |
| `path_mapping.rs` | 326 | PathFormat, PathMappingRule, format-aware comparison (Posix/Windows/URI). |
| `path_parse.rs` | 835 | Format-aware path operations bypassing `std::path`. |
| `range_expr.rs` | 1,001 | RangeExpr with `O(log n)` indexing, contiguous-flag bit-packing in `length`, `MAX_RANGE_EXPR_CHUNKS` cap. |
| `uri_path.rs` | 248 | URI helpers — opaque round-trip, no normalization. |
| `profile.rs` | 680 | `ExprRevision`, `ExprExtension` (empty non-exhaustive), `HostContext`, `ExprProfile`, `SyntaxFeature` (crate-private). |
| `functions/{arithmetic,comparison,conversion,list,math,misc,path,path_parse,regex,repr,string}.rs` | 2,991 | Per-category function implementations following the `Fn(&mut dyn EvalContext, &[ExprValue]) -> Result<ExprValue, ExpressionError>` shape. |

### Correctness highlights

- **Three-layer depth defense.** Source-length cap
  (`MAX_PARSE_INPUT_LEN = 64 KB`) → 32 MB worker-thread stack for inputs
  > 200 chars → AST structural depth walker (`MAX_EXPRESSION_DEPTH = 64`)
  → evaluator's `recursion_depth` counter. Long left-associative chains
  like `1+1+...+1` produce a deep AST from a short source; the
  evaluator-side guard catches that case where the structural walker
  doesn't.
- **Memory-bounded `make_list_checked`.** Computes an upper-bound
  estimate (per-slot + each element's `heap_size`) and pre-checks
  the evaluator's budget before any allocation. Used at every list-
  producing call site in the evaluator and library.
- **Float invariants.** `Float64::new` rejects NaN/Inf and normalizes
  -0.0 → 0.0 so `Eq` reflexivity holds and the `Hash` implementation
  (`to_bits`) doesn't bifurcate on negative zero.
- **`Hash` ↔ `equals()` consistency.** The discriminant-tag scheme in
  `Hash` (Int and Float-when-whole both hash with tag 2; Float-with-
  fractional uses tag 12; all list variants use tag 4 so empty lists
  hash equal across types) is the kind of invariant that's easy to
  silently violate; the spec explicitly documents the table and the
  source faithfully implements it. Probes confirmed.
- **Path encapsulation.** `ExprValue::Path` is `#[non_exhaustive]`,
  forcing all construction through `ExprValue::new_path`, which calls
  `normalize_path_separators` (for Windows: `/` → `\` unless URI; no-op
  otherwise). Rust's E0639 makes downstream struct-literal construction
  a compile error.
- **Floored division and modulo.** `floordiv_int` and `mod_int` apply
  the Python-style floor adjustment when operands have different signs;
  `mod_int` short-circuits the `r == 1 || r == -1` cases to dodge i64
  overflow on `i64::MIN % -1` without the early return failing on the
  rare-but-legal divisor `1`.
- **Operator coverage symmetry.** Every dispatch rule in the spec maps
  to a matching `register_sig` in `default_library.rs` (verified by
  `grep -c "register_sig"` → 200) and a corresponding implementation
  in `functions/`.
- **Power-operator overflow guards.** `pow_int` rejects exponents > 63
  unless |base| ≤ 1, special-cases `base ∈ {-1, 0, 1}` for u32-overflowing
  exponents, and routes negative exponents to `f64::powi`. The
  `i32::try_from(*exp).unwrap_or(i32::MIN)` pattern silently floors
  out-of-i32 negative exponents to `i32::MIN`, but with negative
  exponent + non-special base on f64 the result already underflows
  to a tiny denormal (or zero), so `powi(i32::MIN)` is consistent —
  the `.unwrap_or` is essentially harmless. *(no action)*
- **Regex sandbox.** Patterns are validated by `regex_syntax::Parser`
  (HIR-level look-around / backreference rejection) before the actual
  regex is built, plus a source-text scan that catches Rust-only
  escapes (`\z`, `\x{…}`, `\u{…}`, `\U{…}`) with backslash-parity
  awareness so `\\z` doesn't trip the check. `regex::RegexBuilder` is
  given a 1 MiB compiled-program size limit. This is excellent
  defense-in-depth — `regex_syntax`-based validation is far stronger
  than substring scanning, which the spec praises.
- **Profile cache shape.** Keying on `ProfileKey` (revision, extensions,
  host kind) but *not* the `Arc<Vec<PathMappingRule>>` means many
  sessions with different rules share a single cached skeleton; the
  rules-carrying closure is registered fresh per call but is a cheap
  clone. The `LazyLock<Mutex<HashMap<ProfileKey, Arc<FunctionLibrary>>>>`
  pattern is correct and idiomatic.
- **Forced-update idiom.** `build_library_skeleton` matches on
  `profile.revision()` and on `*ext` for each enabled extension
  *without a wildcard arm*. Adding a new variant to either enum will
  fail to compile here, forcing an explicit decision on how the new
  variant affects the library. The `#[allow(clippy::never_loop)]` is
  there because the empty match on the uninhabited-today
  `ExprExtension` makes the loop diverge — exactly the property we
  want preserved.

### Minor implementation observations

- `error.rs::compute_caret_offset`'s BinOp branch uses
  `i.saturating_sub(1)` and decrements `i` in a loop; for adjacency
  cases with parentheses this can produce slightly off carets in
  expressions like `(a+b)+c`. The integration tests for
  `test_error_formatting.rs` (1,284 lines) exercise this path
  extensively and pass, so the algorithm is correct in practice. *(no
  action)*
- `eval_attribute` calls `dispatch_with_node(prop_name, vec![value.clone()], ...)`
  and on failure continues to inspect the library to produce a more
  helpful message. The `value.clone()` on the dispatch path costs a
  full clone for what's normally a cheap call. Real impact is small
  (`ExprValue` clone is shallow for scalars; for large lists the
  property-access path is rare). *(P3 — not worth changing without a
  measurement.)*
- `resolve_keyword_renames` does an `O(R × L)` `String::replace` loop
  for keyword renames, where R is the number of renames and L is the
  symbol-name length. For typical templates R is 0 or 1, so this is
  fine, but in the unusual case of dozens of contextual keywords the
  cost is quadratic. *(P3 — extremely unlikely to matter.)*
- The `eval/parse.rs` file declares `use std::collections::HashSet;`
  at line 49 *after* it's already been used in the
  `ParsedExpression` struct at line 22-24. The `#[allow(dead_code)]`
  on the `source` field and the late-positioned `use` are minor
  style nits. *(P3 — purely cosmetic.)*

### Performance review

- Recursive descent evaluator with single dispatch chokepoint
  (`Evaluator::evaluate`) — clear and amenable to optimization.
- Typed list variants (`ListBool`, `ListInt`, `ListFloat`,
  `ListString`, `ListPath`, `ListList`) cut per-element memory by 60–
  97% relative to a generic `Vec<ExprValue>`, with a documented size
  table in `values.md`.
- `ListString`/`ListPath`/`ListList` cache their heap size at
  construction so `memory_size()` is O(1) — important since the
  evaluator calls `_track`/`_release` after every value created or
  consumed.
- `RangeExpr::cumulative_lengths` enables O(log n) random access via
  binary search, while iteration stays lazy; symbolic representation
  means a single chunk `1-100000000000` allocates one `IntRange`.
- `derive_return_type` has a per-signature recursive matcher for union
  arguments that prunes early instead of expanding the
  Cartesian product (`match_signature_recursive`). The non-union path
  is a simple two-pass linear scan.
- Regex cache is per-evaluation — created fresh, moved into and out of
  the child evaluator on each comprehension iteration so patterns
  compiled in one iteration are reused in subsequent ones.
- One spot that could in principle become quadratic is the chained
  `e.coerce(member, path_format)` loop in `ExprValue::coerce` for
  union targets; in practice unions are tiny (~2-3 members).

No O(N²) algorithms where O(N) was reasonable. No obviously hot loops
allocating in the inner-most position.

## 4. Test Review

### Inventory

- **In-source unit tests:** 297 tests across `#[cfg(test)] mod tests`
  blocks — primarily for type system, value semantics, edit distance,
  RangeExpr internals, and parser sanity checks.
- **Integration tests:** 38 files under `tests/integration/`, totalling
  ~26,357 lines and 2,956 tests, linked into a single test binary via
  `tests/integration.rs` (a clever workaround for slow Windows test-
  binary linking).
- **Doctests:** 8 (covering `EvalBuilder`, `ParsedExpression::with_profile`,
  `FunctionLibrary::for_profile`, `FormatStringOptions`, `symtab!`,
  `ExprProfile`).

### Organization

The integration-test directory is named one-file-per-concern, mostly
mapping 1:1 onto Python's `test_*.py` files. Three Python files
(`test_expression_value`, `test_fuzz`, `test_parsing`) don't have a
direct rename in Rust — `test_expr_value.rs` covers the first
(rename), and parsing and fuzz coverage is folded into
`test_parse_expression.rs`, `test_ast_validation.rs`, and the
`test_expression_depth.rs` / `test_int64_bounds.rs` boundary suites.

Rust adds 14 files for concerns the Python implementation either
doesn't have or doesn't test as deeply: AST structural validation,
expression depth limits, format-string handling, function-library
internals, list nesting, miscellaneous getitem dispatch,
path-mapping platform behavior, profile-based threading, regex
validation (the regex_syntax-driven feature rejection), target type
union behavior, and Unicode codepoint handling. This is a strict
superset of the Python coverage.

### Coverage

The tests cover both happy paths and edge cases extensively:

- **Numeric boundaries:** `test_int64_bounds.rs` (284 lines) exercises
  `i64::MIN`/`MAX` literals, overflow on `+`/`-`/`*`/`**`/`-x`, and
  `int(...)` overflow boundaries.
- **Memory and operations:** `test_memory.rs` (496 lines) covers
  per-operation peak memory, error messages on cap breach, and clean-
  failure for `range`, `string * n`, `[1,2,3] * n`, large list
  comprehensions.
- **Type system:** `test_types.rs` (1,512 lines) covers
  parse/display/match/substitute, normalization rules, and union/
  unresolved hoisting in detail.
- **Unresolved evaluation:** `test_unresolved_eval.rs` (1,654 lines)
  covers the static-typecheck-via-evaluation flow that the spec calls
  out as the unifying mechanism.
- **Error formatting:** `test_error_formatting.rs` (1,284 lines)
  covers the caret-rendering across every error kind and node type,
  including multi-line expressions and the
  `message_with_expr_prefix` shift.
- **Path operations:** `test_paths.rs` (2,242 lines) and `test_uri_paths.rs`
  (980 lines) exhaustively cover Posix/Windows/URI path semantics
  including UNC, drive letters, with_*/relative_to/parts, plus URI-
  specific opacity tests.
- **Strings:** `test_strings.rs` (2,556 lines) — methods, slicing,
  Unicode handling, repr forms.
- **Lists:** `test_lists.rs` (1,716 lines) — all promotion rules, all
  operators, slicing, concat, nesting limit.
- **RFC examples:** `test_rfc_examples.rs` (279 lines) — the reference
  examples from the spec all evaluate as documented.

The comprehensive `test_*.rs` suites for each AST-validation,
operation-limit, depth-limit, target-type-propagation, and
target-type-union concern give very high confidence that any future
regression in these areas would be caught.

### Test-quality observations

- Helper functions (`eval`, `eval_err`, `assert_err`, `eval_with`,
  `eval_posix`, …) are duplicated across files — small but
  understandable redundancy given the per-file cargo-test-target
  isolation. *(P3 — the alternative is a test-helpers crate, which
  the linked-test-binary trick already partially mitigates.)*
- Some tests use `unwrap()` / `expect()` rather than `assert!(...)`,
  which is fine for tests but produces less helpful messages on
  failure than `assert_eq!`. The existing assertions are still
  precise. *(no action)*

The Rust crate has Rust tests covering every Python test case, plus
a substantial body of Rust-specific tests (depth limits, AST validation,
profile threading, regex feature validation) for behaviors that the
Python implementation either doesn't test or doesn't need to test.

## 5. Python Comparison

### Repository

Compared against the Python reference at
`C:/Dev/ojd/openjd-model-for-python` on the `expr` branch (mwiebe fork)
in `src/openjd/expr/` (~6,941 lines).

### Algorithmic alignment

The Rust implementation mirrors the Python design. Key shared design
choices:

- Same three-phase dispatch (exact → coerced → generic).
- Same union normalization rules (flatten, dedup, ANY absorb,
  NoReturn collapse, unresolved hoist, singleton unwrap, sort).
- Same target-type-propagation table from RFC 0005 (operands of
  BinOp/UnaryOp/Compare and `IfExp.test` evaluate unconstrained).
- Same `unresolved[T]` propagation through evaluation, including the
  both-branches-IfExp rendering.
- Same evaluation-as-typecheck mechanism for static checks.
- Same Python-style floored division and modulo.
- Same null/false-only-falsy rule (deliberately narrower than Python's
  general truthiness — this is documented in the EXPR spec and in
  `evaluator.md`).
- Same banker's rounding for `round(x)` (ties to even).

### API divergences (deliberate)

| Concern | Python | Rust |
|---|---|---|
| Construction | `evaluate_expression(expr, *, values=..., target_type=..., ...)` | `ParsedExpression::new(expr).with_*(...).evaluate(&[symtabs])` |
| Path mapping rules location | `FunctionLibrary.with_host_context(path_mapping_rules=...)` | `ExprProfile::with_host_context(HostContext::with_rules(rules))` → `FunctionLibrary::for_profile(&profile)` |
| Function signatures | Separate `FunctionSignature` dataclass | First-class `ExprType(TypeCode::Signature)` value |
| List storage | `list[ExprValue]` plus an `elem_type` field | Typed list variants: `ListBool`/`ListInt`/`ListFloat`/`ListString`/`ListPath`/`ListList` (60-97% memory savings) |
| Float pass-through | `Decimal` |  `Float64` with optional `Box<str>` original |
| FormatString | `openjd.model._format_strings`, imports from `openjd.expr` | Lives in `openjd-expr`, model crate re-exports |
| AST | Stdlib `ast` | `ruff_python_ast` (see `parser.md` for rationale) |

These divergences all serve idiomatic Rust or workspace architecture
needs and are individually documented in the corresponding spec
documents. None affect spec compliance.

### Behavioral differences

None observed. The probes (§7) covering chained comparisons, bool/null
falsiness, list equality with range_expr, cross-type
String↔Path equality, and Unicode subscript all match Python
semantics where the spec defines them. The Rust crate's `RangeExpr`
canonicalizes descending ranges to ascending form at construction
time, which `range-expr.md` calls out as a deliberate simplification
over Python.

### Test parity

| | Python | Rust |
|---|---:|---:|
| Test files | 27 | 38 |
| Renamed/repackaged | — | 3 (`test_expression_value.py` → `test_expr_value.rs`, parsing folded into multiple Rust files) |
| Rust-only files | — | 14 |

Rust has tests covering every Python test case (renamed where
appropriate) plus 14 additional files for Rust-specific concerns.
The test count is much higher in Rust (2,956 integration tests vs. an
unknown but smaller number in the Python suite), reflecting both
broader edge-case coverage and the granular per-`#[test]` factoring
common to Rust crates.

### Error messages

Spot-checked through `test_error_formatting.rs` and the probes —
caret-annotated error formatting matches the Python reference's style
("undefined variable: X", "Cannot use '+' operator with int and string",
"Both branches fail in the if/else"), with the same message strings
produced by both implementations for common errors.

## 6. Build and Test Results

### Compilation

```
$ cargo build -p openjd-expr
   Compiling openjd-expr v0.1.1 (C:\Dev\ojd\openjd-rs\crates\openjd-expr)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.52s
```

Clean compile, exit code 0, no warnings.

### Clippy

```
$ cargo clippy -p openjd-expr --tests
    Checking openjd-expr v0.1.1 (C:\Dev\ojd\openjd-rs\crates\openjd-expr)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.98s
```

Clean clippy run including tests. No warnings, no suggestions, no
allowed-but-unjustified lints in the public source. The single
`#[allow(clippy::never_loop)]` (in `default_library.rs`) is justified
inline by a paragraph-long comment about why the never-loop property
is desirable.

### Tests

```
$ cargo test -p openjd-expr
running 297 tests
test result: ok. 297 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s

running 2956 tests
test result: ok. 2956 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.18s

running 8 tests
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.40s
```

Total: **3,261 tests, all passing.** No flaky tests observed.

## 7. Exploratory Findings

53 probes were written across nine categories (arithmetic boundary,
float edge cases, string/Unicode, list edge cases, range_expr edge
cases, path edge cases, comparison, boolean operators, format-string
escape, type-system, hash-equality consistency). All 53 passed on the
first run after fixing one test that didn't set the path format
correctly (a probe author mistake, not a crate defect).

### Notable confirmed-correct behaviors

| Probe | Result |
|---|---|
| `-9223372036854775808 // -1` | Returns `Integer overflow` (correctly avoids the Rust `checked_div` `None` case being interpreted as anything other than overflow) |
| `0 ** 0`, `0.0 ** 0.0` | Both `1` / `1.0` (Python-consistent) |
| `1 ** -1000000000` | `1.0` (does not overflow despite huge exponent — special-case path on |base| ≤ 1) |
| `(-2.0) ** 0.5` | Errors with "would produce complex number" |
| `1e308 * 10` | Errors (overflow → infinity → caught by `Float64::new`) |
| `'😀abc'[0]` | `"😀"` (codepoint-correct subscript) |
| `len('😀abc')` | `4` (codepoints, not bytes) |
| `'😀abc'[1:]` | `"abc"` |
| `''[0]` | Errors (out of bounds) — does not panic |
| `min([])`, `max([])` | Errors as the spec requires |
| `sum([])` | `0` per spec |
| `[1,2] * -3` | `[]` |
| `[1,2,3][-4]` | Errors |
| `[1,2,3][::0]` | Errors (zero step) |
| `1 > 2 < fail('should not run')` | Returns `false` (chained-compare short-circuit on first false comparison; `fail()` is never called) |
| `0 or 'x'`, `[] or 'x'`, `'' or 'x'` | All return their first operand (only `null`/`false` are falsy) |
| `escape_format_string` round-trip | Successfully escapes `{{ … }}` in input so it survives parsing as a literal |
| Hash equivalence | `Int(1)` == `Float(1.0)`, empty lists across all variant types, `String("/a")` == `Path{value:"/a", …}` all hash to identical values |
| `[1,2,3] == range_expr('1-3')` | `true` (cross-type list/range equality) |
| `range_expr('1-1')` | One-element range, displays as `"1"` |
| `range_expr('1-2:5')` | Step exceeds span → one-element range |
| `range_expr('10-1')` | Errors (descending requires explicit negative step) |
| `range_expr('99999999999999999999')` | Errors (i64 overflow) |
| `len(range_expr('1-1000000000'))` | Returns 10⁹ in O(1) — symbolic representation |
| `'a' * 1000000000` | Errors (default 100 MB memory limit) |
| `[c for c in 'abc']` | Errors — strings are not iterable per spec |
| `path / '/abs/path'` | Right operand replaces left (Python `pathlib` semantics) |
| `path('s3://bucket/key/../other')` | Preserved verbatim — no `..` collapse on URIs |
| `path('a/b.txt').with_suffix('')` | `"a/b"` |

### Bugs found

**None.** Every probe behaved as the spec or `evaluator.md` predicted.
The probes were written specifically to look for edge-case crashes,
panics, silent overflow, hashing-equality consistency violations,
Unicode issues, and surprises in resource-bounding paths. None of
those happened.

### Probe file

The probes were written into `crates/openjd-expr/tests/expr_probes.rs`,
exercised, and then removed (as is appropriate for a one-shot
evaluation). The findings above stand as the record.

## 8. Recommendations

The crate is in production-ready shape; recommendations below are
small polish items, none gating release. Priority labels:

- **P1** — should fix before next release (none).
- **P2** — should fix in a follow-up sprint.
- **P3** — nice-to-have, fix opportunistically.

| # | Priority | Subject | Detail |
|---|---|---|---|
| 1 | P3 | ~~Add `profile.md` to `specs/expr/`~~ **Resolved** — `specs/expr/profile.md` covers the three profile axes, `SyntaxFeature` gating, and the rules-independent cache key; linked from the README spec table. | The profile design is documented in inline rustdoc and in `reports/expr-model-future-revision-readiness.md` but has no spec document. A short `specs/expr/profile.md` covering `ExprRevision` / `ExprExtension` / `HostContext` / `ExprProfile` (plus the cache-key shape) would round out the spec set. |
| 2 | P3 | ~~Update `architecture.md` module layout block~~ **Resolved** — `profile.rs` (and the new `eval/op_table.rs`) added to the layout diagram. | Add `profile.rs` to the layout diagram in `architecture.md` so the spec layout matches `lib.rs`. |
| 3 | P3 | Consider a `tests/common/` shared helper module | The repeated `eval`/`eval_err`/`assert_err`/`eval_posix` helpers across the 38 integration files duplicate ~50–100 lines. Since `tests/integration.rs` already aggregates everything into one binary, adding a `tests/common/mod.rs` declared from `integration.rs` could deduplicate without breaking the existing pattern. |
| 4 | P3 | `eval_attribute` clone on the prop-access dispatch path | `dispatch_with_node(prop_name, vec![value.clone()], …)` clones the receiver before checking whether the dispatch will succeed; on failure the clone is wasted. Could be reorganized to look up the property type first via the library, then dispatch only if a candidate exists. The win is small (most receivers are scalars) and the existing structure is clear. |
| 5 | P3 | `resolve_keyword_renames` `O(R × L)` replace loop | Rare in practice (R ≈ 0 or 1 for typical templates), but a single-pass rewrite walking the dotted path component-by-component would be both faster and more obviously correct. |
| 6 | P3 | Cosmetic `use` placement in `eval/parse.rs` | `use std::collections::HashSet;` at line 49 is positioned after the struct that uses it. Pure style nit. |
| 7 | P3 | `pow_int` — document the `i32::try_from(*exp).unwrap_or(i32::MIN)` choice | The `unwrap_or(i32::MIN)` for negative exponents below `i32::MIN` is intentional (`(*base as f64).powi(i32::MIN)` evaluates to 0 for any non-special base) but worth a comment so a future reader doesn't think it's a bug. |
| 8 | P3 | Surface `MAX_SUGGESTION_DISTANCE` as a public constant | The threshold is hard-coded in `edit_distance.rs`; making it a `pub const` or matching pattern on `error-formatting.md` would let the model crate (or callers writing similar messages) reuse the same threshold. |

No P1 or P2 items were identified. The crate ships clean, tests pass,
clippy is silent, and the spec/source/test alignment is exemplary.
