// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

/// S3 integration tests for S3DataCache.
///
/// These tests require a real S3 bucket and AWS credentials.
/// They are `#[ignore]`d so they don't run in normal `cargo test`.
///
/// Required environment variables:
///   OPENJD_TEST_S3_BUCKET  – S3 bucket name (tests skip if unset)
///
/// Optional environment variables:
///   OPENJD_TEST_S3_PREFIX  – key prefix (default: "openjd-snapshots-test")
///   AWS_REGION             – AWS region  (default: "us-west-2")
///
/// AWS credentials are resolved via the default credential chain
/// (env vars, ~/.aws/credentials, IMDS, etc.).
///
/// Run with:
///   RUSTUP_TOOLCHAIN=stable OPENJD_TEST_S3_BUCKET=my-bucket \
///     cargo test -p openjd-snapshots --test integration -- --ignored test_s3_integration::
use openjd_snapshots::{
    cache_sync_manifest, collect_abs_snapshot, download_abs_manifest, hash_upload_abs_manifest,
    join_snapshot, subtree_snapshot, AbsManifest, AsyncDataCache, CacheSyncOptions, CollectOptions,
    DownloadOptions, FileEntry, HashAlgorithm, HashUploadOptions, Manifest, ManifestRef,
    RelManifest, S3DataCache, SymlinkPolicy,
};
use std::sync::Arc;
use tempfile::TempDir;

struct S3TestConfig {
    bucket: String,
    prefix: String,
    region: String,
}

fn s3_test_config() -> Option<S3TestConfig> {
    Some(S3TestConfig {
        bucket: std::env::var("OPENJD_TEST_S3_BUCKET").ok()?,
        prefix: std::env::var("OPENJD_TEST_S3_PREFIX")
            .unwrap_or_else(|_| "openjd-snapshots-test".into()),
        region: std::env::var("AWS_REGION").unwrap_or_else(|_| "us-west-2".into()),
    })
}

/// Create an S3 client from within an async context.
async fn make_s3_client_async(region: &str) -> aws_sdk_s3::Client {
    use aws_sdk_s3::config::Region;
    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(Region::new(region.to_string()))
        .load()
        .await;
    aws_sdk_s3::Client::new(&config)
}

fn test_prefix(base: &str) -> String {
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{base}/test-snapshots/{id}/Data")
}

async fn cleanup_prefix(client: &aws_sdk_s3::Client, bucket: &str, prefix: &str) {
    let mut continuation_token: Option<String> = None;
    loop {
        let mut req = client.list_objects_v2().bucket(bucket).prefix(prefix);
        if let Some(token) = continuation_token.take() {
            req = req.continuation_token(token);
        }
        let resp = req.send().await.unwrap();
        for obj in resp.contents() {
            if let Some(key) = obj.key() {
                let _ = client.delete_object().bucket(bucket).key(key).send().await;
            }
        }
        if resp.is_truncated() == Some(true) {
            continuation_token = resp.next_continuation_token().map(|s| s.to_string());
        } else {
            break;
        }
    }
}

#[tokio::test]
#[ignore] // Requires AWS credentials and S3 bucket access
async fn s3_round_trip_collect_upload_download() {
    let Some(config) = s3_test_config() else {
        eprintln!("Skipping: OPENJD_TEST_S3_BUCKET not set");
        return;
    };
    let s3_client = make_s3_client_async(&config.region).await;
    let prefix = test_prefix(&config.prefix);

    let src_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();

    // Create test files
    std::fs::create_dir_all(src_dir.path().join("sub")).unwrap();
    std::fs::write(src_dir.path().join("hello.txt"), b"hello world").unwrap();
    std::fs::write(src_dir.path().join("sub/data.bin"), b"binary data").unwrap();

    let data_cache = Arc::new(S3DataCache::new(
        config.bucket.clone(),
        prefix.clone(),
        s3_client.clone(),
    ));

    // COLLECT
    let snapshot = collect_abs_snapshot(
        &[src_dir.path().to_path_buf()],
        &[] as &[std::path::PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    // HASH_UPLOAD to S3
    let upload_result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(snapshot),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        HashUploadOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(upload_result.statistics.uploaded_files, 2);

    let abs_snap = match upload_result.manifest {
        AbsManifest::Snapshot(s) => s,
        _ => panic!("expected snapshot"),
    };

    // Verify objects exist in S3
    for f in &abs_snap.files {
        if let Some(ref hash) = f.hash {
            assert!(
                openjd_snapshots::AsyncDataCache::object_exists(&*data_cache, hash, "xxh128")
                    .await
                    .unwrap(),
                "object should exist in S3: {hash}"
            );
        }
    }

    // Remap to download dir
    let rel = subtree_snapshot(
        &abs_snap,
        &src_dir.path().to_string_lossy(),
        SymlinkPolicy::ExcludeAll,
    )
    .unwrap();
    let abs_dl = join_snapshot(&rel, &dst_dir.path().to_string_lossy()).unwrap();

    // DOWNLOAD from S3
    let dl_result = download_abs_manifest(
        &AbsManifest::Snapshot(abs_dl),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(dl_result.statistics.downloaded_files, 2);

    // Verify downloaded files match originals
    assert_eq!(
        std::fs::read_to_string(dst_dir.path().join("hello.txt")).unwrap(),
        "hello world"
    );
    assert_eq!(
        std::fs::read(dst_dir.path().join("sub/data.bin")).unwrap(),
        b"binary data"
    );

    // Cleanup
    cleanup_prefix(&s3_client, &config.bucket, &prefix).await;
}

#[tokio::test]
#[ignore] // Requires AWS credentials and S3 bucket access
async fn s3_cache_sync_server_side_copy() {
    let Some(config) = s3_test_config() else {
        eprintln!("Skipping: OPENJD_TEST_S3_BUCKET not set");
        return;
    };
    let s3_client = make_s3_client_async(&config.region).await;
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let src_prefix = format!("{}/test-cache-sync/{id}/src/Data", config.prefix);
    let dst_prefix = format!("{}/test-cache-sync/{id}/dst/Data", config.prefix);

    let src = Arc::new(S3DataCache::new(
        config.bucket.clone(),
        src_prefix,
        s3_client.clone(),
    ));
    let dst = Arc::new(S3DataCache::new(
        config.bucket.clone(),
        dst_prefix,
        s3_client.clone(),
    ));

    // Put test data in source
    src.put_object("hash_a", "xxh128", b"alpha".to_vec())
        .await
        .unwrap();
    src.put_object("hash_b", "xxh128", b"bravo".to_vec())
        .await
        .unwrap();

    // Build manifest referencing those objects
    let manifest =
        RelManifest::Snapshot(Manifest::new(HashAlgorithm::Xxh128, -1).with_files(vec![
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
        ]));

    // First sync: should copy both objects
    let result = cache_sync_manifest(
        &[&manifest as &dyn ManifestRef],
        src.clone() as Arc<dyn AsyncDataCache>,
        dst.clone() as Arc<dyn AsyncDataCache>,
        CacheSyncOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(result.statistics.copied_objects, 2);
    assert_eq!(result.statistics.skipped_objects, 0);
    assert!(dst.object_exists("hash_a", "xxh128").await.unwrap());
    assert_eq!(dst.get_object("hash_b", "xxh128").await.unwrap(), b"bravo");

    // Second sync: should skip all (already exist in destination)
    let result2 = cache_sync_manifest(
        &[&manifest as &dyn ManifestRef],
        src.clone() as Arc<dyn AsyncDataCache>,
        dst.clone() as Arc<dyn AsyncDataCache>,
        CacheSyncOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(result2.statistics.skipped_objects, 2);
    assert_eq!(result2.statistics.copied_objects, 0);

    // Cleanup
    let cleanup_prefix_str = format!("{}/test-cache-sync/{id}", config.prefix);
    cleanup_prefix(&s3_client, &config.bucket, &cleanup_prefix_str).await;
}
