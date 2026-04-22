# openjd-snapshots Quality Evaluation Report

**Date:** 2026-04-13
**Crate:** `openjd-snapshots` v0.1.0
**Location:** `~/openjd-rs/crates/openjd-snapshots`

## Executive Summary

The `openjd-snapshots` crate is a well-architected Rust library implementing content-addressed file tree snapshot operations for job attachments. It supports S3 and filesystem backends, v2023 and v2025 manifest formats, and provides a comprehensive set of operations (COLLECT, HASH, HASH_UPLOAD, DOWNLOAD, DIFF, COMPOSE, FILTER, SUBTREE, PARTITION, JOIN, CACHE_SYNC).

The crate compiles cleanly with zero warnings, and all 942 tests pass (3 ignored S3 integration tests requiring real AWS credentials). The specifications are thorough and well-organized. The implementation is generally high quality with good use of Rust idioms.

However, the evaluation identified several issues ranging from a potential correctness bug to performance concerns and specification gaps. These are detailed below.

---

## 1. Compilation and Test Results

- **Build:** Clean compilation, zero errors, zero warnings.
- **Tests:** 942 passed, 0 failed, 3 ignored (S3 integration tests).
- **Test binaries:** 20 test binaries covering unit tests (inline `#[cfg(test)]` modules) and 16 integration test files.
- **Quality probe tests:** 5 additional tests written during this evaluation all pass, confirming identified issues.

---

## 2. Specifications Review

### 2.1 Files Reviewed

| File | Topic | Assessment |
|------|-------|------------|
| `README.md` | Index | Complete, well-organized |
| `snapshot_overview.md` | Architecture | Excellent — glossary, use cases, design choices, constants, dependencies |
| `snapshot_manifest_types.md` | Data structures | Thorough — validation rules, serde behavior, path normalization |
| `snapshot_hash_cache.md` | Caching | Good — covers both hash cache and S3 check cache |
| `snapshot_data_cache.md` | Storage abstraction | Good — sync/async traits, S3/FS implementations |
| `snapshot_symlink_handling.md` | Symlinks | Excellent — all 6 policies, escaping detection, cycle detection |
| `snapshot_operation_collect.md` | COLLECT | Good — walkdir traversal, metadata extraction |
| `snapshot_operation_hash.md` | HASH | Good — rayon parallelism, cache integration |
| `snapshot_operation_hash_upload.md` | HASH_UPLOAD | Good — pipeline architecture, memory limits |
| `snapshot_operation_hash_upload_pipeline.md` | Pipeline details | Good — memory pool, dedup, task architecture |
| `snapshot_operation_hash_upload_s3.md` | S3 specifics | Good — multipart, streaming, probabilistic validation |
| `snapshot_operation_download.md` | DOWNLOAD | Good — conflict resolution, symlink ordering |
| `snapshot_operation_download_pipeline.md` | Pipeline details | Good — atomicity, chunked files, preallocation |
| `snapshot_operation_filter.md` | FILTER | Good — glob patterns, include/exclude |
| `snapshot_operation_diff.md` | DIFF | Good — hash state validation, preserve_runnable |
| `snapshot_operation_compose.md` | COMPOSE | Good — trie-based, reconciliation |
| `snapshot_operation_subtree.md` | SUBTREE | Good — symlink resolution, identity subtree |
| `snapshot_operation_partition.md` | PARTITION | Good — auto-root, explicit roots |
| `snapshot_operation_join.md` | JOIN | Good — prefix prepending |
| `snapshot_operation_cache_sync.md` | CACHE_SYNC | Good — server-side copy, multipart fallback |

### 2.2 Specification Strengths

- **Comprehensive coverage:** Every operation has its own spec document with clear inputs, outputs, algorithms, and edge cases.
- **Design rationale:** The overview document explains *why* design choices were made (phantom types, tokio vs rayon, etc.).
- **Cross-references:** Specs reference each other appropriately (e.g., symlink handling spec referenced from COLLECT, SUBTREE, DOWNLOAD).
- **Constants documented:** DEFAULT_FILE_CHUNK_SIZE, WHOLE_FILE_CHUNK_SIZE, DEFAULT_S3_MULTIPART_PART_SIZE all specified.

### 2.3 Specification Gaps

1. **~~No duplicate path handling specified.~~** *(Fixed)* `validate()` now rejects duplicate paths across files and dirs. Deserialization (`decode_v2023`, `decode_v2025`) also rejects manifests with duplicate paths. The spec has been updated.

2. **~~Memory pool u32 limitation not documented.~~** *(Fixed)* The pool now uses 4KB permit granularity, supporting up to ~16TB with u32 permits.

3. **~~`hash_file_chunked` read semantics not specified.~~** *(Fixed)* `hash_file_chunked` and `process_chunked_async` now use `read_exact()` to guarantee chunk boundaries match `chunk_size`. The spec should be updated to document this guarantee.

4. **~~Error type taxonomy not specified.~~** *(Fixed)* `snapshot_error_handling.md` documents the `SnapshotError` enum, all variants, the conversion strategy, message conventions, and usage by operation.

5. **~~`preallocate_file` failure behavior not specified.~~** *(Fixed)* `preallocate_file` now checks the `posix_fallocate` return value and falls back to `set_len()` on failure. The spec should be updated to document this fallback behavior.

6. **~~S3 streaming error propagation not specified.~~** *(Not applicable)* The current code fully buffers S3 responses before writing; there is no streaming channel pattern where errors could be silently discarded.

---

## 3. Implementation Review

### 3.1 Files Reviewed

| File | Lines | Role |
|------|-------|------|
| `src/lib.rs` | ~50 | Crate root, re-exports |
| `src/error.rs` | ~20 | Error types |
| `src/hash.rs` | ~100 | Hash functions |
| `src/path_util.rs` | ~80 | Path normalization |
| `src/manifest.rs` | ~400 | Manifest types, validation |
| `src/codec.rs` | ~500 | v2023/v2025 encoding/decoding |
| `src/data_cache.rs` | ~800 | Storage abstraction, S3/FS backends |
| `src/s3_check_cache.rs` | ~100 | S3 existence cache |
| `src/hash_cache.rs` | ~120 | File hash cache |
| `src/ops/mod.rs` | ~20 | Operation module re-exports |
| `src/ops/collect.rs` | ~300 | COLLECT operation |
| `src/ops/hash_op.rs` | ~200 | HASH operation |
| `src/ops/hash_upload.rs` | ~400 | HASH_UPLOAD operation |
| `src/ops/download.rs` | ~500 | DOWNLOAD operation |
| `src/ops/diff.rs` | ~150 | DIFF operation |
| `src/ops/compose.rs` | ~200 | COMPOSE operation |
| `src/ops/filter.rs` | ~80 | FILTER operation |
| `src/ops/subtree.rs` | ~250 | SUBTREE operation |
| `src/ops/partition.rs` | ~200 | PARTITION operation |
| `src/ops/join.rs` | ~80 | JOIN operation |
| `src/ops/cache_sync.rs` | ~250 | CACHE_SYNC operation |
| `src/ops/rate.rs` | ~60 | Rate calculation |
| `src/ops/memory_pool.rs` | ~100 | Memory pool |
| `src/bin/bench.rs` | ~400 | Benchmark binary |

### 3.2 Implementation Strengths

- **Clean type system:** Phantom type parameters (`Abs`/`Rel`, `Full`/`Diff`) provide compile-time guarantees for path style and manifest kind. This is idiomatic Rust and prevents many classes of bugs.
- **Appropriate concurrency model:** rayon for CPU-bound hashing, tokio for I/O-bound transfers. This is the right choice for each workload.
- **Content-addressed deduplication:** DashMap-based concurrent dedup in HASH_UPLOAD prevents redundant uploads.
- **Comprehensive symlink handling:** All 6 policies implemented with cycle detection, escaping detection, and topological sorting.
- **Cross-implementation compatibility:** v2023 canonical JSON encoding matches Python bitwise, verified by fixture-based tests.
- **Good error messages:** Validation errors include the offending path, making debugging straightforward.
- **Security:** ExpectedBucketOwner set on all S3 operations for confused deputy protection.

### 3.3 Issues Found

#### ~~CRITICAL — Memory Pool u32 Truncation Bug~~

*(Fixed)* The memory pool now uses a coarser permit granularity of 4096 bytes (1 permit = 4KB) instead of 1 permit = 1 byte. With u32 permits this supports single allocations up to ~16TB, well above the 16GB `MAX_MEMORY_BYTES` limit. The `bytes_to_permits()` helper rounds up using `div_ceil` so sub-granularity requests still acquire at least 1 permit. Two regression tests verify that 8GB values don't truncate and that rounding behaves correctly.

#### ~~HIGH — `hash_file_chunked` Uses Non-Filling Reads~~

*(Fixed)* Both `hash_file_chunked()` in `src/hash.rs` and `process_chunked_async()` in `src/ops/hash_upload.rs` now use `read_exact()` to fill the buffer completely for each chunk. On `UnexpectedEof` (the final partial chunk), the code seeks back to the known consumed position and uses `read_to_end()` to get the exact remainder. This guarantees chunk boundaries are always determined by `chunk_size`, not by OS buffering behavior.

#### ~~HIGH — `preallocate_file` Silently Ignores Errors on Linux~~

*(Fixed)* The `posix_fallocate` return value is now checked. On failure (e.g., unsupported filesystem like ZFS, NFS, tmpfs, or disk full), the code falls back to `set_len()` which uses `ftruncate` — universally supported. Disk-full conditions propagate properly since `set_len()` will also fail in that case.

#### ~~MEDIUM — Sequential `object_exists` Checks in HASH_UPLOAD Work-Item Building~~

*(Fixed)* The work-item building phase in `hash_upload_manifest` is now structured in three phases: (1) gather cache-hit candidates via local hash cache lookups, (2) fire all `object_exists` calls concurrently via `FuturesUnordered`, and (3) build work items for files that weren't fully skipped. This turns O(N) sequential S3 round-trips into concurrent requests.

#### ~~MEDIUM — S3 Streaming Silently Discards Send Errors~~

*(Not applicable)* The report described a `let _ = tx.send(chunk).await` pattern with mpsc channels, but the current code does not use streaming channels. S3 downloads use `resp.body.collect().await` to fully buffer the response before writing to disk via `spawn_blocking`. There is no reader/writer pipeline where send errors could be silently discarded.

#### MEDIUM — No Duplicate Path Validation in Manifests

**File:** `src/manifest.rs`, `validate()` method

The `validate()` method checks path format, deleted constraints, symlink constraints, and chunk hash counts, but does not check for duplicate paths in `files` or `dirs`. A manifest with two entries for the same path passes validation.

**Impact:** Could cause undefined behavior in operations that build HashMaps keyed by path (e.g., DIFF, COMPOSE) — the second entry would silently overwrite the first.

**Demonstrated by:** `test_quality_probes::validate_allows_duplicate_file_paths` and `validate_allows_duplicate_dir_paths`.

#### ~~MEDIUM — Phantom Type Bypass via Deserialization~~

*(Documented)* A doc comment has been added to the `Manifest` struct warning that `#[serde(skip)]` on `PhantomData` means direct `serde_json::from_str` bypasses phantom type constraints. The comment directs users to the `decode_v2023`/`decode_v2025` functions and notes that `validate()` must be called if deserializing directly. This is the appropriate fix because the codec functions already produce correctly-typed manifests, and a custom `Deserialize` impl would duplicate `validate()` logic.

#### ~~LOW — `SnapshotError::HashMismatch` Is Dead Code~~

*(Fixed)* The `HashMismatch` variant has been removed from `SnapshotError`.

#### LOW — `SnapshotError::Other(String)` Loses Error Context

**File:** `src/error.rs`

SQLite errors are converted via `.map_err(|e| SnapshotError::Other(e.to_string()))`, losing the original error type and source chain. S3 SDK errors are double-wrapped through `std::io::Error` then `SnapshotError::Io`.

**Impact:** Harder to diagnose errors in production; `source()` chain is broken.

**Fix:** Add dedicated variants (e.g., `Sqlite(rusqlite::Error)`, `S3(String)`) or use `#[source]` on `Other`.

#### ~~LOW — Symlink Topological Sort Cycle Fallback Is O(n²)~~

*(Fixed)* The cycle fallback now uses a `HashSet` for O(1) membership checks instead of `Vec::contains`.

#### ~~LOW — No Cache Eviction/Pruning~~

*(Partially fixed)* `S3CheckCache::new()` now prunes expired entries (older than 30 days) on open via a `DELETE WHERE` statement. This keeps the S3 check cache bounded across sessions. The hash cache has no natural TTL — staleness is determined at read time by mtime comparison — so it remains unpruned. A future schema change (`hashesV5`) adding a `last_accessed_time` column is documented in the spec as future work.

#### LOW — `expand_dir_symlink` Is O(n) Per Directory Symlink

**File:** `src/ops/subtree.rs`

`expand_dir_symlink` iterates over ALL files to find those under the target directory. With many directory symlinks, this becomes O(n × d) where d is the number of directory symlinks.

**Impact:** Slow subtree extraction with many directory symlinks.

**Fix:** Pre-build a prefix index or use sorted iteration with binary search.

#### ~~LOW — macOS Memory Detection Falls Back to 256MB~~

*(Fixed)* `detect_system_memory()` now uses `sysctlbyname` on macOS to read `hw.memsize` (total) and `vm.page_free_count * vm.pagesize` (available), instead of falling back to 256MB.

---

## 4. Test Suite Review

### 4.1 Test Files Reviewed

| File | Tests | Topic |
|------|-------|-------|
| `test_collect.rs` | ~80 | COLLECT operation |
| `test_hash.rs` | ~35 | HASH operation |
| `test_hash_upload.rs` | ~55 | HASH_UPLOAD operation |
| `test_download.rs` | ~45 | DOWNLOAD operation |
| `test_codec.rs` | ~40 | v2023/v2025 encoding |
| `test_v2023_canonical.rs` | ~29 | Cross-implementation canonical JSON |
| `test_diff.rs` | ~45 | DIFF operation |
| `test_filter.rs` | ~30 | FILTER operation |
| `test_join.rs` | ~25 | JOIN operation |
| `test_subtree.rs` | ~55 | SUBTREE operation |
| `test_compose.rs` | ~30 | COMPOSE operation |
| `test_partition.rs` | ~35 | PARTITION operation |
| `test_round_trip.rs` | 6 | End-to-end pipeline |
| `test_cache_sync.rs` | 8 | CACHE_SYNC operation |
| `test_s3_integration.rs` | 2 | Real S3 integration |
| `test_s3_data_cache.rs` | ~45 | S3 emulation tests |
| `test_chunk_size.rs` | ~35 | file_chunk_size_bytes preservation |
| `test_manifest.rs` | 10 | clear_hashes |
| `test_quality_probes.rs` | 5 | Evaluation probe tests |
| Inline `#[cfg(test)]` | ~170 | Unit tests across 19 source files |

**Total: ~945 tests**

### 4.2 Test Strengths

1. **Exhaustive symlink policy coverage:** All 6 policies tested with files, directories, chains, cycles (self-referential, 2-node, 3-node), broken symlinks, escaping/non-escaping.
2. **Cross-implementation compatibility:** v2023 canonical JSON fixtures from the Python implementation, verified bitwise.
3. **Cache integration testing:** Hash cache, S3 check cache, and data cache interactions thoroughly tested.
4. **Deduplication testing:** Content-addressed dedup and concurrent dedup with multiple workers.
5. **file_chunk_size_bytes preservation:** Dedicated test file verifying preservation through every operation.
6. **Progress callback testing:** Invocation, cancellation, monotonic timing, rate calculation.
7. **Invariant tests:** Subtree single-step vs two-step equivalence verified.
8. **S3 emulation:** Uses s3s for in-process S3 testing without real AWS credentials.

### 4.3 Test Gaps

1. **No error message assertions.** Unlike openjd-expr and openjd-model (per AGENTS.md test quality standard), error messages are only checked for presence, not full content. This means error message regressions would go undetected.

2. **No Windows tests.** All symlink tests are `#[cfg(unix)]`. No Windows path handling tests beyond `normalize_path` unit tests. The `preallocate_file` Windows code path is untested.

3. **No concurrent/race condition tests.** No tests for concurrent collect, concurrent download to same file, or TOCTOU races in `create_copy_path`.

4. **No disk-full/IO-error simulation.** No tests for what happens when disk is full during download, cache write, or preallocation.

5. **~~No corrupted data tests.~~** *(Fixed)* Five tests added to `test_download.rs`: corrupted cache object, truncated cache object, missing object in data cache, stale hash cache entry triggering re-download, and corrupted chunked cache object. Additionally, inline hash verification was added to the download path — small whole files and small chunks are now verified via `hash_data()` on the in-memory bytes before writing to disk. Large files/chunks using multipart byte-range downloads are not verified (no per-range hash to check against).

6. **No v2025 cross-implementation fixtures.** v2023 has extensive Python fixture-based canonical tests, but v2025 only has round-trip tests.

7. **~~No test for memory pool with large values.~~** *(Fixed)* `large_values_no_truncation` test verifies 8GB values don't truncate. `sub_granularity_rounds_up` tests rounding behavior.

8. **No test for `hash_file_chunked` with large files.** No test verifying chunk boundary correctness with files large enough to trigger multiple `read_exact` calls. The non-filling read bug has been fixed, but a regression test for large files would be valuable.

9. **~~No round-trip test with symlinks.~~** *(Fixed)* `test_round_trip.rs` now includes `round_trip_with_symlinks` covering collect→upload→download with `CollapseAll` policy. Uses a runtime `symlinks_supported()` check so it runs on Unix and on Windows with Developer Mode enabled.

10. **~~No test for `HashMismatch` error variant usage.~~** The variant has been removed, so this gap no longer applies.

---

## 5. Quality Probe Tests

Five probe tests were written and added to `tests/test_quality_probes.rs`. All pass, confirming the identified issues:

| Test | What It Demonstrates |
|------|---------------------|
| `validate_allows_duplicate_file_paths` | `validate()` accepts manifests with duplicate file paths |
| `validate_allows_duplicate_dir_paths` | `validate()` accepts manifests with duplicate dir paths |
| `hash_file_chunked_consistent_chunk_count` | Chunk hashing is deterministic (now uses `read_exact` for guaranteed chunk boundaries) |
| `hash_mismatch_error_variant_exists_but_unused` | *(Removed)* `HashMismatch` variant no longer exists |
| `manifest_deserialization_ignores_phantom_types` | Deserialization bypasses phantom type constraints; `validate()` catches it |

---

## 6. Recommendations

### Priority 1 — Correctness Fixes

1. **~~Fix memory pool u32 truncation.~~** *(Fixed)* Uses 4KB permit granularity (1 permit = 4096 bytes) to support up to ~16TB with u32 permits.

2. **~~Use filling reads in `hash_file_chunked`.~~** *(Fixed)* Both `hash_file_chunked` and `process_chunked_async` now use `read_exact()` with proper final-chunk handling via seek-back and `read_to_end()`.

3. **Add duplicate path validation to `validate()`.** Build a `HashSet` of paths and reject duplicates. This prevents silent data loss in DIFF and COMPOSE operations.

### Priority 2 — Robustness Improvements

4. **~~Check `posix_fallocate` return value.~~** *(Fixed)* Return value is now checked; falls back to `set_len()` on failure.

5. **~~Propagate S3 streaming errors.~~** *(Not applicable)* The current code fully buffers S3 responses before writing; there is no streaming channel pattern.

6. **~~Parallelize `object_exists` checks in HASH_UPLOAD.~~** *(Fixed)* Work-item building now uses a three-phase approach with `FuturesUnordered` for concurrent existence checks.

7. **Add validating deserialization.** Consider a `Manifest::deserialize_and_validate()` method that combines deserialization with validation, or implement a custom `Deserialize` that checks path constraints.

### Priority 3 — Code Quality

8. **~~Remove or use `HashMismatch` variant.~~** *(Fixed)* The variant has been removed.

9. **Improve error type hierarchy.** Add dedicated variants for SQLite and S3 errors to preserve error context and source chains.

10. **~~Add cache eviction.~~** *(Partially fixed)* S3 check cache now prunes expired entries on open. Hash cache eviction is documented as future work requiring a schema change.

11. **~~Fix macOS memory detection.~~** *(Fixed)* Uses `sysctlbyname` on macOS to detect total and available memory.

12. **~~Use `HashSet` in symlink topological sort fallback.~~** *(Fixed)* Cycle fallback now uses `HashSet` for O(1) lookups.

### Priority 4 — Specification Updates

13. **~~Add error handling spec.~~** *(Fixed)* `snapshot_error_handling.md` documents the enum, conversion strategy, message format, and per-operation usage.

14. **Document duplicate path policy.** Specify whether manifests may contain duplicate paths and what the expected behavior is.

15. **~~Document memory pool u32 limitation.~~** *(Fixed)* The bug is resolved; the pool now uses 4KB granularity.

16. **~~Document `hash_file_chunked` read semantics.~~** *(Fixed in code)* The implementation now uses `read_exact()` for exact chunk boundaries. The spec should be updated to document this guarantee.

### Priority 5 — Test Improvements

17. **Add error message assertions.** Follow the AGENTS.md test quality standard by asserting full error message content, not just error presence.

18. **Add memory pool large-value tests.** Test with values exceeding u32::MAX to catch the truncation bug.

19. **Add v2025 cross-implementation fixtures.** Create Python-generated v2025 fixtures for bitwise compatibility testing.

20. **Add round-trip test with symlinks.** Extend `test_round_trip.rs` to cover symlink collect→upload→download.

---

## 7. Summary Scorecard

| Dimension | Score | Notes |
|-----------|-------|-------|
| Specification completeness | 8/10 | Thorough but missing error handling spec and some edge case documentation |
| Specification accuracy | 9/10 | Accurately represents implementation; minor gaps noted |
| Implementation correctness | 9/10 | All critical and high issues fixed; remaining items are robustness improvements |
| Implementation ergonomics | 9/10 | Clean API, good type safety, builder pattern |
| Implementation performance | 9/10 | Good parallelism; sequential object_exists fixed; O(n²) symlink edge case remains |
| Error quality | 7/10 | Good validation messages; error type hierarchy loses context |
| Naming consistency | 9/10 | Consistent with other openjd-rs crates |
| Rust best practices | 8/10 | Good overall; unsafe code is minimal and well-scoped |
| Test coverage | 8/10 | Excellent happy path and edge case coverage; gaps in error simulation and Windows |
| Test organization | 9/10 | Well-structured, clear naming, good use of helpers |

**Overall: Strong implementation with a few targeted fixes needed.** All critical and high-severity issues have been fixed: memory pool u32 truncation (Priority 1, item 1), non-filling reads in `hash_file_chunked` (Priority 1, item 2), silent `posix_fallocate` failures (Priority 2, item 4), and sequential `object_exists` checks in HASH_UPLOAD (Priority 2, item 6). Remaining items are robustness improvements and code quality enhancements.
