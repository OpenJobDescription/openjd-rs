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
use std::io::BufRead;
use std::process::{Command, Stdio};
use std::sync::mpsc;

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

    let mut child = Command::new(&cmd.command)
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

    // Background threads read child output and send lines via channel
    let (out_tx, out_rx) = mpsc::channel::<String>();

    let tx1 = out_tx.clone();
    let stdout_thread = std::thread::spawn(move || {
        let reader = std::io::BufReader::new(child_stdout);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    if tx1.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let tx2 = out_tx.clone();
    let stderr_thread = std::thread::spawn(move || {
        let reader = std::io::BufReader::new(child_stderr);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    if tx2.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

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

    // Drain any remaining output
    while let Ok(line) = out_rx.try_recv() {
        send(&Response::Out { out: line });
    }

    // Wait for child to exit
    let status = child.wait().map_err(|e| e.to_string())?;
    let exit_code = status.code().unwrap_or(-1);

    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

    Ok(exit_code)
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

/// Send CTRL_BREAK_EVENT to a process.
fn send_ctrl_break(pid: u32) -> bool {
    use windows::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};
    unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid).is_ok() }
}

/// Kill a single process by PID.
fn kill_process(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
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

/// Snapshot all `(pid, parent_pid)` pairs in one toolhelp pass.
fn snapshot_process_parents() -> Vec<(u32, u32)> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut pairs = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if let Ok(snap) = snap {
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
    pairs
}

/// Kill a process tree: collect all descendants, then kill leaf-to-root.
fn kill_process_tree(root_pid: u32) {
    // Breadth-first order lists every parent before its children, so the
    // reverse kills leaves before their ancestors.
    let to_kill = collect_tree(root_pid);
    for &pid in to_kill.iter().rev() {
        kill_process(pid);
    }
}

/// Collect the process tree rooted at `root_pid` in breadth-first order.
///
/// Walks a single toolhelp snapshot with a visited set instead of recursing
/// with a fresh snapshot per level: `th32ParentProcessID` is recorded at
/// child creation time, so PID reuse can make the recorded parent graph
/// cyclic, and the previous recursive implementation had no cycle guard and
/// overflowed the thread's stack (0xc00000fd) when it hit such a cycle.
fn collect_tree(root_pid: u32) -> Vec<u32> {
    use std::collections::HashSet;
    let parents = snapshot_process_parents();
    let mut result = vec![root_pid];
    let mut seen: HashSet<u32> = HashSet::from([root_pid]);
    let mut i = 0;
    while i < result.len() {
        let pid = result[i];
        i += 1;
        for &(child, ppid) in &parents {
            if ppid == pid && seen.insert(child) {
                result.push(child);
            }
        }
    }
    result
}
