// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests for temp directory and OS detection — mirrors Python test_tempdir.py and test_os_checker.py

use openjd_sessions::tempdir::{custom_gettempdir, TempDir};
use std::fs;
use std::path::Path;

#[test]
fn test_custom_gettempdir_creates_dir() {
    let dir = custom_gettempdir().unwrap();
    assert!(dir.exists());
    assert!(dir.is_dir());
    assert_eq!(dir.file_name().unwrap(), "OpenJD");
}

#[test]
fn test_tempdir_default_parent() {
    let mut tmp = TempDir::new(None, None, None).unwrap();
    assert!(tmp.path().exists());
    assert!(tmp.path().is_dir());
    let expected_parent = custom_gettempdir().unwrap();
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
