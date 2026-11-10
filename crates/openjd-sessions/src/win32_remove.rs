// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Remove a directory tree on Windows, continuing past entries that cannot be
//! deleted and reporting each one.
//!
//! `std::fs::remove_dir_all` stops at the first error, so one locked file
//! leaves the whole tree on disk and the error does not say which file it was.
//! This walk deletes everything it can, as Python's `shutil.rmtree` does with an
//! error handler.
//!
//! The session directory is writable by the job user, so a job process still
//! running during cleanup could swap a directory for a junction mid-walk. Two
//! rules stop the walk from following one out of the tree:
//!
//! - every open uses `FILE_FLAG_OPEN_REPARSE_POINT`, and a link is removed as a
//!   link, never traversed;
//! - each directory on the path being walked is held open without
//!   `FILE_SHARE_DELETE` while its entries are removed, so it cannot be renamed
//!   or replaced until the walk has left it.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

// Win32 constants from winnt.h and winbase.h.
const FILE_LIST_DIRECTORY: u32 = 0x0001;
const FILE_READ_ATTRIBUTES: u32 = 0x0080;
const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
const FILE_SHARE_READ: u32 = 0x0001;
const FILE_SHARE_WRITE: u32 = 0x0002;
const FILE_SHARE_DELETE: u32 = 0x0004;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;

/// An entry that one removal pass could not delete.
#[derive(Debug)]
pub(crate) struct RemoveFailure {
    pub(crate) path: PathBuf,
    pub(crate) is_dir: bool,
    pub(crate) error: io::Error,
}

/// Removes `root` and everything under it, as far as possible. Returns every
/// entry that could not be removed, in the order the walk reached it. An empty
/// result means `root` is gone.
pub(crate) fn remove_tree(root: &Path) -> Vec<RemoveFailure> {
    let mut failures = Vec::new();
    remove_dir_tree(root, true, &mut failures);
    failures
}

fn remove_dir_tree(dir: &Path, is_root: bool, failures: &mut Vec<RemoveFailure>) {
    let pin = match pin_dir(dir) {
        Ok(pin) => pin,
        Err(e) if !is_root && e.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            failures.push(RemoveFailure {
                path: dir.to_path_buf(),
                is_dir: true,
                error,
            });
            return;
        }
    };
    match std::fs::read_dir(dir) {
        Ok(entries) => {
            for entry in entries {
                match entry {
                    Ok(entry) => remove_entry(&entry, failures),
                    Err(error) => failures.push(RemoveFailure {
                        path: dir.to_path_buf(),
                        is_dir: true,
                        error,
                    }),
                }
            }
        }
        Err(error) => {
            failures.push(RemoveFailure {
                path: dir.to_path_buf(),
                is_dir: true,
                error,
            });
            return;
        }
    }
    // Release the pin: removing the directory needs a handle with DELETE access.
    drop(pin);
    record(failures, dir, true, remove_clearing_readonly(dir, true));
}

fn remove_entry(entry: &std::fs::DirEntry, failures: &mut Vec<RemoveFailure>) {
    let path = entry.path();
    let kind = match entry.file_type() {
        Ok(kind) => kind,
        Err(error) => {
            failures.push(RemoveFailure {
                path,
                is_dir: false,
                error,
            });
            return;
        }
    };
    if kind.is_symlink() {
        // A symlink or junction: remove the link itself, never its target.
        let is_dir = kind.is_symlink_dir();
        let result = if is_dir {
            std::fs::remove_dir(&path)
        } else {
            std::fs::remove_file(&path)
        };
        record(failures, &path, is_dir, result);
    } else if kind.is_dir() {
        remove_dir_tree(&path, false, failures);
    } else {
        record(
            failures,
            &path,
            false,
            remove_clearing_readonly(&path, false),
        );
    }
}

fn record(failures: &mut Vec<RemoveFailure>, path: &Path, is_dir: bool, result: io::Result<()>) {
    match result {
        Ok(()) => {}
        // Gone already, for example removed by the process that held it.
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(error) => failures.push(RemoveFailure {
            path: path.to_path_buf(),
            is_dir,
            error,
        }),
    }
}

/// Opens `dir` so that it cannot be renamed, deleted or replaced while the
/// handle is held. Refuses a directory that is a reparse point.
///
/// The handle needs `FILE_LIST_DIRECTORY`: Windows enforces a handle's share
/// mode only when it has data access, so an attributes-only handle would not
/// stop a rename.
fn pin_dir(dir: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(dir)?;
    let meta = file.metadata()?;
    if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::other(
            "it is a link or reparse point, so it was not followed",
        ));
    }
    if !meta.is_dir() {
        return Err(io::Error::other("it is not a directory"));
    }
    Ok(file)
}

/// Removes a file or an empty directory, clearing the read-only attribute
/// first if that is what stops it. `std::fs::remove_dir_all` removes read-only
/// files too.
fn remove_clearing_readonly(path: &Path, is_dir: bool) -> io::Result<()> {
    let remove = |p: &Path| {
        if is_dir {
            std::fs::remove_dir(p)
        } else {
            std::fs::remove_file(p)
        }
    };
    let error = match remove(path) {
        Ok(()) => return Ok(()),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => e,
        Err(e) => return Err(e),
    };
    if clear_readonly(path, is_dir).unwrap_or(false) {
        remove(path)
    } else {
        Err(error)
    }
}

/// Clears the read-only attribute through a handle, so a link swapped in at
/// `path` is not followed. Returns whether the attribute was set and cleared.
fn clear_readonly(path: &Path, is_dir: bool) -> io::Result<bool> {
    let flags = FILE_FLAG_OPEN_REPARSE_POINT
        | if is_dir {
            FILE_FLAG_BACKUP_SEMANTICS
        } else {
            0
        };
    let file = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(flags)
        .open(path)?;
    let meta = file.metadata()?;
    if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !meta.permissions().readonly()
    {
        return Ok(false);
    }
    let mut perms = meta.permissions();
    #[allow(
        clippy::permissions_set_readonly_false,
        reason = "Windows: clears FILE_ATTRIBUTE_READONLY on a file being deleted"
    )]
    perms.set_readonly(false);
    file.set_permissions(perms)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hold(path: &Path) -> File {
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(path)
            .unwrap()
    }

    fn names(failures: &[RemoveFailure], root: &Path) -> Vec<String> {
        failures
            .iter()
            .map(|f| f.path.strip_prefix(root).unwrap().display().to_string())
            .collect()
    }

    #[test]
    fn removes_everything_but_a_locked_file_and_its_parents() {
        let td = crate::tempdir::TempDir::new(None, None, None).unwrap();
        let root = td.path().to_path_buf();
        std::fs::create_dir(root.join("a_sub")).unwrap();
        let _held = hold(&root.join("a_sub").join("locked.bin"));
        for i in 0..300 {
            std::fs::write(root.join(format!("b{i:03}.txt")), b"x").unwrap();
            std::fs::write(root.join("a_sub").join(format!("c{i:03}.txt")), b"x").unwrap();
        }
        std::fs::create_dir_all(root.join("z").join("deep")).unwrap();
        std::fs::write(root.join("z").join("deep").join("f"), b"x").unwrap();

        let failures = remove_tree(&root);

        assert_eq!(names(&failures, &root), ["a_sub\\locked.bin", "a_sub", ""]);
        assert_eq!(failures[0].error.raw_os_error(), Some(32));
        assert!(!failures[0].is_dir && failures[1].is_dir && failures[2].is_dir);
        let left: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left, ["a_sub"]);
        let left_in_sub: Vec<_> = std::fs::read_dir(root.join("a_sub"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left_in_sub, ["locked.bin"]);
    }

    #[test]
    fn removes_the_whole_tree_when_nothing_is_locked() {
        let td = crate::tempdir::TempDir::new(None, None, None).unwrap();
        let root = td.path().to_path_buf();
        std::fs::create_dir_all(root.join("a").join("b")).unwrap();
        std::fs::write(root.join("a").join("b").join("f"), b"x").unwrap();
        assert!(remove_tree(&root).is_empty());
        assert!(!root.exists());
    }

    #[test]
    fn removes_read_only_files_and_directories() {
        let td = crate::tempdir::TempDir::new(None, None, None).unwrap();
        let root = td.path().to_path_buf();
        let dir = root.join("ro_dir");
        std::fs::create_dir(&dir).unwrap();
        let file = dir.join("ro.txt");
        std::fs::write(&file, b"x").unwrap();
        for p in [&file, &dir] {
            let mut perms = std::fs::metadata(p).unwrap().permissions();
            perms.set_readonly(true);
            std::fs::set_permissions(p, perms).unwrap();
        }
        let failures = remove_tree(&root);
        assert!(failures.is_empty(), "{failures:?}");
        assert!(!root.exists());
    }

    #[test]
    fn a_missing_root_is_reported_as_not_found() {
        let failures = remove_tree(Path::new(r"C:\definitely\not\here\openjd"));
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].error.kind(), io::ErrorKind::NotFound);
    }

    /// A junction inside the tree is removed as a link; what it points at survives.
    #[test]
    fn a_junction_is_removed_without_touching_its_target() {
        let target = crate::tempdir::TempDir::new(None, None, None).unwrap();
        std::fs::write(target.path().join("keep.txt"), b"keep").unwrap();
        let td = crate::tempdir::TempDir::new(None, None, None).unwrap();
        let root = td.path().to_path_buf();
        make_junction(&root.join("link"), target.path());

        let failures = remove_tree(&root);

        assert!(failures.is_empty(), "{failures:?}");
        assert!(!root.exists());
        assert_eq!(
            std::fs::read(target.path().join("keep.txt")).unwrap(),
            b"keep"
        );
    }

    /// A root that is itself a junction is refused, and its target is untouched.
    #[test]
    fn a_root_that_is_a_junction_is_not_followed() {
        let target = crate::tempdir::TempDir::new(None, None, None).unwrap();
        std::fs::write(target.path().join("keep.txt"), b"keep").unwrap();
        let td = crate::tempdir::TempDir::new(None, None, None).unwrap();
        let link = td.path().join("root_link");
        make_junction(&link, target.path());

        let failures = remove_tree(&link);

        assert_eq!(failures.len(), 1);
        assert_eq!(
            failures[0].error.to_string(),
            "it is a link or reparse point, so it was not followed"
        );
        assert_eq!(
            std::fs::read(target.path().join("keep.txt")).unwrap(),
            b"keep"
        );
    }

    /// While a directory is pinned, nobody can rename it.
    #[test]
    fn a_pinned_directory_cannot_be_renamed() {
        let td = crate::tempdir::TempDir::new(None, None, None).unwrap();
        let dir = td.path().join("pinned");
        std::fs::create_dir(&dir).unwrap();
        let pin = pin_dir(&dir).unwrap();
        let err = std::fs::rename(&dir, td.path().join("moved")).expect_err("rename must fail");
        assert_eq!(err.raw_os_error(), Some(32), "{err}");
        drop(pin);
        std::fs::rename(&dir, td.path().join("moved")).unwrap();
    }

    fn make_junction(link: &Path, target: &Path) {
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "mklink /J failed");
    }
}
