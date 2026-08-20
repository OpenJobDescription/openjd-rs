// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Fuzz `RangeExpr` parsing and materialization.
//!
//! `RangeExpr` parses strings like `"1-10:2"` into (start, end, step) integer
//! ranges used by step parameter spaces. The documented crash
//! `RangeExpr::from_str("-9223372036854775807-9223372036854775807")` was an
//! `attempt to subtract with overflow` in the parser; range length/indexing
//! also does integer arithmetic that must not overflow. Parsing MUST return
//! `Err` on malformed or out-of-range input, and materializing a parsed range
//! MUST NOT panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::str::FromStr;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };

    if let Ok(range) = openjd_expr::RangeExpr::from_str(s) {
        // A successfully parsed range must report a consistent length and be
        // safely indexable/iterable without overflow. `len()` and `get()` both
        // do index arithmetic over i64 bounds. Bound the materialization so a
        // legitimately huge (but valid) range doesn't turn the fuzzer into a
        // multi-second loop; the arithmetic we care about is exercised by the
        // first handful of elements and the length computation.
        let len = range.len();
        let _ = range.is_empty();
        for i in 0..len.min(256) {
            let _ = range.get(i as i64);
        }
        // Also drive the iterator adaptor a bounded distance.
        for v in range.iter().take(256) {
            let _ = range.contains(v);
        }
    }
});
