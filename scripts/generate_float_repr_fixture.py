#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
"""Generate crates/openjd-expr/tests/integration/data/float_repr_cpython.txt.

The expression language writes a float as the shortest decimal string that
reads back as the same binary value (Expression Language §1.3.4), and that
text is CPython's `repr(float)`. This script records `repr` for a fixed set of
doubles so the Rust tests compare against CPython without running Python.

Run it with a current CPython (3.12 or later):

    uv run --python 3.12 python scripts/generate_float_repr_fixture.py

The output file is checked in. Regenerate it only to add values; the seed is
fixed, so a rerun on the same CPython gives the same file.

Each data line is

    <IEEE 754 bits as 16 hex digits> <repr(float)> [<openjd text>]

The optional third column is an intended difference from `repr`: the text
openjd writes instead. There is exactly one, negative zero. The expression
language normalizes -0.0 to 0.0 and never keeps the sign, so openjd writes
`0.0` where `repr` writes `-0.0`. Every other value must match `repr`
exactly.
"""

import math
import random
import struct
import sys
from pathlib import Path

OUT = (
    Path(__file__).resolve().parent.parent
    / "crates/openjd-expr/tests/integration/data/float_repr_cpython.txt"
)

# The values openjd writes differently from `repr`, with the text it writes.
OPENJD_EXCEPTIONS = {struct.unpack("<Q", struct.pack("<d", -0.0))[0]: "0.0"}


def bits(x: float) -> int:
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def from_bits(b: int) -> float:
    return struct.unpack("<d", struct.pack("<Q", b))[0]


def steps_around(x: float, n: int) -> list[float]:
    """x and the n doubles on each side of it."""
    out = [x]
    lo = hi = x
    for _ in range(n):
        lo = math.nextafter(lo, -math.inf)
        hi = math.nextafter(hi, math.inf)
        out += [lo, hi]
    return out


def has_shortest_tie(x: float) -> bool:
    """Whether a second string with as few digits as repr(x) reads back as x.

    Then the shortest digits are a tie, and `repr` takes the one nearer the
    exact value, rounding half to even. Rust's `{}` rounds those ties up.
    """
    text = repr(x)
    if "e" in text or "." not in text:
        return False
    head, last = text[:-1], text[-1]
    for alt in (int(last) - 1, int(last) + 1):
        if 0 <= alt <= 9 and float(head + str(alt)) == x:
            return True
    return False


def values() -> list[float]:
    rng = random.Random(20261113)
    vals: list[float] = [0.0, -0.0]

    # Integer values: small ones, powers of two and ten, and the 2**53 edge.
    vals += [float(i) for i in range(1, 101)]
    vals += [float(2**k) for k in range(0, 1024)]
    vals += [float(10**k) for k in range(0, 23)]
    vals += steps_around(float(2**53), 3)
    vals += [float(n) for n in (123456789, 9007199254740993, 123456789012345678)]
    vals += [float(rng.randrange(1, 10**17)) for _ in range(300)]

    # Every finite, nonzero power of ten, and the doubles beside each.
    for k in range(-330, 310):
        x = float(f"1e{k}")
        if x != 0.0 and math.isfinite(x):
            vals += steps_around(x, 1)

    # Where repr switches between fixed and exponent notation: below 1e-4 and
    # at 1e16. Walk a few doubles each side, plus the neighbouring decades.
    for x in (1e-5, 1e-4, 1e-3, 1e15, 1e16, 1e17, 9999999999999998.0):
        vals += steps_around(x, 4)
    vals += [0.00009999999999999999, 0.0001000000000000001, 1.5e-4, 9.5e-5]
    vals += [1.5e15, 1.5e16, 1234567890123456.7, 12345678901234567.0]

    # Subnormals and the smallest and largest magnitudes.
    vals += steps_around(5e-324, 3)
    vals += steps_around(sys.float_info.min, 3)
    vals += steps_around(from_bits(0x000FFFFFFFFFFFFF), 2)  # largest subnormal
    vals += steps_around(sys.float_info.max, 3)
    vals += [from_bits(rng.randrange(1, 1 << 52)) for _ in range(200)]

    # Ties between two shortest digit strings (1000000000000000.25 is written
    # 1000000000000000.2). They need a fraction finer than the last digit, so
    # look near 2**40 to 2**53.
    ties: list[float] = []
    while len(ties) < 300:
        base = 2.0 ** rng.randrange(40, 54)
        x = base + rng.randrange(0, 1 << 20) * math.ulp(base)
        if has_shortest_tie(x):
            ties.append(x)
    vals += ties

    # Familiar decimals whose binary values are inexact.
    vals += [0.1, 0.2, 0.3, 0.1 + 0.2, 1 / 3, 2 / 3, math.pi, math.e, 1.1, 2.675]
    vals += [0.5, 1.5, 2.5, 100.25, 1e-7, 1.25e-5, 123.456, 3.14159, 6.02214076e23]

    # Short decimals, the kind people write in templates.
    for _ in range(1000):
        digits = rng.randrange(1, 18)
        mantissa = rng.randrange(1, 10**digits)
        exponent = rng.randrange(-30, 30)
        vals.append(float(f"{mantissa}e{exponent}"))
    for _ in range(300):
        vals.append(round(rng.uniform(0, 10 ** rng.randrange(0, 8)), rng.randrange(0, 7)))

    # Random finite doubles over the whole range.
    while len(vals) < 6000:
        x = from_bits(rng.getrandbits(64))
        if math.isfinite(x):
            vals.append(x)

    # Negatives: -0.0 and a sample of the rest (the sign is written the same
    # way for every value, so a sample is enough).
    vals += [-0.0, -1.0, -sys.float_info.max, -5e-324]
    vals += [-x for x in rng.sample(vals, 1000)]

    seen: set[int] = set()
    unique = []
    for x in vals:
        b = bits(x)
        if math.isfinite(x) and b not in seen:
            seen.add(b)
            unique.append(x)
    return unique


def main() -> None:
    lines = [
        "# GENERATED by scripts/generate_float_repr_fixture.py -- do not edit.",
        f"# CPython {sys.version.split()[0]}: repr(float) for each double.",
        "# <IEEE 754 bits, 16 hex digits> <repr(float)> [<openjd text, an intended difference>]",
        "# The only intended difference: openjd writes -0.0 as 0.0 (expr normalizes -0 to 0).",
    ]
    for x in values():
        b = bits(x)
        line = f"{b:016x} {x!r}"
        if b in OPENJD_EXCEPTIONS:
            line += f" {OPENJD_EXCEPTIONS[b]}"
        lines.append(line)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("\n".join(lines) + "\n")
    print(f"wrote {len(lines) - 4} values to {OUT}")


if __name__ == "__main__":
    main()
