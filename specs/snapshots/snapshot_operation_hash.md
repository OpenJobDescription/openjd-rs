# HASH Operation: `hash_abs_manifest()`

[README](README.md) · HASH Operation

**Location:** `ops/hash_op.rs`

Fills in hashes for a manifest created by `collect_abs_snapshot()` or `diff_snapshots()`. The input manifest must have absolute paths.

The operation exposes three entry points. The concrete-typed variants preserve the input type in the result so callers can chain further operations without matching on an enum; the enum-dispatching variant is useful when the caller holds an [`AbsManifest`] without statically knowing whether it is a snapshot or a diff.

```rust
// Concrete-typed entry points (preferred when the caller knows the concrete type)
pub fn hash_abs_snapshot(
    manifest: &AbsSnapshot,
    options: HashOptions,
) -> Result<HashResult<AbsSnapshot>>;

pub fn hash_abs_snapshot_diff(
    manifest: &AbsSnapshotDiff,
    options: HashOptions,
) -> Result<HashResult<AbsSnapshotDiff>>;

// Enum-dispatching entry point
pub fn hash_abs_manifest(
    manifest: &AbsManifest,
    options: HashOptions,
) -> Result<HashResult<AbsManifest>>;
```

All three share identical parameters, algorithm, and statistics; they differ only in the static type of the manifest carried in and out. This mirrors the three-entry-point pattern used by `subtree_*` (`subtree_snapshot` / `subtree_snapshot_diff` / `subtree_manifest`) and `join_*`.

## Parameters

```rust
pub struct HashOptions {
    pub hash_cache: Option<Arc<HashCache>>,
    pub force_rehash: bool,
    pub file_chunk_size_bytes: Option<i64>,
    pub on_progress: Option<Box<dyn Fn(&HashStatistics) -> bool + Send + Sync>>,
    pub max_workers: Option<usize>,
}
```

| Parameter | Description |
|-----------|-------------|
| `hash_cache` | Optional hash cache for efficiency |
| `force_rehash` | If `true`, ignore cache and recalculate all hashes |
| `file_chunk_size_bytes` | `None` = preserve from input. `WHOLE_FILE_CHUNK_SIZE` = no chunking. |
| `on_progress` | Callback for progress reporting. Return `true` to continue, `false` to cancel. |
| `max_workers` | Maximum rayon parallelism. Default: available CPUs. |

## Returns

```rust
/// Generic over the manifest type `M`; `M` defaults to `AbsManifest` so bare
/// `HashResult` still names the enum-dispatch shape.
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
    pub total_time: f64,      // seconds
    pub rate: f64,             // bytes/second
    pub progress: f64,         // 0.0 to 100.0
    pub progress_message: String,
}
```

The returned `M` matches the input: `hash_abs_snapshot` returns `HashResult<AbsSnapshot>`, `hash_abs_snapshot_diff` returns `HashResult<AbsSnapshotDiff>`, and `hash_abs_manifest` returns `HashResult<AbsManifest>`.

## Implementation

Uses `rayon` for parallel hashing across CPU cores. Each file is hashed via `hash_file()` (whole file) or `hash_file_chunked()` (chunked), both using `xxhash_rust::xxh3::xxh3_128`.

Progress tracking uses `SlidingWindowRate` with a 12-second window for smooth rate estimation. The progress callback is invoked from worker threads; `HashStatistics` is built under a `Mutex`.

### Entry Type Handling

| Entry Type | Action |
|------------|--------|
| Regular file (≤ chunk size or `WHOLE_FILE_CHUNK_SIZE`) | Compute single `hash` |
| Large file (> chunk size, chunking enabled) | Compute `chunk_hashes` |
| Symlink | Pass through unchanged |
| Deleted marker | Pass through unchanged (diff manifests only) |
| Directory | Pass through unchanged |

### Hash Cache Behavior

| Condition | Behavior |
|-----------|----------|
| `hash_cache` provided, `force_rehash=false` | Check cache by (path, mtime); use cached hash on hit |
| `hash_cache` provided, `force_rehash=true` | Always compute hash, update cache |
| `hash_cache` is `None` | Always compute hash |

On cache miss, the computed hash is stored in the cache for future lookups.

### File Chunking

| `file_chunk_size_bytes` | File Size | Behavior |
|------------------------|-----------|----------|
| `WHOLE_FILE_CHUNK_SIZE` (-1) | Any | Hash entire file as whole |
| Positive value | ≤ chunk size | Compute single `hash` |
| Positive value | > chunk size | Compute `chunk_hashes` (one per chunk) |

### Rate Calculation

Uses `SlidingWindowRate` (in `ops/rate.rs`):
- Maintains a `VecDeque<(timestamp, cumulative_bytes)>` of samples
- Window size: 12 seconds (`RATE_WINDOW_SECONDS`)
- Rate = `(current_bytes - oldest_bytes) / (current_time - oldest_time)`

### Cancellation

The progress callback can return `false` to cancel. An `AtomicBool` is checked by worker threads; when set, remaining work items are skipped and `SnapshotError::Cancelled` is returned.

## Example

Using the concrete entry point (preserves `AbsSnapshot` through the result):

```rust
use openjd_snapshots::{
    collect_abs_snapshot, hash_abs_snapshot,
    CollectOptions, HashOptions, HashCache,
};
use std::sync::Arc;

let manifest = collect_abs_snapshot(&["/projects/my_scene"], &[] as &[&str], CollectOptions::default())?;
let cache = Arc::new(HashCache::open_default()?);

let result = hash_abs_snapshot(
    &manifest,
    HashOptions {
        hash_cache: Some(cache),
        on_progress: Some(Box::new(|stats| {
            let rate_mb = stats.rate / (1024.0 * 1024.0);
            println!("{:.1}% - {:.1} MB/s", stats.progress, rate_mb);
            true
        })),
        ..Default::default()
    },
)?;

// result.manifest is AbsSnapshot; no enum unwrap needed.
println!("Hashed {} bytes, skipped {} bytes",
    result.statistics.hashed_bytes, result.statistics.skipped_bytes);
let snapshot: AbsSnapshot = result.manifest;
```

Using the enum entry point (when the caller holds an `AbsManifest`):

```rust
use openjd_snapshots::{hash_abs_manifest, AbsManifest, HashOptions};

let result = hash_abs_manifest(&abs_manifest, HashOptions::default())?;
match result.manifest {
    AbsManifest::Snapshot(s) => { /* use s */ }
    AbsManifest::Diff(d)     => { /* use d */ }
}
```
