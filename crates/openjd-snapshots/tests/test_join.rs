// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

/// Rust port of all tests from
/// deadline-cloud/test/unit/deadline_job_attachments/snapshots/operations/test_join_manifest.py
use openjd_snapshots::{
    join_snapshot, join_snapshot_diff, join_snapshot_diff_rel, join_snapshot_rel, DirEntry,
    FileEntry, HashAlgorithm, Manifest, Snapshot, SnapshotDiff, DEFAULT_FILE_CHUNK_SIZE,
};

fn snap(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> Snapshot {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(files)
        .with_dirs(dirs)
}

fn snap_diff(files: Vec<FileEntry>, dirs: Vec<DirEntry>) -> SnapshotDiff {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(files)
        .with_dirs(dirs)
}

fn hfile(path: &str, hash: &str, size: u64, mtime: u64) -> FileEntry {
    let mut e = FileEntry::file(path, size, mtime);
    e.hash = Some(hash.into());
    e
}

// Helper-function tests for normalize_path / is_absolute_path live in the
// crate-internal unit tests (src/path_util.rs).

// ===== TestJoinManifestRelSnapshot =====

#[test]
fn rel_basic_join_relative_prefix() {
    let m = snap(
        vec![
            hfile("wood.png", "h1", 100, 1000),
            hfile("metal.png", "h2", 200, 2000),
        ],
        vec![DirEntry::new("sub")],
    );
    let result = join_snapshot_rel(&m, "assets/textures").unwrap();
    let file_paths: Vec<&str> = result.files.iter().map(|f| f.path.as_str()).collect();
    assert!(file_paths.contains(&"assets/textures/wood.png"));
    assert!(file_paths.contains(&"assets/textures/metal.png"));
    assert_eq!(result.dirs[0].path, "assets/textures/sub");
}

#[test]
fn rel_symlink_targets_prefixed() {
    let m = snap(
        vec![
            hfile("wood.png", "h1", 100, 1000),
            FileEntry::symlink("current", "wood.png"),
        ],
        vec![],
    );
    let result = join_snapshot_rel(&m, "assets/textures").unwrap();
    let by_path: std::collections::HashMap<&str, &FileEntry> =
        result.files.iter().map(|f| (f.path.as_str(), f)).collect();
    assert!(by_path.contains_key("assets/textures/wood.png"));
    assert!(by_path.contains_key("assets/textures/current"));
    assert_eq!(
        by_path["assets/textures/current"].symlink_target.as_deref(),
        Some("assets/textures/wood.png")
    );
}

#[test]
fn rel_preserves_file_metadata() {
    let m = snap(vec![hfile("wood.png", "hash1", 100, 1000)], vec![]);
    let result = join_snapshot_rel(&m, "prefix").unwrap();
    assert_eq!(result.files.len(), 1);
    let e = &result.files[0];
    assert_eq!(e.path, "prefix/wood.png");
    assert_eq!(e.hash.as_deref(), Some("hash1"));
    assert_eq!(e.size, Some(100));
    assert_eq!(e.mtime, Some(1000));
}

#[test]
fn rel_preserves_total_size() {
    let m = snap(
        vec![
            hfile("a.txt", "h1", 100, 1000),
            hfile("b.txt", "h2", 200, 2000),
        ],
        vec![],
    );
    let result = join_snapshot_rel(&m, "prefix").unwrap();
    assert_eq!(result.total_size, 300);
}

#[test]
fn rel_preserves_runnable_flag() {
    let mut f = hfile("script.sh", "h1", 100, 1000);
    f.runnable = true;
    let m = snap(vec![f], vec![]);
    let result = join_snapshot_rel(&m, "prefix").unwrap();
    assert!(result.files[0].runnable);
}

#[test]
fn rel_preserves_chunkhashes() {
    let mut f = FileEntry::file("large.bin", 512 * 1024 * 1024, 1000);
    f.chunk_hashes = Some(vec!["c1".into(), "c2".into()]);
    let m = snap(vec![f], vec![]);
    let result = join_snapshot_rel(&m, "prefix").unwrap();
    assert_eq!(
        result.files[0].chunk_hashes.as_deref(),
        Some(&["c1".to_string(), "c2".to_string()][..])
    );
}

// ===== TestJoinManifestAbsSnapshot =====

#[test]
fn abs_basic_join_absolute_prefix() {
    let m = snap(
        vec![hfile("old/wood.png", "h1", 100, 1000)],
        vec![DirEntry::new("old/sub")],
    );
    let result = join_snapshot(&m, "/projects/scene").unwrap();
    assert_eq!(result.files[0].path, "/projects/scene/old/wood.png");
    assert_eq!(result.dirs[0].path, "/projects/scene/old/sub");
}

#[test]
fn abs_symlink_targets_prefixed() {
    let m = snap(
        vec![
            hfile("data/wood.png", "h1", 100, 1000),
            FileEntry::symlink("data/current", "data/wood.png"),
        ],
        vec![],
    );
    let result = join_snapshot(&m, "/projects/scene").unwrap();
    let by_path: std::collections::HashMap<&str, &FileEntry> =
        result.files.iter().map(|f| (f.path.as_str(), f)).collect();
    assert_eq!(
        by_path["/projects/scene/data/current"]
            .symlink_target
            .as_deref(),
        Some("/projects/scene/data/wood.png")
    );
}

// ===== TestJoinManifestDiff =====

#[test]
fn diff_preserves_deleted_markers() {
    let m = snap_diff(
        vec![FileEntry::deleted("old.txt")],
        vec![DirEntry {
            path: "old_dir".into(),
            deleted: true,
        }],
    );
    let result = join_snapshot_diff_rel(&m, "prefix").unwrap();
    assert_eq!(result.files[0].path, "prefix/old.txt");
    assert!(result.files[0].deleted);
    assert_eq!(result.dirs[0].path, "prefix/old_dir");
    assert!(result.dirs[0].deleted);
}

#[test]
fn diff_does_not_preserve_parent_manifest_hash() {
    let mut m = snap_diff(vec![hfile("file.txt", "h1", 100, 1000)], vec![]);
    m.parent_manifest_hash = Some("parent_hash_123".into());
    let result = join_snapshot_diff_rel(&m, "prefix").unwrap();
    assert!(result.parent_manifest_hash.is_none());
}

#[test]
fn diff_preserves_file_chunk_size_bytes() {
    let m: SnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, 128 * 1024 * 1024)
        .with_files(vec![hfile("file.txt", "h1", 100, 1000)]);
    let result = join_snapshot_diff_rel(&m, "prefix").unwrap();
    assert_eq!(result.file_chunk_size_bytes, 128 * 1024 * 1024);
}

#[test]
fn diff_returns_rel_with_rel_prefix() {
    let m = snap_diff(vec![hfile("a.txt", "h1", 10, 1000)], vec![]);
    let result = join_snapshot_diff_rel(&m, "prefix").unwrap();
    // Verify it's a SnapshotDiff (Rel, Diff) by checking paths are relative
    assert!(!result.files[0].path.starts_with('/'));
}

#[test]
fn diff_returns_abs_with_abs_prefix() {
    let m = snap_diff(vec![hfile("a.txt", "h1", 10, 1000)], vec![]);
    let result = join_snapshot_diff(&m, "/prefix").unwrap();
    assert!(result.files[0].path.starts_with('/'));
}

// ===== TestJoinManifestValidation =====

#[test]
fn empty_prefix_raises_error() {
    let m = snap(vec![hfile("file.txt", "h1", 100, 1000)], vec![]);
    let result = join_snapshot(&m, "");
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("empty"));
}

#[test]
fn empty_prefix_raises_error_rel() {
    let m = snap(vec![hfile("file.txt", "h1", 100, 1000)], vec![]);
    let result = join_snapshot_rel(&m, "");
    assert!(result.is_err());
}

#[test]
fn empty_prefix_raises_error_diff() {
    let m = snap_diff(vec![hfile("file.txt", "h1", 100, 1000)], vec![]);
    let result = join_snapshot_diff(&m, "");
    assert!(result.is_err());
}

#[test]
fn empty_prefix_raises_error_diff_rel() {
    let m = snap_diff(vec![hfile("file.txt", "h1", 100, 1000)], vec![]);
    let result = join_snapshot_diff_rel(&m, "");
    assert!(result.is_err());
}

// ===== TestJoinManifestWindowsPaths =====

#[test]
fn windows_absolute_prefix() {
    let m = snap(vec![hfile("wood.png", "h1", 100, 1000)], vec![]);
    let result = join_snapshot(&m, "C:/projects/scene").unwrap();
    assert_eq!(result.files[0].path, "C:/projects/scene/wood.png");
}

#[cfg(windows)]
#[test]
fn windows_backslash_prefix_normalized() {
    let m = snap(vec![hfile("wood.png", "h1", 100, 1000)], vec![]);
    let result = join_snapshot(&m, "C:\\projects\\scene").unwrap();
    assert_eq!(result.files[0].path, "C:/projects/scene/wood.png");
}

// ===== TestGetOutputManifestType (type-level in Rust) =====
// In Python, _get_output_manifest_type determines the output type at runtime.
// In Rust, the type is determined at compile time by which function you call.
// These tests verify the four combinations work correctly.

#[test]
fn rel_snapshot_rel_prefix_returns_snapshot() {
    let m = snap(vec![hfile("a.txt", "h1", 10, 1000)], vec![]);
    let result = join_snapshot_rel(&m, "prefix").unwrap();
    assert!(!result.files[0].path.starts_with('/'));
}

#[test]
fn rel_snapshot_abs_prefix_returns_abs_snapshot() {
    let m = snap(vec![hfile("a.txt", "h1", 10, 1000)], vec![]);
    let result = join_snapshot(&m, "/prefix").unwrap();
    assert!(result.files[0].path.starts_with('/'));
}

#[test]
fn rel_diff_rel_prefix_returns_snapshot_diff() {
    let m = snap_diff(vec![hfile("a.txt", "h1", 10, 1000)], vec![]);
    let result = join_snapshot_diff_rel(&m, "prefix").unwrap();
    assert!(!result.files[0].path.starts_with('/'));
}

#[test]
fn rel_diff_abs_prefix_returns_abs_snapshot_diff() {
    let m = snap_diff(vec![hfile("a.txt", "h1", 10, 1000)], vec![]);
    let result = join_snapshot_diff(&m, "/prefix").unwrap();
    assert!(result.files[0].path.starts_with('/'));
}

// ===== Additional edge-case tests =====

#[test]
fn empty_manifest_join() {
    let m = snap(vec![], vec![]);
    let result = join_snapshot(&m, "/root").unwrap();
    assert!(result.files.is_empty());
    assert!(result.dirs.is_empty());
    assert_eq!(result.total_size, 0);
}

#[test]
fn empty_manifest_join_rel() {
    let m = snap(vec![], vec![]);
    let result = join_snapshot_rel(&m, "prefix").unwrap();
    assert!(result.files.is_empty());
    assert!(result.dirs.is_empty());
}

#[test]
fn empty_diff_manifest_join() {
    let m = snap_diff(vec![], vec![]);
    let result = join_snapshot_diff(&m, "/root").unwrap();
    assert!(result.files.is_empty());
    assert!(result.dirs.is_empty());
}

#[test]
fn deeply_nested_paths() {
    let m = snap(
        vec![hfile("a/b/c/d/e/f/g/h/deep.txt", "h1", 10, 1)],
        vec![DirEntry::new("a/b/c/d/e/f/g/h")],
    );
    let result = join_snapshot(&m, "/root").unwrap();
    assert_eq!(result.files[0].path, "/root/a/b/c/d/e/f/g/h/deep.txt");
    assert_eq!(result.dirs[0].path, "/root/a/b/c/d/e/f/g/h");
}

#[test]
fn paths_with_spaces_and_special_chars() {
    let m = snap(
        vec![
            hfile("my project/file (1).txt", "h1", 10, 1),
            hfile("data/name with spaces.bin", "h2", 20, 2),
            hfile("special!@#$%/file.txt", "h3", 30, 3),
        ],
        vec![DirEntry::new("my project")],
    );
    let result = join_snapshot(&m, "/root").unwrap();
    let paths: Vec<&str> = result.files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(&"/root/my project/file (1).txt"));
    assert!(paths.contains(&"/root/data/name with spaces.bin"));
    assert!(paths.contains(&"/root/special!@#$%/file.txt"));
    assert_eq!(result.dirs[0].path, "/root/my project");
}

#[test]
fn unicode_paths() {
    let m = snap(
        vec![
            hfile("日本語/ファイル.txt", "h1", 10, 1),
            hfile("données/café.blend", "h2", 20, 2),
        ],
        vec![],
    );
    let result = join_snapshot(&m, "/projects").unwrap();
    assert_eq!(result.files[0].path, "/projects/日本語/ファイル.txt");
    assert_eq!(result.files[1].path, "/projects/données/café.blend");
}

#[test]
fn symlink_chain_targets_all_prefixed() {
    let m = snap(
        vec![
            hfile("real.txt", "h1", 10, 1),
            FileEntry::symlink("link1", "real.txt"),
            FileEntry::symlink("link2", "link1"),
        ],
        vec![],
    );
    let result = join_snapshot(&m, "/root").unwrap();
    let by_path: std::collections::HashMap<&str, &FileEntry> =
        result.files.iter().map(|f| (f.path.as_str(), f)).collect();
    assert_eq!(
        by_path["/root/link1"].symlink_target.as_deref(),
        Some("/root/real.txt")
    );
    assert_eq!(
        by_path["/root/link2"].symlink_target.as_deref(),
        Some("/root/link1")
    );
}

#[test]
fn dir_symlink_target_prefixed() {
    let m = snap(
        vec![FileEntry::symlink("link_dir", "actual_dir")],
        vec![DirEntry::new("actual_dir")],
    );
    let result = join_snapshot(&m, "/root").unwrap();
    assert_eq!(
        result.files[0].symlink_target.as_deref(),
        Some("/root/actual_dir")
    );
    assert_eq!(result.dirs[0].path, "/root/actual_dir");
}

#[test]
fn prefix_with_dotdot_normalized() {
    let m = snap(vec![hfile("a.txt", "h1", 10, 1)], vec![]);
    let result = join_snapshot(&m, "/root/sub/../other").unwrap();
    assert_eq!(result.files[0].path, "/root/other/a.txt");
}

#[test]
fn join_manifest_wrapper_snapshot() {
    use openjd_snapshots::{join_manifest, AbsManifest, RelManifest};
    let m = RelManifest::Snapshot(snap(vec![hfile("a.txt", "h1", 10, 1)], vec![]));
    let result = join_manifest(&m, "/root").unwrap();
    match result {
        AbsManifest::Snapshot(s) => assert_eq!(s.files[0].path, "/root/a.txt"),
        AbsManifest::Diff(_) => panic!("expected Snapshot"),
    }
}

#[test]
fn join_manifest_wrapper_diff() {
    use openjd_snapshots::{join_manifest, AbsManifest, RelManifest};
    let m = RelManifest::Diff(snap_diff(
        vec![hfile("a.txt", "h1", 10, 1), FileEntry::deleted("b.txt")],
        vec![],
    ));
    let result = join_manifest(&m, "/root").unwrap();
    match result {
        AbsManifest::Diff(d) => {
            assert_eq!(d.files[0].path, "/root/a.txt");
            assert_eq!(d.files[1].path, "/root/b.txt");
            assert!(d.files[1].deleted);
        }
        AbsManifest::Snapshot(_) => panic!("expected Diff"),
    }
}

#[test]
fn join_manifest_rel_wrapper() {
    use openjd_snapshots::{join_manifest_rel, RelManifest};
    let m = RelManifest::Snapshot(snap(vec![hfile("a.txt", "h1", 10, 1)], vec![]));
    let result = join_manifest_rel(&m, "sub/dir").unwrap();
    match result {
        RelManifest::Snapshot(s) => assert_eq!(s.files[0].path, "sub/dir/a.txt"),
        RelManifest::Diff(_) => panic!("expected Snapshot"),
    }
}

#[test]
fn single_component_file_at_root() {
    let m = snap(vec![hfile("readme.md", "h1", 50, 1)], vec![]);
    let result = join_snapshot(&m, "/").unwrap();
    assert_eq!(result.files[0].path, "/readme.md");
}
