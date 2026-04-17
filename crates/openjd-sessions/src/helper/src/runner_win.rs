// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Windows implementation of the helper runner.
//!
//! Spawns a child process with CREATE_NEW_PROCESS_GROUP, reads its stdout
//! on background threads, and handles cancel commands via a channel from main.

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
pub fn run_command(
    cmd: &RunCommand,
    cancel_rx: &mpsc::Receiver<CancelMethod>,
) -> Result<i32, String> {
    use std::os::windows::process::CommandExt;
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

/// Get child PIDs of a process.
fn get_child_pids(parent_pid: u32) -> Vec<u32> {
    use windows::Win32::Foundation::CloseHandle;
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
fn kill_process_tree(root_pid: u32) {
    let mut to_kill = Vec::new();
    collect_tree(root_pid, &mut to_kill);
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
