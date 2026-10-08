#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
"""
Keep the Python packages' pins on each other in step with their versions.

Each Python package under python/ takes its version from the `version` in its
Cargo.toml, which release-plz bumps. When a package depends on another in-repo
package (openjd-cli on openjd-sessions and openjd-model, openjd-sessions on
openjd-model), its pyproject.toml pin must admit the version being released
alongside it. This script rewrites those pins to

    "<name> >= <in-repo version>,< <next breaking version>"

where the next breaking version is the next minor for 0.x and the next major
otherwise, matching Cargo's caret requirements.

Usage:
    scripts/sync_python_pins.py           # rewrite pins in place
    scripts/sync_python_pins.py --check   # exit 1 if any pin is out of date

The release-plz workflow runs it on the Release PR branch after release-plz
bumps versions, and CI runs --check on every pull request.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
PYTHON_DIR = REPO_ROOT / "python"

# Python distribution name -> package directory under python/.
PACKAGES = {
    "openjd-model": "openjd-model",
    "openjd-sessions": "openjd-sessions",
    "openjd-cli": "openjd-cli",
}

_CARGO_VERSION = re.compile(r'(?m)^version = "(?P<version>[^"]+)"')


def cargo_version(package_dir: str) -> str:
    text = (PYTHON_DIR / package_dir / "Cargo.toml").read_text(encoding="utf-8")
    match = _CARGO_VERSION.search(text)
    if match is None:
        raise SystemExit(f"error: no version in python/{package_dir}/Cargo.toml")
    return match.group("version")


def pin_for(name: str, version: str) -> str:
    major, minor = (int(part) for part in version.split(".")[:2])
    upper = f"0.{minor + 1}" if major == 0 else f"{major + 1}"
    return f'"{name} >= {version},< {upper}"'


def sync(check: bool) -> int:
    versions = {name: cargo_version(directory) for name, directory in PACKAGES.items()}
    stale = []
    for directory in PACKAGES.values():
        path = PYTHON_DIR / directory / "pyproject.toml"
        text = path.read_text(encoding="utf-8")
        updated = text
        for name, version in versions.items():
            # Matches a quoted requirement on `name` (and not on a longer name
            # that starts with it) inside the dependencies array.
            pattern = re.compile(rf'"{re.escape(name)}\s*[<>=!~][^"]*"')
            updated = pattern.sub(pin_for(name, version), updated)
        if updated != text:
            stale.append(path.relative_to(REPO_ROOT))
            if not check:
                path.write_text(updated, encoding="utf-8")

    if not stale:
        print("Python package pins are up to date.")
        return 0
    if check:
        print("These files have pins on in-repo Python packages that don't match", file=sys.stderr)
        print("those packages' current versions:", file=sys.stderr)
        for path in stale:
            print(f"  {path}", file=sys.stderr)
        print("Run scripts/sync_python_pins.py to update them.", file=sys.stderr)
        return 1
    for path in stale:
        print(f"Updated {path}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--check", action="store_true", help="report stale pins without writing")
    return sync(check=parser.parse_args().check)


if __name__ == "__main__":
    sys.exit(main())
