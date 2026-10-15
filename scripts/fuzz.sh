#!/usr/bin/env bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
#
# Fuzzing driver for fuzz/. See fuzz/README.md.
#
#   scripts/fuzz.sh run [TARGET...]     fuzz each target (default: all) for
#                                       FUZZ_SECONDS (default 60) each
#   scripts/fuzz.sh replay [TARGET...]  run every committed corpus and
#                                       regression input through the
#                                       sanitizer build (no mutation)
#   scripts/fuzz.sh corpus              rebuild fuzz/corpus from the repo's
#                                       fixtures (plus OPENJD_SPECS_DIR if set)
#                                       and minimize it with `cargo fuzz cmin`
#   scripts/fuzz.sh cmin [TARGET...]    re-minimize fuzz/corpus in place,
#                                       folding in FUZZ_WORK_DIR if present
#   scripts/fuzz.sh cmin-work [TARGET...]  minimize FUZZ_WORK_DIR in place
#   scripts/fuzz.sh dict                regenerate fuzz/dicts/expr.dict
#   scripts/fuzz.sh list                print target names
#
# Environment:
#   FUZZ_TOOLCHAIN  nightly toolchain for cargo-fuzz (default: pinned below)
#   FUZZ_SECONDS    per-target time budget for `run` (default 60)
#   FUZZ_JOBS       parallel libFuzzer workers per target for `run` (default 1)
#   FUZZ_WORK_DIR   writable corpus root `run` grows (default fuzz/work);
#                   CI caches it between runs so coverage accumulates
#
# Every run uses the committed corpus as read-only seeds and grows
# FUZZ_WORK_DIR/<target>. A crash leaves its input in fuzz/artifacts/<target>/;
# once fixed, copy it to fuzz/regressions/<target>/ so the replay tests keep
# it fixed on every platform.

set -euo pipefail

FUZZ_TOOLCHAIN="${FUZZ_TOOLCHAIN:-nightly-2026-10-07}"
FUZZ_SECONDS="${FUZZ_SECONDS:-60}"
FUZZ_JOBS="${FUZZ_JOBS:-1}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FUZZ_DIR="$REPO_ROOT/fuzz"
FUZZ_WORK_DIR="${FUZZ_WORK_DIR:-$FUZZ_DIR/work}"
cd "$REPO_ROOT"

cargo_fuzz() { cargo "+${FUZZ_TOOLCHAIN}" fuzz "$@"; }

all_targets() { cargo_fuzz list; }

targets_or_all() {
    if [[ $# -gt 0 ]]; then
        printf '%s\n' "$@"
    else
        all_targets
    fi
}

# Text targets read tokens from the shared dictionary; generated targets
# consume raw bytes, where a dictionary only adds noise.
dict_args() {
    case "$1" in
        *_gen | expr_int_range | expr_value | expr_function_call | expr_path | \
            expr_symbol_table | sessions_framer | sessions_embedded_files) ;;
        *) echo "-dict=$FUZZ_DIR/dicts/expr.dict" ;;
    esac
}

# Number of finding files (crash-/timeout-/oom-/leak-) libFuzzer has left
# for a target. Not slow-unit-*: those are reports (at 10 s), not failures.
artifact_count() {
    local d="$FUZZ_DIR/artifacts/$1"
    [[ -d "$d" ]] || { echo 0; return; }
    find "$d" -maxdepth 1 -type f \( -name 'crash-*' -o -name 'timeout-*' -o -name 'oom-*' \
        -o -name 'leak-*' \) | wc -l | tr -d ' '
}

LIBFUZZER_ARGS=(-timeout=20 -rss_limit_mb=3072 -malloc_limit_mb=2048 -max_len=65536 -print_final_stats=1)

cmd="${1:-run}"
shift || true

case "$cmd" in
    list)
        all_targets
        ;;

    run)
        # `cargo fuzz run` builds only the target it runs. A failing target
        # does not stop the rest of the list; the exit status reports it.
        failed=()
        while IFS= read -r t; do
            [[ -z "$t" ]] && continue
            mkdir -p "$FUZZ_WORK_DIR/$t" "$FUZZ_DIR/corpus/$t"
            seeds=("$FUZZ_DIR/corpus/$t")
            [[ -d "$FUZZ_DIR/regressions/$t" ]] && seeds+=("$FUZZ_DIR/regressions/$t")
            echo "::group::fuzz $t (${FUZZ_SECONDS}s x ${FUZZ_JOBS})"
            parallel=()
            # Fork mode: N worker processes, stops (non-zero) on the first
            # crash. Fork mode ignores timeouts and OOMs by default; keep
            # them fatal, as they are in single-process mode.
            [[ "$FUZZ_JOBS" -gt 1 ]] &&
                parallel=(-fork="$FUZZ_JOBS" -ignore_timeouts=0 -ignore_ooms=0)
            # A finding is also detected by the artifact it leaves: fork
            # mode's parent can exit 0 after a child crash (it exits with
            # the raw wait status, which truncates to 0 on some platforms).
            before="$(artifact_count "$t")"
            # shellcheck disable=SC2046
            cargo_fuzz run --debug-assertions "$t" \
                "$FUZZ_WORK_DIR/$t" "${seeds[@]}" -- \
                "${LIBFUZZER_ARGS[@]}" $(dict_args "$t") \
                -max_total_time="$FUZZ_SECONDS" ${parallel[@]+"${parallel[@]}"} ||
                failed+=("$t")
            if [[ "$(artifact_count "$t")" -gt "$before" ]] &&
                [[ " ${failed[*]-} " != *" $t "* ]]; then
                failed+=("$t")
            fi
            echo "::endgroup::"
        done < <(targets_or_all "$@")
        if [[ ${#failed[@]} -gt 0 ]]; then
            echo "fuzzing found failures in: ${failed[*]} (inputs in fuzz/artifacts/)" >&2
            exit 1
        fi
        ;;

    replay)
        cargo_fuzz build --debug-assertions
        while IFS= read -r t; do
            [[ -z "$t" ]] && continue
            dirs=()
            for d in "$FUZZ_DIR/corpus/$t" "$FUZZ_DIR/regressions/$t"; do
                [[ -d "$d" ]] && [[ -n "$(ls -A "$d")" ]] && dirs+=("$d")
            done
            [[ ${#dirs[@]} -eq 0 ]] && continue
            echo "::group::replay $t"
            cargo_fuzz run --debug-assertions "$t" "${dirs[@]}" -- \
                "${LIBFUZZER_ARGS[@]}" -runs=0
            echo "::endgroup::"
        done < <(targets_or_all "$@")
        ;;

    corpus)
        raw="$(mktemp -d)"
        trap 'rm -rf "$raw"' EXIT
        cargo run --manifest-path "$FUZZ_DIR/Cargo.toml" --no-default-features \
            --example build_corpus -- "$raw" ${OPENJD_SPECS_DIR:+"$OPENJD_SPECS_DIR"}
        cargo_fuzz build --debug-assertions
        while IFS= read -r t; do
            [[ -z "$t" ]] && continue
            rm -rf "$FUZZ_DIR/corpus/$t"
            mkdir -p "$FUZZ_DIR/corpus/$t"
            cargo_fuzz cmin --debug-assertions "$t" "$raw/$t" -- \
                "${LIBFUZZER_ARGS[@]}"
            # cmin minimizes in place; move the survivors into the corpus.
            cp "$raw/$t"/* "$FUZZ_DIR/corpus/$t/"
        done < <(all_targets)
        ;;

    cmin-work)
        # Minimize FUZZ_WORK_DIR/<target> in place (CI does this before
        # caching it, so the cached corpus does not grow without bound).
        while IFS= read -r t; do
            [[ -z "$t" ]] && continue
            [[ -d "$FUZZ_WORK_DIR/$t" ]] && [[ -n "$(ls -A "$FUZZ_WORK_DIR/$t")" ]] || continue
            cargo_fuzz cmin --debug-assertions "$t" "$FUZZ_WORK_DIR/$t" -- "${LIBFUZZER_ARGS[@]}"
        done < <(targets_or_all "$@")
        ;;

    cmin)
        cargo_fuzz build --debug-assertions
        while IFS= read -r t; do
            [[ -z "$t" ]] && continue
            merged="$(mktemp -d)"
            cp "$FUZZ_DIR/corpus/$t"/* "$merged/" 2>/dev/null || true
            cp "$FUZZ_WORK_DIR/$t"/* "$merged/" 2>/dev/null || true
            cargo_fuzz cmin --debug-assertions "$t" "$merged" -- "${LIBFUZZER_ARGS[@]}"
            rm -rf "$FUZZ_DIR/corpus/$t"
            mkdir -p "$FUZZ_DIR/corpus/$t"
            cp "$merged"/* "$FUZZ_DIR/corpus/$t/"
            rm -rf "$merged"
        done < <(targets_or_all "$@")
        ;;

    dict)
        cargo run -q --manifest-path "$FUZZ_DIR/Cargo.toml" --no-default-features \
            --example build_dict > "$FUZZ_DIR/dicts/expr.dict"
        ;;

    *)
        sed -n '7,20p' "${BASH_SOURCE[0]}" >&2
        exit 2
        ;;
esac
