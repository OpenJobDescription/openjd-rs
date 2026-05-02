# HASH_UPLOAD Operation: `hash_upload_abs_manifest()`

[README](README.md) · HASH_UPLOAD Operation

**Location:** `ops/hash_upload.rs`

Fills in hashes for a manifest AND uploads file content to a data cache in a single pipelined pass, avoiding reading files twice.

```rust
pub async fn hash_upload_abs_manifest(
    manifest: &AbsManifest,
    data_cache: Arc<dyn AsyncDataCache>,
    options: HashUploadOptions,
) -> Result<UploadResult>
```

## Parameters

```rust
pub struct HashUploadOptions {
    pub hash_cache: Option<Arc<HashCache>>,
    pub force_rehash: bool,
    pub file_chunk_size_bytes: Option<i64>,
    pub on_progress: Option<Box<dyn Fn(&UploadStatistics) -> bool + Send + Sync>>,
    pub max_workers: Option<usize>,
    pub max_memory_bytes: Option<usize>,
}
```

| Parameter | Description |
|-----------|-------------|
| `data_cache` | `Arc<dyn AsyncDataCache>` — S3 or filesystem destination |
| `hash_cache` | Optional hash cache for efficiency |
| `force_rehash` | If `true`, ignore cache and recalculate all hashes |
| `file_chunk_size_bytes` | `None` = preserve from input. `WHOLE_FILE_CHUNK_SIZE` = no chunking. |
| `on_progress` | Callback for progress. Return `false` to cancel. |
| `max_workers` | Maximum parallel workers. Default: available CPUs. |
| `max_memory_bytes` | Maximum memory for buffering. Default: auto-detect. |

## Returns

```rust
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

## Default Memory Limit

```
min(16GB, max(256MB, total_memory/4, available_memory - 1GB))
```

Detected via `/proc/meminfo` on Linux. Falls back to 256MB if detection fails.

## Entry Type Handling

| Entry Type | Action |
|------------|--------|
| Regular file | Read+Hash → Upload (single pass) |
| Large file (chunking enabled) | Read+Hash → Upload (per chunk) |
| Large file (no chunking, > memory) | Two-pass: hash first, then verify+upload |
| Symlink | Pass through unchanged |
| Deleted marker | Pass through unchanged |
| Directory | Pass through unchanged |

## Cache Integration

| Cache | Purpose |
|-------|---------|
| `HashCache` | Skip hashing for files with unchanged mtime |
| `S3CheckCache` | Skip upload for files already in S3 (S3DataCache only) |

When both caches hit, the file is completely skipped (no read, no hash, no upload).

## When to Use HASH vs HASH_UPLOAD

| Use Case | Recommended |
|----------|-------------|
| Local manifest creation (no upload) | HASH |
| Diff computation only | HASH |
| Job submission with upload | HASH_UPLOAD |
| Output sync from worker | HASH_UPLOAD |

## Related Documentation

- [Pipeline Architecture](snapshot_operation_hash_upload_pipeline.md) — tokio tasks, memory pool, deduplication
- [S3-Specific Behavior](snapshot_operation_hash_upload_s3.md) — multipart uploads, cache validation, streaming
- [Hash Cache](snapshot_hash_cache.md) — local hash caching
- [Data Cache](snapshot_data_cache.md) — S3DataCache and FileSystemDataCache
