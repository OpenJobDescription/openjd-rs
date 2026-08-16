// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

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

/// Reject manifests whose regular (non-symlink, non-deleted) files already
/// carry a whole-file hash or chunk hashes.
pub(crate) fn validate_files_not_hashed(files: &[crate::manifest::FileEntry]) -> crate::Result<()> {
    for file in files {
        if file.symlink_target.is_none()
            && !file.deleted
            && (file.hash.is_some() || file.chunk_hashes.is_some())
        {
            return Err(crate::SnapshotError::Validation(format!(
                "file already has hashes set, cannot re-hash: {}",
                file.path
            )));
        }
    }
    Ok(())
}

/// Look up the cached per-chunk hashes for a file, returning them only when
/// every chunk is fresh in the cache and the file has at least one chunk.
pub(crate) fn cached_chunk_hashes(
    cache: &crate::hash_cache::HashCache,
    path: &std::path::Path,
    alg: &str,
    chunk_size: u64,
    file_size: u64,
    mtime: u64,
) -> Option<Vec<String>> {
    let mut hashes = Vec::new();
    let mut offset: u64 = 0;
    while offset < file_size {
        let end = std::cmp::min(offset + chunk_size, file_size);
        let h = cache.get_if_fresh(path, alg, offset as i64, end as i64, mtime)?;
        hashes.push(h);
        offset = end;
    }
    if hashes.is_empty() {
        return None;
    }
    Some(hashes)
}

/// Record per-chunk hashes for a file in the hash cache (best effort:
/// individual cache write failures are ignored).
pub(crate) fn put_chunk_hashes(
    cache: &crate::hash_cache::HashCache,
    path: &std::path::Path,
    alg: &str,
    chunk_size: u64,
    file_size: u64,
    hashes: &[String],
    mtime: u64,
) {
    let mut offset: u64 = 0;
    for h in hashes {
        let end = std::cmp::min(offset + chunk_size, file_size);
        let _ = cache.put(path, alg, offset as i64, end as i64, h, mtime);
        offset = end;
    }
}

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
