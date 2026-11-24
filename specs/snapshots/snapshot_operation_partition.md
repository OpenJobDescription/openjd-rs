# PARTITION Operation: `partition_manifest()`

[README](README.md) · PARTITION Operation

**Location:** `ops/partition.rs`

Partitions a manifest into multiple `(root, Snapshot)` pairs, dividing entries by their root paths. Each `Snapshot` is an extracted subtree with paths relative to its root.

```rust
pub fn partition_manifest(
    manifest: &AbsSnapshot,
    options: PartitionOptions,
) -> Result<Vec<(String, Snapshot)>>
```

## Parameters

```rust
pub struct PartitionOptions {
    pub roots: Option<Vec<String>>,
    pub referenced_paths: Option<Vec<String>>,
    pub symlink_policy: SymlinkPolicy,
}
```

| Parameter | Description |
|-----------|-------------|
| `roots` | Optional list of root paths. No root may be a subpath of another. |
| `referenced_paths` | Paths referenced by the workload, influencing auto-root determination even if no files exist under them. |
| `symlink_policy` | How to handle escaping symlinks. Only `CollapseAll`, `CollapseEscaping`, `ExcludeAll` supported. |

## Returns

`Vec<(String, Snapshot)>` where each tuple is `(root_path, relative_manifest)`.

## Validation Rules

| Condition | Behavior |
|-----------|----------|
| A root is given more than once after normalization (`/a` and `/a/`) | Error: `root '<root>' is given more than once` |
| A root is a subpath of another root (`/a` under `/`, `a` under `.`) | Error: `root '<nested>' is a subpath of root '<outer>'` |
| `Preserve` or `TransitiveIncludeTargets` policy | Error |

A path lies under a root by the rule `subtree_snapshot()` uses to select a
root's entries: it equals the root or continues it after a `/`, a root that
ends in `/` (`/`, `C:/`) is a prefix as-is, and `.` contains every relative
path.

## Output Ordering

1. First: entries for each explicitly provided root (in `roots` order)
2. Then: auto-determined roots for remaining entries (sorted alphabetically)

If no entries exist under an explicitly provided root, its `Snapshot` is empty but still included.

## Auto-Root Determination

Roots are derived from the paths a root must contain: the manifest's
[common-root contributors](snapshot_manifest_types.md#common-root) (each
non-deleted file's parent directory and each non-deleted empty directory)
plus the normalized `referenced_paths`. Paths are grouped by volume: the
POSIX root `/`, each drive root (`C:/`), and the relative root `.`. Paths on
different volumes share no directory. UNC paths normalize to `/server/...`,
so they fall on the `/` volume.

| Scenario | Behavior |
|----------|----------|
| `roots` is `None` | One root per volume: the longest common directory of that volume's paths. For a single-volume manifest without `referenced_paths` this is `Manifest::common_root()`. |
| `roots` provided | Provided roots first, then roots for the paths no provided root contains. Per volume, that is their longest common directory when it lies below the volume root; otherwise one root per top-level directory (`/p/q` and `/r` give `/p/q` and `/r`, not `/`). A remaining path that is the volume root itself, such as the parent of a file directly under `/`, is covered by the volume root. |

Every entry is in exactly one partition. A derived root can contain a
provided root: with `roots: ["/x"]` and files `/x/a.txt` and `/f.txt`, the
derived root is `/`. A derived root's partition leaves out the entries
under any provided root, so that example gives `("/x", [a.txt])` and
`("/", [f.txt])`. A symlink in a derived root's partition whose target is
under a provided root escapes that partition: `symlink_policy` collapses it
to a copy of its target (`CollapseEscaping`) or leaves it out
(`ExcludeEscaping`), so no partition holds a link to a target it lacks.

**Divergence from Python.** The Python job attachments uploader
(`S3AssetManager._get_asset_groups`) also groups by volume, but gives the
paths outside the provided roots one root per volume, their longest common
directory, which is often `/`. PARTITION splits that remainder per
top-level directory instead, so a job's roots name the directories it uses:
files `/p/q/b.txt` and `/var/tmp/g.txt` outside `roots: ["/x"]` give the
roots `/p/q` and `/var/tmp`, where Python gives `/`.

The common-directory rule is `longest_common_dir_prefix()` in `path_util.rs`,
the implementation `common_root()` uses. It keeps a root's trailing slash
(`/`, `C:/`) and gives `.` for relative paths that share no directory.
Non-empty directories do not contribute, because their descendants already
imply them: a manifest with dir `/a/b` and file `/a/b/c/f.txt` partitions at
`/a/b/c`.

## Implementation Details

- `Manifest::root_contributors()` lists the paths a root must contain, for both `common_root()` and partitioning
- `path_util.rs` holds the path rules: `parent_dir()`, `volume_root()`, `longest_common_dir_prefix()`, and `strip_root()` (the under-a-root test, shared with `subtree_snapshot()`)
- The unit test `auto_root_matches_common_root` pins single-volume auto-partitioning to `common_root()`
- Each partition is extracted with the subtree rules of `subtree_snapshot()`; a derived root's extraction also leaves out the provided roots it contains (`subtree_excluding()` in `ops/subtree.rs`)

## Example

```rust
use openjd_snapshots::{partition_manifest, PartitionOptions};

// Auto-partition (no roots provided)
let partitions = partition_manifest(&manifest, PartitionOptions::default())?;
// Result: [("/projects/scene", Snapshot with relative paths)]

// Explicit roots with remainder
let partitions = partition_manifest(&manifest, PartitionOptions {
    roots: Some(vec!["/projects/scene".into()]),
    ..Default::default()
})?;
// Result: [
//   ("/projects/scene", Snapshot),      // explicit root
//   ("/data/shared", Snapshot),          // auto-determined remainder
// ]
```

## Relationship to SUBTREE and JOIN

PARTITION is conceptually the inverse of multiple JOIN + COMPOSE:

```rust
let [(root1, rel1), (root2, rel2)] = partition_manifest(&abs_manifest, opts)?;

// Inverse:
let abs1 = join_snapshot(&rel1, &root1)?;
let abs2 = join_snapshot(&rel2, &root2)?;
// compose abs1 and abs2 ≈ original abs_manifest
```
