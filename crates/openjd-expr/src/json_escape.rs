// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! JSON string escaping, shared by list display strings and `repr_json`.
//!
//! Both escape what JSON requires — `"`, `\`, and everything below `U+0020`.
//! They differ only on non-ASCII, which the spec leaves to the implementation:
//! [`write_escaped`] preserves it (display strings), [`write_escaped_ascii`]
//! encodes it as `\uXXXX` (`repr_json`, which is specified to). See
//! `specs/expr/values.md` ("List display strings").

use std::fmt::Write;

/// Escape a string for JSON output, preserving non-ASCII characters verbatim.
pub(crate) fn write_escaped(s: &str, buf: &mut String) {
    write_escaped_inner(s, buf, false);
}

/// Escape a string for JSON output, encoding non-ASCII characters as `\uXXXX`
/// (with surrogate pairs for characters above `U+FFFF`). Matches Python's
/// `json.dumps(ensure_ascii=True)`.
pub(crate) fn write_escaped_ascii(s: &str, buf: &mut String) {
    write_escaped_inner(s, buf, true);
}

fn write_escaped_inner(s: &str, buf: &mut String, ensure_ascii: bool) {
    for c in s.chars() {
        match c {
            '"' => buf.push_str("\\\""),
            '\\' => buf.push_str("\\\\"),
            '\x08' => buf.push_str("\\b"),
            '\x0c' => buf.push_str("\\f"),
            '\n' => buf.push_str("\\n"),
            '\r' => buf.push_str("\\r"),
            '\t' => buf.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(buf, "\\u{:04x}", c as u32);
            }
            c if ensure_ascii && !c.is_ascii() => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    let _ = write!(buf, "\\u{unit:04x}");
                }
            }
            c => buf.push(c),
        }
    }
}
