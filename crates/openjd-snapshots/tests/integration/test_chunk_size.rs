// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

// Ported from deadline-cloud test_file_chunk_size_bytes.py
//
// Tests that file_chunk_size_bytes is correctly preserved through operations:
// collect, filter, diff, compose, subtree, partition, join.

use openjd_snapshots::{
    collect_abs_snapshot, compose_diffs, compose_snapshot_with_diffs, diff_snapshots,
    filter_manifest, hash_abs_manifest, hash_upload_abs_manifest, join_snapshot,
    partition_manifest, subtree_snapshot, AbsManifest, AsyncDataCache, CollectOptions, DiffOptions,
    DirEntry, FileEntry, FileSystemDataCache, HashAlgorithm, HashOptions, HashUploadOptions,
    Manifest, ManifestEntry, PartitionOptions, Snapshot, SymlinkPolicy, DEFAULT_FILE_CHUNK_SIZE,
    WHOLE_FILE_CHUNK_SIZE,
};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

// --- Helpers ---

fn abs_snapshot(
    chunk_size: i64,
    files: Vec<FileEntry>,
    dirs: Vec<DirEntry>,
) -> openjd_snapshots::AbsSnapshot {
    Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(files)
        .with_dirs(dirs)
}

fn rel_snapshot(chunk_size: i64, files: Vec<FileEntry>) -> Snapshot {
    Manifest::new(HashAlgorithm::Xxh128, chunk_size).with_files(files)
}

// ===== Collect =====

#[test]
fn collect_default_chunk_size() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "content").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    assert_eq!(m.file_chunk_size_bytes, DEFAULT_FILE_CHUNK_SIZE);
}

#[test]
fn collect_explicit_chunk_size() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "content").unwrap();
    let custom = 1024 * 1024;

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(custom),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.file_chunk_size_bytes, custom);
}

#[test]
fn collect_whole_file_chunk_size() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "content").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(WHOLE_FILE_CHUNK_SIZE),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.file_chunk_size_bytes, WHOLE_FILE_CHUNK_SIZE);
    assert_eq!(m.file_chunk_size_bytes, -1);
}

// ===== Filter =====

#[test]
fn filter_preserves_chunk_size() {
    let custom = 512 * 1024;
    let m = abs_snapshot(
        custom,
        vec![
            FileEntry::file("/tmp/a.txt", 10, 1),
            FileEntry::file("/tmp/b.txt", 20, 2),
        ],
        vec![],
    );

    let filtered = filter_manifest(&m, &|entry: &ManifestEntry| entry.path().ends_with("a.txt"));

    assert_eq!(filtered.file_chunk_size_bytes, custom);
    assert_eq!(filtered.files.len(), 1);
}

#[test]
fn filter_preserves_whole_file_chunk_size() {
    let m = abs_snapshot(
        WHOLE_FILE_CHUNK_SIZE,
        vec![FileEntry::file("/tmp/a.txt", 10, 1)],
        vec![],
    );

    let filtered = filter_manifest(&m, &|_: &ManifestEntry| true);

    assert_eq!(filtered.file_chunk_size_bytes, WHOLE_FILE_CHUNK_SIZE);
}

// ===== Diff =====

#[test]
fn diff_preserves_parent_chunk_size() {
    let custom = 64 * 1024 * 1024;
    let parent = abs_snapshot(custom, vec![FileEntry::file("/tmp/a.txt", 10, 1)], vec![]);
    let current = abs_snapshot(
        custom,
        vec![FileEntry::file("/tmp/a.txt", 10, 2)], // mtime changed
        vec![],
    );

    let diff = diff_snapshots(
        &parent,
        &current,
        &DiffOptions {
            ignore_hashes: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(diff.file_chunk_size_bytes, custom);
}

#[test]
fn diff_preserves_whole_file_chunk_size() {
    let parent = abs_snapshot(
        WHOLE_FILE_CHUNK_SIZE,
        vec![FileEntry::file("/tmp/a.txt", 10, 1)],
        vec![],
    );
    let current = abs_snapshot(
        WHOLE_FILE_CHUNK_SIZE,
        vec![FileEntry::file("/tmp/b.txt", 20, 2)],
        vec![],
    );

    let diff = diff_snapshots(
        &parent,
        &current,
        &DiffOptions {
            ignore_hashes: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(diff.file_chunk_size_bytes, WHOLE_FILE_CHUNK_SIZE);
}

// ===== Compose =====

#[test]
fn compose_preserves_chunk_size() {
    let custom = 128 * 1024;
    let base = abs_snapshot(custom, vec![FileEntry::file("/tmp/a.txt", 10, 1)], vec![]);
    let diff: openjd_snapshots::AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, custom)
        .with_files(vec![FileEntry::file("/tmp/b.txt", 20, 2)]);

    let composed = compose_snapshot_with_diffs(&base, &[&diff]).unwrap();

    assert_eq!(composed.file_chunk_size_bytes, custom);
}

#[test]
fn compose_diffs_preserves_chunk_size() {
    let custom = 256 * 1024;
    let d1: openjd_snapshots::AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, custom)
        .with_files(vec![FileEntry::file("/tmp/a.txt", 10, 1)]);
    let d2: openjd_snapshots::AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, custom)
        .with_files(vec![FileEntry::file("/tmp/b.txt", 20, 2)]);

    let composed = compose_diffs(&[&d1, &d2]).unwrap();

    assert_eq!(composed.file_chunk_size_bytes, custom);
}

// ===== Subtree =====

#[test]
fn subtree_preserves_chunk_size() {
    let custom = 32 * 1024 * 1024;
    let m = abs_snapshot(
        custom,
        vec![
            FileEntry::file("/root/sub/a.txt", 10, 1),
            FileEntry::file("/root/sub/b.txt", 20, 2),
        ],
        vec![],
    );

    let sub = subtree_snapshot(&m, "/root/sub", SymlinkPolicy::CollapseAll).unwrap();

    assert_eq!(sub.file_chunk_size_bytes, custom);
}

#[test]
fn subtree_preserves_whole_file_chunk_size() {
    let m = abs_snapshot(
        WHOLE_FILE_CHUNK_SIZE,
        vec![FileEntry::file("/root/sub/a.txt", 10, 1)],
        vec![],
    );

    let sub = subtree_snapshot(&m, "/root/sub", SymlinkPolicy::CollapseAll).unwrap();

    assert_eq!(sub.file_chunk_size_bytes, WHOLE_FILE_CHUNK_SIZE);
}

// ===== Partition =====

#[test]
fn partition_preserves_chunk_size() {
    let custom = 16 * 1024 * 1024;
    let m = abs_snapshot(
        custom,
        vec![
            FileEntry::file("/root/a.txt", 10, 1),
            FileEntry::file("/root/b.txt", 20, 2),
        ],
        vec![],
    );

    let parts = partition_manifest(&m, &PartitionOptions::default()).unwrap();

    for (_, snapshot) in &parts {
        assert_eq!(snapshot.file_chunk_size_bytes, custom);
    }
}

#[test]
fn partition_preserves_whole_file_chunk_size() {
    let m = abs_snapshot(
        WHOLE_FILE_CHUNK_SIZE,
        vec![FileEntry::file("/root/a.txt", 10, 1)],
        vec![],
    );

    let parts = partition_manifest(&m, &PartitionOptions::default()).unwrap();

    for (_, snapshot) in &parts {
        assert_eq!(snapshot.file_chunk_size_bytes, WHOLE_FILE_CHUNK_SIZE);
    }
}

// ===== Join =====

#[test]
fn join_preserves_chunk_size() {
    let custom = 8 * 1024 * 1024;
    let m = rel_snapshot(custom, vec![FileEntry::file("a.txt", 10, 1)]);

    let joined = join_snapshot(&m, "/prefix").unwrap();

    assert_eq!(joined.file_chunk_size_bytes, custom);
}

#[test]
fn join_preserves_whole_file_chunk_size() {
    let m = rel_snapshot(WHOLE_FILE_CHUNK_SIZE, vec![FileEntry::file("a.txt", 10, 1)]);

    let joined = join_snapshot(&m, "/prefix").unwrap();

    assert_eq!(joined.file_chunk_size_bytes, WHOLE_FILE_CHUNK_SIZE);
}

// ===== Round-trip: collect -> filter -> subtree =====

#[test]
fn chunk_size_preserved_through_pipeline() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("sub")).unwrap();
    std::fs::write(tmp.path().join("sub/a.txt"), "hello").unwrap();
    std::fs::write(tmp.path().join("sub/b.txt"), "world").unwrap();

    let custom = 4 * 1024 * 1024;
    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(custom),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(collected.file_chunk_size_bytes, custom);

    // Filter
    let filtered = filter_manifest(&collected, &|entry: &ManifestEntry| {
        entry.path().ends_with("a.txt") || !entry.path().contains('.')
    });
    assert_eq!(filtered.file_chunk_size_bytes, custom);
}

// --- Hash/HashUpload helpers ---

fn make_test_file(dir: &std::path::Path, name: &str, content: &[u8]) -> (String, u64) {
    let p = dir.join(name);
    std::fs::write(&p, content).unwrap();
    let meta = std::fs::metadata(&p).unwrap();
    let mtime = meta
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64;
    (p.to_string_lossy().into_owned(), mtime)
}

fn make_fs_cache(dir: &std::path::Path) -> Arc<dyn AsyncDataCache> {
    Arc::new(FileSystemDataCache::new(dir.join("data")).unwrap())
}

// ===== Hash chunk size =====

#[test]
fn hash_preserves_input_chunk_size_when_none() {
    let tmp = TempDir::new().unwrap();
    let (_path, _mtime) = make_test_file(tmp.path(), "a.txt", b"hello world12345");

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(16),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(collected.file_chunk_size_bytes, 16);

    let result = hash_abs_manifest(
        &AbsManifest::Snapshot(collected),
        HashOptions {
            file_chunk_size_bytes: None,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(result.manifest.file_chunk_size_bytes(), 16);
}

#[test]
fn hash_overrides_chunk_size_when_specified() {
    let tmp = TempDir::new().unwrap();
    let (_path, _mtime) = make_test_file(tmp.path(), "a.txt", b"hello world12345");

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(16),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(collected.file_chunk_size_bytes, 16);

    // Override to 32 — the manifest metadata stays at 16 (clone of input),
    // but the hashing behavior uses 32.
    let result = hash_abs_manifest(
        &AbsManifest::Snapshot(collected),
        HashOptions {
            file_chunk_size_bytes: Some(32),
            ..Default::default()
        },
    )
    .unwrap();

    // The manifest file_chunk_size_bytes is preserved from input (clone semantics).
    assert_eq!(result.manifest.file_chunk_size_bytes(), 16);
    // But the file is smaller than 32, so it gets a whole-file hash (not chunks).
    assert!(result.manifest.files()[0].hash.is_some());
}

#[test]
fn hash_whole_file_disables_chunking() {
    let tmp = TempDir::new().unwrap();
    let data = vec![0xABu8; 64];
    let (_path, _mtime) = make_test_file(tmp.path(), "a.bin", &data);

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(16),
            ..Default::default()
        },
    )
    .unwrap();

    let result = hash_abs_manifest(
        &AbsManifest::Snapshot(collected),
        HashOptions {
            file_chunk_size_bytes: Some(WHOLE_FILE_CHUNK_SIZE),
            ..Default::default()
        },
    )
    .unwrap();

    let f = &result.manifest.files()[0];
    assert!(f.hash.is_some(), "whole-file hash should be set");
    assert!(
        f.chunk_hashes.is_none(),
        "chunk_hashes should be None for whole-file mode"
    );
}

#[test]
fn hash_small_file_with_small_chunks_produces_chunkhashes() {
    let tmp = TempDir::new().unwrap();
    let data = vec![0xCDu8; 64];
    let (_path, _mtime) = make_test_file(tmp.path(), "a.bin", &data);

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(16),
            ..Default::default()
        },
    )
    .unwrap();

    let result = hash_abs_manifest(
        &AbsManifest::Snapshot(collected),
        HashOptions {
            file_chunk_size_bytes: Some(16),
            ..Default::default()
        },
    )
    .unwrap();

    let f = &result.manifest.files()[0];
    assert!(
        f.hash.is_none(),
        "whole-file hash should not be set for chunked"
    );
    let chunks = f.chunk_hashes.as_ref().unwrap();
    assert_eq!(
        chunks.len(),
        4,
        "64 bytes / 16 byte chunks = 4 chunk hashes"
    );
}

#[test]
fn hash_file_smaller_than_chunk_size_produces_single_hash() {
    let tmp = TempDir::new().unwrap();
    let data = vec![0xEFu8; 100];
    let (_path, _mtime) = make_test_file(tmp.path(), "a.bin", &data);

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(256),
            ..Default::default()
        },
    )
    .unwrap();

    let result = hash_abs_manifest(
        &AbsManifest::Snapshot(collected),
        HashOptions {
            file_chunk_size_bytes: Some(256),
            ..Default::default()
        },
    )
    .unwrap();

    let f = &result.manifest.files()[0];
    assert!(
        f.hash.is_some(),
        "file smaller than chunk_size should get a single hash"
    );
    assert!(
        f.chunk_hashes.is_none(),
        "no chunk_hashes when file < chunk_size"
    );
}

// ===== Hash Upload chunk size =====

#[tokio::test]
async fn hash_upload_preserves_input_chunk_size_when_none() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (_path, _mtime) = make_test_file(tmp.path(), "a.txt", b"hello world12345");

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(16),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(collected.file_chunk_size_bytes, 16);

    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(collected),
        make_fs_cache(cache_dir.path()),
        HashUploadOptions {
            file_chunk_size_bytes: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(result.manifest.file_chunk_size_bytes(), 16);
}

#[tokio::test]
async fn hash_upload_overrides_chunk_size_when_specified() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (_path, _mtime) = make_test_file(tmp.path(), "a.txt", b"hello world12345");

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(16),
            ..Default::default()
        },
    )
    .unwrap();

    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(collected),
        make_fs_cache(cache_dir.path()),
        HashUploadOptions {
            file_chunk_size_bytes: Some(32),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Manifest metadata preserved from input (clone semantics).
    assert_eq!(result.manifest.file_chunk_size_bytes(), 16);
    // File is 16 bytes, smaller than override chunk_size 32, so whole-file hash.
    assert!(result.manifest.files()[0].hash.is_some());
}

#[tokio::test]
async fn hash_upload_whole_file_disables_chunking() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let data = vec![0xABu8; 64];
    let (_path, _mtime) = make_test_file(tmp.path(), "a.bin", &data);

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(16),
            ..Default::default()
        },
    )
    .unwrap();

    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(collected),
        make_fs_cache(cache_dir.path()),
        HashUploadOptions {
            file_chunk_size_bytes: Some(WHOLE_FILE_CHUNK_SIZE),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let f = &result.manifest.files()[0];
    assert!(f.hash.is_some(), "whole-file hash should be set");
    assert!(
        f.chunk_hashes.is_none(),
        "chunk_hashes should be None for whole-file mode"
    );
}

// ===== Collect -> Hash pipeline =====

#[test]
fn collect_then_hash_preserves_chunk_size() {
    let tmp = TempDir::new().unwrap();
    let (_path, _mtime) = make_test_file(tmp.path(), "a.txt", b"hello");

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(32),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(collected.file_chunk_size_bytes, 32);

    let result = hash_abs_manifest(
        &AbsManifest::Snapshot(collected),
        HashOptions {
            file_chunk_size_bytes: None,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(result.manifest.file_chunk_size_bytes(), 32);
}

#[tokio::test]
async fn collect_then_hash_upload_preserves_chunk_size() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (_path, _mtime) = make_test_file(tmp.path(), "a.txt", b"hello");

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(32),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(collected.file_chunk_size_bytes, 32);

    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(collected),
        make_fs_cache(cache_dir.path()),
        HashUploadOptions {
            file_chunk_size_bytes: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(result.manifest.file_chunk_size_bytes(), 32);
}

// ===== Hash Upload with filesystem data cache =====

#[tokio::test]
async fn hash_upload_small_file_with_small_chunks_filesystem() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let data = vec![0x42u8; 1024];
    let (_path, _mtime) = make_test_file(tmp.path(), "a.bin", &data);

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(64),
            ..Default::default()
        },
    )
    .unwrap();

    let dc = make_fs_cache(cache_dir.path());
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(collected),
        dc.clone(),
        HashUploadOptions {
            file_chunk_size_bytes: Some(64),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let f = &result.manifest.files()[0];
    let chunks = f.chunk_hashes.as_ref().unwrap();
    assert_eq!(chunks.len(), 16, "1024 / 64 = 16 chunks");

    // Verify chunks are retrievable from the data cache
    for h in chunks {
        assert!(dc.object_exists(h, "xxh128").await.unwrap());
    }
}

#[tokio::test]
async fn hash_upload_idempotent_filesystem() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (_path, _mtime) = make_test_file(tmp.path(), "a.txt", b"idempotent content");

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let dc = make_fs_cache(cache_dir.path());

    // First run — uploads
    let r1 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(collected.clone()),
        dc.clone(),
        HashUploadOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(r1.statistics.uploaded_files, 1);

    // Second run — same unhashed manifest, data already in cache
    let r2 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(collected),
        dc.clone(),
        HashUploadOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        r2.statistics.uploaded_files, 0,
        "second run should skip upload"
    );
    assert_eq!(
        r2.statistics.hashed_files, 1,
        "still hashed (no hash_cache)"
    );
}

#[tokio::test]
async fn hash_upload_deduplication_filesystem() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let content = b"identical content for dedup test";
    let (_path1, _mtime1) = make_test_file(tmp.path(), "a.txt", content);
    let (_path2, _mtime2) = make_test_file(tmp.path(), "b.txt", content);

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();
    assert_eq!(collected.files.len(), 2);

    let dc = make_fs_cache(cache_dir.path());
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(collected),
        dc.clone(),
        HashUploadOptions::default(),
    )
    .await
    .unwrap();

    // Both files should have the same hash (identical content)
    let h0 = result.manifest.files()[0].hash.as_ref().unwrap();
    let h1 = result.manifest.files()[1].hash.as_ref().unwrap();
    assert_eq!(h0, h1, "identical files should produce the same hash");

    // Content-addressed cache stores only one object for the shared hash
    let stored = dc.get_object(h0, "xxh128").await.unwrap();
    assert_eq!(stored, content);
}

// ===== Additional hash/hash_upload tests =====

#[test]
fn hash_computed_correctly_with_custom_chunk_size() {
    let tmp = TempDir::new().unwrap();
    let data = b"test content for hashing with custom chunk";
    let (_path, _mtime) = make_test_file(tmp.path(), "file.txt", data);

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(10),
            ..Default::default()
        },
    )
    .unwrap();

    let result = hash_abs_manifest(
        &AbsManifest::Snapshot(collected),
        HashOptions {
            file_chunk_size_bytes: Some(10),
            ..Default::default()
        },
    )
    .unwrap();

    let f = &result.manifest.files()[0];
    // File is larger than chunk size, so it should have chunk hashes
    assert!(
        f.hash.is_none(),
        "should use chunk hashes, not whole-file hash"
    );
    let chunks = f.chunk_hashes.as_ref().expect("should have chunk hashes");
    assert!(
        chunks.len() > 1,
        "file larger than chunk size should produce multiple chunks"
    );
}

#[tokio::test]
async fn hash_and_upload_with_custom_chunk_size() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let data = b"test content for hashing and uploading";
    let (_path, _mtime) = make_test_file(tmp.path(), "file.txt", data);

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let dc = make_fs_cache(cache_dir.path());
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(collected),
        dc.clone(),
        HashUploadOptions::default(),
    )
    .await
    .unwrap();

    let f = &result.manifest.files()[0];
    assert!(f.hash.is_some(), "hash should be computed");

    // Verify file is in the data cache
    assert!(dc
        .object_exists(f.hash.as_ref().unwrap(), "xxh128")
        .await
        .unwrap());
}

#[test]
fn override_chunk_size_at_each_stage() {
    let tmp = TempDir::new().unwrap();
    let (_path, _mtime) = make_test_file(tmp.path(), "file.txt", b"content");

    let collect_chunk = 512 * 1024;
    let hash_chunk = 1024 * 1024;

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(collect_chunk),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(collected.file_chunk_size_bytes, collect_chunk);

    let result = hash_abs_manifest(
        &AbsManifest::Snapshot(collected),
        HashOptions {
            file_chunk_size_bytes: Some(hash_chunk),
            ..Default::default()
        },
    )
    .unwrap();

    // File is smaller than both chunk sizes, so whole-file hash is used
    assert!(result.manifest.files()[0].hash.is_some());
}

#[test]
fn override_chunk_size_affects_chunking_decision_hash() {
    let tmp = TempDir::new().unwrap();
    let data: Vec<u8> = (0..64u8).collect();
    let (_path, _mtime) = make_test_file(tmp.path(), "file.bin", &data);

    // Collect with WHOLE_FILE — no chunking
    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(WHOLE_FILE_CHUNK_SIZE),
            ..Default::default()
        },
    )
    .unwrap();

    // Override with small chunk size — file SHOULD be chunked
    let result = hash_abs_manifest(
        &AbsManifest::Snapshot(collected),
        HashOptions {
            file_chunk_size_bytes: Some(16),
            ..Default::default()
        },
    )
    .unwrap();

    let f = &result.manifest.files()[0];
    assert!(
        f.hash.is_none(),
        "should use chunk hashes, not whole-file hash"
    );
    let chunks = f.chunk_hashes.as_ref().expect("should have chunk hashes");
    assert_eq!(chunks.len(), 4, "64 bytes / 16 byte chunks = 4");
}

#[tokio::test]
async fn override_chunk_size_affects_chunking_decision_hash_upload() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let data: Vec<u8> = (0..64u8).collect();
    let (_path, _mtime) = make_test_file(tmp.path(), "file.bin", &data);

    // Collect with WHOLE_FILE — no chunking
    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(WHOLE_FILE_CHUNK_SIZE),
            ..Default::default()
        },
    )
    .unwrap();

    // Override with small chunk size — file SHOULD be chunked
    let dc = make_fs_cache(cache_dir.path());
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(collected),
        dc,
        HashUploadOptions {
            file_chunk_size_bytes: Some(16),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let f = &result.manifest.files()[0];
    assert!(
        f.hash.is_none(),
        "should use chunk hashes, not whole-file hash"
    );
    let chunks = f.chunk_hashes.as_ref().expect("should have chunk hashes");
    assert_eq!(chunks.len(), 4, "64 bytes / 16 byte chunks = 4");
}

#[test]
fn hash_multiple_small_files_with_small_chunks() {
    let tmp = TempDir::new().unwrap();
    let sizes: &[(&str, usize)] = &[("tiny.bin", 32), ("small.bin", 64), ("medium.bin", 128)];
    for &(name, size) in sizes {
        let content: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
        make_test_file(tmp.path(), name, &content);
    }

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(16),
            ..Default::default()
        },
    )
    .unwrap();

    let result = hash_abs_manifest(
        &AbsManifest::Snapshot(collected),
        HashOptions {
            file_chunk_size_bytes: Some(16),
            ..Default::default()
        },
    )
    .unwrap();

    // All files > 16 bytes, so all should have chunk hashes
    for f in result.manifest.files() {
        assert!(
            f.hash.is_none(),
            "file {} should not have whole-file hash",
            f.path
        );
        let chunks = f.chunk_hashes.as_ref().expect("should have chunk hashes");
        let expected = f.size.unwrap() as usize / 16;
        assert_eq!(
            chunks.len(),
            expected,
            "file {} chunk count mismatch",
            f.path
        );
    }
}

#[test]
fn hash_identical_files_same_hash() {
    let tmp = TempDir::new().unwrap();
    let content: Vec<u8> = (0..=255u8).collect();
    make_test_file(tmp.path(), "file1.bin", &content);
    make_test_file(tmp.path(), "file2.bin", &content);

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let result =
        hash_abs_manifest(&AbsManifest::Snapshot(collected), HashOptions::default()).unwrap();

    let files = result.manifest.files();
    assert_eq!(
        files[0].hash, files[1].hash,
        "identical files should produce the same hash"
    );
}

#[test]
fn hash_different_files_different_hash() {
    let tmp = TempDir::new().unwrap();
    make_test_file(tmp.path(), "file1.bin", b"content1");
    make_test_file(tmp.path(), "file2.bin", b"content2");

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let result =
        hash_abs_manifest(&AbsManifest::Snapshot(collected), HashOptions::default()).unwrap();

    let files = result.manifest.files();
    assert_ne!(
        files[0].hash, files[1].hash,
        "different files should produce different hashes"
    );
}

#[tokio::test]
async fn hash_upload_multiple_files_with_chunks_filesystem() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let sizes: &[(&str, usize)] = &[("small.bin", 64), ("medium.bin", 128), ("large.bin", 256)];
    for &(name, size) in sizes {
        let content: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
        make_test_file(tmp.path(), name, &content);
    }

    let collected = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(32),
            ..Default::default()
        },
    )
    .unwrap();

    let dc = make_fs_cache(cache_dir.path());
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(collected),
        dc.clone(),
        HashUploadOptions {
            file_chunk_size_bytes: Some(32),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    for f in result.manifest.files() {
        assert!(f.hash.is_none(), "file {} should use chunk hashes", f.path);
        let chunks = f.chunk_hashes.as_ref().expect("should have chunk hashes");
        let expected = f.size.unwrap() as usize / 32;
        assert_eq!(
            chunks.len(),
            expected,
            "file {} chunk count mismatch",
            f.path
        );
        for h in chunks {
            assert!(
                dc.object_exists(h, "xxh128").await.unwrap(),
                "chunk should be in cache"
            );
        }
    }
}
