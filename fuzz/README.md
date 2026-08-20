<!--
Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
Copyright by contributors to this project.
SPDX-License-Identifier: (Apache-2.0 OR MIT)
-->

# openjd-rs fuzzing

Coverage-guided ([libFuzzer](https://llvm.org/docs/LibFuzzer.html) via
[`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz)) fuzz targets for the
crates that parse or evaluate untrusted input. These targets encode, as
permanent executable checks, the class of bug the quality-evaluation reports
kept rediscovering by hand: arithmetic overflow, char-boundary panics, and
other crashes reachable from attacker-controlled expression text, templates,
and manifests.

This crate is **not** a member of the root workspace (it has its own empty
`[workspace]` table). It builds only under a nightly toolchain with
AddressSanitizer, so keeping it out of the workspace leaves the stable
build/test/clippy/MSRV jobs untouched. It runs in its own
[`.github/workflows/fuzz.yml`](../.github/workflows/fuzz.yml) workflow.

## Targets

| Target | Entry point | What it guards |
|--------|-------------|----------------|
| `expr_parse` | `ParsedExpression::new` | Expression parser: no panic/abort below the input-length and depth caps. |
| `expr_evaluate` | `ParsedExpression::new` + `evaluate` (both POSIX and Windows path formats) | Full evaluator against a fuzzer-seeded symbol table — the arithmetic / path / coercion code where most historical crashes lived, including char-boundary handling of multibyte paths in the Windows path branch. |
| `range_expr` | `RangeExpr::from_str` + `len`/`get`/`iter` | Range parsing and materialization: integer overflow on extreme endpoints. |
| `int_range_new` | `IntRange::new` | Public range constructor with arbitrary `(start, end, step)` i64 triples — overflow in normalization the text parser can't reach (e.g. `step == i64::MIN`). |
| `range_expr_slice` | `RangeExpr::slice` | Public slice with arbitrary `i64` indices — overflow in the sub-range stride remapping. |
| `expr_type_parse` | `ExprType::parse` | Public recursive type-string parser (`list[int]`, `union[...]`) — malformed / deeply-nested input. |
| `format_string` | `FormatString::new` + `resolve_string_with` | Format-string parse + resolve, including embedded expressions. |
| `copy_symbol_value` | `copy_symbol_value` | Dotted-symbol walk/copy between symbol tables — adversarial names (empty segments, deep nesting, collisions). |
| `model_decode` | `document_string_to_object` + `decode_{job,environment}_template` | YAML/JSON template decode + validation (the `openjd check` path). |
| `model_create_job` | `preprocess_job_parameters` + `create_job` | Full template → job instantiation (the `openjd run` path): parameter coercion, format-string eval, parameter-space iteration and chunk arithmetic — one layer past `model_decode`. |
| `snapshot_decode` | `decode_manifest` + `Manifest::validate` | Snapshot manifest decode + invariant validation. |
| `snapshot_ops` | `diff`/`compose`/`partition`/`subtree`/`filter` on decoded manifests | Manifest operations over attacker-controlled decoded manifests — merging, size accounting, path arithmetic past `validate()`. |

Every target's invariant is the same: for **any** input, the fuzzed code must
return `Ok` or `Err` — never panic, abort, or hang.

## Running locally

```sh
# One-time: nightly toolchain + cargo-fuzz.
rustup toolchain install nightly-2026-05-15 --component rust-src
cargo install cargo-fuzz --locked

# Build + smoke-fuzz every target (this is exactly what CI runs). The target
# list is derived from `cargo fuzz list`, so new targets are picked up
# automatically — no list to maintain.
scripts/run_fuzz.sh

# Longer campaign, or only specific targets:
FUZZ_SECONDS=300 scripts/run_fuzz.sh
scripts/run_fuzz.sh expr_evaluate range_expr

# Or drive cargo-fuzz directly for one target, seeded with its committed corpus.
cargo +nightly-2026-05-15 fuzz run expr_evaluate fuzz/seeds/expr_evaluate

# Reproduce / minimize a crash artifact.
cargo +nightly-2026-05-15 fuzz run range_expr fuzz/artifacts/range_expr/crash-<hash>
cargo +nightly-2026-05-15 fuzz tmin range_expr fuzz/artifacts/range_expr/crash-<hash>
```

`cargo fuzz build` builds every target with `overflow-checks` and
`debug-assertions` on (see `Cargo.toml`), so arithmetic overflow traps instead
of wrapping — the whole point of fuzzing this code base.

## Corpus

`seeds/<target>/` holds a **small, curated** set of starter inputs — one per
structurally-distinct input shape, plus known edge cases (multibyte strings,
i64 boundaries, symbol-table-prefixed expressions) — mined from the crates' own
tests and sample templates. Keep these lean: the goal is to prime each grammar
path once on a cold run, not to mirror the evolved corpus. The mutator
rediscovers trivial variants in seconds, so committing near-duplicates only
bloats the tree. A dozen-to-a-few-dozen files per target is the target size.

CI seeds each run from this directory. The evolving runtime corpus
(`corpus/<target>/`) and crash artifacts (`artifacts/`) are git-ignored;
locally, `cargo fuzz` grows the runtime corpus across runs so coverage compounds
as you keep fuzzing. Note that running a target with `fuzz/seeds/<t>` as the
corpus argument makes libFuzzer write newly-discovered inputs (SHA1-named files)
back into the seed dir — `.gitignore` excludes those so they can't be committed
by accident; only readably-named curated seeds are tracked.
