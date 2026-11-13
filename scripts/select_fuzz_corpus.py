#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
#
# Reduce fuzz/corpus/<target>/ in place to a greedy set cover of the openjd
# functions its inputs execute.
#
# The committed corpus has two jobs: execute every public function the API
# gate (scripts/check_fuzz_api_coverage.py) checks, and seed fuzzing. An edge
# set cover (`cargo fuzz cmin`) keeps thousands of files; covering functions
# instead keeps about a tenth of them. The CI-cached grown corpus (fuzz/work,
# see .github/workflows/fuzz.yml) carries the edge-level detail between runs.
#
# Each input is replayed alone through the `replay` test built with
# `-Cinstrument-coverage`; a function counts if its llvm entry counter is
# nonzero. Some inputs reach a function only on some runs (hash-seeded
# collections vary between processes), so each input is replayed RUNS times
# and only the functions seen on every run count: the selection then relies on
# inputs that reach each function reliably. Which inputs it picks can still
# differ slightly between runs (model_job_gen), but every result covers the
# same functions.
# Every monomorphization of every function in an openjd crate is a
# separate element to cover, which is stricter than the gate (any
# instantiation of a public function counts there), so a selection that
# covers everything here keeps the gate passing. Regressions are not
# counted, so the corpus stands on its own.
#
# Usage:
#   scripts/select_fuzz_corpus.py [TARGET...]    # default: every target
#
# Needs Python 3.11+ and a stable toolchain with llvm-tools
# (`rustup component add llvm-tools`). scripts/fuzz.sh runs this after
# `cargo fuzz cmin` in `corpus` and `cmin`.

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
FUZZ = REPO / "fuzz"
TARGET_DIR = FUZZ / "target" / "select-corpus"
RUNS = 2


def build_replay() -> Path:
    """Build the coverage-instrumented `replay` test binary; return its path."""
    env = {
        **os.environ,
        "RUSTFLAGS": "-Cinstrument-coverage",
        "CARGO_TARGET_DIR": str(TARGET_DIR),
        # Build scripts are instrumented too; discard their profiles.
        "LLVM_PROFILE_FILE": os.devnull,
    }
    out = subprocess.run(
        [
            "cargo", "test", "--manifest-path", str(FUZZ / "Cargo.toml"), "--no-default-features",
            "--test", "replay", "--no-run", "--message-format=json",
        ],
        cwd=REPO, env=env, check=True, stdout=subprocess.PIPE, text=True,
    ).stdout
    exes = [
        m["executable"]
        for m in map(json.loads, out.splitlines())
        if m.get("reason") == "compiler-artifact" and m.get("executable")
        and m["target"]["name"] == "replay"
    ]
    if len(exes) != 1:
        sys.exit(f"select_fuzz_corpus: expected one replay test binary, found {exes}")
    return Path(exes[0])


def llvm_profdata() -> Path:
    def rustc(*a: str) -> str:
        return subprocess.run(["rustc", *a], check=True, capture_output=True, text=True).stdout

    sysroot = rustc("--print", "sysroot").strip()
    host = next(l.split()[1] for l in rustc("-vV").splitlines() if l.startswith("host:"))
    exe = Path(sysroot, "lib", "rustlib", host, "bin",
               "llvm-profdata.exe" if os.name == "nt" else "llvm-profdata")
    if not exe.is_file():
        sys.exit("select_fuzz_corpus: llvm-profdata not found; run `rustup component add llvm-tools`")
    return exe


def executed(profdata: Path, profraw: Path) -> frozenset[str]:
    """Mangled names of the openjd functions a raw profile saw execute."""
    text = subprocess.run([str(profdata), "show", "--all-functions", str(profraw)],
                          check=True, capture_output=True, text=True).stdout
    fns, name = set(), None
    for line in text.splitlines():
        if line.startswith("  ") and not line.startswith("   ") and line.endswith(":"):
            name = line.strip()[:-1]
        elif name is not None and line.strip().startswith("Function count:"):
            if int(line.split(":", 1)[1]) > 0 and "openjd" in name:
                fns.add(name)
            name = None
    return frozenset(fns)


def coverage(replay: Path, profdata: Path, work: Path, target: str,
             inp: Path | None) -> frozenset[str]:
    """Functions executed on every one of RUNS replays of one input (None: the harness alone)."""
    return frozenset.intersection(*(replay_once(replay, profdata, work, target, inp)
                                    for _ in range(RUNS)))


def replay_once(replay: Path, profdata: Path, work: Path, target: str,
                inp: Path | None) -> frozenset[str]:
    """Functions executed by replaying one input once."""
    d = Path(tempfile.mkdtemp(dir=work))
    try:
        tree = d / "tree" / target
        tree.mkdir(parents=True)
        if inp is not None:
            shutil.copy(inp, tree / inp.name)
        profraw = d / "replay.profraw"
        env = {**os.environ, "OPENJD_FUZZ_REPLAY_DIR": str(d / "tree"),
               "OPENJD_FUZZ_TARGET": target, "LLVM_PROFILE_FILE": str(profraw)}
        r = subprocess.run([str(replay), "replay_corpus", "--exact", "--test-threads=1"],
                           env=env, capture_output=True, text=True)
        if r.returncode != 0:
            raise RuntimeError(f"replay of {inp or 'no input'} for {target} failed:\n"
                               f"{r.stdout[-2000:]}{r.stderr[-2000:]}")
        return executed(profdata, profraw)
    finally:
        shutil.rmtree(d, ignore_errors=True)


def greedy(cands: dict[str, frozenset[str]], need: set[str]) -> list[str]:
    """Greedy set cover; ties go to the input covering fewer functions, then by name."""
    picked = []
    while need:
        best = min(cands, key=lambda f: (-len(need & cands[f]), len(cands[f]), f))
        gain = need & cands[best]
        if not gain:
            break
        picked.append(best)
        need -= gain
    return picked


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("targets", nargs="*")
    ap.add_argument("--corpus", type=Path, default=FUZZ / "corpus")
    args = ap.parse_args()
    targets = args.targets or sorted(p.name for p in args.corpus.iterdir() if p.is_dir())

    replay, profdata = build_replay(), llvm_profdata()
    TARGET_DIR.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(dir=TARGET_DIR))
    try:
        # Measure every target before deleting anything, so a failure leaves
        # the corpus untouched.
        plans = []
        for t in targets:
            files = sorted(p for p in (args.corpus / t).iterdir() if p.is_file())
            if not files:
                sys.exit(f"select_fuzz_corpus: {args.corpus / t} is empty")
            with ThreadPoolExecutor(os.cpu_count()) as ex:
                harness = ex.submit(coverage, replay, profdata, work, t, None)
                covs = list(ex.map(lambda p: coverage(replay, profdata, work, t, p), files))
            cands = {p.name: c for p, c in zip(files, covs)}
            need = set().union(*covs) - harness.result()
            # A target directory must stay non-empty (replay.rs checks it).
            keep = set(greedy(cands, set(need))) or {min(cands, key=lambda f: (len(cands[f]), f))}
            plans.append((t, files, keep, len(need)))
    except RuntimeError as e:
        sys.exit(f"select_fuzz_corpus: {e}")
    finally:
        shutil.rmtree(work, ignore_errors=True)
    for t, files, keep, n in plans:
        for p in files:
            if p.name not in keep:
                p.unlink()
        print(f"{t}: kept {len(keep)} of {len(files)} inputs ({n} functions)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
