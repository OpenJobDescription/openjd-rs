// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Win32 API helpers for Windows session management.
//!
//! Mirrors Python `_win32/_helpers.py` and `_win32/_api.py`.

use std::collections::HashMap;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::Authentication::Identity::{GetUserNameExW, NameSamCompatible};
use windows::Win32::Security::{LogonUserW, LOGON32_LOGON_INTERACTIVE, LOGON32_PROVIDER_DEFAULT};
use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::GetCurrentProcessId;

/// Returns the current process user in SAM-compatible format (DOMAIN\user).
pub fn get_process_user() -> Result<String, windows::core::Error> {
    let mut size = 0u32;
    // First call to get required buffer size
    unsafe {
        let _ = GetUserNameExW(NameSamCompatible, None, &mut size);
    }
    if size == 0 {
        return Err(windows::core::Error::from_thread());
    }
    let mut buf = vec![0u16; size as usize];
    unsafe {
        if !GetUserNameExW(NameSamCompatible, Some(PWSTR(buf.as_mut_ptr())), &mut size) {
            return Err(windows::core::Error::from_thread());
        }
    }
    Ok(String::from_utf16_lossy(&buf[..size as usize]))
}

/// Returns the Windows Session ID of the current process.
pub fn get_current_process_session_id() -> u32 {
    let mut session_id = 0u32;
    unsafe {
        let pid = GetCurrentProcessId();
        let _ = ProcessIdToSessionId(pid, &mut session_id);
    }
    session_id
}

/// Returns true if the current process is running in Windows Session 0
/// (i.e. as a service or via SSH).
pub fn is_session_zero() -> bool {
    get_current_process_session_id() == 0
}

/// Logon token wrapper that closes the handle on drop.
#[derive(Debug)]
pub struct LogonToken {
    handle: HANDLE,
}

impl LogonToken {
    pub fn as_handle(&self) -> HANDLE {
        self.handle
    }
}

impl Drop for LogonToken {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

/// Attempt to log on as the given user with a password.
///
/// Returns a `LogonToken` that closes the handle on drop.
pub fn logon_user(username: &str, password: &str) -> Result<LogonToken, windows::core::Error> {
    let username_w: Vec<u16> = username.encode_utf16().chain(std::iter::once(0)).collect();
    let password_w: Vec<u16> = password.encode_utf16().chain(std::iter::once(0)).collect();
    let mut token = HANDLE::default();

    unsafe {
        LogonUserW(
            PCWSTR(username_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR(password_w.as_ptr()),
            LOGON32_LOGON_INTERACTIVE,
            LOGON32_PROVIDER_DEFAULT,
            &mut token,
        )?;
    }

    Ok(LogonToken { handle: token })
}

/// Create an environment block for a logon token and return it as a HashMap.
pub fn environment_for_user(
    token: HANDLE,
) -> Result<HashMap<String, String>, windows::core::Error> {
    let mut block_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
    unsafe {
        CreateEnvironmentBlock(&mut block_ptr, Some(token), false)?;
    }

    let env = parse_environment_block(block_ptr);

    unsafe {
        let _ = DestroyEnvironmentBlock(block_ptr);
    }

    Ok(env)
}

/// Parse a Win32 environment block (null-delimited, double-null terminated).
fn parse_environment_block(block: *mut std::ffi::c_void) -> HashMap<String, String> {
    let mut env = HashMap::new();
    let mut ptr = block as *const u16;

    unsafe {
        loop {
            let start = ptr;
            let mut len = 0usize;
            while *ptr != 0 {
                ptr = ptr.add(1);
                len += 1;
            }
            if len == 0 {
                break;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(start, len));
            if let Some((key, val)) = s.split_once('=') {
                env.insert(key.to_string(), val.to_string());
            }
            ptr = ptr.add(1);
        }
    }

    env
}

/// Build a null-delimited, double-null-terminated environment block from a HashMap.
///
/// Suitable for `CreateProcessAsUserW` / `CreateProcessWithLogonW`
/// with `CREATE_UNICODE_ENVIRONMENT`.
pub fn environment_block_from_map(env: &HashMap<String, String>) -> Vec<u16> {
    let mut block = Vec::new();
    for (key, val) in env {
        let entry: Vec<u16> = format!("{key}={val}").encode_utf16().collect();
        block.extend_from_slice(&entry);
        block.push(0);
    }
    block.push(0);
    block
}

// ---------------------------------------------------------------------------
// Cross-user process spawning
// ---------------------------------------------------------------------------

use windows::Win32::Foundation::{SetHandleInformation, HANDLE_FLAGS, HANDLE_FLAG_INHERIT};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CreateProcessAsUserW, CreateProcessWithLogonW, CREATE_NEW_PROCESS_GROUP,
    CREATE_UNICODE_ENVIRONMENT, LOGON_WITH_PROFILE, PROCESS_INFORMATION, STARTUPINFOW,
    STARTUPINFOW_FLAGS,
};

/// Result of spawning a cross-user process.
pub struct SpawnedProcess {
    pub process_handle: HANDLE,
    pub pid: u32,
    /// Read end of the stdout pipe. Caller owns this handle.
    pub stdout_read: std::os::windows::io::OwnedHandle,
}

/// Merge the user's environment block with additional env vars.
///
/// All keys are uppercased for Windows case-insensitivity.
/// Entries with `None` values are removed.
fn merge_environment(
    token: HANDLE,
    extra: &HashMap<String, Option<String>>,
) -> Result<Vec<u16>, String> {
    let user_env =
        environment_for_user(token).map_err(|e| format!("CreateEnvironmentBlock failed: {e}"))?;

    let mut merged: HashMap<String, String> = user_env
        .into_iter()
        .map(|(k, v)| (k.to_uppercase(), v))
        .collect();

    for (k, v) in extra {
        match v {
            Some(val) => {
                merged.insert(k.to_uppercase(), val.clone());
            }
            None => {
                merged.remove(&k.to_uppercase());
            }
        }
    }

    Ok(environment_block_from_map(&merged))
}

/// Create an inheritable pipe, returning (read_handle, write_handle).
fn create_stdout_pipe() -> Result<(HANDLE, HANDLE), String> {
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: true.into(),
    };
    let mut read_handle = HANDLE::default();
    let mut write_handle = HANDLE::default();

    unsafe {
        CreatePipe(&mut read_handle, &mut write_handle, Some(&sa), 0)
            .map_err(|e| format!("CreatePipe failed: {e}"))?;
        // The read end should NOT be inherited by the child
        SetHandleInformation(read_handle, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0))
            .map_err(|e| format!("SetHandleInformation failed: {e}"))?;
    }

    Ok((read_handle, write_handle))
}

/// Spawn a process as another user.
///
/// Uses `CreateProcessWithLogonW` when a password is provided, or
/// `CreateProcessAsUserW` when a logon token is provided.
pub fn spawn_as_user(
    args: &[String],
    env_vars: &HashMap<String, Option<String>>,
    working_dir: Option<&std::path::Path>,
    password: Option<&str>,
    username: &str,
    logon_token: Option<HANDLE>,
) -> Result<SpawnedProcess, String> {
    use std::os::windows::io::FromRawHandle;

    let (stdout_read, stdout_write) = create_stdout_pipe()?;

    // Build command line
    let cmdline_str = args_to_cmdline(args);
    let mut cmdline: Vec<u16> = cmdline_str
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let cwd = working_dir.map(|d| {
        let s: Vec<u16> = d
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        s
    });
    let cwd_ptr = cwd
        .as_ref()
        .map(|s| PCWSTR(s.as_ptr()))
        .unwrap_or(PCWSTR::null());

    // STARTUPINFOW: redirect stdout+stderr to our pipe
    let si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        dwFlags: STARTUPINFOW_FLAGS(0x00000100 | 0x00000001), // STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW
        wShowWindow: 0,                                       // SW_HIDE
        hStdOutput: stdout_write,
        hStdError: stdout_write, // merge stderr into stdout
        hStdInput: HANDLE::default(),
        ..Default::default()
    };

    let mut pi = PROCESS_INFORMATION::default();
    let creation_flags = CREATE_NEW_PROCESS_GROUP | CREATE_UNICODE_ENVIRONMENT;

    let result = if let Some(pw) = password {
        let user_w: Vec<u16> = username.encode_utf16().chain(std::iter::once(0)).collect();
        let pw_w: Vec<u16> = pw.encode_utf16().chain(std::iter::once(0)).collect();

        // For password path, logon to get env block, then call CreateProcessWithLogonW
        let token = logon_user(username, pw).map_err(|e| format!("LogonUser failed: {e}"))?;
        let mut env_block = merge_environment(token.as_handle(), env_vars)?;

        unsafe {
            CreateProcessWithLogonW(
                PCWSTR(user_w.as_ptr()),
                PCWSTR::null(), // domain
                PCWSTR(pw_w.as_ptr()),
                LOGON_WITH_PROFILE,
                PCWSTR::null(), // application name
                Some(PWSTR(cmdline.as_mut_ptr())),
                creation_flags,
                Some(env_block.as_mut_ptr() as *const std::ffi::c_void),
                cwd_ptr,
                &si,
                &mut pi,
            )
        }
    } else if let Some(token) = logon_token {
        let mut env_block = merge_environment(token, env_vars)?;

        unsafe {
            CreateProcessAsUserW(
                Some(token),
                PCWSTR::null(),
                Some(PWSTR(cmdline.as_mut_ptr())),
                None, // process security attributes
                None, // thread security attributes
                true, // inherit handles
                creation_flags,
                Some(env_block.as_mut_ptr() as *const std::ffi::c_void),
                cwd_ptr,
                &si,
                &mut pi,
            )
        }
    } else {
        return Err("Must provide either password or logon_token".into());
    };

    // Close the write end of the pipe (child has it now)
    unsafe {
        let _ = CloseHandle(stdout_write);
    }

    result.map_err(|e| format!("CreateProcess failed: {e}"))?;

    // Close the thread handle (we only need the process handle)
    unsafe {
        let _ = CloseHandle(pi.hThread);
    }

    let stdout_owned = unsafe {
        std::os::windows::io::OwnedHandle::from_raw_handle(
            stdout_read.0 as std::os::windows::io::RawHandle,
        )
    };

    Ok(SpawnedProcess {
        process_handle: pi.hProcess,
        pid: pi.dwProcessId,
        stdout_read: stdout_owned,
    })
}

/// Convert args to a Windows command line string.
fn args_to_cmdline(args: &[String]) -> String {
    // Use the same algorithm as Rust's std::process::Command on Windows
    let mut cmdline = String::new();
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            cmdline.push(' ');
        }
        append_arg(&mut cmdline, arg);
    }
    cmdline
}

/// Append a single argument to a command line, quoting as needed.
/// Follows the Windows command-line escaping convention.
fn append_arg(cmdline: &mut String, arg: &str) {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        cmdline.push_str(arg);
        return;
    }
    cmdline.push('"');
    let mut backslashes = 0usize;
    for c in arg.chars() {
        if c == '\\' {
            backslashes += 1;
        } else {
            if c == '"' {
                for _ in 0..=backslashes {
                    cmdline.push('\\');
                }
            }
            backslashes = 0;
        }
        cmdline.push(c);
    }
    for _ in 0..backslashes {
        cmdline.push('\\');
    }
    cmdline.push('"');
}
