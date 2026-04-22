# Job Attachments Snapshots — Rust Design Specs

This directory contains the design specifications for the `openjd-snapshots` crate,
ported from the Python design docs in `deadline-cloud/docs/design/job_attachments_snapshots*.md`
and adapted for the idiomatic Rust implementation.

## Documents

| Document | Description |
|----------|-------------|
| [snapshot_overview.md](snapshot_overview.md) | Top-level overview, glossary, use cases, operations summary, design choices |
| [snapshot_manifest_types.md](snapshot_manifest_types.md) | Manifest structs, entry types, phantom type parameters, validation, serde |
| [snapshot_data_cache.md](snapshot_data_cache.md) | `ContentAddressedDataCache` / `AsyncDataCache` traits, `S3DataCache`, `FileSystemDataCache` |
| [snapshot_symlink_handling.md](snapshot_symlink_handling.md) | `SymlinkPolicy` enum, escaping detection, collapsing, cycle handling |
| [snapshot_hash_cache.md](snapshot_hash_cache.md) | Local SQLite hash cache and S3 check cache |
| [snapshot_error_handling.md](snapshot_error_handling.md) | `SnapshotError` enum, error conversion strategy, message conventions |
| [snapshot_operation_collect.md](snapshot_operation_collect.md) | COLLECT — `collect_abs_snapshot()` |
| [snapshot_operation_hash.md](snapshot_operation_hash.md) | HASH — `hash_abs_manifest()` |
| [snapshot_operation_hash_upload.md](snapshot_operation_hash_upload.md) | HASH_UPLOAD — `hash_upload_abs_manifest()` |
| [snapshot_operation_hash_upload_pipeline.md](snapshot_operation_hash_upload_pipeline.md) | HASH_UPLOAD pipeline: tokio tasks, memory pool, deduplication |
| [snapshot_operation_hash_upload_s3.md](snapshot_operation_hash_upload_s3.md) | HASH_UPLOAD S3: multipart uploads, cache validation, streaming |
| [snapshot_operation_download.md](snapshot_operation_download.md) | DOWNLOAD — `download_abs_manifest()` |
| [snapshot_operation_download_pipeline.md](snapshot_operation_download_pipeline.md) | DOWNLOAD pipeline: tokio tasks, atomicity, chunked files, S3 multi-part |
| [snapshot_operation_filter.md](snapshot_operation_filter.md) | FILTER — `filter_manifest()` |
| [snapshot_operation_diff.md](snapshot_operation_diff.md) | DIFF — `diff_snapshots()` |
| [snapshot_operation_compose.md](snapshot_operation_compose.md) | COMPOSE — `compose_snapshot_with_diffs()`, `compose_diffs()` |
| [snapshot_operation_subtree.md](snapshot_operation_subtree.md) | SUBTREE — `subtree_snapshot()`, `subtree_snapshot_diff()`, etc. |
| [snapshot_operation_partition.md](snapshot_operation_partition.md) | PARTITION — `partition_manifest()` |
| [snapshot_operation_join.md](snapshot_operation_join.md) | JOIN — `join_snapshot()`, `join_snapshot_diff()`, etc. |
| [snapshot_operation_cache_sync.md](snapshot_operation_cache_sync.md) | CACHE_SYNC — `cache_sync_manifest()` |
