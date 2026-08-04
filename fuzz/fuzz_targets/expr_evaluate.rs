// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Fuzz the full parse + evaluate pipeline.
//!
//! This is the highest-value target: the historically-found bugs
//! (`attempt to (add|sub|mul|neg) with overflow` in `range_expr`, `arithmetic`,
//! `list`, `math`, `string`, `regex`; char-boundary panics in `path`; silent
//! saturation in numeric coercion) all live in the *evaluator*, reachable only
//! once an expression parses and then runs against a symbol table.
//!
//! Evaluation MUST always terminate with `Ok` or `Err` — never a panic or
//! abort. The evaluator enforces its own memory and operation-count budgets,
//! so a well-behaved malicious expression is rejected with an error rather than
//! hanging; this target confirms that guarantee holds for arbitrary input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use openjd_expr::{ParsedExpression, PathFormat, SymbolTable};

/// Build a symbol table from a JSON object by calling `SymbolTable::set` for
/// each scalar entry. Dotted keys (e.g. `"Param.Foo"`) build nested scopes,
/// matching how job parameters and session variables are namespaced. Wiring
/// variables this way lets the fuzzer reach evaluator paths that depend on
/// runtime values — arithmetic on user-supplied integers, path operations on
/// user-supplied strings — which an empty environment would never exercise.
fn symtab_from_json(v: &serde_json::Value) -> SymbolTable {
    let mut st = SymbolTable::new();
    if let Some(obj) = v.as_object() {
        for (k, val) in obj {
            // Only scalar values map cleanly onto ExprValue via `set`. Ignore
            // set() conflicts (e.g. "A" and "A.B" both present) — a partial
            // table is still a useful evaluation environment.
            let _ = match val {
                serde_json::Value::String(s) => st.set(k, s.as_str()),
                serde_json::Value::Bool(b) => st.set(k, *b),
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        st.set(k, i)
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };
        }
    }
    st
}

/// Split the fuzz input into an optional JSON symbol-table document and the
/// expression source. A NUL byte separates the two halves: everything before
/// the first NUL (if it parses as a JSON object) seeds the symbol table;
/// everything after is the expression. With no NUL, the whole input is the
/// expression evaluated against an empty table.
fn split_input(data: &[u8]) -> (SymbolTable, &[u8]) {
    match data.iter().position(|&b| b == 0) {
        Some(idx) => {
            let (head, tail) = (&data[..idx], &data[idx + 1..]);
            let symtab = std::str::from_utf8(head)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                .map(|v| symtab_from_json(&v))
                .unwrap_or_default();
            (symtab, tail)
        }
        None => (SymbolTable::new(), data),
    }
}

fuzz_target!(|data: &[u8]| {
    let (symtab, expr_bytes) = split_input(data);

    let Ok(expr) = std::str::from_utf8(expr_bytes) else {
        return;
    };

    // Only expressions that parse can be evaluated. A parse error is an
    // expected outcome, not a finding.
    let Ok(parsed) = ParsedExpression::new(expr) else {
        return;
    };

    // Evaluate under BOTH path formats. `path()` operations format and split
    // paths differently on POSIX vs Windows (separator handling, drive-letter
    // parsing, prefix stripping), and the Windows branch does byte-index work
    // on the path string — so char-boundary safety on multibyte paths must
    // hold in both. The default `evaluate()` only ever uses POSIX, leaving the
    // Windows path code unfuzzed; drive both explicitly here. Errors are fine;
    // the invariant under test is "no panic, no abort, always returns".
    let _ = parsed
        .with_path_format(PathFormat::Posix)
        .evaluate(&[&symtab]);
    let _ = parsed
        .with_path_format(PathFormat::Windows)
        .evaluate(&[&symtab]);
});
