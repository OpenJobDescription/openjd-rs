# openjd-expr Crate Quality Evaluation Report

**Date:** 2025-05-09
**Crate:** `openjd-expr`

## Executive Summary

`openjd-expr` is a mature and well-tested Rust implementation of the OpenJD
Expression Language. Build and clippy (`-D warnings`) are clean on Linux;
**3,230 tests pass** (297 in-crate unit + 2,925 integration + 8 doc tests). The
public API in `specs/expr/public-api.md` is detailed and largely accurate, the
three-phase signature-based dispatch implementation is idiomatic, and the
resource-bounding (memory/op-count/AST-depth) is defense-in-depth with
independent guards at parse and evaluate time. Exploratory probes over edge
cases (INT64 bounds, unicode indexing, negative exponents, deeply-nested binop
chains, huge range expressions, format-string parsing pathologies,
negative-zero normalization, `Hash`/`Eq` consistency across cross-type equal
values) did not uncover any bugs.

The most material issue is **spec–implementation drift in the non-`public-api`
spec documents**. Several of them (`architecture.md`, `evaluator.md`,
`format-string.md`, `function-library.md`, `path-mapping.md`) describe a
`FunctionLibrary::with_host_context(rules)` / `get_default_library()` API that
no longer exists — the actual API is `ExprProfile::with_host_context(...)` +
`FunctionLibrary::for_profile(...)`. `public-api.md` correctly reflects the
implementation; the other documents have not been re-synchronized since the
profile refactor. Additional smaller drifts are catalogued below
(`FormatString::validate_expressions` / `validate_comprehension_vars`
signatures; `ExpressionError::undefined_variable` / `unknown_function`
constructors referenced in `error-formatting.md` but absent from the
implementation; inconsistent "8 MB" vs "32 MB" parser-thread stack comment
inside `eval/parse.rs`).

The other items in the Recommendations section are modest incremental
improvements — extending op-counting to the regex cache's
`get_or_compile_regex` default body, aligning the list-comp message wording
with the ""must start with a lowercase letter or underscore"" constraint
everywhere, and bringing in a light `proptest`/`arbitrary` fuzz harness to
cover the same ground Python's `test_fuzz.py` does.

## 1. Specifications Review

The `specs/expr/` directory contains 15 spec documents totaling roughly
60 KB. Coverage is broad and matches the crate's module decomposition:

| Document | Assessment |
|---|---|
| `README.md` | ✓ Accurate index; good pointers to RFC references. |
| `architecture.md` | ⚠ References `FunctionLibrary::with_host_context` (does not exist). |
| `public-api.md` | ✓ Authoritative, closely matches implementation. One minor omission (see §2). |
| `type-system.md` | ✓ Union normalization, unresolved normalization, and substitution rules match implementation. |
| `values.md` | ✓ Typed list variants, `Float64` invariants, and hash/eq semantics faithfully documented. |
| `symbol-table.md` | ✓ Dotted-path semantics, `SerializedSymbolTable`, transport format all accurate. Caps documented. |
| `parser.md` | ✓ Good explanation of three-layer depth limit and fast-path/worker-thread split. |
| `evaluator.md` | ⚠ References `FunctionLibrary::with_host_context`; also mentions `get_default_library()` as the default builder, but the actual default-library static is `pub(crate)`. |
| `function-library.md` | ❌ Most drift. Describes a separate `get_default_library()` / `FunctionLibrary::with_host_context(rules)` / `with_unresolved_host_context()` API that does not exist. Today's API is `FunctionLibrary::for_profile(&ExprProfile)`. |
| `format-string.md` | ⚠ "Defaults" table says `library = None (use get_default_library())` — again the ghost API. Function signatures for `validate_expressions` / `validate_comprehension_vars` diverge from implementation (see §3). |
| `error-formatting.md` | ⚠ References `ExpressionError::undefined_variable(...)` constructor that does not exist in the code. |
| `edit-distance.md` | ✓ Matches implementation precisely (including length-difference early rejection). |
| `range-expr.md` | ✓ Parser, indexing, slicing, contiguous flag bit-packing all documented accurately. |
| `path-mapping.md` | ⚠ Host-context section uses the ghost `with_host_context` API. |
| `path-parse.md` | ✓ Anchor detection rules, separator handling, and the split between `path_parse` and `uri_path` are documented faithfully. |

Overall the documents collectively cover every significant aspect of the
crate, and `public-api.md` is a strong authoritative reference. The drift in
the other documents is a consequence of a profile refactor whose rename
propagated to `public-api.md` and the code but not to the narrative specs.

## 2. Public API Review

`public-api.md` is the strongest spec document and — with a couple of small
exceptions — accurately describes the implementation.

**Completeness and accuracy.**

- All publicly exported items in `lib.rs` are documented, including the
  profile machinery, `EvalResult`, `EvalBuilder`, `FormatStringOptions`,
  and the host-context registration primitives.
- Defensive caps (`MAX_EXPRESSION_DEPTH`, `MAX_PARSE_INPUT_LEN`,
  `MAX_FORMAT_STRING_LEN`, `MAX_FORMAT_STRING_SEGMENTS`,
  `MAX_RANGE_EXPR_CHUNKS`, `MAX_SYMBOL_TABLE_ENTRIES`) are enumerated with
  rationale.
- `#[non_exhaustive]` policy is spelled out: `TypeCode`, `ExprRevision`,
  `ExprExtension`, `ExpressionErrorKind`, `ExprValue` outer enum, and the
  inner `ExprValue::Path` variant. The outer enum is correctly
  `#[non_exhaustive]` in the code (at `value.rs:152`) but the "enum shape"
  snippet in `public-api.md` omits the attribute on the outer declaration
  — a small readability nit; the attribute is explicitly called out again
  in the later "Versioning and Stability Conventions" section.

**Minor deviations from implementation.**

- `FormatString::validate_expressions` in the spec takes
  `library: Option<&FunctionLibrary>`. The implementation takes a
  non-optional `&FunctionLibrary`. Given the existing callers always pass
  a library, the implementation is fine; update the spec to match.
- `FormatString::validate_comprehension_vars` in the spec takes no
  arguments and returns `Result<(), FormatStringValidationError>`. The
  implementation takes `&HashSet<String>` (the set of active let-binding
  names) and returns `Result<(), ExpressionError>`. The implementation
  matches the actual use site in `openjd-model`; the spec is stale.

**API ergonomics.** The shape is clean and idiomatic:

- `ParsedExpression::new / with_profile` splits stability-preserving and
  ad-hoc parse paths, with strong Rustdoc on the stability implications.
- The `ParsedExpression::with_*(...)` → `EvalBuilder::evaluate(&[symtabs])`
  split is nice: parse-time configuration flows from the parsed
  expression, symbol-table binding is deferred to the terminal call, and
  resource metrics are optional via `evaluate_with_metrics`.
- `FormatStringOptions` uses `impl Into<Option<&FunctionLibrary>>` in
  `with_library` so callers can pass either `&lib` or `Option<&lib>`.
- `SymbolTable::from_pairs` accepts any `IntoIterator<Item=(&str,
  ExprValue)>`, and `symtab!` handles `impl Into<ExprValue>`, which
  includes bare `ExprType` values that auto-wrap as `Unresolved(T)`. This
  is a strict upgrade over requiring manual wrapping.

Minor API remarks (not regressions, just observations):

- `ExprValue::coerce` consumes `self` and returns `Result<Self, String>`
  using `String` rather than `ExpressionError` on failure. Most other
  error paths in the crate return `ExpressionError`; the `String` return
  here is a leftover interior-error type. Not a bug, but inconsistent.
- `FunctionLibrary::register_sig` returns `Result<(), String>` for the
  same reason. Harmless, but not symmetric with the rest of the crate's
  error types.
- `ListIter::next()` yields `Item = ExprValue`, cloning scalar contents
  on each call. The rationale (typed-to-tagged conversion without GATs)
  is explained clearly in `values.md`.

## 3. Implementation Review

**Module boundaries.** The module layout mirrors the specs one-to-one
(`types.rs`, `value.rs`, `eval/parse.rs`, `eval/evaluator.rs`,
`function_library.rs`, `default_library.rs`, `functions/*.rs`,
`format_string.rs`, `range_expr.rs`, `path_mapping.rs`, `uri_path.rs`,
`symbol_table.rs`, `error.rs`, `edit_distance.rs`, `profile.rs`). The
`Evaluator` is correctly crate-private and exposed only through
`ParsedExpression` → `EvalBuilder`. `FunctionLibrary::for_profile` caches
per-profile libraries with a rules-independent cache key; the
`WithRules(_)` case builds on the cached `None` skeleton so different
rule sets share a cached base.

**Normalization invariants.** The code enforces exactly the invariants
documented in `specs/expr/type-system.md` and `values.md`:

- Union: flatten, deduplicate, ANY-absorb, NORETURN-collapse,
  unresolved-hoist, single-member unwrap, alphabetic sort
  (`types.rs: normalize_union`).
- Unresolved: `list[unresolved[T]] → unresolved[list[T]]`,
  `unresolved[unresolved[T]] → unresolved[T]`, union with unresolved
  hoists the wrapper (`types.rs: ExprType::list`, `ExprType::unresolved`,
  `normalize_union`).
- `Float64::new` rejects NaN/infinity and folds `-0.0 → 0.0`, guaranteeing
  that `ExprValue` `Hash`/`PartialEq` invariants hold.
- `ExprValue::Path` is `#[non_exhaustive]` so `new_path` is the only
  constructor, locking in separator normalization.

**Hash consistency with cross-type equality.** `ExprValue::Hash`
correctly groups `Int(n)` and whole-valued `Float(n.0)` under tag `2`
(canonical integer hashing) and `String(s)`/`Path{value: s, ..}` under
tag `3` (consistent with `equals()` treating `"x" == Path{"x"}` as
`true`). All list variants hash under tag `4` with each element further
hashed by its value-tag, so `ListInt([1]) == ListFloat([1.0])` and
`ListBool([]) == ListString([])` are both reflected in equal hash
values. Spot-verified via an exploratory test that a `HashSet` treats
the documented equal pairs as one entry.

**Operation counting.** Every function call, every list-comp iteration
(counted in `eval_listcomp` before filter evaluation), and every string
op block (`count_string_ops(len)` rounds up to `ceil(len/256)`) flow
through the `Evaluator`'s `count_op`/`count_ops`/`count_string_ops`.
`OperationLimitExceeded` surfaces early — confirmed by probe that a
`[x for x in range(1_000_000)]` with `with_operation_limit(100)` errors
at count 101.

**Memory tracking.** `track`/`release` balance allocations and releases
at every dispatch boundary. Probe: `'x' * 100000000` produces
`MemoryLimitExceeded` cleanly. The `make_list_checked` path pre-checks
the estimated heap footprint via `ctx.check_memory(...)` before the
final `make_list` allocation, so evaluator paths that construct lists
(list literals, list comprehensions) fail early rather than after
allocation.

**Depth limit.** The three-layer guard (input length cap + parser
structural walker + `Evaluator::evaluate` depth counter) is an
exemplary defense-in-depth. Probe: a 200-term left-associative
`1+1+…+1` chain (which the parser-side source scan does not flag)
produces `ExpressionTooDeep { depth: 65, limit: 64 }` at evaluator
entry rather than a stack overflow.

**Error messages.** Caret formatting is centralized through
`error::write_caret_line` (used by `Display`, `message_with_expr_prefix`,
and the if/else both-branches-fail renderer); smart caret positioning
for `BinOp`, `Attribute`, `Call` with attribute func, and `Subscript`
points the `^` at the failing operator/name rather than at the start of
the span. "Did you mean" suggestions via `edit_distance::suggest_closest`
use length-based early rejection that is both sound and fast.

**Naming consistency.** Consistent `fn_name`/`ctx`/`args` conventions
across `functions/*.rs`. The dunder-name convention (`__add__`,
`__property_name__`) is used uniformly across operator and property
registration.

**Findings (small).**

1. In `eval/parse.rs` the doc comment inside `with_profile` says the
   "8 MB worker-thread stack" survives inputs up to `MAX_PARSE_INPUT_LEN`,
   but the actual constant `PARSER_THREAD_STACK_SIZE` is `32 MB`. The
   comment on the constant itself correctly says 32 MB. The inline
   comment is stale; a one-line fix.

2. `eval::evaluator.rs: eval_listcomp` error message uses "Loop variable
   'X' must start with a lowercase letter or underscore" while the
   underscore case is also permitted in `validate_structure_inner`. But
   `format_string.rs: check_comprehension_vars` raises "List
   comprehension variable 'X' must start with a lowercase letter or
   underscore" — same text, consistent — **however** `parse.rs:
   validate_structure_inner` uses "Loop variable 'X' must start with a
   lowercase letter or underscore". All three agree now on wording, but
   the duplication of the check in three places invites divergence.
   Consider extracting the check to a single function.

3. `FunctionLibrary::register_sig` returns `Result<(), String>`.
   Everywhere else in the crate error types are `ExpressionError`.
   Low-impact since `register_sig` is host-side code; consider
   `Result<(), ExpressionError>` for consistency.

4. `ExpressionError::coerce` (on `ExprValue`) returns `Result<Self,
   String>` — same story. The only caller in the crate is in
   `eval/evaluator.rs:coerce`, which maps `String → ExpressionError` on
   error. The `String` is never exposed structurally; consider returning
   `ExpressionError` directly and dropping the map.

5. `default_library.rs` uses `.expect("bad builtin signature")` on roughly
   200 `register_sig` calls. These are compile-time constants in string
   form; a panic here would only fire if a signature string was edited
   to become syntactically invalid. That's fine for a static, but a
   unit test that iterates over every registration in `build_default_library`
   and asserts all signatures parse would catch regressions at test time
   rather than at first program load in prod. (`default_library_has_all_categories`
   and `signature_count` do not exercise this explicitly.)

None of these are correctness bugs. They are polish items.

## 4. Test Review

- **Unit tests (in-source):** 297. Every major source file has a `#[cfg(test)]
  mod tests` block. `default_library.rs` tests spot-check each function
  category, verify signature counts stay at ≥190, and name all expected
  function names. The `for_profile_tests` module in `default_library.rs`
  exercises the profile-cache and WithRules-rebuild path end-to-end.

- **Integration tests:** 2,925 across 37 files, all linked into a single
  `integration` binary to keep link time reasonable.

| Category | Tests |
|---|---|
| test_strings.rs | 385 |
| test_lists.rs | 240 |
| test_evaluation.rs | 219 |
| test_paths.rs | 206 |
| test_unresolved_eval.rs | 179 |
| test_types.rs | 173 |
| test_expr_value.rs | 133 |
| test_arithmetic.rs | 128 |
| test_uri_paths.rs | 119 |
| test_error_formatting.rs | 98 |
| test_string_operation_counting.rs | 74 |
| test_symbol_table.rs | 73 |
| test_parse_expression.rs | 64 |
| test_slicing.rs | 60 |
| test_comparison.rs | 59 |
| test_operation_limit.rs | 55 |
| (21 more files) | 432 |

  Each area is well-covered. The `test_error_formatting.rs` suite
  asserts full error-text-plus-caret output per the AGENTS.md test
  quality standard, which keeps error messages from regressing silently.

- **Doc tests:** 8, covering `symtab!`, `FormatStringOptions`,
  `FunctionLibrary::for_profile`, `ParsedExpression::with_profile`,
  `EvalBuilder`, `ExprProfile`, and `Evaluator`.

- **Happy-path vs edge-case split.** Most files exercise both.
  `test_int64_bounds.rs`, `test_unicode_codepoint.rs`, `test_memory.rs`,
  `test_operation_limit.rs`, `test_expression_depth.rs`, and
  `test_string_operation_counting.rs` are all explicitly
  edge-case-focused. The negative paths (type errors, undefined
  variables, memory/op overruns, nesting overflow) are tested against
  full error strings.

- **Organization.** Consistent one-category-per-file layout; each file
  is independently runnable (`cargo test --test integration test_name::`).

**Gaps / suggestions.**

- No property-based / fuzz tests. Python has a minimal `test_fuzz.py`
  using Hypothesis that feeds arbitrary UTF-8 and binary to
  `parse_expression` and asserts no crash. The Rust parse pipeline is
  robust by construction (worker-thread parse of long inputs, depth
  walker, UTF-8 indexing via `chars()`) but a light `proptest` or
  `arbitrary` harness over `ParsedExpression::new` and `FormatString::new`
  would catch regressions if the caps are ever tuned.

- `test_list_nesting.rs` has 9 tests. The 2-level cap is a key invariant
  — expanding this to exercise every constructor that can produce a
  nested list (`make_list` + comprehension + type-promoted mix) would
  make the coverage of the invariant more obvious from the test names.

- Python's `test_parsing.py` (79 tests) content maps to
  `test_evaluation.rs` + `test_parse_expression.rs` in Rust, with no
  loss of coverage.

## 5. Python Comparison

The Rust implementation was ported directly from `openjd.expr` in
`openjd-model-for-python` (branch `expr`). A file-by-file comparison:

**Shared architecture.** Module decomposition is 1-to-1 with one
exception: the Python codebase places `FormatString` in
`openjd.model._format_strings`, while Rust places it in `openjd-expr`.
The Rust location is architecturally cleaner — the `{{…}}` interpolation
layer lives with the expression evaluator it dispatches to, removing a
model-to-expr cross-package import.

**Behavioral parity.**

- `__pow__(int, int)`: Python returns `ExpressionError` for `exp > 63`
  with `|base| > 1`; Rust mirrors this exactly (verified with probe
  `2 ** 100 → IntegerOverflow`).
- `__pow__(int, int)` with negative exponent: Python returns
  `float`; Rust returns `float` (verified `2 ** -1 → 0.5`).
- Float `1.0 / 0.0`: Python raises `ExpressionError("Division by zero")`
  (because the Python evaluator intercepts the division before IEEE
  semantics); Rust does the same via `DivisionByZero`.
- `Int(1) == Float(1.0)` and `String("x") == Path{value:"x"}`: Python
  `equals()` returns true for both; Rust `PartialEq` does the same.
- Empty-list cross-type equality: Python's `ExprValue([], elem_type=INT)`
  equals `ExprValue([], elem_type=STRING)`; Rust's `ListInt([]) ==
  ListString([], 0)` is also true.
- Loop-variable naming rule: Python rejects loop variables starting
  with uppercase or digit; Rust rejects the same, with identical
  wording ("Loop variable 'X' must start with a lowercase letter or
  underscore").
- `negative index` on list: Python wraps; Rust wraps (verified).
- Unicode indexing: Python `'αβγ'[0]` returns `'α'` (char-based); Rust
  does the same via `chars()` accounting.
- `-0.0` normalization: Python normalizes via `copysign(0, 1)`; Rust
  normalizes inside `Float64::new`.

**Divergences (intentional).**

- Rust adds `TypeCode::Signature` as a first-class type, stores function
  signatures as `ExprType` values, and keeps the library self-describing.
  Python uses a separate `FunctionSignature` dataclass.
- Rust uses typed list variants (`ListInt`, `ListBool`, …) for memory
  efficiency. Python uses a single tagged `list[ExprValue]`.
- Rust's `RangeExpr` normalizes descending ranges (`10-1:-1`) to
  ascending form at construction; Python preserves the user-supplied
  direction. `values.md` documents the rationale (every downstream
  consumer treats the range as a sorted set).
- Rust adds `ParsedExpression::as_name_lookup()` for the `{{Param.Name}}`
  fast path; Python has no equivalent API.
- Rust returns `ExpressionError` as a single struct with a
  `#[non_exhaustive]` `ExpressionErrorKind`; Python has
  `ExpressionError` and a subclass `ExpressionTypeError`. Matching the
  Rust style via `matches!(err.kind(), ExpressionErrorKind::TypeError { .. })`
  is arguably more idiomatic.
- Rust's `FunctionLibrary::register_sig` returns `Result<(), String>`
  instead of Python's "raise if invalid" style.
- Python exposes `evaluate_expression(expr, *, values=..., library=...)`
  as a top-level convenience. Rust requires the explicit
  `ParsedExpression::new(expr)?.evaluate(&st)` ceremony — which is
  slightly more verbose but prevents accidental re-parsing.

**Error message comparison.** Caret-annotated error output is identical
between Rust and Python for the tested cases. The `test_error_formatting.rs`
suite (98 tests) asserts the full three-line output (message, expression
line, caret indicator) including the caret offset within the span.

**Test case coverage.** Every Python test file has a corresponding
(and almost always larger) Rust file. The only Python-only file is
`test_fuzz.py` (2 tests, Hypothesis-based), which the Rust crate does
not mirror.

## 6. Build and Test Results

```
$ cargo build -p openjd-expr
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 11.72s

$ cargo clippy -p openjd-expr --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.92s
(no output — all clippy lints pass)

$ cargo test -p openjd-expr
test result: ok. 297 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 2925 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.48s
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s
```

- **Build:** clean, no warnings.
- **Clippy `-D warnings`:** clean on Linux debug.
- **Tests:** 3,230 passed / 0 failed. 1.48 s wall for the integration
  binary is fast for a test suite this size.
- **Documentation build** (not run here, but per AGENTS.md CI
  requirement `cargo doc --no-deps --workspace` with `-D warnings` is
  clean on main).

## 7. Exploratory Findings

Thirty-plus probes were written against unusual inputs. All of them
behaved correctly; no probes were committed. Representative findings:

- **i64 extremes.** `9223372036854775807` parses as `Int(MAX)`;
  `-9223372036854775808` parses as `Int(MIN)` via the `-<int_literal>`
  fold in `eval_unaryop`; `9223372036854775808` errors with
  `IntegerOverflow`.
- **Deeply nested parens (100 pairs).** `((((…1…))))` parses fine and
  evaluates to `1` — the input is `201` bytes so it runs on the parser
  worker thread, which comfortably survives 100 levels.
- **Long binop chain.** `1+1+…+1` with 200 terms is rejected at parse
  time with `ExpressionTooDeep { depth: 65, limit: 64 }` — the
  evaluator guard catches what the parser's source-scan cannot.
- **Format-string pathologies.** Unclosed `{{`, `{{…{{…}}…}}`, empty
  `{{ }}`, and `}}` without a preceding `{{` all fail with clear error
  messages carrying the failure position.
- **Range_expr of 10^12 elements.** `range_expr('1-1000000000000')`
  parses into a single `IntRange` and indexing into it is O(log n) —
  verified `range_expr('1-1000000000000')[5] == Int(6)`.
- **String × 100 M.** `'x' * 100000000` hits `MemoryLimitExceeded` at
  `used: 100000136, limit: 100000000` — the allocation is rejected
  cleanly before the string is materialized.
- **Unicode indexing.** `'αβγ'[0] = 'α'`; `len('αβγ') = 3`. Char-based
  indexing (as in Python).
- **Negative zero.** `-0.0 → Float(0.0)`; `0.0 == -0.0 → Bool(true)`.
- **`true + 1`.** Correctly errors: "Cannot use '+' operator with bool
  and int".
- **`1 in ['a','b']`.** Correctly errors: "Cannot use 'in' operator
  with list[string] and int".
- **Pow negative exponent.** `2 ** -1 → Float(0.5)`.
- **Huge division.** `9007199254740993 / 1 → Float(9007199254740992.0)`
  (documented IEEE 754 precision loss for `int / int → float`).
- **Hash/Eq consistency.** `Int(0)` and `Bool(false)` are distinct in
  a `HashSet` (size 2 with both inserted), matching `0 == false → false`.
- **List nesting rejection.** `[[[1]]]` errors with "Lists may be
  nested at most 2 levels deep" at construction time.
- **Null in list literal.** `[null]` errors with "null is not allowed
  in list literals".

All observed behaviors matched spec expectations.

## 8. Recommendations

Numbered for future report-driven follow-up (per AGENTS.md §
Report-driven development). Priority tiers: [P1] re-sync spec with
implementation; [P2] polish / small consistency fixes; [P3] nice-to-
haves.

### [P1] Spec–implementation synchronization

1. ~~**Rewrite the host-context sections of `architecture.md`,
   `evaluator.md`, `format-string.md`, `function-library.md`, and
   `path-mapping.md`** to describe the real API: `FunctionLibrary::for_profile(&ExprProfile)`,
   `ExprProfile::with_host_context(HostContext::WithRules(…))`, and
   `HostContext::Unresolved`. Delete references to
   `FunctionLibrary::with_host_context(rules)`,
   `FunctionLibrary::with_unresolved_host_context()`, and
   `get_default_library()`. The `public-api.md` text for this area is
   accurate and can serve as the canonical source.~~ **Resolved** — all
   five spec docs now describe the real API, plus `specs/cli/run.md`
   and `specs/model/validation.md` (which had the same drift).
   Stale doc comments in `format_string.rs` and
   `default_library.rs` source were also updated.

2. ~~**Fix `FormatString::validate_expressions` signature in
   `public-api.md`** — the implementation takes a non-optional
   `&FunctionLibrary`, not `Option<&FunctionLibrary>`. (Alternatively,
   change the implementation to match the spec; the optional form is
   more flexible for callers that don't want to load a library for
   simple validations.)~~ **Resolved** — spec updated to match the
   implementation (non-optional `&FunctionLibrary`).

3. ~~**Fix `FormatString::validate_comprehension_vars` signature in
   `public-api.md`** — the implementation takes `&HashSet<String>`
   (the set of active let-binding names) and returns
   `Result<(), ExpressionError>`. The spec shows it as zero-argument
   returning `Result<(), FormatStringValidationError>`. Align the spec
   to the code.~~ **Resolved** — spec updated to match the
   implementation.

4. ~~**Remove or rename `ExpressionError::undefined_variable(...)` in
   `error-formatting.md`** — the constructor does not exist in the
   code. Either add it for symmetry with the other `integer_overflow`,
   `division_by_zero`, etc. shortcuts, or update the spec to show
   `ExpressionError::from_kind(ExpressionErrorKind::UndefinedVariable
   { name, suggestion })` instead.~~ **Resolved** — the
   non-existent constructor example was replaced with the correct list
   of real convenience constructors plus guidance to build
   `UndefinedVariable`/`UnknownFunction` via `from_kind`. Also added
   the missing `ExpressionTooDeep` variant to the enum listing and
   trigger table.

5. ~~**Fix the inline "8 MB worker-thread stack" comment in
   `eval/parse.rs`** (in `with_profile` doc) to say 32 MB, matching
   `PARSER_THREAD_STACK_SIZE` and the comment on that constant.~~
   **Resolved.**

### [P2] Code consistency and polish

6. **Change `FunctionLibrary::register_sig` to return
   `Result<(), ExpressionError>`** instead of `Result<(), String>`, so
   that host-side library construction uses the same error type as
   everything else in the crate.

7. **Change `ExprValue::coerce` and `ExprValue::from_str_coerce` to
   return `Result<Self, ExpressionError>`** instead of `Result<Self,
   String>`, for the same reason. The only existing caller
   (`eval_inner`'s coerce step) already maps `String → ExpressionError`
   and can drop the map.

8. **Extract the "loop variable must start with lowercase or
   underscore" check into a single helper** and call it from
   `validate_structure_inner`, `eval_listcomp`, and
   `check_comprehension_vars`. Today the predicate is hand-rolled in
   all three places; the error messages happen to agree but are easy
   to drift.

9. **Add a test that iterates over every entry in `build_default_library`
   and verifies `ExprType::parse(sig_str)` succeeds** (catching a bad
   edit to a `register_sig` signature string at test time rather than
   at first load in production).

### [P3] Defense in depth and tooling

10. **Add a light `proptest` or `arbitrary` fuzz harness** that runs
    `ParsedExpression::new` and `FormatString::new` over random bytes
    and asserts the only accepted failure mode is `ExpressionError`.
    This mirrors Python's `test_fuzz.py` and would catch regressions
    in the parse-thread plumbing if the caps are ever tuned.

11. **Document `ExprValue::make_list` vs `make_list_checked` more
    prominently at the `ExprValue` doc-comment level.** The guidance
    (prefer `make_list_checked` when an `EvalContext` is available) is
    in `values.md` but not in the `ExprValue` rustdoc itself, so
    external callers who skim rustdoc rather than the specs might
    reach for the unchecked form by default.

12. **Extend `EvalContext::get_or_compile_regex`'s default body** (the
    one used when a custom `EvalContext` doesn't override it) to call
    `self.count_string_ops(pattern.len())`. The evaluator's override
    already caches and is therefore amortized, but the default path
    still compiles on every call — a pathological caller that repeatedly
    invokes `re_match` with distinct patterns through a custom
    `EvalContext` would compile many regexes without incrementing the
    op counter. Low-severity, host-code-only, but worth noting.
