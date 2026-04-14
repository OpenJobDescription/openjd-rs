// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Persistent cross-user helper process for subprocess execution via sudo.
//!
//! Instead of spawning `sudo -u <user> -i` per action (paying ~1s login cost
//! each time), the helper is launched once at session start and communicates
//! over newline-delimited JSON on stdin/stdout.

use std::path::Path;

use crate::action::{ActionMessage, ActionState};
use crate::error::SessionError;
use crate::logging::LogContent;
use crate::session_log;
use crate::session_user::SessionUser;

/// Manages a long-lived helper process for cross-user command execution.
///
/// The helper is launched once via `sudo -u <user> -i <helper_path>` and
/// communicates over newline-delimited JSON on stdin/stdout.
pub(crate) struct CrossUserHelper {
    child: std::process::Child,
    stdin: std::io::BufWriter<std::process::ChildStdin>,
    stdout: std::io::BufReader<std::process::ChildStdout>,
}

impl CrossUserHelper {
    /// Spawn the helper binary as the given user via sudo.
    ///
    /// Returns `(helper, cancel_writer)` where `cancel_writer` is a dup'd copy
    /// of the helper's stdin fd. This allows sending cancel commands even while
    /// the helper struct is moved to a runner during action execution.
    pub fn spawn(
        helper_path: &Path,
        user: &dyn SessionUser,
    ) -> Result<(Self, std::fs::File), SessionError> {
        let mut child = std::process::Command::new("sudo")
            .args(["-u", user.user(), "-i", &helper_path.to_string_lossy()])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|source| SessionError::SubprocessStart {
                command: format!("sudo -u {} -i {}", user.user(), helper_path.display()),
                source,
            })?;

        let child_stdin = child.stdin.take().expect("stdin was piped");

        // Dup the stdin fd so we can write cancel commands from Session::cancel_action
        // while the helper is owned by a runner.
        use std::os::unix::io::AsRawFd;
        let raw_fd = child_stdin.as_raw_fd();
        let dup_fd = nix::unistd::dup(raw_fd)
            .map_err(|e| SessionError::Runtime(format!("Failed to dup helper stdin fd: {e}")))?;
        let cancel_writer = unsafe { std::os::unix::io::FromRawFd::from_raw_fd(dup_fd) };

        let stdin = std::io::BufWriter::new(child_stdin);
        let stdout = std::io::BufReader::new(child.stdout.take().expect("stdout was piped"));

        Ok((
            Self {
                child,
                stdin,
                stdout,
            },
            cancel_writer,
        ))
    }

    /// Send a JSON command to the helper (writes JSON + newline, then flushes).
    pub fn send_command(&mut self, cmd: &serde_json::Value) -> Result<(), SessionError> {
        use std::io::Write;
        serde_json::to_writer(&mut self.stdin, cmd)
            .map_err(|e| SessionError::Runtime(format!("Failed to write to helper stdin: {e}")))?;
        self.stdin.write_all(b"\n").map_err(|e| {
            SessionError::Runtime(format!("Failed to write newline to helper: {e}"))
        })?;
        self.stdin
            .flush()
            .map_err(|e| SessionError::Runtime(format!("Failed to flush helper stdin: {e}")))?;
        Ok(())
    }

    /// Read one line from the helper's stdout and parse as JSON.
    pub fn read_response(&mut self) -> Result<serde_json::Value, SessionError> {
        use std::io::BufRead;
        let mut line = String::new();
        self.stdout.read_line(&mut line).map_err(|e| {
            SessionError::Runtime(format!("Failed to read from helper stdout: {e}"))
        })?;
        if line.is_empty() {
            return Err(SessionError::Runtime(
                "Helper process closed stdout unexpectedly".into(),
            ));
        }
        serde_json::from_str(line.trim_end())
            .map_err(|e| SessionError::Runtime(format!("Failed to parse helper response: {e}")))
    }

    /// Send "shutdown" and wait for the child to exit.
    pub fn shutdown(&mut self) {
        let _ = self.send_command(&serde_json::Value::String("shutdown".into()));
        let _ = self.child.wait();
    }
}

/// Execute a subprocess via a CrossUserHelper, returning the result.
///
/// This is the shared core used by both `Session::run_subprocess_via_helper`
/// and the script runners when a helper is available.
///
/// Uses `tokio::task::block_in_place` to avoid blocking the async runtime
/// during the synchronous helper I/O loop. This requires the multi-thread
/// tokio runtime (which the crate already depends on via `rt-multi-thread`).
///
/// # Cancel safety
///
/// The cancel_writer (dup'd fd) and helper.stdin (BufWriter) both write to the
/// same underlying pipe, but cannot race in practice:
/// - `send_command` completes and flushes before the response loop begins
/// - The timeout thread only wakes after the configured timeout duration,
///   long after `send_command` has finished
/// - `Session::cancel_action` requires `&mut self`, which is exclusively held
///   by the async method that called `run_via_helper`, preventing concurrent
///   cancel writes during command submission
pub(crate) fn run_via_helper(
    helper: &mut CrossUserHelper,
    config: &crate::subprocess::SubprocessConfig,
    filter: &mut crate::action_filter::ActionFilter,
    session_id: &str,
    message_tx: tokio::sync::mpsc::UnboundedSender<ActionMessage>,
    cancel_writer: Option<&std::fs::File>,
) -> Result<crate::subprocess::SubprocessResult, SessionError> {
    // Spawn timeout thread if configured — sends cancel via the dup'd stdin fd.
    // Uses a Condvar so the thread stops immediately when the command completes,
    // preventing orphaned timeout threads from cancelling subsequent commands.
    let timed_out = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done = std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let _timeout_guard = if let (Some(timeout), Some(writer)) = (config.timeout, cancel_writer) {
        use std::io::Write;
        let mut writer = writer
            .try_clone()
            .map_err(|e| SessionError::Runtime(format!("Failed to clone cancel_writer: {e}")))?;
        let timed_out = timed_out.clone();
        let done = done.clone();
        let handle = std::thread::spawn(move || {
            let (lock, cvar) = &*done;
            let guard = lock.lock().unwrap();
            let (guard, _) = cvar.wait_timeout_while(guard, timeout, |d| !*d).unwrap();
            if *guard {
                return;
            } // command finished before timeout
            drop(guard);
            timed_out.store(true, std::sync::atomic::Ordering::Release);
            let _ = writeln!(writer, "{{\"cancel\":\"SIGTERM\"}}");
            let _ = writer.flush();
            // Grace period — also cancellable
            let guard = lock.lock().unwrap();
            let (guard, _) = cvar
                .wait_timeout_while(guard, std::time::Duration::from_secs(5), |d| !*d)
                .unwrap();
            if *guard {
                return;
            }
            drop(guard);
            let _ = writeln!(writer, "{{\"cancel\":\"SIGKILL\"}}");
            let _ = writer.flush();
        });
        Some(handle)
    } else {
        None
    };

    // Guard that signals the timeout condvar on any exit path (including ? errors).
    struct DoneGuard(std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>);
    impl Drop for DoneGuard {
        fn drop(&mut self) {
            let (lock, cvar) = &*self.0;
            *lock.lock().unwrap() = true;
            cvar.notify_one();
        }
    }
    let _done_guard = DoneGuard(done);

    // Build the env map (only set values; unsets are excluded).
    let env: serde_json::Map<String, serde_json::Value> = config
        .env_vars
        .iter()
        .filter_map(|(k, v)| {
            v.as_ref()
                .map(|val| (k.clone(), serde_json::Value::String(val.clone())))
        })
        .collect();

    let cmd = serde_json::json!({
        "command": config.args[0],
        "args": &config.args[1..],
        "env": env,
        "cwd": config.working_dir,
    });

    helper.send_command(&cmd)?;

    // Log the actual command (not the helper protocol)
    session_log!(
        info,
        session_id,
        LogContent::FILE_PATH | LogContent::PROCESS_CONTROL,
        "Running command {}",
        crate::subprocess::format_command_for_log(&config.args)
    );

    let mut stdout_collected = String::new();
    let mut saw_fail = false;

    loop {
        let resp = helper.read_response()?;

        if let Some(pid) = resp.get("pid").and_then(|v| v.as_i64()) {
            session_log!(
                info,
                session_id,
                LogContent::PROCESS_CONTROL,
                "Command started as pid: {}",
                pid
            );
            continue;
        }

        if let Some(line) = resp.get("out").and_then(|v| v.as_str()) {
            let line = if line.len() > 64 * 1024 {
                &line[..64 * 1024]
            } else {
                line
            };
            let (display, pass_through) = crate::subprocess::process_line(
                line,
                filter,
                session_id,
                &message_tx,
                &mut saw_fail,
            );
            if pass_through && filter.min_log_level() <= 20 {
                session_log!(info, session_id, LogContent::COMMAND_OUTPUT, "{}", display);
            }
            stdout_collected.push_str(line);
            stdout_collected.push('\n');
            continue;
        }

        if let Some(code) = resp.get("exited").and_then(|v| v.as_i64()) {
            let exit_code = code as i32;
            session_log!(
                info,
                session_id,
                LogContent::PROCESS_CONTROL,
                "Process exit code: {}",
                exit_code
            );

            // Check if cancel was requested via the watch channel.
            let canceled = config
                .cancel_request_rx
                .as_ref()
                .is_some_and(|rx| rx.has_changed().unwrap_or(false));

            let state = if canceled {
                ActionState::Canceled
            } else if timed_out.load(std::sync::atomic::Ordering::Acquire) {
                ActionState::Timeout
            } else if saw_fail {
                ActionState::Failed
            } else if exit_code == 0 {
                ActionState::Success
            } else {
                ActionState::Failed
            };
            return Ok(crate::subprocess::SubprocessResult {
                state,
                exit_code: Some(exit_code),
                stdout: stdout_collected,
            });
        }

        if let Some(msg) = resp.get("error").and_then(|v| v.as_str()) {
            return Err(SessionError::SubprocessStart {
                command: config.args[0].clone(),
                source: std::io::Error::other(msg.to_string()),
            });
        }

        return Err(SessionError::Runtime(format!(
            "Unexpected helper response: {}",
            resp
        )));
    }
}
