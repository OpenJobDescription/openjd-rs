// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Windows executable resolution behavior through the public `Session` API.
//!
//! These tests pin the Windows PATH-search semantics that mirror the Python
//! implementation's `_win32/_locate_executable.py`, which resolves the
//! command with `shutil.which` over `working_dir;PATH` before every action:
//!
//! 1. PATHEXT-aware, earliest-directory-wins search: a `.bat` in an earlier
//!    PATH directory must win over an `.exe` in a later one (Rust's
//!    `std::process::Command` alone only appends `.exe` and would pick the
//!    later `.exe`).
//! 2. No fallback to the worker process's own PATH or `CreateProcessW`'s
//!    legacy search: a command absent from the action's PATH must fail with
//!    "Could not find executable file", not silently resolve via the parent
//!    environment, application directory, or system directories.
//! 3. The session working directory is searched first, so a bare name
//!    materialized into the working directory resolves.

#![cfg(windows)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use openjd_sessions::{ActionState, Session, SessionConfig, SessionState};

/// Create a session with stdout collection enabled and no session user.
fn make_session(root: PathBuf) -> Session {
    let config = SessionConfig {
        session_id: "win32-locate-test".into(),
        job_parameter_values: HashMap::new(),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: None,
        os_env_vars: None,
        session_root_directory: Some(root),
        user: None,
        profile: None,
        cancel_token: None,
        debug_collect_stdout: true,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    };
    Session::with_config(config).unwrap()
}

/// Write a `.bat` file that prints `marker` (CRLF line endings).
fn write_bat(path: &Path, marker: &str) {
    std::fs::write(path, format!("@echo off\r\necho {marker}\r\n")).unwrap();
}

/// Copy `whoami.exe` (a small, non-interactive, always-present system
/// executable) to `dest` so a real runnable `.exe` exists under that name.
fn copy_system_exe(dest: &Path) {
    let system32 = PathBuf::from(std::env::var("SYSTEMROOT").unwrap()).join("System32");
    std::fs::copy(system32.join("whoami.exe"), dest).unwrap();
}

/// PATH value for the action's environment.
fn path_env(dirs: &[&Path]) -> HashMap<String, String> {
    let joined = dirs
        .iter()
        .map(|d| d.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(";");
    HashMap::from([("PATH".to_string(), joined)])
}

/// Finding 1: a `.bat` in an earlier PATH directory must beat an `.exe` of
/// the same basename in a later PATH directory (Windows canonical search
/// order, as used by cmd.exe, where.exe, and Python's shutil.which).
#[tokio::test(flavor = "multi_thread")]
async fn test_bat_earlier_in_path_beats_exe_later() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir_a = tmp.path().join("dirA");
    let dir_b = tmp.path().join("dirB");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    write_bat(&dir_a.join("pick.bat"), "RESOLVED-BAT-A");
    copy_system_exe(&dir_b.join("pick.exe"));

    let root = tmp.path().join("session");
    std::fs::create_dir_all(&root).unwrap();
    let mut session = make_session(root);
    let env = path_env(&[&dir_a, &dir_b]);
    let r = session
        .run_subprocess("pick", None, None, Some(&env), false, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    assert!(
        r.stdout.contains("RESOLVED-BAT-A"),
        "expected the .bat in the earlier PATH directory to win; stdout: {}",
        r.stdout
    );
    session.cleanup();
}

/// Finding 1 corollary: a bare name whose only match is a `.bat` on the
/// action's PATH must resolve (std::process::Command alone only tries
/// `.exe` and reports "program not found").
#[tokio::test(flavor = "multi_thread")]
async fn test_bare_name_resolves_bat_only_match() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir_a = tmp.path().join("dirA");
    std::fs::create_dir_all(&dir_a).unwrap();
    write_bat(&dir_a.join("onlybat.bat"), "ONLY-BAT");

    let root = tmp.path().join("session");
    std::fs::create_dir_all(&root).unwrap();
    let mut session = make_session(root);
    let env = path_env(&[&dir_a]);
    let r = session
        .run_subprocess("onlybat", None, None, Some(&env), false, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    assert!(
        r.stdout.contains("ONLY-BAT"),
        "expected the .bat-only match to resolve; stdout: {}",
        r.stdout
    );
    session.cleanup();
}

/// A `.bat`/`.cmd` invoked with an argument containing a newline must fail
/// with a spawn error, not run with a truncated argument. Rust's std
/// refuses to spawn a batch file with arguments it cannot safely escape
/// for cmd.exe (the BatBadBut mitigation, CVE-2024-24576); cmd.exe has no
/// escape for an embedded newline and would silently truncate the argument
/// there. This pins that the refusal (a) still happens now that resolution
/// finds `.bat` files, and (b) surfaces as a clear SubprocessStart error.
#[tokio::test(flavor = "multi_thread")]
async fn test_bat_with_newline_arg_fails_to_spawn() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir_a = tmp.path().join("dirA");
    std::fs::create_dir_all(&dir_a).unwrap();
    // The .bat would echo its first argument if it ever ran.
    std::fs::write(dir_a.join("nltool.bat"), "@echo off\r\necho %1\r\n").unwrap();

    let root = tmp.path().join("session");
    std::fs::create_dir_all(&root).unwrap();
    let mut session = make_session(root);
    let env = path_env(&[&dir_a]);
    let err = session
        .run_subprocess(
            "nltool",
            Some(&["line one\nline two".to_string()]),
            None,
            Some(&env),
            false,
            None,
        )
        .await
        .expect_err("a newline argument to a .bat must be rejected, not truncated");
    let msg = err.to_string();
    assert!(
        msg.contains("batch file arguments are invalid"),
        "expected std's batch-argument rejection; got: {msg}"
    );
    session.cleanup();
}

/// Finding 2: a command that is not on the action's PATH must fail with
/// "Could not find executable file" (matching the Python implementation),
/// not silently resolve through the worker process's own PATH or
/// CreateProcessW's legacy search (application directory, system
/// directories). `whoami` exists in System32 and on the parent's PATH, so
/// success here would prove the fallback leak.
#[tokio::test(flavor = "multi_thread")]
async fn test_command_absent_from_action_path_fails() {
    let tmp = tempfile::TempDir::new().unwrap();
    let empty_dir = tmp.path().join("empty");
    std::fs::create_dir_all(&empty_dir).unwrap();

    let root = tmp.path().join("session");
    std::fs::create_dir_all(&root).unwrap();
    let mut session = make_session(root);
    let env = path_env(&[&empty_dir]);
    let err = session
        .run_subprocess("whoami", None, None, Some(&env), false, None)
        .await
        .expect_err("whoami is not on the action's PATH and must not resolve via fallback search");
    assert!(
        err.to_string()
            .contains("Could not find executable file: whoami"),
        "expected Python-parity not-found error; got: {err}"
    );
    session.cleanup();
}

/// Finding 3: the session working directory is searched (first), matching
/// Python's `working_dir;PATH` search string — a script materialized into
/// the working directory is runnable by bare name.
#[tokio::test(flavor = "multi_thread")]
async fn test_working_directory_is_searched() {
    let tmp = tempfile::TempDir::new().unwrap();
    let empty_dir = tmp.path().join("empty");
    std::fs::create_dir_all(&empty_dir).unwrap();

    let root = tmp.path().join("session");
    std::fs::create_dir_all(&root).unwrap();
    let mut session = make_session(root);
    write_bat(
        &session.working_directory().join("wdtool.bat"),
        "WORKING-DIR-BAT",
    );
    let env = path_env(&[&empty_dir]);
    let r = session
        .run_subprocess("wdtool", None, None, Some(&env), false, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    assert!(
        r.stdout.contains("WORKING-DIR-BAT"),
        "expected the working-directory .bat to resolve; stdout: {}",
        r.stdout
    );
    session.cleanup();
}

/// The working directory wins over PATH when both contain a match,
/// because it is prepended to the search path (Python parity).
#[tokio::test(flavor = "multi_thread")]
async fn test_working_directory_wins_over_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir_a = tmp.path().join("dirA");
    std::fs::create_dir_all(&dir_a).unwrap();
    write_bat(&dir_a.join("dup.bat"), "FROM-PATH-DIR");

    let root = tmp.path().join("session");
    std::fs::create_dir_all(&root).unwrap();
    let mut session = make_session(root);
    write_bat(
        &session.working_directory().join("dup.bat"),
        "FROM-WORKING-DIR",
    );
    let env = path_env(&[&dir_a]);
    let r = session
        .run_subprocess("dup", None, None, Some(&env), false, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    assert!(
        r.stdout.contains("FROM-WORKING-DIR"),
        "expected the working-directory match to win over PATH; stdout: {}",
        r.stdout
    );
    session.cleanup();
}

/// The action's PATHEXT (not the worker process's) restricts candidates:
/// with PATHEXT=.EXE, a `.bat` earlier in PATH is not runnable, so the
/// `.exe` later in PATH must win.
#[tokio::test(flavor = "multi_thread")]
async fn test_action_pathext_restricts_candidates() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir_a = tmp.path().join("dirA");
    let dir_b = tmp.path().join("dirB");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    write_bat(&dir_a.join("pickext.bat"), "SHOULD-NOT-RUN");
    copy_system_exe(&dir_b.join("pickext.exe"));

    let root = tmp.path().join("session");
    std::fs::create_dir_all(&root).unwrap();
    let mut session = make_session(root);
    let mut env = path_env(&[&dir_a, &dir_b]);
    env.insert("PATHEXT".to_string(), ".EXE".to_string());
    let r = session
        .run_subprocess("pickext", None, None, Some(&env), false, None)
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    assert!(
        !r.stdout.contains("SHOULD-NOT-RUN"),
        "PATHEXT=.EXE must exclude the earlier .bat; stdout: {}",
        r.stdout
    );
    session.cleanup();
}

/// An explicit extension outside PATHEXT is not runnable: `script.ps1`
/// must fail with the Python-parity not-found error (shutil.which
/// returns None), not resolve to the file and die later with
/// "%1 is not a valid Win32 application" (error 193).
#[tokio::test(flavor = "multi_thread")]
async fn test_explicit_extension_outside_pathext_not_found() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir_a = tmp.path().join("dirA");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::write(dir_a.join("script.ps1"), "Write-Output 'hi'\r\n").unwrap();

    let root = tmp.path().join("session");
    std::fs::create_dir_all(&root).unwrap();
    let mut session = make_session(root);
    let mut env = path_env(&[&dir_a]);
    // Pin PATHEXT explicitly: some hosts add .PS1 to their machine-wide
    // PATHEXT, which would legitimately make the .ps1 runnable.
    env.insert(
        "PATHEXT".to_string(),
        ".COM;.EXE;.BAT;.CMD;.VBS;.JS;.WS;.MSC".to_string(),
    );
    let err = session
        .run_subprocess("script.ps1", None, None, Some(&env), false, None)
        .await
        .expect_err(".ps1 is outside PATHEXT and must be not-found, not spawn-failed");
    assert!(
        err.to_string()
            .contains("Could not find executable file: script.ps1"),
        "expected Python-parity not-found error; got: {err}"
    );
    session.cleanup();
}

/// Absolute paths bypass resolution entirely and are passed to the OS
/// as-is (extension resolution included) — unchanged behavior.
#[tokio::test(flavor = "multi_thread")]
async fn test_absolute_path_passthrough() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("session");
    std::fs::create_dir_all(&root).unwrap();
    let mut session = make_session(root);
    let system32 = PathBuf::from(std::env::var("SYSTEMROOT").unwrap()).join("System32");
    let empty_dir = tmp.path().join("empty");
    std::fs::create_dir_all(&empty_dir).unwrap();
    let env = path_env(&[&empty_dir]);
    let r = session
        .run_subprocess(
            &system32.join("whoami.exe").to_string_lossy(),
            None,
            None,
            Some(&env),
            false,
            None,
        )
        .await
        .unwrap();
    assert_eq!(r.state, ActionState::Success);
    assert_eq!(session.state(), SessionState::Ready);
    session.cleanup();
}
