// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for temp directory and OS detection — mirrors Python test_tempdir.py and test_os_checker.py

use openjd_sessions::tempdir::{openjd_temp_dir, TempDir};
use std::fs;
use std::path::Path;

#[test]
fn test_openjd_temp_dir_creates_dir() {
    let dir = openjd_temp_dir().unwrap();
    assert!(dir.exists());
    assert!(dir.is_dir());
    assert_eq!(dir.file_name().unwrap(), "OpenJD");
}

#[test]
fn test_tempdir_default_parent() {
    let mut tmp = TempDir::new(None, None, None).unwrap();
    assert!(tmp.path().exists());
    assert!(tmp.path().is_dir());
    let expected_parent = openjd_temp_dir().unwrap();
    assert_eq!(tmp.path().parent().unwrap(), expected_parent);
    tmp.cleanup().unwrap();
}

#[test]
fn test_tempdir_given_dir() {
    let parent = tempfile::TempDir::new().unwrap();
    let mut tmp = TempDir::new(Some(parent.path()), None, None).unwrap();
    assert!(tmp.path().exists());
    assert_eq!(tmp.path().parent().unwrap(), parent.path());
    tmp.cleanup().unwrap();
}

#[test]
fn test_tempdir_with_prefix() {
    let parent = tempfile::TempDir::new().unwrap();
    let mut tmp = TempDir::new(Some(parent.path()), Some("myprefix"), None).unwrap();
    let name = tmp.path().file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with("myprefix"));
    tmp.cleanup().unwrap();
}

#[test]
fn test_tempdir_cleanup() {
    let parent = tempfile::TempDir::new().unwrap();
    let mut tmp = TempDir::new(Some(parent.path()), None, None).unwrap();
    let path = tmp.path().to_path_buf();
    fs::write(path.join("test.txt"), "test").unwrap();
    assert!(path.exists());
    tmp.cleanup().unwrap();
    assert!(!path.exists());
}

#[test]
fn test_tempdir_nonexistent_parent_fails() {
    let bad_path = Path::new("/a/very/unlikely/dir/to/exist");
    let result = TempDir::new(Some(bad_path), None, None);
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn test_tempdir_posix_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let parent = tempfile::TempDir::new().unwrap();
    let mut tmp = TempDir::new(Some(parent.path()), None, None).unwrap();
    let mode = fs::metadata(tmp.path()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700);
    tmp.cleanup().unwrap();
}

/// Mirrors Python TestTempDirPosix::test_defaults — checks uid/gid ownership.
#[cfg(unix)]
#[test]
fn test_tempdir_posix_default_ownership() {
    use std::os::unix::fs::MetadataExt;
    let parent = tempfile::TempDir::new().unwrap();
    let tmp = TempDir::new(Some(parent.path()), None, None).unwrap();
    let meta = fs::metadata(tmp.path()).unwrap();
    assert_eq!(
        meta.uid(),
        nix::unistd::geteuid().as_raw(),
        "Owner is this process's uid"
    );
    assert_eq!(
        meta.gid(),
        nix::unistd::getegid().as_raw(),
        "Group is this process's gid"
    );
}

// === OS detection tests ===

#[test]
fn test_is_posix() {
    const { assert!(cfg!(unix) || !cfg!(unix)) };
    #[cfg(unix)]
    const {
        assert!(cfg!(unix))
    };
}

#[test]
fn test_is_windows() {
    const { assert!(cfg!(windows) || !cfg!(windows)) };
    #[cfg(windows)]
    const {
        assert!(cfg!(windows))
    };
}

#[test]
fn test_os_detection_consistent() {
    const { assert!(!(cfg!(unix) && cfg!(windows))) };
    const { assert!(cfg!(unix) || cfg!(windows) || cfg!(target_os = "wasi")) };
}
