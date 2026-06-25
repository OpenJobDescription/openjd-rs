# Public API

[README](README.md) · Public API

> **Status: experimental.** The `openjd-snapshots` crate's public API
> may change without notice. See
> [snapshot_overview.md](snapshot_overview.md#status) for on-disk format
> status (v2023 stable, v2025 experimental draft).

This document is the authoritative reference for the `openjd-snapshots` crate's public API. All public types, functions, traits, and constants are listed here with their signatures.

Items are organized by where they are re-exported. Most items are re-exported at the crate root (e.g., `openjd_snapshots::FileEntry`). Items only accessible via module paths are noted with their full path.

## Constants

```rust
pub const DEFAULT_FILE_CHUNK_SIZE: i64 = 256 * 1024 * 1024;   // 256 MB
pub const WHOLE_FILE_CHUNK_SIZE: i64 = -1;                      // Sentinel: no chunking
pub const DEFAULT_S3_MULTIPART_PART_SIZE: usize = 32 * 1024 * 1024; // 32 MB
```

Module-path only:

```rust
// openjd_snapshots::hash_cache::WHOLE_FILE_RANGE_END
pub const WHOLE_FILE_RANGE_END: i64 = -1;  // Sentinel for hash cache range_end (whole-file hash)
```

## Error Types

```rust
#[non_exhaustive]
pub enum SnapshotError {
    Io(std::io::Error),
    Validation(String),
    FileNotFound(String),
    Cancelled,
    Cache(String),
    S3(String),
    Task(String),
}

pub type Result<T> = std::result::Result<T, SnapshotError>;
```

## Hashing

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HashAlgorithm {
    #[serde(rename = "xxh128")]
    Xxh128,
}

impl HashAlgorithm {
    pub fn extension(&self) -> &'static str;
}
```

Implements `Display` (returns the extension string, e.g., `"xxh128"`).

The following hash functions are public but only accessible via the `hash` module path (e.g., `openjd_snapshots::hash::hash_data`):

```rust
pub fn hash_data(data: &[u8]) -> String;
pub fn hash_file(path: &Path) -> std::io::Result<String>;
pub fn hash_file_chunked(
    path: &Path,
    chunk_size: u64,
    expected_size: u64,
) -> std::io::Result<Vec<String>>;
```

`expected_size` is mandatory on `hash_file_chunked`. Content-addressed
hashing requires the file on disk to match what the manifest claims,
so the function checks the actual file size against `expected_size`
before hashing and returns `io::ErrorKind::InvalidData` on mismatch.
This lets callers detect file-size drift (for example, a file that
was truncated or extended after being recorded in a manifest) without
a separate stat call, and guarantees that the returned chunk hashes
describe exactly the bytes the caller intended to hash.

Re-exported at the crate root:

```rust
pub fn human_readable_file_size(bytes: u64) -> String;
```

## Manifest Types

### Phantom Type Markers

These are accessible via the `manifest` module path (e.g., `openjd_snapshots::manifest::Abs`):

```rust
pub struct Abs;
pub struct Rel;
pub struct Full;
pub struct Diff;
```

### Manifest Struct

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest<P, K> {
    pub hash_alg: HashAlgorithm,
    pub files: Vec<FileEntry>,
    pub dirs: Vec<DirEntry>,
    pub total_size: u64,
    pub parent_manifest_hash: Option<String>,
    pub file_chunk_size_bytes: i64,
}
```

#### Type Aliases

```rust
pub type AbsSnapshot     = Manifest<Abs, Full>;
pub type AbsSnapshotDiff = Manifest<Abs, Diff>;
pub type Snapshot         = Manifest<Rel, Full>;
pub type SnapshotDiff     = Manifest<Rel, Diff>;
```

#### Methods (all `Manifest<P, K>`)

```rust
pub fn new(hash_alg: HashAlgorithm, file_chunk_size_bytes: i64) -> Self;
pub fn with_files(self, files: Vec<FileEntry>) -> Self;
pub fn with_dirs(self, dirs: Vec<DirEntry>) -> Self;
pub fn with_parent_hash(self, hash: Option<String>) -> Self;
pub fn clear_hashes(&mut self);
pub fn recompute_total_size(&mut self);
pub fn common_root(&self) -> Option<String>;
```

`common_root` returns the longest directory prefix that is an ancestor (or
equal to) every meaningful non-deleted path in the manifest. Files contribute
their parent directory; empty dirs contribute themselves; non-empty dirs
(those with a strict descendant declared elsewhere in the manifest) are
ignored so that a dir entry like `/` does not collapse the result toward
root when it merely contains other declared entries. Returns `None` if the
manifest is empty (or all entries are deleted). Composes with
[`subtree_snapshot`] to extract a relative-path subtree from an absolute
manifest without the caller needing to know a root path.

#### Methods (where `P: ValidatePaths, K: ValidateKind`)

```rust
pub fn validate(&self) -> Result<()>;
```

### FileEntry

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub hash: Option<String>,
    pub size: Option<u64>,
    pub mtime: Option<u64>,
    pub chunk_hashes: Option<Vec<String>>,   // serialized as "chunkhashes"
    pub symlink_target: Option<String>,
    pub runnable: bool,
    pub deleted: bool,
}

impl FileEntry {
    pub fn new(path: impl Into<String>) -> Self;
    pub fn file(path: impl Into<String>, size: u64, mtime: u64) -> Self;
    pub fn symlink(path: impl Into<String>, target: impl Into<String>) -> Self;
    pub fn deleted(path: impl Into<String>) -> Self;
}
```

Implements `Display`.

### DirEntry

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DirEntry {
    pub path: String,
    pub deleted: bool,
}

impl DirEntry {
    pub fn new(path: impl Into<String>) -> Self;
    pub fn deleted(path: impl Into<String>) -> Self;
}
```

Implements `Display`.

### SymlinkPolicy

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymlinkPolicy {
    CollapseEscaping,
    CollapseAll,
    ExcludeEscaping,
    ExcludeAll,
    Preserve,
    TransitiveIncludeTargets,
}
```

Implements `Display`.

### Enum Wrappers

```rust
#[derive(Debug)]
pub enum AbsManifest {
    Snapshot(AbsSnapshot),
    Diff(AbsSnapshotDiff),
}

#[derive(Debug)]
pub enum RelManifest {
    Snapshot(Snapshot),
    Diff(SnapshotDiff),
}
```

Both provide accessor methods: `files()`, `dirs()`, `hash_alg()`, `file_chunk_size_bytes()`, `total_size()`, `parent_manifest_hash()`.

Both implement `ManifestRef`.

### ManifestEntry

```rust
pub enum ManifestEntry<'a> {
    File(&'a FileEntry),
    Dir(&'a DirEntry),
}

impl<'a> ManifestEntry<'a> {
    pub fn path(&self) -> &str;
}
```

### Validation Traits

Accessible via the `manifest` module path (e.g., `openjd_snapshots::manifest::ValidatePaths`):

```rust
pub trait ValidatePaths {
    fn validate_path(path: &str) -> Result<()>;
}

pub trait ValidateKind {
    fn validate_deleted(deleted: bool) -> Result<()>;
}
```

Implemented by `Abs`, `Rel` (for `ValidatePaths`) and `Full`, `Diff` (for `ValidateKind`).

### ManifestRef Trait

```rust
pub trait ManifestRef {
    fn files(&self) -> &[FileEntry];
    fn hash_alg(&self) -> HashAlgorithm;
    fn file_chunk_size_bytes(&self) -> i64;
}
```

Implemented by `AbsManifest` and `RelManifest`.

## Codec

### Types

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManifestFormat {
    V2023,
    V2025,
}

pub enum DecodedManifest {
    AbsSnapshot(AbsSnapshot),
    AbsSnapshotDiff(AbsSnapshotDiff),
    Snapshot(Snapshot),
    SnapshotDiff(SnapshotDiff),
}
```

### Encode Functions

```rust
pub fn encode_snapshot_v2023(manifest: &Snapshot) -> Result<String>;
pub fn encode_snapshot_diff_v2023(manifest: &SnapshotDiff) -> Result<String>;
pub fn encode_abs_snapshot_v2025(m: &AbsSnapshot) -> Result<String>;
pub fn encode_abs_snapshot_diff_v2025(m: &AbsSnapshotDiff) -> Result<String>;
pub fn encode_snapshot_v2025(m: &Snapshot) -> Result<String>;
pub fn encode_snapshot_diff_v2025(m: &SnapshotDiff) -> Result<String>;
```

The generic `encode_v2025` is also public via the `codec` module path:

```rust
// openjd_snapshots::codec::encode_v2025
pub fn encode_v2025<P: Clone + Debug, K: Clone + Debug>(
    manifest: &Manifest<P, K>, spec_version: &str,
) -> Result<String>;
```

### Decode Functions

```rust
pub fn decode_manifest(json: &str) -> Result<DecodedManifest>;
pub fn decode_v2023(json: &str) -> Result<Snapshot>;
pub fn decode_v2023_as_diff(json: &str) -> Result<SnapshotDiff>;
pub fn decode_v2025(json: &str) -> Result<DecodedManifest>;
```

The v2023 format has no structural distinction between a full snapshot and
a diff. `decode_v2023` and `decode_v2023_as_diff` return the same data
structurally; they differ only in the phantom `Kind` parameter of the
returned manifest. `decode_v2023_as_diff` is a convenience for callers that
want to feed a v2023-formatted manifest into a diff-consuming pipeline like
[`compose_snapshot_with_diffs`] without reconstructing the manifest.

## Data Cache

### Async Trait

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

    // Streaming helpers (with default implementations)
    async fn copy_object_to_file(&self, hash: &str, algorithm: &str,
        dest: &Path) -> std::io::Result<u64>;
    async fn write_object_to_file_at_offset(&self, hash: &str, algorithm: &str,
        dest: &Path, offset: u64) -> std::io::Result<u64>;
}
```

### Multipart Extension Trait

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

### Range-Read Extension Trait

```rust
#[async_trait]
pub trait RangeReadDataCache: AsyncDataCache {
    async fn get_object_range(&self, hash: &str, algorithm: &str,
                              start: u64, end: u64) -> std::io::Result<Vec<u8>>;
    async fn stream_range_to_file_at_offset(&self, hash: &str, algorithm: &str,
        range_start: u64, range_end: u64, dest: &Path, file_offset: u64) -> std::io::Result<u64>;
}
```

### CopyResult

```rust
#[derive(Debug, PartialEq)]
pub enum CopyResult {
    ServerSideCopy,
    NotSupported,
}
```

### FileSystemDataCache

```rust
pub struct FileSystemDataCache {
    pub root_path: PathBuf,
}

impl FileSystemDataCache {
    pub fn new(root_path: impl Into<PathBuf>) -> Result<Self>;
}
```

Implements `AsyncDataCache`. Does not implement
`MultipartDataCache` or `RangeReadDataCache`, so `as_multipart()` and
`as_range_read()` return `None`.

### S3DataCache

```rust
pub struct S3DataCache { /* private fields */ }

impl S3DataCache {
    pub fn new(bucket: String, key_prefix: String, client: aws_sdk_s3::Client) -> Self;
    pub async fn new_with_auto_account_id(
        bucket: String, key_prefix: String,
        s3_client: aws_sdk_s3::Client, sts_client: aws_sdk_sts::Client,
    ) -> Result<Self>;

    // Consuming builder-style setters
    pub fn with_multipart_part_size(self, size: usize) -> Self;
    pub fn with_s3_check_cache(self, cache: Option<Arc<S3CheckCache>>) -> Self;
    pub fn with_force_s3_check(self, force: bool) -> Self;
    pub fn with_expected_bucket_owner(self, owner: Option<String>) -> Self;
    pub fn with_copy_config(self, config: S3CopyConfig) -> Self;

    // Accessors
    pub fn client(&self) -> &aws_sdk_s3::Client;
    pub fn expected_bucket_owner(&self) -> Option<&str>;
    pub fn copy_config(&self) -> Option<&S3CopyConfig>;
    pub fn cache_key(&self, hash: &str, algorithm: &str) -> String;
    pub fn check_cache_exists(&self, hash: &str, algorithm: &str) -> bool;
    pub fn record_in_check_cache(&self, hash: &str, algorithm: &str);
    pub fn is_cache_validation_invalidated(&self) -> bool;
}
```

Implements `AsyncDataCache` (with `copy_from` for S3-to-S3 server-side copy),
`MultipartDataCache`, and `RangeReadDataCache`. By default `copy_from` performs
a single `CopyObject` with no extra `HeadObject`. Calling `with_copy_config`
opts in to size-aware routing: `copy_from` then issues one `HeadObject` and uses
parallel multipart `UploadPartCopy` for sources above the configured threshold
(required both to copy sources > 5 GiB and to parallelize large cross-region
copies).

### S3CopyConfig

`TransferConfig`-style tuning for `S3DataCache::copy_from`. Used to control when
a server-side copy switches from a single `CopyObject` to a parallel multipart
`UploadPartCopy`, and how that multipart copy is sized and parallelized.

```rust
pub struct S3CopyConfig {
    /// Sources strictly larger than this use multipart UploadPartCopy.
    /// Default 5 MiB — a performance cutoff, since parallel multipart copy
    /// beats single CopyObject for cross-region transfers at every size at or
    /// above S3's 5 MiB minimum part size. Clamped to 5 GiB at copy time (the
    /// CopyObject ceiling).
    pub multipart_threshold: u64,
    /// Target size of each UploadPartCopy part. Default 5 MiB (the S3 minimum
    /// part size, which maximizes parallelism). Grown automatically to keep
    /// the part count <= 10,000 and clamped to the S3 part-size range
    /// (5 MiB..=5 GiB).
    pub part_size: u64,
    /// Maximum number of concurrent UploadPartCopy requests per object.
    /// Default 64.
    pub max_concurrency: usize,
}

pub const S3_MAX_SINGLE_COPY_BYTES: u64 = 5 * 1024 * 1024 * 1024;
```

Implements `Default` (5 MiB threshold, 5 MiB parts, concurrency 64) — the
empirically validated cross-region copy settings.

### CacheValidationState

Accessible via the `data_cache` module path (e.g., `openjd_snapshots::data_cache::CacheValidationState`):

```rust
pub struct CacheValidationState { /* ... */ }

impl CacheValidationState {
    pub fn new() -> Self;
    pub fn is_invalidated(&self) -> bool;
}
```

Implements `Default`.

## Hash Cache

```rust
pub struct HashCache { /* Mutex<rusqlite::Connection> */ }

impl HashCache {
    pub fn new(cache_dir: impl AsRef<Path>) -> Result<Self>;
    pub fn open_default() -> Result<Self>;
    pub fn get(&self, file_path: &Path, algorithm: &str,
               range_start: i64, range_end: i64) -> Option<(String, u64)>;
    pub fn put(&self, file_path: &Path, algorithm: &str,
               range_start: i64, range_end: i64, hash: &str, mtime: u64) -> Result<()>;
    pub fn get_if_fresh(&self, file_path: &Path, algorithm: &str,
                        range_start: i64, range_end: i64, current_mtime: u64) -> Option<String>;
}
```

## S3 Check Cache

```rust
pub struct S3CheckCache { /* Mutex<rusqlite::Connection> */ }

impl S3CheckCache {
    pub fn new(cache_dir: impl AsRef<Path>) -> Result<Self>;
    pub fn open_default() -> Result<Self>;
    pub fn get_entry(&self, s3_key: &str) -> Option<String>;
    pub fn put_entry(&self, s3_key: &str) -> Result<()>;
}
```

## Operations

### Progress Callback Type

Accessible via the `ops` module path (e.g., `openjd_snapshots::ops::ProgressFn`):

```rust
pub type ProgressFn<S> = dyn Fn(&S) -> bool + Send + Sync;
```

### COLLECT

```rust
pub fn collect_abs_snapshot(
    directories: &[impl AsRef<Path>],
    filenames: &[impl AsRef<Path>],
    options: CollectOptions,
) -> Result<AbsSnapshot>
```

```rust
#[derive(Debug, Clone)]
pub struct CollectOptions {
    pub optional_filenames: Vec<PathBuf>,
    pub symlink_policy: SymlinkPolicy,          // Default: CollapseEscaping
    pub file_chunk_size_bytes: Option<i64>,      // Default: None (= DEFAULT_FILE_CHUNK_SIZE)
}
```

Implements `Default`.

### HASH

```rust
// Concrete-typed entry points (preserve the input manifest type in the result)
pub fn hash_abs_snapshot(
    manifest: &AbsSnapshot,
    options: HashOptions,
) -> Result<HashResult<AbsSnapshot>>;

pub fn hash_abs_snapshot_diff(
    manifest: &AbsSnapshotDiff,
    options: HashOptions,
) -> Result<HashResult<AbsSnapshotDiff>>;

// Enum-dispatching entry point (use when the caller holds an `AbsManifest`)
pub fn hash_abs_manifest(
    manifest: &AbsManifest,
    options: HashOptions,
) -> Result<HashResult<AbsManifest>>;
```

```rust
pub struct HashOptions {
    pub hash_cache: Option<Arc<HashCache>>,
    pub force_rehash: bool,
    pub file_chunk_size_bytes: Option<i64>,
    pub on_progress: Option<Box<dyn Fn(&HashStatistics) -> bool + Send + Sync>>,
    pub max_workers: Option<usize>,
}

/// `M` defaults to `AbsManifest`, so bare `HashResult` refers to the
/// enum-dispatching shape returned by `hash_abs_manifest`.
pub struct HashResult<M = AbsManifest> {
    pub manifest: M,
    pub statistics: HashStatistics,
}

pub struct HashStatistics {
    pub total_files: usize,
    pub total_bytes: u64,
    pub hashed_files: usize,
    pub hashed_bytes: u64,
    pub skipped_files: usize,
    pub skipped_bytes: u64,
    pub total_time: f64,
    pub rate: f64,
    pub progress: f64,
    pub progress_message: String,
}
```

`HashOptions` and `HashStatistics` implement `Default`.

### HASH_UPLOAD

```rust
pub async fn hash_upload_abs_manifest(
    manifest: &AbsManifest,
    data_cache: Arc<dyn AsyncDataCache>,
    options: HashUploadOptions,
) -> Result<UploadResult>
```

```rust
pub struct HashUploadOptions {
    pub hash_cache: Option<Arc<HashCache>>,
    pub force_rehash: bool,
    pub file_chunk_size_bytes: Option<i64>,
    pub on_progress: Option<Box<dyn Fn(&UploadStatistics) -> bool + Send + Sync>>,
    pub max_workers: Option<usize>,
    pub max_memory_bytes: Option<usize>,
}

pub struct UploadResult {
    pub manifest: AbsManifest,
    pub statistics: UploadStatistics,
}

pub struct UploadStatistics {
    pub total_files: usize,
    pub total_bytes: u64,
    pub hashed_files: usize,
    pub hashed_bytes: u64,
    pub uploaded_files: usize,
    pub uploaded_bytes: u64,
    pub skipped_files: usize,
    pub skipped_bytes: u64,
    pub total_time: f64,
    pub rate: f64,
    pub progress: f64,
    pub progress_message: String,
}
```

`HashUploadOptions` and `UploadStatistics` implement `Default`.

### DOWNLOAD

```rust
pub async fn download_abs_manifest(
    manifest: &AbsManifest,
    data_cache: Arc<dyn AsyncDataCache>,
    options: DownloadOptions,
) -> Result<DownloadResult>
```

```rust
pub struct DownloadOptions {
    pub hash_cache: Option<Arc<HashCache>>,
    pub file_conflict_resolution: FileConflictResolution,  // Default: Overwrite
    pub apply_deletes: bool,                                // Default: true
    pub symlink_policy: SymlinkPolicy,                      // Default: Preserve
    pub on_progress: Option<Box<dyn Fn(&DownloadStatistics) -> bool + Send + Sync>>,
    pub max_workers: Option<usize>,
    pub max_memory_bytes: Option<usize>,
}

pub struct DownloadResult {
    pub manifest: AbsManifest,
    pub statistics: DownloadStatistics,
}

pub struct DownloadStatistics {
    pub total_files: usize,
    pub total_bytes: u64,
    pub downloaded_files: usize,
    pub downloaded_bytes: u64,
    pub skipped_files: usize,
    pub skipped_bytes: u64,
    pub total_time: f64,
    pub rate: f64,
    pub progress: f64,
    pub progress_message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileConflictResolution {
    Skip,
    Overwrite,
    CreateCopy,
}
```

`DownloadOptions` and `DownloadStatistics` implement `Default`.

### DIFF

```rust
pub fn diff_snapshots<P: Clone>(
    parent: &Manifest<P, Full>,
    current: &Manifest<P, Full>,
    options: &DiffOptions,
) -> Result<Manifest<P, Diff>>

pub fn entries_differ(
    parent: &FileEntry,
    current: &FileEntry,
    ignore_hashes: bool,
    ignore_runnable: bool,
) -> bool
```

```rust
pub struct DiffOptions {
    pub parent_manifest_hash: Option<String>,
    pub ignore_hashes: bool,
    /// Dual-purpose flag: ignores `runnable` when comparing, *and* copies the
    /// parent's `runnable` into the diff entry for modified files. The
    /// ignore-on-compare half is delegated to `entries_differ` via its
    /// `ignore_runnable` parameter; the copy-on-emit half is handled inside
    /// `diff_snapshots`.
    pub preserve_runnable: bool,
}
```

Implements `Default`.

### COMPOSE

```rust
pub fn compose_snapshot_with_diffs<P: Clone>(
    base: &Manifest<P, Full>,
    diffs: &[&Manifest<P, Diff>],
) -> Result<Manifest<P, Full>>

pub fn compose_diffs<P: Clone>(
    diffs: &[&Manifest<P, Diff>],
) -> Result<Manifest<P, Diff>>
```

### FILTER

```rust
pub fn filter_manifest<P: Clone, K: Clone>(
    manifest: &Manifest<P, K>,
    filter: &dyn Fn(&ManifestEntry) -> bool,
) -> Manifest<P, K>
```

```rust
pub struct IncludeExcludePathsFilter { /* ... */ }

impl IncludeExcludePathsFilter {
    pub fn new(include: &[&str], exclude: &[&str]) -> Result<Self, glob::PatternError>;
    pub fn matches(&self, entry: &ManifestEntry) -> bool;
    pub fn matches_path(&self, path: &str) -> bool;
}
```

Implements `Debug` and `Display`.

### SUBTREE

```rust
pub fn subtree_snapshot(
    manifest: &AbsSnapshot, subtree: &str, symlink_policy: SymlinkPolicy,
) -> Result<Snapshot>

pub fn subtree_snapshot_diff(
    manifest: &AbsSnapshotDiff, subtree: &str, symlink_policy: SymlinkPolicy,
) -> Result<SnapshotDiff>

pub fn subtree_rel_snapshot(
    manifest: &Snapshot, subtree: &str, symlink_policy: SymlinkPolicy,
) -> Result<Snapshot>

pub fn subtree_rel_snapshot_diff(
    manifest: &SnapshotDiff, subtree: &str, symlink_policy: SymlinkPolicy,
) -> Result<SnapshotDiff>

pub fn subtree_manifest(
    manifest: &AbsManifest, subtree: &str, symlink_policy: SymlinkPolicy,
) -> Result<RelManifest>

pub fn subtree_rel_manifest(
    manifest: &RelManifest, subtree: &str, symlink_policy: SymlinkPolicy,
) -> Result<RelManifest>
```

### JOIN

```rust
pub fn join_snapshot(manifest: &Snapshot, prefix: &str) -> Result<AbsSnapshot>;
pub fn join_snapshot_diff(manifest: &SnapshotDiff, prefix: &str) -> Result<AbsSnapshotDiff>;
pub fn join_snapshot_rel(manifest: &Snapshot, prefix: &str) -> Result<Snapshot>;
pub fn join_snapshot_diff_rel(manifest: &SnapshotDiff, prefix: &str) -> Result<SnapshotDiff>;
pub fn join_manifest(manifest: &RelManifest, prefix: &str) -> Result<AbsManifest>;
pub fn join_manifest_rel(manifest: &RelManifest, prefix: &str) -> Result<RelManifest>;
```

### PARTITION

```rust
pub fn partition_manifest(
    manifest: &AbsSnapshot,
    options: &PartitionOptions,
) -> Result<Vec<(String, Snapshot)>>

pub fn partition_rel_manifest(
    manifest: &Snapshot,
    options: &PartitionOptions,
) -> Result<Vec<(String, Snapshot)>>
```

```rust
pub struct PartitionOptions {
    pub roots: Option<Vec<String>>,
    pub referenced_paths: Option<Vec<String>>,
    pub symlink_policy: SymlinkPolicy,          // Default: CollapseEscaping
}
```

Implements `Default`.

### CACHE_SYNC

```rust
pub async fn cache_sync_manifest(
    manifests: &[&dyn ManifestRef],
    source: Arc<dyn AsyncDataCache>,
    destination: Arc<dyn AsyncDataCache>,
    options: CacheSyncOptions,
) -> Result<CacheSyncResult>
```

```rust
pub struct CacheSyncOptions {
    pub max_workers: Option<usize>,
    pub max_memory_bytes: Option<usize>,
    pub on_progress: Option<Box<dyn Fn(&CacheSyncStatistics) -> bool + Send + Sync>>,
}

pub struct CacheSyncResult {
    pub statistics: CacheSyncStatistics,
}

pub struct CacheSyncStatistics {
    pub total_objects: usize,
    pub total_bytes: u64,
    pub copied_objects: usize,
    pub copied_bytes: u64,
    pub skipped_objects: usize,
    pub skipped_bytes: u64,
    pub total_time: f64,
    pub rate: f64,
    pub progress: f64,
    pub progress_message: String,
}
```

`CacheSyncOptions` and `CacheSyncStatistics` implement `Default`.
