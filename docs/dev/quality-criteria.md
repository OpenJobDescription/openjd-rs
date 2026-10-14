# openjd-rs Quality Criteria

The single source of truth for what "good" means in this repository and for
what counts as evidence that a piece of work meets that bar. Two skills apply
it:

| Skill | Applies the criteria to |
|-------|-------------------------|
| [`eval-crate`](../../skills/eval-crate/SKILL.md) | A whole crate, to produce a quality report |
| [`implement-change`](../../skills/implement-change/SKILL.md) | One change and its blast radius, in the audit step of its audit/fix loop |

Change a criterion here, not in either skill. The skills define *scope* and
*procedure*. This file defines *criteria*, *evidence*, *labels*, and
*severity*. A skill MAY narrow which criteria it applies. It MUST NOT redefine
any of them.

MUST, SHOULD, and MAY are used in the RFC 2119 sense.

## 1. The three artifacts

Every crate has three artifacts that MUST stay aligned, and every criterion
below is about the relationship between them:

1. **Specs** in `specs/<crate>/`. These describe goals, design decisions and
   rationale, and how the implementation achieves them. Each crate MUST have a
   `public-api.md` that describes its public API separately from internal
   design.
2. **Implementation** in `crates/openjd-<crate>/src/`.
3. **Tests**, both in-crate and under `crates/openjd-<crate>/tests/`.

## 2. Criteria

Each criterion has a stable ID. Findings MUST cite the ID of the criterion
they fall under, so that reports and audits can be compared over time.

**Q1. Specs describe the code.** Specs MUST accurately and completely describe
what the code does, including goals and design rationale. Every significant
part of the implementation SHOULD be covered at an appropriate level of
abstraction. A spec that describes behavior the code no longer has is a
defect, even when the code is right.

**Q2. Public API.** `public-api.md` MUST fully and accurately describe the
crate's public API: every public type, function, trait, and constant, with its
signature. The implementation MUST expose precisely that API and nothing more.
Every public item MUST have a `///` doc comment. The API SHOULD be ergonomic
and consistent with the other openjd crates.

**Q3. Code implements the specs.** Code MUST faithfully implement what the
specs say. Error messages MUST be high quality: specific, actionable, located
(a path or a caret), and consistent with the Python reference where one exists.
Naming MUST be consistent within the crate and across the openjd crates.

**Q4. Tests confirm the specs.** Tests MUST confirm that the implementation
does what the specs say. They SHOULD cover both the happy path and edge cases,
and be organized so a reviewer can find the test for a given spec statement.
Tests for validation or evaluation failures MUST assert the full error message,
as described in [AGENTS.md § Test Quality Standard](../../AGENTS.md#test-quality-standard).
A behavior counts as tested only if breaking it makes a test fail (see §3).

**Q5. Conformance to the OpenJD specifications.** For `expr`, `model`, and
`sessions`, behavior MUST comply with the formal specifications and
conformance tests in `openjd-specifications`. The relevant documents are listed
in the `eval-crate` skill's Specification References table.

**Q6. Rust practice and performance.** Code SHOULD follow Rust idioms where they
make sense, and MUST prefer `Result` to panicking on input-dependent failures.
Algorithms MUST NOT be `O(N²)` when `O(N)` is readily available. Performance is
balanced against readability, not maximized.

**Q7. Python reference parity.** Where a crate has a Python equivalent (listed
in the `eval-crate` skill's Python Reference Branch table), divergences in
behavior, error messages, and API design MUST be either removed or documented
in the specs as intentional. Every Python test case SHOULD have a Rust
counterpart.

**Q8. Consistency beyond the crate.** A change to a concept MUST carry through
everywhere the concept appears: other crates' specs and code, `specs/*.md`
cross-cutting documents, `README.md`, `AGENTS.md`, `DEVELOPMENT.md`,
`docs/dev/`, and CLI help text. Per-crate scans miss this kind of drift, so it
gets its own criterion.

**Q9. Repository conventions.** Work MUST pass every check that CI runs on it.
`.github/workflows/ci.yml` is authoritative for the exact commands, and the
CI Pipeline table in `AGENTS.md` summarizes them. They include formatting,
clippy with `-D warnings`, build, tests, conformance, docs with
`-D warnings`, `cargo deny`, MSRV, the copyright-header check, cross-user
tests, and the `openjd-for-js` build. Each of these runs on both the workspace and the
helper sub-crate at `crates/openjd-sessions/src/helper/` where CI does.
New source files covered by `scripts/check_copyright_headers.sh` MUST carry
the copyright header. Work that resolves a report item MUST strike that item
through in the same change, as described in `AGENTS.md`.

## 3. Evidence

No claim is accepted on the strength of having been generated. Each kind of
claim needs a specific kind of evidence:

| Claim | Evidence |
|-------|----------|
| "the spec says X" | file and section, in `specs/` or `openjd-specifications` |
| "the code does Y" | `file:line` |
| "behavior is Z" | an executed command and its output |
| "this is a bug" | a test that fails on the code in question |
| "this is untested" | a mutation that the test suite fails to catch |
| "Python diverges" | the same input run through both implementations, with both outputs |
| "this is dead code" | a demonstration that it is unreachable |
| "this is fixed" | the test that failed before the fix, now passing |

The bug, untested, Python-divergence, dead-code, and fixed claims MUST be
established by execution, not by reasoning.

## 4. Finding labels

Every finding carries exactly one label:

- **`CONFIRMED`** means the finding has the evidence §3 requires for its claim,
  and that evidence is attached (the repro and its real output).
- **`PLAUSIBLE`** means the finding is supported by reading but not yet by
  the required evidence. It MUST be presented as unverified.
- **`WITHDRAWN`** means verification refuted the finding. It is removed from
  the output and counted, along with the reason.

Only `CONFIRMED` findings are stated as fact.

## 5. Severity

Every finding carries one severity. Severity describes the finding's
consequence, not the effort needed to fix it.

| Severity | Meaning | Examples |
|----------|---------|----------|
| **high** | Wrong behavior, or a statement people will act on that is false. | Incorrect results, panics on reachable input, conformance violations, security issues, `public-api.md` not matching the code, a spec that contradicts the code in an area other work relies on |
| **medium** | Correct today, but quality has degraded in a way that will cause defects or cost later. | Specified behavior with no test that would catch its breakage, misleading or unlocated error messages, inconsistent public naming, undocumented Python divergences, avoidable super-linear algorithms on realistic inputs, missing `///` docs |
| **low** | Polish. | Wording, internal naming, test organization, minor duplication, idiomatic rewrites with no behavioral effect |

A report's prioritized recommendations MAY reorder findings by value or
dependency, but MUST NOT change their severity.

## 6. Independent verification

Findings MUST be verified in a pass separate from the one that generated them.
The verifier MUST NOT be given the generator's reasoning, only the claims.

### Verifier capability

Several kinds of claim in §3 can only be established by execution, so a
verifier that cannot execute cannot do its job. It may still read code, but a
read-only verdict is not a verification. Every verifier MUST be able to write
scratch files, compile them, run them, and read their output. It SHOULD use
that ability freely: exploratory probe tests are the normal way to check a
claim, not a last resort.

Before doing anything else, each verifier runs a capability probe:

1. Write a scratch integration test at
   `crates/<crate>/tests/audit_probe_<id>.rs`. `<crate>` is the crate under
   review, or `openjd-expr` when no crate is involved, and `<id>` is unique to
   this verifier. The test calls one public item of the crate and asserts on
   the result.
2. Run `cargo test -p <crate> --test audit_probe_<id>`, and confirm that the
   output reports the test as run and passed (`1 passed`).
3. Delete the file, and confirm that `git status --short` matches the state
   before step 1.

If any step fails because of the environment rather than a mistake in the
probe, the verifier MUST stop and return the verdict **`ENVIRONMENT FAILURE`**
instead of findings. Environment causes include a missing `cargo` or `rustc`,
a missing toolchain or target, no write access to the tree, a tool allowlist
that denies writing files or running `cargo`, and dependency downloads that
are blocked. The verdict MUST name the step that failed, the command, its
output, and what needs to change in the environment. The verifier MUST NOT
fall back to a read-only review.

Scratch files from the probe and from later exploratory probes are named
`audit_probe_*`. They live under a crate's `tests/` directory or outside
`crates/` (such as a temp directory), and are removed before the verifier
returns. Report findings by quoting a probe's source and output, not by
leaving the file behind.

### Verifier rules
1. When several verifiers run, they MUST get disjoint slices and different
   angles, chosen from: spec fidelity (Q1, Q2, Q8), implementation correctness
   (Q3, Q5, Q6), test adequacy by mutation (Q4), and Python divergence (Q7).
2. Each verifier MUST be told that it is one of several and will not see the
   others' results, so that it checks rather than defers.
3. Each finding comes back labelled as in §4.
4. A finding that would change a maintainer's decision MUST be settled by
   execution, not by another round of review.

### Mutation hygiene

Mutation checks share one working tree and one `target/` directory, so they
are easy to corrupt:

- Run mutation checks **one at a time**. Concurrent mutations compile against
  each other, which makes "the suite did not fail" meaningless. Other angles
  MAY run in parallel.
- Before the first mutation, record the tree's starting state: the output of
  `git status --short`, and a saved copy of `git diff` (for example
  `git diff --output=/tmp/start.diff`).
- Undo each mutation before the next one by restoring the file's
  *pre-mutation* content, and delete any file the mutation created, meaning
  any path that was not in the recorded `git status --short`. Then confirm
  that `git status --short` matches the starting state and that `git diff`
  matches the saved copy (`diff`). `git status` alone cannot show a mutation
  left in a file that already had uncommitted edits. Neither command covers
  the content of an untracked (`??`) file, so compare each mutated untracked
  file with its aside copy as well. A leftover file under `crates/` compiles
  into the next run.
- `git checkout -- <path>` restores the content git has recorded for the file,
  so it is a correct undo only for a tracked file that `git status --short`
  showed as unmodified before the mutation. Otherwise it discards uncommitted
  work, such as an uncommitted change under audit or a failing test written
  earlier in the same run, and for an untracked file it fails. For any other file, copy
  it aside to a location outside `crates/` (such as a temp directory) before
  mutating it, and copy it back afterwards. A copy left under `crates/` can
  compile as an extra test target.
- Never use `git clean` or `git stash`. Uncommitted deliverables (a report
  being written, probe tests, an uncommitted change) live in the working tree, and both
  commands remove them from it.
