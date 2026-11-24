# Developer documentation

This directory holds extended developer guides that go deeper
than [DEVELOPMENT.md](../../DEVELOPMENT.md). DEVELOPMENT.md is the entry point
— it explains the recommended overall workflow to follow when developing in
this codebase.

For a mechanical quick reference (commands, crate map, CI jobs, release
process) see [AGENTS.md](../../AGENTS.md).

## Topics

- [Quality criteria](quality-criteria.md) — the single source of truth for the
  repo's quality bar, evidence rules, finding labels, and severity, shared by
  the `eval-crate` and `implement-change` skills.
- [Regenerating quality reports](regenerating-quality-reports.md) — when and
  how to produce a fresh `reports/<crate>-quality-evaluation-report.md` with
  the `eval-crate` skill, and how to land it as a baseline PR.
- [Working through quality-report findings](working-through-findings.md) —
  how to choose findings to focus on, scope each PR, and drive an agent
  through addressing them between baseline and the next regeneration.

## Agent skills

Agent skills live in [`skills/`](../../skills), one directory per skill, each
with a `SKILL.md`. Agent harnesses find them through symlinks:

- **Kiro** reads `.kiro/skills/`, which is a single symlink to `../skills`. A
  new skill is picked up automatically.
- **Claude Code** reads `.claude/skills/`, which holds one symlink per skill.
  A new skill needs its own link, committed alongside it:

  ```bash
  ln -s ../../skills/<name> .claude/skills/<name>
  ```

  On Windows, enable symlinks in git (`git config core.symlinks true`, which
  needs Developer Mode or an elevated shell) and create the link with
  `New-Item -ItemType SymbolicLink -Path .claude\skills\<name> -Target ..\..\skills\<name>`.
  Check that `git ls-files -s .claude/skills/<name>` shows mode `120000`, so it
  was committed as a link and not as a copy.
