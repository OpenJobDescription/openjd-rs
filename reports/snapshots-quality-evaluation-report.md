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

2. **Memory pool u32 limitation not documented.** The specs describe the memory pool using tokio::sync::Semaphore but don't mention the u32 permit limit, which caps single allocations at ~4GB despite the pool supporting up to 16GB.

3. **`hash_file_chunked` read semantics not specified.** The hash spec doesn't specify whether chunk boundaries must be exact (i.e., whether partial reads from `file.read()` are acceptable). This matters for cross-platform hash consistency.

4. **Error type taxonomy not specified.** There's no spec document for the error handling approach. The `SnapshotError` enum and its variants are not documented in the specs.

5. **`preallocate_file` failure behavior not specified.** The download pipeline spec mentions preallocation but doesn't specify what happens when it fails (currently silently ignored on Linux).

6. **S3 streaming error propagation not specified.** The download pipeline spec doesn't specify behavior when the file writer fails mid-stream (currently the S3 reader continues downloading and discarding data).

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

#### CRITICAL — Memory Pool u32 Truncation Bug

**File:** `src/ops/memory_pool.rs`, `acquire()` method

```rust
pub async fn acquire(&self, size: usize) -> OwnedSemaphorePermit {
    let clamped = size.min(self.max_bytes) as u32;
    // ...
}
```

The `as u32` cast silently truncates values above `u32::MAX` (~4GB). The pool's `max_bytes` can be up to 16GB (`MAX_MEMORY_BYTES`), and `Semaphore::new()` accepts `usize`, so the semaphore is initialized correctly. But `acquire_many_owned()` takes `u32`, so any single allocation request above ~4GB silently wraps. A 5GB request would acquire only ~0.7GB of permits, allowing the pool to over-commit memory.

**Impact:** Could cause OOM on systems with large memory pools processing files >4GB.

**Fix:** Use a coarser permit granularity (e.g., 1 permit = 4096 bytes) to support up to 16TB with u32 permits.

#### HIGH — `hash_file_chunked` Uses Non-Filling Reads

**File:** `src/hash.rs`, `hash_file_chunked()` function

```rust
let n = file.read(&mut buf)?;
```

`file.read()` may return fewer bytes than the buffer size even when more data is available. This means chunk boundaries could shift depending on OS buffering behavior, producing different chunk hashes for the same file content. On Linux with local files this is practically deterministic, but the Rust documentation explicitly states that `read()` is not guaranteed to fill the buffer.

The same pattern appears in `src/ops/hash_upload.rs` in `process_chunked_async`.

**Impact:** Theoretically non-deterministic chunk hashes across platforms or under I/O pressure. In practice, local file reads on Linux fill the buffer, so this hasn't manifested as a bug yet.

**Fix:** Replace `file.read(&mut buf)` with a fill loop or `Read::read_exact()` (handling the final partial chunk).

#### HIGH — `preallocate_file` Silently Ignores Errors on Linux

**File:** `src/ops/download.rs`, `preallocate_file()` function

```rust
let _ = unsafe { libc::posix_fallocate(f.as_raw_fd(), 0, size as libc::off_t) };
```

The return value of `posix_fallocate` is discarded. If it fails (disk full, unsupported filesystem), the file remains zero-length. Subsequent offset writes will extend the file on demand, but without preallocation the writes may fail partway through or produce fragmented files.

**Impact:** Silent performance degradation or confusing errors on disk-full conditions.

**Fix:** Check the return value; fall back to `set_len()` on failure.

#### MEDIUM — Sequential `object_exists` Checks in HASH_UPLOAD Work-Item Building

**File:** `src/ops/hash_upload.rs`

The work-item building loop calls `data_cache.object_exists()` sequentially for each file's cached hash. For chunked files, this means N sequential HeadObject calls to S3. With a manifest of 1000 chunked files averaging 4 chunks each, this is 4000 sequential S3 round-trips before any upload work begins.

**Impact:** Slow startup for large manifests with many cache hits, especially with S3 backend.

**Fix:** Batch the existence checks or parallelize them using `FuturesUnordered`.

#### MEDIUM — S3 Streaming Silently Discards Send Errors

**File:** `src/data_cache.rs`, three locations in S3DataCache

```rust
let _ = tx.send(chunk).await;
```

If the file writer task fails (e.g., disk full), the mpsc sender returns `Err`, but the S3 reader continues downloading and discarding all remaining data. The error is eventually surfaced when `writer.await` completes, but network bandwidth is wasted.

**Impact:** Wasted bandwidth on write failures; delayed error reporting.

**Fix:** Check the send result and break the read loop on error.

#### MEDIUM — No Duplicate Path Validation in Manifests

**File:** `src/manifest.rs`, `validate()` method

The `validate()` method checks path format, deleted constraints, symlink constraints, and chunk hash counts, but does not check for duplicate paths in `files` or `dirs`. A manifest with two entries for the same path passes validation.

**Impact:** Could cause undefined behavior in operations that build HashMaps keyed by path (e.g., DIFF, COMPOSE) — the second entry would silently overwrite the first.

**Demonstrated by:** `test_quality_probes::validate_allows_duplicate_file_paths` and `validate_allows_duplicate_dir_paths`.

#### MEDIUM — Phantom Type Bypass via Deserialization

**File:** `src/manifest.rs`

Since `PhantomData` is `#[serde(skip)]`, deserializing JSON into any `Manifest<P, K>` variant succeeds regardless of path content. An absolute-path manifest can be deserialized as `Snapshot` (relative). The `validate()` method catches this, but callers must remember to call it.

**Impact:** Type safety guarantee is only enforced at runtime via `validate()`, not at deserialization time.

**Demonstrated by:** `test_quality_probes::manifest_deserialization_ignores_phantom_types`.

#### LOW — `SnapshotError::HashMismatch` Is Dead Code

**File:** `src/error.rs`

The `HashMismatch { expected, actual }` variant is defined but never constructed anywhere in the crate. Grep confirms zero usages outside the definition.

**Impact:** Dead code; misleading API surface.

**Fix:** Either use it where hash verification occurs (e.g., in the two-pass streaming upload) or remove it.

#### LOW — `SnapshotError::Other(String)` Loses Error Context

**File:** `src/error.rs`

SQLite errors are converted via `.map_err(|e| SnapshotError::Other(e.to_string()))`, losing the original error type and source chain. S3 SDK errors are double-wrapped through `std::io::Error` then `SnapshotError::Io`.

**Impact:** Harder to diagnose errors in production; `source()` chain is broken.

**Fix:** Add dedicated variants (e.g., `Sqlite(rusqlite::Error)`, `S3(String)`) or use `#[source]` on `Other`.

#### LOW — Symlink Topological Sort Cycle Fallback Is O(n²)

**File:** `src/ops/download.rs`

When symlink cycles exist, the fallback code uses `sorted.contains(&i)` which is O(n), making the overall cycle handling O(n²). Cyclic symlinks are also silently created without warning.

**Impact:** Performance degradation with many cyclic symlinks (unlikely in practice).

**Fix:** Use a `HashSet` for the sorted set; log a warning for cyclic symlinks.

#### LOW — No Cache Eviction/Pruning

**Files:** `src/hash_cache.rs`, `src/s3_check_cache.rs`

Neither the hash cache nor the S3 check cache has any eviction or pruning mechanism. The SQLite tables grow indefinitely. The S3 check cache has a 30-day TTL on reads but never deletes expired rows.

**Impact:** Gradual disk usage growth over time.

**Fix:** Add periodic pruning of expired entries, or VACUUM on open.

#### LOW — `expand_dir_symlink` Is O(n) Per Directory Symlink

**File:** `src/ops/subtree.rs`

`expand_dir_symlink` iterates over ALL files to find those under the target directory. With many directory symlinks, this becomes O(n × d) where d is the number of directory symlinks.

**Impact:** Slow subtree extraction with many directory symlinks.

**Fix:** Pre-build a prefix index or use sorted iteration with binary search.

#### LOW — macOS Memory Detection Falls Back to 256MB

**File:** `src/ops/memory_pool.rs`

`detect_system_memory()` only reads `/proc/meminfo` (Linux). On macOS, it falls back to 256MB, which is far below typical available memory.

**Impact:** Suboptimal performance on macOS.

**Fix:** Use `sysctl` on macOS to detect memory.

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

5. **No corrupted data tests.** No tests for corrupted cache entries, truncated files, or hash mismatches during download verification.

6. **No v2025 cross-implementation fixtures.** v2023 has extensive Python fixture-based canonical tests, but v2025 only has round-trip tests.

7. **No test for memory pool with large values.** All memory pool tests use small values (100-1024) that fit in u32, missing the truncation bug.

8. **No test for `hash_file_chunked` with large files.** No test verifying chunk boundary correctness with files large enough to trigger partial reads.

9. **No round-trip test with symlinks.** `test_round_trip.rs` tests the full pipeline but doesn't include symlinks.

10. **No test for `HashMismatch` error variant usage.** The variant exists but is never tested in a production code path (only in the quality probe test).

---

## 5. Quality Probe Tests

Five probe tests were written and added to `tests/test_quality_probes.rs`. All pass, confirming the identified issues:

| Test | What It Demonstrates |
|------|---------------------|
| `validate_allows_duplicate_file_paths` | `validate()` accepts manifests with duplicate file paths |
| `validate_allows_duplicate_dir_paths` | `validate()` accepts manifests with duplicate dir paths |
| `hash_file_chunked_consistent_chunk_count` | Chunk hashing is deterministic on Linux (but uses non-filling reads) |
| `hash_mismatch_error_variant_exists_but_unused` | `HashMismatch` variant can be constructed but is never used in production |
| `manifest_deserialization_ignores_phantom_types` | Deserialization bypasses phantom type constraints; `validate()` catches it |

---

## 6. Recommendations

### Priority 1 — Correctness Fixes

1. **Fix memory pool u32 truncation.** Switch to coarser permit granularity (e.g., 1 permit = 4096 bytes). This is a real bug that could cause OOM with files >4GB on systems with >4GB memory pools.

2. **Use filling reads in `hash_file_chunked`.** Replace `file.read(&mut buf)` with a loop that fills the buffer completely (except for the final chunk). This ensures cross-platform chunk hash consistency.

3. **Add duplicate path validation to `validate()`.** Build a `HashSet` of paths and reject duplicates. This prevents silent data loss in DIFF and COMPOSE operations.

### Priority 2 — Robustness Improvements

4. **Check `posix_fallocate` return value.** Fall back to `set_len()` on failure instead of silently ignoring the error.

5. **Propagate S3 streaming errors.** Check `tx.send()` results and break the read loop on error to avoid wasting bandwidth.

6. **Parallelize `object_exists` checks in HASH_UPLOAD.** Use `FuturesUnordered` or `join_all` for the work-item building phase to avoid sequential S3 round-trips.

7. **Add validating deserialization.** Consider a `Manifest::deserialize_and_validate()` method that combines deserialization with validation, or implement a custom `Deserialize` that checks path constraints.

### Priority 3 — Code Quality

8. **Remove or use `HashMismatch` variant.** Either use it in the two-pass streaming upload hash verification, or remove it to reduce dead code.

9. **Improve error type hierarchy.** Add dedicated variants for SQLite and S3 errors to preserve error context and source chains.

10. **Add cache eviction.** Implement periodic pruning of expired S3 check cache entries and optionally stale hash cache entries.

11. **Fix macOS memory detection.** Use `sysctl` to detect available memory on macOS instead of falling back to 256MB.

12. **Use `HashSet` in symlink topological sort fallback.** Replace `sorted.contains()` with a `HashSet` lookup to avoid O(n²) behavior.

### Priority 4 — Specification Updates

13. **Add error handling spec.** Document the `SnapshotError` enum, error conversion strategy, and error message format.

14. **Document duplicate path policy.** Specify whether manifests may contain duplicate paths and what the expected behavior is.

15. **Document memory pool u32 limitation.** Either fix the bug and document the new granularity, or document the 4GB single-allocation limit.

16. **Document `hash_file_chunked` read semantics.** Specify that chunk boundaries must be exact and reads must fill the buffer.

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
| Implementation correctness | 7/10 | Memory pool u32 bug is significant; partial reads are a latent risk |
| Implementation ergonomics | 9/10 | Clean API, good type safety, builder pattern |
| Implementation performance | 8/10 | Good parallelism; sequential object_exists and O(n²) edge cases |
| Error quality | 7/10 | Good validation messages; error type hierarchy loses context |
| Naming consistency | 9/10 | Consistent with other openjd-rs crates |
| Rust best practices | 8/10 | Good overall; unsafe code is minimal and well-scoped |
| Test coverage | 8/10 | Excellent happy path and edge case coverage; gaps in error simulation and Windows |
| Test organization | 9/10 | Well-structured, clear naming, good use of helpers |

**Overall: Strong implementation with a few targeted fixes needed.** The most urgent items are the memory pool u32 truncation (Priority 1, item 1) and the non-filling reads in `hash_file_chunked` (Priority 1, item 2).
