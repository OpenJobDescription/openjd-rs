use crate::hash::DEFAULT_S3_MULTIPART_PART_SIZE;
use crate::s3_check_cache::S3CheckCache;
use std::any::Any;
use std::path::PathBuf;
use std::sync::Arc;

/// Content-addressed data cache abstraction.
/// Files are stored with keys derived from their content hash: `{hash}.{algorithm}`.
pub trait ContentAddressedDataCache: Send + Sync {
    /// Returns the storage key/path for a given hash.
    fn object_key(&self, hash: &str, algorithm: &str) -> String;
    /// Checks if an object with the given hash already exists.
    fn object_exists(&self, hash: &str, algorithm: &str) -> std::io::Result<bool>;
    /// Writes data to the cache. Returns the key.
    fn put_object(&self, hash: &str, algorithm: &str, data: &[u8]) -> std::io::Result<String>;
    /// Reads data from the cache.
    fn get_object(&self, hash: &str, algorithm: &str) -> std::io::Result<Vec<u8>>;
}

use async_trait::async_trait;

/// Result of a copy_from attempt.
#[derive(Debug, PartialEq)]
pub enum CopyResult {
    /// Server-side copy completed (no data transited through client)
    ServerSideCopy,
    /// Not supported for this cache combination; caller should fall back to get+put
    NotSupported,
}

/// Async content-addressed data cache for use in tokio pipelines.
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
    fn multipart_part_size(&self) -> usize {
        crate::hash::DEFAULT_S3_MULTIPART_PART_SIZE
    }

    // Multipart upload
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

    // Byte-range download
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

impl ContentAddressedDataCache for FileSystemDataCache {
    fn object_key(&self, hash: &str, algorithm: &str) -> String {
        self.object_path(hash, algorithm)
            .to_string_lossy()
            .into_owned()
    }

    fn object_exists(&self, hash: &str, algorithm: &str) -> std::io::Result<bool> {
        Ok(self.object_path(hash, algorithm).exists())
    }

    fn put_object(&self, hash: &str, algorithm: &str, data: &[u8]) -> std::io::Result<String> {
        let path = self.object_path(hash, algorithm);
        std::fs::write(&path, data)?;
        Ok(path.to_string_lossy().into_owned())
    }

    fn get_object(&self, hash: &str, algorithm: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.object_path(hash, algorithm))
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

    async fn create_multipart_upload(
        &self,
        _hash: &str,
        _algorithm: &str,
    ) -> std::io::Result<String> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "multipart not supported for filesystem",
        ))
    }
    async fn upload_part(
        &self,
        _hash: &str,
        _algorithm: &str,
        _upload_id: &str,
        _part_number: i32,
        _data: Vec<u8>,
    ) -> std::io::Result<String> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "multipart not supported for filesystem",
        ))
    }
    async fn complete_multipart_upload(
        &self,
        _hash: &str,
        _algorithm: &str,
        _upload_id: &str,
        _parts: Vec<(i32, String)>,
    ) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "multipart not supported for filesystem",
        ))
    }
    async fn abort_multipart_upload(
        &self,
        _hash: &str,
        _algorithm: &str,
        _upload_id: &str,
    ) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "multipart not supported for filesystem",
        ))
    }
    async fn get_object_range(
        &self,
        _hash: &str,
        _algorithm: &str,
        _start: u64,
        _end: u64,
    ) -> std::io::Result<Vec<u8>> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "range get not supported for filesystem",
        ))
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

    #[test]
    fn put_and_get_object() {
        let tmp = TempDir::new().unwrap();
        let cache = FileSystemDataCache::new(tmp.path().join("cache")).unwrap();
        ContentAddressedDataCache::put_object(&cache, "abc123", "xxh128", b"hello").unwrap();
        let data = ContentAddressedDataCache::get_object(&cache, "abc123", "xxh128").unwrap();
        assert_eq!(data, b"hello");
    }

    #[test]
    fn object_exists_check() {
        let tmp = TempDir::new().unwrap();
        let cache = FileSystemDataCache::new(tmp.path().join("cache")).unwrap();
        assert!(!ContentAddressedDataCache::object_exists(&cache, "abc123", "xxh128").unwrap());
        ContentAddressedDataCache::put_object(&cache, "abc123", "xxh128", b"data").unwrap();
        assert!(ContentAddressedDataCache::object_exists(&cache, "abc123", "xxh128").unwrap());
    }

    #[test]
    fn object_key_format() {
        let tmp = TempDir::new().unwrap();
        let cache = FileSystemDataCache::new(tmp.path().join("cache")).unwrap();
        let key = ContentAddressedDataCache::object_key(&cache, "abc123", "xxh128");
        assert!(key.ends_with("abc123.xxh128"));
    }

    #[test]
    fn get_nonexistent_returns_error() {
        let tmp = TempDir::new().unwrap();
        let cache = FileSystemDataCache::new(tmp.path().join("cache")).unwrap();
        assert!(ContentAddressedDataCache::get_object(&cache, "missing", "xxh128").is_err());
    }

    #[tokio::test]
    async fn copy_from_default_returns_not_supported() {
        let tmp = TempDir::new().unwrap();
        let src = FileSystemDataCache::new(tmp.path().join("src")).unwrap();
        let dst = FileSystemDataCache::new(tmp.path().join("dst")).unwrap();
        let result = dst.copy_from(&src, "abc", "xxh128").await.unwrap();
        assert_eq!(result, CopyResult::NotSupported);
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
    pub bucket: String,
    pub key_prefix: String,
    pub client: aws_sdk_s3::Client,
    pub multipart_part_size: usize,
    pub s3_check_cache: Option<Arc<S3CheckCache>>,
    pub force_s3_check: bool,
    pub expected_bucket_owner: Option<String>,
    pub cache_validation: CacheValidationState,
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
        }
    }

    /// Returns the cache key for a given hash: "{bucket}/{prefix}/{hash}.{algorithm}"
    pub fn cache_key(&self, hash: &str, algorithm: &str) -> String {
        format!("{}/{}", self.bucket, self.object_key(hash, algorithm))
    }

    /// Create with auto-detected account ID from AWS credentials.
    pub async fn new_with_auto_account_id(
        bucket: String,
        key_prefix: String,
        s3_client: aws_sdk_s3::Client,
        sts_client: aws_sdk_sts::Client,
    ) -> crate::Result<Self> {
        let resp = sts_client.get_caller_identity().send().await.map_err(|e| {
            crate::SnapshotError::Other(format!("STS GetCallerIdentity failed: {e}"))
        })?;
        let account = resp
            .account()
            .ok_or_else(|| crate::SnapshotError::Other("STS response missing Account".into()))?
            .to_string();
        let mut cache = Self::new(bucket, key_prefix, s3_client);
        cache.expected_bucket_owner = Some(account);
        Ok(cache)
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
}

/// Run a future to completion, handling the case where we're already inside a tokio runtime.
fn block_on_async<F: std::future::Future>(f: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(f)),
        Err(_) => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to create tokio runtime");
            rt.block_on(f)
        }
    }
}

impl ContentAddressedDataCache for S3DataCache {
    fn object_key(&self, hash: &str, algorithm: &str) -> String {
        format!("{}/{hash}.{algorithm}", self.key_prefix)
    }

    fn object_exists(&self, hash: &str, algorithm: &str) -> std::io::Result<bool> {
        if self.check_cache_exists(hash, algorithm) {
            return Ok(true);
        }
        block_on_async(AsyncDataCache::object_exists(self, hash, algorithm))
    }

    fn put_object(&self, hash: &str, algorithm: &str, data: &[u8]) -> std::io::Result<String> {
        block_on_async(AsyncDataCache::put_object(
            self,
            hash,
            algorithm,
            data.to_vec(),
        ))
    }

    fn get_object(&self, hash: &str, algorithm: &str) -> std::io::Result<Vec<u8>> {
        block_on_async(AsyncDataCache::get_object(self, hash, algorithm))
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
        let copy_source = format!("{}/{}", src_s3.bucket, src_key);
        self.client
            .copy_object()
            .bucket(&self.bucket)
            .key(&dst_key)
            .copy_source(&copy_source)
            .set_expected_bucket_owner(self.expected_bucket_owner.clone())
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
        let mut body = resp.body;
        let dest = dest.to_path_buf();
        // Stream chunks to file without buffering the entire response
        let (tx, mut rx) = tokio::sync::mpsc::channel::<bytes::Bytes>(4);
        let writer = tokio::task::spawn_blocking(move || {
            use std::io::Write;
            let mut f = std::fs::File::create(&dest)?;
            let mut total: u64 = 0;
            while let Some(chunk) = rx.blocking_recv() {
                f.write_all(&chunk)?;
                total += chunk.len() as u64;
            }
            Ok::<_, std::io::Error>(total)
        });
        while let Some(chunk) = body
            .try_next()
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?
        {
            let _ = tx.send(chunk).await;
        }
        drop(tx);
        writer.await.map_err(std::io::Error::other)?
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
        let mut body = resp.body;
        let dest = dest.to_path_buf();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<bytes::Bytes>(4);
        let writer = tokio::task::spawn_blocking(move || {
            use std::io::{Seek, SeekFrom, Write};
            let mut f = std::fs::OpenOptions::new().write(true).open(&dest)?;
            f.seek(SeekFrom::Start(offset))?;
            let mut total: u64 = 0;
            while let Some(chunk) = rx.blocking_recv() {
                f.write_all(&chunk)?;
                total += chunk.len() as u64;
            }
            Ok::<_, std::io::Error>(total)
        });
        while let Some(chunk) = body
            .try_next()
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?
        {
            let _ = tx.send(chunk).await;
        }
        drop(tx);
        writer.await.map_err(std::io::Error::other)?
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
        let mut body = resp.body;
        let dest = dest.to_path_buf();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<bytes::Bytes>(4);
        let writer = tokio::task::spawn_blocking(move || {
            use std::io::{Seek, SeekFrom, Write};
            let mut f = std::fs::OpenOptions::new().write(true).open(&dest)?;
            f.seek(SeekFrom::Start(file_offset))?;
            let mut total: u64 = 0;
            while let Some(chunk) = rx.blocking_recv() {
                f.write_all(&chunk)?;
                total += chunk.len() as u64;
            }
            Ok::<_, std::io::Error>(total)
        });
        while let Some(chunk) = body
            .try_next()
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?
        {
            let _ = tx.send(chunk).await;
        }
        drop(tx);
        writer.await.map_err(std::io::Error::other)?
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

#[cfg(test)]
mod block_on_async_tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn block_on_async_without_runtime() {
        let result = block_on_async(async { 42 });
        assert_eq!(result, 42);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn block_on_async_inside_runtime() {
        let result = block_on_async(async { 42 });
        assert_eq!(result, 42);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sync_trait_inside_runtime() {
        let tmp = TempDir::new().unwrap();
        let cache = FileSystemDataCache::new(tmp.path().join("cache")).unwrap();

        // These calls use the sync ContentAddressedDataCache trait.
        // Before the fix, calling from within a tokio runtime would panic with
        // "Cannot start a runtime from within a runtime" for S3DataCache.
        // FileSystemDataCache doesn't go through block_on_async, but this
        // validates the sync trait works inside an async context.
        ContentAddressedDataCache::put_object(&cache, "abc", "xxh128", b"hello").unwrap();
        assert!(ContentAddressedDataCache::object_exists(&cache, "abc", "xxh128").unwrap());
        let data = ContentAddressedDataCache::get_object(&cache, "abc", "xxh128").unwrap();
        assert_eq!(data, b"hello");
    }
}
