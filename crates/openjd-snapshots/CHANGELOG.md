# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
## [0.2.0](https://github.com/OpenJobDescription/openjd-rs/compare/0.1.0...0.2.0) - 2026-05-14

### Bug fixes

- Reject invalid file_chunk_size_bytes in Manifest::validate()

- Add EB unit, expand join/cache_sync tests, update specs

- Add public API spec, reject empty paths, fix spec errors

- Cache error handling, docs, and test coverage

- Address quality evaluation findings

- Interleave directory creation with file download submission

- Symlink validation and hash upload pipeline

- Atomic downloads with random temp files, update MemoryPool spec

- Address snapshots quality evaluation findings

- Reject duplicate paths in manifests

- Handle ARN-based buckets in S3 CopyObject copy source

- Resolve CI test failures on macOS and Windows

- Replace streaming channel pattern with collect+write in S3DataCache

- Use TempDir in snapshot tests

- Resolve all clippy warnings, rustfmt, and rustdoc issues

- Address all findings from quality evaluation


### CI

- Add copyright header and THIRD-PARTY-LICENSES freshness checks

- Add dry-run release-plz automation for crates.io publishing


### Documentation

- Update Kiro-generated report on snapshots crate


### Features

- Rename CLI binary to `openjd`; mark snapshots and for-js experimental; add per-crate READMEs

- [**breaking**] Make hash_upload_abs_manifest and download_abs_manifest async

- Add Manifest::common_root() primitive and decode_v2023_as_diff

- Restore file mtime from manifest on download

- Implement concurrent upload deduplication in hash_upload

- Standardize error handling across all openjd-rs crates

- Windows cross-user helper and path_format parameter

- Use agentic workflow to port Python OpenJD to Rust


### Miscellaneous

- Switch to dual Apache-2.0 OR MIT license with attribution tooling

- Update Cargo dependencies to latest versions


### Performance

- Consolidate each crate's integration tests into a single binary


### Refactor

- [**breaking**] Remove sync ContentAddressedDataCache trait

- Address three quality-evaluation recommendations

- Make path_util crate-private

- Address six quality-evaluation recommendations

