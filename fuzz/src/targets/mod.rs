// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Fuzz targets. Each is `fn(&[u8])` and panics on a finding.
//!
//! Input conventions:
//! - **Text targets** (`*_text`, decoders) take UTF-8 text, optionally
//!   followed by a NUL and *control bytes*: the text is what a user or file
//!   would supply, and the control bytes steer the environment the text runs
//!   in (symbol types and values, profile, path format). Seeds are plain
//!   text; libFuzzer grows the control tail itself.
//! - **Generated targets** (`*_gen`) feed all bytes to a structure-aware
//!   generator in [`crate::gen`].

pub mod cli;
pub mod expr;
pub mod model;
pub mod sessions;
pub mod snapshots;

/// A fuzz target entry point.
pub type Target = fn(&[u8]);

/// Every target, by name. `tests/replay.rs` checks this list against the
/// `[[bin]]` entries in `Cargo.toml`, the files in `fuzz_targets/`, and the
/// corpus directories, so a target cannot be added in one place and
/// forgotten in another.
pub const TARGETS: &[(&str, Target)] = &[
    ("expr_parse", expr::parse),
    ("expr_eval_text", expr::eval_text),
    ("expr_eval_gen", expr::eval_gen),
    ("expr_format_string", expr::format_string),
    ("expr_range", expr::range),
    ("expr_int_range", expr::int_range),
    ("expr_type", expr::types),
    ("expr_value", expr::values),
    ("expr_function_call", expr::function_call),
    ("expr_path", expr::paths),
    ("expr_symbol_table", expr::symbol_table),
    ("model_decode_text", model::decode_text),
    ("model_job_gen", model::job_gen),
    ("model_env_gen", model::env_gen),
    ("model_misc", model::misc),
    ("sessions_directives", sessions::directives),
    ("sessions_framer", sessions::framer),
    ("sessions_embedded_files", sessions::embedded_files),
    ("snapshots_decode_text", snapshots::decode_text),
    ("snapshots_ops_gen", snapshots::ops_gen),
    ("cli_args", cli::args),
];

/// Split `data` at the first NUL into (text, control bytes). Returns `None`
/// when the text half is not UTF-8.
pub fn text_and_control(data: &[u8]) -> Option<(&str, &[u8])> {
    let (text, ctl) = match data.iter().position(|&b| b == 0) {
        Some(i) => (&data[..i], &data[i + 1..]),
        None => (data, &[][..]),
    };
    std::str::from_utf8(text).ok().map(|t| (t, ctl))
}

/// A short, stable digest of a string, used to vary choices for seeds that
/// carry no control bytes.
pub fn digest(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100_0000_01b3)
    })
}
