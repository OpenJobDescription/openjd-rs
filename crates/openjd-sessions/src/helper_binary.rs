// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Embedded cross-user helper binary — written to disk at session start.

use std::path::{Path, PathBuf};

use crate::error::SessionError;
use crate::session_user::SessionUser;

const HELPER_BINARY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/openjd_helper"));

/// Write the embedded helper binary to `working_dir/openjd_helper[.exe]`, set
/// appropriate permissions, and return the path.
pub(crate) fn write_helper(
    working_dir: &Path,
    user: &dyn SessionUser,
) -> Result<PathBuf, SessionError> {
    let filename = if cfg!(windows) {
        "openjd_helper.exe"
    } else {
        "openjd_helper"
    };
    let path = working_dir.join(filename);
    std::fs::write(&path, HELPER_BINARY).map_err(|source| SessionError::WorkingDirectory {
        path: path.clone(),
        source,
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(Some(grp)) = nix::unistd::Group::from_name(user.group()) {
            nix::unistd::chown(&path, None, Some(grp.gid)).map_err(|e| {
                SessionError::Runtime(format!(
                    "Could not change ownership of '{}': {e}",
                    path.display()
                ))
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
