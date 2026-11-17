#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
"""Generate crates/openjd-expr/tests/integration/data/round_cpython.txt.

`round(x, ndigits)` rounds half-to-even on the exact value of its input, as
CPython's `round` does (Expression Language §2.2.2), and an `int` result
outside the 64-bit signed range is an integer overflow error. This script
records CPython's answers for a fixed set of inputs so the Rust tests compare
against CPython without running Python.

Run it with a current CPython (3.12 or later):

    uv run --python 3.12 python scripts/generate_round_cpython_fixture.py

The output file is checked in. The seed is fixed, so a rerun on the same
CPython gives the same file.

Each data line is one of

    i <x> <ndigits> int <result>
    i <x> <ndigits> overflow
    f <IEEE 754 bits of x as 16 hex digits> <repr(x)> <ndigits> int <result>
    f <bits> <repr(x)> <ndigits> overflow
    f <bits> <repr(x)> <ndigits> float <bits of the result> <text>

For `ndigits <= 0` the result is the exact value of `x` rounded
half-to-even to a multiple of `10**-ndigits`, computed with `fractions`.
CPython's `round(x, ndigits)` rounds the same value but returns the nearest
float to it; the script checks the two agree. `overflow` means the integer
result is outside -2**63 .. 2**63-1. For
`ndigits > 0` the result is a float, and `<text>` is `f"{x:.{ndigits}f}"`,
the exact value of `x` rounded half-to-even to `ndigits` places, which is the
text openjd displays. A negative zero is written without its sign, because
the expression language has no signed zero.
"""

import math
import random
import struct
from fractions import Fraction
from pathlib import Path

OUT = (
    Path(__file__).resolve().parent.parent
    / "crates/openjd-expr/tests/integration/data/round_cpython.txt"
)
I64_MIN = -(2**63)
I64_MAX = 2**63 - 1


def bits(x: float) -> str:
    return struct.pack(">d", x).hex()


def int_result(r: int) -> str:
    return f"int {r}" if I64_MIN <= r <= I64_MAX else "overflow"


def float_line(x: float, n: int) -> str:
    head = f"f {bits(x)} {x!r} {n}"
    if n <= 0:
        # The exact rational value of x rounded half-to-even. CPython's
        # round(x, n) rounds the same exact value but returns a float, so
        # an integer it cannot represent comes back as the nearest double:
        # round(-2.0**63, -1) is -9223372036854775810 exactly, which is out
        # of range, while CPython's float result is -2.0**63. Where the
        # float is exact, the two agree, which the check below confirms.
        r = int(round(Fraction(x), n))
        try:
            f = round(x, n)
            assert f == r or float(r) == f, (x, n, r, f)
        except OverflowError:
            # The exact result exceeds the float range.
            assert abs(r) > I64_MAX
        return f"{head} {int_result(r)}"
    r = round(x, n)
    text = f"{x:.{n}f}"
    if text.startswith("-") and float(text) == 0.0:
        text = text[1:]
        r = 0.0
    assert float(text) == r, (x, n, text, r)
    return f"{head} float {bits(r)} {text}"


def int_line(x: int, n: int) -> str:
    assert I64_MIN <= x <= I64_MAX, x
    return f"i {x} {n} {int_result(round(x, n))}"


def main() -> None:
    rng = random.Random(56)
    lines = []

    # --- float, ndigits <= 0 -------------------------------------------
    crafted_float = [
        (5e300, -301), (4e300, -301), (-5e300, -301), (1e300, -301),
        (9.3e18, -18), (-9.3e18, -18), (9.2e18, -17), (9.2e18, -18),
        (9.2e18, -19), (9.5e18, -18), (-9.5e18, -18), (4.9e18, -19),
        (5e18, -19), (-5e18, -19), (5.000000000000001e18, -19),
        (6e19, -20), (4e19, -20), (5e19, -20), (1.5e19, -19),
        (3.5506936090295e18, -6), (3.5506936090295e18, 0),
        (2.0**63, 0), (-(2.0**63), 0), (2.0**63, -1), (-(2.0**63), -1),
        (2.0**63 - 1024, 0), (2.0**63 - 1024, -1), (-(2.0**63) + 1024, 0),
        (2.0**127, 0), (2.0**127, -38), (2.0**127, -39), (-(2.0**127), -39),
        (2.0**127 - 2.0**74, -38), (2.0**127 - 2.0**74, -39),
        (2.0**127 - 2.0**74, 0), (1e300, 0), (1e300, -1),
        (1.7e308, -308), (1.7e308, -309), (4.9e307, -308),
        (0.5, 0), (1.5, 0), (2.5, 0), (-0.5, 0), (-1.5, 0), (-2.5, 0),
        (0.49999999999999994, 0), (4503599627370497.0, 0),
        (1234.5, -1), (1250.0, -2), (1350.0, -2), (-1250.0, -2),
        (1250.0000000000002, -2), (0.4, -1), (5.0, -1), (15.0, -1),
        (25.0, -1), (25.000000000000004, -1),
    ]
    for x, n in crafted_float:
        lines.append(float_line(x, n))
    for _ in range(400):
        e = rng.randint(-3, 130)
        x = rng.uniform(1, 10) * 10.0**e
        if rng.random() < 0.5:
            x = -x
        digits = max(1, len(str(int(abs(x)))))
        n = -rng.randint(max(0, digits - 3), digits + 2)
        lines.append(float_line(x, n))
    # Values that are near ties in decimal but not exactly ties in binary.
    for _ in range(200):
        k = rng.randint(1, 22)
        m = rng.randint(0, 10**rng.randint(1, 19))
        x = float(m * 10**k + 5 * 10 ** (k - 1))
        if rng.random() < 0.5:
            x = -x
        lines.append(float_line(x, -k))

    # --- float, ndigits > 0 ---------------------------------------------
    crafted_pos = [
        (2.675, 2), (0.125, 2), (0.375, 2), (0.25, 1), (0.75, 1),
        (-0.125, 2), (-2.675, 2), (1.005, 2), (0.0004, 2), (-0.0004, 2),
        (3.5, 2), (1e308, 2), (1.7976931348623157e308, 1),
        (5e-324, 323), (5e-324, 324), (5e-324, 1074), (0.1, 17),
        (0.1, 30), (2.5, 1), (1e22, 3), (123456.785, 2),
    ]
    for x, n in crafted_pos:
        lines.append(float_line(x, n))
    for _ in range(300):
        e = rng.randint(-8, 20)
        x = rng.uniform(1, 10) * 10.0**e
        if rng.random() < 0.5:
            x = -x
        lines.append(float_line(x, rng.randint(1, 20)))
    for _ in range(200):
        # A decimal tie such as 0.xxx5, which binary usually misses.
        n = rng.randint(1, 8)
        x = float(f"{rng.randint(0, 10**n)}5e-{n + 1}")
        lines.append(float_line(x, n))

    # --- int, ndigits < 0 -----------------------------------------------
    crafted_int = [
        (I64_MAX, -1), (I64_MIN, -1), (I64_MAX, -18), (I64_MIN, -18),
        (I64_MAX, -19), (I64_MIN, -19), (I64_MAX, -20), (I64_MIN, -20),
        (3550693609029500000, -6), (4999999999999999999, -19),
        (5000000000000000000, -19), (5000000000000000001, -19),
        (-5000000000000000000, -19), (-5000000000000000001, -19),
        (9223372036854775805, -1), (9223372036854775804, -1),
        (-9223372036854775805, -1), (9150000000000000000, -17),
        (9050000000000000000, -17), (150, -2), (250, -2), (155, -1),
        (-155, -1), (11, -1), (0, -5), (1, -1), (5, -1), (15, -1),
    ]
    for x, n in crafted_int:
        lines.append(int_line(x, n))
    for _ in range(400):
        x = rng.randint(I64_MIN, I64_MAX) >> rng.randint(0, 62)
        lines.append(int_line(x, -rng.randint(1, 20)))
    for _ in range(200):
        k = rng.randint(1, 18)
        q = 10**k
        x = (rng.randint(I64_MIN // q, I64_MAX // q)) * q + q // 2
        if I64_MIN <= x <= I64_MAX:
            lines.append(int_line(x, -k))

    assert all(math.isfinite(float(line.split()[2])) for line in lines if line[0] == "f")
    header = (
        "# Generated by scripts/generate_round_cpython_fixture.py; see that\n"
        "# script for the format. Do not edit by hand.\n"
    )
    OUT.write_text(header + "\n".join(lines) + "\n")
    print(f"wrote {len(lines)} cases to {OUT}")


if __name__ == "__main__":
    main()
