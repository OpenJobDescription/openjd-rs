# HASH_UPLOAD Pipeline Architecture

[README](README.md) · [HASH_UPLOAD](snapshot_operation_hash_upload.md) · Pipeline Architecture

**Location:** `ops/hash_upload.rs`, `ops/memory_pool.rs`

## Pipeline Architecture

The Rust implementation replaces the Python two-thread-pool + manual memory pool with a `tokio` runtime and semaphore-based memory bounding:

```
                    ┌──────────────────────────────────┐
                    │  MemoryPool (tokio::sync::Semaphore)│
                    │  (bounded by max_memory_bytes)    │
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
  │  async upload tasks (tokio::spawn)                    │
  │  (S3 PutObject / multipart, or fs copy)               │
  │  release semaphore permits on completion              │
  └───────────────────────────────────────────────────────┘
```

### Key Design Points

1. **`tokio::sync::Semaphore`** with `max_memory_bytes / 4096` permits (4KB granularity) replaces the Python memory pool
2. **`tokio::task::spawn_blocking`** for disk I/O (read + hash) keeps the async runtime responsive
3. **`tokio::spawn`** for async S3 uploads via `aws-sdk-s3`
4. **`DashMap`** for concurrent upload deduplication instead of `Dict + Lock`

## Memory Pool

**Location:** `ops/memory_pool.rs`

```rust
pub(crate) struct MemoryPool {
    semaphore: Arc<Semaphore>,
    max_bytes: usize,
}

impl MemoryPool {
    pub fn new(max_bytes: usize) -> Self;
    pub async fn acquire(&self, size: usize) -> OwnedSemaphorePermit;
    pub fn max_bytes(&self) -> usize;
    pub fn available(&self) -> usize;
}
```

- 1 permit = 4KB (`PERMIT_GRANULARITY = 4096`). Allocations are rounded up to this granularity. This coarser granularity avoids `u32` overflow in `acquire_many_owned`, supporting pools up to ~16TB with `u32` permits.
- If `size > max_bytes`, it is clamped so a single large allocation can proceed once all other permits are released.
- Natural backpressure: when the semaphore is exhausted, `acquire()` awaits until uploads free memory.

### Default Memory Limit

```rust
pub(crate) fn default_max_memory_bytes() -> usize {
    // min(16GB, max(256MB, total_ram/4, available_ram - 1GB))
}
```

Detected via `/proc/meminfo` on Linux. Falls back to 256MB.

## Cache Check Architecture

All cache checks are performed inside the tokio task, not on the main thread. This parallelizes HeadObject calls and ensures skipped items never consume memory pool resources.

```
┌─────────────────────────────────────────────────────────────────┐
│                     Worker Task (per item)                      │
├─────────────────────────────────────────────────────────────────┤
│  1. Check hash cache (via Arc<HashCache>)                       │
│     └─► If hit + mtime match, get cached_hash                   │
│                                                                 │
│  2. Check if object exists in data cache:                       │
│     a. S3 check cache lookup                                    │
│     b. If miss, HeadObject call to S3                           │
│     └─► If exists, mark as skipped (no memory allocation)       │
│                                                                 │
│  3. If object doesn't exist (need to upload):                   │
│     a. memory_pool.acquire(file_size).await                     │
│     b. spawn_blocking: read file + compute hash                 │
│     c. If actual hash != cached_hash:                           │
│        - Re-check HeadObject with actual hash                   │
│        - If exists, skip upload (drop permit)                   │
│     d. Upload data (async)                                      │
│     e. Drop permit (releases memory)                            │
└─────────────────────────────────────────────────────────────────┘
```

## Concurrent Upload Deduplication

When multiple files or chunks have identical content (same hash), the pipeline prevents redundant concurrent uploads using `DashMap<String, tokio::sync::broadcast::Sender<()>>`:

1. **First task with hash:** Inserts hash into map with a new broadcast channel, proceeds to upload
2. **Subsequent tasks with same hash:** Find existing entry, subscribe to broadcast, await completion
3. **Upload completion:** First task sends on broadcast and removes hash from map
4. **Waiting tasks:** Wake up, mark as skipped, release memory

This is separate from the S3 check cache (which prevents re-uploading from previous operations). Concurrent deduplication handles duplicates within a single HASH_UPLOAD invocation.

## Error Handling

- If upload fails, the operation raises an error with details
- Partial uploads are not cleaned up (content-addressable storage is idempotent)
- The hash cache is updated even if upload fails (hash is still valid)
- Cancellation via `AtomicBool` checked by worker tasks

## Performance vs Python

| Aspect | Python | Rust |
|--------|--------|------|
| Hashing | GIL-limited threads | True parallel via `spawn_blocking` |
| Memory bounding | Manual memory pool with locks | `tokio::sync::Semaphore` (lock-free) |
| Upload deduplication | `Dict + Lock` | `DashMap` (lock-free) |
| S3 calls | boto3 (sync in threads) | `aws-sdk-s3` (native async) |
| Disk reads | 2× (hash then upload) or 1× (pipeline) | 1× (pipeline) |
