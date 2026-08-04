// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Fuzz `copy_symbol_value`.
//!
//! `copy_symbol_value(symbol, source, dest)` walks a dotted symbol name
//! (`Param.Foo.Bar`) into a source symbol table and copies the matching value
//! (and any nested subtable) into a destination table. It splits on `.` and
//! indexes into the parsed parts, so an adversarial symbol name — empty
//! segments, trailing dots, very deep nesting, names that collide with
//! existing entries — must not panic. It returns `()`, so the only observable
//! failure is a panic/abort.

#![no_main]

use libfuzzer_sys::fuzz_target;
use openjd_expr::format_string::copy_symbol_value;
use openjd_expr::SymbolTable;

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

// First NUL splits a JSON object (the source table) from the symbol name.
fuzz_target!(|data: &[u8]| {
    let (src, symbol) = match data.iter().position(|&b| b == 0) {
        Some(i) => {
            let source = std::str::from_utf8(&data[..i])
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                .map(|v| symtab_from_json(&v))
                .unwrap_or_default();
            (source, &data[i + 1..])
        }
        None => (SymbolTable::new(), data),
    };
    let Ok(symbol) = std::str::from_utf8(symbol) else {
        return;
    };
    let mut dest = SymbolTable::new();
    copy_symbol_value(symbol, &src, &mut dest);
});
