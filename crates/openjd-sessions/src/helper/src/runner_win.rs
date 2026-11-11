// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Windows implementation of the helper runner.
//!
//! Spawns a child process with CREATE_NEW_PROCESS_GROUP, reads its stdout
//! on background threads, and handles cancel commands via a channel from main.
//!
//! When the helper is configured with a [`JobObject`], every workload is
//! also assigned to the job. This is technically redundant — Windows
//! already associates new child processes with every Job Object their
//! parent belongs to — but keeping the explicit `assign_process` call
//! gives us a clear failure signal if the workload was launched with
//! `CREATE_BREAKAWAY_FROM_JOB` for any reason in the future.

use super::job_object::JobObject;
use super::protocol::{send, CancelMethod, Response, RunCommand};
use crate::framer::{fit_out_payload, LineFramer};
use crate::process_tree::{collect_tree_validated, process_identity_matches, ValidatedProcess};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;

/// Queue slots shared by the stdout and stderr reader threads. Bounds the
/// worst-case aggregate queue at 256 lines of at most `MAX_LINE_BYTES`
/// (64 KiB) each, ~16 MiB, while staying far above what the 50 ms consumer
/// poll drains per tick in normal operation.
const OUT_QUEUE_LINES: usize = 256;

/// Run a command, receiving cancel signals from the provided channel.
///
/// Architecture:
/// - Background threads read child stdout + stderr, send lines via channel
/// - Main thread drains output lines
/// - Cancel signals arrive via `cancel_rx` from the stdin reader in main.rs
///
/// If `job` is `Some`, the spawned workload is explicitly assigned to it.
/// In practice the workload would already inherit the helper's job, but
/// the explicit assignment makes the invariant testable.
pub fn run_command(
    cmd: &RunCommand,
    cancel_rx: &mpsc::Receiver<CancelMethod>,
    job: Option<&JobObject>,
) -> Result<i32, String> {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use windows::Win32::Foundation::HANDLE;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;

    let command = locate_executable(&cmd.command, &cmd.env, &cmd.cwd)?;

    let mut child = Command::new(&command)
        .args(&cmd.args)
        .envs(&cmd.env)
        .current_dir(&cmd.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map_err(|e| e.to_string())?;

    let child_pid = child.id();

    // Defence in depth: ensure the workload is in the helper's job
    // object. New processes inherit job membership from their parent on
    // Windows 8+, so this is normally already true. We re-assert it to
    // surface a clear error if anything ever flips that default (e.g. a
    // future `CREATE_BREAKAWAY_FROM_JOB` flag) and to make the test
    // `test_helper_crash_during_execution_windows` deterministic.
    if let Some(job) = job {
        let raw: HANDLE = HANDLE(child.as_raw_handle() as *mut _);
        if let Err(e) = job.assign_process(raw) {
            // Don't fail the run on this — log and continue. The workload
            // is still functional; we just lose the cleanup guarantee.
            eprintln!("openjd_helper: {e}");
        }
    }

    send(&Response::Pid { pid: child_pid });

    let child_stdout = child.stdout.take().unwrap();
    let child_stderr = child.stderr.take().unwrap();

    // Background threads frame child output into bounded lines, sent via a
    // bounded channel. The bound restores the backpressure the Unix runner
    // gets for free (frame + emit in one loop): when the parent drains our
    // stdout slowly, the queue fills, the reader threads park in `send`, and
    // the child blocks on its full pipe instead of growing host memory.
    let (out_tx, out_rx) = mpsc::sync_channel::<String>(OUT_QUEUE_LINES);

    let tx1 = out_tx.clone();
    let stdout_thread = std::thread::spawn(move || frame_child_output(child_stdout, tx1));

    let tx2 = out_tx.clone();
    let stderr_thread = std::thread::spawn(move || frame_child_output(child_stderr, tx2));

    drop(out_tx);

    // Drain output lines, checking for cancel between receives.
    let mut escalation_deadline: Option<std::time::Instant> = None;
    loop {
        // Check for cancel (non-blocking)
        if let Ok(method) = cancel_rx.try_recv() {
            escalation_deadline = handle_cancel(child_pid, &method);
        }

        // If a soft signal was sent and the grace window expired, escalate.
        if let Some(deadline) = escalation_deadline {
            if std::time::Instant::now() >= deadline {
                kill_process_tree(child_pid);
                escalation_deadline = None;
            }
        }

        // Read output with a short timeout so we can check cancel periodically
        match out_rx.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok(line) => {
                send(&Response::Out { out: line });
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Check if child has exited
                if let Ok(Some(_)) = child.try_wait() {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // Both stdout and stderr closed
                break;
            }
        }
    }

    // Drain until both reader threads finish and drop their senders. With the
    // bounded channel a `try_recv` pass followed by `join` could deadlock: a
    // reader parked in `send` on a full queue would never exit, and nothing
    // would drain. Blocking `recv` returns Err(Disconnected) only after both
    // threads have flushed their final (possibly newline-less) line via
    // `framer.finish()` and exited, so this delivers everything and makes the
    // joins below immediate. Its wait is bounded by the readers reaching EOF,
    // the same bound the joins already had.
    while let Ok(line) = out_rx.recv() {
        send(&Response::Out { out: line });
    }

    // Wait for child to exit
    let status = child.wait().map_err(|e| e.to_string())?;
    let exit_code = status.code().unwrap_or(-1);

    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

    Ok(exit_code)
}

/// Frame `reader` into bounded lines, one Out payload per line sent via `tx`.
///
/// Caps per-line memory at 64 KiB, escapes invalid UTF-8 instead of erroring
/// the thread, and flushes the trailing partial line at EOF. Aggregate memory
/// is bounded by the sync channel: `send` parks this thread when the queue is
/// full, so a slow consumer backpressures into the child's pipe instead of
/// growing the queue. Blocking 8 KiB reads on this dedicated thread are fine.
fn frame_child_output<R: Read>(mut reader: R, tx: mpsc::SyncSender<String>) {
    let mut framer = LineFramer::new();
    let mut buf = [0u8; 8 * 1024];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break, // EOF
            Ok(n) => n,
            // A signal can interrupt the read mid-call; retry rather than end the stream.
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break, // read error ends the thread, as lines() did
        };
        let mut lines = Vec::new();
        framer.push(&buf[..n], &mut |line| lines.push(to_payload(&line)));
        for line in lines {
            if tx.send(line).is_err() {
                return; // receiver dropped; stop without flushing
            }
        }
    }
    let mut lines = Vec::new();
    framer.finish(&mut |line| lines.push(to_payload(&line)));
    for line in lines {
        if tx.send(line).is_err() {
            return;
        }
    }
}

/// Turn one framed line into an `Out` payload. Strips only the CRLF's trailing
/// `\r` (the framer keeps it; only `\n` ends a line), not all trailing
/// whitespace like the Unix runner's `trim_end`, then caps the result at the
/// 128 KiB response limit via [`fit_out_payload`], matching Unix's bound.
fn to_payload(line: &str) -> String {
    let stripped = line.strip_suffix('\r').unwrap_or(line);
    fit_out_payload(stripped).to_string()
}

/// Handle a cancel request. Returns an escalation deadline for soft signals
/// (the caller must force-kill if the child is still alive after this instant).
fn handle_cancel(child_pid: u32, method: &CancelMethod) -> Option<std::time::Instant> {
    match method {
        CancelMethod::Terminate => {
            kill_process_tree(child_pid);
            None
        }
        CancelMethod::NotifyThenTerminate {
            notify_period_in_seconds,
        } => {
            // Platform-appropriate soft signal
            if !send_ctrl_break(child_pid) {
                // Couldn't even deliver the signal; kill immediately.
                kill_process_tree(child_pid);
                None
            } else {
                // Signal delivered — give the child the notify period to exit.
                Some(
                    std::time::Instant::now()
                        + std::time::Duration::from_secs(*notify_period_in_seconds),
                )
            }
        }
    }
}

/// Resolve `command` to an absolute path with canonical Windows search
/// semantics (PATHEXT-aware, earliest PATH directory wins), searching the
/// working directory first: `{cwd};{PATH}`.
///
/// Runs in the helper — i.e. **as the target user** — so it can resolve
/// executables in directories only the target user can read.
///
/// PATH and PATHEXT are each taken from exactly one source, chosen before
/// searching: a key in the action's env vars (case-insensitive) is used
/// exclusively — the helper's own value is never consulted then, even if
/// the search finds nothing. Only when the env map defines no such key at
/// all is the helper's own environment used (the target user's
/// environment block, inherited from `CreateEnvironmentBlock` at spawn).
/// This matches the `.envs()` merge at spawn — an action-supplied value
/// overwrites the inherited one — so resolution always searches with the
/// values the workload actually sees, never a union of the two.
///
/// Absolute paths pass through unchanged (the OS resolves the extension).
/// A not-found result is a hard error with the same message the Python
/// implementation raises — resolving here, before `Command::new`, prevents
/// `CreateProcessW`'s legacy fallback search (application directory, system
/// directories, the helper's own PATH lookup for `.exe` only).
fn locate_executable(
    command: &str,
    env: &std::collections::HashMap<String, String>,
    cwd: &str,
) -> Result<String, String> {
    if std::path::Path::new(command).is_absolute() {
        return Ok(command.to_string());
    }
    let env_var = |name: &str| -> Option<String> {
        env.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    };
    let path_var = env_var("PATH").unwrap_or_else(|| std::env::var("PATH").unwrap_or_default());
    let pathext =
        env_var("PATHEXT").unwrap_or_else(|| std::env::var("PATHEXT").unwrap_or_default());
    let search_path = format!("{cwd};{path_var}");
    match crate::win32_which::locate_in(command, &search_path, &pathext, std::path::Path::new(cwd))
    {
        Some(found) => Ok(found.to_string_lossy().into_owned()),
        None => Err(format!("Could not find executable file: {command}")),
    }
}

/// Send CTRL_BREAK_EVENT to a process.
fn send_ctrl_break(pid: u32) -> bool {
    use windows::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};
    unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid).is_ok() }
}

/// The current system wall clock as FILETIME ticks (100-ns intervals since
/// 1601-01-01 UTC), the epoch `GetProcessTimes` reports creation times in.
///
/// Same as `subprocess.rs`: derived from the standard-library system clock,
/// which reads the same UTC clock as `GetSystemTimeAsFileTime`, so no extra
/// `windows` feature is needed.
fn system_time_as_filetime_ticks() -> u64 {
    // 11_644_473_600 seconds separate the FILETIME epoch (1601) from the
    // Unix epoch (1970); 10_000_000 FILETIME ticks make one second.
    const SECS_1601_TO_1970: u64 = 11_644_473_600;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    now.as_secs()
        .saturating_add(SECS_1601_TO_1970)
        .saturating_mul(10_000_000)
        .saturating_add((now.subsec_nanos() / 100) as u64)
}

/// Snapshot all `(pid, parent_pid)` pairs in one toolhelp pass, with a
/// wall-clock stamp taken right after `CreateToolhelp32Snapshot` returns.
///
/// The snapshot freezes the process list when it is created, so the stamp
/// upper-bounds the creation time of every listed process.
/// `collect_tree_validated` rejects a descendant that reads newer than the
/// stamp: the mark of a PID reused after the snapshot.
fn snapshot_process_parents() -> (u64, Vec<(u32, u32)>) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut pairs = Vec::new();
    // Only the no-snapshot fallback keeps this 0 (and `pairs` is then empty).
    let mut stamp: u64 = 0;
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if let Ok(snap) = snap {
            stamp = system_time_as_filetime_ticks();
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(snap, &mut entry).is_ok() {
                loop {
                    pairs.push((entry.th32ProcessID, entry.th32ParentProcessID));
                    if Process32NextW(snap, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
    }
    (stamp, pairs)
}

/// Read a process's creation time, as FILETIME ticks, from an open handle
/// granting at least `PROCESS_QUERY_LIMITED_INFORMATION`. `None` if
/// `GetProcessTimes` fails.
fn creation_time_from_handle(handle: windows::Win32::Foundation::HANDLE) -> Option<u64> {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::GetProcessTimes;
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: `handle` is a live process handle from a successful
    // OpenProcess with query rights; the four out-params are writable
    // FILETIMEs on this frame, and only `creation` is read.
    let ok = unsafe {
        GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user).is_ok()
    };
    if !ok {
        return None;
    }
    Some(((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64)
}

/// Read the creation time of the process currently at `pid`. Fail closed:
/// any failure to open it or read its times returns `None`.
fn process_creation_time(pid: u32) -> Option<u64> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: OpenProcess returns a handle we own; it is closed on the only
    // path that has one. On failure `?` returns before a handle exists.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let ct = creation_time_from_handle(handle);
        let _ = CloseHandle(handle);
        ct
    }
}

/// Terminate `target` only if the process now at its PID still has the
/// creation time it was collected with. The identity read and the
/// `TerminateProcess` use one handle, so the PID can't be reused between the
/// check and the kill. Returns whether a terminate was issued.
///
/// Unlike `subprocess.rs`, a skipped kill isn't logged: the helper's stderr
/// shares the JSON response pipe, so a log line would break the protocol.
fn kill_process_checked(target: &ValidatedProcess) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };
    // SAFETY: OpenProcess returns a handle we own, with query and terminate
    // rights; it is closed on every path after a successful open.
    unsafe {
        let Ok(handle) = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
            false,
            target.pid,
        ) else {
            // Can't open, so can't validate: fail closed.
            return false;
        };
        let current = creation_time_from_handle(handle);
        let issued = process_identity_matches(target.creation_time, current)
            && TerminateProcess(handle, 1).is_ok();
        let _ = CloseHandle(handle);
        issued
    }
}

/// Kill a process tree: collect the descendants of `root_pid`, validating
/// every edge against creation times, then kill leaf-to-root.
///
/// Every caller holds the workload's live `Child`, whose handle keeps the
/// root's PID from being reused, so the root needs no pinned identity
/// (`expected_root_ct = None`), as for `subprocess.rs`'s immediate
/// `send_terminate`. `th32ParentProcessID` is never updated when a parent
/// exits and Windows reuses PIDs, so without the validation a live,
/// unrelated process whose dead creator's PID is now in the tree would be
/// killed.
fn kill_process_tree(root_pid: u32) {
    let (snapshot_time, parents) = snapshot_process_parents();
    let tree = collect_tree_validated(
        root_pid,
        &parents,
        snapshot_time,
        None,
        &mut process_creation_time,
    );
    // Breadth-first order lists every parent before its children, so the
    // reverse kills leaves before their ancestors. An empty tree means the
    // root's identity couldn't be read: kill nothing rather than risk a
    // reused PID.
    for target in tree.iter().rev() {
        kill_process_checked(target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framer::MAX_LINE_BYTES;

    /// Mirror the runner's per-line transform: frame `chunks` through a fresh
    /// `LineFramer` and apply `to_payload` to every emitted line, flushing the
    /// trailing partial at EOF.
    fn payloads(chunks: &[&[u8]]) -> Vec<String> {
        let mut framer = LineFramer::new();
        let mut out = Vec::new();
        for chunk in chunks {
            framer.push(chunk, &mut |line| out.push(to_payload(&line)));
        }
        framer.finish(&mut |line| out.push(to_payload(&line)));
        out
    }

    /// Drive the real `frame_child_output` over `input` (a `&[u8]` is a
    /// `Read`), returning the payloads its thread would send. Both the stdout
    /// and stderr threads call this one function, so exercising it once covers
    /// the framing both channels receive. Runs single-threaded, so the
    /// capacity must exceed the lines any one test emits (all emit < 10).
    fn frame_reader(input: &[u8]) -> Vec<String> {
        let (tx, rx) = mpsc::sync_channel::<String>(OUT_QUEUE_LINES);
        frame_child_output(input, tx);
        rx.into_iter().collect()
    }

    /// Guards CRLF stripping: the framer keeps the `\r` (only `\n` ends a
    /// line), so the runner must drop the single trailing `\r` of each line.
    #[test]
    fn crlf_stripped_from_payload() {
        assert_eq!(
            payloads(&[b"a\r\nb\r\n"]),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    /// Guards that a lone `\r` not at the line end (progress-bar carriage
    /// return) is preserved; only a single trailing `\r` is stripped.
    #[test]
    fn lone_cr_preserved_in_payload() {
        assert_eq!(
            payloads(&[b"p 10%\rp 20%\n"]),
            vec!["p 10%\rp 20%".to_string()]
        );
    }

    /// Guards a `\r` landing exactly on a chunk boundary (chunk ends `\r`,
    /// next chunk starts `\n`): it still frames as one `\r\n` line and strips
    /// to no CR.
    #[test]
    fn cr_on_chunk_boundary_stripped() {
        assert_eq!(payloads(&[b"x\r", b"\n"]), vec!["x".to_string()]);
    }

    /// Guards an empty line via `\r\n`: the framer emits `"\r"`, the runner
    /// strips it to an empty payload.
    #[test]
    fn empty_crlf_line_yields_empty_payload() {
        assert_eq!(payloads(&[b"\r\n"]), vec![String::new()]);
    }

    /// Guards invalid UTF-8 through the reader path: bytes are backslash-
    /// escaped, not errored, and the thread keeps emitting.
    #[test]
    fn invalid_utf8_escaped_not_errored() {
        assert_eq!(frame_reader(b"a\x97b\n"), vec![r"a\x97b".to_string()]);
    }

    /// Guards bounded output: a line over `MAX_LINE_BYTES` yields a single
    /// truncated payload (<= `MAX_LINE_BYTES`) through the Windows reader path.
    #[test]
    fn over_cap_line_truncated_through_reader() {
        let mut input = vec![b'a'; 1024 * 1024];
        input.push(b'\n');
        let out = frame_reader(&input);
        assert_eq!(out.len(), 1, "over-cap line must emit exactly one Out");
        assert!(out[0].len() <= MAX_LINE_BYTES);
    }

    /// The PIDs of `parent`'s children in a fresh snapshot whose image name is
    /// `exe` (case-insensitive).
    fn children_named(parent: u32, exe: &str) -> Vec<u32> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        let mut found = Vec::new();
        // SAFETY: the snapshot handle is ours and closed below; `entry` is a
        // correctly sized PROCESSENTRY32W on this frame.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).expect("snapshot");
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut more = Process32FirstW(snap, &mut entry).is_ok();
            while more {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(260);
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                if entry.th32ParentProcessID == parent && name.eq_ignore_ascii_case(exe) {
                    found.push(entry.th32ProcessID);
                }
                more = Process32NextW(snap, &mut entry).is_ok();
            }
            let _ = CloseHandle(snap);
        }
        found
    }

    /// Guards the real tree kill after the PID-reuse validation: the creation
    /// times, the snapshot stamp and the walk must agree on a genuine child
    /// and grandchild, or the validation would prune them and leak the tree.
    /// `cmd /c ping` gives cmd.exe as the child and PING.EXE as the
    /// grandchild; both must end with `TerminateProcess`'s exit code 1.
    #[test]
    fn kill_process_tree_kills_child_and_grandchild() {
        use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, WaitForSingleObject,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        };
        let mut child = Command::new("cmd")
            .args(["/c", "ping -n 60 127.0.0.1"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn cmd");
        let root = child.id();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let grandchild = loop {
            if let Some(&pid) = children_named(root, "PING.EXE").first() {
                break pid;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                panic!("ping never started under cmd (pid {root})");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        // Holding this handle keeps the grandchild's PID from being reused
        // before its exit code is read.
        // SAFETY: the handle is ours and closed below.
        let gc = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                grandchild,
            )
        }
        .expect("open ping");

        kill_process_tree(root);

        let status = child.wait().expect("wait cmd");
        // SAFETY: `gc` is a live handle with SYNCHRONIZE and query rights.
        let (waited, gc_code) = unsafe {
            let waited = WaitForSingleObject(gc, 10_000);
            let mut code = 0u32;
            let _ = GetExitCodeProcess(gc, &mut code);
            let _ = CloseHandle(gc);
            (waited, code)
        };
        assert_eq!(status.code(), Some(1), "cmd must be terminated");
        assert_eq!(waited, WAIT_OBJECT_0, "ping must have exited");
        assert_eq!(gc_code, 1, "ping must be terminated");
    }

    /// Guards stdout/stderr symmetry: both threads route through this one
    /// `frame_child_output`, so this shared-path check (lines split on `\n`,
    /// CRLF stripped, EOF partial flushed) is the framing both channels get.
    #[test]
    fn frame_child_output_shared_by_both_channels() {
        assert_eq!(
            frame_reader(b"one\r\ntwo\nthree"),
            vec!["one".to_string(), "two".to_string(), "three".to_string()]
        );
    }
}
