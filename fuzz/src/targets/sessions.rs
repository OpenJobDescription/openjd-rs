// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! openjd-sessions targets: the inputs a running job's subprocess and
//! template control on the worker.
//!
//! - **Redaction never leaks** (REDACTED_ENV_VARS): once a line registers a
//!   secret with `openjd_redacted_env`, neither that line's display form nor
//!   any later output line contains it, and every later output line is
//!   masked exactly as an independent reference computes (so partial leaks
//!   at overlapping or adjacent secrets fail too).
//! - **Environment changes are spawnable**: no `SetEnv` /
//!   `RedactedEnv` with a NUL or an invalid name reaches the session.
//! - **Output decoding**: the helper's framer and
//!   the in-process decoder agree byte for byte, framing is independent of
//!   how the stream is chunked, and every framed line fits the response cap.
//! - **Embedded files stay inside the session directory**,
//!   and the session accepts every filename template validation accepted.
//! - **Ending-only mode**: over any sequence of states, the session
//!   reports `ReadyEnding` exactly when a reference model's stored state is
//!   `Ready` and it is ending-only, and `clear_ending_only` succeeds only
//!   from `ReadyEnding`, leaving `Ready`, and otherwise fails with
//!   `InvalidState` and changes nothing.

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

/// `<directive>NAME=value` in its plainest form (`openjd_env: `,
/// `openjd_redacted_env: `): an ASCII identifier, a non-empty value with no
/// quote, NUL, or surrounding whitespace. The filter must accept every such
/// line.
fn well_formed_assignment<'a>(line: &'a str, directive: &str) -> Option<(&'a str, &'a str)> {
    let (name, value) = line.strip_prefix(directive)?.split_once('=')?;
    let ident = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    let plain =
        !value.is_empty() && value.trim() == value && !value.contains(['"', '\0', '\r', '\n']);
    (ident && plain).then_some((name, value))
}

/// What the session's redaction should display for `line`, computed
/// independently: every byte inside any occurrence of a value (overlapping
/// or adjacent occurrences included) is covered, and each maximal covered
/// run becomes `********`. A line that is a registered middle line of a
/// multi-line value is replaced whole.
fn reference_redact(line: &str, values: &[String], lines: &[String]) -> String {
    if lines.iter().any(|l| l == line) {
        return "********".to_string();
    }
    let mut covered = vec![false; line.len()];
    for v in values.iter().filter(|v| !v.is_empty()) {
        for (i, _) in line.match_indices(v.as_str()) {
            covered[i..i + v.len()].iter_mut().for_each(|c| *c = true);
        }
        // `match_indices` skips overlapping occurrences; find those too.
        let mut start = 0;
        while let Some(p) = line.get(start..).and_then(|r| r.find(v.as_str())) {
            let at = start + p;
            covered[at..at + v.len()].iter_mut().for_each(|c| *c = true);
            start = at + line[at..].chars().next().map_or(1, char::len_utf8);
        }
    }
    let mut out = String::new();
    let mut i = 0;
    while i < line.len() {
        if covered[i] {
            while i < line.len() && covered[i] {
                i += 1;
            }
            out.push_str("********");
        } else {
            let c = line[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// Mirror of how the filter registers a multi-line redacted value: its
/// middle lines are redacted as whole lines.
fn register_middle_lines(value: &str, lines: &mut Vec<String>) {
    let parts: Vec<&str> = value.split('\n').collect();
    if parts.len() > 2 {
        lines.extend(
            parts[1..parts.len() - 1]
                .iter()
                .filter(|p| !p.is_empty())
                .map(|p| p.to_string()),
        );
    }
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
    let (mut ref_values, mut ref_lines): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    for raw in text.split('\n') {
        let line = truncate_line(raw);
        oracle!(
            line.len() <= LOG_LINE_MAX_LENGTH,
            "truncate_line left {} bytes",
            line.len()
        );
        oracle!(raw.starts_with(line), "truncate_line is not a prefix");
        oracle!(
            raw.len() > LOG_LINE_MAX_LENGTH || line == raw,
            "truncate_line changed a {}-byte line",
            raw.len()
        );
        let (display, _pass, messages, _failed) = process_line(line, &mut filter, "s");
        let mut new_secrets = Vec::new();
        // Derived from the input, not from the filter's own messages, so a
        // filter that stops recognizing the directive still fails here.
        if let Some((n, v)) = well_formed_assignment(line, "openjd_env: ") {
            oracle!(
                messages.iter().any(|m| matches!(
                    m,
                    ActionMessage::SetEnv { name, value } if name == n && value == v
                )),
                "well-formed {line:?} produced no SetEnv({n}={v}): {messages:?}"
            );
        }
        if let Some((n, v)) = well_formed_assignment(line, "openjd_redacted_env: ") {
            oracle!(
                messages.iter().any(|m| matches!(
                    m,
                    ActionMessage::RedactedEnv { name, value } if name == n && value == v
                )),
                "well-formed {line:?} produced no RedactedEnv({n}): {messages:?}"
            );
            new_secrets.push(v.to_string());
        }
        for m in &messages {
            // A message's display (it goes to logs) never shows an env value.
            if let ActionMessage::SetEnv { name, .. } | ActionMessage::RedactedEnv { name, .. } = m
            {
                let shown = m.to_string();
                let kind = if matches!(m, ActionMessage::SetEnv { .. }) {
                    "SetEnv"
                } else {
                    "RedactedEnv"
                };
                oracle!(
                    shown == format!("{kind}({name})"),
                    "message display {shown:?} is not just the name of {name:?}"
                );
            }
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
        // Output that is not a directive is redacted exactly as the
        // reference says (catches partial leaks the check above can't).
        // Values registered by this line apply from the next line on.
        // The value set is the filter's own (malformed directives register
        // values too); that it holds every directive's value is checked
        // below. Values registered by this line apply from the next line on.
        // The filter matches the `openjd_` prefix case-insensitively (a
        // malformed directive such as `oPenjd_env: ..` gets an error), so
        // only lines without it in any case are plain output.
        if !line.trim_start().to_lowercase().starts_with("openjd_") {
            let want = reference_redact(line, &ref_values, &ref_lines);
            oracle!(
                display == want,
                "{line:?} displays as {display:?}, want {want:?} (secrets {ref_values:?})"
            );
        }
        for m in &messages {
            if let ActionMessage::RedactedEnv { value, .. } = m {
                register_middle_lines(value, &mut ref_lines);
            }
        }
        ref_values = filter.redacted_values().iter().cloned().collect();
        for v in secrets.iter().chain(&new_secrets) {
            oracle!(
                v.is_empty() || ref_values.contains(v),
                "redacted value {v:?} is not in the filter's redaction set"
            );
        }
        secrets.extend(new_secrets);
        let _ = filter.min_log_level();
    }

    let cmd = String::from_utf8_lossy(body);
    let redacted = redact_openjd_redacted_env_requests(&cmd);
    if let Some(pos) = cmd.find("openjd_redacted_env:") {
        let secret = &cmd[pos + "openjd_redacted_env:".len()..];
        // Everything up to and including the token is kept verbatim (it is
        // not secret); only what follows it must be masked.
        let kept = pos + "openjd_redacted_env:".len();
        // Everything after the token is replaced, whatever it is.
        oracle!(
            redacted == format!("{} ********", &cmd[..kept]),
            "redact_openjd_redacted_env_requests({cmd:?}) gives {redacted:?} (secret {secret:?})"
        );
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
    // Caller limits reach the session through `SessionLimits::from`, field
    // for field.
    let mut caller = openjd_model::CallerLimits::default();
    let mut lim = || flip(&mut u).then(|| gen::upto(&mut u, 1 << 20));
    caller.max_resolved_arg_len = lim();
    caller.max_resolved_data_len = lim();
    caller.max_eval_memory_bytes = lim();
    caller.max_eval_operations = lim();
    let limits = openjd_sessions::SessionLimits::from(&caller);
    oracle!(
        limits.max_resolved_arg_len == caller.max_resolved_arg_len
            && limits.max_resolved_data_len == caller.max_resolved_data_len
            && limits.max_eval_memory_bytes == caller.max_eval_memory_bytes
            && limits.max_eval_operations == caller.max_eval_operations,
        "SessionLimits::from({caller:?}) gives {limits:?}"
    );
    let mut ef = EmbeddedFiles::new(scope, root.clone(), "s")
        .with_user(None)
        .with_limits(limits);
    #[cfg(unix)]
    {
        use openjd_sessions::SessionUser;
        let (name, group) = (gen::ident(&mut u), flip(&mut u).then(|| gen::ident(&mut u)));
        let user = openjd_sessions::PosixSessionUser::new(&name, group.as_deref());
        oracle!(
            user.user() == name
                && group.as_deref().is_none_or(|g| user.group() == g)
                && !user.group().is_empty(),
            "PosixSessionUser({name:?}, {group:?}) reports {:?} / {:?}",
            user.user(),
            user.group()
        );
        let _ = (
            user.is_process_user(),
            user.as_any().is::<openjd_sessions::PosixSessionUser>(),
        );
    }
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

/// Session state machine: `Session::clear_ending_only` against a
/// reference model of the stored state and the ending-only flag, as
/// `Session::set_state_for_test` and `clear_ending_only` document them.
///
/// The session-wide cancel token, which makes `clear_ending_only` refuse
/// with `SessionCanceled`, is covered by openjd-sessions' integration tests
/// (`tests/integration/test_brittle_session.rs`): setting it
/// needs `Session::with_config`, which creates directories.
pub fn state(data: &[u8]) {
    use openjd_sessions::{Session, SessionError, SessionState};
    const STATES: [SessionState; 5] = [
        SessionState::Ready,
        SessionState::ReadyEnding,
        SessionState::Running,
        SessionState::Canceling,
        SessionState::Ended,
    ];
    // Dropping a session that was never cleaned up removes its working
    // directory, so give it one that is never created.
    let dir = std::env::temp_dir()
        .join(format!("openjd-fuzz-sessions-state-{}", std::process::id()))
        .join("never-created");
    let mut session = Session::new_for_test(dir);
    // The model: `ReadyEnding` is stored as `Ready` plus the flag.
    let (mut stored, mut ending) = (SessionState::Ready, false);
    let reported = |stored: SessionState, ending: bool| {
        if stored == SessionState::Ready && ending {
            SessionState::ReadyEnding
        } else {
            stored
        }
    };
    let mut u = Unstructured::new(data);
    for _ in 0..64 {
        if u.is_empty() {
            break;
        }
        let before = reported(stored, ending);
        if flip(&mut u) {
            let next = *pick(&mut u, &STATES);
            session.set_state_for_test(next);
            if next == SessionState::ReadyEnding {
                (stored, ending) = (SessionState::Ready, true);
            } else {
                stored = next;
            }
        } else {
            match session.clear_ending_only() {
                Ok(()) => {
                    oracle!(
                        before == SessionState::ReadyEnding,
                        "clear_ending_only succeeded from {before:?}"
                    );
                    ending = false;
                }
                Err(SessionError::InvalidState { expected, current }) => oracle!(
                    before != SessionState::ReadyEnding
                        && expected == [SessionState::ReadyEnding]
                        && current == before,
                    "clear_ending_only from {before:?} failed with InvalidState \
                     {{ expected: {expected:?}, current: {current:?} }}"
                ),
                Err(e) => oracle!(false, "clear_ending_only from {before:?} failed: {e:?}"),
            }
        }
        let want = reported(stored, ending);
        oracle!(
            session.state() == want,
            "after a step from {before:?} the session reports {:?}, the model {want:?}",
            session.state()
        );
    }
}
