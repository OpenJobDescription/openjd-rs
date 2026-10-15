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

`snapshots` is a manual run only. Its reference is a personal fork of a draft
manifest format, so `.github/workflows/eval_crate.yml` does not offer it on any
trigger — automating a personal-fork checkout into a job that holds Bedrock
credentials is not a trade worth making. Run the skill by hand for that crate
until the v2025 reference lands in the org.

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

### Quality Criteria

The criteria, evidence rules, finding labels, severity scale, and verification
rules this evaluation applies are defined in `docs/dev/quality-criteria.md`
([link](../../docs/dev/quality-criteria.md)). Read that file before starting. It is
shared with the `implement-change` skill, so criteria are changed there and
never restated here.

This skill applies the criteria with these settings:

- **Scope**: the whole crate (its specs, source, and tests), plus anything the
  crate's concepts reach under Q8.
- **Criteria**: all of Q1–Q8. For Q9, check only what step 6 below runs
  (build, test, and warnings).
- **Verification** (quality-criteria §6): subagents are REQUIRED. Use all
  four angles, give each a disjoint slice, and run mutation subagents one at
  a time. Every subagent runs the "Verifier capability" probe first. If the
  probe fails, either for this run's own agent before step 6 or for any
  verifier, stop. Write the report with the `ENVIRONMENT FAILURE` diagnosis as
  its Executive Summary, list every unfinished section in the summary's
  `sections_incomplete`, and do not present unverified findings as
  `CONFIRMED`. Follow "Mutation hygiene" exactly. In particular, failing tests
  written in step 7 are uncommitted work, so `git checkout --` must not be
  used to undo a mutation of a file that holds them.
- **Output**: each finding is tagged with its label, severity, and criterion
  ID, for example `[CONFIRMED · medium · Q4]`.

### Evaluation Procedure

1. **Clean slate**: Do not read a previous report or summary. Both of this run's files carry today's date in the name, so `Write` creates them and never has to overwrite something you would first have to read. Last week's conclusions must not anchor this week's.
2. **Read and understand the specs** in `specs/<crate-name>/`.
3. **Read and understand the implementation** in `crates/openjd-<crate-name>/src/`.
4. **Read and understand the tests** in the crate source and `crates/openjd-<crate-name>/tests/`.
5. **Compare with the Python reference** (see Python Reference Branch table) where applicable.
6. **Build and test**: Run `cargo build -p openjd-<crate-name>` and `cargo test -p openjd-<crate-name>`. Confirm clean compilation (no errors or warnings) and all tests pass.
7. **Exploratory testing**: Actively try to find bugs. Look for edge cases, boundary conditions, and unusual inputs that might cause undefined behavior, crashes, or logic errors. Write failing tests that demonstrate any issues found. Include these in the report.
8. **Verify every finding** per quality-criteria §3 (Evidence) and §6 (Independent verification). Drop the withdrawn findings and label the plausible ones before writing.
9. **Write the report** to `reports/<crate-name>-quality-evaluation-report-<YYYY-MM-DD>.md`.
10. **Write the run summary** to `reports/<crate-name>-eval-summary-<YYYY-MM-DD>.json`:

    ```json
    {
      "crate": "expr", "date": "YYYY-MM-DD", "commit": "<sha evaluated>",
      "findings": {
        "confirmed": { "high": 0, "medium": 0, "low": 0 },
        "plausible": { "high": 0, "medium": 0, "low": 0 }
      },
      "withdrawn": 0,
      "build_clean": true, "tests_pass": true, "sections_incomplete": [],
      "headline": "One sentence, used as the commit message body."
    }
    ```

    Counts MUST match the report body, and MUST keep the `CONFIRMED`/`PLAUSIBLE`
    split from step 8 — a flat total reads as though everything were verified.
    `sections_incomplete` names any section cut short, so a truncated run is not
    mistaken for a clean one.

    `date` MUST match the date in both filenames. Under the workflow you are
    given the date and have no `date` command, so use the one you were given
    verbatim. Run by hand, take it from `date -u +%Y-%m-%d`. Either way it is
    observed, never guessed.

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

Every finding in sections 1–7 MUST be tagged with its label, severity, and
criterion, for example `[CONFIRMED · high · Q3]` or `[PLAUSIBLE · low · Q1]`
(see quality-criteria §2, §4, §5). An untagged finding is a defect in the
report.

## Quick Reference

`<date>` below is the run date, `YYYY-MM-DD`. Writing under it is what keeps
`Write` creating rather than overwriting — see step 1.

| Input | Specs | Source | Tests | Report |
|-------|-------|--------|-------|--------|
| `expr` | `specs/expr/` | `crates/openjd-expr/src/` | `crates/openjd-expr/tests/` | `reports/expr-quality-evaluation-report-<date>.md` |
| `model` | `specs/model/` | `crates/openjd-model/src/` | `crates/openjd-model/tests/` | `reports/model-quality-evaluation-report-<date>.md` |
| `sessions` | `specs/sessions/` | `crates/openjd-sessions/src/` | `crates/openjd-sessions/tests/` | `reports/sessions-quality-evaluation-report-<date>.md` |
| `cli` | `specs/cli/` | `crates/openjd-cli/src/` | `crates/openjd-cli/tests/` | `reports/cli-quality-evaluation-report-<date>.md` |
| `snapshots` | `specs/snapshots/` | `crates/openjd-snapshots/src/` | `crates/openjd-snapshots/tests/` | `reports/snapshots-quality-evaluation-report-<date>.md` |
