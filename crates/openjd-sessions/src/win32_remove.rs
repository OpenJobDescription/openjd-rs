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
//! The walk is iterative, with an explicit stack, so a deep tree cannot
//! overflow the thread's call stack and abort the process.
//!
//! The session directory is writable by the job user, so a job process still
//! running during cleanup could swap a directory for a junction mid-walk. Two
//! rules stop the walk from following one out of the tree, matching the
//! time-of-check/time-of-use safety of `std::fs::remove_dir_all`:
//!
//! - a reparse point is removed as a link, never traversed: a child is tested
//!   with [`std::fs::DirEntry::file_type`], which does not follow it, and any
//!   directory the walk opens uses `FILE_FLAG_OPEN_REPARSE_POINT` and is
//!   refused if it turns out to be a reparse point;
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
/// entry that could not be removed, in the order the walk reached it: a
/// directory comes after its contents (post-order). An empty result means
/// `root` is gone.
pub(crate) fn remove_tree(root: &Path) -> Vec<RemoveFailure> {
    remove_tree_observed(root, |_| {})
}

/// [`remove_tree`], calling `on_dir_pinned` once for each directory just after
/// it is pinned and before its entries are processed, while the pin is held.
/// Tests use it to observe the walk mid-flight; `remove_tree` passes a no-op.
fn remove_tree_observed(root: &Path, mut on_dir_pinned: impl FnMut(&Path)) -> Vec<RemoveFailure> {
    let mut failures = Vec::new();

    // A directory whose contents are being removed. Its pin is held until the
    // frame is popped, so the directory cannot be swapped while the walk is
    // inside it. `entries` is read once, up front, so no directory handle is
    // open for enumeration while children are deleted.
    struct Frame {
        path: PathBuf,
        _pin: File,
        entries: std::vec::IntoIter<std::fs::DirEntry>,
        announced: bool,
    }

    let push =
        |path: &Path, is_root: bool, stack: &mut Vec<Frame>, failures: &mut Vec<RemoveFailure>| {
            match open_dir_frame(path) {
                Ok((pin, entries)) => stack.push(Frame {
                    path: path.to_path_buf(),
                    _pin: pin,
                    entries,
                    announced: false,
                }),
                // Gone before we reached it; nothing to remove. The root not
                // existing is still a failure, as `remove_dir_all` reports.
                Err(e) if !is_root && e.kind() == io::ErrorKind::NotFound => {}
                Err(error) => failures.push(RemoveFailure {
                    path: path.to_path_buf(),
                    is_dir: true,
                    error,
                }),
            }
        };

    let mut stack: Vec<Frame> = Vec::new();
    push(root, true, &mut stack, &mut failures);

    while let Some(frame) = stack.last_mut() {
        if !frame.announced {
            frame.announced = true;
            on_dir_pinned(&frame.path);
        }
        if let Some(entry) = frame.entries.next() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_symlink() => {
                    // A symlink or junction: remove the link, never its target.
                    let is_dir = kind.is_symlink_dir();
                    let result = if is_dir {
                        std::fs::remove_dir(&path)
                    } else {
                        std::fs::remove_file(&path)
                    };
                    record(&mut failures, &path, is_dir, result);
                }
                Ok(kind) if kind.is_dir() => {
                    push(&path, false, &mut stack, &mut failures);
                }
                Ok(_) => record(
                    &mut failures,
                    &path,
                    false,
                    remove_clearing_readonly(&path, false),
                ),
                Err(error) => failures.push(RemoveFailure {
                    path,
                    is_dir: false,
                    error,
                }),
            }
        } else {
            // Children done. Release the pin (removing the directory needs a
            // handle with DELETE access, which the pin does not allow), then
            // remove the directory itself.
            let frame = stack.pop().expect("last_mut returned a frame");
            let Frame { path, _pin, .. } = frame;
            drop(_pin);
            record(
                &mut failures,
                &path,
                true,
                remove_clearing_readonly(&path, true),
            );
        }
    }

    failures
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

/// Pins `dir` and reads its entries. The pin is returned so the caller can hold
/// it while the entries are removed; see the module docs.
fn open_dir_frame(dir: &Path) -> io::Result<(File, std::vec::IntoIter<std::fs::DirEntry>)> {
    let pin = pin_dir(dir)?;
    let entries: Vec<std::fs::DirEntry> = std::fs::read_dir(dir)?.collect::<io::Result<_>>()?;
    Ok((pin, entries.into_iter()))
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

    /// A tree deep enough to overflow a recursive walk's call stack is removed
    /// without aborting the process. The walk runs on a small-stack thread, so
    /// a recursive implementation would overflow at this depth; the iterative
    /// one returns normally.
    #[test]
    fn a_deep_tree_does_not_overflow_the_stack() {
        let td = crate::tempdir::TempDir::new(None, None, None).unwrap();
        // Build with a verbatim \\?\ path so MAX_PATH does not cap the depth.
        let mut deep = format!(r"\\?\{}", td.path().display());
        for i in 0..1500 {
            deep.push_str(&format!(r"\d{i:04}"));
        }
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(format!(r"{deep}\leaf.txt"), b"x").unwrap();
        let root = td.path().to_path_buf();

        let handle = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || remove_tree(&root))
            .unwrap();
        let failures = handle.join().expect("the walk must not overflow the stack");

        assert!(failures.is_empty(), "{failures:?}");
        assert!(!td.path().exists());
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

    /// A file symlink inside the tree is removed as a link; its target survives.
    #[test]
    fn a_file_symlink_is_removed_without_touching_its_target() {
        let target = crate::tempdir::TempDir::new(None, None, None).unwrap();
        let target_file = target.path().join("keep.txt");
        std::fs::write(&target_file, b"keep").unwrap();
        let td = crate::tempdir::TempDir::new(None, None, None).unwrap();
        let root = td.path().to_path_buf();
        std::os::windows::fs::symlink_file(&target_file, root.join("flink")).unwrap();

        let failures = remove_tree(&root);

        assert!(failures.is_empty(), "{failures:?}");
        assert!(!root.exists());
        assert_eq!(std::fs::read(&target_file).unwrap(), b"keep");
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

    /// `pin_dir` refuses a reparse point. That is how a directory swapped for a
    /// junction *after* it was classified as a real directory is kept out of
    /// the walk: the walk opens it with `pin_dir`, which refuses it. (A junction
    /// that is already there when the walk reaches it is classified as a link by
    /// `DirEntry::file_type` and removed as one, so it never reaches `pin_dir`.)
    #[test]
    fn pin_dir_refuses_a_reparse_point() {
        let target = crate::tempdir::TempDir::new(None, None, None).unwrap();
        std::fs::write(target.path().join("keep.txt"), b"keep").unwrap();
        let td = crate::tempdir::TempDir::new(None, None, None).unwrap();
        let link = td.path().join("swapped");
        make_junction(&link, target.path());

        let err = pin_dir(&link).expect_err("a reparse point must be refused");
        assert_eq!(
            err.to_string(),
            "it is a link or reparse point, so it was not followed"
        );
        assert_eq!(
            std::fs::read(target.path().join("keep.txt")).unwrap(),
            b"keep"
        );
        let _ = std::fs::remove_dir(&link);
    }

    /// `clear_readonly` opens with `FILE_FLAG_OPEN_REPARSE_POINT`, so clearing
    /// the attribute on a path that is a link does not touch the link's target.
    #[test]
    fn clear_readonly_does_not_follow_a_link() {
        let target = crate::tempdir::TempDir::new(None, None, None).unwrap();
        let target_file = target.path().join("keep.txt");
        std::fs::write(&target_file, b"keep").unwrap();
        let mut ro = std::fs::metadata(&target_file).unwrap().permissions();
        ro.set_readonly(true);
        std::fs::set_permissions(&target_file, ro).unwrap();
        let td = crate::tempdir::TempDir::new(None, None, None).unwrap();
        let link = td.path().join("flink");
        std::os::windows::fs::symlink_file(&target_file, &link).unwrap();

        // The link itself is not a reparse-free file, so clear_readonly reports
        // "not cleared" and leaves the target's attribute set.
        assert!(!clear_readonly(&link, false).unwrap());
        assert!(std::fs::metadata(&target_file)
            .unwrap()
            .permissions()
            .readonly());
        let _ = std::fs::remove_file(&link);
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

    /// A directory another process holds open with DELETE access and full
    /// sharing cannot be pinned, so the walk records it as a failure and does
    /// not delete its contents. This is the documented pin limitation; the test
    /// locks it in so it cannot change silently.
    #[test]
    fn a_directory_held_with_delete_access_is_recorded_not_descended() {
        const DELETE: u32 = 0x0001_0000;
        let td = crate::tempdir::TempDir::new(None, None, None).unwrap();
        let root = td.path().to_path_buf();
        let held_dir = root.join("held");
        std::fs::create_dir(&held_dir).unwrap();
        std::fs::write(held_dir.join("inside.txt"), b"x").unwrap();
        // Another handle on the directory, with DELETE + full sharing, as a tool
        // about to rename or delete it would hold.
        let _handle = OpenOptions::new()
            .access_mode(DELETE | FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(&held_dir)
            .unwrap();

        let failures = remove_tree(&root);

        // "held" is recorded (cannot be pinned) and the root after it, and its
        // contents are left alone — not deleted through an unpinned walk.
        assert_eq!(names(&failures, &root), ["held", ""], "{failures:?}");
        assert_eq!(failures[0].error.raw_os_error(), Some(32));
        assert!(held_dir.join("inside.txt").exists());
    }

    /// Each directory stays pinned while its entries are removed: an attempt to
    /// rename it mid-walk fails. This is the CVE-2022-21658 protection.
    #[test]
    fn a_directory_is_pinned_while_its_entries_are_removed() {
        let td = crate::tempdir::TempDir::new(None, None, None).unwrap();
        let root = td.path().to_path_buf();
        let mid = root.join("mid");
        std::fs::create_dir(&mid).unwrap();
        std::fs::write(mid.join("leaf.txt"), b"x").unwrap();
        let moved = root.join("moved");

        let mut checked = false;
        let failures = remove_tree_observed(&root, |pinned| {
            if pinned.ends_with("mid") {
                let err = std::fs::rename(&mid, &moved)
                    .expect_err("a pinned directory must not be renamable");
                assert_eq!(err.raw_os_error(), Some(32), "{err}");
                checked = true;
            }
        });

        assert!(checked, "the walk never pinned mid");
        assert!(failures.is_empty(), "{failures:?}");
        assert!(!root.exists());
    }

    /// A child that disappears after it is enumerated but before the walk pins
    /// it is not a failure: the walk skips the NotFound and finishes cleanly.
    #[test]
    fn a_child_that_vanishes_before_it_is_pinned_is_not_a_failure() {
        let td = crate::tempdir::TempDir::new(None, None, None).unwrap();
        let root = td.path().to_path_buf();
        let gone = root.join("gone");
        std::fs::create_dir(&gone).unwrap();
        std::fs::write(root.join("keep.txt"), b"x").unwrap();

        // When the root is pinned, delete `gone` before the walk reaches it.
        let failures = remove_tree_observed(&root, |pinned| {
            if pinned == root {
                std::fs::remove_dir(&gone).unwrap();
            }
        });

        assert!(failures.is_empty(), "{failures:?}");
        assert!(!root.exists());
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
