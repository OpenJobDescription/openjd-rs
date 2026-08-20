// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Fuzz the expression parser.
//!
//! `ParsedExpression::new` accepts arbitrary user-supplied expression text
//! (from job templates, `let` bindings, format strings). A malformed or
//! adversarial expression MUST surface as an `Err`, never a panic, an abort,
//! or a hang. The parser has its own input-length and depth caps; this target
//! exercises the paths below those caps where the ruff-based recursive-descent
//! parser and the structural depth walker actually run.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The parser operates on &str. Reject non-UTF-8 input cheaply rather than
    // lossily transforming it — feeding it lossy data would mask real
    // char-boundary behaviour behind U+FFFD replacements.
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };

    // `new` uses `ExprProfile::latest()` — the widest syntax surface (every
    // extension enabled), which maximizes the grammar the fuzzer can reach.
    // A returned error is a valid, expected outcome; only a panic/abort is a
    // finding.
    let _ = openjd_expr::ParsedExpression::new(s);
});
