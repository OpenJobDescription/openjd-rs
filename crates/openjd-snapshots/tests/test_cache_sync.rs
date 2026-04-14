// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.

//! Integration tests for cache_sync_manifest across all cache type combinations.

use aws_sdk_s3::config::{Credentials, Region};
use openjd_snapshots::{
    cache_sync_manifest, AsyncDataCache, CacheSyncOptions, FileEntry, FileSystemDataCache,
    HashAlgorithm, Manifest, ManifestRef, RelManifest, S3DataCache,
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
