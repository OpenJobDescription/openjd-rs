use openjd_snapshots::path_util::{is_absolute_path, normalize_path};
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

// ===== TestHelperFunctions =====

#[test]
fn normalize_path_removes_trailing_slash() {
    assert_eq!(normalize_path("assets/textures/"), "assets/textures");
    assert_eq!(normalize_path("/projects/scene/"), "/projects/scene");
}

#[test]
fn normalize_path_preserves_leading_slash() {
    assert_eq!(normalize_path("/projects/scene"), "/projects/scene");
}

#[test]
fn join_path_relative() {
    assert_eq!(
        normalize_path("assets/textures/wood.png"),
        "assets/textures/wood.png"
    );
    assert_eq!(normalize_path("a/b/c/d.txt"), "a/b/c/d.txt");
}

#[test]
fn join_path_absolute_posix() {
    assert_eq!(
        normalize_path("/projects/scene/wood.png"),
        "/projects/scene/wood.png"
    );
    assert_eq!(normalize_path("/a/b/c/d.txt"), "/a/b/c/d.txt");
}

#[test]
fn join_path_absolute_windows() {
    assert_eq!(
        normalize_path("C:/projects/scene/wood.png"),
        "C:/projects/scene/wood.png"
    );
    assert_eq!(normalize_path("C:/a/b/c/d.txt"), "C:/a/b/c/d.txt");
}

#[test]
fn is_absolute_path_posix() {
    assert!(is_absolute_path("/home/user/file.txt"));
    assert!(is_absolute_path("/"));
}

#[test]
fn is_absolute_path_windows_drive() {
    assert!(is_absolute_path("C:/Users/file.txt"));
    assert!(is_absolute_path("D:/Projects/file.txt"));
}

#[test]
fn is_absolute_path_relative() {
    assert!(!is_absolute_path("assets/file.txt"));
    assert!(!is_absolute_path("file.txt"));
    assert!(!is_absolute_path("./file.txt"));
    assert!(!is_absolute_path("../file.txt"));
}

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

// ===== TestPathSeparatorHandling =====

#[cfg(windows)]
#[test]
fn normalize_path_converts_backslashes() {
    assert_eq!(normalize_path("C:\\projects\\scene"), "C:/projects/scene");
}

#[cfg(not(windows))]
#[test]
fn normalize_path_preserves_backslashes_on_posix() {
    // On POSIX, backslashes are preserved as filename characters.
    // "C:\projects\scene" has drive prefix "C:", so byte[2] ('\') is skipped as separator,
    // and "projects\scene" becomes a single component (no '/' to split on).
    assert_eq!(normalize_path("C:\\projects\\scene"), "C:/projects\\scene");
}

#[test]
fn normalize_path_preserves_forward_slashes() {
    assert_eq!(normalize_path("dir/name"), "dir/name");
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

#[test]
fn is_absolute_path_windows_unc() {
    assert!(is_absolute_path("//server/share"));
    assert!(is_absolute_path("\\\\server\\share"));
}
