// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Secure temporary directory creation — mirrors Python `_tempdir.py`.

use std::path::{Path, PathBuf};
use std::time::Duration;

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
    /// [`SESSION_DIR_NAME_LENGTH`] random characters from
    /// [`SESSION_DIR_NAME_ALPHABET`].
    ///
    /// A name that already exists is replaced by a fresh one, up to
    /// [`SESSION_DIR_NAME_ATTEMPTS`] times.
    pub(crate) fn new_session_working_dir(
        dir: &Path,
        user: Option<&dyn SessionUser>,
    ) -> Result<Self, SessionError> {
        Self::new_session_working_dir_with(dir, user, random_session_dir_name)
    }

    /// [`TempDir::new_session_working_dir`] with the candidate names supplied
    /// by `next_name`, so tests can force collisions.
    fn new_session_working_dir_with(
        dir: &Path,
        user: Option<&dyn SessionUser>,
        next_name: impl FnMut() -> std::io::Result<String>,
    ) -> Result<Self, SessionError> {
        let path = create_unique_dir(dir, SESSION_DIR_NAME_ATTEMPTS, next_name)?;
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
    ///
    /// On Windows this can block for about 31 s. A removal that leaves anything
    /// behind is retried after 1, 2, 4, 8 and 16 s, because a process that has
    /// just exited, or been terminated, can hold its file handles for a while
    /// afterwards. Each pass deletes everything it can, and each failed pass is
    /// logged as a warning naming the files that could not be deleted and the
    /// processes holding them open. On other platforms a failure is returned at
    /// once.
    pub fn cleanup(&mut self) -> Result<(), SessionError> {
        self.cleanup_with_delays(None, CLEANUP_RETRY_DELAYS)
    }

    /// [`Self::cleanup`], logging each failed attempt against `session_id` and
    /// retrying after each of `delays`.
    pub(crate) fn cleanup_with_delays(
        &mut self,
        session_id: Option<&str>,
        delays: &[Duration],
    ) -> Result<(), SessionError> {
        if self.cleaned_up {
            return Ok(());
        }
        self.cleaned_up = true;
        let dir = self.path.clone();
        remove_with_retry(
            &self.path,
            delays,
            remove_pass,
            std::thread::sleep,
            |failure, retry_in| {
                if cfg!(windows) {
                    log_failed_attempt(session_id, &dir, failure, retry_in);
                }
            },
        )
        .map_err(|e| SessionError::TempDir {
            path: self.path.clone(),
            source: e,
        })
    }

    /// Record the directory as removed without touching the disk. For a
    /// directory nested in one whose own cleanup removes it.
    pub(crate) fn mark_cleaned_up(&mut self) {
        self.cleaned_up = true;
    }
}

/// Delays before each retry of a failed [`TempDir::cleanup`]: about 31 s in total.
///
/// Windows refuses to delete a file or directory while a process holds a handle
/// to it that was not opened with `FILE_SHARE_DELETE`. Other platforms do not
/// retry: a failure there is a permission
/// problem that waiting will not fix.
#[cfg(windows)]
pub(crate) const CLEANUP_RETRY_DELAYS: &[Duration] = &[
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(16),
];
/// Delays before each retry of a failed [`TempDir::cleanup`]. None off Windows.
#[cfg(not(windows))]
pub(crate) const CLEANUP_RETRY_DELAYS: &[Duration] = &[];

/// Most undeletable files a failed-attempt log line looks up holders for.
#[cfg(any(windows, test))]
const MAX_FILES_TO_INSPECT: usize = 256;
/// Most undeletable file names a failed-attempt log line lists.
const MAX_FILES_TO_NAME: usize = 3;

/// Content flags of the cleanup warning and error log lines.
pub(crate) const CLEANUP_LOG_CONTENT: crate::logging::LogContent =
    crate::logging::LogContent::FILE_PATH
        .union(crate::logging::LogContent::PROCESS_CONTROL)
        .union(crate::logging::LogContent::EXCEPTION_INFO);

/// A process holding a file open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileHolder {
    pub(crate) pid: u32,
    pub(crate) name: String,
}

/// What one removal pass left behind.
#[derive(Debug)]
pub(crate) struct PassFailure {
    /// The first error the pass hit.
    pub(crate) error: std::io::Error,
    /// The files that could not be deleted, in the order the pass reached
    /// them, at most [`MAX_FILES_TO_INSPECT`]. Directories are not listed.
    pub(crate) files: Vec<PathBuf>,
    /// How many files could not be deleted, including any beyond `files`.
    pub(crate) file_count: usize,
}

impl PassFailure {
    /// Summarize a pass's `(path, is_dir, error)` failures. `None` when there were none.
    #[cfg(any(windows, test))]
    pub(crate) fn from_failures(
        failures: impl IntoIterator<Item = (PathBuf, bool, std::io::Error)>,
    ) -> Option<Self> {
        let mut first = None;
        let mut files = Vec::new();
        let mut file_count = 0;
        for (path, is_dir, error) in failures {
            if !is_dir {
                file_count += 1;
                if files.len() < MAX_FILES_TO_INSPECT {
                    files.push(path);
                }
            }
            first.get_or_insert(error);
        }
        first.map(|error| Self {
            error,
            files,
            file_count,
        })
    }
}

impl From<std::io::Error> for PassFailure {
    fn from(error: std::io::Error) -> Self {
        Self {
            error,
            files: Vec::new(),
            file_count: 0,
        }
    }
}

/// One removal pass over `path`.
///
/// Windows deletes everything it can and reports each file it could not, as
/// Python's `shutil.rmtree` does with an error handler. Elsewhere this is
/// `std::fs::remove_dir_all`, which stops at the first error: cleanup does not
/// retry there, so the extra detail would go unused.
fn remove_pass(path: &Path) -> Result<(), PassFailure> {
    #[cfg(windows)]
    {
        let failures = crate::win32_remove::remove_tree(path)
            .into_iter()
            .map(|f| (f.path, f.is_dir, f.error));
        PassFailure::from_failures(failures).map_or(Ok(()), Err)
    }
    #[cfg(not(windows))]
    {
        std::fs::remove_dir_all(path).map_err(PassFailure::from)
    }
}

/// Calls `remove` on `path`, and again after each of `delays` until it succeeds.
///
/// `on_failure` is called after every failed attempt with the delay before
/// the next one, or `None` after the last. `NotFound` on the first attempt is
/// returned as is, with no `on_failure` call. On a retry it counts as success:
/// an earlier attempt that reported a failure partway through can still have
/// removed the directory.
fn remove_with_retry(
    path: &Path,
    delays: &[Duration],
    mut remove: impl FnMut(&Path) -> Result<(), PassFailure>,
    mut sleep: impl FnMut(Duration),
    mut on_failure: impl FnMut(&PassFailure, Option<Duration>),
) -> std::io::Result<()> {
    let mut result = remove(path);
    for &delay in delays {
        let Err(failure) = &result else { break };
        if failure.error.kind() == std::io::ErrorKind::NotFound {
            break;
        }
        on_failure(failure, Some(delay));
        sleep(delay);
        result = match remove(path) {
            Err(f) if f.error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        };
    }
    if let Err(failure) = &result {
        if failure.error.kind() != std::io::ErrorKind::NotFound {
            on_failure(failure, None);
        }
    }
    result.map_err(|f| f.error)
}

/// A cleanup log line: its level, content flags and text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CleanupLog {
    pub(crate) level: log::Level,
    pub(crate) content: crate::logging::LogContent,
    pub(crate) message: String,
}

/// Send `line` to the `openjd.sessions` logger, tagged with `session_id` when given.
pub(crate) fn emit_cleanup_log(session_id: Option<&str>, line: &CleanupLog) {
    let bits = line.content.bits();
    let ts = crate::logging::timestamp_usec();
    match session_id {
        Some(id) => log::log!(
            target: "openjd.sessions",
            line.level,
            session_id = id,
            openjd_log_content = bits,
            openjd_timestamp_usec = ts;
            "{}", line.message
        ),
        None => log::log!(
            target: "openjd.sessions",
            line.level,
            openjd_log_content = bits,
            openjd_timestamp_usec = ts;
            "{}", line.message
        ),
    }
}

/// Log one failed removal of `dir`: what is left, and who is holding it open.
fn log_failed_attempt(
    session_id: Option<&str>,
    dir: &Path,
    failure: &PassFailure,
    retry_in: Option<Duration>,
) {
    #[cfg(windows)]
    let holders = (!failure.files.is_empty())
        .then(|| crate::win32_file_locks::processes_holding(&failure.files));
    #[cfg(not(windows))]
    let holders: Option<Result<Vec<FileHolder>, String>> = None;
    let line = failed_attempt_log(dir, failure, holders.as_ref(), retry_in);
    emit_cleanup_log(session_id, &line);
}

/// The log line for one failed removal of `dir`. `holders` is `None` when no
/// lookup was made. The text matches Python's `TempDir.cleanup`.
pub(crate) fn failed_attempt_log(
    dir: &Path,
    failure: &PassFailure,
    holders: Option<&Result<Vec<FileHolder>, String>>,
    retry_in: Option<Duration>,
) -> CleanupLog {
    let mut msg = format!(
        "Could not delete temporary directory {}: {}.",
        dir.display(),
        os_error_text(&failure.error)
    );
    if failure.file_count > 0 {
        let names: Vec<String> = failure
            .files
            .iter()
            .take(MAX_FILES_TO_NAME)
            .map(|p| p.strip_prefix(dir).unwrap_or(p).display().to_string())
            .collect();
        msg.push_str(&format!(
            " {} file(s) could not be deleted, including {}.",
            failure.file_count,
            names.join(", ")
        ));
        msg.push_str(&describe_holders(holders));
    }
    match retry_in {
        Some(d) => msg.push_str(&format!(" Retrying in {} s.", d.as_secs_f64())),
        None => msg.push_str(" Giving up."),
    }
    CleanupLog {
        level: log::Level::Warn,
        content: CLEANUP_LOG_CONTENT,
        message: msg,
    }
}

/// An error's text without Rust's " (os error N)" suffix or a trailing period,
/// matching Python's `OSError.strerror`.
pub(crate) fn os_error_text(err: &std::io::Error) -> String {
    let text = err.to_string();
    let text = match text.rsplit_once(" (os error ") {
        Some((head, tail)) if tail.ends_with(')') => head.to_string(),
        _ => text,
    };
    text.trim_end().trim_end_matches('.').to_string()
}

/// The sentence naming who holds the remaining files. Empty when no lookup was made.
fn describe_holders(holders: Option<&Result<Vec<FileHolder>, String>>) -> String {
    match holders {
        Some(Ok(h)) if !h.is_empty() => {
            let mut seen = std::collections::HashSet::new();
            let list: Vec<String> = h
                .iter()
                .filter(|x| seen.insert(x.pid))
                .map(|x| format!("{} (pid {})", x.name, x.pid))
                .collect();
            format!(" Files are still locked open by: {}.", list.join(", "))
        }
        Some(Ok(_)) => " No running process was found holding them open.".to_string(),
        Some(Err(e)) => format!(" The processes holding them open could not be determined: {e}."),
        None => String::new(),
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
/// Kept short because the path counts against Windows `MAX_PATH` (260) for
/// job applications that are not long-path aware. The name carries no part
/// of the session ID; the session log records the path beside the ID.
/// Matches openjd-sessions-for-python PR #348.
pub(crate) const SESSION_DIR_NAME_LENGTH: usize = 8;

/// Characters a session working directory's name is drawn from. Lower case
/// only, so two names never differ only in case (which case-insensitive
/// filesystems treat as the same name), and no character is reserved on
/// Windows.
pub(crate) const SESSION_DIR_NAME_ALPHABET: &[u8; 37] = b"abcdefghijklmnopqrstuvwxyz0123456789_";

/// How many names [`TempDir::new_session_working_dir`] tries before giving
/// up.
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

/// Create a new directory under `parent` named by `next_name`, drawing a
/// fresh name each time the candidate already exists.
///
/// Each attempt is an exclusive `create_dir`, so an existing directory is
/// never reused and concurrent callers never share a path. Any other error,
/// or a failure of `next_name` (reported against `parent`), is returned at
/// once. After `attempts` names that all exist, fails with an
/// `AlreadyExists` error naming `parent`.
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

    fn sharing_violation() -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "file in use")
    }

    fn failure(error: std::io::Error) -> PassFailure {
        PassFailure::from(error)
    }

    #[test]
    fn remove_retry_stops_at_the_first_success() {
        let mut calls = 0;
        let mut slept = Vec::new();
        let delays = [
            Duration::from_millis(1),
            Duration::from_millis(2),
            Duration::from_millis(3),
        ];
        let result = remove_with_retry(
            Path::new("unused"),
            &delays,
            |_| {
                calls += 1;
                if calls < 3 {
                    Err(failure(sharing_violation()))
                } else {
                    Ok(())
                }
            },
            |d| slept.push(d),
            |_, _| {},
        );
        assert!(result.is_ok());
        assert_eq!(calls, 3);
        assert_eq!(slept, delays[..2]);
    }

    #[test]
    fn remove_retry_reports_each_failure_with_the_next_delay() {
        let mut reported = Vec::new();
        let delays = [Duration::from_millis(1), Duration::from_millis(2)];
        let _ = remove_with_retry(
            Path::new("unused"),
            &delays,
            |_| Err(failure(sharing_violation())),
            |_| {},
            |f, next| reported.push((f.error.kind(), next)),
        );
        let kind = std::io::ErrorKind::PermissionDenied;
        assert_eq!(
            reported,
            [
                (kind, Some(delays[0])),
                (kind, Some(delays[1])),
                (kind, None)
            ]
        );
    }

    #[test]
    fn remove_retry_reports_nothing_when_the_first_attempt_succeeds() {
        let mut reported = 0;
        let result = remove_with_retry(
            Path::new("unused"),
            &[Duration::ZERO],
            |_| Ok(()),
            |_| {},
            |_, _| reported += 1,
        );
        assert!(result.is_ok());
        assert_eq!(reported, 0);
    }

    #[test]
    fn remove_retry_returns_the_last_error_when_exhausted() {
        let mut calls = 0;
        let delays = [Duration::ZERO, Duration::ZERO];
        let result = remove_with_retry(
            Path::new("unused"),
            &delays,
            |_| {
                calls += 1;
                Err(failure(sharing_violation()))
            },
            |_| {},
            |_, _| {},
        );
        let err = result.expect_err("every attempt failed");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(calls, 3, "one attempt plus one per delay");
    }

    #[test]
    fn remove_retry_treats_not_found_on_a_retry_as_success() {
        let mut calls = 0;
        let result = remove_with_retry(
            Path::new("unused"),
            &[Duration::ZERO],
            |_| {
                calls += 1;
                if calls == 1 {
                    Err(failure(sharing_violation()))
                } else {
                    Err(failure(std::io::ErrorKind::NotFound.into()))
                }
            },
            |_| {},
            |_, _| {},
        );
        assert!(result.is_ok());
    }

    #[test]
    fn remove_retry_keeps_not_found_on_the_first_attempt_as_an_error() {
        let mut reported = 0;
        let result = remove_with_retry(
            Path::new("unused"),
            &[Duration::ZERO],
            |_| Err(failure(std::io::ErrorKind::NotFound.into())),
            |_| {},
            |_, _| reported += 1,
        );
        assert_eq!(
            result.expect_err("directory was never there").kind(),
            std::io::ErrorKind::NotFound
        );
        assert_eq!(reported, 0, "a missing directory is not a lock to report");
    }

    #[test]
    fn pass_failure_keeps_the_first_error_and_counts_only_files() {
        let io = |s: &str| std::io::Error::other(s.to_string());
        let mut entries = vec![
            (PathBuf::from("root/a/locked.bin"), false, io("first")),
            (PathBuf::from("root/a"), true, io("dir")),
        ];
        for i in 0..300 {
            entries.push((PathBuf::from(format!("root/f{i}")), false, io("later")));
        }
        let f = PassFailure::from_failures(entries).expect("there were failures");
        assert_eq!(f.error.to_string(), "first");
        assert_eq!(f.file_count, 301);
        assert_eq!(f.files.len(), 256, "the inspect cap, as a literal");
        assert_eq!(f.files[0], PathBuf::from("root/a/locked.bin"));
        assert_eq!(f.files[1], PathBuf::from("root/f0"));
        assert!(PassFailure::from_failures(Vec::new()).is_none());
    }

    #[test]
    fn cleanup_is_idempotent_after_success() {
        let mut td = TempDir::new(None, None, None).unwrap();
        td.cleanup().unwrap();
        assert!(!td.path().exists());
        // A second call touches nothing and still reports success.
        td.cleanup().unwrap();
    }

    #[test]
    fn cleanup_after_a_failure_is_a_no_op_ok() {
        // The first cleanup fails: the directory is already gone, a NotFound
        // that is an error on the first pass. The second call returns Ok
        // without a disk touch, so the path is still absent when it succeeds.
        let mut td = TempDir::new(None, None, None).unwrap();
        std::fs::remove_dir(td.path()).unwrap();
        assert!(td.cleanup().is_err());
        assert!(!td.path().exists());
        td.cleanup().expect("second call is a no-op Ok");
    }

    #[test]
    fn os_error_text_drops_the_os_error_suffix_and_trailing_period() {
        let os = std::io::Error::from_raw_os_error(2);
        let text = os_error_text(&os);
        assert!(!text.contains("(os error"), "{text}");
        assert!(!text.ends_with('.'), "{text}");
        assert_eq!(
            text,
            os.to_string()
                .split(" (os error ")
                .next()
                .unwrap()
                .trim_end_matches('.')
        );
        assert_eq!(os_error_text(&std::io::Error::other("plain.")), "plain");
        assert_eq!(
            os_error_text(&std::io::Error::other("a (os error x")),
            "a (os error x"
        );
    }

    #[test]
    fn cleanup_log_content_is_file_path_process_control_and_exception_info() {
        // Written as a literal so a changed flag fails here: FILE_PATH (2) |
        // EXCEPTION_INFO (16) | PROCESS_CONTROL (32).
        assert_eq!(CLEANUP_LOG_CONTENT.bits(), 50);
    }

    fn holder(pid: u32, name: &str) -> FileHolder {
        FileHolder {
            pid,
            name: name.to_string(),
        }
    }

    fn files_failure(dir: &Path, files: &[&str], count: usize) -> PassFailure {
        PassFailure {
            error: std::io::Error::other("The process cannot access the file."),
            files: files.iter().map(|f| dir.join(f)).collect(),
            file_count: count,
        }
    }

    #[test]
    fn failed_attempt_names_files_holders_and_next_delay() {
        let dir = Path::new("root");
        let f = files_failure(dir, &["a.bin", "b.bin", "c.bin", "d.bin"], 4);
        let holders = Ok(vec![
            holder(42, "UnrealShaderCompileWorker.exe"),
            holder(42, "UnrealShaderCompileWorker.exe"),
            holder(7, "p4.exe"),
        ]);
        let line = failed_attempt_log(dir, &f, Some(&holders), Some(Duration::from_secs(4)));
        assert_eq!(
            line,
            CleanupLog {
                level: log::Level::Warn,
                content: CLEANUP_LOG_CONTENT,
                message: format!(
                    "Could not delete temporary directory root: The process cannot access the \
                     file. 4 file(s) could not be deleted, including {}, {}, {}. Files are still \
                     locked open by: UnrealShaderCompileWorker.exe (pid 42), p4.exe (pid 7). \
                     Retrying in 4 s.",
                    Path::new("a.bin").display(),
                    Path::new("b.bin").display(),
                    Path::new("c.bin").display(),
                ),
            }
        );
    }

    #[test]
    fn failed_attempt_says_when_no_holder_was_found_or_lookup_failed() {
        let dir = Path::new("root");
        let f = files_failure(dir, &["a"], 1);
        let head = "Could not delete temporary directory root: The process cannot access the \
                    file. 1 file(s) could not be deleted, including a.";
        let none = Ok(Vec::new());
        assert_eq!(
            failed_attempt_log(dir, &f, Some(&none), None).message,
            format!("{head} No running process was found holding them open. Giving up.")
        );
        let failed = Err("RmGetList failed (5)".to_string());
        assert_eq!(
            failed_attempt_log(dir, &f, Some(&failed), None).message,
            format!(
                "{head} The processes holding them open could not be determined: RmGetList \
                 failed (5). Giving up."
            )
        );
        assert_eq!(
            failed_attempt_log(dir, &f, None, None).message,
            format!("{head} Giving up.")
        );
    }

    #[test]
    fn failed_attempt_with_no_undeletable_files_names_only_the_directory() {
        let dir = Path::new("root");
        let f = failure(sharing_violation());
        let none = Ok(Vec::new());
        assert_eq!(
            failed_attempt_log(dir, &f, Some(&none), Some(Duration::from_secs(1))).message,
            "Could not delete temporary directory root: file in use. Retrying in 1 s."
        );
    }

    /// Delete everything in a fresh temp dir via the Windows pass, with
    /// `locked` held open throughout; return the warnings logged.
    #[cfg(windows)]
    fn warnings_for_locked_file_among_siblings(
        locked: &str,
        siblings: &[String],
        hold_for: Duration,
    ) -> (PathBuf, Vec<String>) {
        use std::os::windows::fs::OpenOptionsExt;
        testing_logger::setup();
        let mut td = TempDir::new(None, None, None).unwrap();
        let root = td.path().to_path_buf();
        for s in siblings {
            let p = root.join(s);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"x").unwrap();
        }
        let lp = root.join(locked);
        std::fs::create_dir_all(lp.parent().unwrap()).unwrap();
        let held = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(&lp)
            .unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(hold_for);
            drop(held);
        });
        let result = td.cleanup_with_delays(Some("session-1"), &[Duration::from_millis(300)]);
        release.join().unwrap();
        let warnings = std::cell::RefCell::new(Vec::new());
        testing_logger::validate(|logs| {
            *warnings.borrow_mut() = logs
                .iter()
                .filter(|l| l.level == log::Level::Warn)
                .map(|l| l.body.clone())
                .collect();
        });
        assert!(result.is_ok(), "{result:?}");
        (root, warnings.into_inner())
    }

    /// The warning names the locked file and its holder, even when hundreds of
    /// deletable files sit beside it.
    #[cfg(windows)]
    #[test]
    fn cleanup_names_the_locked_file_not_its_siblings() {
        let siblings: Vec<String> = (0..300).map(|i| format!("b{i:03}.txt")).collect();
        let (root, warnings) = warnings_for_locked_file_among_siblings(
            r"a_sub\locked.bin",
            &siblings,
            Duration::from_millis(150),
        );
        assert_eq!(
            warnings,
            [format!(
                "Could not delete temporary directory {}: The process cannot access the file \
                 because it is being used by another process. 1 file(s) could not be deleted, \
                 including a_sub\\locked.bin. Files are still locked open by: {} (pid {}). \
                 Retrying in 0.3 s.",
                root.display(),
                this_exe_name(),
                std::process::id()
            )]
        );
    }

    /// One failed pass already deletes every file that is not locked.
    #[cfg(windows)]
    #[test]
    fn a_failed_pass_leaves_only_the_locked_file() {
        use std::os::windows::fs::OpenOptionsExt;
        let td = TempDir::new(None, None, None).unwrap();
        let root = td.path().to_path_buf();
        std::fs::create_dir(root.join("a_sub")).unwrap();
        let held = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(root.join("a_sub").join("locked.bin"))
            .unwrap();
        for i in 0..50 {
            std::fs::write(root.join(format!("b{i:03}.txt")), b"x").unwrap();
            std::fs::write(root.join("a_sub").join(format!("c{i:03}.txt")), b"x").unwrap();
        }
        let f = remove_pass(&root).expect_err("locked file");
        assert_eq!(f.file_count, 1);
        assert_eq!(f.files, [root.join("a_sub").join("locked.bin")]);
        assert_eq!(f.error.raw_os_error(), Some(32));
        let left: Vec<PathBuf> = walk(&root);
        assert_eq!(left, [root.join("a_sub").join("locked.bin")]);
        drop(held);
        assert!(remove_pass(&root).is_ok());
        assert!(!root.exists());
    }

    #[cfg(windows)]
    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            if e.file_type().unwrap().is_dir() {
                out.extend(walk(&e.path()));
            } else {
                out.push(e.path());
            }
        }
        out
    }

    #[cfg(windows)]
    fn this_exe_name() -> String {
        std::env::current_exe()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    #[cfg(not(windows))]
    #[test]
    fn cleanup_does_not_retry_off_windows() {
        assert!(CLEANUP_RETRY_DELAYS.is_empty());
    }

    /// Written as literals so a changed constant fails here.
    #[cfg(windows)]
    #[test]
    fn cleanup_retry_delays_double_from_one_second() {
        let secs: Vec<u64> = CLEANUP_RETRY_DELAYS.iter().map(|d| d.as_secs()).collect();
        assert_eq!(secs, [1, 2, 4, 8, 16]);
    }

    /// A file held open without `FILE_SHARE_DELETE` for less time than the
    /// retries cover: the first removal fails with a sharing violation, a
    /// retry succeeds, and the failed attempt is logged naming the holder.
    #[cfg(windows)]
    #[test]
    fn cleanup_waits_for_a_briefly_held_file() {
        let (root, warnings) =
            warnings_for_locked_file_among_siblings("locked.bin", &[], Duration::from_millis(150));
        assert!(!root.exists());
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("including locked.bin."),
            "{}",
            warnings[0]
        );
        assert!(
            warnings[0].ends_with("Retrying in 0.3 s."),
            "{}",
            warnings[0]
        );
    }

    /// A file held open for the whole cleanup is reported by path, with the
    /// sharing-violation error, after every attempt.
    #[cfg(windows)]
    #[test]
    fn cleanup_reports_a_file_held_throughout() {
        use std::os::windows::fs::OpenOptionsExt;
        let td = TempDir::new(None, None, None).unwrap();
        let held = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(td.path().join("locked.bin"))
            .unwrap();
        let mut reported = Vec::new();
        let result = remove_with_retry(
            td.path(),
            &[Duration::from_millis(50)],
            remove_pass,
            std::thread::sleep,
            |f, next| reported.push((f.files.clone(), f.file_count, next)),
        );
        let err = result.expect_err("held throughout");
        assert_eq!(err.raw_os_error(), Some(32), "{err}");
        let locked = vec![td.path().join("locked.bin")];
        assert_eq!(
            reported,
            [
                (locked.clone(), 1, Some(Duration::from_millis(50))),
                (locked, 1, None)
            ]
        );
        drop(held);
        drop(td);
    }

    /// Runs its closure when dropped, so a failing Windows test still stops the
    /// process it started, lifts an ACL it added, and removes its directory.
    #[cfg(windows)]
    struct OnDrop<F: FnMut()>(F);

    #[cfg(windows)]
    impl<F: FnMut()> Drop for OnDrop<F> {
        fn drop(&mut self) {
            (self.0)();
        }
    }

    /// A copy of `ping.exe` at `dir\name`.
    #[cfg(windows)]
    fn copy_ping(dir: &Path, name: &str) -> PathBuf {
        let exe = dir.join(name);
        let system = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        std::fs::copy(Path::new(&system).join("System32").join("ping.exe"), &exe).unwrap();
        exe
    }

    /// Runs `program args` with no console window and waits for success.
    #[cfg(windows)]
    fn run_hidden(program: &str, args: &[&std::ffi::OsStr]) {
        use std::os::windows::process::CommandExt;
        let status = std::process::Command::new(program)
            .args(args)
            .stdout(std::process::Stdio::null())
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .status()
            .unwrap();
        assert!(status.success(), "{program} {args:?}");
    }

    /// Starts `exe` (a copy of `ping.exe`) with no console window.
    #[cfg(windows)]
    fn run_ping(exe: &Path) -> std::process::Child {
        use std::os::windows::process::CommandExt;
        std::process::Command::new(exe)
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn()
            .unwrap()
    }

    /// The holders listed in a warning that ends "Giving up.", given the text
    /// before the list.
    #[cfg(windows)]
    fn holders_in<'a>(body: &'a str, start: &str) -> Vec<&'a str> {
        body.strip_prefix(start)
            .and_then(|rest| rest.strip_suffix(". Giving up."))
            .unwrap_or_else(|| panic!("unexpected warning: {body}"))
            .split(", ")
            .collect()
    }

    /// The holder lookup covers every file reported, not only the first: a file
    /// this process holds, and a running program held by its own process.
    #[cfg(windows)]
    #[test]
    fn failed_attempt_names_the_holders_of_every_file() {
        use std::os::windows::fs::OpenOptionsExt;
        testing_logger::setup();
        let mut td = TempDir::new(None, None, None).unwrap();
        let dir = td.path().to_path_buf();
        let held = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(dir.join("a_locked.bin"))
            .unwrap();
        let exe = copy_ping(&dir, "b_ping.exe");
        let mut child = run_ping(&exe);
        let ping_pid = child.id();
        let guard_dir = dir.clone();
        let mut held = Some(held);
        let _restore = OnDrop(move || {
            let _ = child.kill();
            let _ = child.wait();
            held.take();
            let _ = std::fs::remove_dir_all(&guard_dir);
        });

        let result = td.cleanup_with_delays(Some("session-1"), &[]);

        assert!(result.is_err(), "held files were deleted");
        let start = format!(
            "Could not delete temporary directory {}: The process cannot access the file \
             because it is being used by another process. 2 file(s) could not be deleted, \
             including a_locked.bin, b_ping.exe. Files are still locked open by: ",
            dir.display()
        );
        let this = format!("{} (pid {})", this_exe_name(), std::process::id());
        let ping = format!("b_ping.exe (pid {ping_pid})");
        testing_logger::validate(|logs| {
            let warns: Vec<String> = logs
                .iter()
                .filter(|l| l.level == log::Level::Warn)
                .map(|l| l.body.clone())
                .collect();
            assert_eq!(warns.len(), 1, "{warns:?}");
            let holders = holders_in(&warns[0], &start);
            // A scanner inspecting the new executable can appear too.
            assert!(holders.contains(&this.as_str()), "{}", warns[0]);
            assert!(holders.contains(&ping.as_str()), "{}", warns[0]);
        });
    }

    /// A running program whose ACL denies writing: deleting it fails with access
    /// denied, and the warning names it and its process.
    #[cfg(windows)]
    #[test]
    fn cleanup_names_a_running_program_that_refuses_writing() {
        testing_logger::setup();
        let mut td = TempDir::new(None, None, None).unwrap();
        let dir = td.path().to_path_buf();
        let exe = copy_ping(&dir, "ping.exe");
        let everyone = "*S-1-1-0";
        let deny = format!("{everyone}:(WD)");
        run_hidden(
            "icacls",
            &[exe.as_os_str(), "/deny".as_ref(), deny.as_ref()],
        );
        let mut child = run_ping(&exe);
        let pid = child.id();
        let (guard_exe, guard_dir) = (exe.clone(), dir.clone());
        let _restore = OnDrop(move || {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::process::Command::new("icacls")
                .args([
                    guard_exe.as_os_str(),
                    "/remove:d".as_ref(),
                    everyone.as_ref(),
                ])
                .stdout(std::process::Stdio::null())
                .status();
            let _ = std::fs::remove_dir_all(&guard_dir);
        });

        let result = td.cleanup_with_delays(Some("session-1"), &[]);

        assert!(result.is_err(), "the running image was deleted");
        let start = format!(
            "Could not delete temporary directory {}: Access is denied. 1 file(s) could not \
             be deleted, including ping.exe. Files are still locked open by: ",
            dir.display()
        );
        let ping = format!("ping.exe (pid {pid})");
        testing_logger::validate(|logs| {
            let warns: Vec<String> = logs
                .iter()
                .filter(|l| l.level == log::Level::Warn)
                .map(|l| l.body.clone())
                .collect();
            assert_eq!(warns.len(), 1, "{warns:?}");
            assert!(
                holders_in(&warns[0], &start).contains(&ping.as_str()),
                "{}",
                warns[0]
            );
        });
    }

    /// A file whose ACLs refuse deletion (the file denies DELETE and its
    /// directory denies deleting children) is reported, and no process holds it.
    #[cfg(windows)]
    #[test]
    fn cleanup_reports_a_file_the_acls_protect() {
        testing_logger::setup();
        let mut td = TempDir::new(None, None, None).unwrap();
        let dir = td.path().to_path_buf();
        let sub = dir.join("sub");
        std::fs::create_dir(&sub).unwrap();
        let file = sub.join("protected.bin");
        std::fs::write(&file, b"x").unwrap();
        let everyone = "*S-1-1-0";
        run_hidden(
            "icacls",
            &[
                file.as_os_str(),
                "/deny".as_ref(),
                format!("{everyone}:(DE)").as_ref(),
            ],
        );
        run_hidden(
            "icacls",
            &[
                sub.as_os_str(),
                "/deny".as_ref(),
                format!("{everyone}:(DC)").as_ref(),
            ],
        );
        let (gf, gs, gd) = (file.clone(), sub.clone(), dir.clone());
        let _restore = OnDrop(move || {
            for p in [&gs, &gf] {
                let _ = std::process::Command::new("icacls")
                    .args([p.as_os_str(), "/remove:d".as_ref(), everyone.as_ref()])
                    .stdout(std::process::Stdio::null())
                    .status();
            }
            let _ = std::fs::remove_dir_all(&gd);
        });

        let result = td.cleanup_with_delays(Some("session-1"), &[]);

        assert!(result.is_err(), "the protected file was deleted");
        assert!(file.exists());
        testing_logger::validate(|logs| {
            let warns: Vec<String> = logs
                .iter()
                .filter(|l| l.level == log::Level::Warn)
                .map(|l| l.body.clone())
                .collect();
            assert_eq!(warns.len(), 1, "{warns:?}");
            assert_eq!(
                warns[0],
                format!(
                    "Could not delete temporary directory {}: Access is denied. 1 file(s) could \
                     not be deleted, including {}. No running process was found holding them \
                     open. Giving up.",
                    dir.display(),
                    Path::new("sub").join("protected.bin").display()
                )
            );
        });
    }

    /// The naming parameters match tempdir.md, written as literals so a
    /// changed constant fails here.
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

    /// Bytes >= 222 are rejected and each kept byte maps to
    /// `ALPHABET[byte % 37]`. 221, the largest kept byte, maps to `_`.
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

    /// Every alphabet character is reachable, including the last one (`_`).
    #[test]
    fn random_session_dir_name_uses_whole_alphabet() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..2000 {
            seen.extend(random_session_dir_name().unwrap().bytes());
        }
        assert_eq!(seen.len(), SESSION_DIR_NAME_ALPHABET.len(), "{seen:?}");
    }

    /// An existing name is skipped and left untouched, and the next
    /// candidate is created.
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

    /// The 10,000th candidate is still tried, and 10,000 taken names fail.
    #[test]
    fn new_session_working_dir_retries_up_to_attempt_bound() {
        let parent = tempfile::tempdir().unwrap();
        std::fs::create_dir(parent.path().join("taken000")).unwrap();

        let mut calls = 0;
        let mut td = TempDir::new_session_working_dir_with(parent.path(), None, || {
            calls += 1;
            Ok(if calls < 10_000 {
                "taken000"
            } else {
                "fresh000"
            }
            .to_string())
        })
        .unwrap();
        assert_eq!(calls, 10_000);
        assert_eq!(td.path(), parent.path().join("fresh000"));
        td.cleanup().unwrap();

        let err = TempDir::new_session_working_dir_with(parent.path(), None, || {
            Ok("taken000".to_string())
        })
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "Failed to create temp directory in {}: \
                 No usable temporary directory name found after 10000 attempts",
                parent.path().display()
            )
        );
    }

    /// The directory is created directly under the root with an
    /// 8-character name and, on POSIX, mode 0700.
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
