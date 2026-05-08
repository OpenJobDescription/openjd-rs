// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

// Ported from deadline-cloud test_manifest.py

use openjd_snapshots::{AbsSnapshot, AbsSnapshotDiff, DirEntry, Snapshot, SnapshotDiff};
use openjd_snapshots::{
    FileEntry, HashAlgorithm, Manifest, DEFAULT_FILE_CHUNK_SIZE, WHOLE_FILE_CHUNK_SIZE,
};

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

// --- TestValidateChunkSize ---

/// `Manifest::validate()` rejects negative `file_chunk_size_bytes` values other
/// than `-1` (`WHOLE_FILE_CHUNK_SIZE`) with a sensible, self-describing error.
/// Guards against a previous bug where the unchecked `as u64` cast wrapped
/// negative values to enormous u64s (e.g. `"size > 18446744073709551614"`).
#[test]
fn validate_rejects_bad_negative_chunk_size_with_sensible_message() {
    let mut f = FileEntry::file("a.bin", 1024, 1);
    f.chunk_hashes = Some(vec!["a".into(), "b".into()]);
    let m: Snapshot = Manifest::new(HashAlgorithm::Xxh128, -2).with_files(vec![f]);
    let err = m.validate().unwrap_err().to_string();
    assert_eq!(
        err,
        "Manifest validation error: invalid fileChunkSizeBytes: got -2, \
         must be -1 (WHOLE_FILE_CHUNK_SIZE) or a positive integer",
    );
}

/// The chunk-size invariant is manifest-level, so `validate()` must reject a
/// bad value even when no file declares `chunk_hashes` (i.e. before any
/// chunk-count computation would ever run).
#[test]
fn validate_rejects_bad_negative_chunk_size_without_chunked_files() {
    let m: Snapshot =
        Manifest::new(HashAlgorithm::Xxh128, -2).with_files(vec![FileEntry::file("a.bin", 10, 1)]);
    let err = m.validate().unwrap_err().to_string();
    assert_eq!(
        err,
        "Manifest validation error: invalid fileChunkSizeBytes: got -2, \
         must be -1 (WHOLE_FILE_CHUNK_SIZE) or a positive integer",
    );
}

/// `Manifest::validate()` rejects `file_chunk_size_bytes = 0` with a sensible
/// error. Guards against a previous bug where `size / 0` (as f64 ceil) produced
/// messages like `"should have 18446744073709551615 chunks (chunk_size=0)"`.
#[test]
fn validate_rejects_zero_chunk_size_with_sensible_message() {
    let mut f = FileEntry::file("a.bin", 3, 1);
    f.chunk_hashes = Some(vec!["a".into()]);
    let m: Snapshot = Manifest::new(HashAlgorithm::Xxh128, 0).with_files(vec![f]);
    let err = m.validate().unwrap_err().to_string();
    assert_eq!(
        err,
        "Manifest validation error: invalid fileChunkSizeBytes: got 0, \
         must be -1 (WHOLE_FILE_CHUNK_SIZE) or a positive integer",
    );
}

/// Same as above: zero is rejected even without any chunked files.
#[test]
fn validate_rejects_zero_chunk_size_without_chunked_files() {
    let m: Snapshot =
        Manifest::new(HashAlgorithm::Xxh128, 0).with_files(vec![FileEntry::file("a.bin", 10, 1)]);
    let err = m.validate().unwrap_err().to_string();
    assert_eq!(
        err,
        "Manifest validation error: invalid fileChunkSizeBytes: got 0, \
         must be -1 (WHOLE_FILE_CHUNK_SIZE) or a positive integer",
    );
}

/// Sanity: `file_chunk_size_bytes = -1` (`WHOLE_FILE_CHUNK_SIZE`) is accepted —
/// it's the sentinel meaning "no chunking".
#[test]
fn validate_accepts_whole_file_chunk_size_sentinel() {
    let m: Snapshot = Manifest::new(HashAlgorithm::Xxh128, WHOLE_FILE_CHUNK_SIZE)
        .with_files(vec![FileEntry::file("a.bin", 10, 1)]);
    assert!(m.validate().is_ok());
}

/// Sanity: a positive `file_chunk_size_bytes` is accepted.
#[test]
fn validate_accepts_positive_chunk_size() {
    let m: Snapshot = Manifest::new(HashAlgorithm::Xxh128, 1024)
        .with_files(vec![FileEntry::file("a.bin", 10, 1)]);
    assert!(m.validate().is_ok());
}

// --- TestPhantomTypes ---

/// Phantom type parameters on [`Manifest<P, K>`] are `#[serde(skip)]`, so
/// deserializing directly via `serde_json::from_str` does not enforce path-style
/// or kind constraints — `validate()` is responsible for catching mismatches at
/// runtime. This test pins that documented two-step behaviour: deserialize
/// succeeds, then `validate()` rejects.
#[test]
fn deserialization_accepts_mismatched_paths_and_validate_rejects_them() {
    let abs: AbsSnapshot =
        Manifest::new(HashAlgorithm::Xxh128, WHOLE_FILE_CHUNK_SIZE).with_files(vec![FileEntry {
            path: "/absolute/path.txt".into(),
            hash: Some("abc123".into()),
            size: Some(100),
            mtime: Some(1000),
            chunk_hashes: None,
            symlink_target: None,
            runnable: false,
            deleted: false,
        }]);

    let json = serde_json::to_string(&abs).unwrap();

    // Step 1: deserializing an absolute-path payload as a relative Snapshot
    // succeeds at the serde layer — phantom types are skipped.
    let rel: Snapshot = serde_json::from_str(&json)
        .expect("serde deserialization does not enforce phantom type constraints");

    // Step 2: validate() catches the mismatch with a clear error.
    let err = rel.validate().unwrap_err().to_string();
    assert_eq!(
        err,
        "Manifest validation error: expected relative path, got: /absolute/path.txt",
    );
}
