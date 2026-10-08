# Releasing openjd-rs

This document describes how openjd-rs crates are released to [crates.io](https://crates.io/),
and how the Python packages under `python/` are released to [PyPI](https://pypi.org/)
(see [Python packages](#python-packages)).

## Overview

Releases are automated with [release-plz](https://release-plz.dev/). Every push
to `main` runs the [Release-plz workflow](./.github/workflows/release-plz.yml),
which opens (or updates) a **Release PR** titled "chore: release". When the
Release PR is merged, the same workflow publishes the updated crates to
crates.io, tags the release commit, and creates a GitHub Release for each
crate that was published.

Version bumps are determined from [conventional commit](https://www.conventionalcommits.org/en/v1.0.0/)
messages on the `main` branch:

| Commit prefix | Bump |
|---------------|------|
| `fix:`, `perf:`, `docs:`, `refactor:`, `test:`, `ci:`, `chore:` | patch |
| `feat:` | minor |
| any type with `!` suffix or a `BREAKING CHANGE:` footer | major |

**Pre-1.0 note:** while a crate is still in the `0.x.y` range, `feat` bumps the
patch (not the minor) because the 0.x line is considered pre-stable. This is
release-plz's default behavior.

Each crate is versioned **independently**: release-plz only bumps the crates
whose files have changed since their last release, cascading bumps through
dependents when an intra-workspace dependency's version changes.

## Crates

| Crate | Published? |
|-------|------------|
| `openjd-expr`      | ✅ yes |
| `openjd-model`     | ✅ yes |
| `openjd-sessions`  | ✅ yes |
| `openjd-cli`       | ✅ yes |
| `openjd-snapshots` | ✅ yes |
| `openjd-for-js`    | ❌ no (`publish = false`, built as npm package) |

## Configuration files

- [`release-plz.toml`](./release-plz.toml) — release-plz configuration: which
  crates are published, changelog template, conventional-commit → section map.
- [`.github/workflows/release-plz.yml`](./.github/workflows/release-plz.yml) —
  the automation workflow (runs on push to `main`).

## Authentication: crates.io Trusted Publishing

This repo authenticates to crates.io via **[Trusted Publishing](https://crates.io/docs/trusted-publishing)**
(OIDC). The workflow exchanges a short-lived GitHub Actions OIDC token for a
short-lived crates.io publish token. No long-lived `CARGO_REGISTRY_TOKEN`
secret is stored in the repo.

---

## Normal release process

The process is:

1. Land regular PRs on `main` using conventional commits.
2. Release-plz automatically opens/updates a single **Release PR** per
   workspace. The PR shows the proposed version bumps and CHANGELOG entries.
3. A maintainer reviews the Release PR, edits the CHANGELOG entries if
   desired, and merges it.
4. On the post-merge run, release-plz publishes the changed crates to
   crates.io, creates git tags (`<crate-name>-v<version>`), and creates
   GitHub Releases.

### Forcing a version bump

If you need to force a particular bump (for example, to cut a `0.2.0` after a
series of `fix:` commits), edit the Release PR directly before merging. You
can change the `version` line in each `Cargo.toml` and update the CHANGELOG
accordingly; release-plz will respect your edits.

### Yanking a release

Use the standard Cargo tooling:

```bash
cargo yank --version <version> <crate-name>
```

Yanks are not automated by release-plz.

---

## Adding a new publishable crate

1. Create the crate under `crates/<new-crate>/`. Ensure `Cargo.toml` sets:
   - `version = "0.1.0"`
   - Workspace inherits for `edition`, `license`, `rust-version`, `authors`,
     `repository`, `homepage`, `readme`.
   - Its own `description`, `keywords` (max 5), `categories`.
   - Intra-workspace deps specify both `path` and `version`, e.g.
     `openjd-expr = { path = "../openjd-expr", version = "0.1.0" }`.
   - `LICENSE-Apache-2.0`, `LICENSE-MIT`, and `NOTICE` symlinked from the
     workspace root (`ln -sf ../../LICENSE-Apache-2.0 crates/<new-crate>/`).
2. Add a `[[package]]` entry for it in `release-plz.toml`.
3. Perform the one-time crate setup: manually publish the first version with
   `cargo publish -p <new-crate>` in dependency order, then register Trusted
   Publishing for it on crates.io pointing at this repo and the
   `release-plz.yml` workflow.
4. Add the crate to the two hand-maintained crate lists (see
   [Crate lists to keep in sync](#crate-lists-to-keep-in-sync)).

## Adding a new non-publishable crate

Add `publish = false` to `[package]` in its `Cargo.toml`, and an entry in
`release-plz.toml` with `publish = false`, `release = false`,
`changelog_update = false`. Then update the crate lists below.

## Python packages

The Python packages under `python/` (`openjd-model`, `openjd-sessions`,
`openjd-cli`) are released by the same release-plz Release PR as the crates.

- **Version source.** Each package's version is the `version` in its
  `Cargo.toml`: for `openjd-model` that's the PyO3 bindings crate
  (`openjd-model-py`); for `openjd-sessions` and `openjd-cli` it's an empty
  `publish = false` release anchor crate (`openjd-sessions-py`,
  `openjd-cli-py`). maturin and hatch read the version from there.
- **Bumps.** The packages are `git_only` in `release-plz.toml`, so release-plz
  takes the last release from the `python-<distribution>-v<version>` tag and
  bumps the version from conventional commits that touch the package's
  directory. A release of `openjd-expr`, `openjd-model`, or `openjd-sessions`
  cascades into `openjd-model-py` through its Cargo dependencies, and from
  there into the other two, so a Rust fix reaches PyPI in the same release.
- **Pins.** release-plz doesn't edit `pyproject.toml`. After it updates the
  Release PR, the workflow runs `scripts/sync_python_pins.py` on the PR branch
  to move the pins between the Python packages to the new versions. CI runs
  `scripts/sync_python_pins.py --check` on every PR.
- **Publishing.** When the Release PR is merged, `release-plz release` creates
  the tags and GitHub Releases. For each Python tag, the workflow builds the
  wheels (six platforms for the abi3 `openjd-model` extension) and sdist from
  the tag, PGP-signs every file, attaches the files and signatures to the
  GitHub Release, and uploads to PyPI in dependency order.

### One-time setup

- **PyPI trusted publishing.** For each of `openjd-model`, `openjd-sessions`
  and `openjd-cli` on PyPI, add a trusted publisher for repository
  `OpenJobDescription/openjd-rs`, workflow `release-plz.yml`, environment
  `release`. Without it, the PyPI upload step fails (and can be re-run once
  the publisher exists).
- **Signing secrets.** The `release` environment needs `AWS_PGP_KEY_SECRET_ROLE`,
  `AWS_PGP_KEY_SECRET` and `PGP_USER`, the same secrets the per-package
  repositories used.

## Crate lists to keep in sync

Two places enumerate the crates by hand and are not derived from the workspace,
so a new crate is invisible to them until it is added:

- The crates table in [README.md](README.md#crates), with the crate's status and
  a one-line description.
- `.github/workflows/eval_crate.yml`, which names the crates in four spots: the
  `workflow_dispatch` `options`, the schedule's crate array, the `case` that
  validates an `eval-crate:<crate>` label, and the Python-reference maps. A crate
  with no Python counterpart to compare against does not belong in that workflow
  at all — leave it out rather than adding a placeholder.
