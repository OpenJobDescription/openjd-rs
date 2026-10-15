// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Fuzz harness for openjd-rs.
//!
//! Every fuzz target is a plain `fn(&[u8])` in [`targets`], listed in
//! [`targets::TARGETS`]. The libFuzzer binaries in `fuzz_targets/` call them,
//! and `tests/replay.rs` replays the committed corpus through them on stable.
//! A target signals a finding by panicking — either a panic inside the code
//! under test, or an [`oracle!`] violation: a property the input must satisfy
//! (type soundness, round trips, redaction, containment, ...) that did not
//! hold. See `fuzz/README.md` for the target catalogue and the bug classes
//! each oracle encodes.

// Overflow checks follow debug assertions unless a profile turns them off,
// and this crate's profiles turn both on. Refuse to build otherwise: a fuzz
// build without overflow checks silently wraps the arithmetic bugs most of
// these targets exist to catch.
#[cfg(not(debug_assertions))]
compile_error!(
    "openjd-fuzz must be built with debug assertions (and overflow checks). \
     Use `cargo fuzz build` / `cargo test` with this crate's profiles."
);

pub mod alloc;
pub mod gen;
pub mod targets;

/// The CLI's argument parsers live in the `openjd` bin crate, which has no
/// library target. Include their source directly (the same `#[path]`
/// technique openjd-sessions uses for the helper's framer). `params.rs`
/// refers to `crate::common`, which resolves to the module below.
#[path = "../../crates/openjd-cli/src/common.rs"]
#[allow(dead_code, clippy::all)]
pub mod common;

/// See [`common`].
#[path = "../../crates/openjd-cli/src/run/params.rs"]
#[allow(dead_code, clippy::all)]
pub mod cli_params;

/// The cross-user helper's line framer (std + serde_json only).
#[path = "../../crates/openjd-sessions/src/helper/src/framer.rs"]
#[allow(dead_code, clippy::all)]
pub mod helper_framer;

/// The cross-user helper's wire protocol.
#[path = "../../crates/openjd-sessions/src/helper/src/protocol.rs"]
#[allow(dead_code, clippy::all)]
pub mod helper_protocol;

/// Report an oracle violation: a property that must hold for every input did
/// not. Panics, which libFuzzer records as a crash with the input attached.
#[macro_export]
macro_rules! oracle {
    ($cond:expr, $($fmt:tt)+) => {
        if !$cond {
            panic!("ORACLE VIOLATION [{}]: {}", stringify!($cond), format!($($fmt)+));
        }
    };
}

/// An oracle for a bug that is known and not yet fixed. A violation is
/// ignored unless `OPENJD_FUZZ_KNOWN=1`, in which case it panics like
/// [`oracle!`]. Each id must be listed in [`known::KNOWN_ISSUES`]; when the
/// bug is fixed, replace the `known_issue!` with `oracle!` and delete the
/// entry, so the fuzzer guards the fix.
#[macro_export]
macro_rules! known_issue {
    ($id:literal, $cond:expr, $($fmt:tt)+) => {
        if !$cond {
            $crate::known::hit($id, || format!($($fmt)+));
        }
    };
}

pub mod known {
    //! Bugs the fuzzer has found that are not fixed yet. Each one is
    //! reproducible from `regressions/known/<id>` with
    //! `OPENJD_FUZZ_KNOWN=1`, and documented in `fuzz/README.md`.

    /// `(id, summary)` for every open known issue.
    pub const KNOWN_ISSUES: &[(&str, &str)] = &[
        (
            "expr-parser-exponential-recovery",
            "Parsing doubles in time per level of unclosed bracket nesting inside \
             ruff_python_parser's error recovery: `\"[i f(\" * 20 + \"x\"` (101 bytes) takes \
             0.8 s in release, so a ~150-byte template field stalls `openjd check` for minutes; 363 bytes \
             of mostly `.` takes 1.6 s \
             (denial of service). Text targets skip such inputs (see \
             `skip_deep_unclosed_brackets`).",
        ),
        (
            "expr-ordering-runtime-type-check",
            "The ordering operators are registered `(T1, T2) -> bool` and refuse mismatched \
             operand types only at run time (`do_compare`; documented in \
             specs/expr/function-library.md), so `Param.Path < 1`, `A > 5 > 5` with a BOOL \
             parameter, or two `range_expr`s validate and then fail on the worker.",
        ),
        (
            "expr-boolop-static-type",
            "`a or b` / `a and b` return an operand, but with an unresolved operand the static \
             pass types the result `bool` (and may fold to a later operand), so validation \
             reports the wrong type: `Param.N or 'y'` is `string` statically, `int` at run time. \
             Errors in operands after an unresolved one are absorbed as if value-dependent, so \
             `Param.Flag or nope_fn()` (an unknown function) passes validation. The same \
             speculative evaluation of `a if c else b` with an unresolved `c` absorbs undefined \
             symbols, so `Session.*` inside a conditional in a template-scope field passes \
             validation and fails in create_job.",
        ),
        (
            "expr-flatten-union-static-type",
            "`flatten(x if c else [])` is `list[nulltype]` statically but a non-empty list at \
             run time: the union argument dispatches to the `(list[nulltype])` overload.",
        ),
        (
            "expr-unbound-return-typevar",
            "Signatures whose return type variable no parameter binds (`__add__: (list[T1], \
             list[T2]) -> list[T3]`, `(list[T1], range_expr) -> list[T2]`) lose the element type statically, and admit operands (`[False] + range_expr(...)`) that static checking then rejects; a later overloaded call \
             then picks the first overload: `min([1.5] + xs)` is `int` statically, `float` at \
             run time.",
        ),
        (
            "expr-union-arg-overload",
            "With a union-typed argument (from a conditional or a union-returning function) \
             static dispatch picks one overload's return type: `(a if c else re_findall(..)) * \
             28` is `int` statically and a list at run time; `L[::z]` on a `range_expr` is \
             `range_expr` statically but a negative step returns `list[int]`.",
        ),
        (
            "expr-dispatch-derive-mismatch",
            "`derive_return_type` and run-time dispatch can pick different overloads when an \
             argument coerces: `path + path` derives `string` (via `(string, string)`) but \
             dispatches `(path, string) -> path`, so validation and run time disagree on the type. \
             Overloads with identical parameters (`re_findall: (string, string) -> list[string]` \
             and `-> list[list[string]]`) derive the first but may return the second.",
        ),
        (
            "expr-sum-empty-float",
            "`sum` of an empty `list[float]` returns `0` (int) but its signature is \
             `(list[float]) -> float`.",
        ),
        (
            "expr-float64-serde-precision",
            "`Float64` serializes as a JSON number, and serde_json without its `float_roundtrip` \
             feature can reparse it one ULP off, so a job's floats do not round-trip through JSON.",
        ),
        (
            "expr-empty-range-transport",
            "An empty `range_expr` (e.g. `range_expr('1-3')[5:9]`) transports as \"\", which \
             does not decode, so it cannot cross the template → session boundary.",
        ),
        (
            "expr-range-structural-eq",
            "`RangeExpr` equality compares stored sub-ranges, not values: `range_expr('24-24:20') \
             != range_expr('24')` (expr report Rec 9).",
        ),
        (
            "model-float-range-unchecked-scope",
            "FLOAT task-parameter ranges skip the symbol-scope check INT/STRING/PATH ranges get: \
             `[1.5, \"{{Param.X}}\"]` with no `X` passes validation and fails in create_job. \
             Existing tests assert the current behavior; needs a spec/Python parity decision.",
        ),
        (
            "model-preprocess-path-idempotence",
            "`preprocess_job_parameters` is not idempotent for PATH / LIST[PATH] values (open \
             #388 family): under the Windows path format a URI element `s3://bucket/x` is \
             normalized to `s3:\\bucket\\x`, which a second pass treats as relative and joins \
             to the working directory.",
        ),
        (
            "snapshots-type-change-diff",
            "`diff_snapshots` on a file → directory change emits a deleted file and a new dir at \
             the same path, which the diff's own `validate()` rejects as a duplicate path.",
        ),
        (
            "snapshots-file-as-ancestor",
            "`Manifest::validate()` accepts a file that is also the ancestor of another entry \
             (`/a` and `/a/b`, including entries beneath a symlink); diff/compose then treat \
             the file's deletion as deleting the subtree, so `compose(a, [diff(a, b)]) != b`, \
             and `subtree_snapshot` with symlink collapsing emits duplicate paths.",
        ),
        (
            "snapshots-collapse-nested-symlink",
            "`subtree_snapshot` with a collapsing symlink policy pulls a symlinked directory's \
             contents into the subtree but leaves a nested symlink's absolute target as-is, so \
             the relative result fails `validate()` (`symlink_target path style mismatch`).",
        ),
        (
            "snapshots-v2025-symlink-target-dirs",
            "The v2025 encoder emits directory entries for a symlink target's ancestors; when \
             one of those is a file (e.g. `/r/a` is a file and `/r/x -> /r/a/c`, or `/r/b -> \
             /r/b/b`), the encoded manifest fails to decode with a duplicate path, though the \
             input passed `validate()`.",
        ),
        (
            "snapshots-compose-dirs",
            "`compose_snapshot_with_diffs` / `compose_diffs` synthesize ancestor dirs (\"/\" can \
             appear twice), mark deleted files as deleted dirs, and composing diffs first does not \
             always equal applying them in sequence.",
        ),
    ];

    /// Known issue `expr-parser-exponential-recovery`: should a text target
    /// skip this expression text? True when brackets nest more than 12 deep
    /// without closing, or the text is longer than 320 bytes: on malformed
    /// input ruff's error recovery grows exponentially with nesting and
    /// superlinearly with length, so the fuzzer would otherwise spend its
    /// time rediscovering the slowdown. A timeout cannot be gated like an
    /// oracle, so the input is skipped instead; `OPENJD_FUZZ_KNOWN=1`
    /// disables the skip.
    pub fn skip_deep_unclosed_brackets(text: &str) -> bool {
        if std::env::var_os("OPENJD_FUZZ_KNOWN").is_some_and(|v| v == "1") {
            return false;
        }
        if text.len() > 320 {
            return true;
        }
        let mut depth = 0i32;
        for c in text.chars() {
            match c {
                '(' | '[' | '{' => {
                    depth += 1;
                    if depth > 12 {
                        return true;
                    }
                }
                ')' | ']' | '}' => depth = (depth - 1).max(0),
                _ => {}
            }
        }
        false
    }

    /// [`skip_deep_unclosed_brackets`] applied to each `{{ ... }}`
    /// interpolation of a format string or template document only, so deep
    /// but harmless JSON/YAML nesting is still fuzzed.
    pub fn skip_deep_interpolations(text: &str) -> bool {
        let mut rest = text;
        while let Some(a) = rest.find("{{") {
            let after = &rest[a + 2..];
            let end = after.find("}}").unwrap_or(after.len());
            if skip_deep_unclosed_brackets(&after[..end]) {
                return true;
            }
            rest = &after[end.min(after.len())..];
        }
        false
    }

    /// Record a known-issue violation.
    pub fn hit(id: &str, msg: impl FnOnce() -> String) {
        assert!(
            KNOWN_ISSUES.iter().any(|(k, _)| *k == id),
            "known_issue! id {id:?} is not listed in KNOWN_ISSUES"
        );
        if std::env::var_os("OPENJD_FUZZ_KNOWN").is_some_and(|v| v == "1") {
            panic!("KNOWN ISSUE [{id}]: {}", msg());
        }
    }
}
