# Regenerating quality reports

The `reports/` directory holds per-crate quality evaluation reports
(`reports/<crate>-quality-evaluation-report.md`). Each one is the artifact that
drives the [report-driven iteration
loop](../../DEVELOPMENT.md#report-driven-iteration-the-primary-quality-loop):
generate a report, commit it verbatim as a baseline, then work through findings
in subsequent PRs that strike through resolved items.

## Why we keep doing this

A quality report is the agentic-AI counterpart to the read-throughs and audits
a careful maintainer would do by hand — except the agent has the patience to
do it across thousands of lines of specs, code, and tests in one pass, every
time. The point isn't to produce a perfect crate from a single report; it's to
make a continuous improvement loop work.

A single run of an agent is stochastic. It samples from some distribution over
the issues that exist in the crate — misalignment between specs and code,
public-API rough edges, weak tests, performance footguns, divergences from the
Python reference. No single run catches everything, but each run finds
*something* with high probability, and the things it finds are usually
worthwhile. After you address the findings from one report and regenerate, the
next pass samples again. New issues surface — partly because the previous fix
shifted the surface area, partly because the agent's attention landed
somewhere different this time. Over many cycles the union of those samples is
a much more thorough audit than any single pass could deliver.

The reports also become a record of how the crate is trending. Reading the
sequence of an old report, the next one, and the latest tells you things a
single snapshot can't:

- **Has the executive-summary characterization changed?** Earlier reports for
  a crate often read "mature implementation, several behavioral bugs found";
  later ones shift to "well-tested, mostly polish remaining." That shift in
  language is signal.
- **Has the criticality of findings shifted?** A healthy trajectory moves
  from correctness bugs and spec-implementation drift, through API ergonomic
  and naming consistency, toward documentation polish and minor test-coverage
  gaps. If the latest report still surfaces the same class of bug the report
  before it did, that's a signal too.
- **Are entire categories disappearing?** Watching "Python comparison" or
  "exploratory findings" sections shrink across regenerations is a concrete
  way to see internal consistency improving.

This loop only works if reports are regenerated regularly and committed
verbatim, with no editorial filtering of what the agent saw. The rest of this
document covers when to retire the current report and how to produce its
replacement.

## When to regenerate

A report is most useful when it reflects the current state of the crate. As
findings get resolved and the code evolves, the report falls out of date.
Regenerate when
one or more of these is true:

- **Most remaining items are stale or low-value.** The high-signal findings
  have been addressed; what's left is either no longer applicable (the
  surrounding code was rewritten) or judgment calls you've already decided
  not to act on.
- **The report no longer reflects the code.** A recent refactor, API change,
  or new feature has moved enough of the surface area that the report's
  observations are out of date even where they haven't been struck through.
- **You want a different lens.** The default `eval-crate` skill evaluates a
  crate against its own specs, tests, and Python reference. If you need a
  different perspective — backwards-compatibility against a pinned older
  version, suitability for a specific downstream consumer, performance
  hotspots — produce a custom variant under a different filename (e.g.
  `reports/<crate>-backcompat-report.md`).

Knowing when to retire a report is a judgment call. It is fine to live with a
report for many PRs while findings get worked through; it is also fine to
regenerate after only a handful if the codebase has shifted enough that the
old report is misleading. Err on the side of regenerating — a fresh report is
cheap and the diff against the previous one is itself informative.

### A worked example: the current `expr` report

At the time of writing, `reports/expr-quality-evaluation-report.md` is in the
state where either choice — keep iterating or regenerate — is defensible.
The report opens by characterizing the crate as mature and well-tested, and
calls out one material issue: spec–implementation drift in the non-`public-api`
spec documents after a profile refactor. All five P1 "Spec–implementation
synchronization" items have been resolved and struck through, for example:

```markdown
1. ~~**Rewrite the host-context sections of `architecture.md`,
   `evaluator.md`, `format-string.md`, `function-library.md`, and
   `path-mapping.md`** to describe the real API …~~ **Resolved** — all
   five spec docs now describe the real API, plus `specs/cli/run.md`
   and `specs/model/validation.md` (which had the same drift). …

5. ~~**Fix the inline "8 MB worker-thread stack" comment in
   `eval/parse.rs`** (in `with_profile` doc) to say 32 MB, matching
   `PARSER_THREAD_STACK_SIZE` and the comment on that constant.~~
   **Resolved.**
```

What remains is seven P2/P3 items: switching a couple of `Result<_, String>`
returns to `Result<_, ExpressionError>` for crate-wide consistency, factoring
a duplicated identifier-validation predicate, adding a `proptest` harness, and
similar polish. The exploratory section found no bugs.

We could keep working through those — they're all real improvements. But the
headline finding is gone, the exploratory section is empty, and the remaining
items are the kind of thing the agent will surface again on the next pass if
they still matter. More importantly, since the last regeneration the crate
has continued to evolve and a fresh pass might surface new, more pressing
concerns that the current report's polish-phase framing would obscure. So
this is a good moment to retire the report and regenerate, and let the next
report tell us where attention should go next.

## How to regenerate

The `eval-crate` skill (in `skills/eval-crate/SKILL.md`) is the canonical
procedure. It assumes the `openjd-rs` repo and the relevant Python reference
repo are checked out side by side; see the skill for the branch table.

### From Claude Code

```
/eval-crate expr
```

The slash command invokes the skill with the crate name as its argument.
Valid crate names are `expr`, `model`, `sessions`, `cli`, and `snapshots`.

### From Kiro

Kiro picks up the same skill definition. Open a Kiro session in the repo and
prompt it with the equivalent of:

```
Run the eval-crate skill for the expr crate.
```

Kiro will read `skills/eval-crate/SKILL.md`, follow the evaluation procedure,
and write the report to the location specified in the skill's quick-reference
table.

### What the skill does

In short: it deletes the old report, reads the specs/source/tests, compares
against the Python reference, builds and tests the crate, runs exploratory
probes, and writes a fresh report at `reports/<crate>-quality-evaluation-report.md`.

The full procedure — including the Python reference branch table, the
required report structure, and the alignment criteria the skill checks
— lives in [`skills/eval-crate/SKILL.md`](../../skills/eval-crate/SKILL.md).
It is short and worth reading directly if you want to understand exactly what
the agent will do, or to adapt it into a custom variant.

A run typically takes around ten minutes and produces a report several
hundred lines long.

### Finishing the worked example: what we got back

The `expr` regeneration completed cleanly. The new report opens with a
notably different headline than the previous one — gone is the "material
spec–implementation drift" framing; the lead is now flat-out positive:

> The `openjd-expr` crate is a high-quality, mature implementation of the
> OpenJD Expression Language. … ~15.5k lines of source paired with ~26k
> lines of tests (a ~1.7× test-to-source ratio), comprehensive specs (14
> documents under `specs/expr/`), and a clean compile + clippy run. All
> 3,261 tests pass (297 in-source unit + 2,956 integration + 8 doctests).

The §8 Recommendations section is the most striking part of the diff. Where
the previous report opened with five P1 spec-sync items, the new one
explicitly notes:

> | # | Priority | Subject | Detail |
> |---|---|---|---|
> | 1 | P3 | Add `profile.md` to `specs/expr/` | … |
> | 2 | P3 | Update `architecture.md` module layout block | … |
> | 3 | P3 | Consider a `tests/common/` shared helper module | … |
> | … | … | … | … |
>
> No P1 or P2 items were identified.

Eight recommendations total, all P3, all polish — the ergonomic clone in
`eval_attribute`, an `O(R × L)` keyword-rename loop, a missing `profile.md`
spec doc, a `pub const` for `MAX_SUGGESTION_DISTANCE`. The 53 exploratory
probes covered arithmetic overflow, Unicode boundaries, hashing equivalence,
range-expr extremes, and format-string escape round-trips, and all passed
without finding a bug.

That is exactly the trend signal the loop is meant to surface: the language
of the executive summary moved from "mature implementation with material
drift" to "production-ready, exemplary alignment", and the criticality of
findings collapsed from a P1-led list to all-P3. We commit this report
verbatim and treat it as one input among several into our read on the
crate's quality — useful especially for the comparison the next
regeneration will make possible.

## Land it untriaged, in a PR of its own

**Do not edit the report before committing it.** The point of the baseline is
that it captures what the agent saw on a single pass, with no human filtering.
Open a PR that adds (or replaces) only the report file:

```
git checkout -b refresh-<crate>-quality-report
git add reports/<crate>-quality-evaluation-report.md
git commit -m "test: Refresh the <crate> quality evaluation report"
```

The PR description can be terse — "Regenerated via `eval-crate`. No code or
spec changes." Reviewers can skim the report, but the bar for merging is low:
this is a snapshot, not a set of decisions. Examples of past baseline-only
commits:

- `ada0a99 test: Use Kiro to generate a fresh evaluation report`
- `5bf462c test: Refresh Kiro-generated reports for expr and model`
- `70eab76 test: Update the expr quality evaluation report`

Triage and fixes happen in separate PRs that follow. Each of those PRs edits
the report in place — striking through the resolved item and appending
`**Resolved.**` or `**Resolved** — <brief note>.` — alongside the code or
spec change. See the [Report-driven iteration
section](../../DEVELOPMENT.md#report-driven-iteration-the-primary-quality-loop)
of DEVELOPMENT.md for the full loop, and any of the existing reports for
examples of the strike-through convention:

```markdown
6. ~~**Decompose `validate_format_strings()`** into per-scope helpers.~~ **Resolved.**
```

Keeping the baseline PR pure makes the diff on each follow-up PR self-explanatory:
the report change shows what claim is being addressed, the code change shows
how, and a reviewer can verify one against the other in a single pass.
