#!/usr/bin/env bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
#
# Regenerates THIRD-PARTY-LICENSES via `cargo about generate` and fails if
# the result differs from the committed file. Keeps the shipped license
# attributions in lockstep with the actual dependency graph in Cargo.lock.
#
# Usage:
#   scripts/check_third_party_licenses.sh          # verify (CI mode)
#   scripts/check_third_party_licenses.sh --update # regenerate in place
#
# Requires cargo-about (install with `cargo install cargo-about --locked`).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT"

if ! command -v cargo-about >/dev/null 2>&1; then
    echo "error: cargo-about not found on PATH." >&2
    echo "Install it with: cargo install cargo-about --locked" >&2
    exit 2
fi

mode="verify"
if [[ $# -ge 1 ]]; then
    case "$1" in
        --update|-u)
            mode="update"
            ;;
        -h|--help)
            sed -n '2,12p' "$0"
            exit 0
            ;;
        *)
            echo "error: unknown argument: $1" >&2
            exit 2
            ;;
    esac
fi

# `cargo about` reads the workspace Cargo.toml, the `about.toml` config
# (which excludes build- and dev-dependencies), and renders every unique
# (crate, license) pair through `about.hbs`.
#
# cargo-about's `private.ignore` flag only excludes workspace members marked
# `publish = false`, so first-party crates that publish to crates.io still
# appear in the rendered list. We strip them out by filtering the workspace
# member names (from `cargo metadata`) out of the generated file. If a license
# block is left with no remaining crates, the whole block is dropped.
raw="$(mktemp)"
generated="$(mktemp)"
trap 'rm -f "$raw" "$generated"' EXIT

cargo about generate about.hbs > "$raw"

if ! command -v jq >/dev/null 2>&1; then
    echo "error: jq not found on PATH (needed to extract workspace member names)." >&2
    exit 2
fi

workspace_pattern="$(
    cargo metadata --no-deps --format-version=1 \
        | jq -r '.packages[].name' \
        | tr -d '\r' \
        | paste -sd'|' -
)"
if [[ -z "$workspace_pattern" ]]; then
    echo "error: cargo metadata returned no workspace members." >&2
    exit 2
fi

awk -v re="^[*][*] ($workspace_pattern); version " '
    /^------$/ {
        if (kept > 0) { printf "%s", block; print "------" }
        block = ""; kept = 0; next
    }
    /^[*][*] / && $0 ~ re { next }
    /^[*][*] /            { block = block $0 "\n"; kept++; next }
                          { block = block $0 "\n" }
    END {
        if (kept > 0) printf "%s", block
    }
' "$raw" > "$generated"

# Ensure consistent EOL.
sed -i 's/\r//' "$generated"

if [[ "$mode" == "update" ]]; then
    cp "$generated" THIRD-PARTY-LICENSES
    echo "Updated THIRD-PARTY-LICENSES."
    exit 0
fi

if ! diff -u THIRD-PARTY-LICENSES "$generated"; then
    echo >&2
    echo "THIRD-PARTY-LICENSES is out of date with respect to Cargo.lock." >&2
    echo "Regenerate it with:" >&2
    echo "  scripts/check_third_party_licenses.sh --update" >&2
    exit 1
fi

echo "THIRD-PARTY-LICENSES is up to date."
