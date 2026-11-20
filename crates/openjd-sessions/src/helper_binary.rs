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
            // Group ownership, then owner rwx + group r-x.
            crate::embedded_files::chown_group_then_widen(&dir, u, 0o750)?;
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
        // Group ownership, then owner rwx + group r-x.
        crate::embedded_files::chown_group_then_widen(&path, user, 0o750)?;
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::session_user::FakeSessionUser;

    /// Session user whose group cannot be resolved on this host.
    fn unresolvable_user() -> FakeSessionUser {
        FakeSessionUser::with_group("openjd_nonexistent_group_586")
    }

    /// Session user whose group is the process's own effective group, so the
    /// chown succeeds without a second OS user or extra privileges.
    fn own_group_user() -> (FakeSessionUser, nix::unistd::Gid) {
        let egid = nix::unistd::getegid();
        let group = nix::unistd::Group::from_gid(egid)
            .unwrap()
            .expect("process effective group has a name");
        (FakeSessionUser::with_group(&group.name), egid)
    }

    /// The helpers directory and the helper binary are group r-x (0o750) with
    /// the session user's group — enough for the job user to traverse and
    /// execute, never to write or modify.
    #[test]
    fn helpers_dir_and_binary_are_0o750_with_the_session_group() {
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::fs::PermissionsExt;

        let (user, egid) = own_group_user();
        let tmp = tempfile::tempdir().unwrap();

        let dir = create_helpers_dir(tmp.path(), Some(&user)).unwrap();
        let meta = std::fs::metadata(&dir).unwrap();
        assert_eq!(
            meta.permissions().mode() & 0o7777,
            0o750,
            "helpers dir mode"
        );
        assert_eq!(meta.gid(), egid.as_raw(), "helpers dir group");

        let bin = write_helper(&dir, &user).unwrap();
        let meta = std::fs::metadata(&bin).unwrap();
        assert_eq!(
            meta.permissions().mode() & 0o7777,
            0o750,
            "helper binary mode"
        );
        assert_eq!(meta.gid(), egid.as_raw(), "helper binary group");
    }

    /// The helpers directory must never be group-accessible unless its group
    /// was set to the session user's group: a failed group lookup is an
    /// error, not a silent skip followed by a 0o750 widen.
    #[test]
    fn create_helpers_dir_fails_closed_when_group_lookup_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let msg = match create_helpers_dir(tmp.path(), Some(&unresolvable_user())) {
            Ok(dir) => panic!(
                "helpers dir with an unresolvable group must fail, created {}",
                dir.display()
            ),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.starts_with("Failed to set permissions on '")
                && msg.ends_with("': Group 'openjd_nonexistent_group_586' not found"),
            "unexpected error message: {msg}"
        );

        // The directory the failed call leaves behind keeps its creation mode
        // (0o700 before umask): no group or other access.
        use std::os::unix::fs::PermissionsExt;
        let leftover = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap())
            .find(|e| e.file_name().to_string_lossy().starts_with(".helpers-"))
            .expect("helpers dir should exist");
        let mode = leftover.metadata().unwrap().permissions().mode();
        assert_eq!(
            mode & 0o077,
            0,
            "helpers dir must stay owner-only: {mode:o}"
        );
    }

    /// The helper binary's permissions must never be set unless its group was
    /// set to the session user's group.
    #[test]
    fn write_helper_fails_closed_when_group_lookup_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let msg = match write_helper(tmp.path(), &unresolvable_user()) {
            Ok(path) => panic!(
                "helper write with an unresolvable group must fail, created {}",
                path.display()
            ),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.starts_with("Failed to set permissions on '")
                && msg.ends_with("': Group 'openjd_nonexistent_group_586' not found"),
            "unexpected error message: {msg}"
        );
    }
}
