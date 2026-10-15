// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Structure-aware input generators.
//!
//! Byte-level mutation alone rarely gets past a parser into the code that
//! historically broke (evaluator arithmetic, type rules, job instantiation,
//! manifest algebra). These generators turn fuzzer bytes into *well-formed*
//! inputs — typed values, well-typed expressions, plausible templates,
//! manifests with colliding paths — so libFuzzer's coverage feedback steers
//! the semantics instead of the syntax.
//!
//! Every generator draws from [`arbitrary::Unstructured`], so an exhausted
//! byte buffer degrades to small default choices rather than failing. Value
//! choices are biased toward the boundaries past bugs lived at: `i64` edges,
//! `2^62` (the range-value bound), multibyte and combining characters,
//! Windows drive and UNC forms, URIs, and format-string braces.

pub mod expr;
pub mod manifest;
pub mod template;
pub mod value;

use arbitrary::Unstructured;

/// Pick one element of a non-empty slice.
pub fn pick<'a, T>(u: &mut Unstructured<'_>, items: &'a [T]) -> &'a T {
    let i = u.choose_index(items.len()).unwrap_or(0);
    &items[i]
}

/// An integer in `0..=max`.
pub fn upto(u: &mut Unstructured<'_>, max: usize) -> usize {
    u.int_in_range(0..=max).unwrap_or(0)
}

/// A coin flip that defaults to `false` once the input is exhausted.
pub fn flip(u: &mut Unstructured<'_>) -> bool {
    u.arbitrary::<bool>().unwrap_or(false)
}

/// True with probability roughly `1/n`.
pub fn one_in(u: &mut Unstructured<'_>, n: u8) -> bool {
    u.int_in_range(0..=n.saturating_sub(1)).unwrap_or(1) == 0
}

/// Integers where past overflow and truncation bugs lived.
pub const INTERESTING_I64: &[i64] = &[
    0,
    1,
    -1,
    2,
    -2,
    3,
    7,
    10,
    100,
    255,
    256,
    1000,
    65_535,
    i32::MAX as i64,
    i32::MIN as i64,
    u32::MAX as i64,
    1 << 32,
    (1 << 53) + 1,
    (1 << 62) - 1,
    1 << 62,
    -(1 << 62),
    i64::MAX,
    i64::MIN,
    i64::MAX - 1,
    i64::MIN + 1,
];

/// An integer: usually a boundary value, sometimes a raw draw.
pub fn int(u: &mut Unstructured<'_>) -> i64 {
    if one_in(u, 3) {
        u.arbitrary().unwrap_or(0)
    } else {
        *pick(u, INTERESTING_I64)
    }
}

/// A small integer, for counts, indices, and widths that should stay cheap.
pub fn small_int(u: &mut Unstructured<'_>) -> i64 {
    u.int_in_range(-20..=40).unwrap_or(0)
}

/// Finite floats at formatting and precision boundaries.
pub const INTERESTING_F64: &[f64] = &[
    0.0,
    -0.0,
    0.5,
    -0.5,
    1.0,
    -1.0,
    1.5,
    2.5,
    0.1,
    1e-4,
    9.999e-5,
    1e16,
    9.999_999_999_999_998e15,
    1e300,
    -1e300,
    f64::MAX,
    f64::MIN,
    f64::MIN_POSITIVE,
    5e-324,
    9_007_199_254_740_993.0,
    9.223_372_036_854_776e18,
    -9.223_372_036_854_776e18,
    123.456,
];

/// A finite float: usually a boundary value, sometimes a raw draw.
pub fn float(u: &mut Unstructured<'_>) -> f64 {
    let f = if one_in(u, 3) {
        u.arbitrary::<f64>().unwrap_or(0.0)
    } else {
        *pick(u, INTERESTING_F64)
    };
    if f.is_finite() {
        f
    } else {
        0.0
    }
}

/// String fragments that have broken byte/char index arithmetic, escaping,
/// path parsing, or format-string scanning.
pub const STRING_FRAGMENTS: &[&str] = &[
    "",
    "a",
    "abc",
    "A",
    " ",
    "  x  ",
    "\t",
    "\n",
    "\r\n",
    "\0",
    "é",
    "e\u{301}",
    "ß",
    "İ",
    "ﬁ",
    "日本語",
    "🎬",
    "👩‍👩‍👧",
    "\u{feff}",
    "\u{200b}",
    "\u{7f}",
    "١٢٣",
    "Ⅻ",
    "²",
    "{{",
    "}}",
    "{{ Param.X }}",
    "*",
    "********",
    "'",
    "\"",
    "\\",
    "\\\\",
    "%PATH%",
    "!x!",
    "$(x)",
    "`x`",
    ";",
    "&",
    "|",
    "^",
    "/",
    "//",
    ".",
    "..",
    "/a/b/c",
    "/a/b/",
    "a/b",
    "../x",
    "C:",
    "C:\\",
    "C:\\a\\b",
    "C://x",
    "c:/a",
    "\\\\server\\share\\x",
    "\\\\?\\C:\\x",
    "s3://bucket/key",
    "s3://bucket//k",
    "file:///tmp/x",
    "https://h/a?b#c",
    "x.tar.gz",
    ".hidden",
    "name.",
    "frame_####.exr",
    "0",
    "-0",
    "1e5",
    "0x10",
    "inf",
    "NaN",
    "1_000",
    "True",
    "None",
    "1-10:2",
    "1,3,5-7",
    "[1, 2]",
    "openjd_env: A=B",
    "openjd_redacted_env: S=secret",
];

/// A string assembled from boundary fragments and raw text.
pub fn string(u: &mut Unstructured<'_>) -> String {
    let n = upto(u, 4);
    let mut s = String::new();
    for _ in 0..n {
        if one_in(u, 4) {
            let raw: &str = u.arbitrary().unwrap_or("");
            s.extend(raw.chars().take(32));
        } else {
            s.push_str(pick(u, STRING_FRAGMENTS));
        }
    }
    s
}

/// A path-shaped string: separators, drives, UNC, and URIs mixed with names.
pub fn path_string(u: &mut Unstructured<'_>) -> String {
    const ROOTS: &[&str] = &[
        "",
        "/",
        "C:\\",
        "C:/",
        "D:",
        "\\\\srv\\share\\",
        "s3://bucket/",
        "file:///",
        "./",
        "../",
        "~/",
    ];
    const PARTS: &[&str] = &[
        "a", "b", "dir", "file.txt", "x.tar.gz", ".hidden", "..", ".", "", "日本", "é", "sp ace",
        "trail.", "con", "a:b",
    ];
    let mut s = pick(u, ROOTS).to_string();
    let n = upto(u, 4);
    for i in 0..n {
        if i > 0 {
            s.push_str(pick(u, &["/", "\\", "//"]));
        }
        s.push_str(pick(u, PARTS));
    }
    if one_in(u, 6) {
        s.push_str(&string(u));
    }
    s
}

/// A valid OpenJD identifier (`[A-Za-z_][A-Za-z0-9_]*`), short and likely to
/// collide with other generated identifiers so lookups and merges interact.
pub fn ident(u: &mut Unstructured<'_>) -> String {
    const NAMES: &[&str] = &[
        "A", "B", "C", "X", "Y", "Frame", "Name", "Out", "In", "File", "Dir", "_a", "v1", "Path",
    ];
    if one_in(u, 8) {
        let len = upto(u, 6) + 1;
        let mut s = String::new();
        for i in 0..len {
            let c = if i == 0 {
                *pick(u, &['a', 'Z', '_', 'q'])
            } else {
                *pick(u, &['a', 'Z', '_', '0', '9'])
            };
            s.push(c);
        }
        s
    } else {
        pick(u, NAMES).to_string()
    }
}
