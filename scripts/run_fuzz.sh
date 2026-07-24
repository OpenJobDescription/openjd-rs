#!/usr/bin/env bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
#
# Build and smoke-fuzz every fuzz target. The target list comes from
# `cargo fuzz list` (the single source of truth — every fuzz/fuzz_targets/*.rs
# with a matching [[bin]] in fuzz/Cargo.toml), so adding a target requires no
# change here or in the CI workflow: it is picked up automatically.
#
# Each target runs time-boxed, seeded with its committed corpus in
# fuzz/seeds/<target>. Any panic, abort, overflow, or char-boundary slice in a
# fuzzed entry point fails the run. A missing seed dir is not fatal (the target
# just starts from an empty corpus).
#
# Usage:
#   scripts/run_fuzz.sh                 # all targets, default budget
#   FUZZ_SECONDS=30 scripts/run_fuzz.sh # override per-target budget
#   scripts/run_fuzz.sh expr_parse …    # only the named targets
#
# Requires a nightly toolchain and cargo-fuzz (see fuzz/README.md). The nightly
# is pinned so a bad nightly can't randomly break the run; override with
# FUZZ_TOOLCHAIN.

set -euo pipefail

# Per-target wall-clock budget, in seconds. Coverage plateaus within a few
# seconds once seeded, so a short smoke run is enough to catch a regression
# reachable from the corpus; deeper campaigns are run manually with a larger
# value (see fuzz/README.md).
FUZZ_SECONDS="${FUZZ_SECONDS:-10}"
FUZZ_TOOLCHAIN="${FUZZ_TOOLCHAIN:-nightly-2026-05-15}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

cargo_fuzz() { cargo "+${FUZZ_TOOLCHAIN}" fuzz "$@"; }

# Targets: either the ones named on the command line, or every registered one.
# Read `cargo fuzz list` line-by-line rather than `mapfile` so the script runs
# on the Bash 3.2 that ships with macOS as well as CI's newer Bash.
if [[ $# -gt 0 ]]; then
    targets=("$@")
else
    targets=()
    while IFS= read -r line; do
        [[ -n "$line" ]] && targets+=("$line")
    done < <(cargo_fuzz list)
fi

if [[ ${#targets[@]} -eq 0 ]]; then
    echo "error: no fuzz targets found (cargo fuzz list returned nothing)" >&2
    exit 1
fi

echo "Fuzzing ${#targets[@]} target(s) for ${FUZZ_SECONDS}s each: ${targets[*]}"

# One build pass produces every target binary.
cargo_fuzz build

common_args=(-max_total_time="${FUZZ_SECONDS}" -timeout=25 -rss_limit_mb=4096)

for target in "${targets[@]}"; do
    echo "::group::fuzz ${target} (${FUZZ_SECONDS}s)"
    # Pass the seed corpus dir only when it exists. Branching (rather than
    # expanding a possibly-empty array) avoids the `unbound variable` abort
    # that Bash 3.2 — the macOS /bin/bash this script supports — raises for
    # "${arr[@]}" on an empty array under `set -u`.
    if [[ -d "fuzz/seeds/${target}" ]]; then
        cargo_fuzz run "${target}" "fuzz/seeds/${target}" -- "${common_args[@]}"
    else
        cargo_fuzz run "${target}" -- "${common_args[@]}"
    fi
    echo "::endgroup::"
done

echo "All ${#targets[@]} fuzz target(s) passed."
