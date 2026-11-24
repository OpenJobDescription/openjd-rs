# SUBTREE Operation

[README](README.md) · SUBTREE Operation

**Location:** `ops/subtree.rs`

Extracts a subtree from a manifest, producing a new manifest rooted at the specified subdirectory. The output always has relative paths.

Rust provides separate functions for each input type:

```rust
pub fn subtree_snapshot(
    manifest: &AbsSnapshot, subtree: &str, symlink_policy: SymlinkPolicy,
) -> Result<Snapshot>

pub fn subtree_snapshot_diff(
    manifest: &AbsSnapshotDiff, subtree: &str, symlink_policy: SymlinkPolicy,
) -> Result<SnapshotDiff>

pub fn subtree_rel_snapshot(
    manifest: &Snapshot, subtree: &str, symlink_policy: SymlinkPolicy,
) -> Result<Snapshot>

pub fn subtree_rel_snapshot_diff(
    manifest: &SnapshotDiff, subtree: &str, symlink_policy: SymlinkPolicy,
) -> Result<SnapshotDiff>
```

## Parameters

| Parameter | Description |
|-----------|-------------|
| `manifest` | Source manifest to extract from |
| `subtree` | Path to the subtree root, or `"."` / `""` for identity transformation |
| `symlink_policy` | How to handle symlinks escaping the new root. Default `CollapseEscaping`. |

## Identity Subtree (`"."` or `""`)

Acts as an identity transformation that applies the `symlink_policy` without rebasing paths. Useful for:
- Collapsing all symlinks before serialization to v2023 format
- Excluding all symlinks from a manifest

Requires relative-path input (since output is always relative).

## Conceptual Model

SUBTREE is a virtual re-rooting operation:

```
Original: /projects/scene/assets/textures/wood.png
SUBTREE(manifest, "/projects/scene/assets/textures")
Result:   wood.png
```

The operation:
1. Filters to entries within the subtree (via `strip_prefix()`)
2. Rebases paths relative to the new root
3. Handles symlinks according to `symlink_policy`

## Path Style Requirements

The `subtree` path must match the manifest's path style:

| Manifest Paths | Subtree Path | Valid |
|----------------|--------------|-------|
| Absolute | Absolute | ✓ |
| Relative | Relative | ✓ |
| Absolute | Relative | ✗ |
| Relative | Absolute | ✗ |

## What Gets Preserved / Dropped

- `file_chunk_size_bytes` — preserved
- `parent_manifest_hash` — NOT preserved (root changed, hash invalid)

## Symlink Handling

| Policy | Within subtree | Escaping subtree |
|--------|---------------|-----------------|
| `CollapseAll` | Collapsed | Collapsed |
| `CollapseEscaping` | Preserved (target rebased) | Collapsed |
| `ExcludeAll` | Excluded | Excluded |
| `ExcludeEscaping` | Preserved (target rebased) | Excluded |

`Preserve` and `TransitiveIncludeTargets` are not supported — escaping symlinks cannot be represented in relative-path output.

### Symlink Resolution

SUBTREE operates on manifest data without filesystem access. Collapsing uses:

- `FileIndex` — the manifest's files by path and in path order, so the files under a directory are found by binary search. A directory is a path with no entry and files under it; a volume root (`/`, `D:/`) is matched as a prefix as-is, like `strip_root()`.
- `resolve_target()` — follows symlink chains up to 64 hops, detecting cycles via `HashSet`, to a file entry or a directory
- `Collapse` — collapses one top-level symlink: a file target becomes a copy of the file entry, a directory target is expanded under the symlink's path (`expand_dir()`), and a missing target or a cycle gives nothing

A symlink whose resolved directory is the subtree root, a directory being expanded, or a directory containing one of those is a cycle and is skipped, since expanding it would copy a directory into itself. Each nested directory symlink is expanded at most once per top-level symlink; when directory links converge, a later path to an already expanded nested directory symlink omits it. Nested file symlinks are copied on every path. This bounds the output of one top-level symlink by the number of nested directory symlinks plus one, times the number of files. The file index is built only when a symlink needs collapsing, so a subtree without one costs no more than the path filter.

## Example

```rust
use openjd_snapshots::{subtree_snapshot, SymlinkPolicy};

// Extract textures subtree from absolute manifest
let textures = subtree_snapshot(
    &full_manifest,
    "/projects/scene/assets/textures",
    SymlinkPolicy::CollapseEscaping,
)?;

for entry in &textures.files {
    println!("{}", entry.path);  // "wood.png", "metal.png", etc.
}
```

## Relationship to FILTER

| Operation | Purpose | Path Transformation |
|-----------|---------|---------------------|
| FILTER | Keep entries matching a predicate | Paths unchanged |
| SUBTREE | Extract entries under a path prefix | Paths rebased to new root |

## Relationship to JOIN

SUBTREE and JOIN are inverse operations:

```rust
let rel = subtree_snapshot(&abs_manifest, "/assets/textures", policy)?;
let abs = join_snapshot(&rel, "/assets/textures")?;
// abs has the same paths as the original entries under /assets/textures
```
