// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Content-addressed directory tree snapshots with S3 integration.
//!
//! This crate captures directory tree snapshots, computes diffs, and transfers data
//! to/from content-addressed storage (S3 or local filesystem). It is a standalone
//! library with no dependency on `openjd-model` or `openjd-expr`.
//!
//! # Manifest Types
//!
//! A manifest describes a set of files, directories, and symlinks with their metadata.
//! Four concrete types are organized by two dimensions — path style and manifest kind:
//!
//! | | Full snapshot | Diff (changes only) |
//! |---|---|---|
//! | **Relative paths** | [`Snapshot`] | [`SnapshotDiff`] |
//! | **Absolute paths** | [`AbsSnapshot`] | [`AbsSnapshotDiff`] |
//!
//! These are type aliases over [`Manifest<P, K>`](Manifest) with phantom type parameters
//! that provide compile-time safety for path style and manifest kind.
//!
//! # Typical Workflows
//!
//! **Upload** (collect → hash+upload → extract relative manifest):
//! ```text
//! COLLECT → AbsSnapshot → HASH_UPLOAD → AbsSnapshot (hashed) → SUBTREE → Snapshot
//! ```
//!
//! **Download** (join to absolute paths → download):
//! ```text
//! Snapshot → JOIN → AbsSnapshot → DOWNLOAD → files on disk
//! ```
//!
//! **Incremental sync** (diff → compose → upload only changes):
//! ```text
//! COLLECT → DIFF(old, new) → SnapshotDiff → COMPOSE(base, diffs) → Snapshot
//! ```
//!
//! # Operations
//!
//! | Operation | Function | Description |
//! |-----------|----------|-------------|
//! | COLLECT | [`collect_abs_snapshot`] | Walk filesystem into an `AbsSnapshot` |
//! | HASH | [`hash_abs_manifest`] | Compute content hashes (CPU-parallel via rayon) |
//! | HASH_UPLOAD | [`hash_upload_abs_manifest`] | Hash and upload in a single pipelined pass |
//! | DOWNLOAD | [`download_abs_manifest`] | Download files from content-addressed storage |
//! | DIFF | [`diff_snapshots`] | Compute changes between two snapshots |
//! | COMPOSE | [`compose_snapshot_with_diffs`], [`compose_diffs`] | Layer manifests together |
//! | FILTER | [`filter_manifest`] | Include/exclude paths by glob pattern |
//! | SUBTREE | [`subtree_snapshot`] | Extract and rebase a subdirectory |
//! | JOIN | [`join_snapshot`] | Prepend a root path to make paths absolute |
//! | PARTITION | [`partition_manifest`] | Split by root directories |
//! | CACHE_SYNC | [`cache_sync_manifest`] | Copy data between caches (S3↔filesystem) |

pub mod codec;
pub mod data_cache;
pub mod error;
pub mod hash;
pub mod hash_cache;
pub mod manifest;
pub mod ops;
pub mod path_util;
pub mod s3_check_cache;

pub use codec::{
    decode_manifest, decode_v2023, decode_v2025, encode_abs_snapshot_diff_v2025,
    encode_abs_snapshot_v2025, encode_snapshot_diff_v2023, encode_snapshot_diff_v2025,
    encode_snapshot_v2023, encode_snapshot_v2025, DecodedManifest, ManifestFormat,
};
pub use data_cache::{
    AsyncDataCache, ContentAddressedDataCache, CopyResult, FileSystemDataCache, S3DataCache,
};
pub use error::{Result, SnapshotError};
pub use hash::{
    human_readable_file_size, HashAlgorithm, DEFAULT_FILE_CHUNK_SIZE,
    DEFAULT_S3_MULTIPART_PART_SIZE, WHOLE_FILE_CHUNK_SIZE,
};
pub use hash_cache::HashCache;
pub use manifest::{
    AbsManifest, AbsSnapshot, AbsSnapshotDiff, DirEntry, FileEntry, Manifest, ManifestEntry,
    ManifestRef, RelManifest, Snapshot, SnapshotDiff, SymlinkPolicy,
};
pub use ops::{
    cache_sync_manifest, collect_abs_snapshot, compose_diffs, compose_snapshot_with_diffs,
    diff_snapshots, download_abs_manifest, entries_differ, filter_manifest, hash_abs_manifest,
    hash_upload_abs_manifest, join_manifest, join_manifest_rel, join_snapshot, join_snapshot_diff,
    join_snapshot_diff_rel, join_snapshot_rel, partition_manifest, partition_rel_manifest,
    subtree_manifest, subtree_rel_manifest, subtree_rel_snapshot, subtree_rel_snapshot_diff,
    subtree_snapshot, subtree_snapshot_diff, CacheSyncOptions, CacheSyncResult,
    CacheSyncStatistics, CollectOptions, DiffOptions, DownloadOptions, DownloadResult,
    DownloadStatistics, FileConflictResolution, HashOptions, HashResult, HashStatistics,
    HashUploadOptions, IncludeExcludePathsFilter, PartitionOptions, UploadResult, UploadStatistics,
};
pub use s3_check_cache::S3CheckCache;
