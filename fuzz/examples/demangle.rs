// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Demangle Rust symbol names, one per line from stdin to stdout, without
//! the hash suffix. Used by `scripts/check_fuzz_api_coverage.py` to map
//! llvm-cov function records to their definition paths.

use std::io::{BufRead, Write};

fn main() {
    let stdin = std::io::stdin();
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    for line in stdin.lock().lines() {
        let line = line.expect("read stdin");
        writeln!(out, "{:#}", rustc_demangle::demangle(&line)).expect("write stdout");
    }
}
