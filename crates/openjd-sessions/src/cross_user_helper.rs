// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

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

/// Manages a long-lived helper process for cross-user command execution (POSIX).
///
/// On POSIX: launched via `sudo -u <user> -i <helper_path>`.
/// Communicates over newline-delimited JSON on stdin/stdout.
#[cfg(unix)]
pub(crate) struct CrossUserHelper {
    child: std::process::Child,
    stdin: std::io::BufWriter<std::process::ChildStdin>,
    stdout: std::io::BufReader<std::process::ChildStdout>,
}

/// Windows variant: the child is a raw process handle from `CreateProcessAsUserW`,
/// not a `std::process::Child`. We wrap stdin/stdout from the pipe handles.
#[cfg(windows)]
pub(crate) struct CrossUserHelperWin {
    process_handle: windows::Win32::Foundation::HANDLE,
    stdin: std::io::BufWriter<std::fs::File>,
    stdout: std::io::BufReader<std::fs::File>,
}

// SAFETY: Windows HANDLE is a kernel object handle that is safe to send across
// threads. The other fields (BufWriter<File>, BufReader<File>) are already Send.
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
            SessionError::Runtime("spawn_as_user_with_stdin did not return stdin pipe".into())
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
                SessionError::Runtime(format!("DuplicateHandle for cancel_writer failed: {e}"))
            })?;
            std::fs::File::from_raw_handle(dup_handle.0 as std::os::windows::io::RawHandle)
        };

        // Convert stdout OwnedHandle → File
        let stdout_file: std::fs::File = spawned.stdout_read.into();

        let stdin = std::io::BufWriter::new(stdin_file);
        let stdout = std::io::BufReader::new(stdout_file);

        Ok((
            Self {
                process_handle: spawned.process_handle,
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

/// Trait for the shared helper interface used by `run_via_helper`.
/// Both `CrossUserHelper` (POSIX) and `CrossUserHelperWin` (Windows) implement this.
pub(crate) trait HelperIO {
    fn send_command(&mut self, cmd: &serde_json::Value) -> Result<(), SessionError>;
    fn read_response(&mut self) -> Result<serde_json::Value, SessionError>;
}

#[cfg(unix)]
impl HelperIO for CrossUserHelper {
    fn send_command(&mut self, cmd: &serde_json::Value) -> Result<(), SessionError> {
        self.send_command(cmd)
    }
    fn read_response(&mut self) -> Result<serde_json::Value, SessionError> {
        self.read_response()
    }
}

#[cfg(windows)]
impl HelperIO for CrossUserHelperWin {
    fn send_command(&mut self, cmd: &serde_json::Value) -> Result<(), SessionError> {
        self.send_command(cmd)
    }
    fn read_response(&mut self) -> Result<serde_json::Value, SessionError> {
        self.read_response()
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
/// The cancel_writer (dup'd fd / DuplicateHandle) and helper.stdin both write
/// to the same underlying pipe, but cannot race in practice:
/// - `send_command` completes and flushes before the response loop begins
/// - The timeout thread only wakes after the configured timeout duration,
///   long after `send_command` has finished
/// - `Session::cancel_action` requires `&mut self`, which is exclusively held
///   by the async method that called `run_via_helper`, preventing concurrent
///   cancel writes during command submission
pub(crate) fn run_via_helper(
    helper: &mut dyn HelperIO,
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
        let cancel_method = config.cancel_method.clone();
        let handle = std::thread::spawn(move || {
            let (lock, cvar) = &*done;
            let guard = lock.lock().unwrap();
            let (guard, _) = cvar.wait_timeout_while(guard, timeout, |d| !*d).unwrap();
            if *guard {
                return;
            } // command finished before timeout
            drop(guard);
            timed_out.store(true, std::sync::atomic::Ordering::Release);
            let notify_period = match cancel_method {
                crate::runner::CancelMethod::NotifyThenTerminate { terminate_delay } => {
                    terminate_delay.as_secs()
                }
                crate::runner::CancelMethod::Terminate => 0,
            };
            if notify_period == 0 {
                // Immediate kill — no grace period.
                let cancel_terminate = r#"{"cancel":"TERMINATE"}"#;
                let _ = writeln!(writer, "{cancel_terminate}");
                let _ = writer.flush();
            } else {
                let cancel_notify = format!(
                    r#"{{"cancel":"NOTIFY_THEN_TERMINATE","notifyPeriodInSeconds":{notify_period}}}"#
                );
                let _ = writeln!(writer, "{cancel_notify}");
                let _ = writer.flush();
                // Grace period — also cancellable
                let guard = lock.lock().unwrap();
                let (guard, _) = cvar
                    .wait_timeout_while(guard, std::time::Duration::from_secs(notify_period), |d| {
                        !*d
                    })
                    .unwrap();
                if *guard {
                    return;
                }
                drop(guard);
                let cancel_terminate = r#"{"cancel":"TERMINATE"}"#;
                let _ = writeln!(writer, "{cancel_terminate}");
                let _ = writer.flush();
            }
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
            if config.collect_stdout {
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
