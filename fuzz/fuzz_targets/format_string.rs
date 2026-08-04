// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Fuzz format-string parsing and resolution.
//!
//! Format strings (`"{{ Param.Foo }}/output"`) embed expressions in literal
//! text and are pervasive in job templates. Parsing splits literal vs.
//! expression segments; resolution evaluates each embedded expression and
//! concatenates. The contextual-keyword rewriting the format-string layer
//! performs was the source of the "string literal silently rewritten"
//! divergence, and any embedded expression can reach the same arithmetic /
//! path / coercion code as the standalone evaluator. Neither parse nor
//! resolve may panic on arbitrary input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use openjd_expr::{FormatString, FormatStringOptions, SymbolTable};

/// Build a symbol table from a JSON object via `SymbolTable::set`, mapping
/// scalar values onto `ExprValue`. Dotted keys build nested scopes. See
/// `expr_evaluate.rs` for the rationale on wiring variables into the fuzzer.
fn symtab_from_json(v: &serde_json::Value) -> SymbolTable {
    let mut st = SymbolTable::new();
    if let Some(obj) = v.as_object() {
        for (k, val) in obj {
            let _ = match val {
                serde_json::Value::String(s) => st.set(k, s.as_str()),
                serde_json::Value::Bool(b) => st.set(k, *b),
                serde_json::Value::Number(n) => match n.as_i64() {
                    Some(i) => st.set(k, i),
                    None => continue,
                },
                _ => continue,
            };
        }
    }
    st
}

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
    let (symtab, fmt_bytes) = split_input(data);

    let Ok(fmt) = std::str::from_utf8(fmt_bytes) else {
        return;
    };

    let Ok(parsed) = FormatString::new(fmt) else {
        return;
    };

    // Resolve to a String. Default options use the POSIX path format and the
    // default function library, matching template-context resolution. Errors
    // are expected; a panic is a finding.
    let opts = FormatStringOptions::new();
    let _ = parsed.resolve_string_with(&symtab, &opts);
});
