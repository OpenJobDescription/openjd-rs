#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
#
# Interleaved wall-time comparison of two or more binaries on one command.
#
# Each round runs every binary once, rotating which goes first, so slow drift
# in the machine (thermal, other load) hits all of them equally. The first
# binary is also copied to a temporary path and timed as an extra entry,
# `<label>-copy`: the null test. Its difference from the original is the noise
# floor of the measurement (quality-criteria Q14); a difference between real
# binaries smaller than that is not a result. Time is a confirmation; claim
# work with scripts/perf/profile.py.
#
# Usage:
#   scripts/perf/bench.py [-n RUNS] [--warmup N] LABEL=BINARY [LABEL=BINARY ...] -- ARGS...
#
# Example:
#   scripts/perf/bench.py -n 40 main=/tmp/openjd-main new=target/release/openjd \
#       -- check samples/expr-demo-template.yaml
#
# Every binary must exit with the same code on every run; a mismatch stops
# the benchmark, because the binaries are then not doing the same job.

from __future__ import annotations

import argparse
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time


def parse_binaries(specs: list[str]) -> list[tuple[str, str]]:
    out = []
    for spec in specs:
        label, sep, path = spec.partition("=")
        if not sep or not label or not path:
            sys.exit(f"expected LABEL=BINARY, got {spec!r}")
        if not (os.path.isfile(path) and os.access(path, os.X_OK)):
            sys.exit(f"{label}: {path} is not an executable file")
        if any(label == seen for seen, _ in out):
            sys.exit(f"duplicate label {label!r}")
        out.append((label, path))
    return out


def run_once(path: str, args: list[str]) -> tuple[float, int]:
    start = time.perf_counter()
    proc = subprocess.run(
        [path, *args], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False
    )
    return time.perf_counter() - start, proc.returncode


def main() -> int:
    argv = sys.argv[1:]
    if "--" not in argv:
        sys.exit("usage: bench.py [-n RUNS] [--warmup N] LABEL=BINARY ... -- ARGS...")
    split = argv.index("--")
    ap = argparse.ArgumentParser(description="Interleaved wall-time comparison with a null test.")
    ap.add_argument("-n", "--runs", type=int, default=30, help="timed rounds (default 30)")
    ap.add_argument("--warmup", type=int, default=2, help="untimed rounds first (default 2)")
    ap.add_argument("--no-null-test", action="store_true", help="skip the copy of the first binary")
    ap.add_argument("binaries", nargs="+", metavar="LABEL=BINARY")
    opts = ap.parse_args(argv[:split])
    cmd_args = argv[split + 1 :]
    if opts.runs < 4:
        sys.exit("--runs must be at least 4 to report quartiles")
    binaries = parse_binaries(opts.binaries)

    with tempfile.TemporaryDirectory(prefix="openjd-bench-") as tmp:
        if not opts.no_null_test:
            label, path = binaries[0]
            copy = os.path.join(tmp, os.path.basename(path) + "-copy")
            shutil.copy2(path, copy)
            binaries.append((f"{label}-copy", copy))

        times: dict[str, list[float]] = {label: [] for label, _ in binaries}
        expected_code: int | None = None
        for i in range(opts.warmup + opts.runs):
            k = i % len(binaries)
            for label, path in binaries[k:] + binaries[:k]:
                elapsed, code = run_once(path, cmd_args)
                if expected_code is None:
                    expected_code = code
                elif code != expected_code:
                    sys.exit(
                        f"{label} exited {code}, others exited {expected_code}: "
                        "the binaries are not doing the same job"
                    )
                if i >= opts.warmup:
                    times[label].append(elapsed * 1000)

    base_label = binaries[0][0]
    base = statistics.median(times[base_label])
    print(f"{opts.runs} interleaved rounds, exit code {expected_code}: {' '.join(cmd_args)}")
    print(f"{'':>14} {'median':>10} {'p25':>9} {'p75':>9}   vs {base_label}")
    for label, _ in binaries:
        q1, _, q3 = statistics.quantiles(times[label], n=4)
        med = statistics.median(times[label])
        print(
            f"{label:>14} {med:>8.2f}ms {q1:>8.2f} {q3:>8.2f}   {100 * (med - base) / base:+6.1f}%"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
