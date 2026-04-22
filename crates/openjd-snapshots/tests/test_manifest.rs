// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Ported from deadline-cloud test_manifest.py

use openjd_snapshots::{AbsSnapshot, AbsSnapshotDiff, DirEntry, Snapshot, SnapshotDiff};
use openjd_snapshots::{FileEntry, HashAlgorithm, Manifest, DEFAULT_FILE_CHUNK_SIZE};

fn abs_snapshot(files: Vec<FileEntry>) -> AbsSnapshot {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files)
}

fn abs_diff(files: Vec<FileEntry>) -> AbsSnapshotDiff {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files)
}

fn rel_snapshot(files: Vec<FileEntry>) -> Snapshot {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files)
}

fn rel_diff(files: Vec<FileEntry>) -> SnapshotDiff {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files)
}

fn hashed_file(path: &str, hash: &str, size: u64, mtime: u64) -> FileEntry {
    let mut f = FileEntry::file(path, size, mtime);
    f.hash = Some(hash.into());
    f
}

// --- TestClearHashes ---

#[test]
fn clear_hashes_clears_hash_from_regular_files() {
    let mut m = abs_snapshot(vec![
        hashed_file("/a/file1.txt", "abc123", 100, 1000),
        hashed_file("/a/file2.txt", "def456", 200, 2000),
    ]);
    m.clear_hashes();
    assert!(m.files[0].hash.is_none());
    assert!(m.files[1].hash.is_none());
}

#[test]
fn clear_hashes_clears_chunkhashes_from_large_files() {
    let mut f = FileEntry::file("/a/large.bin", 512 * 1024 * 1024, 1000);
    f.chunk_hashes = Some(vec!["chunk1".into(), "chunk2".into()]);
    let mut m = abs_snapshot(vec![f]);
    m.clear_hashes();
    assert!(m.files[0].chunk_hashes.is_none());
}

#[test]
fn clear_hashes_preserves_symlinks() {
    let mut m = abs_snapshot(vec![FileEntry::symlink("/a/link", "/a/target")]);
    m.clear_hashes();
    assert_eq!(m.files[0].symlink_target.as_deref(), Some("/a/target"));
}

#[test]
fn clear_hashes_preserves_deleted_entries() {
    let mut m = abs_diff(vec![FileEntry::deleted("/a/deleted.txt")]);
    m.clear_hashes();
    assert!(m.files[0].deleted);
}

#[test]
fn clear_hashes_works_on_abs_snapshot() {
    let mut m = abs_snapshot(vec![hashed_file("/a/file.txt", "abc", 100, 1000)]);
    m.clear_hashes();
    assert!(m.files[0].hash.is_none());
}

#[test]
fn clear_hashes_works_on_abs_snapshot_diff() {
    let mut m = abs_diff(vec![hashed_file("/a/file.txt", "abc", 100, 1000)]);
    m.clear_hashes();
    assert!(m.files[0].hash.is_none());
}

#[test]
fn clear_hashes_works_on_rel_snapshot() {
    let mut m = rel_snapshot(vec![hashed_file("file.txt", "abc", 100, 1000)]);
    m.clear_hashes();
    assert!(m.files[0].hash.is_none());
}

#[test]
fn clear_hashes_works_on_rel_snapshot_diff() {
    let mut m = rel_diff(vec![hashed_file("file.txt", "abc", 100, 1000)]);
    m.clear_hashes();
    assert!(m.files[0].hash.is_none());
}

#[test]
fn clear_hashes_preserves_other_file_metadata() {
    let mut f = hashed_file("/a/file.txt", "abc123", 100, 1000);
    f.runnable = true;
    let mut m = abs_snapshot(vec![f]);
    m.clear_hashes();
    let entry = &m.files[0];
    assert_eq!(entry.path, "/a/file.txt");
    assert_eq!(entry.size, Some(100));
    assert_eq!(entry.mtime, Some(1000));
    assert!(entry.runnable);
}

#[test]
fn clear_hashes_mixed_entries() {
    let mut chunked = FileEntry::file("/a/chunked.bin", 512 * 1024 * 1024, 2000);
    chunked.chunk_hashes = Some(vec!["c1".into(), "c2".into()]);

    let mut m = abs_diff(vec![
        hashed_file("/a/hashed.txt", "abc", 100, 1000),
        chunked,
        FileEntry::symlink("/a/link", "/a/target"),
        FileEntry::deleted("/a/deleted.txt"),
        FileEntry::file("/a/unhashed.txt", 50, 3000),
    ]);
    m.clear_hashes();

    assert!(m.files[0].hash.is_none());
    assert!(m.files[1].chunk_hashes.is_none());
    assert_eq!(m.files[2].symlink_target.as_deref(), Some("/a/target"));
    assert!(m.files[3].deleted);
    assert!(m.files[4].hash.is_none());
}

// --- TestValidateDuplicatePaths ---

#[test]
fn validate_rejects_duplicate_file_paths() {
    let m = rel_snapshot(vec![
        FileEntry::file("a.txt", 10, 100),
        FileEntry::file("a.txt", 20, 200),
    ]);
    let err = m.validate().unwrap_err().to_string();
    assert!(err.contains("duplicate path: a.txt"));
}

#[test]
fn validate_rejects_duplicate_dir_paths() {
    let m: Snapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_dirs(vec![DirEntry::new("dir"), DirEntry::new("dir")]);
    let err = m.validate().unwrap_err().to_string();
    assert!(err.contains("duplicate path: dir"));
}

#[test]
fn validate_rejects_file_dir_same_path() {
    let mut m = rel_snapshot(vec![FileEntry::file("a", 10, 100)]);
    m.dirs = vec![DirEntry::new("a")];
    let err = m.validate().unwrap_err().to_string();
    assert!(err.contains("duplicate path: a"));
}

#[test]
fn validate_accepts_unique_paths() {
    let mut m = rel_snapshot(vec![
        FileEntry::file("a.txt", 10, 100),
        FileEntry::file("b.txt", 20, 200),
    ]);
    m.dirs = vec![DirEntry::new("c")];
    assert!(m.validate().is_ok());
}
