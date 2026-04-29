# DIFF Operation: `diff_snapshots()`

[README](README.md) · DIFF Operation

**Location:** `ops/diff.rs`

Computes the difference between two snapshot manifests.

```rust
pub fn diff_snapshots<P: Clone>(
    parent: &Manifest<P, Full>,
    current: &Manifest<P, Full>,
    options: &DiffOptions,
) -> Result<Manifest<P, Diff>>
```

## Parameters

```rust
pub struct DiffOptions {
    pub parent_manifest_hash: Option<String>,
    pub ignore_hashes: bool,
    pub preserve_runnable: bool,
}
```

| Parameter | Description |
|-----------|-------------|
| `parent` | The parent snapshot manifest |
| `current` | The current snapshot manifest |
| `parent_manifest_hash` | Optional hash of the parent manifest (stored in output diff) |
| `ignore_hashes` | If `true`, compare by metadata only (size, mtime, runnable) — fast mode |
| `preserve_runnable` | If `true`, copy `runnable` from parent for modified files (cross-platform) |

## Returns

`Manifest<P, Diff>` with:
- `parent_manifest_hash` if provided
- New/modified entries with full content
- Deleted entries with `deleted=true` markers (including explicit directory deletion markers)

## Comparison Modes

| Mode | `ignore_hashes` | Comparison Fields |
|------|-----------------|-------------------|
| Full | `false` | hash, chunk_hashes, size, mtime, runnable |
| Fast | `true` | size, mtime, runnable only |

## Hash State Validation

When `ignore_hashes=false`, both manifests must have compatible hash states:

| Parent State | Current State | Result |
|--------------|---------------|--------|
| Hashed | Hashed | ✓ Proceeds |
| Unhashed | Unhashed | ✓ Proceeds |
| Empty/symlinks-only | Any | ✓ Proceeds |
| Hashed | Unhashed | ✗ Error |
| Unhashed | Hashed | ✗ Error |

## Entry Comparison: `entries_differ()`

```rust
pub fn entries_differ(
    parent: &FileEntry,
    current: &FileEntry,
    ignore_hashes: bool,
    ignore_runnable: bool,
) -> bool
```

- Type transitions (file ↔ symlink) are always different
- Symlinks compared by `symlink_target` only
- Regular files compared by hash/chunk_hashes (unless `ignore_hashes`), size, mtime, and runnable (unless `ignore_runnable`)

The final parameter is called `ignore_runnable` rather than `preserve_runnable` because at this layer it only controls the comparison — it cannot "preserve" anything, since `entries_differ` returns a `bool` and never constructs a diff entry. Callers who want the full [`preserve_runnable` behaviour](#the-preserve_runnable-option) pass `DiffOptions::preserve_runnable` through as `ignore_runnable`; the "copy parent's runnable into the diff" half happens in `diff_snapshots` itself. This matches the Python reference (`_entries_differ(..., ignore_runnable=...)`).

## The `preserve_runnable` Option

The `DiffOptions::preserve_runnable` flag controls two behaviours of `diff_snapshots` at once:

1. **Comparison**: `runnable` is ignored when deciding whether a file changed. This is delegated to `entries_differ` via its `ignore_runnable` parameter.
2. **Preservation**: when a file *is* reported as modified (size, mtime, or hash differs), the parent entry's `runnable` value is copied into the diff entry.

### Motivation

The `runnable` field captures the POSIX execute bit. On Windows, all files report `runnable=false`. This creates a cross-platform problem:

1. Manifest created on POSIX with `script.sh` having `runnable=true`
2. User modifies `script.sh` on Windows
3. New manifest has `runnable=false`
4. Diff shows modification, but applying back to POSIX incorrectly removes the execute bit

With `preserve_runnable=true`:
- `runnable` differences alone do not mark a file as modified.
- When a file is modified for other reasons, the diff carries the parent's `runnable`.
- New files (not present in the parent) always use the current manifest's `runnable`.

## Implementation

Builds a `HashMap<&str, &FileEntry>` index of the parent, then:
1. Iterates current entries to detect additions and modifications
2. Iterates parent index for deletions
3. When a directory is deleted, all its contents receive explicit deletion markers

## Example

```rust
use openjd_snapshots::{diff_snapshots, DiffOptions};

let diff = diff_snapshots(
    &parent_snapshot,
    &current_snapshot,
    &DiffOptions {
        parent_manifest_hash: Some(parent_hash),
        ignore_hashes: false,
        preserve_runnable: true,
    },
)?;

for entry in &diff.files {
    if entry.deleted {
        println!("deleted: {}", entry.path);
    } else {
        println!("new/modified: {}", entry.path);
    }
}
```
