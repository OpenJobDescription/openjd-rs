# CACHE_SYNC Operation: `cache_sync_manifest()`

[README](README.md) · CACHE_SYNC Operation

**Location:** `ops/cache_sync.rs` (new)

Ensures all data referenced by a manifest exists in a destination data cache, copying from a source data cache as needed. Unlike HASH_UPLOAD (which reads from the local filesystem), CACHE_SYNC transfers between two `AsyncDataCache` instances — any combination of S3 and filesystem.

```rust
pub async fn cache_sync_manifest(
    manifests: &[&dyn ManifestRef],
    source: Arc<dyn AsyncDataCache>,
    destination: Arc<dyn AsyncDataCache>,
    options: CacheSyncOptions,
) -> Result<CacheSyncResult>
```

## Motivation

Several workflows need to copy data between caches without touching the local filesystem:

1. **Migrate data between S3 buckets** — Move a render farm's content-addressed data from one bucket/prefix to another (account migration, region change).
2. **Pre-seed a local cache from S3** — Before going offline or into a restricted network, pull all data for a set of manifests into a `FileSystemDataCache` on a local NAS.
3. **Promote debug snapshot to S3** — A debug snapshot captured to `FileSystemDataCache` needs to be pushed to S3 for a job submission without re-reading the original source files.
4. **Cross-region replication** — Copy job attachment data from one region's S3 bucket to another for multi-region rendering.
5. **Cache warming** — Given a manifest of expected inputs, ensure a destination cache is populated before jobs start.

In all these cases the source files on disk may no longer exist or may have changed. The authoritative data is in the source cache, keyed by content hash. CACHE_SYNC transfers that data directly.

## Parameters

```rust
pub struct CacheSyncOptions {
    pub max_workers: Option<usize>,
    pub max_memory_bytes: Option<usize>,
    pub on_progress: Option<Box<dyn Fn(&CacheSyncStatistics) -> bool + Send + Sync>>,
}
```

| Parameter | Description |
|-----------|-------------|
| `manifests` | Slice of manifest references (absolute or relative, snapshot or diff). Only hashed, non-deleted, non-symlink file entries are synced. Entries without hashes are skipped. Hashes are deduplicated across all manifests. |
| `source` | `Arc<dyn AsyncDataCache>` — where data is read from |
| `destination` | `Arc<dyn AsyncDataCache>` — where data is written to |
| `max_workers` | Maximum parallel transfer tasks. Default: available CPUs. |
| `max_memory_bytes` | Maximum memory for in-flight data. Default: auto-detect. |
| `on_progress` | Callback for progress. Return `false` to cancel. |

## Returns

```rust
pub struct CacheSyncResult {
    pub statistics: CacheSyncStatistics,
}

#[derive(Debug, Default, Clone)]
pub struct CacheSyncStatistics {
    pub total_objects: usize,
    pub total_bytes: u64,
    pub copied_objects: usize,
    pub copied_bytes: u64,
    pub skipped_objects: usize,   // Already in destination
    pub skipped_bytes: u64,
    pub total_time: f64,          // seconds
    pub rate: f64,                // bytes/second (sliding window)
    pub progress: f64,            // 0.0 to 100.0
    pub progress_message: String,
}
```

## Manifest Input

CACHE_SYNC accepts a slice of `&dyn ManifestRef` — it doesn't need absolute paths since it never touches the local filesystem. It extracts the set of unique `(hash, algorithm)` pairs to sync across all manifests:

- Regular files with `hash` set → one object to sync
- Regular files with `chunk_hashes` set → one object per chunk hash
- Symlinks, deleted entries, unhashed files, directories → skipped

Duplicate hashes (same content referenced by multiple files) are deduplicated before syncing.

## Pipeline Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    CACHE_SYNC PIPELINE                                  │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  For each unique (hash, algorithm):                                     │
│                                                                         │
│  1. Check destination.object_exists(hash, alg)                          │
│     └─► If exists, mark as skipped                                      │
│                                                                         │
│  2. Try destination.copy_from(source, hash, alg)                        │
│     └─► ServerSideCopy (S3→S3): done, no data through client            │
│     └─► NotSupported: fall through to step 3                            │
│                                                                         │
│  3. Fallback (cross-type transfers):                                    │
│     a. memory_pool.acquire(object_size).await                           │
│     b. source.get_object(hash, alg).await                               │
│     c. destination.put_object(hash, alg, data).await                    │
│     d. Drop permit (releases memory)                                    │
│                                                                         │
│  Memory bounded by MemoryPool (tokio::sync::Semaphore)                  │
│  Concurrent transfers via tokio::spawn                                  │
│  Deduplication via HashSet (upfront, before spawning tasks)              │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### Large Object Handling

For objects larger than `2 × destination.multipart_part_size()`:

1. Read from source using `get_object_range()` in parts
2. Upload to destination using multipart upload (`create_multipart_upload` / `upload_part` / `complete_multipart_upload`)
3. Each part is bounded by the memory pool — only a few parts in flight at once

This avoids buffering an entire large object in memory.

### S3-to-S3 Direct Copy

When both source and destination are `S3DataCache`, the pipeline should use S3's server-side copy APIs instead of downloading and re-uploading data:

- **`CopyObject`** — For objects ≤ 5GB, a single API call copies data directly between buckets (or prefixes within the same bucket) without the data transiting through the client. This is the common case for content-addressed objects.
- **`UploadPartCopy`** — For objects > 5GB, use multipart upload with `UploadPartCopy` for each part. Each part is copied server-side.

This means S3→S3 transfers:
- Use zero client bandwidth (data stays within AWS)
- Are significantly faster (no download/upload round-trip)
- Don't consume memory pool permits (no data buffered locally)

The `AsyncDataCache` trait needs a method to support this:

```rust
/// Copy an object from another cache, server-side if possible.
/// Returns true if a server-side copy was performed, false if the caller
/// should fall back to get+put.
async fn copy_from(
    &self,
    source: &dyn AsyncDataCache,
    hash: &str,
    algorithm: &str,
) -> std::io::Result<CopyResult>;

pub enum CopyResult {
    ServerSideCopy,   // Done — no data transited through client
    NotSupported,     // Caller should fall back to get_object + put_object
}
```

`S3DataCache` implements this by checking if `source` is also an `S3DataCache` (via `Any` downcast), and if so, calling `CopyObject` with `CopySource` pointing to the source bucket/key. `FileSystemDataCache` returns `NotSupported`.

The pipeline flow becomes:

```
For each unique (hash, algorithm):
  1. destination.object_exists() → skip if present
  2. destination.copy_from(source, hash, alg)
     └─► ServerSideCopy → done (no memory, no bandwidth)
     └─► NotSupported → fall back to get_object + put_object (memory-bounded)
```

### S3 Batch Operations — TODO/Research

For very large syncs (tens of thousands of objects or more), individual `CopyObject` calls may be slow due to per-request overhead. S3 Batch Operations could potentially handle this more efficiently:

**What to research:**

1. **S3 Batch Operations `S3CopyObject`** — Can submit a manifest of copy operations as a single batch job. S3 processes them server-side with high parallelism. Relevant for syncs with 10k+ objects.
   - How to construct the CSV/JSON manifest of `(source_bucket, source_key, dest_bucket, dest_key)` pairs
   - IAM role requirements for the batch job
   - Latency characteristics — batch jobs are async and may take minutes to start; is this acceptable for interactive workflows?
   - Cost comparison: batch operations have per-job and per-object charges vs. free `CopyObject` calls

2. **Threshold heuristic** — At what object count does batch become faster than parallel `CopyObject` calls? Likely depends on:
   - Number of objects (batch overhead amortized over more objects)
   - Whether the user is willing to wait for async job completion
   - Cross-region vs. same-region (batch may handle cross-region more efficiently)

3. **Hybrid approach** — Use parallel `CopyObject` for small syncs (< N objects), offer a `--batch` flag or auto-detect for large syncs. The batch path would:
   - Write a CSV manifest to a temp S3 key
   - Submit `CreateJob` to S3 Batch Operations
   - Poll for completion
   - Report results

4. **Alternative: S3 Replication** — For ongoing cross-region needs, S3 Replication Rules may be more appropriate than CACHE_SYNC. Worth documenting as guidance for users rather than implementing.

This is deferred — the initial implementation should use parallel `CopyObject`/`UploadPartCopy` which is simple, fast for typical workloads, and doesn't require additional IAM setup.

### Concurrent Transfer Deduplication

Hashes are deduplicated upfront via `HashSet` before spawning transfer tasks. Since all work items are extracted from manifests before the pipeline starts, this is simpler than runtime deduplication and equally effective.

## Integrity

Content-addressed storage is inherently idempotent — writing the same hash twice produces the same result. CACHE_SYNC does not re-verify hashes during transfer because:

1. The source cache already satisfies the content-addressed invariant (data was verified on original upload by HASH_UPLOAD)
2. The hash serves as both the key and the checksum — if the data were corrupted, it would be stored under a different key
3. S3 `CopyObject` preserves data integrity server-side; S3 and filesystem writes have their own integrity checks

If stronger verification is needed, a future `--verify` flag could re-hash data after reading from source and compare against the key.

## Error Handling

| Condition | Behavior |
|-----------|----------|
| Source object missing | `SnapshotError::Io` (surfaced from data cache) |
| Destination write fails | Error propagated, partial sync is safe (idempotent) |
| Cancellation via callback | `SnapshotError::Cancelled` |
| Source and destination are the same cache | No-op for all objects (existence checks all pass) |

Partial syncs are safe to retry — objects already copied will be skipped on the next run.

## Example

```rust
use openjd_snapshots::{
    cache_sync_manifest, CacheSyncOptions,
    S3DataCache, FileSystemDataCache,
};
use std::sync::Arc;

// Sync from S3 to local filesystem cache
let source = Arc::new(S3DataCache::new(/* bucket, prefix, client */));
let destination = Arc::new(FileSystemDataCache::new("/mnt/local-cache/Data")?);

let result = cache_sync_manifest(
    &[&manifest as &dyn ManifestRef],
    source,
    destination,
    CacheSyncOptions::default(),
).await?;

println!("Copied {} objects ({} bytes), skipped {} (already present)",
    result.statistics.copied_objects,
    result.statistics.copied_bytes,
    result.statistics.skipped_objects,
);
```

```rust
// Sync between two S3 buckets (e.g., cross-region)
let source = Arc::new(S3DataCache::new(/* us-west-2 bucket */));
let destination = Arc::new(S3DataCache::new(/* eu-west-1 bucket */));

let result = cache_sync_manifest(
    &[&manifest as &dyn ManifestRef], source, destination,
    CacheSyncOptions {
        max_workers: Some(32),  // High parallelism for cross-region
        ..Default::default()
    },
).await?;
```

## Relationship to Other Operations

| Operation | Source | Destination | Hashing |
|-----------|--------|-------------|---------|
| HASH_UPLOAD | Local filesystem | Data cache | Yes (reads files, computes hashes) |
| DOWNLOAD | Data cache | Local filesystem | No (uses manifest hashes) |
| CACHE_SYNC | Data cache | Data cache | No (uses manifest hashes) |

CACHE_SYNC completes the triangle — HASH_UPLOAD moves data from filesystem to cache, DOWNLOAD moves data from cache to filesystem, and CACHE_SYNC moves data between caches.

## CLI Integration

```
openjd snapshot cache-sync <manifest> --source <cache-uri> --dest <cache-uri>
```

Where `<cache-uri>` is either `s3://bucket/prefix` or a local path. This enables scripted cache migration and pre-seeding workflows.

## Testing

Unit tests use `s3s` (in-process S3 mock) for all FS↔S3 combinations without requiring credentials. These run as part of normal `cargo test`.

S3 integration tests in `tests/test_s3_integration.rs` exercise real S3 operations including server-side `CopyObject`. They require `OPENJD_TEST_S3_BUCKET` to be set and are `#[ignore]`d by default. See `AGENTS.md` for details.
