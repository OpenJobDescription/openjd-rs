// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

#[cfg(windows)]
mod job_object;
mod protocol;
#[cfg(unix)]
mod runner;
#[cfg(windows)]
mod runner_win;

use protocol::{constant_time_eq, send, Command, Response, AUTH_TOKEN_LEN};
use std::io::{BufRead, Read};

/// Maximum bytes the helper will read for a single JSON line from stdin.
/// Matches the 128 KB limit used for subprocess log lines (2× `LOG_LINE_MAX_LENGTH`).
/// Lines exceeding this are rejected with an error response.
const MAX_LINE_LENGTH: usize = 128 * 1024;

/// Read a single line from `reader`, enforcing [`MAX_LINE_LENGTH`].
/// Returns `Ok(0)` on EOF, `Ok(n)` on success, or `Err` if the line is too long.
fn read_bounded_line(reader: &mut impl BufRead, buf: &mut String) -> std::io::Result<usize> {
    buf.clear();
    let n = Read::take(&mut *reader, MAX_LINE_LENGTH as u64).read_line(buf)?;
    if n == MAX_LINE_LENGTH && !buf.ends_with('\n') {
        // Line exceeded the limit — drain the rest so the stream stays aligned
        let mut discard = Vec::new();
        let _ = reader.read_until(b'\n', &mut discard);
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("line exceeds {MAX_LINE_LENGTH} byte limit"),
        ));
    }
    Ok(n)
}

/// Parse the `--auth-token <TOKEN>` argument from the helper's argv.
///
/// Exits the process with status 2 and a one-line stderr diagnostic if the
/// option is missing or the value is not exactly [`AUTH_TOKEN_LEN`] bytes long.
/// No JSON is emitted in either case.
fn parse_auth_token_or_exit() -> String {
    let args: Vec<String> = std::env::args().collect();
    // Expect: <helper_path> --auth-token <VALUE>
    // Allow future arguments after the token without failing hard here —
    // additional options would fail at the point they'd be parsed.
    let mut token: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--auth-token" {
            if i + 1 >= args.len() {
                eprintln!("openjd_helper: --auth-token requires a value");
                std::process::exit(2);
            }
            token = Some(args[i + 1].clone());
            i += 2;
        } else {
            eprintln!("openjd_helper: unknown argument: {}", args[i]);
            std::process::exit(2);
        }
    }
    let Some(t) = token else {
        eprintln!("openjd_helper: missing required --auth-token <TOKEN> argument");
        std::process::exit(2);
    };
    if t.len() != AUTH_TOKEN_LEN {
        eprintln!(
            "openjd_helper: --auth-token must be exactly {AUTH_TOKEN_LEN} characters (got {})",
            t.len()
        );
        std::process::exit(2);
    }
    t
}

fn main() {
    let expected_token = parse_auth_token_or_exit();

    #[cfg(unix)]
    run_unix(&expected_token);
    #[cfg(windows)]
    run_windows(&expected_token);
}

/// Verify that `cmd`'s embedded token matches `expected`. On mismatch, sends
/// `{"error":"invalid token"}` and returns `false`.
fn verify_token(cmd: &Command, expected: &str) -> bool {
    if constant_time_eq(cmd.token().as_bytes(), expected.as_bytes()) {
        true
    } else {
        send(&Response::Error {
            error: "invalid token".into(),
        });
        false
    }
}

/// POSIX: single-threaded stdin reading. The runner uses poll() to multiplex
/// stdin (cancel) and child stdout internally.
#[cfg(unix)]
fn run_unix(expected_token: &str) {
    let stdin = std::io::stdin();
    let mut reader = std::io::BufReader::new(stdin.lock());
    let mut line = String::new();

    loop {
        line.clear();
        match read_bounded_line(&mut reader, &mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                send(&Response::Error {
                    error: format!("stdin read error: {e}"),
                });
                continue;
            }
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let cmd: Command = match serde_json::from_str(trimmed) {
            Ok(c) => c,
            Err(e) => {
                send(&Response::Error {
                    error: format!("parse error: {e}"),
                });
                continue;
            }
        };
        if !verify_token(&cmd, expected_token) {
            continue;
        }
        match cmd {
            Command::Run { run, .. } => {
                match runner::run_command(&run, &mut reader, expected_token) {
                    Ok(code) => send(&Response::Exited { exited: code }),
                    Err(e) => send(&Response::Error { error: e }),
                }
            }
            Command::Shutdown { .. } => break,
            Command::Cancel { .. } => {}
        }
    }
}

/// Windows: stdin is read on a background thread because we can't poll()
/// pipes. Commands are dispatched via channels — Run/Shutdown to the main
/// loop, Cancel to the runner.
///
/// Before processing any commands, the helper places itself inside a
/// Job Object configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Any
/// workload the runner subsequently spawns is automatically a member of
/// the job (Windows propagates job membership to children), so when the
/// helper exits — even via `TerminateProcess` from the parent — the kernel
/// closes the job and terminates every workload. This prevents orphaned
/// grandchildren from holding inherited handles open and stalling the
/// session (or, in CI, the test runner).
#[cfg(windows)]
fn run_windows(expected_token: &str) {
    use std::sync::mpsc;

    // Best-effort: if the job object can't be created, log a warning and
    // continue. The helper still functions; it just loses the descendant-
    // cleanup guarantee on crash.
    let job = job_object::JobObject::setup_for_current_process();

    enum MainCmd {
        Run(protocol::RunCommand),
        Shutdown,
    }

    let (main_tx, main_rx) = mpsc::channel::<MainCmd>();
    let (cancel_tx, cancel_rx) = mpsc::channel::<protocol::CancelMethod>();

    // The reader thread owns its own copy of the expected token so the
    // stdin parsing path stays fully self-contained.
    let token_owned = expected_token.to_string();

    // Stdin reader thread
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut reader = std::io::BufReader::new(stdin.lock());
        let mut line = String::new();

        loop {
            line.clear();
            match read_bounded_line(&mut reader, &mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => {
                    send(&Response::Error {
                        error: format!("stdin read error: {e}"),
                    });
                    continue;
                }
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let cmd: Command = match serde_json::from_str(trimmed) {
                Ok(c) => c,
                Err(e) => {
                    send(&Response::Error {
                        error: format!("parse error: {e}"),
                    });
                    continue;
                }
            };
            if !verify_token(&cmd, &token_owned) {
                continue;
            }
            match cmd {
                Command::Run { run, .. } => {
                    if main_tx.send(MainCmd::Run(run)).is_err() {
                        break;
                    }
                }
                Command::Shutdown { .. } => {
                    let _ = main_tx.send(MainCmd::Shutdown);
                    break;
                }
                Command::Cancel { method, .. } => {
                    let _ = cancel_tx.send(method);
                }
            }
        }
    });

    // Main loop: wait for Run or Shutdown commands
    for cmd in main_rx {
        match cmd {
            MainCmd::Run(run) => match runner_win::run_command(&run, &cancel_rx, job.as_ref()) {
                Ok(code) => send(&Response::Exited { exited: code }),
                Err(e) => send(&Response::Error { error: e }),
            },
            MainCmd::Shutdown => break,
        }
    }
}
