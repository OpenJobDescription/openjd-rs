// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Embedded cross-user helper binary — written to disk at session start.
//!
//! The helper is placed in a randomized subdirectory with restrictive
//! permissions (0o750 on POSIX, group r-x only) so that the job user
//! can execute the binary via `sudo` but cannot modify or replace it.

use std::path::{Path, PathBuf};

use crate::error::SessionError;
use crate::session_user::SessionUser;

const HELPER_BINARY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/openjd_helper"));

/// Create a restricted helpers directory inside `working_dir`.
///
/// Returns the path to the new directory. On POSIX, the directory is owned
/// by the process user with group set to the session user's group and mode
/// 0o750 — the job user can traverse and read (needed for `sudo -u <user> -i`
/// to execute the helper binary) but cannot write or modify files.
pub(crate) fn create_helpers_dir(
    working_dir: &Path,
    user: Option<&dyn SessionUser>,
) -> Result<PathBuf, SessionError> {
    let dir = working_dir.join(format!(".helpers-{}", uuid::Uuid::new_v4().simple()));

    #[cfg(unix)]
    {
        // Always create at 0o700 (owner-only). For cross-user, we widen to
        // 0o750 only *after* chown sets the correct group — so the group
        // never has access until it's the right group.
        nix::unistd::mkdir(&dir, nix::sys::stat::Mode::from_bits_truncate(0o700)).map_err(|e| {
            SessionError::WorkingDirectory {
                path: dir.clone(),
                source: std::io::Error::from(e),
            }
        })?;

        if let Some(u) = user.filter(|u| !u.is_process_user()) {
            if let Ok(Some(grp)) = nix::unistd::Group::from_name(u.group()) {
                nix::unistd::chown(&dir, None, Some(grp.gid)).map_err(|e| {
                    SessionError::PathPermissions {
                        path: dir.display().to_string(),
                        reason: e.to_string(),
                    }
                })?;
            }
            // Now that the group is correct, grant group r-x.
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o750)).map_err(
                |source| SessionError::WorkingDirectory {
                    path: dir.clone(),
                    source,
                },
            )?;
        }
    }

    #[cfg(not(unix))]
    {
        let _ = user;
        std::fs::create_dir(&dir).map_err(|source| SessionError::WorkingDirectory {
            path: dir.clone(),
            source,
        })?;
    }

    Ok(dir)
}

/// Write the embedded helper binary to `helpers_dir/<random>[.exe]`, set
/// appropriate permissions, and return the path.
///
/// The binary name is randomized to prevent prediction. The helpers directory
/// is 0o750 (owner rwx, group r-x) so the job user can traverse and execute
/// but cannot modify or replace the binary.
pub(crate) fn write_helper(
    helpers_dir: &Path,
    user: &dyn SessionUser,
) -> Result<PathBuf, SessionError> {
    let ext = if cfg!(windows) { ".exe" } else { "" };
    let filename = format!("h-{}{}", uuid::Uuid::new_v4().simple(), ext);
    let path = helpers_dir.join(filename);
    std::fs::write(&path, HELPER_BINARY).map_err(|source| SessionError::WorkingDirectory {
        path: path.clone(),
        source,
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(Some(grp)) = nix::unistd::Group::from_name(user.group()) {
            nix::unistd::chown(&path, None, Some(grp.gid)).map_err(|e| {
                SessionError::PathPermissions {
                    path: path.display().to_string(),
                    reason: e.to_string(),
                }
            })?;
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o750)).map_err(
            |source| SessionError::WorkingDirectory {
                path: path.clone(),
                source,
            },
        )?;
    }

    #[cfg(not(unix))]
    let _ = user; // suppress unused warning on Windows

    Ok(path)
}
