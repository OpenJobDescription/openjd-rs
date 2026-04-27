// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

// Ported from deadline-cloud test_compose_manifest.py (62 tests)

use openjd_snapshots::{
    compose_diffs, compose_snapshot_with_diffs, AbsSnapshot, AbsSnapshotDiff, DirEntry, FileEntry,
    HashAlgorithm, Manifest, Snapshot, SnapshotDiff, DEFAULT_FILE_CHUNK_SIZE,
};

// --- Helpers ---

fn snap(files: Vec<FileEntry>) -> Snapshot {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files)
}

fn snap_with_dirs(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> Snapshot {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(files)
        .with_dirs(dirs)
}

fn diff(files: Vec<FileEntry>) -> SnapshotDiff {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files)
}

fn diff_with_dirs(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> SnapshotDiff {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(files)
        .with_dirs(dirs)
}

fn abs_snap(files: Vec<FileEntry>) -> AbsSnapshot {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files)
}

#[allow(dead_code)]
fn abs_diff(files: Vec<FileEntry>) -> AbsSnapshotDiff {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files)
}

fn hf(path: &str, hash: &str, size: u64, mtime: u64) -> FileEntry {
    let mut f = FileEntry::file(path, size, mtime);
    f.hash = Some(hash.into());
    f
}

fn deleted_dir(path: &str) -> DirEntry {
    DirEntry {
        path: path.into(),
        deleted: true,
    }
}

fn fpaths(m: &Snapshot) -> std::collections::HashSet<String> {
    m.files.iter().map(|f| f.path.clone()).collect()
}

fn fpaths_abs(m: &AbsSnapshot) -> std::collections::HashSet<String> {
    m.files.iter().map(|f| f.path.clone()).collect()
}

#[allow(dead_code)]
fn dpaths(m: &Snapshot) -> std::collections::HashSet<String> {
    m.dirs
        .iter()
        .filter(|d| !d.deleted)
        .map(|d| d.path.clone())
        .collect()
}

fn diff_fpaths(m: &SnapshotDiff) -> std::collections::HashSet<String> {
    m.files
        .iter()
        .filter(|f| !f.deleted)
        .map(|f| f.path.clone())
        .collect()
}

fn diff_fpaths_abs(m: &AbsSnapshotDiff) -> std::collections::HashSet<String> {
    m.files
        .iter()
        .filter(|f| !f.deleted)
        .map(|f| f.path.clone())
        .collect()
}

// ===== TestComposeManifestsValidation =====
// Note: Rust API separates compose_snapshot_with_diffs and compose_diffs,
// so "empty list" and "type mismatch" errors are handled at compile time or
// by the respective function signatures. We test what's testable.

#[test]
fn compose_diffs_empty_list_raises_error() {
    let result = compose_diffs::<openjd_snapshots::manifest::Rel>(&[]);
    assert!(result.is_err());
}

// Test: snapshot_followed_by_snapshot_raises_error
// Not applicable in Rust — compose_snapshot_with_diffs accepts &[&Manifest<P, Diff>],
// so passing a Manifest<P, Full> is a compile-time error.

// Test: diff_composition_with_snapshot_raises_error
// Not applicable in Rust — compose_diffs accepts &[&Manifest<P, Diff>],
// so passing a Manifest<P, Full> is a compile-time error.

#[test]
fn single_snapshot_returns_as_is() {
    let base = snap(vec![hf("file.txt", "h1", 100, 1000)]);
    let result = compose_snapshot_with_diffs(&base, &[]).unwrap();
    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].path, "file.txt");
    assert_eq!(result.files[0].hash.as_deref(), Some("h1"));
    assert_eq!(result.files[0].size, Some(100));
    assert_eq!(result.files[0].mtime, Some(1000));
}

#[test]
fn snapshot_with_no_diffs_returns_snapshot() {
    let base = snap(vec![hf("a.txt", "h1", 10, 1), hf("b.txt", "h2", 20, 2)]);
    let result = compose_snapshot_with_diffs(&base, &[]).unwrap();
    assert_eq!(
        fpaths(&result),
        ["a.txt", "b.txt"].into_iter().map(String::from).collect()
    );
    assert_eq!(result.total_size, 30);
}

#[test]
fn single_diff_returns_as_is() {
    let d = diff(vec![hf("file.txt", "h1", 100, 1000)]);
    let result = compose_diffs(&[&d]).unwrap();
    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].path, "file.txt");
    assert_eq!(result.files[0].hash.as_deref(), Some("h1"));
}

// ===== TestComposeManifestsSnapshotDiffs =====

#[test]
fn snapshot_diff_adds_new_file() {
    let base = snap(vec![hf("existing.txt", "h1", 100, 1000)]);
    let d = diff(vec![hf("new.txt", "h2", 200, 2000)]);
    let result = compose_snapshot_with_diffs(&base, &[&d]).unwrap();
    assert_eq!(
        fpaths(&result),
        ["existing.txt", "new.txt"]
            .into_iter()
            .map(String::from)
            .collect()
    );
}

#[test]
fn snapshot_diff_modifies_existing_file() {
    let base = snap(vec![hf("file.txt", "h1", 100, 1000)]);
    let d = diff(vec![hf("file.txt", "h2", 200, 2000)]);
    let result = compose_snapshot_with_diffs(&base, &[&d]).unwrap();
    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].hash.as_deref(), Some("h2"));
    assert_eq!(result.files[0].size, Some(200));
    assert_eq!(result.files[0].mtime, Some(2000));
}

#[test]
fn snapshot_diff_deletes_file() {
    let base = snap(vec![
        hf("keep.txt", "h1", 100, 1000),
        hf("delete.txt", "h2", 200, 2000),
    ]);
    let d = diff(vec![FileEntry::deleted("delete.txt")]);
    let result = compose_snapshot_with_diffs(&base, &[&d]).unwrap();
    assert_eq!(
        fpaths(&result),
        ["keep.txt"].into_iter().map(String::from).collect()
    );
    assert!(result.files.iter().all(|f| !f.deleted));
}

#[test]
fn snapshot_diff_deletes_empty_directory() {
    let base = snap_with_dirs(
        vec![],
        vec![DirEntry::new("keep_dir"), DirEntry::new("delete_dir")],
    );
    let d = diff_with_dirs(vec![], vec![deleted_dir("delete_dir")]);
    let result = compose_snapshot_with_diffs(&base, &[&d]).unwrap();
    let dp: std::collections::HashSet<_> = result.dirs.iter().map(|d| d.path.as_str()).collect();
    assert!(dp.contains("keep_dir"));
    assert!(!dp.contains("delete_dir"));
}

#[test]
fn snapshot_multiple_diffs_applied_in_order() {
    let base = snap(vec![hf("file.txt", "v1", 100, 1000)]);
    let d1 = diff(vec![hf("file.txt", "v2", 100, 2000)]);
    let d2 = diff(vec![hf("file.txt", "v3", 100, 3000)]);
    let result = compose_snapshot_with_diffs(&base, &[&d1, &d2]).unwrap();
    assert_eq!(result.files[0].hash.as_deref(), Some("v3"));
}

#[test]
fn snapshot_add_then_delete_removes_file() {
    let base = snap(vec![]);
    let d1 = diff(vec![hf("temp.txt", "h1", 100, 1000)]);
    let d2 = diff(vec![FileEntry::deleted("temp.txt")]);
    let result = compose_snapshot_with_diffs(&base, &[&d1, &d2]).unwrap();
    assert!(result.files.is_empty());
}

#[test]
fn snapshot_delete_then_add_restores_file() {
    let base = snap(vec![hf("file.txt", "v1", 100, 1000)]);
    let d1 = diff(vec![FileEntry::deleted("file.txt")]);
    let d2 = diff(vec![hf("file.txt", "v2", 200, 3000)]);
    let result = compose_snapshot_with_diffs(&base, &[&d1, &d2]).unwrap();
    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].hash.as_deref(), Some("v2"));
}

#[test]
fn snapshot_symlink_handling() {
    let base = snap(vec![FileEntry::symlink("link.txt", "old_target.txt")]);
    let d = diff(vec![FileEntry::symlink("link.txt", "new_target.txt")]);
    let result = compose_snapshot_with_diffs(&base, &[&d]).unwrap();
    assert_eq!(
        result.files[0].symlink_target.as_deref(),
        Some("new_target.txt")
    );
}

#[test]
fn snapshot_runnable_flag_preserved() {
    let base = snap(vec![hf("script.sh", "h1", 100, 1000)]);
    let mut f = hf("script.sh", "h2", 100, 2000);
    f.runnable = true;
    let d = diff(vec![f]);
    let result = compose_snapshot_with_diffs(&base, &[&d]).unwrap();
    assert!(result.files[0].runnable);
}

#[test]
fn snapshot_chunkhashes_preserved() {
    let base = snap(vec![]);
    let mut f = FileEntry::file("large.bin", 768 * 1024 * 1024, 1000);
    f.chunk_hashes = Some(vec!["c1".into(), "c2".into(), "c3".into()]);
    let d = diff(vec![f]);
    let result = compose_snapshot_with_diffs(&base, &[&d]).unwrap();
    assert_eq!(
        result.files[0].chunk_hashes.as_deref(),
        Some(&["c1".to_string(), "c2".to_string(), "c3".to_string()][..])
    );
}

#[test]
fn snapshot_total_size_excludes_symlinks() {
    let base = snap(vec![
        hf("file.txt", "h1", 100, 1000),
        FileEntry::symlink("link.txt", "file.txt"),
    ]);
    // compose with no diffs - just verify total_size
    let result = compose_snapshot_with_diffs(&base, &[]).unwrap();
    assert_eq!(result.total_size, 100);
}

// ===== TestComposeManifestsSnapshotDiffsAbsolute =====

#[test]
fn abs_snapshot_diff_adds_new_file() {
    let base = abs_snap(vec![hf("/project/existing.txt", "h1", 100, 1000)]);
    let d: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(vec![hf("/project/new.txt", "h2", 200, 2000)]);
    let result = compose_snapshot_with_diffs(&base, &[&d]).unwrap();
    assert_eq!(
        fpaths_abs(&result),
        ["/project/existing.txt", "/project/new.txt"]
            .into_iter()
            .map(String::from)
            .collect()
    );
}

// ===== TestComposeManifestsDiffs =====

#[test]
fn diff_result_is_diff_type() {
    let d1 = diff(vec![hf("file1.txt", "h1", 100, 1000)]);
    let d2 = diff(vec![hf("file2.txt", "h2", 200, 2000)]);
    let _result: SnapshotDiff = compose_diffs(&[&d1, &d2]).unwrap();
    // If this compiles, the return type is SnapshotDiff
}

#[test]
fn diff_parent_hash_from_first_diff() {
    let d1 =
        diff(vec![hf("file1.txt", "h1", 100, 1000)]).with_parent_hash(Some("first_parent".into()));
    let d2 =
        diff(vec![hf("file2.txt", "h2", 200, 2000)]).with_parent_hash(Some("second_parent".into()));
    let result = compose_diffs(&[&d1, &d2]).unwrap();
    assert_eq!(result.parent_manifest_hash.as_deref(), Some("first_parent"));
}

#[test]
fn diff_additions_merged() {
    let d1 = diff(vec![hf("file1.txt", "h1", 100, 1000)]);
    let d2 = diff(vec![hf("file2.txt", "h2", 200, 2000)]);
    let result = compose_diffs(&[&d1, &d2]).unwrap();
    assert_eq!(
        diff_fpaths(&result),
        ["file1.txt", "file2.txt"]
            .into_iter()
            .map(String::from)
            .collect()
    );
}

#[test]
fn diff_later_modification_overrides_earlier() {
    let d1 = diff(vec![hf("file.txt", "v1", 100, 1000)]);
    let d2 = diff(vec![hf("file.txt", "v2", 200, 2000)]);
    let result = compose_diffs(&[&d1, &d2]).unwrap();
    let f = result.files.iter().find(|f| f.path == "file.txt").unwrap();
    assert_eq!(f.hash.as_deref(), Some("v2"));
}

#[test]
fn diff_deletion_marker_preserved() {
    let d1 = diff(vec![FileEntry::deleted("file.txt")]);
    let result = compose_diffs(&[&d1]).unwrap();
    assert!(result.files[0].deleted);
}

#[test]
fn diff_add_then_delete_preserves_deletion() {
    let d1 = diff(vec![hf("file.txt", "h1", 100, 1000)]);
    let d2 = diff(vec![FileEntry::deleted("file.txt")]);
    let result = compose_diffs(&[&d1, &d2]).unwrap();
    let f = result.files.iter().find(|f| f.path == "file.txt").unwrap();
    assert!(f.deleted);
}

#[test]
fn diff_delete_then_add_clears_deletion() {
    let d1 = diff(vec![FileEntry::deleted("file.txt")]);
    let d2 = diff(vec![hf("file.txt", "h1", 100, 1000)]);
    let result = compose_diffs(&[&d1, &d2]).unwrap();
    let f = result.files.iter().find(|f| f.path == "file.txt").unwrap();
    assert!(!f.deleted);
    assert_eq!(f.hash.as_deref(), Some("h1"));
}

#[test]
fn diff_directory_deletion_preserved() {
    let d1 = diff_with_dirs(vec![], vec![deleted_dir("old_dir")]);
    let result = compose_diffs(&[&d1]).unwrap();
    let dir = result.dirs.iter().find(|d| d.path == "old_dir").unwrap();
    assert!(dir.deleted);
}

#[test]
fn diff_directory_deletion_then_file_added_reconciles() {
    let d1 = diff_with_dirs(
        vec![FileEntry::deleted("dir/file.txt")],
        vec![deleted_dir("dir")],
    );
    let d2 = diff_with_dirs(
        vec![hf("dir/newfile.txt", "h1", 100, 1000)],
        vec![DirEntry::new("dir")],
    );
    let result = compose_diffs(&[&d1, &d2]).unwrap();
    let dir = result.dirs.iter().find(|d| d.path == "dir" && !d.deleted);
    assert!(dir.is_some());
    let f = result
        .files
        .iter()
        .find(|f| f.path == "dir/newfile.txt")
        .unwrap();
    assert!(!f.deleted);
}

#[test]
fn diff_nested_directory_deletion_reconciliation() {
    let d1 = diff_with_dirs(
        vec![FileEntry::deleted("a/b/c/old.txt")],
        vec![deleted_dir("a"), deleted_dir("a/b"), deleted_dir("a/b/c")],
    );
    let d2 = diff_with_dirs(
        vec![hf("a/b/c/new.txt", "h1", 100, 1000)],
        vec![
            DirEntry::new("a"),
            DirEntry::new("a/b"),
            DirEntry::new("a/b/c"),
        ],
    );
    let result = compose_diffs(&[&d1, &d2]).unwrap();
    let non_deleted: std::collections::HashSet<_> = result
        .dirs
        .iter()
        .filter(|d| !d.deleted)
        .map(|d| d.path.as_str())
        .collect();
    assert!(non_deleted.contains("a"));
    assert!(non_deleted.contains("a/b"));
    assert!(non_deleted.contains("a/b/c"));
}

#[test]
fn diff_total_size_excludes_deleted_entries() {
    let d1 = diff(vec![
        hf("keep.txt", "h1", 100, 1000),
        FileEntry::deleted("delete.txt"),
    ]);
    let result = compose_diffs(&[&d1]).unwrap();
    assert_eq!(result.total_size, 100);
}

// ===== TestComposeManifestsDiffsAbsolute =====

#[test]
fn abs_diff_additions_merged() {
    let d1: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(vec![hf("/project/file1.txt", "h1", 100, 1000)]);
    let d2: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(vec![hf("/project/file2.txt", "h2", 200, 2000)]);
    let result = compose_diffs(&[&d1, &d2]).unwrap();
    assert_eq!(
        diff_fpaths_abs(&result),
        ["/project/file1.txt", "/project/file2.txt"]
            .into_iter()
            .map(String::from)
            .collect()
    );
}

#[test]
fn abs_diff_returns_abs_diff_type() {
    let d1: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(vec![hf("/file1.txt", "h1", 100, 1000)]);
    let d2: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(vec![hf("/file2.txt", "h2", 200, 2000)]);
    let _result: AbsSnapshotDiff = compose_diffs(&[&d1, &d2]).unwrap();
}

// ===== TestComposeFileChunkSizeValidation =====

#[test]
fn snapshot_diffs_same_chunk_size_succeeds() {
    let base: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, 128 * 1024 * 1024)
        .with_files(vec![hf("/file1.txt", "h1", 100, 1000)]);
    let d: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, 128 * 1024 * 1024)
        .with_files(vec![hf("/file2.txt", "h2", 200, 2000)]);
    let result = compose_snapshot_with_diffs(&base, &[&d]).unwrap();
    assert_eq!(result.file_chunk_size_bytes, 128 * 1024 * 1024);
}

#[test]
fn snapshot_diffs_different_chunk_size_raises() {
    let base: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, 128 * 1024 * 1024)
        .with_files(vec![hf("/file1.txt", "h1", 100, 1000)]);
    let d: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, 256 * 1024 * 1024)
        .with_files(vec![hf("/file2.txt", "h2", 200, 2000)]);
    let err = compose_snapshot_with_diffs(&base, &[&d]).unwrap_err();
    assert!(err.to_string().contains("file_chunk_size_bytes"));
}

#[test]
fn diffs_same_chunk_size_succeeds() {
    let d1: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, 128 * 1024 * 1024)
        .with_files(vec![hf("/file1.txt", "h1", 100, 1000)]);
    let d2: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, 128 * 1024 * 1024)
        .with_files(vec![hf("/file2.txt", "h2", 200, 2000)]);
    let result = compose_diffs(&[&d1, &d2]).unwrap();
    assert_eq!(result.file_chunk_size_bytes, 128 * 1024 * 1024);
}

#[test]
fn diffs_different_chunk_size_raises() {
    let d1: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, 128 * 1024 * 1024)
        .with_files(vec![hf("/file1.txt", "h1", 100, 1000)]);
    let d2: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, 256 * 1024 * 1024)
        .with_files(vec![hf("/file2.txt", "h2", 200, 2000)]);
    let err = compose_diffs(&[&d1, &d2]).unwrap_err();
    assert!(err.to_string().contains("file_chunk_size_bytes"));
}

#[test]
fn relative_snapshot_diffs_preserves_chunk_size() {
    let base: Snapshot = Manifest::new(HashAlgorithm::Xxh128, 64 * 1024 * 1024)
        .with_files(vec![hf("file1.txt", "h1", 100, 1000)]);
    let d: SnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, 64 * 1024 * 1024)
        .with_files(vec![hf("file2.txt", "h2", 200, 2000)]);
    let result = compose_snapshot_with_diffs(&base, &[&d]).unwrap();
    assert_eq!(result.file_chunk_size_bytes, 64 * 1024 * 1024);
}

#[test]
fn relative_diffs_preserves_chunk_size() {
    let d1: SnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, 64 * 1024 * 1024)
        .with_files(vec![hf("file1.txt", "h1", 100, 1000)]);
    let d2: SnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, 64 * 1024 * 1024)
        .with_files(vec![hf("file2.txt", "h2", 200, 2000)]);
    let result = compose_diffs(&[&d1, &d2]).unwrap();
    assert_eq!(result.file_chunk_size_bytes, 64 * 1024 * 1024);
}
