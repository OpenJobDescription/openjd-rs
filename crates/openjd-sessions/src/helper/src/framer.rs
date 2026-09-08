// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Bounded line framer for the cross-user helper's stdout reader.
//!
//! Currently wired into the Unix runner only; the Windows runner is covered by a follow-up.
//!
//! Turns arbitrary byte chunks (from `fill_buf`) into bounded logical lines so
//! a newline-less workload cannot grow the line buffer without limit. Splits on
//! `\n` only (a lone `\r` stays in the line); each line yields one string, so
//! the caller emits exactly one `Response::Out` per line.
//!
//! Deliberately `std` + `serde_json` only: it compiles into the standalone
//! helper binary (a nested crate that cannot depend on `openjd-sessions`) and
//! is also `#[path]`-included into `openjd-sessions` under `cfg(all(test,
//! unix))` for unit tests and the `decode_backslashreplace` equivalence check.
//! So it references nothing from `openjd-sessions`, `nix`, or `tokio`.

use std::borrow::Cow;

/// Max bytes of a raw logical line before truncation; the remainder is
/// discarded.
///
/// Mirrors `subprocess.rs`'s `LOG_LINE_MAX_LENGTH` (duplicated because the
/// helper is a separate crate); the two must stay in sync.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// Max bytes of a serialized JSON response line the parent accepts.
///
/// Mirrors the parent's hard limit (`Read::take(128 KiB)`): a longer line
/// becomes a per-line `SessionError::HelperCommunication`. [`fit_out_payload`]
/// keeps every `Response::Out` under it.
pub const MAX_RESPONSE_LINE_LENGTH: usize = 128 * 1024;

/// Lowercase hex digits by nibble value, for rendering bytes as `\xNN`.
const HEX_DIGITS: [u8; 16] = *b"0123456789abcdef";

/// Decode bytes as UTF-8, escaping each invalid byte as `\xNN` (lowercase hex).
///
/// Ported verbatim from `subprocess.rs::decode_backslashreplace` (the helper
/// cannot depend on `openjd-sessions`); an equivalence test there pins the two.
///
/// Mirrors CPython's `bytes.decode("utf-8", errors="backslashreplace")`.
/// Escaping (vs U+FFFD) preserves the original byte values in the log, which
/// helps identify a subprocess emitting a non-UTF-8 code page (e.g. cp1252).
/// Valid UTF-8 passes through and is returned borrowed without allocating.
pub fn decode_backslashreplace(bytes: &[u8]) -> Cow<'_, str> {
    // Fast path: fully valid UTF-8 needs no allocation.
    if let Ok(valid) = std::str::from_utf8(bytes) {
        return Cow::Borrowed(valid);
    }

    // `utf8_chunks` yields (valid prefix, invalid bytes) pairs. Like CPython,
    // escape every byte of an invalid sequence individually.
    let mut out = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        for &byte in chunk.invalid() {
            out.push('\\');
            out.push('x');
            // Nibbles are always < 16.
            out.push(HEX_DIGITS[(byte >> 4) as usize] as char);
            out.push(HEX_DIGITS[(byte & 0x0f) as usize] as char);
        }
    }
    Cow::Owned(out)
}

/// Truncate `s` to at most `max` bytes on a UTF-8 char boundary.
///
/// std-only equivalent of `subprocess.rs::truncate_line`'s
/// `floor_char_boundary`; never splits a multi-byte character.
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

/// Decode a raw line and re-cap it at `MAX_LINE_BYTES`.
///
/// Decoding can expand up to 4x (each bad byte becomes `\xNN`), so a raw line
/// at the cap can decode to 4x; this second truncation re-bounds the result.
fn decode_and_truncate(bytes: &[u8]) -> String {
    let decoded = decode_backslashreplace(bytes);
    truncate_char_boundary(&decoded, MAX_LINE_BYTES).to_string()
}

/// serde_json's escaped byte length for one `char` (an equivalence test pins
/// this against serde_json).
///
/// `"` and `\` and the five named controls (`\b \t \n \f \r`) escape to 2 bytes;
/// other controls below `0x20` to `\uXXXX` (6); everything else (incl. `0x7F`
/// and non-ASCII) passes through as raw UTF-8.
fn json_escaped_len(c: char) -> usize {
    match c {
        '"' | '\\' => 2,
        '\u{08}' | '\u{09}' | '\u{0A}' | '\u{0C}' | '\u{0D}' => 2,
        c if (c as u32) < 0x20 => 6,
        c => c.len_utf8(),
    }
}

/// JSON-expansion guard: longest prefix of `s` whose serialized `Response::Out`
/// (envelope plus `send`'s trailing `\n`) fits [`MAX_RESPONSE_LINE_LENGTH`].
///
/// A sub-`MAX_LINE_BYTES` line can still overflow once serialized: control
/// characters expand 6x (`\u0001`), so 64 KiB decoded can reach 384 KiB.
/// Summing escaped lengths and stopping at the budget keeps every line valid
/// and avoids a per-line `HelperCommunication` error.
pub fn fit_out_payload(s: &str) -> &str {
    // `{"out":"..."}` envelope plus `send`'s `\n`. The delimiting `"` are
    // envelope; only the payload's escaped chars count against the budget.
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
/// [`push`](LineFramer::push) takes `fill_buf` chunks and emits one string per
/// `\n`-terminated line. An over-cap line keeps its truncated 64 KiB prefix,
/// discards the rest to the next `\n`, and still emits its single `Out` once at
/// the line end (same timing as any line). [`finish`](LineFramer::finish)
/// flushes a trailing partial line at EOF.
#[derive(Default)]
pub struct LineFramer {
    /// Raw bytes of the current line, capped at `MAX_LINE_BYTES`.
    buf: Vec<u8>,
    /// Set once an over-cap line filled `buf`: the rest up to the next `\n` is
    /// discarded and the `Out` still emits at the line end, not at the cap.
    discarding: bool,
}

impl LineFramer {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            discarding: false,
        }
    }

    /// Feed one chunk, emitting each complete line through `emit`. A chunk may
    /// hold any number of newlines; a line may span many chunks.
    pub fn push<F: FnMut(String)>(&mut self, mut data: &[u8], emit: &mut F) {
        while !data.is_empty() {
            match data.iter().position(|&b| b == b'\n') {
                Some(nl) => {
                    // Accumulate up to the newline (no-op while discarding),
                    // then emit the kept buffer once at the line end, giving an
                    // over-cap line the same timing as an ordinary one.
                    self.accumulate(&data[..nl]);
                    let line = std::mem::take(&mut self.buf);
                    emit(decode_and_truncate(&line));
                    // Newline ends the line; reset (buf already emptied above).
                    self.discarding = false;
                    data = &data[nl + 1..];
                }
                None => {
                    self.accumulate(data);
                    return;
                }
            }
        }
    }

    /// Flush the final partial line at EOF (a trailing newline-less line is
    /// still delivered). Emits once when `buf` holds a partial or over-cap
    /// prefix, then resets.
    pub fn finish<F: FnMut(String)>(&mut self, emit: &mut F) {
        if self.discarding || !self.buf.is_empty() {
            let line = std::mem::take(&mut self.buf);
            emit(decode_and_truncate(&line));
        }
        // buf is already empty here.
        self.discarding = false;
    }

    /// Append a newline-free segment to `buf`. On exceeding `MAX_LINE_BYTES`,
    /// keep the truncated prefix and start discarding the rest; the `Out` emits
    /// later at the line end. While discarding, further bytes are dropped.
    fn accumulate(&mut self, data: &[u8]) {
        if self.discarding {
            return;
        }
        let room = MAX_LINE_BYTES - self.buf.len();
        if data.len() <= room {
            self.buf.extend_from_slice(data);
        } else {
            // Over cap: keep the prefix, discard the rest; emit at line end.
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
        // 0x01 serializes to \u0001: 6x expansion.
        let s = "\u{01}".repeat(64 * 1024);
        let fitted = fit_out_payload(&s);
        let json = serde_json::to_string(&fitted).unwrap();
        assert!(json.len() - 2 + ENVELOPE <= MAX_RESPONSE_LINE_LENGTH);
        assert!(fitted.chars().count() + 1 > (MAX_RESPONSE_LINE_LENGTH - ENVELOPE) / 6);
    }

    #[test]
    fn fit_guard_backslashreplace_expansion() {
        // 0x97 -> `\x97` (4 chars), then JSON-escaped backslash: 5 bytes/byte.
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
        // Per-char lengths must sum to serde_json's length (minus 2 quotes).
        let s = "abc\"\\\u{08}\u{09}\u{0A}\u{0C}\u{0D}\u{01}\u{1F}€😀\u{7F}";
        let computed: usize = s.chars().map(json_escaped_len).sum();
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(computed, json.len() - 2);

        // Exhaustive check: every scalar 0..=0xFF plus representatives at each
        // UTF-8 length must match serde_json's length (minus 2 quotes).
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
        // Mirror of the untagged `Response::Out` (`{"out":"..."}`), serialized
        // identically to the real response. fit_out_payload must keep it plus
        // send()'s `\n` within the response limit for worst-case expansions.
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
            // Strict `<` leaves room for send()'s trailing newline.
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
        // Framer keeps trailing \r (runner trims); only \n ends the line.
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
        // 1 MiB, no newline: nothing emits while the line is open; the single
        // truncated Out (<= MAX_LINE_BYTES) emits only at the newline.
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
        // Over-cap line ending at EOF emits exactly one truncated Out via finish().
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
        // After an over-cap line, the following line frames normally.
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
