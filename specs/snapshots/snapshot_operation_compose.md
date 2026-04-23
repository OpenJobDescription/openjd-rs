# COMPOSE Operation

[README](README.md) · COMPOSE Operation

**Location:** `ops/compose.rs`

Layers multiple manifests together into a single manifest, as if applying each manifest as a set of changes in order. Rust provides two separate functions making the type relationships explicit at compile time:

```rust
pub fn compose_snapshot_with_diffs<P: Clone>(
    base: &Manifest<P, Full>,
    diffs: &[&Manifest<P, Diff>],
) -> Manifest<P, Full>

pub fn compose_diffs<P: Clone>(
    diffs: &[&Manifest<P, Diff>],
) -> Result<Manifest<P, Diff>>
```

## Composition Semantics

The result represents the directory tree you would get by:
1. Starting with the first manifest's directory tree
2. Applying each subsequent manifest as a "patch" — adding new entries, updating modified entries, removing deleted entries

### Snapshot + Diffs → Snapshot

`compose_snapshot_with_diffs(base, &[diff1, diff2])`:
- `deleted=true` markers remove entries from the result
- Output is a `Manifest<P, Full>` (no deletion markers)

### Diffs → Diff

`compose_diffs(&[diff1, diff2, diff3])`:
- Tracks cumulative changes including deletion markers
- `parent_manifest_hash` comes from the first diff
- Output includes both current entries AND deletion markers

## Trie-Based Implementation

Both functions use a trie (prefix tree) where each node represents a path component. The trie structurally models the real filesystem, with directories implicit in the tree structure rather than tracked separately:

```rust
struct TrieNode {
    children: HashMap<String, TrieNode>,
    file_entry: Option<FileEntry>,
    deleted: bool,
}
```

A node is a **file** if it has a `file_entry`. A node is a **directory** if it has no `file_entry` and is not deleted. This means directories don't need separate tracking — they are derived from the trie structure during collection.

### Why a Trie?

1. **Structural filesystem model:** The trie mirrors the directory hierarchy, so directory existence is implicit from having children
2. **Efficient path operations:** Insert, lookup, delete are O(path depth)
3. **Cascading deletions:** Directory subtrees can be efficiently removed
4. **Unified tracking:** Files and directories are managed by the same data structure, preventing state divergence

### Snapshot + Diffs

For each diff applied in order:
1. File deletions remove subtrees via `delete_subtree`
2. Directory deletions remove only empty directories via `delete_if_empty` (deepest first)
3. File additions/modifications insert or update nodes
4. Directory additions create nodes in the trie

The final trie contains only entries that exist after all diffs are applied. Directories are collected by traversing the trie for non-file, non-deleted nodes.

### Diff + Diffs

For each diff applied in order:
1. File deletions set `deleted=true` via `mark_deleted` (does NOT clear children)
2. Directory deletions set `deleted=true` via `mark_deleted`
3. File additions clear `deleted` and set `file_entry`
4. Directory additions clear `deleted`

After all diffs are applied, `reconcile_deleted_flags()` handles directories that were deleted then re-populated. Files and directories (including deletion markers) are collected from the trie.

### The Reconciliation Step

`reconcile_deleted_flags()` handles this scenario:
1. diff1 deletes `/dir/` and all its contents
2. diff2 adds `/dir/newfile.txt`

After diff2, `/dir/` must NOT be marked as deleted because it has a non-deleted child. The reconciliation traverses the trie depth-first and clears `deleted` on any node with non-deleted descendants.

### Key Design Point: `mark_deleted` Does Not Clear Children

In `compose_diffs`, `mark_deleted` sets the `deleted` flag and clears `file_entry`, but does NOT clear children. This is critical because a directory deletion marker in a diff only represents "this directory was empty and deleted" — it does not cascade to children that may have been added by later diffs.

## Validation Rules

| Condition | Behavior |
|-----------|----------|
| Empty diffs list | Returns base unchanged (snapshot+diffs) or error (diffs-only) |
| Manifests have different `file_chunk_size_bytes` | Error |

## Example

```rust
use openjd_snapshots::{compose_snapshot_with_diffs, compose_diffs};

// Apply diffs to a base snapshot
let final_state = compose_snapshot_with_diffs(&base, &[&diff1, &diff2]);
// final_state is a Snapshot with deletions applied

// Combine multiple diffs
let combined = compose_diffs(&[&diff1, &diff2, &diff3])?;
// combined is a SnapshotDiff with deletion markers preserved
```

## Relationship to PARTITION and JOIN

PARTITION is conceptually the inverse of multiple JOIN operations followed by COMPOSE:

```rust
let [(root1, rel1), (root2, rel2)] = partition_manifest(&abs_manifest, opts)?;

// Inverse:
let abs1 = join_snapshot(&rel1, &root1)?;
let abs2 = join_snapshot(&rel2, &root2)?;
let composed = compose_snapshot_with_diffs(&abs1, &[]);
// Then merge abs2 entries...
```
