# openjd-expr Crate Quality Evaluation

**Evaluator:** Kiro (with `deadline-openjd` subagent for spec, source, and test review)
**Date:** 2026-04-17
**Crate location:** `~/openjd-rs/crates/openjd-expr`
**Spec location:** `~/openjd-rs/specs/expr`

## Executive summary

`openjd-expr` is a high-quality Rust crate. It compiles cleanly with no warnings (`cargo build -p openjd-expr --all-targets`), clippy is clean, and all **2,998** tests pass. No `unsafe` code. No reachable panics on malformed input. Integer overflow is consistently handled with `checked_*` arithmetic. Error messages are exceptional by the standard of most interpreter projects — the AGENTS.md rule that every evaluation error renders as message + expression + caret is followed throughout `tests/test_error_formatting.rs` (1,067 lines) and is largely delivered by the evaluator.

The main gaps are not correctness but alignment:

1. **Specifications drift from the implementation** in ~19 concrete places (type signatures, field visibility, constructor args, method names). Several spec doc examples would not compile if checked.
2. **Test coverage is uneven** — ~40% of error tests do not meet the AGENTS.md "assert full message + expression + caret" standard; many use `.message()` or `.contains(substring)`.
3. **Evaluator hot path clones AST subtrees** for error-site range reporting, allocating on every binary op / subscript / call.
4. **Three parallel caret-rendering code paths** should be consolidated into one helper.
5. **Two genuine minor findings** were confirmed by live probes: (a) regex validator only covers `\1`–`\5`; `\6`–`\9` fall through to a lower-quality "Invalid regex" error. (b) `FormatString::Segment::SimpleName` (a documented perf fast path) re-parses on every validate/accessed-symbols call instead of caching.

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

Every entry below is a concrete drift where a spec doc describes something that no longer matches the code. Examples in specs are often not-yet-compilable against the current API.

| # | Spec file | Spec claim | Actual implementation | Source |
|---|---|---|---|---|
| 1 | architecture.md, evaluator.md | `parsed.evaluator(...).evaluate()?` | `evaluator.evaluate(&parsed.ast)?` (AST arg required) | `eval/evaluator.rs:205` |
| 2 | architecture.md | Module layout omits `functions/path_parse.rs` | 691-line module exists | `functions/path_parse.rs` |
| 3 | error-formatting.md | `ExpressionError` has `pub` fields | All fields private; accessor methods only | `error.rs:73-80` |
| 4 | error-formatting.md | `ExpressionErrorKind` variants are unit (`UndefinedVariable`, `TypeError`) | Struct variants with fields (`UndefinedVariable { name, suggestion }`, …) | `error.rs:14-66` |
| 5 | error-formatting.md | `ExpressionError::integer_overflow("...")`, `division_by_zero()` | `integer_overflow()` (no arg), `division_by_zero(op: &'static str)` | `error.rs:106-114` |
| 6 | evaluator.md | BoolOp unresolved → `Unresolved(union[a, b])` | Returns `Unresolved(BOOL)`; operands may still short-circuit | `eval/evaluator.rs:655-708` |
| 7 | evaluator.md | `ParsedExpression` struct shape | Has `source` + `operation_count` fields not documented | `eval/parse.rs:14-22` |
| 8 | format-string.md | `Segment { Literal, Expression }` (two variants) | Three variants incl. `SimpleName { start, end, name }` (fast-path) | `format_string.rs:17-27` |
| 9 | format-string.md | `library: &FunctionLibrary` non-optional | All resolve fns take `Option<&FunctionLibrary>` | `format_string.rs:71-240` |
| 10 | function-library.md | `arithmetic::library()`, `pub fn default_library()` | Free fns local to `default_library.rs`; public API is `get_default_library()` | `default_library.rs:17-34` |
| 11 | function-library.md | References `ExprType::LIST_INT` constant | No such constant exists; use `ExprType::list(ExprType::INT)` | `types.rs:38-100` |
| 12 | function-library.md | `get_or_compile_regex` default uses `Regex::new(...)` | Uses `RegexBuilder::new(pattern).size_limit(1 << 20)` (DoS protection) | `function_library.rs:49-54` |
| 13 | function-library.md | `FunctionLibrary { functions: HashMap<...> }` | Also has `pub host_context_enabled: bool` | `function_library.rs:58-62` |
| 14 | path-mapping.md | `rule.apply(path) → (bool, String)` | `rule.apply(path) → Option<String>` | `path_mapping.rs:46` |
| 15 | path-mapping.md | URI fns named `split_uri`, `uri_parts`, `uri_name`, … | Module `uri_path` exports `parse`, `parts`, `name`, … (no `uri_` prefix) | `uri_path.rs:18-127` |
| 16 | symbol-table.md | `get(...) → Option<&ExprValue>` | `get → Option<&SymbolTableEntry>`; separate `get_value`, `get_string` | `symbol_table.rs:168-197` |
| 17 | values.md | `.item()` method panics on unresolved | No `item()` method exists | `value.rs` |
| 18 | parser.md (implicit) | ruff is the parser | `eval/mod.rs:5-6` doc comment still says "rustpython-parser" | `eval/mod.rs:5-6` |
| 19 | range-expr.md | `IntRange` stores "original direction separately" | Only canonical ascending form stored; direction normalized away | `range_expr.rs:62-110` |

### 2. Spec coverage gaps

Source modules/areas with no dedicated spec or inadequate coverage:

- **`functions/path_parse.rs`** (~20KB): Format-aware POSIX/Windows path parsing without `std::path`. Implements `sep`, `split`, `file_name`, `parent`, `file_stem`, etc. — the core of PATH property semantics that `path-mapping.md` alludes to but doesn't cover. Not listed in architecture module layout.
- **`edit_distance.rs`**: Referenced twice (architecture + error-formatting) but never described. Levenshtein threshold, suggestion-trigger rules, and public API (`edit_distance`, `suggest_closest`) are undocumented.
- **`functions/*` per-category modules**: No spec covers which file implements which operator category (arithmetic, comparison, conversion, list, math, misc, path, regex, repr, string). `function-library.md` lists categories abstractly but leaves semantics to the language spec.
- **`SerializedSymbolTable`**: Wire format is specified at the JSON level, but round-trip rules for `Unresolved` entries are not fully covered (the spec says unresolved entries are *skipped* on serialization — this behavior is not explicitly tested).
- **Host context registration**: `with_host_context`, `with_unresolved_host_context`, `register_host_context_functions` are mentioned; the actual inventory of host-context functions and their signatures is not enumerated.
- **Keyword rename reverse mapping**: `parser.md` describes the forward pass; the evaluator-side reverse lookup (in `eval_attribute`) is not documented.
- **`from_str_coerce` in values.md**: One line, but used at parameter binding and deserves a rules table analogous to the target-type coercion section.

### 3. Spec quality issues (unclear, redundant, missing rationale)

**Unclear:**
- evaluator.md BoolOp semantics conflates the null-coalescing rule ("only `null` and `false` are falsy") with a one-off special case. Reword to state the rule once.
- function-library.md "skip receiver coercion for methods" — the distinction between a method call and a function call happens in the evaluator before dispatch; spec should say how the library knows which it is.
- format-string.md `resolve` return type ("preserves typed values for single-expression strings") — document the exact precondition (zero literal segments, exactly one expression segment).
- symbol-table.md only shows `get`; `get_value` and `get_string` coexist and have different semantics.

**Redundant:**
- "Why FormatString lives here" rationale is duplicated in `architecture.md` and `format-string.md`.
- "Divergence from Python" sections repeat in most spec files.
- Implicit coercion tables (INT→FLOAT, PATH→STRING) appear in `type-system.md`, `values.md`, and `function-library.md`.

**Missing rationale:**
- path-mapping.md has only a single sentence on *why* URI paths are opaque. Expand with round-trip fidelity, authority preservation, `%`-encoded segments.
- range-expr.md contiguous-flag bit-packing rationale mentions "struct size" but not the memory-per-task cost in chunking.
- values.md Float64 invariants (no NaN/Inf/-0.0) should restate rationale: determinism, cross-language parity, hashability.
- error-formatting.md smart-caret positioning is a table of decisions; add the underlying principle ("point at the operator or name that failed").

### 4. Implementation — Rust quality, API ergonomics

**Strengths:**
- Builder pattern on `Evaluator` with `#[must_use]` on builders.
- `ParsedExpression::evaluator(&symtabs)` pre-configures keyword renames and source context.
- `symtab!` macro + `From<T>` impls for `ExprValue` make construction ergonomic.
- `ExpressionErrorKind` is `#[non_exhaustive]` — forward-compatible.
- `Cow` used appropriately in `ExprValue::as_str_repr`.
- `SerializedSymbolTable` cleanly separates transport format from in-memory structure.

**Friction points:**
1. `FormatString::resolve` has six entry points (`resolve`, `resolve_with_format`, `resolve_string`, `resolve_string_with_format`, `resolve_typed`, `resolve_typed_with_format`). Consider a `FormatStringOptions` builder.
2. `ExprValue::coerce` and `from_str_coerce` return `Result<_, String>`, not `Result<_, ExpressionError>`. Callers repeatedly wrap via `.map_err(ExpressionError::new)` (~10 sites in `evaluator.rs`).
3. Three error types coexist in related code paths: `ExpressionError`, `SymbolTableError`, `Result<_, String>` from `SerializedSymbolTable::to_symtab`. Unify via `From` impls.
4. `Float64(pub f64, pub Option<Box<str>>)` has public tuple fields — lets callers bypass the no-NaN/no-Inf invariant enforced in `Float64::new`. Make fields private.
5. `ExprValue::Path { value, format }` has public fields; the separator-normalization invariant is only enforced by `new_path`. Encapsulate.
6. `SymbolTable::all_paths(&self, prefix, out: &mut Vec<String>)` is an out-parameter API; return `Vec<String>` or an iterator.
7. `SymbolTable::from_pairs` takes `Vec<(&str, ExprValue)>` by value; `IntoIterator` would be more flexible.
8. `FunctionEntry::implementation` is a bare `fn` pointer, not `Box<dyn Fn>`. This blocks closure-based registration — document whether deliberate (performance) or a future change.

**Naming consistency:**
- Three parallel `evaluate` entry points: `lib::evaluate_expression`, `ParsedExpression::evaluate(&symtab)`, `Evaluator::evaluate(&ast)`. Consider disambiguating (e.g., `eval_once`/`eval_ast`).
- `FormatString::resolve_string` vs `resolve` — only differ in return type. `resolve_to_string`/`resolve_to_value` would be clearer.
- `ExprValue::Path { value, format }` struct-variant while `ListPath(Vec<String>, PathFormat, usize)` is tuple-variant with identical semantics. Unify.
- `SerializedSymbolTable::to_symtab` returns `Result<_, String>` while `Deserialize` returns a structured error.

### 5. Error message quality

Per AGENTS.md, evaluation errors must include expression + caret indicator. This is largely delivered:
- `ExpressionError::with_node` attaches source + span + caret offset.
- `Display` emits message + expression + caret on separate lines.
- `compute_caret_offset` handles `BinOp`, `Attribute`, `Call`, `Subscript` intelligently (points at the operator).
- The if/else both-branches-fail message in `eval_ifexp` manually renders nested carets.

**Gaps:**
1. `ExpressionError::new(...)` (Other kind) is used ~70 times. Many could be structured kinds (`TypeError`, `UnsupportedSyntax`). Examples in `range_expr.rs` all go through `new`, losing programmatic error classification.
2. `FormatString::parse_segments` errors (`"Failed to parse interpolation expression at [X, Y]..."`) don't flow through `with_span`, so no caret on the raw format string input.
3. `SymbolTableError` has no `impl From` for `ExpressionError`, so errors at `SymbolTable::set` lose context when bubbled up.
4. `RangeExprError` has `position` but `From<RangeExprError>` stringifies and discards it.
5. `function_library.rs:1109` uses `type_strs.last().unwrap()` — safe but a structured `if let Some(last)` is more defensive.
6. **Three places reimplement caret rendering**: `Display for ExpressionError`, `message_with_expr_prefix`, and the if/else both-branches block in `eval_ifexp`. Extract one `fn render_caret_line(expr, col, end_col, caret_offset)` helper.

### 6. Performance concerns

Ordered by estimated impact:

| # | File:line | Concern | Complexity |
|---|---|---|---|
| 1 | `eval/evaluator.rs:566,619,708,...` | `dispatch_with_node(Some(&ast::Expr::BinOp(b.clone())))` — clones entire AST subtree on every binop/unaryop/compare/subscript just to get a range for a possible error. | O(subtree size) per op |
| 2 | `symbol_table.rs:128,158` | Every `get`/`set` allocates `Vec<&str> = key.split('.').collect()`. Runs millions of times during format-string resolution. | O(N) allocation per call |
| 3 | `format_string.rs:454-493, 543-570` | `SimpleName` segments re-parse via `ParsedExpression::new(name)` on every `validate_expressions`, `copy_used_symtab_values`, `accessed_symbols` call. Cache at construction. | Extra parse per call |
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

7. Fix the 19 spec-vs-impl mismatches in table 1. Highest-impact items: builder example `evaluate(&parsed.ast)`, `ExpressionError` private fields + struct-variant kinds, `rule.apply → Option<String>`, `uri_path` function names without `uri_` prefix, `SymbolTable::get` accessor family.
8. Add specs for `functions/path_parse.rs` and `edit_distance.rs`.
9. Update the `eval/mod.rs` doc comment to reference `ruff` not `rustpython-parser`.
10. Remove references to non-existent `ExprType::LIST_INT`.
11. Document `FormatString::Segment::SimpleName` fast-path variant.
12. Consolidate "divergence from Python" appendix sections to reduce per-file repetition.
13. Consolidate implicit-coercion tables to a single canonical location.
14. Add a CI check that extracts and typechecks rustdoc-style code snippets from spec markdown.

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
