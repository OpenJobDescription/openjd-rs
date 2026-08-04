#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
#
# Hard gate: every public function that takes untrusted input (a &str, &[u8],
# serde_json::Value, or &Path parameter) must be accounted for in
# fuzz/fuzz_coverage.toml — either mapped to a fuzz target that exercises it, or
# explicitly classified as not-untrusted with a reason. This makes "all
# untrusted inputs are fuzzed" an executable, non-rotting check instead of a
# prose convention.
#
# It enforces four invariants:
#   1. No orphan targets    — every fuzz/fuzz_targets/*.rs is listed in [targets].
#   2. No phantom targets    — every [targets] entry has a matching target file.
#   3. No dangling entries    — every classified function still exists in source.
#   4. Ratchet (completeness) — every input-shaped public fn is classified, so a
#      newly-added untrusted entry point fails CI until a human triages it.
#
# It deliberately does NOT verify that a target *meaningfully* exercises a
# function (that needs coverage instrumentation) or that a not-untrusted reason
# is honest (that is the one human-reviewed line per entry). It runs on stable
# Rust with no build — pure source + manifest analysis — so it fits the fast
# Compliance CI job alongside the copyright-header check.
#
# Usage: scripts/check_fuzz_coverage.py

import re
import sys
import tomllib
from pathlib import Path

# Crates whose public API is security-relevant (parse/decode/evaluate untrusted
# input). openjd-cli is a bin-only crate (no lib target) and openjd-for-js is a
# thin wasm shim over these crates, so neither exposes a fuzzable library API.
CRATES = ["openjd-expr", "openjd-model", "openjd-snapshots"]

# Parameter-type fragments that indicate a function consumes externally-shaped
# input. Anything taking bytes, text, parsed JSON, or a filesystem path.
INPUT_TYPE_MARKERS = ["&str", "&[u8]", "serde_json::Value", "&Path", "&std::path::Path"]

REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST = REPO_ROOT / "fuzz" / "fuzz_coverage.toml"
TARGET_DIR = REPO_ROOT / "fuzz" / "fuzz_targets"

# `pub fn name` — not `pub(crate) fn` (has no bare " fn "), not private fns.
# Only the `pub fn <name>` prefix is matched here; the optional generic block
# and the arg-list paren are located by `find_arg_paren` with bracket
# balancing, so signatures whose generics contain nested angle brackets
# (e.g. `foo<T: Into<String>>(input: &str)`) are handled — a regex like
# `<[^>]*>` would stop at the first `>` and silently skip such a function,
# letting it escape the ratchet.
FN_START = re.compile(r"\bpub\s+fn\s+([a-z_][a-z0-9_]*)")


def public_module_files(crate_src: Path) -> list[Path]:
    """Return .rs files reachable through `pub mod` from lib.rs.

    A `pub fn` inside a module that is not itself fully public (e.g. snapshots'
    `mod path_util`, or expr's `pub(crate) mod edit_distance`) is not public
    API, so its functions must not count toward the untrusted surface. We
    resolve visibility at the top level (the common case, and the one that has
    actually bitten us) by reading lib.rs's module declarations; files under a
    non-public top-level module are excluded.

    Only a bare `pub mod foo;` counts as public. Any restricted visibility —
    `pub(crate)`, `pub(super)`, `pub(in path)` — is narrower than `pub` and so
    is treated as private. Matching the restriction explicitly matters: a
    `(pub\\s+)?` prefix would fail to match `pub(crate) mod foo;` at all
    (there is no whitespace after `pub`), leaving the module out of
    `private_tops` and silently pulling its crate-internal `pub fn`s into the
    scanned surface.
    """
    lib = crate_src / "lib.rs"
    text = lib.read_text(encoding="utf-8", errors="replace")
    private_tops = set()
    mod_decl = re.compile(r"^\s*(pub\s*(\([^)]*\))?\s+)?mod\s+([a-z_][a-z0-9_]*)\s*;", re.M)
    for m in mod_decl.finditer(text):
        # No `pub` at all, or a `pub(...)` visibility restriction → private.
        if not m.group(1) or m.group(2):
            private_tops.add(m.group(3))

    files = []
    for rs in sorted(crate_src.rglob("*.rs")):
        rel = rs.relative_to(crate_src)
        top = rel.parts[0]
        # `foo.rs` or `foo/…` where `foo` is a private top-level module → skip.
        stem_top = top[:-3] if top.endswith(".rs") else top
        if stem_top in private_tops:
            continue
        files.append(rs)
    return files


def find_arg_paren(text: str, after_name_idx: int) -> int | None:
    """Given the index just past a `pub fn <name>`, return the index of the
    arg-list opening `(`, or None if the shape is unexpected.

    Skips an optional generic block `<...>` with full angle-bracket balancing
    (so nested generics like `<T: Into<String>>` are handled) plus surrounding
    whitespace. `->` inside a bound (e.g. `<F: Fn(&str) -> bool>`) is treated as
    a token so its `>` does not close the block early. The first `(` at
    generic-depth 0 is the arg list.
    """
    i = after_name_idx
    n = len(text)
    while i < n and text[i].isspace():
        i += 1
    if i < n and text[i] == "<":
        depth = 0
        while i < n:
            c = text[i]
            # Skip `->` so the `>` of a return-type arrow inside a bound (e.g.
            # `Fn(&str) -> bool`) is not counted as closing the generic block.
            if c == "-" and i + 1 < n and text[i + 1] == ">":
                i += 2
                continue
            if c == "<":
                depth += 1
            elif c == ">":
                depth -= 1
                if depth == 0:
                    i += 1
                    break
            i += 1
        while i < n and text[i].isspace():
            i += 1
    if i < n and text[i] == "(":
        return i
    return None


def extract_signature(text: str, fn_start_idx: int, paren_idx: int) -> str:
    """Return the normalized `pub fn …(…)` slice, balancing parens."""
    depth = 0
    i = paren_idx
    while i < len(text):
        c = text[i]
        if c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                break
        i += 1
    return " ".join(text[fn_start_idx : i + 1].split())


def input_shaped_functions() -> dict[str, str]:
    """Map "crate-relative-file::fn_name" -> normalized signature, for every
    input-shaped public function across the security-relevant crates."""
    found: dict[str, str] = {}
    for crate in CRATES:
        crate_src = REPO_ROOT / "crates" / crate / "src"
        for rs in public_module_files(crate_src):
            text = rs.read_text(encoding="utf-8", errors="replace")
            # Skip #[cfg(test)] modules: crude but effective — drop everything
            # from a `mod tests {` onward is unsafe (nested), so instead skip
            # files whose path is a test module. Test fns are rarely `pub` and
            # never take these types as public API, so residual noise is nil.
            for m in FN_START.finditer(text):
                paren = find_arg_paren(text, m.end())
                if paren is None:
                    continue
                sig = extract_signature(text, m.start(), paren)
                # Normalize explicit lifetimes out of reference types before
                # marker-matching, so `&'a str` / `&'de [u8]` are recognized the
                # same as `&str` / `&[u8]`. Without this, a lifetime-annotated
                # parameter (e.g. `job_template_dir: &'a str`) would silently
                # escape classification and defeat the ratchet.
                normalized = re.sub(r"&\s*'[a-z_][a-z0-9_]*\s+", "&", sig)
                if any(marker in normalized for marker in INPUT_TYPE_MARKERS):
                    rel = rs.relative_to(REPO_ROOT).as_posix()
                    found[f"{rel}::{m.group(1)}"] = sig
    return found


def target_files() -> set[str]:
    return {p.stem for p in TARGET_DIR.glob("*.rs")}


def main() -> int:
    if not MANIFEST.exists():
        print(f"error: manifest not found: {MANIFEST}", file=sys.stderr)
        return 1

    manifest = tomllib.loads(MANIFEST.read_text(encoding="utf-8"))
    declared_targets = set(manifest.get("targets", {}).keys())
    fuzzed = {e["fn"]: e for e in manifest.get("fuzzed", [])}
    not_untrusted = {e["fn"]: e for e in manifest.get("not_untrusted", [])}
    classified = set(fuzzed) | set(not_untrusted)

    errors: list[str] = []

    # (1)/(2) target files ↔ [targets] table.
    actual_targets = target_files()
    for orphan in sorted(actual_targets - declared_targets):
        errors.append(
            f"fuzz target '{orphan}' exists in fuzz/fuzz_targets/ but is not "
            f"listed in [targets] of {MANIFEST.name}"
        )
    for phantom in sorted(declared_targets - actual_targets):
        errors.append(
            f"[targets] lists '{phantom}' but fuzz/fuzz_targets/{phantom}.rs "
            f"does not exist"
        )

    # fuzzed[].target must name a real target.
    for fn, entry in sorted(fuzzed.items()):
        tgt = entry.get("target")
        if tgt not in declared_targets:
            errors.append(
                f"fuzzed entry '{fn}' names target '{tgt}', which is not in "
                f"[targets]"
            )

    # An entry can't be both fuzzed and not-untrusted.
    for fn in sorted(set(fuzzed) & set(not_untrusted)):
        errors.append(f"'{fn}' is listed as both fuzzed and not_untrusted")

    surface = input_shaped_functions()

    # (3) No dangling: every classified fn still exists in source.
    for fn in sorted(classified - set(surface)):
        errors.append(
            f"manifest classifies '{fn}' but no such input-shaped public "
            f"function exists (renamed, removed, or made private?)"
        )

    # (4) Ratchet: every input-shaped public fn is classified.
    for fn in sorted(set(surface) - classified):
        errors.append(
            f"input-shaped public function '{fn}' is not classified in "
            f"{MANIFEST.name}\n      {surface[fn]}\n      → add it to [[fuzzed]] "
            f"(with the target that exercises it) or [[not_untrusted]] (with a "
            f"reason it does not take untrusted input)"
        )

    if errors:
        print("Fuzz coverage check failed:\n", file=sys.stderr)
        for e in errors:
            print(f"  - {e}", file=sys.stderr)
        print(
            f"\n{len(surface)} input-shaped public functions scanned; "
            f"{len(classified)} classified.",
            file=sys.stderr,
        )
        return 1

    print(
        f"Fuzz coverage check passed: {len(surface)} input-shaped public "
        f"functions, all classified ({len(fuzzed)} fuzzed, "
        f"{len(not_untrusted)} not-untrusted); {len(actual_targets)} fuzz targets."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
