// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Fuzz the public type-string parser `ExprType::parse`.
//!
//! `ExprType::parse` turns strings like `list[int]`, `union[int, string]`, or
//! `unresolved[T]` into `ExprType` values. It is a public, recursive parser
//! over untrusted text (type annotations can originate from function-signature
//! DSL strings), so it must reject malformed input with `Err` and must not
//! recurse into a stack overflow on deeply nested type strings.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    // `parse` has its own depth guard; a returned `Err(String)` is the expected
    // outcome for malformed or too-deep input. Only a panic/abort is a finding.
    let _ = openjd_expr::ExprType::parse(s);
});
