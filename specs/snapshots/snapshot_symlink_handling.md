# Symlink Handling

[README](README.md) · Symlink Handling

## Symlink Representation in Manifests

Symlinks are stored as `FileEntry` entries with `symlink_target` set:

| Field | Value |
|-------|-------|
| `path` | The symlink's path |
| `symlink_target` | Target path (relative to manifest root, not symlink location) |
| `hash` | Always `None` |
| `size` | Always `None` |
| `mtime` | Always `None` |

### Target Storage Format

Symlink targets are stored relative to the manifest root, not relative to the symlink location (unlike POSIX filesystem symlinks). This simplifies manifest operations since targets can be looked up directly in the manifest's path index.

The target path style always matches the manifest's path style: absolute in `AbsSnapshot`/`AbsSnapshotDiff`, relative in `Snapshot`/`SnapshotDiff`. For example, COLLECT produces an `AbsSnapshot` so symlink targets are absolute paths. SUBTREE converts to a `Snapshot` and rebases symlink targets to relative paths. Validation enforces that `symlink_target` paths conform to the manifest's path style.

## SymlinkPolicy Enum

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

| Policy | Description |
|--------|-------------|
| `CollapseEscaping` | Preserve symlinks whose targets are included; collapse escaping symlinks |
| `CollapseAll` | Collapse all symlinks to files/directories |
| `ExcludeEscaping` | Preserve symlinks whose targets are included; exclude escaping symlinks |
| `ExcludeAll` | Exclude all symlinks from the result |
| `Preserve` | Keep all symlinks as-is (absolute paths only) |
| `TransitiveIncludeTargets` | Keep symlinks and add their targets (COLLECT only) |

### Policy Support by Operation

| Policy | COLLECT | SUBTREE | PARTITION | DOWNLOAD |
|--------|---------|---------|-----------|----------|
| `CollapseEscaping` | ✓ (default) | ✓ (default) | ✓ (default) | — |
| `CollapseAll` | ✓ | ✓ | ✓ | — |
| `ExcludeEscaping` | ✓ | ✓ | — | — |
| `ExcludeAll` | ✓ | ✓ | ✓ | ✓ |
| `Preserve` | ✓ | — | — | ✓ (default) |
| `TransitiveIncludeTargets` | ✓ | — | — | — |

## Escaping vs Non-Escaping Symlinks

A symlink is "escaping" if its target is outside the relevant root path:

- **COLLECT:** Target is outside the collected directories/files
- **SUBTREE:** Target is outside the new subtree root
- **PARTITION:** Target is outside the partition's root

## Escaping Detection (COLLECT)

COLLECT uses a two-pass algorithm for `CollapseEscaping` and `ExcludeEscaping`:

**Pass 1 — Build the collected set:**
1. Walk all directories, collect all non-symlink files and directories
2. Collect non-symlink files from `filenames` and `optional_filenames`
3. Defer all symlinks for later processing
4. Result: a `HashSet<String>` of all collected paths

**Pass 2 — Process deferred symlinks:**
For each `DeferredSymlink`, check if its resolved target is within the collected set (direct match or prefix match).

**Why two passes?** A single-pass approach cannot correctly identify escaping symlinks because the full collected set isn't known until all paths are visited.

## Collapsing Behavior

When a symlink is collapsed, it is replaced with the actual content at its target:

**File target:** The symlink entry becomes a regular file entry using the symlink's path but the target's metadata.

**Directory target:** The symlink is replaced with the entire directory tree at the target. All entries appear under the symlink's path. Nested symlinks are handled recursively.

In SUBTREE, collapsing looks up targets in the manifest data (no filesystem access). The `resolve_symlink()` helper follows symlink chains up to 64 hops, detecting cycles. The `expand_dir_symlink()` helper remaps directory contents under the symlink's path.

## Cycle Detection

| Policy | Behavior |
|--------|----------|
| `Preserve` | No recursion; symlinks recorded as-is |
| `ExcludeAll` | No recursion; all symlinks skipped |
| `ExcludeEscaping` | Cycles in escaping symlinks skipped |
| `CollapseAll` | Cycles detected; cyclic symlink skipped |
| `CollapseEscaping` | Cycles detected when collapsing escaping symlinks |
| `TransitiveIncludeTargets` | Cycles detected; cyclic target skipped |

Cycle detection uses a `HashSet<String>` of visited paths. When a cycle is detected, the cyclic symlink is silently skipped.

## SUBTREE Symlink Handling

SUBTREE operates on manifest data without filesystem access. When re-rooting, symlinks that were "within root" may now "escape" the new subtree root.

- Collapsing looks up targets in the manifest, not the filesystem
- Missing targets are excluded
- `Preserve` and `TransitiveIncludeTargets` are not supported
- Preserved symlinks have their targets rebased by removing the subtree prefix

## DOWNLOAD Symlink Handling

DOWNLOAD creates symlinks on the filesystem. Only `Preserve` (default) and `ExcludeAll` are supported.

For chained symlinks, targets are created before symlinks via topological sorting of the dependency graph using `std::os::unix::fs::symlink`.

## Choosing a Policy

| Use Case | Recommended Policy |
|----------|-------------------|
| Job submission (portable manifest) | `CollapseEscaping` (default) |
| Debug snapshot (complete capture) | `CollapseAll` or `TransitiveIncludeTargets` |
| Exclude all symlinks | `ExcludeAll` |
| Preserve symlink structure (absolute paths only) | `Preserve` |
