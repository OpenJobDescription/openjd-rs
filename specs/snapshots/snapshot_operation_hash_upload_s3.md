# HASH_UPLOAD S3-Specific Behavior

[README](README.md) · [HASH_UPLOAD](snapshot_operation_hash_upload.md) · S3 Behavior

## Multipart Upload

Multipart upload is used when uploading to `S3DataCache` and the file/chunk size is at least `2 × multipart_part_size` (default threshold: 64MB with 32MB parts). The threshold is `multipart_threshold()` in `data_cache.rs`, shared with DOWNLOAD and CACHE_SYNC.

### Multipart Coordination

The `AsyncDataCache` trait provides multipart operations:

```rust
async fn create_multipart_upload(&self, hash: &str, algorithm: &str) -> io::Result<String>;
async fn upload_part(&self, hash: &str, algorithm: &str, upload_id: &str,
                     part_number: i32, data: Vec<u8>) -> io::Result<String>;
async fn complete_multipart_upload(&self, hash: &str, algorithm: &str, upload_id: &str,
                                   parts: Vec<(i32, String)>) -> io::Result<()>;
async fn abort_multipart_upload(&self, hash: &str, algorithm: &str,
                                 upload_id: &str) -> io::Result<()>;
```

**Coordination flow** (`upload_multipart()` in `data_cache.rs`, shared by HASH_UPLOAD and CACHE_SYNC):
1. `create_multipart_upload` → get `upload_id`
2. Parts are the `split_ranges(size, part_size)` byte ranges. Each is read and uploaded concurrently; each returns `(part_number, etag)` on success
3. Once every part has uploaded, the parts are sorted by number and `complete_multipart_upload` is called
4. On any failure, including a failed read or a failed completion, `abort_multipart_upload` is called and the first error is returned

## Large Unchunked Files: Two Passes

When file chunking is disabled (`WHOLE_FILE_CHUNK_SIZE`), the data cache supports multipart upload, and a file is at least the multipart threshold, the file is read twice:

```
┌─────────────────────────────────────────────────────────────────┐
│  Pass 1: Compute Hashes (discard data)                          │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  Check the file size on disk matches the manifest       │    │
│  │  Read part 1 → hash (whole file + part) → discard       │    │
│  │  Read part 2 → hash (whole file + part) → discard       │    │
│  │  ...                                                    │    │
│  │  Result: whole-file hash + per-part hashes              │    │
│  └─────────────────────────────────────────────────────────┘    │
│                                                                 │
│  HeadObject check with the whole-file hash                      │
│  └─► If exists, skip upload (done)                              │
│                                                                 │
│  Pass 2: Read, Verify, and Upload Parts                         │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  Create S3 multipart upload session                     │    │
│  │  For each part (concurrently):                          │    │
│  │    1. spawn_blocking: read part + compute part hash     │    │
│  │    2. Verify part hash matches pass 1                   │    │
│  │       └─► If mismatch: abort, return error              │    │
│  │    3. Upload part                                       │    │
│  └─────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

**Why two passes?**
1. S3 object keys are based on content hash — must know hash before creating multipart session
2. Discarding data in pass 1 avoids buffering the entire file
3. If object already exists (HeadObject hit), skip pass 2 entirely

**Hash verification:** Pass 1 is `hash_file_and_parts()`, which reads the file through the same chunk reader as the HASH operation, so a file whose size on disk differs from the manifest fails before anything is uploaded. Each part read in pass 2 is checked against its pass 1 hash. If a part differs (the file changed between passes), the upload is aborted and an `Io` error of kind `InvalidData` is returned: `<path> changed while uploading: part at offset <offset> hashed <before> before the upload and <after> during it`. The object stored under a hash is therefore always the content that produced that hash.

## Probabilistic S3 Cache Validation

The S3 check cache can become stale if objects are deleted from S3. HASH_UPLOAD performs inline validation:

```
For each item where S3 check cache says "exists":

  Item 1-100:     Always verify with HeadObject
  Item 101+:      1% random sampling

  If ANY verification fails:
  1. Mark cache as invalid
  2. Delete S3 check cache database
  3. Re-queue all previously skipped items
  4. Continue without cache
```

### Sampling Strategy

| Item Number | Verification | Rationale |
|-------------|--------------|-----------|
| 1–100 | Always HeadObject | Catch stale cache early |
| 101+ | 1% random sample | Balance coverage vs. performance |

### Expected HeadObject Overhead (Warm Cache)

| Total Items | Verified | Time (8 workers) |
|-------------|----------|------------------|
| 100 | 100 | ~0.2s |
| 1,000 | 109 | ~0.2s |
| 10,000 | 199 | ~0.4s |
| 100,000 | 1,099 | ~2.1s |

### Recovery Flow

1. **Detect:** HeadObject returns 404 for cached item
2. **Invalidate:** Delete S3 check cache database
3. **Re-queue:** Collect all items skipped due to cache
4. **Retry:** Re-submit skipped items (now use HeadObject directly)
5. **Continue:** Process remaining items without cache

## FileSystem Data Cache

For `FileSystemDataCache`, behavior is simpler:
- No multipart uploads (single-file writes)
- Existence check via filesystem `exists()`
- Unchunked files are read once into memory, hashed, and written; there is no two-pass path
