// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Build seed corpora from the repository's own fixtures.
//!
//! `cargo run --example build_corpus --no-default-features -- <out-dir> [extra-dir...]`
//!
//! Sources, so seeds look like what the code actually sees:
//! - every string literal in `crates/**/*.rs` (the test suites hold
//!   thousands of expressions, format strings, range expressions, type
//!   strings, templates, and manifests);
//! - every YAML/JSON file under `crates/` and `samples/`, plus any extra
//!   directories given (CI passes the openjd-specifications conformance
//!   suite), and the `{{ ... }}` interpolations inside them.
//!
//! Each candidate is routed to the targets whose parser accepts it.
//! Generated (`*_gen`) and binary targets get deterministic pseudo-random
//! byte seeds. `scripts/fuzz.sh corpus` runs this and then `cargo fuzz cmin`
//! to minimize before committing.

use openjd_expr::{ExprType, FormatString, ParsedExpression, RangeExpr};
use openjd_fuzz::targets::TARGETS;
use openjd_model::template::parse::document_string_to_object;
use openjd_model::{CallerLimits, DocumentType};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Extract string literals (`"..."`, `r"..."`, `r#"..."#`) from Rust source.
fn rust_strings(src: &str) -> Vec<String> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'\'' => {
                // Skip char literals ('"', '\\') but not lifetimes.
                if b.get(i + 2) == Some(&b'\'') {
                    i += 3;
                    continue;
                }
                if b.get(i + 1) == Some(&b'\\') {
                    if let Some(end) = b[i + 2..].iter().position(|&c| c == b'\'') {
                        i += end + 3;
                        continue;
                    }
                }
                i += 1;
            }
            b'r' if matches!(b.get(i + 1), Some(b'"') | Some(b'#'))
                && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')) =>
            {
                let mut j = i + 1;
                let mut hashes = 0;
                while b.get(j) == Some(&b'#') {
                    hashes += 1;
                    j += 1;
                }
                if b.get(j) != Some(&b'"') {
                    i += 1;
                    continue;
                }
                let close = format!("\"{}", "#".repeat(hashes));
                if let Some(end) = src[j + 1..].find(&close) {
                    out.push(src[j + 1..j + 1 + end].to_string());
                    i = j + 1 + end + close.len();
                } else {
                    i += 1;
                }
            }
            b'"' if i == 0 || b[i - 1] != b'b' => {
                let mut s = String::new();
                let mut j = i + 1;
                let chars: Vec<char> = src[j..].chars().take(1 << 20).collect();
                let mut k = 0;
                while k < chars.len() {
                    let c = chars[k];
                    if c == '"' {
                        break;
                    }
                    if c == '\\' && k + 1 < chars.len() {
                        k += 1;
                        match chars[k] {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            'r' => s.push('\r'),
                            '0' => s.push('\0'),
                            '\\' => s.push('\\'),
                            '"' => s.push('"'),
                            '\'' => s.push('\''),
                            '\n' => {
                                while k + 1 < chars.len() && chars[k + 1].is_whitespace() {
                                    k += 1;
                                }
                            }
                            'u' => {
                                let rest: String = chars[k..].iter().take(12).collect();
                                if let (Some(a), Some(z)) = (rest.find('{'), rest.find('}')) {
                                    if let Some(ch) = u32::from_str_radix(&rest[a + 1..z], 16)
                                        .ok()
                                        .and_then(char::from_u32)
                                    {
                                        s.push(ch);
                                    }
                                    k += z;
                                }
                            }
                            'x' => {
                                let hex: String = chars[k + 1..].iter().take(2).collect();
                                if let Ok(v) = u8::from_str_radix(&hex, 16) {
                                    s.push(v as char);
                                }
                                k += 2;
                            }
                            other => s.push(other),
                        }
                    } else {
                        s.push(c);
                    }
                    k += 1;
                }
                j += chars[..k].iter().map(|c| c.len_utf8()).sum::<usize>();
                out.push(s);
                i = j + 1;
            }
            _ => i += 1,
        }
    }
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == "target" || name == "node_modules" || name.starts_with('.') {
            continue;
        }
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// `{{ ... }}` bodies inside a text.
fn interpolations(s: &str) -> Vec<String> {
    let mut v = Vec::new();
    let mut rest = s;
    while let Some(a) = rest.find("{{") {
        let after = &rest[a + 2..];
        let Some(z) = after.find("}}") else { break };
        v.push(after[..z].trim().to_string());
        rest = &after[z + 2..];
    }
    v
}

struct Seeds(BTreeMap<&'static str, BTreeSet<Vec<u8>>>);

impl Seeds {
    fn add(&mut self, target: &'static str, data: impl Into<Vec<u8>>) {
        let data = data.into();
        if !data.is_empty() && data.len() <= 64 * 1024 {
            self.0.entry(target).or_default().insert(data);
        }
    }

    fn route_text(&mut self, s: &str) {
        let limits = CallerLimits::default();
        if s.len() < 4096 {
            if ParsedExpression::new(s).is_ok() {
                self.add("expr_parse", s);
                self.add("expr_eval_text", s);
            } else if s.len() < 256 {
                // Near-misses are useful parser seeds too.
                self.add("expr_parse", s);
            }
            if s.contains("{{") && FormatString::new(s).is_ok() {
                self.add("expr_format_string", s);
            }
            if RangeExpr::from_str(s).is_ok() {
                self.add("expr_range", s);
            }
            if ExprType::parse(s).is_ok() {
                self.add("expr_type", s);
            }
            if s.contains(" = ") {
                self.add("model_misc", s);
            }
            if s.starts_with("openjd_") {
                self.add("sessions_directives", format!("\x01\x01{s}"));
            }
            if s.contains('=') || s.starts_with('[') || s.starts_with('{') {
                self.add("cli_args", s);
            }
        }
        if s.contains("specificationVersion") {
            let ok = [DocumentType::Yaml, DocumentType::Json]
                .into_iter()
                .any(|t| document_string_to_object(s, t, &limits).is_ok());
            if ok {
                self.add("model_decode_text", s);
            }
            if openjd_snapshots::decode_manifest(s).is_ok() {
                self.add("snapshots_decode_text", s);
            }
        }
        for e in interpolations(s) {
            if ParsedExpression::new(&e).is_ok() {
                self.add("expr_eval_text", e.as_str());
                self.add("expr_parse", e.as_str());
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = PathBuf::from(
        args.first()
            .expect("usage: build_corpus <out-dir> [extra-dir...]"),
    );
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    walk(&repo.join("crates"), &mut files);
    walk(&repo.join("samples"), &mut files);
    for extra in &args[1..] {
        walk(Path::new(extra), &mut files);
    }

    let mut seeds = Seeds(BTreeMap::new());
    for f in &files {
        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("");
        let Ok(text) = std::fs::read_to_string(f) else {
            continue;
        };
        match ext {
            "rs" => {
                for s in rust_strings(&text) {
                    seeds.route_text(&s);
                }
            }
            "yaml" | "yml" | "json" => {
                seeds.route_text(&text);
                if text.contains("\"files\"") || text.contains("manifestVersion") {
                    seeds.add("snapshots_decode_text", text.as_str());
                }
            }
            _ => {}
        }
    }

    // Byte seeds for generated and binary targets: a deterministic xorshift
    // stream, varied lengths.
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    for (name, _) in TARGETS {
        let generated = name.ends_with("_gen")
            || matches!(
                *name,
                "expr_int_range"
                    | "expr_value"
                    | "expr_function_call"
                    | "expr_path"
                    | "expr_symbol_table"
                    | "sessions_framer"
                    | "sessions_embedded_files"
                    | "sessions_directives"
            );
        if generated {
            for i in 0..400 {
                let len = 8 + (i * 37) % 600;
                let mut v = Vec::with_capacity(len);
                for _ in 0..len {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    v.push(x as u8);
                }
                seeds.add(name, v);
            }
        }
    }

    for (name, _) in TARGETS {
        let dir = out.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let set = seeds.0.get(name).cloned().unwrap_or_default();
        for (i, data) in set.iter().enumerate() {
            std::fs::write(dir.join(format!("seed-{i:05}")), data).unwrap();
        }
        println!("{name}: {} seeds", set.len());
    }
}
