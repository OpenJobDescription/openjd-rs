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
/// ignored unless `OPENJD_FUZZ_KNOWN` is `1` or names its id, in which case
/// it panics like [`oracle!`]. Each id must be listed in [`known::KNOWN_ISSUES`]; when the
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
    //! reproducible from `regressions/<target>/known-<id>[--note]` with
    //! `OPENJD_FUZZ_KNOWN=<id>`, and documented in `fuzz/README.md`.

    /// `(id, summary)` for every open known issue.
    pub const KNOWN_ISSUES: &[(&str, &str)] = &[
        (
            "model-param-type-serde-spelling",
            "`JobParameterType` serializes with serde's variant names (`\"Int\"`, \
             `\"ListListInt\"`) while its `Display` and the template spelling are `\"INT\"`, \
             `\"LIST[LIST[INT]]\"`, so a serialized `Job`'s `parameters[*].paramType` (and \
             openjd-for-js, which uses `{:?}`) does not use the spec spelling.",
        ),
        (
            "expr-escape-segment-cap",
            "`escape_format_string` writes each `{{` and `}}` as its own interpolation \
             (`}}` becomes `{{ \"}\" + \"}\" }}`), and literal text between them \
             counts as segments too, so escaping e.g. `a}}` 501 times gives a format string that \
             does not parse (`too many interpolation segments (1002); maximum is 1000`).",
        ),
        (
            "expr-unbound-return-typevar",
            "Signatures whose return type variable no parameter binds (`__add__: (list[T1], \
             list[T2]) -> list[T3]`, `(list[T1], range_expr) -> list[T2]`) lose the element type \
             statically, so a value can fall outside its static type: \
             `([L] + [[1.5] for v in E]).flatten() + range_expr('1-3')`, with `L` an empty \
             `list[float]` and `E` an empty list, is `list[float]` statically and `[1, 2, 3]` at \
             run time. An expression that runs can \
             also be rejected: the same expression wrapped in `range_expr(...)` is refused with \
             `No matching signature for range_expr(list[float])`, though it evaluates to `1-3`.",
        ),
        (
            "expr-speculative-budget-rejection",
            "The static pass evaluates speculatively what it can't decide (both branches of \
             a conditional with an unresolved test, an `and` / `or` operand after an \
             unresolved one, a comprehension over an unresolved list), and a budget error \
             there is never absorbed (specs/expr/evaluator.md), so an expression whose \
             expensive part no run evaluates is rejected: \
             `1.0 if Param.S not in [] else round(1e16, 4611686018427387903)` fails \
             validation with an operation-limit error and evaluates to 1.0 for every value \
             of `Param.S`.",
        ),
        (
            "expr-range-slice-static-type",
            "A stepped slice of an unresolved `range_expr` is typed `range_expr` whatever the \
             step (evaluator.rs, the unresolved slice path), but a negative step gives \
             `list[int]` (specs/expr/function-library.md; the signature returns \
             `range_expr | list[int]`), so `R[::-1]` gives a value outside its static type (also \
             nested in a list, or where a branch that used it as a list was dropped), and an \
             operation that needs the list is rejected though it runs: `sorted(R[::-1])` fails \
             validation with `No matching signature for sorted(range_expr)`.",
        ),
        (
            "expr-flatten-empty-element-type",
            "`flatten` of a list of empty typed lists returns an untyped empty list \
             (`list[nulltype]`), so what follows loses the element type: \
             `flatten([Param.F]).sum()` with an empty `list[float]` parameter is `float` \
             statically and `0` (an int) at run time, while `sum(Param.F)` is `0.0`; and with an \
             empty `list[bool]` parameter it is rejected statically (`sum() is not available \
             for list[bool]`) though it evaluates to `0`.",
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
            "expr-empty-range-transport",
            "An empty `range_expr` (e.g. `range_expr('1-3')[5:9]`) transports as \"\", which \
             does not decode, so it cannot cross the template → session boundary.",
        ),
        (
            "expr-range-list-hash",
            "`ExprValue::equals` compares a `range_expr` with a `list[int]` by value \
             (`range_expr('33') == [33]`), but `Hash` tags a range separately and hashes its \
             structure, so equal values hash differently. Hashing a range's elements is not \
             an option (a range can hold up to `i64::MAX` values); the list hashes would need \
             to change too.",
        ),
        (
            "model-preprocess-path-idempotence",
            "`preprocess_job_parameters` is not idempotent for PATH / LIST[PATH] values: \
             without the EXPR extension, a URI default such as `s3://bucket/x` \
             under template-dir walk-up is normalized to `s3:\\bucket\\x` (Windows) or \
             `s3:/bucket/x` (POSIX) without being joined; a second pass treats it as a submitted \
             relative path and joins it to the working directory.",
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
            "snapshots-subtree-root-symlink-empty-target",
            "`subtree_snapshot` gives a symlink whose target is the subtree root itself an \
             empty target (`/r/d/l -> /r/d` under `subtree(\"/r/d\", ExcludeEscaping)` \
             becomes `l -> \"\"`), so the result fails `validate()` (`symlink_target path \
             style mismatch for l: `).",
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
             appear twice) and mark deleted files as deleted dirs, and composing diffs first does \
             not always equal applying them in sequence where a path changes between file and \
             directory, lies below a directory a diff deletes, or resolves through a symlink: \
             the combined diff loses the change.",
        ),
    ];

    thread_local! {
        static RECORDED: std::cell::RefCell<std::collections::BTreeSet<String>> =
            const { std::cell::RefCell::new(std::collections::BTreeSet::new()) };
    }

    fn record(id: &str) {
        assert!(
            KNOWN_ISSUES.iter().any(|(k, _)| *k == id),
            "known_issue! id {id:?} is not listed in KNOWN_ISSUES"
        );
        RECORDED.with(|r| {
            if !r.borrow().contains(id) {
                r.borrow_mut().insert(id.to_string());
            }
        });
    }

    /// The known-issue ids hit on this thread since the last call (a set,
    /// so it stays bounded over a long fuzzing run). The replay tests use
    /// it to check that each `known-<id>` reproducer still reproduces.
    pub fn take_recorded() -> std::collections::BTreeSet<String> {
        RECORDED.with(|r| std::mem::take(&mut *r.borrow_mut()))
    }

    /// Record a known-issue violation, and fail if `OPENJD_FUZZ_KNOWN` is
    /// `1` (every known issue) or a comma-separated list naming `id`. One id
    /// at a time reproduces an issue that an earlier gate on the same input
    /// would otherwise report first.
    pub fn hit(id: &str, msg: impl FnOnce() -> String) {
        record(id);
        let fail = std::env::var("OPENJD_FUZZ_KNOWN")
            .is_ok_and(|v| v == "1" || v.split(',').any(|x| x.trim() == id));
        if fail {
            panic!("KNOWN ISSUE [{id}]: {}", msg());
        }
    }
}
