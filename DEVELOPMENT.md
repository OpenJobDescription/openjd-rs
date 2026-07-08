# Development documentation

This guide describes how to work on openjd-rs. It assumes you are developing alongside an
AI coding agent — we use this approach for almost all changes. The workflows here are
tool-agnostic; any capable agent (Kiro, Claude Code, Codex, etc.) can follow them.

For the mechanical quick reference (commands, crate map, CI jobs, release process) see
[AGENTS.md](AGENTS.md). For deeper guides on individual procedures (regenerating quality
reports, etc.) see [docs/dev/](docs/dev/README.md). For a historical record of how the
project was originally ported from Python and the prompts used, see
[specs/rust-port-agent-method.md](specs/rust-port-agent-method.md).

## Environment

1. A [Rust toolchain](https://rustup.rs/) (stable channel, MSRV 1.94.1).
2. `cargo` (included with the Rust toolchain).
3. Nightly rustfmt for formatting checks (`rustup toolchain install nightly`).

Linux, macOS, and Windows all work. Some sessions tests require Docker (Linux) or a test
user account (Windows) — see `specs/sessions/cross-user-testing.md`.

## The three artifacts

Every change touches some subset of three artifacts that must stay aligned:

1. **Specs** in `specs/<crate>/`. These describe goals, design decisions, and the public
   API. Every crate has a `public-api.md` that is authoritative for its surface.
2. **Implementation** in `crates/openjd-<crate>/src/`.
3. **Tests** in-crate and under `crates/openjd-<crate>/tests/`.

Whatever order you edit these in, before committing, confirm they line up. If behavior
changed, the spec must reflect it. If the spec changed, the code and tests must match.

## Workflows

### Making a bug fix or small change

1. Point the agent at the spec and code for the area. A prompt like *"Read
   `specs/expr/evaluator.md` and `crates/openjd-expr/src/eval/evaluator.rs` before
   answering"* keeps its claims grounded.
2. Write a failing test first where practical. For validation and evaluation errors,
   assert on the full multi-line error message — see the "Test Quality Standard" section
   in AGENTS.md.
3. Implement the fix. Update the spec if behavior changed.
4. Run `cargo clippy --all-features --all-targets --workspace -- -D warnings` and
   `cargo test --workspace`. These are the minimum before opening a PR.

### Adding a feature from an RFC

New features typically start as an RFC in
[openjd-specifications](https://github.com/OpenJobDescription/openjd-specifications).

**Codevelop the RFC and the implementation together.** Keep a branch (or draft PR) with
an in-progress implementation alongside the RFC, and use it to stress-test the design.
Seeing how the feature looks in real code, real tests, and real error messages surfaces
issues that thought experiments miss — awkward APIs, ambiguous edge cases, compatibility
problems with existing specs. Iterate on the RFC and the implementation in the same
loop; land the RFC when the implementation validates it.

1. Read any existing RFC drafts and the relevant wiki spec. Have the agent summarize
   them and identify the crates affected.
2. Start a branch for the implementation. Update `specs/<crate>/` with the Rust-side
   design, including `public-api.md`.
3. Implement enough to evaluate the design in practice — it doesn't have to be complete
   or merge-ready. Write tests that exercise the interesting cases, especially edge
   cases the RFC is uncertain about.
4. Feed what you learn back into the RFC. Revise both sides until the design holds up.
5. When conformance tests exist for the feature, run them: see "Running the Conformance
   Suite" in AGENTS.md.

For larger features, delegate implementation of well-scoped pieces to subagent runs so
the main session stays focused on design and review.

### Report-driven iteration (the primary quality loop)

Most polish work — performance, API ergonomics, error-message quality, closing gaps with
the Python reference — happens through a report-driven loop. This is the single most
important workflow in the project.

1. **Generate a report.** Use the `eval-crate` skill (or a custom variant for a specific
   lens like backwards compatibility or suitability for a downstream consumer). Custom
   variants get their own filename under `reports/`.
2. **Commit the report verbatim.** A single PR that introduces the report with no
   edits. This establishes a clean baseline reviewers can diff against later.
3. **Triage and address.** Pick an item, a group of items, or a partial step toward an
   item — whatever granularity fits the change. Prompt the agent with a specific
   reference like *"Implement finding 4.2 from `reports/expr-quality-evaluation-report.md`"*,
   plus direction where needed.
4. **PR the change and mark the report.** The same PR edits the report: strike through
   the resolved item and append `**Resolved.**` or `**Resolved** — <brief note>.`

   ```markdown
   6. ~~**Decompose `validate_format_strings()`** into per-scope helpers.~~ **Resolved.**
   ```

   Reviewers see both the code change and the claim in the report diff, and can verify
   one against the other. See
   [docs/dev/working-through-findings.md](docs/dev/working-through-findings.md) for
   how to choose which findings to focus on, how to scope a PR, and example prompts
   for driving the agent through the work.
5. **Repeat, then retire.** When remaining items are stale, low-value, or the report no
   longer reflects the code, delete it and regenerate from scratch. Knowing when to
   retire a report is a judgment call. See
   [docs/dev/regenerating-quality-reports.md](docs/dev/regenerating-quality-reports.md)
   for the full procedure.

### Reviewing a PR

Review in two passes, from high level to low level:

1. **Review the `reports/` and `specs/` changes first.** These communicate the intent of
   the PR — what problem is being solved and what design has been chosen. If you have
   suggestions about the approach or want to fine-tune the direction, raise them here
   and stop. Don't read the code yet. This keeps the conversation focused on the design
   while it is still cheap to change, without getting pulled into implementation
   details that may become moot.
2. **Once the intent is agreed, review the code.** Assess code quality and, separately,
   how faithfully the code reflects the `reports/` and `specs/` diffs. An agent can do a
   first pass of that comparison: *"Compare the code changes in this PR against the
   changes to `specs/expr/evaluator.md` and `reports/expr-quality-evaluation-report.md`.
   List any discrepancies or claims in the specs not realized in the code."* Take the
   agent's output as hypotheses, then read the code yourself to confirm before leaving
   feedback on the PR.

### Integration with downstream consumers

Changes that affect Python bindings, the Deadline Cloud worker agent, or other
consumers usually need coordinated work across repos. Have the agent read the consumer
code to understand how the API is used before proposing API changes.

## Delegating work to subagents

For large tasks, a main agent session running subagents for well-scoped pieces works
better than doing everything in one long session. Good subagent tasks are self-contained
with clear success criteria: implement one finding from a report, port one group of
Python tests, write the tests for one spec section.

The main session plans, delegates, and validates. Subagents execute. Validation can
itself be a subagent run, but a human should verify before merge.

## Commands

```bash
cargo build --release                                                  # release build → target/release/openjd
cargo test --workspace                                                 # all tests
cargo test -p openjd-expr                                              # one crate
cargo clippy --all-features --all-targets --workspace -- -D warnings   # lint
cargo fmt --all                                                        # apply formatting
cargo doc --no-deps --workspace                                        # docs
scripts/coverage.sh                                                    # coverage report
```

See AGENTS.md for CI jobs, the conformance suite, and S3 integration tests.

### `OPENJD_TEST_PYTHON`

The `openjd-cli` integration tests run templates whose actions invoke
`python`. The harness probes `PATH` for `python`, then `python3` and the
versioned `python3.x` names, building a temporary shim so the fixtures stay
portable. To pin a specific interpreter (e.g. a particular venv) instead of
relying on auto-detection, set `OPENJD_TEST_PYTHON` to its absolute path:

```bash
OPENJD_TEST_PYTHON=/path/to/venv/bin/python cargo test -p openjd-cli
```

If the path doesn't exist the harness falls back to the `PATH` probe.

## Coding style

- `cargo fmt` before committing (nightly rustfmt).
- `cargo clippy` clean with `-D warnings`.
- All public items documented with `///`.
- Prefer `Result` over panicking.
- Conventional commit messages.
- Copyright headers on new files (CI checks this).
