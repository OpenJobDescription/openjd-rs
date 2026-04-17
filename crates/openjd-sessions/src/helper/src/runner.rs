use super::protocol::{send, Command as HelperCommand, Response, RunCommand};
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;
use std::io::BufRead;
use std::os::unix::io::{AsRawFd, BorrowedFd};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

/// Run a command, reading cancel commands from the provided stdin reader.
/// The stdin reader must be the same one used by main() to avoid buffering conflicts.
pub fn run_command(cmd: &RunCommand, stdin_buf: &mut std::io::BufReader<std::io::StdinLock<'_>>) -> Result<i32, String> {
    let mut child = unsafe {
        Command::new(&cmd.command)
            .args(&cmd.args)
            .envs(&cmd.env)
            .current_dir(&cmd.cwd)
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

        let pollfds = unsafe {
            let mut fds = [
                PollFd::new(BorrowedFd::borrow_raw(stdin_raw), PollFlags::POLLIN),
                PollFd::new(BorrowedFd::borrow_raw(child_raw), PollFlags::POLLIN),
            ];
            let _ = poll(&mut fds, timeout);
            fds
        };

        // Check for cancel on stdin
        if pollfds[0]
            .revents()
            .is_some_and(|r| r.contains(PollFlags::POLLIN))
        {
            let mut line = String::new();
            if stdin_buf.read_line(&mut line).unwrap_or(0) > 0 {
                if let Ok(HelperCommand::Cancel(method)) = serde_json::from_str::<HelperCommand>(&line) {
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
            }
        }

        // Check for child output
        if pollfds[1]
            .revents()
            .is_some_and(|r| r.contains(PollFlags::POLLIN))
        {
            let mut line = String::new();
            match child_buf.read_line(&mut line) {
                Ok(0) => {
                    let status = child.wait().map_err(|e| e.to_string())?;
                    return Ok(status.code().unwrap_or(-1));
                }
                Ok(_) => send(&Response::Out {
                    out: line.trim_end().to_string(),
                }),
                Err(e) => return Err(e.to_string()),
            }
        }

        // Check for child stdout closed
        if pollfds[1]
            .revents()
            .is_some_and(|r| r.intersects(PollFlags::POLLHUP | PollFlags::POLLERR))
        {
            let mut line = String::new();
            while child_buf.read_line(&mut line).unwrap_or(0) > 0 {
                send(&Response::Out {
                    out: line.trim_end().to_string(),
                });
                line.clear();
            }
            let status = child.wait().map_err(|e| e.to_string())?;
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
                return Ok(status.code().unwrap_or(-1));
            }
        }
    }
}
