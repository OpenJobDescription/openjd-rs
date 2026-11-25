#!/usr/bin/env python3
# /// script
# requires-python = ">=3.9"
# dependencies = ["pyyaml>=6"]
# ///
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
#
# Differential test: run two `openjd` binaries over many templates and compare
# everything they print.
#
# A performance change must not change any output (quality-criteria Q14).
# Every template under each DIR is run through `check`, and job templates also
# through `summary`, once per extension set. Exit code, stdout and stderr must
# match exactly. A conformance `*.test.yaml` file contributes its `template:`.
#
# Some output is ordered by a randomly seeded hash map (for example some
# environment `variables` errors). When the outputs differ, the old binary is
# run again; if it disagrees with itself, the case is nondeterministic and the
# two outputs are compared as multisets of lines instead, and counted
# separately. A real ordering change in deterministic output is still caught.
#
# Usage (uv provides PyYAML from the header above):
#   uv run scripts/perf/difftest.py OLD NEW DIR [DIR ...] [--extensions SET ...]
#
# Example:
#   uv run scripts/perf/difftest.py /tmp/openjd-main target/release/openjd \
#       ../openjd-specifications/conformance-tests/2023-09 \
#       --extensions default --extensions '' --extensions EXPR,FEATURE_BUNDLE_1
#
# CallerLimits cannot be set from the CLI, so library behaviour under other
# limits needs a library-level probe (see docs/dev/performance.md).

from __future__ import annotations

import argparse
import difflib
import os
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor

import yaml


def collect(dirs: list[str], scratch: str) -> list[str]:
    """Template paths to run, with conformance test templates extracted."""
    out = []
    for top in dirs:
        if not os.path.isdir(top):
            sys.exit(f"{top} is not a directory")
        for root, subdirs, files in os.walk(top):
            subdirs.sort()
            for name in sorted(files):
                path = os.path.join(root, name)
                if name.endswith(".test.yaml"):
                    try:
                        with open(path, encoding="utf-8") as fh:
                            doc = yaml.safe_load(fh)
                    except (OSError, UnicodeDecodeError, yaml.YAMLError) as e:
                        sys.exit(f"{path}: {e}")
                    if not isinstance(doc, dict) or "template" not in doc:
                        continue
                    dest = os.path.join(scratch, f"{len(out):05d}-{name}")
                    with open(dest, "w", encoding="utf-8") as fh:
                        yaml.safe_dump(
                            doc["template"], fh, sort_keys=False, allow_unicode=True
                        )
                    out.append(dest)
                elif name.endswith((".yaml", ".yml", ".json")):
                    out.append(path)
    return out


def is_job_template(path: str) -> bool:
    try:
        with open(path, encoding="utf-8", errors="replace") as fh:
            return "jobtemplate-" in fh.read()
    except OSError as e:
        sys.exit(f"{path}: {e.strerror}")


# Exit code, stdout and stderr as bytes, so output that is not UTF-8 is
# compared exactly; it is decoded only to show a difference.
Result = tuple[int, bytes, bytes]
# Exit status recorded for an invocation that timed out; no real exit is this.
TIMED_OUT = -(10**9)


def run(binary: str, args: list[str], timeout: float) -> Result:
    try:
        p = subprocess.run(
            [binary, *args], capture_output=True, timeout=timeout, check=False
        )
    except subprocess.TimeoutExpired:
        return (TIMED_OUT, b"", f"<timed out after {timeout}s>".encode())
    return (p.returncode, p.stdout, p.stderr)


def as_multiset(r: Result) -> tuple[int, list[bytes], list[bytes]]:
    return (r[0], sorted(r[1].splitlines()), sorted(r[2].splitlines()))


def compare(
    old: str, new: str, args: list[str], timeout: float
) -> tuple[str, Result, Result]:
    """'same', 'unordered' (nondeterministic order, same lines), 'timeout', or
    'differ'."""
    a, b = run(old, args, timeout), run(new, args, timeout)
    if TIMED_OUT in (a[0], b[0]):
        return "timeout", a, b
    if a == b:
        return "same", a, b
    again = run(old, args, timeout)
    if again == b:
        return "same", again, b
    if again != a and as_multiset(again) == as_multiset(a) == as_multiset(b):
        return "unordered", a, b
    return "differ", a, b


def show(args: list[str], a: Result, b: Result) -> str:
    lines = [f"== openjd {' '.join(args)}", f"   exit {a[0]} -> {b[0]}"]
    for name, x, y in (("stdout", a[1], b[1]), ("stderr", a[2], b[2])):
        if x != y:
            diff = difflib.unified_diff(
                x.decode(errors="backslashreplace").splitlines(),
                y.decode(errors="backslashreplace").splitlines(),
                f"old {name}",
                f"new {name}",
                lineterm="",
                n=2,
            )
            lines.extend(
                "   " + (d if len(d) <= 200 else d[:197] + "...")
                for d in list(diff)[:40]
            )
    return "\n".join(lines)


PROBE_TEMPLATE = """\
specificationVersion: jobtemplate-2023-09
name: probe
steps:
- name: s
  script:
    actions:
      onRun:
        command: echo
"""


def check_extension_sets(
    old: str, new: str, ext_sets: list, scratch: str, timeout: float
) -> None:
    """Exit unless both binaries accept every extension set.

    A set they reject (a misspelt name) fails every template the same way on
    both sides, which would read as "0 differences".
    """
    probe = os.path.join(scratch, "extension-probe.yaml")
    with open(probe, "w", encoding="utf-8") as fh:
        fh.write(PROBE_TEMPLATE)
    for ext in ext_sets:
        if ext is None:
            continue
        for b in (old, new):
            code, out, err = run(b, ["check", probe, "--extensions", ext], timeout)
            if code != 0:
                msg = (err or out).decode(errors="replace").strip().splitlines()
                sys.exit(
                    f"{b} rejects --extensions {ext!r} on a minimal template: "
                    f"{msg[0] if msg else f'exit {code}'}"
                )


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Compare two openjd binaries' output over many templates."
    )
    ap.add_argument("old")
    ap.add_argument("new")
    ap.add_argument("dirs", nargs="+")
    ap.add_argument(
        "--extensions",
        action="append",
        metavar="SET",
        help=(
            "pass --extensions SET (repeatable); 'default' runs without the flag, "
            "which is also what happens when this option is not given"
        ),
    )
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    ap.add_argument(
        "--timeout", type=float, default=120.0, help="seconds per invocation"
    )
    ap.add_argument("--show", type=int, default=5, help="differences to print")
    opts = ap.parse_args()
    # Absolute, so a bare name is the file named, not a PATH lookup.
    opts.old, opts.new = os.path.abspath(opts.old), os.path.abspath(opts.new)
    for b in (opts.old, opts.new):
        if not (os.path.isfile(b) and os.access(b, os.X_OK)):
            sys.exit(f"{b} is not an executable file")
    if opts.jobs < 1:
        sys.exit("--jobs must be at least 1")
    if opts.timeout <= 0:
        sys.exit("--timeout must be positive")
    ext_sets = [None] if opts.extensions is None else opts.extensions
    ext_sets = [None if e == "default" else e for e in ext_sets]

    with tempfile.TemporaryDirectory(prefix="openjd-difftest-") as scratch:
        check_extension_sets(opts.old, opts.new, ext_sets, scratch, opts.timeout)
        templates = collect(opts.dirs, scratch)
        if not templates:
            sys.exit(f"no templates found under {', '.join(opts.dirs)}")
        cases = []
        for path in templates:
            cmds = ["check", "summary"] if is_job_template(path) else ["check"]
            for cmd in cmds:
                for ext in ext_sets:
                    cases.append(
                        [cmd, path] + ([] if ext is None else ["--extensions", ext])
                    )
        with ThreadPoolExecutor(max_workers=opts.jobs) as pool:
            results = list(
                pool.map(
                    lambda c: (c, *compare(opts.old, opts.new, c, opts.timeout)), cases
                )
            )

    differ = [(c, a, b) for c, kind, a, b in results if kind == "differ"]
    unordered = sum(1 for _, kind, _, _ in results if kind == "unordered")
    timeouts = [c for c, kind, _, _ in results if kind == "timeout"]
    print(
        f"{len(templates)} templates, {len(cases)} invocations, {len(differ)} differences"
        + (
            f" ({unordered} with nondeterministic line order, compared as multisets)"
            if unordered
            else ""
        )
    )
    for c, a, b in differ[: opts.show]:
        print(show(c, a, b))
    if timeouts:
        print(
            f"{len(timeouts)} invocations timed out after {opts.timeout}s and were not compared:"
        )
        for c in timeouts[: opts.show]:
            print(f"== openjd {' '.join(c)}")
    return 1 if differ or timeouts else 0


if __name__ == "__main__":
    sys.exit(main())
