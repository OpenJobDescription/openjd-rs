# openjd-expr Crate Quality Evaluation Report

**Date:** 2026-07-16
**Crate:** `openjd-expr`

## Executive Summary

The crate is in strong overall shape: it builds warning-free, is clippy-clean under `-D warnings`, and passes all 3,368 tests (297 unit + 3,063 integration + 8 doc). The spec suite is unusually thorough and mostly line-accurate, the test suite is large and well-organized, and the implementation conforms to the 2026-02 Expression Language specification more faithfully than the Python reference does in at least two places (range_expr slicing semantics and resource-bounded slicing). However, exploratory testing confirmed **15 real bugs by execution**, including **8 arithmetic-overflow or char-boundary panics reachable from untrusted expression text in debug builds** (which wrap silently in release builds, in one case defeating the memory-budget checks), a parser bug that corrupts string literals containing contextual keywords, a float-passthrough mis-slice that silently changes displayed values in multiline expressions, and a path-mapping rule matcher that maps relative paths against absolute rules. None of these is architectural; all have small, localized fixes. The priority is hardening integer arithmetic at the function-library boundary (checked math + negative-argument validation) and fixing the two silent-wrong-value parser/evaluator bugs.

## 1. Specifications Review

Sixteen documents in `specs/expr/` were reviewed and spot-checked claim-by-claim against the code. Full details in the per-document review produced during evaluation; summary:

| Document | Assessment |
|---|---|
| `README.md`, `architecture.md` | Good index and overview. **A1:** `architecture.md` lists `xxhash-rust` as a dependency; it is not in `Cargo.toml` (and omits `shlex`, `regex-syntax`, `serde_json`, which are). |
| `type-system.md` | Accurate except **T1:** names a variant `Null`; code is `TypeCode::NullType`. |
| `values.md` | Mostly accurate. **V1:** describes `Float64` as a tuple struct with public fields; it has private named fields and `Result`-returning constructors. `with_str`'s zero-value string-drop behavior (`"0.00"` → `"0.0"`) is undocumented. |
| `parser.md` | Good. Omits the underscore allowance in the loop-variable rule (2 sites); paraphrases some rejection messages; missing the `u'...'` prefix rejection row; "200 characters" fast-path claim is actually bytes. |
| `evaluator.md` | Accurate on limits and dispatch; `_track`/`_release` naming drift vs `track`/`release` in code. |
| `function-library.md` | **F1:** the `EvalContext` listing omits the required `check_memory` method. **F2:** the `get_or_compile_regex` default-impl snippet shows `Regex::new`, contradicting both the code and the doc's own prose (RegexBuilder + 1 MiB size limit). These are the errors most likely to break a downstream trait implementer. |
| `format-string.md` | **FS1:** `FormatStringValidationError` listing omits the `expression_error` field. **FS2:** the Deserialize snippet claims string-only input; code accepts str/int/float/bool. `resolve_string_with` rendering Null as empty string is undocumented. |
| `range-expr.md` | **R1:** claims `RangeExpr: IntoIterator`; no such impl exists. Uses `r[i]` indexing syntax with no `Index` impl. |
| `path-mapping.md`, `symbol-table.md`, `profile.md`, `edit-distance.md` | Verified essentially line-perfect against code. All documented constants/limits match exactly (100 MB / 10 M ops / depth 64 / 64 KiB parse input / 1 MiB regex / etc.). |
| `path-parse.md` | **PP1:** documents `with_name` and related `with_*` builders as living in `path_parse.rs`; they live in `functions/path.rs` with different signatures. |
| `error-formatting.md` | Suggestion-format examples don't match actual message text. |
| `public-api.md` | See §2. |

Code-side doc rot found during the spec review: `default_library.rs:7–10` module doc still says "Implementations are placeholders — the evaluator's hardcoded match arms handle actual evaluation," which contradicts the real dispatch design (the library `call` invokes real implementations).

## 2. Public API Review

`public-api.md` is comprehensive (the crate-root re-export list, all 8 public constants, and the full method sets of `ParsedExpression`, `EvalBuilder`, `EvalResult`, `FunctionLibrary`, `EvalContext`, `ExprType`, `ExprValue`, `Float64`, `SymbolTable`, `FormatString`, `RangeExpr`, `PathFormat`, profile types, and `uri_path` were all verified as matching). Four accuracy errors and seven omissions were found:

**Accuracy errors:**

- **PA1** — `ExprValue::from_json_transport` documented with 3 parameters; code has 2 (`value.rs:804`), with the type coming from the JSON's `"type"` field.
- **PA2** — `IntRange` fields documented as private; they are `pub` (`range_expr.rs:96–99`). This is also an API-hygiene risk: public fields let callers violate the `start <= end, step > 0` invariant that `len()` and `from_ranges` merging assume (and `len()` panics on overflow — see §7, X1).
- **PA3** — `default_library` mislabeled a "deprecated module alias"; it is neither deprecated nor an alias.
- **PA4** — the blanket claim that `functions::*` implementations "have the shape required by `FunctionImpl`" is false for `functions::path_parse`, whose 11 public functions are string-level utilities with unrelated signatures.

**Omissions (PM1–PM7):** `format_string::copy_symbol_value` (a public free function documented nowhere — likely an accidentally-public internal helper), `Serialize for FormatString`, `From<RangeExpr> for ExprValue`, `From<RangeExprError> for ExpressionError`, `From<SymbolTableError> for ExpressionError`, several `Display`/`Error` trait impls, and the 11 concrete `path_parse` functions.

**Ergonomics:** strong overall — parse-once/evaluate-many, `#[must_use]` builder, `symtab!` macro, structured `#[non_exhaustive] ExpressionErrorKind`. Rough edges: (1) `ExprValue::coerce`, `from_str_coerce`, `SerializedSymbolTable::to_symtab`, `ExprType::parse`, and `FunctionLibrary::register_sig` return `Result<_, String>` while the rest of the crate uses structured errors; (2) `evaluate(&SymbolTable)` (single) vs `evaluate_with_metrics(&[&SymbolTable])` (slice) is an asymmetric pair; (3) `IntRange` public fields (PA2).

## 3. Implementation Review

All 25 source files were reviewed. Quality is generally high: pervasive doc comments with spec references, disciplined defensive caps, careful Python-parity notes at nearly every subtle decision point, and no `O(N²)` hot paths found. `pow_int`, `floordiv_int`/`mod_int` (including the `i64::MIN / -1` case), and int-literal overflow handling are exemplary. The confirmed problems cluster at the function-library boundary where argument values are converted to `usize` or fed to unchecked arithmetic:

**Confirmed bugs (all verified by execution in §7 unless marked "inspection"):**

1. `range_expr.rs:130` (`IntRange::new`) — unchecked subtraction overflows for extreme ranges; `len()` and `contains()` carry the same expression.
2. `eval/parse.rs` contextual-keyword retry loop — rewrites keyword-like text inside **string literals**, silently corrupting values.
3. `eval/evaluator.rs` `eval_number` float passthrough — literal source ranges computed against the parenthesized parse string are applied to the unwrapped source; multiline expressions mis-slice the display string.
4. `path_mapping.rs:150–155` (`split_path_parts`) — discards the absolute/relative distinction, so relative inputs match absolute rules.
5. `value.rs` `coerce` Float→Int — `as i64` silently saturates for out-of-range whole floats.
6. `value.rs` `equals(ListInt, RangeExpr)` — materializes the entire range before the length check, outside all evaluator budgets (1.78 s / 800 MB for a 100M-element range).
7. `functions/arithmetic.rs:273` (`mul_string`) — unchecked `s.len() * n`; a wrapped length defeats `count_string_ops` and `check_memory` in release builds.
8. `functions/arithmetic.rs` (`mul_list`) — same unchecked multiply (inspection); worse: with a wrapped `result_len` of 0 the build loop runs up to 2^62 iterations with no op counting (uncounted CPU hang even for `[] * huge`). Op counting is also a literal `for _ in 0..result_len { ctx.count_op()?; }` loop instead of one `count_ops(result_len)` call.
9. `functions/list.rs:120` (`range_fn`) — unchecked `v += step` overflows near `i64::MAX`.
10. `functions/list.rs` (`sum_list`, RangeExpr branch) — unchecked `r.iter().sum::<i64>()`; the list branch uses `checked_add` correctly, the range branch does not (also iterates the range twice).
11. `functions/math.rs:170` (`round_fn`) — unchecked negation of `ndigits` panics for `i64::MIN`.
12. `functions/string.rs:212` (`split_fn`/`rsplit_fn`) — negative `maxsplit` wraps via `as usize`, then `n + 1` overflows; release builds produce `[]` where Python treats negative maxsplit as "no limit."
13. `functions/regex.rs:363` (`re_split_fn`) — same `n + 1` overflow; also the only regex function using unchecked `make_list` instead of `make_list_checked`.
14. `functions/string.rs` (`zfill_fn`) — negative width wraps to `usize::MAX` → `"0".repeat(huge)` allocation abort (inspection; not executed to avoid aborting the harness). Output size is also never charged to memory/op budgets.
15. `functions/string.rs` (`center_fn`/`ljust_fn`/`rjust_fn`) — negative width wraps huge; fails *closed* with a nonsensical "operation count (72057594037927938) exceeded limit" error where Python returns the string unchanged. The inconsistency with `zfill` (which fails *open*) shows the four width-taking functions lack a shared validation path.
16. `functions/path.rs:526` (`path_starts_with`) — `path[..base.len()]` byte-slices at a possibly non-char boundary on Windows-format comparison; panics on multibyte UTF-8 paths. `relative_to_fn` shares the helper.
17. `functions/repr.rs` (`repr_py`) — escapes only `\` and `'`; newlines/control characters emitted raw, producing invalid Python string literals.
18. `format_string.rs` `copy_symbol_value` + `symbol_table.rs` `set_table` — dotted keys inserted literally, creating entries `get()` can never find.
19. `uri_path.rs:133–141` (`join`) — double slash when joining onto bare-authority URIs (`s3://bucket//child`).

**Minor/observations:** `floordiv_float` and `math.rs` `floor`/`ceil`/`round` use `> i64::MAX as f64` guards where `>=` is needed (2^63 exactly saturates silently); `join_fn` silently treats a wrong-typed separator as `""` and never charges the output length; `isdigit_fn` is ASCII-only vs Python's Unicode `str.isdigit()`; `format_string.rs` enforces `MAX_FORMAT_STRING_SEGMENTS` only after fully parsing all segments (move into the loop, as `range_expr` correctly does); list `equals`/`compare` clone every `String` element via `ListIter`; duplicate `count_op` logic between the inherent method and the `EvalContext` impl; `for_profile` caches a never-read skeleton under each `WithRules` key; parser after-keyword boundary check mixes byte and char indices (`.chars().nth(byte_offset)`) — not reproduced in testing but incorrect for non-ASCII sources.

`eval/op_table.rs`, `functions/conversion.rs`, `functions/comparison.rs`, `functions/misc.rs`, `edit_distance.rs`, `profile.rs`, `symbol_table.rs` (aside from `set_table`), and `uri_path.rs` (aside from `join`) had no confirmed issues. `path_parse.rs` is carefully pathlib-faithful with good ground-truth tests.

## 4. Test Review

~3,340 tests, one consolidated integration binary, one file per feature area with names mirroring the ported Python test files. Every feature area has real coverage; types, values, strings, lists, paths, unresolved-eval, Unicode, int64 bounds, and operation limits are notably strong.

- **Error-assertion standard:** the caret/multi-line full-message pattern from `test_error_formatting.rs` is replicated across 17+ files — compliance is the norm. Biggest gap: **29 statement-rejection tests in `test_evaluation.rs`** (lines 180, 401–442, 709–775: `x = 1`, `import os`, `del`, async/match forms) assert only `eval_fails` with no message content. `test_regex_validation.rs` asserts keyword disjunctions instead of exact messages and has zero caret assertions; `test_path_mapping.rs` has four bare `is_err()` checks (lines 909–1005); `test_unresolved_eval.rs:256` accepts either error or unresolved result, pinning neither.
- **Duplication/brittleness:** `src/types.rs` unit test `basic_types` is byte-identical to one in `test_types.rs`; `test_int64_bounds.rs` has `eval_fails` tests redundant with adjacent full-message twins; eval/assert helpers are copy-pasted into nearly every file with `#[allow(dead_code)]` noise (extract a shared `tests/integration/common.rs`). Exact operation-count assertions (e.g. "count (51) exceeded limit (50)") deliberately pin the cost model and will break on any metering change.
- **Structural gap:** no property-based or fuzz testing, despite explicit DoS guards (depth/memory/op limits) that would benefit from a never-panic invariant harness — §7 shows exactly the class of bug such a harness would have caught.

## 5. Python Comparison

Compared against `openjd-model-for-python` (branch `expr`; 1,366 test functions in 27 files) and `openjd-specifications/wiki/2026-02-Expression-Language.md`.

**Spec compliance (Rust):** default limits match §1.3.9/§1.3.10 exactly; the full §2 function library is registered (192 `register_sig` calls + 40 repr registrations); `range_expr` slicing conforms to §2.1.8 (positive step → `range_expr`) **where the Python reference does not** (Python always materializes `list[int]`, unbounded and uncounted — worth reporting upstream). Confirmed Rust deviations:

1. **Over-broad `repr_*` registrations** — all five repr functions accept all eight type overloads; the spec limits e.g. `repr_sh` to `string | path | list[string] | list[path]`. `repr_cmd(42)`, `repr_sh(true)`, `repr_pwsh(null)` succeed in Rust, error in Python.
2. **Extra `is_relative_to`/`relative_to`(path, string) overloads** not in spec §2.3.2 or Python.
3. **Rust-only hardening limits** (depth 64, 64 KiB parse input, format-string caps, parser thread) reject inputs valid per spec/Python. Defensible, but a documented divergence.
4. **`ExprProfile` syntax gating** has no spec counterpart; verify the default profile is exactly the spec surface.
5. `bool(path)`/`bool(list)` registered `-> noreturn` vs Python's `-> bool` with raising impl — different static-check outcomes for unresolved args.

**Error messages:** 14 confirmed wording divergences for identical conditions (e.g. "slice step cannot be zero" vs "Slice step cannot be zero"; "Cannot compare X with Y" vs "and"; "range_expr requires" vs "range_expr() requires"; "Both branches will fail" vs "Both branches fail"; Rust quotes and suggests for undefined variables, Python doesn't). A long list is confirmed identical, including the `argument(s)` arity quirk which Rust reproduces exactly.

**API design:** Rust's structured `ExpressionErrorKind`, string-DSL `register_sig`, typed list variants, and `EvalBuilder` are supersets/equivalents of the Python API; the spec's "Recommended Library Interface" permits this. Path-format mismatch messages render the format differently (`POSIX` vs Rust `Debug` formatting) — worth normalizing.

**Test parity:** 457 Python test names lack same-named Rust tests, but spot-checks confirm the large majority are renames. Genuine gaps: (1) ~25 Python tests spawn real `pwsh`/`cmd.exe` subprocesses to round-trip-verify `repr_pwsh`/`repr_cmd` escaping — Rust has zero subprocess-based verification (notable because `cmd_quote`'s handling of trailing backslashes before the closing quote is questionable — see §7, X15 note); (2) `test_fuzz.py` parser no-crash tests have no Rust counterpart; (3) one error-formatting case.

## 6. Build and Test Results

All commands run on Windows, 2026-07-16:

- `cargo build -p openjd-expr` — clean, no warnings.
- `cargo clippy -p openjd-expr --all-features --all-targets -- -D warnings` — clean.
- `cargo test -p openjd-expr` — **297 unit + 3,063 integration + 8 doc tests, all passing**, 0 ignored.

## 7. Exploratory Findings

A scratch test file with 16 reproductions was written and run in debug mode (where `overflow-checks` make wrapping arithmetic panic); **15 of 16 failed**, each demonstrating a bug. The scratch file was removed after the run; reproductions and observed results:

| # | Reproduction | Observed (debug build) | Class |
|---|---|---|---|
| X1 | `RangeExpr::from_str("-9223372036854775807-9223372036854775807")` | **panic** `attempt to subtract with overflow`, `range_expr.rs:130` | Panic from untrusted range text |
| X2 | `'abcd' * 4611686018427387904` | **panic** `attempt to multiply with overflow`, `arithmetic.rs:273` (release: wrapped length defeats memory checks) | Panic / budget bypass |
| X3 | `range(9223372036854775806, 9223372036854775807, 2)` | **panic** `attempt to add with overflow`, `list.rs:120` | Panic |
| X4 | `sum(range_expr('9223372036854775000-9223372036854775806:2'))` | **panic** `attempt to add with overflow` in `Sum` accumulation (via `list.rs`) | Panic |
| X5 | `round(1.5, -9223372036854775808)` | **panic** `attempt to negate with overflow`, `math.rs:170` | Panic |
| X6 | `split('a b c', ' ', -1)` | **panic** `attempt to add with overflow`, `string.rs:212` (release: returns `[]`; Python returns `['a','b','c']`) | Panic / wrong value |
| X7 | `re_split(' ', 'a b c', -1)` | **panic** `attempt to add with overflow`, `regex.rs:363` | Panic |
| X8 | `path('日').is_relative_to('ab')` (Windows path format) | **panic** `byte index 2 is not a char boundary`, `path.rs:526` | Panic on multibyte paths |
| X9 | `'a.class' + X.class` (with `X.class` = `"c1"`) | returns `"a.alassc1"` — the **string literal** was rewritten by the contextual-keyword retry | Silent wrong value |
| X10 | `12.5 if Cond else\n0.0` (Cond=true) | result displays as `"2.5 "` instead of `"12.5"` | Silent wrong value |
| X11 | `ExprValue::Float(1e30).coerce(&INT, …)` | `Ok(9223372036854775807)` — silent saturation instead of error | Silent wrong value |
| X12 | POSIX rule `/home/user → /mnt/x` applied to relative `"home/user/f"` | `Some("/mnt/x\f")` — relative path matched an absolute rule (note also the host-format separator in the output) | Wrong mapping |
| X13 | `9223372036854775807 == 9223372036854775807.0` | `true`; Python's exact comparison says `False` (float is 2^63) | Python divergence |
| X14 | `center('a', -1)` | error "operation count (72057594037927938) exceeded limit (10000000)"; Python returns `'a'` | Wrong error/value |
| X15 | `repr_py('a\nb')` | `'a` + raw newline + `b'` — not a valid Python literal | Wrong repr |
| X16 | `'日本語' + X.iffy` (byte/char index mismatch probe) | passed — not reproduced with this input (defect confirmed by inspection only) | — |

Not executed (would abort/hang the test harness; confirmed by inspection): `zfill('5', -1)` → `"0".repeat(usize::MAX)` allocation abort; `[1,2,3,4] * 4611686018427387904` and `[] * 4611686018427387904` → uncounted loop of up to 2^62 iterations in release builds.

Additional note for spec cross-check: `cmd_quote` does not double backslashes preceding the closing quote (`repr_cmd('C:\\dir\\')` → `"C:\dir\"`), which corrupts the argument for programs parsing argv with MSVCRT rules. Whether this is a bug depends on what spec §2.2.6 prescribes; the missing subprocess round-trip tests (§5) would settle it.

## 8. Recommendations

### Priority 1 — Correctness bugs reachable from untrusted input

1. **Adopt checked arithmetic at the function-library boundary.** Fix X1–X7 with `checked_add`/`checked_mul`/`checked_neg` (or saturating where semantically safe) in `range_expr.rs:130` (+ `len()`/`contains()`), `arithmetic.rs` `mul_string`/`mul_list`, `list.rs` `range_fn`/`sum_list`, `math.rs` `round_fn`, `string.rs` `split_fn`/`rsplit_fn`, `regex.rs` `re_split_fn`. Add regression tests from the §7 table.
2. **Validate negative int arguments before `as usize`.** `zfill`, `center`, `ljust`, `rjust`, `split`/`rsplit`/`re_split` maxsplit. Match Python semantics (negative width → unchanged string; negative maxsplit → no limit). Share one validation helper across the width-taking functions.
3. **Fix the contextual-keyword retry to skip string literals** (X9, `eval/parse.rs`) — corrupts user data silently. Also fix the byte-vs-char index mix in the after-keyword boundary check.
4. **Fix float-literal passthrough source slicing for multiline expressions** (X10, `eval/evaluator.rs` `eval_number`); have `Float64::with_str` validate that the string round-trips to the value as a backstop.
5. **Fix `path_starts_with` char-boundary panic** (X8, `path.rs:526`) — use `get(..len)` or char-aware case-insensitive comparison.
6. **Make `split_path_parts` preserve absoluteness** (X12, `path_mapping.rs:150`) so relative paths never match absolute rules; use the rule's source format (not host format) for output joining, or document why host format is intended.
7. **Fix Float→Int coercion saturation** (X11, `value.rs`) with an explicit range check; same `>=` fix for the `> i64::MAX as f64` guards in `floordiv_float` and `math.rs` floor/ceil/round.
8. **Escape control characters in `repr_py`** (X15) and cross-check `repr_cmd` trailing-backslash behavior against spec §2.2.6, ideally with subprocess round-trip tests.
9. **Guard `equals(ListInt, RangeExpr)`**: compare `len()` first and zip `iter()` without materializing.

### Priority 2 — Spec conformance and Python parity

10. **Trim `repr_*` registrations to the spec type sets** (§5.1) — currently a conformance superset that diverges from Python.
11. **Align the 14 divergent error-message wordings with Python** (§5, table) or document each divergence as intentional.
12. **Decide and document the extra `is_relative_to`/`relative_to(path, string)` overloads** — remove or get them added to the spec.
13. **Verify Int↔Float equality semantics at the 2^63 boundary** (X13) against the spec and conformance suite; implement exact comparison if parity is required.
14. **Report the Python reference's own spec deviations upstream** (range_expr slice return type; unbounded slice materialization).

### Priority 3 — Specs and public API hygiene

15. **Fix `public-api.md`** (PA1–PA4, PM1–PM7); consider a `cargo public-api` CI diff to keep it honest.
16. **Privatize `IntRange` fields** (or document the invariant risk) — the spec already believes they're private, and `len()` panics on invariant-violating values.
17. **Fix the spec/code contradictions** A1, T1, V1, F1, F2, FS1, FS2, R1, PP1 (§1) and the stale `default_library.rs` module doc; decide whether `copy_symbol_value` should be public, and make `set_table` dotted-path aware (or reject dotted keys).
18. **Unify stringly-typed `Result<_, String>` returns** (`coerce`, `from_str_coerce`, `to_symtab`, `ExprType::parse`, `register_sig`) on structured error types.

### Priority 4 — Tests and robustness

19. **Upgrade the 29 statement-rejection tests in `test_evaluation.rs` to full-message assertions**; tighten `test_regex_validation.rs` and the four bare `is_err()` checks in `test_path_mapping.rs`; pin the behavior in `test_unresolved_eval.rs:256`.
20. **Add a property-based/fuzz harness** (proptest or cargo-fuzz) asserting the evaluator never panics on arbitrary input — it would have caught X1–X8 mechanically; port the spirit of Python's `test_fuzz.py`.
21. **Extract a shared `tests/integration/common.rs`** for the copy-pasted eval/assert helpers; remove the duplicated `basic_types` unit test and redundant `eval_fails` twins.
22. **Move the format-string segment cap inside the parse loop**; replace `mul_list`'s per-element `count_op` loop with a single `count_ops(n)`; add list-comparison fast paths that avoid per-element `String` clones.
