#!/usr/bin/env bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
#
# Code coverage for openjd-rs using cargo-llvm-cov.
#
# Prerequisites:
#   cargo install cargo-llvm-cov
#   rustup component add llvm-tools-preview
#
# Usage:
#   ./coverage.sh                    # summary table to stdout
#   ./coverage.sh --html             # HTML report in coverage/html/
#   ./coverage.sh --lcov             # lcov file at coverage/lcov.info
#   ./coverage.sh -p openjd-snapshots  # single crate only
#
# All arguments are forwarded to cargo llvm-cov.

set -euo pipefail

# TODO: Replace with rust-toolchain.toml so the whole project pins the toolchain,
# rather than hardcoding +stable here.
TOOLCHAIN="+stable"

ARGS=("$@")

# Detect which format flag was given (if any)
format=""
for arg in "${ARGS[@]}"; do
    case "$arg" in
        --html) format=html ;;
        --lcov) format=lcov ;;
        --json|--cobertura) format=other ;;
    esac
done

case "$format" in
    html)
        exec cargo $TOOLCHAIN llvm-cov test --workspace --output-dir coverage "${ARGS[@]}"
        ;;
    lcov)
        # Remove --lcov from args, use --output-path instead
        FILTERED=()
        for arg in "${ARGS[@]}"; do
            [[ "$arg" != "--lcov" ]] && FILTERED+=("$arg")
        done
        mkdir -p coverage
        exec cargo $TOOLCHAIN llvm-cov test --workspace --lcov --output-path coverage/lcov.info "${FILTERED[@]}"
        ;;
    other)
        exec cargo $TOOLCHAIN llvm-cov test --workspace "${ARGS[@]}"
        ;;
    *)
        exec cargo $TOOLCHAIN llvm-cov test --workspace --summary-only "${ARGS[@]}"
        ;;
esac
