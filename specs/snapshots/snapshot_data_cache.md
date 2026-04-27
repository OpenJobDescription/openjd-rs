# Content-Addressed Data Cache

[README](README.md) · Data Cache

**Location:** `data_cache.rs`

## Overview

A content-addressed data cache stores data using the content's hash as the key, enabling deduplication, efficient retrieval, and integrity verification. The crate provides four trait abstractions and two concrete implementations:

```
ContentAddressedDataCache (sync trait)
AsyncDataCache            (core async trait, used by HASH_UPLOAD, DOWNLOAD, CACHE_SYNC)
├── MultipartDataCache    (extension trait: S3-style multipart upload)
└── RangeReadDataCache    (extension trait: byte-range reads)

S3DataCache           - implements all three async traits (multipart + range reads)
FileSystemDataCache   - implements only AsyncDataCache (no multipart, no range reads)
```

Callers who need multipart or range capabilities discover them at runtime via
`AsyncDataCache::as_multipart()` / `AsyncDataCache::as_range_read()`, which return
`Some(&dyn MultipartDataCache)` / `Some(&dyn RangeReadDataCache)` when the backing
cache supports the capability. The default implementation returns `None`, so
backends opt in by overriding these accessors.

## Storage Key Format

Files are stored with keys derived from their content hash:

```
{prefix}/{hash}.{algorithm}
```

Examples:
- S3: `Data/a1b2c3d4e5f67890abcdef1234567890.xxh128`
- Filesystem: `/mnt/cache/a1b2c3d4e5f67890abcdef1234567890.xxh128`

## Design Principles

### Hash-Then-Upload Invariant

HASH_UPLOAD always computes the hash while reading data for upload, never uploading based on a pre-computed hash alone. This guarantees the content-addressed storage invariant: data stored for a hash key always equals its hash.

**Large file exception:** When a file exceeds `max_memory_bytes` with `WHOLE_FILE_CHUNK_SIZE` mode, a two-pass approach is used. Per-part hashes are re-verified during the upload pass; mismatches abort the upload.

### Existence Checking Strategy

**S3DataCache:**
1. Check local `S3CheckCache` (SQLite database of known-existing hashes)
2. If miss, make `HeadObject` API call to S3
3. Cache positive results for future checks

**FileSystemDataCache:**
- Direct filesystem `exists()` check (no caching)

## Sync Trait: `ContentAddressedDataCache`

```rust
pub trait ContentAddressedDataCache: Send + Sync {
    fn object_key(&self, hash: &str, algorithm: &str) -> String;
    fn object_exists(&self, hash: &str, algorithm: &str) -> std::io::Result<bool>;
    fn put_object(&self, hash: &str, algorithm: &str, data: &[u8]) -> std::io::Result<String>;
    fn get_object(&self, hash: &str, algorithm: &str) -> std::io::Result<Vec<u8>>;
}
```

## Async Trait: `AsyncDataCache`

Core async trait used by HASH_UPLOAD, DOWNLOAD, and CACHE_SYNC pipelines. Every async
cache backend implements this trait. Backends that additionally support S3-style
multipart upload or byte-range reads implement the extension traits
[`MultipartDataCache`](#multipart-extension-trait-multipartdatacache) and
[`RangeReadDataCache`](#range-read-extension-trait-rangereaddatacache), and override
`as_multipart` / `as_range_read` so callers can discover the capability through a
trait object.

```rust
#[async_trait]
pub trait AsyncDataCache: Send + Sync {
    fn object_key(&self, hash: &str, algorithm: &str) -> String;
    fn as_any(&self) -> &dyn Any;
    async fn object_exists(&self, hash: &str, algorithm: &str) -> std::io::Result<bool>;
    async fn put_object(&self, hash: &str, algorithm: &str, data: Vec<u8>) -> std::io::Result<String>;
    async fn get_object(&self, hash: &str, algorithm: &str) -> std::io::Result<Vec<u8>>;

    // Server-side copy (default: NotSupported)
    async fn copy_from(&self, source: &dyn AsyncDataCache, hash: &str, algorithm: &str)
        -> std::io::Result<CopyResult>;

    fn multipart_part_size(&self) -> usize;  // Default: 32MB

    // Capability discovery — default: None
    fn as_multipart(&self) -> Option<&dyn MultipartDataCache>;
    fn as_range_read(&self) -> Option<&dyn RangeReadDataCache>;

    // Streaming helpers (with default implementations using get_object)
    async fn copy_object_to_file(/* ... */) -> std::io::Result<u64>;
    async fn write_object_to_file_at_offset(/* ... */) -> std::io::Result<u64>;
}
```

### Multipart extension trait: `MultipartDataCache`

```rust
#[async_trait]
pub trait MultipartDataCache: AsyncDataCache {
    async fn create_multipart_upload(&self, hash: &str, algorithm: &str) -> std::io::Result<String>;
    async fn upload_part(&self, hash: &str, algorithm: &str, upload_id: &str,
                         part_number: i32, data: Vec<u8>) -> std::io::Result<String>;
    async fn complete_multipart_upload(&self, hash: &str, algorithm: &str, upload_id: &str,
                                       parts: Vec<(i32, String)>) -> std::io::Result<()>;
    async fn abort_multipart_upload(&self, hash: &str, algorithm: &str,
                                    upload_id: &str) -> std::io::Result<()>;
}
```

### Range-read extension trait: `RangeReadDataCache`

```rust
#[async_trait]
pub trait RangeReadDataCache: AsyncDataCache {
    async fn get_object_range(&self, hash: &str, algorithm: &str,
                              start: u64, end: u64) -> std::io::Result<Vec<u8>>;

    // Stream a byte range directly to a file (default uses get_object_range + write)
    async fn stream_range_to_file_at_offset(/* ... */) -> std::io::Result<u64>;
}
```

The streaming helpers on `AsyncDataCache` (`copy_object_to_file`,
`write_object_to_file_at_offset`) have default implementations that read into memory
then write, so they work for any backend. `S3DataCache` and `FileSystemDataCache`
override them for efficiency.

## FileSystemDataCache

Content-addressed storage backed by a local or network filesystem.

```rust
pub struct FileSystemDataCache {
    pub root_path: PathBuf,  // Must be absolute
}
```

### Construction

```rust
let cache = FileSystemDataCache::new("/tmp/debug_snapshot/Data")?;
// Validates root_path is absolute, creates directory if needed
```

### Behavior

- `object_key()` → `{root_path}/{hash}.{algorithm}`
- `object_exists()` → filesystem `exists()` check
- `put_object()` → write file to `{root_path}/{hash}.{algorithm}`
- `get_object()` → read file from cache
- Does not implement `MultipartDataCache` or `RangeReadDataCache`; `as_multipart()`
  and `as_range_read()` both return `None`. Callers that need these capabilities
  must fall back to `get_object` + `put_object`.

### Use Cases

- Debug snapshots: create portable zip files with manifest + data
- Local testing: test upload/download logic without S3
- Network storage: NFS or SMB mounted paths as shared caches

## S3DataCache

Content-addressed storage backed by Amazon S3.

```rust
pub struct S3DataCache { /* private fields */ }

impl S3DataCache {
    pub fn new(bucket: String, key_prefix: String, client: aws_sdk_s3::Client) -> Self;
    pub async fn new_with_auto_account_id(
        bucket: String, key_prefix: String,
        s3_client: aws_sdk_s3::Client, sts_client: aws_sdk_sts::Client,
    ) -> Result<Self>;

    // Consuming builder-style setters for optional configuration
    pub fn with_multipart_part_size(self, size: usize) -> Self; // Default: 32MB
    pub fn with_s3_check_cache(self, cache: Option<Arc<S3CheckCache>>) -> Self;
    pub fn with_force_s3_check(self, force: bool) -> Self;
    pub fn with_expected_bucket_owner(self, owner: Option<String>) -> Self;
}
```

All configuration fields are private; construct with `new` or `new_with_auto_account_id`, then chain `with_*` setters for optional state.

### Account ID and Security

The `account_id` controls `ExpectedBucketOwner` on S3 API calls, preventing confused deputy attacks:

| Value | Behavior |
|-------|----------|
| `AccountId::Auto(id)` | Auto-detected from STS at construction |
| `AccountId::Explicit(id)` | Use provided account ID |
| `AccountId::NoCheck` | Disable ExpectedBucketOwner checks |

### Multipart Transfers

Files larger than `2 × multipart_part_size` (default 64MB) use S3 multipart upload/download with parallel parts.

### S3 Check Cache

Local SQLite database tracking `(s3_key)` tuples known to exist in S3. Entries expire after 30 days. Avoids redundant `HeadObject` calls (~15ms each).

## Cache Hierarchy

| Cache | Scope | Purpose |
|-------|-------|---------|
| `HashCache` | Local filesystem | Maps (path, mtime, byte range) → hash to skip re-hashing |
| `S3CheckCache` | Per S3 bucket | Tracks which hashes exist in S3 to skip HeadObject calls |
| `AsyncDataCache` | Remote storage | The actual content storage (S3 or filesystem) |

**Upload flow with all caches:**
1. `HashCache` hit → skip reading file, use cached hash
2. `S3CheckCache` hit → skip HeadObject, assume exists
3. `HeadObject` confirms existence → skip upload
4. Otherwise → read, hash, upload

**Download flow with hash cache:**
1. Local file exists with matching mtime in `HashCache`
2. Cached hash matches manifest hash → skip download
3. Otherwise → download from data cache

**Cache sync flow:**
1. `destination.object_exists(hash, alg)` → skip if present
2. Otherwise → `source.get_object` → `destination.put_object`
