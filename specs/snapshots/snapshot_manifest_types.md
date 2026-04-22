# Manifest Types

[README](README.md) · Manifest Types

**Location:** `manifest.rs`

## Overview

The crate provides four concrete manifest types organized by two dimensions (path style × manifest kind), encoded as phantom type parameters on a single generic struct:

| | Snapshot (full capture) | Diff (changes only) |
|---|---|---|
| **Relative paths** | `Snapshot` | `SnapshotDiff` |
| **Absolute paths** | `AbsSnapshot` | `AbsSnapshotDiff` |

## Phantom Type Parameters

```rust
/// Marker types for path style (zero-sized, compile-time only).
pub struct Abs;
pub struct Rel;

/// Marker types for manifest kind.
pub struct Full;   // Snapshot — no deleted entries
pub struct Diff;   // SnapshotDiff — may contain deleted entries
```

Validation is enforced via trait bounds:

- `ValidatePaths` for `Abs` rejects relative paths; for `Rel` rejects absolute paths.
- `ValidateKind` for `Full` rejects `deleted=true` entries; for `Diff` allows them.

Operations that require absolute paths take `Manifest<Abs, _>` — passing a relative manifest is a compile error.

## Manifest Struct

```rust
pub struct Manifest<P, K> {
    pub hash_alg: HashAlgorithm,
    pub files: Vec<FileEntry>,
    pub dirs: Vec<DirEntry>,
    pub total_size: u64,
    pub parent_manifest_hash: Option<String>,
    pub file_chunk_size_bytes: i64,
    _phantom: PhantomData<(P, K)>,
}
```

## Type Aliases

```rust
pub type AbsSnapshot     = Manifest<Abs, Full>;
pub type AbsSnapshotDiff = Manifest<Abs, Diff>;
pub type Snapshot         = Manifest<Rel, Full>;
pub type SnapshotDiff     = Manifest<Rel, Diff>;
```

## Enum Wrappers

Enum wrappers group manifest types for function signatures where the exact type varies at runtime:

```rust
pub enum AbsManifest {
    Snapshot(AbsSnapshot),
    Diff(AbsSnapshotDiff),
}

pub enum RelManifest {
    Snapshot(Snapshot),
    Diff(SnapshotDiff),
}
```

Both provide accessor methods via a macro: `files()`, `dirs()`, `hash_alg()`, `file_chunk_size_bytes()`, `total_size()`, `parent_manifest_hash()`.

## Manifest Fields

| Field | Type | Description |
|-------|------|-------------|
| `hash_alg` | `HashAlgorithm` | Hashing algorithm (currently only `Xxh128`) |
| `files` | `Vec<FileEntry>` | File/symlink entries |
| `dirs` | `Vec<DirEntry>` | Directory entries (for empty dirs) |
| `total_size` | `u64` | Total size of all non-deleted, non-symlink files in bytes |
| `parent_manifest_hash` | `Option<String>` | Hash of parent manifest (for diffs) |
| `file_chunk_size_bytes` | `i64` | File chunk size for large file hashing (-1 = whole file) |

## Methods

| Method | Description |
|--------|-------------|
| `new(hash_alg, file_chunk_size_bytes)` | Create empty manifest |
| `with_files(files)` | Builder: set files and recompute total_size |
| `with_dirs(dirs)` | Builder: set dirs |
| `with_parent_hash(hash)` | Builder: set parent_manifest_hash |
| `clear_hashes()` | Set `hash` and `chunk_hashes` to `None` for all regular files |
| `recompute_total_size()` | Recalculate `total_size` from file entries |
| `validate()` | Run all validation checks (requires `P: ValidatePaths, K: ValidateKind`) |

## FileEntry

```rust
pub struct FileEntry {
    pub path: String,                       // POSIX-normalized
    pub hash: Option<String>,
    pub size: Option<u64>,
    pub mtime: Option<u64>,                 // microseconds since epoch
    pub chunk_hashes: Option<Vec<String>>,  // serialized as "chunkhashes"
    pub symlink_target: Option<String>,
    pub runnable: bool,
    pub deleted: bool,
}
```

### Convenience Constructors

| Constructor | Description |
|-------------|-------------|
| `FileEntry::new(path)` | Empty entry with normalized path |
| `FileEntry::file(path, size, mtime)` | Regular file with metadata |
| `FileEntry::symlink(path, target)` | Symlink entry |
| `FileEntry::deleted(path)` | Deletion marker |

All constructors apply `normalize_path()` to the path (and symlink target).

### Entry States

| State | `deleted` | `symlink_target` | `hash` | `chunk_hashes` | `size`/`mtime` |
|-------|-----------|------------------|--------|---------------|----------------|
| Deleted | true | None | None | None | None |
| Symlink | false | Set | None | None | None |
| Unhashed file | false | None | None | None | Required |
| Hashed (single) | false | None | Set | None | Required |
| Hashed (chunked) | false | None | None | Set | Required |

### Validation Rules

- **No duplicate paths:** A manifest must not contain multiple file entries with the same `path`, multiple directory entries with the same `path`, or a file entry and directory entry with the same `path`. Deserialization (`decode_v2023`, `decode_v2025`) also rejects manifests with duplicate paths.
- Deleted entries: only `path` and `deleted=true`; all other fields must be `None`/`false`
- Symlinks: `symlink_target` set; `hash` and `chunk_hashes` must be `None`
- Regular files: at most one of `hash` or `chunk_hashes`; must have `size` and `mtime`
- Chunkhashes count must equal `ceil(size / file_chunk_size_bytes)`
- Chunkhashes not allowed when `file_chunk_size_bytes == WHOLE_FILE_CHUNK_SIZE`

## DirEntry

```rust
pub struct DirEntry {
    pub path: String,
    pub deleted: bool,
}
```

Constructor `DirEntry::new(path)` normalizes the path.

### Implicit vs Explicit Directories

- Parent directories of files/symlinks are implicit — they don't need to be in `dirs`.
- Empty directories must be explicitly listed. The v2023 format does not support empty directories.

## ManifestEntry

An enum for filter operations that can reference either entry type:

```rust
pub enum ManifestEntry<'a> {
    File(&'a FileEntry),
    Dir(&'a DirEntry),
}
```

Provides `path()` accessor.

## Path Normalization

All paths use POSIX forward slash `/`. Normalization (in `path_util.rs`):

- On Windows: backslashes converted to `/`, `\\?\` prefix stripped
- On POSIX: backslashes preserved (valid filename characters)
- `.` and `..` components collapsed
- Trailing slashes removed (except root `/`)
- Empty string → empty string; only dots → `"."`

### Absolute Path Detection

A path is absolute if it starts with `/` or matches a Windows drive letter (`C:`, `C:/`).

## Serde Serialization

The `Manifest` struct uses `#[serde(rename_all = "camelCase")]` for JSON field names. Notable:

- `chunk_hashes` serializes as `"chunkhashes"` (not camelCase) via explicit `#[serde(rename)]`
- `runnable` and `deleted` are skipped when `false`
- `Option` fields are skipped when `None`
- `dirs` is skipped when empty
- The `_phantom` field is skipped via `#[serde(skip)]`

## HashAlgorithm

```rust
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HashAlgorithm {
    #[serde(rename = "xxh128")]
    Xxh128,
}
```

Only `xxh128` is supported. Provides `extension()` → `"xxh128"`.

## SymlinkPolicy

```rust
pub enum SymlinkPolicy {
    CollapseEscaping,
    CollapseAll,
    ExcludeEscaping,
    ExcludeAll,
    Preserve,
    TransitiveIncludeTargets,
}
```

See [snapshot_symlink_handling.md](snapshot_symlink_handling.md) for detailed documentation.

## File Chunked Hashing

| `file_chunk_size_bytes` | File Size | Hash Field |
|------------------------|-----------|------------|
| `WHOLE_FILE_CHUNK_SIZE` (-1) | Any | `hash` (whole file) |
| Positive value | ≤ chunk size | `hash` |
| Positive value | > chunk size | `chunk_hashes` |

When `chunk_hashes` is used, chunk count = `ceil(size / file_chunk_size_bytes)`.

## Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `DEFAULT_FILE_CHUNK_SIZE` | 256 MB | Default file chunk size |
| `WHOLE_FILE_CHUNK_SIZE` | -1 | Sentinel: hash whole file |
