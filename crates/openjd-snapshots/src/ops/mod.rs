pub mod cache_sync;
pub mod collect;
pub mod compose;
pub mod diff;
pub mod download;
pub mod filter;
pub mod hash_op;
pub mod hash_upload;
pub mod join;
pub(crate) mod memory_pool;
pub mod partition;
mod rate;
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
pub use hash_op::{hash_abs_manifest, HashOptions, HashResult, HashStatistics};
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
