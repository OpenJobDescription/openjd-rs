// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Consolidated integration-test binary.
//!
//! Each `tests/integration/test_*.rs` file is included as a module here so
//! cargo links one test executable for this crate instead of one per file.
//!
//! S3 integration tests live in `integration/test_s3_integration.rs` and
//! every test there is `#[ignore]`d. Run them with:
//!     cargo test -p openjd-snapshots --test integration -- --ignored test_s3_integration::

#[path = "integration/test_cache_sync.rs"]
mod test_cache_sync;
#[path = "integration/test_chunk_size.rs"]
mod test_chunk_size;
#[path = "integration/test_codec.rs"]
mod test_codec;
#[path = "integration/test_collect.rs"]
mod test_collect;
#[path = "integration/test_compose.rs"]
mod test_compose;
#[path = "integration/test_diff.rs"]
mod test_diff;
#[path = "integration/test_download.rs"]
mod test_download;
#[path = "integration/test_error_messages.rs"]
mod test_error_messages;
#[path = "integration/test_filter.rs"]
mod test_filter;
#[path = "integration/test_hash.rs"]
mod test_hash;
#[path = "integration/test_hash_upload.rs"]
mod test_hash_upload;
#[path = "integration/test_join.rs"]
mod test_join;
#[path = "integration/test_manifest.rs"]
mod test_manifest;
#[path = "integration/test_partition.rs"]
mod test_partition;
#[path = "integration/test_round_trip.rs"]
mod test_round_trip;
#[path = "integration/test_s3_data_cache.rs"]
mod test_s3_data_cache;
#[path = "integration/test_s3_integration.rs"]
mod test_s3_integration;
#[path = "integration/test_subtree.rs"]
mod test_subtree;
#[path = "integration/test_upload_dedup.rs"]
mod test_upload_dedup;
#[path = "integration/test_v2023_canonical.rs"]
mod test_v2023_canonical;
