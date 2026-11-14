#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
#
# Gate: every public function of the library crates is executed by the fuzz
# corpus, or explicitly excluded with a reason.
#
# "Public function" comes from the compiler, not a regex: rustdoc JSON lists
# every item reachable from each crate root, with its definition path.
# "Executed" comes from the compiler too: the committed corpus and regressions
# are replayed (`cargo test` in fuzz/) under llvm source-based coverage, and a
# function counts as fuzzed only if llvm's record for that function (matched
# by demangled definition path, any generic instantiation) has a nonzero
# execution count. Matching by name, not by source span, keeps functions that
# share a span (macro-generated methods) apart. Naming a function in a
# registry proves nothing; this check cannot be satisfied by a claim.
#
# Rules (all enforced):
#   1. Every public free function, inherent method, and method of a
#      hand-written (not derived) trait impl on a public type, in every
#      workspace library crate (see NOT_GATED), has a nonzero
#      execution count under corpus replay, unless it is listed in [exclude]
#      in fuzz/api_coverage.toml.
#   2. Every [exclude] entry is an exact function path (no globs, which would
#      silently exempt functions added later) naming a public function.
#   3. No excluded function is in fact covered; the exclusion is stale and
#      would hide a later loss of coverage.
#
# Usage:
#   scripts/check_fuzz_api_coverage.py --rustdoc-dir DIR --coverage cov.json
#   scripts/check_fuzz_api_coverage.py --generate   # build both inputs, then check
#
# --generate needs a nightly toolchain (rustdoc JSON) and cargo-llvm-cov.

import argparse
import json
import os
import subprocess
import sys
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
# Workspace library crates the gate does not cover, with the reason. Every
# other library crate in the workspace is covered, so a new crate is gated
# from its first public function.
NOT_GATED = {
    "openjd_for_js": "experimental wasm bindings over openjd-model / openjd-expr, "
    "which are gated themselves",
}


def workspace_libs() -> list[str]:
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            cwd=REPO,
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    )
    libs = sorted(
        t["name"]
        for p in meta["packages"]
        for t in p["targets"]
        if {"lib", "rlib"} & set(t["kind"])
    )
    stale = set(NOT_GATED) - set(libs)
    assert not stale, f"NOT_GATED names crates that are not workspace libraries: {stale}"
    return [c for c in libs if c not in NOT_GATED]


CRATES = workspace_libs()
POLICY = REPO / "fuzz" / "api_coverage.toml"


# Hand-written trait impls whose methods are not public behavior worth
# fuzzing on their own.
UNGATED_TRAITS = {"Debug", "Drop"}


def public_functions(doc: dict) -> dict[str, tuple[str, int, str]]:
    """Map 'crate::public::path::fn' -> (file, begin_line, definition_path)
    for every public free function and inherent method reachable from the
    crate root. A re-exported item is listed once, under its shortest public
    path."""
    index = doc["index"]
    paths = doc["paths"]
    # item id -> (public path, file, begin line, definition path)
    out: dict[int, tuple[str, str, int, str]] = {}
    seen: set = set()

    def add(path: str, item, defpath: str | None):
        s = item.get("span")
        if not s or item.get("crate_id") != 0 or not defpath:
            return
        prev = out.get(item["id"])
        if prev is None or len(path) < len(prev[0]):
            out[item["id"]] = (path, s["filename"], s["begin"][0], defpath)

    def defpath_of(item_id) -> str | None:
        p = paths.get(str(item_id))
        return "::".join(p["path"]) if p else None

    def methods(type_item, type_path: str):
        type_def = defpath_of(type_item["id"])
        inner = type_item["inner"]
        kind = next(iter(inner))
        for impl_id in inner[kind].get("impls", []):
            impl = index.get(str(impl_id))
            if not impl:
                continue
            body = impl["inner"]["impl"]
            if body.get("is_synthetic") or body.get("blanket_impl") is not None:
                continue
            trait = body.get("trait")
            if trait is not None:
                # Methods of a hand-written trait impl, as
                # `Type::<Trait>::method`. Derived impls are generated code.
                name = trait["path"].split("::")[-1]
                derived = any("automatically_derived" in str(a) for a in impl.get("attrs", []))
                # rustdoc also lists `impl From<A> for B` under `A`; take each
                # impl only under its own `for` type.
                for_id = (body.get("for") or {}).get("resolved_path", {}).get("id")
                if (
                    derived
                    or name in UNGATED_TRAITS
                    or impl.get("crate_id") != 0
                    or for_id != type_item["id"]
                ):
                    continue
                for mid in body["items"]:
                    m = index.get(str(mid))
                    if m and "function" in m["inner"]:
                        add(
                            f"{type_path}::<{name}>::{m['name']}",
                            m,
                            f"{type_def}::<{name}>::{m['name']}" if type_def else None,
                        )
                continue
            for mid in body["items"]:
                m = index.get(str(mid))
                if m and "function" in m["inner"] and m.get("visibility") == "public":
                    add(
                        f"{type_path}::{m['name']}",
                        m,
                        f"{type_def}::{m['name']}" if type_def else None,
                    )

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
            add(path, item, defpath_of(item_id))
        elif kind in ("struct", "enum", "union"):
            methods(item, path)

    root = doc["root"]
    walk(root, index[str(root)]["name"])
    return {path: (file, begin, defpath) for path, file, begin, defpath in out.values()}


def strip_generics(s: str) -> str:
    """Drop every balanced `<...>` group (generic arguments), then the `::`
    that introduced a turbofish."""
    out, depth = [], 0
    for c in s:
        if c == "<":
            depth += 1
        elif c == ">":
            depth -= 1
        elif depth == 0:
            out.append(c)
    r = "".join(out)
    while "::::" in r:
        r = r.replace("::::", "::")
    return r.removesuffix("::")


def definition_path(demangled: str) -> str | None:
    """`<crate::m::Type<T>>::method::<U>` -> `crate::m::Type::method`;
    `<crate::m::Type as a::b::Trait<X>>::f` -> `crate::m::Type::<Trait>::f`;
    `crate::m::f::<T>` -> `crate::m::f`. None for closures and shims."""
    if "{" in strip_generics(demangled):
        return None
    if demangled.startswith("<"):
        depth = 0
        for i, c in enumerate(demangled):
            depth += c == "<"
            depth -= c == ">"
            if depth == 0:
                break
        inner, rest = demangled[1:i], demangled[i + 1 :]
        top = strip_generics(inner)
        trait = None
        if " as " in top:
            top, trait_path = top.split(" as ", 1)
            trait = trait_path.split("::")[-1]
        if not top.replace("::", "").replace("_", "").isalnum():
            return None
        if trait is not None:
            top = f"{top}::<{trait}>"
        return top + strip_generics(rest)
    return strip_generics(demangled)


def executed_functions(cov: dict) -> set[str]:
    """Definition paths of every function llvm-cov saw execute."""
    fns = [fn for data in cov["data"] for fn in data["functions"]]
    demangled = subprocess.run(
        [
            "cargo", "run", "-q", "--manifest-path", "fuzz/Cargo.toml", "--no-default-features",
            "--example", "demangle",
        ],
        cwd=REPO,
        check=True,
        input="\n".join(fn["name"] for fn in fns) + "\n",
        capture_output=True,
        text=True,
    ).stdout.splitlines()
    assert len(demangled) == len(fns), "demangler output does not match its input"
    executed: set[str] = set()
    for d, fn in zip(demangled, fns):
        path = definition_path(d)
        if path is not None and fn["count"] > 0:
            executed.add(path)
    return executed


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
            # Only the corpus and regression replays count, not the harness
            # self-checks that run in the same test binary.
            "--", "replay_",
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
    executed = executed_functions(json.loads(args.coverage.read_text(encoding="utf-8")))

    functions: dict[str, tuple[str, int, str]] = {}
    for crate in CRATES:
        doc = json.loads((args.rustdoc_dir / f"{crate}.json").read_text(encoding="utf-8"))
        functions.update(public_functions(doc))

    failures = []
    pattern_hits = {p: 0 for p in excludes}
    rows = []
    n_cov = n_exc = 0
    for path in sorted(functions):
        file, begin, defpath = functions[path]
        # A function with no llvm record was never code-generated (an
        # uninstantiated generic, or dead code), so it was not executed.
        covered = defpath in executed
        excluded = [p for p in excludes if p == path]
        for p in excluded:
            pattern_hits[p] += 1
        if excluded:
            n_exc += 1
            status = "excluded"
            if covered:
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
        if any(c in p for c in "*?["):
            failures.append(f"exclusions must be exact function paths, not patterns: {p!r}")
        elif hits == 0:
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
