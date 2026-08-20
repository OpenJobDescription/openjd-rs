// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Fuzz the public `RangeExpr::slice` constructor.
//!
//! `slice(start, stop, step)` is the structural twin of `IntRange::new`: a
//! public method that takes raw `i64` indices and does a lot of unchecked index
//! arithmetic (cumulative-length sums, ceil-division, per-sub-range stride
//! remapping). The evaluator only ever calls it with parser-derived, in-bounds
//! indices, so the adversarial `i64` domain — negative, `i64::MAX`, `i64::MIN`,
//! strides that overflow when multiplied — is not otherwise exercised.
//!
//! `slice` MUST return `Ok` or `Err` for any indices — never panic, abort, or
//! hang — and any returned `RangeExpr` MUST materialize without panicking.

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::str::FromStr;

// The fuzzer supplies the source range text plus the three slice indices.
// Splitting the input this way lets the mutator explore both the base range
// shape and the slice arguments independently.
fuzz_target!(|input: (&str, i64, i64, i64)| {
    let (text, start, stop, step) = input;

    let Ok(range) = openjd_expr::RangeExpr::from_str(text) else {
        return;
    };

    if let Ok(sliced) = range.slice(start, stop, step) {
        // A returned slice must be safe to walk: len/get do index arithmetic
        // over the remapped sub-ranges. Bound the walk so a valid-but-large
        // result doesn't stall the fuzzer.
        let len = sliced.len();
        let _ = sliced.is_empty();
        for i in 0..len.min(256) {
            let _ = sliced.get(i as i64);
        }
    }
});
