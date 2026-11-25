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

**Q2. Public API.** The crate MUST expose precisely the API its specs describe
and nothing more. Two tools enforce the mechanical parts, so review can
concentrate on the prose:

- Each library crate (expr, model, sessions, snapshots) has a
  `specs/<crate>/public-api.txt`, or a `public-api.<os>.txt` where the API
  differs by platform. `scripts/check_public_api.sh --update` generates it,
  and it lists every public item with its signature. CI fails when it does
  not match the code. It is the authority on what is public and on
  signatures.
- The `missing_docs` lint is enabled for every workspace crate through
  `[workspace.lints]`, and it fails CI when a public item has no `///` doc
  comment. Items that predate the lint carry an item-level
  `allow(missing_docs, reason = "missing-docs burn-down ...")` attribute.
  Those attributes are known debt, and new code MUST NOT add one. The lint
  still checks new items, except inside an item that carries the attribute,
  such as a new field of an allowed struct or a new variant of an allowed
  enum. Review MUST check those by hand.

`public-api.md` MUST describe every public module, type, trait, function,
method, constant, and static in the listing: what it is for, how the items fit
together, and their invariants and error behavior. Trait impls need prose only
when they carry behavior a user relies on. Where `public-api.md` shows a
signature, the signature MUST match the listing. Doc comments MUST be accurate.
The API SHOULD be ergonomic and consistent with the other openjd crates.

`public-api.md` SHOULD contain only what a caller needs: purpose, signatures,
invariants, and error behavior. That includes behavior that differs from the
Python reference in a way a caller can observe, stated as behavior. Design
rationale (including why a behavior differs from Python), algorithms,
performance and memory notes, and test or fuzz coverage belong in the crate's
other spec files, linked from `public-api.md`.

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
balanced against readability, not maximized. A change made for performance
follows Q14.

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
`-D warnings`, `cargo deny`, MSRV, the copyright-header check, the public API
listings, cross-user tests, and the `openjd-for-js` build. Each of these runs on both the workspace and the
helper sub-crate at `crates/openjd-sessions/src/helper/` where CI does.
The fuzz crate at `fuzz/` is its own workspace, which the commands above do
not build. Its replay tests, fmt, and clippy MUST pass, as
`.github/workflows/fuzz.yml` runs them, when a change alters library
behavior, public API, dependencies, or `fuzz/` itself. Every public function
of openjd-expr, openjd-model, openjd-sessions, and openjd-snapshots MUST be
executed by the committed fuzz corpus or excluded with a reason in
`fuzz/api_coverage.toml`, which `scripts/check_fuzz_api_coverage.py` checks.
A bug found by fuzzing MUST get a unit test in its crate and its input in
`fuzz/regressions/<target>/`, as described in `AGENTS.md` "Fuzzing".
New source files covered by `scripts/check_copyright_headers.sh` MUST carry
the copyright header. Work that resolves a report item MUST strike that item
through in the same change, as described in `AGENTS.md`.

**Q10. Text describes the current code.** Specs, doc comments, and code
comments, including those in tests, MUST describe the code as it is. They
MUST NOT narrate the change that produced it or the implementation it
replaced, with wording such as "before this change", "previously", "used
to", "no longer", "was changed to", "the old code", or "now X instead of Y".
That history belongs in the commit message and the PR description. In the
tree it means nothing to a reader who never saw the old code. Two kinds of
text remain allowed:

- A regression test MAY state, in the present tense, the failure it guards
  against.
- Rationale MAY explain why the code does not use an alternative design, as
  long as it is stated as a reason for the current code ("X is done this way
  because Y would ...") rather than as a history of it.

The listed words are signals to check, not banned words. "After the new DACL
is applied, the file no longer inherits ACEs" describes what the code does at
run time, and is fine. So is a comparison with how the Python reference
implementation behaves. A document whose stated purpose is a plan or a record
of how work was done, such as a migration plan, is not a description of the
code and is outside Q10.

For example, a spec that says "(Before this change, the runner and each
handle took their own dup of the pipe, and a failed dup left that handle
unable to cancel.)" narrates history. As rationale for the current design, it
reads: "The runner and its handles share one dup of the pipe, because a
separate dup per handle can fail and leave that handle unable to cancel." In
the same way, a test comment "a lost cancel used to leave the action running"
becomes "a cancel must reach the helper even when the dup fails".

A Q10 finding is low severity, or medium when a reader could take the past
behavior it describes for the current behavior.

**Q11. One home per rule.** A semantic rule — a spec constraint, a parse or
coercion rule, a wire format, a classification, a policy constant — SHOULD
have a single implementation that every path routes through. Parallel copies
of a rule (per platform, per entry point, per format version, per execution
mode) drift. When two implementations are genuinely necessary — a crate
boundary such as the sessions helper, wasm32 vs native, a documented
streaming trade-off — the copies MUST cross-reference each other at both
sites and MUST be pinned by an equivalence test that fails when either copy
changes alone. Existing equivalence pins: openjd-expr's
`tests/integration/test_coercion_drift.rs` and the sessions helper framer
test in `crates/openjd-sessions/src/subprocess.rs`. A change that fixes
a rule MUST fix every copy of it in this repository, or route the copies
through one implementation. Copies are easy to miss in the cross-user helper
(`crates/openjd-sessions/src/helper/`), in public `validate_*` functions
beside the internal checks they mirror, and in the fuzz-target oracles in
`fuzz/`, which repeat the library's exemptions. A fix belongs where the code
is on the target branch, including code that a merged refactor has moved. A
copy in a Python reference implementation falls under Q7. Incidentally
similar code that encodes no shared rule is not a Q11 finding.

**Q12. Sibling shapes of a type.** A rule about how a value of one type is
parsed, checked, coerced, typed, rendered, or path-mapped MUST also apply to
the type's sibling shapes, unless the specs say how a shape differs. For a
list, it applies to each element. Tests SHOULD cover each shape. The sibling
shapes are:

- template parameter types `T` and `LIST[T]`, plus `LIST[LIST[INT]]` for
  `INT`;
- expression types `T`, `list[T]`, `list[list[T]]`, the empty
  `list[nulltype]`, `T?`, a union that contains `T` or `list[T]`, and
  `unresolved[T]`, plus `range_expr` for `int`;
- both ends of each implicit coercion the value passes through (2026-02
  Expression Language §1.2.3), such as `path` and `string` or `int` and
  `float`, together with the list form of each coercion, which applies
  element by element;
- each representation of a value: a JSON or YAML number and a numeric string
  (`5.0` and `"5.0"`), and a `FLOAT` whose original text is kept (`2.50`) as
  well as one that is computed and rendered again.

Tests SHOULD include these edge values:

| Type | Edge values |
|------|-------------|
| `FLOAT` | `2.50` (text kept), `-0.0`, `1e999`, `inf`, `nan`, and an integral `5.0` given where an `INT` is expected |
| `INT` | `i64::MIN`, `i64::MAX`, a value above `i64::MAX`, `+5`, `1_000`, `٣` (U+0663) |
| `PATH` | relative, absolute, a URI (`s3://bucket/key`), a trailing separator, a Windows drive path, a UNC path |
| `STRING` | empty, surrounded by white space, non-ASCII |
| Any list | empty, one element, elements of mixed types |

For example, the `PATH` default rules apply to each `LIST[PATH]` element
(#421), and `path([])` matches a `path(list[nulltype])` overload (#576).

**Q13. Every source of a value, at every stage.** A rule about a value MUST
hold wherever the value can come from:

- a literal, a format string embedded in a longer string, a whole-field
  `{{ ... }}` expression, a list element, and a fully static expression;
- a job template default, an environment template default, and a submitted
  value, given as CLI `-p` text, as JSON text, or as a native list through a
  binding;
- a step written with a SimpleAction such as `bash:`, as well as one written
  with a `<StepScript>` (`script:` with `actions`);
- every field the specification links to the rule, including rarely used
  ones such as `notifyPeriodInSeconds`, environment `onEnter` and `onExit`
  timeouts, SimpleAction `timeout`, and host requirement amounts.

It MUST also hold at each stage that applies it: template decode and
`openjd check`, `create_job`, and run time. Each stage reports a violation
with a located message (Q3), and SHOULD use the message and field path that
the other stages use. An earlier stage MUST NOT reject a template that every
later stage accepts, with one exception: a name that resolves nowhere MUST be
reported at template validation, even in a sub-expression that some runs
skip. Whether a name exists does not depend on any value, so every run that
evaluates that sub-expression fails. `specs/expr/values.md` and
`specs/expr/evaluator.md` state both rules for the constructs they cover.

**Q14. Performance changes.** A change made for performance, and every speed
or memory claim such a change makes, MUST meet these rules. Q14 judges
changes; a whole-crate evaluation does not apply it.
`skills/implement-change/SKILL.md` § "Performance guideline" gives the
techniques and tools.

- **Output is unchanged.** Every message, path, caret, count, and order MUST
  be the same as before, for valid and invalid input, under any
  `CallerLimits` and extension set. Passing tests is not enough evidence: the
  claim needs a differential of the old and new code (§3).
- **Claims count work.** A speed claim MUST cite counted work (instructions,
  evaluations, or allocations) measured on both sides. Wall time supports a
  claim only when the two binaries run interleaved, with a null test (the same
  binary at two paths) and a no-op control (a minimal input: `--version` or a
  minimal template for the CLI). A count without a time confirmation is not
  enough either: a change can do less work and still run slower.
- **Peak memory does not rise.** A change MUST NOT raise peak memory unless
  it states the increase and bounds it with a named constant, which a
  counting-allocator test pins. A change that keeps a value alive longer
  (sharing, caching, buffering errors to keep their order) MUST show its peak
  with such a test either way. Error messages can embed evaluated values, so
  they count.
- **Shortcuts take only the proven path.** A shared or cached result is used
  only where it is identical to what the original code computed, such as an
  evaluation that reported nothing. Every other path keeps the original
  sequence.
- **Removed work is the speed-up.** Splitting, moving, or inlining code is
  presumed speed-neutral within a crate in release builds; a claim that it is
  faster needs the counted evidence above like any other. Splitting an
  `async fn` inside a CPU-bound loop adds a state machine per iteration and is
  not neutral.

Q14 does not ask for speed. Q6 still balances performance against
readability; Q14 governs how a performance change is made and proven.

## 3. Evidence

No claim is accepted on the strength of having been generated. Each kind of
claim needs a specific kind of evidence:

| Claim | Evidence |
|-------|----------|
| "the spec says X" | file and section, in `specs/` or `openjd-specifications` |
| "the code does Y" | `file:line` |
| "behavior is Z" | an executed command and its output |
| "this is a bug" | a test that fails on the code in question |
| "this is untested" | a mutant that survives the test suite: a `cargo mutants` result, or a hand mutation made in an isolated copy (see §6 "Mutation testing") |
| "Python diverges" | the same input run through both implementations, with both outputs |
| "this is dead code" | a demonstration that it is unreachable |
| "this is fixed" | the test that failed before the fix, now passing |
| "these two implementations diverge" | the same input run through both paths, with both outputs |
| "this pair is pinned" | the equivalence test, by name |
| "output is unchanged" (Q14) | a differential of the old and new code with the invocation count and `0 differences`: the binaries over the conformance templates and edge cases for what the CLI reaches, and library-level probes for other `CallerLimits` and for crates the CLI does not drive |
| "this is faster" (Q14) | counted work on both sides (for example a callgrind instruction diff), plus interleaved wall time with a null test and a no-op control |
| "memory is not higher" (Q14) | peak heap from a counting allocator on both sides, one process per workload |

The bug, untested, Python-divergence, dead-code, fixed,
two-implementation-divergence, and Q14 claims MUST be established by
execution, not by reasoning.

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
| **high** | Wrong behavior, or a statement people will act on that is false. | Incorrect results, panics on reachable input, conformance violations, security issues, `public-api.md` contradicting the code or the public API listing, a spec that contradicts the code in an area other work relies on, a performance change that alters output or raises peak memory without a stated bound |
| **medium** | Correct today, but quality has degraded in a way that will cause defects or cost later. | Specified behavior with no test that would catch its breakage, misleading or unlocated error messages, inconsistent public naming, undocumented Python divergences, a rule implemented twice without the cross-references and equivalence pin Q11 requires, avoidable super-linear algorithms on realistic inputs, a performance claim without the evidence Q14 requires, a stated memory bound with no test that fails when it is exceeded, missing `///` docs |
| **low** | Polish. | Wording, internal naming, test organization, minor duplication that encodes no rule, idiomatic rewrites with no behavioral effect |

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
   angles, chosen from: spec fidelity (Q1, Q2, Q8, Q10), implementation
   correctness (Q3, Q5, Q6, Q11, Q12, Q13), test adequacy by mutation (Q4),
   and Python divergence (Q7), plus, for a performance change only,
   performance (Q14: parity differential and peak memory).
2. Each verifier MUST be told that it is one of several and will not see the
   others' results, so that it checks rather than defers.
3. Each finding comes back labelled as in §4.
4. A finding that would change a maintainer's decision MUST be settled by
   execution, not by another round of review.

### Mutation testing

A mutation check needs a tree that holds exactly the code under review. The
shared working tree is not one: it may hold uncommitted work, such as a report
being written or failing tests written earlier in the run, and other verifiers
may be writing probes in it. So mutation checks never touch the shared
working tree:

- Do all mutation work in a dedicated worktree of the commit under review,
  next to the repository rather than inside it, for example
  `git worktree add --detach ../mutants-<id> HEAD`. `<id>` is unique to the
  verifier. A sibling directory stays inside the parent directory, which an
  agent's file tools can usually edit when `/tmp` is out of bounds. Each
  verifier uses its own worktree, so mutation verifiers MAY run in parallel
  with each other and with other angles, if the machine has the disk and CPU:
  each worktree is a cold build of several GB. A worktree adds no commits or
  branches, so it does not count as touching git history. Remove it with
  `git worktree remove --force <path>` before returning.
- Use [cargo-mutants](https://mutants.rs/) first. Run it inside the worktree,
  scoped to what is under review, because a whole crate has thousands of
  mutants and takes hours. For a change, use `--in-diff` with the change's
  diff, for example
  `git diff --output=../change-<id>.diff $(git merge-base HEAD origin/main) HEAD`
  and then `cargo mutants --in-diff ../change-<id>.diff`. Write the diff with
  `--output`, not a shell redirect: Windows PowerShell 5.1's `>` writes
  UTF-16, which cargo-mutants reads as an empty diff with no error. For a
  slice of a crate, use `cargo mutants -p <crate> -f <path>`. Pass `--output`
  with a directory outside the repository. Never pass `--in-place`.
  cargo-mutants builds and tests in copies of the tree, one per `-j` job, so
  parallel jobs cannot compile against each other. A `MISSED` mutant is the
  evidence for "this is untested". Quote its line from the output. If
  `cargo mutants --version` fails, say so in the output and fall back to hand
  mutations.
- cargo-mutants applies a fixed set of mutations. Among others, it replaces
  function bodies with default values, swaps operators, deletes `!` and
  unary `-`, deletes match arms and struct-literal fields, and forces match
  guards to `true` or `false`. It does not edit string literals or constants
  inside a function body, or delete a single statement. Make those mutations
  by hand, in the worktree, one at a time. Run
  the relevant tests there, and undo each before the next, either with
  `git -C <worktree> checkout -- <path>` or by reverting the edit and
  confirming with
  `git show HEAD:<path> | diff --strip-trailing-cr - <worktree>/<path>`.
  The worktree holds only committed content, so both undos are correct
  there. To check an
  error-message assertion, for example, change the message text and confirm
  that a test fails. Quote the mutation's diff and the test output as
  evidence.
- Never use `git clean`, `git stash`, or `git checkout -- <path>` in the
  shared working tree. Uncommitted deliverables (a report being written, probe
  tests, an uncommitted change) live there, and these commands remove them.
