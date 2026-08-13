---
name: eval-crate
description: Perform a quality evaluation of an openjd-rs crate by reviewing its specs, implementation, and tests for alignment, completeness, and correctness. Use when running `/eval-crate CRATE_NAME` where crate-name is one of expr, model, sessions, cli, or snapshots.
tags: [openjd, rust, quality, evaluation, crate]
---

# Eval Crate

## Overview
Evaluate an openjd-rs crate's specifications, implementation, and tests for alignment, completeness, correctness, and Rust best practices. Produces a detailed quality report with actionable recommendations.

## Usage
Use this skill when:
- Evaluating the quality of a specific openjd-rs crate (e.g. `/eval-crate expr`)
- The user provides a crate name as the first argument

Valid crate names: `expr`, `model`, `sessions`, `cli`, `snapshots`

## Prerequisites

### Repository Layout

This skill assumes the `openjd-rs` repo and the Python reference implementation (`openjd-model-for-python`) are checked out side by side in the same parent directory. The skill resolves paths relative to the `openjd-rs` repo root.

```
<parent>/
├── openjd-rs/                  # This repo
├── openjd-model-for-python/    # Python reference (see branch table below)
├── openjd-specifications/      # Canonical specs (mainline branch)
└── deadline-cloud/             # For snapshots Python reference
```

### Python Reference Branch

The Python repo MUST be on the correct branch for the crate being evaluated:

| Crate | Repo | GitHub org/user | Branch |
|-------|------|-----------------|--------|
| expr | openjd-model-for-python | OpenJobDescription | `mainline` |
| model | openjd-model-for-python | OpenJobDescription | `mainline` |
| sessions | openjd-sessions-for-python | OpenJobDescription | `mainline` |
| cli | openjd-cli | OpenJobDescription | `mainline` |
| snapshots | deadline-cloud | mwiebe (fork) | `manifest-format-2-prototype` |

`snapshots` is beta: evaluated on request only, and excluded from the weekly workflow.

### Specification References

The `openjd-specifications` repo (mainline) contains the canonical specifications:

| Crate | Primary References |
|-------|--------------------|
| expr | `wiki/2026-02-Expression-Language.md` |
| model | `wiki/2023-09-Template-Schemas.md` |
| sessions | `wiki/2023-09-Template-Schemas.md`, `wiki/How-Jobs-Are-Run.md` |
| cli | — |
| snapshots | (in deadline-cloud) `docs/design/job_attachments_snapshots.md` |

## Core Concepts

### Three Artifacts

Every crate has three complementary artifacts that MUST be reviewed together:

1. **Specifications** — in `specs/<crate-name>/`. Describe goals, design decisions, and how the implementation achieves them. MUST include a dedicated public API spec (e.g. `public-api.md`) separate from internal design details.
2. **Implementation source** — in `crates/openjd-<crate-name>/src/`. The Rust code.
3. **Tests** — in-crate unit tests plus `crates/openjd-<crate-name>/tests/`.

### Alignment Criteria

The evaluation checks that all three artifacts are aligned:

1. **Specs ↔ Implementation**: Specs MUST accurately and completely describe what the code does, including goals and design rationale. Every significant part of the implementation SHOULD be covered at an appropriate level of abstraction.
2. **Public API**: Specs MUST include a full and accurate description of the crate's public API. The implementation MUST implement precisely and only the public API specified. The API SHOULD be ergonomic.
3. **Implementation ↔ Specs**: Code MUST faithfully implement what the specs say. Error messages MUST be high quality. Naming MUST be consistent within the crate and across other openjd crates.
4. **Tests ↔ Specs**: Tests MUST confirm the implementation does what the specs say. Tests SHOULD cover both happy path and edge cases, and be clearly organized for review.
5. **Specification compliance**: For `expr`, `model`, and `sessions`, the implementation MUST comply with the formal specifications and conformance tests in `openjd-specifications` (see Specification References table). Review the relevant specification documents and verify the implementation conforms.
6. **Rust best practices**: Follow Rust idioms where it makes sense. No `O(N²)` algorithms when `O(N)` is possible. Balance performance with readability.
7. **Python comparison**: Where the crate has a Python equivalent (see branch table above), compare both implementation algorithms and tests. Note meaningful divergences in behavior, error messages, and API design. Check whether there are Rust tests covering every Python test case.

### Claim Verification

No claim reaches the report on the strength of having been generated. Required
evidence, by kind of claim:

| Claim | Evidence |
|-------|----------|
| "the spec says X" | file + section in `specs/` or `openjd-specifications` |
| "the code does Y" | `file:line` |
| "behaviour is Z" | an executed command and its output |
| "this is a bug" | a test that fails on current code |
| "this is untested" | a mutation the suite fails to catch |
| "Python diverges" | the same input run through both, both outputs |

The last three MUST be executed rather than reasoned. Dead code MUST be shown
unreachable before being called dead.

### Verification with Subagents

Findings MUST be verified in a pass separate from the one that generated them.

1. Subagents MUST get disjoint slices and different angles — spec fidelity,
   implementation correctness, test adequacy by mutation, Python divergence.
2. Each MUST be told it is one of several and will not see the others' results.
3. Each finding returns `CONFIRMED` (with the repro and its output), `PLAUSIBLE`,
   or `WITHDRAWN` (with the reason).
4. Only `CONFIRMED` claims are stated as fact. `PLAUSIBLE` MUST be labelled.
   `WITHDRAWN` are removed and counted in the run summary.
5. Claims that would change a maintainer's decision MUST be verified by
   execution, not by another subagent round.
6. Mutation subagents MUST run **one at a time**. They share one working tree and
   one `target/`, so concurrent mutations compile against each other and "the
   suite did not fail" means nothing. The other angles may run in parallel.
7. Each MUST restore the tree before finishing: `git checkout -- <path>` for an
   edited file, `rm` for one it created, then `git status --short` to confirm.
   Re-run the suite after restoring to be sure the result was not a stale build.

### Evaluation Procedure

1. **Clean slate**: Do not read the previous `reports/<crate-name>-quality-evaluation-report.md` or `-eval-summary.json`; write both with `Write`, which truncates. Overwrite rather than delete first, so an interrupted run leaves the previous report in place instead of nothing.
2. **Read and understand the specs** in `specs/<crate-name>/`.
3. **Read and understand the implementation** in `crates/openjd-<crate-name>/src/`.
4. **Read and understand the tests** in the crate source and `crates/openjd-<crate-name>/tests/`.
5. **Compare with the Python reference** (see Python Reference Branch table) where applicable.
6. **Build and test**: Run `cargo build -p openjd-<crate-name>` and `cargo test -p openjd-<crate-name>`. Confirm clean compilation (no errors or warnings) and all tests pass.
7. **Exploratory testing**: Actively try to find bugs. Look for edge cases, boundary conditions, and unusual inputs that might cause undefined behavior, crashes, or logic errors. Write failing tests that demonstrate any issues found. Include these in the report.
8. **Verify every finding** per "Claim Verification" and "Verification with Subagents". Drop the withdrawn and label the plausible before writing.
9. **Write the report** to `reports/<crate-name>-quality-evaluation-report.md`.
10. **Write the run summary** to `reports/<crate-name>-eval-summary.json`:

    ```json
    {
      "crate": "expr", "date": "YYYY-MM-DD", "commit": "<sha evaluated>",
      "findings": { "high": 0, "medium": 0, "low": 0 }, "withdrawn": 0,
      "build_clean": true, "tests_pass": true, "sections_incomplete": [],
      "headline": "One sentence, used as the commit message body."
    }
    ```

    Counts MUST match the report body. `sections_incomplete` names any section cut
    short, so a truncated run is not mistaken for a clean one.

### Report Structure

The report MUST include:

```markdown
# openjd-<crate-name> Crate Quality Evaluation Report

**Date:** YYYY-MM-DD
**Crate:** `openjd-<crate-name>`

## Executive Summary
Overall assessment in one paragraph.

## 1. Specifications Review
Itemized review of each spec document: coverage, accuracy, gaps.

## 2. Public API Review
Completeness and accuracy of the public API spec. API ergonomics. Match between spec and implementation.

## 3. Implementation Review
Itemized review of source files: correctness, ergonomics, naming, performance.

## 4. Test Review
Coverage assessment, organization, happy path vs edge cases.

## 5. Python Comparison
Behavioral differences, error message comparison, API design divergences.

## 6. Build and Test Results
Compilation output, test results, any warnings.

## 7. Exploratory Findings
Bugs found, failing tests written, undefined behavior discovered.

## 8. Verification
How findings were verified: the subagent slices and angles used, and per finding
its label (CONFIRMED with the repro / PLAUSIBLE / WITHDRAWN with the reason).
State the withdrawn count and what could not be verified, and why.

## 9. Recommendations
Prioritized list of improvements.
```

Every finding in sections 1–7 MUST be tagged `[CONFIRMED]` or `[PLAUSIBLE]`.
An untagged finding is a defect in the report.

## Quick Reference

| Input | Specs | Source | Tests | Report |
|-------|-------|--------|-------|--------|
| `expr` | `specs/expr/` | `crates/openjd-expr/src/` | `crates/openjd-expr/tests/` | `reports/expr-quality-evaluation-report.md` |
| `model` | `specs/model/` | `crates/openjd-model/src/` | `crates/openjd-model/tests/` | `reports/model-quality-evaluation-report.md` |
| `sessions` | `specs/sessions/` | `crates/openjd-sessions/src/` | `crates/openjd-sessions/tests/` | `reports/sessions-quality-evaluation-report.md` |
| `cli` | `specs/cli/` | `crates/openjd-cli/src/` | `crates/openjd-cli/tests/` | `reports/cli-quality-evaluation-report.md` |
| `snapshots` | `specs/snapshots/` | `crates/openjd-snapshots/src/` | `crates/openjd-snapshots/tests/` | `reports/snapshots-quality-evaluation-report.md` |
