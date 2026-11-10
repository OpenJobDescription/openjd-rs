// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Find the processes holding files open, via the Windows Restart Manager.
//!
//! Used only to explain a failed cleanup in the log, so every failure here is
//! reported to the caller as a string and never stops the cleanup.

use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, ERROR_MORE_DATA, ERROR_SUCCESS};
use windows::Win32::System::RestartManager::{
    RmEndSession, RmGetList, RmRegisterResources, RmStartSession, CCH_RM_SESSION_KEY,
    RM_PROCESS_INFO,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::tempdir::FileHolder;

/// Returns the processes that hold any of `files` open.
pub(crate) fn processes_holding(files: &[PathBuf]) -> Result<Vec<FileHolder>, String> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let wide: Vec<Vec<u16>> = files
        .iter()
        .map(|p| p.as_os_str().encode_wide().chain(Some(0)).collect())
        .collect();
    let names: Vec<PCWSTR> = wide.iter().map(|w| PCWSTR(w.as_ptr())).collect();

    let mut handle = 0u32;
    let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];
    // SAFETY: `handle` and `key` outlive the call; `key` has the documented
    // CCH_RM_SESSION_KEY + 1 capacity.
    let rc = unsafe { RmStartSession(&mut handle, None, PWSTR(key.as_mut_ptr())) };
    if rc != ERROR_SUCCESS {
        return Err(format!("RmStartSession failed ({})", rc.0));
    }
    let result = list_holders(handle, &names);
    // SAFETY: `handle` came from a successful RmStartSession.
    unsafe {
        let _ = RmEndSession(handle);
    }
    result
}

fn list_holders(handle: u32, names: &[PCWSTR]) -> Result<Vec<FileHolder>, String> {
    // SAFETY: `names` points into NUL-terminated buffers owned by the caller.
    let rc = unsafe { RmRegisterResources(handle, Some(names), None, None) };
    if rc != ERROR_SUCCESS {
        return Err(format!("RmRegisterResources failed ({})", rc.0));
    }
    let mut infos: Vec<RM_PROCESS_INFO> = Vec::new();
    // The list can grow between calls, so ask again a few times on ERROR_MORE_DATA.
    for _ in 0..3 {
        let mut needed = 0u32;
        let mut count = infos.len() as u32;
        let mut reasons = 0u32;
        let buf = if infos.is_empty() {
            None
        } else {
            Some(infos.as_mut_ptr())
        };
        // SAFETY: `buf` is null or points to `count` initialized entries.
        let rc = unsafe { RmGetList(handle, &mut needed, &mut count, buf, &mut reasons) };
        if rc == ERROR_SUCCESS {
            infos.truncate(count as usize);
            return Ok(infos.iter().map(|i| holder_from(i, image_name)).collect());
        }
        if rc != ERROR_MORE_DATA {
            return Err(format!("RmGetList failed ({})", rc.0));
        }
        infos = vec![RM_PROCESS_INFO::default(); needed as usize];
    }
    Err("RmGetList kept reporting more processes".into())
}

/// The holder named by `info`: the executable's file name from `image_name`,
/// or the Restart Manager's application name when that lookup fails.
fn holder_from(info: &RM_PROCESS_INFO, image_name: impl Fn(u32) -> Option<String>) -> FileHolder {
    let pid = info.Process.dwProcessId;
    let name = image_name(pid).unwrap_or_else(|| {
        let end = info
            .strAppName
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(info.strAppName.len());
        String::from_utf16_lossy(&info.strAppName[..end])
    });
    FileHolder { pid, name }
}

/// The executable's file name, such as `UnrealShaderCompileWorker.exe`.
fn image_name(pid: u32) -> Option<String> {
    // SAFETY: OpenProcess with a query-only right; the handle is closed below.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` holds `len` u16s and outlives the call.
    let ok = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    };
    // SAFETY: `process` is a valid handle from OpenProcess.
    unsafe {
        let _ = CloseHandle(process);
    }
    ok.ok()?;
    let path = PathBuf::from(String::from_utf16_lossy(&buf[..len as usize]));
    path.file_name().map(|n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;

    #[test]
    fn finds_this_process_holding_a_file() {
        let dir = crate::tempdir::TempDir::new(None, None, None).unwrap();
        let path = dir.path().join("held.bin");
        let _held = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let holders = processes_holding(&[path]).unwrap();
        assert!(
            holders.contains(&FileHolder {
                pid: std::process::id(),
                name: this_exe_name(),
            }),
            "expected this process, by pid and executable name, in {holders:?}"
        );
    }

    fn this_exe_name() -> String {
        std::env::current_exe()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn image_name_is_the_executable_file_name() {
        assert_eq!(image_name(std::process::id()), Some(this_exe_name()));
        // pid 0 is the System Idle Process, which cannot be opened.
        assert_eq!(image_name(0), None);
    }

    fn info_named(pid: u32, app_name: &str) -> RM_PROCESS_INFO {
        let mut info = RM_PROCESS_INFO::default();
        info.Process.dwProcessId = pid;
        for (slot, c) in info.strAppName.iter_mut().zip(app_name.encode_utf16()) {
            *slot = c;
        }
        info
    }

    #[test]
    fn holder_prefers_the_image_name() {
        let h = holder_from(&info_named(7, "Perforce"), |pid| {
            assert_eq!(pid, 7);
            Some("p4.exe".into())
        });
        assert_eq!(
            h,
            FileHolder {
                pid: 7,
                name: "p4.exe".into()
            }
        );
    }

    #[test]
    fn holder_falls_back_to_the_restart_manager_app_name() {
        let h = holder_from(&info_named(7, "Perforce Helix"), |_| None);
        assert_eq!(
            h,
            FileHolder {
                pid: 7,
                name: "Perforce Helix".into()
            }
        );
    }

    #[test]
    fn holder_app_name_uses_the_whole_buffer_when_unterminated() {
        let full = "x".repeat(256);
        let h = holder_from(&info_named(7, &full), |_| None);
        assert_eq!(h.name, full);
    }

    #[test]
    fn a_file_nobody_holds_has_no_holders() {
        let dir = crate::tempdir::TempDir::new(None, None, None).unwrap();
        let path = dir.path().join("free.bin");
        std::fs::write(&path, b"x").unwrap();
        assert!(processes_holding(&[path]).unwrap().is_empty());
    }
}
