# DOWNLOAD Pipeline Architecture

[README](README.md) · [DOWNLOAD](snapshot_operation_download.md) · Pipeline Architecture

**Location:** `ops/download.rs`

## Parallel Download Architecture

The DOWNLOAD operation uses a `tokio` runtime with bounded concurrency:

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    DOWNLOAD PIPELINE                                    │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  Regular Files:                                                         │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │  1. tokio::spawn → pre-allocate temp file + handle conflicts     │   │
│  │  2. For large files (≥ 2 × part size): parallel part downloads   │   │
│  │     For small files: single get_object request                   │   │
│  │  3. Atomic replace (std::fs::rename) + restore mtime             │   │
│  └──────────────────────────────────────────────────────────────────┘   │
│                                                                         │
│  Chunked Files (chunks stored separately in data cache):                │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │  1. Pre-allocate temp file (truncate to exact size)              │   │
│  │  2. tokio::spawn all chunk downloads in parallel                 │   │
│  │     - Each chunk writes to non-overlapping byte range            │   │
│  │  3. All chunks done + verified → atomic replace + restore mtime  │   │
│  └──────────────────────────────────────────────────────────────────┘   │
│                                                                         │
│  Memory bounded by MemoryPool (tokio::sync::Semaphore)                  │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

## Interleaved Directory Creation

Directories are created before submitting file downloads for entries within them:

```rust
// Directories sorted by path (parents before children)
for dir_path in sorted_dirs {
    create_directory(dir_path);
    for entry in files_in_dir {
        pipeline.submit_file(entry);  // Downloads start immediately
    }
}
```

## Atomic File Downloads

All downloads are atomic to ensure target files are never partial or corrupt:

1. **Temporary file creation:** Download to temp file beside target (e.g., `myfile.dat.tmp<random_hex>`)
2. **Atomic move:** `std::fs::rename()` atomically moves temp to final location
3. **Error cleanup:** Temp file deleted on any error

This applies to all three download paths: small whole-file, multipart, and chunked.

## Hash Verification

Every byte written is checked against the manifest hash it was downloaded
for before the temp file is renamed into place:

| Download path | Check |
|---------------|-------|
| Small whole file, small chunk (single `get_object`) | The data is hashed in memory before it is written |
| Whole file in byte-range parts | The assembled temp file is hashed once all parts are written |
| Chunk in byte-range parts | Each such chunk's byte range of the temp file is hashed once all chunks are written |

A mismatch deletes the temp file and fails the download with an `Io` error
of kind `InvalidData`: `hash mismatch for <path>: expected <hash>, got <hash>`
for a whole file, or `hash mismatch for chunk at offset <offset>: expected
<hash>, got <hash>` for a chunk.

This is stricter than the Python job attachments downloader, which writes
downloaded objects without hashing them. A content-addressed object that does
not match its key indicates a corrupt or tampered cache, so DOWNLOAD fails
rather than write it.

Before any filesystem change, each chunked file's `chunk_hashes` must match
the manifest's chunk size: the file must be larger than one chunk and have
one hash per chunk, the rule `Manifest::validate` applies. Otherwise the
download fails with a validation error such as `file '<path>' with size 12
should have 4 chunks (chunk_size=3), got 2`.

## S3 Multi-Part Download

Files and chunks of at least `2 × multipart_part_size` (default 64MB; `multipart_threshold()` in `data_cache.rs`) use parallel byte-range requests via `get_object_range()` or `stream_range_to_file_at_offset()`:

```
For a 100MB file with 32MB parts:

  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐
  │ Part 0   │ │ Part 1   │ │ Part 2   │ │ Part 3   │
  │ 0-32MB   │ │ 32-64MB  │ │ 64-96MB  │ │ 96-100MB │
  └────┬─────┘ └────┬─────┘ └────┬─────┘ └────┬─────┘
       │            │            │            │
       ▼            ▼            ▼            ▼
  ┌─────────────────────────────────────────────────────┐
  │              Pre-allocated Temp File (100MB)        │
  │  [part 0 region][part 1 region][part 2 region][p3]  │
  └─────────────────────────────────────────────────────┘
```

Each part uses `Range: bytes={start}-{end}` and writes to its non-overlapping region of the pre-allocated temp file. No locking required.

## Parallel Chunked File Downloads

Files exceeding the chunk size are stored as multiple chunks in the data cache. All chunks download in parallel:

1. Pre-allocate temp file to exact size via `truncate(size)` (sparse file on supporting filesystems)
2. Submit all chunk downloads to the tokio runtime
3. Each chunk writes to `offset = chunk_index × file_chunk_size_bytes`; the chunk ranges are `split_ranges(size, file_chunk_size_bytes)`, the same ranges the hash cache keys chunks by
4. Once every chunk has completed, chunks fetched in byte-range parts are verified (see [Hash Verification](#hash-verification)), then the temp file is moved into place and its mtime restored
5. On any chunk failure or hash mismatch, temp file deleted, first error propagated

## Deletion Ordering

Deletions (from diff manifests) are processed longest-path-first:

1. Directories only deleted if empty (uses `rmdir()`, not recursive delete)
2. Explicit deletion required: diff must list all files/subdirs before parent directory
3. Untracked files preserve the directory

## Modification Time Restoration

Downloaded files have `mtime` set to the manifest value via `filetime` or `std::fs::set_permissions`, preserving original timestamps. The returned manifest has `mtime` updated to match actual filesystem values.

## Symlink Handling

For chained symlinks, targets are created before symlinks via topological sorting. Created via `std::os::unix::fs::symlink`.
