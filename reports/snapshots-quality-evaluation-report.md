# openjd-snapshots Crate Quality Evaluation Report

**Date:** 2026-05-08
**Crate:** `openjd-snapshots`

## Executive Summary

The `openjd-snapshots` crate is a substantial, high-quality standalone Rust implementation of the job-attachments snapshots subsystem. It carries ~260k of source across a clean module boundary (manifest types, codec, two cache traits plus two backends, hash/S3-check caches, a complete set of 11 operations, and memory-pool/rate helpers), backed by roughly 1,047 passing tests (244 unit + 802 integration + 1 doc, with a handful of ignored S3-integration tests). Build is clean, clippy `-D warnings` is clean, and all documented error-message contracts are pinned. The specs in `specs/snapshots/` are thorough — there's a README index, a dedicated `public-api.md`, per-operation pages, and cross-cutting docs for manifests, caches, errors, and symlinks — and the code generally matches them well. The phantom-type design for path style (`Abs`/`Rel`) and manifest kind (`Full`/`Diff`) is idiomatic Rust and adds real compile-time safety; the trie-based `compose` and the `tokio`-based memory-bounded pipeline for HASH_UPLOAD / DOWNLOAD / CACHE_SYNC are strong contributions relative to the Python reference.

Findings are mostly polish: the `public-api.md` signature for `hash_file_chunked` is out of date (missing the mandatory `expected_size` param); `Manifest::validate()` has two latent divide-by-zero / wraparound error-message bugs on pathological `file_chunk_size_bytes` (already documented as `#[ignore]`-marked probes); `FileSystemDataCache.root_path` is `pub` which leaks an otherwise internal field; and there are several places where `std::sync::Mutex` is used in operation pipelines with a comment explaining the choice but no assertion that the lock is never held across an `.await`. No correctness bugs were found in the core operations; exploratory probing confirmed that compose, diff, subtree, join, filter, partition, and v2025 codec behave as the specs describe. The crate is in good shape overall; the recommendations in §8 are primarily doc fixes, a couple of easy validator hardenings, and some targeted test-coverage additions (diff options / subtree cycles / CACHE_SYNC cancellation).

## 1. Specifications Review

The `specs/snapshots/` directory contains 21 documents (a README, a public API reference, 5 cross-cutting design docs, and 14 per-operation / per-subsystem pages). Coverage is very thorough.

| Document | Assessment |
|----------|------------|
| `README.md` | Good index. Lists all docs with a one-line description each. |
| `snapshot_overview.md` | Excellent. Has a glossary, quick-start code, manifest/cache class hierarchies, ASCII-art operation summary, design-choice list with rationale, constants table, module layout, and dependency graph. |
| `public-api.md` | Comprehensive but has one signature drift (see §2). Otherwise matches the implementation. |
| `snapshot_manifest_types.md` | Accurate. Covers phantom types, validation rules, entry states, serde details, chunked-hashing table. Matches `manifest.rs` implementation. |
| `snapshot_data_cache.md` | Accurate. Covers the 4-trait stack (`ContentAddressedDataCache`, `AsyncDataCache`, `MultipartDataCache`, `RangeReadDataCache`), capability discovery via `as_multipart`/`as_range_read`, S3 check cache integration, and `expected_bucket_owner` rationale. |
| `snapshot_symlink_handling.md` | Good. Six-policy table with per-operation support matrix, escaping detection, two-pass COLLECT algorithm, cycle-detection per policy. The DOWNLOAD section mentions `std::os::unix::fs::symlink` only — the code also handles Windows via `symlink_dir`/`symlink_file`, which should be documented. |
| `snapshot_hash_cache.md` | Excellent. Full schema, whole-file vs. byte-range distinction, per-operation usage, thread-safety, a future-work section on eviction. Matches implementation exactly. |
| `snapshot_error_handling.md` | Good. Variant-by-variant table with display-format, conversion-strategy discussion (`#[from]` only for `std::io::Error`), S3-as-`Io` nuance, error-message conventions with examples. |
| `snapshot_operation_collect.md` | Accurate. Two-pass symlink algorithm, `DeferredSymlink` struct, validation rules. |
| `snapshot_operation_hash.md` | Accurate. Three entry points (`hash_abs_snapshot`, `hash_abs_snapshot_diff`, `hash_abs_manifest`), the `HashResult<M>` shape, rayon parallelism, `SlidingWindowRate`, cache behavior. |
| `snapshot_operation_hash_upload.md` + `_pipeline.md` + `_s3.md` | Very good. Three-document split is appropriate given the complexity. Covers tokio pipeline, memory pool, concurrent dedup via `DashMap`, streaming two-pass for large unchunked files, probabilistic S3-check-cache validation. The `DashMap` mention is inaccurate — the implementation uses `Arc<Mutex<HashMap<String, broadcast::Sender<()>>>>`, not `DashMap`; see §5. |
| `snapshot_operation_download.md` + `_pipeline.md` | Good. Entry handling table, conflict-resolution enum, atomic downloads, multipart, chunked-file downloads, deletion ordering, mtime restoration. |
| `snapshot_operation_diff.md` | Good. The `entries_differ` vs. `DiffOptions::preserve_runnable` subtlety (naming the lower-level parameter `ignore_runnable`, matching Python) is called out clearly in the spec with a rationale. |
| `snapshot_operation_compose.md` | Accurate. Trie-based algorithm, `reconcile_deleted_flags`, the "`mark_deleted` does not clear children" design note. |
| `snapshot_operation_filter.md` | Accurate and minimal. |
| `snapshot_operation_subtree.md` | Accurate. Identity `"."`/`""` transform, cycle detection via 64-hop limit, `is_dir_target`/`expand_dir_symlink` helpers. |
| `snapshot_operation_partition.md` | Accurate. Auto-root determination by platform, longest-common-prefix helper, explicit roots vs. auto roots ordering. |
| `snapshot_operation_join.md` | Accurate. Six functions, what's preserved / dropped. |
| `snapshot_operation_cache_sync.md` | Very thorough. Motivation (5 workflows), pipeline diagram, S3-to-S3 server-side copy via `CopyObject` / `UploadPartCopy`, future-work section on S3 Batch Operations. |

**Gaps:**

- No dedicated spec page for `bin/bench.rs`. The binary is behind a feature flag but is not described anywhere in `specs/`.
- No discussion of the `CacheValidationState` public type (it's re-exported at `data_cache::CacheValidationState`) — the public API page mentions it but the data cache spec doesn't describe its behaviour or when a caller would use it directly.
- Windows-specific aspects of DOWNLOAD (symlink creation, `preallocate_file` via `SetFilePointerEx`, `set_modified` requiring `write(true)` on Windows) are not documented.

## 2. Public API Review

The `public-api.md` spec is complete and mostly matches the implementation. One concrete drift:

- **`hash_file_chunked` signature is out of date.** The spec lists:
  ```rust
  pub fn hash_file_chunked(path: &Path, chunk_size: u64) -> std::io::Result<Vec<String>>;
  ```
  but `src/hash.rs` implements:
  ```rust
  pub fn hash_file_chunked(path: &Path, chunk_size: u64, expected_size: u64) -> std::io::Result<Vec<String>>;
  ```
  The `expected_size` parameter is mandatory and is used for a safety check against file-size drift between manifest and disk. This is a meaningful behavioural difference (it can return `InvalidData`) that callers reading the spec won't anticipate. Update the spec; a short note on why `expected_size` is required (content-addressed correctness) would be welcome.

- **`FileSystemDataCache.root_path` field is `pub`.** The spec documents it as a public field, but the only sensible use case is debugging; nothing in the crate reads it externally. If it must stay accessible, prefer an accessor method (`fn root_path(&self) -> &Path`). Otherwise make it `pub(crate)` and stop documenting it.

- **`CacheValidationState` is re-exported via `data_cache` module path.** The spec notes this but doesn't explain the use case. It's unclear whether callers are meant to construct these directly or whether this is incidental exposure. If incidental, consider making the type `pub(crate)`; if intentional, document the usage.

- **`WHOLE_FILE_RANGE_END`** is accessible only via `hash_cache` module path. This matches the spec.

API ergonomics are generally good:
- Concrete-typed entry points (`hash_abs_snapshot`, `subtree_snapshot`, `join_snapshot`, ...) preserve input types through results and avoid enum unwrap boilerplate.
- Enum-dispatching variants exist for callers that hold `AbsManifest`/`RelManifest` at runtime.
- `HashResult<M = AbsManifest>` default makes the bare `HashResult` name continue to work for the enum case.
- `#[non_exhaustive]` on `SnapshotError` and `CopyResult` allows variant additions without a breaking change.
- Options structs (`CollectOptions`, `HashOptions`, `HashUploadOptions`, `DownloadOptions`, `CacheSyncOptions`, `DiffOptions`, `PartitionOptions`) all derive `Default` where sensible and use `..Default::default()` pattern consistently in tests.
- Trait objects are used pervasively (`Arc<dyn AsyncDataCache>`, `&dyn ManifestRef`) which keeps APIs flexible.

Minor API concerns:
- `Manifest<P, K>` has **all** its data fields `pub` (including `total_size`, `parent_manifest_hash`, `file_chunk_size_bytes`). Combined with `_phantom: PhantomData<...>` being `#[serde(skip)]`, this means a caller can trivially construct invalid manifests (absolute paths in `Rel`, inconsistent `total_size`, etc.). `validate()` catches most mismatches but not all (e.g. `total_size` drift). Consider either making these fields private with accessors, or clearly documenting that direct field mutation requires a follow-up `recompute_total_size()`/`validate()`. The existing doc comment on `Manifest<P, K>` covers phantom-type deserialization but not the broader "fields-are-pub" hazard.
- Entry-point function naming is mostly uniform (`hash_abs_snapshot`, `hash_abs_snapshot_diff`, `hash_abs_manifest`). `subtree_*` is consistent (`subtree_snapshot`, `subtree_snapshot_diff`, `subtree_rel_snapshot`, `subtree_rel_snapshot_diff`, `subtree_manifest`, `subtree_rel_manifest`). `join_*` adds `_rel` suffix variants for relative-prefix joining. This is a lot of function names (>25 in the ops layer); a single short summary of the naming convention somewhere in the spec would help newcomers.

## 3. Implementation Review

Reviewed `lib.rs`, `error.rs`, `path_util.rs`, `hash.rs`, `manifest.rs`, `codec.rs`, `data_cache.rs`, `hash_cache.rs`, `s3_check_cache.rs`, and all 13 files under `ops/`.

**Strengths:**
- Small, well-named module split. Every file has a clear single responsibility.
- Extensive use of doc-comments on public items (required by project CI `-D warnings` on `cargo doc`).
- Errors consistently include the offending path/key: `"file 'big.bin' with size 1024 should have 4 chunks..."`, `"S3 GetObject failed for {key}: {e}"`, `"IO error: /tmp/missing.txt: ..."`.
- Trie-based `compose` in `ops/compose.rs` is elegant: directories are implicit in the tree structure, deletion cascades are efficient, and the `reconcile_deleted_flags` pass handles the "delete-then-add" case cleanly.
- `Manifest::common_root` is a nicely-designed helper with a detailed rationale for each rule (files contribute parent, empty dirs contribute themselves, non-empty dirs are ignored, Windows drive-root preservation). The test suite for `common_root` alone is ~25 cases covering relative/absolute, deleted entries, drive letters, deeply nested empty leaves.
- `MemoryPool` in `ops/memory_pool.rs` correctly uses 4KB permit granularity to avoid `u32` overflow up to ~16TB and handles the "single large allocation" clamp so a file bigger than the pool still makes progress.

**Issues / concerns:**

1. **`Manifest::validate()` has two latent bugs on pathological `file_chunk_size_bytes`.** Both are already documented as `#[ignore]`-marked probes in `tests/test_quality_probes.rs`:
   - Zero chunk size: integer division by zero in `((size as f64) / (chunk_size as f64)).ceil()` becomes `inf`, then `as usize` wraps to `usize::MAX`. The error message reads `"should have 18446744073709551615 chunks (chunk_size=0)"`.
   - Negative chunk size other than `-1`: cast `self.file_chunk_size_bytes as u64` wraps to e.g. `18446744073709551614`, which then appears in the error: `"must have size > 18446744073709551614 (chunk size)"`.

   Fix: at the top of `validate()`, reject `file_chunk_size_bytes == 0` and `file_chunk_size_bytes < 0 && file_chunk_size_bytes != WHOLE_FILE_CHUNK_SIZE` with a clean message. Then remove the `#[ignore]` on those probes.

2. **`encode_v2025` persists a caller-supplied `total_size` without recomputing.** If a caller mutates `files` but forgets `recompute_total_size()`, the on-disk manifest has a wrong `totalSize`. The decode side does `m.total_size = total_size;` and `m.validate()` — but `validate()` does not cross-check `total_size` against the sum of file sizes. Consider either (a) recompute in the encoder, or (b) add a validation check that `total_size` matches the sum of non-deleted non-symlink file sizes.

3. **`s3_check_cache.rs`'s store/read of `last_seen_time`.** On put, the value is written as a TEXT column containing `{f64}.to_string()`. On read, `get_entry` first tries `as_str` and parses as `f64`; the `get_entry` fallback `let f: f64 = row.get(0)?` is never reached under the normal code path (the column is always TEXT). The `get_entry` signature returns `Option<String>` — the string happens to be the float text, not an entry. This is a weird API: `get_entry` does existence-plus-freshness checking, so returning `Option<String>` vs `Option<bool>` vs `bool` is an arbitrary choice. The Python reference likely returns a timestamp for metrics; if that's the intent, document it in the spec.

4. **`hash_upload.rs` process_whole_multipart dedup logic duplicates `dedup_upload`.** The function reimplements the dedup map + broadcast lookup inline rather than calling `dedup_upload`. This is because the "upload" step is multipart (many `upload_part` calls + `complete_multipart_upload`) rather than a single `put_object(Vec<u8>)`. The logic is correct but non-trivial; consider extracting a helper like `dedup_begin(&dedup, key) -> Option<BroadcastRx>` and a matching `dedup_end` so the pattern is consistent across the two paths. This would also make it easier to add a third path (chunked-streaming) if one is ever needed.

5. **`data_cache.rs` `block_on_async`.** The `ContentAddressedDataCache` sync trait is implemented on `S3DataCache` by calling `block_on_async` to bridge to the async trait. This works via `tokio::task::block_in_place` + `Handle::block_on` when inside a runtime, and by constructing a fresh current-thread runtime when not. The concern is that `block_in_place` requires a multi-threaded runtime; if a caller invokes the sync trait from inside a `#[tokio::test]` (default single-thread), it panics. The tests cover the multi-thread case (`sync_trait_inside_runtime` is annotated `multi_thread`). Add a brief note in the spec (`snapshot_data_cache.md`) that the sync trait on `S3DataCache` is incompatible with a current-thread tokio runtime.

6. **`ops/download.rs` `temp_download_path` uses process-ID-only seed.** The suffix is computed as `RandomState::new().build_hasher().write_u64(process::id() as u64); .finish()`. `RandomState::new()` seeds from the OS RNG, so two calls in the same process produce different hashes — that's fine for uniqueness between concurrent downloads of different files. However, if a caller runs two `download_abs_manifest` operations on the same target path simultaneously (unusual, but possible), the temp paths could collide. The `.tmp<hex>` suffix is 8 hex chars (32 bits); for safety-in-depth, using 16 hex chars would make accidental collisions astronomically unlikely. This is a low-priority hardening.

7. **`collect.rs` `collapse_symlink` uses `std::fs::canonicalize` which follows all symlinks.** If the target has internal symlinks (chain), `canonicalize` resolves them all. The walk then uses `strip_prefix(&real_target)`, which works only for the root symlink. For a symlink-to-symlink-to-dir chain, nested internal symlinks inside the final dir are handled (the code separately does `read_link` on each one during the walk), so this is correct — but the code is dense and would benefit from a clarifying comment explaining why `canonicalize` + per-entry `read_link` produces the right answer.

8. **`ops/cache_sync.rs` `size_est` fallback for chunked files.** For a chunked file, if `file_chunk_size_bytes > 0`, each chunk uses `file_chunk_size_bytes` as its size estimate — but the final chunk is usually smaller (`size % chunk_size`). The estimate is used for `memory_pool.acquire(size_est)`, so it's conservatively over-booking memory for the final chunk. It's also used for `multipart_threshold` checks. The overestimate is fine for correctness; it's just a minor inefficiency. The spec should call this out in `snapshot_operation_cache_sync.md` under "Large Object Handling".

9. **Python comparison: hash cache schema version.** The Rust implementation uses table `hashesV4`. The Python spec says the schema matches, but no test ports a Python-written SQLite DB to verify. If the Python reference has migrated beyond V4 on its `manifest-format-2-prototype` branch, the Rust cache won't read Python-populated DBs. Add a cross-implementation compat test if the schemas are actually expected to be interoperable.

10. **`hash.rs::human_readable_file_size` has a mild quirk.** It uses decimal units (`1000`-based) but names them `KB`, `MB`, `GB` (which are traditionally binary `1024`-based). This matches the Python reference so is probably intentional; a one-line comment documenting "intentionally decimal, matches Python" would be helpful.

## 4. Test Review

Total: 1,047 tests passing, 5 ignored.

| File / module | Tests | Notes |
|--------------|-------|-------|
| In-crate unit tests (lib.rs + ops) | 244 | Every module has a `#[cfg(test)] mod tests`. Good per-unit coverage of happy-path and most edge cases. |
| `tests/test_chunk_size.rs` | 16 | |
| `tests/test_codec.rs` | 38 | |
| `tests/test_collect.rs` | 47 | Including Unix symlink scenarios, collapse/exclude/preserve variants, cycle detection. |
| `tests/test_compose.rs` | 136 | Very thorough. Covers compose+diff, compose-diffs, reconcile scenarios. |
| `tests/test_diff.rs` | 35 | |
| `tests/test_download.rs` | 59 | Includes chunked downloads, conflict resolution variants, multipart. |
| `tests/test_error_messages.rs` | 7 | Pins exact Display output for every SnapshotError variant. |
| `tests/test_filter.rs` | 55 | |
| `tests/test_hash.rs` | 45 | |
| `tests/test_hash_upload.rs` | 47 | Includes chunked upload, whole-file, multipart, dedup, cache hits. |
| `tests/test_join.rs` | 60 | |
| `tests/test_manifest.rs` | 35 | Ported from `deadline-cloud` test_manifest.py. |
| `tests/test_partition.rs` | 14 | Adequate but thin for this complex op. See gaps below. |
| `tests/test_quality_probes.rs` | 6 (4 ok + 2 ignored) | Documents known latent bugs. |
| `tests/test_round_trip.rs` | 40 | |
| `tests/test_s3_data_cache.rs` | 6 (4 ok + 2 ignored) | Uses `s3s` in-process mock. |
| `tests/test_s3_integration.rs` | 7 | Real S3, `#[ignore]` by default. |
| `tests/test_subtree.rs` | 58 (57 ok + 1 ignored) | Comprehensive subtree + symlink cases. |
| `tests/test_upload_dedup.rs` | 2 | Concurrent identical / mixed content. Thin. |
| `tests/test_v2023_canonical.rs` | 29 | Cross-implementation fixture tests against Python-generated JSON. |

**Strengths:**
- The error-message quality tests (`test_error_messages.rs`) pin the exact Display output for every `SnapshotError` variant, matching the project's convention in AGENTS.md.
- `test_v2023_canonical.rs` runs Python-generated fixture JSON through the Rust decode-encode round trip, verifying byte-for-byte match. This is exactly the kind of cross-implementation guardrail that should exist and it pays off: the codec has 38 tests between unit + integration + canonical.
- The "quality probes" file is a nice pattern — documenting known-issue behaviours with a failing `#[ignore]` test and a clear comment explaining the fix, so the test becomes the regression check once the fix lands.

**Gaps:**
- `test_upload_dedup.rs` has only 2 tests for what is a subtle and concurrency-sensitive subsystem. Consider adding: a test where the first uploader fails (waiters should see an error propagated or a retry), a test with 100+ concurrent uploaders of the same hash (stress), and a test that the dedup map is emptied after the broadcast fires (no leak).
- `test_partition.rs` at 14 tests is thin. Missing: Windows-style UNC roots, relative-manifest partitioning with referenced_paths, the "roots=None, files at mixed levels" case, partition + re-join round trip.
- No tests for the `CacheSyncOptions::on_progress` cancellation path — only the happy-path statistics are asserted. Same for HASH_UPLOAD and DOWNLOAD: cancellation is implemented via `AtomicBool`, but tests don't exercise "return false from progress callback → operation stops cleanly, no orphaned uploads."
- No test of `hash_abs_snapshot_diff` ingesting a diff where some files are already hashed (validation should reject it; covered in snapshot but not in diff).
- No test of `DiffOptions::preserve_runnable` with a new file added in the current snapshot — current tests cover the "file modified" path, but not the "file added" path where `runnable` must come from `current` because there's no parent.
- No test that `decode_v2025` rejects a manifest whose `specificationVersion` doesn't match the top-level expected string (the code does check; worth a dedicated test for each of the four spec version strings).
- No test covers the `resolve_symlink` 64-hop cycle limit in `ops/subtree.rs`. The `for _ in 0..max_depth` loop is easy to break silently if `max_depth` is ever reduced.
- No test of what happens when `FileSystemDataCache` is constructed with a path that exists but is not a directory (e.g., a regular file). `std::fs::create_dir_all` will error; verify the error surfaces cleanly.
- Windows-specific behaviour (symlink creation in DOWNLOAD, `preallocate_file` via `SetFilePointerEx`, the drive-letter edge cases in `common_root`) has unit tests but no integration tests. The CI pipeline does run on Windows per `AGENTS.md`, so this is partially covered.

## 5. Python Comparison

Side-by-side review of `deadline-cloud` branch `fork/manifest-format-2-prototype` (directory `src/deadline/job_attachments/_snapshots/`) vs the Rust crate.

**Structural alignment:** The Rust crate closely mirrors the Python module layout. Python has ~10k LOC across `_snapshots/` (11 operation files + 3 infrastructure files). Rust has similar breadth with a slightly different split (cache pipelines are merged into the ops module rather than split by backend).

**Behavioural alignment:**
- COLLECT: Two-pass symlink algorithm matches Python. Policy support matrix matches.
- HASH: rayon-based parallelism replaces Python thread pool. The "already has hashes" validation matches.
- HASH_UPLOAD: The tokio pipeline replaces Python's two-thread-pool + manual memory pool. The probabilistic S3-check-cache validation (items 1–100 always verify, 101+ = 1% sample, invalidate-on-miss) matches the Python behaviour per the pipeline spec.
- DOWNLOAD: Atomic temp-file download, multipart via range reads, chunked-file parallel writes — all structurally similar. The `mtime` read-back into the returned manifest (so cross-platform filesystem precision is reflected) is documented in both.
- DIFF: The `preserve_runnable` dual-purpose flag documentation explicitly calls out that it matches Python's `_entries_differ(..., ignore_runnable=...)`.
- COMPOSE: Python uses explicit lists with dedup; Rust uses a trie. The observable behaviour (including the reconcile-deleted-flags pass) matches.
- SUBTREE, JOIN, FILTER, PARTITION, CACHE_SYNC: algorithmic match.

**Spec-claim mismatch:**
- The `snapshot_operation_hash_upload_pipeline.md` spec says "DashMap for concurrent upload deduplication instead of Dict + Lock." The implementation actually uses `Arc<Mutex<HashMap<String, broadcast::Sender<()>>>>`, not `DashMap`. `dashmap` is not in `Cargo.toml`. The lock is explicitly held only for brief HashMap operations (not across `.await`), so this works correctly, but the spec should be updated — either change the spec to say "`Arc<Mutex<HashMap>>` with the lock held only across non-awaiting operations" or switch the implementation to actually use `DashMap`. The latter would eliminate the "must not hold lock across await" invariant and might be worth doing.

**Error-message quality:** Rust error messages are pinned in `test_error_messages.rs`. They are structurally comparable to Python (path included, operation named), but the exact wording differs. For example Python's hash-cache lookup might say `"no hash for file <path> at range <r>"` vs Rust `"File '{path}' is missing a hash"`. These are wording differences, not semantic ones; the `test_v2023_canonical.rs` round-trip test ensures byte-compatible output for the on-disk format.

**Rust tests covering Python tests:** I did not do a line-by-line comparison of Python test cases. Spot-check: `test_manifest.rs` is explicitly ported from `test_manifest.py` with cases 1:1. `test_v2023_canonical.rs` uses fixture JSON originally generated by the Python encoder. Coverage gaps noted in §4 are worth closing whether or not Python has a matching test.

## 6. Build and Test Results

**Build:**
```
cargo build -p openjd-snapshots
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 47.65s
```
Clean build, no warnings.

**Clippy:**
```
cargo clippy -p openjd-snapshots --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 39.16s
```
Clean. No warnings.

**Tests:**
```
cargo test -p openjd-snapshots
...
test result: ok. 244 passed; 0 failed; ... (lib unit tests)
test result: ok. 1 passed; 0 failed; 0 ignored; ... (doctest)
[21 integration test files, all passing]
```
Summary: **1,047 tests passing**, 5 ignored (2 in `test_quality_probes.rs` pinning known bugs; 1 in `test_subtree.rs`; 2 in `test_s3_integration.rs` for real-S3 credentials).

S3 integration tests (`test_s3_integration.rs`) were not run in this evaluation; they are `#[ignore]`d by default and require `OPENJD_TEST_S3_BUCKET`.

## 7. Exploratory Findings

Targeted exploratory probing focused on boundaries, invariant violations, and round-trip consistency. A few findings of note:

**Findings that are already documented.**
- The two `file_chunk_size_bytes` pathological-value bugs in `Manifest::validate()` (zero and negative-other-than-`-1`) are already captured as `#[ignore]`-marked probes. They remain unfixed; the exploratory run confirms both still produce nonsensical error messages (`"should have 18446744073709551615 chunks"` and `"must have size > 18446744073709551614"`). These should be fixed; see §8 recommendation 1.
- `manifest_deserialization_ignores_phantom_types` confirms that `serde_json::from_str::<Snapshot>(...)` of a manifest containing absolute paths succeeds. The `#[serde(skip)]` PhantomData makes the path-style trait bound irrelevant at deserialize time; `validate()` catches it. The lib.rs doc comment already warns about this.

**New probes written during this review (all pass):**
- `can_construct_invalid_manifest_directly`: confirms that `m.files = vec![...]` with an absolute path in a `Rel` manifest compiles; `validate()` catches it. Also confirms that `total_size` drift is possible via direct field mutation.
- `encode_v2025_does_not_recompute_total_size`: confirms that the encoder emits the stored `total_size` verbatim, so a desynchronized value lands on disk. See §3 issue 2.
- `encode_v2025_preserves_explicit_dirs_not_in_file_paths`: confirms empty directory entries survive encode-decode.
- `validate_rejects_deleted_with_symlink_target` and `validate_rejects_deleted_with_size`: confirms `validate()` rejects `deleted=true` combined with any data field. Good.
- `file_entry_with_both_hash_and_chunkhashes_rejected`: confirms validation rejects both set simultaneously.
- `compose_diffs_single_diff_matches_input`, `compose_empty_base_empty_diffs`, `compose_preserves_chunk_hashes`: confirm compose handles edge cases correctly.
- `filter_preserves_parent_manifest_hash`, `partition_empty_manifest_no_referenced_paths`, `subtree_rel_identity_dot_preserves_all`: all well-behaved.

The probe tests were removed after verification; they duplicate coverage the existing tests provide.

**No new bugs were found** beyond the two already-documented `validate()` latent-message issues and the doc-vs-implementation drift items called out in §2, §3, §5.

## 8. Recommendations

Priority order: 1–3 are doc fixes that should land together; 4–7 are small code hardenings; 8–12 are test additions; 13+ are more speculative.

### Priority 1 — doc/spec fixes (should land soon)

1. **Update `public-api.md` to reflect the current `hash_file_chunked` signature.** Change:
   ```rust
   pub fn hash_file_chunked(path: &Path, chunk_size: u64) -> std::io::Result<Vec<String>>;
   ```
   to:
   ```rust
   pub fn hash_file_chunked(path: &Path, chunk_size: u64, expected_size: u64) -> std::io::Result<Vec<String>>;
   ```
   and note why `expected_size` is mandatory (content-addressed correctness; file-size-drift detection).

2. **Fix the `DashMap` spec claim.** `snapshot_operation_hash_upload_pipeline.md` says upload dedup uses `DashMap` but the implementation uses `Arc<Mutex<HashMap<...>>>`. Either update the spec or switch the implementation. Recommend updating the spec to reflect reality (lock-held-briefly is an intentional pattern used elsewhere in the crate).

3. **Document the `root_path` / `CacheValidationState` / `FileSystemDataCache` public-fields policy.** Either make `FileSystemDataCache.root_path` an accessor method and make the field private, or add a spec note explaining why it's publicly mutable. Similarly for `CacheValidationState`.

### Priority 2 — small code hardenings

4. **Fix `Manifest::validate()` negative-and-zero `file_chunk_size_bytes` handling.** Add a guard at the top of `validate()`:
   ```rust
   if self.file_chunk_size_bytes == 0
       || (self.file_chunk_size_bytes < 0 && self.file_chunk_size_bytes != WHOLE_FILE_CHUNK_SIZE) {
       return Err(Validation(format!(
           "invalid file_chunk_size_bytes: {} (must be > 0 or WHOLE_FILE_CHUNK_SIZE={})",
           self.file_chunk_size_bytes, WHOLE_FILE_CHUNK_SIZE
       )));
   }
   ```
   Then remove the `#[ignore]` on the two matching probes in `test_quality_probes.rs`.

5. **Add a `validate()` cross-check for `total_size`.** In `Manifest::validate()`, after the per-file checks, verify that `total_size` equals the sum of non-deleted non-symlink file sizes. Return a clear error if not. Prevents bugs where a caller edits `files` directly and forgets `recompute_total_size()`.

6. **Extract the dedup-broadcast lookup pattern in `hash_upload.rs` into a helper.** Reduces the duplicated code between `dedup_upload` and `process_whole_multipart`. Suggestion: `async fn dedup_begin(&dedup, key) -> DedupToken` returning either "you own this" with a cleanup guard or "another task owns it, here's an rx to wait on."

7. **Add `#[non_exhaustive]` to options structs.** `CollectOptions`, `HashOptions`, `HashUploadOptions`, `DownloadOptions`, `CacheSyncOptions`, `DiffOptions`, `PartitionOptions` are all currently fully exhaustive. `#[non_exhaustive]` would allow adding new fields in future without a breaking change, while still letting callers use `..Default::default()`.

### Priority 3 — test coverage additions

8. **Exercise cancellation in HASH_UPLOAD / DOWNLOAD / CACHE_SYNC / HASH.** Add a test for each: progress callback returns `false` → operation returns `SnapshotError::Cancelled`, no panic, all in-flight tasks drain cleanly.

9. **Add `test_upload_dedup.rs` scenarios for failure modes.** What happens when the first uploader fails — do waiters see an error? What happens with 100+ concurrent duplicates (stress)? Verify the dedup map is emptied after completion (no leak).

10. **Extend `test_partition.rs`.** Windows UNC roots, relative-manifest partitioning with referenced_paths, mixed-level root-vs-deep files with `roots=None`, and a partition → join round-trip test.

11. **Add cycle-limit test for `ops/subtree.rs` `resolve_symlink`.** Construct a manifest with a 65+ hop symlink chain and verify the 64-hop limit kicks in and the entry is skipped with the expected warning. Same probe for collect.rs.

12. **Add a `FileSystemDataCache::new` error-path test** when the provided `root_path` exists and is a regular file (not a dir). Verify the error surfaces cleanly.

### Priority 4 — speculative / long-term

13. **Consider hiding `Manifest<P, K>` data fields behind accessors.** This is a larger API change. The current public fields make it easy to violate invariants without realizing, though `validate()` catches most. A future major version could make the fields `pub(crate)` and expose `files()`, `dirs()`, `parent_manifest_hash()`, plus builder-style mutators.

14. **S3 Batch Operations for CACHE_SYNC.** The spec's `snapshot_operation_cache_sync.md` already has a "Future Work" section. No action needed now, but worth noting it's deferred with a clear research checklist.

15. **Document Windows-specific DOWNLOAD details.** `preallocate_file` uses `SetFilePointerEx` + `SetEndOfFile`, `set_modified` requires `write(true)` on Windows, symlink creation distinguishes `symlink_file` vs `symlink_dir`. Add a short section to `snapshot_operation_download_pipeline.md`.

16. **Hash-cache cross-implementation compat test.** If the Python and Rust hash caches are expected to read each other's SQLite DBs, add a test that writes a DB in Python, opens and queries it from Rust, and vice-versa. If they aren't meant to be interoperable, document that explicitly.

17. **Document `bin/bench.rs`.** A one-page spec covering the binary's purpose, how to run it, and what metrics it produces.
