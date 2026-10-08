# AGENTS.md

## Project Overview

`python/openjd-model` (formerly the openjd-model-for-python repository) is the Python distribution of [Open Job Description](https://github.com/OpenJobDescription)'s data model, the expression language used inside templates, and the session runtime types those models drive. It ships a single PyPI package (`openjd-model`) with three import roots — `openjd.expr`, `openjd.model`, and (consumed from `python/openjd-sessions`) `openjd.sessions` — backed by a single PyO3 extension module, `openjd._openjd_rs`.

The canonical specification lives in [openjd-specifications](https://github.com/OpenJobDescription/openjd-specifications). The Rust crates it wraps, `openjd-expr`, `openjd-model`, and `openjd-sessions`, are in the same repository under `crates/`, and the bindings depend on them by path, so a change to a crate and the bindings that expose it land together. See "Changing the Rust crates and the bindings together" below.

This package is currently in transition from a pure-Python implementation to Rust-backed bindings:

- `openjd.model` (and its alias `openjd.model.v0`) is the **pure-Python reference** built on Pydantic. It is what consumers import today.
- `openjd.model._v1` is the **Rust-backed v1** that re-exports symbols from `_openjd_rs`. It will eventually become the default `openjd.model`.
- `openjd.expr` is **Rust-only** — there is no pure-Python predecessor to compare against, so the Python interface spec and the bindings together are the contract.

See [README.md](README.md) for user-facing documentation and [DEVELOPMENT.md](DEVELOPMENT.md) for general developer setup.

## Quick Reference

```bash
# Run from python/openjd-model. Build the extension into your active
# environment (hatch does this for its own environments).
maturin develop

# Rebuild the .pyi type stub (needs the patched stub-gen tool).
scripts/generate_stubs.sh

hatch run test                          # Full suite — enforces the 94% coverage gate
hatch run test-subset test/openjd/expr  # One subtree — measures coverage but doesn't enforce the gate
hatch run benchmark                     # Performance benchmarks (separate from test/)
hatch run lint                          # ruff + black --check
hatch run fmt                           # black + lint
hatch run typing                        # mypy

# Rust-side checks against the bindings crate (openjd-model-py). The crate has
# no Rust tests; it's tested from Python.
cargo build  -p openjd-model-py --all-targets
cargo clippy -p openjd-model-py --all-targets -- -D warnings
```

Python: **3.9+** (declared via `abi3-py39` in `Cargo.toml`, enforced by `pyproject.toml` `requires-python`).

## Component Map

The PyO3 extension exports one Python module — `openjd._openjd_rs` — that wraps three Rust crates from this workspace's `crates/` directory, each surfaced under its own Python import root:

```
Cargo.toml                             (PyO3 crate, name=openjd-model-py, lib=_openjd_rs)
rust-bindings/
├── src/lib.rs                         #[pymodule] _openjd_rs registration
├── src/expr/      ──→ openjd.expr     wraps openjd-rs::openjd-expr
├── src/model/     ──→ openjd.model._v1 wraps openjd-rs::openjd-model
├── src/sessions/  ──→ openjd.sessions._v1 (re-exported from python/openjd-sessions)
│                                      wraps openjd-rs::openjd-sessions
└── src/bin/stub_gen.rs                pyo3-stub-gen entry point (feature-gated)
```

Changes to `rust-bindings/src/lib.rs` (exception registration, class registration, function registration) affect **every** import root — review them carefully.

The crate name is `openjd-model-py` and the cdylib name is `_openjd_rs`. Every `#[pyclass]` in the bindings has a `Py`-prefixed Rust identifier (e.g. `PyJob`) but is registered under its public Python name via `#[pyo3(name = "...")]` and the `register_renamed_exception` helper in `lib.rs`. Without that helper, `repr`, pickle, and tracebacks leak the `Py`-prefixed names — so when you add a new `create_exception!` exception, you **must** call `register_renamed_exception` for it.

### `expr` — `rust-bindings/src/expr/`

Rust-only. There is no pure-Python predecessor — the Python interface spec and the bindings are the contract. Wraps `openjd-rs::openjd-expr`.

- **Public surface (`mod.rs`)** — re-exports the symbols used by `lib.rs`.
- **Profile types** (`profile.rs`) — `PyExprProfile`, `PyExprRevision`, `PyExprExtension`, `PyHostContext`, `PyFunctionLibrary`. The "profile" plumbing is the canonical input to `evaluate_expression` / `ParsedExpression.evaluate` / `FormatString.resolve*`.
- **Values and types** (`expr_value.rs`, `expr_type.rs`) — `PyExprValue`, `PyExprType`, `PyTypeCode`. Includes `unresolved` and `from_float` constructors.
- **Symbol table** (`symbol_table.rs`) — `PySymbolTable`. Hierarchical key store with dotted-path support.
- **Parsing and evaluation** (`parsed_expression.rs`, `evaluate.rs`) — `parse_expression`, `evaluate_expression`, `ParsedExpression`.
- **Format strings** (`format_string.rs`) — `PyFormatString`, `escape_format_string`, `PyFormatStringValidationError`.
- **Range expressions** (`range_expr.rs`) — `PyRangeExpr`, `PyRangeExprError`.
- **Path mapping** (`path_mapping.rs`, `path_format.rs`) — `PyPathMappingRule`, `PyPathFormat`.
- **Errors** (`errors.rs`) — `PyExpressionError`, `PyExpressionTypeError`.

Spec entry point: `specs/python-expr-interface.md`. Python wrapper: `src/openjd/expr/__init__.py`. Tests: `test/openjd/expr/`. Reference branch for parity: `mwiebe/openjd-model-for-python` `expr`.

### `model` — `rust-bindings/src/model/`

Has both a pure-Python implementation (`openjd.model` / `openjd.model.v0`, built on Pydantic) and a Rust-backed v1 (`openjd.model._v1`). Wraps `openjd-rs::openjd-model`.

- **Decoding** (`decode.rs`) — `decode_job_template_str/dict`, `decode_environment_template_str/dict`.
- **Templates** (`template.rs`) — `PyJobTemplate`, `PyEnvironmentTemplate`.
- **Job creation** (`create_job_fns.rs`) — `py_create_job`, `py_preprocess_job_parameters`, `py_create_environment`, `py_deserialize_step`, `py_evaluate_let_bindings`.
- **Job/Step types** (`job.rs`, `step_param_space.rs`, `step_dependency_graph.rs`) — `PyJob`, `PyStep`, `PyStepParameterSpace`, `PyStepParameterSpaceIterator`, `PyStepDependencyGraph`, `PyStepDependencyNode`, `PyStepDependencyEdge`, plus action and embedded-file types.
- **Profile types** (`profile.rs`) — `PyModelProfile`, `PyModelExtension`, `PySpecificationRevision`, `PyCallerLimits`, `PyValidationContext`. The model uses its own profile that is distinct from the expr profile but built on the same idea.
- **Parameter types** (`types.rs`) — `PyJobParameterType`, `PyTaskParameterType`, `PyJobParameterValue`, `PyTaskParameterValue`, `PyDocumentType`, `PyTemplateSpecificationVersion`.
- **Errors** (`errors.rs`) — `PyDecodeValidationError`, `PyModelValidationError`, `PyUnsupportedSchema`.
- **Merge** — `py_merge_job_parameter_definitions` exposes openjd-rs's parameter merge logic.

Spec entry point: `specs/python-model-interface.md`. Python wrappers: pure-Python under `src/openjd/model/` (and its alias `src/openjd/model/v0/`) and Rust-backed under `src/openjd/model/_v1/`. Tests: `test/openjd/model_v0/` (pure-Python) and `test/openjd/model_v1/` (Rust-backed). Reference branch for parity: `OpenJobDescription/openjd-model-for-python` `mainline`.

### `sessions` — `rust-bindings/src/sessions/`

Sessions binding source lives **here**, but the Python wrapper module lives in `python/openjd-sessions`. Change both in the same PR when the binding API changes. Wraps `openjd-rs::openjd-sessions`.

- **Session lifecycle** (`session.rs`) — `PySession`, `PySessionState`, `PyActionState`, `PyActionStatus`, `PyActionResult`, `PyScriptRunnerState`.
- **Cross-user execution** (`session_user.rs`) — `PyPosixSessionUser`, `PyWindowsSessionUser`, `PyBadCredentialsException`. Platform-specific: PosixSessionUser on Unix, WindowsSessionUser on Windows.
- **Types** (`types.rs`) — Action and session enum/state types exposed to Python.
- **Errors** (`errors.rs`) — `PySessionError`.

Spec entry point: `specs/python-sessions-interface.md`. Python wrapper: `../openjd-sessions/src/openjd/sessions/_v1/`. Tests: `../openjd-sessions/test/openjd/sessions_v0/` and `sessions_v1/`.

When working on `sessions`, edit both packages in the same change set. The wrapper module re-exports symbols from `openjd._openjd_rs`, so a binding rename without a wrapper update will silently break imports.

## Navigating the Codebase

### Spec + code co-evolution

The `specs/` directory holds three Python interface specs — `python-expr-interface.md`, `python-model-interface.md`, `python-sessions-interface.md` — each acting as the public-API contract for one binding component. Specs and code evolve together; within a coding session you might edit the bindings first and then update the spec, or write the spec first and then implement, or iterate on both simultaneously. The order doesn't matter, but **before committing, always confirm the spec and code line up.** If you changed behavior, the spec must reflect it. If you changed the spec, the bindings (and the wrapper module that re-exports from `_openjd_rs`) must match.

Each spec is the public-API document for its component — the equivalent of `public-api.md` in `openjd-rs`. When you add or change a public symbol (class, function, exception, constant), update the spec in the same commit.

The structure:

```
specs/
├── python-expr-interface.md      # public API for openjd.expr
├── python-model-interface.md     # public API for openjd.model._v1
└── python-sessions-interface.md  # public API for openjd.sessions._v1
```

### Three-way alignment

Every binding component has **four** artifacts that must stay aligned:

1. **Python interface spec** — `specs/python-<component>-interface.md`.
2. **PyO3 binding source** — `rust-bindings/src/<component>/` plus the relevant registration block in `rust-bindings/src/lib.rs`.
3. **Python wrapper module** — `src/openjd/<component>/__init__.py` (or `src/openjd/model/_v1/__init__.py` for `model`; `../openjd-sessions/src/openjd/sessions/_v1/__init__.py` for `sessions`).
4. **Tests** — `test/openjd/<component>/` (or `test/openjd/model_v0/` and `test/openjd/model_v1/` for `model`; the corresponding `test/openjd/sessions_v0/` and `sessions_v1/` in `../openjd-sessions` for `sessions`).

A spec change without a wrapper update is invisible to users. A binding rename without a wrapper update breaks imports silently. A new exception class without `register_renamed_exception` shows up in tracebacks under its `Py`-prefixed internal name. A new symbol that is not in the spec is unsupported even if it works. **Treat all four artifacts as one unit when reviewing or modifying a component.**

The fifth artifact — parity with the pure-Python reference (`openjd.model.v0`, `openjd.sessions.v0`) — is owned by the `eval-bindings` skill (see `skills/eval-bindings/SKILL.md`).

### Report-driven development

Many tasks originate from quality evaluation reports in `reports/`, generated by `/eval-bindings <component>`. Each report has a numbered Recommendations section with priority groupings.

**Workflow for report-driven changes:**

1. Read the relevant report in `reports/` to understand the recommendation.
2. Implement the change.
3. In the **same commit**, update the report to mark the item as resolved by striking it through with `~~` and appending `**Resolved.**` or `**Resolved** — <brief note>.`

Example — before:
```markdown
6. **Decompose `decode_job_template`** into per-version helpers.
```

After:
```markdown
6. ~~**Decompose `decode_job_template`** into per-version helpers.~~ **Resolved.**
```

This keeps reports accurate as a living record of what's been done and what remains. If most items in a report are resolved, suggest to the user that they run the `eval-bindings` skill to regenerate a fresh report for that component.

Current reports:

| Report | Component |
|--------|-----------|
| `reports/expr-bindings-quality-evaluation-report.md` | `openjd.expr` |
| `reports/model-bindings-quality-evaluation-report.md` | `openjd.model._v1` |
| `reports/sessions-bindings-quality-evaluation-report.md` | `openjd.sessions._v1` |

## Conventions

### Commit Messages

This repo uses [conventional commit](https://www.conventionalcommits.org/en/v1.0.0/) syntax — required by release-plz, which derives the next version from the commits that touch this directory. All commits must use it.

Types and bump rules are the repository's (see the root `AGENTS.md` and `RELEASING.md`): `feat`, `fix`, `test`, `docs`, `refactor`, `ci`, `chore`, `perf`. Append `!` for breaking changes (e.g., `feat!: ...`) and include a `BREAKING CHANGE` footer. While the major version is `0.x`, minor bumps cover backwards-incompatible changes and patch bumps cover bug fixes and backwards-compatible changes (see README "Versioning").

### Test Quality Standard

When writing tests that check for validation, decoding, or evaluation failures, assert on the **full error message content** — not just that an error occurred. This ensures error messages are stable, human-readable, and match the pure-Python reference where one exists.

**Bindings: assert exception class + message + (where applicable) the field path**

Every `decode_*` / `create_job` / `evaluate_*` failure test must assert the exception class, the message body, and (for model) the field path. The pure-Python reference is on Pydantic, so error messages flow through pydantic's path-prefixed format; the Rust bindings reproduce the same format (`steps[0] -> script -> actions -> onRun -> command:\n\tmust not be empty.`). Tests that just match `pytest.raises(ModelValidationError)` without inspecting the message let regressions through.

**Reference parity**

For every pure-Python reference test under `test/openjd/model_v0/` (and the equivalent in `../openjd-sessions/test/openjd/sessions_v0/`), there should be an equivalent test under `test/openjd/model_v1/` (or `sessions-v1/`) exercising the same behaviour through the binding. New behaviour added on the binding side without a v0 counterpart is a divergence — note it in the relevant `reports/<component>-bindings-quality-evaluation-report.md`.

**Why:** The Python contract for v1 is supposed to be the same as the v0 contract (modulo the documented differences in the spec like Pydantic-vs-no-Pydantic). Catching message regressions and missing reference tests is how we keep that promise.

**Known gaps live in `test_known_gaps.py`**

Each component has a `test/openjd/<component>/test_known_gaps.py` (and the sibling repo has `test/openjd/sessions-v1/test_known_gaps.py`). It holds **only** `pytest.xfail`-marked tests that document a behaviour gap between the binding and the reference. Use it as the landing pad for exploratory findings — including everything generated by the `eval-bindings` skill.

When a gap is resolved, **do not leave the test in `test_known_gaps.py` as a passing regression test**. Move it to its proper home in the sibling test files (e.g. a hashing fix → `test_range_expr.py`, a pickle fix → `test_pickle.py`) so the regression test is co-located with the surface it covers. The file is being driven to zero; an xpass is a signal that something is mis-categorised.

### PyO3 Conventions

Items the `eval-bindings` skill checks; these are the recurring sources of behaviour drift between the binding and the underlying Rust crate. Follow them when writing or modifying `rust-bindings/src/`:

- **Exception registration** — every `create_exception!` exception must be registered with `register_renamed_exception` in `lib.rs`. Without it, `__name__` defaults to the binding-internal `Py`-prefixed identifier and leaks into tracebacks, pickle, and IDE tooltips.
- **Type conversions** — `IntoPyObject`/`FromPyObject` impls must preserve types faithfully. Watch the `int` ↔ `i64`/`u64` boundary, `float` NaN/Inf passthrough, `pathlib.Path` ↔ `Path`/`PathBuf`, and ordered vs unordered collections.
- **GIL handling** — long-running Rust calls (sessions subprocess wait, large template parses) should release the GIL via `Python::allow_threads`. Holding the GIL across blocking I/O blocks every other Python thread.
- **`#[pyclass]` constructor signatures** — Python `__init__` signatures must match what the spec advertises. Keep `#[pyo3(signature = ...)]` aligned with `text_signature` and the spec.
- **ABI3 compatibility** — `Cargo.toml` declares `abi3-py39`. Don't introduce code that uses Python C-API features beyond ABI3.
- **Stub generation** — `scripts/generate_stubs.sh` runs the `stub_gen` binary and produces `src/openjd/_openjd_rs.pyi`. The stub must match the bindings; regenerate it whenever you change a public symbol's signature.

### Coding Style

- **Python:** `hatch run fmt` (black + ruff) before committing. `hatch run typing` (mypy) must pass. See [DEVELOPMENT.md](DEVELOPMENT.md#coding-style-requirements) for the project's keyword-only-arguments and underscore-prefix-private conventions — they apply to the Python wrappers and tests, not to the Rust bindings.
- **Rust:** `cargo fmt` before committing. `cargo clippy` is currently informational in CI (see `.github/workflows/rust_quality.yml`) but **don't add new lints** — clean ones up as you touch surrounding code, and prefer landing PRs that don't add fresh clippy output.
- **Public Python symbols** must have docstrings. `#[pyclass]` and `#[pyfunction]` items should have `///` doc comments — pyo3-stub-gen propagates them into `_openjd_rs.pyi`.
- **Re-exports vs. private names:** when something needs to be reachable from `from openjd.<component> import X` but isn't part of the spec, prefix the wrapper-module re-export with `_` (the wrapper modules already do this, e.g. `_RsSpecificationRevision`).

## CI Pipeline

PRs run these checks (all must pass):

| Workflow | What it does |
|----------|--------------|
| **Python** (`python.yml` at the repo root) | `hatch run lint`, `hatch build`, and `hatch run test` on `{ubuntu, windows, macos} × {3.9 … 3.14}`. hatch installs this package through the maturin build backend, which compiles the extension from the in-repo crates. Also checks `THIRD-PARTY-LICENSES.txt`. Runs for changes under `python/` or to `openjd-expr`/`-model`/`-sessions`. |
| **CI** (`ci.yml` at the repo root) | rustfmt, clippy (`-D warnings`), rustdoc, and cargo-deny over the whole workspace, including this crate. |
| **CodeQL** (`codeql.yml` at the repo root) | GitHub's static analysis for Rust and Python. |

Before recommending the user push, at minimum run:

```bash
hatch run lint
hatch run test
cargo clippy -p openjd-model-py --all-targets -- -D warnings
```

If you've changed a public binding signature, also regenerate the stubs:

```bash
scripts/generate_stubs.sh
```

and commit the resulting `src/openjd/_openjd_rs.pyi`.

## Releasing

Releases are automated by the repository's release-plz workflow; see the "Python packages" section of the root `RELEASING.md`. The package version is the `version` in this directory's `Cargo.toml`, which release-plz bumps on the Release PR from conventional commits that touch this directory, or when one of the Rust crates the bindings depend on is released. maturin reads it from there for the wheel, and `openjd.model.version` reads it back from the installed package's metadata.

## Changing the Rust crates and the bindings together

`Cargo.toml` depends on `openjd-expr`, `openjd-model`, and `openjd-sessions` by path (`../../crates/...`), so the bindings always build against the crates in the same commit. To expose a new crate API to Python, change the crate, the binding source, the wrapper module, the spec, and the tests in one PR. If a crate change alters behavior Python users can see (an error type or message, say), the Python tests here fail in the same PR; update them, and mark the commit as breaking if the Python behavior change is breaking.

There is no `[patch.crates-io]` step and no publish-then-bump sequence: merging the PR and then the Release PR ships both the crates and this package.

## Compliance and Copyright Headers

Every source file (`*.py`, `*.rs`, `*.sh`) must start with the Apache-2.0 copyright header:

```
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
```

(Use `//` for Rust.) `test/openjd/test_copyright_header.py` enforces presence of the `Copyright Amazon.com…` line on every checked file in the test suite (it does not check the SPDX line — but include it on new files to keep the project consistent). `scripts/add_copyright_headers.sh <files…>` adds the header to specific files passed on the command line; it is a per-file utility, not a tree walker.
