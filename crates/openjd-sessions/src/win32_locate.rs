// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Windows executable resolution — mirrors Python `_win32/_locate_executable.py`.
//!
//! Rust's `std::process::Command` alone is not a faithful Windows command
//! search: it only appends `.exe` to bare names (never consulting PATHEXT,
//! so a `.bat` in an earlier PATH directory loses to an `.exe` in a later
//! one), and when the child environment's PATH has no match it falls back
//! to `CreateProcessW`'s legacy search (application directory, system
//! directories, the parent process's PATH). Resolving the command to an
//! absolute path here, before spawn, gives canonical Windows semantics and
//! makes "not on the action's PATH" a hard error instead of a silent
//! resolution through the worker's own environment.

use std::collections::HashMap;
use std::path::Path;

// The search itself lives in the helper's source tree and is compiled into
// both binaries (see the module docs in the file). `#[path]` inclusion —
// not a shared dependency crate — because the helper is a standalone
// nested Cargo project that cannot depend on this crate.
#[path = "helper/src/win32_which.rs"]
mod win32_which;

/// Resolve the executable in `args[0]` for Windows, returning updated args.
///
/// - Absolute paths are returned as-is (the OS resolves extensions).
/// - Other commands are resolved with `shutil.which` semantics (PATHEXT
///   tried per directory, earliest PATH directory wins; an explicit
///   extension outside PATHEXT is not runnable and reports not-found)
///   over a search path of `{working_dir};{PATH}`, so executables in the
///   session working directory take precedence.
/// - PATH and PATHEXT each come from exactly one source, chosen before
///   searching: a key in `os_env_vars` (case-insensitive) is used
///   exclusively — the process environment is never consulted then, even
///   if the search finds nothing. Only when `os_env_vars` defines no such
///   key at all is the process environment's value used, matching the
///   environment merge applied at spawn (an action-supplied value
///   overwrites the inherited one) and the Python implementation's
///   `_get_path_var_for_shutil_which`.
/// - Resolution failure is an `Err` with the Python-parity message
///   `Could not find executable file: <command>`.
///
/// This function serves the **same-user** spawn path (`run_subprocess`).
/// Cross-user actions are resolved inside the embedded helper instead
/// (`helper/src/runner_win.rs::locate_executable`, same search — both
/// compile the shared `win32_which.rs`) — the helper runs as the target
/// user, so it can probe directories only that user can read, and its
/// PATH/PATHEXT fallback is the target user's environment rather than
/// the service user's.
pub(crate) fn locate_windows_executable(
    args: &[String],
    os_env_vars: Option<&HashMap<String, Option<String>>>,
    working_dir: &Path,
) -> Result<Vec<String>, String> {
    let cmd = Path::new(&args[0]);

    // Absolute paths: leave as-is (the OS resolves the extension).
    if cmd.is_absolute() {
        return Ok(args.to_vec());
    }

    let env_var = |name: &str| -> Option<String> {
        os_env_vars.and_then(|env| {
            env.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .and_then(|(_, v)| v.clone())
        })
    };
    let path_var = env_var("PATH").unwrap_or_else(|| std::env::var("PATH").unwrap_or_default());
    let pathext =
        env_var("PATHEXT").unwrap_or_else(|| std::env::var("PATHEXT").unwrap_or_default());
    let search_path = format!("{};{}", working_dir.display(), path_var);

    match win32_which::locate_in(&args[0], &search_path, &pathext, working_dir) {
        Some(found) => {
            let mut result = args.to_vec();
            result[0] = found.to_string_lossy().into_owned();
            Ok(result)
        }
        None => Err(format!("Could not find executable file: {}", args[0])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::write(path, "").unwrap();
    }

    fn env_with_path(dirs: &[&Path]) -> HashMap<String, Option<String>> {
        let joined = dirs
            .iter()
            .map(|d| d.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(";");
        // Mixed-case key: the lookup must be case-insensitive.
        HashMap::from([("Path".to_string(), Some(joined))])
    }

    fn args(cmd: &str) -> Vec<String> {
        vec![cmd.to_string(), "arg1".to_string()]
    }

    /// A `.bat` in an earlier PATH directory wins over an `.exe` in a later
    /// one — canonical Windows search order (PATHEXT tried per directory).
    #[test]
    fn bat_earlier_in_path_beats_exe_later() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir_a = tmp.path().join("a");
        let dir_b = tmp.path().join("b");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();
        touch(&dir_a.join("tool.bat"));
        touch(&dir_b.join("tool.exe"));
        let wd = tmp.path();

        let resolved =
            locate_windows_executable(&args("tool"), Some(&env_with_path(&[&dir_a, &dir_b])), wd)
                .unwrap();
        assert_eq!(
            resolved[0].to_lowercase(),
            dir_a.join("tool.bat").to_string_lossy().to_lowercase(),
            "earliest PATH directory must win across extensions"
        );
        assert_eq!(resolved[1], "arg1", "remaining args are preserved");
    }

    /// A bare name whose only match is a `.bat` resolves (std's `.exe`-only
    /// probing would miss it).
    #[test]
    fn bare_name_resolves_bat_only_match() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir_a = tmp.path().join("a");
        std::fs::create_dir_all(&dir_a).unwrap();
        touch(&dir_a.join("onlybat.bat"));

        let resolved = locate_windows_executable(
            &args("onlybat"),
            Some(&env_with_path(&[&dir_a])),
            tmp.path(),
        )
        .unwrap();
        assert_eq!(
            resolved[0].to_lowercase(),
            dir_a.join("onlybat.bat").to_string_lossy().to_lowercase()
        );
    }

    /// The working directory is searched first, before any PATH entry.
    #[test]
    fn working_dir_wins_over_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir_a = tmp.path().join("a");
        let wd = tmp.path().join("wd");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&wd).unwrap();
        touch(&dir_a.join("dup.bat"));
        touch(&wd.join("dup.bat"));

        let resolved =
            locate_windows_executable(&args("dup"), Some(&env_with_path(&[&dir_a])), &wd).unwrap();
        assert_eq!(
            resolved[0].to_lowercase(),
            wd.join("dup.bat").to_string_lossy().to_lowercase()
        );
    }

    /// A command absent from the search path is a hard error with the
    /// Python-parity message — no fallback to the process's own PATH for
    /// bare names. `whoami` exists on the real PATH, so success here would
    /// prove a fallback leak.
    #[test]
    fn absent_command_is_error_not_process_path_fallback() {
        let tmp = tempfile::TempDir::new().unwrap();
        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();

        let err =
            locate_windows_executable(&args("whoami"), Some(&env_with_path(&[&empty])), &empty)
                .unwrap_err();
        assert_eq!(err, "Could not find executable file: whoami");
    }

    /// Absolute paths pass through untouched.
    #[test]
    fn absolute_path_passthrough() {
        let a = args(r"C:\Windows\System32\whoami.exe");
        let resolved = locate_windows_executable(&a, None, Path::new(".")).unwrap();
        assert_eq!(resolved, a);
    }

    /// Without action env vars (or without PATH in them), resolution falls
    /// back to the process environment's PATH.
    #[test]
    fn falls_back_to_process_path() {
        // `whoami` is on every Windows system PATH via System32.
        let resolved =
            locate_windows_executable(&args("whoami"), Some(&HashMap::new()), Path::new("."))
                .unwrap();
        assert!(
            resolved[0].to_lowercase().ends_with("whoami.exe"),
            "expected whoami.exe from process PATH; got {}",
            resolved[0]
        );
    }
}
