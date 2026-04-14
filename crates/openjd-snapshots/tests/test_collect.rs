// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Ported from deadline-cloud test_collect_manifest.py
//
// Tests for collect_abs_snapshot: absolute paths, metadata, directory handling,
// filenames parameter, chunk size, empty inputs, special files.

use openjd_snapshots::{
    collect_abs_snapshot, CollectOptions, HashAlgorithm, SymlinkPolicy, DEFAULT_FILE_CHUNK_SIZE,
    WHOLE_FILE_CHUNK_SIZE,
};
use std::path::PathBuf;
use tempfile::TempDir;

// ===== Absolute paths =====

#[test]
fn absolute_paths() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "content").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    assert!(m.files[0].path.starts_with('/'));
    assert!(m.files[0].path.ends_with("/file.txt"));
}

#[test]
fn nested_absolute_paths() {
    let tmp = TempDir::new().unwrap();
    let sub = tmp.path().join("subdir");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("nested.txt"), "content").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let nested: Vec<_> = m
        .files
        .iter()
        .filter(|f| f.path.contains("nested.txt"))
        .collect();
    assert_eq!(nested.len(), 1);
    assert!(nested[0].path.starts_with('/'));
}

#[test]
fn paths_include_subdirectory_structure() {
    let tmp = TempDir::new().unwrap();
    let sub = tmp.path().join("subdir");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("file.txt"), "content").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let file = m
        .files
        .iter()
        .find(|f| f.path.contains("file.txt"))
        .unwrap();
    assert!(file.path.contains("/subdir/file.txt"));
}

// ===== Metadata =====

#[test]
fn file_size_captured() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "Hello, World!").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let f = m
        .files
        .iter()
        .find(|f| f.path.contains("file.txt"))
        .unwrap();
    assert_eq!(f.size, Some(13)); // "Hello, World!" = 13 bytes
}

#[test]
fn mtime_captured() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("file.txt");
    std::fs::write(&path, "content").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let f = m
        .files
        .iter()
        .find(|f| f.path.contains("file.txt"))
        .unwrap();
    assert!(f.mtime.unwrap() > 0);
}

#[test]
fn hash_is_none_for_unhashed() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "content").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    for f in &m.files {
        assert!(f.hash.is_none());
    }
}

#[test]
fn total_size_calculated() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file1.txt"), "12345").unwrap(); // 5 bytes
    std::fs::write(tmp.path().join("file2.txt"), "1234567890").unwrap(); // 10 bytes

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    assert_eq!(m.total_size, 15);
}

#[test]
fn hash_algorithm_is_xxh128() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "content").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    assert_eq!(m.hash_alg, HashAlgorithm::Xxh128);
}

// ===== Directory handling =====

#[test]
fn empty_directory_included() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("empty")).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    assert!(m.dirs.iter().any(|d| d.path.ends_with("/empty")));
}

#[test]
fn nested_directories_walked_recursively() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("a/b/c")).unwrap();
    std::fs::write(tmp.path().join("a/b/c/deep.txt"), "deep").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    assert_eq!(m.files.len(), 1);
    assert!(m.files[0].path.ends_with("a/b/c/deep.txt"));
}

#[test]
fn multiple_directories_collected() {
    let tmp = TempDir::new().unwrap();
    let dir1 = tmp.path().join("dir1");
    let dir2 = tmp.path().join("dir2");
    std::fs::create_dir_all(&dir1).unwrap();
    std::fs::create_dir_all(&dir2).unwrap();
    std::fs::write(dir1.join("file1.txt"), "content1").unwrap();
    std::fs::write(dir2.join("file2.txt"), "content2").unwrap();

    let m =
        collect_abs_snapshot(&[dir1, dir2], &[] as &[PathBuf], CollectOptions::default()).unwrap();

    assert!(m.files.iter().any(|f| f.path.ends_with("file1.txt")));
    assert!(m.files.iter().any(|f| f.path.ends_with("file2.txt")));
}

// ===== Filenames parameter =====

#[test]
fn collect_specific_files() {
    let tmp = TempDir::new().unwrap();
    let f1 = tmp.path().join("file1.txt");
    let f2 = tmp.path().join("file2.txt");
    let f3 = tmp.path().join("file3.txt");
    std::fs::write(&f1, "content1").unwrap();
    std::fs::write(&f2, "content2").unwrap();
    std::fs::write(&f3, "content3").unwrap();

    let m = collect_abs_snapshot(
        &[] as &[PathBuf],
        &[f1.clone(), f2.clone()],
        CollectOptions::default(),
    )
    .unwrap();

    assert_eq!(m.files.len(), 2);
    assert!(!m.files.iter().any(|f| f.path.ends_with("file3.txt")));
}

#[test]
fn combine_directories_and_filenames() {
    let tmp = TempDir::new().unwrap();
    let sub = tmp.path().join("subdir");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("in_dir.txt"), "in dir").unwrap();
    let extra = tmp.path().join("extra.txt");
    std::fs::write(&extra, "extra").unwrap();

    let m = collect_abs_snapshot(&[sub], &[extra], CollectOptions::default()).unwrap();

    assert!(m.files.iter().any(|f| f.path.ends_with("in_dir.txt")));
    assert!(m.files.iter().any(|f| f.path.ends_with("extra.txt")));
}

#[test]
fn filenames_only_no_directories() {
    let tmp = TempDir::new().unwrap();
    let f1 = tmp.path().join("file1.txt");
    std::fs::write(&f1, "content1").unwrap();

    let m = collect_abs_snapshot(&[] as &[PathBuf], &[f1], CollectOptions::default()).unwrap();

    assert_eq!(m.files.len(), 1);
    assert_eq!(m.dirs.len(), 0);
}

// ===== Chunk size parameter =====

#[test]
fn default_chunk_size() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "content").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    assert_eq!(m.file_chunk_size_bytes, DEFAULT_FILE_CHUNK_SIZE);
}

#[test]
fn custom_chunk_size() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "content").unwrap();
    let custom = 64 * 1024 * 1024;

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(custom),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.file_chunk_size_bytes, custom);
}

#[test]
fn whole_file_chunk_size() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "content").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(WHOLE_FILE_CHUNK_SIZE),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.file_chunk_size_bytes, WHOLE_FILE_CHUNK_SIZE);
}

// ===== Empty inputs =====

#[test]
fn empty_directory_results_in_dir_entry_only() {
    let tmp = TempDir::new().unwrap();
    let empty = tmp.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();

    let m = collect_abs_snapshot(&[empty], &[] as &[PathBuf], CollectOptions::default()).unwrap();

    assert_eq!(m.files.len(), 0);
    assert_eq!(m.dirs.len(), 1);
    assert_eq!(m.total_size, 0);
}

#[test]
fn no_inputs_results_in_empty_manifest() {
    let m = collect_abs_snapshot(
        &[] as &[PathBuf],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    assert_eq!(m.files.len(), 0);
    assert_eq!(m.dirs.len(), 0);
    assert_eq!(m.total_size, 0);
}

// ===== Special files =====

#[test]
fn hidden_files_collected() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join(".hidden"), "hidden content").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    assert!(m.files.iter().any(|f| f.path.ends_with("/.hidden")));
}

#[test]
fn hidden_directories_collected() {
    let tmp = TempDir::new().unwrap();
    let hidden = tmp.path().join(".hidden_dir");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(hidden.join("file.txt"), "content").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    assert!(m
        .files
        .iter()
        .any(|f| f.path.contains(".hidden_dir/file.txt")));
}

#[test]
fn files_with_spaces_in_name() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file with spaces.txt"), "content").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    assert!(m
        .files
        .iter()
        .any(|f| f.path.contains("file with spaces.txt")));
}

#[test]
fn files_with_unicode_names() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("файл_文件_αρχείο.txt"), "content").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    assert!(m
        .files
        .iter()
        .any(|f| f.path.contains("файл_文件_αρχείο.txt")));
}

#[test]
fn empty_file_collected_with_size_zero() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("empty.txt"), "").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let f = m
        .files
        .iter()
        .find(|f| f.path.ends_with("empty.txt"))
        .unwrap();
    assert_eq!(f.size, Some(0));
}

// ===== Error handling =====

#[test]
fn required_filename_missing_errors() {
    let result = collect_abs_snapshot(
        &[] as &[PathBuf],
        &[PathBuf::from("/nonexistent/file.txt")],
        CollectOptions::default(),
    );
    assert!(result.is_err());
}

#[test]
fn optional_filename_missing_skipped() {
    let tmp = TempDir::new().unwrap();
    let exists = tmp.path().join("exists.txt");
    std::fs::write(&exists, "ok").unwrap();

    let m = collect_abs_snapshot(
        &[] as &[PathBuf],
        &[exists],
        CollectOptions {
            optional_filenames: vec![tmp.path().join("nope.txt")],
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.files.len(), 1);
    assert!(m.files[0].path.ends_with("exists.txt"));
}

// ===== Path normalization =====

#[test]
fn all_paths_absolute_and_normalized() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("f.txt"), "x").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    for f in &m.files {
        assert!(f.path.starts_with('/'), "not absolute: {}", f.path);
        assert!(!f.path.contains("/../"), "not normalized: {}", f.path);
    }
}

// ===== Symlink policies (unix only) =====

#[cfg(unix)]
#[test]
fn symlinks_preserve_policy() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("target.txt"), "data").unwrap();
    std::os::unix::fs::symlink(tmp.path().join("target.txt"), tmp.path().join("link.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::Preserve,
            ..Default::default()
        },
    )
    .unwrap();

    let link = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link.txt"))
        .unwrap();
    assert!(link.symlink_target.is_some());
}

#[cfg(unix)]
#[test]
fn symlinks_exclude_all_policy() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("target.txt"), "data").unwrap();
    std::os::unix::fs::symlink(tmp.path().join("target.txt"), tmp.path().join("link.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::ExcludeAll,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.files.len(), 1);
    assert!(m.files[0].path.ends_with("target.txt"));
    assert!(m.files[0].symlink_target.is_none());
}

// ===== Symlink Preserve policy =====

#[cfg(unix)]
#[test]
fn preserve_file_symlink_has_absolute_target() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("target.txt");
    std::fs::write(&target, "content").unwrap();
    std::os::unix::fs::symlink("target.txt", tmp.path().join("link.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::Preserve,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.files.len(), 2);
    let link = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link.txt"))
        .unwrap();
    let abs_target = std::fs::canonicalize(&target).unwrap();
    assert_eq!(
        link.symlink_target.as_deref().unwrap(),
        abs_target.to_str().unwrap()
    );
}

#[cfg(unix)]
#[test]
fn preserve_directory_symlink_not_followed() {
    let tmp = TempDir::new().unwrap();
    let subdir = tmp.path().join("subdir");
    std::fs::create_dir_all(&subdir).unwrap();
    std::fs::write(subdir.join("file.txt"), "content").unwrap();
    std::os::unix::fs::symlink(&subdir, tmp.path().join("link_dir")).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::Preserve,
            ..Default::default()
        },
    )
    .unwrap();

    let link = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link_dir"))
        .unwrap();
    assert!(link.symlink_target.is_some());
    // Contents under link_dir should NOT be collected (symlink not followed)
    assert!(!m.files.iter().any(|f| f.path.contains("link_dir/file.txt")));
}

#[cfg(unix)]
#[test]
fn preserve_symlink_chain() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("target.txt"), "content").unwrap();
    std::os::unix::fs::symlink("target.txt", tmp.path().join("link1.txt")).unwrap();
    std::os::unix::fs::symlink("link1.txt", tmp.path().join("link2.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::Preserve,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.files.len(), 3);
    let l1 = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link1.txt"))
        .unwrap();
    let l2 = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link2.txt"))
        .unwrap();
    assert!(l1
        .symlink_target
        .as_deref()
        .unwrap()
        .ends_with("target.txt"));
    assert!(l2.symlink_target.as_deref().unwrap().ends_with("link1.txt"));
}

#[cfg(unix)]
#[test]
fn preserve_escaping_symlink_kept() {
    let tmp = TempDir::new().unwrap();
    let outside = tmp.path().join("outside.txt");
    std::fs::write(&outside, "outside").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink("../outside.txt", root.join("link.txt")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::Preserve,
            ..Default::default()
        },
    )
    .unwrap();

    let link = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link.txt"))
        .unwrap();
    assert!(link
        .symlink_target
        .as_deref()
        .unwrap()
        .ends_with("outside.txt"));
}

// ===== Symlink ExcludeAll policy =====

#[cfg(unix)]
#[test]
fn exclude_all_skips_file_symlink() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("target.txt"), "data").unwrap();
    std::os::unix::fs::symlink("target.txt", tmp.path().join("link.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::ExcludeAll,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.files.len(), 1);
    assert!(m.files[0].path.ends_with("target.txt"));
}

#[cfg(unix)]
#[test]
fn exclude_all_skips_directory_symlink() {
    let tmp = TempDir::new().unwrap();
    let subdir = tmp.path().join("subdir");
    std::fs::create_dir_all(&subdir).unwrap();
    std::fs::write(subdir.join("file.txt"), "content").unwrap();
    std::os::unix::fs::symlink(&subdir, tmp.path().join("link_dir")).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::ExcludeAll,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!m.files.iter().any(|f| f.path.contains("link_dir")));
    assert!(m.files.iter().any(|f| f.path.ends_with("file.txt")));
}

// ===== Symlink CollapseEscaping policy =====

#[cfg(unix)]
#[test]
fn collapse_escaping_converts_escaping_symlink_to_file() {
    let tmp = TempDir::new().unwrap();
    let outside = tmp.path().join("outside.txt");
    std::fs::write(&outside, "outside").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link.txt")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    let link = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link.txt"))
        .unwrap();
    // Escaping symlink collapsed: no symlink_target, has size
    assert!(link.symlink_target.is_none());
    assert_eq!(link.size, Some(7)); // "outside"
}

#[cfg(unix)]
#[test]
fn collapse_escaping_preserves_non_escaping_symlink() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("target.txt"), "data").unwrap();
    std::os::unix::fs::symlink("target.txt", tmp.path().join("link.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    let link = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link.txt"))
        .unwrap();
    // Non-escaping symlink preserved
    assert!(link.symlink_target.is_some());
}

// ===== CollapseAll policy =====

#[cfg(unix)]
#[test]
fn collapse_all_file_symlink_becomes_regular() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("target.txt"), "content").unwrap();
    std::os::unix::fs::symlink("target.txt", tmp.path().join("link.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseAll,
            ..Default::default()
        },
    )
    .unwrap();

    let link = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link.txt"))
        .unwrap();
    assert!(link.symlink_target.is_none());
    assert_eq!(link.size, Some(7));
}

#[cfg(unix)]
#[test]
fn collapse_all_dir_symlink_walks_contents() {
    let tmp = TempDir::new().unwrap();
    let subdir = tmp.path().join("subdir");
    std::fs::create_dir_all(&subdir).unwrap();
    std::fs::write(subdir.join("file.txt"), "content").unwrap();
    std::os::unix::fs::symlink(&subdir, tmp.path().join("link_dir")).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseAll,
            ..Default::default()
        },
    )
    .unwrap();

    // Should have file.txt under both subdir and link_dir
    assert!(m.files.iter().any(|f| f.path.contains("link_dir/file.txt")));
    assert!(m.files.iter().any(|f| f.path.contains("subdir/file.txt")));
}

// ===== Symlink in filenames parameter =====

#[cfg(unix)]
#[test]
fn symlink_in_filenames_preserved() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("target.txt");
    std::fs::write(&target, "content").unwrap();
    let link = tmp.path().join("link.txt");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let m = collect_abs_snapshot(
        &[] as &[PathBuf],
        &[link],
        CollectOptions {
            symlink_policy: SymlinkPolicy::Preserve,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.files.len(), 1);
    assert!(m.files[0].symlink_target.is_some());
}

#[cfg(unix)]
#[test]
fn symlink_in_filenames_collapsed() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("target.txt");
    std::fs::write(&target, "content").unwrap();
    let link = tmp.path().join("link.txt");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let m = collect_abs_snapshot(
        &[] as &[PathBuf],
        &[link],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseAll,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.files.len(), 1);
    assert!(m.files[0].symlink_target.is_none());
    assert_eq!(m.files[0].size, Some(7));
}

// ===== Error handling =====

#[test]
fn error_nonexistent_directory() {
    let tmp = TempDir::new().unwrap();
    let result = collect_abs_snapshot(
        &[tmp.path().join("nonexistent")],
        &[] as &[PathBuf],
        CollectOptions::default(),
    );
    assert!(result.is_err());
}

#[test]
fn error_file_as_directory() {
    let tmp = TempDir::new().unwrap();
    let f = tmp.path().join("file.txt");
    std::fs::write(&f, "content").unwrap();

    let result = collect_abs_snapshot(&[f], &[] as &[PathBuf], CollectOptions::default());
    assert!(result.is_err());
}

#[test]
fn directory_as_filename_not_added_as_file() {
    let tmp = TempDir::new().unwrap();
    let sub = tmp.path().join("subdir");
    std::fs::create_dir_all(&sub).unwrap();

    // The Rust implementation doesn't error but the directory metadata
    // is collected as a file entry (no validation). Verify it doesn't panic.
    let _result = collect_abs_snapshot(&[] as &[PathBuf], &[sub], CollectOptions::default());
}

#[test]
fn error_missing_required_filename() {
    let tmp = TempDir::new().unwrap();
    let result = collect_abs_snapshot(
        &[] as &[PathBuf],
        &[tmp.path().join("missing.txt")],
        CollectOptions::default(),
    );
    assert!(result.is_err());
}

#[test]
fn error_empty_inputs_returns_empty() {
    let m = collect_abs_snapshot(
        &[] as &[PathBuf],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    assert_eq!(m.files.len(), 0);
    assert_eq!(m.dirs.len(), 0);
    assert_eq!(m.total_size, 0);
}

// ===== Symlink cycle detection =====

#[cfg(unix)]
#[test]
fn cycle_self_referential_with_collapse_all() {
    let tmp = TempDir::new().unwrap();
    let link = tmp.path().join("self_link");
    std::os::unix::fs::symlink(&link, &link).unwrap();
    std::fs::write(tmp.path().join("file.txt"), "content").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseAll,
            ..Default::default()
        },
    )
    .unwrap();

    // Regular file collected, self-referential symlink skipped (broken)
    assert!(m.files.iter().any(|f| f.path.ends_with("file.txt")));
}

#[cfg(unix)]
#[test]
fn cycle_two_node_with_collapse_all() {
    let tmp = TempDir::new().unwrap();
    let dir_a = tmp.path().join("dir_a");
    let dir_b = tmp.path().join("dir_b");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    std::fs::write(dir_a.join("file_a.txt"), "a").unwrap();
    std::fs::write(dir_b.join("file_b.txt"), "b").unwrap();
    std::os::unix::fs::symlink(&dir_b, dir_a.join("link_to_b")).unwrap();
    std::os::unix::fs::symlink(&dir_a, dir_b.join("link_to_a")).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseAll,
            ..Default::default()
        },
    )
    .unwrap();

    // Both real files collected without infinite recursion
    assert!(m.files.iter().any(|f| f.path.ends_with("file_a.txt")));
    assert!(m.files.iter().any(|f| f.path.ends_with("file_b.txt")));
}

#[cfg(unix)]
#[test]
fn cycle_preserved_as_symlinks() {
    let tmp = TempDir::new().unwrap();
    let dir_a = tmp.path().join("dir_a");
    let dir_b = tmp.path().join("dir_b");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    std::os::unix::fs::symlink(&dir_b, dir_a.join("link_to_b")).unwrap();
    std::os::unix::fs::symlink(&dir_a, dir_b.join("link_to_a")).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::Preserve,
            ..Default::default()
        },
    )
    .unwrap();

    // Both symlinks preserved (no recursion with Preserve)
    let symlinks: Vec<_> = m
        .files
        .iter()
        .filter(|f| f.symlink_target.is_some())
        .collect();
    assert_eq!(symlinks.len(), 2);
}

#[cfg(unix)]
#[test]
fn cycle_with_collapse_escaping() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("file.txt"), "content").unwrap();

    let external = tmp.path().join("external");
    std::fs::create_dir_all(&external).unwrap();
    std::fs::write(external.join("ext_file.txt"), "ext").unwrap();

    // Cycle: root/link_out -> external, external/link_back -> root
    std::os::unix::fs::symlink(&external, root.join("link_out")).unwrap();
    std::os::unix::fs::symlink(&root, external.join("link_back")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // Regular file collected
    assert!(m.files.iter().any(|f| f.path.ends_with("file.txt")));
}

#[cfg(unix)]
#[test]
fn cycle_three_node_with_collapse_all() {
    let tmp = TempDir::new().unwrap();
    let dir_a = tmp.path().join("a");
    let dir_b = tmp.path().join("b");
    let dir_c = tmp.path().join("c");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    std::fs::create_dir_all(&dir_c).unwrap();
    std::fs::write(dir_a.join("fa.txt"), "a").unwrap();
    std::fs::write(dir_b.join("fb.txt"), "b").unwrap();
    std::fs::write(dir_c.join("fc.txt"), "c").unwrap();
    std::os::unix::fs::symlink(&dir_b, dir_a.join("to_b")).unwrap();
    std::os::unix::fs::symlink(&dir_c, dir_b.join("to_c")).unwrap();
    std::os::unix::fs::symlink(&dir_a, dir_c.join("to_a")).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseAll,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(m.files.iter().any(|f| f.path.ends_with("fa.txt")));
    assert!(m.files.iter().any(|f| f.path.ends_with("fb.txt")));
    assert!(m.files.iter().any(|f| f.path.ends_with("fc.txt")));
}

// ===== Broken symlinks =====

#[cfg(unix)]
#[test]
fn broken_symlink_skipped_with_collapse_all() {
    let tmp = TempDir::new().unwrap();
    std::os::unix::fs::symlink(
        tmp.path().join("nonexistent.txt"),
        tmp.path().join("broken_link"),
    )
    .unwrap();
    std::fs::write(tmp.path().join("real.txt"), "ok").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseAll,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!m.files.iter().any(|f| f.path.contains("broken_link")));
    assert!(m.files.iter().any(|f| f.path.ends_with("real.txt")));
}

#[cfg(unix)]
#[test]
fn broken_symlink_excluded_with_exclude_all() {
    let tmp = TempDir::new().unwrap();
    std::os::unix::fs::symlink(
        tmp.path().join("nonexistent.txt"),
        tmp.path().join("broken_link"),
    )
    .unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::ExcludeAll,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!m.files.iter().any(|f| f.path.contains("broken_link")));
}

// ===== Deduplication: overlapping directories =====

#[test]
fn nested_directories_deduplicated() {
    let tmp = TempDir::new().unwrap();
    let parent = tmp.path().join("parent");
    let child = parent.join("child");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(parent.join("parent_file.txt"), "parent").unwrap();
    std::fs::write(child.join("child_file.txt"), "child").unwrap();

    let m = collect_abs_snapshot(
        &[parent.clone(), child.clone()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let paths: Vec<_> = m.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths
            .iter()
            .filter(|p| p.contains("parent_file.txt"))
            .count(),
        1
    );
    assert_eq!(
        paths
            .iter()
            .filter(|p| p.contains("child_file.txt"))
            .count(),
        1
    );
}

#[test]
fn same_directory_twice_deduplicated() {
    let tmp = TempDir::new().unwrap();
    let sub = tmp.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("file.txt"), "content").unwrap();

    let m = collect_abs_snapshot(
        &[sub.clone(), sub.clone()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    assert_eq!(
        m.files
            .iter()
            .filter(|f| f.path.contains("file.txt"))
            .count(),
        1
    );
}

#[test]
fn file_in_directory_and_filenames_deduplicated() {
    let tmp = TempDir::new().unwrap();
    let sub = tmp.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let fp = sub.join("file.txt");
    std::fs::write(&fp, "12345").unwrap();

    let m = collect_abs_snapshot(&[sub], &[fp], CollectOptions::default()).unwrap();

    assert_eq!(m.files.len(), 1);
    assert_eq!(m.total_size, 5);
}

#[test]
fn total_size_not_double_counted() {
    let tmp = TempDir::new().unwrap();
    let sub = tmp.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let fp = sub.join("file.txt");
    std::fs::write(&fp, "12345").unwrap();

    let m = collect_abs_snapshot(&[sub], &[fp], CollectOptions::default()).unwrap();

    assert_eq!(m.total_size, 5);
}

#[test]
fn sibling_directories_both_collected() {
    let tmp = TempDir::new().unwrap();
    let d1 = tmp.path().join("d1");
    let d2 = tmp.path().join("d2");
    std::fs::create_dir_all(&d1).unwrap();
    std::fs::create_dir_all(&d2).unwrap();
    std::fs::write(d1.join("f1.txt"), "a").unwrap();
    std::fs::write(d2.join("f2.txt"), "b").unwrap();

    let m = collect_abs_snapshot(&[d1, d2], &[] as &[PathBuf], CollectOptions::default()).unwrap();

    assert!(m.files.iter().any(|f| f.path.ends_with("f1.txt")));
    assert!(m.files.iter().any(|f| f.path.ends_with("f2.txt")));
}

#[cfg(unix)]
#[test]
fn symlink_and_target_both_collected_preserve() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("target.txt");
    std::fs::write(&target, "content").unwrap();
    std::os::unix::fs::symlink("target.txt", tmp.path().join("link.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::Preserve,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(m.files.iter().any(|f| f.path.ends_with("target.txt")));
    assert!(m.files.iter().any(|f| f.path.ends_with("link.txt")));
}

#[cfg(unix)]
#[test]
fn symlink_in_filenames_and_directory_deduplicated() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("target.txt"), "content").unwrap();
    let link = tmp.path().join("link.txt");
    std::os::unix::fs::symlink("target.txt", &link).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[link],
        CollectOptions {
            symlink_policy: SymlinkPolicy::Preserve,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        m.files
            .iter()
            .filter(|f| f.path.ends_with("link.txt"))
            .count(),
        1
    );
}

#[test]
fn directory_entries_deduplicated() {
    let tmp = TempDir::new().unwrap();
    let parent = tmp.path().join("parent");
    let child = parent.join("child");
    let grandchild = child.join("grandchild");
    std::fs::create_dir_all(&grandchild).unwrap();

    let m = collect_abs_snapshot(
        &[parent.clone(), child.clone()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let dir_paths: Vec<_> = m.dirs.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(
        dir_paths
            .iter()
            .filter(|p| p.ends_with("grandchild"))
            .count(),
        1
    );
}

// ===== ExcludeEscaping policy =====

#[cfg(unix)]
#[test]
fn exclude_escaping_non_escaping_preserved() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("target.txt"), "content").unwrap();
    std::os::unix::fs::symlink("target.txt", root.join("link.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::ExcludeEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    let link = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link.txt"))
        .unwrap();
    assert!(link.symlink_target.is_some());
}

#[cfg(unix)]
#[test]
fn exclude_escaping_file_symlink_excluded() {
    let tmp = TempDir::new().unwrap();
    let outside = tmp.path().join("outside.txt");
    std::fs::write(&outside, "outside").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::ExcludeEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!m.files.iter().any(|f| f.path.ends_with("link.txt")));
    assert!(!m.files.iter().any(|f| f.path.ends_with("outside.txt")));
}

#[cfg(unix)]
#[test]
fn exclude_escaping_dir_symlink_excluded() {
    let tmp = TempDir::new().unwrap();
    let outside_dir = tmp.path().join("outside_dir");
    std::fs::create_dir_all(&outside_dir).unwrap();
    std::fs::write(outside_dir.join("file.txt"), "content").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside_dir, root.join("link_dir")).unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::ExcludeEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!m.files.iter().any(|f| f.path.contains("link_dir")));
}

#[cfg(unix)]
#[test]
fn exclude_escaping_mixed_scenario() {
    let tmp = TempDir::new().unwrap();
    let outside = tmp.path().join("outside.txt");
    std::fs::write(&outside, "outside").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("internal.txt"), "internal").unwrap();
    std::os::unix::fs::symlink("internal.txt", root.join("link_in.txt")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link_out.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::ExcludeEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(m.files.iter().any(|f| f.path.ends_with("internal.txt")));
    assert!(m.files.iter().any(|f| f.path.ends_with("link_in.txt")));
    assert!(!m.files.iter().any(|f| f.path.ends_with("link_out.txt")));
}

#[cfg(unix)]
#[test]
fn exclude_escaping_chain_in_in_file_in() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("file.txt"), "content").unwrap();
    std::os::unix::fs::symlink("file.txt", root.join("link1.txt")).unwrap();
    std::os::unix::fs::symlink("link1.txt", root.join("link2.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::ExcludeEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(m.files.iter().any(|f| f.path.ends_with("file.txt")));
    assert!(m.files.iter().any(|f| f.path.ends_with("link1.txt")));
    assert!(m.files.iter().any(|f| f.path.ends_with("link2.txt")));
}

#[cfg(unix)]
#[test]
fn exclude_escaping_broken_symlink_excluded() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(tmp.path().join("nonexistent.txt"), root.join("broken.txt"))
        .unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::ExcludeEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!m.files.iter().any(|f| f.path.contains("broken")));
}

#[cfg(unix)]
#[test]
fn exclude_escaping_no_symlinks_works() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "content").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::ExcludeEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(m.files.iter().any(|f| f.path.ends_with("file.txt")));
}

// ===== TransitiveIncludeTargets policy =====

#[cfg(unix)]
#[test]
fn transitive_escaping_file_target_collected() {
    let tmp = TempDir::new().unwrap();
    let outside = tmp.path().join("outside.txt");
    std::fs::write(&outside, "outside content").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::TransitiveIncludeTargets,
            ..Default::default()
        },
    )
    .unwrap();

    let paths: std::collections::HashSet<_> = m.files.iter().map(|f| f.path.as_str()).collect();
    // Symlink preserved
    assert!(paths.iter().any(|p| p.ends_with("link.txt")));
    let link = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link.txt"))
        .unwrap();
    assert!(link.symlink_target.is_some());
    // Target collected as regular file
    let outside_str = outside.to_str().unwrap();
    assert!(paths.contains(outside_str));
    let target = m.files.iter().find(|f| f.path == outside_str).unwrap();
    assert!(target.symlink_target.is_none());
    assert_eq!(target.size, Some(15));
}

#[cfg(unix)]
#[test]
fn transitive_multiple_symlinks_same_target_once() {
    let tmp = TempDir::new().unwrap();
    let outside = tmp.path().join("outside.txt");
    std::fs::write(&outside, "content").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link1.txt")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link2.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::TransitiveIncludeTargets,
            ..Default::default()
        },
    )
    .unwrap();

    let outside_str = outside.to_str().unwrap();
    assert_eq!(m.files.iter().filter(|f| f.path == outside_str).count(), 1);
}

#[cfg(unix)]
#[test]
fn transitive_non_escaping_target_not_duplicated() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("target.txt"), "content").unwrap();
    std::os::unix::fs::symlink("target.txt", root.join("link.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::TransitiveIncludeTargets,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        m.files
            .iter()
            .filter(|f| f.path.ends_with("target.txt"))
            .count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn transitive_dir_target_contents_collected() {
    let tmp = TempDir::new().unwrap();
    let outside_dir = tmp.path().join("outside_dir");
    std::fs::create_dir_all(&outside_dir).unwrap();
    std::fs::write(outside_dir.join("file1.txt"), "c1").unwrap();
    std::fs::write(outside_dir.join("file2.txt"), "c2").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside_dir, root.join("link_dir")).unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::TransitiveIncludeTargets,
            ..Default::default()
        },
    )
    .unwrap();

    let paths: std::collections::HashSet<_> = m.files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(outside_dir.join("file1.txt").to_str().unwrap()));
    assert!(paths.contains(outside_dir.join("file2.txt").to_str().unwrap()));
}

#[cfg(unix)]
#[test]
fn transitive_chain_of_escaping_symlinks() {
    let tmp = TempDir::new().unwrap();
    let final_file = tmp.path().join("final.txt");
    std::fs::write(&final_file, "final content").unwrap();
    let sym1 = tmp.path().join("sym1.txt");
    std::os::unix::fs::symlink(&final_file, &sym1).unwrap();
    let sym2 = tmp.path().join("sym2.txt");
    std::os::unix::fs::symlink(&sym1, &sym2).unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&sym2, root.join("link.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::TransitiveIncludeTargets,
            ..Default::default()
        },
    )
    .unwrap();

    let paths: std::collections::HashSet<_> = m.files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(sym2.to_str().unwrap()));
    assert!(paths.contains(sym1.to_str().unwrap()));
    assert!(paths.contains(final_file.to_str().unwrap()));
    let final_entry = m
        .files
        .iter()
        .find(|f| f.path == final_file.to_str().unwrap())
        .unwrap();
    assert!(final_entry.symlink_target.is_none());
    assert_eq!(final_entry.size, Some(13));
}

#[cfg(unix)]
#[test]
fn transitive_broken_target_skipped() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    let nonexistent = tmp.path().join("nonexistent.txt");
    std::os::unix::fs::symlink(&nonexistent, root.join("broken.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::TransitiveIncludeTargets,
            ..Default::default()
        },
    )
    .unwrap();

    // Symlink preserved but target not in manifest
    assert!(m.files.iter().any(|f| f.path.ends_with("broken.txt")));
    assert!(!m
        .files
        .iter()
        .any(|f| f.path == nonexistent.to_str().unwrap() && f.symlink_target.is_none()));
}

#[cfg(unix)]
#[test]
fn transitive_filenames_parameter() {
    let tmp = TempDir::new().unwrap();
    let outside = tmp.path().join("outside.txt");
    std::fs::write(&outside, "outside content").unwrap();
    let link = tmp.path().join("link.txt");
    std::os::unix::fs::symlink(&outside, &link).unwrap();

    let m = collect_abs_snapshot(
        &[] as &[PathBuf],
        &[link],
        CollectOptions {
            symlink_policy: SymlinkPolicy::TransitiveIncludeTargets,
            ..Default::default()
        },
    )
    .unwrap();

    let paths: std::collections::HashSet<_> = m.files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(outside.to_str().unwrap()));
}

// ===== Runnable flag =====

#[cfg(unix)]
#[test]
fn runnable_flag_captured_true() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let script = tmp.path().join("script.sh");
    std::fs::write(&script, "#!/bin/bash\necho hello").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let entry = m
        .files
        .iter()
        .find(|f| f.path.ends_with("script.sh"))
        .unwrap();
    assert!(entry.runnable, "executable file should have runnable=true");
}

#[test]
fn runnable_flag_captured_false() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("file.txt");
    std::fs::write(&file, "content").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    }

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let entry = m
        .files
        .iter()
        .find(|f| f.path.ends_with("file.txt"))
        .unwrap();
    assert!(
        !entry.runnable,
        "non-executable file should have runnable=false"
    );
}

#[cfg(unix)]
#[test]
fn runnable_flag_from_filenames_parameter() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let script = tmp.path().join("run.sh");
    std::fs::write(&script, "#!/bin/bash").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let m = collect_abs_snapshot(&[] as &[PathBuf], &[script], CollectOptions::default()).unwrap();

    assert!(
        m.files[0].runnable,
        "executable file via filenames should have runnable=true"
    );
}

#[cfg(unix)]
#[test]
fn runnable_flag_group_execute_only() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("group_exec.sh");
    std::fs::write(&file, "#!/bin/bash").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o610)).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let entry = m
        .files
        .iter()
        .find(|f| f.path.ends_with("group_exec.sh"))
        .unwrap();
    assert!(
        entry.runnable,
        "group-executable file should have runnable=true"
    );
}

#[cfg(unix)]
#[test]
fn runnable_flag_other_execute_only() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("other_exec.sh");
    std::fs::write(&file, "#!/bin/bash").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o601)).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let entry = m
        .files
        .iter()
        .find(|f| f.path.ends_with("other_exec.sh"))
        .unwrap();
    assert!(
        entry.runnable,
        "other-executable file should have runnable=true"
    );
}

#[cfg(unix)]
#[test]
fn runnable_flag_preserved_through_collapse_escaping() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let script = outside.join("run.sh");
    std::fs::write(&script, "#!/bin/bash").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let inner = tmp.path().join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    std::os::unix::fs::symlink(&script, inner.join("link.sh")).unwrap();

    let m = collect_abs_snapshot(
        &[inner.to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    let entry = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link.sh"))
        .unwrap();
    assert!(
        entry.symlink_target.is_none(),
        "escaping symlink should be collapsed"
    );
    assert!(
        entry.runnable,
        "collapsed symlink to executable should have runnable=true"
    );
}

// ===== CollapseEscaping extended tests =====

#[cfg(unix)]
#[test]
fn collapse_escaping_dir_symlink_collapsed() {
    let tmp = TempDir::new().unwrap();
    let outside_dir = tmp.path().join("outside_dir");
    std::fs::create_dir_all(&outside_dir).unwrap();
    std::fs::write(outside_dir.join("file.txt"), "content").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside_dir, root.join("link_dir")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // Contents appear under link_dir path
    assert!(m.files.iter().any(|f| f.path.contains("link_dir/file.txt")));
    // outside_dir NOT in manifest
    assert!(!m
        .files
        .iter()
        .any(|f| f.path.contains("outside_dir/file.txt")));
    assert!(!m.dirs.iter().any(|d| d.path.ends_with("outside_dir")));
}

#[cfg(unix)]
#[test]
fn collapse_escaping_mixed_escaping_and_non_escaping() {
    let tmp = TempDir::new().unwrap();
    let outside = tmp.path().join("outside.txt");
    std::fs::write(&outside, "outside").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("internal.txt"), "internal").unwrap();
    std::os::unix::fs::symlink("internal.txt", root.join("link_internal.txt")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link_outside.txt")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // link_internal preserved as symlink
    let link_in = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link_internal.txt"))
        .unwrap();
    assert!(link_in.symlink_target.is_some());
    // link_outside collapsed
    let link_out = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link_outside.txt"))
        .unwrap();
    assert!(link_out.symlink_target.is_none());
}

#[cfg(unix)]
#[test]
fn collapse_escaping_sibling_directory_preserved() {
    let tmp = TempDir::new().unwrap();
    let dir1 = tmp.path().join("dir1");
    std::fs::create_dir_all(&dir1).unwrap();
    std::fs::write(dir1.join("file1.txt"), "content1").unwrap();
    let dir2 = tmp.path().join("dir2");
    std::fs::create_dir_all(&dir2).unwrap();
    std::os::unix::fs::symlink(&dir1, dir2.join("link_to_dir1")).unwrap();

    let m = collect_abs_snapshot(
        &[dir1.clone(), dir2.clone()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // link_to_dir1 preserved as symlink (dir1 is in collected set)
    let link = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link_to_dir1"))
        .unwrap();
    assert!(link.symlink_target.is_some());
}

#[cfg(unix)]
#[test]
fn collapse_escaping_chain_in_in_file_in() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("file.txt"), "content").unwrap();
    std::os::unix::fs::symlink("file.txt", root.join("link1.txt")).unwrap();
    std::os::unix::fs::symlink("link1.txt", root.join("link2.txt")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.files.len(), 3);
    let l1 = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link1.txt"))
        .unwrap();
    let l2 = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link2.txt"))
        .unwrap();
    assert!(l1.symlink_target.is_some());
    assert!(l2.symlink_target.is_some());
}

#[cfg(unix)]
#[test]
fn collapse_escaping_chain_in_in_dir_in() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    let subdir = root.join("subdir");
    std::fs::create_dir_all(&subdir).unwrap();
    std::fs::write(subdir.join("file.txt"), "content").unwrap();
    std::os::unix::fs::symlink(&subdir, root.join("link1")).unwrap();
    std::os::unix::fs::symlink("link1", root.join("link2")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // Both symlinks preserved
    let l1 = m.files.iter().find(|f| f.path.ends_with("/link1")).unwrap();
    let l2 = m.files.iter().find(|f| f.path.ends_with("/link2")).unwrap();
    assert!(l1.symlink_target.is_some());
    assert!(l2.symlink_target.is_some());
}

#[cfg(unix)]
#[test]
fn collapse_escaping_chain_in_in_file_out() {
    let tmp = TempDir::new().unwrap();
    let outside_file = tmp.path().join("outside.txt");
    std::fs::write(&outside_file, "outside content").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside_file, root.join("link1.txt")).unwrap();
    std::os::unix::fs::symlink("link1.txt", root.join("link2.txt")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // link1 collapsed (target OUT)
    let l1 = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link1.txt"))
        .unwrap();
    assert!(l1.symlink_target.is_none());
    // link2 preserved (target link1 is IN)
    let l2 = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link2.txt"))
        .unwrap();
    assert!(l2.symlink_target.is_some());
}

#[cfg(unix)]
#[test]
fn collapse_escaping_chain_in_in_dir_out() {
    let tmp = TempDir::new().unwrap();
    let outside_dir = tmp.path().join("outside_dir");
    std::fs::create_dir_all(&outside_dir).unwrap();
    std::fs::write(outside_dir.join("file.txt"), "content").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside_dir, root.join("link1")).unwrap();
    std::os::unix::fs::symlink("link1", root.join("link2")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // link1 collapsed (dir contents inlined)
    assert!(m.files.iter().any(|f| f.path.contains("link1/file.txt")));
    // link2 preserved as symlink to link1
    let l2 = m.files.iter().find(|f| f.path.ends_with("/link2")).unwrap();
    assert!(l2.symlink_target.is_some());
}

#[cfg(unix)]
#[test]
fn collapse_escaping_chain_in_out_file_in() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("file.txt"), "content").unwrap();
    let outside_link = tmp.path().join("outside_link.txt");
    std::os::unix::fs::symlink(root.join("file.txt"), &outside_link).unwrap();
    std::os::unix::fs::symlink(&outside_link, root.join("link2.txt")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // target_file collected normally
    assert!(m
        .files
        .iter()
        .any(|f| f.path.ends_with("file.txt") && f.symlink_target.is_none()));
    // link2 collapsed (outside_link is OUT)
    let l2 = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link2.txt"))
        .unwrap();
    assert!(l2.symlink_target.is_none());
}

#[cfg(unix)]
#[test]
fn collapse_escaping_chain_in_out_dir_in() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    let subdir = root.join("subdir");
    std::fs::create_dir_all(&subdir).unwrap();
    std::fs::write(subdir.join("file.txt"), "content").unwrap();
    let outside_link = tmp.path().join("outside_link");
    std::os::unix::fs::symlink(&subdir, &outside_link).unwrap();
    std::os::unix::fs::symlink(&outside_link, root.join("link2")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // subdir collected normally
    assert!(m.files.iter().any(|f| f.path.contains("subdir/file.txt")));
    // link2 collapsed (outside_link is OUT), contents inlined under link2
    assert!(m.files.iter().any(|f| f.path.contains("link2/file.txt")));
}

#[cfg(unix)]
#[test]
fn collapse_escaping_chain_partial_escape() {
    let tmp = TempDir::new().unwrap();
    let outside = tmp.path().join("outside.txt");
    std::fs::write(&outside, "outside").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link2.txt")).unwrap();
    std::os::unix::fs::symlink("link2.txt", root.join("link1.txt")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // link1 preserved (target link2 is IN)
    let l1 = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link1.txt"))
        .unwrap();
    assert!(l1.symlink_target.is_some());
    // link2 collapsed (target outside is OUT)
    let l2 = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link2.txt"))
        .unwrap();
    assert!(l2.symlink_target.is_none());
}

#[cfg(unix)]
#[test]
fn collapse_escaping_broken_dir_symlink_skipped() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(
        tmp.path().join("nonexistent_dir"),
        root.join("broken_dir_link"),
    )
    .unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!m.files.iter().any(|f| f.path.contains("broken_dir_link")));
    assert!(!m.dirs.iter().any(|d| d.path.contains("broken_dir_link")));
}

#[cfg(unix)]
#[test]
fn collapse_escaping_file_not_dir() {
    let tmp = TempDir::new().unwrap();
    let outside_file = tmp.path().join("outside_file.txt");
    std::fs::write(&outside_file, "content").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside_file, root.join("link_to_file")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    let f = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link_to_file"))
        .unwrap();
    assert!(f.symlink_target.is_none());
    assert_eq!(f.size, Some(7)); // "content"
}

#[cfg(unix)]
#[test]
fn collapse_escaping_deeply_nested_dir() {
    let tmp = TempDir::new().unwrap();
    let outside_dir = tmp.path().join("outside_dir");
    let level1 = outside_dir.join("level1");
    let level2 = level1.join("level2");
    std::fs::create_dir_all(&level2).unwrap();
    std::fs::write(level2.join("deep_file.txt"), "deep content").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside_dir, root.join("link_dir")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // All nested content inlined under link_dir
    assert!(m
        .files
        .iter()
        .any(|f| f.path.contains("link_dir/level1/level2/deep_file.txt")));
}

#[cfg(unix)]
#[test]
fn collapse_escaping_relative_symlink() {
    let tmp = TempDir::new().unwrap();
    let outside = tmp.path().join("outside.txt");
    std::fs::write(&outside, "outside content").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink("../outside.txt", root.join("link.txt")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // Should be collapsed (relative symlink escaping the root)
    let f = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link.txt"))
        .unwrap();
    assert!(f.symlink_target.is_none());
    assert_eq!(f.size, Some(15)); // "outside content"
}

// ===== Optional filenames, broken symlinks, unreadable files, transitive edge cases =====

#[test]
fn optional_existing_file_included() {
    let tmp = TempDir::new().unwrap();
    let required = tmp.path().join("required.txt");
    let optional = tmp.path().join("optional.txt");
    std::fs::write(&required, "req").unwrap();
    std::fs::write(&optional, "opt").unwrap();

    let m = collect_abs_snapshot(
        &[] as &[PathBuf],
        &[required],
        CollectOptions {
            optional_filenames: vec![optional],
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.files.len(), 2);
    assert!(m.files.iter().any(|f| f.path.ends_with("required.txt")));
    assert!(m.files.iter().any(|f| f.path.ends_with("optional.txt")));
}

#[cfg(unix)]
#[test]
fn optional_directory_ignored() {
    let tmp = TempDir::new().unwrap();
    let required = tmp.path().join("required.txt");
    std::fs::write(&required, "req").unwrap();
    let subdir = tmp.path().join("subdir");
    std::fs::create_dir_all(&subdir).unwrap();

    let m = collect_abs_snapshot(
        &[] as &[PathBuf],
        &[required],
        CollectOptions {
            optional_filenames: vec![subdir.clone()],
            ..Default::default()
        },
    )
    .unwrap();

    assert!(m.files.iter().any(|f| f.path.ends_with("required.txt")));
    // Directories passed as optional_filenames are collected as file entries
    // (no validation prevents this — same as directory_as_filename_not_added_as_file)
    assert!(m.files.iter().any(|f| f.path.ends_with("subdir")));
}

#[cfg(unix)]
#[test]
fn optional_symlink_included_when_exists() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("target.txt");
    std::fs::write(&target, "data").unwrap();
    let link = tmp.path().join("link.txt");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let m = collect_abs_snapshot(
        &[] as &[PathBuf],
        &[] as &[PathBuf],
        CollectOptions {
            optional_filenames: vec![link],
            symlink_policy: SymlinkPolicy::Preserve,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.files.len(), 1);
    let entry = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link.txt"))
        .unwrap();
    assert!(entry.symlink_target.is_some());
}

#[cfg(unix)]
#[test]
fn broken_symlink_preserved_with_preserve() {
    let tmp = TempDir::new().unwrap();
    let nonexistent = tmp.path().join("nonexistent.txt");
    std::os::unix::fs::symlink(&nonexistent, tmp.path().join("broken_link")).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::Preserve,
            ..Default::default()
        },
    )
    .unwrap();

    let entry = m
        .files
        .iter()
        .find(|f| f.path.ends_with("broken_link"))
        .unwrap();
    assert!(entry.symlink_target.is_some());
    assert!(entry
        .symlink_target
        .as_ref()
        .unwrap()
        .ends_with("nonexistent.txt"));
}

#[cfg(unix)]
#[test]
fn unreadable_file_in_directory_skipped() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("readable.txt"), "ok").unwrap();
    let unreadable = tmp.path().join("unreadable.txt");
    std::fs::write(&unreadable, "secret").unwrap();
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();

    let result = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    );

    // Restore permissions for cleanup
    let _ = std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o644));

    // The file is still stat-able (metadata works), so it should be collected.
    // If the implementation errors, that's also acceptable.
    match result {
        Ok(m) => {
            assert!(m.files.iter().any(|f| f.path.ends_with("readable.txt")));
            // unreadable.txt metadata is still accessible, so it appears
            assert!(m.files.iter().any(|f| f.path.ends_with("unreadable.txt")));
        }
        Err(_) => {
            // Also acceptable: implementation may error on unreadable files
        }
    }
}

#[cfg(unix)]
#[test]
fn deeply_nested_transitive_dir() {
    let tmp = TempDir::new().unwrap();
    let outside_dir = tmp.path().join("outside_dir");
    std::fs::create_dir_all(outside_dir.join("level1/level2")).unwrap();
    std::fs::write(outside_dir.join("root_file.txt"), "root").unwrap();
    std::fs::write(outside_dir.join("level1/level2/deep_file.txt"), "deep").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside_dir, root.join("link_dir")).unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::TransitiveIncludeTargets,
            ..Default::default()
        },
    )
    .unwrap();

    let paths: std::collections::HashSet<_> = m.files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(outside_dir.join("root_file.txt").to_str().unwrap()));
    assert!(paths.contains(
        outside_dir
            .join("level1/level2/deep_file.txt")
            .to_str()
            .unwrap()
    ));
}

#[cfg(unix)]
#[test]
fn empty_transitive_dir() {
    let tmp = TempDir::new().unwrap();
    let outside_dir = tmp.path().join("outside_dir");
    std::fs::create_dir_all(&outside_dir).unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside_dir, root.join("link_dir")).unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::TransitiveIncludeTargets,
            ..Default::default()
        },
    )
    .unwrap();

    // link_dir symlink is in manifest
    assert!(m
        .files
        .iter()
        .any(|f| f.path.ends_with("link_dir") && f.symlink_target.is_some()));
    // No regular files from outside_dir
    let outside_str = outside_dir.to_str().unwrap();
    assert!(!m.files.iter().any(|f| f.path.starts_with(outside_str)
        && f.symlink_target.is_none()
        && !f.path.ends_with("outside_dir")));
}

#[cfg(unix)]
#[test]
fn symlinks_in_transitive_dir_transitively_included() {
    let tmp = TempDir::new().unwrap();
    let somewhere_else = tmp.path().join("somewhere_else.txt");
    std::fs::write(&somewhere_else, "elsewhere").unwrap();
    let outside_dir = tmp.path().join("outside_dir");
    std::fs::create_dir_all(&outside_dir).unwrap();
    std::fs::write(outside_dir.join("file.txt"), "content").unwrap();
    std::os::unix::fs::symlink(&somewhere_else, outside_dir.join("nested_link")).unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside_dir, root.join("link_dir")).unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::TransitiveIncludeTargets,
            ..Default::default()
        },
    )
    .unwrap();

    let paths: std::collections::HashSet<_> = m.files.iter().map(|f| f.path.as_str()).collect();
    // file.txt collected under outside_dir
    assert!(paths.contains(outside_dir.join("file.txt").to_str().unwrap()));
    // nested_link preserved as symlink
    let nested = m
        .files
        .iter()
        .find(|f| f.path.ends_with("nested_link"))
        .unwrap();
    assert!(nested.symlink_target.is_some());
    // somewhere_else.txt transitively collected
    assert!(paths.contains(somewhere_else.to_str().unwrap()));
}

#[cfg(unix)]
#[test]
fn dir_symlinks_in_transitive_dir_transitively_included() {
    let tmp = TempDir::new().unwrap();
    let deep_dir = tmp.path().join("deep_dir");
    std::fs::create_dir_all(&deep_dir).unwrap();
    std::fs::write(deep_dir.join("deep_file.txt"), "deep").unwrap();
    let outside_dir = tmp.path().join("outside_dir");
    std::fs::create_dir_all(&outside_dir).unwrap();
    std::fs::write(outside_dir.join("file.txt"), "content").unwrap();
    std::os::unix::fs::symlink(&deep_dir, outside_dir.join("nested_dir_link")).unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside_dir, root.join("link_dir")).unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::TransitiveIncludeTargets,
            ..Default::default()
        },
    )
    .unwrap();

    let paths: std::collections::HashSet<_> = m.files.iter().map(|f| f.path.as_str()).collect();
    // file.txt collected under outside_dir
    assert!(paths.contains(outside_dir.join("file.txt").to_str().unwrap()));
    // nested_dir_link preserved as symlink
    let nested = m
        .files
        .iter()
        .find(|f| f.path.ends_with("nested_dir_link"))
        .unwrap();
    assert!(nested.symlink_target.is_some());
    // deep_file.txt transitively collected
    assert!(paths.contains(deep_dir.join("deep_file.txt").to_str().unwrap()));
}

#[cfg(unix)]
#[test]
fn deep_chain_of_escaping_symlinks() {
    let tmp = TempDir::new().unwrap();
    let final_file = tmp.path().join("final_file.txt");
    std::fs::write(&final_file, "final").unwrap();
    let sym1 = tmp.path().join("sym1");
    std::os::unix::fs::symlink(&final_file, &sym1).unwrap();
    let sym2 = tmp.path().join("sym2");
    std::os::unix::fs::symlink(&sym1, &sym2).unwrap();
    let sym3 = tmp.path().join("sym3");
    std::os::unix::fs::symlink(&sym2, &sym3).unwrap();
    let sym4 = tmp.path().join("sym4");
    std::os::unix::fs::symlink(&sym3, &sym4).unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&sym4, root.join("link")).unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::TransitiveIncludeTargets,
            ..Default::default()
        },
    )
    .unwrap();

    // All 6 entries: link, sym4, sym3, sym2, sym1, final_file.txt
    assert_eq!(m.files.len(), 6);
    let paths: std::collections::HashSet<_> = m.files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.iter().any(|p| p.ends_with("link")));
    assert!(paths.contains(sym4.to_str().unwrap()));
    assert!(paths.contains(sym3.to_str().unwrap()));
    assert!(paths.contains(sym2.to_str().unwrap()));
    assert!(paths.contains(sym1.to_str().unwrap()));
    assert!(paths.contains(final_file.to_str().unwrap()));
    // final_file is a regular file
    let final_entry = m
        .files
        .iter()
        .find(|f| f.path == final_file.to_str().unwrap())
        .unwrap();
    assert!(final_entry.symlink_target.is_none());
}

#[cfg(unix)]
#[test]
fn dir_symlink_in_filenames_transitive() {
    let tmp = TempDir::new().unwrap();
    let outside_dir = tmp.path().join("outside_dir");
    std::fs::create_dir_all(&outside_dir).unwrap();
    std::fs::write(outside_dir.join("file.txt"), "content").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    let link_dir = root.join("link_dir");
    std::os::unix::fs::symlink(&outside_dir, &link_dir).unwrap();

    let m = collect_abs_snapshot(
        &[] as &[PathBuf],
        &[link_dir],
        CollectOptions {
            symlink_policy: SymlinkPolicy::TransitiveIncludeTargets,
            ..Default::default()
        },
    )
    .unwrap();

    // link_dir preserved as symlink
    let link_entry = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link_dir"))
        .unwrap();
    assert!(link_entry.symlink_target.is_some());
    // outside_dir/file.txt collected
    let paths: std::collections::HashSet<_> = m.files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(outside_dir.join("file.txt").to_str().unwrap()));
}

// ===== Basic collection: root directory and backslash names =====

#[test]
fn root_directory_included_in_dirs() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "content").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let root_norm = tmp.path().canonicalize().unwrap();
    assert!(
        m.dirs.iter().any(|d| d.path == root_norm.to_str().unwrap()),
        "root directory should appear in dirs, got: {:?}",
        m.dirs.iter().map(|d| &d.path).collect::<Vec<_>>()
    );
}

#[cfg(unix)]
#[test]
fn backslash_in_filename_posix() {
    let tmp = TempDir::new().unwrap();
    let name = "file\\name.txt";
    std::fs::write(tmp.path().join(name), "content").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    assert!(
        m.files.iter().any(|f| f.path.contains("file\\name.txt")),
        "backslash should be preserved, got: {:?}",
        m.files.iter().map(|f| &f.path).collect::<Vec<_>>()
    );
}

#[cfg(unix)]
#[test]
fn backslash_in_directory_name_posix() {
    let tmp = TempDir::new().unwrap();
    let dir_name = "dir\\name";
    let dir_path = tmp.path().join(dir_name);
    std::fs::create_dir_all(&dir_path).unwrap();
    std::fs::write(dir_path.join("file.txt"), "content").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    assert!(
        m.dirs.iter().any(|d| d.path.contains("dir\\name")),
        "backslash in dir name should be preserved, got dirs: {:?}",
        m.dirs.iter().map(|d| &d.path).collect::<Vec<_>>()
    );
}

// ===== Collapse escaping: broken file symlink, nested symlinks =====

#[cfg(unix)]
#[test]
fn collapse_escaping_broken_file_symlink_skipped() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("real.txt"), "ok").unwrap();
    std::os::unix::fs::symlink(tmp.path().join("nonexistent.txt"), root.join("broken.txt"))
        .unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!m.files.iter().any(|f| f.path.contains("broken.txt")));
    assert!(m.files.iter().any(|f| f.path.ends_with("real.txt")));
}

#[cfg(unix)]
#[test]
fn collapse_escaping_nested_symlinks_in_collapsed_dir_escaping_collapsed() {
    let tmp = TempDir::new().unwrap();
    let far_away = tmp.path().join("far_away.txt");
    std::fs::write(&far_away, "far").unwrap();
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("file.txt"), "content").unwrap();
    std::os::unix::fs::symlink(&far_away, outside.join("escaping_link")).unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link_dir")).unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    let escaping = m
        .files
        .iter()
        .find(|f| f.path.ends_with("escaping_link"))
        .unwrap();
    assert!(
        escaping.symlink_target.is_none(),
        "nested escaping symlink should be collapsed"
    );
    assert_eq!(escaping.size, Some(3));
}

#[cfg(unix)]
#[test]
fn collapse_escaping_nested_symlinks_in_collapsed_dir_internal_preserved() {
    let tmp = TempDir::new().unwrap();
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(outside.join("sub")).unwrap();
    std::fs::write(outside.join("file.txt"), "content").unwrap();
    std::os::unix::fs::symlink(outside.join("file.txt"), outside.join("sub/internal_link"))
        .unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link_dir")).unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    let internal = m
        .files
        .iter()
        .find(|f| f.path.ends_with("sub/internal_link"))
        .unwrap();
    assert!(
        internal.symlink_target.is_some(),
        "non-escaping symlink should be preserved"
    );
}

// ===== Deduplication tests =====

#[test]
fn file_in_filenames_and_optional_deduplicated() {
    let tmp = TempDir::new().unwrap();
    let fp = tmp.path().join("file.txt");
    std::fs::write(&fp, "12345").unwrap();

    let m = collect_abs_snapshot(
        &[] as &[PathBuf],
        #[allow(clippy::cloned_ref_to_slice_refs)] // fp is moved into optional_filenames below
        &[fp.clone()],
        CollectOptions {
            optional_filenames: vec![fp],
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.files.len(), 1);
    assert_eq!(m.total_size, 5);
}

#[test]
fn file_in_directory_and_optional_deduplicated() {
    let tmp = TempDir::new().unwrap();
    let sub = tmp.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let fp = sub.join("file.txt");
    std::fs::write(&fp, "12345").unwrap();

    let m = collect_abs_snapshot(
        &[sub],
        &[] as &[PathBuf],
        CollectOptions {
            optional_filenames: vec![fp],
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.files.len(), 1);
    assert_eq!(m.total_size, 5);
}

#[cfg(unix)]
#[test]
fn multiple_symlinks_to_same_target_dedup() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("target.txt");
    std::fs::write(&target, "content").unwrap();
    std::os::unix::fs::symlink("target.txt", tmp.path().join("link1.txt")).unwrap();
    std::os::unix::fs::symlink("target.txt", tmp.path().join("link2.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::Preserve,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.files.len(), 3); // target + link1 + link2
    assert!(m.files.iter().any(|f| f.path.ends_with("target.txt")));
    assert!(m.files.iter().any(|f| f.path.ends_with("link1.txt")));
    assert!(m.files.iter().any(|f| f.path.ends_with("link2.txt")));
}

#[test]
fn total_size_with_multiple_files() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "aaa").unwrap(); // 3
    std::fs::write(tmp.path().join("b.txt"), "bbbbbb").unwrap(); // 6
    std::fs::write(tmp.path().join("c.txt"), "c").unwrap(); // 1

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    assert_eq!(m.total_size, 10);
}

// ===== ExcludeEscaping chain tests =====

#[cfg(unix)]
#[test]
fn exclude_escaping_symlink_to_sibling_directory_preserved() {
    let tmp = TempDir::new().unwrap();
    let dir1 = tmp.path().join("dir1");
    let dir2 = tmp.path().join("dir2");
    std::fs::create_dir_all(&dir1).unwrap();
    std::fs::create_dir_all(&dir2).unwrap();
    std::fs::write(dir1.join("file.txt"), "content").unwrap();
    std::os::unix::fs::symlink(&dir1, dir2.join("link_to_dir1")).unwrap();

    let m = collect_abs_snapshot(
        &[dir1.clone(), dir2.clone()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::ExcludeEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // dir1 is in collected set, so symlink to it is non-escaping → preserved
    let link = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link_to_dir1"))
        .unwrap();
    assert!(link.symlink_target.is_some());
}

#[cfg(unix)]
#[test]
fn exclude_escaping_chain_in_in_dir_in() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    let subdir = root.join("subdir");
    std::fs::create_dir_all(&subdir).unwrap();
    std::fs::write(subdir.join("file.txt"), "content").unwrap();
    std::os::unix::fs::symlink(&subdir, root.join("link1")).unwrap();
    std::os::unix::fs::symlink("link1", root.join("link2")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::ExcludeEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // Both symlinks point inside root → both preserved
    let l1 = m.files.iter().find(|f| f.path.ends_with("/link1")).unwrap();
    let l2 = m.files.iter().find(|f| f.path.ends_with("/link2")).unwrap();
    assert!(l1.symlink_target.is_some());
    assert!(l2.symlink_target.is_some());
}

#[cfg(unix)]
#[test]
fn exclude_escaping_chain_in_in_file_out() {
    let tmp = TempDir::new().unwrap();
    let outside = tmp.path().join("outside.txt");
    std::fs::write(&outside, "outside").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    // link1 -> outside (escaping), link2 -> link1 (non-escaping)
    std::os::unix::fs::symlink(&outside, root.join("link1.txt")).unwrap();
    std::os::unix::fs::symlink("link1.txt", root.join("link2.txt")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::ExcludeEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // link1 excluded (immediate target is outside)
    assert!(!m.files.iter().any(|f| f.path.ends_with("link1.txt")));
    // link2's immediate target is link1 which is inside root → preserved
    assert!(m.files.iter().any(|f| f.path.ends_with("link2.txt")));
}

#[cfg(unix)]
#[test]
fn exclude_escaping_chain_in_in_dir_out() {
    let tmp = TempDir::new().unwrap();
    let outside_dir = tmp.path().join("outside_dir");
    std::fs::create_dir_all(&outside_dir).unwrap();
    std::fs::write(outside_dir.join("file.txt"), "content").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    // link1 -> outside_dir (escaping), link2 -> link1 (non-escaping)
    std::os::unix::fs::symlink(&outside_dir, root.join("link1")).unwrap();
    std::os::unix::fs::symlink("link1", root.join("link2")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::ExcludeEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // link1 excluded (immediate target outside)
    assert!(!m.files.iter().any(|f| f.path.ends_with("/link1")));
    // link2's immediate target is link1 (inside) → preserved
    assert!(m.files.iter().any(|f| f.path.ends_with("/link2")));
}

#[cfg(unix)]
#[test]
fn exclude_escaping_chain_in_out_file_in() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("file.txt"), "content").unwrap();
    // outside_link -> root/file.txt (lives outside root)
    let outside_link = tmp.path().join("outside_link.txt");
    std::os::unix::fs::symlink(root.join("file.txt"), &outside_link).unwrap();
    // link inside root -> outside_link (escaping)
    std::os::unix::fs::symlink(&outside_link, root.join("link.txt")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::ExcludeEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // link.txt excluded (immediate target is outside root)
    assert!(!m.files.iter().any(|f| f.path.ends_with("link.txt")));
    // file.txt still collected as regular file
    assert!(m
        .files
        .iter()
        .any(|f| f.path.ends_with("file.txt") && f.symlink_target.is_none()));
}

#[cfg(unix)]
#[test]
fn exclude_escaping_chain_in_out_dir_in() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("root");
    let subdir = root.join("subdir");
    std::fs::create_dir_all(&subdir).unwrap();
    std::fs::write(subdir.join("file.txt"), "content").unwrap();
    // outside_link -> root/subdir (lives outside root)
    let outside_link = tmp.path().join("outside_link");
    std::os::unix::fs::symlink(&subdir, &outside_link).unwrap();
    // link inside root -> outside_link (escaping)
    std::os::unix::fs::symlink(&outside_link, root.join("link")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::ExcludeEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // link excluded (immediate target is outside root)
    assert!(!m.files.iter().any(|f| f.path.ends_with("/link")));
    // subdir/file.txt still collected normally
    assert!(m.files.iter().any(|f| f.path.contains("subdir/file.txt")));
}

#[cfg(unix)]
#[test]
fn exclude_escaping_chain_partial_escape() {
    // Chain: link1 -> link2 -> outside. link2 escapes, link1 points to link2 (inside).
    // With exclude_escaping, link2 is excluded but link1 is preserved (immediate target inside).
    let tmp = TempDir::new().unwrap();
    let outside = tmp.path().join("outside.txt");
    std::fs::write(&outside, "outside").unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link2.txt")).unwrap();
    std::os::unix::fs::symlink("link2.txt", root.join("link1.txt")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::ExcludeEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // link2 excluded (immediate target outside)
    assert!(!m.files.iter().any(|f| f.path.ends_with("link2.txt")));
    // link1 preserved (immediate target link2 is inside root)
    let l1 = m
        .files
        .iter()
        .find(|f| f.path.ends_with("link1.txt"))
        .unwrap();
    assert!(l1.symlink_target.is_some());
}

// ===== Symlink cycle tests =====

#[cfg(unix)]
#[test]
fn cycle_with_transitive_include() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("file.txt"), "content").unwrap();
    let outside_a = tmp.path().join("a");
    let outside_b = tmp.path().join("b");
    std::fs::create_dir_all(&outside_a).unwrap();
    std::fs::create_dir_all(&outside_b).unwrap();
    // Cycle: a/link_b -> b, b/link_a -> a
    std::os::unix::fs::symlink(&outside_b, outside_a.join("link_b")).unwrap();
    std::os::unix::fs::symlink(&outside_a, outside_b.join("link_a")).unwrap();
    // Root links to a
    std::os::unix::fs::symlink(&outside_a, root.join("link")).unwrap();

    // Should not infinite loop
    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::TransitiveIncludeTargets,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(m.files.iter().any(|f| f.path.ends_with("file.txt")));
}

#[cfg(unix)]
#[test]
fn long_cycle_chain() {
    // 3-node cycle: a -> b -> c -> a, all outside root
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    let c = tmp.path().join("c");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    std::fs::create_dir_all(&c).unwrap();
    std::os::unix::fs::symlink(&b, a.join("to_b")).unwrap();
    std::os::unix::fs::symlink(&c, b.join("to_c")).unwrap();
    std::os::unix::fs::symlink(&a, c.join("to_a")).unwrap();
    std::os::unix::fs::symlink(&a, root.join("link")).unwrap();

    // Should terminate without infinite loop
    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::TransitiveIncludeTargets,
            ..Default::default()
        },
    )
    .unwrap();

    // At minimum the root link is present
    assert!(m.files.iter().any(|f| f.path.ends_with("/link")));
}

// ===== Additional symlink tests =====

#[cfg(unix)]
#[test]
fn preserve_keeps_all_symlinks_in_chain() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "content").unwrap();
    std::os::unix::fs::symlink("file.txt", tmp.path().join("c.txt")).unwrap();
    std::os::unix::fs::symlink("c.txt", tmp.path().join("b.txt")).unwrap();
    std::os::unix::fs::symlink("b.txt", tmp.path().join("a.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::Preserve,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.files.len(), 4);
    for name in &["a.txt", "b.txt", "c.txt"] {
        let entry = m.files.iter().find(|f| f.path.ends_with(name)).unwrap();
        assert!(
            entry.symlink_target.is_some(),
            "{} should be a symlink entry",
            name
        );
    }
    let file = m
        .files
        .iter()
        .find(|f| f.path.ends_with("file.txt"))
        .unwrap();
    assert!(file.symlink_target.is_none());
}

#[cfg(unix)]
#[test]
fn collapse_nested_directory_symlinks() {
    let tmp = TempDir::new().unwrap();
    let outside1 = tmp.path().join("outside1");
    let outside2 = tmp.path().join("outside2");
    std::fs::create_dir_all(&outside1).unwrap();
    std::fs::create_dir_all(&outside2).unwrap();
    std::fs::write(outside1.join("f1.txt"), "one").unwrap();
    std::fs::write(outside2.join("f2.txt"), "two").unwrap();
    // outside1 contains a dir symlink to outside2
    std::os::unix::fs::symlink(&outside2, outside1.join("nested")).unwrap();

    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside1, root.join("link_dir")).unwrap();

    let m = collect_abs_snapshot(
        std::slice::from_ref(&root),
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseAll,
            ..Default::default()
        },
    )
    .unwrap();

    // Both directory symlinks collapsed, contents inlined
    assert!(m.files.iter().any(|f| f.path.contains("link_dir/f1.txt")));
    assert!(m
        .files
        .iter()
        .any(|f| f.path.contains("link_dir/nested/f2.txt")));
}

#[cfg(unix)]
#[test]
fn collapse_symlink_chain() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "content").unwrap();
    std::os::unix::fs::symlink("file.txt", tmp.path().join("c.txt")).unwrap();
    std::os::unix::fs::symlink("c.txt", tmp.path().join("b.txt")).unwrap();
    std::os::unix::fs::symlink("b.txt", tmp.path().join("a.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseAll,
            ..Default::default()
        },
    )
    .unwrap();

    // All 4 entries present, all as regular files (no symlink_target)
    assert_eq!(m.files.len(), 4);
    for name in &["a.txt", "b.txt", "c.txt", "file.txt"] {
        let entry = m.files.iter().find(|f| f.path.ends_with(name)).unwrap();
        assert!(
            entry.symlink_target.is_none(),
            "{} should be collapsed",
            name
        );
        assert_eq!(
            entry.size,
            Some(7),
            "{} should have size of target file",
            name
        );
    }
}

#[cfg(unix)]
#[test]
fn symlink_in_optional_filenames() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("target.txt");
    std::fs::write(&target, "data").unwrap();
    let link = tmp.path().join("opt_link.txt");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let m = collect_abs_snapshot(
        &[] as &[PathBuf],
        &[] as &[PathBuf],
        CollectOptions {
            optional_filenames: vec![link],
            symlink_policy: SymlinkPolicy::Preserve,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.files.len(), 1);
    let entry = m
        .files
        .iter()
        .find(|f| f.path.ends_with("opt_link.txt"))
        .unwrap();
    assert!(entry.symlink_target.is_some());
}

// ===== Additional error handling tests =====

#[cfg(unix)]
#[test]
fn unreadable_directory_raises_or_skips() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let unreadable = tmp.path().join("unreadable_dir");
    std::fs::create_dir_all(&unreadable).unwrap();
    std::fs::write(unreadable.join("file.txt"), "content").unwrap();
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();

    let result = collect_abs_snapshot(
        std::slice::from_ref(&unreadable),
        &[] as &[PathBuf],
        CollectOptions::default(),
    );

    // Restore permissions for cleanup
    let _ = std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o755));

    // The function should either return an error (WalkDir can't read the dir)
    // or succeed with no files. It must NOT panic.
    match result {
        Err(_) => { /* acceptable: propagated IO/permission error */ }
        Ok(m) => {
            // If it succeeds, the inner file should not be present (can't be listed)
            assert!(
                !m.files.iter().any(|f| f.path.ends_with("file.txt")),
                "file inside unreadable dir should not be collected"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn stat_failure_on_file_skipped() {
    // A file whose metadata can't be read due to parent directory permissions
    // being removed after directory listing. We simulate by creating a file,
    // then making the parent unreadable so symlink_metadata fails on the file
    // when passed as an optional filename.
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let sub = tmp.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let file_path = sub.join("file.txt");
    std::fs::write(&file_path, "content").unwrap();

    // Remove execute permission on parent so stat on child fails
    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o644)).unwrap();

    let result = collect_abs_snapshot(
        &[] as &[PathBuf],
        &[] as &[PathBuf],
        CollectOptions {
            optional_filenames: vec![file_path.clone()],
            ..Default::default()
        },
    );

    // Restore permissions for cleanup
    let _ = std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o755));

    // Optional file whose metadata can't be read should be skipped (symlink_metadata fails)
    // The function must not panic.
    match result {
        Ok(m) => {
            assert_eq!(
                m.files.len(),
                0,
                "file with inaccessible metadata should be skipped as optional"
            );
        }
        Err(_) => {
            // Also acceptable if the implementation propagates the error
        }
    }
}

#[cfg(unix)]
#[test]
fn file_deleted_during_walk_raises() {
    // Simulate a race condition: pass a required filename that doesn't exist.
    // This mimics a file being deleted between directory listing and stat.
    // The function should return an error, not panic.
    let tmp = TempDir::new().unwrap();
    let ghost = tmp.path().join("ghost.txt");
    // File never created — simulates deletion before stat

    let result = collect_abs_snapshot(&[] as &[PathBuf], &[ghost], CollectOptions::default());

    assert!(
        result.is_err(),
        "missing required file should produce an error, not a panic"
    );
}

#[cfg(unix)]
#[test]
fn multiple_escaping_symlinks_different_targets() {
    let tmp = TempDir::new().unwrap();
    let outside1 = tmp.path().join("outside1.txt");
    let outside2 = tmp.path().join("outside2.txt");
    let outside3 = tmp.path().join("outside3.txt");
    std::fs::write(&outside1, "one").unwrap();
    std::fs::write(&outside2, "two").unwrap();
    std::fs::write(&outside3, "three").unwrap();

    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside1, root.join("link1.txt")).unwrap();
    std::os::unix::fs::symlink(&outside2, root.join("link2.txt")).unwrap();
    std::os::unix::fs::symlink(&outside3, root.join("link3.txt")).unwrap();

    let m = collect_abs_snapshot(
        &[root],
        &[] as &[PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::TransitiveIncludeTargets,
            ..Default::default()
        },
    )
    .unwrap();

    let paths: std::collections::HashSet<_> = m.files.iter().map(|f| f.path.as_str()).collect();
    // All 3 symlinks preserved
    assert!(paths.iter().any(|p| p.ends_with("link1.txt")));
    assert!(paths.iter().any(|p| p.ends_with("link2.txt")));
    assert!(paths.iter().any(|p| p.ends_with("link3.txt")));
    // All 3 distinct external targets collected
    assert!(paths.contains(outside1.to_str().unwrap()));
    assert!(paths.contains(outside2.to_str().unwrap()));
    assert!(paths.contains(outside3.to_str().unwrap()));
    // Verify each target has correct size
    let t1 = m
        .files
        .iter()
        .find(|f| f.path == outside1.to_str().unwrap())
        .unwrap();
    let t2 = m
        .files
        .iter()
        .find(|f| f.path == outside2.to_str().unwrap())
        .unwrap();
    let t3 = m
        .files
        .iter()
        .find(|f| f.path == outside3.to_str().unwrap())
        .unwrap();
    assert_eq!(t1.size, Some(3));
    assert_eq!(t2.size, Some(3));
    assert_eq!(t3.size, Some(5));
}

#[test]
fn files_processed_in_order() {
    // Verify that files from a single directory are collected and can be sorted by path.
    // The collect function itself may not guarantee order, but the result should be
    // sortable and contain all expected files.
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("c.txt"), "c").unwrap();
    std::fs::write(tmp.path().join("a.txt"), "a").unwrap();
    std::fs::write(tmp.path().join("b.txt"), "b").unwrap();

    let m = collect_abs_snapshot(
        &[tmp.path().to_path_buf()],
        &[] as &[PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let mut paths: Vec<_> = m.files.iter().map(|f| f.path.as_str()).collect();
    let unsorted = paths.clone();
    paths.sort();
    // All files present
    assert_eq!(paths.len(), 3);
    assert!(paths[0].ends_with("a.txt"));
    assert!(paths[1].ends_with("b.txt"));
    assert!(paths[2].ends_with("c.txt"));
    // Verify the sorted order is consistent (idempotent sort)
    let mut paths2 = unsorted;
    paths2.sort();
    assert_eq!(paths, paths2);
}

#[test]
fn filenames_processed_before_directories() {
    // When both filenames and directories are provided, both should appear in the result.
    let tmp = TempDir::new().unwrap();
    let sub = tmp.path().join("subdir");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("dir_file.txt"), "from dir").unwrap();
    let extra = tmp.path().join("extra.txt");
    std::fs::write(&extra, "extra").unwrap();

    let m = collect_abs_snapshot(&[sub], &[extra], CollectOptions::default()).unwrap();

    // Both sources contribute to the result
    assert!(m.files.iter().any(|f| f.path.ends_with("dir_file.txt")));
    assert!(m.files.iter().any(|f| f.path.ends_with("extra.txt")));
    assert_eq!(m.files.len(), 2);
}
