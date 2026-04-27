// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Windows executable resolution — mirrors Python `_win32/_locate_executable.py`.

use std::path::Path;

use crate::session_user::SessionUser;

/// Resolve the executable in `args[0]` for Windows, returning updated args.
///
/// - Absolute paths are returned as-is (OS resolves extensions).
/// - Relative names are resolved via `which` using the provided PATH + working_dir.
/// - Cross-user resolution falls back to same-user lookup (the executable must be
///   accessible to both users).
#[allow(dead_code)]
pub fn locate_windows_executable(
    args: &[String],
    _user: Option<&dyn SessionUser>,
    os_env_vars: Option<&std::collections::HashMap<String, Option<String>>>,
    working_dir: &str,
) -> Vec<String> {
    let mut result = args.to_vec();
    let cmd = Path::new(&args[0]);

    // Absolute paths: leave as-is
    if cmd.is_absolute() {
        return result;
    }

    // Build PATH with working_dir prepended
    let path_var = os_env_vars
        .and_then(|env| {
            env.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("path"))
                .and_then(|(_, v)| v.as_ref().map(|s| s.as_str()))
        })
        .or_else(|| std::env::var("PATH").ok().as_deref().map(|_| ""))
        .unwrap_or("");

    let search_path = format!("{};{}", working_dir, path_var);

    // Use which crate with custom path
    match which::which_in(&args[0], Some(&search_path), working_dir) {
        Ok(found) => {
            result[0] = found.to_string_lossy().to_string();
        }
        Err(_) => {
            // Leave as-is; let the OS fail naturally
        }
    }

    result
}
