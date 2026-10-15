// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! openjd-sessions targets: the inputs a running job's subprocess and
//! template control on the worker.
//!
//! - **Redaction never leaks** (REDACTED_ENV_VARS): once a line registers a
//!   secret with `openjd_redacted_env`, neither that line's display form nor
//!   any later output line contains it.
//! - **Environment changes are spawnable** (#360): no `SetEnv` /
//!   `RedactedEnv` with a NUL or an invalid name reaches the session.
//! - **Output decoding** (#298, 1e1de45, #366, #369): the helper's framer and
//!   the in-process decoder agree byte for byte, framing is independent of
//!   how the stream is chunked, and every framed line fits the response cap.
//! - **Embedded files stay inside the session directory** (#359, 7d4c950),
//!   and the session accepts every filename template validation accepted.

use crate::gen::{self, flip, one_in, pick, upto};
use crate::helper_framer::{self, LineFramer, MAX_LINE_BYTES, MAX_RESPONSE_LINE_LENGTH};
use crate::helper_protocol::{constant_time_eq, Command, Response};
use crate::oracle;
use arbitrary::Unstructured;
use openjd_model::job::EmbeddedFile;
use openjd_model::types::{EndOfLine, FileType};
use openjd_sessions::embedded_files::{
    convert_line_endings, symtab_key, EmbeddedFiles, EmbeddedFilesScope,
};
use openjd_sessions::fuzzing::{
    decode_backslashreplace, process_line, redact_openjd_redacted_env_requests, truncate_line,
    ActionFilter, LOG_LINE_MAX_LENGTH,
};
use openjd_sessions::ActionMessage;
use std::path::{Path, PathBuf};

fn env_name_ok(name: &str) -> bool {
    let mut c = name.chars();
    matches!(c.next(), Some(ch) if ch == '_' || ch.is_ascii_alphabetic())
        && c.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

/// A stream of subprocess output lines through the action filter.
pub fn directives(data: &[u8]) {
    let mut u = Unstructured::new(&data[..data.len().min(2)]);
    let echo = flip(&mut u);
    let redactions = flip(&mut u);
    let body = &data[data.len().min(2)..];

    let text = decode_backslashreplace(body);
    let mut filter = ActionFilter::new("s", echo, redactions);
    let mut secrets: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = truncate_line(raw);
        oracle!(
            line.len() <= LOG_LINE_MAX_LENGTH,
            "truncate_line left {} bytes",
            line.len()
        );
        oracle!(raw.starts_with(line), "truncate_line is not a prefix");
        let (display, _pass, messages, _failed) = process_line(line, &mut filter, "s");
        let mut new_secrets = Vec::new();
        for m in &messages {
            match m {
                ActionMessage::Progress(p) => {
                    oracle!(
                        (0.0..=100.0).contains(p),
                        "progress {p} out of range from {line:?}"
                    )
                }
                ActionMessage::SetEnv { name, value }
                | ActionMessage::RedactedEnv { name, value } => {
                    oracle!(
                        !name.contains('\0') && !value.contains('\0'),
                        "env change with a NUL from {line:?}"
                    );
                    oracle!(
                        env_name_ok(name),
                        "env change with invalid name {name:?} from {line:?}"
                    );
                    if matches!(m, ActionMessage::RedactedEnv { .. }) {
                        new_secrets.push(value.clone());
                    }
                }
                ActionMessage::UnsetEnv { name } => {
                    oracle!(
                        env_name_ok(name),
                        "unset with invalid name {name:?} from {line:?}"
                    )
                }
                _ => {}
            }
        }
        // The registering line must hide the secret too. The directive's own
        // fixed text (`openjd_redacted_env: NAME=`) is not secret and stays
        // visible, so only the rest of the line is checked; secrets made of
        // the mask character, or too short to mean anything, are skipped.
        let visible: &str = if let Some(rest) = display.strip_prefix("openjd_redacted_env: ") {
            rest.split_once('=').map(|(_, v)| v).unwrap_or(rest)
        } else if display.starts_with("openjd_") {
            display.split_once(": ").map(|(_, v)| v).unwrap_or(&display)
        } else {
            &display
        };
        for s in new_secrets.iter().chain(secrets.iter()) {
            if s.len() >= 3 && !s.contains('*') {
                oracle!(
                    !visible.contains(s.as_str()),
                    "secret {s:?} visible in output {display:?} (input {line:?})"
                );
            }
        }
        secrets.extend(new_secrets);
        let _ = filter.min_log_level();
    }

    let cmd = String::from_utf8_lossy(body);
    let redacted = redact_openjd_redacted_env_requests(&cmd);
    if let Some(pos) = cmd.find("openjd_redacted_env:") {
        let secret = &cmd[pos + "openjd_redacted_env:".len()..];
        if secret.trim().len() >= 3 && !secret.contains('*') && !cmd[..pos].contains(secret) {
            oracle!(
                !redacted.contains(secret),
                "redact_openjd_redacted_env_requests leaked {secret:?}"
            );
        }
    }
}

/// The cross-user helper's output framer, protocol parsing, and the
/// in-process decoder.
pub fn framer(data: &[u8]) {
    // Differential: the helper's decoder is a copy of the session's.
    let a = decode_backslashreplace(data);
    let b = helper_framer::decode_backslashreplace(data);
    oracle!(a == b, "decoders disagree on {data:?}: {a:?} vs {b:?}");
    if let Ok(s) = std::str::from_utf8(data) {
        oracle!(a == s, "valid UTF-8 changed by decoding");
    }

    // Framing is independent of chunking.
    let mut u = Unstructured::new(&data[..data.len().min(8)]);
    let whole = {
        let mut f = LineFramer::new();
        let mut out = Vec::new();
        f.push(data, &mut |l| out.push(l));
        f.finish(&mut |l| out.push(l));
        out
    };
    let chunked = {
        let mut f = LineFramer::new();
        let mut out = Vec::new();
        let mut rest = data;
        while !rest.is_empty() {
            let n = (u.int_in_range(1..=7).unwrap_or(7) as usize).min(rest.len());
            f.push(&rest[..n], &mut |l| out.push(l));
            rest = &rest[n..];
        }
        f.finish(&mut |l| out.push(l));
        out
    };
    oracle!(whole == chunked, "framing depends on chunk boundaries");
    let newlines = data.iter().filter(|&&b| b == b'\n').count();
    let trailing = !data.is_empty() && *data.last().unwrap() != b'\n';
    oracle!(
        whole.len() == newlines + trailing as usize,
        "{} lines framed from {newlines} newlines (trailing partial: {trailing})",
        whole.len()
    );
    for line in &whole {
        let fitted = helper_framer::fit_out_payload(line);
        oracle!(line.starts_with(fitted), "fit_out_payload is not a prefix");
        let json = serde_json::to_string(&Response::Out {
            out: fitted.to_string(),
        })
        .unwrap();
        oracle!(
            json.len() < MAX_RESPONSE_LINE_LENGTH,
            "framed response line is {} bytes, cap {MAX_RESPONSE_LINE_LENGTH}",
            json.len()
        );
    }
    // A long line is truncated to the cap (its decoded form may expand).
    let long = vec![b'x'; MAX_LINE_BYTES + 3];
    let mut f = LineFramer::new();
    let mut out = Vec::new();
    f.push(&long, &mut |l| out.push(l));
    f.finish(&mut |l| out.push(l));
    oracle!(
        out.len() == 1 && out[0].len() <= MAX_LINE_BYTES,
        "over-cap line not truncated"
    );

    // Helper command parsing never panics; tokens compare by value.
    if let Ok(cmd) = serde_json::from_slice::<Command>(data) {
        let t = cmd.token().as_bytes();
        oracle!(constant_time_eq(t, t), "constant_time_eq(x, x) is false");
    }
    let half = data.len() / 2;
    oracle!(
        constant_time_eq(&data[..half], &data[half..]) == (data[..half] == data[half..]),
        "constant_time_eq disagrees with =="
    );
}

/// Embedded-file path allocation and line-ending conversion.
pub fn embedded_files(data: &[u8]) {
    let mut u = Unstructured::new(data);
    let root: PathBuf = if cfg!(windows) {
        PathBuf::from("C:\\openjd\\session\\files")
    } else {
        PathBuf::from("/openjd/session/files")
    };
    let n = upto(&mut u, 3) + 1;
    let mut files = Vec::new();
    for _ in 0..n {
        let filename = if one_in(&mut u, 2) {
            gen::string(&mut u)
        } else {
            let base = pick(
                &mut u,
                &["a.sh", "x", ".", "..", "C:x", "a:b", ". .", "con"],
            )
            .to_string();
            format!(
                "{base}{}",
                if one_in(&mut u, 3) {
                    gen::string(&mut u)
                } else {
                    String::new()
                }
            )
        };
        files.push(EmbeddedFile {
            name: gen::ident(&mut u),
            file_type: FileType::Text,
            filename: Some(filename),
            data: None,
            runnable: Some(flip(&mut u)),
            end_of_line: None,
        });
    }
    let scope = if flip(&mut u) {
        EmbeddedFilesScope::Step
    } else {
        EmbeddedFilesScope::Env
    };
    let mut ef = EmbeddedFiles::new(scope, root.clone(), "s")
        .with_user(None)
        .with_limits(openjd_sessions::SessionLimits::default());
    #[cfg(unix)]
    let _ = openjd_sessions::PosixSessionUser::new("user", Some("group"));
    let mut st = openjd_expr::SymbolTable::new();
    let accepted = ef.allocate_file_paths(&files, &mut st).is_ok();
    if accepted {
        for f in &files {
            let key = symtab_key(scope, &f.name);
            let Some(v) = st.get_value(&key) else {
                continue;
            };
            let s = v.to_display_string();
            let p = Path::new(&s);
            oracle!(
                p.parent() == Some(root.as_path()),
                "embedded file {:?} allocated outside the files directory: {s:?}",
                f.filename
            );
        }
    }

    // Differential with template validation: a filename the model accepts
    // must be accepted by the session (the session check is only a
    // defense-in-depth repeat of the spec rule).
    for f in &files {
        let fname = f.filename.as_deref().unwrap_or("");
        let doc = serde_json::json!({
            "specificationVersion": "jobtemplate-2023-09",
            "name": "J",
            "steps": [{"name": "S", "script": {
                "actions": {"onRun": {"command": "x"}},
                "embeddedFiles": [{"name": "F", "type": "TEXT", "filename": fname, "data": "d"}],
            }}],
        });
        let model_ok = openjd_model::decode_job_template(doc, None, &Default::default()).is_ok();
        let mut single = EmbeddedFiles::new(EmbeddedFilesScope::Step, root.clone(), "s");
        let one = EmbeddedFile {
            name: "F".into(),
            ..f.clone()
        };
        let session_ok = single
            .allocate_file_paths(&[one], &mut openjd_expr::SymbolTable::new())
            .is_ok();
        // The session's documented Windows-only rules (a `:` anywhere; a name
        // of only dots and spaces) are host-specific, so the platform-neutral
        // model cannot apply them.
        let windows_only =
            cfg!(windows) && (fname.contains(':') || fname.trim_end_matches([' ', '.']).is_empty());
        oracle!(
            !model_ok || session_ok || windows_only,
            "filename {fname:?} passes template validation but the session rejects it"
        );
    }

    // Line endings.
    let text = gen::string(&mut u);
    // Both conversions keep every line break and only rewrite `\r\n` pairs;
    // a lone `\r` is data (it may then sit before a converted `\n`).
    let newlines = text.matches('\n').count();
    let lf = String::from_utf8(convert_line_endings(&text, EndOfLine::Lf)).unwrap();
    oracle!(
        lf.matches('\n').count() == newlines,
        "LF conversion changed the line count of {text:?}"
    );
    oracle!(
        lf.len() == text.len() - text.matches("\r\n").count(),
        "LF conversion of {text:?} removed more than the CR of each CRLF"
    );
    let crlf = String::from_utf8(convert_line_endings(&text, EndOfLine::Crlf)).unwrap();
    oracle!(
        crlf.matches('\n').count() == newlines,
        "CRLF conversion changed the line count of {text:?}"
    );
    oracle!(
        crlf.match_indices('\n')
            .all(|(i, _)| i > 0 && crlf.as_bytes()[i - 1] == b'\r'),
        "CRLF conversion left a bare LF in {text:?}"
    );
    let _ = convert_line_endings(&text, EndOfLine::Auto);
}
