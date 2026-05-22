// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Windows Job Object guard for the helper process tree.
//!
//! Wraps the helper and any workload it spawns in a single Job Object with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. When the helper process dies — for
//! any reason, including `TerminateProcess` from the parent session, an
//! abnormal exit, or the OS killing it for a memory limit — the kernel
//! closes the helper's last handle to the job, which fires `KILL_ON_JOB_CLOSE`
//! and terminates every process associated with the job.
//!
//! ## Why this is needed
//!
//! `TerminateProcess` on Windows does **not** propagate to descendants.
//! Without a Job Object, a workload spawned by the helper survives the
//! helper's death as an orphan, holding inherited handles to the helper's
//! pipes. In CI, an orphan with a duplicated handle to the test runner's
//! output pipe will keep the runner alive long after the test reported
//! success, eventually causing GitHub Actions' watchdog to declare the
//! runner lost.
//!
//! ## Process-tree semantics
//!
//! Windows automatically associates a child process with every Job Object
//! its parent belongs to (since Windows 8, jobs can be nested). The helper
//! puts **itself** in the job at startup, which means every workload it
//! later spawns inherits the job by default — no per-spawn assignment is
//! needed. We still call `AssignProcessToJobObject` explicitly on each
//! workload as a defence-in-depth measure (idempotent on jobs that
//! already contain the process).
//!
//! ## Failure mode
//!
//! If `setup()` fails (e.g. a sandboxed environment that forbids job
//! object creation), we log a warning to stderr and continue without the
//! guard. The helper still works; it just leaks descendants on crash the
//! way it did before this module existed. Returning an error from `setup`
//! and falling back gracefully is preferable to refusing to start the
//! helper, since the workload-execution path itself doesn't depend on the
//! job object.

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::GetCurrentProcess;

/// A Windows Job Object configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
///
/// The wrapper owns the handle. When the value is dropped (or the process
/// exits and the kernel closes its handles), all processes in the job are
/// terminated by the OS.
pub struct JobObject {
    handle: OwnedHandle,
}

impl JobObject {
    /// Create a new job object, place the current process in it, and
    /// configure it to kill all members on close.
    ///
    /// Returns `None` if any of the underlying Win32 calls fail. On failure,
    /// a one-line warning is printed to stderr; the helper continues
    /// without a job-object guard.
    pub fn setup_for_current_process() -> Option<Self> {
        match Self::try_setup_for_current_process() {
            Ok(j) => Some(j),
            Err(e) => {
                eprintln!("openjd_helper: {e}; continuing without process-tree guard");
                None
            }
        }
    }

    /// Inner constructor that returns a structured error. The public
    /// [`setup_for_current_process`] turns failure into a logged warning
    /// plus `None`; this is split out so the FFI plumbing is only mixed
    /// with `?`-propagated errors, not with logging.
    fn try_setup_for_current_process() -> Result<Self, String> {
        let handle = create_job_object()?;
        set_kill_on_job_close(&handle)?;
        assign_process_to_job(&handle, current_process_handle())?;
        Ok(Self { handle })
    }

    /// Explicitly add a process to the job. This is redundant when the
    /// helper itself is already in the job (Windows associates children
    /// automatically), but is cheap and provides defence in depth in case
    /// a workload is launched with `CREATE_BREAKAWAY_FROM_JOB` for any
    /// reason in the future.
    ///
    /// `process_handle` must be a valid HANDLE for a running process the
    /// caller has the necessary access rights to.
    pub fn assign_process(&self, process_handle: HANDLE) -> Result<(), String> {
        assign_process_to_job(&self.handle, process_handle)
    }
}

// ────────────────────────────────────────────────────────────────────
// Win32 wrappers. Each helper has the smallest possible `unsafe` block
// and its own SAFETY comment, rather than one big `unsafe` covering the
// entire constructor.
// ────────────────────────────────────────────────────────────────────

/// Create an unnamed job object and wrap it in an [`OwnedHandle`].
fn create_job_object() -> Result<OwnedHandle, String> {
    // SAFETY: `CreateJobObjectW` with both arguments null is the documented
    // "create a fresh, unnamed job object with default security" form. The
    // returned handle, when valid, is exclusively owned by us until we
    // close it, which matches `OwnedHandle`'s invariant.
    let raw: HANDLE = unsafe { CreateJobObjectW(None, None) }
        .map_err(|e| format!("CreateJobObjectW failed ({e:?})"))?;

    if raw.is_invalid() {
        return Err("CreateJobObjectW returned an invalid handle".into());
    }

    // SAFETY: `raw` came from a successful `CreateJobObjectW`, so it is a
    // valid, freshly-allocated, exclusively-owned kernel handle — exactly
    // what `OwnedHandle::from_raw_handle` requires.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw.0 as *mut _) })
}

/// Configure the job to terminate all members when its last handle closes.
fn set_kill_on_job_close(handle: &OwnedHandle) -> Result<(), String> {
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

    // SAFETY: `handle` is a borrowed, valid job-object handle. `info` lives
    // through the call and is the correct type/size for
    // `JobObjectExtendedLimitInformation`.
    unsafe {
        SetInformationJobObject(
            borrow_handle(handle),
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    }
    .map_err(|e| format!("SetInformationJobObject failed ({e:?})"))
}

/// Place `process` into the job represented by `job_handle`.
fn assign_process_to_job(job_handle: &OwnedHandle, process: HANDLE) -> Result<(), String> {
    // SAFETY: `job_handle` is a borrowed, valid job-object handle.
    // `process` is provided by the caller as a valid process HANDLE; we
    // do not close it. `AssignProcessToJobObject` does not retain the
    // process handle past the call.
    unsafe { AssignProcessToJobObject(borrow_handle(job_handle), process) }
        .map_err(|e| format!("AssignProcessToJobObject failed ({e:?})"))
}

/// Pseudo-handle for the current process. Win32 returns a constant value
/// (`-1`) here, never an error, so this is genuinely safe to call.
fn current_process_handle() -> HANDLE {
    // SAFETY: `GetCurrentProcess` has no preconditions and never fails;
    // the returned pseudo-handle is valid for the lifetime of the process
    // and does not need to be closed.
    unsafe { GetCurrentProcess() }
}

/// Borrow an [`OwnedHandle`] as a Win32 [`HANDLE`] for the duration of an
/// FFI call.
///
/// The returned `HANDLE` is a non-owning view: it must not outlive `h` and
/// the caller must not close it.
fn borrow_handle(h: &OwnedHandle) -> HANDLE {
    HANDLE(h.as_raw_handle() as *mut _)
}
