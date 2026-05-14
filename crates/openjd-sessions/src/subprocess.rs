// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Async subprocess execution with real-time message streaming.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::action::ActionMessage;
use crate::action::ActionState;
use crate::action_filter::{ActionFilter, ActionMessageKind, ActionMessageValue};
use crate::error::SessionError;
use crate::logging::LogContent;
use crate::runner::CancelMethod;
use crate::session_log;
use crate::session_user::SessionUser;
use std::sync::Arc;

/// Grace time to wait for `c.wait()` to reap the child when the stdout
/// read loop exited through a non-kill path (natural EOF, read error) or
/// after a `NotifyThenTerminate` cancellation where the process may still
/// be winding down gracefully in response to the notify signal. Used
/// when no terminate has been issued yet from inside the stdout-read
/// loop.
const STDOUT_GRACE_TIME: Duration = Duration::from_secs(5);

/// Grace time to wait for `c.wait()` to reap the child after we have
/// already issued `send_terminate` from inside the stdout-read loop
/// (timeout fired, urgent cancel with `time_limit=0`, or
/// `CancelMethod::Terminate`).
///
/// `send_terminate` resolves to SIGKILL on Unix and `TerminateProcess`
/// (via `kill_process_tree`) on Windows. In these cases the process
/// tree was killed at least `STDOUT_DRAIN_AFTER_KILL` (1s) ago — by
/// the time the drain deadline fires and we get here, `c.wait()`
/// should reap the already-dead child almost immediately. Two seconds
/// is a generous bound for runtime scheduling delay (especially on
/// loaded Windows CI) while still being much tighter than the 5s we
/// allow on graceful-exit paths.
///
/// A shorter bound here means the three "timeout should have fired
/// quickly" integration tests in `tests/test_session.rs` don't pick up
/// 3-4s of dead time on every run, which in turn means their 10s
/// assertion holds with comfortable margin on slow CI.
const STDOUT_GRACE_TIME_POST_TERMINATE: Duration = Duration::from_secs(2);

/// Grace time to drain stdout after sending a kill signal.
const STDOUT_DRAIN_AFTER_KILL: Duration = Duration::from_secs(1);

/// Maximum line length for stdout reading.
pub(crate) const LOG_LINE_MAX_LENGTH: usize = 64 * 1024;

/// Truncate a line to at most `LOG_LINE_MAX_LENGTH` bytes on a valid UTF-8 char boundary.
pub(crate) fn truncate_line(line: &str) -> &str {
    if line.len() > LOG_LINE_MAX_LENGTH {
        &line[..line.floor_char_boundary(LOG_LINE_MAX_LENGTH)]
    } else {
        line
    }
}

/// Result of running a subprocess action.
#[derive(Debug)]
pub struct SubprocessResult {
    pub state: ActionState,
    pub exit_code: Option<i32>,
    pub stdout: String,
}

/// Configuration for running a subprocess.
pub struct SubprocessConfig {
    pub args: Vec<String>,
    pub env_vars: HashMap<String, Option<String>>,
    pub working_dir: Option<PathBuf>,
    pub timeout: Option<Duration>,
    pub user: Option<Arc<dyn SessionUser>>,
    pub cancel_method: CancelMethod,
    pub cancel_request_rx: Option<tokio::sync::watch::Receiver<Option<Duration>>>,
    /// Whether to accumulate all stdout into `SubprocessResult.stdout`.
    /// Intended for debugging only — production callers should leave this
    /// `false` and observe output through the real-time callback.
    /// Default is `false` — lines are still streamed through the filter and
    /// callback in real time, but the collected string stays empty.
    pub debug_collect_stdout: bool,
}

// ---------------------------------------------------------------------------
// Platform-specific signal / process-group helpers
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod platform {
    use super::*;

    /// Send SIGTERM to the process group.
    pub fn notify_process_group(pgid: i32) -> Result<(), std::io::Error> {
        nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(pgid),
            nix::sys::signal::Signal::SIGTERM,
        )
        .map_err(std::io::Error::other)
    }

    /// Send SIGKILL to the process group.
    pub fn terminate_process_group(pgid: i32) -> Result<(), std::io::Error> {
        nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(pgid),
            nix::sys::signal::Signal::SIGKILL,
        )
        .map_err(std::io::Error::other)
    }

    /// Send SIGKILL to the process group.
    pub fn send_terminate(pid: i32) {
        let _ = terminate_process_group(pid);
    }

    /// Send SIGTERM to the process group.
    pub fn send_notify(pid: i32) {
        let _ = notify_process_group(pid);
    }

    /// Spawn a delayed SIGKILL after a grace period.
    pub fn spawn_delayed_terminate(pid: i32, delay: Duration) {
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = terminate_process_group(pid);
        });
    }

    /// Configure the Command for POSIX: setsid + dup2 stderr→stdout via pre_exec.
    ///
    /// Returns `None` — on POSIX the merge happens in the child via dup2,
    /// so the caller reads from `child.stdout` as normal.
    ///
    /// # Safety
    /// Calls `pre_exec` which runs in the forked child before exec.
    pub unsafe fn configure_command(
        cmd: &mut Command,
        use_setsid: bool,
    ) -> Option<Box<dyn tokio::io::AsyncRead + Unpin + Send>> {
        cmd.pre_exec(move || {
            // Redirect stderr to stdout so output ordering is preserved
            if nix::libc::dup2(1, 2) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if use_setsid {
                nix::libc::setsid();
            }
            Ok(())
        });
        None
    }
}

#[cfg(windows)]
mod platform {
    use super::*;

    use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, TerminateProcess, CREATE_NEW_PROCESS_GROUP,
        PROCESS_QUERY_INFORMATION, PROCESS_TERMINATE,
    };

    /// Send CTRL_BREAK_EVENT to a process group for graceful cancellation.
    ///
    /// Mirrors Python's `_signal_win_subprocess.py`: detach from current console,
    /// attach to the target's console, send CTRL_BREAK, then re-attach to our own.
    ///
    /// Note: When running as a Windows service (Session 0), console manipulation
    /// doesn't work reliably. In that case we return false so the caller falls
    /// back to terminate (immediate kill).
    fn send_ctrl_break(pid: u32) -> bool {
        use windows::Win32::System::Console::{
            AttachConsole, FreeConsole, GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT,
        };

        // Console APIs don't work from Session 0 (Windows services).
        // Fall back to terminate for reliable cancellation.
        if crate::win32::is_session_zero() {
            log::info!(target: "openjd.sessions", "Running in Session 0, skipping CTRL_BREAK (will fall back to terminate)");
            return false;
        }

        unsafe {
            // Detach from our console
            let _ = FreeConsole();
            // Attach to the target process's console
            if AttachConsole(pid).is_err() {
                // Re-attach to parent if we can't attach to target
                let _ = AttachConsole(u32::MAX); // ATTACH_PARENT_PROCESS
                return false;
            }
            let ok = GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid).is_ok();
            // Detach from target and re-attach to parent
            let _ = FreeConsole();
            let _ = AttachConsole(u32::MAX);
            ok
        }
    }

    /// Kill a single process by PID.
    fn kill_process(pid: u32) -> bool {
        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, false, pid);
            if let Ok(h) = handle {
                let ok = TerminateProcess(h, 1).is_ok();
                let _ = CloseHandle(h);
                ok
            } else {
                false
            }
        }
    }

    /// Check if a process is still alive.
    #[allow(dead_code)]
    fn is_process_alive(pid: u32) -> bool {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_INFORMATION, false, pid);
            if let Ok(h) = handle {
                let mut code = 0u32;
                let _ = GetExitCodeProcess(h, &mut code);
                let _ = CloseHandle(h);
                code == STILL_ACTIVE.0 as u32
            } else {
                false
            }
        }
    }

    /// Get child PIDs of a process using CreateToolhelp32Snapshot.
    fn get_child_pids(parent_pid: u32) -> Vec<u32> {
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        let mut children = Vec::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if let Ok(snap) = snap {
                let mut entry = PROCESSENTRY32W {
                    dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                    ..Default::default()
                };
                if Process32FirstW(snap, &mut entry).is_ok() {
                    loop {
                        if entry.th32ParentProcessID == parent_pid {
                            children.push(entry.th32ProcessID);
                        }
                        if Process32NextW(snap, &mut entry).is_err() {
                            break;
                        }
                    }
                }
                let _ = CloseHandle(snap);
            }
        }
        children
    }

    /// Kill a process tree: collect all descendants, then kill leaf-to-root.
    /// Mirrors Python's `_windows_process_killer.py`.
    fn kill_process_tree(root_pid: u32) {
        let mut to_kill = Vec::new();
        collect_tree(root_pid, &mut to_kill);
        // Kill in reverse order (children first)
        for &pid in to_kill.iter().rev() {
            kill_process(pid);
        }
    }

    fn collect_tree(pid: u32, result: &mut Vec<u32>) {
        result.push(pid);
        for child in get_child_pids(pid) {
            collect_tree(child, result);
        }
    }

    /// Terminate: kill the entire process tree.
    pub fn send_terminate(pid: i32) {
        kill_process_tree(pid as u32);
    }

    /// Notify: send CTRL_BREAK_EVENT for graceful shutdown.
    pub fn send_notify(pid: i32) {
        if !send_ctrl_break(pid as u32) {
            log::warn!(target: "openjd.sessions", "Failed to send CTRL_BREAK to pid {pid}, falling back to terminate");
            send_terminate(pid);
        }
    }

    /// Delayed terminate: kill the process tree after a grace period.
    pub fn spawn_delayed_terminate(pid: i32, delay: Duration) {
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            kill_process_tree(pid as u32);
        });
    }

    /// Configure the Command for Windows: CREATE_NEW_PROCESS_GROUP + merge stderr into stdout.
    ///
    /// Creates a single OS pipe and sets both stdout and stderr to the write end,
    /// mirroring POSIX `dup2(1, 2)`. Returns the read end as an async reader.
    ///
    /// # Safety
    /// This function is safe on Windows (no pre_exec).
    pub unsafe fn configure_command(
        cmd: &mut Command,
        _use_setsid: bool,
    ) -> Option<Box<dyn tokio::io::AsyncRead + Unpin + Send>> {
        use std::os::windows::io::{FromRawHandle, OwnedHandle};
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Security::SECURITY_ATTRIBUTES;
        use windows::Win32::System::Pipes::CreatePipe;

        // CREATE_NEW_PROCESS_GROUP is required for CTRL_BREAK_EVENT to work
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP.0);

        // Create an anonymous pipe: read_handle for us, write_handle for the child
        let mut read_handle = HANDLE::default();
        let mut write_handle = HANDLE::default();
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            bInheritHandle: true.into(),
            lpSecurityDescriptor: std::ptr::null_mut(),
        };
        if CreatePipe(&mut read_handle, &mut write_handle, Some(&sa), 0).is_err() {
            // Fall back to separate pipes
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());
            return None;
        }

        // Convert write handle to Stdio for the child process.
        // We need two copies: one for stdout, one for stderr.
        let write_owned = OwnedHandle::from_raw_handle(write_handle.0);
        let write_stdio_stdout = std::process::Stdio::from(write_owned);

        // Duplicate the write handle for stderr
        use windows::Win32::Foundation::DuplicateHandle;
        use windows::Win32::System::Threading::GetCurrentProcess;
        let mut write_handle_dup = HANDLE::default();
        let current_process = GetCurrentProcess();
        if DuplicateHandle(
            current_process,
            write_handle,
            current_process,
            &mut write_handle_dup,
            0,
            true, // bInheritHandle
            windows::Win32::Foundation::DUPLICATE_SAME_ACCESS,
        )
        .is_err()
        {
            // Fall back: just use the one handle for stdout, pipe stderr separately
            cmd.stdout(write_stdio_stdout);
            cmd.stderr(std::process::Stdio::piped());
            let read_owned = OwnedHandle::from_raw_handle(read_handle.0);
            let read_std: std::fs::File = std::fs::File::from(read_owned);
            let read_tokio = tokio::fs::File::from_std(read_std);
            return Some(Box::new(read_tokio));
        }
        let write_owned_dup = OwnedHandle::from_raw_handle(write_handle_dup.0);
        let write_stdio_stderr = std::process::Stdio::from(write_owned_dup);

        cmd.stdout(write_stdio_stdout);
        cmd.stderr(write_stdio_stderr);

        // Convert read handle to an async reader
        let read_owned = OwnedHandle::from_raw_handle(read_handle.0);
        let read_std: std::fs::File = std::fs::File::from(read_owned);
        let read_tokio = tokio::fs::File::from_std(read_std);
        Some(Box::new(read_tokio))
    }
}

use platform::*;

/// Write cancel_info.json to the working directory as required by the OpenJD spec
/// for NotifyThenTerminate cancelation.
fn write_cancel_info(working_dir: &Path, terminate_delay: Duration) {
    let notify_end = std::time::SystemTime::now() + terminate_delay;
    let secs = notify_end
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Format as ISO 8601 UTC
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let total_days = secs / 86400;
    // Simple date calculation from days since epoch
    let (y, mo, d) = days_to_ymd(total_days);
    let timestamp = format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z");
    let info = serde_json::json!({ "NotifyEnd": timestamp });
    let path = working_dir.join("cancel_info.json");
    let _ = std::fs::write(&path, serde_json::to_string(&info).unwrap_or_default());
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(total_days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = total_days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Format a command argument list for logging, applying redaction for any
/// `openjd_redacted_env:` tokens that may appear in the arguments.
pub(crate) fn format_command_for_log(args: &[String]) -> String {
    let joined =
        shlex::try_join(args.iter().map(|s| s.as_str())).unwrap_or_else(|_| args.join(" "));
    crate::action_filter::redact_openjd_redacted_env_requests(&joined)
}

/// Run a subprocess asynchronously with real-time stdout streaming through an ActionFilter.
///
/// Spawns the process with merged environment variables, streams stdout line-by-line
/// through the ActionFilter, supports cancellation and timeout, uses process groups
/// (setsid) for proper signal delivery, and handles stdout grace time for detached
/// grandchild processes.
///
/// Parsed `ActionMessage` values are sent through `message_tx` in real-time as
/// stdout lines are processed.
pub async fn run_subprocess(
    config: SubprocessConfig,
    filter: &mut ActionFilter,
    session_id: &str,
    message_tx: mpsc::UnboundedSender<ActionMessage>,
    cancel_token: CancellationToken,
) -> Result<SubprocessResult, SessionError> {
    let args = &config.args;
    if args.is_empty() {
        return Err(SessionError::Runtime("No command specified".into()));
    }

    // Cross-user execution must go through the helper binary.
    if config.user.as_deref().is_some_and(|u| !u.is_process_user()) {
        return Err(SessionError::Runtime(
            "Cross-user subprocess execution requires the helper binary. \
             Use run_via_helper instead of run_subprocess for cross-user actions."
                .into(),
        ));
    }

    // Build merged environment
    let mut merged: HashMap<String, String> = std::env::vars().collect();
    for (k, v) in &config.env_vars {
        match v {
            Some(val) => {
                merged.insert(k.clone(), val.clone());
            }
            None => {
                merged.remove(k);
            }
        }
    }

    // Log the command line (redacting any openjd_redacted_env tokens)
    session_log!(
        info,
        session_id,
        LogContent::FILE_PATH | LogContent::PROCESS_CONTROL,
        "Running command {}",
        format_command_for_log(args)
    );

    // Spawn the process via tokio::process::Command (same-user only;
    // cross-user was rejected above).
    #[cfg(windows)]
    let win32_process_handle: Option<windows::Win32::Foundation::HANDLE> = None;

    #[allow(unused_mut)]
    let (mut child, pid, stdout_for_reading): (
        Option<tokio::process::Child>,
        i32,
        Option<Box<dyn tokio::io::AsyncRead + Unpin + Send>>,
    ) = {
        let mut cmd = Command::new(&args[0]);
        cmd.args(&args[1..]);
        cmd.env_clear();
        for (k, v) in &merged {
            cmd.env(k, v);
        }
        if let Some(dir) = &config.working_dir {
            cmd.current_dir(dir);
        }
        let merged_reader = unsafe { configure_command(&mut cmd, true) };
        if merged_reader.is_none() {
            cmd.stdout(std::process::Stdio::piped());
        }
        let mut c = cmd.spawn().map_err(|e| {
            session_log!(
                info,
                session_id,
                LogContent::EXCEPTION_INFO | LogContent::PROCESS_CONTROL,
                "Process failed to start: '{}': {}",
                args[0],
                e
            );
            SessionError::SubprocessStart {
                command: args[0].clone(),
                source: e,
            }
        })?;
        let p = c.id().unwrap_or(0) as i32;
        let stdout = merged_reader.or_else(|| {
            c.stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>)
        });
        (Some(c), p, stdout)
    };

    session_log!(
        info,
        session_id,
        LogContent::PROCESS_CONTROL,
        "Command started as pid: {}",
        pid
    );
    session_log!(
        info,
        session_id,
        LogContent::BANNER | LogContent::COMMAND_OUTPUT,
        "Output:"
    );

    // Read merged stdout+stderr from the child
    let mut cancel_requested = false;
    let mut timed_out = false;
    // True once we've issued `send_terminate` on the process tree from
    // inside the stdout read loop (timeout path, urgent cancel, or
    // `CancelMethod::Terminate`). `send_terminate` is the crate's
    // platform-agnostic "kill now" — SIGKILL on Unix, `TerminateProcess`
    // via `kill_process_tree` on Windows.
    //
    // Stays false for `NotifyThenTerminate` cancel — there the terminate
    // is only scheduled (via `spawn_delayed_terminate`), so the process
    // may still be winding down gracefully in response to the notify
    // signal when the loop exits and needs the longer `STDOUT_GRACE_TIME`
    // on the final `c.wait()`.
    let mut terminate_sent = false;
    let mut stdout_collected = String::new();
    let mut saw_fail = false;

    if let Some(stdout) = stdout_for_reading {
        let mut reader = BufReader::new(stdout);
        let mut line_buf = Vec::new();

        // Create timeout future once, pin it for reuse across loop iterations
        let timeout_fut = async {
            match config.timeout {
                Some(d) => tokio::time::sleep(d).await,
                None => std::future::pending().await,
            }
        };
        tokio::pin!(timeout_fut);

        // Grace period for stdout to drain after process termination.
        // On Windows, killed processes (especially MSYS2 sh.exe) may leave
        // inherited pipe handles open in orphaned child processes, preventing
        // EOF. This deadline ensures we don't hang forever.
        let drain_deadline = tokio::time::sleep(Duration::MAX);
        tokio::pin!(drain_deadline);

        loop {
            tokio::select! {
                biased;

                _ = &mut drain_deadline, if cancel_requested => {
                    session_log!(info, session_id, LogContent::PROCESS_CONTROL,
                        "Stdout drain grace period expired, stopping read loop");
                    break;
                }

                _ = cancel_token.cancelled(), if !cancel_requested => {
                    cancel_requested = true;
                    drain_deadline.as_mut().reset(tokio::time::Instant::now() + STDOUT_DRAIN_AFTER_KILL);
                    let time_limit = config.cancel_request_rx.as_ref()
                        .and_then(|rx| *rx.borrow());

                    match (&config.cancel_method, time_limit) {
                        (_, Some(limit)) if limit.is_zero() => {
                            session_log!(info, session_id, LogContent::PROCESS_CONTROL, "Urgent cancel (time_limit=0), sending SIGKILL to process group {}", pid);
                            send_terminate(pid);
                            terminate_sent = true;
                        }
                        (CancelMethod::Terminate, _) => {
                            session_log!(info, session_id, LogContent::PROCESS_CONTROL, "Sending SIGKILL to process group {}", pid);
                            send_terminate(pid);
                            terminate_sent = true;
                        }
                        (CancelMethod::NotifyThenTerminate { terminate_delay }, _) => {
                            let delay = match time_limit {
                                Some(limit) => limit.min(*terminate_delay),
                                None => *terminate_delay,
                            };
                            if let Some(dir) = &config.working_dir {
                                write_cancel_info(dir, delay);
                            }
                            session_log!(info, session_id, LogContent::PROCESS_CONTROL, "Sending SIGTERM to process group {} (grace period: {:?})", pid, delay);
                            send_notify(pid);
                            spawn_delayed_terminate(pid, delay);
                            // Deliberately do NOT set terminate_sent here —
                            // the terminate is only scheduled (via
                            // `spawn_delayed_terminate`), not yet delivered.
                            // The process may still be exiting gracefully in
                            // response to the notify signal, and the final
                            // `c.wait()` should get the longer
                            // `STDOUT_GRACE_TIME`.
                        }
                    }
                }

                _ = &mut timeout_fut, if !cancel_requested && !timed_out => {
                    timed_out = true;
                    cancel_requested = true;
                    drain_deadline.as_mut().reset(tokio::time::Instant::now() + STDOUT_DRAIN_AFTER_KILL);
                    session_log!(info, session_id, LogContent::PROCESS_CONTROL, "Action timed out, sending SIGKILL to process group");
                    send_terminate(pid);
                    terminate_sent = true;
                }

                n = reader.read_until(b'\n', &mut line_buf) => {
                    match n {
                        Ok(0) => break, // EOF
                        Ok(_) => {
                            // Strip trailing newline (and \r on Windows)
                            if line_buf.last() == Some(&b'\n') {
                                line_buf.pop();
                            }
                            if line_buf.last() == Some(&b'\r') {
                                line_buf.pop();
                            }
                            let line = String::from_utf8_lossy(&line_buf);
                            let line = truncate_line(&line).to_string();
                            line_buf.clear();
                            let (display, pass_through) = process_line(&line, filter, session_id, &message_tx, &mut saw_fail);
                            if pass_through && filter.min_log_level() <= 20 {
                                session_log!(info, session_id, LogContent::COMMAND_OUTPUT, "{}", display);
                            }
                            if config.debug_collect_stdout {
                                stdout_collected.push_str(&display);
                                stdout_collected.push('\n');
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    }

    // Wait for process to exit
    let exit_status = if let Some(ref mut c) = child {
        // If we already issued `send_terminate` from inside the read loop
        // (timeout, urgent cancel, or Terminate cancel), the child should
        // be dead and `c.wait()` just needs to reap it — a short bound
        // is plenty. Otherwise give the full 5s to accommodate graceful
        // shutdown (natural EOF or `NotifyThenTerminate`).
        let grace = if terminate_sent {
            STDOUT_GRACE_TIME_POST_TERMINATE
        } else {
            STDOUT_GRACE_TIME
        };
        match tokio::time::timeout(grace, c.wait()).await {
            Ok(Ok(s)) => Some(s),
            Ok(Err(_)) => {
                send_terminate(pid);
                None
            }
            Err(_) => {
                send_terminate(pid);
                c.wait().await.ok()
            }
        }
    } else {
        // Windows cross-user: wait on the raw process handle
        #[cfg(windows)]
        {
            win32_process_handle.map(|h| {
                use std::os::windows::process::ExitStatusExt;
                use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
                unsafe {
                    let _ = WaitForSingleObject(h, 60000);
                    let mut code = 0u32;
                    let _ = GetExitCodeProcess(h, &mut code);
                    let _ = windows::Win32::Foundation::CloseHandle(h);
                    std::process::ExitStatus::from_raw(code)
                }
            })
        }
        #[cfg(not(windows))]
        {
            None
        }
    };

    let exit_code = exit_status.and_then(|s| s.code());
    session_log!(
        info,
        session_id,
        LogContent::PROCESS_CONTROL,
        "Process exit code: {}",
        exit_code.map_or("N/A".to_string(), |c| c.to_string())
    );

    let state = if timed_out {
        ActionState::Timeout
    } else if cancel_requested || cancel_token.is_cancelled() {
        ActionState::Canceled
    } else if saw_fail {
        ActionState::Failed
    } else if exit_status.is_some_and(|s| s.success()) {
        ActionState::Success
    } else {
        ActionState::Failed
    };

    Ok(SubprocessResult {
        state,
        exit_code,
        stdout: stdout_collected,
    })
}

pub(crate) fn process_line(
    line: &str,
    filter: &mut ActionFilter,
    session_id: &str,
    message_tx: &mpsc::UnboundedSender<ActionMessage>,
    saw_fail: &mut bool,
) -> (String, bool) {
    let (callbacks, pass_through, display) = filter.filter_message(line, session_id);
    for cb in callbacks {
        let cancel = cb.cancel;
        let msg = match cb.kind {
            ActionMessageKind::Progress => {
                if let ActionMessageValue::Float(v) = cb.value {
                    Some(ActionMessage::Progress(v))
                } else {
                    None
                }
            }
            ActionMessageKind::Status => {
                if let ActionMessageValue::String(s) = cb.value {
                    Some(ActionMessage::Status(s))
                } else {
                    None
                }
            }
            ActionMessageKind::Fail => {
                if let ActionMessageValue::String(s) = cb.value {
                    *saw_fail = true;
                    Some(ActionMessage::Fail(s))
                } else {
                    None
                }
            }
            ActionMessageKind::Env => {
                if let ActionMessageValue::EnvVar { name, value } = cb.value {
                    Some(ActionMessage::SetEnv { name, value })
                } else {
                    None
                }
            }
            ActionMessageKind::UnsetEnv => {
                if let ActionMessageValue::String(name) = cb.value {
                    Some(ActionMessage::UnsetEnv { name })
                } else {
                    None
                }
            }
            ActionMessageKind::RedactedEnv => {
                if let ActionMessageValue::EnvVar { name, value } = cb.value {
                    Some(ActionMessage::RedactedEnv { name, value })
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(msg) = msg {
            let _ = message_tx.send(msg);
        }
        if cancel {
            let fail_msg = "Action canceled due to malformed command".to_string();
            let _ = message_tx.send(ActionMessage::CancelMarkFailed {
                fail_message: fail_msg,
            });
        }
    }
    (display, pass_through)
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn test_cancel_ntt_with_zero_time_limit_is_immediate() {
        use tokio_util::sync::CancellationToken;

        let token = CancellationToken::new();
        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(None);
        let (msg_tx, _msg_rx) = tokio::sync::mpsc::unbounded_channel();

        let config = SubprocessConfig {
            args: vec!["sleep".into(), "30".into()],
            env_vars: HashMap::new(),
            working_dir: None,
            timeout: None,
            user: None,
            cancel_method: CancelMethod::NotifyThenTerminate {
                terminate_delay: Duration::from_secs(60),
            },
            cancel_request_rx: Some(cancel_rx),
            debug_collect_stdout: false,
        };

        let t = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let _ = _cancel_tx.send(Some(Duration::ZERO));
            t.cancel();
        });

        let mut filter = crate::action_filter::ActionFilter::new("test", true, false);
        let start = std::time::Instant::now();
        let result = run_subprocess(config, &mut filter, "test", msg_tx, token)
            .await
            .unwrap();
        let elapsed = start.elapsed();

        assert_eq!(result.state, ActionState::Canceled);
        assert!(
            elapsed < Duration::from_secs(5),
            "took {:?}, expected < 5s",
            elapsed
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_cancel_ntt_without_time_limit_uses_default() {
        use tokio_util::sync::CancellationToken;

        let token = CancellationToken::new();
        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(None);
        let (msg_tx, _msg_rx) = tokio::sync::mpsc::unbounded_channel();

        // Use python3 to reliably ignore SIGTERM in a single process. Using
        // `sh -c 'trap "" TERM; sleep 30'` is fragile: some sh implementations
        // install a userspace handler for the trap instead of SIG_IGN, so the
        // SIG_IGN inheritance rule does not protect the child `sleep`, and
        // killpg(SIGTERM) kills `sleep` — terminating the script within ms
        // rather than waiting for the 1s SIGKILL.
        //
        // The script writes a sentinel file after installing the handler so
        // the test can wait for readiness before canceling — avoiding a race
        // where SIGTERM arrives before SIG_IGN is installed.
        let ready_dir = tempfile::tempdir().unwrap();
        let ready_path = ready_dir.path().join("ready");
        let py_script = format!(
            "import signal, time, pathlib; signal.signal(signal.SIGTERM, signal.SIG_IGN); pathlib.Path('{}').write_text('ok'); time.sleep(30)",
            ready_path.display()
        );
        let config = SubprocessConfig {
            args: vec!["python3".into(), "-c".into(), py_script],
            env_vars: HashMap::new(),
            working_dir: None,
            timeout: None,
            user: None,
            cancel_method: CancelMethod::NotifyThenTerminate {
                terminate_delay: Duration::from_secs(1),
            },
            cancel_request_rx: Some(cancel_rx),
            debug_collect_stdout: false,
        };

        let t = token.clone();
        let ready_path_clone = ready_path.clone();
        tokio::spawn(async move {
            // Wait for the python process to install its SIGTERM handler
            for _ in 0..100 {
                if ready_path_clone.exists() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            t.cancel();
        });

        let mut filter = crate::action_filter::ActionFilter::new("test", true, false);
        let start = std::time::Instant::now();
        let result = run_subprocess(config, &mut filter, "test", msg_tx, token)
            .await
            .unwrap();
        let elapsed = start.elapsed();

        assert_eq!(result.state, ActionState::Canceled);
        // After cancel, the 1s terminate_delay must elapse before SIGKILL
        assert!(
            elapsed >= Duration::from_millis(800),
            "took {:?}, expected >= 800ms",
            elapsed
        );
        assert!(
            elapsed < Duration::from_secs(10),
            "took {:?}, expected < 10s",
            elapsed
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_cancel_terminate_ignores_time_limit() {
        use tokio_util::sync::CancellationToken;

        let token = CancellationToken::new();
        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(None);
        let (msg_tx, _msg_rx) = tokio::sync::mpsc::unbounded_channel();

        let config = SubprocessConfig {
            args: vec!["sleep".into(), "30".into()],
            env_vars: HashMap::new(),
            working_dir: None,
            timeout: None,
            user: None,
            cancel_method: CancelMethod::Terminate,
            cancel_request_rx: Some(cancel_rx),
            debug_collect_stdout: false,
        };

        let t = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let _ = _cancel_tx.send(Some(Duration::from_secs(10)));
            t.cancel();
        });

        let mut filter = crate::action_filter::ActionFilter::new("test", true, false);
        let start = std::time::Instant::now();
        let result = run_subprocess(config, &mut filter, "test", msg_tx, token)
            .await
            .unwrap();
        let elapsed = start.elapsed();

        assert_eq!(result.state, ActionState::Canceled);
        assert!(
            elapsed < Duration::from_secs(2),
            "took {:?}, expected < 2s",
            elapsed
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn test_cancel_terminate_on_windows() {
        use tokio_util::sync::CancellationToken;

        let token = CancellationToken::new();
        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(None);
        let (msg_tx, _msg_rx) = tokio::sync::mpsc::unbounded_channel();

        // Use powershell sleep which is a real process (not a shell builtin)
        let config = SubprocessConfig {
            args: vec![
                "powershell".into(),
                "-Command".into(),
                "Start-Sleep 30".into(),
            ],
            env_vars: HashMap::new(),
            working_dir: None,
            timeout: None,
            user: None,
            cancel_method: CancelMethod::Terminate,
            cancel_request_rx: Some(cancel_rx),
            debug_collect_stdout: false,
        };

        let t = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            t.cancel();
        });

        let mut filter = crate::action_filter::ActionFilter::new("test", true, false);
        let start = std::time::Instant::now();
        let result = run_subprocess(config, &mut filter, "test", msg_tx, token)
            .await
            .unwrap();
        let elapsed = start.elapsed();

        assert_eq!(result.state, ActionState::Canceled);
        assert!(
            elapsed < Duration::from_secs(5),
            "Cancel took {:?}, expected < 5s — process was not killed promptly",
            elapsed
        );
    }

    #[test]
    fn test_format_command_for_log_simple() {
        let args = vec!["echo".to_string(), "hello".to_string(), "world".to_string()];
        let result = format_command_for_log(&args);
        assert_eq!(result, "echo hello world");
    }

    #[test]
    fn test_format_command_for_log_with_spaces() {
        let args = vec!["echo".to_string(), "hello world".to_string()];
        let result = format_command_for_log(&args);
        // Should be shell-quoted
        assert!(result.contains("hello world"), "got: {result}");
    }

    #[test]
    fn test_format_command_for_log_redacts_secret() {
        let args = vec![
            "python".to_string(),
            "-c".to_string(),
            "print('openjd_redacted_env: PASSWORD=secret123')".to_string(),
        ];
        let result = format_command_for_log(&args);
        assert!(!result.contains("secret123"), "secret leaked in: {result}");
        assert!(
            result.contains("openjd_redacted_env:"),
            "token missing in: {result}"
        );
        assert!(
            result.contains("********"),
            "redaction missing in: {result}"
        );
    }

    #[test]
    fn test_format_command_for_log_no_redaction_needed() {
        let args = vec![
            "python".to_string(),
            "-c".to_string(),
            "print('hello')".to_string(),
        ];
        let result = format_command_for_log(&args);
        assert!(
            result.contains("print('hello')") || result.contains("print"),
            "got: {result}"
        );
        assert!(!result.contains("********"));
    }

    // ── Tier 1: Pure function tests ──────────────────────────────────

    #[test]
    fn test_days_to_ymd_epoch() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn test_days_to_ymd_known_date() {
        // 2024-02-29 is a leap day. Days since epoch = 19782
        assert_eq!(days_to_ymd(19782), (2024, 2, 29));
    }

    #[test]
    fn test_days_to_ymd_end_of_year() {
        // 2023-12-31 = day 19722
        assert_eq!(days_to_ymd(19722), (2023, 12, 31));
    }

    #[test]
    fn test_days_to_ymd_y2k() {
        // 2000-01-01 = day 10957
        assert_eq!(days_to_ymd(10957), (2000, 1, 1));
    }

    #[test]
    fn test_write_cancel_info_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        write_cancel_info(dir.path(), Duration::from_secs(30));
        let path = dir.path().join("cancel_info.json");
        assert!(path.exists());
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let ts = content["NotifyEnd"].as_str().unwrap();
        assert!(ts.ends_with('Z'), "Expected UTC timestamp, got: {ts}");
        assert!(ts.contains('T'), "Expected ISO 8601, got: {ts}");
    }

    #[test]
    fn test_process_line_plain_text() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut filter = ActionFilter::new("test", true, false);
        let mut saw_fail = false;
        let (display, pass_through) =
            process_line("hello world", &mut filter, "test", &tx, &mut saw_fail);
        assert!(pass_through);
        assert_eq!(display, "hello world");
        assert!(!saw_fail);
    }

    #[test]
    fn test_process_line_progress() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut filter = ActionFilter::new("test", true, false);
        let mut saw_fail = false;
        let (_display, _pass_through) = process_line(
            "openjd_progress: 0.5",
            &mut filter,
            "test",
            &tx,
            &mut saw_fail,
        );
        assert!(!saw_fail);
        match rx.try_recv().unwrap() {
            ActionMessage::Progress(v) => assert!((v - 0.5).abs() < f64::EPSILON),
            other => panic!("Expected Progress, got: {other:?}"),
        }
    }

    #[test]
    fn test_process_line_status() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut filter = ActionFilter::new("test", true, false);
        let mut saw_fail = false;
        process_line(
            "openjd_status: rendering frame 42",
            &mut filter,
            "test",
            &tx,
            &mut saw_fail,
        );
        assert!(!saw_fail);
        match rx.try_recv().unwrap() {
            ActionMessage::Status(s) => assert_eq!(s, "rendering frame 42"),
            other => panic!("Expected Status, got: {other:?}"),
        }
    }

    #[test]
    fn test_process_line_fail() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut filter = ActionFilter::new("test", true, false);
        let mut saw_fail = false;
        process_line(
            "openjd_fail: out of memory",
            &mut filter,
            "test",
            &tx,
            &mut saw_fail,
        );
        assert!(saw_fail);
        match rx.try_recv().unwrap() {
            ActionMessage::Fail(s) => assert_eq!(s, "out of memory"),
            other => panic!("Expected Fail, got: {other:?}"),
        }
    }

    #[test]
    fn test_process_line_env() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut filter = ActionFilter::new("test", true, false);
        let mut saw_fail = false;
        process_line(
            "openjd_env: MY_VAR=my_value",
            &mut filter,
            "test",
            &tx,
            &mut saw_fail,
        );
        match rx.try_recv().unwrap() {
            ActionMessage::SetEnv { name, value } => {
                assert_eq!(name, "MY_VAR");
                assert_eq!(value, "my_value");
            }
            other => panic!("Expected SetEnv, got: {other:?}"),
        }
    }

    #[test]
    fn test_process_line_unset_env() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut filter = ActionFilter::new("test", true, false);
        let mut saw_fail = false;
        process_line(
            "openjd_unset_env: MY_VAR",
            &mut filter,
            "test",
            &tx,
            &mut saw_fail,
        );
        match rx.try_recv().unwrap() {
            ActionMessage::UnsetEnv { name } => assert_eq!(name, "MY_VAR"),
            other => panic!("Expected UnsetEnv, got: {other:?}"),
        }
    }

    #[test]
    fn test_process_line_redacted_env() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut filter = ActionFilter::new("test", true, false);
        let mut saw_fail = false;
        process_line(
            "openjd_redacted_env: SECRET=hunter2",
            &mut filter,
            "test",
            &tx,
            &mut saw_fail,
        );
        match rx.try_recv().unwrap() {
            ActionMessage::RedactedEnv { name, value } => {
                assert_eq!(name, "SECRET");
                assert_eq!(value, "hunter2");
            }
            other => panic!("Expected RedactedEnv, got: {other:?}"),
        }
    }

    // ── Tier 2: Same-user integration tests ──────────────────────────

    #[cfg(unix)]
    fn run_simple(args: Vec<String>) -> (SubprocessResult, Vec<ActionMessage>) {
        run_with_config(SubprocessConfig {
            args,
            env_vars: HashMap::new(),
            working_dir: None,
            timeout: None,
            user: None,
            cancel_method: CancelMethod::Terminate,
            cancel_request_rx: None,
            debug_collect_stdout: true,
        })
    }

    #[cfg(unix)]
    fn run_with_config(config: SubprocessConfig) -> (SubprocessResult, Vec<ActionMessage>) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();
            let mut filter = ActionFilter::new("test", true, false);
            let token = CancellationToken::new();
            let result = run_subprocess(config, &mut filter, "test", msg_tx, token)
                .await
                .unwrap();
            let mut msgs = Vec::new();
            while let Ok(m) = msg_rx.try_recv() {
                msgs.push(m);
            }
            (result, msgs)
        })
    }

    #[cfg(unix)]
    #[test]
    fn test_run_subprocess_success() {
        let (r, _) = run_simple(vec!["echo".into(), "hello".into()]);
        assert_eq!(r.state, ActionState::Success);
        assert_eq!(r.exit_code, Some(0));
        assert!(r.stdout.contains("hello"), "stdout: {}", r.stdout);
    }

    #[cfg(unix)]
    #[test]
    fn test_run_subprocess_failure_exit_code() {
        let (r, _) = run_simple(vec!["sh".into(), "-c".into(), "exit 42".into()]);
        assert_eq!(r.state, ActionState::Failed);
        assert_eq!(r.exit_code, Some(42));
    }

    #[cfg(unix)]
    #[test]
    fn test_run_subprocess_command_not_found() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = rt.block_on(async {
            let (msg_tx, _) = mpsc::unbounded_channel();
            let mut filter = ActionFilter::new("test", true, false);
            let token = CancellationToken::new();
            let config = SubprocessConfig {
                args: vec!["/nonexistent/binary_xyz".into()],
                env_vars: HashMap::new(),
                working_dir: None,
                timeout: None,
                user: None,
                cancel_method: CancelMethod::Terminate,
                cancel_request_rx: None,
                debug_collect_stdout: false,
            };
            run_subprocess(config, &mut filter, "test", msg_tx, token).await
        });
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("/nonexistent/binary_xyz"), "error: {msg}");
    }

    #[cfg(unix)]
    #[test]
    fn test_run_subprocess_empty_args() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = rt.block_on(async {
            let (msg_tx, _) = mpsc::unbounded_channel();
            let mut filter = ActionFilter::new("test", true, false);
            let token = CancellationToken::new();
            let config = SubprocessConfig {
                args: vec![],
                env_vars: HashMap::new(),
                working_dir: None,
                timeout: None,
                user: None,
                cancel_method: CancelMethod::Terminate,
                cancel_request_rx: None,
                debug_collect_stdout: false,
            };
            run_subprocess(config, &mut filter, "test", msg_tx, token).await
        });
        assert!(err.is_err());
        assert!(
            err.unwrap_err().to_string().contains("No command"),
            "expected empty args error"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_run_subprocess_timeout() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (r, _) = rt.block_on(async {
            let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();
            let mut filter = ActionFilter::new("test", true, false);
            let token = CancellationToken::new();
            let config = SubprocessConfig {
                args: vec!["sleep".into(), "30".into()],
                env_vars: HashMap::new(),
                working_dir: None,
                timeout: Some(Duration::from_millis(500)),
                user: None,
                cancel_method: CancelMethod::Terminate,
                cancel_request_rx: None,
                debug_collect_stdout: false,
            };
            let r = run_subprocess(config, &mut filter, "test", msg_tx, token)
                .await
                .unwrap();
            let mut msgs = Vec::new();
            while let Ok(m) = msg_rx.try_recv() {
                msgs.push(m);
            }
            (r, msgs)
        });
        assert_eq!(r.state, ActionState::Timeout);
    }

    #[cfg(unix)]
    #[test]
    fn test_run_subprocess_timeout_drains_stdout() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (r, _) = rt.block_on(async {
            let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();
            let mut filter = ActionFilter::new("test", true, false);
            let token = CancellationToken::new();
            let config = SubprocessConfig {
                args: vec![
                    "sh".into(),
                    "-c".into(),
                    "echo before_timeout; sleep 30".into(),
                ],
                env_vars: HashMap::new(),
                working_dir: None,
                timeout: Some(Duration::from_millis(500)),
                user: None,
                cancel_method: CancelMethod::Terminate,
                cancel_request_rx: None,
                debug_collect_stdout: true,
            };
            let r = run_subprocess(config, &mut filter, "test", msg_tx, token)
                .await
                .unwrap();
            let mut msgs = Vec::new();
            while let Ok(m) = msg_rx.try_recv() {
                msgs.push(m);
            }
            (r, msgs)
        });
        assert_eq!(r.state, ActionState::Timeout);
        assert!(
            r.stdout.contains("before_timeout"),
            "output before timeout should be captured: {:?}",
            r.stdout
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_run_subprocess_env_vars() {
        let mut env = HashMap::new();
        env.insert("OPENJD_TEST_VAR".into(), Some("test_value_42".into()));
        let (r, _) = run_with_config(SubprocessConfig {
            args: vec!["sh".into(), "-c".into(), "echo $OPENJD_TEST_VAR".into()],
            env_vars: env,
            working_dir: None,
            timeout: None,
            user: None,
            cancel_method: CancelMethod::Terminate,
            cancel_request_rx: None,
            debug_collect_stdout: true,
        });
        assert_eq!(r.state, ActionState::Success);
        assert!(r.stdout.contains("test_value_42"), "stdout: {}", r.stdout);
    }

    #[cfg(unix)]
    #[test]
    fn test_run_subprocess_env_var_unset() {
        // Set a var then unset it — should not appear in child
        std::env::set_var("OPENJD_UNSET_TEST", "should_be_gone");
        let mut env = HashMap::new();
        env.insert("OPENJD_UNSET_TEST".into(), None);
        let (r, _) = run_with_config(SubprocessConfig {
            args: vec![
                "sh".into(),
                "-c".into(),
                "echo VAL=${OPENJD_UNSET_TEST:-UNSET}".into(),
            ],
            env_vars: env,
            working_dir: None,
            timeout: None,
            user: None,
            cancel_method: CancelMethod::Terminate,
            cancel_request_rx: None,
            debug_collect_stdout: true,
        });
        assert_eq!(r.state, ActionState::Success);
        assert!(r.stdout.contains("VAL=UNSET"), "stdout: {}", r.stdout);
    }

    #[cfg(unix)]
    #[test]
    fn test_run_subprocess_working_dir() {
        let dir = tempfile::tempdir().unwrap();
        let (r, _) = run_with_config(SubprocessConfig {
            args: vec!["pwd".into()],
            env_vars: HashMap::new(),
            working_dir: Some(dir.path().to_path_buf()),
            timeout: None,
            user: None,
            cancel_method: CancelMethod::Terminate,
            cancel_request_rx: None,
            debug_collect_stdout: true,
        });
        assert_eq!(r.state, ActionState::Success);
        // Resolve symlinks for comparison (macOS /tmp -> /private/tmp)
        let expected = dir.path().canonicalize().unwrap();
        let actual = PathBuf::from(r.stdout.trim()).canonicalize().unwrap();
        assert_eq!(actual, expected);
    }

    #[cfg(unix)]
    #[test]
    fn test_run_subprocess_openjd_progress() {
        let (r, msgs) = run_simple(vec![
            "sh".into(),
            "-c".into(),
            "echo 'openjd_progress: 0.75'".into(),
        ]);
        assert_eq!(r.state, ActionState::Success);
        assert!(
            msgs.iter().any(
                |m| matches!(m, ActionMessage::Progress(v) if (*v - 0.75).abs() < f64::EPSILON)
            ),
            "Expected Progress(0.75), got: {msgs:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_run_subprocess_openjd_status() {
        let (r, msgs) = run_simple(vec![
            "sh".into(),
            "-c".into(),
            "echo 'openjd_status: rendering'".into(),
        ]);
        assert_eq!(r.state, ActionState::Success);
        assert!(
            msgs.iter()
                .any(|m| matches!(m, ActionMessage::Status(s) if s == "rendering")),
            "Expected Status(rendering), got: {msgs:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_run_subprocess_openjd_fail_sets_failed() {
        let (r, msgs) = run_simple(vec![
            "sh".into(),
            "-c".into(),
            "echo 'openjd_fail: something broke'".into(),
        ]);
        assert_eq!(
            r.state,
            ActionState::Failed,
            "openjd_fail should cause Failed state even with exit 0"
        );
        assert!(
            msgs.iter()
                .any(|m| matches!(m, ActionMessage::Fail(s) if s == "something broke")),
            "Expected Fail message, got: {msgs:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_run_subprocess_openjd_env() {
        let (r, msgs) = run_simple(vec![
            "sh".into(),
            "-c".into(),
            "echo 'openjd_env: FOO=bar'".into(),
        ]);
        assert_eq!(r.state, ActionState::Success);
        assert!(msgs.iter().any(|m| matches!(m, ActionMessage::SetEnv { name, value } if name == "FOO" && value == "bar")),
            "Expected SetEnv, got: {msgs:?}");
    }

    #[cfg(unix)]
    #[test]
    fn test_run_subprocess_stderr_merged() {
        // stderr should be merged into stdout
        let (r, _) = run_simple(vec![
            "sh".into(),
            "-c".into(),
            "echo stdout_line; echo stderr_line >&2".into(),
        ]);
        assert_eq!(r.state, ActionState::Success);
        assert!(r.stdout.contains("stdout_line"), "stdout: {}", r.stdout);
        assert!(
            r.stdout.contains("stderr_line"),
            "stderr should be merged into stdout: {}",
            r.stdout
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_run_subprocess_multiline_output() {
        let (r, _) = run_simple(vec![
            "sh".into(),
            "-c".into(),
            "echo line1; echo line2; echo line3".into(),
        ]);
        assert_eq!(r.state, ActionState::Success);
        assert!(r.stdout.contains("line1\n"), "stdout: {:?}", r.stdout);
        assert!(r.stdout.contains("line2\n"), "stdout: {:?}", r.stdout);
        assert!(r.stdout.contains("line3\n"), "stdout: {:?}", r.stdout);
    }

    #[cfg(unix)]
    #[test]
    fn test_run_subprocess_debug_collect_stdout_false_by_default() {
        let (r, _) = run_simple(vec!["echo".into(), "hello".into()]);
        // run_simple sets debug_collect_stdout: true, so stdout is captured
        assert!(r.stdout.contains("hello"));

        // With debug_collect_stdout: false (default), stdout should be empty
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let r = rt.block_on(async {
            let (msg_tx, _) = mpsc::unbounded_channel();
            let mut filter = ActionFilter::new("test", true, false);
            let token = CancellationToken::new();
            let config = SubprocessConfig {
                args: vec!["echo".into(), "hello".into()],
                env_vars: HashMap::new(),
                working_dir: None,
                timeout: None,
                user: None,
                cancel_method: CancelMethod::Terminate,
                cancel_request_rx: None,
                debug_collect_stdout: false,
            };
            run_subprocess(config, &mut filter, "test", msg_tx, token)
                .await
                .unwrap()
        });
        assert_eq!(r.state, ActionState::Success);
        assert!(
            r.stdout.is_empty(),
            "stdout should be empty when debug_collect_stdout is false: {:?}",
            r.stdout
        );
    }

    #[test]
    fn test_truncate_line_multibyte_boundary() {
        // '€' is 3 bytes in UTF-8. LOG_LINE_MAX_LENGTH (65536) % 3 == 1,
        // so the byte boundary falls inside a multi-byte character.
        let s = "€".repeat(LOG_LINE_MAX_LENGTH); // 3 * LOG_LINE_MAX_LENGTH bytes
        let truncated = truncate_line(&s);
        assert!(truncated.len() <= LOG_LINE_MAX_LENGTH);
        // Must be valid UTF-8 (the fact that we can call .chars() without panic proves it)
        assert!(truncated.chars().count() > 0);
    }

    #[test]
    fn test_truncate_line_short_line_unchanged() {
        let s = "hello";
        assert_eq!(truncate_line(s), "hello");
    }

    #[cfg(unix)]
    #[test]
    fn test_run_subprocess_invalid_utf8_continues() {
        // printf outputs raw bytes: valid line, then 0xFF (invalid UTF-8), then another valid line.
        // All lines including those after invalid bytes must be captured.
        let (r, _) = run_simple(vec![
            "sh".into(),
            "-c".into(),
            r#"echo before; printf '\xff\n'; echo after"#.into(),
        ]);
        assert_eq!(r.state, ActionState::Success);
        assert!(
            r.stdout.contains("before"),
            "line before invalid UTF-8 should be captured: {:?}",
            r.stdout
        );
        assert!(
            r.stdout.contains("after"),
            "line after invalid UTF-8 should be captured: {:?}",
            r.stdout
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_run_subprocess_progress_error_in_stdout() {
        let (r, _) = run_simple(vec![
            "sh".into(),
            "-c".into(),
            "echo 'openjd_progress: 200.0'".into(),
        ]);
        assert!(
            r.stdout.contains("ERROR"),
            "out-of-range progress error should appear in stdout: {:?}",
            r.stdout
        );
    }

    /// When a process exits with non-zero and the cancel token has been
    /// cancelled, the result should be `Canceled` not `Failed`.
    ///
    /// This covers the pyo3 binding's cancel path where the cross-user helper
    /// kills the process (non-zero exit) while the token is cancelled, but the
    /// select loop's cancel branch may not have fired (so `cancel_requested`
    /// could be false). The `is_cancelled()` check in state determination
    /// ensures the correct result regardless of select ordering.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_cancel_token_set_but_process_killed_externally() {
        use tokio_util::sync::CancellationToken;

        let token = CancellationToken::new();
        let (msg_tx, _msg_rx) = tokio::sync::mpsc::unbounded_channel();

        // Process that exits immediately with non-zero
        let config = SubprocessConfig {
            args: vec!["sh".into(), "-c".into(), "exit 42".into()],
            env_vars: HashMap::new(),
            working_dir: None,
            timeout: None,
            user: None,
            cancel_method: CancelMethod::Terminate,
            cancel_request_rx: None,
            debug_collect_stdout: false,
        };

        // Cancel from OS thread — simulates the pyo3 binding's cancel path
        let token_clone = token.clone();
        std::thread::spawn(move || {
            token_clone.cancel();
        });

        // Small yield to let the OS thread run
        tokio::time::sleep(Duration::from_millis(1)).await;

        let mut filter = crate::action_filter::ActionFilter::new("test", true, false);
        let result = run_subprocess(config, &mut filter, "test", msg_tx, token)
            .await
            .unwrap();

        // The token is cancelled and the process exited non-zero.
        // The result must be Canceled, not Failed.
        assert_eq!(
            result.state,
            ActionState::Canceled,
            "Non-zero exit with cancelled token should be Canceled, not {:?}",
            result.state
        );
    }
}
