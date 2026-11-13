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
#                                       and the committed inputs, minimized
#                                       with `cargo fuzz cmin`, then reduced
#                                       by scripts/select_fuzz_corpus.py
#   scripts/fuzz.sh cmin [TARGET...]    re-minimize fuzz/corpus in place,
#                                       folding in FUZZ_WORK_DIR if present
#                                       (same two steps as `corpus`)
#   scripts/fuzz.sh cmin-work [TARGET...]  minimize FUZZ_WORK_DIR in place
#   scripts/fuzz.sh dict                regenerate fuzz/dicts/expr.dict
#   scripts/fuzz.sh sync-lock           reset fuzz/Cargo.lock to the workspace
#                                       lock's versions, keeping the fuzz-only
#                                       pins (after a dependency bump)
#   scripts/fuzz.sh list                print target names
#
# Environment:
#   FUZZ_TOOLCHAIN  nightly toolchain for cargo-fuzz (default: pinned below)
#   FUZZ_SECONDS    per-target time budget for `run` (default 60)
#   FUZZ_JOBS       parallel libFuzzer workers per target for `run` (default 1)
#   FUZZ_WORK_DIR   writable corpus root `run` grows (default fuzz/work);
#                   CI caches it between runs so coverage accumulates
#   FUZZ_ASAN_DLL_DIR  Windows only: directory holding the MSVC ASan runtime
#                   (clang_rt.asan_dynamic-x86_64.dll); found automatically
#                   from the Visual C++ tools when unset
#
# Windows: run from Git Bash with the Visual C++ x64 tools installed.
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

IS_WINDOWS=0
case "$(uname -s)" in MINGW* | MSYS* | CYGWIN*) IS_WINDOWS=1 ;; esac

# Windows (MSVC) runs the fuzz binaries against the ASan runtime DLL that
# ships with the Visual C++ tools. Put its directory on PATH:
# FUZZ_ASAN_DLL_DIR if set, else the developer prompt's VCToolsInstallDir,
# else the newest Visual Studio with the x64 tools (prerelease included, as
# rustc's linker lookup does) and its default tools version.
ASAN_DLL=clang_rt.asan_dynamic-x86_64.dll
asan_path_ready=0
windows_asan_path() {
    [[ "$asan_path_ready" -eq 1 ]] && return
    local dir="${FUZZ_ASAN_DLL_DIR:-}"
    if [[ -z "$dir" && -n "${VCToolsInstallDir:-}" ]]; then
        dir="$(cygpath -u "$VCToolsInstallDir")/bin/Hostx64/x64"
    fi
    local program_files_x86
    program_files_x86="$(printenv 'ProgramFiles(x86)' || true)"
    if [[ -z "$dir" && -n "$program_files_x86" ]]; then
        local vswhere install version
        vswhere="$(cygpath -u "$program_files_x86")/Microsoft Visual Studio/Installer/vswhere.exe"
        if [[ -x "$vswhere" ]]; then
            # A failing vswhere falls through to the message below.
            install="$("$vswhere" -latest -prerelease -products '*' \
                -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 \
                -property installationPath 2> /dev/null | tr -d '\r')" || install=""
            local version_file="$install/VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt"
            if [[ -n "$install" && -f "$version_file" ]]; then
                version="$(tr -d '\r\n ' < "$version_file")"
                dir="$install/VC/Tools/MSVC/$version/bin/Hostx64/x64"
            fi
        fi
    fi
    if [[ -z "$dir" || ! -f "$dir/$ASAN_DLL" ]]; then
        echo "fuzz.sh: can't find $ASAN_DLL (looked in '${dir:-<no Visual C++ tools found>}');" \
            "install the Visual C++ x64 tools or set FUZZ_ASAN_DLL_DIR" >&2
        exit 1
    fi
    PATH="$(cygpath -u "$dir"):$PATH"
    asan_path_ready=1
}

# On Windows, cargo-fuzz's `/include:main` linker flag would reach every
# crate, and a dependency that also builds a DLL fails to link with it;
# fuzz/build.rs adds the flag for the fuzz binaries only.
cargo_fuzz() {
    if [[ "$IS_WINDOWS" -eq 1 && "$1" != list ]]; then
        local sub="$1"
        shift
        windows_asan_path
        cargo "+${FUZZ_TOOLCHAIN}" fuzz "$sub" --no-include-main-msvc "$@"
    else
        cargo "+${FUZZ_TOOLCHAIN}" fuzz "$@"
    fi
}

# Exits non-zero if `cargo fuzz list` fails. Callers read it with
# `targets="$(...)"` before the loop: a failure inside a here-string or
# process substitution would not stop the script under `set -e`.
all_targets() {
    local out
    out="$(cargo_fuzz list)" || { echo "fuzz.sh: cargo fuzz list failed" >&2; exit 1; }
    [[ -n "$out" ]] || { echo "fuzz.sh: cargo fuzz list printed no targets" >&2; exit 1; }
    printf '%s\n' "$out"
}

# The named targets (each must exist, so a typo fails instead of replaying
# nothing), or every target.
targets_or_all() {
    local all
    all="$(all_targets)" || exit 1
    if [[ $# -gt 0 ]]; then
        local t
        for t in "$@"; do
            grep -qxF -- "$t" <<< "$all" || { echo "fuzz.sh: unknown target '$t'" >&2; exit 1; }
        done
        printf '%s\n' "$@"
    else
        printf '%s\n' "$all"
    fi
}

# `name version` of every package in a Cargo.lock.
lock_packages() {
    awk '/^name = /{n=$3} /^version = /{gsub(/"/,"",n); v=$3; gsub(/"/,"",v); print n, v}' "$1"
}

# Text targets read tokens from the shared dictionary; generated targets
# consume raw bytes, where a dictionary only adds noise.
dict_args() {
    case "$1" in
        *_gen | expr_int_range | expr_value | expr_function_call | expr_path | \
            expr_symbol_table | sessions_framer | sessions_embedded_files | sessions_state) ;;
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
# The committed corpus is built in two steps: an edge-only set cover (no
# hit-count or value-profile features), then scripts/select_fuzz_corpus.py,
# which keeps a set cover of the openjd functions each target executes. That
# is about a tenth of the edge cover's files, and still executes everything
# the API gate checks. The CI-cached corpus keeps all features.
COMMITTED_CMIN_ARGS=(-use_counters=0 -use_value_profile=0 -set_cover_merge=1)

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
        targets="$(targets_or_all "$@")"
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
        done <<< "$targets"
        if [[ ${#failed[@]} -gt 0 ]]; then
            echo "fuzzing found failures in: ${failed[*]} (inputs in fuzz/artifacts/)" >&2
            exit 1
        fi
        ;;

    replay)
        targets="$(targets_or_all "$@")"
        # `cargo fuzz run` builds the target it runs, so named targets build
        # only themselves (CI's Windows/macOS build check replays one).
        if [[ $# -eq 0 ]]; then
            cargo_fuzz build --debug-assertions
        fi
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
        done <<< "$targets"
        ;;

    corpus)
        raw="$(mktemp -d)"
        trap 'rm -rf "$raw"' EXIT
        cargo run --manifest-path "$FUZZ_DIR/Cargo.toml" --no-default-features \
            --example build_corpus -- "$raw" ${OPENJD_SPECS_DIR:+"$OPENJD_SPECS_DIR"}
        cargo_fuzz build --debug-assertions
        targets="$(all_targets)"
        while IFS= read -r t; do
            [[ -z "$t" ]] && continue
            # Keep the committed inputs as candidates too: fixtures alone
            # may not reach everything the API gate checks.
            mkdir -p "$raw/$t"
            if [[ -d "$FUZZ_DIR/corpus/$t" ]]; then
                cp "$FUZZ_DIR/corpus/$t"/* "$raw/$t/" 2>/dev/null || true
            fi
            cargo_fuzz cmin --debug-assertions "$t" "$raw/$t" -- \
                "${LIBFUZZER_ARGS[@]}" "${COMMITTED_CMIN_ARGS[@]}"
            # cmin minimizes in place; replace the corpus with the survivors
            # only once it has succeeded.
            rm -rf "$FUZZ_DIR/corpus/$t"
            mkdir -p "$FUZZ_DIR/corpus/$t"
            cp "$raw/$t"/* "$FUZZ_DIR/corpus/$t/"
        done <<< "$targets"
        # Unquoted on purpose: one argument per target name.
        # shellcheck disable=SC2086
        python3 "$REPO_ROOT/scripts/select_fuzz_corpus.py" $targets
        ;;

    cmin-work)
        # Minimize FUZZ_WORK_DIR/<target> in place (CI does this before
        # caching it, so the cached corpus does not grow without bound).
        targets="$(targets_or_all "$@")"
        while IFS= read -r t; do
            [[ -z "$t" ]] && continue
            [[ -d "$FUZZ_WORK_DIR/$t" ]] && [[ -n "$(ls -A "$FUZZ_WORK_DIR/$t")" ]] || continue
            cargo_fuzz cmin --debug-assertions "$t" "$FUZZ_WORK_DIR/$t" -- "${LIBFUZZER_ARGS[@]}"
        done <<< "$targets"
        ;;

    cmin)
        targets="$(targets_or_all "$@")"
        cargo_fuzz build --debug-assertions
        while IFS= read -r t; do
            [[ -z "$t" ]] && continue
            merged="$(mktemp -d)"
            cp "$FUZZ_DIR/corpus/$t"/* "$merged/" 2>/dev/null || true
            cp "$FUZZ_WORK_DIR/$t"/* "$merged/" 2>/dev/null || true
            cargo_fuzz cmin --debug-assertions "$t" "$merged" -- \
                "${LIBFUZZER_ARGS[@]}" "${COMMITTED_CMIN_ARGS[@]}"
            rm -rf "$FUZZ_DIR/corpus/$t"
            mkdir -p "$FUZZ_DIR/corpus/$t"
            cp "$merged"/* "$FUZZ_DIR/corpus/$t/"
            rm -rf "$merged"
        done <<< "$targets"
        # shellcheck disable=SC2086
        python3 "$REPO_ROOT/scripts/select_fuzz_corpus.py" $targets
        ;;

    sync-lock)
        # Start fuzz/Cargo.lock from the workspace lock so the harness
        # builds against the dependency versions the crates ship with, let
        # cargo add the fuzz-only packages, then put those back at their
        # committed versions (they decide how corpus bytes decode).
        old="$(mktemp)"
        trap 'rm -f "$old"' EXIT
        cp "$FUZZ_DIR/Cargo.lock" "$old"
        cp "$REPO_ROOT/Cargo.lock" "$FUZZ_DIR/Cargo.lock"
        cargo metadata --format-version 1 --manifest-path "$FUZZ_DIR/Cargo.toml" > /dev/null
        ws="$(lock_packages "$REPO_ROOT/Cargo.lock" | cut -d' ' -f1 | sort -u)"
        new="$(lock_packages "$FUZZ_DIR/Cargo.lock")"
        while read -r name version; do
            [[ -z "$name" ]] && continue
            grep -qxF -- "$name" <<< "$ws" && continue
            current="$(awk -v n="$name" '$1 == n {print $2}' <<< "$new" | head -n1)"
            [[ -z "$current" || "$current" == "$version" ]] && continue
            cargo update --manifest-path "$FUZZ_DIR/Cargo.toml" -p "$name@$current" --precise "$version"
        done < <(lock_packages "$old")
        ;;

    dict)
        cargo run -q --manifest-path "$FUZZ_DIR/Cargo.toml" --no-default-features \
            --example build_dict > "$FUZZ_DIR/dicts/expr.dict"
        ;;

    *)
        sed -n '7,24p' "${BASH_SOURCE[0]}" >&2
        exit 2
        ;;
esac
