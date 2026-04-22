# Hash Cache and S3 Check Cache

[README](README.md) · Hash Cache

**Location:** `hash_cache.rs`, `s3_check_cache.rs`

## Hash Cache

The hash cache is a local SQLite database that stores file hashes keyed by path, modification time, and byte range. It enables HASH, HASH_UPLOAD, and DOWNLOAD operations to skip re-hashing or re-downloading unchanged files.

### Properties

| Property | Value |
|----------|-------|
| Location | `~/.deadline/job_attachments/hash_cache.db` |
| Format | SQLite with WAL journaling |
| Thread safety | `Mutex<rusqlite::Connection>` |

### Schema

Table `hashesV4`:

| Field | Type | Description |
|-------|------|-------------|
| `file_path` | blob | Absolute file path (UTF-8 encoded) |
| `hash_algorithm` | text | Hash algorithm (e.g., `xxh128`) |
| `range_start` | integer | Start byte offset (0 for whole-file) |
| `range_end` | integer | End byte offset (-1 for whole-file) |
| `file_hash` | text | The computed hash value |
| `last_modified_time` | timestamp | File mtime when hash was computed |

Primary key: `(file_path, hash_algorithm, range_start, range_end)`.

### API

```rust
pub struct HashCache {
    conn: Mutex<rusqlite::Connection>,
}

impl HashCache {
    pub fn new(cache_dir: impl AsRef<Path>) -> Result<Self>;
    pub fn open_default() -> Result<Self>;  // ~/.deadline/job_attachments/

    pub fn get(&self, file_path: &Path, algorithm: &str,
               range_start: i64, range_end: i64) -> Option<(String, u64)>;

    pub fn put(&self, file_path: &Path, algorithm: &str,
               range_start: i64, range_end: i64,
               hash: &str, mtime: u64) -> Result<()>;

    pub fn get_if_fresh(&self, file_path: &Path, algorithm: &str,
                        range_start: i64, range_end: i64,
                        current_mtime: u64) -> Option<String>;
}
```

### Whole-File vs Byte-Range Hashes

| Entry Type | `range_start` | `range_end` | Description |
|------------|---------------|-------------|-------------|
| Whole-file | 0 | `WHOLE_FILE_RANGE_END` (-1) | Hash of entire file |
| Byte-range | ≥ 0 | > 0 | Hash of bytes in range [start, end) |

Byte-range support enables efficient caching for file chunked files. Different chunk sizes can coexist.

### Cache Lookup Behavior

1. Query by `(file_path, hash_algorithm, range_start, range_end)`
2. If entry exists, compare `last_modified_time` with current file mtime
3. If mtime matches, return cached hash (hit)
4. If mtime differs or entry missing, return `None` (miss)

### Usage by Operation

**HASH:**

| Scenario | Behavior |
|----------|----------|
| `hash_cache` provided, `force_rehash=false` | Check cache; use cached hash on hit |
| `hash_cache` provided, `force_rehash=true` | Always compute hash, update cache |
| `hash_cache` is `None` | Always compute hash, no caching |

**HASH_UPLOAD:**
1. Hash cache hit + S3 check cache hit → skip entirely
2. Hash cache hit + S3 miss → must re-read and re-hash (cached hash not trusted for upload)
3. Hash cache miss → read, hash, upload, update cache

**DOWNLOAD:**
1. Local file exists with matching mtime in hash cache
2. Cached hash matches manifest hash → skip download
3. Otherwise → download from data cache

### Thread Safety

The `Mutex<rusqlite::Connection>` serializes all database access. WAL journaling allows concurrent readers during writes. For the HASH operation, `rayon` parallel workers share the single `HashCache` via `Arc<HashCache>`.

## S3 Check Cache

**Location:** `s3_check_cache.rs`

Local SQLite database tracking which hashes are known to exist in S3, avoiding redundant `HeadObject` calls.

### Properties

| Property | Value |
|----------|-------|
| Location | `~/.deadline/job_attachments/s3_check_cache.db` |
| Format | SQLite with WAL journaling |
| Expiry | 30 days |

### Schema

Table `s3checkV1`:

| Field | Type | Description |
|-------|------|-------------|
| `s3_key` | text (primary key) | S3 object key |
| `last_seen_time` | timestamp | When the entry was last verified |

### API

```rust
pub struct S3CheckCache {
    conn: Mutex<rusqlite::Connection>,
}

impl S3CheckCache {
    pub fn new(cache_dir: impl AsRef<Path>) -> Result<Self>;
    pub fn open_default() -> Result<Self>;

    pub fn get_entry(&self, s3_key: &str) -> Option<String>;  // Returns None if expired
    pub fn put_entry(&self, s3_key: &str) -> Result<()>;
}
```

### Expiry

Entries older than 30 days (`ENTRY_EXPIRY_DAYS`) are treated as expired and return `None` from `get_entry()`. Expired entries are also pruned from the database when the cache is opened via `new()`, keeping the database from growing unboundedly across sessions.

### Probabilistic Validation

During HASH_UPLOAD, the S3 check cache is validated inline:

| Item Number | Verification | Rationale |
|-------------|--------------|-----------|
| 1–100 | Always HeadObject | Catch stale cache early |
| 101+ | 1% random sample | Balance coverage vs. performance |

If any verification fails, the cache is invalidated, deleted, and all previously-skipped items are re-queued.

See [snapshot_operation_hash_upload_s3.md](snapshot_operation_hash_upload_s3.md) for details.

## Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `WHOLE_FILE_RANGE_END` | -1 | Sentinel for hash cache `range_end` indicating whole-file hash |
| `ENTRY_EXPIRY_DAYS` | 30 | S3 check cache entry expiry in days |

## Future Work: Hash Cache Eviction

The hash cache currently has no eviction mechanism. Entries are invalidated at read time when the file's mtime changes, but stale entries for deleted or renamed files accumulate indefinitely.

To support eviction, the `hashesV4` schema would need a new column to track when each entry was last accessed or written — for example, a `last_accessed_time` timestamp updated on both `put` and `get_if_fresh` hits. A prune-on-open step (similar to the S3 check cache) could then delete entries older than a configurable threshold (e.g., 90 days since last access).

Schema change (would require a new table version, e.g., `hashesV5`):

```sql
CREATE TABLE hashesV5(
    file_path blob,
    hash_algorithm text,
    range_start integer,
    range_end integer,
    file_hash text,
    last_modified_time timestamp,
    last_accessed_time timestamp,
    PRIMARY KEY (file_path, hash_algorithm, range_start, range_end)
);
```

Considerations:
- The Python `HashCache` in `deadline-cloud` also lacks eviction. Any schema change should be coordinated so both implementations can read the same database, or a migration path should be provided.
- Updating `last_accessed_time` on every cache hit adds a write to every read path. This could be mitigated by batching updates or only updating when the existing timestamp is older than some threshold (e.g., 1 day).
- An alternative to time-based eviction is size-based: prune the least-recently-accessed entries when the database exceeds a configured size. This is more complex but bounds disk usage directly.
