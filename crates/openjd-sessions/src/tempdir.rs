// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Secure temporary directory creation — mirrors Python `_tempdir.py`.

use std::path::{Path, PathBuf};

use crate::error::SessionError;
use crate::session_user::SessionUser;

/// Controls behavior when a parent directory is world-writable without the sticky bit.
///
/// On POSIX systems, a world-writable directory without the sticky bit allows any
/// user to rename or delete files belonging to other users. This is a security risk
/// for session working directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StickyBitPolicy {
    /// Refuse to create the session if a parent directory is unsafe.
    /// This is the default — fail-closed is the secure choice.
    #[default]
    Strict,
    /// Log a warning but allow the session to proceed.
    Warn,
    /// Skip the check entirely.
    Disabled,
}

/// Returns the OpenJD temp directory, creating it if needed.
///
/// `base_dir` is the OpenJD directory itself — the directory under which
/// individual session and file temp directories will be created.
///
/// - `None` → derive from the environment:
///   - Unix: `<std::env::temp_dir()>/OpenJD` (typically `/tmp/OpenJD`).
///   - Windows: `%PROGRAMDATA%\Amazon\OpenJD` (with a warning and fallback to
///     `C:\ProgramData\Amazon\OpenJD` if `PROGRAMDATA` is unset).
/// - `Some(p)` → use `p` directly.
///
/// Tests should pass `Some(...)` to avoid mutating process-global environment
/// variables, which races with parallel tests that read them.
pub fn openjd_temp_dir(base_dir: Option<&Path>) -> Result<PathBuf, SessionError> {
    let dir = match base_dir {
        Some(p) => p.to_path_buf(),
        None => default_openjd_dir(),
    };

    std::fs::create_dir_all(&dir).map_err(|e| SessionError::TempDir {
        path: dir.clone(),
        source: e,
    })?;
    Ok(dir)
}

/// Compute the default OpenJD directory from the environment.
///
/// Split out so the env-var inspection (and Windows warning) can be tested
/// without `openjd_temp_dir` doing filesystem work, and without tests having
/// to mutate process-global env vars.
fn default_openjd_dir() -> PathBuf {
    #[cfg(unix)]
    {
        std::env::temp_dir().join("OpenJD")
    }

    #[cfg(windows)]
    {
        openjd_dir_from_programdata(std::env::var("PROGRAMDATA").ok())
    }
}

/// Build the Windows OpenJD directory from a `PROGRAMDATA` value.
///
/// Pure function: takes an explicit value (as if from the environment),
/// performs no I/O, and is fully testable without touching real env vars.
/// Logs a warning when `programdata` is `None` and falls back to
/// `C:\ProgramData`.
#[cfg(windows)]
fn openjd_dir_from_programdata(programdata: Option<String>) -> PathBuf {
    let program_data = programdata.unwrap_or_else(|| {
        log::warn!(
            target: "openjd.sessions",
            "Environment variable \"PROGRAMDATA\" is not set. \
             Creating session working directories under C:\\ProgramData"
        );
        r"C:\ProgramData".to_string()
    });
    PathBuf::from(program_data).join("Amazon").join("OpenJD")
}

/// Check parent directories for world-writable dirs missing the sticky bit (POSIX only).
///
/// Returns the first offending path, or `None` if all parents are safe.
#[cfg(unix)]
pub fn find_missing_sticky_bit(root_dir: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt;

    const S_IWOTH: u32 = 0o002;
    const S_ISVTX: u32 = 0o1000;

    for parent in root_dir.ancestors().skip(1) {
        if let Ok(meta) = std::fs::metadata(parent) {
            let mode = meta.mode();
            if (mode & S_IWOTH) != 0 && (mode & S_ISVTX) == 0 {
                return Some(parent.to_path_buf());
            }
        }
    }
    None
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
    /// - `dir`: parent directory (defaults to `openjd_temp_dir(None)`)
    /// - `prefix`: optional name prefix
    /// - `user`: optional session user for cross-user ownership
    pub fn new(
        dir: Option<&Path>,
        prefix: Option<&str>,
        _user: Option<&dyn SessionUser>,
    ) -> Result<Self, SessionError> {
        let parent = match dir {
            Some(d) => d.to_path_buf(),
            None => openjd_temp_dir(None)?,
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
                // Fail closed: a cross-user directory without the DACL grant is
                // unusable by the session user (and silently skipping the grant
                // previously masked the failure until a subprocess hit it).
                let process_user = crate::win32::get_process_user().map_err(|e| {
                    SessionError::PathPermissions {
                        path: path.display().to_string(),
                        reason: format!("Could not determine the process user for DACL setup: {e}"),
                    }
                })?;
                if let Err(e) = crate::win32_permissions::set_permissions(
                    &path.to_string_lossy(),
                    &[process_user.as_str()],
                    &[u.user()],
                    &[],
                ) {
                    return Err(SessionError::PathPermissions {
                        path: path.display().to_string(),
                        reason: e.to_string(),
                    });
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
    /// Creates a world-writable dir without the sticky bit and verifies detection.
    #[cfg(unix)]
    #[test]
    fn find_missing_sticky_bit_detects_world_writable_without_sticky() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("world_writable");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();

        let child = dir.join("child");
        std::fs::create_dir(&child).unwrap();

        let result = find_missing_sticky_bit(&child);
        assert_eq!(result, Some(dir));
    }

    /// Mirrors Python TestSession::test_posix_permissions_no_warning.
    /// A dir with the sticky bit set should not be flagged.
    #[cfg(unix)]
    #[test]
    fn find_missing_sticky_bit_none_when_sticky_set() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("sticky_dir");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o1777)).unwrap();

        let child = dir.join("child");
        std::fs::create_dir(&child).unwrap();

        let result = find_missing_sticky_bit(&child);
        assert_eq!(result, None);
    }

    /// Mirrors Python TestTempDirWindows::test_windows_temp_dir — verifies the
    /// warning when `PROGRAMDATA` is unset.
    ///
    /// Tests the pure helper `openjd_dir_from_programdata` directly so we
    /// don't have to mutate the real `PROGRAMDATA` env var, which would race
    /// with parallel tests that read it.
    #[cfg(windows)]
    #[test]
    fn openjd_dir_from_programdata_warns_when_unset() {
        testing_logger::setup();
        let dir = openjd_dir_from_programdata(None);
        assert_eq!(dir, PathBuf::from(r"C:\ProgramData\Amazon\OpenJD"));
        testing_logger::validate(|captured_logs| {
            assert!(
                captured_logs.iter().any(|log| {
                    log.level == log::Level::Warn && log.body.contains("PROGRAMDATA")
                }),
                "Expected a warning about PROGRAMDATA not being set"
            );
        });
    }

    /// Confirms the helper composes the path correctly when `PROGRAMDATA` is
    /// provided. No warning should be emitted.
    #[cfg(windows)]
    #[test]
    fn openjd_dir_from_programdata_uses_provided_value() {
        testing_logger::setup();
        let dir = openjd_dir_from_programdata(Some(r"D:\ProgramData".to_string()));
        assert_eq!(dir, PathBuf::from(r"D:\ProgramData\Amazon\OpenJD"));
        testing_logger::validate(|captured_logs| {
            assert!(
                !captured_logs
                    .iter()
                    .any(|log| log.level == log::Level::Warn),
                "Expected no warning when PROGRAMDATA is provided"
            );
        });
    }

    /// Mirrors Python TestTempDirWindows::test_windows_temp_dir — positive
    /// path: when an explicit OpenJD directory is provided, `openjd_temp_dir`
    /// uses it and creates it (and any missing parents) on disk.
    ///
    /// This test passes the directory directly via the `base_dir` parameter
    /// instead of mutating `PROGRAMDATA`, which would race with parallel
    /// tests that read it.
    #[cfg(windows)]
    #[test]
    fn openjd_temp_dir_honors_base_dir_override() {
        let custom_root = std::env::temp_dir().join("OpenJDBaseDirTest");
        // Ensure a clean slate so create_dir_all does real work.
        let _ = std::fs::remove_dir_all(&custom_root);

        let openjd_dir = custom_root.join("Amazon").join("OpenJD");
        let dir = openjd_temp_dir(Some(&openjd_dir))
            .expect("openjd_temp_dir should succeed with override");
        assert_eq!(
            dir, openjd_dir,
            "openjd_temp_dir should return the path it was given"
        );
        assert!(dir.exists(), "openjd_temp_dir should create the directory");
        assert!(
            custom_root.join("Amazon").exists(),
            "missing parents should be created as a side effect"
        );

        // Clean up
        let _ = std::fs::remove_dir_all(&custom_root);
    }
}
