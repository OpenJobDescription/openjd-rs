// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for S3DataCache using s3s + s3s-fs for in-process S3 emulation.

use aws_sdk_s3::config::{Credentials, Region};
use openjd_snapshots::hash::hash_data;
use openjd_snapshots::ContentAddressedDataCache as SyncCache;
use openjd_snapshots::{
    collect_abs_snapshot, download_abs_manifest, hash_upload_abs_manifest, join_snapshot,
    subtree_snapshot, AbsManifest, AsyncDataCache, CollectOptions, CopyResult, DirEntry,
    DownloadOptions, FileConflictResolution, FileEntry, HashAlgorithm, HashUploadOptions, Manifest,
    MultipartDataCache, RangeReadDataCache, S3DataCache, SymlinkPolicy, DEFAULT_FILE_CHUNK_SIZE,
};
use s3s::auth::SimpleAuth;
use s3s::service::S3ServiceBuilder;
use s3s_fs::FileSystem;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use tempfile::TempDir;

const BUCKET: &str = "test-bucket";
const PREFIX: &str = "Data";
const ACCESS_KEY: &str = "AKIAIOSFODNN7EXAMPLE";
const SECRET_KEY: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";

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

async fn make_s3_fixture() -> (TempDir, Arc<S3DataCache>) {
    let tmp = TempDir::new().unwrap();
    let client = make_s3_client(tmp.path());
    client.create_bucket().bucket(BUCKET).send().await.unwrap();
    let data_cache = Arc::new(S3DataCache::new(
        BUCKET.to_string(),
        PREFIX.to_string(),
        client,
    ));
    (tmp, data_cache)
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
        std::path::absolute(&p)
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        content.len() as u64,
        mtime,
    )
}

fn upload_to_s3(cache: &dyn openjd_snapshots::ContentAddressedDataCache, content: &[u8]) -> String {
    let hash = hash_data(content);
    cache.put_object(&hash, "xxh128", content).unwrap();
    hash
}

type AbsSnapshot = Manifest<openjd_snapshots::manifest::Abs, openjd_snapshots::manifest::Full>;
type AbsSnapshotDiff = Manifest<openjd_snapshots::manifest::Abs, openjd_snapshots::manifest::Diff>;

// ===== Category 1: S3DataCache Basic Operations =====

#[tokio::test(flavor = "multi_thread")]
async fn s3_put_and_get_object() {
    let (_tmp, cache) = make_s3_fixture().await;
    openjd_snapshots::ContentAddressedDataCache::put_object(
        &*cache,
        "abc123",
        "xxh128",
        b"hello world",
    )
    .unwrap();
    let data = openjd_snapshots::ContentAddressedDataCache::get_object(&*cache, "abc123", "xxh128")
        .unwrap();
    assert_eq!(data, b"hello world");
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_object_exists_after_put() {
    let (_tmp, cache) = make_s3_fixture().await;
    // Note: s3s-fs returns NoSuchKey for missing objects on HeadObject,
    // but the AWS SDK expects NotFound. So object_exists returns Err
    // for missing objects instead of Ok(false). We test the positive case.
    SyncCache::put_object(&*cache, "abc123", "xxh128", b"data").unwrap();
    assert!(SyncCache::object_exists(&*cache, "abc123", "xxh128").unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_object_key_format() {
    let (_tmp, cache) = make_s3_fixture().await;
    let key = SyncCache::object_key(&*cache, "abc123", "xxh128");
    assert_eq!(key, "Data/abc123.xxh128");
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_get_nonexistent_returns_error() {
    let (_tmp, cache) = make_s3_fixture().await;
    assert!(SyncCache::get_object(&*cache, "missing", "xxh128").is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_object_exists_nonexistent_returns_error() {
    // s3s-fs returns NoSuchKey (not NotFound) for HeadObject on missing keys,
    // which the SDK doesn't map to is_not_found(). This causes object_exists
    // to return Err instead of Ok(false) when used with s3s.
    let (_tmp, cache) = make_s3_fixture().await;
    assert!(SyncCache::object_exists(&*cache, "nonexistent", "xxh128").is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_copy_from_between_buckets() {
    let tmp = TempDir::new().unwrap();
    let client = make_s3_client(tmp.path());
    client
        .create_bucket()
        .bucket("src-bucket")
        .send()
        .await
        .unwrap();
    client
        .create_bucket()
        .bucket("dst-bucket")
        .send()
        .await
        .unwrap();

    let src = Arc::new(S3DataCache::new(
        "src-bucket".to_string(),
        PREFIX.to_string(),
        client.clone(),
    ));
    let dst = Arc::new(S3DataCache::new(
        "dst-bucket".to_string(),
        PREFIX.to_string(),
        client,
    ));

    // Put object in source
    SyncCache::put_object(&*src, "abc123", "xxh128", b"copy me").unwrap();

    // Server-side copy
    let result = dst
        .copy_from(src.as_ref() as &dyn AsyncDataCache, "abc123", "xxh128")
        .await
        .unwrap();
    assert_eq!(result, CopyResult::ServerSideCopy);

    // Verify data arrived
    let data = SyncCache::get_object(&*dst, "abc123", "xxh128").unwrap();
    assert_eq!(data, b"copy me");
}

// ===== Category 2: Hash+Upload with S3 =====

#[tokio::test(flavor = "multi_thread")]
async fn s3_hash_upload_single_file() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "test.txt", b"hello world");

    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(vec![FileEntry::file(&path, size, mtime)]);

    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        HashUploadOptions::default(),
    )
    .await
    .unwrap();

    let hash = result.manifest.files()[0].hash.as_ref().unwrap();
    assert!(!hash.is_empty());
    assert!(SyncCache::object_exists(&*data_cache, hash, "xxh128").unwrap());
    let stored = SyncCache::get_object(&*data_cache, hash, "xxh128").unwrap();
    assert_eq!(stored, b"hello world");
    assert_eq!(result.statistics.uploaded_files, 1);
    assert_eq!(result.statistics.uploaded_bytes, 11);
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_hash_upload_duplicate_content_stored_once() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;
    let tmp = TempDir::new().unwrap();
    let (path1, size1, mtime1) = make_test_file(tmp.path(), "file1.txt", b"same content");
    let (path2, size2, mtime2) = make_test_file(tmp.path(), "file2.txt", b"same content");

    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(vec![
            FileEntry::file(&path1, size1, mtime1),
            FileEntry::file(&path2, size2, mtime2),
        ]);

    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        HashUploadOptions::default(),
    )
    .await
    .unwrap();

    let files = result.manifest.files();
    assert_eq!(files[0].hash, files[1].hash);
    assert_eq!(
        result.statistics.uploaded_files + result.statistics.skipped_files,
        2
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_hash_upload_second_upload_skips() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "test.txt", b"content");

    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(vec![FileEntry::file(&path, size, mtime)]);

    let r1 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest.clone()),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        HashUploadOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(r1.statistics.uploaded_files, 1);

    // Clear hashes so we can re-upload
    let mut manifest2 = manifest;
    manifest2.clear_hashes();

    let r2 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest2),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        HashUploadOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(r2.statistics.uploaded_files, 0);
    assert_eq!(r2.statistics.skipped_files, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_hash_upload_symlinks_and_deleted_pass_through() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "real.txt", b"data");

    let manifest: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(vec![
            FileEntry::file(&path, size, mtime),
            FileEntry::symlink("/tmp/link", "/tmp/target"),
            FileEntry::deleted("/tmp/gone"),
        ]);

    let result = hash_upload_abs_manifest(
        &AbsManifest::Diff(manifest),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        HashUploadOptions::default(),
    )
    .await
    .unwrap();

    let files = result.manifest.files();
    assert!(files[0].hash.is_some());
    assert!(files[1].hash.is_none());
    assert!(files[2].hash.is_none());
    assert_eq!(result.statistics.total_files, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_hash_upload_empty_manifest() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;

    let manifest: AbsSnapshot =
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![]);

    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        HashUploadOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(result.statistics.total_files, 0);
}

// ===== Category 3: Download with S3 =====

#[tokio::test(flavor = "multi_thread")]
async fn s3_download_single_file() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;
    let tmp = TempDir::new().unwrap();

    let hash = upload_to_s3(&*data_cache, b"hello world");
    let dest = tmp.path().join("output.txt");

    let mut entry = FileEntry::file(dest.to_string_lossy().to_string(), 11, 1000);
    entry.hash = Some(hash);

    let manifest: AbsSnapshot =
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![entry]);

    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "hello world");
    assert_eq!(result.statistics.downloaded_files, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_download_creates_parent_dirs() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;
    let tmp = TempDir::new().unwrap();

    let hash = upload_to_s3(&*data_cache, b"nested");
    let dest = tmp.path().join("a/b/c/file.txt");

    let mut entry = FileEntry::file(dest.to_string_lossy().to_string(), 6, 1000);
    entry.hash = Some(hash);

    let manifest: AbsSnapshot =
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![entry]);

    download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "nested");
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_download_chunked_file() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;
    let tmp = TempDir::new().unwrap();

    let chunks: Vec<&[u8]> = vec![b"aaa", b"bbb", b"ccc", b"ddd"];
    let chunk_hashes: Vec<String> = chunks
        .iter()
        .map(|c| upload_to_s3(&*data_cache, c))
        .collect();

    let dest = tmp.path().join("chunked.bin");
    let mut entry = FileEntry::file(dest.to_string_lossy().to_string(), 12, 1000);
    entry.chunk_hashes = Some(chunk_hashes);

    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, 3).with_files(vec![entry]);

    download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(std::fs::read(&dest).unwrap(), b"aaabbbcccddd");
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_download_skip_conflict() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;
    let tmp = TempDir::new().unwrap();

    let hash = upload_to_s3(&*data_cache, b"new content");
    let dest = tmp.path().join("existing.txt");
    std::fs::write(&dest, b"old content").unwrap();

    let mut entry = FileEntry::file(dest.to_string_lossy().to_string(), 11, 1000);
    entry.hash = Some(hash);

    let manifest: AbsSnapshot =
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![entry]);

    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions {
            file_conflict_resolution: FileConflictResolution::Skip,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old content");
    assert_eq!(result.statistics.skipped_files, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_download_overwrite_conflict() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;
    let tmp = TempDir::new().unwrap();

    let hash = upload_to_s3(&*data_cache, b"new content");
    let dest = tmp.path().join("existing.txt");
    std::fs::write(&dest, b"old content").unwrap();

    let mut entry = FileEntry::file(dest.to_string_lossy().to_string(), 11, 1000);
    entry.hash = Some(hash);

    let manifest: AbsSnapshot =
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![entry]);

    download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "new content");
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_download_create_copy_conflict() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;
    let tmp = TempDir::new().unwrap();

    let hash = upload_to_s3(&*data_cache, b"new");
    let dest = tmp.path().join("file.txt");
    std::fs::write(&dest, b"old").unwrap();

    let mut entry = FileEntry::file(dest.to_string_lossy().to_string(), 3, 1000);
    entry.hash = Some(hash);

    let manifest: AbsSnapshot =
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![entry]);

    download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions {
            file_conflict_resolution: FileConflictResolution::CreateCopy,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old");
    let copy = tmp.path().join("file (1).txt");
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), "new");
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_download_applies_deletes() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;
    let tmp = TempDir::new().unwrap();

    let file_to_delete = tmp.path().join("gone.txt");
    std::fs::write(&file_to_delete, b"bye").unwrap();

    let manifest: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(vec![FileEntry::deleted(
            file_to_delete.to_string_lossy().to_string(),
        )]);

    download_abs_manifest(
        &AbsManifest::Diff(manifest),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions::default(),
    )
    .await
    .unwrap();

    assert!(!file_to_delete.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_download_creates_manifest_dirs() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;
    let tmp = TempDir::new().unwrap();

    let dir_path = tmp.path().join("new_dir");
    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_dirs(vec![DirEntry::new(dir_path.to_string_lossy().to_string())]);

    download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions::default(),
    )
    .await
    .unwrap();

    assert!(dir_path.is_dir());
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_round_trip_collect_upload_download() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;
    let src = TempDir::new().unwrap();
    let dst = TempDir::new().unwrap();

    std::fs::create_dir_all(src.path().join("sub")).unwrap();
    std::fs::write(src.path().join("hello.txt"), b"hello world").unwrap();
    std::fs::write(src.path().join("sub/data.bin"), b"binary data").unwrap();

    let snapshot = collect_abs_snapshot(
        &[src.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let upload_result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(snapshot),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        HashUploadOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(upload_result.statistics.uploaded_files, 2);

    let abs_snap = match upload_result.manifest {
        AbsManifest::Snapshot(ref s) => s,
        _ => panic!("expected snapshot"),
    };

    let rel = subtree_snapshot(
        abs_snap,
        &src.path().to_string_lossy(),
        SymlinkPolicy::ExcludeAll,
    )
    .unwrap();
    let abs_dl = join_snapshot(&rel, &dst.path().to_string_lossy()).unwrap();

    let dl_result = download_abs_manifest(
        &AbsManifest::Snapshot(abs_dl),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(dl_result.statistics.downloaded_files, 2);

    assert_eq!(
        std::fs::read_to_string(dst.path().join("hello.txt")).unwrap(),
        "hello world"
    );
    assert_eq!(
        std::fs::read(dst.path().join("sub/data.bin")).unwrap(),
        b"binary data"
    );
}

// ===== Category 4: AsyncDataCache Operations =====

#[tokio::test(flavor = "multi_thread")]
#[ignore] // s3s-fs does not fully support CompleteMultipartUpload; works against real S3
async fn s3_multipart_upload_round_trip() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;

    use openjd_snapshots::{AsyncDataCache, MultipartDataCache};

    let hash = "multipart_test_hash";
    let alg = "xxh128";

    // Create multipart upload
    let upload_id = data_cache.create_multipart_upload(hash, alg).await.unwrap();

    // Upload 3 parts
    let part1 = data_cache
        .upload_part(hash, alg, &upload_id, 1, b"part1data".to_vec())
        .await
        .unwrap();
    let part2 = data_cache
        .upload_part(hash, alg, &upload_id, 2, b"part2data".to_vec())
        .await
        .unwrap();
    let part3 = data_cache
        .upload_part(hash, alg, &upload_id, 3, b"part3data".to_vec())
        .await
        .unwrap();

    // Complete
    data_cache
        .complete_multipart_upload(
            hash,
            alg,
            &upload_id,
            vec![(1, part1), (2, part2), (3, part3)],
        )
        .await
        .unwrap();

    // Verify content
    let data = AsyncDataCache::get_object(&*data_cache, hash, alg)
        .await
        .unwrap();
    assert_eq!(data, b"part1datapart2datapart3data");
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_get_object_range() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;

    use openjd_snapshots::{AsyncDataCache, RangeReadDataCache};

    // Upload a file
    let content = b"hello world, this is range test data";
    AsyncDataCache::put_object(&*data_cache, "range_hash", "xxh128", content.to_vec())
        .await
        .unwrap();

    // Get a range (bytes 6-10 inclusive = "world")
    let range_data = data_cache
        .get_object_range("range_hash", "xxh128", 6, 10)
        .await
        .unwrap();
    assert_eq!(range_data, b"world");
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_abort_multipart_upload() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;

    use openjd_snapshots::{AsyncDataCache, MultipartDataCache};

    let hash = "abort_test_hash";
    let alg = "xxh128";

    let upload_id = data_cache.create_multipart_upload(hash, alg).await.unwrap();
    data_cache
        .upload_part(hash, alg, &upload_id, 1, b"data".to_vec())
        .await
        .unwrap();

    // Abort instead of complete
    data_cache
        .abort_multipart_upload(hash, alg, &upload_id)
        .await
        .unwrap();

    // Object should not exist
    assert!(!AsyncDataCache::object_exists(&*data_cache, hash, alg)
        .await
        .unwrap_or(false));
}

// ===== Category 5: S3CheckCache Integration =====

#[tokio::test(flavor = "multi_thread")]
async fn s3_check_cache_hit_skips_head_object() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;
    let tmp = TempDir::new().unwrap();
    let check_cache = Arc::new(openjd_snapshots::S3CheckCache::new(tmp.path()).unwrap());

    // Pre-populate the check cache so it thinks the object exists
    let cache_key = data_cache.cache_key("abc123", "xxh128");
    check_cache.put_entry(&cache_key).unwrap();

    // Attach the check cache to the data cache
    let data_cache_with_check = S3DataCache::new(
        BUCKET.to_string(),
        PREFIX.to_string(),
        data_cache.client().clone(),
    )
    .with_s3_check_cache(Some(check_cache));

    // check_cache_exists should return true without any S3 call
    assert!(data_cache_with_check.check_cache_exists("abc123", "xxh128"));
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_check_cache_miss_calls_head_object() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;
    let tmp = TempDir::new().unwrap();
    let check_cache = Arc::new(openjd_snapshots::S3CheckCache::new(tmp.path()).unwrap());

    let data_cache_with_check = S3DataCache::new(
        BUCKET.to_string(),
        PREFIX.to_string(),
        data_cache.client().clone(),
    )
    .with_s3_check_cache(Some(check_cache));

    // No entry in check cache — check_cache_exists returns false
    assert!(!data_cache_with_check.check_cache_exists("missing", "xxh128"));

    // Upload an object, then call object_exists which should do HeadObject and populate cache
    SyncCache::put_object(&*data_cache, "missing", "xxh128", b"data").unwrap();
    assert!(SyncCache::object_exists(&data_cache_with_check, "missing", "xxh128").unwrap());

    // Now the check cache should have the entry
    assert!(data_cache_with_check.check_cache_exists("missing", "xxh128"));
}

// ===== Category 6: Multipart Download =====

/// Small part size so multipart kicks in for small test files.
const SMALL_PART_SIZE: usize = 32;

async fn make_s3_fixture_small_parts() -> (TempDir, Arc<S3DataCache>) {
    let tmp = TempDir::new().unwrap();
    let client = make_s3_client(tmp.path());
    client.create_bucket().bucket(BUCKET).send().await.unwrap();
    let data_cache = S3DataCache::new(BUCKET.to_string(), PREFIX.to_string(), client)
        .with_multipart_part_size(SMALL_PART_SIZE);
    (tmp, Arc::new(data_cache))
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_multipart_download_large_file() {
    let (_s3_tmp, data_cache) = make_s3_fixture_small_parts().await;
    let tmp = TempDir::new().unwrap();

    // File size >= 2 * SMALL_PART_SIZE (64 bytes) triggers multipart
    let content: Vec<u8> = (0..100u8).collect();
    let hash = upload_to_s3(&*data_cache, &content);
    let dest = tmp.path().join("large.bin");

    let mut entry = FileEntry::file(
        dest.to_string_lossy().to_string(),
        content.len() as u64,
        1000,
    );
    entry.hash = Some(hash);

    let manifest: AbsSnapshot =
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![entry]);

    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(std::fs::read(&dest).unwrap(), content);
    assert_eq!(result.statistics.downloaded_files, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_multipart_download_small_file() {
    let (_s3_tmp, data_cache) = make_s3_fixture_small_parts().await;
    let tmp = TempDir::new().unwrap();

    // File size < 2 * SMALL_PART_SIZE (64 bytes) — single request path
    let content = b"short";
    let hash = upload_to_s3(&*data_cache, content);
    let dest = tmp.path().join("small.bin");

    let mut entry = FileEntry::file(
        dest.to_string_lossy().to_string(),
        content.len() as u64,
        1000,
    );
    entry.hash = Some(hash);

    let manifest: AbsSnapshot =
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![entry]);

    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(std::fs::read(&dest).unwrap(), content);
    assert_eq!(result.statistics.downloaded_files, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_multipart_download_chunked_file() {
    let (_s3_tmp, data_cache) = make_s3_fixture_small_parts().await;
    let tmp = TempDir::new().unwrap();

    // Each chunk is large enough to trigger multipart on its own if it were a file,
    // but chunked files go through the chunk-reassembly path (not multipart).
    // This verifies chunked download still works with a small part size.
    let chunk_size = SMALL_PART_SIZE * 3; // 96 bytes per chunk
    let chunks: Vec<Vec<u8>> = (0..3u8).map(|i| vec![i; chunk_size]).collect();
    let chunk_hashes: Vec<String> = chunks
        .iter()
        .map(|c| upload_to_s3(&*data_cache, c))
        .collect();

    let total_size = chunks.iter().map(|c| c.len()).sum::<usize>();
    let dest = tmp.path().join("chunked_large.bin");
    let mut entry = FileEntry::file(dest.to_string_lossy().to_string(), total_size as u64, 1000);
    entry.chunk_hashes = Some(chunk_hashes);

    let manifest: AbsSnapshot =
        Manifest::new(HashAlgorithm::Xxh128, chunk_size as i64).with_files(vec![entry]);

    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions::default(),
    )
    .await
    .unwrap();

    let expected: Vec<u8> = chunks.into_iter().flatten().collect();
    assert_eq!(std::fs::read(&dest).unwrap(), expected);
    assert_eq!(result.statistics.downloaded_files, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_multipart_download_mixed_sizes() {
    let (_s3_tmp, data_cache) = make_s3_fixture_small_parts().await;
    let tmp = TempDir::new().unwrap();

    // Large file (>= 2 * 32 = 64 bytes) — multipart path
    let large_content: Vec<u8> = (0..100u8).collect();
    let large_hash = upload_to_s3(&*data_cache, &large_content);

    // Small file (< 64 bytes) — single request path
    let small_content = b"tiny";
    let small_hash = upload_to_s3(&*data_cache, small_content);

    let large_dest = tmp.path().join("large.bin");
    let small_dest = tmp.path().join("small.bin");

    let mut large_entry = FileEntry::file(
        large_dest.to_string_lossy().to_string(),
        large_content.len() as u64,
        1000,
    );
    large_entry.hash = Some(large_hash);

    let mut small_entry = FileEntry::file(
        small_dest.to_string_lossy().to_string(),
        small_content.len() as u64,
        1000,
    );
    small_entry.hash = Some(small_hash);

    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(vec![large_entry, small_entry]);

    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(std::fs::read(&large_dest).unwrap(), large_content);
    assert_eq!(std::fs::read(&small_dest).unwrap(), small_content);
    assert_eq!(result.statistics.downloaded_files, 2);
}

// ===== Category 6: ExpectedBucketOwner (Account ID) =====

/// Helper: create S3DataCache with expected_bucket_owner set.
async fn make_s3_fixture_with_expected_bucket_owner(owner: &str) -> (TempDir, Arc<S3DataCache>) {
    let tmp = TempDir::new().unwrap();
    let client = make_s3_client(tmp.path());
    client.create_bucket().bucket(BUCKET).send().await.unwrap();

    let cache = S3DataCache::new(BUCKET.to_string(), PREFIX.to_string(), client)
        .with_expected_bucket_owner(Some(owner.to_string()));
    (tmp, Arc::new(cache))
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_data_cache_expected_bucket_owner_none() {
    // S3DataCache created without expected_bucket_owner — all operations succeed
    let (_tmp, cache) = make_s3_fixture().await;
    assert!(cache.expected_bucket_owner().is_none());

    SyncCache::put_object(&*cache, "abc", "xxh128", b"data").unwrap();
    assert!(SyncCache::object_exists(&*cache, "abc", "xxh128").unwrap());
    assert_eq!(
        SyncCache::get_object(&*cache, "abc", "xxh128").unwrap(),
        b"data"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_data_cache_expected_bucket_owner_set() {
    // S3DataCache with expected_bucket_owner — s3s doesn't validate the field,
    // so operations succeed. This confirms the field is passed through without error.
    let (_tmp, cache) = make_s3_fixture_with_expected_bucket_owner("123456789012").await;
    assert_eq!(cache.expected_bucket_owner(), Some("123456789012"));

    SyncCache::put_object(&*cache, "abc", "xxh128", b"data").unwrap();
    assert!(SyncCache::object_exists(&*cache, "abc", "xxh128").unwrap());
    assert_eq!(
        SyncCache::get_object(&*cache, "abc", "xxh128").unwrap(),
        b"data"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_data_cache_expected_bucket_owner_mismatch() {
    // s3s does not enforce ExpectedBucketOwner, so a mismatched value still succeeds.
    // This test documents that behavior and confirms the field is wired through.
    let (_tmp, cache) = make_s3_fixture_with_expected_bucket_owner("999999999999").await;

    SyncCache::put_object(&*cache, "abc", "xxh128", b"data").unwrap();
    assert!(SyncCache::object_exists(&*cache, "abc", "xxh128").unwrap());
    assert_eq!(
        SyncCache::get_object(&*cache, "abc", "xxh128").unwrap(),
        b"data"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_upload_with_expected_bucket_owner() {
    // Full hash_upload pipeline with expected_bucket_owner set
    let (_s3_tmp, cache) = make_s3_fixture_with_expected_bucket_owner("123456789012").await;
    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "test.txt", b"hello owner");

    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(vec![FileEntry::file(&path, size, mtime)]);

    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        cache.clone() as Arc<dyn AsyncDataCache>,
        HashUploadOptions::default(),
    )
    .await
    .unwrap();

    let hash = result.manifest.files()[0].hash.as_ref().unwrap();
    assert!(!hash.is_empty());
    assert_eq!(result.statistics.uploaded_files, 1);
    let stored = SyncCache::get_object(&*cache, hash, "xxh128").unwrap();
    assert_eq!(stored, b"hello owner");
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_download_with_expected_bucket_owner() {
    // Full download pipeline with expected_bucket_owner set
    let (_s3_tmp, cache) = make_s3_fixture_with_expected_bucket_owner("123456789012").await;
    let tmp = TempDir::new().unwrap();

    let hash = upload_to_s3(&*cache, b"owner download");
    let dest = tmp.path().join("output.txt");

    let mut entry = FileEntry::file(dest.to_string_lossy().to_string(), 14, 1000);
    entry.hash = Some(hash);

    let manifest: AbsSnapshot =
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![entry]);

    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "owner download");
    assert_eq!(result.statistics.downloaded_files, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_upload_excludes_expected_bucket_owner_when_disabled() {
    // When expected_bucket_owner is None, uploads succeed without the header.
    let (_s3_tmp, cache) = make_s3_fixture().await;
    assert!(cache.expected_bucket_owner().is_none());

    let tmp = TempDir::new().unwrap();
    let (path, size, mtime) = make_test_file(tmp.path(), "test.txt", b"no owner header");

    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(vec![FileEntry::file(&path, size, mtime)]);

    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        cache.clone() as Arc<dyn AsyncDataCache>,
        HashUploadOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(result.statistics.uploaded_files, 1);
    let hash = result.manifest.files()[0].hash.as_ref().unwrap();
    let stored = SyncCache::get_object(&*cache, hash, "xxh128").unwrap();
    assert_eq!(stored, b"no owner header");
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_download_excludes_expected_bucket_owner_when_disabled() {
    // When expected_bucket_owner is None, downloads succeed without the header.
    let (_s3_tmp, cache) = make_s3_fixture().await;
    assert!(cache.expected_bucket_owner().is_none());

    let tmp = TempDir::new().unwrap();
    let hash = upload_to_s3(&*cache, b"no owner download");
    let dest = tmp.path().join("output.txt");

    let mut entry = FileEntry::file(dest.to_string_lossy().to_string(), 16, 1000);
    entry.hash = Some(hash);

    let manifest: AbsSnapshot =
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![entry]);

    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "no owner download");
    assert_eq!(result.statistics.downloaded_files, 1);
}

// ===== FileSystemDataCache Async Tests =====

use openjd_snapshots::FileSystemDataCache;

fn make_fs_async_cache() -> (TempDir, Arc<dyn AsyncDataCache>) {
    let tmp = TempDir::new().unwrap();
    let cache: Arc<dyn AsyncDataCache> =
        Arc::new(FileSystemDataCache::new(tmp.path().join("cache")).unwrap());
    (tmp, cache)
}

#[tokio::test(flavor = "multi_thread")]
async fn fs_async_put_and_get_object() {
    let (_tmp, cache) = make_fs_async_cache();
    cache
        .put_object("abc123", "xxh128", b"hello world".to_vec())
        .await
        .unwrap();
    let data = cache.get_object("abc123", "xxh128").await.unwrap();
    assert_eq!(data, b"hello world");
}

#[tokio::test(flavor = "multi_thread")]
async fn fs_async_object_exists() {
    let (_tmp, cache) = make_fs_async_cache();
    assert!(!cache.object_exists("abc123", "xxh128").await.unwrap());
    cache
        .put_object("abc123", "xxh128", b"data".to_vec())
        .await
        .unwrap();
    assert!(cache.object_exists("abc123", "xxh128").await.unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn fs_async_object_key_format() {
    let (_tmp, cache) = make_fs_async_cache();
    let key = cache.object_key("abc123", "xxh128");
    assert!(key.ends_with("abc123.xxh128"));
}

#[tokio::test(flavor = "multi_thread")]
async fn fs_async_copy_object_to_file() {
    let (_tmp, cache) = make_fs_async_cache();
    cache
        .put_object("abc123", "xxh128", b"copy me".to_vec())
        .await
        .unwrap();
    let dest = _tmp.path().join("dest.bin");
    let n = cache
        .copy_object_to_file("abc123", "xxh128", &dest)
        .await
        .unwrap();
    assert_eq!(n, 7);
    assert_eq!(std::fs::read(&dest).unwrap(), b"copy me");
}

#[tokio::test(flavor = "multi_thread")]
async fn fs_async_write_object_to_file_at_offset() {
    let (_tmp, cache) = make_fs_async_cache();
    cache
        .put_object("abc123", "xxh128", b"HELLO".to_vec())
        .await
        .unwrap();

    let dest = _tmp.path().join("preallocated.bin");
    // Create a 20-byte zero-filled file
    std::fs::write(&dest, [0u8; 20]).unwrap();

    let n = cache
        .write_object_to_file_at_offset("abc123", "xxh128", &dest, 10)
        .await
        .unwrap();
    assert_eq!(n, 5);

    let contents = std::fs::read(&dest).unwrap();
    assert_eq!(&contents[10..15], b"HELLO");
    assert_eq!(&contents[0..10], &[0u8; 10]);
}

#[tokio::test(flavor = "multi_thread")]
async fn fs_async_has_no_multipart_or_range_capability() {
    let (_tmp, cache) = make_fs_async_cache();

    // FileSystemDataCache implements only AsyncDataCache; callers can discover
    // that it does not support multipart/range via these accessors.
    assert!(cache.as_multipart().is_none());
    assert!(cache.as_range_read().is_none());
}

// ===== S3 Streaming Download Methods =====

const TINY_PART_SIZE: usize = 8;

async fn make_s3_fixture_tiny_parts() -> (TempDir, Arc<S3DataCache>) {
    let tmp = TempDir::new().unwrap();
    let client = make_s3_client(tmp.path());
    client.create_bucket().bucket(BUCKET).send().await.unwrap();
    let data_cache = S3DataCache::new(BUCKET.to_string(), PREFIX.to_string(), client)
        .with_multipart_part_size(TINY_PART_SIZE);
    (tmp, Arc::new(data_cache))
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_stream_range_to_file_at_offset() {
    let (_s3_tmp, data_cache) = make_s3_fixture_tiny_parts().await;
    let tmp = TempDir::new().unwrap();

    let content: Vec<u8> = (0..50u8).collect();
    let hash = upload_to_s3(&*data_cache, &content);
    let dest = tmp.path().join("stream_range.bin");
    std::fs::write(&dest, vec![0u8; 50]).unwrap();
    data_cache
        .stream_range_to_file_at_offset(&hash, "xxh128", 0, 24, &dest, 0)
        .await
        .unwrap();
    data_cache
        .stream_range_to_file_at_offset(&hash, "xxh128", 25, 49, &dest, 25)
        .await
        .unwrap();

    assert_eq!(std::fs::read(&dest).unwrap(), content);
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_stream_write_object_to_file_at_offset() {
    let (_s3_tmp, data_cache) = make_s3_fixture_tiny_parts().await;
    let tmp = TempDir::new().unwrap();

    let content: Vec<u8> = (0..20u8).collect();
    let hash = upload_to_s3(&*data_cache, &content);
    let dest = tmp.path().join("write_offset.bin");
    std::fs::write(&dest, vec![0u8; 40]).unwrap();
    data_cache
        .write_object_to_file_at_offset(&hash, "xxh128", &dest, 10)
        .await
        .unwrap();

    let result = std::fs::read(&dest).unwrap();
    assert_eq!(&result[10..30], &content[..]);
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_stream_copy_object_to_file() {
    let (_s3_tmp, data_cache) = make_s3_fixture_tiny_parts().await;
    let tmp = TempDir::new().unwrap();

    let content: Vec<u8> = (0..30u8).collect();
    let hash = upload_to_s3(&*data_cache, &content);
    let dest = tmp.path().join("copy_obj.bin");
    data_cache
        .copy_object_to_file(&hash, "xxh128", &dest)
        .await
        .unwrap();

    assert_eq!(std::fs::read(&dest).unwrap(), content);
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_download_chunked_with_stream_range() {
    // chunk_size=16, part_size=8 → multipart_threshold = 2*8 = 16
    // Each 16-byte chunk >= threshold → stream_range_to_file_at_offset
    let (_s3_tmp, data_cache) = make_s3_fixture_tiny_parts().await;
    let tmp = TempDir::new().unwrap();

    let chunks: Vec<Vec<u8>> = (0..3u8).map(|i| vec![i; 16]).collect();
    let chunk_hashes: Vec<String> = chunks
        .iter()
        .map(|c| upload_to_s3(&*data_cache, c))
        .collect();

    let dest = tmp.path().join("chunked_stream.bin");
    let mut entry = FileEntry::file(dest.to_string_lossy().to_string(), 48, 1000);
    entry.chunk_hashes = Some(chunk_hashes);

    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, 16).with_files(vec![entry]);

    download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions::default(),
    )
    .await
    .unwrap();

    let expected: Vec<u8> = chunks.into_iter().flatten().collect();
    assert_eq!(std::fs::read(&dest).unwrap(), expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_download_chunked_mixed_stream_and_write() {
    // chunk_size=16, part_size=8 → multipart_threshold = 16
    // chunk0 (16 bytes) >= 16 → stream_range_to_file_at_offset
    // chunk1 (16 bytes) >= 16 → stream_range_to_file_at_offset
    // chunk2 (8 bytes)  <  16 → write_object_to_file_at_offset
    let (_s3_tmp, data_cache) = make_s3_fixture_tiny_parts().await;
    let tmp = TempDir::new().unwrap();

    let chunk0: Vec<u8> = vec![0xAA; 16];
    let chunk1: Vec<u8> = vec![0xBB; 16];
    let chunk2: Vec<u8> = vec![0xCC; 8];
    let chunk_hashes = vec![
        upload_to_s3(&*data_cache, &chunk0),
        upload_to_s3(&*data_cache, &chunk1),
        upload_to_s3(&*data_cache, &chunk2),
    ];

    let dest = tmp.path().join("chunked_mixed.bin");
    let mut entry = FileEntry::file(dest.to_string_lossy().to_string(), 40, 1000);
    entry.chunk_hashes = Some(chunk_hashes);

    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, 16).with_files(vec![entry]);

    download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions::default(),
    )
    .await
    .unwrap();

    let mut expected = Vec::new();
    expected.extend_from_slice(&chunk0);
    expected.extend_from_slice(&chunk1);
    expected.extend_from_slice(&chunk2);
    assert_eq!(std::fs::read(&dest).unwrap(), expected);
}

// ===== S3 Error Paths =====

#[tokio::test(flavor = "multi_thread")]
async fn s3_get_object_nonexistent_returns_error() {
    let (_tmp, data_cache) = make_s3_fixture().await;
    let result =
        async { AsyncDataCache::get_object(&*data_cache, "nonexistent_hash", "xxh128").await }
            .await;
    assert!(result.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_get_object_range_nonexistent_returns_error() {
    let (_tmp, data_cache) = make_s3_fixture().await;
    let result = async {
        data_cache
            .get_object_range("nonexistent_hash", "xxh128", 0, 10)
            .await
    }
    .await;
    assert!(result.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_copy_object_to_file_nonexistent_returns_error() {
    let (_tmp, data_cache) = make_s3_fixture().await;
    let dest = _tmp.path().join("dest.bin");
    let result = async {
        data_cache
            .copy_object_to_file("nonexistent_hash", "xxh128", &dest)
            .await
    }
    .await;
    assert!(result.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_write_object_to_file_at_offset_nonexistent_returns_error() {
    let (_tmp, data_cache) = make_s3_fixture().await;
    let dest = _tmp.path().join("preallocated.bin");
    std::fs::write(&dest, [0u8; 64]).unwrap();
    let result = async {
        data_cache
            .write_object_to_file_at_offset("nonexistent_hash", "xxh128", &dest, 0)
            .await
    }
    .await;
    assert!(result.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_stream_range_to_file_nonexistent_returns_error() {
    let (_tmp, data_cache) = make_s3_fixture().await;
    let dest = _tmp.path().join("preallocated.bin");
    std::fs::write(&dest, [0u8; 64]).unwrap();
    let result = async {
        data_cache
            .stream_range_to_file_at_offset("nonexistent_hash", "xxh128", 0, 10, &dest, 0)
            .await
    }
    .await;
    assert!(result.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_abort_nonexistent_multipart_returns_error() {
    let (_tmp, data_cache) = make_s3_fixture().await;
    let result = async {
        data_cache
            .abort_multipart_upload("somehash", "xxh128", "fake_upload_id")
            .await
    }
    .await;
    assert!(result.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_upload_part_nonexistent_returns_error() {
    let (_tmp, data_cache) = make_s3_fixture().await;
    let result = async {
        data_cache
            .upload_part("somehash", "xxh128", "fake_upload_id", 1, b"data".to_vec())
            .await
    }
    .await;
    assert!(result.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_complete_multipart_nonexistent_returns_error() {
    let (_tmp, data_cache) = make_s3_fixture().await;
    let result = async {
        data_cache
            .complete_multipart_upload(
                "somehash",
                "xxh128",
                "fake_upload_id",
                vec![(1, "fake_etag".to_string())],
            )
            .await
    }
    .await;
    assert!(result.is_err());
}

// ===== S3 Cache Validation Branches =====

#[tokio::test(flavor = "multi_thread")]
async fn s3_force_s3_check_bypasses_cache() {
    let (_s3_tmp, base_cache) = make_s3_fixture().await;
    let tmp = TempDir::new().unwrap();
    let check_cache = Arc::new(openjd_snapshots::S3CheckCache::new(tmp.path()).unwrap());

    let dc = S3DataCache::new(
        BUCKET.to_string(),
        PREFIX.to_string(),
        base_cache.client().clone(),
    )
    .with_s3_check_cache(Some(check_cache.clone()))
    .with_force_s3_check(true);

    // Put entry in check cache
    let cache_key = dc.cache_key("test_hash", "xxh128");
    check_cache.put_entry(&cache_key).unwrap();

    // force_s3_check should bypass the cache — covers line 352
    assert!(!dc.check_cache_exists("test_hash", "xxh128"));

    // Upload the object so HeadObject succeeds, then verify object_exists
    // goes through the HeadObject path (not the cache short-circuit)
    SyncCache::put_object(&*base_cache, "test_hash", "xxh128", b"data").unwrap();
    let exists = AsyncDataCache::object_exists(&dc, "test_hash", "xxh128")
        .await
        .unwrap();
    assert!(exists);
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_cache_validation_stale_entry_invalidates() {
    let (_s3_tmp, base_cache) = make_s3_fixture().await;
    let tmp = TempDir::new().unwrap();
    let check_cache = Arc::new(openjd_snapshots::S3CheckCache::new(tmp.path()).unwrap());

    let dc = S3DataCache::new(
        BUCKET.to_string(),
        PREFIX.to_string(),
        base_cache.client().clone(),
    )
    .with_s3_check_cache(Some(check_cache.clone()));

    // Put a stale entry — object doesn't actually exist in S3
    let cache_key = dc.cache_key("stale_hash", "xxh128");
    check_cache.put_entry(&cache_key).unwrap();

    // check_cache_exists returns true (cache has the entry)
    assert!(dc.check_cache_exists("stale_hash", "xxh128"));

    // object_exists enters the probabilistic verification path (should_verify()
    // returns true for the first 100 calls). HeadObject fails because the object
    // doesn't exist. s3s returns a generic service error (not is_not_found()),
    // so the code returns Err rather than triggering invalidation.
    let result = AsyncDataCache::object_exists(&dc, "stale_hash", "xxh128").await;
    assert!(
        result.is_err(),
        "s3s HeadObject on missing key returns Err (not Ok(false))"
    );

    // Invalidation is NOT triggered because s3s doesn't return is_not_found()
    // (it returns NoSuchKey instead of NotFound). This is a known s3s limitation.
    assert!(!dc.is_cache_validation_invalidated());
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_cache_validation_valid_entry_confirmed() {
    let (_s3_tmp, data_cache) = make_s3_fixture().await;
    let tmp = TempDir::new().unwrap();
    let check_cache = Arc::new(openjd_snapshots::S3CheckCache::new(tmp.path()).unwrap());

    // Upload a real object
    SyncCache::put_object(&*data_cache, "real_hash", "xxh128", b"real data").unwrap();

    let dc = S3DataCache::new(
        BUCKET.to_string(),
        PREFIX.to_string(),
        data_cache.client().clone(),
    )
    .with_s3_check_cache(Some(check_cache.clone()));

    // Populate check cache
    let cache_key = dc.cache_key("real_hash", "xxh128");
    check_cache.put_entry(&cache_key).unwrap();

    // Probabilistic verification does HeadObject, object exists → returns true
    let exists = AsyncDataCache::object_exists(&dc, "real_hash", "xxh128")
        .await
        .unwrap();
    assert!(exists);
    assert!(!dc.is_cache_validation_invalidated());
}
