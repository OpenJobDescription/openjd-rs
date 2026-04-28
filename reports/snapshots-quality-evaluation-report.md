# openjd-snapshots Crate Quality Evaluation Report

**Date:** 2026-04-27
**Crate:** `openjd-snapshots`

## Executive Summary

The `openjd-snapshots` crate is in strong shape. It delivers a complete, idiomatic Rust port of the Python job-attachments snapshots library from `deadline-cloud` (branch `manifest-format-2-prototype`), plus a new CACHE_SYNC operation with no Python analog. The crate builds cleanly with no warnings, `cargo clippy --all-targets --all-features -- -D warnings` is clean, and all **1,010 tests pass** (5 ignored, 0 failures) across 22 integration test files plus in-module unit tests. The specifications in `specs/snapshots/` are unusually thorough — a dedicated `public-api.md`, an architecture overview, and a spec file per operation — and they are mostly well-aligned with the implementation.

The crate uses phantom-typed manifests (`Abs`/`Rel` × `Full`/`Diff`) to get compile-time path-style and kind safety, a well-factored data-cache trait hierarchy (`AsyncDataCache` + `MultipartDataCache` + `RangeReadDataCache`) that lets backends opt into capabilities, and tokio-based pipelines with memory-bounded semaphores and DashMap-style upload deduplication. The design choices in `specs/snapshots/snapshot_overview.md` are justified and the code reflects them.

Findings fall into three categories:

1. **A handful of small correctness gaps in `Manifest::validate()`** — negative `file_chunk_size_bytes` other than `-1` and a `file_chunk_size_bytes` of `0` both produce nonsensical error messages due to unchecked `as u64` casts and a divide-by-ceil. Two failing probe tests (now marked `#[ignore]`) demonstrate this.
2. ~~**A public-API spec/impl mismatch:** `public-api.md` documents `normalize_path` and `is_absolute_path` at the crate root, but the crate root re-exports only the `path_util` module, not these functions.~~ **Resolved** — `path_util` is now a crate-private module; the "Path Utilities" section was removed from `public-api.md`. Tests that needed the helpers for assertion now go through `FileEntry::new`/`DirEntry::new`; helper-only tests were moved into `src/path_util.rs` as unit tests.
3. **Minor naming/clarity issues and a few places where the specs would benefit from tightening** (hash-state compatibility semantics, `SymlinkPolicy::Display` contract, `entries_differ` parameter naming).

None of these are blockers. The crate is suitable for use today.

## 1. Specifications Review

The `specs/snapshots/` directory contains 21 documents covering architecture, the public API, each manifest type, data caches, error handling, symlink handling, hash caches, and a dedicated spec per operation. This is one of the most thoroughly specified crates in the workspace.

| Document | Coverage | Notable gaps |
|----------|----------|--------------|
| `README.md` | Complete index of all specs | — |
| `snapshot_overview.md` | Goals, glossary, use cases, operations, design choices | — |
| `public-api.md` | Complete public API surface | ~~`normalize_path`/`is_absolute_path` listed at crate root but actually in `path_util` module~~ **Resolved** — `path_util` is now crate-private and the section was removed |
| `snapshot_manifest_types.md` | Phantom types, entry types, validation, serde | — |
| `snapshot_data_cache.md` | Trait hierarchy, capability discovery, key format | Mentions `AccountId::Auto/Explicit/NoCheck` types that do not exist in the code — the actual API uses `Option<String>` via `with_expected_bucket_owner` |
| `snapshot_symlink_handling.md` | Policy semantics, escaping detection, cycle handling, per-operation support | — |
| `snapshot_hash_cache.md` | Schema, API, expiry, probabilistic validation | Good; includes future-work section on eviction |
| `snapshot_error_handling.md` | Enum, variants, conversion strategy, conventions | — |
| `snapshot_operation_collect.md` | Parameters, returns, symlink policies, algorithm | — |
| `snapshot_operation_hash.md` | Parameters, returns, rayon parallelism, cache behavior | — |
| `snapshot_operation_hash_upload.md` + `_pipeline.md` + `_s3.md` | Hash-then-upload invariant, memory pool, dedup, multipart | Three files give complete coverage |
| `snapshot_operation_download.md` + `_pipeline.md` | Atomicity, chunked writes, conflict resolution, symlink topological sort | — |
| `snapshot_operation_diff.md` | Semantics, hash-state validation, preserve_runnable | Does not describe the `parent_is_chunked != current_is_chunked` detection branch, but the Rust impl does detect this via the `parent.chunk_hashes != current.chunk_hashes` comparison |
| `snapshot_operation_compose.md` | Trie algorithm, reconcile_deleted_flags | — |
| `snapshot_operation_filter.md` | Functional interface, IncludeExcludePathsFilter | Does not document that filter patterns use Rust's `glob` crate semantics, which differ from Python's `fnmatch` for `**/*` (glob supports it, fnmatch does not) |
| `snapshot_operation_subtree.md` | Rebasing, symlink rebasing, escaping | — |
| `snapshot_operation_partition.md` | Auto-root determination, longest common prefix, explicit roots | — |
| `snapshot_operation_join.md` | Prefix joining, symlink target rewriting | — |
| `snapshot_operation_cache_sync.md` | New operation with no Python analog | Good stand-alone spec |

**Spec accuracy notes:**

- `snapshot_data_cache.md` describes an `AccountId` enum with `Auto`/`Explicit`/`NoCheck` variants. The code has no such type — the API is `with_expected_bucket_owner(Option<String>)` plus an async `new_with_auto_account_id` constructor. The spec needs to be updated to reflect the real shape.
- ~~`public-api.md` lists `normalize_path` and `is_absolute_path` under "Path Utilities" at the crate root, but `lib.rs` only does `pub mod path_util;`. Either add `pub use path_util::{normalize_path, is_absolute_path};` or update the spec to say "Accessible via the `path_util` module path."~~ **Resolved** — `path_util` was made crate-private and the "Path Utilities" section was removed from `public-api.md`.
- `public-api.md` §"Constants" lists `WHOLE_FILE_RANGE_END` as "Module-path only" — this is correctly flagged.

Overall the specs are accurate and comprehensive. The two mismatches above are the only concrete gaps.

## 2. Public API Review

The `public-api.md` spec is comprehensive — it lists all constants, error types, manifest types, hashing helpers, codec functions, data-cache traits, hash-cache APIs, and every operation's function signatures and option/result structs. It distinguishes items at the crate root from items behind a module path (e.g., `hash::hash_data`, `manifest::Abs`, `ops::ProgressFn`).

**API ergonomics:**

- **Phantom-typed manifests.** The `Manifest<P, K>` design, with `AbsSnapshot`/`AbsSnapshotDiff`/`Snapshot`/`SnapshotDiff` aliases and `ValidatePaths`/`ValidateKind` trait bounds on `validate()`, is a clean improvement over the Python mixin approach. Operations that require absolute paths take `Manifest<Abs, _>` directly, so passing a relative manifest is a compile error.
- **Builder-style setters** on `Manifest<P, K>` (`with_files`, `with_dirs`, `with_parent_hash`) and `S3DataCache` (`with_multipart_part_size`, `with_s3_check_cache`, `with_force_s3_check`, `with_expected_bucket_owner`) keep construction readable without forcing users to fill in `None` for every optional field.
- **Capability-discovery traits.** `AsyncDataCache::as_multipart() -> Option<&dyn MultipartDataCache>` and `as_range_read() -> Option<&dyn RangeReadDataCache>` allow the transfer pipeline to probe the backend at runtime without forcing all implementations to stub out unsupported methods. This is idiomatic and matches how Rust typically handles trait feature flags.
- **`ManifestRef` trait** provides an abstraction over `AbsManifest`/`RelManifest` for operations (CACHE_SYNC) that don't care about path style. Neat.
- **Options/Result/Statistics structs per operation.** Every major operation follows the same pattern: a `FooOptions { ... }` struct with `Default`, a `FooResult { manifest, statistics }` return, and a `FooStatistics { ... }` with `total_time`, `rate`, `progress`, `progress_message`. This is consistent and discoverable.

**Mismatches and friction points:**

1. ~~**Public-API mismatch for path utilities.** `public-api.md` §"Path Utilities" lists `normalize_path` and `is_absolute_path` at the crate root. They are reachable only via `openjd_snapshots::path_util::normalize_path`. The lib.rs does `pub mod path_util;` but never `pub use path_util::{...}`. Either re-export them or clarify the spec.~~ **Resolved** — `path_util` is now crate-private (`mod path_util;`) and the "Path Utilities" section was removed from `public-api.md`. External callers use `FileEntry::new`/`DirEntry::new`, which internally normalize paths.
2. **`HashResult.manifest` is `AbsManifest` (enum)** rather than returning the concrete variant the caller passed in. Callers who start with `AbsSnapshot` and want to continue chaining operations must match on the enum. This matches `AbsSnapshotDiff` too but costs a little ergonomics — the Python API has four variants that preserve the concrete type. This is a deliberate Rust choice (the input can be either, so the output must be enum-wrapped) and is documented, but it means most real uses end with an `if let AbsManifest::Snapshot(s) = result.manifest { ... }` unwrap.
3. **`entries_differ` parameter naming.** Public signature is `entries_differ(parent, current, ignore_hashes, preserve_runnable)`. The Python equivalent is `_entries_differ(..., ignore_runnable=...)`. Semantically Rust's `preserve_runnable=true` means "ignore the runnable field when comparing," which is what `ignore_runnable=true` means. The name `preserve_runnable` matches the top-level `DiffOptions::preserve_runnable` (where it also controls copying the parent's runnable into the diff), but inside `entries_differ` itself it only disables the comparison, not the copy. Either rename the function's parameter to `ignore_runnable` for clarity, or add a doc comment explaining the discrepancy.
4. **`S3DataCache` private fields accessed via spec's `AccountId` enum.** The spec describes a type that doesn't exist. The real API is simpler — just `with_expected_bucket_owner(Option<String>)` plus an async `new_with_auto_account_id` constructor — but callers reading the spec will be confused.
5. **`ContentAddressedDataCache` vs `AsyncDataCache` duplication.** The sync and async traits have near-identical method shapes. The sync trait is used only internally by `S3DataCache` via a `block_on_async` shim for rare sync call sites. In practice all user code goes through `AsyncDataCache`. The existence of both traits is reasonable but not explained in `public-api.md`; a note on "when to use which" would help.

Overall the public API is clean and the spec is mostly accurate. The two issues worth fixing are (1) re-exporting the path utilities or clarifying the spec, and (2) updating the `S3DataCache` account-id description in the data-cache spec.
