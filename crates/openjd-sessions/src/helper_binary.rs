// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Embedded cross-user helper binary — written to disk at session start.
//!
//! The helper is placed in a randomized subdirectory with restrictive
//! permissions so that the job user can execute the binary but cannot
//! modify or replace it.
//!
//! - POSIX: directory and binary are 0o750 (owner rwx, group r-x),
//!   owned by the process user with group set to the session user's group.
//! - Windows: directory and binary have an explicit DACL granting the
//!   process user Full Control and the session user Read & Execute.
//!   The session user's ACE intentionally omits `FILE_WRITE_DATA`,
//!   `FILE_APPEND_DATA`, `DELETE`, and `FILE_DELETE_CHILD` so the job
//!   user cannot overwrite, replace, or delete the helper binary.

use std::path::{Path, PathBuf};

use crate::error::SessionError;
use crate::session_user::SessionUser;

const HELPER_BINARY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/openjd_helper"));

/// Create a restricted helpers directory inside `working_dir`.
///
/// Returns the path to the new directory.
///
/// - On POSIX: the directory is owned by the process user with group set
///   to the session user's group and mode 0o750 — the job user can
///   traverse and read (needed for `sudo -u <user> -i` to execute the
///   helper binary) but cannot write or modify files.
/// - On Windows: the directory's DACL grants the process user Full
///   Control and the session user Read & Execute (inheritable). This
///   prevents the job user from modifying or deleting the helper binary
///   that will be written inside, even though the parent session working
///   directory's inherited DACL grants the session user Modify.
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

    #[cfg(windows)]
    {
        std::fs::create_dir(&dir).map_err(|source| SessionError::WorkingDirectory {
            path: dir.clone(),
            source,
        })?;

        // Lock down the DACL for cross-user sessions. Without this the
        // directory inherits the session working dir's DACL, which grants
        // the session user Modify — enough to replace or delete the helper
        // binary written inside. We explicitly set, with inheritance from
        // the parent severed:
        //   process user → Full Control (inheritable to children)
        //   session user → Read & Execute only (inheritable to children)
        if let Some(u) = user.filter(|u| !u.is_process_user()) {
            let process_user =
                crate::win32::get_process_user().map_err(|e| SessionError::PathPermissions {
                    path: dir.display().to_string(),
                    reason: format!("Could not determine process user: {e}"),
                })?;
            crate::win32_permissions::set_permissions_protected(
                &dir.to_string_lossy(),
                &[process_user.as_str()],
                &[],
                &[u.user()],
            )
            .map_err(|reason| SessionError::PathPermissions {
                path: dir.display().to_string(),
                reason,
            })?;
        }
    }

    Ok(dir)
}

/// Write the embedded helper binary to `helpers_dir/<random>[.exe]`, set
/// appropriate permissions, and return the path.
///
/// The binary name is randomized to prevent prediction. Permissions are
/// tightened so the job user can execute the binary but cannot modify or
/// replace it:
///
/// - POSIX: 0o750 (owner rwx, group r-x), group = session user's group.
/// - Windows: explicit DACL with process user Full Control and session
///   user Read & Execute. This is defense-in-depth over the helpers
///   directory's DACL — if the directory's DACL is ever weakened, the
///   binary itself still refuses session-user writes.
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

    #[cfg(windows)]
    {
        if !user.is_process_user() {
            let process_user =
                crate::win32::get_process_user().map_err(|e| SessionError::PathPermissions {
                    path: path.display().to_string(),
                    reason: format!("Could not determine process user: {e}"),
                })?;
            // Protected DACL: no inheritance from the helpers directory,
            // so even if something weakens that dir's DACL the binary is
            // still enforced as read-only for the session user.
            crate::win32_permissions::set_permissions_protected(
                &path.to_string_lossy(),
                &[process_user.as_str()],
                &[],
                &[user.user()],
            )
            .map_err(|reason| SessionError::PathPermissions {
                path: path.display().to_string(),
                reason,
            })?;
        }
    }

    Ok(path)
}
