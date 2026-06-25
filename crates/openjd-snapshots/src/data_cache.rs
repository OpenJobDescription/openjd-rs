// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use crate::hash::DEFAULT_S3_MULTIPART_PART_SIZE;
use crate::s3_check_cache::S3CheckCache;
use std::any::Any;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

/// Result of a copy_from attempt.
#[derive(Debug, PartialEq)]
pub enum CopyResult {
    /// Server-side copy completed (no data transited through client)
    ServerSideCopy,
    /// Not supported for this cache combination; caller should fall back to get+put
    NotSupported,
}

// S3 service limits used to size and bound server-side multipart copies.
//
// These are Amazon S3 API limits, not arbitrary tuning knobs. AWS may change
// them over time, so they are pinned here with links to the authoritative
// public docs for review and future maintenance:
//
// - Multipart upload limits (max parts, min/max part size):
//   https://docs.aws.amazon.com/AmazonS3/latest/userguide/qfacts.html
// - CopyObject (single-call 5 GiB source limit):
//   https://docs.aws.amazon.com/AmazonS3/latest/API/API_CopyObject.html
// - UploadPartCopy (per-part byte-range copy):
//   https://docs.aws.amazon.com/AmazonS3/latest/API/API_UploadPartCopy.html

/// Maximum number of parts allowed in an S3 multipart upload (S3 limit: 10,000).
/// See <https://docs.aws.amazon.com/AmazonS3/latest/userguide/qfacts.html>.
const S3_MAX_MULTIPART_PARTS: u64 = 10_000;
/// Maximum size (in bytes) of a copy source for a single `CopyObject` call
/// (S3 limit: 5 GiB). Sources larger than this must be copied with multipart
/// `UploadPartCopy`. See
/// <https://docs.aws.amazon.com/AmazonS3/latest/API/API_CopyObject.html>.
pub const S3_MAX_SINGLE_COPY_BYTES: u64 = 5 * 1024 * 1024 * 1024;
/// S3 minimum multipart part size (S3 limit: 5 MiB). Applies to every part
/// except the last. See
/// <https://docs.aws.amazon.com/AmazonS3/latest/userguide/qfacts.html>.
const S3_MIN_PART_SIZE: u64 = 5 * 1024 * 1024;
/// S3 maximum size of a single part / `UploadPartCopy` byte range
/// (S3 limit: 5 GiB). See
/// <https://docs.aws.amazon.com/AmazonS3/latest/userguide/qfacts.html>.
const S3_MAX_PART_SIZE: u64 = 5 * 1024 * 1024 * 1024;
/// Default multipart-copy threshold (5 MiB). This is a *performance*
/// threshold, not S3's API cap: cross-region single `CopyObject` runs at
/// roughly single-stream speed (~5 MiB/s), while parallel `UploadPartCopy`
/// reaches hundreds of MiB/s. Benchmarks show multipart wins at every size at
/// or above S3's 5 MiB minimum part size, so 5 MiB is the natural cutoff —
/// anything smaller can only be a single part anyway.
const DEFAULT_COPY_THRESHOLD: u64 = 5 * 1024 * 1024;
/// Default target part size for server-side multipart copies (5 MiB). Equal to
/// the S3 minimum part size, which maximizes the number of parts (and thus
/// available parallelism); the effective part size is grown automatically for
/// large objects to respect the 10,000-part cap.
const DEFAULT_COPY_PART_SIZE: u64 = 5 * 1024 * 1024;
/// Default maximum number of concurrent `UploadPartCopy` requests per object.
/// 64 is the empirically validated inner-concurrency sweet spot for
/// cross-region copies.
const DEFAULT_COPY_CONCURRENCY: usize = 64;

/// Tuning parameters for `S3DataCache` server-side copies, analogous to the
/// AWS SDK `TransferConfig`.
///
/// Sources at or below [`multipart_threshold`](S3CopyConfig::multipart_threshold)
/// are copied with a single `CopyObject`; larger sources are copied
/// server-side with parallel `UploadPartCopy` requests so the data never
/// transits through the client. A single `CopyObject` can copy sources only up
/// to [`S3_MAX_SINGLE_COPY_BYTES`] (5 GiB), so the threshold is always capped
/// at 5 GiB regardless of the configured value.
#[derive(Debug, Clone)]
pub struct S3CopyConfig {
    /// Sources strictly larger than this use multipart `UploadPartCopy`.
    /// Defaults to 5 MiB — a performance cutoff, since parallel multipart copy
    /// outperforms single `CopyObject` for cross-region transfers at every size
    /// at or above S3's 5 MiB minimum part size. Values above 5 GiB are clamped
    /// to 5 GiB at copy time because `CopyObject` cannot copy bigger sources.
    pub multipart_threshold: u64,
    /// Target size of each `UploadPartCopy` part. Default 5 MiB (the S3 minimum
    /// part size), which maximizes parallelism. The effective part size is
    /// grown automatically when needed to keep the part count at or below S3's
    /// 10,000-part limit, and is clamped to the S3 part size range of
    /// 5 MiB..=5 GiB.
    pub part_size: u64,
    /// Maximum number of concurrent `UploadPartCopy` requests for a single
    /// object. Default 64.
    pub max_concurrency: usize,
}

impl Default for S3CopyConfig {
    fn default() -> Self {
        Self {
            multipart_threshold: DEFAULT_COPY_THRESHOLD,
            part_size: DEFAULT_COPY_PART_SIZE,
            max_concurrency: DEFAULT_COPY_CONCURRENCY,
        }
    }
}

impl S3CopyConfig {
    /// The size above which a copy uses multipart `UploadPartCopy`, never more
    /// than the 5 GiB `CopyObject` ceiling.
    fn effective_threshold(&self) -> u64 {
        self.multipart_threshold.min(S3_MAX_SINGLE_COPY_BYTES)
    }

    /// Compute the part size to use for an object of `object_size` bytes.
    ///
    /// Starts from the configured [`part_size`](S3CopyConfig::part_size), grows
    /// it so the resulting part count does not exceed S3's 10,000-part limit,
    /// and clamps the result to the S3 part size range (5 MiB..=5 GiB).
    fn effective_part_size(&self, object_size: u64) -> u64 {
        let min_required = object_size.div_ceil(S3_MAX_MULTIPART_PARTS);
        self.part_size
            .max(min_required)
            .clamp(S3_MIN_PART_SIZE, S3_MAX_PART_SIZE)
    }
}

/// Async content-addressed data cache for use in tokio pipelines.
///
/// This is the core trait implemented by every async cache backend. Backends that
/// additionally support S3-style multipart upload or byte-range reads implement the
/// companion traits [`MultipartDataCache`] and [`RangeReadDataCache`] and override
/// [`as_multipart`](AsyncDataCache::as_multipart) /
/// [`as_range_read`](AsyncDataCache::as_range_read) so callers can discover the
/// extra capability through a trait object.
#[async_trait]
pub trait AsyncDataCache: Send + Sync {
    fn object_key(&self, hash: &str, algorithm: &str) -> String;
    fn as_any(&self) -> &dyn Any;
    async fn object_exists(&self, hash: &str, algorithm: &str) -> std::io::Result<bool>;
    async fn put_object(
        &self,
        hash: &str,
        algorithm: &str,
        data: Vec<u8>,
    ) -> std::io::Result<String>;
    async fn get_object(&self, hash: &str, algorithm: &str) -> std::io::Result<Vec<u8>>;

    /// Attempt a server-side copy from another cache. Returns `NotSupported` by default.
    async fn copy_from(
        &self,
        _source: &dyn AsyncDataCache,
        _hash: &str,
        _algorithm: &str,
    ) -> std::io::Result<CopyResult> {
        Ok(CopyResult::NotSupported)
    }

    /// Returns the part size for multipart transfers. Default: 32MB.
    ///
    /// Callers use this value as a threshold hint (e.g. whether a file is large
    /// enough to warrant multipart upload) before checking whether the backend
    /// actually supports multipart via [`as_multipart`](AsyncDataCache::as_multipart).
    fn multipart_part_size(&self) -> usize {
        crate::hash::DEFAULT_S3_MULTIPART_PART_SIZE
    }

    /// Returns `Some(self)` as a [`MultipartDataCache`] if the backend supports
    /// S3-style multipart upload. Default: `None`.
    fn as_multipart(&self) -> Option<&dyn MultipartDataCache> {
        None
    }

    /// Returns `Some(self)` as a [`RangeReadDataCache`] if the backend supports
    /// byte-range reads. Default: `None`.
    fn as_range_read(&self) -> Option<&dyn RangeReadDataCache> {
        None
    }

    /// Copy a cached object directly to a file. The default reads into memory then writes.
    /// Filesystem implementations can override for zero-copy (e.g. sendfile).
    async fn copy_object_to_file(
        &self,
        hash: &str,
        algorithm: &str,
        dest: &std::path::Path,
    ) -> std::io::Result<u64> {
        let data = self.get_object(hash, algorithm).await?;
        let len = data.len() as u64;
        tokio::fs::write(dest, &data).await?;
        Ok(len)
    }

    /// Write a cached object into an already-open file at a given offset.
    /// Default reads into memory then writes. Filesystem can override for efficiency.
    async fn write_object_to_file_at_offset(
        &self,
        hash: &str,
        algorithm: &str,
        dest: &std::path::Path,
        offset: u64,
    ) -> std::io::Result<u64> {
        let data = self.get_object(hash, algorithm).await?;
        let len = data.len() as u64;
        let dest = dest.to_path_buf();
        tokio::task::spawn_blocking(move || {
            use std::io::{Seek, SeekFrom, Write};
            let mut f = std::fs::OpenOptions::new().write(true).open(&dest)?;
            f.seek(SeekFrom::Start(offset))?;
            f.write_all(&data)?;
            Ok::<_, std::io::Error>(len)
        })
        .await
        .map_err(std::io::Error::other)?
    }
}

/// Extension trait for caches that support S3-style multipart upload.
#[async_trait]
pub trait MultipartDataCache: AsyncDataCache {
    async fn create_multipart_upload(&self, hash: &str, algorithm: &str)
        -> std::io::Result<String>;
    async fn upload_part(
        &self,
        hash: &str,
        algorithm: &str,
        upload_id: &str,
        part_number: i32,
        data: Vec<u8>,
    ) -> std::io::Result<String>;
    async fn complete_multipart_upload(
        &self,
        hash: &str,
        algorithm: &str,
        upload_id: &str,
        parts: Vec<(i32, String)>,
    ) -> std::io::Result<()>;
    async fn abort_multipart_upload(
        &self,
        hash: &str,
        algorithm: &str,
        upload_id: &str,
    ) -> std::io::Result<()>;
}

/// Extension trait for caches that support byte-range reads.
#[async_trait]
pub trait RangeReadDataCache: AsyncDataCache {
    async fn get_object_range(
        &self,
        hash: &str,
        algorithm: &str,
        start: u64,
        end: u64,
    ) -> std::io::Result<Vec<u8>>;

    /// Stream a byte-range of a cached object directly to a file at a given offset.
    /// Default uses get_object_range + write. S3 overrides to stream without buffering.
    async fn stream_range_to_file_at_offset(
        &self,
        hash: &str,
        algorithm: &str,
        range_start: u64,
        range_end: u64,
        dest: &std::path::Path,
        file_offset: u64,
    ) -> std::io::Result<u64> {
        let data = self
            .get_object_range(hash, algorithm, range_start, range_end)
            .await?;
        let len = data.len() as u64;
        let dest = dest.to_path_buf();
        tokio::task::spawn_blocking(move || {
            use std::io::{Seek, SeekFrom, Write};
            let mut f = std::fs::OpenOptions::new().write(true).open(&dest)?;
            f.seek(SeekFrom::Start(file_offset))?;
            f.write_all(&data)?;
            Ok::<_, std::io::Error>(len)
        })
        .await
        .map_err(std::io::Error::other)?
    }
}

/// Content-addressed storage backed by a local or network filesystem.
pub struct FileSystemDataCache {
    pub root_path: PathBuf,
}

impl FileSystemDataCache {
    pub fn new(root_path: impl Into<PathBuf>) -> crate::Result<Self> {
        let root_path = root_path.into();
        if !root_path.is_absolute() {
            return Err(crate::SnapshotError::Validation(
                "root_path must be absolute".into(),
            ));
        }
        std::fs::create_dir_all(&root_path)?;
        Ok(Self { root_path })
    }

    fn object_path(&self, hash: &str, algorithm: &str) -> PathBuf {
        self.root_path.join(format!("{hash}.{algorithm}"))
    }
}

#[async_trait]
impl AsyncDataCache for FileSystemDataCache {
    fn object_key(&self, hash: &str, algorithm: &str) -> String {
        self.object_path(hash, algorithm)
            .to_string_lossy()
            .into_owned()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn object_exists(&self, hash: &str, algorithm: &str) -> std::io::Result<bool> {
        Ok(self.object_path(hash, algorithm).exists())
    }

    async fn put_object(
        &self,
        hash: &str,
        algorithm: &str,
        data: Vec<u8>,
    ) -> std::io::Result<String> {
        let path = self.object_path(hash, algorithm);
        tokio::fs::write(&path, &data).await?;
        Ok(path.to_string_lossy().into_owned())
    }

    async fn get_object(&self, hash: &str, algorithm: &str) -> std::io::Result<Vec<u8>> {
        tokio::fs::read(self.object_path(hash, algorithm)).await
    }

    async fn copy_object_to_file(
        &self,
        hash: &str,
        algorithm: &str,
        dest: &std::path::Path,
    ) -> std::io::Result<u64> {
        let src = self.object_path(hash, algorithm);
        let dest = dest.to_path_buf();
        tokio::task::spawn_blocking(move || std::fs::copy(&src, &dest))
            .await
            .map_err(std::io::Error::other)?
    }

    async fn write_object_to_file_at_offset(
        &self,
        hash: &str,
        algorithm: &str,
        dest: &std::path::Path,
        offset: u64,
    ) -> std::io::Result<u64> {
        let src = self.object_path(hash, algorithm);
        let dest = dest.to_path_buf();
        tokio::task::spawn_blocking(move || {
            use std::io::{Seek, SeekFrom};
            let mut src_file = std::fs::File::open(&src)?;
            let src_len = src_file.metadata()?.len();
            let mut dest_file = std::fs::OpenOptions::new().write(true).open(&dest)?;
            dest_file.seek(SeekFrom::Start(offset))?;
            std::io::copy(&mut src_file, &mut dest_file)?;
            Ok::<_, std::io::Error>(src_len)
        })
        .await
        .map_err(std::io::Error::other)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn new_creates_directory() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        assert!(!cache_dir.exists());
        let _cache = FileSystemDataCache::new(&cache_dir).unwrap();
        assert!(cache_dir.exists());
    }

    #[test]
    fn rejects_relative_path() {
        let result = FileSystemDataCache::new("relative/path");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn put_and_get_object() {
        let tmp = TempDir::new().unwrap();
        let cache = FileSystemDataCache::new(tmp.path().join("cache")).unwrap();
        cache
            .put_object("abc123", "xxh128", b"hello".to_vec())
            .await
            .unwrap();
        let data = cache.get_object("abc123", "xxh128").await.unwrap();
        assert_eq!(data, b"hello");
    }

    #[tokio::test]
    async fn object_exists_check() {
        let tmp = TempDir::new().unwrap();
        let cache = FileSystemDataCache::new(tmp.path().join("cache")).unwrap();
        assert!(!cache.object_exists("abc123", "xxh128").await.unwrap());
        cache
            .put_object("abc123", "xxh128", b"data".to_vec())
            .await
            .unwrap();
        assert!(cache.object_exists("abc123", "xxh128").await.unwrap());
    }

    #[test]
    fn object_key_format() {
        let tmp = TempDir::new().unwrap();
        let cache = FileSystemDataCache::new(tmp.path().join("cache")).unwrap();
        let key = AsyncDataCache::object_key(&cache, "abc123", "xxh128");
        assert!(key.ends_with("abc123.xxh128"));
    }

    #[tokio::test]
    async fn get_nonexistent_returns_error() {
        let tmp = TempDir::new().unwrap();
        let cache = FileSystemDataCache::new(tmp.path().join("cache")).unwrap();
        assert!(cache.get_object("missing", "xxh128").await.is_err());
    }

    #[tokio::test]
    async fn copy_from_default_returns_not_supported() {
        let tmp = TempDir::new().unwrap();
        let src = FileSystemDataCache::new(tmp.path().join("src")).unwrap();
        let dst = FileSystemDataCache::new(tmp.path().join("dst")).unwrap();
        let result = dst.copy_from(&src, "abc", "xxh128").await.unwrap();
        assert_eq!(result, CopyResult::NotSupported);
    }

    #[test]
    fn format_copy_source_regular_bucket() {
        let result = super::format_copy_source("my-bucket", "Data/abc123.xxh128");
        assert_eq!(result, "my-bucket/Data/abc123.xxh128");
    }

    #[test]
    fn format_copy_source_access_point_arn() {
        let arn = "arn:aws:s3:us-west-2:123456789012:accesspoint/my-access-point";
        let result = super::format_copy_source(arn, "Data/abc123.xxh128");
        assert_eq!(
            result,
            "arn:aws:s3:us-west-2:123456789012:accesspoint/my-access-point/object/Data/abc123.xxh128"
        );
    }

    #[test]
    fn format_copy_source_outpost_arn() {
        let arn = "arn:aws:s3-outposts:us-west-2:123456789012:outpost/my-outpost";
        let result = super::format_copy_source(arn, "Data/abc123.xxh128");
        assert_eq!(
            result,
            "arn:aws:s3-outposts:us-west-2:123456789012:outpost/my-outpost/object/Data/abc123.xxh128"
        );
    }

    #[test]
    fn copy_config_defaults() {
        let cfg = super::S3CopyConfig::default();
        // Benchmarked cross-region defaults: 5 MiB performance threshold,
        // 5 MiB parts (max parallelism), 64-way inner concurrency.
        assert_eq!(cfg.multipart_threshold, 5 * 1024 * 1024);
        assert_eq!(cfg.part_size, 5 * 1024 * 1024);
        assert_eq!(cfg.max_concurrency, 64);
    }

    #[test]
    fn effective_threshold_never_exceeds_5gib() {
        let cfg = super::S3CopyConfig {
            // Caller asks for an impossible 10 GiB threshold.
            multipart_threshold: 10 * 1024 * 1024 * 1024,
            ..Default::default()
        };
        // CopyObject can't copy > 5 GiB, so the threshold is clamped.
        assert_eq!(cfg.effective_threshold(), 5 * 1024 * 1024 * 1024);
    }

    #[test]
    fn effective_part_size_uses_configured_size_when_part_count_fits() {
        let cfg = super::S3CopyConfig {
            part_size: 256 * 1024 * 1024,
            ..Default::default()
        };
        // 8 GiB object => 32 parts at 256 MiB, well under 10k.
        let size = 8u64 * 1024 * 1024 * 1024;
        assert_eq!(cfg.effective_part_size(size), 256 * 1024 * 1024);
    }

    #[test]
    fn effective_part_size_grows_to_respect_10k_part_limit() {
        let cfg = super::S3CopyConfig {
            part_size: super::S3_MIN_PART_SIZE, // 5 MiB
            ..Default::default()
        };
        // 5 TiB at 5 MiB parts would be ~1.05M parts (> 10k), so the part size
        // must grow to keep <= 10,000 parts.
        let size = 5u64 * 1024 * 1024 * 1024 * 1024;
        let ps = cfg.effective_part_size(size);
        let num_parts = size.div_ceil(ps);
        assert!(
            num_parts <= super::S3_MAX_MULTIPART_PARTS,
            "{num_parts} parts"
        );
        assert!(ps >= size.div_ceil(super::S3_MAX_MULTIPART_PARTS));
    }

    #[test]
    fn effective_part_size_clamps_to_min() {
        let cfg = super::S3CopyConfig {
            part_size: 1024, // 1 KiB, below the 5 MiB S3 minimum
            ..Default::default()
        };
        // Small object, but part size must still respect the 5 MiB floor.
        assert_eq!(
            cfg.effective_part_size(10 * 1024 * 1024),
            super::S3_MIN_PART_SIZE
        );
    }
}

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tracing::warn;

fn rand_u64() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let s = RandomState::new();
    let mut h = s.build_hasher();
    h.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64,
    );
    h.finish()
}

pub struct CacheValidationState {
    hit_count: AtomicU64,
    invalidated: AtomicBool,
}

impl Default for CacheValidationState {
    fn default() -> Self {
        Self::new()
    }
}

impl CacheValidationState {
    pub fn new() -> Self {
        Self {
            hit_count: AtomicU64::new(0),
            invalidated: AtomicBool::new(false),
        }
    }

    fn should_verify(&self) -> bool {
        let count = self.hit_count.fetch_add(1, Ordering::Relaxed) + 1;
        if count <= 100 {
            return true;
        }
        rand_u64().is_multiple_of(100)
    }

    fn invalidate(&self) {
        self.invalidated.store(true, Ordering::Relaxed);
    }

    pub fn is_invalidated(&self) -> bool {
        self.invalidated.load(Ordering::Relaxed)
    }
}

/// Content-addressed storage backed by Amazon S3.
pub struct S3DataCache {
    bucket: String,
    key_prefix: String,
    client: aws_sdk_s3::Client,
    multipart_part_size: usize,
    s3_check_cache: Option<Arc<S3CheckCache>>,
    force_s3_check: bool,
    expected_bucket_owner: Option<String>,
    cache_validation: CacheValidationState,
    copy_config: Option<S3CopyConfig>,
}

impl S3DataCache {
    pub fn new(bucket: String, key_prefix: String, client: aws_sdk_s3::Client) -> Self {
        Self {
            bucket,
            key_prefix,
            client,
            multipart_part_size: DEFAULT_S3_MULTIPART_PART_SIZE,
            s3_check_cache: None,
            force_s3_check: false,
            expected_bucket_owner: None,
            cache_validation: CacheValidationState::new(),
            // Single CopyObject by default; opt in to parallel multipart
            // UploadPartCopy with with_copy_config.
            copy_config: None,
        }
    }

    /// Set the multipart part size for S3 transfers.
    pub fn with_multipart_part_size(mut self, size: usize) -> Self {
        self.multipart_part_size = size;
        self
    }

    /// Attach an S3 check cache used to skip HeadObject calls.
    pub fn with_s3_check_cache(mut self, cache: Option<Arc<S3CheckCache>>) -> Self {
        self.s3_check_cache = cache;
        self
    }

    /// If true, always call HeadObject even when the check cache has an entry.
    pub fn with_force_s3_check(mut self, force: bool) -> Self {
        self.force_s3_check = force;
        self
    }

    /// Set the expected bucket owner account ID for S3 requests.
    pub fn with_expected_bucket_owner(mut self, owner: Option<String>) -> Self {
        self.expected_bucket_owner = owner;
        self
    }

    /// Enable server-side multipart copy and set its tuning parameters
    /// (multipart threshold, part size, and concurrency) for
    /// [`copy_from`](AsyncDataCache::copy_from).
    ///
    /// By default (no copy config) `copy_from` performs a single `CopyObject`
    /// with no extra `HeadObject` — valid for sources up to S3's 5 GiB
    /// single-copy limit. Providing a config (e.g. [`S3CopyConfig::default`],
    /// the benchmarked cross-region settings) opts in to size-aware routing:
    /// `copy_from` then issues one `HeadObject` to learn the source size and
    /// uses parallel `UploadPartCopy` for sources above the threshold (required
    /// both to copy sources > 5 GiB and to parallelize large cross-region
    /// copies for throughput). `copy_config` is consulted only by `copy_from`,
    /// which is used only by cache-sync.
    pub fn with_copy_config(mut self, config: S3CopyConfig) -> Self {
        self.copy_config = Some(config);
        self
    }

    /// Returns the server-side copy tuning parameters, if multipart copy has
    /// been enabled via [`with_copy_config`](S3DataCache::with_copy_config).
    pub fn copy_config(&self) -> Option<&S3CopyConfig> {
        self.copy_config.as_ref()
    }

    /// Returns a reference to the underlying AWS S3 client.
    pub fn client(&self) -> &aws_sdk_s3::Client {
        &self.client
    }

    /// Returns the expected bucket owner account ID, if configured.
    pub fn expected_bucket_owner(&self) -> Option<&str> {
        self.expected_bucket_owner.as_deref()
    }

    /// Returns the cache key for a given hash: "{bucket}/{prefix}/{hash}.{algorithm}"
    pub fn cache_key(&self, hash: &str, algorithm: &str) -> String {
        format!("{}/{}", self.bucket, self.object_key(hash, algorithm))
    }

    /// Returns `true` if the probabilistic S3-check-cache validation has detected
    /// a stale entry and invalidated the local cache. Primarily useful for tests.
    pub fn is_cache_validation_invalidated(&self) -> bool {
        self.cache_validation.is_invalidated()
    }

    /// Create with auto-detected account ID from AWS credentials.
    pub async fn new_with_auto_account_id(
        bucket: String,
        key_prefix: String,
        s3_client: aws_sdk_s3::Client,
        sts_client: aws_sdk_sts::Client,
    ) -> crate::Result<Self> {
        let resp =
            sts_client.get_caller_identity().send().await.map_err(|e| {
                crate::SnapshotError::S3(format!("STS GetCallerIdentity failed: {e}"))
            })?;
        let account = resp
            .account()
            .ok_or_else(|| crate::SnapshotError::S3("STS response missing Account".into()))?
            .to_string();
        Ok(Self::new(bucket, key_prefix, s3_client).with_expected_bucket_owner(Some(account)))
    }

    /// Check the S3 check cache for an entry (without HeadObject).
    pub fn check_cache_exists(&self, hash: &str, algorithm: &str) -> bool {
        if self.force_s3_check {
            return false;
        }
        if let Some(ref cache) = self.s3_check_cache {
            cache.get_entry(&self.cache_key(hash, algorithm)).is_some()
        } else {
            false
        }
    }

    /// Record that an object exists in S3 (update the check cache).
    pub fn record_in_check_cache(&self, hash: &str, algorithm: &str) {
        if let Some(ref cache) = self.s3_check_cache {
            let _ = cache.put_entry(&self.cache_key(hash, algorithm));
        }
    }

    fn object_key(&self, hash: &str, algorithm: &str) -> String {
        format!("{}/{hash}.{algorithm}", self.key_prefix)
    }

    /// Copy a source object into this cache server-side using a multipart
    /// upload composed of parallel `UploadPartCopy` requests. Used for sources
    /// larger than the single-`CopyObject` ceiling (5 GiB) or larger than the
    /// configured [`multipart_threshold`](S3CopyConfig::multipart_threshold).
    ///
    /// `copy_source` is the pre-formatted `x-amz-copy-source` value and `size`
    /// is the source object size in bytes. On any part failure the multipart
    /// upload is aborted and the first error is returned.
    async fn multipart_copy(
        &self,
        src_s3: &S3DataCache,
        dst_key: &str,
        copy_source: &str,
        size: u64,
        cfg: &S3CopyConfig,
    ) -> std::io::Result<()> {
        use futures_util::stream::StreamExt;

        // Pick a part size for this object: the configured size, grown if
        // needed so the part count stays within S3's 10,000-part cap, then
        // split the byte range into that many parts.
        let part_size = cfg.effective_part_size(size);
        let num_parts = size.div_ceil(part_size);

        // Open the multipart upload on the destination. All parts copied below
        // attach to the returned upload_id; it must be completed or aborted.
        let create = self
            .client
            .create_multipart_upload()
            .bucket(&self.bucket)
            .key(dst_key)
            .set_expected_bucket_owner(self.expected_bucket_owner.clone())
            .send()
            .await
            .map_err(|e| {
                std::io::Error::other(format!(
                    "S3 CreateMultipartUpload failed for {dst_key}: {e}"
                ))
            })?;
        let upload_id = create
            .upload_id()
            .map(|s| s.to_string())
            .ok_or_else(|| std::io::Error::other("CreateMultipartUpload missing upload_id"))?;
        let upload_id = upload_id.as_str();

        // Precompute the (1-based part number, inclusive byte range) for each
        // part. The last part is short unless `size` is an exact multiple of
        // `part_size`.
        let part_ranges: Vec<(i32, u64, u64)> = (0..num_parts)
            .map(|i| {
                let part_number = (i + 1) as i32;
                let start = i * part_size;
                let end = std::cmp::min(start + part_size, size) - 1;
                (part_number, start, end)
            })
            .collect();

        // Copy every part server-side with UploadPartCopy, up to
        // `max_concurrency` requests in flight at once. No object bytes pass
        // through the client. Each part returns its (part number, ETag), which
        // CompleteMultipartUpload needs to stitch the object back together.
        let copy_results: Vec<std::io::Result<(i32, String)>> =
            futures_util::stream::iter(part_ranges)
                .map(|(part_number, start, end)| async move {
                    let range = format!("bytes={start}-{end}");
                    let resp = self
                        .client
                        .upload_part_copy()
                        .bucket(&self.bucket)
                        .key(dst_key)
                        .upload_id(upload_id)
                        .part_number(part_number)
                        .copy_source(copy_source)
                        .copy_source_range(&range)
                        // Destination and source bucket-owner guards
                        // (confused-deputy protection).
                        .set_expected_bucket_owner(self.expected_bucket_owner.clone())
                        .set_expected_source_bucket_owner(src_s3.expected_bucket_owner.clone())
                        .send()
                        .await
                        .map_err(|e| {
                            std::io::Error::other(format!(
                                "S3 UploadPartCopy failed for {dst_key} part {part_number}: {e}"
                            ))
                        })?;
                    let etag = resp
                        .copy_part_result()
                        .and_then(|r| r.e_tag())
                        .map(|s| s.to_string())
                        .ok_or_else(|| {
                            std::io::Error::other(format!(
                                "UploadPartCopy missing ETag for {dst_key} part {part_number}"
                            ))
                        })?;
                    Ok((part_number, etag))
                })
                .buffer_unordered(cfg.max_concurrency.max(1))
                .collect()
                .await;

        // Separate successful parts from the first error. We drain the whole
        // stream first (rather than short-circuiting) so there are no parts
        // still in flight when we decide whether to complete or abort.
        let mut parts = Vec::with_capacity(num_parts as usize);
        let mut first_err = None;
        for r in copy_results {
            match r {
                Ok(p) => parts.push(p),
                Err(e) => {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
            }
        }

        // On any failure, abort the upload so S3 doesn't retain the
        // in-progress parts (which would otherwise accrue storage charges),
        // then surface the first error.
        if let Some(e) = first_err {
            let _ = self
                .client
                .abort_multipart_upload()
                .bucket(&self.bucket)
                .key(dst_key)
                .upload_id(upload_id)
                .set_expected_bucket_owner(self.expected_bucket_owner.clone())
                .send()
                .await;
            return Err(e);
        }

        // All parts succeeded. S3 requires the parts in ascending part-number
        // order (they completed out of order due to buffer_unordered), each
        // with its ETag, to finalize and assemble the destination object.
        parts.sort_by_key(|(num, _)| *num);
        let completed_parts: Vec<_> = parts
            .into_iter()
            .map(|(num, etag)| {
                aws_sdk_s3::types::CompletedPart::builder()
                    .part_number(num)
                    .e_tag(etag)
                    .build()
            })
            .collect();
        let upload = aws_sdk_s3::types::CompletedMultipartUpload::builder()
            .set_parts(Some(completed_parts))
            .build();
        self.client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(dst_key)
            .upload_id(upload_id)
            .multipart_upload(upload)
            .set_expected_bucket_owner(self.expected_bucket_owner.clone())
            .send()
            .await
            .map_err(|e| {
                std::io::Error::other(format!(
                    "S3 CompleteMultipartUpload failed for {dst_key}: {e}"
                ))
            })?;
        Ok(())
    }
}

/// Format the `x-amz-copy-source` value for S3 CopyObject.
///
/// For regular buckets: `{bucket}/{key}`
/// For access point ARNs: `{arn}/object/{key}`
fn format_copy_source(bucket: &str, key: &str) -> String {
    if bucket.starts_with("arn:") {
        format!("{}/object/{}", bucket, key)
    } else {
        format!("{}/{}", bucket, key)
    }
}

#[async_trait]
impl AsyncDataCache for S3DataCache {
    fn object_key(&self, hash: &str, algorithm: &str) -> String {
        format!("{}/{hash}.{algorithm}", self.key_prefix)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn multipart_part_size(&self) -> usize {
        self.multipart_part_size
    }

    fn as_multipart(&self) -> Option<&dyn MultipartDataCache> {
        Some(self)
    }

    fn as_range_read(&self) -> Option<&dyn RangeReadDataCache> {
        Some(self)
    }

    async fn copy_from(
        &self,
        source: &dyn AsyncDataCache,
        hash: &str,
        algorithm: &str,
    ) -> std::io::Result<CopyResult> {
        let Some(src_s3) = source.as_any().downcast_ref::<S3DataCache>() else {
            return Ok(CopyResult::NotSupported);
        };
        let src_key = src_s3.object_key(hash, algorithm);
        let dst_key = self.object_key(hash, algorithm);
        let copy_source = format_copy_source(&src_s3.bucket, &src_key);

        // Without a copy config (the default), do a single CopyObject and skip
        // the size lookup entirely — one API call, valid up to S3's 5 GiB copy
        // limit. Multipart copy (and the HeadObject it needs to size its parts)
        // is opt-in via with_copy_config.
        if let Some(cfg) = self.copy_config.as_ref() {
            // Opted in: learn the source size so we can route large or
            // throughput-sensitive copies to parallel UploadPartCopy. A single
            // CopyObject cannot copy sources > 5 GiB, and UploadPartCopy needs
            // explicit byte ranges, so the size is required here.
            let head = src_s3
                .client
                .head_object()
                .bucket(&src_s3.bucket)
                .key(&src_key)
                .set_expected_bucket_owner(src_s3.expected_bucket_owner.clone())
                .send()
                .await
                .map_err(|e| {
                    std::io::Error::other(format!(
                        "S3 HeadObject failed for copy source {src_key}: {e}"
                    ))
                })?;
            let size = head.content_length().unwrap_or_default().max(0) as u64;
            if size > cfg.effective_threshold() {
                self.multipart_copy(src_s3, &dst_key, &copy_source, size, cfg)
                    .await?;
                self.record_in_check_cache(hash, algorithm);
                return Ok(CopyResult::ServerSideCopy);
            }
        }

        self.client
            .copy_object()
            .bucket(&self.bucket)
            .key(&dst_key)
            .copy_source(&copy_source)
            .set_expected_bucket_owner(self.expected_bucket_owner.clone())
            .set_expected_source_bucket_owner(src_s3.expected_bucket_owner.clone())
            .send()
            .await
            .map_err(|e| std::io::Error::other(format!("S3 CopyObject failed: {e}")))?;
        self.record_in_check_cache(hash, algorithm);
        Ok(CopyResult::ServerSideCopy)
    }

    async fn object_exists(&self, hash: &str, algorithm: &str) -> std::io::Result<bool> {
        if self.check_cache_exists(hash, algorithm) && !self.cache_validation.is_invalidated() {
            if !self.cache_validation.should_verify() {
                return Ok(true);
            }
            // Probabilistic verification: do HeadObject to confirm
            let key = AsyncDataCache::object_key(self, hash, algorithm);
            return match self
                .client
                .head_object()
                .bucket(&self.bucket)
                .key(&key)
                .set_expected_bucket_owner(self.expected_bucket_owner.clone())
                .send()
                .await
            {
                Ok(_) => Ok(true),
                Err(e) => {
                    if e.as_service_error().is_some_and(|se| se.is_not_found()) {
                        warn!(key = %key, "S3 check cache stale entry detected, invalidating cache");
                        self.cache_validation.invalidate();
                        Ok(false)
                    } else {
                        Err(std::io::Error::other(format!(
                            "S3 HeadObject failed for {key}: {e}"
                        )))
                    }
                }
            };
        }
        let key = AsyncDataCache::object_key(self, hash, algorithm);
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&key)
            .set_expected_bucket_owner(self.expected_bucket_owner.clone())
            .send()
            .await
        {
            Ok(_) => {
                self.record_in_check_cache(hash, algorithm);
                Ok(true)
            }
            Err(e) => {
                if e.as_service_error().is_some_and(|se| se.is_not_found()) {
                    Ok(false)
                } else {
                    Err(std::io::Error::other(format!(
                        "S3 HeadObject failed for {key}: {e}"
                    )))
                }
            }
        }
    }

    async fn put_object(
        &self,
        hash: &str,
        algorithm: &str,
        data: Vec<u8>,
    ) -> std::io::Result<String> {
        let key = AsyncDataCache::object_key(self, hash, algorithm);
        let body = aws_sdk_s3::primitives::ByteStream::from(data);
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(body)
            .set_expected_bucket_owner(self.expected_bucket_owner.clone())
            .send()
            .await
            .map_err(|e| std::io::Error::other(format!("S3 PutObject failed for {key}: {e}")))?;
        self.record_in_check_cache(hash, algorithm);
        Ok(key)
    }

    async fn get_object(&self, hash: &str, algorithm: &str) -> std::io::Result<Vec<u8>> {
        let key = AsyncDataCache::object_key(self, hash, algorithm);
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .set_expected_bucket_owner(self.expected_bucket_owner.clone())
            .send()
            .await
            .map_err(|e| std::io::Error::other(format!("S3 GetObject failed for {key}: {e}")))?;
        let bytes = resp.body.collect().await.map_err(|e| {
            std::io::Error::other(format!("S3 GetObject body read failed for {key}: {e}"))
        })?;
        Ok(bytes.to_vec())
    }

    async fn copy_object_to_file(
        &self,
        hash: &str,
        algorithm: &str,
        dest: &std::path::Path,
    ) -> std::io::Result<u64> {
        let key = AsyncDataCache::object_key(self, hash, algorithm);
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .set_expected_bucket_owner(self.expected_bucket_owner.clone())
            .send()
            .await
            .map_err(|e| std::io::Error::other(format!("S3 GetObject failed for {key}: {e}")))?;
        let bytes = resp.body.collect().await.map_err(|e| {
            std::io::Error::other(format!("S3 GetObject body read failed for {key}: {e}"))
        })?;
        let data = bytes.to_vec();
        let len = data.len() as u64;
        let dest = dest.to_path_buf();
        tokio::task::spawn_blocking(move || std::fs::write(&dest, &data))
            .await
            .map_err(std::io::Error::other)??;
        Ok(len)
    }

    async fn write_object_to_file_at_offset(
        &self,
        hash: &str,
        algorithm: &str,
        dest: &std::path::Path,
        offset: u64,
    ) -> std::io::Result<u64> {
        let key = AsyncDataCache::object_key(self, hash, algorithm);
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .set_expected_bucket_owner(self.expected_bucket_owner.clone())
            .send()
            .await
            .map_err(|e| std::io::Error::other(format!("S3 GetObject failed for {key}: {e}")))?;
        let bytes = resp.body.collect().await.map_err(|e| {
            std::io::Error::other(format!("S3 GetObject body read failed for {key}: {e}"))
        })?;
        let data = bytes.to_vec();
        let len = data.len() as u64;
        let dest = dest.to_path_buf();
        tokio::task::spawn_blocking(move || {
            use std::io::{Seek, SeekFrom, Write};
            let mut f = std::fs::OpenOptions::new().write(true).open(&dest)?;
            f.seek(SeekFrom::Start(offset))?;
            f.write_all(&data)?;
            Ok::<_, std::io::Error>(len)
        })
        .await
        .map_err(std::io::Error::other)?
    }
}

#[async_trait]
impl MultipartDataCache for S3DataCache {
    async fn create_multipart_upload(
        &self,
        hash: &str,
        algorithm: &str,
    ) -> std::io::Result<String> {
        let key = AsyncDataCache::object_key(self, hash, algorithm);
        let resp = self
            .client
            .create_multipart_upload()
            .bucket(&self.bucket)
            .key(&key)
            .set_expected_bucket_owner(self.expected_bucket_owner.clone())
            .send()
            .await
            .map_err(|e| {
                std::io::Error::other(format!("S3 CreateMultipartUpload failed for {key}: {e}"))
            })?;
        resp.upload_id()
            .map(|s| s.to_string())
            .ok_or_else(|| std::io::Error::other("missing upload_id"))
    }

    async fn upload_part(
        &self,
        hash: &str,
        algorithm: &str,
        upload_id: &str,
        part_number: i32,
        data: Vec<u8>,
    ) -> std::io::Result<String> {
        let key = AsyncDataCache::object_key(self, hash, algorithm);
        let body = aws_sdk_s3::primitives::ByteStream::from(data);
        let resp = self
            .client
            .upload_part()
            .bucket(&self.bucket)
            .key(&key)
            .upload_id(upload_id)
            .part_number(part_number)
            .body(body)
            .set_expected_bucket_owner(self.expected_bucket_owner.clone())
            .send()
            .await
            .map_err(|e| {
                std::io::Error::other(format!(
                    "S3 UploadPart failed for {key} part {part_number}: {e}"
                ))
            })?;
        resp.e_tag()
            .map(|s| s.to_string())
            .ok_or_else(|| std::io::Error::other("missing ETag"))
    }

    async fn complete_multipart_upload(
        &self,
        hash: &str,
        algorithm: &str,
        upload_id: &str,
        parts: Vec<(i32, String)>,
    ) -> std::io::Result<()> {
        let key = AsyncDataCache::object_key(self, hash, algorithm);
        let completed_parts: Vec<_> = parts
            .into_iter()
            .map(|(num, etag)| {
                aws_sdk_s3::types::CompletedPart::builder()
                    .part_number(num)
                    .e_tag(etag)
                    .build()
            })
            .collect();
        let upload = aws_sdk_s3::types::CompletedMultipartUpload::builder()
            .set_parts(Some(completed_parts))
            .build();
        self.client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(&key)
            .upload_id(upload_id)
            .multipart_upload(upload)
            .set_expected_bucket_owner(self.expected_bucket_owner.clone())
            .send()
            .await
            .map_err(|e| {
                std::io::Error::other(format!("S3 CompleteMultipartUpload failed for {key}: {e}"))
            })?;
        self.record_in_check_cache(hash, algorithm);
        Ok(())
    }

    async fn abort_multipart_upload(
        &self,
        hash: &str,
        algorithm: &str,
        upload_id: &str,
    ) -> std::io::Result<()> {
        let key = AsyncDataCache::object_key(self, hash, algorithm);
        self.client
            .abort_multipart_upload()
            .bucket(&self.bucket)
            .key(&key)
            .upload_id(upload_id)
            .set_expected_bucket_owner(self.expected_bucket_owner.clone())
            .send()
            .await
            .map_err(|e| {
                std::io::Error::other(format!("S3 AbortMultipartUpload failed for {key}: {e}"))
            })?;
        Ok(())
    }
}

#[async_trait]
impl RangeReadDataCache for S3DataCache {
    async fn get_object_range(
        &self,
        hash: &str,
        algorithm: &str,
        start: u64,
        end: u64,
    ) -> std::io::Result<Vec<u8>> {
        let key = AsyncDataCache::object_key(self, hash, algorithm);
        let range = format!("bytes={start}-{end}");
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .range(&range)
            .set_expected_bucket_owner(self.expected_bucket_owner.clone())
            .send()
            .await
            .map_err(|e| {
                std::io::Error::other(format!("S3 GetObject range failed for {key}: {e}"))
            })?;
        let bytes = resp.body.collect().await.map_err(|e| {
            std::io::Error::other(format!(
                "S3 GetObject range body read failed for {key}: {e}"
            ))
        })?;
        Ok(bytes.to_vec())
    }

    async fn stream_range_to_file_at_offset(
        &self,
        hash: &str,
        algorithm: &str,
        range_start: u64,
        range_end: u64,
        dest: &std::path::Path,
        file_offset: u64,
    ) -> std::io::Result<u64> {
        let key = AsyncDataCache::object_key(self, hash, algorithm);
        let range = format!("bytes={range_start}-{range_end}");
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .range(&range)
            .set_expected_bucket_owner(self.expected_bucket_owner.clone())
            .send()
            .await
            .map_err(|e| {
                std::io::Error::other(format!("S3 GetObject range failed for {key}: {e}"))
            })?;
        let bytes = resp.body.collect().await.map_err(|e| {
            std::io::Error::other(format!(
                "S3 GetObject range body read failed for {key}: {e}"
            ))
        })?;
        let data = bytes.to_vec();
        let len = data.len() as u64;
        let dest = dest.to_path_buf();
        tokio::task::spawn_blocking(move || {
            use std::io::{Seek, SeekFrom, Write};
            let mut f = std::fs::OpenOptions::new().write(true).open(&dest)?;
            f.seek(SeekFrom::Start(file_offset))?;
            f.write_all(&data)?;
            Ok::<_, std::io::Error>(len)
        })
        .await
        .map_err(std::io::Error::other)?
    }
}

#[cfg(test)]
mod cache_validation_tests {
    use super::*;

    #[test]
    fn cache_validation_first_100_always_verify() {
        let state = CacheValidationState::new();
        for _ in 0..100 {
            assert!(state.should_verify());
        }
    }

    #[test]
    fn cache_validation_after_100_probabilistic() {
        let state = CacheValidationState::new();
        // Exhaust the first 100 guaranteed verifications
        for _ in 0..100 {
            state.should_verify();
        }
        // After 100, not all should return true (1% chance each)
        let count = (0..1000).filter(|_| state.should_verify()).count();
        assert!(
            count < 1000,
            "expected some false results after 100, but all returned true"
        );
    }

    #[test]
    fn cache_validation_invalidate() {
        let state = CacheValidationState::new();
        assert!(!state.is_invalidated());
        state.invalidate();
        assert!(state.is_invalidated());
    }

    #[test]
    fn cache_validation_thread_safety() {
        use std::sync::Arc;
        let state = Arc::new(CacheValidationState::new());
        let mut handles = vec![];
        for _ in 0..8 {
            let s = Arc::clone(&state);
            handles.push(std::thread::spawn(move || {
                for _ in 0..200 {
                    s.should_verify();
                }
                s.invalidate();
                s.is_invalidated();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        // If we get here without panic, thread safety is confirmed
        assert!(state.is_invalidated());
    }
}
