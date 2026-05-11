# openjd-snapshots

> **Status: experimental.** This crate is under active development and its
> public API may change without notice between releases. Its v2025 on-disk
> manifest format is an **experimental draft** whose wire format is
> expected to change. The v2023 on-disk format is stable and is the format
> that [AWS Deadline Cloud] uses.

[AWS Deadline Cloud]: https://aws.amazon.com/deadline-cloud/

Content-addressed directory tree snapshots with S3 integration. Part of
the [`openjd-rs`] workspace. Independent of the other `openjd-*` crates —
it does not depend on `openjd-model` or `openjd-expr`.

[`openjd-rs`]: https://github.com/OpenJobDescription/openjd-rs

## What this crate provides

A pipeline of operations on directory tree manifests:

| Operation | Function | Description |
|-----------|----------|-------------|
| COLLECT | `collect_abs_snapshot` | Walk filesystem into an `AbsSnapshot` (metadata only, no hashing yet). |
| HASH | `hash_abs_manifest` | Compute content hashes in parallel (rayon, xxHash3). |
| HASH_UPLOAD | `hash_upload_abs_manifest` | Hash and upload to content-addressed storage in a single pipelined pass. |
| DOWNLOAD | `download_abs_manifest` | Download files from content-addressed storage, with atomicity and S3 multi-part support. |
| DIFF | `diff_snapshots` | Compute changes between two snapshots. |
| COMPOSE | `compose_snapshot_with_diffs` | Layer diffs onto a base snapshot. |
| FILTER | `filter_manifest` | Include/exclude by glob pattern. |
| SUBTREE | `subtree_snapshot` | Extract and rebase a subdirectory. |
| JOIN | `join_snapshot` | Prepend a root to make relative paths absolute. |
| PARTITION | `partition_manifest` | Split by root directories. |
| CACHE_SYNC | `cache_sync_manifest` | Copy between content-addressed stores (S3 ↔ filesystem). |

Backed by:

- **xxHash3 (128-bit)** for content addressing (via
  [`xxhash-rust`](https://crates.io/crates/xxhash-rust)).
- **tokio** for async pipelines.
- **rayon** for CPU-parallel hashing.
- **aws-sdk-s3** (AWS SDK for Rust) for S3 operations, via the `rustls
  0.23 + aws-lc` TLS stack.
- **SQLite** (bundled, via [`rusqlite`](https://crates.io/crates/rusqlite))
  for local hash and S3-presence caches.

## On-disk manifest formats

Two JSON manifest formats are supported, with very different maturity:

- **v2023** — **stable.** The on-disk format that [AWS Deadline Cloud]'s
  job attachments use. Identified on disk by
  `"manifestVersion": "2023-03-03"`. Entry points: `encode_snapshot_v2023`,
  `encode_snapshot_diff_v2023`, `decode_v2023`, `decode_v2023_as_diff`.
- **v2025** — **experimental draft.** A proposed evolution adding
  deletions, explicit directories, symlinks, and file chunking. Identified
  on disk by `specificationVersion` strings that include the
  `beta-2025-12` tag. The wire format is expected to change before any
  stable release; do not use it for long-term storage or for interop with
  external systems.

## Compile-time safety

Manifests are strongly typed by path style and manifest kind via
phantom type parameters:

| | Full snapshot | Diff (changes only) |
|---|---|---|
| **Relative paths** | `Snapshot` | `SnapshotDiff` |
| **Absolute paths** | `AbsSnapshot` | `AbsSnapshotDiff` |

Functions that require absolute paths take `AbsSnapshot` / `AbsManifest`,
so passing a relative manifest is a compile error.

## Quick example

```rust,ignore
use openjd_snapshots::{
    collect_abs_snapshot, CollectOptions, encode_snapshot_v2023,
    SymlinkPolicy, subtree_snapshot,
};

// 1. Walk the filesystem into an AbsSnapshot (no hashing yet).
let snap = collect_abs_snapshot(
    &["/path/to/project"],
    &[] as &[&str],
    CollectOptions::default(),
)?;

// 2. Hash + upload in a pipelined pass (requires an S3DataCache).
//    See the module docs for `hash_upload_abs_manifest`.

// 3. Extract a relative manifest for storage or transport.
let rel = subtree_snapshot(&snap, "/path/to/project", SymlinkPolicy::CollapseEscaping)?;

// 4. Encode to v2023 JSON (the stable Deadline Cloud format).
let json = encode_snapshot_v2023(&rel)?;
# Ok::<(), openjd_snapshots::SnapshotError>(())
```

## Minimum supported Rust version

Rust **1.92**. Enforced in CI.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-Apache-2.0] or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT License ([LICENSE-MIT] or <http://opensource.org/licenses/MIT>)

at your option.

[LICENSE-Apache-2.0]: LICENSE-Apache-2.0
[LICENSE-MIT]: LICENSE-MIT

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.

## Security

See <https://aws.amazon.com/security/vulnerability-reporting/> or email
[AWS Security](mailto:aws-security@amazon.com) to report a potential
security issue. Please do not create a public GitHub issue.
