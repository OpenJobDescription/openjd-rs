# Releasing openjd-rs

This document describes how openjd-rs crates are released to [crates.io](https://crates.io/).

## Overview

Releases are automated with [release-plz](https://release-plz.dev/). Every push
to `mainline` runs the [Release-plz workflow](./.github/workflows/release-plz.yml),
which opens (or updates) a **Release PR** titled "chore: release". When the
Release PR is merged, the same workflow publishes the updated crates to
crates.io, tags the release commit, and creates a GitHub Release for each
crate that was published.

Version bumps are determined from [conventional commit](https://www.conventionalcommits.org/en/v1.0.0/)
messages on the `mainline` branch:

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
| `openjd-snapshots` | ❌ no (`publish = false`) |
| `openjd-for-js`    | ❌ no (`publish = false`, built as npm package) |

## Configuration files

- [`release-plz.toml`](./release-plz.toml) — release-plz configuration: which
  crates are published, changelog template, conventional-commit → section map.
- [`.github/workflows/release-plz.yml`](./.github/workflows/release-plz.yml) —
  the automation workflow (runs on push to `mainline`).

## Authentication: crates.io Trusted Publishing

This repo authenticates to crates.io via **[Trusted Publishing](https://crates.io/docs/trusted-publishing)**
(OIDC). The workflow exchanges a short-lived GitHub Actions OIDC token for a
short-lived crates.io publish token. No long-lived `CARGO_REGISTRY_TOKEN`
secret is stored in the repo.

---

## Current state: dry-run

The workflow ships in **dry-run mode**: the `release` job runs
`cargo publish --dry-run` and does not create tags, GitHub Releases, or
publish to crates.io. The `release-pr` job is unaffected — it will still open
a Release PR so we can see what the automation proposes.

This lets us validate the configuration, watch a Release PR update itself
across several merges, and confirm OIDC is wired up correctly before the first
real publish happens.

To go live, remove the `dry_run: true` line from the `release-plz-release`
job in [`.github/workflows/release-plz.yml`](./.github/workflows/release-plz.yml).
Do this only after the initial manual-publish and Trusted-Publishing setup has
been completed for each crate. (See the crates.io Trusted Publishing docs and
release-plz documentation for the one-time setup steps.)

---

## Normal release process

The process is:

1. Land regular PRs on `mainline` using conventional commits.
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

## Adding a new non-publishable crate

Add `publish = false` to `[package]` in its `Cargo.toml`, and an entry in
`release-plz.toml` with `publish = false`, `release = false`,
`changelog_update = false`.
