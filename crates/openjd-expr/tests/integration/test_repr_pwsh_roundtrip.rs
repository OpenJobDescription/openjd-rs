// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Round-trip tests for `repr_pwsh`: evaluate the repr, hand the literal to a
//! real PowerShell interpreter, and verify PowerShell reconstructs the exact
//! value. Windows-only, since that is where `powershell.exe` is guaranteed.
#![cfg(windows)]

use openjd_expr::{ParsedExpression, SymbolTable};
use std::sync::atomic::{AtomicU32, Ordering};

fn repr_pwsh(expr: &str) -> String {
    ParsedExpression::new(&format!("repr_pwsh({expr})"))
        .and_then(|p| p.evaluate(&SymbolTable::new()))
        .unwrap()
        .to_display_string()
}

static SCRIPT_ID: AtomicU32 = AtomicU32::new(0);

/// Assign `literal` to a PowerShell variable and echo it back as JSON.
///
/// The script is written to a temp file (avoiding a second layer of
/// command-line quoting) with a UTF-8 BOM so Windows PowerShell 5.1 reads
/// non-ASCII content correctly.
fn pwsh_eval_to_json(literal: &str) -> serde_json::Value {
    let script = format!(
        "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8\n\
         $v = {literal}\n\
         ConvertTo-Json -InputObject $v -Compress -Depth 5\n"
    );
    let path = std::env::temp_dir().join(format!(
        "openjd_pwsh_roundtrip_{}_{}.ps1",
        std::process::id(),
        SCRIPT_ID.fetch_add(1, Ordering::Relaxed),
    ));
    std::fs::write(&path, format!("\u{FEFF}{script}")).unwrap();
    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&path)
        .output()
        .expect("failed to launch powershell.exe");
    std::fs::remove_file(&path).ok();
    assert!(
        output.status.success(),
        "powershell failed for literal {literal:?}:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!("powershell output for {literal:?} is not JSON ({e}): {stdout:?}")
    })
}

/// Evaluate `repr_pwsh(expr)`, run the literal through PowerShell, and
/// assert PowerShell reconstructs `expected`.
fn assert_roundtrip(expr: &str, expected: serde_json::Value) {
    let literal = repr_pwsh(expr);
    let actual = pwsh_eval_to_json(&literal);
    assert_eq!(
        actual, expected,
        "repr_pwsh({expr}) rendered {literal:?}, which PowerShell read back differently"
    );
}

use serde_json::json;

#[test]
fn roundtrip_string_with_quotes_and_dollar() {
    assert_roundtrip(
        r#""it's $var \"quoted\" 100% done""#,
        json!("it's $var \"quoted\" 100% done"),
    );
}

#[test]
fn roundtrip_windows_path_string() {
    assert_roundtrip(r#"'C:\\out\\a.exr'"#, json!("C:\\out\\a.exr"));
}

#[test]
fn roundtrip_non_ascii_string() {
    assert_roundtrip("'café ☃'", json!("café ☃"));
}

#[test]
fn roundtrip_flat_string_list() {
    assert_roundtrip(r#"["it's", "b c", "d"]"#, json!(["it's", "b c", "d"]));
}

#[test]
fn roundtrip_flat_int_list() {
    assert_roundtrip("[1, 2, 3]", json!([1, 2, 3]));
}

#[test]
fn roundtrip_mixed_scalar_reprs() {
    assert_roundtrip("[1.5, 2.5]", json!([1.5, 2.5]));
    assert_roundtrip("[true, false]", json!([true, false]));
}

#[test]
fn roundtrip_nested_string_lists() {
    assert_roundtrip(
        r#"[["a b", "it's"], ["c"]]"#,
        json!([["a b", "it's"], ["c"]]),
    );
}

#[test]
fn roundtrip_nested_int_lists() {
    assert_roundtrip("[[1, 2], [3]]", json!([[1, 2], [3]]));
}

#[test]
fn roundtrip_single_nested_list_is_not_flattened() {
    // The regression this file exists for: `@(@(1, 2))` flattens to
    // `@(1, 2)` in PowerShell; repr_pwsh must emit `@(,@(1, 2))`.
    assert_roundtrip("[[1, 2]]", json!([[1, 2]]));
}

#[test]
fn roundtrip_empty_list() {
    assert_roundtrip("[]", json!([]));
}
