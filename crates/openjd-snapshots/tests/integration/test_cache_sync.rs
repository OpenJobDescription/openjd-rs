// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Integration tests for cache_sync_manifest across all cache type combinations.

use aws_sdk_s3::config::{Credentials, Region};
use openjd_snapshots::{
    cache_sync_manifest, AsyncDataCache, CacheSyncOptions, FileEntry, FileSystemDataCache,
    HashAlgorithm, Manifest, ManifestRef, RelManifest, S3CopyConfig, S3DataCache,
};
use s3s::auth::SimpleAuth;
use s3s::service::S3ServiceBuilder;
use s3s_fs::FileSystem;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

const BUCKET: &str = "test-bucket";
const PREFIX: &str = "Data";
const ACCESS_KEY: &str = "AKIAIOSFODNN7EXAMPLE";
const SECRET_KEY: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";

fn test_manifest(files: Vec<FileEntry>) -> RelManifest {
    RelManifest::Snapshot(Manifest::new(HashAlgorithm::Xxh128, -1).with_files(files))
}

fn make_s3_client(tmp: &Path) -> aws_sdk_s3::Client {
    let fs_root = tmp.join("s3");
    std::fs::create_dir_all(&fs_root).unwrap();
    let fs = FileSystem::new(&fs_root).unwrap();
    let service = {
        let mut b = S3ServiceBuilder::new(fs);
        b.set_auth(SimpleAuth::from_single(ACCESS_KEY, SECRET_KEY));
        b.build()
    };
    let http_client = s3s_aws::Client::from(service);
    let cred = Credentials::new(ACCESS_KEY, SECRET_KEY, None, None, "test");
    let config = aws_sdk_s3::Config::builder()
        .behavior_version_latest()
        .credentials_provider(cred)
        .http_client(http_client)
        .region(Region::new("us-west-2"))
        .endpoint_url("http://localhost:0")
        .force_path_style(true)
        .build();
    aws_sdk_s3::Client::from_conf(config)
}

fn make_fs_cache() -> (TempDir, Arc<dyn AsyncDataCache>) {
    let tmp = TempDir::new().unwrap();
    let cache: Arc<dyn AsyncDataCache> =
        Arc::new(FileSystemDataCache::new(tmp.path().join("data")).unwrap());
    (tmp, cache)
}

async fn make_s3_cache() -> (TempDir, Arc<dyn AsyncDataCache>) {
    let tmp = TempDir::new().unwrap();
    let client = make_s3_client(tmp.path());
    client.create_bucket().bucket(BUCKET).send().await.unwrap();
    let cache: Arc<dyn AsyncDataCache> = Arc::new(S3DataCache::new(
        BUCKET.to_string(),
        PREFIX.to_string(),
        client,
    ));
    (tmp, cache)
}

async fn put(cache: &Arc<dyn AsyncDataCache>, hash: &str, alg: &str, data: &[u8]) {
    cache.put_object(hash, alg, data.to_vec()).await.unwrap();
}

async fn exists(cache: &Arc<dyn AsyncDataCache>, hash: &str, alg: &str) -> bool {
    cache.object_exists(hash, alg).await.unwrap_or(false)
}

async fn get(cache: &Arc<dyn AsyncDataCache>, hash: &str, alg: &str) -> Vec<u8> {
    cache.get_object(hash, alg).await.unwrap()
}

async fn run_sync_test(src: Arc<dyn AsyncDataCache>, dst: Arc<dyn AsyncDataCache>) {
    put(&src, "hash_a", "xxh128", b"alpha").await;
    put(&src, "hash_b", "xxh128", b"bravo").await;

    let manifest = test_manifest(vec![
        {
            let mut e = FileEntry::new("a.txt");
            e.hash = Some("hash_a".into());
            e.size = Some(5);
            e
        },
        {
            let mut e = FileEntry::new("b.txt");
            e.hash = Some("hash_b".into());
            e.size = Some(5);
            e
        },
    ]);

    let result = cache_sync_manifest(
        &[&manifest as &dyn ManifestRef],
        src,
        dst.clone(),
        CacheSyncOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(result.statistics.copied_objects, 2);
    assert_eq!(result.statistics.skipped_objects, 0);
    assert!(exists(&dst, "hash_a", "xxh128").await);
    assert_eq!(get(&dst, "hash_b", "xxh128").await, b"bravo");
}

async fn run_sync_skip_test(src: Arc<dyn AsyncDataCache>, dst: Arc<dyn AsyncDataCache>) {
    put(&src, "hash_a", "xxh128", b"alpha").await;
    put(&src, "hash_b", "xxh128", b"bravo").await;
    put(&dst, "hash_a", "xxh128", b"alpha").await; // pre-populate

    let manifest = test_manifest(vec![
        {
            let mut e = FileEntry::new("a.txt");
            e.hash = Some("hash_a".into());
            e.size = Some(5);
            e
        },
        {
            let mut e = FileEntry::new("b.txt");
            e.hash = Some("hash_b".into());
            e.size = Some(5);
            e
        },
    ]);

    let result = cache_sync_manifest(
        &[&manifest as &dyn ManifestRef],
        src,
        dst,
        CacheSyncOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(result.statistics.copied_objects, 1);
    assert_eq!(result.statistics.skipped_objects, 1);
}

// ===== FS → FS =====

#[tokio::test]
async fn cache_sync_fs_to_fs() {
    let (_sd, src) = make_fs_cache();
    let (_dd, dst) = make_fs_cache();
    run_sync_test(src, dst).await;
}

#[tokio::test]
async fn cache_sync_fs_to_fs_skip_existing() {
    let (_sd, src) = make_fs_cache();
    let (_dd, dst) = make_fs_cache();
    run_sync_skip_test(src, dst).await;
}

// ===== FS → S3 =====

#[tokio::test]
async fn cache_sync_fs_to_s3() {
    let (_sd, src) = make_fs_cache();
    let (_s3d, dst) = make_s3_cache().await;
    run_sync_test(src, dst).await;
}

#[tokio::test]
async fn cache_sync_fs_to_s3_skip_existing() {
    let (_sd, src) = make_fs_cache();
    let (_s3d, dst) = make_s3_cache().await;
    run_sync_skip_test(src, dst).await;
}

// ===== S3 → FS =====

#[tokio::test]
async fn cache_sync_s3_to_fs() {
    let (_s3d, src) = make_s3_cache().await;
    let (_dd, dst) = make_fs_cache();
    run_sync_test(src, dst).await;
}

#[tokio::test]
async fn cache_sync_s3_to_fs_skip_existing() {
    let (_s3d, src) = make_s3_cache().await;
    let (_dd, dst) = make_fs_cache();
    run_sync_skip_test(src, dst).await;
}

// ===== S3 → S3 =====

#[tokio::test]
async fn cache_sync_s3_to_s3() {
    let (_s3d, src) = make_s3_cache().await;
    let (_s3d2, dst) = make_s3_cache().await;
    run_sync_test(src, dst).await;
}

#[tokio::test]
async fn cache_sync_s3_to_s3_skip_existing() {
    let (_s3d, src) = make_s3_cache().await;
    let (_s3d2, dst) = make_s3_cache().await;
    run_sync_skip_test(src, dst).await;
}

/// End-to-end: cache_sync over an S3 destination that opted in to multipart
/// copy must route a >5 MiB object through server-side parallel UploadPartCopy.
/// Source and destination share one s3s-fs backend (two buckets) so the
/// server-side copy actually executes instead of falling back to get+put.
#[tokio::test(flavor = "multi_thread")]
async fn cache_sync_s3_to_s3_multipart_when_opted_in() {
    let tmp = TempDir::new().unwrap();
    let client = make_s3_client(tmp.path());
    for b in ["src-bucket", "dst-bucket"] {
        client.create_bucket().bucket(b).send().await.unwrap();
    }

    let src: Arc<dyn AsyncDataCache> = Arc::new(S3DataCache::new(
        "src-bucket".to_string(),
        PREFIX.to_string(),
        client.clone(),
    ));
    // Destination opts in to the benchmarked cross-region copy settings.
    let dst: Arc<dyn AsyncDataCache> = Arc::new(
        S3DataCache::new("dst-bucket".to_string(), PREFIX.to_string(), client)
            .with_copy_config(S3CopyConfig::default()),
    );

    // 6 MiB object => above the 5 MiB threshold => multipart (2 parts).
    let payload: Vec<u8> = (0..6 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    put(&src, "big6m", "xxh128", &payload).await;

    let manifest = test_manifest(vec![{
        let mut e = FileEntry::new("big.bin");
        e.hash = Some("big6m".into());
        e.size = Some(payload.len() as u64);
        e
    }]);

    let result = cache_sync_manifest(
        &[&manifest as &dyn ManifestRef],
        src,
        dst.clone(),
        CacheSyncOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(result.statistics.copied_objects, 1);
    assert_eq!(get(&dst, "big6m", "xxh128").await, payload);
}

// ===== Cancellation =====

#[tokio::test]
async fn cache_sync_cancellation_via_progress() {
    let (_sd, src) = make_fs_cache();
    let (_dd, dst) = make_fs_cache();

    // Put many objects so cancellation has a chance to fire
    for i in 0..20 {
        put(&src, &format!("hash_{i}"), "xxh128", b"data").await;
    }

    let manifest = test_manifest(
        (0..20)
            .map(|i| {
                let mut e = FileEntry::new(format!("f{i}.txt"));
                e.hash = Some(format!("hash_{i}"));
                e.size = Some(4);
                e
            })
            .collect(),
    );

    let call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let cc = call_count.clone();

    let result = cache_sync_manifest(
        &[&manifest as &dyn ManifestRef],
        src,
        dst,
        CacheSyncOptions {
            on_progress: Some(Box::new(move |_stats| {
                // Cancel after 2 progress callbacks
                cc.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 2
            })),
            ..Default::default()
        },
    )
    .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("cancelled"),
        "expected Cancelled, got: {err}"
    );
    assert!(
        call_count.load(std::sync::atomic::Ordering::Relaxed) >= 2,
        "progress should have been called at least twice"
    );
}

// ===== Progress callbacks =====

#[tokio::test]
async fn cache_sync_progress_reports_statistics() {
    let (_sd, src) = make_fs_cache();
    let (_dd, dst) = make_fs_cache();

    put(&src, "hash_a", "xxh128", b"alpha").await;
    put(&src, "hash_b", "xxh128", b"bravo").await;

    let manifest = test_manifest(vec![
        {
            let mut e = FileEntry::new("a.txt");
            e.hash = Some("hash_a".into());
            e.size = Some(5);
            e
        },
        {
            let mut e = FileEntry::new("b.txt");
            e.hash = Some("hash_b".into());
            e.size = Some(5);
            e
        },
    ]);

    let final_stats = Arc::new(std::sync::Mutex::new(None));
    let fs = final_stats.clone();

    let result = cache_sync_manifest(
        &[&manifest as &dyn ManifestRef],
        src,
        dst,
        CacheSyncOptions {
            on_progress: Some(Box::new(move |stats| {
                *fs.lock().unwrap() = Some(stats.clone());
                true
            })),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(result.statistics.total_objects, 2);
    assert_eq!(result.statistics.copied_objects, 2);

    // Progress callback should have been invoked with meaningful stats
    let last = final_stats.lock().unwrap().clone().unwrap();
    assert!(last.copied_objects > 0 || last.skipped_objects > 0);
}

// ===== Error: source object missing =====

#[tokio::test]
async fn cache_sync_missing_source_object_errors() {
    let (_sd, src) = make_fs_cache();
    let (_dd, dst) = make_fs_cache();

    // Don't put anything in source — manifest references a hash that doesn't exist
    let manifest = test_manifest(vec![{
        let mut e = FileEntry::new("missing.txt");
        e.hash = Some("nonexistent_hash".into());
        e.size = Some(100);
        e
    }]);

    let result = cache_sync_manifest(
        &[&manifest as &dyn ManifestRef],
        src,
        dst,
        CacheSyncOptions::default(),
    )
    .await;

    assert!(
        result.is_err(),
        "should error when source object is missing"
    );
}

// ===== Mixed manifest types =====

#[tokio::test]
async fn cache_sync_multiple_manifests_deduplicates() {
    let (_sd, src) = make_fs_cache();
    let (_dd, dst) = make_fs_cache();

    put(&src, "shared", "xxh128", b"shared_data").await;
    put(&src, "unique_a", "xxh128", b"aaa").await;
    put(&src, "unique_b", "xxh128", b"bbb").await;

    let manifest1 = test_manifest(vec![
        {
            let mut e = FileEntry::new("shared.txt");
            e.hash = Some("shared".into());
            e.size = Some(11);
            e
        },
        {
            let mut e = FileEntry::new("a.txt");
            e.hash = Some("unique_a".into());
            e.size = Some(3);
            e
        },
    ]);
    let manifest2 = test_manifest(vec![
        {
            let mut e = FileEntry::new("shared_copy.txt");
            e.hash = Some("shared".into()); // same hash as manifest1
            e.size = Some(11);
            e
        },
        {
            let mut e = FileEntry::new("b.txt");
            e.hash = Some("unique_b".into());
            e.size = Some(3);
            e
        },
    ]);

    let result = cache_sync_manifest(
        &[
            &manifest1 as &dyn ManifestRef,
            &manifest2 as &dyn ManifestRef,
        ],
        src,
        dst.clone(),
        CacheSyncOptions::default(),
    )
    .await
    .unwrap();

    // "shared" hash appears in both manifests but should only be copied once
    assert_eq!(result.statistics.copied_objects, 3); // shared + unique_a + unique_b
    assert!(exists(&dst, "shared", "xxh128").await);
    assert!(exists(&dst, "unique_a", "xxh128").await);
    assert!(exists(&dst, "unique_b", "xxh128").await);
}

// ===== Skips symlinks, deleted, unhashed =====

#[tokio::test]
async fn cache_sync_skips_non_syncable_entries() {
    let (_sd, src) = make_fs_cache();
    let (_dd, dst) = make_fs_cache();

    put(&src, "real_hash", "xxh128", b"real").await;

    let manifest = test_manifest(vec![
        {
            // Hashed file — should sync
            let mut e = FileEntry::new("real.txt");
            e.hash = Some("real_hash".into());
            e.size = Some(4);
            e
        },
        // Symlink — should skip
        FileEntry::symlink("link.txt", "real.txt"),
        // Unhashed file — should skip
        FileEntry::file("unhashed.txt", 100, 1),
    ]);

    let result = cache_sync_manifest(
        &[&manifest as &dyn ManifestRef],
        src,
        dst.clone(),
        CacheSyncOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(result.statistics.total_objects, 1); // only the hashed file
    assert_eq!(result.statistics.copied_objects, 1);
    assert!(exists(&dst, "real_hash", "xxh128").await);
}

// ===== Chunk hashes =====

#[tokio::test]
async fn cache_sync_handles_chunk_hashes() {
    let (_sd, src) = make_fs_cache();
    let (_dd, dst) = make_fs_cache();

    put(&src, "chunk_0", "xxh128", b"part0").await;
    put(&src, "chunk_1", "xxh128", b"part1").await;
    put(&src, "chunk_2", "xxh128", b"part2").await;

    let manifest = test_manifest(vec![{
        let mut e = FileEntry::file("big.bin", 30, 1);
        e.chunk_hashes = Some(vec!["chunk_0".into(), "chunk_1".into(), "chunk_2".into()]);
        e
    }]);

    let result = cache_sync_manifest(
        &[&manifest as &dyn ManifestRef],
        src,
        dst.clone(),
        CacheSyncOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(result.statistics.copied_objects, 3); // one per chunk
    assert!(exists(&dst, "chunk_0", "xxh128").await);
    assert!(exists(&dst, "chunk_1", "xxh128").await);
    assert!(exists(&dst, "chunk_2", "xxh128").await);
}

// ===== Empty manifest =====

#[tokio::test]
async fn cache_sync_empty_manifest() {
    let (_sd, src) = make_fs_cache();
    let (_dd, dst) = make_fs_cache();

    let manifest = test_manifest(vec![]);

    let result = cache_sync_manifest(
        &[&manifest as &dyn ManifestRef],
        src,
        dst,
        CacheSyncOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(result.statistics.total_objects, 0);
    assert_eq!(result.statistics.copied_objects, 0);
    assert_eq!(result.statistics.skipped_objects, 0);
}

// ===== Same source and destination =====

#[tokio::test]
async fn cache_sync_same_cache_is_noop() {
    let (_sd, cache) = make_fs_cache();

    put(&cache, "hash_a", "xxh128", b"alpha").await;

    let manifest = test_manifest(vec![{
        let mut e = FileEntry::new("a.txt");
        e.hash = Some("hash_a".into());
        e.size = Some(5);
        e
    }]);

    let result = cache_sync_manifest(
        &[&manifest as &dyn ManifestRef],
        cache.clone(),
        cache,
        CacheSyncOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(result.statistics.skipped_objects, 1);
    assert_eq!(result.statistics.copied_objects, 0);
}
