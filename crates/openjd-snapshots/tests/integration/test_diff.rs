// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

/// Rust port of the first ~30 tests from
/// deadline-cloud/test/unit/deadline_job_attachments/snapshots/operations/test_diff_snapshots.py
use openjd_snapshots::{
    diff_snapshots, entries_differ, filter_manifest, AbsSnapshot, DiffOptions, DirEntry, FileEntry,
    HashAlgorithm, Manifest, Snapshot, DEFAULT_FILE_CHUNK_SIZE,
};

fn abs(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> AbsSnapshot {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(files)
        .with_dirs(dirs)
}

fn rel(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> Snapshot {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(files)
        .with_dirs(dirs)
}

fn opts() -> DiffOptions {
    DiffOptions::default()
}

fn hfile(path: &str, hash: &str, size: u64, mtime: u64) -> FileEntry {
    let mut e = FileEntry::file(path, size, mtime);
    e.hash = Some(hash.into());
    e
}

// ===== TestComputeDiffManifestAbsSnapshot =====

#[test]
fn abs_no_changes_returns_empty_diff() {
    let m = abs(vec![hfile("/file.txt", "hash1", 100, 1000)], vec![]);
    let diff = diff_snapshots(&m, &m, &opts()).unwrap();
    assert_eq!(diff.files.len(), 0);
    assert_eq!(diff.dirs.len(), 0);
}

#[test]
fn abs_new_file_detected() {
    let parent = abs(vec![hfile("/old.txt", "hash1", 100, 1000)], vec![]);
    let current = abs(
        vec![
            hfile("/old.txt", "hash1", 100, 1000),
            hfile("/new.txt", "hash2", 50, 2000),
        ],
        vec![],
    );
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    let new = diff.files.iter().find(|f| f.path == "/new.txt").unwrap();
    assert_eq!(new.hash.as_deref(), Some("hash2"));
    assert!(!new.deleted);
}

#[test]
fn abs_modified_file_detected() {
    let parent = abs(vec![hfile("/file.txt", "hash1", 100, 1000)], vec![]);
    let current = abs(vec![hfile("/file.txt", "hash2", 100, 2000)], vec![]);
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].path, "/file.txt");
    assert_eq!(diff.files[0].hash.as_deref(), Some("hash2"));
}

#[test]
fn abs_same_hash_different_mtime_is_modified() {
    let parent = abs(vec![hfile("/file.txt", "hash1", 100, 1000)], vec![]);
    let current = abs(vec![hfile("/file.txt", "hash1", 100, 2000)], vec![]);
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].mtime, Some(2000));
}

#[test]
fn abs_same_hash_different_size_is_modified() {
    let parent = abs(vec![hfile("/file.txt", "hash1", 100, 1000)], vec![]);
    let current = abs(vec![hfile("/file.txt", "hash1", 200, 1000)], vec![]);
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].size, Some(200));
}

#[test]
fn abs_deleted_file_has_marker() {
    let parent = abs(
        vec![
            hfile("/keep.txt", "hash1", 100, 1000),
            hfile("/delete.txt", "hash2", 200, 2000),
        ],
        vec![],
    );
    let current = abs(vec![hfile("/keep.txt", "hash1", 100, 1000)], vec![]);
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    let del = diff.files.iter().find(|f| f.path == "/delete.txt").unwrap();
    assert!(del.deleted);
}

#[test]
fn abs_unchanged_file_not_in_diff() {
    let parent = abs(
        vec![
            hfile("/unchanged.txt", "hash1", 100, 1000),
            hfile("/changed.txt", "hash2", 200, 2000),
        ],
        vec![],
    );
    let current = abs(
        vec![
            hfile("/unchanged.txt", "hash1", 100, 1000),
            hfile("/changed.txt", "hash3", 200, 3000),
        ],
        vec![],
    );
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    let paths: Vec<&str> = diff.files.iter().map(|f| f.path.as_str()).collect();
    assert!(!paths.contains(&"/unchanged.txt"));
    assert!(paths.contains(&"/changed.txt"));
}

#[test]
fn abs_parent_manifest_hash_stored() {
    let m = abs(vec![hfile("/file.txt", "hash1", 100, 1000)], vec![]);
    let o = DiffOptions {
        parent_manifest_hash: Some("abc123def456".into()),
        ..opts()
    };
    let diff = diff_snapshots(&m, &m, &o).unwrap();
    assert_eq!(diff.parent_manifest_hash.as_deref(), Some("abc123def456"));
}

#[test]
fn abs_parent_manifest_hash_optional() {
    let m = abs(vec![hfile("/file.txt", "hash1", 100, 1000)], vec![]);
    let diff = diff_snapshots(&m, &m, &opts()).unwrap();
    assert!(diff.parent_manifest_hash.is_none());
}

#[test]
fn abs_new_directory_included() {
    let parent = abs(vec![], vec![DirEntry::new("/old_dir")]);
    let current = abs(
        vec![],
        vec![DirEntry::new("/old_dir"), DirEntry::new("/new_dir")],
    );
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    let dir_paths: Vec<&str> = diff.dirs.iter().map(|d| d.path.as_str()).collect();
    assert!(dir_paths.contains(&"/new_dir"));
}

#[test]
fn abs_deleted_directory_has_marker() {
    let parent = abs(
        vec![],
        vec![DirEntry::new("/keep_dir"), DirEntry::new("/delete_dir")],
    );
    let current = abs(vec![], vec![DirEntry::new("/keep_dir")]);
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    let del = diff.dirs.iter().find(|d| d.path == "/delete_dir").unwrap();
    assert!(del.deleted);
}

#[test]
fn abs_symlink_change_detected() {
    let parent = abs(
        vec![FileEntry::symlink("/link.txt", "/old_target.txt")],
        vec![],
    );
    let current = abs(
        vec![FileEntry::symlink("/link.txt", "/new_target.txt")],
        vec![],
    );
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].path, "/link.txt");
    assert_eq!(
        diff.files[0].symlink_target.as_deref(),
        Some("/new_target.txt")
    );
}

#[test]
fn abs_new_symlink_included() {
    let parent = abs(vec![], vec![]);
    let current = abs(vec![FileEntry::symlink("/link.txt", "/target.txt")], vec![]);
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].symlink_target.as_deref(), Some("/target.txt"));
}

#[test]
fn abs_deleted_symlink_has_marker() {
    let parent = abs(vec![FileEntry::symlink("/link.txt", "/target.txt")], vec![]);
    let current = abs(vec![], vec![]);
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].path, "/link.txt");
    assert!(diff.files[0].deleted);
}

#[test]
fn abs_preserves_runnable_flag() {
    let parent = abs(vec![], vec![]);
    let mut f = hfile("/script.sh", "hash1", 100, 1000);
    f.runnable = true;
    let current = abs(vec![f], vec![]);
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    assert!(diff.files[0].runnable);
}

#[test]
fn abs_preserves_chunkhashes() {
    let parent = abs(vec![], vec![]);
    let mut f = FileEntry::file("/large.bin", 512 * 1024 * 1024, 1000);
    f.chunk_hashes = Some(vec!["chunk1".into(), "chunk2".into()]);
    let current = abs(vec![f], vec![]);
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    assert_eq!(
        diff.files[0].chunk_hashes.as_deref(),
        Some(&["chunk1".to_string(), "chunk2".to_string()][..])
    );
}

#[test]
fn abs_chunked_file_modification_detected() {
    let mut pf = FileEntry::file("/large.bin", 512 * 1024 * 1024, 1000);
    pf.chunk_hashes = Some(vec!["chunk1".into(), "chunk2".into()]);
    let mut cf = FileEntry::file("/large.bin", 512 * 1024 * 1024, 2000);
    cf.chunk_hashes = Some(vec!["chunk1".into(), "chunk3".into()]);
    let parent = abs(vec![pf], vec![]);
    let current = abs(vec![cf], vec![]);
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    assert_eq!(diff.files.len(), 1);
    assert_eq!(
        diff.files[0].chunk_hashes.as_deref(),
        Some(&["chunk1".to_string(), "chunk3".to_string()][..])
    );
}

#[test]
fn abs_total_size_calculated() {
    let parent = abs(vec![hfile("/delete.txt", "h1", 100, 1000)], vec![]);
    let current = abs(vec![hfile("/new.txt", "h2", 50, 2000)], vec![]);
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    assert_eq!(diff.total_size, 50);
}

#[test]
fn abs_symlinks_not_counted_in_total_size() {
    let parent = abs(vec![], vec![]);
    let current = abs(
        vec![
            hfile("/file.txt", "h1", 100, 1000),
            FileEntry::symlink("/link.txt", "/file.txt"),
        ],
        vec![],
    );
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    assert_eq!(diff.total_size, 100);
}

// ===== TestComputeDiffManifestRelSnapshot =====

#[test]
fn rel_no_changes_returns_empty_diff() {
    let m = rel(vec![hfile("file.txt", "hash1", 100, 1000)], vec![]);
    let diff = diff_snapshots(&m, &m, &opts()).unwrap();
    assert_eq!(diff.files.len(), 0);
    assert_eq!(diff.dirs.len(), 0);
}

#[test]
fn rel_new_file_detected() {
    let parent = rel(vec![hfile("old.txt", "hash1", 100, 1000)], vec![]);
    let current = rel(
        vec![
            hfile("old.txt", "hash1", 100, 1000),
            hfile("new.txt", "hash2", 50, 2000),
        ],
        vec![],
    );
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    let new = diff.files.iter().find(|f| f.path == "new.txt").unwrap();
    assert_eq!(new.hash.as_deref(), Some("hash2"));
}

#[test]
fn rel_deleted_file_has_marker() {
    let parent = rel(
        vec![
            hfile("keep.txt", "hash1", 100, 1000),
            hfile("delete.txt", "hash2", 200, 2000),
        ],
        vec![],
    );
    let current = rel(vec![hfile("keep.txt", "hash1", 100, 1000)], vec![]);
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    let del = diff.files.iter().find(|f| f.path == "delete.txt").unwrap();
    assert!(del.deleted);
}

// ===== TestEntriesDiffer =====

#[test]
fn entries_differ_same_hash_not_different() {
    let e1 = hfile("/f.txt", "abc123", 10, 1000);
    let e2 = hfile("/f.txt", "abc123", 10, 1000);
    assert!(!entries_differ(&e1, &e2, false, false));
}

#[test]
fn entries_differ_different_hash_is_different() {
    let e1 = hfile("/f.txt", "abc123", 10, 1000);
    let e2 = hfile("/f.txt", "def456", 10, 1000);
    assert!(entries_differ(&e1, &e2, false, false));
}

#[test]
fn entries_differ_different_mtime_is_different() {
    let e1 = hfile("/f.txt", "abc123", 10, 1000);
    let e2 = hfile("/f.txt", "abc123", 10, 2000);
    assert!(entries_differ(&e1, &e2, false, false));
}

#[test]
fn entries_differ_different_size_is_different() {
    let e1 = hfile("/f.txt", "abc123", 10, 1000);
    let e2 = hfile("/f.txt", "abc123", 20, 1000);
    assert!(entries_differ(&e1, &e2, false, false));
}

#[test]
fn entries_differ_different_runnable_is_different() {
    let e1 = hfile("/f.txt", "abc123", 10, 1000);
    let mut e2 = hfile("/f.txt", "abc123", 10, 1000);
    e2.runnable = true;
    assert!(entries_differ(&e1, &e2, false, false));
}

#[test]
fn entries_differ_preserve_runnable_skips_comparison() {
    let e1 = hfile("/f.txt", "abc123", 10, 1000);
    let mut e2 = hfile("/f.txt", "abc123", 10, 1000);
    e2.runnable = true;
    // preserve_runnable=true means runnable differences are ignored
    assert!(!entries_differ(&e1, &e2, false, true));
}

#[test]
fn entries_differ_same_symlink_target_not_different() {
    let e1 = FileEntry::symlink("/link.txt", "/target.txt");
    let e2 = FileEntry::symlink("/link.txt", "/target.txt");
    assert!(!entries_differ(&e1, &e2, false, false));
}

#[test]
fn entries_differ_different_symlink_target_is_different() {
    let e1 = FileEntry::symlink("/link.txt", "/target1.txt");
    let e2 = FileEntry::symlink("/link.txt", "/target2.txt");
    assert!(entries_differ(&e1, &e2, false, false));
}

#[test]
fn entries_differ_regular_to_symlink_is_different() {
    let e1 = hfile("/file.txt", "abc123", 10, 1000);
    let e2 = FileEntry::symlink("/file.txt", "/other.txt");
    assert!(entries_differ(&e1, &e2, false, false));
}

#[test]
fn entries_differ_symlink_to_regular_is_different() {
    let e1 = FileEntry::symlink("/file.txt", "/other.txt");
    let e2 = hfile("/file.txt", "abc123", 10, 1000);
    assert!(entries_differ(&e1, &e2, false, false));
}

#[test]
fn entries_differ_ignore_hashes_skips_hash() {
    let e1 = hfile("/f.txt", "abc123", 10, 1000);
    let e2 = hfile("/f.txt", "def456", 10, 1000);
    assert!(!entries_differ(&e1, &e2, true, false));
}

#[test]
fn entries_differ_ignore_hashes_skips_chunkhashes() {
    let mut e1 = FileEntry::file("/large.bin", 512 * 1024 * 1024, 1000);
    e1.chunk_hashes = Some(vec!["h1".into(), "h2".into()]);
    let mut e2 = FileEntry::file("/large.bin", 512 * 1024 * 1024, 1000);
    e2.chunk_hashes = Some(vec!["h1".into(), "h3".into()]);
    assert!(!entries_differ(&e1, &e2, true, false));
}

// ===== TestIgnoreHashesMode =====

#[test]
fn ignore_hashes_same_metadata_not_modified() {
    let parent = abs(vec![hfile("/file.txt", "hash1", 100, 1000)], vec![]);
    let current = abs(vec![hfile("/file.txt", "hash2", 100, 1000)], vec![]);
    let o = DiffOptions {
        ignore_hashes: true,
        ..opts()
    };
    let diff = diff_snapshots(&parent, &current, &o).unwrap();
    assert_eq!(diff.files.len(), 0);
}

#[test]
fn ignore_hashes_different_mtime_is_modified() {
    let parent = abs(vec![hfile("/file.txt", "hash1", 100, 1000)], vec![]);
    let current = abs(vec![hfile("/file.txt", "hash1", 100, 2000)], vec![]);
    let o = DiffOptions {
        ignore_hashes: true,
        ..opts()
    };
    let diff = diff_snapshots(&parent, &current, &o).unwrap();
    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].path, "/file.txt");
}

#[test]
fn ignore_hashes_different_size_is_modified() {
    let parent = abs(vec![hfile("/file.txt", "hash1", 100, 1000)], vec![]);
    let current = abs(vec![hfile("/file.txt", "hash1", 200, 1000)], vec![]);
    let o = DiffOptions {
        ignore_hashes: true,
        ..opts()
    };
    let diff = diff_snapshots(&parent, &current, &o).unwrap();
    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].path, "/file.txt");
}

// ===== TestPreserveRunnableMode =====

#[test]
fn preserve_runnable_copies_from_parent() {
    let mut pf = hfile("/script.sh", "hash1", 100, 1000);
    pf.runnable = true;
    let parent = abs(vec![pf], vec![]);
    let current = abs(vec![hfile("/script.sh", "hash2", 100, 2000)], vec![]);
    let o = DiffOptions {
        preserve_runnable: true,
        ..opts()
    };
    let diff = diff_snapshots(&parent, &current, &o).unwrap();
    assert_eq!(diff.files.len(), 1);
    assert!(diff.files[0].runnable);
}

#[test]
fn preserve_runnable_does_not_affect_new_files() {
    let parent = abs(vec![], vec![]);
    let mut f = hfile("/script.sh", "hash1", 100, 1000);
    f.runnable = true;
    let current = abs(vec![f], vec![]);
    let o = DiffOptions {
        preserve_runnable: true,
        ..opts()
    };
    let diff = diff_snapshots(&parent, &current, &o).unwrap();
    assert_eq!(diff.files.len(), 1);
    assert!(diff.files[0].runnable);
}

// ===== TestDirectoryDeletionSemantics =====

#[test]
fn deleted_directory_includes_contained_files() {
    let parent = abs(
        vec![
            hfile("/keep.txt", "h1", 100, 1000),
            hfile("/deleted_dir/file1.txt", "h2", 50, 2000),
            hfile("/deleted_dir/file2.txt", "h3", 75, 3000),
        ],
        vec![DirEntry::new("/deleted_dir")],
    );
    let current = abs(vec![hfile("/keep.txt", "h1", 100, 1000)], vec![]);
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    let deleted_files: Vec<&str> = diff
        .files
        .iter()
        .filter(|f| f.deleted)
        .map(|f| f.path.as_str())
        .collect();
    assert!(deleted_files.contains(&"/deleted_dir/file1.txt"));
    assert!(deleted_files.contains(&"/deleted_dir/file2.txt"));
    let deleted_dirs: Vec<&str> = diff
        .dirs
        .iter()
        .filter(|d| d.deleted)
        .map(|d| d.path.as_str())
        .collect();
    assert!(deleted_dirs.contains(&"/deleted_dir"));
}

#[test]
fn deleted_directory_includes_subdirectories() {
    let parent = abs(
        vec![],
        vec![
            DirEntry::new("/deleted_dir"),
            DirEntry::new("/deleted_dir/subdir"),
        ],
    );
    let current = abs(vec![], vec![]);
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    let deleted_dirs: Vec<&str> = diff
        .dirs
        .iter()
        .filter(|d| d.deleted)
        .map(|d| d.path.as_str())
        .collect();
    assert!(deleted_dirs.contains(&"/deleted_dir"));
    assert!(deleted_dirs.contains(&"/deleted_dir/subdir"));
}

// ===== TestHashStateValidation =====

#[test]
fn hash_state_both_hashed_succeeds() {
    let parent = abs(vec![hfile("/file.txt", "hash1", 100, 1000)], vec![]);
    let current = abs(vec![hfile("/file.txt", "hash2", 100, 2000)], vec![]);
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    assert_eq!(diff.files.len(), 1);
}

#[test]
fn hash_state_both_unhashed_succeeds() {
    let parent = abs(vec![FileEntry::file("/file.txt", 100, 1000)], vec![]);
    let current = abs(vec![FileEntry::file("/file.txt", 100, 2000)], vec![]);
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    assert_eq!(diff.files.len(), 1);
}

#[test]
fn hash_state_hashed_vs_unhashed_errors() {
    let parent = abs(vec![hfile("/file.txt", "hash1", 100, 1000)], vec![]);
    let current = abs(vec![FileEntry::file("/file.txt", 100, 2000)], vec![]);
    let result = diff_snapshots(&parent, &current, &opts());
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("hashed"));
    assert!(msg.contains("unhashed"));
}

#[test]
fn hash_state_unhashed_vs_hashed_errors() {
    let parent = abs(vec![FileEntry::file("/file.txt", 100, 1000)], vec![]);
    let current = abs(vec![hfile("/file.txt", "hash1", 100, 2000)], vec![]);
    let result = diff_snapshots(&parent, &current, &opts());
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("hashed"));
    assert!(msg.contains("unhashed"));
}

#[test]
fn hash_state_ignore_hashes_bypasses_validation() {
    let parent = abs(vec![hfile("/file.txt", "hash1", 100, 1000)], vec![]);
    let current = abs(vec![FileEntry::file("/file.txt", 100, 2000)], vec![]);
    let o = DiffOptions {
        ignore_hashes: true,
        ..opts()
    };
    let diff = diff_snapshots(&parent, &current, &o).unwrap();
    assert_eq!(diff.files.len(), 1);
}

#[test]
fn hash_state_symlinks_ignored_in_check() {
    let parent = abs(
        vec![
            hfile("/file.txt", "hash1", 100, 1000),
            FileEntry::symlink("/link", "/target"),
        ],
        vec![],
    );
    let current = abs(
        vec![
            hfile("/file.txt", "hash2", 100, 2000),
            FileEntry::symlink("/link", "/target"),
        ],
        vec![],
    );
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    assert_eq!(diff.files.len(), 1);
}

#[test]
fn hash_state_empty_manifests_succeed() {
    let parent = abs(vec![], vec![]);
    let current = abs(vec![], vec![]);
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    assert_eq!(diff.files.len(), 0);
}

#[test]
fn hash_state_empty_vs_hashed_succeeds() {
    let empty = abs(vec![], vec![]);
    let hashed = abs(vec![hfile("/file.txt", "hash1", 100, 1000)], vec![]);
    let diff = diff_snapshots(&empty, &hashed, &opts()).unwrap();
    assert_eq!(diff.files.len(), 1);
    let diff = diff_snapshots(&hashed, &empty, &opts()).unwrap();
    assert_eq!(diff.files.len(), 1);
}

#[test]
fn hash_state_empty_vs_unhashed_succeeds() {
    let empty = abs(vec![], vec![]);
    let unhashed = abs(vec![FileEntry::file("/file.txt", 100, 1000)], vec![]);
    let diff = diff_snapshots(&empty, &unhashed, &opts()).unwrap();
    assert_eq!(diff.files.len(), 1);
    let diff = diff_snapshots(&unhashed, &empty, &opts()).unwrap();
    assert_eq!(diff.files.len(), 1);
}

#[test]
fn hash_state_symlink_only_manifests_succeed() {
    let parent = abs(vec![FileEntry::symlink("/link1", "/target1")], vec![]);
    let current = abs(vec![FileEntry::symlink("/link2", "/target2")], vec![]);
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    assert_eq!(diff.files.len(), 2);
}

#[test]
fn hash_state_symlink_only_vs_hashed_succeeds() {
    let symlink_only = abs(vec![FileEntry::symlink("/link", "/target")], vec![]);
    let hashed = abs(vec![hfile("/file.txt", "hash1", 100, 1000)], vec![]);
    let diff = diff_snapshots(&symlink_only, &hashed, &opts()).unwrap();
    assert_eq!(diff.files.len(), 2);
}

#[test]
fn hash_state_symlink_only_vs_unhashed_succeeds() {
    let symlink_only = abs(vec![FileEntry::symlink("/link", "/target")], vec![]);
    let unhashed = abs(vec![FileEntry::file("/file.txt", 100, 1000)], vec![]);
    let diff = diff_snapshots(&symlink_only, &unhashed, &opts()).unwrap();
    assert_eq!(diff.files.len(), 2);
}

// ===== TestComputeDiffWithFilter =====

#[test]
fn filter_both_for_correct_deletions() {
    let parent = abs(
        vec![
            hfile("/model.blend", "h1", 100, 1000),
            hfile("/texture.png", "h2", 200, 2000),
        ],
        vec![],
    );
    let current = abs(
        vec![
            hfile("/model.blend", "h1", 100, 1000),
            hfile("/new.blend", "h3", 150, 3000),
        ],
        vec![],
    );
    let filter_fn = |e: &openjd_snapshots::ManifestEntry| e.path().ends_with(".blend");
    let filtered_parent = filter_manifest(&parent, &filter_fn);
    let filtered_current = filter_manifest(&current, &filter_fn);
    let diff = diff_snapshots(&filtered_parent, &filtered_current, &opts()).unwrap();
    let paths: Vec<&str> = diff.files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(&"/new.blend"));
    assert!(!paths.contains(&"/texture.png"));
    assert!(diff.files.iter().all(|f| !f.deleted));
}

// ===== TestComputeDiffManifestRelSnapshot (additional) =====

#[test]
fn rel_returns_rel_diff_type() {
    // Rust type system enforces this at compile time, but verify the diff works
    let parent = rel(vec![hfile("a.txt", "h1", 10, 1000)], vec![]);
    let current = rel(vec![hfile("a.txt", "h1", 10, 1000)], vec![]);
    let diff = diff_snapshots(&parent, &current, &opts()).unwrap();
    assert_eq!(diff.files.len(), 0);
    assert_eq!(diff.dirs.len(), 0);
}

// ===== entries_differ additional tests =====

#[test]
fn entries_differ_same_chunkhashes_not_different() {
    let mut e1 = FileEntry::file("/large.bin", 512 * 1024 * 1024, 1000);
    e1.chunk_hashes = Some(vec!["h1".into(), "h2".into()]);
    let mut e2 = FileEntry::file("/large.bin", 512 * 1024 * 1024, 1000);
    e2.chunk_hashes = Some(vec!["h1".into(), "h2".into()]);
    assert!(!entries_differ(&e1, &e2, false, false));
}

#[test]
fn entries_differ_different_chunkhashes_is_different() {
    let mut e1 = FileEntry::file("/large.bin", 512 * 1024 * 1024, 1000);
    e1.chunk_hashes = Some(vec!["h1".into(), "h2".into()]);
    let mut e2 = FileEntry::file("/large.bin", 512 * 1024 * 1024, 1000);
    e2.chunk_hashes = Some(vec!["h1".into(), "h3".into()]);
    assert!(entries_differ(&e1, &e2, false, false));
}

#[test]
fn entries_differ_regular_to_chunked_is_different() {
    let e1 = hfile("/file.bin", "abc123", 100, 1000);
    let mut e2 = FileEntry::file("/file.bin", 512 * 1024 * 1024, 2000);
    e2.chunk_hashes = Some(vec!["h1".into(), "h2".into()]);
    assert!(entries_differ(&e1, &e2, false, false));
}

#[test]
fn entries_differ_chunked_to_regular_is_different() {
    let mut e1 = FileEntry::file("/file.bin", 512 * 1024 * 1024, 1000);
    e1.chunk_hashes = Some(vec!["h1".into(), "h2".into()]);
    let e2 = hfile("/file.bin", "abc123", 100, 2000);
    assert!(entries_differ(&e1, &e2, false, false));
}
