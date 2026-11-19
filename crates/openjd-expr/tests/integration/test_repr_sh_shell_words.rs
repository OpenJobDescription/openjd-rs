// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! `repr_sh` checked by the property Expression Language §2.2.6 defines: a
//! POSIX shell reads the output of a string or path back as exactly one word
//! equal to the input, and the output of a list of N items as N words in order.
//!
//! The spec doesn't fix the bytes (Python's `shlex.quote` and the Rust `shlex`
//! crate quote `it's` differently, and both conform), so these tests hand the
//! output to `sh` rather than comparing it with an expected string. They shell
//! out, so they run only on Unix. The byte-level tests in `test_strings.rs` pin
//! this implementation's own quoting.

#![cfg(unix)]

use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use openjd_expr::{ExprType, ExprValue, ParsedExpression, PathFormat, SymbolTable};

/// Inputs that exercise every character class POSIX §2.2 says may need quoting,
/// plus the cases where this crate's quoting differs from `shlex.quote`.
fn inputs() -> Vec<String> {
    let mut v: Vec<String> = [
        "",
        "hello",
        "hello world",
        "it's",
        "say \"hi\"",
        "a'b\"c",
        "x'y\"z",
        "''",
        "'",
        "\"",
        "a\\b",
        "a\\",
        "\\",
        "$HOME",
        "${x}",
        "$(id)",
        "a$b'c",
        "`id`",
        "!x",
        "!",
        "*",
        "a?b",
        "[ab]",
        "{a,b}",
        "~",
        "~/x",
        "#x",
        "a#b",
        "a;b",
        "a&b",
        "a|b",
        "a>b",
        "a<b",
        "(x)",
        "a\nb",
        "a\n",
        "\n",
        "a\tb",
        "a\rb",
        "-x",
        "-",
        "--flag=1",
        "-n",
        "=",
        "a=b",
        "=a",
        "if",
        "then",
        "{",
        "%",
        "^",
        ",",
        "\u{e9}",
        "\u{65e5}\u{672c}",
        "\u{1F600}",
        "\u{85}",
        "\u{a0}",
        "\u{200b}",
        "\u{2028}",
        "\u{feff}",
        "a b  c",
        " leading",
        "trailing ",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    // Every ASCII character other than NUL, alone and between two letters.
    for byte in 1u8..=0x7f {
        let c = char::from(byte);
        v.push(c.to_string());
        v.push(format!("a{c}b"));
    }
    v
}

fn repr_sh_of(value: ExprValue) -> String {
    let mut st = SymbolTable::new();
    st.set("Param.V", value).unwrap();
    let parsed = ParsedExpression::new("repr_sh(Param.V)").unwrap();
    match parsed.evaluate(&st).unwrap() {
        ExprValue::String(s) => s,
        other => panic!("repr_sh returned {other:?}, not a string"),
    }
}

static SHELL_DIR_ID: AtomicU32 = AtomicU32::new(0);

/// Run every output through one `sh` process and return the words it read for
/// each. The script prints each case's word count, then each word, all NUL
/// terminated, so empty words and words holding newlines stay unambiguous.
///
/// `sh` runs in a fresh empty directory, so if a quoting bug lets an input
/// such as `a>b` act as a redirection, the file lands there and not in the
/// source tree.
fn shell_words(outputs: &[String]) -> Vec<Vec<String>> {
    let mut script = String::new();
    for out in outputs {
        script.push_str("set --");
        if !out.is_empty() {
            script.push(' ');
            script.push_str(out);
        }
        script
            .push_str("\nprintf '%s\\0' \"$#\"\nfor w in \"$@\"; do printf '%s\\0' \"$w\"; done\n");
    }
    let dir = std::env::temp_dir().join(format!(
        "openjd_repr_sh_words_{}_{}",
        std::process::id(),
        SHELL_DIR_ID.fetch_add(1, Ordering::Relaxed),
    ));
    std::fs::create_dir(&dir).expect("create a directory for sh");
    let result = Command::new("sh")
        .arg("-c")
        .arg(&script)
        .current_dir(&dir)
        .output();
    std::fs::remove_dir_all(&dir).expect("remove the directory for sh");
    let result = result.expect("run sh");
    assert!(
        result.status.success(),
        "sh failed: {}\nscript:\n{script}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8(result.stdout).expect("sh output is UTF-8");
    let mut fields = stdout.split('\0');
    let mut cases = Vec::with_capacity(outputs.len());
    for _ in outputs {
        let count: usize = fields.next().unwrap().parse().unwrap();
        cases.push(
            (0..count)
                .map(|_| fields.next().unwrap().to_string())
                .collect(),
        );
    }
    assert_eq!(
        fields.next(),
        Some(""),
        "trailing output after the last case"
    );
    assert_eq!(fields.next(), None);
    cases
}

#[test]
fn repr_sh_string_is_read_back_as_one_equal_word() {
    let inputs = inputs();
    let outputs: Vec<String> = inputs
        .iter()
        .map(|s| repr_sh_of(ExprValue::String(s.clone())))
        .collect();
    for ((input, output), words) in inputs.iter().zip(&outputs).zip(shell_words(&outputs)) {
        assert_eq!(
            words,
            vec![input.clone()],
            "repr_sh({input:?}) = {output:?} is not read back as one word"
        );
    }
}

#[test]
fn repr_sh_path_is_read_back_as_its_string_form() {
    let inputs: Vec<String> = inputs().into_iter().filter(|s| !s.is_empty()).collect();
    let outputs: Vec<String> = inputs
        .iter()
        .map(|s| repr_sh_of(ExprValue::new_path(s, PathFormat::Posix)))
        .collect();
    for ((input, output), words) in inputs.iter().zip(&outputs).zip(shell_words(&outputs)) {
        // A POSIX path's string form can normalize the text (for example a
        // trailing `/`), so compare with what string() gives for the path.
        let string_form = ExprValue::new_path(input, PathFormat::Posix).to_display_string();
        assert_eq!(
            words,
            vec![string_form],
            "repr_sh(path({input:?})) = {output:?} is not read back as one word"
        );
    }
}

#[test]
fn repr_sh_list_is_read_back_as_one_word_per_item() {
    let inputs = inputs();
    let lists: Vec<Vec<String>> = vec![
        inputs.clone(),
        inputs.iter().rev().cloned().collect(),
        vec![String::new()],
        vec![String::new(), String::new()],
        vec!["it's".into(), String::new(), "a b".into(), "x\"y".into()],
        vec!["echo".into(), "hello world".into()],
    ];
    let outputs: Vec<String> = lists
        .iter()
        .map(|items| {
            let values = items.iter().map(|s| ExprValue::String(s.clone())).collect();
            repr_sh_of(ExprValue::make_list(values, ExprType::STRING).unwrap())
        })
        .collect();
    for ((items, output), words) in lists.iter().zip(&outputs).zip(shell_words(&outputs)) {
        assert_eq!(&words, items, "repr_sh({items:?}) = {output:?}");
    }
}

#[test]
fn repr_sh_path_list_is_read_back_as_one_word_per_item() {
    let items = ["/tmp/a b", "c", "/it's/$x", "rel/*"];
    let values = items
        .iter()
        .map(|s| ExprValue::new_path(*s, PathFormat::Posix))
        .collect();
    let output = repr_sh_of(ExprValue::make_list(values, ExprType::PATH).unwrap());
    assert_eq!(
        shell_words(std::slice::from_ref(&output)),
        vec![items.iter().map(|s| s.to_string()).collect::<Vec<_>>()],
        "repr_sh of a path list = {output:?}"
    );
}

#[test]
fn repr_sh_empty_list_is_read_back_as_no_words() {
    let output = ParsedExpression::new("repr_sh([])")
        .unwrap()
        .evaluate(&SymbolTable::new())
        .unwrap()
        .to_display_string();
    assert_eq!(output, "");
    assert_eq!(shell_words(&[output]), vec![Vec::<String>::new()]);
}
