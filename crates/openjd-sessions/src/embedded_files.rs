// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Embedded file materialization.
//!
//! Mirrors Python `_embedded_files.py`.

use std::fs;
use std::path::{Path, PathBuf};
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

struct FileRecord {
    _symbol: String,
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
            let filename = if let Some(ref fname_fs) = file.filename {
                let resolved = fname_fs
                    .resolve_string_with(symtab, &openjd_expr::FormatStringOptions::new())
                    .map_err(|e| SessionError::FormatString {
                        context: format!("embedded file '{}' filename", file.name),
                        reason: e.to_string(),
                    })?;
                self.target_directory.join(resolved)
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
                _symbol: symbol,
                filename,
                file: file.clone(),
            });
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
