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
        user: Option<&dyn SessionUser>,
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

        Self::secure(path, user)
    }

    /// Create a session working directory directly under `dir`, named with
    /// [`SESSION_DIR_NAME_LENGTH`] random characters and nothing else.
    ///
    /// Matches Python's `mkdtemp(prefix="")`: the name is drawn from
    /// [`SESSION_DIR_NAME_ALPHABET`], each candidate is created with an
    /// exclusive `create_dir`, and a name that already exists is retried
    /// with a fresh one, up to [`SESSION_DIR_NAME_ATTEMPTS`] times.
    pub(crate) fn new_session_working_dir(
        dir: &Path,
        user: Option<&dyn SessionUser>,
    ) -> Result<Self, SessionError> {
        let path = create_unique_dir(dir, SESSION_DIR_NAME_ATTEMPTS, random_session_dir_name)?;
        Self::secure(path, user)
    }

    /// Apply ownership and permissions to the just-created directory `path`.
    fn secure(path: PathBuf, user: Option<&dyn SessionUser>) -> Result<Self, SessionError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if let Some(u) = user.filter(|u| !u.is_process_user()) {
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
            if let Some(u) = user.filter(|u| !u.is_process_user()) {
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

    #[allow(
        missing_docs,
        reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
    )]
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

/// Length of a session working directory's name.
///
/// Every character of the working directory's path is charged against
/// `MAX_PATH` (260) for the Windows applications a job runs, many of which
/// are not long-path aware, so the name is this many random characters and
/// nothing else. It carries no part of the session ID; the session log
/// records the working directory's path beside the full session ID, which is
/// how a retained or orphaned directory is traced back to its session.
/// Matches Python's `SESSION_DIR_NAME_LENGTH` (openjd-sessions-for-python
/// PR #348), where the name comes from `mkdtemp(prefix="")`.
pub(crate) const SESSION_DIR_NAME_LENGTH: usize = 8;

/// Characters a session working directory's name is drawn from: CPython's
/// `tempfile._RandomNameSequence.characters`. Lower case only, so two names
/// cannot differ only in case (NTFS folds case), and free of every
/// Windows-reserved character.
pub(crate) const SESSION_DIR_NAME_ALPHABET: &[u8; 37] = b"abcdefghijklmnopqrstuvwxyz0123456789_";

/// How many names [`TempDir::new_session_working_dir`] tries before giving
/// up. CPython's `tempfile.TMP_MAX` fallback value.
pub(crate) const SESSION_DIR_NAME_ATTEMPTS: usize = 10_000;

/// Generate one candidate session working directory name:
/// [`SESSION_DIR_NAME_LENGTH`] characters drawn uniformly from
/// [`SESSION_DIR_NAME_ALPHABET`] with the OS CSPRNG.
fn random_session_dir_name() -> std::io::Result<String> {
    session_dir_name_from(|buf| {
        getrandom::fill(buf).map_err(|e| {
            std::io::Error::other(format!("Failed to generate a random directory name: {e}"))
        })
    })
}

/// Build a session working directory name from the random bytes that `fill`
/// writes, calling it again for more bytes as needed.
///
/// Rejection sampling keeps the draw uniform: 222 is the largest multiple of
/// 37 that fits in a byte, so bytes >= 222 are discarded and each kept byte
/// maps to `SESSION_DIR_NAME_ALPHABET[byte % 37]`.
fn session_dir_name_from(
    mut fill: impl FnMut(&mut [u8]) -> std::io::Result<()>,
) -> std::io::Result<String> {
    const LIMIT: u8 =
        (256 / SESSION_DIR_NAME_ALPHABET.len() * SESSION_DIR_NAME_ALPHABET.len()) as u8;
    let mut name = String::with_capacity(SESSION_DIR_NAME_LENGTH);
    let mut buf = [0u8; 16];
    while name.len() < SESSION_DIR_NAME_LENGTH {
        fill(&mut buf)?;
        for b in buf {
            if b < LIMIT && name.len() < SESSION_DIR_NAME_LENGTH {
                let i = usize::from(b) % SESSION_DIR_NAME_ALPHABET.len();
                name.push(char::from(SESSION_DIR_NAME_ALPHABET[i]));
            }
        }
    }
    Ok(name)
}

/// Create a new directory under `parent` named by `next_name`, retrying with
/// a fresh name each time the candidate already exists.
///
/// Each attempt is an exclusive `create_dir`, so an existing directory is
/// never reused and concurrent callers cannot be handed the same path. Any
/// error other than `AlreadyExists` is returned at once, as is a failure of
/// `next_name` (reported against `parent`). After `attempts` names that all
/// exist, fails with an `AlreadyExists` error naming `parent`.
fn create_unique_dir(
    parent: &Path,
    attempts: usize,
    mut next_name: impl FnMut() -> std::io::Result<String>,
) -> Result<PathBuf, SessionError> {
    for _ in 0..attempts {
        let name = next_name().map_err(|source| SessionError::TempDir {
            path: parent.to_path_buf(),
            source,
        })?;
        let path = parent.join(name);
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(SessionError::TempDir { path, source: e }),
        }
    }
    Err(SessionError::TempDir {
        path: parent.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("No usable temporary directory name found after {attempts} attempts"),
        ),
    })
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

    /// The naming parameters are the documented ones (tempdir.md, "Session
    /// working directory name"), spelled out literally so that a change to a
    /// constant cannot pass by also changing what the tests compare against.
    #[test]
    fn session_dir_name_parameters_match_spec() {
        assert_eq!(SESSION_DIR_NAME_LENGTH, 8);
        assert_eq!(
            SESSION_DIR_NAME_ALPHABET,
            b"abcdefghijklmnopqrstuvwxyz0123456789_"
        );
        assert_eq!(SESSION_DIR_NAME_ATTEMPTS, 10_000);
    }

    /// A candidate name is exactly 8 characters from `[a-z0-9_]`.
    #[test]
    fn random_session_dir_name_length_and_alphabet() {
        for _ in 0..1000 {
            let name = random_session_dir_name().unwrap();
            assert_eq!(name.len(), 8, "{name:?}");
            assert!(
                name.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
                "{name:?}"
            );
        }
    }

    /// Bytes at or above 222 are rejected and each kept byte maps to
    /// `ALPHABET[byte % 37]`, so the draw is uniform. 221 is the largest
    /// accepted byte and maps to the last character, `_`.
    #[test]
    fn session_dir_name_from_rejects_biased_bytes() {
        let bytes: [u8; 12] = [222, 255, 0, 36, 37, 221, 1, 2, 3, 4, 25, 26];
        let mut calls = 0;
        let name = session_dir_name_from(|buf| {
            calls += 1;
            buf.fill(222);
            buf[..bytes.len()].copy_from_slice(&bytes);
            Ok(())
        })
        .unwrap();
        // 222 and 255 rejected; 0->a, 36->_, 37->a, 221->_, 1->b, 2->c, 3->d, 4->e.
        assert_eq!(name, "a_a_bcde");
        assert_eq!(calls, 1);
    }

    /// When a buffer yields too few accepted bytes, more are requested.
    #[test]
    fn session_dir_name_from_refills_until_full() {
        let mut calls = 0;
        let name = session_dir_name_from(|buf| {
            calls += 1;
            buf.fill(255);
            buf[0] = 24 + calls; // one accepted byte per call: z, 0, 1, ...
            Ok(())
        })
        .unwrap();
        assert_eq!(name, "z0123456");
        assert_eq!(calls, 8);
    }

    /// Every alphabet character is reachable, including the last one (`_`),
    /// which an off-by-one in the rejection limit or the modulus would drop.
    #[test]
    fn random_session_dir_name_uses_whole_alphabet() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..2000 {
            seen.extend(random_session_dir_name().unwrap().bytes());
        }
        assert_eq!(seen.len(), SESSION_DIR_NAME_ALPHABET.len(), "{seen:?}");
    }

    /// A name that already exists is skipped, not reused and not an error:
    /// the next candidate is created instead and the existing directory is
    /// left untouched.
    #[test]
    fn create_unique_dir_retries_on_collision() {
        let parent = tempfile::tempdir().unwrap();
        let taken = parent.path().join("taken000");
        std::fs::create_dir(&taken).unwrap();
        std::fs::write(taken.join("marker"), "x").unwrap();
        let mut names = vec!["taken000", "fresh000"].into_iter();
        let path =
            create_unique_dir(parent.path(), 10, || Ok(names.next().unwrap().to_string())).unwrap();
        assert_eq!(path, parent.path().join("fresh000"));
        assert!(path.is_dir());
        assert!(taken.join("marker").exists());
        assert_eq!(names.next(), None, "exactly two candidates are drawn");
    }

    /// When every candidate exists, creation stops after `attempts` tries
    /// with an error naming the parent directory.
    #[test]
    fn create_unique_dir_gives_up_after_attempts() {
        let parent = tempfile::tempdir().unwrap();
        std::fs::create_dir(parent.path().join("taken000")).unwrap();
        let mut calls = 0;
        let err = create_unique_dir(parent.path(), 3, || {
            calls += 1;
            Ok("taken000".to_string())
        })
        .unwrap_err();
        assert_eq!(calls, 3);
        assert_eq!(
            err.to_string(),
            format!(
                "Failed to create temp directory in {}: \
                 No usable temporary directory name found after 3 attempts",
                parent.path().display()
            )
        );
    }

    /// An error other than `AlreadyExists` is returned at once, naming the
    /// candidate path, without drawing another name.
    #[test]
    fn create_unique_dir_returns_other_errors_immediately() {
        let parent = tempfile::tempdir().unwrap();
        let missing = parent.path().join("missing");
        let mut calls = 0;
        let err = create_unique_dir(&missing, 10, || {
            calls += 1;
            Ok("abcdefgh".to_string())
        })
        .unwrap_err();
        assert_eq!(calls, 1);
        let SessionError::TempDir { path, source } = &err else {
            panic!("expected SessionError::TempDir, got {err:?}");
        };
        assert_eq!(path, &missing.join("abcdefgh"));
        assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
    }

    /// A failure to generate a name is reported against the parent directory.
    #[test]
    fn create_unique_dir_reports_name_generation_failure() {
        let parent = tempfile::tempdir().unwrap();
        let err = create_unique_dir(parent.path(), 10, || {
            Err(std::io::Error::other("no entropy"))
        })
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "Failed to create temp directory in {}: no entropy",
                parent.path().display()
            )
        );
    }

    /// The session working directory is created directly under the given
    /// root with an 8-character name, and secured like any other `TempDir`.
    #[test]
    fn new_session_working_dir_creates_short_name_under_root() {
        let parent = tempfile::tempdir().unwrap();
        let mut td = TempDir::new_session_working_dir(parent.path(), None).unwrap();
        assert_eq!(td.path().parent(), Some(parent.path()));
        let name = td.path().file_name().unwrap().to_str().unwrap();
        assert_eq!(name.len(), SESSION_DIR_NAME_LENGTH);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(td.path()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        td.cleanup().unwrap();
        assert!(!td.path().exists());
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
