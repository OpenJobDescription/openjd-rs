#!/usr/bin/env bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
#
# Verifies that every source file in the repository carries the
# "Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved."
# header within its first 10 lines. This mirrors the test_copyright_header.py
# check used by the Python openjd libraries so the Rust port stays consistent.
#
# Usage: scripts/check_copyright_headers.sh
#
# Exits non-zero and prints the offending file paths if any file is missing
# a valid header.

set -euo pipefail

# Resolve the repo root (the parent of the directory containing this script)
# so the script works regardless of the caller's current working directory.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT"

# Top-level directories to scan. Kept narrow (same spirit as the Python test)
# so we don't pick up generated output under target/ or anything a developer
# may drop into the workspace root.
TOP_LEVEL_DIRS=(
    "crates"
    "scripts"
    "testing_containers"
    ".github"
)

# File extensions (globs) to check. Matches the set the Python test uses,
# plus .rs for the Rust sources that make up the bulk of this repo.
PATTERNS=(
    "*.rs"
    "*.sh"
    "*.py"
    "Dockerfile"
)

# The header we look for. Matches the phrase used by every Amazon-authored
# OpenJD source file. Case-insensitive to be forgiving of minor variations.
HEADER_REGEX='Copyright Amazon\.com, Inc\. or its affiliates\. All Rights Reserved\.'

# Files in the first 10 lines are searched, matching the Python test. Most
# source files have the header on line 1, but some Rust tests put a
# `#![allow(...)]` crate attribute above the header.
HEADER_LOOKAHEAD_LINES=10

missing=()
checked=0

for top in "${TOP_LEVEL_DIRS[@]}"; do
    if [[ ! -d "$top" ]]; then
        continue
    fi
    for pattern in "${PATTERNS[@]}"; do
        # -print0 / read -d '' handles paths containing spaces safely.
        while IFS= read -r -d '' file; do
            # Sub-crate build artefacts (e.g. helper target/) are not ignored
            # by the find expression below because they live under crates/,
            # so we filter them out explicitly.
            case "$file" in
                */target/*) continue ;;
            esac
            checked=$((checked + 1))
            if ! head -n "$HEADER_LOOKAHEAD_LINES" "$file" \
                | grep -Eqi "$HEADER_REGEX"; then
                missing+=("$file")
            fi
        done < <(find "$top" -type f -name "$pattern" -print0)
    done
done

if [[ ${#missing[@]} -gt 0 ]]; then
    echo "Missing copyright header in the following files:" >&2
    printf '  %s\n' "${missing[@]}" >&2
    echo >&2
    echo "Each source file must include, within its first ${HEADER_LOOKAHEAD_LINES} lines:" >&2
    echo "  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved." >&2
    exit 1
fi

echo "Copyright header check passed ($checked files scanned)."
