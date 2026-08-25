#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
"""Generate crates/openjd-expr/src/functions/unicode_tables.rs from CPython.

The OpenJD expression language string classification functions (isdigit,
isalpha, isalnum, isspace, isupper, islower) are specified to match Python's
str methods of the same name. Rust's std char methods use different Unicode
properties (e.g. `char::is_alphabetic()` is the `Alphabetic` property, which
is a superset of Python's `L*` general categories), so we generate lookup
tables directly from the CPython runtime that defines the semantics.

Run with the CPython version whose Unicode tables the crate should pin to:

    python scripts/generate_unicode_tables.py

The output file is checked in; regenerate it when intentionally moving to a
newer Unicode version, and update the spec note in
specs/expr/function-library.md to match.

Table definitions (per CPython Objects/unicodectype.c semantics):
- DIGIT:   chr(cp).isdigit()   — Numeric_Type = Decimal or Digit
- DECIMAL: chr(cp).isdecimal() — Numeric_Type = Decimal (general category Nd)
- ALPHA:   chr(cp).isalpha()   — general category Lu/Ll/Lt/Lm/Lo
- ALNUM:   chr(cp).isalnum()   — ALPHA or Numeric_Type != None
- SPACE:   chr(cp).isspace()   — Zs/Zl/Zp or bidirectional WS/B/S
- UPPER:   chr(cp).isupper()   — Uppercase property (Lu + Other_Uppercase)
- LOWER:   chr(cp).islower()   — Lowercase property (Ll + Other_Lowercase)
- CASED:   UPPER or LOWER or general category Lt
- CASE_IGNORABLE: the Unicode Case_Ignorable property, probed from CPython's
  Final_Sigma handling in str.lower (unicodedata does not expose it)
- IDENT_START:    valid first characters of a Python identifier
  (XID_Start plus '_'), probed via str.isidentifier()
- IDENT_CONTINUE: valid non-first identifier characters (XID_Continue),
  probed via ('a' + c).isidentifier()

IDENT_START/IDENT_CONTINUE back the regex capture-group-name validation:
CPython's re module (sre_parse) checks group names with name.isidentifier(),
so probing isidentifier() reproduces the exact rule, including the NFKC-free
per-character form (isidentifier does not normalize).

str.isupper()/str.islower() are not per-character predicates: they require at
least one cased character and that every cased character is upper/lowercase.
The CASED table supports that composition in Rust.

The DECIMAL table additionally backs `decimal_digit_value()`, which maps a
Numeric_Type=Decimal character to its digit value 0-9. CPython's int() and
float() replace such characters with their ASCII equivalents before parsing
(Python/pystrtod.c via _PyUnicode_TransformDecimalAndSpaceToASCII), so the
expression language's int()/float() do the same. UAX #44 guarantees decimal
digits occur in contiguous 0-9 runs; the generator verifies every DECIMAL
range is a whole number of zero-aligned runs so the Rust side can compute
the value as (cp - range_start) % 10.

Additionally TITLE_MAP holds the full titlecase mapping (ToTitleFull) for
every code point whose titlecase differs from itself, probed as the
single-character str.title() result. Python's str.title() and
str.capitalize() titlecase word-start characters (e.g. U+01C6 dz-digraph
becomes U+01C5, and U+00DF sharp s becomes "Ss"), which Rust's
char::to_uppercase does not provide. CASE_IGNORABLE and CASED together
implement the Final_Sigma context rule for lowering U+03A3 within
title()/capitalize().
"""

import sys
import unicodedata
from datetime import date

OUT_PATH = "crates/openjd-expr/src/functions/unicode_tables.rs"

SURROGATE_RANGE = range(0xD800, 0xE000)


def build_ranges(predicate):
    """Return sorted inclusive (start, end) ranges of code points matching predicate."""
    ranges = []
    start = None
    for cp in range(0x110000):
        # Surrogates cannot exist in Rust `char` or in well-formed Python str
        # data; treat them as non-matching.
        matches = cp not in SURROGATE_RANGE and predicate(chr(cp))
        if matches and start is None:
            start = cp
        elif not matches and start is not None:
            ranges.append((start, cp - 1))
            start = None
    if start is not None:
        ranges.append((start, 0x10FFFF))
    return ranges


def is_cased(ch):
    return ch.isupper() or ch.islower() or unicodedata.category(ch) == "Lt"


def is_case_ignorable(ch):
    """Probe the Case_Ignorable property via CPython's Final_Sigma handling.

    U+03A3 GREEK CAPITAL LETTER SIGMA lowercases to final sigma (U+03C2) iff
    it is preceded by a cased character with only case-ignorable characters
    in between. So with a cased prefix 'A', sigma stays final across `ch`
    iff `ch` is case-ignorable or itself cased; with an uncased prefix '1',
    sigma is final only if `ch` is cased. The difference isolates
    Case_Ignorable.
    """
    with_cased_prefix = ("A" + ch + "\u03a3").lower().endswith("\u03c2")
    with_uncased_prefix = ("1" + ch + "\u03a3").lower().endswith("\u03c2")
    return with_cased_prefix and not with_uncased_prefix


TABLES = [
    ("DIGIT", "str.isdigit code points (Numeric_Type = Decimal or Digit).", lambda c: c.isdigit()),
    (
        "DECIMAL",
        "str.isdecimal code points (Numeric_Type = Decimal, category Nd).",
        lambda c: c.isdecimal(),
    ),
    ("ALPHA", "str.isalpha code points (general category Lu/Ll/Lt/Lm/Lo).", lambda c: c.isalpha()),
    ("ALNUM", "str.isalnum code points (ALPHA or any Numeric_Type).", lambda c: c.isalnum()),
    ("SPACE", "str.isspace code points (Zs/Zl/Zp or bidi WS/B/S).", lambda c: c.isspace()),
    ("UPPER", "Uppercase code points (Lu + Other_Uppercase).", lambda c: c.isupper()),
    ("LOWER", "Lowercase code points (Ll + Other_Lowercase).", lambda c: c.islower()),
    ("CASED", "Cased code points (UPPER or LOWER or category Lt).", is_cased),
    (
        "CASE_IGNORABLE",
        "Case_Ignorable code points (for the Final_Sigma context rule).",
        is_case_ignorable,
    ),
    (
        "IDENT_START",
        "Valid first characters of a Python identifier (XID_Start + '_'),\n/// probed via str.isidentifier().",
        lambda c: c.isidentifier(),
    ),
    (
        "IDENT_CONTINUE",
        "Valid non-first Python identifier characters (XID_Continue),\n/// probed via ('a' + c).isidentifier().",
        lambda c: ("a" + c).isidentifier(),
    ),
]


def build_title_map():
    """Full titlecase mappings (ToTitleFull) where titlecase(c) != c.

    A single-character str.title() applies ToTitleFull (the first character
    of a title() word is titlecased), so probing it per code point yields
    the exact mapping CPython uses, including one-to-many mappings from
    SpecialCasing.txt (e.g. U+00DF -> "Ss").
    """
    entries = []
    for cp in range(0x110000):
        if cp in SURROGATE_RANGE:
            continue
        ch = chr(cp)
        titled = ch.title()
        if titled != ch:
            entries.append((cp, titled))
    return entries


def verify(table, predicate):
    """Round-trip check: table membership must equal the CPython predicate."""
    idx = 0
    for cp in range(0x110000):
        while idx < len(table) and table[idx][1] < cp:
            idx += 1
        in_table = idx < len(table) and table[idx][0] <= cp
        expected = cp not in SURROGATE_RANGE and predicate(chr(cp))
        assert in_table == expected, f"table/CPython mismatch at U+{cp:04X}"


def verify_decimal_alignment(ranges):
    """DECIMAL ranges must be whole zero-aligned 0-9 runs.

    decimal_digit_value() in the generated Rust computes the digit value as
    (cp - range_start) % 10, which is only correct if every range starts at
    a digit with value 0 and each successive code point increments the value
    mod 10. Check the exact value of every code point against unicodedata.
    """
    for start, end in ranges:
        assert (end - start + 1) % 10 == 0, f"DECIMAL range U+{start:04X}..U+{end:04X} not runs of 10"
        for cp in range(start, end + 1):
            expected = (cp - start) % 10
            actual = unicodedata.decimal(chr(cp))
            assert actual == expected, f"U+{cp:04X}: decimal value {actual} != {expected}"


def main():
    lines = [
        "// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.",
        "// Copyright by contributors to this project.",
        "// SPDX-License-Identifier: (Apache-2.0 OR MIT)",
        "",
        "//! Unicode lookup tables for Python-parity string classification.",
        "//!",
        f"//! GENERATED by scripts/generate_unicode_tables.py on {date.today()} — do not edit.",
        f"//! Source: CPython {sys.version.split()[0]}, Unicode {unicodedata.unidata_version}.",
        "//!",
        "//! See the script docstring for the definition of each table.",
        "",
        "/// The Unicode version these tables were generated from.",
        "#[allow(dead_code)]",
        f'pub const UNICODE_VERSION: &str = "{unicodedata.unidata_version}";',
        "",
        "/// True if `c` falls in one of `table`'s inclusive ranges.",
        "pub fn in_table(table: &[(u32, u32)], c: char) -> bool {",
        "    let cp = c as u32;",
        "    table",
        "        .binary_search_by(|&(start, end)| {",
        "            if end < cp {",
        "                std::cmp::Ordering::Less",
        "            } else if start > cp {",
        "                std::cmp::Ordering::Greater",
        "            } else {",
        "                std::cmp::Ordering::Equal",
        "            }",
        "        })",
        "        .is_ok()",
        "}",
    ]
    for name, doc, predicate in TABLES:
        ranges = build_ranges(predicate)
        verify(ranges, predicate)
        if name == "DECIMAL":
            verify_decimal_alignment(ranges)
        print(f"{name}: {len(ranges)} ranges, {sum(e - s + 1 for s, e in ranges)} code points")
        lines.append("")
        lines.append(f"/// {doc}")
        lines.append("#[rustfmt::skip]")
        lines.append(f"pub static {name}: &[(u32, u32)] = &[")
        row = []
        for s, e in ranges:
            row.append(f"({s:#x}, {e:#x})")
            if len(row) == 6:
                lines.append("    " + ", ".join(row) + ",")
                row = []
        if row:
            lines.append("    " + ", ".join(row) + ",")
        lines.append("];")
    lines.extend(
        [
            "",
            "/// The decimal digit value (0-9) of `c` when it has Numeric_Type=Decimal",
            "/// (general category Nd), or `None` otherwise.",
            "///",
            "/// CPython's int() and float() replace such characters with their ASCII",
            "/// equivalents before parsing; the expression language's int()/float()",
            "/// use this to do the same. Every DECIMAL range is a whole number of",
            "/// zero-aligned 0-9 runs (verified by the generator), so the value is",
            "/// `(cp - range_start) % 10`.",
            "pub fn decimal_digit_value(c: char) -> Option<u32> {",
            "    let cp = c as u32;",
            "    DECIMAL",
            "        .binary_search_by(|&(start, end)| {",
            "            if end < cp {",
            "                std::cmp::Ordering::Less",
            "            } else if start > cp {",
            "                std::cmp::Ordering::Greater",
            "            } else {",
            "                std::cmp::Ordering::Equal",
            "            }",
            "        })",
            "        .ok()",
            "        .map(|i| (cp - DECIMAL[i].0) % 10)",
            "}",
        ]
    )
    title_map = build_title_map()
    print(f"TITLE_MAP: {len(title_map)} entries")
    lines.extend(
        [
            "",
            "/// Full titlecase mappings (ToTitleFull) where titlecase(c) != c,",
            "/// probed as CPython's single-character str.title() result. Sorted by",
            "/// code point for binary search.",
            "#[rustfmt::skip]",
            "pub static TITLE_MAP: &[(u32, &str)] = &[",
        ]
    )
    row = []
    for cp, titled in title_map:
        escaped = "".join(f"\\u{{{ord(c):x}}}" for c in titled)
        row.append(f'({cp:#x}, "{escaped}")')
        if len(row) == 4:
            lines.append("    " + ", ".join(row) + ",")
            row = []
    if row:
        lines.append("    " + ", ".join(row) + ",")
    lines.extend(
        [
            "];",
            "",
            "/// The full titlecase mapping for `c`, or `None` when it maps to itself.",
            "pub fn title_mapping(c: char) -> Option<&'static str> {",
            "    let cp = c as u32;",
            "    TITLE_MAP",
            "        .binary_search_by_key(&cp, |&(k, _)| k)",
            "        .ok()",
            "        .map(|i| TITLE_MAP[i].1)",
            "}",
        ]
    )
    lines.extend(
        [
            "",
            "#[cfg(test)]",
            "mod tests {",
            "    use super::*;",
            "",
            "    /// Every table must be sorted, non-overlapping, non-adjacent, and",
            "    /// free of surrogate code points — the invariants `in_table`'s",
            "    /// binary search and the generator's range-merging rely on.",
            "    #[test]",
            "    fn tables_are_well_formed() {",
            f"        for table in [{', '.join(name for name, _, _ in TABLES)}] {{",
            "            let mut prev_end: Option<u32> = None;",
            "            for &(start, end) in table {",
            "                assert!(start <= end);",
            "                assert!(end <= 0x10FFFF);",
            "                assert!(!(start <= 0xDFFF && end >= 0xD800), \"surrogates in table\");",
            "                if let Some(p) = prev_end {",
            "                    // Adjacent ranges would indicate a generator bug.",
            "                    assert!(start > p + 1, \"unmerged ranges at {start:#x}\");",
            "                }",
            "                prev_end = Some(end);",
            "            }",
            "        }",
            "    }",
            "",
            "    /// Exhaustive: `in_table` must agree with a linear sweep for every",
            "    /// code point, covering all range-boundary edge cases. The sweep",
            "    /// uses a forward-advancing cursor over the ranges (same trick as",
            "    /// `verify()` in the generator) so the whole test is linear, not",
            "    /// quadratic — it must stay fast in the debug profile.",
            "    #[test]",
            "    fn binary_search_matches_linear_scan() {",
            "        for table in [DIGIT, DECIMAL, SPACE, CASED] {",
            "            let mut idx = 0;",
            "            for cp in 0..=0x10FFFFu32 {",
            "                let Some(c) = char::from_u32(cp) else { continue };",
            "                while idx < table.len() && table[idx].1 < cp {",
            "                    idx += 1;",
            "                }",
            "                let linear = idx < table.len() && table[idx].0 <= cp;",
            "                assert_eq!(in_table(table, c), linear, \"U+{cp:04X}\");",
            "            }",
            "        }",
            "    }",
            "",
            "    /// Every DECIMAL range must be a whole number of zero-aligned 0-9",
            "    /// runs — the invariant `decimal_digit_value`'s `% 10` computation",
            "    /// relies on (the generator verifies the per-code-point values",
            "    /// against unicodedata).",
            "    #[test]",
            "    fn decimal_ranges_are_zero_aligned_runs() {",
            "        for &(start, end) in DECIMAL {",
            "            assert_eq!((end - start + 1) % 10, 0, \"U+{start:04X}..U+{end:04X}\");",
            "        }",
            "    }",
            "",
            "    /// Spot-check `decimal_digit_value` against known code points, and",
            "    /// confirm DECIMAL is a subset of DIGIT.",
            "    #[test]",
            "    fn decimal_digit_value_known_points() {",
            "        for (i, c) in ('0'..='9').enumerate() {",
            "            assert_eq!(decimal_digit_value(c), Some(i as u32));",
            "        }",
            "        assert_eq!(decimal_digit_value('\\u{0663}'), Some(3)); // ARABIC-INDIC THREE",
            "        assert_eq!(decimal_digit_value('\\u{0967}'), Some(1)); // DEVANAGARI ONE",
            "        assert_eq!(decimal_digit_value('\\u{ff17}'), Some(7)); // FULLWIDTH SEVEN",
            "        assert_eq!(decimal_digit_value('\\u{1d7d8}'), Some(0)); // MATH DOUBLE-STRUCK ZERO",
            "        assert_eq!(decimal_digit_value('\\u{00b2}'), None); // SUPERSCRIPT TWO (No)",
            "        assert_eq!(decimal_digit_value('\\u{216b}'), None); // ROMAN NUMERAL TWELVE (Nl)",
            "        assert_eq!(decimal_digit_value('a'), None);",
            "        for cp in 0..=0x10FFFFu32 {",
            "            let Some(c) = char::from_u32(cp) else { continue };",
            "            if decimal_digit_value(c).is_some() {",
            "                assert!(in_table(DIGIT, c), \"U+{cp:04X} decimal but not DIGIT\");",
            "            }",
            "        }",
            "    }",
            "",
            "    /// TITLE_MAP must be sorted by code point with no duplicates, every",
            "    /// mapping must differ from its key, and `title_mapping` must agree",
            "    /// with a linear sweep (forward cursor, so the test stays linear).",
            "    #[test]",
            "    fn title_map_is_well_formed() {",
            "        let mut prev: Option<u32> = None;",
            "        for &(cp, mapped) in TITLE_MAP {",
            "            if let Some(p) = prev {",
            "                assert!(cp > p, \"unsorted or duplicate at {cp:#x}\");",
            "            }",
            "            assert!(!mapped.is_empty());",
            "            assert_ne!(char::from_u32(cp).map(String::from).as_deref(), Some(mapped));",
            "            prev = Some(cp);",
            "        }",
            "        let mut idx = 0;",
            "        for cp in 0..=0x10FFFFu32 {",
            "            let Some(c) = char::from_u32(cp) else { continue };",
            "            while idx < TITLE_MAP.len() && TITLE_MAP[idx].0 < cp {",
            "                idx += 1;",
            "            }",
            "            let linear = (idx < TITLE_MAP.len() && TITLE_MAP[idx].0 == cp)",
            "                .then(|| TITLE_MAP[idx].1);",
            "            assert_eq!(title_mapping(c), linear, \"U+{cp:04X}\");",
            "        }",
            "    }",
            "}",
        ]
    )
    lines.append("")
    with open(OUT_PATH, "w", newline="\n", encoding="utf-8") as f:
        f.write("\n".join(lines))
    print(f"wrote {OUT_PATH}")


if __name__ == "__main__":
    main()
