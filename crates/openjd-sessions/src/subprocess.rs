// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

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

/// Grace time to wait for stdout to close after process exits.
const STDOUT_GRACE_TIME: Duration = Duration::from_secs(5);

/// Maximum line length for stdout reading.
const LOG_LINE_MAX_LENGTH: usize = 64 * 1024;

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

    /// Send SIGTERM for cross-user: signal the sudo process directly (sudo forwards SIGTERM).
    pub fn notify_cross_user(sudo_pid: i32) -> Result<(), std::io::Error> {
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(sudo_pid),
            nix::sys::signal::Signal::SIGTERM,
        )
        .map_err(std::io::Error::other)
    }

    /// Send SIGKILL for cross-user: try direct killpg with CAP_KILL elevation, fall back to sudo kill.
    pub fn terminate_cross_user(pgid: i32, user: &dyn SessionUser) {
        // Try using CAP_KILL for direct signal delivery
        let (has_cap_kill, _guard) = crate::capabilities::try_use_cap_kill();
        if has_cap_kill
            && nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pgid),
                nix::sys::signal::Signal::SIGKILL,
            )
            .is_ok()
        {
            return;
        }
        // Fall back to sudo kill
        let _ = std::process::Command::new("sudo")
            .args([
                "-u",
                user.user(),
                "-i",
                "kill",
                "-s",
                "kill",
                "--",
                &format!("-{pgid}"),
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }

    /// Send a terminate signal (SIGKILL) to the process, handling cross-user via sudo.
    pub fn send_terminate(pid: i32, sudo_child_pgid: Option<i32>, user: Option<&dyn SessionUser>) {
        if let Some(user) = user.filter(|u| !u.is_process_user()) {
            if let Some(pgid) = sudo_child_pgid {
                terminate_cross_user(pgid, user);
            }
        } else {
            let _ = terminate_process_group(pid);
        }
    }

    /// Send a notify signal (SIGTERM) to the process, handling cross-user via sudo.
    pub fn send_notify(pid: i32, user: Option<&dyn SessionUser>) {
        if let Some(_user) = user.filter(|u| !u.is_process_user()) {
            let _ = notify_cross_user(pid);
        } else {
            let _ = notify_process_group(pid);
        }
    }

    /// Find the child process group ID of a sudo process.
    ///
    /// Retries for up to 1 second since sudo takes time to fork.
    pub fn find_sudo_child_pgid(sudo_pid: u32) -> Option<i32> {
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(1);
        let sudo_pgid = nix::unistd::getpgid(Some(nix::unistd::Pid::from_raw(sudo_pid as i32)))
            .ok()
            .map(|p| p.as_raw());

        loop {
            // Try procfs first (Linux)
            if let Some(child_pid) = find_child_pid_procfs(sudo_pid) {
                if let Ok(pgid) = nix::unistd::getpgid(Some(nix::unistd::Pid::from_raw(child_pid)))
                {
                    let pgid = pgid.as_raw();
                    // Wait until the child has its own process group (not sudo's)
                    if sudo_pgid != Some(pgid) {
                        return Some(pgid);
                    }
                } else {
                    return None; // Process already exited
                }
            } else {
                // Fall back to pgrep
                if let Some(child_pid) = find_child_pid_pgrep(sudo_pid) {
                    if let Ok(pgid) =
                        nix::unistd::getpgid(Some(nix::unistd::Pid::from_raw(child_pid)))
                    {
                        let pgid = pgid.as_raw();
                        if sudo_pgid != Some(pgid) {
                            return Some(pgid);
                        }
                    } else {
                        return None;
                    }
                }
            }

            if start.elapsed() >= timeout {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn find_child_pid_procfs(parent_pid: u32) -> Option<i32> {
        let task_dir = format!("/proc/{parent_pid}/task");
        let mut child_pids = std::collections::HashSet::new();
        if let Ok(entries) = std::fs::read_dir(&task_dir) {
            for entry in entries.flatten() {
                let children_path = entry.path().join("children");
                if let Ok(contents) = std::fs::read_to_string(&children_path) {
                    for token in contents.split_whitespace() {
                        if let Ok(pid) = token.parse::<i32>() {
                            child_pids.insert(pid);
                        }
                    }
                }
            }
        }
        if child_pids.len() == 1 {
            child_pids.into_iter().next()
        } else {
            None
        }
    }

    fn find_child_pid_pgrep(parent_pid: u32) -> Option<i32> {
        let output = std::process::Command::new("pgrep")
            .args(["-P", &parent_pid.to_string()])
            .stdin(std::process::Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.trim().lines().collect();
        if lines.len() == 1 {
            lines[0].trim().parse().ok()
        } else {
            None
        }
    }

    /// Spawn a delayed SIGKILL after a grace period, handling cross-user via sudo.
    pub fn spawn_delayed_terminate(
        pid: i32,
        sudo_child_pgid: Option<i32>,
        user: Option<Arc<dyn SessionUser>>,
        delay: Duration,
    ) {
        if let Some(ref user) = user {
            if !user.is_process_user() {
                let pgid = sudo_child_pgid;
                let u = user.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    if let Some(pgid) = pgid {
                        terminate_cross_user(pgid, &*u);
                    }
                });
                return;
            }
        }
        let cancel_pid = pid;
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = terminate_process_group(cancel_pid);
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
    fn send_ctrl_break(pid: u32) -> bool {
        use windows::Win32::System::Console::{
            AttachConsole, FreeConsole, GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT,
        };
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
                let mut entry = PROCESSENTRY32W::default();
                entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
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
    pub fn send_terminate(
        pid: i32,
        _sudo_child_pgid: Option<i32>,
        _user: Option<&dyn SessionUser>,
    ) {
        kill_process_tree(pid as u32);
    }

    /// Notify: send CTRL_BREAK_EVENT for graceful shutdown.
    pub fn send_notify(pid: i32, _user: Option<&dyn SessionUser>) {
        if !send_ctrl_break(pid as u32) {
            log::warn!(target: "openjd.sessions", "Failed to send CTRL_BREAK to pid {pid}, falling back to terminate");
            send_terminate(pid, None, None);
        }
    }

    /// No sudo on Windows.
    pub fn find_sudo_child_pgid(_sudo_pid: u32) -> Option<i32> {
        None
    }

    /// Delayed terminate: kill the process tree after a grace period.
    pub fn spawn_delayed_terminate(
        pid: i32,
        _sudo_child_pgid: Option<i32>,
        _user: Option<Arc<dyn SessionUser>>,
        delay: Duration,
    ) {
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
        let write_owned = OwnedHandle::from_raw_handle(write_handle.0 as *mut std::ffi::c_void);
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
            let read_owned = OwnedHandle::from_raw_handle(read_handle.0 as *mut std::ffi::c_void);
            let read_std: std::fs::File = std::fs::File::from(read_owned);
            let read_tokio = tokio::fs::File::from_std(read_std);
            return Some(Box::new(read_tokio));
        }
        let write_owned_dup =
            OwnedHandle::from_raw_handle(write_handle_dup.0 as *mut std::ffi::c_void);
        let write_stdio_stderr = std::process::Stdio::from(write_owned_dup);

        cmd.stdout(write_stdio_stdout);
        cmd.stderr(write_stdio_stderr);

        // Convert read handle to an async reader
        let read_owned = OwnedHandle::from_raw_handle(read_handle.0 as *mut std::ffi::c_void);
        let read_std: std::fs::File = std::fs::File::from(read_owned);
        let read_tokio = tokio::fs::File::from_std(read_std);
        Some(Box::new(read_tokio))
    }
}

use platform::*;

/// Generate a shell script that sets up env vars, cd, and exec's the command.
/// Mirrors Python's `_generate_command_shell_script`.
#[cfg(unix)]
fn generate_command_shell_script(
    args: &[String],
    env_vars: &HashMap<String, Option<String>>,
    working_dir: Option<&Path>,
) -> Result<String, SessionError> {
    let quote = |s: &str| -> Result<String, SessionError> {
        shlex::try_quote(s).map(|c| c.into_owned()).map_err(|_| {
            SessionError::Runtime(format!(
                "Cannot shell-quote string containing null byte: {s:?}"
            ))
        })
    };
    let mut script = String::from("#!/bin/sh\n");
    for (name, value) in env_vars {
        match value {
            Some(val) => {
                script.push_str(&format!("export {}={}\n", name, quote(val)?));
            }
            None => {
                script.push_str(&format!("unset {name}\n"));
            }
        }
    }
    if let Some(dir) = working_dir {
        script.push_str(&format!("cd {}\n", quote(&dir.to_string_lossy())?));
    }
    let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let joined = shlex::try_join(args_str.iter().copied())
        .map_err(|_| SessionError::Runtime("Command args contain null byte".into()))?;
    script.push_str(&format!("exec {joined}\n"));
    Ok(script)
}

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

    let cross_user = config.user.as_deref().filter(|u| !u.is_process_user());

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

    let (final_args, use_setsid, _script_path) = if let Some(_user) = cross_user {
        #[cfg(unix)]
        {
            // Cross-user: generate shell script wrapper
            let script_content = generate_command_shell_script(
                args,
                &config.env_vars,
                config.working_dir.as_deref(),
            )?;
            let script_dir = config
                .working_dir
                .as_deref()
                .unwrap_or_else(|| std::path::Path::new("/tmp"));
            let script_path = script_dir.join(format!("_openjd_run_{}.sh", std::process::id()));
            std::fs::write(&script_path, &script_content).map_err(|e| {
                SessionError::SubprocessStart {
                    command: args[0].clone(),
                    source: e,
                }
            })?;
            {
                use std::os::unix::fs::PermissionsExt;
                let _ =
                    std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o770));
                if let Ok(Some(grp)) = nix::unistd::Group::from_name(_user.group()) {
                    let _ = nix::unistd::chown(&script_path, None, Some(grp.gid));
                }
            }
            let sudo_args = vec![
                "sudo".to_string(),
                "-u".to_string(),
                _user.user().to_string(),
                "-i".to_string(),
                "setsid".to_string(),
                "-w".to_string(),
                script_path.to_string_lossy().to_string(),
            ];
            (sudo_args, false, Some(script_path))
        }
        #[cfg(windows)]
        {
            // Cross-user on Windows: stub — full CreateProcessAsUserW in Phase 3
            // For now, just run the command directly (same-user fallback)
            (args.clone(), false, None::<PathBuf>)
        }
    } else {
        (args.clone(), true, None)
    };

    // Log the command line (redacting any openjd_redacted_env tokens)
    if final_args != *args {
        // Cross-user: log the actual command, not the sudo wrapper
        session_log!(
            info,
            session_id,
            LogContent::FILE_PATH | LogContent::PROCESS_CONTROL,
            "Running command {}",
            format_command_for_log(args)
        );
        log::debug!(target: "openjd.sessions", "Wrapper: {}", format_command_for_log(&final_args));
    } else {
        session_log!(
            info,
            session_id,
            LogContent::FILE_PATH | LogContent::PROCESS_CONTROL,
            "Running command {}",
            format_command_for_log(&final_args)
        );
    }

    // Spawn the process. On Windows cross-user, we use Win32 APIs directly;
    // otherwise we use tokio::process::Command.
    #[cfg(windows)]
    let mut win32_process_handle: Option<windows::Win32::Foundation::HANDLE> = None;

    #[allow(unused_mut)]
    let (mut child, pid, stdout_for_reading): (
        Option<tokio::process::Child>,
        i32,
        Option<Box<dyn tokio::io::AsyncRead + Unpin + Send>>,
    ) = {
        #[cfg(windows)]
        if let Some(_cross) = cross_user {
            use crate::session_user::WindowsSessionUser;
            let win_user = config.user.as_ref().unwrap();
            if let Some(wu) = win_user.as_any().downcast_ref::<WindowsSessionUser>() {
                match crate::win32::spawn_as_user(
                    args,
                    &config.env_vars,
                    config.working_dir.as_deref(),
                    wu.password(),
                    wu.user(),
                    wu.logon_token(),
                ) {
                    Ok(spawned) => {
                        let p = spawned.pid as i32;
                        win32_process_handle = Some(spawned.process_handle);
                        let std_file: std::fs::File = spawned.stdout_read.into();
                        let tokio_file = tokio::fs::File::from_std(std_file);
                        (
                            None,
                            p,
                            Some(Box::new(tokio_file)
                                as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
                        )
                    }
                    Err(e) => {
                        session_log!(
                            info,
                            session_id,
                            LogContent::EXCEPTION_INFO | LogContent::PROCESS_CONTROL,
                            "Process failed to start: '{}': {}",
                            args[0],
                            e
                        );
                        return Err(SessionError::SubprocessStart {
                            command: args[0].clone(),
                            source: std::io::Error::new(std::io::ErrorKind::Other, e),
                        });
                    }
                }
            } else {
                return Err(SessionError::Runtime(
                    "Cross-user on Windows requires WindowsSessionUser".into(),
                ));
            }
        } else {
            let mut cmd = Command::new(&final_args[0]);
            cmd.args(&final_args[1..]);
            cmd.env_clear();
            for (k, v) in &merged {
                cmd.env(k, v);
            }
            if let Some(dir) = &config.working_dir {
                cmd.current_dir(dir);
            }
            let merged_reader = unsafe { configure_command(&mut cmd, use_setsid) };
            if merged_reader.is_none() {
                // POSIX: configure_command sets up dup2 in pre_exec, we pipe stdout normally
                cmd.stdout(std::process::Stdio::piped());
            }
            let mut c = cmd.spawn().map_err(|e| {
                session_log!(
                    info,
                    session_id,
                    LogContent::EXCEPTION_INFO | LogContent::PROCESS_CONTROL,
                    "Process failed to start: '{}': {}",
                    final_args[0],
                    e
                );
                SessionError::SubprocessStart {
                    command: final_args[0].clone(),
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
        }

        #[cfg(not(windows))]
        {
            let mut cmd = Command::new(&final_args[0]);
            cmd.args(&final_args[1..]);
            if cross_user.is_none() {
                cmd.env_clear();
                for (k, v) in &merged {
                    cmd.env(k, v);
                }
            }
            if cross_user.is_none() {
                if let Some(dir) = &config.working_dir {
                    cmd.current_dir(dir);
                }
            }
            let merged_reader = unsafe { configure_command(&mut cmd, use_setsid) };
            if merged_reader.is_none() {
                cmd.stdout(std::process::Stdio::piped());
            }
            let mut c = cmd.spawn().map_err(|e| {
                session_log!(
                    info,
                    session_id,
                    LogContent::EXCEPTION_INFO | LogContent::PROCESS_CONTROL,
                    "Process failed to start: '{}': {}",
                    final_args[0],
                    e
                );
                SessionError::SubprocessStart {
                    command: final_args[0].clone(),
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
        }
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

    // For cross-user, find sudo's child process group ID
    let sudo_child_pgid = if cross_user.is_some() {
        find_sudo_child_pgid(pid as u32)
    } else {
        None
    };

    // Read merged stdout+stderr from the child
    let mut cancel_requested = false;
    let mut timed_out = false;
    let mut stdout_collected = String::new();
    let mut saw_fail = false;

    if let Some(stdout) = stdout_for_reading {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        // Create timeout future once, pin it for reuse across loop iterations
        let timeout_fut = async {
            match config.timeout {
                Some(d) => tokio::time::sleep(d).await,
                None => std::future::pending().await,
            }
        };
        tokio::pin!(timeout_fut);

        loop {
            tokio::select! {
                biased;

                _ = cancel_token.cancelled(), if !cancel_requested => {
                    cancel_requested = true;
                    let time_limit = config.cancel_request_rx.as_ref()
                        .and_then(|rx| *rx.borrow());

                    match (&config.cancel_method, time_limit) {
                        (_, Some(limit)) if limit.is_zero() => {
                            session_log!(info, session_id, LogContent::PROCESS_CONTROL, "Urgent cancel (time_limit=0), sending SIGKILL to process group {}", pid);
                            send_terminate(pid, sudo_child_pgid, config.user.as_deref());
                        }
                        (CancelMethod::Terminate, _) => {
                            session_log!(info, session_id, LogContent::PROCESS_CONTROL, "Sending SIGKILL to process group {}", pid);
                            send_terminate(pid, sudo_child_pgid, config.user.as_deref());
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
                            send_notify(pid, config.user.as_deref());
                            spawn_delayed_terminate(pid, sudo_child_pgid, config.user.clone(), delay);
                        }
                    }
                }

                _ = &mut timeout_fut, if !cancel_requested && !timed_out => {
                    timed_out = true;
                    cancel_requested = true;
                    session_log!(info, session_id, LogContent::PROCESS_CONTROL, "Action timed out, sending SIGKILL to process group");
                    send_terminate(pid, sudo_child_pgid, config.user.as_deref());
                    break;
                }

                result = lines.next_line() => {
                    match result {
                        Ok(Some(line)) => {
                            let line = if line.len() > LOG_LINE_MAX_LENGTH {
                                line[..LOG_LINE_MAX_LENGTH].to_string()
                            } else {
                                line
                            };
                            let (display, pass_through) = process_line(&line, filter, session_id, &message_tx, &mut saw_fail);
                            if pass_through && filter.min_log_level() <= 20 {
                                session_log!(info, session_id, LogContent::COMMAND_OUTPUT, "{}", display);
                            }
                            stdout_collected.push_str(&line);
                            stdout_collected.push('\n');
                        }
                        Ok(None) => break, // EOF
                        Err(_) => break,
                    }
                }
            }
        }
    }

    // Wait for process to exit
    let exit_status = if let Some(ref mut c) = child {
        match tokio::time::timeout(STDOUT_GRACE_TIME, c.wait()).await {
            Ok(Ok(s)) => Some(s),
            Ok(Err(_)) => {
                send_terminate(pid, sudo_child_pgid, config.user.as_deref());
                None
            }
            Err(_) => {
                send_terminate(pid, sudo_child_pgid, config.user.as_deref());
                c.wait().await.ok()
            }
        }
    } else {
        // Windows cross-user: wait on the raw process handle
        #[cfg(windows)]
        {
            win32_process_handle.and_then(|h| {
                use std::os::windows::process::ExitStatusExt;
                use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
                unsafe {
                    let _ = WaitForSingleObject(h, 60000);
                    let mut code = 0u32;
                    let _ = GetExitCodeProcess(h, &mut code);
                    let _ = windows::Win32::Foundation::CloseHandle(h);
                    Some(std::process::ExitStatus::from_raw(code))
                }
            })
        }
        #[cfg(not(windows))]
        {
            None
        }
    };

    // Clean up script file
    if let Some(ref sp) = _script_path {
        let _ = std::fs::remove_file(sp);
    }

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
    } else if cancel_requested {
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
    let is_redacted_env = line.starts_with("openjd_redacted_env: ");
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
                    if is_redacted_env {
                        Some(ActionMessage::RedactedEnv { name, value })
                    } else {
                        Some(ActionMessage::SetEnv { name, value })
                    }
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
    #[test]
    fn test_empty_string_arg() {
        let script =
            generate_command_shell_script(&["echo".into(), "".into()], &HashMap::new(), None)
                .unwrap();
        let exec_line = script.lines().find(|l| l.starts_with("exec")).unwrap();
        assert!(exec_line.contains("echo"), "script was: {script}");
        assert!(
            exec_line.len() > "exec echo".len(),
            "empty arg missing, script was: {script}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_args_with_spaces() {
        let script = generate_command_shell_script(
            &["echo".into(), "hello world".into()],
            &HashMap::new(),
            None,
        )
        .unwrap();
        let exec_line = script.lines().find(|l| l.starts_with("exec")).unwrap();
        assert!(exec_line.contains("hello world"), "script was: {script}");
    }

    #[cfg(unix)]
    #[test]
    fn test_args_with_single_quotes() {
        let script =
            generate_command_shell_script(&["echo".into(), "it's".into()], &HashMap::new(), None)
                .unwrap();
        let exec_line = script.lines().find(|l| l.starts_with("exec")).unwrap();
        assert!(exec_line.contains("echo"), "script was: {script}");
        assert!(exec_line.contains("it"), "script was: {script}");
    }

    #[cfg(unix)]
    #[test]
    fn test_args_with_double_quotes() {
        let script = generate_command_shell_script(
            &["echo".into(), r#"say "hello""#.into()],
            &HashMap::new(),
            None,
        )
        .unwrap();
        let exec_line = script.lines().find(|l| l.starts_with("exec")).unwrap();
        assert!(exec_line.contains("hello"), "script was: {script}");
    }

    #[cfg(unix)]
    #[test]
    fn test_args_with_special_characters() {
        let input = "a$b`c\\d;e|f&g";
        let script =
            generate_command_shell_script(&["echo".into(), input.into()], &HashMap::new(), None)
                .unwrap();
        let exec_line = script.lines().find(|l| l.starts_with("exec")).unwrap();
        // Verify round-trip: shlex::split of the exec args should recover the original
        let parsed = shlex::split(&exec_line["exec ".len()..]).unwrap();
        assert_eq!(parsed, vec!["echo", input]);
    }

    #[cfg(unix)]
    #[test]
    fn test_env_var_quoting() {
        let mut env = HashMap::new();
        env.insert("FOO".into(), Some("bar baz".into()));
        env.insert("REMOVE".into(), None);
        let script = generate_command_shell_script(&["cmd".into()], &env, None).unwrap();
        assert!(script.contains("export FOO="), "script was: {script}");
        assert!(script.contains("bar baz"), "script was: {script}");
        assert!(script.contains("unset REMOVE"), "script was: {script}");
    }

    #[cfg(unix)]
    #[test]
    fn test_working_dir() {
        let script = generate_command_shell_script(
            &["cmd".into()],
            &HashMap::new(),
            Some(Path::new("/tmp/my dir")),
        )
        .unwrap();
        assert!(script.contains("cd "), "script was: {script}");
        assert!(script.contains("/tmp/my dir"), "script was: {script}");
    }

    #[cfg(unix)]
    #[test]
    fn test_script_structure() {
        let mut env = HashMap::new();
        env.insert("K".into(), Some("V".into()));
        let script =
            generate_command_shell_script(&["a".into(), "b".into()], &env, Some(Path::new("/w")))
                .unwrap();
        assert!(script.starts_with("#!/bin/sh\n"), "script was: {script}");
        assert!(script.ends_with('\n'), "script was: {script}");
        let last_line = script.trim_end().lines().last().unwrap();
        assert!(last_line.starts_with("exec"), "script was: {script}");
    }

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
        };

        let t = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let _ = _cancel_tx.send(Some(Duration::ZERO));
            t.cancel();
        });

        let mut filter = crate::action_filter::ActionFilter::new("test", false, false);
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

        // Use a script that traps SIGTERM so the process survives until SIGKILL
        let config = SubprocessConfig {
            args: vec!["sh".into(), "-c".into(), "trap '' TERM; sleep 30".into()],
            env_vars: HashMap::new(),
            working_dir: None,
            timeout: None,
            user: None,
            cancel_method: CancelMethod::NotifyThenTerminate {
                terminate_delay: Duration::from_secs(1),
            },
            cancel_request_rx: Some(cancel_rx),
        };

        let t = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            t.cancel();
        });

        let mut filter = crate::action_filter::ActionFilter::new("test", false, false);
        let start = std::time::Instant::now();
        let result = run_subprocess(config, &mut filter, "test", msg_tx, token)
            .await
            .unwrap();
        let elapsed = start.elapsed();

        assert_eq!(result.state, ActionState::Canceled);
        assert!(
            elapsed >= Duration::from_millis(800),
            "took {:?}, expected >= 800ms",
            elapsed
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "took {:?}, expected < 5s",
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
        };

        let t = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let _ = _cancel_tx.send(Some(Duration::from_secs(10)));
            t.cancel();
        });

        let mut filter = crate::action_filter::ActionFilter::new("test", false, false);
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
        let mut filter = ActionFilter::new("test", false, false);
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
        let mut filter = ActionFilter::new("test", false, false);
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
        let mut filter = ActionFilter::new("test", false, false);
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
        let mut filter = ActionFilter::new("test", false, false);
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
        let mut filter = ActionFilter::new("test", false, false);
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
        let mut filter = ActionFilter::new("test", false, false);
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
        let mut filter = ActionFilter::new("test", false, false);
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
            let mut filter = ActionFilter::new("test", false, false);
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
            let mut filter = ActionFilter::new("test", false, false);
            let token = CancellationToken::new();
            let config = SubprocessConfig {
                args: vec!["/nonexistent/binary_xyz".into()],
                env_vars: HashMap::new(),
                working_dir: None,
                timeout: None,
                user: None,
                cancel_method: CancelMethod::Terminate,
                cancel_request_rx: None,
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
            let mut filter = ActionFilter::new("test", false, false);
            let token = CancellationToken::new();
            let config = SubprocessConfig {
                args: vec![],
                env_vars: HashMap::new(),
                working_dir: None,
                timeout: None,
                user: None,
                cancel_method: CancelMethod::Terminate,
                cancel_request_rx: None,
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
            let mut filter = ActionFilter::new("test", false, false);
            let token = CancellationToken::new();
            let config = SubprocessConfig {
                args: vec!["sleep".into(), "30".into()],
                env_vars: HashMap::new(),
                working_dir: None,
                timeout: Some(Duration::from_millis(500)),
                user: None,
                cancel_method: CancelMethod::Terminate,
                cancel_request_rx: None,
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
}
