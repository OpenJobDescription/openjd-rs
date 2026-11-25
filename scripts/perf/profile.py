#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
#
# Instruction-level profiles of one command with valgrind's callgrind tool.
#
# Instruction counts are exact and do not depend on machine load or binary
# layout, so a difference between two profiles is a difference in work done
# (quality-criteria Q14). This reads callgrind's output format directly
# (https://valgrind.org/docs/manual/cl-format.html), so it does not depend on
# the text layout of `callgrind_annotate`, which changes between releases.
#
# Usage:
#   scripts/perf/profile.py run OUT -- BINARY [ARGS...]
#       Profile a command; writes the callgrind file OUT.
#   scripts/perf/profile.py report OUT [--top N]
#       Total instructions, self cost by area, top functions by self and
#       inclusive cost, and the busiest Rust allocation call sites.
#   scripts/perf/profile.py diff OLD NEW [--top N] [--match REGEX]
#       Per-function change in inclusive cost between two profiles.
#   scripts/perf/profile.py callers OUT REGEX [--top N]
#       For each function matching REGEX: its self cost and its callers, with
#       call counts and the instructions each caller spent in it.
#
# Profile release builds: `cargo build --release -p openjd-cli`. Valgrind runs
# on Linux; on other platforms run it in a Linux container. Inclusive costs
# follow callgrind's call records, so a recursive function's inclusive cost
# counts its recursive calls again, as `callgrind_annotate --inclusive=yes`
# does.

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass, field

# Self-cost areas, matched against the function name in order; first match
# wins. A heuristic grouping for reading a report, not a measurement.
AREAS = [
    (
        "allocator",
        (
            r"^(_int_(free|malloc|realloc)|malloc|free|realloc|calloc|cfree"
            r"|malloc_consolidate|__rustc::__r[du][sl]_|__rust_(alloc|dealloc|realloc)"
            r"|<alloc::raw_vec::RawVecInner[^>]*>::finish_grow)"
        ),
    ),
    ("memcpy/memmove/memset", r"^__(memcpy|memmove|memset|memcmp)"),
    ("hashing", r"(sip::Hasher|hash_one|BuildHasher|core::hash::)"),
    ("yaml parse", r"(granit_parser|serde_saphyr)"),
    ("serde/json", r"(serde_json|serde_core)"),
    ("expr parser", r"(ruff_python|openjd_expr::eval::parse)"),
    ("expr evaluator", r"openjd_expr::eval::evaluator"),
    ("expr types/library", r"openjd_expr::(function_library|types|default_library)"),
    ("expr other", r"openjd_expr::"),
    ("model", r"openjd_model::"),
    ("sessions", r"openjd_sessions::"),
    ("snapshots", r"openjd_snapshots::"),
    ("dynamic loader", r"^(_dl_|do_lookup_x|check_match|strcmp$)"),
    ("std/core/alloc other", r"^<?(core|alloc|std)::"),
]
ALLOC_FN = re.compile(r"^__rustc::__rust_(alloc|alloc_zeroed|realloc)$")


@dataclass
class Profile:
    """Costs of one callgrind run, by function name (first event only)."""

    event: str
    total: int
    self_cost: Counter = field(default_factory=Counter)
    # caller -> callee -> [call count, inclusive cost of those calls]
    calls: dict = field(default_factory=lambda: defaultdict(lambda: defaultdict(lambda: [0, 0])))

    def inclusive(self) -> Counter:
        inc = Counter(self.self_cost)
        for caller, callees in self.calls.items():
            for _, cost in callees.values():
                inc[caller] += cost
        return inc

    def callers_of(self, callee: str) -> list[tuple[str, int, int]]:
        out = []
        for caller, callees in self.calls.items():
            if callee in callees:
                n, cost = callees[callee]
                out.append((caller, n, cost))
        return sorted(out, key=lambda r: -r[2])


def _first_cost(fields: list[str], npos: int) -> int:
    """The first event's cost from a cost line, after its position columns."""
    rest = fields[npos:]
    return int(rest[0]) if rest else 0


def load(path: str) -> Profile:
    """Parse a callgrind output file (format version 1)."""
    names: dict[str, str] = {}

    def resolve(spec: str) -> str:
        # Name compression: "(id) name" defines id, "(id)" refers to it.
        m = re.match(r"^\((\d+)\)(?: (.*))?$", spec)
        if not m:
            return spec
        if m.group(2) is not None:
            names[m.group(1)] = m.group(2)
        return names.get(m.group(1), spec)

    event, total, npos = "", 0, 1
    prof = None
    fn = ""
    pending_call: tuple[str, int] | None = None  # (callee, count)
    with open(path, encoding="utf-8", errors="replace") as fh:
        for raw in fh:
            line = raw.rstrip("\n")
            if not line or line.startswith("#"):
                continue
            c = line[0]
            if c.isdigit() or c in "+-*":
                if prof is None:
                    prof = Profile(event or "Ir", total)
                cost = _first_cost(line.split(), npos)
                if pending_call is not None:
                    callee, count = pending_call
                    rec = prof.calls[fn][callee]
                    rec[0] += count
                    rec[1] += cost
                    pending_call = None
                else:
                    prof.self_cost[fn] += cost
                continue
            key, _, value = line.partition("=")
            if key == "fn":
                fn = resolve(value)
            elif key == "cfn":
                callee = resolve(value)
                pending_call = (callee, 0)
            elif key == "calls":
                if pending_call is None:
                    sys.exit(f"{path}: 'calls=' without a preceding 'cfn=': {line!r}")
                pending_call = (pending_call[0], int(value.split()[0]))
            elif key in ("fl", "fi", "fe", "ob", "cob", "cfi", "cfl"):
                resolve(value)  # keep the compression table complete
            elif line.startswith("events:"):
                event = line.split()[1]
            elif line.startswith("positions:"):
                npos = len(line.split()) - 1
            elif line.startswith(("summary:", "totals:")):
                total = int(line.split()[1])
    if prof is None:
        sys.exit(f"{path}: no cost lines; not a callgrind file?")
    if not prof.total:
        prof.total = sum(prof.self_cost.values())
    return prof


def tool(name: str) -> str:
    if name == "valgrind" and os.environ.get("VALGRIND"):
        return os.environ["VALGRIND"]
    found = shutil.which(name)
    if not found:
        sys.exit(f"{name} not found on PATH (install valgrind, or set VALGRIND)")
    return found


def area(name: str) -> str:
    for label, pattern in AREAS:
        if re.search(pattern, name):
            return label
    return "other"


def pct(n: int, total: int) -> str:
    return f"{100 * n / total:5.1f}%" if total else "  n/a"


def short(name: str, width: int = 120) -> str:
    return name if len(name) <= width else name[: width - 3] + "..."


def is_openjd(name: str) -> bool:
    return bool(re.match(r"^<?openjd_", name)) and "{closure" not in name


def cmd_run(args: argparse.Namespace) -> int:
    if not args.command:
        sys.exit("run: give the command after '--'")
    proc = subprocess.run(
        [tool("valgrind"), "--tool=callgrind", f"--callgrind-out-file={args.out}", *args.command],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if not os.path.exists(args.out):
        sys.stderr.write(proc.stderr)
        sys.exit("valgrind wrote no profile")
    prof = load(args.out)
    print(f"exit {proc.returncode}; {prof.total:,} {prof.event} -> {args.out}")
    return 0


def cmd_report(args: argparse.Namespace) -> int:
    prof = load(args.out)
    total = prof.total
    print(f"total {prof.event}: {total:,}")
    areas = Counter()
    for name, cost in prof.self_cost.items():
        areas[area(name)] += cost
    print("\nself cost by area:")
    for label, cost in areas.most_common():
        print(f"  {pct(cost, total)}  {cost:>14,}  {label}")
    print(f"\ntop {args.top} functions by self cost:")
    for name, cost in prof.self_cost.most_common(args.top):
        print(f"  {pct(cost, total)}  {short(name)}")
    print(f"\ntop {args.top} openjd functions by inclusive cost:")
    inc = prof.inclusive()
    for name, cost in [(n, c) for n, c in inc.most_common() if is_openjd(n)][: args.top]:
        print(f"  {pct(cost, total)}  {short(name)}")
    allocs = Counter()
    for caller, callees in prof.calls.items():
        for callee, (n, _) in callees.items():
            if ALLOC_FN.match(callee):
                allocs[caller] += n
    if allocs:
        print(f"\nRust heap allocations: {sum(allocs.values()):,} calls; top callers:")
        for name, n in allocs.most_common(args.top):
            print(f"  {n:>10,}  {short(name)}")
    return 0


def cmd_diff(args: argparse.Namespace) -> int:
    old, new = load(args.old), load(args.new)
    if old.event != new.event:
        sys.exit(f"profiles count different events: {old.event} vs {new.event}")
    change = 100 * (new.total - old.total) / old.total if old.total else 0.0
    print(f"total {old.event}: {old.total:,} -> {new.total:,} ({change:+.1f}%)")
    a, b = old.inclusive(), new.inclusive()
    keep = re.compile(args.match) if args.match else None
    rows = []
    for name in set(a) | set(b):
        if keep.search(name) if keep else is_openjd(name):
            rows.append((b.get(name, 0) - a.get(name, 0), name))
    rows.sort()
    for title, subset in (
        ("largest inclusive decreases", rows[: args.top]),
        ("largest inclusive increases", [r for r in reversed(rows[-args.top :]) if r[0] > 0]),
    ):
        print(f"\n{title}:")
        for delta, name in subset:
            if delta == 0:
                continue
            print(
                f"  {delta:>+14,}  {a.get(name, 0):>13,} -> {b.get(name, 0):>13,}  {short(name, 90)}"
            )
    return 0


def cmd_callers(args: argparse.Namespace) -> int:
    prof = load(args.out)
    pattern = re.compile(args.regex)
    callees = sorted({c for cs in prof.calls.values() for c in cs} | set(prof.self_cost))
    matched = [c for c in callees if pattern.search(c)]
    if not matched:
        print(f"no function matches {args.regex!r}")
        return 1
    for callee in matched:
        print(f"== {short(callee)}  (self {prof.self_cost.get(callee, 0):,})")
        for caller, n, cost in prof.callers_of(callee)[: args.top]:
            print(f"  {cost:>14,} {prof.event}  {n:>9,}x  {short(caller, 100)}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Instruction-level profiles of openjd commands (callgrind)."
    )
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("run", help="profile a command")
    p.add_argument("out")
    p.add_argument("command", nargs=argparse.REMAINDER)
    p = sub.add_parser("report", help="where the instructions go")
    p.add_argument("out")
    p.add_argument("--top", type=int, default=20)
    p = sub.add_parser("diff", help="per-function change between two profiles")
    p.add_argument("old")
    p.add_argument("new")
    p.add_argument("--top", type=int, default=15)
    p.add_argument("--match", help="regex of functions to compare (default: openjd functions)")
    p = sub.add_parser("callers", help="who calls the matching functions")
    p.add_argument("out")
    p.add_argument("regex")
    p.add_argument("--top", type=int, default=10)
    args = ap.parse_args()
    if args.cmd == "run" and args.command[:1] == ["--"]:
        args.command = args.command[1:]
    return {"run": cmd_run, "report": cmd_report, "diff": cmd_diff, "callers": cmd_callers}[
        args.cmd
    ](args)


if __name__ == "__main__":
    sys.exit(main())
