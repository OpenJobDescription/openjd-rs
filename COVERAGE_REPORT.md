# openjd-rs Code Coverage Report

**Date:** 2026-04-10
**Tool:** cargo-llvm-cov 0.8.5 with rustc 1.94.1 (stable)
**Overall:** 89.9% line coverage (45,431 lines, 4,581 missed)

**Note:** Coverage now uses `--workspace` to include integration tests
across all crates. The previous report undercounted by excluding
integration tests (e.g., the CLI's 135 integration tests were invisible).

## Summary by Crate

| Crate | Lines | Missed | Line % | Status |
|-------|-------|--------|--------|--------|
| openjd-expr | 15,307 | 1,401 | 90.8% | ✅ Good |
| openjd-model | 10,818 | 768 | 92.9% | ✅ Good |
| openjd-sessions | 5,315 | 915 | 82.8% | ⚠️ Needs work |
| openjd-cli | 2,391 | 378 | 84.2% | ✅ Good (was 0% — measurement bug) |
| openjd-snapshots | 11,600 | 1,121 | 90.3% | ✅ Good |

## Changes Since Initial Assessment

| File | Before | After | Delta |
|------|--------|-------|-------|
| `expr_parameters.rs` | 48.1% | **99.1%** | +51.0 |
| `create_job.rs` | 69.8% | **88.7%** | +18.9 |
| **openjd-model overall** | ~82.6% | **89.8%** | +7.2 |
| **Workspace total** | 85.4% | **87.3%** | +1.9 |

Tests added: **158 new tests** across 3 test files.

## Per-File Detail: openjd-model (improved crate)

| File | Lines | Missed | Line % | Notes |
|------|-------|--------|--------|-------|
| `template/expr_parameters.rs` | 341 | 3 | **99.1%** | 3 unreachable serde branches |
| `create_job.rs` | 888 | 100 | **88.7%** | Defensive error paths remain |
| `validate/format_strings.rs` | 486 | 19 | 96.1% | |
| `validate/structure.rs` | 590 | 23 | 96.1% | |
| `step_param_space.rs` | 492 | 78 | 84.2% | Lazy iteration paths |
| `template/parameters.rs` | 712 | 171 | 76.0% | Largest remaining gap |
| `parse.rs` | 229 | 3 | 98.7% | |
| `types.rs` | 279 | 24 | 91.4% | |
| `error.rs` | 154 | 14 | 90.9% | |
| `template/task_parameters.rs` | 79 | 7 | 91.1% | |
| `template/constrained_strings.rs` | 64 | 9 | 85.9% | |
| `template/actions.rs` | 18 | 18 | 0.0% | Syntax sugar only |
| Other files | — | — | 89-100% | |

## Per-File Detail: openjd-expr

| File | Lines | Missed | Line % | Notes |
|------|-------|--------|--------|-------|
| `eval/evaluator.rs` | 922 | 72 | 92.2% | Core evaluator |
| `eval/parse.rs` | 323 | 16 | 95.1% | |
| `value.rs` | 480 | 42 | 91.3% | |
| `types.rs` | 449 | 7 | 98.4% | |
| `function_library.rs` | 420 | 34 | 91.9% | |
| `format_string.rs` | 347 | 56 | 83.9% | Error formatting paths |
| `range_expr.rs` | 310 | 32 | 89.7% | |
| `default_library.rs` | 363 | 0 | 100% | |
| `functions/misc.rs` | 154 | 53 | 65.6% | Lowest in crate |
| `functions/path.rs` | 216 | 3 | 98.6% | |
| `functions/string.rs` | 183 | 25 | 86.3% | |
| `functions/arithmetic.rs` | 226 | 39 | 82.7% | |
| Other files | — | — | 83-100% | |

## Per-File Detail: openjd-sessions

| File | Lines | Missed | Line % | Notes |
|------|-------|--------|--------|-------|
| `action_filter.rs` | 824 | 13 | **98.4%** | Well tested |
| `session.rs` | 724 | 139 | 80.8% | |
| `subprocess.rs` | 394 | 284 | **27.9%** | Highest risk gap |
| `runner/mod.rs` | 56 | 30 | 46.4% | |
| `embedded_files.rs` | 172 | 47 | 72.7% | |
| `runner/env_script.rs` | 156 | 24 | 84.6% | |
| `runner/step_script.rs` | 78 | 12 | 84.6% | |
| `tempdir.rs` | 70 | 21 | 70.0% | |
| `session_user.rs` | 30 | 30 | 0.0% | Linux capabilities |
| `logging.rs` | 14 | 3 | 78.6% | |

## Per-File Detail: openjd-snapshots

| File | Lines | Missed | Line % | Notes |
|------|-------|--------|--------|-------|
| `ops/download.rs` | 712 | 66 | 90.7% | |
| `ops/hash_upload.rs` | 566 | 84 | 85.2% | |
| `ops/collect.rs` | 525 | 35 | 93.3% | |
| `codec.rs` | 567 | 80 | 85.9% | |
| `data_cache.rs` | 357 | 64 | 82.1% | |
| `manifest.rs` | 371 | 44 | 88.1% | |
| `ops/subtree.rs` | 372 | 19 | 94.9% | |
| `ops/partition.rs` | 330 | 23 | 93.0% | |
| `ops/diff.rs` | 313 | 3 | **99.0%** | |
| `ops/compose.rs` | 321 | 13 | 96.0% | |
| `ops/hash_op.rs` | 360 | 4 | **98.9%** | |
| `ops/filter.rs` | 152 | 3 | 98.0% | |
| `ops/memory_pool.rs` | 217 | 4 | 98.2% | |
| Other files | — | — | 84-100% | |

## Remaining High-Priority Gaps

### 1. openjd-cli (0% — no tests)
The CLI binary has zero test coverage. It's tested externally by the conformance
suite but not by `cargo test`. Adding `assert_cmd` integration tests would be the
highest-impact improvement.

### 2. openjd-sessions `subprocess.rs` (28%)
The process execution engine. Most of the uncovered code is the actual subprocess
spawning, I/O handling, and signal management — hard to unit test but critical for
correctness.

### 3. openjd-sessions `session_user.rs` (0%)
Linux-specific user switching. Requires root/CAP_SETUID
to test, so this is typically integration-tested in CI environments.

### 4. openjd-model `parameters.rs` (76%)
The base parameter types (STRING, INT, FLOAT, PATH). The `check_constraints` and
`validate_definition` methods for these types have similar coverage gaps to what
`expr_parameters.rs` had before our work. This would be the next natural target.

### 5. openjd-expr `functions/misc.rs` (66%)
Miscellaneous functions (repr, type conversion). Nearly half the functions are
untested.

## What We Improved

Over this session we added 158 tests across 3 files:

- **`test_expr_param_constraints.rs`**: +102 tests (Phase 1)
  - Exhaustive `check_value_constraints` coverage for all 8 EXPR parameter types
  - Every accepted value tested, boundary rejections for every constraint

- **`test_expr_parameters.rs`**: +56 tests (Phase 2)
  - All `validate_definition` error branches for default-vs-constraint validation
  - Boundary testing: at-boundary passes, one-past-boundary fails

- **`test_create_job.rs`**: +40 tests (Phases 1-3) + **`test_merge_job_parameters.rs`**: +14 tests
  - Env-env merge conflicts (type, objectType, dataFlow)
  - Type coercion (Int↔Float, Bool, RangeExpr, JSON list parsing)
  - Merged constraint validation (FLOAT range, STRING allowedValues, INT min/max)
  - Expression-based range resolution (INT/FLOAT/STRING/PATH)
  - Host requirement attribute resolution
  - `evaluate_let_bindings` standalone function
  - `build_symbol_table` LIST[PATH] handling
  - PATH default edge cases

## Coverage Infrastructure

- Fixed `scripts/coverage.sh` to use `cargo +stable` (TODO: `rust-toolchain.toml`)
- Coverage can be run with: `./scripts/coverage.sh` (summary) or `./scripts/coverage.sh --html` (detailed)
