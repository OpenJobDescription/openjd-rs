# Job Attachments Snapshots — Rust Implementation Spec

> **Status: experimental.** This spec describes the `openjd-snapshots` crate,
> which is under active development — its public API may change without
> notice. See [snapshots/snapshot_overview.md](snapshots/snapshot_overview.md#status)
> for on-disk format status: v2023 is stable (the format AWS Deadline
> Cloud uses); v2025 is an experimental draft whose wire format is
> expected to change.

## Overview

Port the job attachments snapshots library from `deadline-cloud` (Python) into a new
`openjd-snapshots` crate in the `openjd-rs` workspace. The library captures directory
tree snapshots, computes diffs, and transfers data to/from content-addressed storage
(S3 or local filesystem). The CLI integration goes into `openjd-cli`.

The Python design document at `~/deadline-cloud/docs/design/job_attachments_snapshots.md`
and its sub-component documents are the authoritative reference. This spec describes how
to map that design into idiomatic Rust.

## Motivation

- Rust gives us memory safety without GC, predictable performance, and easy parallelism
  via `tokio` or `rayon` — all critical for a data pipeline that hashes and transfers
  large file trees.
- A single `openjd` binary can ship snapshot operations alongside template validation
  and local job execution, reducing deployment complexity.
- The Python implementation uses threads with a manual memory pool; Rust's ownership
  model and async I/O can achieve the same bounded-memory pipeline more naturally.

## Crate Layout

```
openjd-rs/
├── Cargo.toml                          # Add openjd-snapshots to workspace members
├── crates/
│   ├── openjd-snapshots/               # NEW — snapshot library crate
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                  # Public API re-exports
│   │       ├── manifest.rs             # Manifest structs + entry types
│   │       ├── data_cache.rs           # AsyncDataCache trait + impls
│   │       ├── hash_cache.rs           # Local SQLite hash cache
│   │       ├── hash.rs                 # Hashing utilities (xxh128, file chunking)
│   │       ├── path_util.rs            # Path normalization, absolute/relative detection
│   │       ├── error.rs                # Error types
│   │       └── ops/                    # One module per operation
│   │           ├── mod.rs
│   │           ├── collect.rs          # COLLECT
│   │           ├── hash.rs             # HASH
│   │           ├── hash_upload.rs      # HASH_UPLOAD (entry point + pipeline)
│   │           ├── download.rs         # DOWNLOAD (entry point + pipeline)
│   │           ├── filter.rs           # FILTER
│   │           ├── diff.rs             # DIFF
│   │           ├── compose.rs          # COMPOSE
│   │           ├── subtree.rs          # SUBTREE
│   │           ├── partition.rs        # PARTITION
│   │           └── join.rs             # JOIN
│   └── openjd-cli/
│       └── src/
│           ├── main.rs                 # Add snapshot subcommands
│           └── snapshot.rs             # NEW — CLI wiring for snapshot ops
```

## Dependency Graph

```
openjd-cli
  ├── openjd-snapshots
  │     ├── aws-sdk-s3
  │     ├── aws-sdk-sts       (account ID auto-detection)
  │     ├── rusqlite           (hash cache, S3 check cache)
  │     ├── xxhash-rust        (xxh128 hashing)
  │     ├── tokio              (async I/O, task spawning)
  │     ├── serde / serde_json (manifest serialization)
  │     ├── glob               (IncludeExcludePathsFilter)
  │     └── thiserror
  ├── openjd-model
  └── openjd-sessions
```

`openjd-snapshots` has no dependency on `openjd-model` or `openjd-expr`. It is a
standalone library that can be used independently of OpenJD templates.

## Core Data Structures

### Manifest Types

The Python design uses four manifest classes along two axes (path style × manifest type).
In Rust, encode these axes as generic parameters on a single struct:

```rust
use std::path::PathBuf;

/// Marker types for path style (zero-sized, compile-time only).
pub struct Abs;
pub struct Rel;

/// Marker types for manifest kind.
pub struct Full;   // Snapshot — no deleted entries
pub struct Diff;   // SnapshotDiff — may contain deleted entries

/// The core manifest type, parameterized by path style and kind.
pub struct Manifest<P, K> {
    pub hash_alg: HashAlgorithm,
    pub files: Vec<FileEntry>,
    pub dirs: Vec<DirEntry>,
    pub total_size: u64,
    pub parent_manifest_hash: Option<String>,
    pub file_chunk_size_bytes: i64,  // -1 = WHOLE_FILE_CHUNK_SIZE
    _phantom: PhantomData<(P, K)>,
}

/// Concrete type aliases matching the Python names.
pub type AbsSnapshot     = Manifest<Abs, Full>;
pub type AbsSnapshotDiff = Manifest<Abs, Diff>;
pub type Snapshot         = Manifest<Rel, Full>;
pub type SnapshotDiff     = Manifest<Rel, Diff>;
```

Validation is enforced at construction via builder methods or `TryFrom` conversions:
- `Manifest<Abs, _>` rejects relative paths.
- `Manifest<Rel, _>` rejects absolute paths.
- `Manifest<_, Full>` rejects entries with `deleted = true`.

### File Entry

```rust
pub struct FileEntry {
    pub path: String,                       // POSIX-normalized
    pub hash: Option<String>,
    pub size: Option<u64>,
    pub mtime: Option<i64>,                 // microseconds since epoch
    pub chunk_hashes: Option<Vec<String>>,
    pub symlink_target: Option<String>,
    pub runnable: bool,
    pub deleted: bool,
}
```

### Directory Entry

```rust
pub struct DirEntry {
    pub path: String,
    pub deleted: bool,
}
```

### Path Normalization

Implement in `path_util.rs`:
- Always use `/` as separator in manifest paths.
- Collapse `.` and `..` components.
- Strip Windows long-path prefix `\\?\` on Windows.
- Strip trailing slashes (except root).
- Detect absolute paths: starts with `/`, or on Windows `X:` drive letter.

### HashAlgorithm

```rust
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HashAlgorithm {
    #[serde(rename = "xxh128")]
    Xxh128,
}
```

Start with only `xxh128`. The Python code supports others but `xxh128` is the default
and only algorithm used in practice.

### Constants

```rust
pub const DEFAULT_FILE_CHUNK_SIZE: i64 = 256 * 1024 * 1024;  // 256 MB
pub const WHOLE_FILE_CHUNK_SIZE: i64 = -1;
pub const DEFAULT_S3_MULTIPART_PART_SIZE: usize = 32 * 1024 * 1024;  // 32 MB
```

## Content-Addressed Data Cache

### Trait

```rust
#[async_trait]
pub trait AsyncDataCache: Send + Sync {
    fn object_key(&self, hash: &str, algorithm: &str) -> String;
    async fn object_exists(&self, hash: &str, algorithm: &str) -> Result<bool>;
}
```

### S3DataCache

```rust
pub struct S3DataCache {
    pub bucket: String,
    pub key_prefix: String,
    pub client: aws_sdk_s3::Client,
    pub s3_check_cache: Option<S3CheckCache>,
    pub multipart_part_size: usize,
    pub account_id: AccountId,
}

pub enum AccountId {
    Auto(String),       // Resolved at construction via STS
    Explicit(String),
    NoCheck,
}
```

### FileSystemDataCache

```rust
pub struct FileSystemDataCache {
    pub root_path: PathBuf,  // Must be absolute
}
```

Both implement `AsyncDataCache`.

## Hash Cache

SQLite database at `~/.deadline/job_attachments/hash_cache.db`.

```rust
pub struct HashCache {
    conn: rusqlite::Connection,  // Single connection; use a pool for concurrent access
}
```

Schema matches the Python implementation:
- Primary key: `(file_path BLOB, hash_algorithm TEXT, range_start INTEGER, range_end INTEGER)`
- `file_hash TEXT`, `last_modified_time TIMESTAMP`

For concurrent access from async tasks, wrap in `tokio::sync::Mutex` or use `r2d2-sqlite`
connection pool. WAL mode is enabled for concurrent readers.

## S3 Check Cache

Same SQLite approach as hash cache, tracking `(bucket, key_prefix, hash)` tuples known
to exist in S3. Probabilistic validation (first 100 always verified, then 1% sampling)
is implemented in the HASH_UPLOAD pipeline.

## Operations

Each operation is a public function in `ops/`. Signatures mirror the Python API adapted
to Rust idioms.

### 1. COLLECT

```rust
pub fn collect_abs_snapshot(
    directories: &[impl AsRef<Path>],
    filenames: &[impl AsRef<Path>],
    options: CollectOptions,
) -> Result<AbsSnapshot>

pub struct CollectOptions {
    pub optional_filenames: Vec<PathBuf>,
    pub symlink_policy: SymlinkPolicy,
    pub file_chunk_size_bytes: Option<i64>,
}
```

Implementation: `walkdir` crate for directory traversal. Two-pass symlink handling
for `COLLAPSE_ESCAPING` (build collected set, then process deferred symlinks).

### 2. HASH

```rust
pub fn hash_abs_manifest(
    manifest: &AbsManifest,
    options: HashOptions,
) -> Result<HashResult>

// AbsManifest = either AbsSnapshot or AbsSnapshotDiff, via an enum wrapper:
pub enum AbsManifest {
    Snapshot(AbsSnapshot),
    Diff(AbsSnapshotDiff),
}

pub struct HashOptions {
    pub hash_cache: Option<Arc<HashCache>>,
    pub force_rehash: bool,
    pub file_chunk_size_bytes: Option<i64>,
    pub on_progress: Option<Box<dyn Fn(&HashProgressMetadata) -> bool + Send>>,
}

pub struct HashResult {
    pub manifest: AbsManifest,
    pub statistics: HashProgressMetadata,
}
```

Single-threaded hashing with optional hash cache. Uses `xxhash_rust::xxh3::xxh3_128`
for computing hashes. Progress callback with sliding-window rate estimation.

### 3. HASH_UPLOAD

```rust
pub async fn hash_upload_abs_manifest(
    manifest: &AbsManifest,
    data_cache: &dyn AsyncDataCache,
    options: HashUploadOptions,
) -> Result<UploadResult>

pub struct HashUploadOptions {
    pub hash_cache: Option<Arc<HashCache>>,
    pub force_rehash: bool,
    pub max_memory_bytes: Option<usize>,
    pub max_workers: Option<usize>,
    pub file_chunk_size_bytes: Option<i64>,
    pub on_progress: Option<Box<dyn Fn(&HashUploadProgressMetadata) -> bool + Send + Sync>>,
}

pub struct UploadResult {
    pub manifest: AbsManifest,
    pub statistics: HashUploadProgressMetadata,
}
```

#### Pipeline Architecture (Rust)

Replace the Python two-thread-pool + manual memory pool with a `tokio` bounded channel:

```
                    ┌──────────────────────────────────┐
                    │  Semaphore (max_memory_bytes)     │
                    └──────────────────────────────────┘
                              │
  ┌───────────────────────────┼───────────────────────────┐
  │  spawn_blocking tasks     │                           │
  │  (read + hash from disk)  │  acquire(file_size)       │
  │                           ▼                           │
  │  ┌─────────┐  ┌─────────┐  ┌─────────┐               │
  │  │ Task 1  │  │ Task 2  │  │ Task N  │               │
  │  └────┬────┘  └────┬────┘  └────┬────┘               │
  └───────┼────────────┼────────────┼─────────────────────┘
          │            │            │
          ▼            ▼            ▼
  ┌───────────────────────────────────────────────────────┐
  │  async upload tasks                                   │
  │  (S3 PutObject / multipart, or fs copy)               │
  │  release semaphore permits on completion              │
  └───────────────────────────────────────────────────────┘
```

- `tokio::sync::Semaphore` with `max_memory_bytes` permits replaces the Python memory pool.
- `tokio::task::spawn_blocking` for disk I/O (read + hash).
- `tokio::spawn` for async S3 uploads via `aws-sdk-s3`.
- Concurrent upload deduplication via `DashMap<String, tokio::sync::broadcast::Sender<()>>`.

#### S3 Multipart Upload

Files > `2 × multipart_part_size` use `CreateMultipartUpload` / `UploadPart` /
`CompleteMultipartUpload`. Parts are uploaded as separate async tasks. An `AtomicUsize`
counter tracks remaining parts; the task that decrements to 0 calls complete.

#### Streaming Files (> max_memory)

Two-pass approach when `WHOLE_FILE_CHUNK_SIZE` and file > `max_memory_bytes`:
1. Stream-read computing full hash + per-part hashes (no allocation).
2. If HeadObject miss, re-read with per-part hash verification and upload.

### 4. DOWNLOAD

```rust
pub async fn download_abs_manifest(
    manifest: &AbsManifest,
    data_cache: &dyn AsyncDataCache,
    options: DownloadOptions,
) -> Result<DownloadResult>

pub struct DownloadOptions {
    pub hash_cache: Option<Arc<HashCache>>,
    pub file_conflict_resolution: FileConflictResolution,
    pub apply_deletes: bool,
    pub symlink_policy: SymlinkPolicy,
    pub max_workers: Option<usize>,
    pub on_progress: Option<Box<dyn Fn(&DownloadProgressMetadata) -> bool + Send + Sync>>,
}

pub enum FileConflictResolution { Skip, Overwrite, CreateCopy }
```

Atomic downloads: write to temp file, then `std::fs::rename`. Parallel downloads via
`tokio::spawn`. Chunked files: download all chunks in parallel, concatenate into temp
file, atomic move when all complete. Symlinks created via `std::os::unix::fs::symlink`
(topologically sorted).

### 5. FILTER

```rust
pub fn filter_manifest<P, K>(
    manifest: &Manifest<P, K>,
    filter: &dyn Fn(&ManifestEntry) -> bool,
) -> Manifest<P, K>
```

Where `ManifestEntry` is an enum over `FileEntry` and `DirEntry`. Returns a new manifest
with only matching entries and recomputed `total_size`.

Built-in `IncludeExcludePathsFilter`:

```rust
pub struct IncludeExcludePathsFilter {
    include: Vec<glob::Pattern>,
    exclude: Vec<glob::Pattern>,
}

impl IncludeExcludePathsFilter {
    pub fn matches(&self, entry: &ManifestEntry) -> bool { ... }
}
```

### 6. DIFF

```rust
pub fn diff_snapshots<P>(
    parent: &Manifest<P, Full>,
    current: &Manifest<P, Full>,
    options: DiffOptions,
) -> Result<Manifest<P, Diff>>

pub struct DiffOptions {
    pub parent_manifest_hash: Option<String>,
    pub ignore_hashes: bool,
    pub preserve_runnable: bool,
}
```

Builds a `HashMap<&str, &FileEntry>` index of the parent, then iterates current entries
to detect additions and modifications. Iterates parent index for deletions.

### 7. COMPOSE

```rust
pub fn compose_manifests<P>(
    manifests: &[&dyn AnyManifest<P>],
) -> Result<Box<dyn AnyManifest<P>>>
```

Uses a trie (prefix tree) internally, matching the Python implementation. Each trie node
holds optional `FileEntry`, `deleted` flag, and children keyed by path component.
`reconcile_deleted_flags()` clears deleted on nodes with non-deleted descendants.

In practice, the concrete return type depends on the input combination:
- `(Snapshot, Diff, ...)` → `Snapshot`
- `(Diff, ...)` → `Diff`

We can model this with an enum return or separate functions:

```rust
pub fn compose_snapshot_with_diffs<P>(
    base: &Manifest<P, Full>,
    diffs: &[&Manifest<P, Diff>],
) -> Manifest<P, Full>

pub fn compose_diffs<P>(
    diffs: &[&Manifest<P, Diff>],
) -> Manifest<P, Diff>
```

### 8. SUBTREE

```rust
pub fn subtree_manifest(
    manifest: &dyn AnyManifest<impl PathStyle>,
    subtree: &str,
    symlink_policy: SymlinkPolicy,
) -> RelManifest
```

Where `RelManifest` is an enum of `Snapshot | SnapshotDiff`. Filters entries under
`subtree`, strips the prefix, handles symlink escaping per policy.

### 9. PARTITION

```rust
pub fn partition_manifest(
    manifest: &dyn AnyManifest<impl PathStyle>,
    options: PartitionOptions,
) -> Vec<(String, RelManifest)>

pub struct PartitionOptions {
    pub roots: Option<Vec<String>>,
    pub referenced_paths: Option<Vec<String>>,
    pub symlink_policy: SymlinkPolicy,
}
```

Auto-root determination: longest common prefix on POSIX, per-drive on Windows.

### 10. JOIN

```rust
pub fn join_manifest(
    manifest: &RelManifest,
    prefix: &str,
) -> AnyManifest
```

Returns `AbsManifest` if prefix is absolute, otherwise a prefixed `RelManifest`.

## SymlinkPolicy

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SymlinkPolicy {
    CollapseEscaping,
    CollapseAll,
    ExcludeEscaping,
    ExcludeAll,
    Preserve,
    TransitiveIncludeTargets,
}
```

## Manifest Serialization

Manifests are serialized to JSON matching the Python on-disk format. Use `serde` with
`#[serde(rename_all = "camelCase")]` to match the existing field names (`hashAlg`,
`totalSize`, `fileChunkSizeBytes`, `parentManifestHash`).

Support both v2023-03-03 and v2025-12-04-beta formats:
- v2023: No symlinks, no empty dirs, no deletions, `WHOLE_FILE_CHUNK_SIZE` only.
- v2025: Full feature set.

Deserialization detects format version from the JSON structure and produces the
appropriate manifest type.

## Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Manifest validation error: {0}")]
    Validation(String),

    #[error("S3 error: {0}")]
    S3(String),

    #[error("Hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("Operation cancelled")]
    Cancelled,

    #[error("{0}")]
    Other(String),
}
```

## CLI Integration

Add snapshot subcommands to `openjd-cli`:

```
openjd snapshot collect <dir>...          # COLLECT + optional HASH + SUBTREE
openjd snapshot diff <parent> <dir>       # COLLECT + DIFF
openjd snapshot upload <manifest> <s3uri>  # JOIN + HASH_UPLOAD
openjd snapshot download <manifest> <dir>  # JOIN + DOWNLOAD
```

These mirror the `deadline manifest snapshot`, `deadline manifest diff`,
`deadline attachment upload`, and `deadline job download-output` workflows
from the Python design doc.

## Implementation Plan

### Phase 1 — Data Structures and Pure Operations

1. Create `crates/openjd-snapshots` with `Cargo.toml`.
2. Implement `manifest.rs`: `Manifest<P, K>`, `FileEntry`, `DirEntry`, validation,
   `clear_hashes()`, serde serialization/deserialization.
3. Implement `path_util.rs`: normalization, absolute detection.
4. Implement `hash.rs`: xxh128 hashing, file chunking logic.
5. Implement pure manifest operations: FILTER, DIFF, COMPOSE, SUBTREE, PARTITION, JOIN.
6. Unit tests for all of the above.

### Phase 2 — Filesystem Operations

7. Implement `hash_cache.rs`: SQLite hash cache with WAL.
8. Implement COLLECT: directory walking, symlink handling (two-pass for `COLLAPSE_ESCAPING`).
9. Implement HASH: single-threaded hashing with hash cache integration.
10. Unit tests with temp directories.

### Phase 3 — Data Transfer Pipeline

11. Implement `data_cache.rs`: trait + `FileSystemDataCache`.
12. Implement HASH_UPLOAD with `FileSystemDataCache` first (simpler, no S3).
13. Implement DOWNLOAD with `FileSystemDataCache`.
14. Add `S3DataCache` with multipart upload/download, S3 check cache,
    probabilistic validation.
15. Implement streaming two-pass for files > `max_memory_bytes`.
16. Integration tests against local filesystem; S3 integration tests behind feature flag.

### Phase 4 — CLI and Polish

17. Add snapshot subcommands to `openjd-cli`.
18. Progress reporting with terminal output.
19. Cross-platform testing (Windows path handling).

## Key Rust-Specific Design Decisions

1. **Phantom type parameters** for path style and manifest kind give compile-time
   guarantees that the Python implementation enforces at runtime. Operations that
   require absolute paths take `Manifest<Abs, _>` — passing a relative manifest
   is a compile error.

2. **`tokio` async runtime** for the transfer pipeline instead of Python's
   `ThreadPoolExecutor`. Async S3 calls via `aws-sdk-s3` avoid blocking threads
   during network I/O. `spawn_blocking` for disk reads keeps the async runtime
   responsive.

3. **`tokio::sync::Semaphore`** for memory bounding instead of a custom memory pool.
   Acquire `file_size` permits before reading, release after upload completes.
   Natural backpressure when the semaphore is exhausted.

4. **`DashMap`** for concurrent upload deduplication instead of `Dict + Lock`.
   Lock-free concurrent hash map avoids contention.

5. **`walkdir`** for directory traversal instead of `os.walk`. Handles symlink
   cycles, cross-device boundaries, and permission errors.

6. **Separate compose functions** (`compose_snapshot_with_diffs` and `compose_diffs`)
   instead of a single polymorphic function. Rust's type system makes the input/output
   type relationships explicit at compile time.

7. **No GIL** — the Python implementation uses threads but is limited by the GIL for
   CPU-bound hashing. Rust's hashing runs truly in parallel across cores.
