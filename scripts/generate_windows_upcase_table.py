#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
"""Generate crates/openjd-expr/src/windows_upcase.rs from the Windows upcase table.

Windows compares file names case-insensitively by mapping each UTF-16 code
unit through the system uppercase table (`RtlUpcaseUnicodeChar`; NTFS keeps a
copy of it in `$UpCase`, and `CompareStringOrdinal(..., bIgnoreCase=TRUE)`
uses the same mapping). Two names refer to the same file exactly when their
code units are equal after that mapping.

That table is not any Unicode case mapping:
- it maps one code unit to one code unit, so characters with multi-character
  uppercase forms (U+00DF sharp s) stay as they are;
- it maps a lowercase letter only to an uppercase or titlecase letter that maps
  back to it,
  so U+0131 dotless i, U+017F long s, U+00B5 micro sign and U+03C2 final sigma
  do not fold to I, S, MU and SIGMA, and no non-ASCII character folds to an
  ASCII one (U+212A KELVIN SIGN does not match 'k');
- it is frozen at an old Unicode version, so later case pairs (Cherokee
  lowercase, Georgian Mtavruli, many Latin Extended additions) do not fold;
- it leaves every surrogate code unit alone, so characters outside the BMP
  never fold.

openjd-expr uses the table so a WINDOWS-format path mapping rule matches the
same paths Windows itself would treat as the same, on any host. The table is
read from the running system, so this script must run on Windows:

    python scripts/generate_windows_upcase_table.py

The generator cross-checks every entry against CompareStringOrdinal. The output
file is checked in; `windows_upcase.rs` also carries a Windows-only test that
re-checks it against the running system.
"""

import ctypes
import platform
import sys
from ctypes import wintypes
from datetime import date

OUT_PATH = "crates/openjd-expr/src/windows_upcase.rs"
CSTR_EQUAL = 2

# The code after the table, kept in rustfmt form so the output passes
# `cargo fmt --check` as written.
RUST_TAIL = r"""];

/// Map `c` through the Windows uppercase table. Characters outside the
/// BMP are returned unchanged, as Windows leaves surrogate code units alone.
pub(crate) fn upcase(c: char) -> char {
    let Ok(cu) = u16::try_from(u32::from(c)) else {
        return c;
    };
    match UPCASE.binary_search_by_key(&cu, |&(k, _)| k) {
        Ok(i) => char::from_u32(u32::from(UPCASE[i].1)).unwrap_or(c),
        Err(_) => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table must be sorted with unique keys, never map to the key
    /// itself, never touch a surrogate, and never fold a non-ASCII
    /// character to an ASCII one.
    #[test]
    fn table_is_well_formed() {
        let mut prev: Option<u16> = None;
        for &(k, v) in UPCASE {
            if let Some(p) = prev {
                assert!(k > p, "unsorted or duplicate at {k:#06x}");
            }
            assert_ne!(k, v);
            assert!(!(0xd800..=0xdfff).contains(&k) && !(0xd800..=0xdfff).contains(&v));
            assert!(k < 0x80 || v >= 0x80, "{k:#06x} folds to ASCII");
            prev = Some(k);
        }
    }

    #[test]
    fn known_points() {
        assert_eq!(upcase('a'), 'A');
        assert_eq!(upcase('A'), 'A');
        assert_eq!(upcase('\u{e9}'), '\u{c9}'); // e acute
        assert_eq!(upcase('\u{3c3}'), '\u{3a3}'); // sigma
        assert_eq!(upcase('\u{430}'), '\u{410}'); // Cyrillic a
        assert_eq!(upcase('\u{ff41}'), '\u{ff21}'); // fullwidth a
    }

    /// Characters Unicode gives an uppercase form that Windows does not fold.
    #[test]
    fn known_points_windows_keeps() {
        assert_eq!(upcase('\u{3c2}'), '\u{3c2}'); // final sigma
        assert_eq!(upcase('\u{131}'), '\u{131}'); // dotless i
        assert_eq!(upcase('\u{17f}'), '\u{17f}'); // long s
        assert_eq!(upcase('\u{b5}'), '\u{b5}'); // micro sign
        assert_eq!(upcase('\u{df}'), '\u{df}'); // sharp s
        assert_eq!(upcase('\u{212a}'), '\u{212a}'); // Kelvin sign
        assert_eq!(upcase('\u{ab70}'), '\u{ab70}'); // Cherokee small a
        assert_eq!(upcase('\u{10428}'), '\u{10428}'); // Deseret, outside the BMP
    }

    /// The single character of a one-character case mapping, if it is one.
    #[cfg(windows)]
    fn single(mut chars: impl Iterator<Item = char>) -> Option<char> {
        let c = chars.next()?;
        chars.next().is_none().then_some(c)
    }

    /// Re-check the checked-in table against the running Windows system:
    /// for each code point and every likely case partner of it, Windows
    /// must call the pair equal exactly when the table maps both to the
    /// same code unit.
    #[cfg(windows)]
    #[test]
    fn matches_compare_string_ordinal() {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CompareStringOrdinal(
                a: *const u16,
                alen: i32,
                b: *const u16,
                blen: i32,
                ignore_case: i32,
            ) -> i32;
        }
        const CSTR_EQUAL: i32 = 2;
        let windows_equal = |a: char, b: char| {
            let (mut ab, mut bb) = ([0u16; 2], [0u16; 2]);
            let a16 = a.encode_utf16(&mut ab);
            let b16 = b.encode_utf16(&mut bb);
            // SAFETY: both buffers hold valid UTF-16 of the given lengths,
            // which are at most 2.
            let r = unsafe {
                CompareStringOrdinal(
                    a16.as_ptr(),
                    a16.len() as i32,
                    b16.as_ptr(),
                    b16.len() as i32,
                    1,
                )
            };
            r == CSTR_EQUAL
        };
        let mut preimages: std::collections::HashMap<u16, Vec<char>> = Default::default();
        for &(k, v) in UPCASE {
            preimages
                .entry(v)
                .or_default()
                .push(char::from_u32(u32::from(k)).unwrap());
        }
        for cp in (0..=0xffffu32).chain([0x10400, 0x10428, 0x1e900, 0x1e922]) {
            let Some(a) = char::from_u32(cp) else {
                continue;
            };
            let mut partners = vec![upcase(a)];
            partners.extend(single(a.to_uppercase()));
            partners.extend(single(a.to_lowercase()));
            if let Ok(cu) = u16::try_from(cp) {
                partners.extend(preimages.get(&cu).into_iter().flatten());
            }
            for b in partners {
                assert_eq!(
                    windows_equal(a, b),
                    upcase(a) == upcase(b),
                    "U+{:04X} vs U+{:04X}",
                    u32::from(a),
                    u32::from(b)
                );
            }
        }
    }
}
"""


def main():
    if sys.platform != "win32":
        sys.exit("This generator reads the Windows system upcase table and must run on Windows.")

    upcase = ctypes.WinDLL("ntdll").RtlUpcaseUnicodeChar
    upcase.argtypes = [wintypes.WCHAR]
    upcase.restype = wintypes.WCHAR

    compare = ctypes.WinDLL("kernel32").CompareStringOrdinal
    compare.argtypes = [
        wintypes.LPCWSTR,
        ctypes.c_int,
        wintypes.LPCWSTR,
        ctypes.c_int,
        wintypes.BOOL,
    ]
    compare.restype = ctypes.c_int

    entries = []
    for cu in range(0x10000):
        mapped = ord(upcase(chr(cu)))
        if mapped == cu:
            continue
        surrogate = 0xD800 <= cu <= 0xDFFF or 0xD800 <= mapped <= 0xDFFF
        assert not surrogate, f"U+{cu:04X} -> U+{mapped:04X} involves a surrogate"
        assert compare(chr(cu), 1, chr(mapped), 1, True) == CSTR_EQUAL, f"U+{cu:04X}"
        entries.append((cu, mapped))

    version = platform.version()
    lines = [
        "// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.",
        "// Copyright by contributors to this project.",
        "// SPDX-License-Identifier: (Apache-2.0 OR MIT)",
        "",
        "//! The Windows uppercase table, for matching WINDOWS-format paths the way",
        "//! Windows compares file names.",
        "//!",
        f"//! GENERATED by scripts/generate_windows_upcase_table.py on {date.today()} — do not edit.",
        f"//! Source: RtlUpcaseUnicodeChar on Windows {version}.",
        "//!",
        "//! See the script docstring for how this table differs from Unicode case",
        "//! mappings.",
        "",
        "/// `(code unit, uppercase code unit)` for every BMP code unit the Windows",
        "/// uppercase table changes, sorted by code unit. Every other code unit,",
        "/// including every surrogate, maps to itself.",
        "#[rustfmt::skip]",
        "static UPCASE: &[(u16, u16)] = &[",
    ]
    row = []
    for cu, mapped in entries:
        row.append(f"(0x{cu:04x}, 0x{mapped:04x})")
        if len(row) == 6:
            lines.append("    " + ", ".join(row) + ",")
            row = []
    if row:
        lines.append("    " + ", ".join(row) + ",")
    lines.append(RUST_TAIL)
    with open(OUT_PATH, "w", newline="\n", encoding="utf-8") as f:
        f.write("\n".join(lines))
    print(f"{len(entries)} entries; wrote {OUT_PATH}")


if __name__ == "__main__":
    main()
