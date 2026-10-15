# Fuzzing openjd-rs

Coverage-guided fuzzing ([cargo-fuzz] / libFuzzer, AddressSanitizer, overflow
checks on) for every library crate, with **property oracles** rather than only
"does not panic", and a CI gate that proves — from compiler coverage data, not
a hand-kept list — that the committed corpus executes every public function.

[cargo-fuzz]: https://github.com/rust-fuzz/cargo-fuzz

## Quick start

```bash
# Replay every committed input through every oracle (stable Rust, any OS).
cargo test --manifest-path fuzz/Cargo.toml --no-default-features

# Fuzz (nightly + cargo-fuzz: `cargo install cargo-fuzz`).
scripts/fuzz.sh run                          # every target, 60 s each
FUZZ_SECONDS=1800 scripts/fuzz.sh run expr_eval_gen
scripts/fuzz.sh replay                       # corpus + regressions under ASan
scripts/fuzz.sh cmin                         # fold fuzz/work/ back into fuzz/corpus/
scripts/fuzz.sh corpus                       # rebuild the corpus from repo fixtures
scripts/fuzz.sh dict                         # regenerate dicts/expr.dict

# Every public function executed by the corpus? (nightly + cargo-llvm-cov)
python3 scripts/check_fuzz_api_coverage.py --generate --report
```

## Layout

| Path | What |
|------|------|
| `src/targets/*.rs` | Target functions (`fn(&[u8])`) and their oracles, one module per crate. `targets::TARGETS` is the registry. |
| `src/gen/*.rs` | Structure-aware generators: typed values, signature-driven expressions, templates, manifests. |
| `fuzz_targets/*.rs` | One-line libFuzzer shims, one per registry entry. |
| `tests/replay.rs` | Stable replay of `corpus/` and `regressions/`; registry/shim/corpus/dictionary consistency checks. |
| `corpus/<target>/` | Committed, minimized seed corpus (mined from the repo's tests and the specification conformance suite, then grown by fuzzing). |
| `regressions/<target>/` | Inputs that found bugs. `known-*` files reproduce open known issues. |
| `dicts/expr.dict` | Dictionary for text targets; function names come from the live function library. |
| `api_coverage.toml` | Public functions deliberately not fuzzed, each with a reason. |

## Targets

| Target | Entry points | Oracles beyond "no panic / hang / abort" |
|--------|--------------|------------------------------------------|
| `expr_parse` | `ParsedExpression::new`, `with_profile` | Profile monotonicity (`current()` ⊆ `latest()`), determinism |
| `expr_eval_text` | parse + evaluate on mutated text; symbols bound from `accessed_symbols` | All of `check_expression` (below) |
| `expr_eval_gen` | Well-typed expressions generated from the library's own signatures | `check_expression`, plus: a well-typed expression is not rejected statically and does not fail at run time with a type error |
| `expr_format_string` | `FormatString::new`, `resolve_with`, `resolve_string_with`, `validate_expressions`, `copy_used_symtab_values`, `escape_format_string` | Static resolution is sound (`min_resolved_string_len` ≤ actual, `resolved_value` exact, `resolved_type` satisfied); literals preserved in order; escaping round-trips; filtered table resolves identically; peak heap bounded |
| `expr_range` | `RangeExpr::from_str` and every query | Reference model against a `Vec<i64>`: `len`/`iter`/`get` (incl. negative)/`contains`/`slice`/`from_values`/`from_ranges`; display ↔ parse; Eq/Hash agreement |
| `expr_int_range` | `IntRange::new` over all of `i64` | Constructor invariants; `len`/`get`/`iter`/`contains` agree |
| `expr_type` | `ExprType::parse`, constructors | display ↔ parse, normalization idempotence, reflexive satisfaction, union idempotence |
| `expr_value` | `ExprValue` transport, `coerce`, `from_str_coerce`, `compare`, `make_list`, `Float64` serde | Transport round trip; equality ⇒ equal hash; `compare` antisymmetric; coercion satisfies its target and is idempotent; scalar display ↔ `from_str_coerce` |
| `expr_function_call` | `FunctionLibrary::call` / `call_method` for every function with signature-shaped arguments | A successful call has a derivable return type its value satisfies; result within the memory budget; peak heap bounded |
| `expr_path` | `PathMappingRule::apply*`, `apply_rules_with_format`, `uri_path::*`, PATH properties | First-match-wins agrees with per-rule `apply`; `is_uri` ⇔ `parse`; `name == stem + suffix` (URI and Posix/Windows) |
| `expr_symbol_table` | `SymbolTable` ops, `SerializedSymbolTable`, serde of `SymbolTable`/`RangeExpr`/`FormatString` | Dotted-key ops against a map model; transport round trip; canonical encoding; `merge_from` |
| `model_decode_text` | `document_string_to_object` (YAML, JSON) → `decode_template` → full pipeline | `pipeline` (below); `decode_template` agrees with `decode_job_template`/`decode_environment_template` |
| `model_job_gen` | Generated job templates (+ env templates) → full pipeline | `pipeline`; the YAML rendering decodes like the JSON |
| `model_env_gen` | Generated environment templates, `merge_job_parameter_definitions`, `convert_environment_with_symtab` | `resolved_symtab` decodes; `pipeline` with the env template |
| `model_misc` | capability validators, `JobParameterType`/`TaskParameterType`/`TemplateSpecificationVersion` parsing, `evaluate_let_bindings` | Spec-string round trips |
| `sessions_directives` | `ActionFilter` / `process_line` on subprocess output (via the `fuzzing` feature) | Redacted secrets never appear in any later output (or their own directive line); env changes have valid names and no NUL; progress in range |
| `sessions_framer` | Cross-user helper `LineFramer`, `fit_out_payload`, protocol `Command` parsing | Helper and session decoders agree byte for byte; framing independent of chunking; every response line under the cap |
| `sessions_embedded_files` | `EmbeddedFiles::allocate_file_paths`, `convert_line_endings` | Allocated paths stay inside the files directory; every filename template validation accepts, the session accepts; line counts preserved |
| `snapshots_decode_text` | `decode_manifest` / `decode_v2023` / `decode_v2025` + `validate` + all encoders | Encoding deterministic; decode(encode(m)) preserves files and metadata; re-encoding is a fixed point |
| `snapshots_ops_gen` | `diff_snapshots`, `compose_*`, `subtree_snapshot`, `join_snapshot`, `partition_manifest`, `filter_manifest`, `entries_differ`, `common_root` | `diff(a, a)` empty; `compose(a, [diff(a, b)]) == b`; compose of diffs ≡ sequential compose; subtree/join stays inside; partitions cover every file; filters keep only matches |
| `cli_args` | CLI `--job-param`, `--task-param`, `--tasks`, `--path-mapping-rules`, `--extensions` parsers (source-included from the bin crate) | `Name=Value` parses to that pair; one task param per argument; only supported extensions accepted |

**`check_expression`** (every expression target) evaluates the same parsed
expression statically — every symbol `Unresolved(T)`, validation-time library —
and concretely with values of type `T`, under both POSIX and Windows path
formats, and requires:

- the concrete value belongs to the static type, and a statically folded value
  equals the run-time value (type soundness — the most frequent bug class in
  this repo's history: #300, #316, #329, #396, #434, ...);
- a statically accepted expression never fails at run time with an
  undefined-symbol, unknown-function, unsupported-syntax, or type error;
- determinism; evaluation metrics within their limits; a target-typed result
  satisfies its target; the result survives the template → session transport
  boundary unchanged;
- **peak heap during evaluation within a constant factor of the memory limit**,
  measured by a tracking global allocator (`src/alloc.rs`). This catches
  "allocate, then check the budget" bugs (#284, `zfill('5', -1)`) that a check
  on the result value cannot see.

**`pipeline`** (every model target) runs preprocess → `create_job` → parameter
space iteration → dependency graph → run-time resolution, and requires:
preprocessing is idempotent (#384, #388); `create_job` is deterministic and
Eq/Hash agree; `len()` matches iteration, `get(i)` equals the i-th iterated
set, and every iterated set passes `contains`/`validate_containment` (#398,
20c7876); topological order respects every dependency; and **a template that
validates and instantiates never fails at run time with a static error** when
its actions, `let` bindings, and embedded files are resolved the way the
session resolves them (#349, #397, #421, #315, #361).

## Enforcement

Each check below is executable, and each was verified to fail when broken:

- **Overflow checks really are on.** `lib.rs` refuses to compile without debug
  assertions, and `tests/replay.rs::overflow_checks_are_enabled` asserts an
  overflow panics.
- **Registry consistency.** `targets::TARGETS`, the `[[bin]]` entries, the
  `fuzz_targets/` shims, and non-empty `corpus/` directories must all agree
  (`registry_matches_bins_files_and_corpus`).
- **Dictionary currency.** Every function/method/property name the library
  registers must be in `dicts/expr.dict`
  (`expression_dictionary_covers_the_library`).
- **Regressions stay fixed on every OS.** `regressions/` replays on Linux,
  macOS, and Windows in CI.
- **Public API coverage** (`scripts/check_fuzz_api_coverage.py`). rustdoc JSON
  enumerates every public free function and inherent method of openjd-expr,
  openjd-model, openjd-sessions, and openjd-snapshots; llvm source coverage of
  the corpus replay must show each one executed, unless `api_coverage.toml`
  excludes it with a reason. A stale exclusion fails too. Naming a function
  proves nothing — only executing it does.

## Known issues

Bugs the fuzzer found that need a design decision are gated with
`known_issue!` (listed in `src/lib.rs::known::KNOWN_ISSUES`, with reproducers
in `regressions/*/known-*`). A gated violation is ignored so CI stays green;
`OPENJD_FUZZ_KNOWN=1` turns every gate back into a hard failure:

```bash
OPENJD_FUZZ_KNOWN=1 cargo test --manifest-path fuzz/Cargo.toml --no-default-features replay_regressions
```

When a known issue is fixed, change its `known_issue!` to `oracle!` and delete
the entry, so the fuzzer guards the fix.

## Adding a target

1. Write `pub fn my_target(data: &[u8])` in `src/targets/<crate>.rs`; panic
   (via `oracle!`) on any violated property.
2. Add it to `targets::TARGETS`, add `fuzz_targets/my_target.rs` and a
   `[[bin]]` entry (copy an existing one).
3. Seed it: extend `examples/build_corpus.rs` or drop files into
   `corpus/my_target/`, then `scripts/fuzz.sh cmin my_target`.
4. Add it to a group in `.github/workflows/fuzz.yml`.

`cargo test --manifest-path fuzz/Cargo.toml --no-default-features` fails
until steps 1–3 agree.

## When the fuzzer finds a bug

1. The crashing input is in `fuzz/artifacts/<target>/` (CI uploads it as a
   workflow artifact).
2. Reproduce: `cargo +nightly fuzz run <target> <file>`, or minimize with
   `cargo +nightly fuzz tmin <target> <file>`.
3. Fix it, add a unit test in the crate, and copy the input to
   `fuzz/regressions/<target>/`. Inputs for generated (`*_gen`) targets are
   interpreted by the generator, so a generator change can make them
   reproduce something else; the unit test is the durable guard.
