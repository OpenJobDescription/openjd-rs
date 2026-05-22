# Developer documentation

This directory holds extended developer guides that go deeper
than [DEVELOPMENT.md](../../DEVELOPMENT.md). DEVELOPMENT.md is the entry point
— it explains the recommended overall workflow to follow when developing in
this codebase.

For a mechanical quick reference (commands, crate map, CI jobs, release
process) see [AGENTS.md](../../AGENTS.md).

## Topics

- [Regenerating quality reports](regenerating-quality-reports.md) — when and
  how to produce a fresh `reports/<crate>-quality-evaluation-report.md` with
  the `eval-crate` skill, and how to land it as a baseline PR.
- [Working through quality-report findings](working-through-findings.md) —
  how to choose findings to focus on, scope each PR, and drive an agent
  through addressing them between baseline and the next regeneration.
