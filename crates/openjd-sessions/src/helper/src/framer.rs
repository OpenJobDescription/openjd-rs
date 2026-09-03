// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Bounded line framer for the cross-user helper's stdout reader.
//!
//! Turns an arbitrary stream of bytes (delivered in `fill_buf` chunks) into
//! bounded logical lines, so a workload that never emits a newline can never
//! grow the helper's line buffer without limit. Splitting is on
//! `\n` only — a lone `\r` stays inside the line — and each logical line
//! produces at most one string, so the caller can emit exactly one
//! `Response::Out` per line (never splitting a line across responses).
//!
//! This module is deliberately `std` + `serde_json` only: it is compiled into
//! the standalone helper binary (a nested Cargo crate that cannot depend on
//! `openjd-sessions`), and it is ALSO `#[path]`-included into `openjd-sessions`
//! under `cfg(all(test, unix))` so the library can unit-test it and assert
//! [`decode_backslashreplace`] stays byte-for-byte equivalent to
//! `subprocess::decode_backslashreplace`. It must therefore reference nothing
//! from `openjd-sessions`, no `nix`, and no `tokio`.

use std::borrow::Cow;

/// Maximum bytes of a single raw logical line the framer will accumulate
/// before it force-truncates and discards the remainder.
///
/// Mirrors `subprocess.rs`'s `LOG_LINE_MAX_LENGTH`. The helper is a separate
/// crate and cannot reference that constant, so the value is duplicated here;
/// the two must stay in sync (the cross-user path and the same-user path
/// truncate log lines at the same size).
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// Maximum bytes of a single serialized JSON response line the parent will
/// accept from the helper's stdout.
///
/// Mirrors `cross_user_helper.rs`'s `MAX_RESPONSE_LINE_LENGTH`, the parent's
/// hard limit when reading responses: a response line longer than this is
/// turned into a `SessionError::HelperCommunication` per line (the parent
/// reads with `Read::take(128 KiB)`). [`fit_out_payload`] uses this to keep
/// every `Response::Out` under the limit.
pub const MAX_RESPONSE_LINE_LENGTH: usize = 128 * 1024;

/// Lowercase hex digits, indexed by nibble value. Used by
/// [`decode_backslashreplace`] to render undecodable bytes as `\xNN`.
const HEX_DIGITS: [u8; 16] = *b"0123456789abcdef";

/// Decode subprocess output as UTF-8, escaping every byte that is not valid
/// UTF-8 as `\xNN` with lowercase hex.
///
/// Ported verbatim from `subprocess.rs::decode_backslashreplace` (the helper
/// cannot depend on `openjd-sessions`). A byte-for-byte equivalence test in
/// `subprocess.rs` guards the two copies against drifting apart.
///
/// This mirrors CPython's `bytes.decode("utf-8", errors="backslashreplace")`,
/// which `openjd-sessions-for-python` uses when reading subprocess output. A
/// subprocess can emit bytes that are not valid UTF-8, for example a Windows
/// DCC application writing its output in the system code page, such as Unreal
/// Engine emitting the cp1252 em dash `0x97`. Escaping those bytes rather than
/// replacing them with U+FFFD preserves the original byte values in the session
/// log, which helps identify the code page the subprocess is emitting.
///
/// Valid UTF-8, including multi-byte sequences, passes through unmodified, and a
/// borrowed string is returned without allocating when the whole input is
/// already valid.
pub fn decode_backslashreplace(bytes: &[u8]) -> Cow<'_, str> {
    // Fast path: the overwhelmingly common case is fully valid UTF-8, which
    // needs no allocation.
    if let Ok(valid) = std::str::from_utf8(bytes) {
        return Cow::Borrowed(valid);
    }

    // `utf8_chunks` splits the input into (valid UTF-8 prefix, invalid byte
    // sequence) pairs, so the valid parts need no re-validation and the invalid
    // sequences are delimited exactly as UTF-8 validation defines them.
    // CPython escapes every byte of an undecodable sequence individually, so a
    // 3-byte invalid sequence becomes three `\xNN` escapes.
    let mut out = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        for &byte in chunk.invalid() {
            out.push('\\');
            out.push('x');
            // Both indices are nibbles, so they are always < 16.
            out.push(HEX_DIGITS[(byte >> 4) as usize] as char);
            out.push(HEX_DIGITS[(byte & 0x0f) as usize] as char);
        }
    }
    Cow::Owned(out)
}

/// Truncate `s` to at most `max` bytes on a valid UTF-8 char boundary.
///
/// Matches `subprocess.rs::truncate_line`'s char-boundary behavior (it uses
/// `floor_char_boundary`); walking back to the previous boundary is the
/// std-only equivalent and never splits a multi-byte character.
fn truncate_char_boundary(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Decode a raw line and enforce the decoded-line byte bound.
///
/// `decode_backslashreplace` can expand up to 4x (each undecodable byte becomes
/// the 4-character escape `\xNN`), so a raw line already at `MAX_LINE_BYTES`
/// can decode to 4x that. The second truncation here guarantees the decoded
/// string is itself at most `MAX_LINE_BYTES`.
fn decode_and_truncate(bytes: &[u8]) -> String {
    let decoded = decode_backslashreplace(bytes);
    truncate_char_boundary(&decoded, MAX_LINE_BYTES).to_string()
}

/// serde_json's escaped length for a single `char`, matching serde_json's
/// output byte-for-byte (an equivalence test pins this against serde_json).
///
/// serde_json escapes `"` and `\` as two-character sequences, the five named
/// control characters (`\b \t \n \f \r`) as two characters, any other control
/// character below `0x20` as the six-character `\uXXXX`, and passes everything
/// else (including `0x7F` and all non-ASCII) through as its raw UTF-8 bytes.
fn json_escaped_len(c: char) -> usize {
    match c {
        '"' | '\\' => 2,
        '\u{08}' | '\u{09}' | '\u{0A}' | '\u{0C}' | '\u{0D}' => 2,
        c if (c as u32) < 0x20 => 6,
        c => c.len_utf8(),
    }
}

/// JSON-expansion guard: return the longest prefix of `s` whose serialized
/// `Response::Out` (the JSON envelope plus the trailing `\n` that `send`
/// writes) still fits within the parent's [`MAX_RESPONSE_LINE_LENGTH`].
///
/// A decoded line that is under `MAX_LINE_BYTES` can still blow past the
/// parent's response limit once serialized: a line of legal UTF-8 control
/// characters expands 6x through serde_json (`\u0001`), so 64 KiB decoded can
/// serialize to 384 KiB. Without this guard the parent
/// (`cross_user_helper.rs`) would turn every such line into a
/// `HelperCommunication` error. Computing the escaped length per character and
/// stopping before the budget is exceeded keeps every response line valid.
pub fn fit_out_payload(s: &str) -> &str {
    // `{"out":"..."}` wrapping plus the `\n` that `send` appends. The two `"`
    // that delimit the payload are part of the envelope; only the payload's
    // own escaped characters count against the remaining budget.
    const ENVELOPE: usize = r#"{"out":""}"#.len() + 1;
    let budget = MAX_RESPONSE_LINE_LENGTH - ENVELOPE;
    let mut used = 0;
    for (i, c) in s.char_indices() {
        let l = json_escaped_len(c);
        if used + l > budget {
            return &s[..i];
        }
        used += l;
    }
    s
}

/// Byte-stream to bounded-logical-line state machine.
///
/// Fed arbitrary `fill_buf` chunks via [`push`](LineFramer::push); emits one
/// string per complete logical line (split on `\n`). A line that reaches
/// `MAX_LINE_BYTES` before its newline keeps its truncated 64 KiB prefix and
/// discards the remainder up to the next `\n`; that line's single `Out` is
/// still emitted exactly once, at the line end (newline or EOF), so an
/// over-cap line has the same emit timing as an ordinary one. Call
/// [`finish`](LineFramer::finish) at EOF to flush any trailing partial line.
#[derive(Default)]
pub struct LineFramer {
    /// Raw bytes of the current line, capped at `MAX_LINE_BYTES`.
    buf: Vec<u8>,
    /// True once an over-cap line has filled `buf` with its kept 64 KiB
    /// prefix: the remainder up to the next `\n` is discarded, and the line's
    /// single `Out` is emitted at the line end (newline or EOF), not when the
    /// cap is reached.
    discarding: bool,
}

impl LineFramer {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            discarding: false,
        }
    }

    /// Feed one chunk of bytes, emitting each complete logical line through
    /// `emit`. A chunk may contain zero, one, or many newlines, and a logical
    /// line may span many chunks.
    pub fn push<F: FnMut(String)>(&mut self, mut data: &[u8], emit: &mut F) {
        while !data.is_empty() {
            match data.iter().position(|&b| b == b'\n') {
                Some(nl) => {
                    // Accumulate the bytes before the newline (a no-op once
                    // we're discarding an over-cap line), then emit the line
                    // exactly once — whether or not it hit the cap — from the
                    // kept buffer. Emitting here, at the line end, gives an
                    // over-cap line the same emit timing as an ordinary one.
                    self.accumulate(&data[..nl]);
                    let line = std::mem::take(&mut self.buf);
                    emit(decode_and_truncate(&line));
                    // Newline ends the line: reset for the next one.
                    self.discarding = false;
                    self.buf.clear();
                    data = &data[nl + 1..];
                }
                None => {
                    self.accumulate(data);
                    return;
                }
            }
        }
    }

    /// Flush the final partial line at EOF (matches the existing `read_line`
    /// EOF behavior: a trailing line without a newline is still delivered).
    /// Emits once when the buffer holds a partial line OR an over-cap line is
    /// still being discarded (its kept prefix is in `buf`), then resets.
    pub fn finish<F: FnMut(String)>(&mut self, emit: &mut F) {
        if self.discarding || !self.buf.is_empty() {
            let line = std::mem::take(&mut self.buf);
            emit(decode_and_truncate(&line));
        }
        self.discarding = false;
        self.buf.clear();
    }

    /// Append `data` (a newline-free segment) to the current line buffer. Once
    /// the buffer would exceed `MAX_LINE_BYTES`, keep the truncated 64 KiB
    /// prefix in `buf` and switch to discarding the remainder; the line's
    /// single `Out` is emitted later, at the line end (newline or EOF), not
    /// here. While discarding, further bytes for this line are dropped.
    fn accumulate(&mut self, data: &[u8]) {
        if self.discarding {
            return;
        }
        let room = MAX_LINE_BYTES - self.buf.len();
        if data.len() <= room {
            self.buf.extend_from_slice(data);
        } else {
            // Over cap: keep the truncated prefix and discard the rest of this
            // logical line. The emit is deferred to the line end.
            self.buf.extend_from_slice(&data[..room]);
            self.discarding = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── JSON-expansion guard tests ───────────────────────────────────

    const ENVELOPE: usize = r#"{"out":""}"#.len() + 1; // {"out":"..."} + send()'s \n

    #[test]
    fn fit_guard_control_chars_six_x_expansion() {
        // Legal UTF-8 control char 0x01 serializes to \u0001 through serde_json:
        // a 6x expansion per character.
        let s = "\u{01}".repeat(64 * 1024);
        let fitted = fit_out_payload(&s);
        let json = serde_json::to_string(&fitted).unwrap();
        assert!(json.len() - 2 + ENVELOPE <= MAX_RESPONSE_LINE_LENGTH);
        assert!(fitted.chars().count() + 1 > (MAX_RESPONSE_LINE_LENGTH - ENVELOPE) / 6);
    }

    #[test]
    fn fit_guard_backslashreplace_expansion() {
        // 0x97 decodes to the 4-char `\x97`; JSON escapes the backslash, so each
        // original byte costs 5 JSON bytes.
        let raw = vec![0x97u8; 64 * 1024];
        let decoded = decode_backslashreplace(&raw);
        let fitted = fit_out_payload(&decoded);
        let json = serde_json::to_string(&fitted).unwrap();
        assert!(json.len() - 2 + ENVELOPE <= MAX_RESPONSE_LINE_LENGTH);
    }

    #[test]
    fn fit_guard_noop_for_plain_ascii() {
        let s = "a".repeat(64 * 1024);
        assert_eq!(fit_out_payload(&s), &s[..]);
    }

    #[test]
    fn fit_guard_cuts_on_char_boundary() {
        let mut s = "\"".repeat((MAX_RESPONSE_LINE_LENGTH - ENVELOPE) / 2 - 1);
        s.push_str("😀😀😀");
        let fitted = fit_out_payload(&s); // not panicking proves it cuts on a char boundary
        let json = serde_json::to_string(&fitted).unwrap();
        assert!(json.len() - 2 + ENVELOPE <= MAX_RESPONSE_LINE_LENGTH);
    }

    #[test]
    fn escaped_len_matches_serde_json_exactly() {
        // A representative mixed string: the per-char lengths must sum to
        // serde_json's serialized length (minus the two delimiting quotes).
        let s = "abc\"\\\u{08}\u{09}\u{0A}\u{0C}\u{0D}\u{01}\u{1F}€😀\u{7F}";
        let computed: usize = s.chars().map(json_escaped_len).sum();
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(computed, json.len() - 2);

        // Exhaustive per-character check: every scalar value in 0..=0xFF (no
        // surrogates fall in that range) plus representative scalars at every
        // UTF-8 length and escaping class. json_escaped_len must equal
        // serde_json's own serialized length for that single character (its
        // serialization minus the two delimiting quotes).
        let representative = [
            0x7Fu32,
            0x80,
            0x7FF,
            0x800,
            0xFFFF,
            0x10000,
            0x10FFFF,
            '€' as u32,
            '😀' as u32,
        ];
        for cp in (0u32..=0xFF).chain(representative) {
            let Some(c) = char::from_u32(cp) else {
                continue; // defensive: no surrogates exist in these ranges
            };
            let serialized = serde_json::to_string(&c.to_string()).unwrap();
            assert_eq!(
                json_escaped_len(c),
                serialized.len() - 2,
                "mismatch for U+{cp:04X}"
            );
        }
    }

    #[test]
    fn out_envelope_within_response_limit_for_worst_cases() {
        // A local mirror of the helper's untagged `Response::Out`: serde
        // serializes this struct byte-for-byte the same as the real response
        // (`{"out":"..."}`), so it is a faithful stand-in for the on-wire size.
        // fit_out_payload must keep the serialized line plus send()'s trailing
        // `\n` within the parent's response limit even for the worst-case
        // JSON expansions.
        #[derive(serde::Serialize)]
        struct OutMirror<'a> {
            out: &'a str,
        }

        let all_ctrl = "\u{01}".repeat(64 * 1024);
        let backslashreplace = decode_backslashreplace(&vec![0x97u8; 64 * 1024]).into_owned();
        let quote_heavy = "\"".repeat(64 * 1024);

        for payload in [&all_ctrl, &backslashreplace, &quote_heavy] {
            let fitted = fit_out_payload(payload);
            let serialized = serde_json::to_vec(&OutMirror { out: fitted }).unwrap();
            // Strict `<` leaves room for the trailing newline that send()
            // appends (len + 1 <= MAX is equivalent to len < MAX).
            assert!(
                serialized.len() < MAX_RESPONSE_LINE_LENGTH,
                "serialized length {} + newline exceeds the response limit",
                serialized.len()
            );
        }
    }

    // ── Framing behavior tests ───────────────────────────────────────

    /// Push each chunk through a fresh framer and collect every emitted line.
    fn frame_chunks(chunks: &[&[u8]]) -> Vec<String> {
        let mut framer = LineFramer::new();
        let mut out = Vec::new();
        for chunk in chunks {
            framer.push(chunk, &mut |s| out.push(s));
        }
        out
    }

    #[test]
    fn plain_lines_split_on_newline() {
        assert_eq!(
            frame_chunks(&[b"a\nb\n"]),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn crlf_kept_for_runner_trim() {
        // The framer keeps the trailing \r; the runner's emit path does the
        // trim_end. Only \n ends the line.
        assert_eq!(frame_chunks(&[b"a\r\n"]), vec!["a\r".to_string()]);
    }

    #[test]
    fn lone_cr_stays_in_line() {
        // A bare \r (progress-bar carriage return) does not split a line.
        assert_eq!(
            frame_chunks(&[b"p 10%\rp 20%\n"]),
            vec!["p 10%\rp 20%".to_string()]
        );
    }

    #[test]
    fn multibyte_utf8_across_push_boundary() {
        // "€" (E2 82 AC) split across two pushes must reassemble into one line.
        assert_eq!(
            frame_chunks(&[b"\xE2\x82", b"\xAC\n"]),
            vec!["€".to_string()]
        );
    }

    #[test]
    fn over_cap_line_emits_single_truncated_out() {
        // 1 MiB with no newline: nothing is emitted while the over-cap line is
        // still open — the truncated 64 KiB prefix is kept and the remainder
        // discarded. The single Out (<= MAX_LINE_BYTES) is emitted only when
        // the line ends with a newline.
        let data = vec![b'a'; 1024 * 1024];
        let mut framer = LineFramer::new();
        let mut out = Vec::new();
        framer.push(&data, &mut |s| out.push(s));
        assert!(
            out.is_empty(),
            "no Out until the over-cap line ends, got {}",
            out.len()
        );
        framer.push(b"\n", &mut |s| out.push(s));
        assert_eq!(out.len(), 1, "over-cap line must emit exactly one Out");
        assert!(out[0].len() <= MAX_LINE_BYTES);
    }

    #[test]
    fn over_cap_line_flushed_once_on_finish() {
        // An over-cap line that ends at EOF (no trailing newline) still emits
        // exactly one truncated Out, from finish() — never zero, never two.
        let data = vec![b'a'; 1024 * 1024];
        let mut framer = LineFramer::new();
        let mut out = Vec::new();
        framer.push(&data, &mut |s| out.push(s));
        assert!(
            out.is_empty(),
            "no Out while the over-cap line is still open"
        );
        framer.finish(&mut |s| out.push(s));
        assert_eq!(
            out.len(),
            1,
            "over-cap line must flush exactly one Out at EOF"
        );
        assert!(out[0].len() <= MAX_LINE_BYTES);
    }

    #[test]
    fn after_discard_next_line_normal() {
        // After an over-cap line's truncated prefix, the remainder is discarded
        // up to the next \n, and the following line frames normally.
        let mut data = vec![b'a'; 1024 * 1024];
        data.extend_from_slice(b"\nnext\n");
        let mut framer = LineFramer::new();
        let mut out = Vec::new();
        framer.push(&data, &mut |s| out.push(s));
        assert_eq!(out.len(), 2);
        assert!(out[0].len() <= MAX_LINE_BYTES);
        assert_eq!(out[1], "next");
    }

    #[test]
    fn finish_flushes_partial_line() {
        // A trailing line with no newline is delivered on finish() (EOF).
        let mut framer = LineFramer::new();
        let mut out = Vec::new();
        framer.push(b"partial", &mut |s| out.push(s));
        assert!(out.is_empty());
        framer.finish(&mut |s| out.push(s));
        assert_eq!(out, vec!["partial".to_string()]);
    }

    #[test]
    fn empty_line_emits_empty_out() {
        assert_eq!(frame_chunks(&[b"\n"]), vec![String::new()]);
    }

    #[test]
    fn invalid_utf8_decoded_backslashreplace() {
        // b"a\x97b\n" -> the decoded line "a\x97b".
        assert_eq!(frame_chunks(&[b"a\x97b\n"]), vec![r"a\x97b".to_string()]);
    }
}
