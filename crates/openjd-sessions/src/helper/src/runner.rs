// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use super::protocol::{constant_time_eq, send, Command as HelperCommand, Response, RunCommand};
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;
use std::io::BufRead;
use std::os::unix::io::{AsRawFd, BorrowedFd};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

/// Run a command, reading cancel commands from the provided stdin reader.
/// The stdin reader must be the same one used by main() to avoid buffering conflicts.
///
/// `expected_token` is the raw byte string the main loop verified at startup.
/// Every cancel command read from stdin during the run is verified against it
/// the same way a run command would be; cancels with the wrong token are
/// reported as `{"error":"invalid token"}` and do not affect the running child.
pub fn run_command(
    cmd: &RunCommand,
    stdin_buf: &mut std::io::BufReader<std::io::StdinLock<'_>>,
    expected_token: &str,
) -> Result<i32, String> {
    let mut child = unsafe {
        Command::new(&cmd.command)
            .args(&cmd.args)
            .envs(&cmd.env)
            .current_dir(&cmd.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .pre_exec(|| {
                nix::libc::dup2(1, 2);
                Ok(())
            })
            .process_group(0)
            .spawn()
            .map_err(|e| e.to_string())?
    };

    let child_pid = Pid::from_raw(child.id() as i32);
    send(&Response::Pid { pid: child.id() });

    let child_stdout = child.stdout.take().unwrap();
    let stdin_raw = stdin_buf.get_ref().as_raw_fd();
    let child_raw = child_stdout.as_raw_fd();

    let mut child_buf = std::io::BufReader::new(child_stdout);
    let mut child_killed = false;
    let mut escalation_deadline: Option<std::time::Instant> = None;

    loop {
        let timeout = if child_killed {
            PollTimeout::from(100u16)
        } else if let Some(deadline) = escalation_deadline {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                // Deadline passed — escalate now.
                let _ = killpg(child_pid, Signal::SIGKILL);
                child_killed = true;
                escalation_deadline = None;
                PollTimeout::from(100u16)
            } else {
                // Poll until the deadline so we can escalate on time.
                let ms = remaining.as_millis().min(u16::MAX as u128) as u16;
                PollTimeout::from(ms.max(1))
            }
        } else {
            PollTimeout::NONE
        };

        // Check if the BufReader already has buffered data from a previous
        // read in the main loop. If so, skip poll() — the data is in userspace
        // and poll() on the raw fd won't see it.
        let stdin_has_buffered = !stdin_buf.buffer().is_empty();

        let pollfds = if stdin_has_buffered {
            // Don't poll — we already know stdin has data
            None
        } else {
            Some(unsafe {
                let mut fds = [
                    PollFd::new(BorrowedFd::borrow_raw(stdin_raw), PollFlags::POLLIN),
                    PollFd::new(BorrowedFd::borrow_raw(child_raw), PollFlags::POLLIN),
                ];
                let _ = poll(&mut fds, timeout);
                fds
            })
        };

        // Check for cancel on stdin
        let stdin_ready = stdin_has_buffered
            || pollfds.as_ref().is_some_and(|fds| {
                fds[0]
                    .revents()
                    .is_some_and(|r| r.contains(PollFlags::POLLIN))
            });
        if stdin_ready {
            let mut line = String::new();
            if stdin_buf.read_line(&mut line).unwrap_or(0) > 0 {
                if let Ok(parsed_cmd) = serde_json::from_str::<HelperCommand>(&line) {
                    // Every cancel command must carry the shared token.
                    // Cancels with a wrong/missing token are rejected with an
                    // error response and must not affect the running child.
                    if !constant_time_eq(parsed_cmd.token().as_bytes(), expected_token.as_bytes()) {
                        send(&Response::Error {
                            error: "invalid token".into(),
                        });
                    } else if let HelperCommand::Cancel { method, .. } = parsed_cmd {
                        match method {
                            super::protocol::CancelMethod::Terminate => {
                                let _ = killpg(child_pid, Signal::SIGKILL);
                                child_killed = true;
                            }
                            super::protocol::CancelMethod::NotifyThenTerminate {
                                notify_period_in_seconds,
                            } => {
                                let _ = killpg(child_pid, Signal::SIGTERM);
                                escalation_deadline = Some(
                                    std::time::Instant::now()
                                        + std::time::Duration::from_secs(notify_period_in_seconds),
                                );
                            }
                        }
                    }
                    // Run/Shutdown lines received mid-run are ignored here —
                    // only Cancel is meaningful inside the runner.
                }
            }
        }

        // Check for child output (only if we actually polled)
        if pollfds.as_ref().is_some_and(|fds| {
            fds[1]
                .revents()
                .is_some_and(|r| r.contains(PollFlags::POLLIN))
        }) {
            let mut line = String::new();
            match child_buf.read_line(&mut line) {
                Ok(0) => {
                    let status = child.wait().map_err(|e| e.to_string())?;
                    // Kill any remaining processes in the child's process group
                    let _ = killpg(child_pid, Signal::SIGKILL);
                    return Ok(status.code().unwrap_or(-1));
                }
                Ok(_) => send(&Response::Out {
                    out: line.trim_end().to_string(),
                }),
                Err(e) => return Err(e.to_string()),
            }
        }

        // Check for child stdout closed (only if we actually polled)
        if pollfds.as_ref().is_some_and(|fds| {
            fds[1]
                .revents()
                .is_some_and(|r| r.intersects(PollFlags::POLLHUP | PollFlags::POLLERR))
        }) {
            let mut line = String::new();
            while child_buf.read_line(&mut line).unwrap_or(0) > 0 {
                send(&Response::Out {
                    out: line.trim_end().to_string(),
                });
                line.clear();
            }
            let status = child.wait().map_err(|e| e.to_string())?;
            // Kill any remaining processes in the child's process group
            let _ = killpg(child_pid, Signal::SIGKILL);
            return Ok(status.code().unwrap_or(-1));
        }

        // After kill or soft signal, poll for child exit even without fd events
        if child_killed || escalation_deadline.is_some() {
            if let Ok(Some(status)) = child.try_wait() {
                let mut line = String::new();
                while child_buf.read_line(&mut line).unwrap_or(0) > 0 {
                    send(&Response::Out {
                        out: line.trim_end().to_string(),
                    });
                    line.clear();
                }
                // Kill any remaining processes in the child's process group
                let _ = killpg(child_pid, Signal::SIGKILL);
                return Ok(status.code().unwrap_or(-1));
            }
        }
    }
}
