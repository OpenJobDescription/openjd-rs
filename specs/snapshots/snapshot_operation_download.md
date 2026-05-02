# DOWNLOAD Operation: `download_abs_manifest()`

[README](README.md) · DOWNLOAD Operation

**Location:** `ops/download.rs`

Downloads files from a data cache to the local filesystem. For snapshots, recreates the directory structure. For diffs, applies changes (download new/modified, delete removed).

```rust
pub async fn download_abs_manifest(
    manifest: &AbsManifest,
    data_cache: Arc<dyn AsyncDataCache>,
    options: DownloadOptions,
) -> Result<DownloadResult>
```

## Parameters

```rust
pub struct DownloadOptions {
    pub hash_cache: Option<Arc<HashCache>>,
    pub file_conflict_resolution: FileConflictResolution,
    pub apply_deletes: bool,
    pub symlink_policy: SymlinkPolicy,
    pub on_progress: Option<Box<dyn Fn(&DownloadStatistics) -> bool + Send + Sync>>,
    pub max_workers: Option<usize>,
    pub max_memory_bytes: Option<usize>,
}

pub enum FileConflictResolution {
    Skip,
    Overwrite,
    CreateCopy,
}
```

| Parameter | Description |
|-----------|-------------|
| `data_cache` | `Arc<dyn AsyncDataCache>` — S3 or filesystem source |
| `hash_cache` | Optional hash cache to skip downloads for unchanged files |
| `file_conflict_resolution` | How to handle existing files. Default `Overwrite`. |
| `apply_deletes` | If `true` (default), apply deletions from diff manifests |
| `symlink_policy` | Only `Preserve` (default) and `ExcludeAll` supported |
| `on_progress` | Callback for progress. Return `false` to cancel. |
| `max_workers` | Maximum parallel workers. Default: available CPUs. |
| `max_memory_bytes` | Maximum memory for buffering. Default: auto-detect. |

## Returns

```rust
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
```

The returned manifest has `mtime` values updated to match actual filesystem timestamps, essential for cross-platform workflows where mtime precision varies.

## Entry Type Handling

| Entry Type | Action |
|------------|--------|
| Regular file | Download using hash as key |
| Large file (chunk_hashes) | Download each chunk, concatenate |
| Symlink | Create symlink (topologically sorted) |
| Deleted file marker (diff) | Delete file if exists |
| Deleted directory marker (diff) | Delete directory if empty (`rmdir`) |
| Directory | Create directory with parents |

## File Conflict Resolution

| Resolution | Behavior |
|------------|----------|
| `Skip` | Skip download if file already exists |
| `Overwrite` | Overwrite existing file (default) |
| `CreateCopy` | Create new file with suffix (e.g., `file (1).ext`) |

When `hash_cache` is provided, files with matching hashes are skipped regardless of this setting.

## Related Documentation

- [Pipeline Architecture](snapshot_operation_download_pipeline.md) — tokio tasks, atomicity, chunked files
- [Hash Cache](snapshot_hash_cache.md) — skip optimization for unchanged files
- [Data Cache](snapshot_data_cache.md) — S3DataCache and FileSystemDataCache
