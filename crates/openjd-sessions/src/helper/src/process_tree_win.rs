// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! The Windows calls around `process_tree`'s walk, for both Windows tree kills
//! (`openjd-sessions`' `subprocess.rs` and the helper's `runner_win.rs`).
//!
//! It logs nothing and reports what it did instead, because the helper can't
//! log. Both crates compile it as a sibling of `process_tree`, which it reaches
//! as `super::process_tree`. See specs/sessions/subprocess.md § Shared
//! implementation.

use super::process_tree::{collect_tree_validated, process_identity_matches, ValidatedProcess};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_TERMINATE,
};

/// What [`kill_process_tree`] did with one collected process. See
/// specs/sessions/subprocess.md § Kill-time revalidation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillOutcome {
    /// The identity matched and `TerminateProcess` succeeded.
    Terminated,
    /// The process couldn't be opened with query and terminate rights.
    OpenFailed,
    /// The process now at the PID has a different creation time.
    Mismatch,
    /// The process opened, but its creation time couldn't be read.
    Unreadable,
    /// The identity matched, but `TerminateProcess` failed.
    TerminateFailed,
}

/// The current system wall clock as FILETIME ticks (100-ns intervals since
/// 1601-01-01 UTC), the epoch `GetProcessTimes` reports creation times in.
/// See specs/sessions/subprocess.md § Snapshot and stamp.
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
/// See specs/sessions/subprocess.md § Snapshot and stamp.
fn snapshot_process_parents() -> (u64, Vec<(u32, u32)>) {
    let mut pairs = Vec::new();
    // Only the no-snapshot fallback keeps this 0 (and `pairs` is then empty).
    let mut stamp: u64 = 0;
    // SAFETY: the snapshot handle is ours and closed below; `entry` is a
    // correctly sized PROCESSENTRY32W on this frame.
    unsafe {
        if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            // Before the enumeration, so the stamp bounds the frozen list.
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
fn creation_time_from_handle(handle: HANDLE) -> Option<u64> {
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
pub fn process_creation_time(pid: u32) -> Option<u64> {
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
/// creation time it was collected with, checked and killed through one
/// handle. See specs/sessions/subprocess.md § Kill-time revalidation.
fn kill_process_checked(target: &ValidatedProcess) -> KillOutcome {
    // SAFETY: OpenProcess returns a handle we own, with query and terminate
    // rights; it is closed on every path after a successful open.
    unsafe {
        let Ok(handle) = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
            false,
            target.pid,
        ) else {
            // Can't open, so can't validate: fail closed.
            return KillOutcome::OpenFailed;
        };
        let current = creation_time_from_handle(handle);
        let outcome = if !process_identity_matches(target.creation_time, current) {
            if current.is_some() {
                KillOutcome::Mismatch
            } else {
                KillOutcome::Unreadable
            }
        } else if TerminateProcess(handle, 1).is_ok() {
            KillOutcome::Terminated
        } else {
            KillOutcome::TerminateFailed
        };
        let _ = CloseHandle(handle);
        outcome
    }
}

/// Kill a process tree: collect the descendants of `root_pid`, validating
/// every edge against creation times, then kill leaf-to-root.
/// `expected_root_ct` is the pinned root identity, or `None` when the caller's
/// live `Child` pins the PID.
///
/// Returns each collected PID with what was done to it, in kill order. An
/// empty result means the root's identity couldn't be established, so nothing
/// was killed. See specs/sessions/subprocess.md § Process Tree Killing.
pub fn kill_process_tree(root_pid: u32, expected_root_ct: Option<u64>) -> Vec<(u32, KillOutcome)> {
    let (snapshot_time, parents) = snapshot_process_parents();
    let tree = collect_tree_validated(
        root_pid,
        &parents,
        snapshot_time,
        expected_root_ct,
        &mut process_creation_time,
    );
    // Reverse breadth-first order kills leaves first.
    tree.iter()
        .rev()
        .map(|target| (target.pid, kill_process_checked(target)))
        .collect()
}
