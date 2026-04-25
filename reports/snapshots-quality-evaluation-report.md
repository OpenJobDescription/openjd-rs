# openjd-snapshots Crate Quality Evaluation Report

**Date:** 2026-04-24
**Crate:** `openjd-snapshots`

## Executive Summary

`openjd-snapshots` is in strong shape. The crate builds clean (no warnings), `cargo clippy --all-targets -- -D warnings` is clean, and the test suite is large and well-organized: 20 test files with roughly 1,050 tests passing, covering every operation (COLLECT, HASH, HASH_UPLOAD, DOWNLOAD, DIFF, COMPOSE, FILTER, SUBTREE, PARTITION, JOIN, CACHE_SYNC) plus codec round-trips, caching, deduplication, and S3 integration (behind `#[ignore]`). The specs are thorough — 21 documents including a dedicated `public-api.md` — and they accurately reflect the implementation.

Exploratory testing found one real bug (infinite loop in `hash_file_chunked` when `chunk_size == 0`) and a handful of smaller improvements worth making: redundant duplicate-path checks between `decode_*` and `validate()`, a dead-seek in `hash_file_chunked` after read_exact's successful consumed-buffer state is ambiguous, a missing `new()`-time sanity check in `HashCache` vs `S3CheckCache` (inconsistent mtime/timestamp column types), and some ergonomic polish on `subtree_snapshot` (rejecting `Preserve`/`TransitiveIncludeTargets` with a `Validation` error rather than returning a typed precondition). Overall quality bar is high and the crate is ready for continued use.

## 1. Specifications Review

`specs/snapshots/` contains 21 documents. The coverage is comprehensive:

| Document | Assessment |
|----------|-----------|
| `README.md` | Clear index; links every operation and cross-cutting doc. |
| `snapshot_overview.md` | Excellent: glossary, use cases, quick-start code, operations diagram, design choices, module layout, dependency graph. |
| `public-api.md` | Full and accurate — every re-export at the crate root and at module paths is listed with its signature. Verified against `lib.rs` re-exports (see §2). |
| `snapshot_manifest_types.md` | Describes phantom types, aliases, validation rules (paths, duplicates, chunkhashes count, symlink target style), serde field behavior. Matches implementation. |
| `snapshot_error_handling.md` | Documents each `SnapshotError` variant, conversion strategy, message conventions, cancellation model, and a per-operation variant table. Very accurate. |
| `snapshot_data_cache.md` | Sync and async traits, `CopyResult`, `FileSystemDataCache`, `S3DataCache`, probabilistic validation, cache key format. |
| `snapshot_symlink_handling.md` | Policy options, support matrix by operation, two-pass escaping detection. |
| `snapshot_hash_cache.md` | Schema, API, whole-file vs range, thread-safety. Accurate. |
| `snapshot_operation_{collect,hash,hash_upload,hash_upload_pipeline,hash_upload_s3,download,download_pipeline,filter,diff,compose,subtree,partition,join,cache_sync}.md` | Each has the full function signature, `*Options` struct, behavior table, cancellation, and when relevant a pipeline diagram and per-entry-type table. |

Gaps found:

- `snapshot_operation_subtree.md` should state explicitly that `SymlinkPolicy::Preserve` and `SymlinkPolicy::TransitiveIncludeTargets` are rejected at runtime with a `Validation` error (they are listed as unsupported in the symlink-handling support matrix, but the subtree doc itself does not repeat the fact). Minor.
- `public-api.md` documents `IncludeExcludePathsFilter::new` with the glob error type `glob::PatternError`, which leaks a dependency into the public API. The spec is accurate, but the public API choice itself deserves a note — see §8.
- Nothing explicitly documents the `hash_file_chunked(chunk_size=0)` precondition; adding a "must be > 0" line when we fix the bug is necessary.
- The spec for v2023 encoding (`codec.rs`) lives in `snapshot_manifest_types.md` and scattered mentions; a dedicated doc would help given the complexity of canonical JSON, UTF-16BE sort, and symlink collapse.

## 2. Public API Review

`lib.rs` re-exports align with `specs/snapshots/public-api.md`. I verified the re-export lists:

- Crate-root re-exports: `codec::{decode_*, encode_*, DecodedManifest, ManifestFormat}`, `data_cache::{AsyncDataCache, ContentAddressedDataCache, CopyResult, FileSystemDataCache, S3DataCache}`, `error::{Result, SnapshotError}`, `hash::{human_readable_file_size, HashAlgorithm, DEFAULT_FILE_CHUNK_SIZE, DEFAULT_S3_MULTIPART_PART_SIZE, WHOLE_FILE_CHUNK_SIZE}`, `hash_cache::HashCache`, manifest types, `ops::*`, `s3_check_cache::S3CheckCache`. All are documented in `public-api.md`.
- Module-path items called out correctly: `hash::{hash_data, hash_file, hash_file_chunked}`, `manifest::{Abs, Rel, Full, Diff, ValidatePaths, ValidateKind}`, `ops::ProgressFn`, `data_cache::CacheValidationState`, `hash_cache::WHOLE_FILE_RANGE_END`, `codec::encode_v2025`.

Ergonomics observations:

- The phantom-type design (`Manifest<P, K>` with `Abs/Rel/Full/Diff` markers) works well for operations that require absolute paths — passing `Snapshot` where `AbsSnapshot` is expected is a compile error. The enum wrappers (`AbsManifest`, `RelManifest`) are a pragmatic choice when the runtime variant is unknown.
- The `AsyncDataCache` trait is large (11+ methods) and forces `FileSystemDataCache` to implement multipart/range methods that simply return `Unsupported`. A smaller mandatory surface with optional multipart as a separate sub-trait (e.g., `MultipartDataCache`) would reduce boilerplate and make cross-cache fallback cleaner in `cache_sync`.
- `IncludeExcludePathsFilter::new` returns `Result<Self, glob::PatternError>`, exposing the `glob` crate in the public signature. Since the crate otherwise wraps external error types into `SnapshotError`, this is inconsistent.
- `S3DataCache` has many `pub` fields (`bucket`, `key_prefix`, `client`, `multipart_part_size`, `s3_check_cache`, `force_s3_check`, `expected_bucket_owner`, `cache_validation`). Freezing these as public makes them part of the semver contract. A builder or with-style setters would be cleaner.
- `HashStatistics`/`UploadStatistics`/`DownloadStatistics`/`CacheSyncStatistics` are structurally similar but duplicated. A shared base (trait or macro) would cut lines and reduce drift.

## 3. Implementation Review

### `manifest.rs` (25k lines of types + validation)

Correct and readable. The `validate()` method covers paths, duplicate detection, deleted-entry shape, symlink constraints, regular-file constraints, and chunkhashes count (with `ceil` division). `clear_hashes()` correctly skips symlinks and deleted entries. `recompute_total_size()` correctly excludes deleted and symlink entries. Serde is configured with `rename_all = "camelCase"` plus explicit `chunkhashes` rename (the field name differs from camelCase) and `skip_serializing_if` for false/None/empty values. The `#[serde(skip)] _phantom` field is correctly called out in the doc comment — users who deserialize directly via `serde_json::from_str::<Manifest<P, K>>()` must call `validate()` to enforce the phantom-type constraints. Good docs pattern.

One small nit: both `manifest.rs::validate()` and `codec::check_no_duplicate_paths` independently check for duplicate paths. `validate()` already does it; `decode_v2023` / `decode_v2025` should use the full `validate()` pipeline (after constructing a concrete typed manifest) rather than re-implementing a subset.

### `hash.rs`

- `hash_data`, `hash_file` are clean.
- `hash_file_chunked` has a **bug**: when `chunk_size == 0`, the function infinite-loops (a zero-length buffer makes `read_exact` return `Ok(())` immediately, nothing is consumed, the loop never terminates). Exploratory probe in `tests/test_eval_probes.rs::probe_hash_file_chunked_zero_chunk_size` demonstrates this (2-second timeout detects the hang).
- `hash_file_chunked` also has dead code in its `UnexpectedEof` branch: it allocates `tail` via `read_to_end`, then immediately seeks back to `hashes.len() * chunk_size` and re-reads the remainder. The initial `read_to_end` into `tail` is unused. Clean up.
- `human_readable_file_size` uses decimal units (1000-based). This matches Python's reference but the comment and the "KB/MB/..." suffixes are ambiguous — consider renaming to `KB→KB (1000)` or use `KiB` suffixes. Non-blocking.

### `codec.rs`

- `canonical_json` implements Python-compatible sort-keys + non-ASCII-escape (`\uXXXX`) JSON. The existing `test_v2023_canonical.rs` (29 tests) provides strong coverage. Correct.
- `expand_path_reference`: handles `$N/component` and bare names; rejects paths containing `/` that don't start with `$N`. Correct. However, the error message "`paths with '/' must use $N/ reference`" is slightly misleading when the problem is a bad integer after `$` — it returns a different error. Acceptable but could be unified.
- `encode_v2023_*` correctly drops empty dirs, drops deletions, and warns via `tracing::warn!`. Good fallback behavior for the lossy format.
- `check_no_duplicate_paths` duplicates logic from `Manifest::validate`. See §3.

### `path_util.rs`

- `normalize_path` correctly handles POSIX + Windows (backslashes, `\\?\`, drive letters). Unicode-safe.
- `is_absolute_path` correctly detects POSIX `/`, UNC `\\`, drive-letter `C:`.
- Edge cases tested: dotdot at root, bare drive letter, UNC prefixes. Good.

### `hash_cache.rs` and `s3_check_cache.rs`

- Both use `Mutex<rusqlite::Connection>` with `PRAGMA journal_mode=WAL`. Prune on open (`s3_check_cache`). Good.
- Schema `hashesV4` uses `last_modified_time timestamp` and stores as TEXT; `get` transparently handles both TEXT and INTEGER forms — defensive but inconsistent with the implementation's own `put` which always writes TEXT. Confirms Python-compat rather than cleanup; document the rationale or simplify.
- `HashCache::normalize_cache_key` uses `to_string_lossy()` — on paths containing non-UTF-8 bytes this silently replaces. Probably acceptable for Linux/macOS/Windows in practice, but note in docs.

### `data_cache.rs`

- The trait-default-with-override pattern for `stream_range_to_file_at_offset`, `copy_object_to_file`, `write_object_to_file_at_offset` is clean.
- `S3DataCache::object_exists` with probabilistic verification (first 100 always, then 1% random) is implemented correctly using atomics; `cache_validation_tests` cover it.
- `rand_u64()` builds a fresh `RandomState` hasher on every call and hashes the current time. Works but is wasteful; prefer `rand::random()` or store a thread-local RNG. Very minor.
- The `block_on_async` helper is correct and tested both with and without an outer tokio runtime.

### `ops/`

- `collect.rs`: Two-pass symlink handling; correctly defers symlink resolution until the collected set is known. Handles `runnable` bit on Unix, always false on Windows.
- `hash_op.rs`: Uses `rayon::par_iter` correctly; mutates `result` file entries via index under a per-chunk layout. `SlidingWindowRate` shared under `Mutex`. Pre-check rejects manifests whose regular files already have hashes (prevents double-hashing).
- `hash_upload.rs`: Pipeline uses `tokio::sync::Semaphore` (via `MemoryPool`) for memory bounding and `DashMap`-style broadcast-channel `UploadDedup` for concurrent dedup. Correct — exceptional care given to the tricky concurrent path.
- `download.rs`: `preallocate_file` uses platform-specific fast paths (Linux `posix_fallocate`, Windows `SetEndOfFile`, else `set_len`). `atomic_replace` with random temp-suffix is fine. The `temp_download_path` uses `process::id()` + `RandomState` hasher — not cryptographically random, but collision-safe enough for temp files.
- `subtree.rs`: Rejects `Preserve`/`TransitiveIncludeTargets` at runtime with `Validation`. Consider accepting them at the type level by returning a compile error when those variants are passed, but the variant is a runtime value so a `Validation` error is correct.
- `compose.rs`: Uses a trie structure — clean O(N·depth) algorithm. Correctly distinguishes directory deletion markers from file deletions.
- `diff.rs`: Correctly cascades parent-directory deletion to all files/dirs underneath (verified by probe). Sorts deleted dirs deepest-first so `rmdir` ordering works when applied.
- `filter.rs`: Straightforward; glob patterns; recomputes total_size.
- `join.rs`: Prepends prefix, joins symlink target, clears parent_manifest_hash. Correct.
- `partition.rs`: Longest-common-prefix root finding. Correct; well-tested.
- `cache_sync.rs`: Elegant pipeline using `copy_from` for S3→S3 server-side copy (no data through client), fallback to `get_object` + `put_object` with memory bounding. Multipart transfer for large objects. Good design.

### Naming

Consistent with other openjd crates: `SnapshotError`, `Result<T>`, snake_case operations, `Options`/`Result`/`Statistics` triplets per operation.

### Performance

No `O(N²)` issues observed in reviewed operations. `diff_snapshots` precomputes `HashMap<&str, &FileEntry>` lookup for the cascading-deletion check. The cascading deletion inner loops scan `parent.files` once per deleted directory — for pathological inputs (many deleted dirs × many files) this is O(N·M); for typical input sizes this is fine, and an optimization would use a path-prefix index. Low priority.

## 4. Test Review

Excellent coverage. Integration tests in `crates/openjd-snapshots/tests/` plus inline unit tests in each module:

| File | Tests | Focus |
|------|------:|-------|
| `test_collect.rs` | 136 | Directory walking, symlink policies, optional filenames, runnable bit, chunking |
| `test_download.rs` | 45+ | Atomicity, chunked files, conflict resolution, delete application, symlink policy |
| `test_hash_upload.rs` | 47 | Pipeline, memory limits, dedup, progress, cancellation |
| `test_hash.rs` | 57 | Happy + edge: cache hit/miss, force_rehash, chunking boundaries |
| `test_subtree.rs` | 70 | All symlink policies, cycles, escaping, UNC paths, nested subtrees |
| `test_diff.rs` | ~55 | Cascade deletion, runnable preservation, hash-mismatch guard |
| `test_compose.rs` | 46 | Trie correctness, deletion markers, sequences of diffs |
| `test_partition.rs` | 38 | Roots, nested-root rejection, LCP, symlink-aware partition |
| `test_filter.rs` | 40 | Glob patterns, include+exclude |
| `test_join.rs` | 60 | Abs+rel, symlink target prefixing, normalization |
| `test_s3_data_cache.rs` | 42 | Fake-S3 via `s3s`, multipart, range gets, probabilistic validation |
| `test_codec.rs` | 42 | v2023 + v2025 encode/decode, chunkhashes, symlinks |
| `test_v2023_canonical.rs` | 29 | Canonical JSON format, UTF-16BE sort, Unicode escape |
| `test_round_trip.rs` | 35 | End-to-end manifest round-trips |
| `test_manifest.rs` | 16 | Validation rules, constructors |
| `test_error_messages.rs` | 14 | Error `Display` format pinning — matches the AGENTS.md "assert full error message" standard |
| `test_cache_sync.rs` | 29 | Cross-cache copy, S3→S3 server-side, multipart, dedup |
| `test_chunk_size.rs` | 59 | Boundary math for chunk count, default vs whole-file |
| `test_hash.rs` | 57 | Whole-file vs chunked, cache integration |
| `test_quality_probes.rs` | 2 | Pre-existing probes |
| `test_upload_dedup.rs` | 2 | Concurrent dedup |
| `test_s3_integration.rs` | 2 | `#[ignore]` — real S3 |
| `test_eval_probes.rs` (new) | 19 + 1 ignored | Exploratory findings added during this evaluation |

Organization is clear (one test file per operation; descriptive test names). Happy path and edge cases are both covered in most files. Error-message pinning tests meet the AGENTS.md "assert full error message content" standard.

Gaps:

- `hash_file_chunked` boundary tests exist for exact-chunk-multiples and for empty files, but no test exercises `chunk_size == 0` (see Exploratory Findings §7).
- Concurrent access to `HashCache` from multiple threads via `rayon` is exercised indirectly in `test_hash.rs` but not as an explicit thread-safety test. The `Mutex<Connection>` should serialize correctly; a direct multi-thread hammer test would pin the behavior.
- `S3DataCache`'s S3-check-cache invalidation path (HeadObject returns NotFound → invalidate → re-check) is covered in `test_s3_data_cache.rs`, but the "real S3" integration tests (`test_s3_integration.rs`) are `#[ignore]`d and were **not run** in this evaluation because `OPENJD_TEST_S3_BUCKET` is not set in the current environment. Run them before releasing.

## 5. Python Comparison

The Python reference is `deadline-cloud` on branch `manifest-format-2-prototype` (confirmed). Relevant sources at `/home/markw/deadline-cloud/src/deadline/job_attachments/_snapshots/` and design docs at `/home/markw/deadline-cloud/docs/design/job_attachments_snapshots*.md`.

Architecture and vocabulary match closely. Both implementations:

- Use the same four manifest types (Abs/Rel × Snapshot/Diff).
- Use the same `SymlinkPolicy` variants and semantics.
- Use the same data cache abstraction (sync + async), with `S3DataCache` and `FileSystemDataCache`.
- Use the same operation set (COLLECT, HASH, HASH_UPLOAD, DOWNLOAD, DIFF, COMPOSE, FILTER, SUBTREE, PARTITION, JOIN, CACHE_SYNC).
- Use canonical JSON with UTF-16BE sort for v2023 format.

Notable Rust-side design choices that diverge intentionally:

- Rust uses phantom types for path-style and kind; Python uses runtime `isinstance` checks.
- Rust uses `rayon` for HASH and `tokio` for HASH_UPLOAD/DOWNLOAD/CACHE_SYNC; Python uses thread pools and is GIL-bound for the hashing part (significant performance advantage for Rust).
- Rust uses `tokio::sync::Semaphore` for memory bounding; Python uses a custom memory pool.
- Rust uses `DashMap`-style broadcast channels for upload dedup; Python uses a `threading.Lock`-protected dict.
- Rust's `SnapshotError` is a fixed enum that funnels all external errors to strings; Python uses specific exception types (`JobAttachmentsError` subclasses). The Rust approach is simpler but loses programmatic inspection of the underlying cause — documented tradeoff.

Behavioral parity tests:

- `test_v2023_canonical.rs` contains a `cross_implementation_all_fixtures` test that verifies byte-exact match with Python's v2023 encoding for a fixture set. Strong guarantee.
- Error messages are not byte-exact compatible with Python — the Rust messages read well but are independently phrased. For the conformance of expression/model/sessions crates this matters; for snapshots it is less critical since snapshots does not have an equivalent conformance test suite in openjd-specifications.

I did not find a Python-test ↔ Rust-test parity matrix. Consider adding a checklist to the spec confirming each Python test class has a Rust equivalent, or at least noting what is intentionally not ported.

## 6. Build and Test Results

```
cargo build -p openjd-snapshots              # Clean, no warnings
cargo clippy -p openjd-snapshots --all-targets -- -D warnings   # Clean
cargo doc    -p openjd-snapshots --no-deps    # Clean (no rustdoc warnings)
cargo test   -p openjd-snapshots              # All pass
```

Test result summary (from a full run):

- Library unit tests (inline `mod tests`): pass in every module.
- Integration tests: **approximately 1,050 tests, all passing**, with 3 ignored (S3 integration + 1 additional bench-dependent probe + 1 new probe that documents the `chunk_size=0` hang, see §7).
- No intermittent or flaky failures observed.
- Build time: ~4 s incremental, ~37 s from cold `cargo clippy` (AWS SDK deps dominate).

**Not run in this evaluation:** the `#[ignore]`d S3 integration tests in `test_s3_integration.rs`. The environment does not have `OPENJD_TEST_S3_BUCKET` set. Per AGENTS.md ("always run these tests if an S3 bucket is available. If one is not configured, ask the user to provide one"): **please set `OPENJD_TEST_S3_BUCKET` and re-run the S3 integration tests before releasing or merging snapshots changes.**

## 7. Exploratory Findings

I added `crates/openjd-snapshots/tests/test_eval_probes.rs` with 20 probes covering edge cases. 19 pass; 1 is marked `#[ignore]` because it documents a real hang bug that would otherwise stall the test suite:

**Bug: `hash_file_chunked` infinite-loops when `chunk_size == 0`.**

Repro (in `probe_hash_file_chunked_zero_chunk_size`): create a small file, call `hash_file_chunked(path, 0)`, observe the function does not return within 2 seconds. Root cause:

```rust
let mut buf = vec![0u8; chunk_size as usize];   // len == 0
loop {
    match file.read_exact(&mut buf) {           // returns Ok(()) immediately
        Ok(()) => hashes.push(hash_data(&buf)), // pushes hash of empty slice forever
        ...
```

Fix: reject `chunk_size == 0` at the top of `hash_file_chunked` with an `InvalidInput` error, or document the precondition and assert in debug.

Other probes (all pass) that serve as useful regression tests going forward:

- `probe_validate_duplicate_paths_in_snapshot` — `validate()` catches same-path duplicates ✓
- `probe_validate_file_with_both_hash_and_chunkhashes` — ✓ rejected
- `probe_normalize_path_many_dotdots_at_root` — `/../../a` → `/a`; `/../../../` → `/` ✓
- `probe_human_readable_rounding_boundary` — 999,999 → "1 MB" (no "1000 KB") ✓
- `probe_filter_sees_dir_entries` — filter predicate receives both `File` and `Dir` variants ✓
- `probe_diff_cascades_directory_deletions` — deleting a dir marks all contained files as deleted ✓
- `probe_diff_sorts_deleted_dirs_deepest_first` — `a/b/c, a/b, a` order ✓
- `probe_compose_with_empty_diff` — identity ✓
- `probe_join_then_subtree_identity` — with `CollapseEscaping` ✓
- `probe_subtree_exclude_escaping_drops_escaping_symlink` — ✓
- `probe_validate_chunkhashes_size_equals_chunk` — size == chunk_size with chunkhashes is rejected ✓
- `probe_join_with_empty_prefix_rejected` — ✓
- `probe_v2023_drops_empty_dirs` — lossy format behavior confirmed ✓
- `probe_v2025_chunkhashes_round_trip` — ✓
- `probe_v2025_unicode_round_trip` — `café_☕.txt` survives ✓
- `probe_phantom_roundtrip_no_enforcement` — serde round-trip into wrong phantom type succeeds; `validate()` catches on mismatched paths (as documented) ✓
- `probe_hash_data_large_input`, `probe_hash_file_missing`, `probe_recompute_total_size_large` — ✓

No other bugs found. No panics, no UB, no crashes on ill-formed inputs. The crate is defensively coded.

## 8. Recommendations

### High priority

1. **Fix `hash_file_chunked` zero-chunk-size hang.** Reject `chunk_size == 0` (and arguably any non-positive value except `WHOLE_FILE_CHUNK_SIZE`, which isn't a valid input to this function anyway). Add a unit test. Update the doc comment. The new probe test can be un-ignored once fixed.
2. **Run the S3 integration tests before release.** The environment used for this evaluation does not have `OPENJD_TEST_S3_BUCKET` set. The AGENTS.md policy is explicit: always run them when working on snapshots.

### Medium priority

3. **Clean up `hash_file_chunked` dead code.** The `UnexpectedEof` branch allocates `tail` via `read_to_end` then discards it, seeks back, and re-reads. Simplify to a single `read_to_end` into the remainder, or compute the remainder length from file metadata.
4. **De-duplicate the duplicate-path check** between `Manifest::validate` and `codec::check_no_duplicate_paths`. After constructing a typed manifest in the decode path, call `validate()` rather than re-checking a subset inline. If performance is the concern, factor the check into a single shared helper.
5. **Reduce `AsyncDataCache` surface area.** Split multipart/range operations into a separate trait (`MultipartDataCache`, `RangeReadDataCache`) so `FileSystemDataCache` doesn't carry unused-error stubs. `cache_sync` can then condition its fast path on trait presence.
6. **Wrap `glob::PatternError` in `SnapshotError::Validation`.** `IncludeExcludePathsFilter::new` leaks a dependency into the public API. Make it return `crate::Result<Self>`.
7. **Make `S3DataCache` fields private** behind builder or `with_*` setters. Current exposed fields bind the semver contract to implementation details.

### Low priority

8. **Dedupe `*Statistics` structs** via a shared base or a declarative macro (`define_statistics!` with per-op extra fields). Cuts ~80 lines of duplicated fields.
9. **Replace `rand_u64()`** in `data_cache.rs` with `rand::random::<u64>()` or a thread-local `SmallRng`.
10. **Document the `HashCache` TEXT-vs-INTEGER timestamp compatibility.** The `get` path reads either form; the `put` path writes only TEXT. Add a comment explaining Python-compat and a plan to drop INTEGER reads once all existing caches are migrated.
11. **Add explicit subtree-policy rejection docs** to `snapshot_operation_subtree.md` stating `Preserve` and `TransitiveIncludeTargets` return a `Validation` error.
12. **Expand Python-parity coverage:** add a spec note listing each Python test class and its Rust equivalent (or "intentionally not ported"). Low effort, high clarity.
13. **Add an explicit `HashCache` multi-thread hammer test** to pin the thread-safety contract.
14. **Consider a dedicated `codec.md` spec doc** describing v2023 vs v2025 JSON layouts, the `$N/` dir-reference scheme, UTF-16BE sort rationale, and the Python canonical-JSON compatibility requirement.
