# AGENTS.md

## Project Overview

openjd-rs is a Rust implementation of the [Open Job Description](https://github.com/OpenJobDescription) specification. It provides a model library, expression language, sessions runtime, job attachments snapshots, and CLI for working with OpenJD job templates.

The canonical specification lives in [openjd-specifications](https://github.com/OpenJobDescription/openjd-specifications). The reference Python implementations are [openjd-model-for-python](https://github.com/OpenJobDescription/openjd-model-for-python), [openjd-sessions-for-python](https://github.com/OpenJobDescription/openjd-sessions-for-python), and [openjd-cli](https://github.com/OpenJobDescription/openjd-cli).

See [README.md](README.md) for user-facing documentation and [DEVELOPMENT.md](DEVELOPMENT.md) for general developer setup.

## Quick Reference

```bash
cargo build --release                    # Build (binary at target/release/openjd)
cargo test --workspace                   # All tests
cargo test -p openjd-expr                # Single crate
cargo clippy --all-features --all-targets --workspace -- -D warnings  # Lint
cargo fmt --all                          # Apply formatting
cargo doc --no-deps --workspace          # Build docs
scripts/coverage.sh                      # Code coverage (see COVERAGE_REPORT.md)
```

MSRV: **1.94.1** (enforced in CI).

## Crate Map

```
openjd-cli
├── openjd-sessions
│   ├── openjd-model
│   │   └── openjd-expr
│   └── openjd-expr
└── openjd-model

openjd-snapshots (standalone, experimental)
openjd-for-js (depends on openjd-model + openjd-expr, experimental)
```

Changes to `openjd-expr` can affect all other crates. `openjd-snapshots` has no in-workspace dependents.

### openjd-expr (`crates/openjd-expr`)

Expression language implementation. The most mature crate.

- **Type system** (`src/types.rs`) — `ExprType` with type codes for primitives, lists, unions, type variables, `unresolved[T]`, `noreturn`, and `any`. Includes string parsing, normalization, and type matching/substitution for generic function signatures.
- **Values** (`src/value.rs`) — `ExprValue` enum with typed list variants (`ListBool`, `ListInt`, `ListFloat`, `ListString`, `ListPath`, `ListList`), float passthrough for preserving original string representations, and `Unresolved` for static type checking.
- **Parser** (`src/eval/parse.rs`) — Uses `ruff_python_parser` to parse Python expression syntax. Handles contextual keywords via same-length identifier replacement.
- **Evaluator** (`src/eval/evaluator.rs`) — Walks the ruff AST with memory-bounded and operation-bounded execution. Implements arithmetic, comparison, logical ops, conditionals, function calls, method calls, list comprehensions, slicing, string operations, path operations, regex, and repr functions.
- **Format strings** (`src/format_string.rs`) — Parses `{{Param.Name}}` and `{{Expr.Name}}` syntax in template strings.
- **Range expressions** (`src/range_expr.rs`) — Parses range expressions like `1-10`, `1-100:10`, `1,5,10-20`.
- **Path mapping** (`src/path_mapping.rs`) — Applies source→destination path mapping rules.
- **Symbol table** (`src/symbol_table.rs`) — Hierarchical key-value store supporting dotted paths (`Param.Frame`).

Spec entry point: `specs/expr/README.md`

### openjd-model (`crates/openjd-model`)

Template parsing, validation, and job creation. Parses YAML/JSON templates, validates against the 2023-09 schema, resolves format strings, and creates job structures.

- **Parsing** (`src/parse.rs`) — Serde-based YAML/JSON deserialization with post-deserialization validation.
- **Template types** (`src/template/`) — All v2023-09 model types including parameters, steps, environments, actions, host requirements.
- **Validation** (`src/validate.rs`, `src/validate/`) — Structural and cross-field constraint validation with Pydantic-compatible error paths.
- **Job creation** (`src/job/`) — Instantiates jobs from templates: parameter resolution, format string evaluation, parameter space iteration, step dependency graphs.
- **Capabilities** (`src/capabilities.rs`) — Host requirement capability matching.

Spec entry point: `specs/model/README.md`

### openjd-sessions (`crates/openjd-sessions`)

Local job execution runtime. Manages session lifecycle, runs actions via subprocess, handles environment setup/teardown.

- **Session** (`src/session.rs`) — Session state machine: enter environments, run task actions, cleanup.
- **Subprocess** (`src/subprocess.rs`) — Process spawning, I/O streaming, signal handling (SIGTERM/SIGKILL), process tree management.
- **Action filter** (`src/action_filter.rs`) — Real-time stdout/stderr message parsing, redaction, and annotation.
- **Cross-user** (`src/cross_user_helper.rs`) — Subprocess execution as a different OS user via an embedded helper binary.
- **Embedded helper** (`src/helper/`) — **Separate Cargo project** with its own `Cargo.toml` and `Cargo.lock`. Built by `build.rs` and embedded into the sessions binary. CI runs clippy and tests on it independently. Platform-specific runners: `src/helper/src/runner.rs` (Unix) and `runner_win.rs` (Windows).
- **Platform-specific code** — `win32.rs`, `win32_permissions.rs`, `win32_locate.rs` use `#[cfg(target_os = "windows")]`. Cross-user tests require Docker (Linux) or a test user account (Windows).

Spec entry point: `specs/sessions/README.md`

### openjd-cli (`crates/openjd-cli`)

CLI binary (`openjd`) with `check`, `summary`, and `run` subcommands.

- **check** (`src/check.rs`) — Template validation.
- **summary** (`src/summary.rs`) — Job/step summary display.
- **run** (`src/run/`) — Local job execution using the sessions runtime.
- **help** (`src/help.rs`) — Context-aware help text with markdown stripping for terminal display.

Spec entry point: `specs/cli/README.md`

### openjd-snapshots (`crates/openjd-snapshots`)

**Status: experimental.** Public API may change without notice. Its on-disk
formats have distinct maturity: **v2023** is stable and is the format AWS
Deadline Cloud uses; **v2025** is an **experimental draft** (on-disk
`specificationVersion` strings carry the `beta-2025-12` tag) whose wire
format is expected to change before any stable release.

Job attachments: content-addressed file tree snapshots with xxHash3 hashing, manifest diffing, S3 upload/download. Standalone — no dependency on other workspace crates.

- **Manifest** (`src/manifest.rs`) — Manifest types, serialization (v2023 and v2025 formats), validation.
- **Operations** (`src/ops/`) — collect, hash, filter, subtree, partition, join, compose, diff, hash_upload, download.
- **Caching** (`src/hash_cache.rs`, `src/data_cache.rs`, `src/s3_check_cache.rs`) — Local hash cache, S3 data cache, upload deduplication.
- **Codec** (`src/codec.rs`) — Binary encoding/decoding for manifest formats.

Spec entry point: `specs/snapshots/README.md`

### openjd-for-js (`crates/openjd-for-js`)

**Status: experimental.** Not published to crates.io or to npm. The JS
surface, the Rust API, the WASM package layout, and the build pipeline may
all change without notice.

ECMAScript/WebAssembly bindings for `openjd-model` and `openjd-expr`. Depends on both via Cargo, compiles to `wasm32-unknown-unknown`, and produces an npm package via `wasm-bindgen`. Source is Rust (`src/lib.rs`, `src/model.rs`, `src/expr.rs`, `src/errors.rs`) alongside npm manifest, Vitest config, and JS-side integration tests (`js-tests/`).

Spec entry point: `specs/openjd-for-js/spec.md`

## Navigating the Codebase

### Spec + code co-evolution

The `specs/` directory is the primary resource for understanding each crate's design. Specs and code evolve together — within a coding session, you might edit the code first and then update the spec, or write the spec first and then implement, or iterate on both simultaneously. The order doesn't matter, but **before committing, always confirm the spec and code line up.** If you changed behavior, the spec must reflect it. If you changed the spec, the code must match.

Every crate's spec directory must include a `public-api.md` that fully describes the crate's public API — all public types, functions, traits, and constants with their signatures. When adding or changing public API surface, update `public-api.md` in the same commit.

The structure:

```
specs/
├── architecture.md              # Top-level crate structure and design
├── expr/README.md               # → expr spec index
├── model/README.md              # → model spec index
├── sessions/README.md           # → sessions spec index
├── snapshots/README.md          # → snapshots spec index
├── cli/README.md                # → cli spec index
├── job-attachments-snapshots.md # Cross-cutting design doc
├── rust-port-agent-method.md    # Porting methodology from Python
└── windows-cross-user-helper.md # Cross-cutting Windows design
```

Start with `specs/<crate>/README.md` for any crate. It indexes all spec documents for that crate and explains the relationships between them.

### Report-driven development

Many tasks originate from quality evaluation reports in `reports/`. Each report has a numbered recommendations section with priority groupings.

**Workflow for report-driven changes:**

1. Read the relevant report in `reports/` to understand the recommendation.
2. Implement the change.
3. In the **same commit**, update the report to mark the item as resolved by striking it through with `~~` and appending `**Resolved.**` or `**Resolved** — <brief note>.`

Example — before:
```markdown
6. **Decompose `validate_format_strings()`** into per-scope helpers.
```

After:
```markdown
6. ~~**Decompose `validate_format_strings()`** into per-scope helpers.~~ **Resolved.**
```

This keeps reports accurate as a living record of what's been done and what remains. If most items in a report are resolved, suggest to the user that they run the `eval-crate` skill to regenerate a fresh report for that crate.

Current reports:
| Report | Crate |
|--------|-------|
| `reports/expr-quality-evaluation-report.md` | openjd-expr |
| `reports/model-quality-evaluation-report.md` | openjd-model |
| `reports/sessions-quality-evaluation-report.md` | openjd-sessions |
| `reports/snapshots-quality-evaluation-report.md` | openjd-snapshots |

## Conventions

### Commit Messages

This repo requires [conventional commit](https://www.conventionalcommits.org/en/v1.0.0/) syntax. All commits must use it.

Types: `feat`, `fix`, `test`, `docs`, `refactor`, `ci`, `chore`, `perf`

Append `!` for breaking changes (e.g., `feat!: ...`) and include a `BREAKING CHANGE` footer. **Note:** This is relaxed during pre-release — make any change that improves things. Enable strict breaking-change tracking once the project reaches stable release.

### Test Quality Standard

When writing tests that check for validation or evaluation failures, assert on the **full error message content** — not just that an error occurred. This ensures error messages are stable, human-readable, and match the Python implementation.

**openjd-expr: assert message + expression + caret**

Every expression evaluation error test must assert the multi-line error including the message, the expression source line, and the caret indicator. See `tests/test_error_formatting.rs`:

```rust
#[test] fn type_error_in_middle() {
    assert_err("1 + int('bad') + 2", &[
        "Cannot convert 'bad' to int\n",
        "  1 + int('bad') + 2\n",
        "      ^~~~~~~~~~",
    ]);
}
```

**openjd-model: assert path + message**

Every template validation error test must assert the field path and message, matching the Python Pydantic error format. See `tests/test_error_messages.rs`:

```rust
#[test]
fn empty_command() {
    check_err(r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": ""}}}}]
    }"#, &[
        "steps[0] -> script -> actions -> onRun -> command:\n\tmust not be empty.",
    ]);
}
```

**Why:** Catches regressions in error message quality, ensures Rust and Python produce comparable output, and makes error paths testable. Conformance tests only check pass/fail — these tests verify the diagnostics.

### Coding Style

- `cargo fmt` before committing (nightly rustfmt for some options).
- `cargo clippy` clean with `-D warnings` — no warnings allowed.
- All public items must have `///` documentation comments.
- Prefer `Result` types over panicking.
- See [DEVELOPMENT.md](DEVELOPMENT.md) for more.

## CI Pipeline

PRs run these checks (all must pass):

| Job | What it does |
|-----|-------------|
| **Rustfmt** | `cargo fmt --all -- --check` (nightly) — includes the helper sub-crate |
| **Clippy** | `cargo clippy` on Linux, Windows, macOS — includes the helper binary |
| **Build** | Release build on all three platforms |
| **Test** | `cargo test --workspace` + helper tests on all three platforms |
| **Conformance** | Full OpenJD conformance suite (1,038 tests) on all three platforms |
| **MSRV** | `cargo check --workspace` with Rust 1.94.1 |
| **Documentation** | `cargo doc --no-deps --workspace` with `-D warnings` |
| **Compliance** | Copyright header check |
| **Cross-User (Linux)** | Docker-based cross-user tests: localuser and LDAP variants |
| **Cross-User (Windows)** | Windows cross-user and permissions tests with a temporary test user |
| **openjd-for-js** | Builds the wasm32 crate with `wasm-bindgen` and runs the vitest suite |

If a dependency update bumps `wasm-bindgen`, also bump the matching
`wasm-bindgen-cli` version pinned in the `openjd-for-js` job in
`.github/workflows/ci.yml` — the CLI and the linked Wasm schema version must
match exactly.

Before recommending the user can push, at minimum run:
```bash
cargo clippy --all-features --all-targets --workspace -- -D warnings
cargo test --workspace
```

## Releasing

Releases are automated via [release-plz](https://release-plz.dev/). Every push to `main` runs `.github/workflows/release-plz.yml`, which maintains a single "chore: release" PR. Merging that PR publishes the changed crates to crates.io via OIDC Trusted Publishing.

- Published crates: `openjd-expr`, `openjd-model`, `openjd-sessions`, `openjd-cli`, `openjd-snapshots` — independent versions, conventional-commit-driven bumps.
- Non-published crate: `openjd-for-js` — marked `publish = false`.

See [RELEASING.md](RELEASING.md) for the full process, one-time setup steps, and how to add new crates.

## Running the Conformance Suite

The [openjd-specifications](https://github.com/OpenJobDescription/openjd-specifications) conformance test suite validates CLI behavior against the spec.

```bash
# Build (produces the CLI at target/release/openjd, which is the
# binary name the conformance test runner expects).
# (run from the openjd-rs repo root)
cargo build --release

# Run the full suite (requires openjd-specifications repo checked out nearby)
cd /path/to/openjd-specifications/conformance-tests
PATH="/path/to/openjd-rs/target/release:$PATH" uv run run_openjd_cli_tests.py '2023-09/*'

# Filter to a category
PATH="/path/to/openjd-rs/target/release:$PATH" uv run run_openjd_cli_tests.py '2023-09/EXPR/*'

# Single test
PATH="/path/to/openjd-rs/target/release:$PATH" uv run run_openjd_cli_tests.py '2023-09/EXPR/jobs/expr1.1.3--keyword-attrs-in-exprs.test.yaml'
```

The test runner expects the CLI to have `check` and `run` subcommands with the same interface as the Python `openjd` CLI.

## S3 Integration Tests

The `openjd-snapshots` crate has integration tests that run against a real S3 bucket. These are `#[ignore]`d by default and require environment variables. **When working on snapshots, always run these tests if an S3 bucket is available. If one is not configured, ask the user to provide one.**

| Variable | Required | Description |
|----------|----------|-------------|
| `OPENJD_TEST_S3_BUCKET` | Yes | S3 bucket name (tests skip if unset) |
| `OPENJD_TEST_S3_PREFIX` | No | Key prefix (default: `openjd-snapshots-test`) |
| `AWS_REGION` | No | AWS region (default: `us-west-2`) |

```bash
AWS_PROFILE=your-profile \
OPENJD_TEST_S3_BUCKET=your-test-bucket \
OPENJD_TEST_S3_PREFIX=OpenJDSnapshotsTests \
cargo test -p openjd-snapshots --test integration -- --ignored test_s3_integration::
```

