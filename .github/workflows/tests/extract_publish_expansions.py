#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
"""Print the publish step's two filename-parsing expansions, space separated.

The publish job recovers a crate name and a run date out of an artifact filename
using bash parameter expansion. Reading the expansions back out of the workflow
means the test exercises what actually ships rather than a copy that can drift.
"""

from __future__ import annotations

import re
import sys

import yaml


def main() -> int:
    with open(sys.argv[1]) as fh:
        workflow = yaml.safe_load(fh)

    run = ""
    for step in workflow["jobs"]["publish"]["steps"]:
        body = step.get("run") or ""
        if "crate=${base%" in body:
            run = body
            break
    if not run:
        print("no publish step parses the filename", file=sys.stderr)
        return 1

    crate = re.search(r"crate=(\$\{base%[^}]*\})", run)
    date = re.search(r"date_part=(\$\{base##[^}]*\})", run)
    if not crate or not date:
        print("could not find both expansions", file=sys.stderr)
        return 1

    print(crate.group(1), date.group(1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
