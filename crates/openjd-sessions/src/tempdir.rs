// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Secure temporary directory creation — mirrors Python `_tempdir.py`.

use std::path::{Path, PathBuf};

use crate::error::SessionError;
use crate::session_user::SessionUser;

/// Returns the platform-specific OpenJD temp directory, creating it if needed.
pub fn openjd_temp_dir() -> Result<PathBuf, SessionError> {
    #[cfg(unix)]
    let base = std::env::temp_dir();

    #[cfg(windows)]
    let base = {
        let program_data = std::env::var("PROGRAMDATA").unwrap_or_else(|_| {
            log::warn!(
                target: "openjd.sessions",
                "Environment variable \"PROGRAMDATA\" is not set. \
                 Creating session working directories under C:\\ProgramData"
            );
            r"C:\ProgramData".to_string()
        });
        PathBuf::from(program_data).join("Amazon")
    };

    let dir = base.join("OpenJD");
    std::fs::create_dir_all(&dir).map_err(|e| SessionError::TempDir {
        path: dir.clone(),
        source: e,
    })?;
    Ok(dir)
}

/// Check parent directories for world-writable dirs missing the sticky bit (POSIX only).
#[cfg(unix)]
pub fn validate_sticky_bit(root_dir: &Path) {
    use std::os::unix::fs::MetadataExt;

    const S_IWOTH: u32 = 0o002;
    const S_ISVTX: u32 = 0o1000;

    for parent in root_dir.ancestors().skip(1) {
        if let Ok(meta) = std::fs::metadata(parent) {
            let mode = meta.mode();
            if (mode & S_IWOTH) != 0 && (mode & S_ISVTX) == 0 {
                log::warn!(
                    target: "openjd.sessions",
                    "Sticky bit is not set on {}. This may pose a risk when running work \
                     on this host as users may modify or delete files in this directory \
                     which do not belong to them.",
                    parent.display()
                );
            }
        }
    }
}

/// A securely-created temporary directory.
///
/// Call [`cleanup()`](TempDir::cleanup) to remove the directory. If dropped
/// without calling `cleanup()`, a best-effort removal is attempted.
///
/// ```
/// use openjd_sessions::TempDir;
///
/// let dir = tempfile::tempdir().unwrap();
/// let mut td = TempDir::new(Some(dir.path()), Some("test-"), None).unwrap();
/// assert!(td.path().exists());
/// td.cleanup().unwrap();
/// assert!(!td.path().exists());
/// ```
pub struct TempDir {
    path: PathBuf,
    cleaned_up: bool,
}

impl std::fmt::Debug for TempDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TempDir")
            .field("path", &self.path)
            .field("cleaned_up", &self.cleaned_up)
            .finish()
    }
}

impl AsRef<std::path::Path> for TempDir {
    fn as_ref(&self) -> &std::path::Path {
        &self.path
    }
}

impl TempDir {
    /// Create a new secure temp directory.
    ///
    /// - `dir`: parent directory (defaults to `openjd_temp_dir()`)
    /// - `prefix`: optional name prefix
    /// - `user`: optional session user for cross-user ownership
    pub fn new(
        dir: Option<&Path>,
        prefix: Option<&str>,
        _user: Option<&dyn SessionUser>,
    ) -> Result<Self, SessionError> {
        let parent = match dir {
            Some(d) => d.to_path_buf(),
            None => openjd_temp_dir()?,
        };

        let prefix = prefix.unwrap_or("");
        let suffix = random_hex();
        let name = format!("{prefix}{suffix}");
        let path = parent.join(name);

        std::fs::create_dir(&path).map_err(|e| SessionError::TempDir {
            path: path.clone(),
            source: e,
        })?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if let Some(u) = _user.filter(|u| !u.is_process_user()) {
                // Cross-user: chown group then set 0o770
                // chown before chmod — security: don't grant group access if chown fails
                if let Ok(Some(grp)) = nix::unistd::Group::from_name(u.group()) {
                    nix::unistd::chown(&path, None, Some(grp.gid)).map_err(|e| {
                        SessionError::PathPermissions {
                            path: path.display().to_string(),
                            reason: format!(
                                "Could not change ownership (error: {e}). Please ensure that uid {} is a member of group {}.",
                                nix::unistd::geteuid(), u.group()
                            ),
                        }
                    })?;
                }
                0o770
            } else {
                0o700
            };
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).map_err(
                |e| SessionError::TempDir {
                    path: path.clone(),
                    source: e,
                },
            )?;
        }

        // Windows: set DACL — full control for process user, modify for session user.
        #[cfg(windows)]
        {
            if let Some(u) = _user.filter(|u| !u.is_process_user()) {
                if let Ok(process_user) = crate::win32::get_process_user() {
                    if let Err(e) = crate::win32_permissions::set_permissions(
                        &path.to_string_lossy(),
                        &[process_user.as_str()],
                        &[u.user()],
                    ) {
                        return Err(SessionError::PathPermissions {
                            path: path.display().to_string(),
                            reason: e.to_string(),
                        });
                    }
                }
            }
        }

        Ok(Self {
            path,
            cleaned_up: false,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Remove the directory and all contents.
    pub fn cleanup(&mut self) -> Result<(), SessionError> {
        if self.cleaned_up {
            return Ok(());
        }
        self.cleaned_up = true;
        std::fs::remove_dir_all(&self.path).map_err(|e| SessionError::TempDir {
            path: self.path.clone(),
            source: e,
        })
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if !self.cleaned_up {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn random_hex() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempdir_debug() {
        let td = TempDir::new(None, None, None).unwrap();
        let dbg = format!("{td:?}");
        assert!(dbg.contains("TempDir"));
        assert!(dbg.contains("cleaned_up: false"));
    }

    #[test]
    fn tempdir_as_ref_path() {
        let td = TempDir::new(None, None, None).unwrap();
        let p: &std::path::Path = td.as_ref();
        assert_eq!(p, td.path());
    }

    /// Mirrors Python TestSession::test_posix_permissions_warning.
    /// Creates a world-writable dir without the sticky bit and verifies the warning.
    #[cfg(unix)]
    #[test]
    fn validate_sticky_bit_warns_on_world_writable_without_sticky() {
        use std::os::unix::fs::PermissionsExt;
        testing_logger::setup();

        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("world_writable");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();

        let child = dir.join("child");
        std::fs::create_dir(&child).unwrap();

        validate_sticky_bit(&child);

        testing_logger::validate(|captured_logs| {
            assert!(
                captured_logs.iter().any(|log| {
                    log.level == log::Level::Warn && log.body.contains("Sticky bit is not set")
                }),
                "Expected a warning about missing sticky bit"
            );
        });
    }

    /// Mirrors Python TestSession::test_posix_permissions_no_warning.
    /// A dir with the sticky bit set should not trigger a warning.
    #[cfg(unix)]
    #[test]
    fn validate_sticky_bit_no_warn_when_sticky_set() {
        use std::os::unix::fs::PermissionsExt;
        testing_logger::setup();

        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("sticky_dir");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o1777)).unwrap();

        let child = dir.join("child");
        std::fs::create_dir(&child).unwrap();

        validate_sticky_bit(&child);

        testing_logger::validate(|captured_logs| {
            assert!(
                !captured_logs
                    .iter()
                    .any(|log| { log.body.contains("Sticky bit is not set") }),
                "Should not warn when sticky bit is set"
            );
        });
    }

    /// Mirrors Python TestTempDirWindows::test_windows_temp_dir — warns when PROGRAMDATA unset.
    #[cfg(windows)]
    #[test]
    fn openjd_temp_dir_warns_when_programdata_unset() {
        testing_logger::setup();
        // Temporarily remove PROGRAMDATA
        let original = std::env::var("PROGRAMDATA").ok();
        unsafe {
            std::env::remove_var("PROGRAMDATA");
        }
        let result = openjd_temp_dir();
        // Restore
        if let Some(val) = original {
            unsafe {
                std::env::set_var("PROGRAMDATA", val);
            }
        }
        assert!(result.is_ok());
        testing_logger::validate(|captured_logs| {
            assert!(
                captured_logs.iter().any(|log| {
                    log.level == log::Level::Warn && log.body.contains("PROGRAMDATA")
                }),
                "Expected a warning about PROGRAMDATA not being set"
            );
        });
    }
}
