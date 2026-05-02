// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for concurrent upload deduplication in hash_upload.
//!
//! Uses a mock AsyncDataCache with artificial latency on put_object to
//! expose the race window between object_exists and put_object. Without
//! deduplication, concurrent workers all see "not found" and all upload.

use async_trait::async_trait;
use openjd_snapshots::{
    hash_upload_abs_manifest, AbsManifest, AbsSnapshot, AsyncDataCache, CopyResult, FileEntry,
    HashAlgorithm, HashUploadOptions, Manifest, DEFAULT_FILE_CHUNK_SIZE,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;
use tempfile::TempDir;

/// A mock data cache where put_object takes a configurable amount of time.
/// This means the first worker's upload is still in-flight when subsequent
/// workers call object_exists, so they all see "not found" and all upload.
struct SlowPutDataCache {
    store: Mutex<HashMap<String, Vec<u8>>>,
    total_puts: AtomicUsize,
    put_delay: std::time::Duration,
}

impl SlowPutDataCache {
    fn new(put_delay: std::time::Duration) -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
            total_puts: AtomicUsize::new(0),
            put_delay,
        }
    }

    fn total_puts(&self) -> usize {
        self.total_puts.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl AsyncDataCache for SlowPutDataCache {
    fn object_key(&self, hash: &str, algorithm: &str) -> String {
        format!("{hash}.{algorithm}")
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn object_exists(&self, hash: &str, algorithm: &str) -> std::io::Result<bool> {
        let key = self.object_key(hash, algorithm);
        Ok(self.store.lock().unwrap().contains_key(&key))
    }

    async fn put_object(
        &self,
        hash: &str,
        algorithm: &str,
        data: Vec<u8>,
    ) -> std::io::Result<String> {
        let key = self.object_key(hash, algorithm);
        // Simulate slow upload (e.g. S3 PutObject) — the object is not visible
        // until the upload completes, so concurrent object_exists calls return false.
        if !self.put_delay.is_zero() {
            tokio::time::sleep(self.put_delay).await;
        }
        self.store.lock().unwrap().insert(key.clone(), data);
        self.total_puts.fetch_add(1, Ordering::SeqCst);
        Ok(key)
    }

    async fn get_object(&self, hash: &str, algorithm: &str) -> std::io::Result<Vec<u8>> {
        let key = self.object_key(hash, algorithm);
        self.store
            .lock()
            .unwrap()
            .get(&key)
            .cloned()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "not found"))
    }

    async fn copy_from(
        &self,
        _source: &dyn AsyncDataCache,
        _hash: &str,
        _algorithm: &str,
    ) -> std::io::Result<CopyResult> {
        Ok(CopyResult::NotSupported)
    }

    fn multipart_part_size(&self) -> usize {
        32 * 1024 * 1024
    }
}

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

/// With multiple workers and identical files, each unique hash should be
/// uploaded exactly once. The slow put_object simulates a real S3 upload
/// where the object isn't visible until the upload completes, creating a
/// window where concurrent workers all see object_exists=false.
#[tokio::test]
async fn concurrent_identical_files_upload_exactly_once() {
    let tmp = TempDir::new().unwrap();
    let content = b"identical content for concurrent dedup test";

    let mut files = Vec::new();
    for i in 0..16 {
        let (p, s, m) = make_test_file(tmp.path(), &format!("dup{i}.txt"), content);
        files.push(FileEntry::file(&p, s, m));
    }

    // Slow put_object: 200ms per upload. With 8 workers, all will call
    // object_exists before the first put_object completes.
    let dc: Arc<dyn AsyncDataCache> =
        Arc::new(SlowPutDataCache::new(std::time::Duration::from_millis(200)));

    let manifest: AbsSnapshot =
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files);

    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            max_workers: Some(8),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // All files should have the same hash
    let hashes: std::collections::HashSet<_> = result
        .manifest
        .files()
        .iter()
        .map(|f| f.hash.as_ref().unwrap().as_str())
        .collect();
    assert_eq!(hashes.len(), 1, "all files should have the same hash");

    // The critical assertion: put_object should be called exactly once.
    let counting = dc.as_any().downcast_ref::<SlowPutDataCache>().unwrap();
    assert_eq!(
        counting.total_puts(),
        1,
        "expected exactly 1 put_object call for identical content, got {}",
        counting.total_puts()
    );
}

/// With a mix of duplicate and unique files, put_object should be called
/// exactly once per unique hash.
#[tokio::test]
async fn concurrent_mixed_content_uploads_once_per_unique_hash() {
    let tmp = TempDir::new().unwrap();

    // 4 files with content_a, 1 with content_b, 3 with content_c = 3 unique
    let mut files = Vec::new();
    for i in 0..4 {
        let (p, s, m) = make_test_file(tmp.path(), &format!("a{i}.txt"), b"content_a_dedup");
        files.push(FileEntry::file(&p, s, m));
    }
    {
        let (p, s, m) = make_test_file(tmp.path(), "b0.txt", b"content_b_unique");
        files.push(FileEntry::file(&p, s, m));
    }
    for i in 0..3 {
        let (p, s, m) = make_test_file(tmp.path(), &format!("c{i}.txt"), b"content_c_dedup");
        files.push(FileEntry::file(&p, s, m));
    }

    let dc: Arc<dyn AsyncDataCache> =
        Arc::new(SlowPutDataCache::new(std::time::Duration::from_millis(200)));

    let manifest: AbsSnapshot =
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files);

    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        HashUploadOptions {
            max_workers: Some(8),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Verify hashes are correct
    let a_hash = result.manifest.files()[0].hash.as_ref().unwrap();
    for i in 1..4 {
        assert_eq!(result.manifest.files()[i].hash.as_ref().unwrap(), a_hash);
    }
    let b_hash = result.manifest.files()[4].hash.as_ref().unwrap();
    assert_ne!(a_hash, b_hash);

    let counting = dc.as_any().downcast_ref::<SlowPutDataCache>().unwrap();
    assert_eq!(
        counting.total_puts(),
        3,
        "expected exactly 3 put_object calls (one per unique hash), got {}",
        counting.total_puts()
    );
}
