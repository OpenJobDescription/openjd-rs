// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

/// CACHE_SYNC: copy a manifest's data from one data cache to another.
pub mod cache_sync;
/// COLLECT: walk the filesystem into an absolute snapshot.
pub mod collect;
/// COMPOSE: layer snapshot diffs onto a base snapshot, or onto each other.
pub mod compose;
/// DIFF: compute the changes between two snapshots.
pub mod diff;
/// DOWNLOAD: write a manifest's files to disk from a data cache.
pub mod download;
/// FILTER: keep only the manifest entries that match a predicate.
pub mod filter;
/// HASH: compute content hashes for a manifest's files.
pub mod hash_op;
/// HASH_UPLOAD: hash a manifest's files and upload them in one pipelined pass.
pub mod hash_upload;
/// JOIN: prepend a root path to a manifest's paths.
pub mod join;
pub(crate) mod memory_pool;
/// PARTITION: split a snapshot by root directory.
pub mod partition;
mod rate;
/// SUBTREE: extract a subdirectory of a snapshot and rebase its paths.
pub mod subtree;

/// Progress callback type used across operations.
pub type ProgressFn<S> = dyn Fn(&S) -> bool + Send + Sync;

pub use cache_sync::{cache_sync_manifest, CacheSyncOptions, CacheSyncResult, CacheSyncStatistics};
pub use collect::{collect_abs_snapshot, CollectOptions};
pub use compose::{compose_diffs, compose_snapshot_with_diffs};
pub use diff::{diff_snapshots, entries_differ, DiffOptions};
pub use download::{
    download_abs_manifest, DownloadOptions, DownloadResult, DownloadStatistics,
    FileConflictResolution,
};
pub use filter::{filter_manifest, IncludeExcludePathsFilter};
pub use hash_op::{
    hash_abs_manifest, hash_abs_snapshot, hash_abs_snapshot_diff, HashOptions, HashResult,
    HashStatistics,
};
pub use hash_upload::{
    hash_upload_abs_manifest, HashUploadOptions, UploadResult, UploadStatistics,
};
pub use join::{
    join_manifest, join_manifest_rel, join_snapshot, join_snapshot_diff, join_snapshot_diff_rel,
    join_snapshot_rel,
};
pub use partition::{partition_manifest, partition_rel_manifest, PartitionOptions};
pub use subtree::{
    subtree_manifest, subtree_rel_manifest, subtree_rel_snapshot, subtree_rel_snapshot_diff,
    subtree_snapshot, subtree_snapshot_diff,
};
