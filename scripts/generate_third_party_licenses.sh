#!/usr/bin/env bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
#
# Generates the third-party license attribution file via `cargo about generate`
# and writes it to the given output path (default: THIRD-PARTY-LICENSES).
#
# This file is NOT committed to source control. It is produced at release time
# and attached to each GitHub Release as a build artifact (see
# .github/workflows/release-plz.yml). crates published to crates.io do not
# bundle their dependencies, so the published .crate carries no attribution
# obligation for them; the obligation only applies to distributed artifacts
# that embed dependency code (e.g. a compiled binary), which is what the
# Release attachment covers.
#
# Usage:
#   scripts/generate_third_party_licenses.sh [OUTPUT_PATH]
#
# Requires cargo-about (install with `cargo install cargo-about --locked --features cli`).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT"

output="${1:-THIRD-PARTY-LICENSES}"

case "${1:-}" in
    -h|--help)
        sed -n '2,20p' "$0"
        exit 0
        ;;
esac

if ! command -v cargo-about >/dev/null 2>&1; then
    echo "error: cargo-about not found on PATH." >&2
    echo "Install it with: cargo install cargo-about --locked --features cli" >&2
    exit 2
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "error: jq not found on PATH (needed to extract workspace member names)." >&2
    exit 2
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

# Ensure consistent LF line endings (portable across GNU/BSD sed).
tr -d '\r' < "$generated" > "$output"

echo "Wrote third-party license attributions to $output" >&2
