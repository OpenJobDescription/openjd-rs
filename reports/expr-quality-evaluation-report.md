# openjd-expr Crate Quality Evaluation

**Evaluator:** Kiro (with `deadline-openjd` subagent for spec, source, and test review)
**Date:** 2026-04-17
**Crate location:** `~/openjd-rs/crates/openjd-expr`
**Spec location:** `~/openjd-rs/specs/expr`

> **Update 2026-04-17 (later):** Findings in sections 1, 2, and 3 have been
> addressed by a spec-alignment pass, and five friction points in section 4
> (items 1, 5, 6, 7, 8) have been addressed by an API-refactor pass. See
> [§17 Changes applied](#17-changes-applied) for what was modified.
> Sections 5–16 (error messages, performance, tests, edge cases) are unchanged
> and still describe the crate as originally evaluated.
>
> **Update 2026-04-18:** Section 5 (error message quality) has been addressed —
> see [§19 Error message quality pass](#19-error-message-quality-pass).

## Executive summary

`openjd-expr` is a high-quality Rust crate. It compiles cleanly with no warnings (`cargo build -p openjd-expr --all-targets`), clippy is clean, and all **2,998** tests pass. No `unsafe` code. No reachable panics on malformed input. Integer overflow is consistently handled with `checked_*` arithmetic. Error messages are exceptional by the standard of most interpreter projects — the AGENTS.md rule that every evaluation error renders as message + expression + caret is followed throughout `tests/test_error_formatting.rs` (1,067 lines) and is largely delivered by the evaluator.

The main gaps are not correctness but alignment:

1. **Specifications drift from the implementation** in ~19 concrete places (type signatures, field visibility, constructor args, method names). Several spec doc examples would not compile if checked.
2. **Test coverage is uneven** — ~40% of error tests do not meet the AGENTS.md "assert full message + expression + caret" standard; many use `.message()` or `.contains(substring)`.
3. **Evaluator hot path clones AST subtrees** for error-site range reporting, allocating on every binary op / subscript / call.
4. **Three parallel caret-rendering code paths** should be consolidated into one helper.
5. **Two genuine minor findings** were confirmed by live probes: (a) regex validator only covers `\1`–`\5`; `\6`–`\9` fall through to a lower-quality "Invalid regex" error. (b) `FormatString::Segment::SimpleName` (a documented perf fast path) re-parses on every validate/accessed-symbols call instead of caching. *Later: `SimpleName` was removed entirely in favor of always-parsing `{{...}}` at construction time, which eliminates the re-parse without adding a cache. See §6 row 3.*

No blocking issues for the crate's release readiness. Recommendations below are prioritized.

---

## Artifacts reviewed

### Specifications (`~/openjd-rs/specs/expr/`, 2,231 lines across 12 files)

| File | Lines | Scope |
|---|---|---|
| `README.md` | 33 | Index and normative references |
| `architecture.md` | 165 | Module layout, design constraints |
| `error-formatting.md` | 184 | Caret error rendering |
| `evaluator.md` | 296 | AST walker, builder pattern, resource limits |
| `format-string.md` | 188 | FormatString parse/resolve/validate |
| `function-library.md` | 276 | Registry, dispatch, operator mapping |
| `parser.md` | 145 | ruff_python_parser integration, AST allowlist |
| `path-mapping.md` | 142 | PathMappingRule, URI paths |
| `range-expr.md` | 162 | Integer ranges |
| `symbol-table.md` | 168 | Hierarchical symtab + wire format |
| `type-system.md` | 129 | TypeCode, unions, coercion |
| `values.md` | 343 | ExprValue, typed lists, ListIter |

### Implementation source (`src/`, ~13K lines across 23 files)

Core modules: `lib.rs` (94), `error.rs` (379), `value.rs` (1,160), `types.rs` (980), `symbol_table.rs` (425), `function_library.rs` (831), `default_library.rs` (771), `format_string.rs` (1,120), `range_expr.rs` (849), `path_mapping.rs` (325), `uri_path.rs` (247), `edit_distance.rs` (125).

Submodules: `eval/` (`evaluator.rs` 1,466, `parse.rs` 721, `mod.rs` 14), `functions/` (arithmetic, comparison, conversion, list, math, misc, path, path_parse, regex, repr, string — ~3.5K lines).

### Tests (`tests/`, ~23K lines across 36 files; plus ~267 `#[cfg(test)]` tests in `src/`)

Organized by feature area with `// === Section ===` headers that map to Python reference test classes. Largest files: `test_strings.rs` (2,436), `test_paths.rs` (1,719), `test_lists.rs` (1,689), `test_types.rs` (1,496), `test_unresolved_eval.rs` (1,454), `test_expr_value.rs` (1,263), `test_parsing.rs` (1,169).

### Verified build/test results

```
cargo build -p openjd-expr --all-targets    → clean, no warnings
cargo clippy  -p openjd-expr --all-targets   → clean
cargo test    -p openjd-expr                 → 2998 passed; 0 failed; 0 ignored
```

---

## Findings by category

### 1. Spec ↔ implementation mismatches

> **Status (2026-04-17 follow-up):** All 19 entries below have been addressed in
> the spec. See [§17 Changes applied](#17-changes-applied) for per-file details.

Every entry below is a concrete drift where a spec doc describes something that no longer matches the code. Examples in specs are often not-yet-compilable against the current API.

| # | Spec file | Spec claim | Actual implementation | Source | Fixed |
|---|---|---|---|---|---|
| 1 | architecture.md, evaluator.md | `parsed.evaluator(...).evaluate()?` | `evaluator.evaluate(&parsed.ast)?` (AST arg required) | `eval/evaluator.rs:205` | ✓ |
| 2 | architecture.md | Module layout omits `functions/path_parse.rs` | 691-line module exists | `functions/path_parse.rs` | ✓ |
| 3 | error-formatting.md | `ExpressionError` has `pub` fields | All fields private; accessor methods only | `error.rs:73-80` | ✓ |
| 4 | error-formatting.md | `ExpressionErrorKind` variants are unit (`UndefinedVariable`, `TypeError`) | Struct variants with fields (`UndefinedVariable { name, suggestion }`, …) | `error.rs:14-66` | ✓ |
| 5 | error-formatting.md | `ExpressionError::integer_overflow("...")`, `division_by_zero()` | `integer_overflow()` (no arg), `division_by_zero(op: &'static str)` | `error.rs:106-114` | ✓ |
| 6 | evaluator.md | BoolOp unresolved → `Unresolved(union[a, b])` | Returns `Unresolved(BOOL)`; operands may still short-circuit | `eval/evaluator.rs:655-708` | ✓ |
| 7 | evaluator.md | `ParsedExpression` struct shape | Has `source` + `operation_count` fields not documented | `eval/parse.rs:14-22` | ✓ |
| 8 | format-string.md | `Segment { Literal, Expression }` (two variants) | Three variants incl. `SimpleName { start, end, name }` (fast-path) | `format_string.rs:17-27` | ✓ (and subsequently reverted: `SimpleName` was removed in a later pass — see §6 row 3) |
| 9 | format-string.md | `library: &FunctionLibrary` non-optional | All resolve fns take `Option<&FunctionLibrary>` | `format_string.rs:71-240` | ✓ |
| 10 | function-library.md | `arithmetic::library()`, `pub fn default_library()` | Free fns local to `default_library.rs`; public API is `get_default_library()` | `default_library.rs:17-34` | ✓ |
| 11 | function-library.md | References `ExprType::LIST_INT` constant | No such constant exists; use `ExprType::list(ExprType::INT)` | `types.rs:38-100` | ✓ |
| 12 | function-library.md | `get_or_compile_regex` default uses `Regex::new(...)` | Uses `RegexBuilder::new(pattern).size_limit(1 << 20)` (DoS protection) | `function_library.rs:49-54` | ✓ |
| 13 | function-library.md | `FunctionLibrary { functions: HashMap<...> }` | Also has `pub host_context_enabled: bool` | `function_library.rs:58-62` | ✓ |
| 14 | path-mapping.md | `rule.apply(path) → (bool, String)` | `rule.apply(path) → Option<String>` | `path_mapping.rs:46` | ✓ |
| 15 | path-mapping.md | URI fns named `split_uri`, `uri_parts`, `uri_name`, … | Module `uri_path` exports `parse`, `parts`, `name`, … (no `uri_` prefix) | `uri_path.rs:18-127` | ✓ |
| 16 | symbol-table.md | `get(...) → Option<&ExprValue>` | `get → Option<&SymbolTableEntry>`; separate `get_value`, `get_string` | `symbol_table.rs:168-197` | ✓ |
| 17 | values.md | `.item()` method panics on unresolved | No `item()` method exists | `value.rs` | ✓ |
| 18 | parser.md (implicit) | ruff is the parser | `eval/mod.rs:5-6` doc comment still says "rustpython-parser" | `eval/mod.rs:5-6` | ✓ (source doc comment updated) |
| 19 | range-expr.md | `IntRange` stores "original direction separately" | Only canonical ascending form stored; direction normalized away | `range_expr.rs:62-110` | ✓ |

### 2. Spec coverage gaps

> **Status (2026-04-17 follow-up):** Addressed. Two new spec files added
> (`edit-distance.md`, `path-parse.md`); existing specs expanded to cover
> host-context inventory, keyword-rename reverse mapping, SerializedSymbolTable
> Unresolved skipping, and `from_str_coerce` rules.

Source modules/areas with no dedicated spec or inadequate coverage:

- **`functions/path_parse.rs`** (~20KB): Format-aware POSIX/Windows path parsing without `std::path`. Implements `sep`, `split`, `file_name`, `parent`, `file_stem`, etc. — the core of PATH property semantics that `path-mapping.md` alludes to but doesn't cover. Not listed in architecture module layout. → **Fixed:** new `specs/expr/path-parse.md`; module now listed in `architecture.md`.
- **`edit_distance.rs`**: Referenced twice (architecture + error-formatting) but never described. Levenshtein threshold, suggestion-trigger rules, and public API (`edit_distance`, `suggest_closest`) are undocumented. → **Fixed:** new `specs/expr/edit-distance.md`; linked from `error-formatting.md`.
- **`functions/*` per-category modules**: No spec covers which file implements which operator category (arithmetic, comparison, conversion, list, math, misc, path, regex, repr, string). `function-library.md` lists categories abstractly but leaves semantics to the language spec. → **Not changed:** per-function semantics intentionally defer to the language spec (RFC 0005/0006). `architecture.md` module table already names each file's category.
- **`SerializedSymbolTable`**: Wire format is specified at the JSON level, but round-trip rules for `Unresolved` entries are not fully covered (the spec says unresolved entries are *skipped* on serialization — this behavior is not explicitly tested). → **Fixed in spec:** `symbol-table.md` now explicitly describes value-only transport semantics; test gap still open (see §12).
- **Host context registration**: `with_host_context`, `with_unresolved_host_context`, `register_host_context_functions` are mentioned; the actual inventory of host-context functions and their signatures is not enumerated. → **Fixed:** `function-library.md` § "Host-Context Function Inventory" now enumerates `apply_path_mapping` with both runtime and stub behavior.
- **Keyword rename reverse mapping**: `parser.md` describes the forward pass; the evaluator-side reverse lookup (in `eval_attribute`) is not documented. → **Fixed:** `parser.md` § "Reverse mapping at evaluation".
- **`from_str_coerce` in values.md**: One line, but used at parameter binding and deserves a rules table analogous to the target-type coercion section. → **Fixed:** `values.md` now has a full per-target-type rules table.

### 3. Spec quality issues (unclear, redundant, missing rationale)

> **Status (2026-04-17 follow-up):** All items below addressed. Each bullet
> carries a **→ Fixed** note indicating the resolution.

**Unclear:**
- evaluator.md BoolOp semantics conflates the null-coalescing rule ("only `null` and `false` are falsy") with a one-off special case. Reword to state the rule once. → **Fixed:** BoolOp section rewritten to state the falsy rule as a single principle with the Python divergence stated separately; the `Param.Flag or true` special case removed.
- function-library.md "skip receiver coercion for methods" — the distinction between a method call and a function call happens in the evaluator before dispatch; spec should say how the library knows which it is. → **Fixed:** Three-Phase Dispatch section now describes the UFCS-plus-flag mechanism.
- format-string.md `resolve` return type ("preserves typed values for single-expression strings") — document the exact precondition (zero literal segments, exactly one expression segment). → **Fixed:** precondition stated explicitly ("exactly one expression segment and zero literal segments").
- symbol-table.md only shows `get`; `get_value` and `get_string` coexist and have different semantics. → **Fixed:** Accessor summary table added with all four accessors and when to use each.

**Redundant:**
- "Why FormatString lives here" rationale is duplicated in `architecture.md` and `format-string.md`. → **Fixed:** removed from `format-string.md`; now a one-line reference to `architecture.md`.
- "Divergence from Python" sections repeat in most spec files. → **Partially fixed:** the low-value one in `range-expr.md` (just "structurally identical") removed; the remaining ones each carry real content and were kept.
- Implicit coercion tables (INT→FLOAT, PATH→STRING) appear in `type-system.md`, `values.md`, and `function-library.md`. → **Fixed:** `values.md` Dispatch Coercion subsection now references `type-system.md` as the canonical source rather than restating rationale.

**Missing rationale:**
- path-mapping.md has only a single sentence on *why* URI paths are opaque. Expand with round-trip fidelity, authority preservation, `%`-encoded segments. → **Fixed:** URI Path Operations § "Why URI paths are opaque" now covers all three.
- range-expr.md contiguous-flag bit-packing rationale mentions "struct size" but not the memory-per-task cost in chunking. → **Fixed:** rationale quantifies cost as "8 MB per million-task step" in concrete terms.
- values.md Float64 invariants (no NaN/Inf/-0.0) should restate rationale: determinism, cross-language parity, hashability. → **Fixed:** all three rationales now spelled out.
- error-formatting.md smart-caret positioning is a table of decisions; add the underlying principle ("point at the operator or name that failed"). → **Fixed:** principle now stated above the table.

### 4. Implementation — Rust quality, API ergonomics

**Strengths:**
- Builder pattern on `Evaluator` with `#[must_use]` on builders.
- `ParsedExpression::evaluator(&symtabs)` pre-configures keyword renames and source context.
- `symtab!` macro + `From<T>` impls for `ExprValue` make construction ergonomic.
- `ExpressionErrorKind` is `#[non_exhaustive]` — forward-compatible.
- `Cow` used appropriately in `ExprValue::as_str_repr`.
- `SerializedSymbolTable` cleanly separates transport format from in-memory structure.

**Friction points:**
1. `FormatString::resolve` has six entry points (`resolve`, `resolve_with_format`, `resolve_string`, `resolve_string_with_format`, `resolve_typed`, `resolve_typed_with_format`). Consider a `FormatStringOptions` builder. **Done** — replaced with two methods (`resolve_with`, `resolve_string_with`) that take `&FormatStringOptions<'_>`; the six pairwise methods have been removed.
2. `ExprValue::coerce` and `from_str_coerce` return `Result<_, String>`, not `Result<_, ExpressionError>`. Callers repeatedly wrap via `.map_err(ExpressionError::new)` (~10 sites in `evaluator.rs`).
3. Three error types coexist in related code paths: `ExpressionError`, `SymbolTableError`, `Result<_, String>` from `SerializedSymbolTable::to_symtab`. Unify via `From` impls.
4. `Float64(pub f64, pub Option<Box<str>>)` has public tuple fields — lets callers bypass the no-NaN/no-Inf invariant enforced in `Float64::new`. Make fields private.
5. `ExprValue::Path { value, format }` has public fields; the separator-normalization invariant is only enforced by `new_path`. Encapsulate. **Done** — variant marked `#[non_exhaustive]`, so external crates can no longer construct it directly (E0639); all construction is funneled through `ExprValue::new_path`. External pattern matches use `..` per the `#[non_exhaustive]` convention.
6. `SymbolTable::all_paths(&self, prefix, out: &mut Vec<String>)` is an out-parameter API; return `Vec<String>` or an iterator. **Done** — signature is now `fn all_paths(&self, prefix: &str) -> Vec<String>`; out-parameter plumbing removed from the three call sites.
7. `SymbolTable::from_pairs` takes `Vec<(&str, ExprValue)>` by value; `IntoIterator` would be more flexible. **Done** — signature is now `fn from_pairs<'a, I>(pairs: I) -> Result<Self, _> where I: IntoIterator<Item = (&'a str, ExprValue)>`. Existing `from_pairs(vec![...])` sites are unaffected (`Vec: IntoIterator`).
8. `FunctionEntry::implementation` is a bare `fn` pointer, not `Box<dyn Fn>`. This blocks closure-based registration — document whether deliberate (performance) or a future change. **Done** — changed to `Arc<dyn Fn(...) + Send + Sync>` (via `FunctionImpl` type alias). `register` and `register_sig` now accept `impl Fn + Send + Sync + 'static`, enabling closure registration with captured state. `Arc` (not `Box`) preserves `FunctionLibrary: Clone`.

**Naming consistency:**
- Three parallel `evaluate` entry points: `lib::evaluate_expression`, `ParsedExpression::evaluate(&symtab)`, `Evaluator::evaluate(&ast)`. Consider disambiguating (e.g., `eval_once`/`eval_ast`). **Fixed** — `Evaluator` is now crate-private. The public API is `evaluate_expression(expr, &symtab)` for the simplest case, `ParsedExpression::evaluate(&symtab)` for a single symbol table, and `parsed.with_*(...)....evaluate(&symtabs)` for full configuration. The `with_*` builder methods now live directly on `ParsedExpression` and return a `EvaluationBuilder` that defers taking symbol tables until its terminal `.evaluate(&[&symtab])` / `.evaluate_with_metrics(&[&symtab])` call.
- `FormatString::resolve_string` vs `resolve` — only differ in return type. `resolve_to_string`/`resolve_to_value` would be clearer.
- `ExprValue::Path { value, format }` struct-variant while `ListPath(Vec<String>, PathFormat, usize)` is tuple-variant with identical semantics. Unify.
- `SerializedSymbolTable::to_symtab` returns `Result<_, String>` while `Deserialize` returns a structured error.

### 5. Error message quality

> **Status (2026-04-18 follow-up):** Four of the six gaps addressed; two closed
> as not-reproducible. See §19 for details. Gap 5 was flagged at a line that no
> longer exists in the codebase, and Gap 4 was fixed by a prior refactor that
> changed the parser's error type.

Per AGENTS.md, evaluation errors must include expression + caret indicator. This is largely delivered:
- `ExpressionError::with_node` attaches source + span + caret offset.
- `Display` emits message + expression + caret on separate lines.
- `compute_caret_offset` handles `BinOp`, `Attribute`, `Call`, `Subscript` intelligently (points at the operator).
- The if/else both-branches-fail message in `eval_ifexp` manually renders nested carets.

**Gaps:**
1. `ExpressionError::new(...)` (Other kind) is used ~70 times. Many could be structured kinds (`TypeError`, `UnsupportedSyntax`). Examples in `range_expr.rs` all go through `new`, losing programmatic error classification. **Partially fixed** — added `ExpressionError::parse_error` convenience constructor and converted all 18 `range_expr.rs` sites to `ParseError` kind. Remaining ~52 sites elsewhere in the crate are out of scope for this pass.
2. `FormatString::parse_segments` errors (`"Failed to parse interpolation expression at [X, Y]..."`) don't flow through `with_span`, so no caret on the raw format string input. **Fixed** — all four error paths in `parse_segments` now attach the raw format-string source via `with_span`, so users see a caret on the failing interpolation.
3. `SymbolTableError` has no `impl From` for `ExpressionError`, so errors at `SymbolTable::set` lose context when bubbled up. **Fixed** — `impl From<SymbolTableError> for ExpressionError` added. The full original message is preserved verbatim.
4. `RangeExprError` has `position` but `From<RangeExprError>` stringifies and discards it. **Not reproducible** — the range parser no longer returns `RangeExprError`; `RangeExpr::from_str` has `type Err = ExpressionError` directly, so the `From<RangeExprError>` impl is dormant (kept only for external callers who construct `RangeExprError` themselves). The `From` impl was still defensively upgraded to attach `with_span`, but it has no effect on any in-tree call path. The original concern was fixed by an earlier refactor that changed the parser's error type; the caret on `range_expr(...)` failures now comes from the evaluator attaching the outer-call AST span, which is the standard flow.
5. `function_library.rs:1109` uses `type_strs.last().unwrap()` — safe but a structured `if let Some(last)` is more defensive. **Not reproducible** — no `type_strs.last().unwrap()` in the current codebase; the code path was restructured in an earlier pass.
6. **Three places reimplement caret rendering**: `Display for ExpressionError`, `message_with_expr_prefix`, and the if/else both-branches block in `eval_ifexp`. Extract one `fn render_caret_line(expr, col, end_col, caret_offset)` helper. **Fixed** — extracted `pub(crate) fn write_caret_line(w, col, end_col, caret_offset)` in `error.rs`; all three sites now delegate to it. As a side effect, the if/else renderer's output is now consistent with `Display` (it previously used a different layout that omitted the leading tildes before the caret).

### 6. Performance concerns

Ordered by estimated impact:

| # | File:line | Concern | Complexity | Status |
|---|---|---|---|---|
| 1 | `eval/evaluator.rs:566,619,708,...` | `dispatch_with_node(Some(&ast::Expr::BinOp(b.clone())))` — clones entire AST subtree on every binop/unaryop/compare/subscript just to get a range for a possible error. | O(subtree size) per op | |
| 2 | `symbol_table.rs:128,158` | Every `get`/`set` allocates `Vec<&str> = key.split('.').collect()`. Runs millions of times during format-string resolution. | O(N) allocation per call | |
| 3 | `format_string.rs:454-493, 543-570` | `SimpleName` segments re-parse via `ParsedExpression::new(name)` on every `validate_expressions`, `copy_used_symtab_values`, `accessed_symbols` call. Cache at construction. | Extra parse per call | **Fixed** — `SimpleName` removed in favor of parsing every `{{...}}` into a `ParsedExpression` at construction. `validate_expressions`, `copy_used_symtab_values`, and `accessed_symbols` all use the cached `ParsedExpression` now. No runtime fast path was re-added; the tradeoff (one extra evaluator setup per simple-name resolve) is considered acceptable until benchmarking says otherwise. |
| 4 | `eval/evaluator.rs:1180-1187` | `eval_listcomp` clones entire `symtabs` Vec per iteration of the loop. | O(L × S) |
| 5 | `eval/evaluator.rs:1035-1055` | `eval_list` uses `seen_types: Vec<ExprType>` with linear `contains()` per element. | O(n²) for large literals |
| 6 | `types.rs:229` | `normalize_union` sorts by `a.to_string()` — renders each type to a string for sorting. | O(n × render) |
| 7 | `value.rs:250` | `make_list` calls `has_list_int`, `has_list_float`, `has_list_path`, `has_list_string` — four separate `iter().any()` passes. | 4× pass |
| 8 | `value.rs:706-736` | `list_iter::next` clones each `String`/`Float64`/`ExprValue` element. Consider a borrowing iterator for read-only passes. | O(1) alloc/element |
| 9 | `edit_distance.rs` | `suggest_closest` runs O(m×n) DP against every available name. Add length-difference early rejection. | O(K × L²) |
| 10 | `error.rs:273-292` | `Display` does `expr.split('\n')` on every render — fine, but cache line info on `with_node`. | O(N) per display |

Overall algorithmic design is reasonable. No O(N²) found where O(N) was clearly achievable in a hot path, except `eval_list`'s type-promotion loop.

### 7. Unwrap / panic / unsafe audit

- **`unsafe`**: 0 occurrences across the crate.
- **`panic!`**: 1 — `function_library.rs:125` `register_sig` on a bad signature literal. Only called from in-crate registration at startup.
- **`unreachable!`**: 3 — `evaluator.rs:650` (Invert guarded above), `evaluator.rs:1275` (comprehension var match guarded by `matches!`), `value.rs:293` (make_list post-promotion invariant).
- **Non-test `.unwrap()` / `.expect()`**: Every occurrence has a preceding length/type check or is a safe integer→f64 cast (`Float64::new(i as f64)` — integers are never NaN/Inf). Specific sites reviewed: `symbol_table.rs:151,312,347,422`; `types.rs:165,231,376,379,567`; `function_library.rs:423,531,558`; `evaluator.rs:492,1110,1168`; `value.rs:285,309,493,741`; `range_expr.rs:277,282`; `functions/misc.rs:116`; `functions/comparison.rs:162`; `functions/list.rs` (`expect("is_list() was true")`). All have provable preconditions.

**Conclusion:** No panic paths reachable from malformed input.

### 8. Integer overflow / undefined behavior

All i64 arithmetic uses `checked_*` and returns `IntegerOverflow`. Confirmed by live probes:
- `-9223372036854775808 // -1` → `ERR: Integer overflow` ✓
- `-9223372036854775808 % -1` → `ERR: Integer overflow` ✓
- `-9223372036854775808 // 2` → `-4611686018427387904` ✓

**Minor concerns (not exploitable from the expression language in practice):**
- `arithmetic.rs:173` `floordiv_float` uses `v.abs() > i64::MAX as f64`. `i64::MAX as f64` rounds to `9223372036854775808.0` (just above `i64::MAX`). Probe: `9.223372036854775e18 // 1.0` succeeds returning `9223372036854774784` (the f64 roundtrip value) — not crashing, but the bound is 1 ULP loose. Use `v >= i64::MAX as f64 || v < i64::MIN as f64` as `conversion.rs:int_from_float` already does, for consistency.
- `arithmetic.rs:254,319` `s.len() * (*n as usize)` and `elements.len() * n as usize` can overflow `usize` on 32-bit targets. Not exploitable today (operation limit catches it in practice — probe `'a' * 9223372036854775807` → `Operation limit exceeded`). Still, a `checked_mul` guard is cleaner.
- `range_expr.rs:109,127` — `(end - start)` and `(end - start) / step + 1` can overflow i64/usize for directly-constructed `IntRange` instances. String-parsed ranges are bounded, but a fuzz input via `from_ranges` could trigger it. Add `checked_sub`/`checked_div` in `IntRange::new`.
- `functions/comparison.rs:140-150` `collect_indices` does `idx += step` in a loop without `checked_add`. Practically unreachable (indices are bounded by the container) but defensive `checked_add` is cheap.
- `functions/math.rs:87` `round_half_even` does `rounded as i64` without pre-validating that `rounded` is in range. Callers reject NaN/Inf so it's safe in practice; document the precondition.

### 9. Pow `u32`-truncation (investigated, not a bug)

I suspected `pow_int`'s `checked_pow(exp as u32)` could silently produce wrong results for exponents > `u32::MAX`. The guard is `if *exp > 63 && !matches!(*base, -1..=1) { overflow }`. For bases `{-1, 0, 1}` the result depends only on parity (for -1) or is trivially 1/0 (for 0, 1). `u32` truncation of a positive i64 preserves parity (since `2^32` is even). Verified by probe:
- `(-1) ** 4294967296` → `1` ✓ (even exponent)
- `(-1) ** 4294967297` → `-1` ✓ (odd exponent)
- `(-1) ** 1000000000001` → `-1` ✓

Not a bug. However, adding a comment in `arithmetic.rs:121` explaining why `exp as u32` is safe here would help future maintainers.

### 10. Confirmed minor finding: regex validator coverage

The regex validator `validate_regex_pattern` explicitly enumerates `\1`–`\5` as rejected backreferences. Python regex supports up to `\99`, and the underlying Rust `regex` crate rejects them all at compile time but with a less friendly message. Live probes:

```
re_match('aa', '(a)\\1')                             → ERR: Unsupported regex feature: backreferences  ✓
re_match('abcdeff', '(a)(b)(c)(d)(e)(f)\\6')         → ERR: Invalid regex: regex parse error: ...       ✗ (message quality)
re_match('abcdefghii', '(a)...(i)\\9')               → ERR: Invalid regex: regex parse error: ...       ✗ (message quality)
```

Users of `\6`–`\9` get a lower-quality error than users of `\1`–`\5`. Extend the list in `functions/regex.rs:56-62` to cover `\6`–`\9`.

### 11. Test suite — organization and coverage

**Strengths:**
- 36 test files mirror Python reference suite structure, with `// === Section ===` headers that map to Python `TestXxx` classes.
- `test_error_formatting.rs` (1,067 lines) is a canonical example of the AGENTS.md gold standard.
- `test_int64_bounds.rs`, `test_memory.rs`, `test_operation_limit.rs`, `test_unicode_codepoint.rs` cover hard edge cases well.
- `test_unresolved_eval.rs` (1,454 lines) systematically verifies unresolved propagation through every operator and function.
- `test_ast_validation.rs` covers rejection of lambda/tuple/dict/set/walrus/starred/f-strings/bitwise ops.

**Weaknesses:**
- ~36 per-file copies of `fn eval`, `fn eval_err`, `fn assert_err` with subtle variations. A shared `tests/common/` module would remove ~250 lines of boilerplate and prevent drift.
- Every file has `#[allow(dead_code)]` helpers (`eval_ok`, `eval_fails`, `parse_fails`) copy-pasted but unused — signals template-reuse rather than curated imports.
- Some files mix end-to-end `evaluate_expression` tests with direct function-pointer invocations through hand-rolled `Ctx` — should be split into separate files.

### 12. Spec-to-test traceability gaps

Found by the test reviewer:

1. **symbol-table.md** — no test that JSON-serializes a symtab containing `Unresolved` entries and verifies they are skipped. The reverse-order conflict (`set("A", int); set("A.B", int)`) error message isn't asserted. `accessed_symbols()` walk stopping at a value entry mid-path isn't tested.
2. **format-string.md** — `validate_comprehension_vars` (let-binding shadowing) has no direct test. `escape_format_string` utility has no round-trip assertion. `FormatStringValidationError` start/end offsets aren't asserted at mid-string positions. No test for multi-byte UTF-8 format-string input where `{{` appears at a non-ASCII column.
3. **error-formatting.md** — Only `OperationLimitExceeded` is asserted via `matches!(err.kind(), …)`. The other 12 `ExpressionErrorKind` variants are exercised only through string matching on the message, never through the typed `.kind()` accessor.
4. **path-mapping.md** — `apply_with_format` POSIX-source → Windows-output with drive-letter synthesis is not asserted in detail. URI `scheme://Host:PORT` with port case-insensitivity isn't probed. `source_path == destination_path` identity mapping and multi-rule ordering ("first match wins") aren't tested.
5. **function-library.md regex** — `\z` (lowercase), `(?P=name)`, `(?(…)` conditional patterns are guarded in source but not individually asserted. `re_sub` `\g<name>` and `${1}` replacement rejection is only partially covered.
6. **values.md** — The tag-based hash strategy's `HashMap` interchangeability (`Int(1) == Float(1.0)` → same bucket) is claimed but not asserted via actual `HashMap::get`.
7. **evaluator.md** — Regex cache per-evaluation semantics have no test; a regression would be silent (perf only). `as_name_lookup` fast path isn't directly exercised.

### 13. Edge-case coverage assessment

**Well-covered:** Unicode codepoint vs byte counting, i64 overflow at literal and arithmetic level, floored-division/modulo invariants with nextafter boundaries, empty lists (`sum([])`, `any([])`, `len([])`), memory/operation bounds, multi-line expressions.

**Weak or missing:**
- Deep nesting (parser tests go 2 deep; no 100-deep stress). Probe: `((((...(((1)))...)))` with 200 parens → succeeds.
- Very long strings / large lists — no 1 MB literal tests.
- Systematic regex edge cases (empty pattern, pattern-of-dot-star).
- Boundary slice indices: `x[i64::MIN:i64::MAX]` wrap-around; `x[0:0:0]` error path confirmed via probe but no test.
- Symbol-table dotted keys containing spaces, unicode, or reserved-word first segment (`"class.x"`).
- Path mapping with trailing slashes on both source and destination.
- `range(INT64_MIN, INT64_MAX)` length-overflow computation.
- Float subnormal boundary (`f64::MIN_POSITIVE` arithmetic).
- `round(x, n)` with `n > 300`.
- Method chain depth (`'x'.upper().lower()…` nesting).

### 14. Error-message-assertion compliance (AGENTS.md rule)

Approximately 60% of error tests meet the gold standard (message + expr + caret as concatenated slices). ~40% fall short:

1. **Substring-only** (`e.contains("Integer overflow")`, no expression/caret):
   - `test_arithmetic.rs` "Bug 1/3" additions use `assert_err(..., &["Integer overflow"])` — only message.
   - `test_int64_bounds.rs` `negate_int_min_via_variable`, `int_from_large_float`, `int_from_string_overflow`, floor/ceil/round large-float tests.
2. **`.message()` calls** — strip caret/expr explicitly:
   - `test_parsing.rs` — nearly every `reject_*_at_parse` test.
   - `test_comparison.rs` `eval_err` helper.
   - `test_unresolved_eval.rs` helper.
3. **`.is_err()` only** — 27 occurrences across 15 files. Examples: `eval_fails("2 ** 63")`, various path_mapping tests, `test_list_nesting.rs`, `test_unicode_codepoint.rs` dead-code helper.
4. **Partial** — expression line present but no caret line: portions of `test_comparison.rs` cross-type ordering tests.

**Highest-value remediation:** convert `test_int64_bounds.rs` "Bug N" cases to full 3-part asserts (caret is the whole point of overflow diagnostics); upgrade ~25 `reject_*_at_parse` tests in `test_parsing.rs`; fix `test_list_nesting.rs`, `test_misc_builtins.rs`, `test_format_strings.rs` substring checks.

### 15. Redundant / weak tests

- `test_error_formatting.rs` contains near-duplicate pairs: `multiline_error_shows_correct_line` + `_v2` (identical expression), `multiline_error_in_parens` + `error_in_parentheses_multiline`, `multiline_error_in_list` + `error_in_list_multiline`, `multiline_error_on_first_line` + `error_on_first_line_multiline`, `exact_int_conversion_error_format` + `function_call_error_caret` (same `"int('bad')"`), `float_literal_infinity` + `float_literal_infinity_in_expression`.
- `eval_fails("2 ** 63")` (`test_int64_bounds.rs::pow_overflow`) is redundant given `pow_overflow_64` which asserts the full message.
- `test_parsing.rs` `stmt_break`, `stmt_continue`, `stmt_pass` use `eval_fails` — only confirm some error; the spec enumerates each specific rejection message.
- `test_misc_builtins.rs::path_from_list_with_non_string` swallows error text.

### 16. Flakiness / isolation

No time-sensitive, network, or filesystem-dependent tests in the expr crate.

Brittleness notes:
- `test_memory.rs::peak_memory_for_string` compares absolute bytes (`ev_size + 50*8`, `< 50_000`). A minor struct-layout change (e.g., adding a field to `Float64`) would break it.
- `test_operation_limit.rs::operation_count_*` hard-codes exact counts like `op_count("sum(range(10))") == 22`. Correct today but brittle to refactoring.
- Platform sensitivity: some tests in `test_misc_builtins.rs` don't explicitly set `with_path_format(PathFormat::Posix)` and may behave differently on Windows.

---

## Live bug-hunt probes (results)

I ran targeted expression probes against the built crate to test hypothesized bugs. Summary:

| Hypothesis | Probe | Result |
|---|---|---|
| `INT_MIN // -1` silently succeeds | `-9223372036854775808 // -1` | ✓ Properly errors |
| `INT_MIN % -1` silently succeeds | `-9223372036854775808 % -1` | ✓ Properly errors |
| `INT_MIN // 2` miscomputes | `-9223372036854775808 // 2` | ✓ Returns `-4611686018427387904` |
| `(-1) ** 2^32` truncates wrongly | `(-1) ** 4294967296` | ✓ Returns `1` (parity-preserving) |
| `(-1) ** (2^32 + 1)` truncates wrongly | `(-1) ** 4294967297` | ✓ Returns `-1` (parity-preserving) |
| Float floor-div boundary precision | `9.223372036854775e18 // 1.0` | OK: `9223372036854774784` (f64 roundtrip; not crash but bound is loose) |
| Regex `\1` backreference rejection | `re_match('aa', '(a)\\1')` | ✓ `ERR: Unsupported regex feature: backreferences` |
| Regex `\6` backreference rejection | `re_match('abcdeff', '(a)...(f)\\6')` | ✗ Falls through to `Invalid regex: regex parse error` (validator only covers `\1`–`\5`) |
| Regex `\9` backreference rejection | `re_match('...', '...\\9')` | ✗ Same as above |
| Regex `\Z` anchor rejection | `re_match('a', 'a\\Z')` | ✓ `Unsupported regex feature: end-of-string anchor \Z` |
| Regex `\z` anchor rejection | `re_match('a', 'a\\z')` | ✓ `Unsupported regex feature: end-of-string anchor \z` |
| Lookahead rejection | `re_match('ab', '(?=a)b')` | ✓ Rejected |
| Range slice step 0 | `range_expr('1-10')[0:10:0]` | ✓ `Slice step cannot be zero` |
| String slice step 0 | `'abc'[0:3:0]` | ✓ Rejected |
| List slice step 0 | `[1,2,3][0:3:0]` | ✓ Rejected |
| Negative index on empty | `[][-1]`, `''[-1]` | ✓ Proper bounds error |
| Zero-to-negative-power | `0 ** -1` | ✓ `Cannot raise zero to a negative power` |
| String repeat huge (n=i64::MAX) | `'a' * 9223372036854775807` | ✓ `Operation limit exceeded` (caught by budget) |
| 200-deep nested parens | `(((...(1)...)))` | ✓ Returns `1` (parser handles it) |

**Only one real bug confirmed:** regex validator coverage of `\6`–`\9` backreferences produces a poorer error message than `\1`–`\5`. Severity: low (user-facing clarity).

---

## Recommendations (prioritized)

### P1 — Highest value, low risk

1. **Consolidate caret rendering** into one `fn render_caret_line(f, expr, col, end_col, caret_offset)` helper used by `Display for ExpressionError`, `message_with_expr_prefix`, and the if/else both-branches-fail block in `eval_ifexp`.
2. **Eliminate AST `.clone()` at error sites** in `evaluator.rs`. Change `ExpressionError::with_node` (or add `with_range`) to accept a `TextRange` directly. Removes an entire class of hot-path allocations.
3. **Cache `SimpleName` parse results** in `FormatString::Segment`. Store the accessed-symbols set at construction; remove repeated `ParsedExpression::new(name)` from `validate_expressions` and `copy_used_symtab_values`.
4. **Extend regex validator** to reject `\6`–`\9` backreferences for symmetry with `\1`–`\5` and to deliver a high-quality error message.
5. **Replace `Vec<&str> = key.split('.').collect()` with iterator chains** in `SymbolTable::get`/`set` hot paths.
6. **Unify error types.** `impl From<SymbolTableError> for ExpressionError`; change `ExprValue::coerce`/`from_str_coerce` to return `ExpressionError`. Removes ~10 `.map_err(ExpressionError::new)` call sites.

### P2 — Spec accuracy

> **Status (2026-04-17 follow-up):** Items 7–13 below have been addressed. Item 14
> (CI check on rustdoc snippets) remains open.

7. ~~Fix the 19 spec-vs-impl mismatches in table 1. Highest-impact items: builder example `evaluate(&parsed.ast)`, `ExpressionError` private fields + struct-variant kinds, `rule.apply → Option<String>`, `uri_path` function names without `uri_` prefix, `SymbolTable::get` accessor family.~~ **Done.**
8. ~~Add specs for `functions/path_parse.rs` and `edit_distance.rs`.~~ **Done** (`specs/expr/path-parse.md`, `specs/expr/edit-distance.md`).
9. ~~Update the `eval/mod.rs` doc comment to reference `ruff` not `rustpython-parser`.~~ **Done.** (Also fixed stale `specs/parser-selection.md` references in `lib.rs`, `Cargo.toml`, `AGENTS.md`, `specs/architecture.md` — they now point at `specs/expr/parser.md`.)
10. ~~Remove references to non-existent `ExprType::LIST_INT`.~~ **Done.**
11. ~~Document `FormatString::Segment::SimpleName` fast-path variant.~~ **Done** (and subsequently obviated — `SimpleName` has since been removed; see §6 row 3).
12. ~~Consolidate "divergence from Python" appendix sections to reduce per-file repetition.~~ **Partial** — the low-value `range-expr.md` one was removed; the others were kept because they carry real information.
13. ~~Consolidate implicit-coercion tables to a single canonical location.~~ **Done** (`values.md` now references `type-system.md`).
14. Add a CI check that extracts and typechecks rustdoc-style code snippets from spec markdown. **Open.**

### P3 — Test quality

15. Create `tests/common/` with shared `eval`, `eval_err`, `assert_err` helpers. Remove the ~36 per-file copies.
16. Upgrade all error tests to the AGENTS.md standard: message + expression + caret. Priority files: `test_int64_bounds.rs`, `test_parsing.rs` (`reject_*_at_parse`), `test_list_nesting.rs`, `test_misc_builtins.rs`, `test_format_strings.rs`.
17. Add tests for the coverage gaps in section 12 (structured `.kind()` assertions for non-`OperationLimit` variants; serialized-symtab Unresolved skipping; path-mapping multi-rule ordering; regex `\z`, `(?P=name)`, conditional patterns; make_list empty-list hint type for BOOL/FLOAT; HashMap interchangeability of Int/Float tags).
18. Remove duplicate tests in `test_error_formatting.rs` (`*_v2` pairs with identical expressions).
19. Add stress tests: 100-level nested expressions, multi-megabyte string literals, UTF-8 column-offset accuracy in format strings.
20. Consider a proptest/fuzz target for expression evaluation with random valid subsets of the grammar.

### P4 — Polish

21. Make `Float64` fields private; require `Float64::new`/`with_str` for construction; provide `.value()` and `.display_str()`.
22. Consider `Arc<FunctionLibrary>` instead of `&'a FunctionLibrary` to decouple evaluator lifetime from library borrow.
23. Shrink `ExprValue` enum size (box `ExprType` for `Unresolved` and the `ListList` tail).
24. Add `#[inline]` to tiny hot-path helpers (`Float64::value`, `ExprValue::is_list`, `Evaluator::count_op`).
25. Use `smallvec` for `arg_types`/`args`/`symtabs` slices (almost always 1–3 elements).
26. Add length-difference early rejection in `edit_distance::suggest_closest`.
27. Fold `make_list`'s four `has_*` passes into one.
28. Add `#[must_use]` to `FunctionLibrary::merge` (currently missing, despite being a builder).
29. Document or remove `#[allow(dead_code)]` helpers in test files.
30. Add `checked_mul` guard in `arithmetic.rs` string/list multiply and `checked_sub` in `IntRange::new` for defense in depth on 32-bit targets.

---

## Overall assessment

`openjd-expr` is a well-engineered crate with careful attention to correctness, error quality, and resource bounding. The design is principled (builder pattern, typed list variants for memory efficiency, non-materializing range slicing, caret-aware errors, bounded execution). **No unsafe code, no reachable panics on malformed input, integer overflow consistently handled.** 2,998 tests pass cleanly.

The largest remaining opportunities are cosmetic but important to downstream quality:
- Specifications have drifted from the implementation; a coordinated update pass is warranted before any external publication.
- A non-trivial fraction of error tests do not exercise the AGENTS.md-mandated full-message-plus-caret assertion pattern.
- Hot-path `.clone()` calls on AST subtrees for error-range capture are a performance footgun worth removing.

No blockers for crate release readiness.

---

## Files touched

- **Created:** `~/openjd-rs/reports/expr-quality-evaluation-report.md` (this document).
- **Unmodified:** All source, spec, and test files in `~/openjd-rs`. Review was read-only.
- **Temporary probe package** at `/tmp/probe` (can be discarded).

---

## 17. Changes applied

A follow-up pass on 2026-04-17 addressed every finding in sections 1, 2, and 3.
Summary of the edits by file:

### Spec files (under `~/openjd-rs/specs/expr/`)

- **`architecture.md`** — added `functions/path_parse.rs` and `functions/string.rs` to the module layout; documented `peak_memory_usage`/`operation_count` fields on `ParsedExpression`; linked `edit-distance.md` next to the `edit_distance.rs` module entry.
- **`evaluator.md`** — corrected the builder example to pass `&parsed.ast` to `Evaluator::evaluate`; changed BoolOp unresolved result from `Unresolved(union[a,b])` to `Unresolved(BOOL)` with short-circuit note; tightened the falsy-definition wording to state the rule once.
- **`error-formatting.md`** — made `ExpressionError` fields private in the struct shown and documented the accessor methods; rewrote `ExpressionErrorKind` to show the actual struct-variant shapes (data-carrying); corrected `integer_overflow()` (no arg) and `division_by_zero(op)` convenience constructors; added the smart-caret principle ("point at the operator or name that failed") above the decision table; linked to `edit-distance.md`.
- **`format-string.md`** — added the `SimpleName { start, end, name }` segment variant with its fast-path rationale; documented `library: Option<&FunctionLibrary>`; documented the exact precondition for typed passthrough (zero literal segments, exactly one expression segment); removed the duplicated "Why FormatString lives here" section in favor of a one-line reference to `architecture.md`.
- **`function-library.md`** — added the `host_context_enabled: bool` field to the `FunctionLibrary` struct; replaced `arithmetic::library()`-style references with the actual module-private `arithmetic()` free functions; replaced `ExprType::LIST_INT` with `ExprType::list(ExprType::INT)`; documented the `RegexBuilder` 1 MiB size-limit default; added a "Host-Context Function Inventory" subsection enumerating `apply_path_mapping` with both runtime and unresolved-stub behavior; clarified the method-vs-function distinction at dispatch time (UFCS flag).
- **`parser.md`** — added a "Reverse mapping at evaluation" subsection covering how `keyword_renames` unwraps placeholder identifiers at attribute-lookup time and in error messages.
- **`path-mapping.md`** — corrected `apply(path) -> Option<String>` signature; replaced the `uri_`-prefixed function-name table with the actual `uri_path::name`/`parent`/`parts`/etc. names; added a "Why URI paths are opaque" rationale covering round-trip fidelity, authority preservation, and percent-encoded segments.
- **`range-expr.md`** — corrected "original direction stored separately" to reflect that `IntRange::new` normalizes to canonical ascending form and discards direction; expanded the contiguous-flag bit-packing rationale to quantify per-task memory cost (8 MB for a million-task step); removed the low-value "Divergence from Python" appendix that only said "structurally identical".
- **`symbol-table.md`** — corrected `get` to return `Option<&SymbolTableEntry>` and added an accessor summary table for `get` / `get_value` / `get_string` / `get_table`; expanded the `Unresolved`-skipping paragraph to explain why the transport format is intentionally value-only.
- **`values.md`** — removed the false `.item()` panic claim and replaced it with accurate unresolved-propagation behavior; added a full `from_str_coerce` rules table (per-target-type parsing behavior); expanded the Float64 no-NaN/Inf rationale to cover determinism, cross-language parity, and hashability; made the Dispatch Coercion subsection reference `type-system.md` as the canonical source.
- **`README.md`** — registered the two new spec files (`edit-distance.md`, `path-parse.md`).

### New spec files

- **`specs/expr/edit-distance.md`** (new) — documents `edit_distance()` (two-row DP Levenshtein on chars for UTF-8 correctness), `suggest_closest()` return formats, the `MAX_SUGGESTION_DISTANCE = 5` threshold, and the two call sites (unknown variable, unknown function).
- **`specs/expr/path-parse.md`** (new) — documents separator handling per `PathFormat`, anchor detection (incl. UNC shares), the public function surface (`sep`, `split`, `file_name`, `parent`, `file_stem`, `extension`, `parts`, `with_name`, ...), the integration table mapping each PATH property/method to its `path_parse` function, and the rationale for lexical (not filesystem-canonicalizing) normalization.

### Source / other

- **`crates/openjd-expr/src/eval/mod.rs`** — module doc comment updated from `rustpython-parser` to `ruff_python_parser`.
- **`crates/openjd-expr/src/lib.rs`** — updated reference from `specs/parser-selection.md` (file does not exist) to `specs/expr/parser.md`.
- **`crates/openjd-expr/Cargo.toml`** — same reference fix.
- **`AGENTS.md`** — same reference fix.
- **`specs/architecture.md`** — same reference fix (two occurrences).

### Verification

```
cargo build -p openjd-expr   → clean, no warnings
cargo test  -p openjd-expr --lib  → 267 passed; 0 failed
```

The only non-comment source change was correcting the `rustpython-parser` reference in the `eval/mod.rs` doc comment. No types, method signatures, field visibilities, or tests were altered. All finding categories in sections 1–3 are now resolved; findings in sections 4–16 (implementation quality, performance, error messages, tests, edge cases) remain as documented and are tracked by the P1/P3/P4 recommendations above.

---

## 18. API refactors applied (section 4 friction points)

A second follow-up pass on 2026-04-17 addressed five of the eight "Friction
points" listed in §4. The items picked were those the user requested as a
batch: item 1 (FormatString entry-point sprawl), item 5 (Path encapsulation),
item 6 (all_paths out-parameter), item 7 (from_pairs inflexible signature),
and item 8 (fn-pointer-only function registration).

### Summary by item

**4.1 `FormatString::resolve*` — consolidated under a builder.**
Six pairwise methods (`resolve`, `resolve_with_format`, `resolve_string`,
`resolve_string_with_format`, `resolve_typed`, `resolve_typed_with_format`)
removed and replaced with:

```rust
pub fn resolve_with(&self, symtab: &SymbolTable, opts: &FormatStringOptions<'_>) -> Result<ExprValue, _>;
pub fn resolve_string_with(&self, symtab: &SymbolTable, opts: &FormatStringOptions<'_>) -> Result<String, _>;
```

`FormatStringOptions::{new, default, with_library, with_path_format,
with_path_mapping_rules, with_target_type}` covers every axis previously
expressed as a separate method. `with_library` accepts
`impl Into<Option<&FunctionLibrary>>`, so callers plumbing through an
`Option<&FunctionLibrary>` don't need to unwrap first.

All workspace call sites migrated (openjd-model `create_job/{ranges, mod}`,
openjd-sessions `runner/embedded_files/session`, openjd-expr integration
tests).

**4.5 `ExprValue::Path` encapsulated.**
Variant marked `#[non_exhaustive]`. Downstream crates can no longer construct
it directly — `ExprValue::new_path(value, format)` is the only way, and it
enforces the separator-normalization invariant. Pattern matching still works
but must include `..` per the standard `#[non_exhaustive]` convention. 159
sites across the workspace were updated (construction → `new_path`, some
patterns → add `..`).

**4.6 `SymbolTable::all_paths` returns `Vec<String>`.**
Signature changed from `fn all_paths(&self, prefix, out: &mut Vec<String>)` to
`fn all_paths(&self, prefix: &str) -> Vec<String>`. The recursive helper
`collect_paths` keeps the out-parameter form privately, so there is no extra
allocation per recursion. Three call sites simplified.

**4.7 `SymbolTable::from_pairs` accepts `IntoIterator`.**
Signature changed from `Vec<(&str, ExprValue)>` to
`IntoIterator<Item = (&'a str, ExprValue)>`. All existing `from_pairs(vec![...])`
call sites compile unchanged (`Vec: IntoIterator`); new code can pass arrays,
`map()` chains, or any iterable directly.

**4.8 `FunctionEntry::implementation` accepts closures.**
Changed from a bare `fn` pointer to
`Arc<dyn Fn(...) -> Result<...> + Send + Sync>` (exposed as the
`FunctionImpl` type alias). `FunctionLibrary::{register, register_sig}` now
take `impl Fn + Send + Sync + 'static`, so closures with captured state can
be registered alongside plain functions. `Arc` (rather than `Box`) preserves
`FunctionLibrary: Clone`, which existing callers depend on.

### Spec updates

- `specs/expr/format-string.md` — "Resolution" section rewritten around the
  two new methods and `FormatStringOptions`; the old section-by-section
  walkthrough of the six pairwise methods is gone.
- `specs/expr/function-library.md` — `FunctionImpl` alias + closure
  registration example; "not closures" note replaced with the real
  `Arc<dyn Fn + Send + Sync>` rationale.
- `specs/expr/symbol-table.md` — accessor summary table lists
  `all_paths(prefix) -> Vec<String>`; construction example shows
  `IntoIterator` inputs (array, `map()` chain, `Vec`).
- `specs/expr/values.md` — `ExprValue::Path` marked `#[non_exhaustive]` in
  the enum shape; new "Path Encapsulation" subsection explaining the
  invariant and external-pattern-match requirement.

### New tests

Twenty new tests were added to exercise the new API contracts:

| Target | File | Count |
|---|---|---|
| `FormatStringOptions` | `src/format_string.rs` (unit) | 6 |
| `ExprValue::new_path` / pattern-match contract | `tests/test_expr_value.rs` | 4 |
| `SymbolTable::all_paths` / `from_pairs` | `tests/test_symbol_table.rs` | 6 |
| Closure-based registration / `Clone` / `Send + Sync` | `tests/test_function_library.rs` | 4 |

### Verification

```
cargo fmt --all -- --check                                       clean
cargo clippy --all-features --all-targets --workspace -- -D warnings  clean
cargo clippy --manifest-path crates/openjd-sessions/src/helper/Cargo.toml -- -D warnings  clean
cargo test --workspace                                           6032 passed; 0 failed
```

(6032 = 6012 before + 20 new tests.)

### Friction points still open

Three of the eight items in §4 were not addressed in this pass and remain on
the backlog: **4.2** (`coerce` / `from_str_coerce` return `Result<_, String>`
instead of `Result<_, ExpressionError>`), **4.3** (three error types —
`ExpressionError`, `SymbolTableError`, `String` — that could be unified via
`From` impls), and **4.4** (`Float64` tuple fields are `pub`, letting callers
bypass the NaN/Inf invariant enforced by `Float64::new`).

---

## 19. Error message quality pass

A third follow-up pass on 2026-04-18 addressed section 5. All six gaps are
either **fixed**, **partially fixed** (item 1, scoped to `range_expr.rs`), or
**closed as not reproducible** (item 5, the line it pointed at no longer
exists). Work order matched the pragmatic order from the report's follow-up
discussion: 5.6 → 5.2 → 5.3 → 5.4 → 5.1 → 5.5.

### 5.6 Consolidate caret rendering

The three parallel implementations in `Display for ExpressionError`,
`ExpressionError::message_with_expr_prefix`, and the if/else both-branches
block in `eval_ifexp` have been replaced with a single
`pub(crate) fn write_caret_line(w, col, end_col, caret_offset)` helper in
`error.rs`. All three sites now delegate to it, so any future layout change
propagates consistently.

A latent inconsistency surfaced during the refactor: the if/else renderer
previously drew `^` at `col + caret_off` with only trailing tildes, producing
e.g. `  ^~~~~` instead of the standard `  ~~^~~~~`. That discrepancy is now
gone, so `assert_err` tests that used substring matching on sub-expressions
still pass while pinning tests can now assert the exact caret-line format.

### 5.2 Format-string parse errors carry a caret

All four error paths in `FormatString::parse_segments` (missing opening
braces, brace mismatch mid-expression, missing closing braces, empty
expression, and the delegated `ParsedExpression::new` failure) now attach
the raw format-string source via `ExpressionError::with_span`. A user-facing
parse failure on `{{ 1 + }}` now renders:

```
Failed to parse interpolation expression at [N, M]. Reason: ...
  hello {{ 1 + }} world
        ^~~~~~~~~
```

### 5.3 `SymbolTableError` → `ExpressionError`

Added `impl From<SymbolTableError> for ExpressionError` that preserves the
full `Display` message. Call sites that bubble a `SymbolTable::set` failure
through `?` no longer need an ad-hoc `.map_err(...)` wrapper, and the
original "not a table" context is retained end-to-end.

### 5.4 `RangeExprError::position` (not reproducible)

Investigation revealed the range-expression parser no longer returns
`RangeExprError`: `impl FromStr for RangeExpr` has `type Err = ExpressionError`,
so the `From<RangeExprError>` impl that allegedly discarded `position` is
dormant — it is kept around only for external callers who might construct
`RangeExprError` themselves. The concern as literally written was fixed by an
earlier refactor that changed the parser's error type. The `From` impl was
defensively upgraded to attach `with_span` anyway, but this change has no
effect on any in-tree call path. Closed as not-reproducible.

Caret quality for `range_expr('…')` failures is unchanged and remains:

```
Expected integer in '1-xx,5'
  range_expr('1-xx,5')
  ^~~~~~~~~~~~~~~~~~~~
```

The inner parse detail (`Expected integer in '1-xx,5'`) is preserved in the
message text, and the outer caret points at the call that triggered the
failure. A future pass could attach both the outer and inner carets (similar
to the if/else both-branches renderer), but that was out of scope for §5.

### 5.1 `ExpressionError::parse_error` convenience + `range_expr.rs` uplift

Added `ExpressionError::parse_error(message)` as a structured constructor
for `ExpressionErrorKind::ParseError`. Converted all 18 `ExpressionError::new`
sites in `range_expr.rs` to `parse_error`, so callers can now pattern-match
on `err.kind() == ParseError(_)` for range-expression failures. The remaining
~52 `ExpressionError::new` sites elsewhere in the crate are out of scope for
this pass and tracked as follow-up.

### 5.5 Defensive `.unwrap()` at `function_library.rs:1109`

The original report called out `type_strs.last().unwrap()`. No such pattern
exists in the current codebase — it was removed in an earlier restructuring
pass. Closed as not-reproducible.

### New tests

| Target | File | Count |
|---|---|---|
| `ifelse` both-branches unified caret format | `tests/test_error_formatting.rs` | 1 |
| Format-string parse-error caret | `tests/test_error_formatting.rs` | 2 |
| `RangeExprError` caret + `ParseError` kind | `tests/test_error_formatting.rs` | 2 (pinning outer-call caret + ParseError kind) |
| `SymbolTableError` → `ExpressionError` conversion | `tests/test_symbol_table.rs` | 1 |

### Verification

```
cargo fmt --all -- --check                                       clean
cargo clippy --all-features --all-targets --workspace -- -D warnings  clean
cargo test --workspace                                           6038 passed; 0 failed
PATH=…/bin:$PATH uv run run_openjd_cli_tests.py '2023-09/*'       1042 passed; 0 failed
```

(6038 = 6032 before + 6 new tests. Full 2023-09 conformance suite green.)
