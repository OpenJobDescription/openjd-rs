// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.

//! Tests for hash_upload_abs_manifest ported from Python.
//!
//! Covers: basic hashing/uploading, metadata preservation, manifest type preservation,
//! special entries (symlinks, deleted), validation, cache integration, and deduplication.

use openjd_snapshots::{
    hash_upload_abs_manifest, AbsManifest, AbsSnapshot, AbsSnapshotDiff, AsyncDataCache, DirEntry,
    FileEntry, FileSystemDataCache, HashAlgorithm, HashCache, HashUploadOptions, Manifest,
    DEFAULT_FILE_CHUNK_SIZE,
};
use std::path::Path;
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use tempfile::TempDir;

fn make_test_file(dir: &Path, name: &str, content: &[u8]) -> (String, u64, u64) {
    let p = dir.join(name);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&p, content).unwrap();
    let meta = std::fs::metadata(&p).unwrap();
    let mtime = meta
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64;
    (
        p.to_string_lossy().into_owned(),
        content.len() as u64,
        mtime,
    )
}

fn make_snapshot(files: Vec<FileEntry>) -> AbsSnapshot {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files)
}

fn make_diff(files: Vec<FileEntry>) -> AbsSnapshotDiff {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files)
}

fn new_data_cache(tmp: &TempDir) -> Arc<dyn AsyncDataCache> {
    Arc::new(FileSystemDataCache::new(tmp.path().join("data")).unwrap())
}

// ===== Basic functionality =====

#[test]
fn empty_manifest() {
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let manifest = make_snapshot(vec![]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(result.manifest.files().len(), 0);
    assert_eq!(result.statistics.total_files, 0);
}

#[test]
fn single_file_hashed_and_stored() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "test.txt", b"Hello, World!");
    let dc = new_data_cache(&cache_dir);

    let manifest = make_snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    let f = &result.manifest.files()[0];
    let hash = f.hash.as_ref().unwrap();
    assert!(!hash.is_empty());
    assert_eq!(hash.len(), 32); // xxh128 hex
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    assert!(rt.block_on(dc.object_exists(hash, "xxh128")).unwrap());
    assert_eq!(
        rt.block_on(dc.get_object(hash, "xxh128")).unwrap(),
        b"Hello, World!"
    );
    assert_eq!(result.statistics.uploaded_files, 1);
    assert_eq!(result.statistics.uploaded_bytes, size);
}

#[test]
fn multiple_files() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (p1, s1, m1) = make_test_file(tmp.path(), "file1.txt", b"Content of file 1");
    let (p2, s2, m2) = make_test_file(tmp.path(), "file2.txt", b"Content of file 2");
    let (p3, s3, m3) = make_test_file(tmp.path(), "subdir/file3.txt", b"Content of file 3");
    let dc = new_data_cache(&cache_dir);

    let manifest = make_snapshot(vec![
        FileEntry::file(&p1, s1, m1),
        FileEntry::file(&p2, s2, m2),
        FileEntry::file(&p3, s3, m3),
    ]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    for f in result.manifest.files() {
        assert!(f.hash.is_some());
        assert_eq!(f.hash.as_ref().unwrap().len(), 32);
    }
    assert_eq!(result.statistics.uploaded_files, 3);
}

#[test]
fn hash_matches_direct_hash() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(
        tmp.path(),
        "test.txt",
        b"Test content for hash verification",
    );
    let dc = new_data_cache(&cache_dir);

    let manifest = make_snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    let expected = openjd_snapshots::hash::hash_file(Path::new(&path)).unwrap();
    assert_eq!(result.manifest.files()[0].hash.as_ref().unwrap(), &expected);
}

#[test]
fn preserves_metadata() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "test.txt", b"Content");
    let dc = new_data_cache(&cache_dir);

    let manifest = make_snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    let f = &result.manifest.files()[0];
    assert_eq!(f.size, Some(size));
    assert_eq!(f.mtime, Some(mtime));
    assert_eq!(f.path, path);
}

#[test]
fn preserves_runnable_flag_true() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "script.sh", b"#!/bin/bash\necho hello");
    let dc = new_data_cache(&cache_dir);

    let mut entry = FileEntry::file(&path, size, mtime);
    entry.runnable = true;
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(make_snapshot(vec![entry])),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    assert!(result.manifest.files()[0].runnable);
    assert!(result.manifest.files()[0].hash.is_some());
}

#[test]
fn preserves_runnable_flag_false() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "script.sh", b"#!/bin/bash\necho hello");
    let dc = new_data_cache(&cache_dir);

    let entry = FileEntry::file(&path, size, mtime);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(make_snapshot(vec![entry])),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    assert!(!result.manifest.files()[0].runnable);
    assert!(result.manifest.files()[0].hash.is_some());
}

// ===== Manifest type preservation =====

#[test]
fn returns_snapshot_for_snapshot_input() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "test.txt", b"content");
    let dc = new_data_cache(&cache_dir);

    let manifest = make_snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();
    assert!(matches!(result.manifest, AbsManifest::Snapshot(_)));
}

#[test]
fn returns_diff_for_diff_input() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "test.txt", b"content");
    let dc = new_data_cache(&cache_dir);

    let manifest = make_diff(vec![FileEntry::file(&path, size, mtime)])
        .with_parent_hash(Some("abc123".into()));
    let result =
        hash_upload_abs_manifest(&AbsManifest::Diff(manifest), dc.clone(), Default::default())
            .unwrap();
    assert!(matches!(result.manifest, AbsManifest::Diff(_)));
    assert_eq!(result.manifest.parent_manifest_hash(), Some("abc123"));
}

// ===== Special entries =====

#[test]
fn symlinks_pass_through_unchanged() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "target.txt", b"Target content");
    let dc = new_data_cache(&cache_dir);

    let manifest = make_snapshot(vec![
        FileEntry::file(&path, size, mtime),
        FileEntry::symlink("/tmp/link.txt", "/tmp/target.txt"),
    ]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    let files = result.manifest.files();
    let regular: Vec<_> = files
        .iter()
        .filter(|f| f.symlink_target.is_none())
        .collect();
    let symlinks: Vec<_> = files
        .iter()
        .filter(|f| f.symlink_target.is_some())
        .collect();

    assert_eq!(regular.len(), 1);
    assert_eq!(symlinks.len(), 1);
    assert!(regular[0].hash.is_some());
    assert!(symlinks[0].hash.is_none());
    assert_eq!(result.statistics.total_files, 1); // only regular files counted
}

#[test]
fn deleted_entries_pass_through() {
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let manifest = make_diff(vec![FileEntry::deleted("/some/deleted/file.txt")]);
    let result =
        hash_upload_abs_manifest(&AbsManifest::Diff(manifest), dc.clone(), Default::default())
            .unwrap();

    assert_eq!(result.manifest.files().len(), 1);
    assert!(result.manifest.files()[0].deleted);
    assert!(result.manifest.files()[0].hash.is_none());
    assert_eq!(result.statistics.total_files, 0);
}

#[test]
fn directories_pass_through() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "file.txt", b"Content");
    let dc = new_data_cache(&cache_dir);

    let manifest = make_snapshot(vec![FileEntry::file(&path, size, mtime)])
        .with_dirs(vec![DirEntry::new("/tmp/somedir")]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    assert_eq!(result.manifest.dirs().len(), 1);
    assert_eq!(result.statistics.uploaded_files, 1);
}

// ===== Validation =====

#[test]
fn rejects_already_hashed_files() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "a.txt", b"hello");
    let dc = new_data_cache(&cache_dir);

    let mut entry = FileEntry::file(&path, size, mtime);
    entry.hash = Some("existing_hash".into());
    let manifest = make_snapshot(vec![entry]);
    let err = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("already has hashes set"));
}

#[test]
fn rejects_already_chunk_hashed_files() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "a.txt", b"hello");
    let dc = new_data_cache(&cache_dir);

    let mut entry = FileEntry::file(&path, size, mtime);
    entry.chunk_hashes = Some(vec!["h1".into()]);
    let manifest = make_snapshot(vec![entry]);
    let err = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("already has hashes set"));
}

// ===== Content-addressable deduplication =====

#[test]
fn duplicate_content_stored_once() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (p1, s1, m1) = make_test_file(tmp.path(), "file1.txt", b"Same content");
    let (p2, s2, m2) = make_test_file(tmp.path(), "file2.txt", b"Same content");
    let dc = new_data_cache(&cache_dir);

    let manifest = make_snapshot(vec![
        FileEntry::file(&p1, s1, m1),
        FileEntry::file(&p2, s2, m2),
    ]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            max_workers: Some(1),
            ..Default::default()
        },
    )
    .unwrap();

    let files = result.manifest.files();
    assert_eq!(files[0].hash, files[1].hash);
    // Second file skipped upload since content already exists
    assert_eq!(result.statistics.uploaded_files, 1);
    assert_eq!(result.statistics.skipped_files, 1);
}

#[test]
fn second_upload_skips_existing() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "test.txt", b"Test content");
    let dc = new_data_cache(&cache_dir);

    let manifest = make_snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let _ = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest.clone()),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    let r2 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(r2.statistics.uploaded_files, 0);
    assert_eq!(r2.statistics.skipped_files, 1);
    assert_eq!(r2.statistics.hashed_files, 1); // still hashed, just not uploaded
}

// ===== Hash cache integration =====

#[test]
fn hash_cache_populates_on_first_run() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let hc_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "test.txt", b"Test content for caching");
    let dc = new_data_cache(&cache_dir);
    let hc = Arc::new(HashCache::new(hc_dir.path()).unwrap());

    let manifest = make_snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc.clone()),
            ..Default::default()
        },
    )
    .unwrap();

    // Hash cache should have an entry
    let cached = hc.get_if_fresh(
        Path::new(&path),
        "xxh128",
        0,
        openjd_snapshots::hash_cache::WHOLE_FILE_RANGE_END,
        mtime,
    );
    assert_eq!(cached.as_ref(), result.manifest.files()[0].hash.as_ref());
}

#[test]
fn hash_cache_enables_full_skip() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let hc_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "test.txt", b"hello");
    let dc = new_data_cache(&cache_dir);
    let hc = Arc::new(HashCache::new(hc_dir.path()).unwrap());

    let manifest = make_snapshot(vec![FileEntry::file(&path, size, mtime)]);

    // First upload populates both caches
    let _ = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest.clone()),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc.clone()),
            ..Default::default()
        },
    )
    .unwrap();

    // Second upload: hash cache hit + data cache hit => full skip
    let r2 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r2.statistics.skipped_files, 1);
    assert_eq!(r2.statistics.hashed_files, 0);
    assert_eq!(r2.statistics.uploaded_files, 0);
}

#[test]
fn hash_cache_miss_on_mtime_change() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let hc_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "test.txt", b"Original content");
    let dc = new_data_cache(&cache_dir);
    let hc = Arc::new(HashCache::new(hc_dir.path()).unwrap());

    let manifest = make_snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let r1 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc.clone()),
            ..Default::default()
        },
    )
    .unwrap();

    // Modify file
    std::thread::sleep(std::time::Duration::from_millis(10));
    std::fs::write(&path, b"Modified content").unwrap();
    let meta = std::fs::metadata(&path).unwrap();
    let new_mtime = meta
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64;
    let new_size = 16u64;

    let manifest2 = make_snapshot(vec![FileEntry::file(&path, new_size, new_mtime)]);
    let r2 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest2),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc),
            ..Default::default()
        },
    )
    .unwrap();

    assert_ne!(r1.manifest.files()[0].hash, r2.manifest.files()[0].hash);
    assert_eq!(r2.statistics.hashed_files, 1);
}

#[test]
fn force_rehash_bypasses_hash_cache() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let hc_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "test.txt", b"content");
    let dc = new_data_cache(&cache_dir);
    let hc = Arc::new(HashCache::new(hc_dir.path()).unwrap());

    let manifest = make_snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let opts = HashUploadOptions {
        hash_cache: Some(hc.clone()),
        ..Default::default()
    };
    let _ = hash_upload_abs_manifest(&AbsManifest::Snapshot(manifest.clone()), dc.clone(), opts)
        .unwrap();

    // With force_rehash, should re-hash even though cache has entry
    let opts2 = HashUploadOptions {
        hash_cache: Some(hc),
        force_rehash: true,
        ..Default::default()
    };
    let r2 = hash_upload_abs_manifest(&AbsManifest::Snapshot(manifest), dc.clone(), opts2).unwrap();
    assert_eq!(r2.statistics.hashed_files, 1);
}

// ===== Chunked upload =====

#[test]
fn chunked_upload_produces_chunk_hashes() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let data = vec![42u8; 64];
    let (path, size, mtime) = make_test_file(tmp.path(), "chunked.bin", &data);
    let dc = new_data_cache(&cache_dir);

    let chunk_size = 16i64;
    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(vec![FileEntry::file(&path, size, mtime)]);

    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .unwrap();

    let f = &result.manifest.files()[0];
    assert!(f.hash.is_none());
    let chunks = f.chunk_hashes.as_ref().unwrap();
    assert_eq!(chunks.len(), 4);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for h in chunks {
        assert!(rt.block_on(dc.object_exists(h, "xxh128")).unwrap());
        assert_eq!(rt.block_on(dc.get_object(h, "xxh128")).unwrap().len(), 16);
    }
}

// ===== Statistics =====

#[test]
fn statistics_are_accurate() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (p1, s1, m1) = make_test_file(tmp.path(), "a.txt", b"aaaa");
    let (p2, s2, m2) = make_test_file(tmp.path(), "b.txt", b"bb");
    let dc = new_data_cache(&cache_dir);

    let manifest = make_snapshot(vec![
        FileEntry::file(&p1, s1, m1),
        FileEntry::file(&p2, s2, m2),
    ]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    assert_eq!(result.statistics.total_files, 2);
    assert_eq!(result.statistics.total_bytes, 6);
    assert_eq!(result.statistics.hashed_files, 2);
    assert_eq!(result.statistics.hashed_bytes, 6);
    assert_eq!(result.statistics.uploaded_files, 2);
    assert_eq!(result.statistics.uploaded_bytes, 6);
    assert_eq!(result.statistics.skipped_files, 0);
}

use std::sync::atomic::{AtomicUsize, Ordering};

// ===== Progress callback and parallelism tests =====

#[test]
fn upload_progress_callback_invoked() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (p1, s1, m1) = make_test_file(tmp.path(), "a.txt", b"aaa");
    let (p2, s2, m2) = make_test_file(tmp.path(), "b.txt", b"bbb");
    let dc = new_data_cache(&cache_dir);

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = call_count.clone();

    let manifest = make_snapshot(vec![
        FileEntry::file(&p1, s1, m1),
        FileEntry::file(&p2, s2, m2),
    ]);
    let _ = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            on_progress: Some(Box::new(move |_stats| {
                cc.fetch_add(1, Ordering::Relaxed);
                true
            })),
            ..Default::default()
        },
    )
    .unwrap();

    assert!(call_count.load(Ordering::Relaxed) >= 1);
}

#[test]
fn upload_progress_callback_cancel() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let mut files = Vec::new();
    for i in 0..20 {
        let (p, s, m) = make_test_file(tmp.path(), &format!("f{i}.txt"), b"data");
        files.push(FileEntry::file(&p, s, m));
    }

    let manifest = make_snapshot(files);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            on_progress: Some(Box::new(|_| false)),
            max_workers: Some(1),
            ..Default::default()
        },
    );

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cancelled"));
}

#[test]
fn parallel_upload_produces_same_results() {
    let tmp = TempDir::new().unwrap();
    let cache_dir1 = TempDir::new().unwrap();
    let cache_dir2 = TempDir::new().unwrap();

    let mut files = Vec::new();
    for i in 0..10 {
        let content = format!("content_{i}");
        let (p, s, m) = make_test_file(tmp.path(), &format!("f{i}.txt"), content.as_bytes());
        files.push(FileEntry::file(&p, s, m));
    }

    let manifest = make_snapshot(files);
    let dc1 = new_data_cache(&cache_dir1);
    let dc2 = new_data_cache(&cache_dir2);

    let r_seq = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest.clone()),
        dc1.clone(),
        HashUploadOptions {
            max_workers: Some(1),
            ..Default::default()
        },
    )
    .unwrap();

    let r_par = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc2.clone(),
        HashUploadOptions {
            max_workers: Some(4),
            ..Default::default()
        },
    )
    .unwrap();

    for (a, b) in r_seq
        .manifest
        .files()
        .iter()
        .zip(r_par.manifest.files().iter())
    {
        assert_eq!(a.hash, b.hash);
    }
}

#[test]
fn statistics_include_hash_counts() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (p1, s1, m1) = make_test_file(tmp.path(), "a.txt", b"aaaa");
    let (p2, s2, m2) = make_test_file(tmp.path(), "b.txt", b"bb");
    let dc = new_data_cache(&cache_dir);

    let manifest = make_snapshot(vec![
        FileEntry::file(&p1, s1, m1),
        FileEntry::file(&p2, s2, m2),
    ]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    assert_eq!(result.statistics.hashed_files, 2);
    assert_eq!(result.statistics.hashed_bytes, 6);
}

// ===== Error handling tests =====

#[test]
fn upload_file_not_found_error() {
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let manifest = make_snapshot(vec![FileEntry::file(
        "/nonexistent/path/file.txt",
        100,
        1000,
    )]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions::default(),
    );
    assert!(result.is_err());
}

#[test]
fn upload_permission_denied_error() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (p, s, m) = make_test_file(tmp.path(), "readonly.txt", b"data");
    let dc = new_data_cache(&cache_dir);

    // Make file unreadable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
    }

    let manifest = make_snapshot(vec![FileEntry::file(&p, s, m)]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            max_workers: Some(1),
            ..Default::default()
        },
    );

    // Restore permissions for cleanup
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644));
    }

    #[cfg(unix)]
    assert!(result.is_err());
}

// ===== Concurrent deduplication tests =====

#[test]
fn concurrent_dedup_identical_files() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let content = b"identical content for dedup test";
    let mut files = Vec::new();
    for i in 0..8 {
        let (p, s, m) = make_test_file(tmp.path(), &format!("dup{i}.txt"), content);
        files.push(FileEntry::file(&p, s, m));
    }

    let manifest = make_snapshot(files);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            max_workers: Some(4),
            ..Default::default()
        },
    )
    .unwrap();

    // All files should have the same hash
    let hashes: std::collections::HashSet<_> = result
        .manifest
        .files()
        .iter()
        .map(|f| f.hash.as_ref().unwrap().as_str())
        .collect();
    assert_eq!(hashes.len(), 1);

    // Only 1 file in cache (all identical)
    let cache_files: Vec<_> = std::fs::read_dir(cache_dir.path().join("data"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .collect();
    assert_eq!(cache_files.len(), 1);
}

#[test]
fn concurrent_dedup_mixed_content() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let (p1, s1, m1) = make_test_file(tmp.path(), "a.txt", b"content_a");
    let (p2, s2, m2) = make_test_file(tmp.path(), "b.txt", b"content_a"); // same as a
    let (p3, s3, m3) = make_test_file(tmp.path(), "c.txt", b"content_c_unique");
    let (p4, s4, m4) = make_test_file(tmp.path(), "d.txt", b"content_a"); // same as a

    let manifest = make_snapshot(vec![
        FileEntry::file(&p1, s1, m1),
        FileEntry::file(&p2, s2, m2),
        FileEntry::file(&p3, s3, m3),
        FileEntry::file(&p4, s4, m4),
    ]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            max_workers: Some(4),
            ..Default::default()
        },
    )
    .unwrap();

    // a, b, d should have same hash; c different
    let files = result.manifest.files();
    assert_eq!(files[0].hash, files[1].hash);
    assert_eq!(files[0].hash, files[3].hash);
    assert_ne!(files[0].hash, files[2].hash);

    // 2 unique files in cache
    let cache_files: Vec<_> = std::fs::read_dir(cache_dir.path().join("data"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .collect();
    assert_eq!(cache_files.len(), 2);
}

#[test]
fn concurrent_dedup_chunked_identical_chunks() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let chunk_size = 256i64;
    let data = vec![42u8; 1024]; // 4 identical chunks
    let (p, s, m) = make_test_file(tmp.path(), "repeated.bin", &data);

    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(vec![FileEntry::file(&p, s, m)]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            file_chunk_size_bytes: Some(chunk_size),
            max_workers: Some(4),
            ..Default::default()
        },
    )
    .unwrap();

    let chunks = result.manifest.files()[0].chunk_hashes.as_ref().unwrap();
    assert_eq!(chunks.len(), 4);
    // All chunks identical
    assert_eq!(
        std::collections::HashSet::<&String>::from_iter(chunks.iter()).len(),
        1
    );

    // Only 1 chunk file in cache
    let cache_files: Vec<_> = std::fs::read_dir(cache_dir.path().join("data"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .collect();
    assert_eq!(cache_files.len(), 1);
}

#[test]
fn concurrent_dedup_chunked_mixed_chunks() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let chunk_size = 16i64;
    // chunk0=A, chunk1=B, chunk2=A, chunk3=C (A appears twice)
    let mut data = Vec::new();
    data.extend_from_slice(&[b'a'; 16]);
    data.extend_from_slice(&[b'b'; 16]);
    data.extend_from_slice(&[b'a'; 16]);
    data.extend_from_slice(&[b'c'; 16]);
    let (p, s, m) = make_test_file(tmp.path(), "mixed.bin", &data);

    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(vec![FileEntry::file(&p, s, m)]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            file_chunk_size_bytes: Some(chunk_size),
            max_workers: Some(4),
            ..Default::default()
        },
    )
    .unwrap();

    let chunks = result.manifest.files()[0].chunk_hashes.as_ref().unwrap();
    assert_eq!(chunks.len(), 4);
    assert_eq!(chunks[0], chunks[2]); // A == A
    assert_ne!(chunks[0], chunks[1]); // A != B
    assert_ne!(chunks[1], chunks[3]); // B != C

    // 3 unique chunk files in cache (A, B, C)
    let cache_files: Vec<_> = std::fs::read_dir(cache_dir.path().join("data"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .collect();
    assert_eq!(cache_files.len(), 3);
}

// ---------------------------------------------------------------------------
// Progress metadata tests
// ---------------------------------------------------------------------------

#[test]
fn upload_progress_fields_populated() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (p, s, m) = make_test_file(tmp.path(), "a.txt", b"hello world");
    let dc = new_data_cache(&cache_dir);
    let manifest = make_snapshot(vec![FileEntry::file(&p, s, m)]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions::default(),
    )
    .unwrap();
    assert!(result.statistics.total_time > 0.0);
    assert!(result.statistics.rate >= 0.0);
    assert!((result.statistics.progress - 100.0).abs() < 0.01);
}

#[test]
fn upload_progress_zero_for_empty_manifest() {
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let manifest = make_snapshot(vec![]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions::default(),
    )
    .unwrap();
    assert_eq!(result.statistics.total_files, 0);
    assert_eq!(result.statistics.progress, 0.0);
}

#[test]
fn upload_progress_callback_receives_timing() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let mut files = Vec::new();
    for i in 0..5 {
        let (p, s, m) = make_test_file(
            tmp.path(),
            &format!("f{i}.txt"),
            format!("content{i}").repeat(100).as_bytes(),
        );
        files.push(FileEntry::file(&p, s, m));
    }
    let manifest = make_snapshot(files);
    let times = Arc::new(std::sync::Mutex::new(Vec::new()));
    let t = times.clone();
    let _ = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            on_progress: Some(Box::new(move |stats| {
                t.lock().unwrap().push(stats.total_time);
                true
            })),
            ..Default::default()
        },
    )
    .unwrap();
    let t = times.lock().unwrap();
    assert!(!t.is_empty());
    for i in 1..t.len() {
        assert!(
            t[i] >= t[i - 1],
            "total_time not monotonic: {} < {}",
            t[i],
            t[i - 1]
        );
    }
}

#[test]
fn upload_progress_rate_positive() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (p, s, m) = make_test_file(tmp.path(), "big.txt", &vec![0u8; 10000]);
    let dc = new_data_cache(&cache_dir);
    let manifest = make_snapshot(vec![FileEntry::file(&p, s, m)]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions::default(),
    )
    .unwrap();
    assert!(result.statistics.rate > 0.0);
}

#[test]
fn upload_progress_with_cache_skip() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let hc_dir = TempDir::new().unwrap();
    let (p, s, m) = make_test_file(tmp.path(), "a.txt", b"cached content");
    let dc = new_data_cache(&cache_dir);
    let cache = Arc::new(HashCache::new(hc_dir.path()).unwrap());
    let manifest = make_snapshot(vec![FileEntry::file(&p, s, m)]);
    // First upload
    let _ = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest.clone()),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(cache.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    // Second upload - full skip
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(cache),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.statistics.skipped_files, 1);
    assert!((result.statistics.progress - 100.0).abs() < 0.01);
}

#[test]
fn upload_progress_upload_skip_on_data_cache_hit() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (p, s, m) = make_test_file(tmp.path(), "a.txt", b"dedup test");
    let dc = new_data_cache(&cache_dir);
    let manifest = make_snapshot(vec![FileEntry::file(&p, s, m)]);
    // First upload
    let _ = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest.clone()),
        dc.clone(),
        HashUploadOptions::default(),
    )
    .unwrap();
    // Second upload - data already in cache, upload skipped but still hashed
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions::default(),
    )
    .unwrap();
    assert_eq!(result.statistics.uploaded_files, 0);
    assert_eq!(result.statistics.skipped_files, 1);
}

// ===== Validation: path tests =====

#[test]
fn rejects_relative_file_path() {
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let manifest = make_snapshot(vec![FileEntry::file("relative/path.txt", 10, 1000)]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    );
    assert!(result.is_err());
}

#[test]
fn rejects_relative_directory_path() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "file.txt", b"data");
    let dc = new_data_cache(&cache_dir);
    let manifest = make_snapshot(vec![FileEntry::file(&path, size, mtime)])
        .with_dirs(vec![DirEntry::new("relative/dir")]);
    // Relative dir paths pass through (dirs aren't read from disk)
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap().manifest.dirs()[0].path, "relative/dir");
}

#[test]
fn accepts_absolute_path() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "abs.txt", b"absolute");
    let dc = new_data_cache(&cache_dir);
    assert!(path.starts_with('/'));
    let manifest = make_snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();
    assert!(result.manifest.files()[0].hash.is_some());
    assert_eq!(result.statistics.uploaded_files, 1);
}

// ===== Cache: force_rehash and statistics =====

#[test]
fn force_rehash_false_uses_cache() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let hc_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "cached.txt", b"cache me");
    let dc = new_data_cache(&cache_dir);
    let hc = Arc::new(HashCache::new(hc_dir.path()).unwrap());

    let manifest = make_snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let _ = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest.clone()),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc.clone()),
            ..Default::default()
        },
    )
    .unwrap();

    // force_rehash defaults to false; second run should skip hashing via cache
    let r2 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc),
            force_rehash: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r2.statistics.hashed_files, 0);
    assert_eq!(r2.statistics.skipped_files, 1);
}

#[test]
fn statistics_count_skipped_bytes() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let hc_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "skip.txt", b"skip these bytes");
    let dc = new_data_cache(&cache_dir);
    let hc = Arc::new(HashCache::new(hc_dir.path()).unwrap());

    let manifest = make_snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let _ = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest.clone()),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc.clone()),
            ..Default::default()
        },
    )
    .unwrap();

    let r2 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r2.statistics.skipped_bytes, size);
    assert_eq!(r2.statistics.hashed_bytes, 0);
}

#[test]
fn chunked_file_with_some_duplicate_chunks() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let chunk_size = 16i64;
    // chunk0=A, chunk1=B, chunk2=A => 3 chunks, 2 unique
    let mut data = vec![b'a'; 16];
    data.extend_from_slice(&[b'b'; 16]);
    data.extend_from_slice(&[b'a'; 16]);
    let (path, size, mtime) = make_test_file(tmp.path(), "dup_chunks.bin", &data);

    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(vec![FileEntry::file(&path, size, mtime)]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            file_chunk_size_bytes: Some(chunk_size),
            max_workers: Some(1),
            ..Default::default()
        },
    )
    .unwrap();

    let chunks = result.manifest.files()[0].chunk_hashes.as_ref().unwrap();
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0], chunks[2]); // A == A
    assert_ne!(chunks[0], chunks[1]); // A != B

    // Only 2 unique objects in cache
    let cache_files: Vec<_> = std::fs::read_dir(cache_dir.path().join("data"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .collect();
    assert_eq!(cache_files.len(), 2);
}

// ===== Progress: cache hits and monotonic time =====

#[test]
fn upload_progress_with_hash_cache_hits() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let hc_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let hc = Arc::new(HashCache::new(hc_dir.path()).unwrap());

    let (p, s, m) = make_test_file(tmp.path(), "prog.txt", b"progress cache test");
    let manifest = make_snapshot(vec![FileEntry::file(&p, s, m)]);

    // First run populates caches
    let _ = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest.clone()),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc.clone()),
            ..Default::default()
        },
    )
    .unwrap();

    // Second run: progress callback should fire and report skipped file
    let saw_skip = Arc::new(AtomicUsize::new(0));
    let ss = saw_skip.clone();
    let r2 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc),
            on_progress: Some(Box::new(move |stats| {
                if stats.skipped_files > 0 {
                    ss.fetch_add(1, Ordering::Relaxed);
                }
                true
            })),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r2.statistics.skipped_files, 1);
    // The final progress callback should have reported the skip
    assert!(saw_skip.load(Ordering::Relaxed) >= 1);
}

#[test]
fn upload_progress_total_time_increases_monotonically() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let mut files = Vec::new();
    for i in 0..5 {
        let (p, s, m) = make_test_file(
            tmp.path(),
            &format!("m{i}.txt"),
            format!("mono{i}").repeat(50).as_bytes(),
        );
        files.push(FileEntry::file(&p, s, m));
    }
    let manifest = make_snapshot(files);

    let times = Arc::new(std::sync::Mutex::new(Vec::new()));
    let t = times.clone();
    let _ = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            on_progress: Some(Box::new(move |stats| {
                t.lock().unwrap().push(stats.total_time);
                true
            })),
            max_workers: Some(1),
            ..Default::default()
        },
    )
    .unwrap();

    let t = times.lock().unwrap();
    assert!(!t.is_empty());
    for i in 1..t.len() {
        assert!(
            t[i] >= t[i - 1],
            "total_time not monotonic: {} < {}",
            t[i],
            t[i - 1]
        );
    }
}

// ===== Progress message tests =====

#[test]
fn upload_progress_message_uses_files_for_whole_file_mode() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let mut files = Vec::new();
    for i in 0..3 {
        let (p, s, m) = make_test_file(
            tmp.path(),
            &format!("f{i}.txt"),
            format!("content{i}").as_bytes(),
        );
        files.push(FileEntry::file(&p, s, m));
    }
    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, -1) // WHOLE_FILE
        .with_files(files);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions::default(),
    )
    .unwrap();
    assert!(
        result.statistics.progress_message.contains("files"),
        "message: {}",
        result.statistics.progress_message
    );
    assert!(
        !result.statistics.progress_message.contains("chunks"),
        "message: {}",
        result.statistics.progress_message
    );
    assert!(
        result.statistics.progress_message.contains("(3 files)"),
        "message: {}",
        result.statistics.progress_message
    );
}

#[test]
fn upload_progress_message_uses_chunks_for_chunked_mode() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let (p, s, m) = make_test_file(tmp.path(), "large.txt", &[b'x'; 100]);
    let chunk_size = 32i64;
    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(vec![FileEntry::file(&p, s, m)]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .unwrap();
    // The message should say "chunks" not "files" since chunk_size > 0
    assert!(
        result.statistics.progress_message.contains("chunks"),
        "message: {}",
        result.statistics.progress_message
    );
    assert!(
        !result.statistics.progress_message.contains("files"),
        "message: {}",
        result.statistics.progress_message
    );
}

#[test]
fn upload_progress_message_contains_rate() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let (p, s, m) = make_test_file(tmp.path(), "test.txt", b"rate test content");
    let manifest = make_snapshot(vec![FileEntry::file(&p, s, m)]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions::default(),
    )
    .unwrap();
    assert!(
        result.statistics.progress_message.contains("/s)"),
        "message: {}",
        result.statistics.progress_message
    );
}

// ===== Memory and chunking boundary tests =====

#[test]
fn hash_upload_with_custom_max_memory() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let mut files = Vec::new();
    for i in 0..5 {
        let content = format!("file_{i}_content").repeat(100);
        let (p, s, m) = make_test_file(tmp.path(), &format!("f{i}.txt"), content.as_bytes());
        files.push(FileEntry::file(&p, s, m));
    }

    let manifest = make_snapshot(files);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            max_memory_bytes: Some(1024), // very small
            max_workers: Some(2),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(result.statistics.total_files, 5);
    assert_eq!(result.statistics.uploaded_files, 5);
    for f in result.manifest.files() {
        assert!(f.hash.is_some());
    }
}

#[test]
fn hash_upload_chunked_boundary_sizes() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let chunk_size = 32i64;

    // Exactly 1 chunk boundary (32 bytes)
    let (p1, s1, m1) = make_test_file(tmp.path(), "exact1.bin", &[b'a'; 32]);
    // Exactly 2 chunks (64 bytes)
    let (p2, s2, m2) = make_test_file(tmp.path(), "exact2.bin", &[b'b'; 64]);
    // 1 byte over boundary (33 bytes -> 2 chunks)
    let (p3, s3, m3) = make_test_file(tmp.path(), "over.bin", &[b'c'; 33]);

    // File at exactly chunk_size is NOT chunked (use_chunks requires file_size > chunk_size)
    let manifest1: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(vec![FileEntry::file(&p1, s1, m1)]);
    let r1 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest1),
        dc.clone(),
        HashUploadOptions {
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .unwrap();
    // 32 bytes == chunk_size, so NOT chunked (use_chunks = file_size > chunk_size)
    assert!(r1.manifest.files()[0].hash.is_some());
    assert!(r1.manifest.files()[0].chunk_hashes.is_none());

    // 64 bytes > chunk_size -> chunked into 2 chunks
    let manifest2: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(vec![FileEntry::file(&p2, s2, m2)]);
    let r2 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest2),
        dc.clone(),
        HashUploadOptions {
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        r2.manifest.files()[0].chunk_hashes.as_ref().unwrap().len(),
        2
    );

    // 33 bytes > chunk_size -> chunked into 2 chunks (32 + 1)
    let manifest3: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(vec![FileEntry::file(&p3, s3, m3)]);
    let r3 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest3),
        dc.clone(),
        HashUploadOptions {
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        r3.manifest.files()[0].chunk_hashes.as_ref().unwrap().len(),
        2
    );
}

#[test]
fn hash_upload_mixed_small_and_large_files() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let chunk_size = 64i64;

    // Small file (below chunk_size) -> whole-file hash
    let (p1, s1, m1) = make_test_file(tmp.path(), "small.txt", b"tiny");
    // Large file (above chunk_size) -> chunked
    let (p2, s2, m2) = make_test_file(tmp.path(), "large.bin", &[b'x'; 200]);

    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size).with_files(vec![
        FileEntry::file(&p1, s1, m1),
        FileEntry::file(&p2, s2, m2),
    ]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .unwrap();

    let files = result.manifest.files();
    // Small file: whole-file hash
    assert!(files[0].hash.is_some());
    assert!(files[0].chunk_hashes.is_none());
    // Large file: chunk hashes (200 / 64 = 3 full + 1 partial = 4 chunks)
    assert!(files[1].hash.is_none());
    let chunks = files[1].chunk_hashes.as_ref().unwrap();
    assert_eq!(chunks.len(), 4); // ceil(200/64) = 4
}

// ===== Additional hash cache and streaming tests =====

#[test]
fn some_files_cached_others_not() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let hc_dir = TempDir::new().unwrap();
    let (p1, s1, m1) = make_test_file(tmp.path(), "file1.txt", b"content1");
    let (p2, s2, m2) = make_test_file(tmp.path(), "file2.txt", b"content2");
    let dc = new_data_cache(&cache_dir);
    let hc = Arc::new(HashCache::new(hc_dir.path()).unwrap());

    // First run: only file1
    let manifest1 = make_snapshot(vec![FileEntry::file(&p1, s1, m1)]);
    let r1 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest1),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let hash1_first = r1.manifest.files()[0].hash.clone().unwrap();

    // Second run: both files
    let manifest2 = make_snapshot(vec![
        FileEntry::file(&p1, s1, m1),
        FileEntry::file(&p2, s2, m2),
    ]);
    let r2 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest2),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc),
            ..Default::default()
        },
    )
    .unwrap();

    let files = r2.manifest.files();
    assert!(files[0].hash.is_some());
    assert!(files[1].hash.is_some());
    assert_eq!(files[0].hash.as_ref().unwrap(), &hash1_first);
}

#[test]
fn some_chunks_cached_others_not() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let hc_dir = TempDir::new().unwrap();
    let mut data = vec![b'a'; 16];
    data.extend_from_slice(&[b'b'; 16]);
    let (path, size, mtime) = make_test_file(tmp.path(), "chunked.bin", &data);
    let dc = new_data_cache(&cache_dir);
    let hc = Arc::new(HashCache::new(hc_dir.path()).unwrap());
    let chunk_size = 16i64;

    // First run populates cache
    let manifest1: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(vec![FileEntry::file(&path, size, mtime)]);
    let r1 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest1),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc.clone()),
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .unwrap();
    let chunks1 = r1.manifest.files()[0].chunk_hashes.clone().unwrap();

    // Second run should use cached chunk hashes
    let manifest2: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(vec![FileEntry::file(&path, size, mtime)]);
    let r2 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest2),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc),
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .unwrap();
    let chunks2 = r2.manifest.files()[0].chunk_hashes.clone().unwrap();

    assert_eq!(chunks1, chunks2);
}

#[test]
fn hash_cache_stores_chunk_ranges() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let hc_dir = TempDir::new().unwrap();
    let data = vec![42u8; 64];
    let (path, size, mtime) = make_test_file(tmp.path(), "chunked.bin", &data);
    let dc = new_data_cache(&cache_dir);
    let hc = Arc::new(HashCache::new(hc_dir.path()).unwrap());
    let chunk_size = 16i64;

    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(vec![FileEntry::file(&path, size, mtime)]);
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc.clone()),
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .unwrap();

    let chunks = result.manifest.files()[0].chunk_hashes.as_ref().unwrap();
    assert_eq!(chunks.len(), 4);

    let p = Path::new(&path);
    for (i, expected_hash) in chunks.iter().enumerate() {
        let start = (i as i64) * 16;
        let end = start + 16;
        let cached = hc.get_if_fresh(p, "xxh128", start, end, mtime);
        assert_eq!(cached.as_ref(), Some(expected_hash));
    }
}

#[test]
fn hash_cache_hit_for_chunked_file() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let hc_dir = TempDir::new().unwrap();
    let mut data = vec![b'a'; 16];
    data.extend_from_slice(&[b'b'; 16]);
    let (path, size, mtime) = make_test_file(tmp.path(), "chunked.bin", &data);
    let dc = new_data_cache(&cache_dir);
    let hc = Arc::new(HashCache::new(hc_dir.path()).unwrap());
    let chunk_size = 16i64;

    // First run
    let manifest1: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(vec![FileEntry::file(&path, size, mtime)]);
    let r1 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest1),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc.clone()),
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .unwrap();
    let chunks1 = r1.manifest.files()[0].chunk_hashes.clone().unwrap();

    // Second run: hash cache + data cache hit => full skip
    let manifest2: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(vec![FileEntry::file(&path, size, mtime)]);
    let r2 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest2),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc),
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .unwrap();
    let chunks2 = r2.manifest.files()[0].chunk_hashes.clone().unwrap();

    assert_eq!(chunks1, chunks2);
    assert_eq!(r2.statistics.skipped_files, 1);
    assert_eq!(r2.statistics.hashed_files, 0);
}

#[test]
fn three_passes_with_same_unhashed_manifest() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let hc_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "test.txt", b"three pass test");
    let dc = new_data_cache(&cache_dir);
    let hc = Arc::new(HashCache::new(hc_dir.path()).unwrap());

    let mut hashes = Vec::new();
    for _ in 0..3 {
        let manifest = make_snapshot(vec![FileEntry::file(&path, size, mtime)]);
        let result = hash_upload_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            dc.clone(),
            HashUploadOptions {
                hash_cache: Some(hc.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        hashes.push(result.manifest.files()[0].hash.clone().unwrap());
    }

    assert_eq!(hashes[0], hashes[1]);
    assert_eq!(hashes[1], hashes[2]);
}

#[test]
fn data_cache_miss_after_hash_cache_hit_reuploads() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let hc_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "test.txt", b"reupload test");
    let dc = new_data_cache(&cache_dir);
    let hc = Arc::new(HashCache::new(hc_dir.path()).unwrap());

    // First run: populates hash cache + data cache
    let manifest1 = make_snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let r1 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest1),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let hash1 = r1.manifest.files()[0].hash.clone().unwrap();

    // Delete the file from data cache
    let data_path = cache_dir
        .path()
        .join("data")
        .join(format!("{hash1}.xxh128"));
    std::fs::remove_file(&data_path).unwrap();

    // Second run: hash cache hits but data cache misses → re-hash and re-upload
    let manifest2 = make_snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let r2 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest2),
        dc.clone(),
        HashUploadOptions {
            hash_cache: Some(hc),
            ..Default::default()
        },
    )
    .unwrap();
    let hash2 = r2.manifest.files()[0].hash.clone().unwrap();

    assert_eq!(hash1, hash2);
    assert!(data_path.exists());
}

#[test]
fn streaming_file_with_hash_cache() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let hc_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "stream.bin", &[0xABu8; 100]);
    let dc = new_data_cache(&cache_dir);
    let hc = Arc::new(HashCache::new(hc_dir.path()).unwrap());

    let opts = || HashUploadOptions {
        hash_cache: Some(hc.clone()),
        max_memory_bytes: Some(32),
        file_chunk_size_bytes: Some(-1),
        ..Default::default()
    };

    // First run
    let manifest1 = make_snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let r1 =
        hash_upload_abs_manifest(&AbsManifest::Snapshot(manifest1), dc.clone(), opts()).unwrap();
    let hash1 = r1.manifest.files()[0].hash.clone().unwrap();

    // Second run: hash cache hit
    let manifest2 = make_snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let r2 =
        hash_upload_abs_manifest(&AbsManifest::Snapshot(manifest2), dc.clone(), opts()).unwrap();
    let hash2 = r2.manifest.files()[0].hash.clone().unwrap();

    assert_eq!(hash1, hash2);
}

#[test]
fn streaming_file_skipped_when_exists_in_data_cache() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let hc_dir = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "stream.bin", &[0xCDu8; 100]);
    let dc = new_data_cache(&cache_dir);
    let hc = Arc::new(HashCache::new(hc_dir.path()).unwrap());

    let opts = || HashUploadOptions {
        hash_cache: Some(hc.clone()),
        max_memory_bytes: Some(32),
        file_chunk_size_bytes: Some(-1),
        ..Default::default()
    };

    // First run uploads
    let manifest1 = make_snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let r1 =
        hash_upload_abs_manifest(&AbsManifest::Snapshot(manifest1), dc.clone(), opts()).unwrap();
    let hash1 = r1.manifest.files()[0].hash.clone().unwrap();

    // Second run: hash cache + data cache hit => full skip
    let manifest2 = make_snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let r2 =
        hash_upload_abs_manifest(&AbsManifest::Snapshot(manifest2), dc.clone(), opts()).unwrap();
    let hash2 = r2.manifest.files()[0].hash.clone().unwrap();

    assert_eq!(hash1, hash2);
    assert_eq!(r2.statistics.skipped_files, 1);
    assert_eq!(r2.statistics.hashed_files, 0);
    assert_eq!(r2.statistics.uploaded_files, 0);
}
