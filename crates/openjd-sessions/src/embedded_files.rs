// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Embedded file materialization.
//!
//! Mirrors Python `_embedded_files.py`.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use openjd_expr::function_library::FunctionLibrary;
use openjd_expr::path_mapping::PathFormat;
use openjd_expr::ExprValue;
use openjd_model::job::EmbeddedFile;
use openjd_model::symbol_table::SymbolTable;

use crate::error::SessionError;
use crate::logging::LogContent;
use crate::session_log;
use crate::session_user::SessionUser;

/// Scope for embedded file symbol table entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddedFilesScope {
    Step,
    Env,
}

impl EmbeddedFilesScope {
    pub fn prefix(&self) -> &'static str {
        match self {
            EmbeddedFilesScope::Step => "Task.File",
            EmbeddedFilesScope::Env => "Env.File",
        }
    }
}

/// Line ending mode for embedded files.
pub use openjd_model::types::EndOfLine;

/// Convert line endings in data based on the specified mode.
pub fn convert_line_endings(data: &str, eol: EndOfLine) -> Vec<u8> {
    match eol {
        EndOfLine::Lf => data.replace("\r\n", "\n").into_bytes(),
        EndOfLine::Crlf => {
            let normalized = data.replace("\r\n", "\n");
            normalized.replace('\n', "\r\n").into_bytes()
        }
        EndOfLine::Auto => {
            #[cfg(windows)]
            {
                convert_line_endings(data, EndOfLine::Crlf)
            }
            #[cfg(not(windows))]
            {
                convert_line_endings(data, EndOfLine::Lf)
            }
        }
    }
}

/// Write an embedded file to disk.
pub fn write_embedded_file(path: &Path, data: &str) -> Result<(), std::io::Error> {
    write_embedded_file_with_options(path, data, false, None)
}

/// Write an embedded file to disk with options.
pub fn write_embedded_file_with_options(
    path: &Path,
    data: &str,
    #[cfg_attr(not(unix), allow(unused))] runnable: bool,
    end_of_line: Option<EndOfLine>,
) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    match end_of_line {
        Some(eol) => fs::write(path, convert_line_endings(data, eol))?,
        None => fs::write(path, data)?,
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if runnable { 0o700 } else { 0o600 };
        let perms = fs::Permissions::from_mode(mode);
        fs::set_permissions(path, perms)?;
    }

    Ok(())
}

/// Change ownership and permissions of a file for cross-user access.
///
/// Sets the group to the user's group and adds group read/write (and execute if runnable).
/// Errors are propagated to match Python's behavior — if chown fails, permissions are
/// NOT widened (security: don't grant group access to the wrong group).
#[cfg(unix)]
pub fn chown_for_user(
    path: &Path,
    user: &dyn SessionUser,
    runnable: bool,
) -> Result<(), SessionError> {
    let grp = nix::unistd::Group::from_name(user.group())
        .map_err(|e| SessionError::PathPermissions {
            path: path.display().to_string(),
            reason: format!("Could not look up group '{}': {e}", user.group()),
        })?
        .ok_or_else(|| SessionError::PathPermissions {
            path: path.display().to_string(),
            reason: format!("Group '{}' not found", user.group()),
        })?;
    nix::unistd::chown(path, None, Some(grp.gid))
        .map_err(|e| SessionError::PathPermissions {
            path: path.display().to_string(),
            reason: format!(
                "Could not change ownership (error: {e}). Please ensure that uid {} is a member of group {}.",
                nix::unistd::geteuid(), user.group()
            ),
        })?;
    // Only widen permissions after chown succeeds — security: don't grant group
    // access if the group wasn't actually set.
    use std::os::unix::fs::PermissionsExt;
    let mode = if runnable { 0o770 } else { 0o660 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|e| {
        SessionError::PathPermissions {
            path: path.display().to_string(),
            reason: e.to_string(),
        }
    })?;
    Ok(())
}

/// Set ACL permissions on a file for cross-user access (Windows).
///
/// Sets process user as full control, session user as modify access.
#[cfg(windows)]
pub fn chown_for_user(
    path: &Path,
    user: &dyn SessionUser,
    _runnable: bool,
) -> Result<(), SessionError> {
    let process_user =
        crate::win32::get_process_user().map_err(|e| SessionError::PathPermissions {
            path: path.display().to_string(),
            reason: format!("Could not determine process user: {e}"),
        })?;
    crate::win32_permissions::set_permissions(
        &path.to_string_lossy(),
        &[process_user.as_str()],
        &[user.user()],
        &[],
    )
    .map_err(|e| SessionError::PathPermissions {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    Ok(())
}

/// Get the symbol table key for an embedded file.
pub fn symtab_key(scope: EmbeddedFilesScope, name: &str) -> String {
    format!("{}.{}", scope.prefix(), name)
}

fn random_hex_filename() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Validate a resolved embedded file filename for path traversal safety.
///
/// Per the 2023-09 spec (§6.1.1 `<Filename>`), an embedded file's `filename`
/// must be a plain basename — directory pathing is disallowed. The
/// `openjd-model` crate enforces this at template validation time by
/// rejecting `/` and `\` in the filename.
///
/// This function is a defense-in-depth check at the point where the
/// filename is joined to the target directory. It protects against:
/// - Bugs or gaps in model-layer validation.
/// - Call paths that reach the session layer without going through full
///   model validation (e.g. an `EmbeddedFile` constructed directly).
///
/// Note: `filename` is a plain string per the 2023-09 schema (not
/// `@fmtstring`), so no expression substitution can introduce separators
/// at session time — the value checked here is exactly the template value.
///
/// Rejects filenames that:
/// - Are empty.
/// - Contain any forward slash (`/`) or backslash (`\`). Backslashes are
///   rejected on all platforms because embedded file filenames are single
///   path components by spec.
/// - Contain a null byte.
/// - Equal `.` or `..`.
/// - Are not a single "normal" path component for the host platform. This
///   catches platform-specific prefixes and roots that contain no separator
///   and therefore slip past the checks above — most notably Windows
///   drive-relative paths like `D:relative`, which `Path::join` treats as a
///   drive prefix and would use to escape the target directory. `Path`
///   parses components for the OS the session runs on, so the rule is always
///   correct for the execution host.
/// - **On Windows only:** consist entirely of dots and spaces (`".. "`,
///   `"..."`, `". "`, ...). Windows strips trailing dots and spaces from the
///   final path component, so such a name collapses to `.` or `..` at the OS
///   layer and resolves to the current/parent directory — escaping the target.
///   Rust only special-cases the exact strings `.`/`..`, so these otherwise
///   parse as ordinary `Normal` components and slip past the checks above and
///   the lexical containment check. Any filename with a separator is already
///   rejected, so only a standalone all-dots-and-spaces name can reach this
///   rule. On POSIX these are legitimate, distinct filenames and are accepted.
///
/// Returns `Ok(())` if the filename is a safe single path component.
fn validate_resolved_filename(resolved: &str) -> Result<(), String> {
    if resolved.is_empty() {
        return Err("must not be empty".into());
    }
    if resolved.contains('\0') {
        return Err("must not contain null bytes".into());
    }
    if resolved.contains('/') || resolved.contains('\\') {
        return Err("must not contain path separators".into());
    }
    if resolved == "." || resolved == ".." {
        return Err(format!("must not be '{resolved}'"));
    }
    // On Windows the filesystem strips trailing dots and spaces from the final
    // path component, so a filename made up entirely of dots and spaces —
    // `".. "`, `"..."`, `". "`, etc. — collapses to `.` or `..` at the OS layer
    // and resolves to the current/parent directory, escaping the target. Rust
    // only special-cases the exact strings `.`/`..`, so these reach here as
    // ordinary `Normal` components and also pass the lexical containment check.
    // Any filename containing a separator is already rejected above, so only a
    // standalone all-dots-and-spaces name can get this far. On POSIX these are
    // legitimate, distinct filenames, so the rule is Windows-only.
    #[cfg(windows)]
    if resolved
        .trim_end_matches(|c| c == ' ' || c == '.')
        .is_empty()
    {
        return Err("must not consist only of dots and spaces".into());
    }
    // Require exactly one normal path component. On Windows this rejects
    // drive-relative anchors such as `D:relative` (parsed as `Prefix` +
    // `Normal`) and bare drives like `C:` (`Prefix` only), neither of which
    // contains a separator. On POSIX such strings are ordinary filenames and
    // are accepted.
    let mut components = Path::new(resolved).components();
    if !matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    ) {
        return Err("must be a single path component".into());
    }
    Ok(())
}

/// Lexically normalize a path by collapsing `.` and `..` components without
/// touching the filesystem.
///
/// Unlike [`std::fs::canonicalize`], this performs no I/O and does not resolve
/// symlinks — appropriate here because the target file does not exist yet and
/// we only want to reason about the path the caller constructed.
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Verify that `candidate` lexically resolves to a location inside `base`.
///
/// This is the final defense-in-depth guarantee before an embedded file is
/// created: even if filename validation were bypassed or a future change let
/// something through, the path we are about to write must still sit within the
/// session files directory. Comparison is lexical (see [`normalize_lexical`]),
/// so it holds regardless of whether the paths exist on disk yet.
///
/// Both `base` and `candidate` must be rooted; a relative path is rejected
/// with an error because lexical `..` handling is only sound for rooted paths.
fn ensure_within(base: &Path, candidate: &Path) -> Result<(), String> {
    // Lexical containment is only sound when both paths are rooted. For a
    // relative path a `..` that pops past the start is silently dropped
    // (`PathBuf::pop` on an empty buffer is a no-op), so an escaping path like
    // `files/../../x` would normalize to `files/x` and spuriously compare as
    // contained. Rooted paths clamp at the root instead. `target_directory` is
    // always derived from an absolute session working directory, so require
    // that invariant explicitly rather than depend on it silently.
    if !base.has_root() || !candidate.has_root() {
        return Err("path containment can only be checked for rooted paths".into());
    }
    let base = normalize_lexical(base);
    let candidate = normalize_lexical(candidate);
    if candidate.starts_with(&base) {
        Ok(())
    } else {
        Err("resolves to a path outside the session files directory".into())
    }
}

struct FileRecord {
    symbol: String,
    filename: PathBuf,
    file: EmbeddedFile,
}

/// Two-phase embedded file materializer.
///
/// Phase 1: `allocate_file_paths()` — creates file paths and registers symbols.
/// Phase 2: `write_file_contents()` — resolves format strings and writes to disk.
pub struct EmbeddedFiles {
    scope: EmbeddedFilesScope,
    target_directory: PathBuf,
    records: Vec<FileRecord>,
    user: Option<Arc<dyn SessionUser>>,
    session_id: String,
}

impl EmbeddedFiles {
    pub fn new(
        scope: EmbeddedFilesScope,
        session_files_directory: PathBuf,
        session_id: &str,
    ) -> Self {
        Self {
            scope,
            target_directory: session_files_directory,
            records: Vec::new(),
            user: None,
            session_id: session_id.to_string(),
        }
    }

    pub fn with_user(mut self, user: Option<Arc<dyn SessionUser>>) -> Self {
        self.user = user;
        self
    }

    pub fn allocate_file_paths(
        &mut self,
        files: &[EmbeddedFile],
        symtab: &mut SymbolTable,
    ) -> Result<(), SessionError> {
        let scope_name = match self.scope {
            EmbeddedFilesScope::Step => "Task",
            EmbeddedFilesScope::Env => "Environment",
        };
        session_log!(
            info,
            &self.session_id,
            LogContent::FILE_PATH,
            "Writing embedded files for {} to disk.",
            scope_name
        );
        for file in files {
            let symbol = symtab_key(self.scope, &file.name);
            let filename = if let Some(ref fname) = file.filename {
                // `filename` is a plain string per the 2023-09 schema (not
                // @fmtstring) — use it literally, no expression resolution.
                //
                // Defense-in-depth: the spec requires `filename` to be a
                // plain basename and the model layer rejects path separators
                // in the raw template. Re-check the value here before it is
                // joined to the target directory, in case model validation
                // was bypassed.
                validate_resolved_filename(fname).map_err(|reason| {
                    SessionError::EmbeddedFilePath {
                        name: file.name.clone(),
                        filename: fname.clone(),
                        reason,
                    }
                })?;
                let joined = self.target_directory.join(fname);
                // Final guarantee: the path we are about to create must resolve
                // to a location inside the session files directory. This backs
                // up the per-component filename validation above.
                ensure_within(&self.target_directory, &joined).map_err(|reason| {
                    SessionError::EmbeddedFilePath {
                        name: file.name.clone(),
                        filename: fname.clone(),
                        reason,
                    }
                })?;
                joined
            } else {
                let name = random_hex_filename();
                let path = self.target_directory.join(&name);
                fs::write(&path, b"").map_err(|e| SessionError::EmbeddedFile {
                    name: file.name.clone(),
                    source: e,
                })?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(|e| {
                        SessionError::EmbeddedFile {
                            name: file.name.clone(),
                            source: e,
                        }
                    })?;
                }
                path
            };
            symtab
                .set(
                    &symbol,
                    ExprValue::new_path(filename.to_string_lossy().to_string(), PathFormat::host()),
                )
                .map_err(|e| SessionError::Runtime(format!("Failed to set {symbol}: {e}")))?;
            self.records.push(FileRecord {
                symbol,
                filename,
                file: file.clone(),
            });
        }
        Ok(())
    }

    /// Re-register previously allocated file paths into a symbol table.
    ///
    /// This is used when a wrap environment's embedded file paths have already
    /// been allocated (on the first wrap-hook invocation) and need to be made
    /// visible in subsequent wrap-hook scopes without re-allocating paths or
    /// creating files on disk. Contents are NOT written — call
    /// `write_file_contents` separately after the scope is fully built.
    pub(crate) fn register_file_paths(&self, symtab: &mut SymbolTable) -> Result<(), SessionError> {
        let scope_name = match self.scope {
            EmbeddedFilesScope::Step => "Task",
            EmbeddedFilesScope::Env => "Environment",
        };
        session_log!(
            info,
            &self.session_id,
            LogContent::FILE_PATH,
            "Reusing embedded file paths for {} scope.",
            scope_name
        );
        for record in &self.records {
            symtab
                .set(
                    &record.symbol,
                    ExprValue::new_path(
                        record.filename.to_string_lossy().to_string(),
                        PathFormat::host(),
                    ),
                )
                .map_err(|e| {
                    SessionError::Runtime(format!("Failed to set {}: {e}", record.symbol))
                })?;
        }
        Ok(())
    }

    pub fn write_file_contents(
        &self,
        symtab: &SymbolTable,
        library: Option<&FunctionLibrary>,
    ) -> Result<(), SessionError> {
        for record in &self.records {
            if let Some(ref data_fs) = record.file.data {
                let resolved = data_fs
                    .resolve_string_with(
                        symtab,
                        &openjd_expr::FormatStringOptions::new().with_library(library),
                    )
                    .map_err(|e| SessionError::FormatString {
                        context: format!("embedded file '{}' data", record.file.name),
                        reason: e.to_string(),
                    })?;
                session_log!(
                    info,
                    &self.session_id,
                    LogContent::FILE_PATH,
                    "Writing: {}",
                    record.filename.display()
                );
                session_log!(
                    debug,
                    &self.session_id,
                    LogContent::FILE_CONTENTS,
                    "Contents:\n{}",
                    &resolved
                );
                let eol = record.file.end_of_line;
                let runnable = record.file.runnable.unwrap_or(false);
                write_embedded_file_with_options(&record.filename, &resolved, runnable, eol)
                    .map_err(|e| SessionError::EmbeddedFile {
                        name: record.file.name.clone(),
                        source: e,
                    })?;
                #[cfg(unix)]
                if let Some(ref user) = self.user {
                    if !user.is_process_user() {
                        chown_for_user(&record.filename, &**user, runnable)?;
                    }
                }
            } else {
                // Ensure file exists for named files with no data
                if record.file.filename.is_some() && !record.filename.exists() {
                    if let Some(parent) = record.filename.parent() {
                        fs::create_dir_all(parent).map_err(|e| SessionError::EmbeddedFile {
                            name: record.file.name.clone(),
                            source: e,
                        })?;
                    }
                    fs::write(&record.filename, b"").map_err(|e| SessionError::EmbeddedFile {
                        name: record.file.name.clone(),
                        source: e,
                    })?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random_hex_filename_length_and_format() {
        let name = random_hex_filename();
        assert_eq!(name.len(), 32);
        assert!(name.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_random_hex_filename_no_collision() {
        assert_ne!(random_hex_filename(), random_hex_filename());
    }

    #[test]
    fn normalize_lexical_collapses_parent_and_current() {
        assert_eq!(
            normalize_lexical(Path::new("/a/b/../c/./d")),
            PathBuf::from("/a/c/d")
        );
    }

    #[test]
    fn ensure_within_accepts_direct_child() {
        let base = Path::new("/session/files");
        assert!(ensure_within(base, &base.join("script.sh")).is_ok());
    }

    #[test]
    fn ensure_within_rejects_parent_escape() {
        let base = Path::new("/session/files");
        assert!(ensure_within(base, Path::new("/session/files/../../etc/passwd")).is_err());
    }

    #[test]
    fn ensure_within_rejects_sibling_with_shared_prefix() {
        // `files-evil` must not be treated as being under `files`.
        let base = Path::new("/session/files");
        assert!(ensure_within(base, Path::new("/session/files-evil/x")).is_err());
    }

    #[test]
    fn ensure_within_rejects_relative_paths() {
        // Lexical `..` handling is only sound for rooted paths: a `..` that pops
        // past the start of a relative path is silently dropped, so an escaping
        // path would spuriously compare as contained. Relative inputs must be
        // rejected outright.
        assert!(ensure_within(Path::new("files"), Path::new("files/../../files/x")).is_err());
    }

    #[test]
    fn validate_accepts_single_component() {
        assert!(validate_resolved_filename("script.sh").is_ok());
    }

    #[test]
    fn validate_rejects_double_dot() {
        assert_eq!(
            validate_resolved_filename(".."),
            Err("must not be '..'".to_string())
        );
    }

    // Windows strips trailing dots and spaces from the final path component, so
    // a name made only of dots and spaces collapses to `.`/`..` at the OS layer
    // and escapes the target directory. These are legitimate filenames on
    // POSIX, so the rejection — and this test — is Windows-only.
    #[cfg(windows)]
    #[test]
    fn validate_rejects_all_dots_and_spaces() {
        for value in [".. ", "...", ". ", "   ", ".. ."] {
            assert_eq!(
                validate_resolved_filename(value),
                Err("must not consist only of dots and spaces".to_string()),
                "expected rejection for {value:?}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn validate_accepts_name_with_trailing_dot() {
        // A named file that merely has a trailing dot does not collapse to a
        // directory reference — Windows resolves `foo...` to `foo`, still inside
        // the target — so it is accepted.
        assert!(validate_resolved_filename("foo...").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn test_chown_for_user_nonexistent_group_returns_error() {
        use crate::session_user::SessionUser;

        #[derive(Debug)]
        struct FakeUser;
        impl SessionUser for FakeUser {
            fn user(&self) -> &str {
                "nobody"
            }
            fn group(&self) -> &str {
                "nonexistent_group_xyz_12345"
            }
            fn is_process_user(&self) -> bool {
                false
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let result = chown_for_user(tmp.path(), &FakeUser, false);
        assert!(result.is_err(), "chown with nonexistent group should fail");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("nonexistent_group_xyz_12345"),
            "error should mention the group: {msg}"
        );
    }
}
