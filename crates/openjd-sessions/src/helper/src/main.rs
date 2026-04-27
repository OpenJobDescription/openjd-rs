// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

mod protocol;
#[cfg(unix)]
mod runner;
#[cfg(windows)]
mod runner_win;

use protocol::{send, Command, Response};
use std::io::BufRead;

fn main() {
    #[cfg(unix)]
    run_unix();
    #[cfg(windows)]
    run_windows();
}

/// POSIX: single-threaded stdin reading. The runner uses poll() to multiplex
/// stdin (cancel) and child stdout internally.
#[cfg(unix)]
fn run_unix() {
    let stdin = std::io::stdin();
    let mut reader = std::io::BufReader::new(stdin.lock());
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
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
        match cmd {
            Command::Run(run) => match runner::run_command(&run, &mut reader) {
                Ok(code) => send(&Response::Exited { exited: code }),
                Err(e) => send(&Response::Error { error: e }),
            },
            Command::Shutdown => break,
            Command::Cancel(_) => {}
        }
    }
}

/// Windows: stdin is read on a background thread because we can't poll()
/// pipes. Commands are dispatched via channels — Run/Shutdown to the main
/// loop, Cancel to the runner.
#[cfg(windows)]
fn run_windows() {
    use std::sync::mpsc;

    enum MainCmd {
        Run(protocol::RunCommand),
        Shutdown,
    }

    let (main_tx, main_rx) = mpsc::channel::<MainCmd>();
    let (cancel_tx, cancel_rx) = mpsc::channel::<protocol::CancelMethod>();

    // Stdin reader thread
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut reader = std::io::BufReader::new(stdin.lock());
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
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
            match cmd {
                Command::Run(run) => {
                    if main_tx.send(MainCmd::Run(run)).is_err() {
                        break;
                    }
                }
                Command::Shutdown => {
                    let _ = main_tx.send(MainCmd::Shutdown);
                    break;
                }
                Command::Cancel(req) => {
                    let _ = cancel_tx.send(req);
                }
            }
        }
    });

    // Main loop: wait for Run or Shutdown commands
    for cmd in main_rx {
        match cmd {
            MainCmd::Run(run) => match runner_win::run_command(&run, &cancel_rx) {
                Ok(code) => send(&Response::Exited { exited: code }),
                Err(e) => send(&Response::Error { error: e }),
            },
            MainCmd::Shutdown => break,
        }
    }
}
