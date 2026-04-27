// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use openjd_snapshots::{
    codec::{
        decode_manifest, decode_v2023, decode_v2025, encode_abs_snapshot_diff_v2025,
        encode_abs_snapshot_v2025, encode_snapshot_diff_v2023, encode_snapshot_diff_v2025,
        encode_snapshot_v2023, encode_snapshot_v2025, DecodedManifest,
    },
    AbsSnapshot, AbsSnapshotDiff, DirEntry, FileEntry, HashAlgorithm, Manifest, Snapshot,
    SnapshotDiff, WHOLE_FILE_CHUNK_SIZE,
};

fn make_snapshot(files: Vec<FileEntry>) -> Snapshot {
    Manifest::new(HashAlgorithm::Xxh128, WHOLE_FILE_CHUNK_SIZE).with_files(files)
}

fn make_snapshot_diff(files: Vec<FileEntry>) -> SnapshotDiff {
    Manifest::new(HashAlgorithm::Xxh128, WHOLE_FILE_CHUNK_SIZE).with_files(files)
}

fn file(path: &str, hash: &str, size: u64, mtime: u64) -> FileEntry {
    let mut f = FileEntry::file(path, size, mtime);
    f.hash = Some(hash.to_string());
    f
}

// ===== v2023 Snapshot encode/decode =====

#[test]
fn v2023_snapshot_basic_roundtrip() {
    let snap = make_snapshot(vec![
        file("file1.txt", "abc123", 100, 1000),
        file("dir/file2.txt", "def456", 200, 2000),
    ]);

    let json = encode_snapshot_v2023(&snap).unwrap();
    assert!(json.contains("\"manifestVersion\":\"2023-03-03\""));

    let decoded = decode_v2023(&json).unwrap();
    assert_eq!(decoded.files.len(), 2);
    assert_eq!(decoded.hash_alg, HashAlgorithm::Xxh128);
    assert_eq!(decoded.total_size, 300);
    assert_eq!(decoded.file_chunk_size_bytes, WHOLE_FILE_CHUNK_SIZE);

    let by_path: std::collections::HashMap<&str, &FileEntry> =
        decoded.files.iter().map(|f| (f.path.as_str(), f)).collect();
    assert_eq!(by_path["file1.txt"].hash.as_deref(), Some("abc123"));
    assert_eq!(by_path["file1.txt"].size, Some(100));
    assert_eq!(by_path["dir/file2.txt"].hash.as_deref(), Some("def456"));
}

#[test]
fn v2023_snapshot_preserves_all_fields() {
    let snap = make_snapshot(vec![file("test.txt", "hash123", 12345, 9876543210)]);

    let json = encode_snapshot_v2023(&snap).unwrap();
    let decoded = decode_v2023(&json).unwrap();

    assert_eq!(decoded.files.len(), 1);
    let f = &decoded.files[0];
    assert_eq!(f.path, "test.txt");
    assert_eq!(f.hash.as_deref(), Some("hash123"));
    assert_eq!(f.size, Some(12345));
    assert_eq!(f.mtime, Some(9876543210));
    assert!(f.symlink_target.is_none());
    assert!(!f.deleted);
    assert!(f.chunk_hashes.is_none());
}

#[test]
fn v2023_symlinks_collapsed_during_encode() {
    let snap = make_snapshot(vec![
        file("target.txt", "abc123", 100, 1000),
        FileEntry::symlink("link.txt", "target.txt"),
    ]);

    let json = encode_snapshot_v2023(&snap).unwrap();
    let decoded = decode_v2023(&json).unwrap();

    assert_eq!(decoded.files.len(), 2);
    let by_path: std::collections::HashMap<&str, &FileEntry> =
        decoded.files.iter().map(|f| (f.path.as_str(), f)).collect();
    // Symlink collapsed to target's content
    assert_eq!(by_path["link.txt"].hash.as_deref(), Some("abc123"));
    assert_eq!(by_path["link.txt"].size, Some(100));
}

#[test]
fn v2023_empty_dirs_dropped() {
    let snap = make_snapshot(vec![file("other/file.txt", "abc123", 100, 1000)])
        .with_dirs(vec![DirEntry::new("other"), DirEntry::new("empty_dir")]);

    let json = encode_snapshot_v2023(&snap).unwrap();
    // v2023 has no dirs field
    assert!(!json.contains("empty_dir"));
    let decoded = decode_v2023(&json).unwrap();
    assert!(decoded.dirs.is_empty());
}

#[test]
fn v2023_deletions_dropped_from_diffs() {
    let diff = make_snapshot_diff(vec![
        file("new_file.txt", "abc123", 100, 1000),
        FileEntry::deleted("deleted_file.txt"),
    ]);

    let json = encode_snapshot_diff_v2023(&diff).unwrap();
    let decoded = decode_v2023(&json).unwrap();

    assert_eq!(decoded.files.len(), 1);
    assert_eq!(decoded.files[0].path, "new_file.txt");
}

#[test]
fn v2023_missing_hash_error() {
    let snap = make_snapshot(vec![FileEntry::file("file.txt", 100, 1000)]);

    let result = encode_snapshot_v2023(&snap);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("missing a hash"), "got: {err}");
}

#[test]
fn v2023_non_whole_file_chunk_size_error() {
    let snap: Snapshot = Manifest::new(HashAlgorithm::Xxh128, 256 * 1024 * 1024)
        .with_files(vec![file("file.txt", "abc123", 100, 1000)]);

    let result = encode_snapshot_v2023(&snap);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("fileChunkSizeBytes"), "got: {err}");
}

#[test]
fn v2023_diff_non_whole_file_chunk_size_error() {
    let diff: SnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, 256 * 1024 * 1024)
        .with_files(vec![file("file.txt", "abc123", 100, 1000)]);

    let result = encode_snapshot_diff_v2023(&diff);
    assert!(result.is_err());
}

#[test]
fn v2023_empty_snapshot_roundtrip() {
    let snap = make_snapshot(vec![]);
    // v2023 with no files produces empty paths array
    let json = encode_snapshot_v2023(&snap).unwrap();
    let decoded = decode_v2023(&json).unwrap();
    assert_eq!(decoded.files.len(), 0);
    assert_eq!(decoded.total_size, 0);
}

#[test]
fn v2023_diff_basic_roundtrip() {
    let diff = make_snapshot_diff(vec![
        file("new_file.txt", "abc123", 100, 1000),
        file("modified.txt", "def456", 200, 2000),
    ]);

    let json = encode_snapshot_diff_v2023(&diff).unwrap();
    let decoded = decode_v2023(&json).unwrap();

    assert_eq!(decoded.files.len(), 2);
    assert_eq!(decoded.total_size, 300);
}

#[test]
fn v2023_diff_only_deletions_produces_empty() {
    let diff = make_snapshot_diff(vec![
        FileEntry::deleted("deleted1.txt"),
        FileEntry::deleted("deleted2.txt"),
    ]);

    let json = encode_snapshot_diff_v2023(&diff).unwrap();
    let decoded = decode_v2023(&json).unwrap();

    assert_eq!(decoded.files.len(), 0);
    assert_eq!(decoded.total_size, 0);
}

#[test]
fn v2023_diff_symlinks_collapsed() {
    let diff = make_snapshot_diff(vec![
        file("target.txt", "abc123", 100, 1000),
        FileEntry::symlink("link.txt", "target.txt"),
    ]);

    let json = encode_snapshot_diff_v2023(&diff).unwrap();
    let decoded = decode_v2023(&json).unwrap();

    assert_eq!(decoded.files.len(), 2);
    let by_path: std::collections::HashMap<&str, &FileEntry> =
        decoded.files.iter().map(|f| (f.path.as_str(), f)).collect();
    assert_eq!(by_path["link.txt"].hash.as_deref(), Some("abc123"));
}

#[test]
fn v2023_canonical_json_sorted_keys() {
    let snap = make_snapshot(vec![file("a.txt", "h1", 10, 1)]);
    let json = encode_snapshot_v2023(&snap).unwrap();
    // Keys should be alphabetically sorted
    let hash_pos = json.find("\"hashAlg\"").unwrap();
    let version_pos = json.find("\"manifestVersion\"").unwrap();
    let paths_pos = json.find("\"paths\"").unwrap();
    let total_pos = json.find("\"totalSize\"").unwrap();
    assert!(hash_pos < version_pos);
    assert!(version_pos < paths_pos);
    assert!(paths_pos < total_pos);
    // No whitespace
    assert!(!json.contains(' '));
    assert!(!json.contains('\n'));
}

#[test]
fn v2023_paths_sorted_by_utf16_be() {
    let snap = make_snapshot(vec![file("b.txt", "h2", 20, 2), file("a.txt", "h1", 10, 1)]);
    let json = encode_snapshot_v2023(&snap).unwrap();
    let a_pos = json.find("\"a.txt\"").unwrap();
    let b_pos = json.find("\"b.txt\"").unwrap();
    assert!(a_pos < b_pos, "paths should be sorted");
}

// ===== v2025 encode/decode =====

#[test]
fn v2025_snapshot_roundtrip() {
    let snap = make_snapshot(vec![
        file("assets/model.obj", "h1", 1000, 100),
        file("assets/textures/wood.png", "h2", 2000, 200),
    ]);

    let json = encode_snapshot_v2025(&snap).unwrap();
    assert!(json.contains("\"specificationVersion\":\"relative-manifest-snapshot-beta-2025-12\""));

    let decoded = decode_v2025(&json).unwrap();
    let snap2 = match decoded {
        DecodedManifest::Snapshot(s) => s,
        other => panic!(
            "expected Snapshot, got {:?}",
            std::mem::discriminant(&other)
        ),
    };
    assert_eq!(snap2.files.len(), 2);
    assert_eq!(snap2.total_size, 3000);
    assert_eq!(snap2.hash_alg, HashAlgorithm::Xxh128);
}

#[test]
fn v2025_abs_snapshot_roundtrip() {
    let m: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, WHOLE_FILE_CHUNK_SIZE)
        .with_files(vec![file("/tmp/a.txt", "h1", 100, 1)]);

    let json = encode_abs_snapshot_v2025(&m).unwrap();
    assert!(json.contains("absolute-manifest-snapshot"));

    let decoded = decode_v2025(&json).unwrap();
    match decoded {
        DecodedManifest::AbsSnapshot(s) => {
            assert_eq!(s.files.len(), 1);
            assert_eq!(s.files[0].path, "/tmp/a.txt");
        }
        other => panic!(
            "expected AbsSnapshot, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

#[test]
fn v2025_abs_snapshot_diff_roundtrip() {
    let m: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, WHOLE_FILE_CHUNK_SIZE)
        .with_files(vec![
            file("/tmp/a.txt", "h1", 100, 1),
            FileEntry::deleted("/tmp/b.txt"),
        ]);

    let json = encode_abs_snapshot_diff_v2025(&m).unwrap();
    assert!(json.contains("absolute-manifest-diff"));

    let decoded = decode_v2025(&json).unwrap();
    match decoded {
        DecodedManifest::AbsSnapshotDiff(s) => {
            assert_eq!(s.files.len(), 2);
        }
        other => panic!(
            "expected AbsSnapshotDiff, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

#[test]
fn v2025_snapshot_diff_roundtrip() {
    let m = make_snapshot_diff(vec![
        file("new.txt", "h1", 100, 1),
        FileEntry::deleted("old.txt"),
    ]);

    let json = encode_snapshot_diff_v2025(&m).unwrap();
    assert!(json.contains("relative-manifest-diff"));

    let decoded = decode_v2025(&json).unwrap();
    match decoded {
        DecodedManifest::SnapshotDiff(s) => {
            assert_eq!(s.files.len(), 2);
            let deleted = s.files.iter().find(|f| f.path == "old.txt").unwrap();
            assert!(deleted.deleted);
        }
        other => panic!(
            "expected SnapshotDiff, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

#[test]
fn v2025_dir_compression_works() {
    let snap = make_snapshot(vec![
        file("assets/model.obj", "h1", 1000, 100),
        file("assets/textures/wood.png", "h2", 2000, 200),
    ]);

    let json = encode_snapshot_v2025(&snap).unwrap();
    // Should contain $N/ references
    assert!(json.contains("$"), "expected $N/ compression in: {json}");
    // "assets" should be a dir entry
    assert!(json.contains("\"assets\""));
}

#[test]
fn v2025_symlinks_preserved() {
    let snap = make_snapshot(vec![
        file("target.txt", "h1", 100, 1),
        FileEntry::symlink("link.txt", "target.txt"),
    ]);

    let json = encode_snapshot_v2025(&snap).unwrap();
    let decoded = decode_v2025(&json).unwrap();
    let snap2 = match decoded {
        DecodedManifest::Snapshot(s) => s,
        other => panic!(
            "expected Snapshot, got {:?}",
            std::mem::discriminant(&other)
        ),
    };

    let link = snap2.files.iter().find(|f| f.path == "link.txt").unwrap();
    assert_eq!(link.symlink_target.as_deref(), Some("target.txt"));
}

#[test]
fn v2025_chunkhashes_preserved() {
    let mut f = FileEntry::file("big.bin", 10000, 500);
    f.chunk_hashes = Some(vec!["ch1".into(), "ch2".into(), "ch3".into()]);
    // 10000 / 4096 = 2.44 -> ceil = 3 chunks, matches the 3 chunk_hashes above.
    let snap: Snapshot = Manifest::new(HashAlgorithm::Xxh128, 4096).with_files(vec![f]);

    let json = encode_snapshot_v2025(&snap).unwrap();
    let decoded = decode_v2025(&json).unwrap();
    let snap2 = match decoded {
        DecodedManifest::Snapshot(s) => s,
        other => panic!(
            "expected Snapshot, got {:?}",
            std::mem::discriminant(&other)
        ),
    };

    assert_eq!(
        snap2.files[0].chunk_hashes.as_deref(),
        Some(&["ch1".to_string(), "ch2".to_string(), "ch3".to_string()][..])
    );
}

#[test]
fn v2025_deleted_entries_preserved() {
    let diff = make_snapshot_diff(vec![
        file("kept.txt", "h1", 100, 1),
        FileEntry::deleted("gone.txt"),
    ]);

    let json = encode_snapshot_diff_v2025(&diff).unwrap();
    let decoded = decode_v2025(&json).unwrap();
    let diff2 = match decoded {
        DecodedManifest::SnapshotDiff(s) => s,
        other => panic!(
            "expected SnapshotDiff, got {:?}",
            std::mem::discriminant(&other)
        ),
    };

    let gone = diff2.files.iter().find(|f| f.path == "gone.txt").unwrap();
    assert!(gone.deleted);
}

#[test]
fn v2025_parent_manifest_hash_preserved() {
    let diff = make_snapshot_diff(vec![file("f.txt", "h1", 10, 1)])
        .with_parent_hash(Some("parent_hash_xyz".into()));

    let json = encode_snapshot_diff_v2025(&diff).unwrap();
    assert!(json.contains("parent_hash_xyz"));

    let decoded = decode_v2025(&json).unwrap();
    let diff2 = match decoded {
        DecodedManifest::SnapshotDiff(s) => s,
        other => panic!(
            "expected SnapshotDiff, got {:?}",
            std::mem::discriminant(&other)
        ),
    };
    assert_eq!(
        diff2.parent_manifest_hash.as_deref(),
        Some("parent_hash_xyz")
    );
}

#[test]
fn v2025_runnable_preserved() {
    let mut f = file("script.sh", "h1", 50, 1);
    f.runnable = true;
    let snap = make_snapshot(vec![f]);

    let json = encode_snapshot_v2025(&snap).unwrap();
    let decoded = decode_v2025(&json).unwrap();
    let snap2 = match decoded {
        DecodedManifest::Snapshot(s) => s,
        other => panic!(
            "expected Snapshot, got {:?}",
            std::mem::discriminant(&other)
        ),
    };
    assert!(snap2.files[0].runnable);
}

// ===== Auto-detection =====

#[test]
fn auto_detect_v2023() {
    let snap = make_snapshot(vec![file("a.txt", "h1", 10, 1)]);
    let json = encode_snapshot_v2023(&snap).unwrap();

    let decoded = decode_manifest(&json).unwrap();
    match decoded {
        DecodedManifest::Snapshot(s) => {
            assert_eq!(s.files.len(), 1);
        }
        other => panic!(
            "expected Snapshot, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

#[test]
fn auto_detect_v2025() {
    let snap = make_snapshot(vec![file("a.txt", "h1", 10, 1)]);
    let json = encode_snapshot_v2025(&snap).unwrap();

    let decoded = decode_manifest(&json).unwrap();
    match decoded {
        DecodedManifest::Snapshot(s) => {
            assert_eq!(s.files.len(), 1);
        }
        other => panic!(
            "expected Snapshot, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

#[test]
fn auto_detect_unknown_format_error() {
    let result = decode_manifest(r#"{"foo":"bar"}"#);
    assert!(result.is_err());
}

// ===== v2025 canonical JSON =====

#[test]
fn v2025_canonical_json_no_whitespace() {
    let snap = make_snapshot(vec![file("a.txt", "h1", 10, 1)]);
    let json = encode_snapshot_v2025(&snap).unwrap();
    assert!(!json.contains(' '));
    assert!(!json.contains('\n'));
}

#[test]
fn v2025_dirs_with_explicit_entries_preserved() {
    let snap = make_snapshot(vec![file("mydir/f.txt", "h1", 10, 1)])
        .with_dirs(vec![DirEntry::new("mydir")]);

    let json = encode_snapshot_v2025(&snap).unwrap();
    let decoded = decode_v2025(&json).unwrap();
    let snap2 = match decoded {
        DecodedManifest::Snapshot(s) => s,
        other => panic!(
            "expected Snapshot, got {:?}",
            std::mem::discriminant(&other)
        ),
    };
    assert!(snap2.dirs.iter().any(|d| d.path == "mydir"));
}

#[test]
fn v2025_deep_nesting_compression() {
    let snap = make_snapshot(vec![
        file("a/b/c/d.txt", "h1", 10, 1),
        file("a/b/e.txt", "h2", 20, 2),
    ]);

    let json = encode_snapshot_v2025(&snap).unwrap();
    // Should round-trip correctly
    let decoded = decode_v2025(&json).unwrap();
    let snap2 = match decoded {
        DecodedManifest::Snapshot(s) => s,
        other => panic!(
            "expected Snapshot, got {:?}",
            std::mem::discriminant(&other)
        ),
    };
    assert_eq!(snap2.files.len(), 2);
    let by_path: std::collections::HashMap<&str, &FileEntry> =
        snap2.files.iter().map(|f| (f.path.as_str(), f)).collect();
    assert!(by_path.contains_key("a/b/c/d.txt"));
    assert!(by_path.contains_key("a/b/e.txt"));
}

// ===== fileChunkSizeBytes round-trip =====

#[test]
fn v2025_default_chunk_size_included_in_json() {
    use openjd_snapshots::DEFAULT_FILE_CHUNK_SIZE;
    let snap: Snapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(vec![file("a.txt", "abc", 10, 1)]);
    let json = encode_snapshot_v2025(&snap).unwrap();
    assert!(
        json.contains("\"fileChunkSizeBytes\":268435456"),
        "default chunk size should be included in JSON, got: {json}"
    );
}

#[test]
fn v2025_non_default_chunk_size_included_in_json() {
    let chunk_size = 128 * 1024 * 1024i64;
    let snap: Snapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(vec![file("a.txt", "abc", 10, 1)]);
    let json = encode_snapshot_v2025(&snap).unwrap();
    assert!(
        json.contains("\"fileChunkSizeBytes\":134217728"),
        "non-default chunk size should appear in JSON, got: {json}"
    );
}

#[test]
fn v2025_whole_file_chunk_size_included_in_json() {
    let snap: Snapshot = Manifest::new(HashAlgorithm::Xxh128, WHOLE_FILE_CHUNK_SIZE)
        .with_files(vec![file("a.txt", "abc", 10, 1)]);
    let json = encode_snapshot_v2025(&snap).unwrap();
    assert!(
        json.contains("\"fileChunkSizeBytes\":-1"),
        "WHOLE_FILE_CHUNK_SIZE should appear in JSON, got: {json}"
    );
}

#[test]
fn v2025_chunk_size_round_trip_non_default() {
    let chunk_size = 128 * 1024 * 1024i64;
    let snap: Snapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(vec![file("a.txt", "abc", 10, 1)]);
    let json = encode_snapshot_v2025(&snap).unwrap();
    let decoded = match decode_v2025(&json).unwrap() {
        DecodedManifest::Snapshot(s) => s,
        other => panic!(
            "expected Snapshot, got {:?}",
            std::mem::discriminant(&other)
        ),
    };
    assert_eq!(decoded.file_chunk_size_bytes, chunk_size);
}

#[test]
fn v2025_chunk_size_round_trip_whole_file() {
    let snap: Snapshot = Manifest::new(HashAlgorithm::Xxh128, WHOLE_FILE_CHUNK_SIZE)
        .with_files(vec![file("a.txt", "abc", 10, 1)]);
    let json = encode_snapshot_v2025(&snap).unwrap();
    let decoded = match decode_v2025(&json).unwrap() {
        DecodedManifest::Snapshot(s) => s,
        other => panic!(
            "expected Snapshot, got {:?}",
            std::mem::discriminant(&other)
        ),
    };
    assert_eq!(decoded.file_chunk_size_bytes, WHOLE_FILE_CHUNK_SIZE);
}

#[test]
fn v2025_missing_chunk_size_defaults_to_default() {
    use openjd_snapshots::DEFAULT_FILE_CHUNK_SIZE;
    // Simulate a v2025 JSON without fileChunkSizeBytes (e.g. from older Python encoder)
    let json = r#"{"dirs":[],"files":[{"hash":"abc","mtime":1,"name":"a.txt","size":10}],"hashAlg":"xxh128","specificationVersion":"relative-manifest-snapshot-beta-2025-12","totalSize":10}"#;
    let decoded = match decode_v2025(json).unwrap() {
        DecodedManifest::Snapshot(s) => s,
        other => panic!(
            "expected Snapshot, got {:?}",
            std::mem::discriminant(&other)
        ),
    };
    assert_eq!(decoded.file_chunk_size_bytes, DEFAULT_FILE_CHUNK_SIZE);
}

#[test]
fn v2025_chunk_size_round_trip_abs_snapshot() {
    let chunk_size = 64 * 1024 * 1024i64;
    let snap: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(vec![file("/tmp/a.txt", "abc", 10, 1)]);
    let json = encode_abs_snapshot_v2025(&snap).unwrap();
    let decoded = match decode_v2025(&json).unwrap() {
        DecodedManifest::AbsSnapshot(s) => s,
        other => panic!(
            "expected AbsSnapshot, got {:?}",
            std::mem::discriminant(&other)
        ),
    };
    assert_eq!(decoded.file_chunk_size_bytes, chunk_size);
}

#[test]
fn v2025_chunk_size_round_trip_snapshot_diff() {
    let chunk_size = 512 * 1024 * 1024i64;
    let diff: SnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(vec![file("a.txt", "abc", 10, 1)]);
    let json = encode_snapshot_diff_v2025(&diff).unwrap();
    let decoded = match decode_v2025(&json).unwrap() {
        DecodedManifest::SnapshotDiff(s) => s,
        other => panic!(
            "expected SnapshotDiff, got {:?}",
            std::mem::discriminant(&other)
        ),
    };
    assert_eq!(decoded.file_chunk_size_bytes, chunk_size);
}

#[test]
fn v2025_chunk_size_round_trip_abs_snapshot_diff() {
    let chunk_size = 32 * 1024 * 1024i64;
    let diff: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, chunk_size)
        .with_files(vec![file("/tmp/a.txt", "abc", 10, 1)]);
    let json = encode_abs_snapshot_diff_v2025(&diff).unwrap();
    let decoded = match decode_v2025(&json).unwrap() {
        DecodedManifest::AbsSnapshotDiff(s) => s,
        other => panic!(
            "expected AbsSnapshotDiff, got {:?}",
            std::mem::discriminant(&other)
        ),
    };
    assert_eq!(decoded.file_chunk_size_bytes, chunk_size);
}

// ===== v2023 implied directories =====

#[test]
fn v2023_implied_directories_no_warning() {
    // Dir 'foo' is implied by file 'foo/bar.txt'. Encoding should succeed without warning.
    let snap =
        make_snapshot(vec![file("foo/bar.txt", "h1", 10, 1)]).with_dirs(vec![DirEntry::new("foo")]);

    let json = encode_snapshot_v2023(&snap).unwrap();
    let decoded = decode_v2023(&json).unwrap();
    // v2023 drops dirs; file should round-trip fine
    assert_eq!(decoded.files.len(), 1);
    assert_eq!(decoded.files[0].path, "foo/bar.txt");
}

// ===== v2023 diff parent_manifest_hash =====

#[test]
fn v2023_diff_with_parent_manifest_hash() {
    let diff = make_snapshot_diff(vec![file("f.txt", "h1", 10, 1)])
        .with_parent_hash(Some("parent_abc".into()));

    let json = encode_snapshot_diff_v2023(&diff).unwrap();
    let decoded = decode_v2023(&json).unwrap();
    // v2023 does not encode parent_manifest_hash, so it should be None after decode
    assert!(decoded.parent_manifest_hash.is_none());
    assert_eq!(decoded.files.len(), 1);
}

#[test]
fn v2023_diff_without_parent_manifest_hash() {
    let diff = make_snapshot_diff(vec![file("f.txt", "h1", 10, 1)]);

    let json = encode_snapshot_diff_v2023(&diff).unwrap();
    let decoded = decode_v2023(&json).unwrap();
    assert!(decoded.parent_manifest_hash.is_none());
}
