// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

// Ported from deadline-cloud hash_abs_manifest Python tests.

use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

use openjd_snapshots::{
    hash_abs_manifest, AbsManifest, DirEntry, FileEntry, HashAlgorithm, HashCache, HashOptions,
    Manifest, DEFAULT_FILE_CHUNK_SIZE,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_file(dir: &Path, name: &str, content: &[u8]) -> (String, u64, u64) {
    let p = dir.join(name);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&p, content).unwrap();
    let meta = std::fs::metadata(&p).unwrap();
    let mtime = meta
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64;
    (
        p.to_string_lossy().into_owned(),
        content.len() as u64,
        mtime,
    )
}

fn snapshot(files: Vec<FileEntry>) -> AbsManifest {
    AbsManifest::Snapshot(
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files),
    )
}

fn diff(files: Vec<FileEntry>, dirs: Vec<DirEntry>, parent: Option<&str>) -> AbsManifest {
    AbsManifest::Diff(
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(files)
            .with_dirs(dirs)
            .with_parent_hash(parent.map(String::from)),
    )
}

fn new_cache() -> (TempDir, Arc<HashCache>) {
    let tmp = TempDir::new().unwrap();
    let cache = Arc::new(HashCache::new(tmp.path()).unwrap());
    (tmp, cache)
}

// ---------------------------------------------------------------------------
// Basic hashing
// ---------------------------------------------------------------------------

#[test]
fn hash_single_file() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "test.txt", b"hello world");

    let m = snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();

    let f = &result.manifest.files()[0];
    assert!(f.hash.is_some());
    assert_eq!(f.hash.as_ref().unwrap().len(), 32);
}

#[test]
fn hash_matches_direct_hash() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "test.txt", b"test content for hashing");

    let m = snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();

    let expected = openjd_snapshots::hash::hash_file(Path::new(&path)).unwrap();
    assert_eq!(
        result.manifest.files()[0].hash.as_deref(),
        Some(expected.as_str())
    );
}

#[test]
fn hash_multiple_files() {
    let tmp = TempDir::new().unwrap();
    let (p1, s1, m1) = make_file(tmp.path(), "a.txt", b"aaa");
    let (p2, s2, m2) = make_file(tmp.path(), "b.txt", b"bbbbb");

    let m = snapshot(vec![
        FileEntry::file(&p1, s1, m1),
        FileEntry::file(&p2, s2, m2),
    ]);
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();

    assert_eq!(result.manifest.files().len(), 2);
    for f in result.manifest.files() {
        assert!(f.hash.is_some());
        assert_eq!(f.hash.as_ref().unwrap().len(), 32);
    }
}

#[test]
fn hash_is_deterministic() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "det.txt", b"deterministic");

    let m1 = snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let m2 = snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let r1 = hash_abs_manifest(&m1, HashOptions::default()).unwrap();
    let r2 = hash_abs_manifest(&m2, HashOptions::default()).unwrap();

    assert_eq!(r1.manifest.files()[0].hash, r2.manifest.files()[0].hash);
}

#[test]
fn preserves_metadata() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "test.txt", b"content");

    let m = snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();

    let f = &result.manifest.files()[0];
    assert_eq!(f.size, Some(size));
    assert_eq!(f.mtime, Some(mtime));
    // Compare against the library's own normalization (via FileEntry::new).
    assert_eq!(f.path, FileEntry::new(&path).path);
}

#[test]
fn total_size_calculated() {
    let tmp = TempDir::new().unwrap();
    let (p1, s1, m1) = make_file(tmp.path(), "a.txt", b"aaa");
    let (p2, s2, m2) = make_file(tmp.path(), "b.txt", b"bbbbb");

    let m = snapshot(vec![
        FileEntry::file(&p1, s1, m1),
        FileEntry::file(&p2, s2, m2),
    ]);
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();
    assert_eq!(result.manifest.total_size(), 8);
}

#[test]
fn preserves_runnable_flag() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "script.sh", b"#!/bin/bash");

    let mut entry = FileEntry::file(&path, size, mtime);
    entry.runnable = true;
    let m = snapshot(vec![entry]);
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();

    assert!(result.manifest.files()[0].runnable);
}

#[test]
fn snapshot_type_preserved() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "test.txt", b"content");

    let m = snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();
    assert!(matches!(result.manifest, AbsManifest::Snapshot(_)));
}

// ---------------------------------------------------------------------------
// Symlinks and deleted pass through
// ---------------------------------------------------------------------------

#[test]
fn symlinks_pass_through() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "real.txt", b"data");

    let m = diff(
        vec![
            FileEntry::file(&path, size, mtime),
            FileEntry::symlink("/tmp/link", "/tmp/target"),
        ],
        vec![],
        None,
    );
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();

    assert!(result.manifest.files()[0].hash.is_some());
    assert!(result.manifest.files()[1].hash.is_none());
    assert_eq!(
        result.manifest.files()[1].symlink_target.as_deref(),
        Some("/tmp/target")
    );
}

#[test]
fn deleted_pass_through() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "real.txt", b"data");

    let m = diff(
        vec![
            FileEntry::file(&path, size, mtime),
            FileEntry::deleted("/tmp/gone"),
        ],
        vec![],
        None,
    );
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();

    assert!(result.manifest.files()[0].hash.is_some());
    assert!(result.manifest.files()[1].hash.is_none());
    assert!(result.manifest.files()[1].deleted);
}

// ---------------------------------------------------------------------------
// Validation: already-hashed rejected
// ---------------------------------------------------------------------------

#[test]
fn rejects_already_hashed_file() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "a.txt", b"hello");

    let mut entry = FileEntry::file(&path, size, mtime);
    entry.hash = Some("existing_hash".into());
    let m = snapshot(vec![entry]);

    let err = hash_abs_manifest(&m, HashOptions::default()).unwrap_err();
    assert!(err.to_string().contains("already has hashes set"));
}

#[test]
fn rejects_already_chunkhashed_file() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "a.txt", b"hello");

    let mut entry = FileEntry::file(&path, size, mtime);
    entry.chunk_hashes = Some(vec!["abc".into()]);
    let m = snapshot(vec![entry]);

    let err = hash_abs_manifest(&m, HashOptions::default()).unwrap_err();
    assert!(err.to_string().contains("already has hashes set"));
}

#[test]
fn already_hashed_symlink_not_rejected() {
    // Symlinks are skipped by validation — they never have hashes anyway
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "real.txt", b"data");

    let m = diff(
        vec![
            FileEntry::file(&path, size, mtime),
            FileEntry::symlink("/tmp/link", "/tmp/target"),
        ],
        vec![],
        None,
    );
    assert!(hash_abs_manifest(&m, HashOptions::default()).is_ok());
}

#[test]
fn already_hashed_deleted_not_rejected() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "real.txt", b"data");

    let m = diff(
        vec![
            FileEntry::file(&path, size, mtime),
            FileEntry::deleted("/tmp/gone"),
        ],
        vec![],
        None,
    );
    assert!(hash_abs_manifest(&m, HashOptions::default()).is_ok());
}

// ---------------------------------------------------------------------------
// Diff manifests
// ---------------------------------------------------------------------------

#[test]
fn diff_hashes_new_files() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "new_file.txt", b"new content");

    let m = diff(
        vec![FileEntry::file(&path, size, mtime)],
        vec![],
        Some("parent123"),
    );
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();

    assert!(matches!(result.manifest, AbsManifest::Diff(_)));
    assert_eq!(result.manifest.parent_manifest_hash(), Some("parent123"));
    assert!(result.manifest.files()[0].hash.is_some());
    assert_eq!(result.manifest.files()[0].hash.as_ref().unwrap().len(), 32);
}

#[test]
fn diff_mixed_entries() {
    let tmp = TempDir::new().unwrap();
    let (new_path, ns, nm) = make_file(tmp.path(), "new.txt", b"new content");
    let (mod_path, ms, mm) = make_file(tmp.path(), "modified.txt", b"modified content");

    let m = diff(
        vec![
            FileEntry::file(&new_path, ns, nm),
            FileEntry::file(&mod_path, ms, mm),
            FileEntry::deleted("/old/deleted.txt"),
        ],
        vec![DirEntry::new("/new/dir"), {
            let mut d = DirEntry::new("/deleted/dir");
            d.deleted = true;
            d
        }],
        Some("parent789"),
    );
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();

    assert!(matches!(result.manifest, AbsManifest::Diff(_)));
    assert_eq!(result.manifest.parent_manifest_hash(), Some("parent789"));
    assert_eq!(result.manifest.dirs().len(), 2);
    assert!(!result.manifest.dirs()[0].deleted);
    assert!(result.manifest.dirs()[1].deleted);

    assert_eq!(result.manifest.files().len(), 3);
    assert!(result.manifest.files()[0].hash.is_some());
    assert!(result.manifest.files()[1].hash.is_some());
    assert!(result.manifest.files()[2].deleted);
    assert!(result.manifest.files()[2].hash.is_none());
}

#[test]
fn diff_parent_hash_none_preserved() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "file.txt", b"content");

    let m = diff(vec![FileEntry::file(&path, size, mtime)], vec![], None);
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();

    assert!(matches!(result.manifest, AbsManifest::Diff(_)));
    assert_eq!(result.manifest.parent_manifest_hash(), None);
}

#[test]
fn diff_type_preserved() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "test.txt", b"content");

    let m = diff(
        vec![FileEntry::file(&path, size, mtime)],
        vec![],
        Some("parent123"),
    );
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();
    assert!(matches!(result.manifest, AbsManifest::Diff(_)));
}

// ---------------------------------------------------------------------------
// Cache integration: populate, hit, force_rehash, stale mtime
// ---------------------------------------------------------------------------

#[test]
fn cache_populated_after_hashing() {
    let tmp = TempDir::new().unwrap();
    let (_cache_dir, cache) = new_cache();
    let (path, size, mtime) = make_file(tmp.path(), "a.txt", b"hello");

    let m = snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let result = hash_abs_manifest(
        &m,
        HashOptions {
            hash_cache: Some(cache.clone()),
            ..Default::default()
        },
    )
    .unwrap();

    let cached = cache.get(
        Path::new(&path),
        "xxh128",
        0,
        openjd_snapshots::hash_cache::WHOLE_FILE_RANGE_END,
    );
    assert!(cached.is_some());
    let (hash, _) = cached.unwrap();
    assert_eq!(
        hash,
        result.manifest.files()[0].hash.as_ref().unwrap().as_str()
    );
}

#[test]
fn cache_hit_returns_cached_hash() {
    let tmp = TempDir::new().unwrap();
    let (_cache_dir, cache) = new_cache();
    let (path, size, mtime) = make_file(tmp.path(), "a.txt", b"hello");

    let fake_hash = "a".repeat(32);
    cache
        .put(
            Path::new(&path),
            "xxh128",
            0,
            openjd_snapshots::hash_cache::WHOLE_FILE_RANGE_END,
            &fake_hash,
            mtime,
        )
        .unwrap();

    let m = snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let result = hash_abs_manifest(
        &m,
        HashOptions {
            hash_cache: Some(cache),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        result.manifest.files()[0].hash.as_deref(),
        Some(fake_hash.as_str())
    );
}

#[test]
fn cache_miss_on_stale_mtime() {
    let tmp = TempDir::new().unwrap();
    let (_cache_dir, cache) = new_cache();
    let (path, size, mtime) = make_file(tmp.path(), "a.txt", b"hello");

    let fake_hash = "a".repeat(32);
    cache
        .put(
            Path::new(&path),
            "xxh128",
            0,
            openjd_snapshots::hash_cache::WHOLE_FILE_RANGE_END,
            &fake_hash,
            0, // stale mtime
        )
        .unwrap();

    let m = snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let result = hash_abs_manifest(
        &m,
        HashOptions {
            hash_cache: Some(cache),
            ..Default::default()
        },
    )
    .unwrap();

    assert_ne!(
        result.manifest.files()[0].hash.as_deref(),
        Some(fake_hash.as_str())
    );
    assert!(result.manifest.files()[0].hash.is_some());
}

#[test]
fn force_rehash_ignores_cache() {
    let tmp = TempDir::new().unwrap();
    let (_cache_dir, cache) = new_cache();
    let (path, size, mtime) = make_file(tmp.path(), "a.txt", b"hello");

    cache
        .put(
            Path::new(&path),
            "xxh128",
            0,
            openjd_snapshots::hash_cache::WHOLE_FILE_RANGE_END,
            "stale_hash",
            mtime,
        )
        .unwrap();

    let m = snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let result = hash_abs_manifest(
        &m,
        HashOptions {
            hash_cache: Some(cache.clone()),
            force_rehash: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert_ne!(
        result.manifest.files()[0].hash.as_deref(),
        Some("stale_hash")
    );
    assert!(result.manifest.files()[0].hash.is_some());

    // Cache should be updated with the new hash
    let cached = cache.get(
        Path::new(&path),
        "xxh128",
        0,
        openjd_snapshots::hash_cache::WHOLE_FILE_RANGE_END,
    );
    let (new_hash, _) = cached.unwrap();
    assert_eq!(
        new_hash,
        result.manifest.files()[0].hash.as_ref().unwrap().as_str()
    );
}

#[test]
fn no_cache_always_computes() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "test.txt", b"content");

    let m = snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();

    let expected = openjd_snapshots::hash::hash_file(Path::new(&path)).unwrap();
    assert_eq!(
        result.manifest.files()[0].hash.as_deref(),
        Some(expected.as_str())
    );
}

// ---------------------------------------------------------------------------
// Chunked hashing
// ---------------------------------------------------------------------------

#[test]
fn chunked_hashing_produces_chunks() {
    let tmp = TempDir::new().unwrap();
    let (path, _size, mtime) = make_file(tmp.path(), "large.bin", &vec![0u8; 1024]);

    let chunk_size = 256i64;
    let m = AbsManifest::Snapshot(
        Manifest::new(HashAlgorithm::Xxh128, chunk_size)
            .with_files(vec![FileEntry::file(&path, 1024, mtime)]),
    );
    let result = hash_abs_manifest(
        &m,
        HashOptions {
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .unwrap();

    let f = &result.manifest.files()[0];
    assert!(f.hash.is_none());
    let chunks = f.chunk_hashes.as_ref().unwrap();
    assert_eq!(chunks.len(), 4); // 1024 / 256
}

#[test]
fn chunked_hashing_is_deterministic() {
    let tmp = TempDir::new().unwrap();
    let (path, _size, mtime) = make_file(tmp.path(), "large.bin", &vec![42u8; 1024]);

    let chunk_size = 256i64;
    let make = || {
        AbsManifest::Snapshot(
            Manifest::new(HashAlgorithm::Xxh128, chunk_size)
                .with_files(vec![FileEntry::file(&path, 1024, mtime)]),
        )
    };
    let opts = || HashOptions {
        file_chunk_size_bytes: Some(chunk_size),
        ..Default::default()
    };

    let r1 = hash_abs_manifest(&make(), opts()).unwrap();
    let r2 = hash_abs_manifest(&make(), opts()).unwrap();

    assert_eq!(
        r1.manifest.files()[0].chunk_hashes,
        r2.manifest.files()[0].chunk_hashes,
    );
}

#[test]
fn chunked_hashing_with_cache() {
    let tmp = TempDir::new().unwrap();
    let (_cache_dir, cache) = new_cache();
    let (path, _size, mtime) = make_file(tmp.path(), "large.bin", &vec![0u8; 1024]);

    let chunk_size = 256i64;
    let m = AbsManifest::Snapshot(
        Manifest::new(HashAlgorithm::Xxh128, chunk_size)
            .with_files(vec![FileEntry::file(&path, 1024, mtime)]),
    );

    // First call populates cache
    let r1 = hash_abs_manifest(
        &m,
        HashOptions {
            hash_cache: Some(cache.clone()),
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .unwrap();

    // Second call should use cache
    let m2 = AbsManifest::Snapshot(
        Manifest::new(HashAlgorithm::Xxh128, chunk_size)
            .with_files(vec![FileEntry::file(&path, 1024, mtime)]),
    );
    let r2 = hash_abs_manifest(
        &m2,
        HashOptions {
            hash_cache: Some(cache),
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        r1.manifest.files()[0].chunk_hashes,
        r2.manifest.files()[0].chunk_hashes,
    );
}

use std::sync::atomic::{AtomicUsize, Ordering};

// ---------------------------------------------------------------------------
// Progress callback and parallelism tests
// ---------------------------------------------------------------------------

#[test]
fn progress_callback_invoked() {
    let tmp = TempDir::new().unwrap();
    let (p1, s1, m1) = make_file(tmp.path(), "a.txt", b"aaa");
    let (p2, s2, m2) = make_file(tmp.path(), "b.txt", b"bbb");

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = call_count.clone();

    let m = snapshot(vec![
        FileEntry::file(&p1, s1, m1),
        FileEntry::file(&p2, s2, m2),
    ]);
    let _ = hash_abs_manifest(
        &m,
        HashOptions {
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
fn progress_callback_cancel() {
    let tmp = TempDir::new().unwrap();
    // Create many files so cancellation has a chance to take effect
    let mut files = Vec::new();
    for i in 0..20 {
        let (p, s, m) = make_file(tmp.path(), &format!("f{i}.txt"), b"data");
        files.push(FileEntry::file(&p, s, m));
    }

    let m = snapshot(files);
    let result = hash_abs_manifest(
        &m,
        HashOptions {
            on_progress: Some(Box::new(|_| false)), // cancel immediately
            max_workers: Some(1),
            ..Default::default()
        },
    );

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cancelled"));
}

#[test]
fn hash_result_has_statistics() {
    let tmp = TempDir::new().unwrap();
    let (p1, s1, m1) = make_file(tmp.path(), "a.txt", b"aaa");
    let (p2, s2, m2) = make_file(tmp.path(), "b.txt", b"bbbbb");

    let m = snapshot(vec![
        FileEntry::file(&p1, s1, m1),
        FileEntry::file(&p2, s2, m2),
    ]);
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();

    assert_eq!(result.statistics.total_files, 2);
    assert_eq!(result.statistics.total_bytes, 8);
    assert_eq!(result.statistics.hashed_files, 2);
    assert_eq!(result.statistics.hashed_bytes, 8);
}

#[test]
fn parallel_hashing_produces_same_results() {
    let tmp = TempDir::new().unwrap();
    let mut files = Vec::new();
    for i in 0..10 {
        let content = format!("content_{i}");
        let (p, s, m) = make_file(tmp.path(), &format!("f{i}.txt"), content.as_bytes());
        files.push(FileEntry::file(&p, s, m));
    }

    let m = snapshot(files);

    let r_seq = hash_abs_manifest(
        &m,
        HashOptions {
            max_workers: Some(1),
            ..Default::default()
        },
    )
    .unwrap();

    let r_par = hash_abs_manifest(
        &m,
        HashOptions {
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
        assert_eq!(a.chunk_hashes, b.chunk_hashes);
    }
}

// ---------------------------------------------------------------------------
// Progress metadata tests
// ---------------------------------------------------------------------------

#[test]
fn progress_fields_populated() {
    let tmp = TempDir::new().unwrap();
    let (p1, s1, m1) = make_file(tmp.path(), "a.txt", b"hello world");
    let m = snapshot(vec![FileEntry::file(&p1, s1, m1)]);
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();
    assert!(result.statistics.total_time > 0.0);
    assert!(result.statistics.rate >= 0.0);
    assert!((result.statistics.progress - 100.0).abs() < 0.01);
}

// ---------------------------------------------------------------------------
// hash_algorithm_preserved (from test_hash_manifest.py)
// ---------------------------------------------------------------------------

#[test]
fn hash_algorithm_preserved() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "test.txt", b"hello");

    let m = snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();

    assert_eq!(result.manifest.hash_alg(), HashAlgorithm::Xxh128);
}

// ---------------------------------------------------------------------------
// computes_hash_without_cache (from test_hash_manifest.py)
// ---------------------------------------------------------------------------

#[test]
fn computes_hash_without_cache() {
    let tmp = TempDir::new().unwrap();
    let (p1, s1, m1) = make_file(tmp.path(), "a.txt", b"aaa");
    let (p2, s2, m2) = make_file(tmp.path(), "b.txt", b"bbb");

    let m = snapshot(vec![
        FileEntry::file(&p1, s1, m1),
        FileEntry::file(&p2, s2, m2),
    ]);
    // No cache provided (default)
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();

    for f in result.manifest.files() {
        assert!(f.hash.is_some());
        assert_eq!(f.hash.as_ref().unwrap().len(), 32);
    }
}

// ---------------------------------------------------------------------------
// Chunking tests (from test_hash_manifest_chunking.py)
// ---------------------------------------------------------------------------

#[test]
fn single_chunk_for_small_file() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "small.txt", b"tiny");

    let chunk_size = 1024i64; // file is smaller than chunk_size
    let m = AbsManifest::Snapshot(
        Manifest::new(HashAlgorithm::Xxh128, chunk_size)
            .with_files(vec![FileEntry::file(&path, size, mtime)]),
    );
    let result = hash_abs_manifest(
        &m,
        HashOptions {
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .unwrap();

    let f = &result.manifest.files()[0];
    // Small file: whole-file hash, no chunk_hashes
    assert!(f.hash.is_some());
    assert!(f.chunk_hashes.is_none());
}

#[test]
fn chunk_hashes_are_different() {
    let tmp = TempDir::new().unwrap();
    // Two chunks with different content
    let mut data = vec![0u8; 512];
    for b in &mut data[256..] {
        *b = 0xFF;
    }
    let (path, _size, mtime) = make_file(tmp.path(), "diff_chunks.bin", &data);

    let chunk_size = 256i64;
    let m = AbsManifest::Snapshot(
        Manifest::new(HashAlgorithm::Xxh128, chunk_size)
            .with_files(vec![FileEntry::file(&path, 512, mtime)]),
    );
    let result = hash_abs_manifest(
        &m,
        HashOptions {
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .unwrap();

    let chunks = result.manifest.files()[0].chunk_hashes.as_ref().unwrap();
    assert_eq!(chunks.len(), 2);
    assert_ne!(chunks[0], chunks[1]);
}

#[test]
fn identical_chunks_same_hash() {
    let tmp = TempDir::new().unwrap();
    let data = vec![42u8; 512]; // two identical 256-byte chunks
    let (path, _size, mtime) = make_file(tmp.path(), "same_chunks.bin", &data);

    let chunk_size = 256i64;
    let m = AbsManifest::Snapshot(
        Manifest::new(HashAlgorithm::Xxh128, chunk_size)
            .with_files(vec![FileEntry::file(&path, 512, mtime)]),
    );
    let result = hash_abs_manifest(
        &m,
        HashOptions {
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .unwrap();

    let chunks = result.manifest.files()[0].chunk_hashes.as_ref().unwrap();
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0], chunks[1]);
}

#[test]
fn force_rehash_ignores_chunk_cache() {
    let tmp = TempDir::new().unwrap();
    let (_cache_dir, cache) = new_cache();
    let data = vec![0u8; 512];
    let (path, _size, mtime) = make_file(tmp.path(), "chunked.bin", &data);

    let chunk_size = 256i64;

    // First hash populates cache
    let m1 = AbsManifest::Snapshot(
        Manifest::new(HashAlgorithm::Xxh128, chunk_size)
            .with_files(vec![FileEntry::file(&path, 512, mtime)]),
    );
    let r1 = hash_abs_manifest(
        &m1,
        HashOptions {
            hash_cache: Some(cache.clone()),
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .unwrap();

    // Force rehash should recompute, not use cache
    let m2 = AbsManifest::Snapshot(
        Manifest::new(HashAlgorithm::Xxh128, chunk_size)
            .with_files(vec![FileEntry::file(&path, 512, mtime)]),
    );
    let r2 = hash_abs_manifest(
        &m2,
        HashOptions {
            hash_cache: Some(cache),
            file_chunk_size_bytes: Some(chunk_size),
            force_rehash: true,
            ..Default::default()
        },
    )
    .unwrap();

    // Results should be the same (same file), but cache was bypassed
    assert_eq!(
        r1.manifest.files()[0].chunk_hashes,
        r2.manifest.files()[0].chunk_hashes,
    );
    // Verify it actually computed hashes (not empty)
    assert!(r2.manifest.files()[0].chunk_hashes.as_ref().unwrap().len() == 2);
}

// ---------------------------------------------------------------------------
// Special entries (from test_hash_manifest_special_entries.py)
// ---------------------------------------------------------------------------

#[test]
fn hash_symlinks_pass_through_unchanged() {
    let entry = FileEntry::symlink("/tmp/link", "/tmp/target");
    let m = diff(vec![entry], vec![], None);
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();

    let f = &result.manifest.files()[0];
    assert!(f.hash.is_none());
    assert!(f.chunk_hashes.is_none());
    assert_eq!(f.symlink_target.as_deref(), Some("/tmp/target"));
}

#[test]
fn hash_diff_preserves_deleted_entries() {
    let entry = FileEntry::deleted("/tmp/gone");
    let m = diff(vec![entry], vec![], None);
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();

    let f = &result.manifest.files()[0];
    assert!(f.deleted);
    assert!(f.hash.is_none());
    assert!(f.chunk_hashes.is_none());
}

#[test]
fn hash_directories_pass_through_unchanged() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "file.txt", b"data");

    let m = diff(
        vec![FileEntry::file(&path, size, mtime)],
        vec![DirEntry::new("/some/dir")],
        None,
    );
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();

    assert_eq!(result.manifest.dirs().len(), 1);
    assert_eq!(result.manifest.dirs()[0].path, "/some/dir");
    assert!(!result.manifest.dirs()[0].deleted);
}

#[test]
fn hash_mixed_entries() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "real.txt", b"content");

    let m = diff(
        vec![
            FileEntry::file(&path, size, mtime),
            FileEntry::symlink("/tmp/link", "/tmp/target"),
            FileEntry::deleted("/tmp/gone"),
        ],
        vec![DirEntry::new("/some/dir")],
        None,
    );
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();

    let files = result.manifest.files();
    // Regular file gets hashed
    assert!(files[0].hash.is_some());
    // Symlink unchanged
    assert!(files[1].hash.is_none());
    assert_eq!(files[1].symlink_target.as_deref(), Some("/tmp/target"));
    // Deleted unchanged
    assert!(files[2].hash.is_none());
    assert!(files[2].deleted);
    // Directory unchanged
    assert_eq!(result.manifest.dirs().len(), 1);
}

// ---------------------------------------------------------------------------
// Validation (from test_hash_manifest_validation.py)
// ---------------------------------------------------------------------------

#[test]
fn hash_rejects_relative_paths() {
    // A manifest with a relative path should fail to hash (file not found)
    let entry = FileEntry::file("relative/path.txt", 10, 1);
    let m = snapshot(vec![entry]);
    assert!(hash_abs_manifest(&m, HashOptions::default()).is_err());
}

#[test]
fn hash_accepts_absolute_paths() {
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_file(tmp.path(), "abs.txt", b"absolute");

    // The library normalizes paths on FileEntry construction; confirm the
    // tempfile path normalizes to an absolute form via that public path.
    let normalized = FileEntry::new(&path).path;
    assert!(
        normalized.starts_with('/') || normalized.chars().nth(1) == Some(':'),
        "path should be absolute: {normalized}"
    );
    let m = snapshot(vec![FileEntry::file(&path, size, mtime)]);
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();
    assert!(result.manifest.files()[0].hash.is_some());
}

// ---------------------------------------------------------------------------
// Progress metadata tests (continued)
// ---------------------------------------------------------------------------

#[test]
fn progress_zero_for_empty_manifest() {
    let m = snapshot(vec![]);
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();
    assert_eq!(result.statistics.total_files, 0);
    assert_eq!(result.statistics.progress, 0.0);
}

#[test]
fn progress_callback_receives_timing() {
    let tmp = TempDir::new().unwrap();
    let mut files = Vec::new();
    for i in 0..5 {
        let (p, s, m) = make_file(
            tmp.path(),
            &format!("f{i}.txt"),
            format!("content{i}").repeat(100).as_bytes(),
        );
        files.push(FileEntry::file(&p, s, m));
    }
    let m = snapshot(files);
    let times = Arc::new(std::sync::Mutex::new(Vec::new()));
    let t = times.clone();
    let _ = hash_abs_manifest(
        &m,
        HashOptions {
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
    for &time in t.iter() {
        assert!(time >= 0.0);
    }
    // Monotonically non-decreasing
    for i in 1..t.len() {
        assert!(t[i] >= t[i - 1]);
    }
}

#[test]
fn progress_rate_is_positive_after_hashing() {
    let tmp = TempDir::new().unwrap();
    let (p, s, m) = make_file(tmp.path(), "big.txt", &vec![0u8; 10000]);
    let m = snapshot(vec![FileEntry::file(&p, s, m)]);
    let result = hash_abs_manifest(&m, HashOptions::default()).unwrap();
    assert!(result.statistics.rate > 0.0);
    assert!(result.statistics.total_time > 0.0);
}

#[test]
fn progress_with_cache_hits_counts_skipped() {
    let tmp = TempDir::new().unwrap();
    let hc_dir = TempDir::new().unwrap();
    let (p, s, m) = make_file(tmp.path(), "a.txt", b"cached");
    let cache = Arc::new(HashCache::new(hc_dir.path()).unwrap());
    let manifest = snapshot(vec![FileEntry::file(&p, s, m)]);
    // First hash populates cache
    let _ = hash_abs_manifest(
        &manifest,
        HashOptions {
            hash_cache: Some(cache.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    // Second hash hits cache
    let result = hash_abs_manifest(
        &manifest,
        HashOptions {
            hash_cache: Some(cache),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.statistics.skipped_files, 1);
    assert!((result.statistics.progress - 100.0).abs() < 0.01);
}
