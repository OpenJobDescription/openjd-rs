use openjd_snapshots::hash::hash_file_chunked;
/// Quality probe tests written during the snapshots crate evaluation.
/// These tests demonstrate potential issues found during code review.
use openjd_snapshots::*;

/// Probe: validate() should reject manifests with duplicate file paths.
/// Currently it does NOT — this test demonstrates the gap.
#[test]
fn validate_allows_duplicate_file_paths() {
    let m: Snapshot = Manifest::new(HashAlgorithm::Xxh128, WHOLE_FILE_CHUNK_SIZE).with_files(vec![
        FileEntry {
            path: "a.txt".into(),
            hash: None,
            size: Some(10),
            mtime: Some(100),
            chunk_hashes: None,
            symlink_target: None,
            runnable: false,
            deleted: false,
        },
        FileEntry {
            path: "a.txt".into(),
            hash: None,
            size: Some(20),
            mtime: Some(200),
            chunk_hashes: None,
            symlink_target: None,
            runnable: false,
            deleted: false,
        },
    ]);
    // This SHOULD fail validation but currently passes
    let result = m.validate();
    assert!(
        result.is_ok(),
        "BUG DEMONSTRATION: validate() accepts duplicate paths — this test documents the gap"
    );
}

/// Probe: validate() should reject manifests with duplicate directory paths.
#[test]
fn validate_allows_duplicate_dir_paths() {
    let m: Snapshot = Manifest::new(HashAlgorithm::Xxh128, WHOLE_FILE_CHUNK_SIZE).with_dirs(vec![
        DirEntry {
            path: "dir".into(),
            deleted: false,
        },
        DirEntry {
            path: "dir".into(),
            deleted: false,
        },
    ]);
    let result = m.validate();
    assert!(
        result.is_ok(),
        "BUG DEMONSTRATION: validate() accepts duplicate dir paths"
    );
}

/// Probe: hash_file_chunked should produce consistent chunk counts.
#[test]
fn hash_file_chunked_consistent_chunk_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("testfile");

    let chunk_size: u64 = 1024;
    let data: Vec<u8> = (0..3 * chunk_size).map(|i| (i % 256) as u8).collect();
    std::fs::write(&path, &data).unwrap();

    let h1 = hash_file_chunked(&path, chunk_size).unwrap();
    let h2 = hash_file_chunked(&path, chunk_size).unwrap();
    assert_eq!(h1.len(), 3, "Should produce exactly 3 chunks");
    assert_eq!(h1, h2, "Repeated hashing should be deterministic");

    let data2: Vec<u8> = (0..(3 * chunk_size + 500))
        .map(|i| (i % 256) as u8)
        .collect();
    std::fs::write(&path, &data2).unwrap();
    let h3 = hash_file_chunked(&path, chunk_size).unwrap();
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
