// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Fuzz the public `IntRange::new` constructor with arbitrary `(start, end,
//! step)` triples.
//!
//! `RangeExpr::from_str` can only ever hand `IntRange::new` a step whose
//! magnitude came from parsing a decimal literal, so the text parser cannot
//! reach values like `i64::MIN`. But `IntRange` is a public type and `new` is
//! a public constructor callable directly with *any* `i64` — so the range
//! normalization arithmetic (including the `-step` negation on the descending
//! branch) must stay panic-free across the full `i64` domain, not just the
//! subset the text parser can produce. `new` MUST return `Ok` or `Err` for
//! every input — never panic or abort under the overflow-checked fuzz build.

#![no_main]

use libfuzzer_sys::fuzz_target;
use openjd_expr::range_expr::IntRange;

// Take three i64s directly from the fuzzer via `arbitrary`. This gives the
// mutator full control over each field (including the i64 boundary values) far
// more efficiently than decoding raw bytes by hand.
fuzz_target!(|triple: (i64, i64, i64)| {
    let (start, end, step) = triple;
    if let Ok(range) = IntRange::new(start, end, step) {
        // A successfully constructed range must also materialize without
        // panicking: len/get/iter do their own index arithmetic over the
        // normalized bounds. Bound the walk so a valid-but-enormous range
        // doesn't stall the fuzzer.
        let len = range.len();
        let _ = range.is_empty();
        for i in 0..len.min(256) {
            let _ = range.get(i);
        }
        for v in range.iter().take(256) {
            let _ = range.contains(v);
        }
    }
});
