#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
#
# Gate: every public function of the library crates is executed by the fuzz
# corpus, or explicitly excluded with a reason.
#
# "Public function" comes from the compiler, not a regex: rustdoc JSON lists
# every item reachable from each crate root, with exact spans. "Executed"
# comes from the compiler too: the committed corpus and regressions are
# replayed (`cargo test` in fuzz/) under llvm source-based coverage, and an
# item counts as fuzzed only if code inside its span ran. Naming a function in
# a registry proves nothing; this check cannot be satisfied by a claim.
#
# Rules (all enforced):
#   1. Every public free function and inherent method of openjd-expr,
#      openjd-model, openjd-sessions, and openjd-snapshots has a nonzero
#      execution count under corpus replay, unless it matches an [exclude]
#      pattern in fuzz/api_coverage.toml.
#   2. Every [exclude] pattern matches at least one public function (no
#      stale exclusions).
#   3. No function excluded by an exact (non-glob) entry is in fact covered;
#      the exclusion is stale and would hide a later loss of coverage.
#
# Usage:
#   scripts/check_fuzz_api_coverage.py --rustdoc-dir DIR --coverage cov.json
#   scripts/check_fuzz_api_coverage.py --generate   # build both inputs, then check
#
# --generate needs a nightly toolchain (rustdoc JSON) and cargo-llvm-cov.

import argparse
import fnmatch
import json
import os
import subprocess
import sys
import tomllib
from collections import defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CRATES = ["openjd_expr", "openjd_model", "openjd_sessions", "openjd_snapshots"]
POLICY = REPO / "fuzz" / "api_coverage.toml"


def public_functions(doc: dict) -> dict[str, tuple[str, int, int]]:
    """Map 'crate::path::fn' -> (file, begin_line, end_line) for every public
    free function and inherent method reachable from the crate root."""
    index = doc["index"]
    out: dict[str, tuple[str, int, int]] = {}
    seen: set = set()

    def span(item):
        s = item.get("span")
        if not s:
            return None
        return (s["filename"], s["begin"][0], s["end"][0])

    def add(path: str, item):
        sp = span(item)
        if sp and item.get("crate_id") == 0:
            out.setdefault(path, sp)

    def methods(type_item, type_path: str):
        inner = type_item["inner"]
        kind = next(iter(inner))
        for impl_id in inner[kind].get("impls", []):
            impl = index.get(str(impl_id))
            if not impl:
                continue
            body = impl["inner"]["impl"]
            if body.get("trait") is not None or body.get("is_synthetic"):
                continue
            for mid in body["items"]:
                m = index.get(str(mid))
                if m and "function" in m["inner"] and m.get("visibility") == "public":
                    add(f"{type_path}::{m['name']}", m)

    def walk(item_id, path: str):
        key = (item_id, path)
        if key in seen:
            return
        seen.add(key)
        item = index.get(str(item_id))
        if not item:
            return
        inner = item["inner"]
        kind = next(iter(inner))
        if kind == "module":
            for child in inner["module"]["items"]:
                c = index.get(str(child))
                if not c or c.get("visibility") != "public":
                    continue
                ck = next(iter(c["inner"]))
                if ck == "use":
                    u = c["inner"]["use"]
                    if u.get("id") is None:
                        continue
                    target = index.get(str(u["id"]))
                    if not target:
                        continue
                    name = u["name"]
                    if u.get("is_glob"):
                        walk(u["id"], path)
                    else:
                        walk(u["id"], f"{path}::{name}")
                else:
                    walk(child, f"{path}::{c['name']}")
        elif kind == "function":
            add(path, item)
        elif kind in ("struct", "enum", "union"):
            methods(item, path)

    root = doc["root"]
    walk(root, index[str(root)]["name"])
    # A re-exported item is reachable under several paths; keep the shortest
    # (the crate-root re-export) so each function is listed once.
    by_span: dict[tuple[str, int, int], str] = {}
    for path, sp in out.items():
        if sp not in by_span or len(path) < len(by_span[sp]):
            by_span[sp] = path
    return {path: sp for sp, path in by_span.items()}


def covered_lines(cov: dict) -> dict[str, list[tuple[int, int, int]]]:
    """Map repo-relative file -> [(start_line, end_line, count)] for every
    function llvm-cov reports, instantiations summed by region."""
    out: dict[str, list[tuple[int, int, int]]] = defaultdict(list)
    for data in cov["data"]:
        for fn in data["functions"]:
            files = fn["filenames"]
            for r in fn["regions"]:
                # [line_start, col_start, line_end, col_end, count, file_id, expanded_file_id, kind]
                ls, _, le, _, count, file_id = r[:6]
                f = files[file_id]
                try:
                    rel = str(Path(f).resolve().relative_to(REPO))
                except ValueError:
                    continue
                out[rel].append((ls, le, count))
    return out


def is_covered(regions, begin: int, end: int) -> bool:
    return any(count > 0 and begin <= ls <= end for ls, _, count in regions)


def generate(out_dir: Path) -> tuple[Path, Path]:
    toolchain = os.environ.get("FUZZ_TOOLCHAIN", "nightly-2026-10-07")
    rd = out_dir / "rustdoc"
    for crate in CRATES:
        subprocess.run(
            [
                "cargo", f"+{toolchain}", "rustdoc", "-q", "-p", crate.replace("_", "-"),
                "--lib", "--", "-Z", "unstable-options", "--output-format", "json",
            ],
            cwd=REPO,
            check=True,
            env={**os.environ, "CARGO_TARGET_DIR": str(rd)},
        )
    cov = out_dir / "coverage.json"
    subprocess.run(
        [
            "cargo", "llvm-cov", "--manifest-path", "fuzz/Cargo.toml", "--no-default-features",
            # The crates under test are path dependencies of the fuzz crate,
            # not members of its workspace, so name them explicitly.
            "--dep-coverage", ",".join(c.replace("_", "-") for c in CRATES),
            "--json", "--output-path", str(cov), "--test", "replay",
        ],
        cwd=REPO,
        check=True,
    )
    return rd / "doc", cov


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--rustdoc-dir", type=Path)
    ap.add_argument("--coverage", type=Path)
    ap.add_argument("--generate", action="store_true")
    ap.add_argument("--work-dir", type=Path, default=REPO / "fuzz" / "target" / "api-coverage")
    ap.add_argument("--report", action="store_true", help="list every function with its status")
    args = ap.parse_args()

    if args.generate:
        args.work_dir.mkdir(parents=True, exist_ok=True)
        args.rustdoc_dir, args.coverage = generate(args.work_dir)
    if not args.rustdoc_dir or not args.coverage:
        ap.error("need --rustdoc-dir and --coverage, or --generate")

    policy = tomllib.loads(POLICY.read_text(encoding="utf-8"))
    excludes: dict[str, str] = policy.get("exclude", {})
    regions = covered_lines(json.loads(args.coverage.read_text(encoding="utf-8")))

    functions: dict[str, tuple[str, int, int]] = {}
    for crate in CRATES:
        doc = json.loads((args.rustdoc_dir / f"{crate}.json").read_text(encoding="utf-8"))
        functions.update(public_functions(doc))

    failures = []
    pattern_hits = {p: 0 for p in excludes}
    rows = []
    n_cov = n_exc = 0
    for path in sorted(functions):
        file, begin, end = functions[path]
        covered = is_covered(regions.get(file, []), begin, end)
        excluded = [p for p in excludes if fnmatch.fnmatchcase(path, p)]
        for p in excluded:
            pattern_hits[p] += 1
        if excluded:
            n_exc += 1
            status = "excluded"
            exact = [p for p in excluded if not any(c in p for c in "*?[")]
            if covered and exact:
                failures.append(
                    f"excluded but executed by the fuzz corpus (drop the exclusion): {path}"
                )
        elif covered:
            n_cov += 1
            status = "fuzzed"
        else:
            status = "MISSING"
            failures.append(f"not executed by the fuzz corpus: {path} ({file}:{begin})")
        rows.append((status, path, f"{file}:{begin}"))

    for p, hits in pattern_hits.items():
        if hits == 0:
            failures.append(f"stale exclusion matches no public function: {p!r}")

    total = len(functions)
    if args.report:
        for status, path, loc in rows:
            print(f"{status:9} {path}  {loc}")
    print(
        f"{total} public functions: {n_cov} executed by the fuzz corpus, "
        f"{n_exc} excluded with a reason, {total - n_cov - n_exc} missing."
    )
    if failures:
        print("\n".join(failures), file=sys.stderr)
        print(
            "\nEither extend a fuzz target (and its corpus) to reach the function, or add an "
            "[exclude] entry with a reason to fuzz/api_coverage.toml.",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
