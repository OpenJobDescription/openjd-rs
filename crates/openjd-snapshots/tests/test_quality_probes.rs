// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use openjd_snapshots::hash::hash_file_chunked;
/// Quality probe tests written during the snapshots crate evaluation.
/// These tests demonstrate potential issues found during code review.
use openjd_snapshots::*;

/// Probe: hash_file_chunked should produce consistent chunk counts.
#[test]
fn hash_file_chunked_consistent_chunk_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("testfile");

    let chunk_size: u64 = 1024;
    let data: Vec<u8> = (0..3 * chunk_size).map(|i| (i % 256) as u8).collect();
    std::fs::write(&path, &data).unwrap();

    let h1 = hash_file_chunked(&path, chunk_size, data.len() as u64).unwrap();
    let h2 = hash_file_chunked(&path, chunk_size, data.len() as u64).unwrap();
    assert_eq!(h1.len(), 3, "Should produce exactly 3 chunks");
    assert_eq!(h1, h2, "Repeated hashing should be deterministic");

    let data2: Vec<u8> = (0..(3 * chunk_size + 500))
        .map(|i| (i % 256) as u8)
        .collect();
    std::fs::write(&path, &data2).unwrap();
    let h3 = hash_file_chunked(&path, chunk_size, data2.len() as u64).unwrap();
    assert_eq!(h3.len(), 4, "Should produce 4 chunks for 3*chunk_size+500");
}

/// Probe: Manifest deserialization doesn't enforce phantom type constraints.
#[test]
fn manifest_deserialization_ignores_phantom_types() {
    let abs: AbsSnapshot =
        Manifest::new(HashAlgorithm::Xxh128, WHOLE_FILE_CHUNK_SIZE).with_files(vec![FileEntry {
            path: "/absolute/path.txt".into(),
            hash: Some("abc123".into()),
            size: Some(100),
            mtime: Some(1000),
            chunk_hashes: None,
            symlink_target: None,
            runnable: false,
            deleted: false,
        }]);

    let json = serde_json::to_string(&abs).unwrap();

    // Deserialize as Snapshot (relative) — succeeds despite absolute paths
    let rel: std::result::Result<Snapshot, _> = serde_json::from_str(&json);
    assert!(
        rel.is_ok(),
        "BUG DEMONSTRATION: deserialization doesn't enforce path type constraints"
    );

    // But validate() catches it
    let rel = rel.unwrap();
    assert!(
        rel.validate().is_err(),
        "validate() correctly rejects absolute paths in Rel manifest"
    );
}

// ============================================================================
// Probes added during April 2026 re-evaluation
// ============================================================================

/// BUG: `Manifest::validate()` does not explicitly reject negative
/// `file_chunk_size_bytes` values other than `-1` (`WHOLE_FILE_CHUNK_SIZE`).
/// Casting a negative i64 to u64 wraps to a huge value and the error message
/// becomes nonsensical. The fix is to check `< 0 && != WHOLE_FILE_CHUNK_SIZE`
/// up front in `validate()`. Ignored until fixed.
#[test]
#[ignore]
fn validate_rejects_bad_negative_chunk_size_with_sensible_message() {
    let mut f = FileEntry::file("a.bin", 1024, 1);
    f.chunk_hashes = Some(vec!["a".into(), "b".into()]);
    let m: Snapshot = Manifest::new(HashAlgorithm::Xxh128, -2).with_files(vec![f]);
    let err = m.validate().unwrap_err().to_string();
    // The ideal error would say something about an invalid chunk size.
    // Today it says "size > 18446744073709551614 (chunk size), got 1024".
    assert!(
        !err.contains("18446744073709551614"),
        "validate() leaks the u64-wrapped value into the error message: {err}"
    );
}

/// BUG: `Manifest::validate()` with `file_chunk_size_bytes = 0` and a file
/// carrying `chunk_hashes` divides by zero when computing expected chunk count
/// and produces a nonsensical error ("should have 18446744073709551615 chunks").
/// The fix is to reject `chunk_size == 0` up front. Ignored until fixed.
#[test]
#[ignore]
fn validate_rejects_zero_chunk_size_with_sensible_message() {
    let mut f = FileEntry::file("a.bin", 3, 1);
    f.chunk_hashes = Some(vec!["a".into()]);
    let m: Snapshot = Manifest::new(HashAlgorithm::Xxh128, 0).with_files(vec![f]);
    let err = m.validate().unwrap_err().to_string();
    assert!(
        !err.contains("18446744073709551615"),
        "validate() leaks divide-by-zero into the error message: {err}"
    );
}

/// Probe: `compose_snapshot_with_diffs` with an empty diff list is a no-op
/// clone (not an error). Document the current behaviour.
#[test]
fn compose_with_empty_diffs_is_no_op() {
    let base: Snapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(vec![FileEntry::file("a.txt", 10, 1)]);
    let out = compose_snapshot_with_diffs::<openjd_snapshots::manifest::Rel>(&base, &[])
        .expect("empty diff list should succeed");
    assert_eq!(out.files.len(), 1);
}

/// Probe: v2025 codec correctly rejects duplicate paths at decode time.
#[test]
fn decode_v2025_rejects_duplicate_file_paths() {
    let dup = r#"{
        "specificationVersion":"absolute-manifest-snapshot-beta-2025-12",
        "hashAlg":"xxh128","totalSize":10,"fileChunkSizeBytes":-1,
        "dirs":[],
        "files":[
            {"name":"/a.txt","hash":"abc","size":10,"mtime":1},
            {"name":"/a.txt","hash":"def","size":20,"mtime":2}
        ]
    }"#;
    let err = decode_v2025(dup).unwrap_err().to_string();
    assert!(err.contains("duplicate path"), "got: {err}");
}

// HISTORICAL NOTE: `specs/snapshots/public-api.md` previously documented
// `normalize_path` and `is_absolute_path` at the crate root, but they were
// only accessible via the `path_util` module. The resolution (April 2026) was
// to make `path_util` crate-private — path handling for external callers is
// provided by `openjd-expr`'s `PathFormat`-aware APIs.
