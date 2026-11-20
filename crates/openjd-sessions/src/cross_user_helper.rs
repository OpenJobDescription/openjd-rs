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

use std::io::Read;
use std::path::Path;

use crate::action::{ActionMessage, ActionState};
use crate::error::SessionError;
use crate::logging::LogContent;

/// Maximum bytes for a single JSON response line from the helper's stdout.
/// Matches the helper binary's own `MAX_LINE_LENGTH` (128 KB).
pub(crate) const MAX_RESPONSE_LINE_LENGTH: usize = 128 * 1024;
use crate::session_log;
use crate::session_user::SessionUser;

/// Length of the shared auth token in characters.
///
/// Matches the helper's `AUTH_TOKEN_LEN` constant. The helper rejects
/// `--auth-token` values of any other length.
pub(crate) const AUTH_TOKEN_LEN: usize = 22;

/// 64-character URL-safe alphabet used to render the auth token.
///
/// Chosen so that `byte & 0x3F` is a uniform index with no rejection
/// sampling. The character set avoids `+` / `/` / `=` / whitespace so the
/// token is safe in argv, JSON, filenames, and log lines without quoting.
const AUTH_TOKEN_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Generate a fresh per-helper auth token: [`AUTH_TOKEN_LEN`] ASCII characters
/// drawn from [`AUTH_TOKEN_ALPHABET`]. Uses the OS CSPRNG via `getrandom::fill`.
///
/// Each character contributes 6 bits of entropy (64-char alphabet), so
/// 22 characters ≥ 128 bits of entropy.
pub(crate) fn generate_auth_token() -> Result<String, SessionError> {
    let mut raw = [0u8; AUTH_TOKEN_LEN];
    getrandom::fill(&mut raw).map_err(|e| {
        SessionError::HelperCommunication(format!("Failed to generate auth token: {e}"))
    })?;
    let mut out = String::with_capacity(AUTH_TOKEN_LEN);
    for b in raw {
        out.push(AUTH_TOKEN_ALPHABET[(b & 0x3F) as usize] as char);
    }
    Ok(out)
}

/// Return `cmd` extended with `"token": <auth_token>`.
///
/// `cmd` must be a JSON object, since every helper command is an object.
/// If the caller's map already contains a `"token"` entry, it is overwritten
/// — `augment_with_token` is the sole owner of that field in outgoing
/// commands.
fn augment_with_token(
    cmd: &serde_json::Value,
    auth_token: &str,
) -> Result<serde_json::Value, SessionError> {
    let serde_json::Value::Object(map) = cmd else {
        return Err(SessionError::HelperCommunication(format!(
            "helper command must be a JSON object, got: {cmd}"
        )));
    };
    let mut with_token = map.clone();
    with_token.insert(
        "token".to_string(),
        serde_json::Value::String(auth_token.to_string()),
    );
    Ok(serde_json::Value::Object(with_token))
}

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
                match Read::take(&mut reader, MAX_RESPONSE_LINE_LENGTH as u64).read_line(&mut line)
                {
                    Ok(0) => break,
                    Ok(n) => {
                        if n == MAX_RESPONSE_LINE_LENGTH && !line.ends_with('\n') {
                            // Line exceeded limit — discard remainder and report error
                            let mut discard = Vec::new();
                            let _ = reader.read_until(b'\n', &mut discard);
                            let result = Err(SessionError::HelperCommunication(format!(
                                "response line exceeds {MAX_RESPONSE_LINE_LENGTH} byte limit"
                            )));
                            if tx.send(result).is_err() {
                                break;
                            }
                            continue;
                        }
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
                match Read::take(&mut reader, MAX_RESPONSE_LINE_LENGTH as u64).read_line(&mut line)
                {
                    Ok(0) => break,
                    Ok(n) => {
                        if n == MAX_RESPONSE_LINE_LENGTH && !line.ends_with('\n') {
                            // Line exceeded limit — discard remainder and report error
                            let mut discard = Vec::new();
                            let _ = reader.read_until(b'\n', &mut discard);
                            let result = Err(SessionError::HelperCommunication(format!(
                                "response line exceeds {MAX_RESPONSE_LINE_LENGTH} byte limit"
                            )));
                            if tx.send(result).is_err() {
                                break;
                            }
                            continue;
                        }
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
/// On POSIX: launched via `sudo -u <user> -i <helper_path> --auth-token <TOKEN>`.
/// Communicates over newline-delimited JSON on stdin/stdout. Every request
/// carries the shared `auth_token` string; the helper rejects anything else.
#[cfg(unix)]
pub(crate) struct CrossUserHelper {
    child: std::process::Child,
    stdin: std::io::BufWriter<std::process::ChildStdin>,
    pub(crate) async_reader: UnixAsyncHelperReader,
    /// 22-char auth token shared with the helper. Included in every command
    /// sent over `stdin` and over the dup'd `cancel_writer`.
    auth_token: String,
}

/// Windows variant: the child is a raw process handle from `CreateProcessAsUserW`,
/// not a `std::process::Child`. We wrap stdin/stdout from the pipe handles.
#[cfg(windows)]
pub(crate) struct CrossUserHelperWin {
    process_handle: windows::Win32::Foundation::HANDLE,
    stdin: std::io::BufWriter<std::fs::File>,
    pub(crate) async_reader: WindowsAsyncHelperReader,
    /// 22-char auth token shared with the helper. Included in every command
    /// sent over `stdin` and over the dup'd `cancel_writer`.
    auth_token: String,
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
    /// Generates a fresh per-helper auth token, passes it to the helper as
    /// `--auth-token <TOKEN>`, and stores it on the returned struct so every
    /// subsequent command can echo it back.
    ///
    /// Returns `(helper, cancel_writer)` where `cancel_writer` is a dup'd copy
    /// of the helper's stdin fd. This allows sending cancel commands even while
    /// the helper struct is moved to a runner during action execution.
    #[cfg(unix)]
    pub fn spawn(
        helper_path: &Path,
        user: &dyn SessionUser,
    ) -> Result<(Self, std::fs::File), SessionError> {
        let auth_token = generate_auth_token()?;
        let mut child = std::process::Command::new("sudo")
            .args([
                "-u",
                user.user(),
                "-i",
                &helper_path.to_string_lossy(),
                "--auth-token",
                &auth_token,
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|source| SessionError::SubprocessStart {
                // Don't include the token in error messages.
                command: format!(
                    "sudo -u {} -i {} --auth-token <redacted>",
                    user.user(),
                    helper_path.display()
                ),
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
                auth_token,
            },
            cancel_writer,
        ))
    }

    /// The shared auth token. Callers that write directly to the helper's
    /// stdin (like the cancel_writer path) must include this in every request.
    pub fn auth_token(&self) -> &str {
        &self.auth_token
    }

    /// Send a JSON command to the helper, injecting the shared auth token.
    ///
    /// `cmd` must be a JSON object; the helper's protocol requires every
    /// command to be an object. The token is inserted as `"token"` before
    /// the line is written.
    pub fn send_command(&mut self, cmd: &serde_json::Value) -> Result<(), SessionError> {
        use std::io::Write;
        let payload = augment_with_token(cmd, &self.auth_token)?;
        serde_json::to_writer(&mut self.stdin, &payload).map_err(|e| {
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

    /// Send a tokenized shutdown command and wait for the child to exit.
    pub fn shutdown(&mut self) {
        let _ = self.send_command(&serde_json::json!({"shutdown": true}));
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
    /// Generates a fresh per-helper auth token, passes it to the helper as
    /// `--auth-token <TOKEN>`, and stores it on the returned struct so every
    /// subsequent command can echo it back.
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

        let auth_token = generate_auth_token()?;

        let spawned = crate::win32::spawn_as_user_with_stdin(
            &[
                helper_path.to_string_lossy().to_string(),
                "--auth-token".to_string(),
                auth_token.clone(),
            ],
            &HashMap::new(),
            None, // working_dir — helper doesn't need one
            wu.password(),
            wu.user(),
            wu.logon_token(),
        )
        .map_err(|e| SessionError::SubprocessStart {
            // Don't include the token in error messages.
            command: format!(
                "spawn_as_user_with_stdin {} --auth-token <redacted>",
                helper_path.display()
            ),
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
                auth_token,
            },
            cancel_writer,
        ))
    }

    /// The shared auth token. Callers that write directly to the helper's
    /// stdin (like the cancel_writer path) must include this in every request.
    pub fn auth_token(&self) -> &str {
        &self.auth_token
    }

    /// Send a JSON command to the helper, injecting the shared auth token.
    pub fn send_command(&mut self, cmd: &serde_json::Value) -> Result<(), SessionError> {
        use std::io::Write;
        let payload = augment_with_token(cmd, &self.auth_token)?;
        serde_json::to_writer(&mut self.stdin, &payload).map_err(|e| {
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

    /// Send a tokenized shutdown command and wait for the process to exit.
    pub fn shutdown(&mut self) {
        let _ = self.send_command(&serde_json::json!({"shutdown": true}));
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
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_via_helper(
    helper: &mut dyn AsyncHelper,
    config: &crate::subprocess::SubprocessConfig,
    filter: &mut crate::action_filter::ActionFilter,
    session_id: &str,
    message_tx: tokio::sync::mpsc::UnboundedSender<ActionMessage>,
    run_gate: Option<&HelperRunGate>,
    cancel_token: &tokio_util::sync::CancellationToken,
) -> Result<crate::subprocess::SubprocessResult, SessionError> {
    // Windows note: executable resolution happens inside the helper
    // (helper/src/runner_win.rs::locate_executable), which runs as the
    // target user — it can resolve executables in directories only that
    // user can read, and its PATH fallback is the target user's own
    // environment (matching what the workload actually runs under).
    // Resolution failures come back over the protocol as an error response
    // and are mapped to SessionError::SubprocessStart below.
    let args = &config.args;

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
        "command": args[0],
        "args": &args[1..],
        "env": env,
        "cwd": config.working_dir,
    });

    // Write Run through the action's gate, so a `cancel_action` /
    // `SessionCancelHandle` cancel that arrived before it is written to the
    // helper right after it. See [`HelperRunGate`]. Every cancel command
    // this function writes (the held one here, and the timeout and
    // bare-token ones below) goes through `send_command`, the helper's own
    // stdin, so none depends on a dup of the cancel pipe.
    // `pending_cancel_sent` records that the held one was written.
    let mut pending_cancel_sent = false;
    match run_gate {
        Some(gate) => gate.start_run(
            helper,
            |helper| helper.send_command(&cmd),
            |helper, time_limit| {
                // Run is already written, so a failure here doesn't fail the
                // action: the run loop goes on reading the helper's
                // responses, and a broken pipe surfaces there.
                if send_cancel_command(helper, &config.cancel_method, time_limit, session_id) {
                    pending_cancel_sent = true;
                }
                Ok(())
            },
        )?,
        None => helper.send_command(&cmd)?,
    }

    // Log the actual command (not the helper protocol)
    session_log!(
        info,
        session_id,
        LogContent::FILE_PATH | LogContent::PROCESS_CONTROL,
        "Running command {}",
        crate::subprocess::format_command_for_log(args)
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
    // Latches so each one-shot cancel source fires once. `CancellationToken`'s
    // `cancelled()` future stays ready forever after the token is canceled, so
    // without `token_seen` its `select!` branch would spin.
    let mut token_seen = false;
    // Whether a cancel command has already reached the helper over the pipe —
    // from the timeout branch, the external-token branch, the pending cancel
    // written after Run above, or `cancel_action` / `SessionCancelHandle`
    // (which write it directly once Run is written). Prevents a second,
    // possibly conflicting, cancel command.
    let mut cancel_delivered = pending_cancel_sent;

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

                    // A cancel reaches a cross-user action over either the
                    // watch channel (`cancel_action` / `SessionCancelHandle`)
                    // or the session-wide/per-action `CancellationToken` (an
                    // external `SessionConfig.cancel_token.cancel()`). Mirror
                    // the same-user loop and classify Canceled on either.
                    // `pending_cancel_sent` makes a held cancel count on its
                    // own. The canceller sends the watch value before its
                    // gate call, so this is a guard for a canceller that
                    // doesn't, not a case the session's cancellers produce.
                    let canceled = pending_cancel_sent
                        || config
                            .cancel_request_rx
                            .as_ref()
                            .is_some_and(|rx| rx.has_changed().unwrap_or(false))
                        || cancel_token.is_cancelled();

                    // Timeout before Canceled, matching the same-user loop
                    // (`subprocess.rs`): a cancel that lands while a
                    // timed-out action is still exiting doesn't relabel it.
                    let state = if timed_out {
                        ActionState::Timeout
                    } else if canceled {
                        ActionState::Canceled
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
                        command: args[0].clone(),
                        source: std::io::Error::other(msg.to_string()),
                    });
                }

                return Err(SessionError::HelperCommunication(format!(
                    "Unexpected helper response: {}",
                    resp
                )));
            }
            _ = cancel_token.cancelled(), if !token_seen => {
                token_seen = true;
                // `cancel_action` / `SessionCancelHandle` fire the watch
                // channel (and write the pipe command themselves) before
                // canceling the token, so a watch that has already changed
                // means the command is on its way — don't send a second,
                // possibly conflicting, one. A bare external
                // `SessionConfig.cancel_token.cancel()` fires only the token:
                // this is the branch that signals the helper for it.
                let watch_fired = config
                    .cancel_request_rx
                    .as_ref()
                    .is_some_and(|rx| rx.has_changed().unwrap_or(false));
                if !cancel_delivered && !watch_fired {
                    send_cancel_command(helper, &config.cancel_method, None, session_id);
                }
                cancel_delivered = true;
            }
            // After a cancel has been delivered the timeout can't fire, as in
            // the same-user loop (`if !cancel_requested && !timed_out`), so a
            // cancel whose grace period outlasts the timeout stays Canceled
            // and the helper's escalation deadline isn't reset.
            _ = &mut timeout_fut, if !timed_out && !cancel_delivered => {
                timed_out = true;
                cancel_delivered = true;
                send_cancel_command(helper, &config.cancel_method, None, session_id);
            }
        }
    }
}

/// Build one cancel command for the cross-user helper, as a single JSON
/// object without the trailing newline.
///
/// This is the only place the session side writes a helper cancel command.
/// [`run_via_helper`]'s timeout and bare-token branches and its write of a
/// cancel [`HelperRunGate`] held until Run (all through
/// [`send_cancel_command`]), and `Session::cancel_action` /
/// `SessionCancelHandle::cancel`
/// (through `session.rs`'s `send_helper_cancel_command`) all call it, so one
/// action gets one answer for the same `time_limit` whichever way the cancel
/// arrives.
///
/// The wire method is chosen as the same-user loop chooses its signal
/// (`subprocess.rs`):
///
/// - `Terminate` gives `TERMINATE`, whatever the `time_limit`.
/// - `NotifyThenTerminate { terminate_delay }` uses the grace
///   `time_limit.map_or(terminate_delay, |l| l.min(terminate_delay))`.
///   The wire carries whole seconds (`notifyPeriodInSeconds` is a `u64`),
///   so the grace is truncated to whole seconds, never rounded up past the
///   caller's bound. A grace under one second, including `Some(0)`, gives
///   `TERMINATE`; otherwise it gives `NOTIFY_THEN_TERMINATE` with that many
///   seconds. The same-user loop keeps the sub-second grace (it sends the
///   notify signal and kills after the exact `Duration`), which the wire
///   can't express.
///
/// `token: None` omits the `"token"` field. [`send_cancel_command`] passes
/// `None` because `send_command` adds the token; otherwise only the
/// test-only injection path in `session.rs` (a cancel writer with no
/// helper) has no token. The token alphabet excludes `"` and `\`, so it is
/// embedded without escaping.
pub(crate) fn helper_cancel_line(
    token: Option<&str>,
    method: &crate::runner::CancelMethod,
    time_limit: Option<std::time::Duration>,
) -> String {
    let notify_period_secs = match method {
        crate::runner::CancelMethod::Terminate => 0,
        crate::runner::CancelMethod::NotifyThenTerminate { terminate_delay } => time_limit
            .map_or(*terminate_delay, |limit| limit.min(*terminate_delay))
            .as_secs(),
    };
    let token_field = match token {
        Some(t) => format!(r#""token":"{t}","#),
        None => String::new(),
    };
    if notify_period_secs == 0 {
        format!(r#"{{{token_field}"cancel":"TERMINATE"}}"#)
    } else {
        format!(
            r#"{{{token_field}"cancel":"NOTIFY_THEN_TERMINATE","notifyPeriodInSeconds":{notify_period_secs}}}"#
        )
    }
}

/// Send one cancel command, built by [`helper_cancel_line`] from the
/// action's `cancel_method` and `time_limit`, through the helper's own
/// stdin (`send_command`, which adds the auth token). [`run_via_helper`]
/// uses it for the cancel [`HelperRunGate`] held until Run and for its
/// timeout and bare-token branches. Returns whether the command was
/// written.
///
/// A failed write is logged and doesn't fail the action: Run is already
/// written, so the run loop goes on reading the helper's responses, and a
/// helper that has gone away shows there as closed stdout.
///
/// A bare token cancel and a timeout carry no `time_limit` (the same-user
/// loop reads `None` from the watch channel too), so they get the action's
/// own method. The same-user loop kills at once on a timeout instead; that
/// difference is known and not changed here.
fn send_cancel_command(
    helper: &mut dyn AsyncHelper,
    cancel_method: &crate::runner::CancelMethod,
    time_limit: Option<std::time::Duration>,
    session_id: &str,
) -> bool {
    let line = helper_cancel_line(None, cancel_method, time_limit);
    let sent = serde_json::from_str::<serde_json::Value>(&line)
        .map_err(|e| SessionError::HelperCommunication(e.to_string()))
        .and_then(|cancel| helper.send_command(&cancel));
    match sent {
        Ok(()) => true,
        Err(e) => {
            session_log!(
                warn,
                session_id,
                LogContent::PROCESS_CONTROL,
                "Failed to send a cancel command to the cross-user helper: {e}"
            );
            false
        }
    }
}

/// Orders a session-side cancel against one action's Run command on the
/// cross-user helper.
///
/// The helper applies a cancel only while it runs a command. With none
/// running, the Unix helper reads and drops it (`helper/src/main.rs`,
/// `Command::Cancel { .. } => {}`); the Windows helper queues it for the
/// next Run.
/// `Session::cancel_action` and `SessionCancelHandle::cancel` find an action
/// as soon as it is registered (`CancelFields::set_action`), which is before
/// [`run_via_helper`] writes its Run command. A cancel written in that
/// window used to be lost on Unix, while
/// [`run_via_helper`]'s token branch saw the watch channel had fired and
/// sent nothing, so the action ran to completion.
///
/// Each action gets a new gate when it is registered. Both sides take its
/// lock:
///
/// - The session side calls [`deliver_cancel`](Self::deliver_cancel). Once
///   Run is written, it writes the cancel command at once. Before that, it
///   holds the cancel's `time_limit` in the gate and writes nothing. A later
///   cancel before Run replaces it, as the same-user loop reads only the
///   watch channel's latest value.
/// - [`run_via_helper`] calls [`start_run`](Self::start_run), which writes
///   Run and then the held cancel, if any, before releasing the lock.
///
/// So in pipe order every session-side cancel follows Run, and it is written
/// once: by the canceller or by [`run_via_helper`], never both. As on the
/// same-user path, which spawns the process and then applies the cancel, the
/// action starts and is then cancelled by its own method. The lock also
/// stops a cancel line being written in the middle of a Run line longer
/// than the pipe's atomic write size.
///
/// The gate is never closed after Run: a cancel that lands after the action
/// exits is written to a helper with no command running, as before.
pub(crate) struct HelperRunGate {
    state: std::sync::Mutex<RunGateState>,
    /// Test-only: called by [`start_run`](Self::start_run) before it takes
    /// the lock, so a test can cancel deterministically in the window
    /// between registration and Run.
    #[cfg(any(test, feature = "test-utils"))]
    before_run: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

enum RunGateState {
    /// Run is not written yet. `Some(time_limit)` is a cancel held for it.
    BeforeRun(Option<Option<std::time::Duration>>),
    /// Run is written; cancels go to the helper at once.
    RunWritten,
}

impl HelperRunGate {
    /// A gate for a newly registered action, before its Run command.
    pub(crate) fn new() -> Self {
        Self {
            state: std::sync::Mutex::new(RunGateState::BeforeRun(None)),
            #[cfg(any(test, feature = "test-utils"))]
            before_run: None,
        }
    }

    /// Like [`new`](Self::new), with a hook that
    /// [`start_run`](Self::start_run) calls before it takes the lock.
    #[cfg(any(test, feature = "test-utils"))]
    pub(crate) fn with_before_run_hook(
        hook: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        Self {
            before_run: hook,
            ..Self::new()
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RunGateState> {
        // Plain data with no invariant a panic could break mid-update.
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Session side: deliver a cancel with `time_limit`. `write` writes the
    /// cancel command; it runs under the lock, and only if Run is written.
    /// Otherwise the cancel is held for [`start_run`](Self::start_run).
    pub(crate) fn deliver_cancel(
        &self,
        time_limit: Option<std::time::Duration>,
        write: impl FnOnce(),
    ) {
        let mut state = self.lock();
        match &mut *state {
            RunGateState::BeforeRun(held) => *held = Some(time_limit),
            RunGateState::RunWritten => write(),
        }
    }

    /// [`run_via_helper`] side: write the Run command with `write_run`, then,
    /// if a cancel was held, write it with `write_cancel(time_limit)`, both
    /// under the lock and both given `ctx` (the helper). If `write_run`
    /// fails, the gate stays before Run and any held cancel is kept.
    pub(crate) fn start_run<C: ?Sized, E>(
        &self,
        ctx: &mut C,
        write_run: impl FnOnce(&mut C) -> Result<(), E>,
        write_cancel: impl FnOnce(&mut C, Option<std::time::Duration>) -> Result<(), E>,
    ) -> Result<(), E> {
        #[cfg(any(test, feature = "test-utils"))]
        if let Some(hook) = &self.before_run {
            hook();
        }
        let mut state = self.lock();
        write_run(ctx)?;
        let held = match std::mem::replace(&mut *state, RunGateState::RunWritten) {
            RunGateState::BeforeRun(held) => held,
            RunGateState::RunWritten => None,
        };
        match held {
            Some(time_limit) => write_cancel(ctx, time_limit),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod run_gate_tests {
    use super::HelperRunGate;
    use std::cell::RefCell;
    use std::time::Duration;

    /// Records the order of writes the gate makes.
    fn run(gate: &HelperRunGate, log: &RefCell<Vec<String>>) {
        gate.start_run::<(), ()>(
            &mut (),
            |_| {
                log.borrow_mut().push("run".into());
                Ok(())
            },
            |_, limit| {
                log.borrow_mut().push(format!("held cancel {limit:?}"));
                Ok(())
            },
        )
        .unwrap();
    }

    fn cancel(gate: &HelperRunGate, limit: Option<Duration>, log: &RefCell<Vec<String>>) {
        gate.deliver_cancel(limit, || log.borrow_mut().push(format!("cancel {limit:?}")));
    }

    #[test]
    fn cancel_before_run_is_written_once_right_after_run() {
        let log = RefCell::new(Vec::new());
        let gate = HelperRunGate::new();
        cancel(&gate, Some(Duration::from_secs(3)), &log);
        assert!(log.borrow().is_empty(), "nothing is written before Run");
        run(&gate, &log);
        assert_eq!(*log.borrow(), ["run", "held cancel Some(3s)"]);
    }

    #[test]
    fn latest_cancel_before_run_wins() {
        let log = RefCell::new(Vec::new());
        let gate = HelperRunGate::new();
        cancel(&gate, Some(Duration::from_secs(3)), &log);
        cancel(&gate, None, &log);
        run(&gate, &log);
        assert_eq!(*log.borrow(), ["run", "held cancel None"]);
    }

    #[test]
    fn cancel_after_run_is_written_by_the_canceller() {
        let log = RefCell::new(Vec::new());
        let gate = HelperRunGate::new();
        run(&gate, &log);
        cancel(&gate, Some(Duration::ZERO), &log);
        assert_eq!(*log.borrow(), ["run", "cancel Some(0ns)"]);
    }

    #[test]
    fn no_cancel_writes_only_run() {
        let log = RefCell::new(Vec::new());
        let gate = HelperRunGate::new();
        run(&gate, &log);
        assert_eq!(*log.borrow(), ["run"]);
    }

    #[test]
    fn failed_run_write_keeps_the_held_cancel() {
        let log = RefCell::new(Vec::new());
        let gate = HelperRunGate::new();
        cancel(&gate, None, &log);
        assert_eq!(
            gate.start_run(
                &mut (),
                |_| Err("broken pipe"),
                |_, _| panic!("no cancel without Run")
            ),
            Err("broken pipe")
        );
        run(&gate, &log);
        assert_eq!(*log.borrow(), ["run", "held cancel None"]);
    }

    /// The lock is held across the held-cancel write, so a cancel that
    /// arrives after Run is written after the held one, not before it
    /// (which would leave the older time limit last).
    #[test]
    fn a_cancel_during_the_held_write_waits_for_it() {
        use std::sync::mpsc;
        use std::sync::{Arc, Mutex};
        let gate = Arc::new(HelperRunGate::new());
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        gate.deliver_cancel(Some(Duration::from_secs(3)), || panic!("before Run"));
        // A channel with timeouts rather than a barrier, so a gate that never
        // writes the held cancel fails the test instead of hanging it.
        let (in_write_tx, in_write_rx) = mpsc::channel::<()>();
        let canceller = {
            let (gate, log) = (gate.clone(), log.clone());
            std::thread::spawn(move || {
                if in_write_rx.recv_timeout(Duration::from_secs(10)).is_ok() {
                    gate.deliver_cancel(None, || log.lock().unwrap().push("later cancel".into()));
                }
            })
        };
        gate.start_run::<(), ()>(
            &mut (),
            |_| Ok(()),
            |_, _| {
                in_write_tx.send(()).unwrap();
                // Give the canceller time to try; it must block on the lock.
                std::thread::sleep(Duration::from_millis(100));
                log.lock().unwrap().push("held cancel".into());
                Ok(())
            },
        )
        .unwrap();
        drop(in_write_tx);
        canceller.join().unwrap();
        assert_eq!(*log.lock().unwrap(), ["held cancel", "later cancel"]);
    }

    #[test]
    fn hook_runs_before_run_without_the_lock() {
        let gate = std::sync::Arc::new(std::sync::OnceLock::<HelperRunGate>::new());
        let in_hook = gate.clone();
        let hook: std::sync::Arc<dyn Fn() + Send + Sync> = std::sync::Arc::new(move || {
            let gate = in_hook.get().unwrap();
            // try_lock, so a start_run that held the lock here fails the
            // test instead of deadlocking it.
            assert!(
                gate.state.try_lock().is_ok(),
                "the hook must run without the gate's lock"
            );
            gate.deliver_cancel(None, || panic!("Run is not written yet"));
        });
        let _ = gate.set(HelperRunGate::with_before_run_hook(Some(hook)));
        let log = RefCell::new(Vec::new());
        run(gate.get().unwrap(), &log);
        assert_eq!(*log.borrow(), ["run", "held cancel None"]);
    }
}

#[cfg(all(test, unix))]
mod send_command_concurrency_tests {
    //! The run loop's cancel commands go through the real
    //! [`CrossUserHelper::send_command`] (a `BufWriter` over the helper's
    //! stdin) while `cancel_action` and handles write theirs over the
    //! session's raw dup of the same pipe. This checks the two never tear
    //! each other's lines, with `cat` standing in for the helper.
    use super::*;
    use crate::runner::CancelMethod;
    use std::os::unix::io::AsFd;
    use std::time::Duration;

    const TOKEN: &str = "AbCdEfGhIjKlMnOpQrStUv";

    /// A `CrossUserHelper` over `cat`, and a raw dup of its stdin, as
    /// `spawn` makes for the session.
    fn helper_over_cat() -> (CrossUserHelper, std::sync::Arc<std::fs::File>) {
        let mut child = std::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let child_stdin = child.stdin.take().unwrap();
        let raw = std::fs::File::from(nix::unistd::dup(child_stdin.as_fd()).unwrap());
        let async_reader = UnixAsyncHelperReader::new(child.stdout.take().unwrap()).unwrap();
        let helper = CrossUserHelper {
            child,
            stdin: std::io::BufWriter::new(child_stdin),
            async_reader,
            auth_token: TOKEN.into(),
        };
        (helper, std::sync::Arc::new(raw))
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn send_command_and_raw_cancel_lines_stay_whole() {
        const N: usize = 5000;
        let (mut helper, raw) = helper_over_cat();
        let handles: Vec<_> = (0..2u64)
            .map(|i| {
                let raw = raw.clone();
                std::thread::spawn(move || {
                    let method = CancelMethod::NotifyThenTerminate {
                        terminate_delay: Duration::from_secs(30 + i),
                    };
                    for _ in 0..N {
                        crate::session::send_helper_cancel_command(
                            &raw,
                            Some(TOKEN),
                            Some(Duration::from_secs(7)),
                            Some(&method),
                        );
                    }
                })
            })
            .collect();
        let run_loop = std::thread::spawn(move || {
            let method = CancelMethod::NotifyThenTerminate {
                terminate_delay: Duration::from_secs(99),
            };
            for _ in 0..N {
                assert!(send_cancel_command(&mut helper, &method, None, "s"));
            }
            helper
        });
        for h in handles {
            h.join().unwrap();
        }
        let mut helper = run_loop.join().unwrap();
        let mut lines = 0;
        while lines < 3 * N {
            let line = tokio::time::timeout(
                Duration::from_secs(30),
                helper.async_reader().next_response(),
            )
            .await
            .expect("cat must echo every line")
            .expect("cat must not close stdout early")
            .unwrap_or_else(|e| panic!("torn cancel line: {e}"));
            serde_json::from_value::<crate::helper_protocol::Command>(line.clone())
                .unwrap_or_else(|e| panic!("mixed cancel line {line}: {e}"));
            lines += 1;
        }
        // Dropping the helper kills `cat`.
    }
}

#[cfg(test)]
mod run_via_helper_gate_tests {
    //! [`run_via_helper`] with a fake helper whose commands go to a file,
    //! so the file shows the order the helper would read them in.
    use super::*;
    use crate::runner::CancelMethod;
    use std::io::Write;
    use std::time::Duration;

    const TOKEN: &str = "AbCdEfGhIjKlMnOpQrStUv";

    struct FakeReader(tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>);

    impl AsyncHelperReader for FakeReader {
        fn next_response(&mut self) -> NextResponseFuture<'_> {
            Box::pin(async move { self.0.recv().await.map(Ok) })
        }
    }

    struct FakeHelper {
        reader: FakeReader,
        pipe: std::fs::File,
        /// Fail every command after the Run command.
        fail_after_run: bool,
    }

    impl AsyncHelper for FakeHelper {
        fn async_reader(&mut self) -> &mut dyn AsyncHelperReader {
            &mut self.reader
        }
        /// A Run command is logged as `RUN <command>`; anything else (a
        /// cancel) as its JSON with the token added, as the real
        /// `send_command` sends it.
        fn send_command(&mut self, cmd: &serde_json::Value) -> Result<(), SessionError> {
            if self.fail_after_run && cmd.get("command").is_none() {
                return Err(SessionError::HelperCommunication("broken pipe".into()));
            }
            match cmd.get("command").and_then(|c| c.as_str()) {
                Some(command) => writeln!(self.pipe, "RUN {command}").unwrap(),
                None => writeln!(self.pipe, "{}", augment_with_token(cmd, TOKEN)?).unwrap(),
            }
            Ok(())
        }
    }

    struct Setup {
        _tmp: tempfile::TempDir,
        path: std::path::PathBuf,
        helper: FakeHelper,
        responses: tokio::sync::mpsc::UnboundedSender<serde_json::Value>,
        config: crate::subprocess::SubprocessConfig,
        cancel_tx: tokio::sync::watch::Sender<Option<Duration>>,
    }

    fn setup(method: CancelMethod, timeout: Option<Duration>) -> Setup {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("pipe.log");
        let pipe = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        let (responses, rx) = tokio::sync::mpsc::unbounded_channel();
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(None);
        Setup {
            helper: FakeHelper {
                reader: FakeReader(rx),
                pipe,
                fail_after_run: false,
            },
            config: crate::subprocess::SubprocessConfig {
                args: vec!["work".into()],
                env_vars: Default::default(),
                working_dir: None,
                timeout,
                user: None,
                cancel_method: method,
                cancel_request_rx: Some(cancel_rx),
                debug_collect_stdout: false,
            },
            path,
            _tmp: tmp,
            responses,
            cancel_tx,
        }
    }

    async fn run(
        s: &mut Setup,
        gate: Option<&HelperRunGate>,
        token: &tokio_util::sync::CancellationToken,
    ) -> crate::subprocess::SubprocessResult {
        let mut filter = crate::action_filter::ActionFilter::new("s", true, false);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        run_via_helper(&mut s.helper, &s.config, &mut filter, "s", tx, gate, token)
            .await
            .unwrap()
    }

    /// The pipe's lines, with each cancel command parsed so key order
    /// doesn't matter.
    fn pipe(s: &Setup) -> Vec<Line> {
        std::fs::read_to_string(&s.path)
            .unwrap()
            .lines()
            .map(|l| match l.strip_prefix("RUN ") {
                Some(command) => Line::Run(command.to_string()),
                None => Line::Cancel(serde_json::from_str(l).unwrap()),
            })
            .collect()
    }

    #[derive(Debug, PartialEq)]
    enum Line {
        Run(String),
        Cancel(serde_json::Value),
    }

    fn cancel(method: &CancelMethod, limit: Option<Duration>) -> Line {
        Line::Cancel(serde_json::from_str(&helper_cancel_line(Some(TOKEN), method, limit)).unwrap())
    }

    fn run_line() -> Line {
        Line::Run("work".into())
    }

    fn ntt(secs: u64) -> CancelMethod {
        CancelMethod::NotifyThenTerminate {
            terminate_delay: Duration::from_secs(secs),
        }
    }

    /// A cancel held before Run is written right after Run, once, from the
    /// runner's method and the cancel's time limit, and the exit counts as
    /// Canceled from the held cancel alone (here neither the watch nor the
    /// token fires).
    #[tokio::test]
    async fn held_cancel_follows_run_with_the_runner_method_and_time_limit() {
        let mut s = setup(ntt(8), None);
        let gate = HelperRunGate::new();
        gate.deliver_cancel(Some(Duration::from_secs(2)), || panic!("before Run"));
        s.responses.send(serde_json::json!({"pid": 7})).unwrap();
        s.responses
            .send(serde_json::json!({"exited": 143}))
            .unwrap();
        let r = run(&mut s, Some(&gate), &Default::default()).await;
        assert_eq!(
            pipe(&s),
            [run_line(), cancel(&ntt(8), Some(Duration::from_secs(2)))]
        );
        assert_eq!(r.state, ActionState::Canceled);
    }

    /// The canceller's watch send and token cancel that follow a held
    /// cancel don't make the token branch send a second command.
    #[tokio::test]
    async fn held_cancel_then_token_sends_one_command() {
        let mut s = setup(CancelMethod::Terminate, None);
        let gate = HelperRunGate::new();
        let token = tokio_util::sync::CancellationToken::new();
        gate.deliver_cancel(None, || panic!("before Run"));
        s.cancel_tx.send(None).unwrap();
        token.cancel();
        let responses = s.responses.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            responses.send(serde_json::json!({"exited": 137})).unwrap();
        });
        let r = run(&mut s, Some(&gate), &token).await;
        assert_eq!(
            pipe(&s),
            [run_line(), cancel(&CancelMethod::Terminate, None)]
        );
        assert_eq!(r.state, ActionState::Canceled);
    }

    /// After a held cancel is written, the action's timeout can't fire a
    /// second command or relabel the action Timeout, as after any other
    /// delivered cancel.
    #[tokio::test]
    async fn held_cancel_disarms_the_timeout() {
        let mut s = setup(ntt(8), Some(Duration::from_millis(10)));
        let gate = HelperRunGate::new();
        gate.deliver_cancel(None, || panic!("before Run"));
        let responses = s.responses.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            responses.send(serde_json::json!({"exited": 143})).unwrap();
        });
        let r = run(&mut s, Some(&gate), &Default::default()).await;
        assert_eq!(pipe(&s), [run_line(), cancel(&ntt(8), None)]);
        assert_eq!(r.state, ActionState::Canceled);
    }

    /// A bare token cancel (the watch channel hasn't fired) sends one
    /// command, from the action's method, through the helper's own stdin,
    /// so it doesn't depend on a dup of the cancel pipe.
    #[tokio::test]
    async fn bare_token_cancel_goes_through_the_helper() {
        let mut s = setup(ntt(8), None);
        let gate = HelperRunGate::new();
        let token = tokio_util::sync::CancellationToken::new();
        token.cancel();
        let responses = s.responses.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            responses.send(serde_json::json!({"exited": 143})).unwrap();
        });
        let r = run(&mut s, Some(&gate), &token).await;
        assert_eq!(pipe(&s), [run_line(), cancel(&ntt(8), None)]);
        assert_eq!(r.state, ActionState::Canceled);
    }

    /// A timeout sends one command, from the action's method, through the
    /// helper's own stdin, and a token cancel after it sends none.
    #[tokio::test]
    async fn timeout_goes_through_the_helper() {
        let mut s = setup(CancelMethod::Terminate, Some(Duration::from_millis(10)));
        let token = tokio_util::sync::CancellationToken::new();
        let responses = s.responses.clone();
        let late_token = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            late_token.cancel();
            tokio::time::sleep(Duration::from_millis(100)).await;
            responses.send(serde_json::json!({"exited": 137})).unwrap();
        });
        let r = run(&mut s, Some(&HelperRunGate::new()), &token).await;
        assert_eq!(
            pipe(&s),
            [run_line(), cancel(&CancelMethod::Terminate, None)]
        );
        assert_eq!(r.state, ActionState::Timeout);
    }

    /// A bare token cancel disarms the timeout: one command, and the action
    /// stays Canceled.
    #[tokio::test]
    async fn token_cancel_disarms_the_timeout() {
        let mut s = setup(ntt(8), Some(Duration::from_millis(100)));
        let token = tokio_util::sync::CancellationToken::new();
        token.cancel();
        let responses = s.responses.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            responses.send(serde_json::json!({"exited": 143})).unwrap();
        });
        let r = run(&mut s, Some(&HelperRunGate::new()), &token).await;
        assert_eq!(pipe(&s), [run_line(), cancel(&ntt(8), None)]);
        assert_eq!(r.state, ActionState::Canceled);
    }

    /// When the canceller has fired the watch channel it writes the command
    /// itself, so the token branch sends none.
    #[tokio::test]
    async fn token_after_the_watch_sends_nothing() {
        let mut s = setup(CancelMethod::Terminate, None);
        let token = tokio_util::sync::CancellationToken::new();
        s.cancel_tx.send(None).unwrap();
        token.cancel();
        let responses = s.responses.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            responses.send(serde_json::json!({"exited": 137})).unwrap();
        });
        let r = run(&mut s, Some(&HelperRunGate::new()), &token).await;
        assert_eq!(pipe(&s), [run_line()]);
        assert_eq!(r.state, ActionState::Canceled);
    }

    /// A failed timeout or token cancel send doesn't fail the action: the
    /// run loop goes on reading the helper's responses to the end.
    #[tokio::test]
    async fn failed_cancel_send_does_not_fail_the_action() {
        for via_timeout in [false, true] {
            let mut s = setup(
                CancelMethod::Terminate,
                via_timeout.then_some(Duration::from_millis(10)),
            );
            s.helper.fail_after_run = true;
            let token = tokio_util::sync::CancellationToken::new();
            if !via_timeout {
                token.cancel();
            }
            let responses = s.responses.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                responses.send(serde_json::json!({"exited": 0})).unwrap();
            });
            let r = run(&mut s, Some(&HelperRunGate::new()), &token).await;
            assert_eq!(pipe(&s), [run_line()], "via_timeout={via_timeout}");
            assert_eq!(r.exit_code, Some(0), "via_timeout={via_timeout}");
            let expected = if via_timeout {
                ActionState::Timeout
            } else {
                ActionState::Canceled
            };
            assert_eq!(r.state, expected, "via_timeout={via_timeout}");
        }
    }

    /// Run is written before the held cancel, so a failed held-cancel send
    /// doesn't fail the action: the run loop goes on reading the helper's
    /// responses to the end, and the helper's own stdin is where a broken
    /// pipe would show.
    #[tokio::test]
    async fn failed_held_cancel_send_does_not_fail_the_action() {
        let mut s = setup(CancelMethod::Terminate, None);
        s.helper.fail_after_run = true;
        let gate = HelperRunGate::new();
        gate.deliver_cancel(None, || panic!("before Run"));
        s.responses.send(serde_json::json!({"pid": 7})).unwrap();
        s.responses.send(serde_json::json!({"exited": 0})).unwrap();
        let r = run(&mut s, Some(&gate), &Default::default()).await;
        assert_eq!(pipe(&s), [run_line()]);
        assert_eq!(r.exit_code, Some(0));
    }

    /// With no cancel, only Run is written and the gate passes cancels
    /// straight to the pipe from then on.
    #[tokio::test]
    async fn without_a_cancel_only_run_is_written() {
        let mut s = setup(CancelMethod::Terminate, None);
        let gate = HelperRunGate::new();
        s.responses.send(serde_json::json!({"exited": 0})).unwrap();
        let r = run(&mut s, Some(&gate), &Default::default()).await;
        assert_eq!(pipe(&s), [run_line()]);
        assert_eq!(r.state, ActionState::Success);
        let mut written = false;
        gate.deliver_cancel(None, || written = true);
        assert!(written, "after Run the canceller writes the command itself");
    }
}

#[cfg(test)]
mod cancel_line_tests {
    use super::helper_cancel_line;
    use crate::helper_protocol::{CancelMethod as WireMethod, Command};
    use crate::runner::CancelMethod;
    use std::time::Duration;

    const TOKEN: &str = "AbCdEfGhIjKlMnOpQrStUv";

    /// What the helper must receive: `None` for TERMINATE, `Some(n)` for
    /// NOTIFY_THEN_TERMINATE with `n` seconds.
    type Wire = Option<u64>;

    fn ntt(d: Duration) -> CancelMethod {
        CancelMethod::NotifyThenTerminate { terminate_delay: d }
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// The full matrix: each method (Terminate, and NotifyThenTerminate with
    /// 0 s, a sub-second, and n s), without a time limit and with one that
    /// is zero, sub-second, smaller, equal and larger.
    fn matrix() -> Vec<(CancelMethod, Option<Duration>, Wire)> {
        vec![
            // Terminate ignores the time limit.
            (CancelMethod::Terminate, None, None),
            (CancelMethod::Terminate, Some(secs(0)), None),
            (CancelMethod::Terminate, Some(ms(500)), None),
            (CancelMethod::Terminate, Some(secs(5)), None),
            (CancelMethod::Terminate, Some(secs(600)), None),
            // A zero grace has nothing to notify for.
            (ntt(secs(0)), None, None),
            (ntt(secs(0)), Some(secs(0)), None),
            (ntt(secs(0)), Some(secs(5)), None),
            // A sub-second grace truncates to zero whole seconds.
            (ntt(ms(999)), None, None),
            (ntt(ms(999)), Some(ms(500)), None),
            (ntt(ms(999)), Some(ms(999)), None),
            (ntt(ms(999)), Some(secs(5)), None),
            // n s, truncated: 10.9 s goes out as 10.
            (ntt(secs(10)), None, Some(10)),
            (ntt(ms(10_900)), None, Some(10)),
            (ntt(secs(10)), Some(secs(0)), None),
            (ntt(secs(10)), Some(ms(500)), None),
            (ntt(secs(10)), Some(ms(1500)), Some(1)),
            (ntt(secs(10)), Some(secs(3)), Some(3)),
            (ntt(secs(10)), Some(secs(10)), Some(10)),
            (ntt(secs(10)), Some(secs(60)), Some(10)),
        ]
    }

    #[test]
    fn helper_cancel_line_matrix_exact_json() {
        for (method, limit, wire) in matrix() {
            let line = helper_cancel_line(Some(TOKEN), &method, limit);
            let expected = match wire {
                None => format!(r#"{{"token":"{TOKEN}","cancel":"TERMINATE"}}"#),
                Some(n) => format!(
                    r#"{{"token":"{TOKEN}","cancel":"NOTIFY_THEN_TERMINATE","notifyPeriodInSeconds":{n}}}"#
                ),
            };
            assert_eq!(line, expected, "{method} with time_limit {limit:?}");
        }
    }

    #[test]
    fn helper_cancel_line_without_token_omits_the_field() {
        assert_eq!(
            helper_cancel_line(None, &CancelMethod::Terminate, None),
            r#"{"cancel":"TERMINATE"}"#
        );
        assert_eq!(
            helper_cancel_line(None, &ntt(secs(7)), None),
            r#"{"cancel":"NOTIFY_THEN_TERMINATE","notifyPeriodInSeconds":7}"#
        );
    }

    /// Every line the builder produces parses with the helper's own
    /// deserializer (`helper/src/protocol.rs`) into the expected
    /// `Command::Cancel`, carrying the token.
    #[test]
    fn helper_cancel_line_round_trips_through_helper_protocol() {
        for (method, limit, wire) in matrix() {
            let line = helper_cancel_line(Some(TOKEN), &method, limit);
            let cmd: Command = serde_json::from_str(&line)
                .unwrap_or_else(|e| panic!("helper rejected {line}: {e}"));
            match (cmd, wire) {
                (
                    Command::Cancel {
                        token,
                        method: WireMethod::Terminate,
                    },
                    None,
                ) => assert_eq!(token, TOKEN),
                (
                    Command::Cancel {
                        token,
                        method:
                            WireMethod::NotifyThenTerminate {
                                notify_period_in_seconds,
                            },
                    },
                    Some(n),
                ) => {
                    assert_eq!(token, TOKEN);
                    assert_eq!(notify_period_in_seconds, n, "{line}");
                }
                (other, _) => panic!(
                    "{method} with time_limit {limit:?}: {line} parsed as {other:?}, \
                     expected {wire:?}"
                ),
            }
        }
        // Without a token the helper still parses it (as an empty token,
        // which its constant-time check then rejects).
        let cmd: Command =
            serde_json::from_str(&helper_cancel_line(None, &CancelMethod::Terminate, None))
                .unwrap();
        assert_eq!(cmd.token(), "");
    }
}

#[cfg(test)]
mod token_tests {
    use super::*;

    #[test]
    fn token_is_22_chars_from_url_safe_alphabet() {
        for _ in 0..100 {
            let t = generate_auth_token().unwrap();
            assert_eq!(
                t.len(),
                AUTH_TOKEN_LEN,
                "token should be {AUTH_TOKEN_LEN} chars",
            );
            assert!(t.is_ascii(), "token must be ASCII");
            for c in t.bytes() {
                assert!(
                    AUTH_TOKEN_ALPHABET.contains(&c),
                    "token contains non-alphabet byte: {c:#x}",
                );
            }
        }
    }

    #[test]
    fn generated_tokens_differ() {
        // This could technically collide, but the probability over two
        // 128-bit draws is astronomically small. If this ever flakes, the
        // CSPRNG is broken.
        let a = generate_auth_token().unwrap();
        let b = generate_auth_token().unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn augment_with_token_inserts_token() {
        let cmd = serde_json::json!({"command": "echo", "args": ["hi"]});
        let out = augment_with_token(&cmd, "T-O-K-E-N").unwrap();
        assert_eq!(out.get("token").and_then(|v| v.as_str()), Some("T-O-K-E-N"),);
        assert_eq!(out.get("command").and_then(|v| v.as_str()), Some("echo"));
    }

    #[test]
    fn augment_rejects_non_object() {
        let err = augment_with_token(&serde_json::json!("shutdown"), "tok").unwrap_err();
        assert!(err.to_string().contains("must be a JSON object"));
    }

    #[test]
    fn augment_overrides_caller_supplied_token() {
        // augment_with_token is the sole owner of the "token" field on
        // outgoing commands — a caller-supplied one must be overwritten
        // so callers can't accidentally (or intentionally) poison it.
        let cmd = serde_json::json!({"token": "bad", "command": "x"});
        let out = augment_with_token(&cmd, "good").unwrap();
        assert_eq!(
            out.get("token").and_then(|v| v.as_str()),
            Some("good"),
            "augment_with_token must not let the caller override the token",
        );
    }
}
