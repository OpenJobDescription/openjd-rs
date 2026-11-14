# Fuzzing openjd-rs

Coverage-guided fuzzing ([cargo-fuzz] / libFuzzer, AddressSanitizer, overflow
checks on) for every library crate, with **property oracles** rather than only
"does not panic", and a CI gate that proves — from compiler coverage data, not
a hand-kept list — that the committed corpus executes every public function
not excluded with a reason in `api_coverage.toml`.

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
scripts/fuzz.sh corpus                       # rebuild the corpus from repo fixtures + committed inputs
# (cmin and corpus also need Python 3.11+ and `rustup component add llvm-tools`)
scripts/fuzz.sh sync-lock                    # fuzz/Cargo.lock := workspace lock's versions + committed fuzz-only pins
scripts/fuzz.sh dict                         # regenerate dicts/expr.dict

# Every public function executed by the corpus? (Python 3.11+, nightly, cargo-llvm-cov)
python3 scripts/check_fuzz_api_coverage.py --generate --report
```

On Windows, run `scripts/fuzz.sh` from Git Bash with the Visual C++ x64 build
tools installed (MSVC's AddressSanitizer). The script handles two
Windows-specific steps; call `cargo fuzz` directly only with the same flag and
`PATH`:

- It passes `--no-include-main-msvc`. cargo-fuzz otherwise adds the
  `/include:main` linker flag to every crate, and `crc-fast` (an
  `openjd-snapshots` dependency that also builds a DLL) fails to link with it.
  `build.rs` adds the flag back for the fuzz binaries only.
- It puts the directory holding `clang_rt.asan_dynamic-x86_64.dll` on `PATH`
  (without it the binaries exit with `STATUS_DLL_NOT_FOUND`). It finds the
  directory from `VCToolsInstallDir` or `vswhere`; set `FUZZ_ASAN_DLL_DIR` to
  override (for example, to match a non-default toolset rustc links with).

## Layout

| Path | What |
|------|------|
| `src/targets/*.rs` | Target functions (`fn(&[u8])`) and their oracles, one module per crate. `targets::TARGETS` is the registry. |
| `src/gen/*.rs` | Structure-aware generators: typed values, signature-driven expressions, templates, manifests. |
| `fuzz_targets/*.rs` | One-line libFuzzer shims, one per registry entry. |
| `tests/replay.rs` | Stable replay of `corpus/` and `regressions/`; registry/shim/corpus/dictionary consistency checks. |
| `corpus/<target>/` | Committed, minimized seed corpus (mined from the repo's tests and the specification conformance suite, then grown by fuzzing). Reduced to a set cover of the openjd functions each target executes (`scripts/select_fuzz_corpus.py`, after `cargo fuzz cmin`), so it stays small; CI caches a richer grown corpus. |
| `regressions/<target>/` | Inputs that found bugs. `known-*` files reproduce open known issues. |
| `dicts/expr.dict` | Dictionary for text targets; function names come from the live function library. |
| `api_coverage.toml` | Public functions deliberately not fuzzed, each with a reason. |
| `build.rs` | On MSVC, links libFuzzer's `main` into the fuzz binaries (see Quick start). |

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
| `sessions_state` | `Session::clear_ending_only` over random `set_state_for_test` sequences | Reported state matches a reference model of the stored state and ending-only flag; `clear_ending_only` succeeds only from `ReadyEnding` (then `Ready`), else `InvalidState` with no change |
| `snapshots_decode_text` | `decode_manifest` / `decode_v2023` / `decode_v2025` + `validate` + all encoders | Encoding deterministic; decode(encode(m)) preserves files and metadata; re-encoding is a fixed point |
| `snapshots_ops_gen` | `diff_snapshots`, `compose_*`, `subtree_snapshot`, `join_snapshot`, `partition_manifest`, `filter_manifest`, `entries_differ`, `common_root` | `diff(a, a)` empty; `compose(a, [diff(a, b)]) == b`; compose of diffs ≡ sequential compose; subtree/join stays inside; partitions cover every file; filters keep only matches |
| `cli_args` | CLI `--job-param`, `--task-param`, `--tasks`, `--path-mapping-rules`, `--extensions` parsers (source-included from the bin crate) | `Name=Value` parses to that pair; one task param per argument; only supported extensions accepted |

**`check_expression`** (every expression target) evaluates the same parsed
expression statically — every symbol `Unresolved(T)`, validation-time library —
and concretely with values of type `T`, under both POSIX and Windows path
formats, and requires:

- the concrete value belongs to the static type, and a statically folded value
  equals the run-time value (type soundness);
- a statically accepted expression never fails at run time with an
  undefined-symbol, unknown-function, unsupported-syntax, or type error;
- determinism; evaluation metrics within their limits; a target-typed result
  satisfies its target; the result survives the template → session transport
  boundary unchanged;
- **peak heap during evaluation within a constant factor of the memory limit**,
  measured by a tracking global allocator (`src/alloc.rs`). This catches
  "allocate, then check the budget" bugs (`zfill('5', -1)`) that a check
  on the result value cannot see.

**`pipeline`** (every model target) runs preprocess → `create_job` → parameter
space iteration → dependency graph → run-time resolution, and requires:
preprocessing is idempotent; `create_job` is deterministic and
Eq/Hash agree; `len()` matches iteration, `get(i)` equals the i-th iterated
set, and every iterated set passes `contains`/`validate_containment`; topological order respects every dependency; and **a template that
validates and instantiates never fails at run time with a static error** when
its actions, `let` bindings, and embedded files are resolved the way the
session resolves them.

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
- **Known issues stay honest.** Each `regressions/<target>/known-<id>[--note]`
  must still reproduce `<id>`, and every known issue must have one
  (`known_issue_reproducers_reproduce`), so a fixed bug fails until its gate
  becomes an `oracle!`. A `--posix-host` / `--windows-host` note marks a
  reproducer that depends on the host's path format; it runs only there.
- **Every target is fuzzed in CI.** The `fuzz.yml` fuzz job runs every
  registered target in turn (`ci_fuzzes_every_target`).
- **Public API coverage** (`scripts/check_fuzz_api_coverage.py`). rustdoc JSON
  enumerates, for every workspace library crate (new crates included unless
  opted out in the script's `NOT_GATED`), every public free function, inherent
  method, and method of a hand-written (not derived) trait impl. llvm source
  coverage of the corpus and regression replays (not the harness self-checks)
  must show each one's own function record executed — matched by demangled
  definition path, so macro-generated methods count separately — unless
  `api_coverage.toml` excludes it by exact path with a reason. Glob
  exclusions, stale exclusions, and exclusions of executed functions all fail.
  Executing a function is the floor, not the goal: call it with generated
  input and check a property, never with constant arguments just to satisfy
  the gate. Not covered: Windows-only code (rustdoc runs on Linux),
  `#[doc(hidden)]` items, and the CLI binary (its argument parsers are fuzzed
  by `cli_args`). Known imprecision: functions are keyed by definition path
  without generic arguments, so several impls of one trait for a type
  (`From<A>`, `From<B>`) or same-named methods of `impl Foo<A>` / `impl
  Foo<B>` count as one entry.

## Known issues

Bugs the fuzzer found that need a design decision are gated with
`known_issue!` (listed in `src/lib.rs::known::KNOWN_ISSUES`, with reproducers
in `regressions/*/known-*`). A gated violation is ignored so CI stays green;
`OPENJD_FUZZ_KNOWN=1` turns every gate back into a hard failure, and
`OPENJD_FUZZ_KNOWN=<id>[,<id>...]` only the named ones (an input can hit more
than one gate, and the first to fire is the one reported):

```bash
OPENJD_FUZZ_KNOWN=1 cargo test --manifest-path fuzz/Cargo.toml --no-default-features --test replay replay_regressions
OPENJD_FUZZ_KNOWN=model-preprocess-path-idempotence cargo test --manifest-path fuzz/Cargo.toml --no-default-features --test replay replay_regressions
```

When a known issue is fixed, change its `known_issue!` to `oracle!` and delete
the entry, so the fuzzer guards the fix. Rename its reproducers after the
behavior they check (no `known-` prefix, and no `fixed-` or issue-number
marker): `regressions/` describes what is checked, not which change fixed it.

## Adding a target

1. Write `pub fn my_target(data: &[u8])` in `src/targets/<crate>.rs`; panic
   (via `oracle!`) on any violated property.
2. Add it to `targets::TARGETS`, add `fuzz_targets/my_target.rs` and a
   `[[bin]]` entry (copy an existing one).
3. Seed it: extend `examples/build_corpus.rs` or drop files into
   `corpus/my_target/`, then `scripts/fuzz.sh cmin my_target`.
CI fuzzes it from then on: the `fuzz.yml` fuzz job runs every target.

`cargo test --manifest-path fuzz/Cargo.toml --no-default-features` fails
until all three steps agree.

## When the fuzzer finds a bug

1. The crashing input is in `fuzz/artifacts/<target>/` (CI uploads it as a
   workflow artifact).
2. Reproduce: `cargo +nightly fuzz run <target> <file>`, or minimize with
   `cargo +nightly fuzz tmin <target> <file>`. On Windows, add
   `--no-include-main-msvc` and put the ASan runtime's directory on `PATH`
   (see Quick start).
3. Fix it, add a unit test in the crate, and copy the input to
   `fuzz/regressions/<target>/`. Inputs for generated (`*_gen`) targets are
   interpreted by the generator, so a generator change can make them
   reproduce something else; the unit test is the durable guard.
