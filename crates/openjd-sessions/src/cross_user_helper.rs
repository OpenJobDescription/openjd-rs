// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Persistent cross-user helper process for subprocess execution.
//!
//! On POSIX, the helper is launched via `sudo -u <user> -i <helper_path>`.
//! On Windows, the helper is launched via `CreateProcessAsUserW` / `CreateProcessWithLogonW`.
//!
//! In both cases the helper communicates over newline-delimited JSON on stdin/stdout,
//! avoiding per-action login costs and enabling reliable cancellation from the
//! target user's context.

use std::path::Path;

use crate::action::{ActionMessage, ActionState};
use crate::error::SessionError;
use crate::logging::LogContent;
use crate::session_log;
use crate::session_user::SessionUser;

// ── Async helper reader ──

/// Future type returned by `AsyncHelperReader::next_response`.
type NextResponseFuture<'a> = std::pin::Pin<
    Box<
        dyn std::future::Future<Output = Option<Result<serde_json::Value, SessionError>>>
            + Send
            + 'a,
    >,
>;

/// Async stream of JSON responses from the helper's stdout.
/// Both platforms use a reader thread + async channel.
pub(crate) trait AsyncHelperReader: Send {
    /// Receive the next JSON response. Returns None when the helper exits.
    fn next_response(&mut self) -> NextResponseFuture<'_>;
}

/// Unix: async reads using a reader thread + channel (same pattern as Windows).
/// While AsyncFd could theoretically provide zero-thread async reads on Unix,
/// it doesn't implement AsyncRead directly. The reader thread approach is
/// simple, uniform across platforms, and has negligible overhead.
#[cfg(unix)]
pub(crate) struct UnixAsyncHelperReader {
    rx: tokio::sync::mpsc::UnboundedReceiver<Result<serde_json::Value, SessionError>>,
    _thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(unix)]
impl UnixAsyncHelperReader {
    pub fn new(stdout: std::process::ChildStdout) -> Result<Self, SessionError> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let thread = std::thread::spawn(move || {
            use std::io::BufRead;
            let mut reader = std::io::BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        let result = serde_json::from_str(line.trim_end()).map_err(|e| {
                            SessionError::HelperCommunication(format!("parse error: {e}"))
                        });
                        if tx.send(result).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            rx,
            _thread: Some(thread),
        })
    }
}

#[cfg(unix)]
impl AsyncHelperReader for UnixAsyncHelperReader {
    fn next_response(&mut self) -> NextResponseFuture<'_> {
        Box::pin(async { self.rx.recv().await })
    }
}

/// Windows: reader thread relays lines from the sync pipe to an async channel.
#[cfg(windows)]
pub(crate) struct WindowsAsyncHelperReader {
    rx: tokio::sync::mpsc::UnboundedReceiver<Result<serde_json::Value, SessionError>>,
    _thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(windows)]
impl WindowsAsyncHelperReader {
    pub fn new(stdout: std::fs::File) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let thread = std::thread::spawn(move || {
            use std::io::BufRead;
            let mut reader = std::io::BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        let result = serde_json::from_str(line.trim_end()).map_err(|e| {
                            SessionError::HelperCommunication(format!("parse error: {e}"))
                        });
                        if tx.send(result).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            rx,
            _thread: Some(thread),
        }
    }
}

#[cfg(windows)]
impl AsyncHelperReader for WindowsAsyncHelperReader {
    fn next_response(&mut self) -> NextResponseFuture<'_> {
        Box::pin(async { self.rx.recv().await })
    }
}

/// Manages a long-lived helper process for cross-user command execution (POSIX).
///
/// On POSIX: launched via `sudo -u <user> -i <helper_path>`.
/// Communicates over newline-delimited JSON on stdin/stdout.
#[cfg(unix)]
pub(crate) struct CrossUserHelper {
    child: std::process::Child,
    stdin: std::io::BufWriter<std::process::ChildStdin>,
    pub(crate) async_reader: UnixAsyncHelperReader,
}

/// Windows variant: the child is a raw process handle from `CreateProcessAsUserW`,
/// not a `std::process::Child`. We wrap stdin/stdout from the pipe handles.
#[cfg(windows)]
pub(crate) struct CrossUserHelperWin {
    process_handle: windows::Win32::Foundation::HANDLE,
    stdin: std::io::BufWriter<std::fs::File>,
    pub(crate) async_reader: WindowsAsyncHelperReader,
}

// SAFETY: `CrossUserHelperWin` is Send because all of its fields can be
// sent across threads:
// - `process_handle: HANDLE` is a Windows kernel object handle (pointer-
//   sized integer). Kernel handles are process-wide and safe to use from
//   any thread. `HANDLE` is `!Send` in `windows-rs` out of caution, but the
//   process handle here is only used for wait/terminate operations that
//   accept any thread's handle.
// - `stdin: BufWriter<File>` is already Send.
// - `async_reader: WindowsAsyncHelperReader` owns an `UnboundedReceiver`
//   and a `JoinHandle`, both of which are Send.
#[cfg(windows)]
unsafe impl Send for CrossUserHelperWin {}

#[cfg(unix)]
impl CrossUserHelper {
    /// Spawn the helper binary as the given user via sudo (POSIX).
    ///
    /// Returns `(helper, cancel_writer)` where `cancel_writer` is a dup'd copy
    /// of the helper's stdin fd. This allows sending cancel commands even while
    /// the helper struct is moved to a runner during action execution.
    #[cfg(unix)]
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
        use std::os::unix::io::{AsFd, FromRawFd, IntoRawFd};
        let dup_fd = nix::unistd::dup(child_stdin.as_fd()).map_err(|e| {
            SessionError::HelperCommunication(format!("Failed to dup helper stdin fd: {e}"))
        })?;
        let cancel_writer = unsafe { FromRawFd::from_raw_fd(dup_fd.into_raw_fd()) };

        let stdin = std::io::BufWriter::new(child_stdin);
        let async_reader =
            UnixAsyncHelperReader::new(child.stdout.take().expect("stdout was piped"))?;

        Ok((
            Self {
                child,
                stdin,
                async_reader,
            },
            cancel_writer,
        ))
    }

    /// Send a JSON command to the helper (writes JSON + newline, then flushes).
    pub fn send_command(&mut self, cmd: &serde_json::Value) -> Result<(), SessionError> {
        use std::io::Write;
        serde_json::to_writer(&mut self.stdin, cmd).map_err(|e| {
            SessionError::HelperCommunication(format!("Failed to write to helper stdin: {e}"))
        })?;
        self.stdin.write_all(b"\n").map_err(|e| {
            SessionError::HelperCommunication(format!("Failed to write newline to helper: {e}"))
        })?;
        self.stdin.flush().map_err(|e| {
            SessionError::HelperCommunication(format!("Failed to flush helper stdin: {e}"))
        })?;
        Ok(())
    }

    /// Send "shutdown" and wait for the child to exit.
    pub fn shutdown(&mut self) {
        let _ = self.send_command(&serde_json::Value::String("shutdown".into()));
        let _ = self.child.wait();
    }
}

#[cfg(unix)]
impl Drop for CrossUserHelper {
    fn drop(&mut self) {
        // Safety net: kill the child if shutdown() wasn't called.
        if let Ok(None) = self.child.try_wait() {
            log::warn!(target: "openjd.sessions", "CrossUserHelper dropped without shutdown(), killing child process");
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(windows)]
impl CrossUserHelperWin {
    /// Spawn the helper binary as the given user via `CreateProcessAsUserW` (Windows).
    ///
    /// Returns `(helper, cancel_writer)` where `cancel_writer` is a `DuplicateHandle`'d
    /// copy of the helper's stdin pipe. This allows sending cancel commands even while
    /// the helper struct is moved to a runner during action execution.
    pub fn spawn(
        helper_path: &Path,
        user: &dyn SessionUser,
    ) -> Result<(Self, std::fs::File), SessionError> {
        use crate::session_user::WindowsSessionUser;
        use std::collections::HashMap;
        use std::os::windows::io::FromRawHandle;
        use windows::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS};
        use windows::Win32::System::Threading::GetCurrentProcess;

        let wu = user
            .as_any()
            .downcast_ref::<WindowsSessionUser>()
            .ok_or_else(|| {
                SessionError::Runtime("Cross-user on Windows requires WindowsSessionUser".into())
            })?;

        let spawned = crate::win32::spawn_as_user_with_stdin(
            &[helper_path.to_string_lossy().to_string()],
            &HashMap::new(),
            None, // working_dir — helper doesn't need one
            wu.password(),
            wu.user(),
            wu.logon_token(),
        )
        .map_err(|e| SessionError::SubprocessStart {
            command: format!("spawn_as_user_with_stdin {}", helper_path.display()),
            source: std::io::Error::other(e),
        })?;

        let stdin_write = spawned.stdin_write.ok_or_else(|| {
            SessionError::HelperCommunication(
                "spawn_as_user_with_stdin did not return stdin pipe".into(),
            )
        })?;

        // Convert OwnedHandle → File for stdin
        let stdin_file: std::fs::File = stdin_write.into();

        // DuplicateHandle to create a cancel_writer (like dup() on POSIX)
        let cancel_writer = unsafe {
            use std::os::windows::io::AsRawHandle;
            let current_process = GetCurrentProcess();
            let src_handle = windows::Win32::Foundation::HANDLE(stdin_file.as_raw_handle());
            let mut dup_handle = windows::Win32::Foundation::HANDLE::default();
            DuplicateHandle(
                current_process,
                src_handle,
                current_process,
                &mut dup_handle,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
            .map_err(|e| {
                SessionError::HelperCommunication(format!(
                    "DuplicateHandle for cancel_writer failed: {e}"
                ))
            })?;
            std::fs::File::from_raw_handle(dup_handle.0 as std::os::windows::io::RawHandle)
        };

        // Convert stdout OwnedHandle → File
        let stdout_file: std::fs::File = spawned.stdout_read.into();

        let stdin = std::io::BufWriter::new(stdin_file);
        let async_reader = WindowsAsyncHelperReader::new(stdout_file);

        Ok((
            Self {
                process_handle: spawned.process_handle,
                stdin,
                async_reader,
            },
            cancel_writer,
        ))
    }

    /// Send a JSON command to the helper (writes JSON + newline, then flushes).
    pub fn send_command(&mut self, cmd: &serde_json::Value) -> Result<(), SessionError> {
        use std::io::Write;
        serde_json::to_writer(&mut self.stdin, cmd).map_err(|e| {
            SessionError::HelperCommunication(format!("Failed to write to helper stdin: {e}"))
        })?;
        self.stdin.write_all(b"\n").map_err(|e| {
            SessionError::HelperCommunication(format!("Failed to write newline to helper: {e}"))
        })?;
        self.stdin.flush().map_err(|e| {
            SessionError::HelperCommunication(format!("Failed to flush helper stdin: {e}"))
        })?;
        Ok(())
    }

    /// Send "shutdown" and wait for the process to exit.
    pub fn shutdown(&mut self) {
        let _ = self.send_command(&serde_json::Value::String("shutdown".into()));
        unsafe {
            let _ =
                windows::Win32::System::Threading::WaitForSingleObject(self.process_handle, 5000);
            let _ = windows::Win32::Foundation::CloseHandle(self.process_handle);
            self.process_handle = windows::Win32::Foundation::INVALID_HANDLE_VALUE;
        }
    }
}

#[cfg(windows)]
impl Drop for CrossUserHelperWin {
    fn drop(&mut self) {
        // Safety net: terminate the process if shutdown() wasn't called.
        if !self.process_handle.is_invalid() {
            log::warn!(target: "openjd.sessions", "CrossUserHelperWin dropped without shutdown(), terminating child process");
            unsafe {
                let _ = windows::Win32::System::Threading::TerminateProcess(self.process_handle, 1);
                let _ = windows::Win32::Foundation::CloseHandle(self.process_handle);
            }
        }
    }
}

/// Trait for helpers that provide both async reading and sync command sending.
pub(crate) trait AsyncHelper: Send {
    fn async_reader(&mut self) -> &mut dyn AsyncHelperReader;
    fn send_command(&mut self, cmd: &serde_json::Value) -> Result<(), SessionError>;
}

#[cfg(unix)]
impl AsyncHelper for CrossUserHelper {
    fn async_reader(&mut self) -> &mut dyn AsyncHelperReader {
        &mut self.async_reader
    }
    fn send_command(&mut self, cmd: &serde_json::Value) -> Result<(), SessionError> {
        CrossUserHelper::send_command(self, cmd)
    }
}

#[cfg(windows)]
impl AsyncHelper for CrossUserHelperWin {
    fn async_reader(&mut self) -> &mut dyn AsyncHelperReader {
        &mut self.async_reader
    }
    fn send_command(&mut self, cmd: &serde_json::Value) -> Result<(), SessionError> {
        CrossUserHelperWin::send_command(self, cmd)
    }
}

/// Execute a subprocess via a CrossUserHelper, returning the result.
///
/// This is async — the helper stdout is read via `AsyncHelperReader`, allowing
/// `drive_action`'s select loop to process `ActionMessage`s (progress, status,
/// env vars) concurrently while the helper runs.
pub(crate) async fn run_via_helper(
    helper: &mut dyn AsyncHelper,
    config: &crate::subprocess::SubprocessConfig,
    filter: &mut crate::action_filter::ActionFilter,
    session_id: &str,
    message_tx: tokio::sync::mpsc::UnboundedSender<ActionMessage>,
    cancel_writer: Option<&std::fs::File>,
) -> Result<crate::subprocess::SubprocessResult, SessionError> {
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

    // Timeout as async future instead of OS thread
    let timeout_fut = match config.timeout {
        Some(d) => {
            use futures_util::FutureExt;
            tokio::time::sleep(d).boxed()
        }
        None => {
            use futures_util::FutureExt;
            futures_util::future::pending::<()>().boxed()
        }
    };
    tokio::pin!(timeout_fut);
    let mut timed_out = false;

    let mut stdout_collected = String::new();
    let mut saw_fail = false;

    loop {
        tokio::select! {
            biased;
            resp = helper.async_reader().next_response() => {
                let resp = match resp {
                    None => return Err(SessionError::HelperCommunication(
                        "Helper process closed stdout unexpectedly".into(),
                    )),
                    Some(r) => r?,
                };

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
                    let line = crate::subprocess::truncate_line(line);
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
                    if config.debug_collect_stdout {
                        stdout_collected.push_str(&display);
                        stdout_collected.push('\n');
                    }
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

                    let canceled = config
                        .cancel_request_rx
                        .as_ref()
                        .is_some_and(|rx| rx.has_changed().unwrap_or(false));

                    let state = if canceled {
                        ActionState::Canceled
                    } else if timed_out {
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

                return Err(SessionError::HelperCommunication(format!(
                    "Unexpected helper response: {}",
                    resp
                )));
            }
            _ = &mut timeout_fut, if !timed_out => {
                timed_out = true;
                // Send cancel to helper via the cancel_writer
                if let Some(writer) = cancel_writer {
                    use std::io::Write;
                    let mut w = writer.try_clone().map_err(|e| {
                        SessionError::HelperCommunication(format!("Failed to clone cancel_writer: {e}"))
                    })?;
                    let cancel_method = &config.cancel_method;
                    let notify_period = match cancel_method {
                        crate::runner::CancelMethod::NotifyThenTerminate { terminate_delay } => {
                            terminate_delay.as_secs()
                        }
                        crate::runner::CancelMethod::Terminate => 0,
                    };
                    if notify_period == 0 {
                        let _ = writeln!(w, r#"{{"cancel":"TERMINATE"}}"#);
                    } else {
                        let _ = writeln!(
                            w,
                            r#"{{"cancel":"NOTIFY_THEN_TERMINATE","notifyPeriodInSeconds":{notify_period}}}"#
                        );
                    }
                    let _ = w.flush();
                }
            }
        }
    }
}
