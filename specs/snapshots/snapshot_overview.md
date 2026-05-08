# Job Attachments Snapshots — Overview

[README](README.md) · Overview

## Overview

The `openjd-snapshots` crate captures directory tree snapshots, computes diffs, and transfers
data to/from content-addressed storage (S3 or local filesystem). It is a standalone library
with no dependency on `openjd-model` or `openjd-expr`.

A snapshot manifest is a data structure that captures a directory tree snapshot—similar to a
zip file's table of contents, but without the actual file content. It records metadata for
each file (path, size, modification time, content hash) and, in newer formats, directories
and symlinks. Operations on these manifests provide efficient change detection, uploads,
downloads, and content-addressable storage workflows.

## Glossary

| Term | Definition |
|------|------------|
| **Manifest** | A data structure describing a set of files, directories, and symlinks with their metadata. The general term encompassing both snapshots and diffs. |
| **Snapshot** | A manifest representing a complete point-in-time capture of a directory tree. Contains all entries that exist at that moment. |
| **Diff** | A manifest representing changes between two snapshots. Contains additions, modifications, and deletions (v2025+ format only). |
| **Absolute manifest** | A manifest where all paths are absolute filesystem paths (e.g., `/home/user/project/file.txt`). Required for filesystem operations. |
| **Relative manifest** | A manifest where all paths are relative to an unspecified root (e.g., `project/file.txt`). Portable across systems. |
| **File chunk** | A portion of a large file when file chunking is enabled. Large files are split into file chunks for parallel processing and storage. |
| **File chunking** | The process of splitting large files into smaller file chunks for hashing and storage. Controlled by `file_chunk_size_bytes`. |
| **Data cache** | Content-addressable storage where file data is stored using content hashes as keys. Can be S3 (`S3DataCache`) or local filesystem (`FileSystemDataCache`). |
| **Hash cache** | A local SQLite database that caches file hashes keyed by path, mtime, and byte range to avoid re-hashing unchanged files. |
| **S3 check cache** | A local SQLite database tracking which hashes are known to exist in S3, avoiding redundant HeadObject calls. |
| **Content-addressable storage** | Storage where data is keyed by its content hash, enabling deduplication and integrity verification. |
| **Escaping symlink** | A symlink whose target is outside the manifest's root directory or collected paths. |
| **Collapsing** | Replacing a symlink with the actual content at its target (file data or directory tree). |

## Quick Start

The most common workflow is collecting files, hashing, and uploading to S3:

```rust
use openjd_snapshots::{
    collect_abs_snapshot, hash_upload_abs_manifest,
    subtree_snapshot, S3DataCache, CollectOptions,
    HashUploadOptions, AbsManifest,
};
use std::sync::Arc;

// 1. Collect directory tree into a manifest (no hashing yet)
let manifest = collect_abs_snapshot(
    &["/projects/my_scene"],
    &[] as &[&str],
    CollectOptions::default(),
)?;

// 2. Hash and upload to S3 in a single pipelined pass
let data_cache = Arc::new(S3DataCache::new(/* ... */));
let result = hash_upload_abs_manifest(
    &AbsManifest::Snapshot(manifest),
    data_cache,
    HashUploadOptions::default(),
).await?;

// 3. Extract as relative-path manifest for storage/transport
let rel = match &result.manifest {
    AbsManifest::Snapshot(s) => subtree_snapshot(
        s, "/projects/my_scene",
        openjd_snapshots::SymlinkPolicy::CollapseEscaping,
    )?,
    _ => unreachable!(),
};

println!("Uploaded {} bytes", result.statistics.uploaded_bytes);
```

To download files from a manifest:

```rust
use openjd_snapshots::{
    join_snapshot, download_abs_manifest, DownloadOptions, AbsManifest,
};

// Convert relative-path manifest to absolute paths, then download
let abs = join_snapshot(&rel_manifest, "/local/destination")?;
download_abs_manifest(
    &AbsManifest::Snapshot(abs),
    data_cache,
    DownloadOptions::default(),
).await?;
```

## Use Cases

1. **Job submission** (`deadline bundle submit`) — Collect input assets, hash+upload to S3, partition into (root, relative manifest) pairs for the CreateJob API.
2. **Debug snapshot** (`deadline bundle submit --save-debug-snapshot`) — Same as above but use `FileSystemDataCache` to write to local disk for a portable zip.
3. **Download job output** (`deadline job download-output`) — Join output manifests to absolute paths, compose into one, download.
4. **Pre-populate data cache** (`deadline queue upload`) — Collect and hash+upload without needing the final manifest.
5. **Snapshot a directory** (`deadline manifest snapshot`) — Collect, optionally hash, extract as relative manifest.
6. **Upload from manifest** (`deadline attachment upload`) — Join to absolute paths, clear hashes, hash+upload, extract as relative.
7. **Compute diff** (`deadline manifest diff`) — Collect current state, filter, diff against parent.
8. **Sync between caches** (`openjd snapshot cache-sync`) — Copy all data referenced by a manifest from one data cache to another (S3→S3, S3→filesystem, filesystem→S3).

## Manifest Classes

The crate provides four concrete manifest types organized by two dimensions:

| | Snapshot (full capture) | Diff (changes only) |
|---|---|---|
| **Relative paths** | `Snapshot` | `SnapshotDiff` |
| **Absolute paths** | `AbsSnapshot` | `AbsSnapshotDiff` |

These are type aliases over `Manifest<P, K>` with phantom type parameters:

```rust
pub type AbsSnapshot     = Manifest<Abs, Full>;
pub type AbsSnapshotDiff = Manifest<Abs, Diff>;
pub type Snapshot         = Manifest<Rel, Full>;
pub type SnapshotDiff     = Manifest<Rel, Diff>;
```

Enum wrappers group them for function signatures:

```rust
pub enum AbsManifest { Snapshot(AbsSnapshot), Diff(AbsSnapshotDiff) }
pub enum RelManifest { Snapshot(Snapshot), Diff(SnapshotDiff) }
```

See [snapshot_manifest_types.md](snapshot_manifest_types.md) for detailed documentation.

## Data Cache Classes

```
AsyncDataCache (trait)
├── S3DataCache         (data on S3, with multipart + S3CheckCache)
└── FileSystemDataCache (data on a file system)
```

See [snapshot_data_cache.md](snapshot_data_cache.md) for detailed documentation.

## Operations

```
┌─────────────────────────────────────────────────────────────────────────┐
│                 FILE SYSTEM AND DATA CACHE OPERATIONS                   │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  1. COLLECT: Paths → AbsSnapshot                                        │
│         Scans directories and files into a snapshot (no hashing).       │
│                                                                         │
│  2. HASH: AbsManifest → AbsManifest                                     │
│         Computes hashes for all files in the manifest.                  │
│                                                                         │
│  3. HASH_UPLOAD: (AbsManifest, AsyncDataCache) → UploadResult           │
│         Hashes files and uploads them to a data cache.                  │
│                                                                         │
│  4. DOWNLOAD: (AbsManifest, AsyncDataCache) → DownloadResult            │
│         Downloads files from a data cache to local filesystem.          │
│                                                                         │
│  5. CACHE_SYNC: (AnyManifest, AsyncDataCache, AsyncDataCache)           │
│                                              → CacheSyncResult          │
│         Copies data between caches for all hashes in a manifest.        │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────┐
│                       MANIFEST TRANSFORMATIONS                          │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  6. FILTER: Manifest<P,K> → Manifest<P,K>                               │
│         Filters entries, returning only those that match.               │
│                                                                         │
│  7. DIFF: (Manifest<P,Full>, Manifest<P,Full>) → Manifest<P,Diff>       │
│         Computes the difference between two snapshots.                  │
│                                                                         │
│  8. COMPOSE:                                                             │
│     compose_snapshot_with_diffs → Manifest<P,Full>                       │
│     compose_diffs → Manifest<P,Diff>                                     │
│         Combines manifests sequentially.                                │
│                                                                         │
│  9. SUBTREE: Manifest → Snapshot or SnapshotDiff                         │
│         Extracts a subtree as a relative-path manifest.                 │
│                                                                         │
│ 10. PARTITION: AbsSnapshot → Vec<(String, Snapshot)>                     │
│         Splits a manifest into multiple (root, manifest) pairs.         │
│                                                                         │
│ 11. JOIN: RelManifest → AbsManifest (or RelManifest)                     │
│         Prepends a prefix to all paths.                                 │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

## Design Choices

1. **Phantom type parameters** for path style (`Abs`/`Rel`) and manifest kind (`Full`/`Diff`) give compile-time guarantees. Operations requiring absolute paths take `Manifest<Abs, _>` — passing a relative manifest is a compile error.
2. File system operations only work with absolute path manifests. Conversion to/from relative path manifests is via SUBTREE and JOIN.
3. Path separators are always POSIX forward slash `/` in manifest path strings.
4. Within a single manifest, all paths share the same style. Symlink targets are stored relative to the manifest root, not relative to the symlink location.
5. In snapshot diffs, directory deletions must be accompanied by deletion of all contents. Applying a directory deletion means `rmdir` if empty, not recursive delete.
6. There is no operation that uploads a pre-hashed manifest. HASH_UPLOAD always hashes data on its way into the cache, guaranteeing the content-addressed storage invariant.
7. Support for v2023 on-disk format is via lossy conversion (symlinks collapsed, empty dirs dropped, deletions dropped, `WHOLE_FILE_CHUNK_SIZE` required).
8. **`tokio` async runtime** for the transfer pipeline instead of thread pools. `spawn_blocking` for disk reads, `tokio::spawn` for async S3 calls.
9. **`tokio::sync::Semaphore`** for memory bounding instead of a custom memory pool. Acquire `file_size` permits before reading, release after upload completes.
10. **`DashMap`** for concurrent upload deduplication. Lock-free concurrent hash map avoids contention.
11. **`rayon`** for parallel hashing in the HASH operation. True CPU parallelism without GIL.
12. **Separate compose functions** (`compose_snapshot_with_diffs` and `compose_diffs`) make input/output type relationships explicit at compile time.

## Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `DEFAULT_FILE_CHUNK_SIZE` | 256 MB (`256 * 1024 * 1024`) | Default threshold for file chunked hashing |
| `WHOLE_FILE_CHUNK_SIZE` | -1 | Sentinel: hash whole file, no file chunks |
| `DEFAULT_S3_MULTIPART_PART_SIZE` | 32 MB (`32 * 1024 * 1024`) | Default part size for S3 multipart transfers |

## Module Organization

```
openjd-snapshots/src/
├── lib.rs              # Public API re-exports
├── manifest.rs         # Manifest<P,K>, FileEntry, DirEntry, SymlinkPolicy, enum wrappers
├── data_cache.rs       # AsyncDataCache, MultipartDataCache, RangeReadDataCache, FileSystemDataCache, S3DataCache
├── hash_cache.rs       # Local SQLite hash cache
├── s3_check_cache.rs   # Local SQLite S3 check cache
├── hash.rs             # HashAlgorithm, hash_data(), hash_file(), hash_file_chunked()
├── path_util.rs        # normalize_path(), is_absolute_path()
├── codec.rs            # Encode/decode v2023 and v2025 JSON formats
├── error.rs            # SnapshotError, Result<T>
└── ops/
    ├── mod.rs          # Re-exports
    ├── collect.rs      # COLLECT
    ├── hash_op.rs      # HASH
    ├── hash_upload.rs  # HASH_UPLOAD
    ├── download.rs     # DOWNLOAD
    ├── filter.rs       # FILTER
    ├── diff.rs         # DIFF
    ├── compose.rs      # COMPOSE
    ├── subtree.rs      # SUBTREE
    ├── partition.rs    # PARTITION
    ├── join.rs         # JOIN
    ├── cache_sync.rs   # CACHE_SYNC
    ├── memory_pool.rs  # Async memory pool (tokio::sync::Semaphore wrapper)
    └── rate.rs         # Sliding window rate calculator
```

## Dependency Graph

```
openjd-cli
  └── openjd-snapshots
        ├── aws-sdk-s3         (S3 data cache)
        ├── aws-sdk-sts        (account ID auto-detection)
        ├── rusqlite           (hash cache, S3 check cache)
        ├── xxhash-rust        (xxh128 hashing)
        ├── tokio              (async I/O, task spawning, semaphore)
        ├── rayon              (parallel hashing in HASH op)
        ├── serde / serde_json (manifest serialization)
        ├── glob               (IncludeExcludePathsFilter)
        ├── walkdir            (directory traversal in COLLECT)
        ├── async-trait        (AsyncDataCache trait)
        ├── dashmap            (concurrent upload deduplication)
        ├── tempfile           (atomic downloads)
        └── thiserror          (error types)
```
