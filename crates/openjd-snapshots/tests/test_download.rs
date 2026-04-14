// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.

//! Tests for download_abs_manifest ported from Python.
//!
//! Covers: basic downloading, directory creation, mtime updates, conflict resolution,
//! diff manifest deletions, validation, chunked files, symlinks, and hash cache.

use openjd_snapshots::{
    download_abs_manifest, hash_upload_abs_manifest, AbsManifest, AbsSnapshot, AbsSnapshotDiff,
    ContentAddressedDataCache, DirEntry, DownloadOptions, FileConflictResolution, FileEntry,
    FileSystemDataCache, HashAlgorithm, HashUploadOptions, Manifest, DEFAULT_FILE_CHUNK_SIZE,
};
use std::sync::Arc;
use tempfile::TempDir;

fn make_snapshot(files: Vec<FileEntry>) -> AbsSnapshot {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files)
}

fn make_diff(files: Vec<FileEntry>) -> AbsSnapshotDiff {
    Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(files)
}

fn new_data_cache(tmp: &TempDir) -> Arc<FileSystemDataCache> {
    Arc::new(FileSystemDataCache::new(tmp.path().join("data")).unwrap())
}

fn store(dc: &dyn ContentAddressedDataCache, content: &[u8]) -> String {
    let hash = openjd_snapshots::hash::hash_data(content);
    dc.put_object(&hash, "xxh128", content).unwrap();
    hash
}

fn hashed_entry(path: &str, content: &[u8], dc: &dyn ContentAddressedDataCache) -> FileEntry {
    let hash = store(dc, content);
    let mut e = FileEntry::file(path, content.len() as u64, 1000);
    e.hash = Some(hash);
    e
}

// ===== Basic downloading =====

#[test]
fn download_empty_manifest() {
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let manifest = make_snapshot(vec![]);
    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(result.statistics.downloaded_files, 0);
    assert_eq!(result.statistics.total_bytes, 0);
}

#[test]
fn download_single_file() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let dest = tmp.path().join("output.txt");

    let entry = hashed_entry(&dest.to_string_lossy(), b"hello world", &*dc);
    let manifest = make_snapshot(vec![entry]);
    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "hello world");
    assert_eq!(result.statistics.downloaded_files, 1);
    assert_eq!(result.statistics.downloaded_bytes, 11);
}

#[test]
fn download_creates_parent_dirs() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let dest = tmp.path().join("a/b/c/file.txt");

    let entry = hashed_entry(&dest.to_string_lossy(), b"nested", &*dc);
    let manifest = make_snapshot(vec![entry]);
    download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "nested");
}

#[test]
fn download_multiple_files_with_subdirs() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let p1 = tmp.path().join("file1.txt");
    let p2 = tmp.path().join("file2.txt");
    let p3 = tmp.path().join("subdir/file3.txt");

    let manifest = make_snapshot(vec![
        hashed_entry(&p1.to_string_lossy(), b"Content 1", &*dc),
        hashed_entry(&p2.to_string_lossy(), b"Content 2", &*dc),
        hashed_entry(&p3.to_string_lossy(), b"Content 3", &*dc),
    ]);
    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    assert_eq!(std::fs::read_to_string(&p1).unwrap(), "Content 1");
    assert_eq!(std::fs::read_to_string(&p2).unwrap(), "Content 2");
    assert_eq!(std::fs::read_to_string(&p3).unwrap(), "Content 3");
    assert_eq!(result.statistics.downloaded_files, 3);
}

#[test]
fn download_updates_mtime_in_manifest() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let dest = tmp.path().join("file.txt");

    let entry = hashed_entry(&dest.to_string_lossy(), b"data", &*dc);
    let manifest = make_snapshot(vec![entry]);
    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    let actual_mtime = result.manifest.files()[0].mtime.unwrap();
    assert_ne!(actual_mtime, 1000); // not the placeholder
    assert!(actual_mtime > 0);
}

#[test]
fn download_creates_manifest_dirs() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let dir_path = tmp.path().join("new_dir");

    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_dirs(vec![DirEntry::new(dir_path.to_string_lossy().to_string())]);
    download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    assert!(dir_path.is_dir());
}

// ===== Conflict resolution =====

#[test]
fn conflict_skip() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let dest = tmp.path().join("existing.txt");
    std::fs::write(&dest, b"old content").unwrap();

    let entry = hashed_entry(&dest.to_string_lossy(), b"new content", &*dc);
    let manifest = make_snapshot(vec![entry]);
    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions {
            file_conflict_resolution: FileConflictResolution::Skip,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old content");
    assert_eq!(result.statistics.skipped_files, 1);
}

#[test]
fn conflict_overwrite() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let dest = tmp.path().join("existing.txt");
    std::fs::write(&dest, b"old content").unwrap();

    let entry = hashed_entry(&dest.to_string_lossy(), b"new content", &*dc);
    let manifest = make_snapshot(vec![entry]);
    download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "new content");
}

#[test]
fn conflict_create_copy() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let dest = tmp.path().join("file.txt");
    std::fs::write(&dest, b"old").unwrap();

    let entry = hashed_entry(&dest.to_string_lossy(), b"new", &*dc);
    let manifest = make_snapshot(vec![entry]);
    download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions {
            file_conflict_resolution: FileConflictResolution::CreateCopy,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old");
    let copy = tmp.path().join("file (1).txt");
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), "new");
}

// ===== Diff manifest deletions =====

#[test]
fn diff_deletes_files() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let file_to_delete = tmp.path().join("delete_me.txt");
    std::fs::write(&file_to_delete, b"bye").unwrap();
    let file_to_keep = tmp.path().join("keep_me.txt");
    std::fs::write(&file_to_keep, b"keep").unwrap();

    let manifest = make_diff(vec![FileEntry::deleted(
        file_to_delete.to_string_lossy().to_string(),
    )]);
    download_abs_manifest(
        &AbsManifest::Diff(manifest),
        dc.clone(),
        DownloadOptions {
            apply_deletes: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!file_to_delete.exists());
    assert!(file_to_keep.exists());
}

#[test]
fn diff_apply_deletes_false_skips_deletions() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let file_to_delete = tmp.path().join("delete_me.txt");
    std::fs::write(&file_to_delete, b"still here").unwrap();

    let manifest = make_diff(vec![FileEntry::deleted(
        file_to_delete.to_string_lossy().to_string(),
    )]);
    download_abs_manifest(
        &AbsManifest::Diff(manifest),
        dc.clone(),
        DownloadOptions {
            apply_deletes: false,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(file_to_delete.exists());
}

#[test]
fn diff_deletes_empty_directory() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let dir_to_delete = tmp.path().join("empty_dir");
    std::fs::create_dir(&dir_to_delete).unwrap();

    let manifest: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_dirs(vec![DirEntry {
            path: dir_to_delete.to_string_lossy().to_string(),
            deleted: true,
        }]);
    download_abs_manifest(
        &AbsManifest::Diff(manifest),
        dc.clone(),
        DownloadOptions {
            apply_deletes: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!dir_to_delete.exists());
}

#[test]
fn non_empty_directory_not_deleted() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let dir = tmp.path().join("non_empty");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("file.txt"), b"content").unwrap();

    let manifest: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_dirs(vec![DirEntry {
            path: dir.to_string_lossy().to_string(),
            deleted: true,
        }]);
    download_abs_manifest(
        &AbsManifest::Diff(manifest),
        dc.clone(),
        DownloadOptions {
            apply_deletes: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(dir.exists());
    assert!(dir.join("file.txt").exists());
}

#[test]
fn deletion_order_children_before_parents() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let parent = tmp.path().join("parent");
    let child = parent.join("child");
    std::fs::create_dir_all(&child).unwrap();
    let file_in_child = child.join("file.txt");
    std::fs::write(&file_in_child, b"content").unwrap();

    let manifest: AbsSnapshotDiff = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
        .with_files(vec![FileEntry::deleted(
            file_in_child.to_string_lossy().to_string(),
        )])
        .with_dirs(vec![
            DirEntry {
                path: parent.to_string_lossy().to_string(),
                deleted: true,
            },
            DirEntry {
                path: child.to_string_lossy().to_string(),
                deleted: true,
            },
        ]);
    download_abs_manifest(
        &AbsManifest::Diff(manifest),
        dc.clone(),
        DownloadOptions {
            apply_deletes: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!file_in_child.exists());
    assert!(!child.exists());
    assert!(!parent.exists());
}

// ===== Validation =====

#[test]
fn rejects_file_with_no_hash() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let dest = tmp.path().join("file.txt");

    // File entry with no hash and no chunk_hashes
    let entry = FileEntry::file(dest.to_string_lossy().to_string(), 100, 1000);
    let manifest = make_snapshot(vec![entry]);
    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    );
    match result {
        Err(e) => assert!(e.to_string().contains("no hash"), "unexpected error: {e}"),
        Ok(_) => panic!("expected error for file with no hash"),
    }
}

// ===== Chunked download =====

#[test]
fn download_chunked_file() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let chunks: Vec<&[u8]> = vec![b"aaa", b"bbb", b"ccc", b"ddd"];
    let chunk_hashes: Vec<String> = chunks
        .iter()
        .map(|c| {
            let h = openjd_snapshots::hash::hash_data(c);
            dc.put_object(&h, "xxh128", c).unwrap();
            h
        })
        .collect();

    let dest = tmp.path().join("chunked.bin");
    let mut entry = FileEntry::file(dest.to_string_lossy().to_string(), 12, 1000);
    entry.chunk_hashes = Some(chunk_hashes);

    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, 3).with_files(vec![entry]);
    download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    assert_eq!(std::fs::read(&dest).unwrap(), b"aaabbbcccddd");
}

// ===== Symlink download =====

#[cfg(unix)]
#[test]
fn download_creates_symlink() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let target = tmp.path().join("target.txt");
    std::fs::write(&target, b"target content").unwrap();
    let link = tmp.path().join("link.txt");

    let manifest = make_snapshot(vec![FileEntry::symlink(
        link.to_string_lossy().to_string(),
        target.to_string_lossy().to_string(),
    )]);
    download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read_to_string(&link).unwrap(), "target content");
}

// ===== Round trip =====

#[test]
fn round_trip_upload_then_download() {
    let src_dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();

    // Create source files
    std::fs::write(src_dir.path().join("hello.txt"), b"hello world").unwrap();
    std::fs::create_dir(src_dir.path().join("sub")).unwrap();
    std::fs::write(src_dir.path().join("sub/data.bin"), b"binary data").unwrap();

    let dc = new_data_cache(&cache_dir);

    // Upload
    let collect_result = openjd_snapshots::collect_abs_snapshot(
        &[src_dir.path().to_path_buf()],
        &[] as &[std::path::PathBuf],
        openjd_snapshots::CollectOptions::default(),
    )
    .unwrap();
    let upload_result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(collect_result),
        dc.clone(),
        HashUploadOptions::default(),
    )
    .unwrap();

    // Remap paths to destination
    let abs_snap = match upload_result.manifest {
        AbsManifest::Snapshot(s) => s,
        _ => panic!("expected snapshot"),
    };
    let rel = openjd_snapshots::subtree_snapshot(
        &abs_snap,
        &src_dir.path().to_string_lossy(),
        openjd_snapshots::SymlinkPolicy::ExcludeAll,
    )
    .unwrap();
    let dl_manifest =
        openjd_snapshots::join_snapshot(&rel, &dst_dir.path().to_string_lossy()).unwrap();

    // Download
    download_abs_manifest(
        &AbsManifest::Snapshot(dl_manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(dst_dir.path().join("hello.txt")).unwrap(),
        "hello world"
    );
    assert_eq!(
        std::fs::read(dst_dir.path().join("sub/data.bin")).unwrap(),
        b"binary data"
    );
}

// ===== Hash cache skip =====

#[test]
fn hash_cache_skip_on_second_download() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let hc_dir = TempDir::new().unwrap();
    let hc = Arc::new(openjd_snapshots::HashCache::new(hc_dir.path()).unwrap());

    let dest = tmp.path().join("file.txt");
    let entry = hashed_entry(&dest.to_string_lossy(), b"hello", &*dc);
    let manifest = make_snapshot(vec![entry]);

    // First download
    let r1 = download_abs_manifest(
        &AbsManifest::Snapshot(manifest.clone()),
        dc.clone(),
        DownloadOptions {
            hash_cache: Some(hc.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r1.statistics.downloaded_files, 1);
    assert_eq!(r1.statistics.skipped_files, 0);

    // Second download - should skip
    let r2 = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions {
            hash_cache: Some(hc),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r2.statistics.downloaded_files, 0);
    assert_eq!(r2.statistics.skipped_files, 1);
}

#[test]
fn hash_cache_stale_mtime_redownloads() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let hc_dir = TempDir::new().unwrap();
    let hc = Arc::new(openjd_snapshots::HashCache::new(hc_dir.path()).unwrap());

    let dest = tmp.path().join("file.txt");
    let entry = hashed_entry(&dest.to_string_lossy(), b"original", &*dc);
    let manifest = make_snapshot(vec![entry]);

    // First download
    download_abs_manifest(
        &AbsManifest::Snapshot(manifest.clone()),
        dc.clone(),
        DownloadOptions {
            hash_cache: Some(hc.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "original");

    // Modify file (changes mtime) - sleep to ensure mtime differs
    std::thread::sleep(std::time::Duration::from_millis(50));
    std::fs::write(&dest, "modified").unwrap();

    // Second download - mtime changed, should re-download
    let r2 = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions {
            hash_cache: Some(hc),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r2.statistics.downloaded_files, 1);
    assert_eq!(r2.statistics.skipped_files, 0);
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "original");
}

#[test]
fn hash_cache_deleted_file_redownloads() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let hc_dir = TempDir::new().unwrap();
    let hc = Arc::new(openjd_snapshots::HashCache::new(hc_dir.path()).unwrap());

    let dest = tmp.path().join("file.txt");
    let entry = hashed_entry(&dest.to_string_lossy(), b"content", &*dc);
    let manifest = make_snapshot(vec![entry]);

    download_abs_manifest(
        &AbsManifest::Snapshot(manifest.clone()),
        dc.clone(),
        DownloadOptions {
            hash_cache: Some(hc.clone()),
            ..Default::default()
        },
    )
    .unwrap();

    std::fs::remove_file(&dest).unwrap();

    let r2 = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions {
            hash_cache: Some(hc),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r2.statistics.downloaded_files, 1);
    assert!(dest.exists());
}

#[test]
fn without_hash_cache_always_downloads() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let dest = tmp.path().join("file.txt");
    let entry = hashed_entry(&dest.to_string_lossy(), b"content", &*dc);
    let manifest = make_snapshot(vec![entry]);

    let r1 = download_abs_manifest(
        &AbsManifest::Snapshot(manifest.clone()),
        dc.clone(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(r1.statistics.downloaded_files, 1);

    let r2 = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(r2.statistics.downloaded_files, 1);
}

// ===== Atomic writes =====

#[test]
fn no_temp_files_after_success() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let dl_dir = tmp.path().join("download");
    std::fs::create_dir_all(&dl_dir).unwrap();

    let p1 = dl_dir.join("file1.txt");
    let p2 = dl_dir.join("file2.txt");
    let manifest = make_snapshot(vec![
        hashed_entry(&p1.to_string_lossy(), b"Content 1", &*dc),
        hashed_entry(&p2.to_string_lossy(), b"Content 2", &*dc),
    ]);

    download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    let tmp_files: Vec<_> = std::fs::read_dir(&dl_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains("tmp"))
        .collect();
    assert!(tmp_files.is_empty(), "temp files left: {:?}", tmp_files);
}

#[test]
fn atomic_write_produces_correct_content() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let dest = tmp.path().join("atomic.txt");

    let entry = hashed_entry(&dest.to_string_lossy(), b"atomic content", &*dc);
    let manifest = make_snapshot(vec![entry]);

    download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "atomic content");
}

// ===== Chunked file hash cache =====

#[test]
fn chunked_download_round_trip_with_hash_cache() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let hc_dir = TempDir::new().unwrap();
    let hc = Arc::new(openjd_snapshots::HashCache::new(hc_dir.path()).unwrap());

    let chunks: Vec<&[u8]> = vec![b"AAAA", b"BBBB", b"CC"];
    let chunk_hashes: Vec<String> = chunks
        .iter()
        .map(|c| {
            let h = openjd_snapshots::hash::hash_data(c);
            dc.put_object(&h, "xxh128", c).unwrap();
            h
        })
        .collect();

    let dest = tmp.path().join("chunked.bin");
    let mut entry = FileEntry::file(dest.to_string_lossy().to_string(), 10, 1000);
    entry.chunk_hashes = Some(chunk_hashes);

    let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, 4).with_files(vec![entry]);

    // First download
    let r1 = download_abs_manifest(
        &AbsManifest::Snapshot(manifest.clone()),
        dc.clone(),
        DownloadOptions {
            hash_cache: Some(hc.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r1.statistics.downloaded_files, 1);
    assert_eq!(std::fs::read(&dest).unwrap(), b"AAAABBBBCC");

    // Second download - file exists with same content, chunked files
    // now use per-chunk hash cache entries, so they get skipped
    let r2 = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions {
            hash_cache: Some(hc),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r2.statistics.skipped_files, 1);
    assert_eq!(r2.statistics.downloaded_files, 0);
}

use std::sync::atomic::{AtomicUsize, Ordering};

// ===== Progress callback and parallelism tests =====

#[test]
fn download_progress_callback_invoked() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let p1 = tmp.path().join("a.txt");
    let p2 = tmp.path().join("b.txt");
    let manifest = make_snapshot(vec![
        hashed_entry(&p1.to_string_lossy(), b"aaa", &*dc),
        hashed_entry(&p2.to_string_lossy(), b"bbb", &*dc),
    ]);

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = call_count.clone();

    let _ = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions {
            on_progress: Some(Box::new(move |_stats| {
                cc.fetch_add(1, Ordering::Relaxed);
                true
            })),
            ..Default::default()
        },
    )
    .unwrap();

    assert!(call_count.load(Ordering::Relaxed) >= 1);
}

#[test]
fn download_progress_callback_cancel() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let mut files = Vec::new();
    for i in 0..20 {
        let p = tmp.path().join(format!("f{i}.txt"));
        files.push(hashed_entry(&p.to_string_lossy(), b"data", &*dc));
    }

    let manifest = make_snapshot(files);
    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions {
            on_progress: Some(Box::new(|_| false)),
            max_workers: Some(1),
            ..Default::default()
        },
    );

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cancelled"));
}

#[test]
fn parallel_download_produces_same_results() {
    let tmp1 = TempDir::new().unwrap();
    let tmp2 = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let mut files_seq = Vec::new();
    let mut files_par = Vec::new();
    for i in 0..10 {
        let content = format!("content_{i}");
        let p1 = tmp1.path().join(format!("f{i}.txt"));
        let p2 = tmp2.path().join(format!("f{i}.txt"));
        let hash = store(&*dc, content.as_bytes());
        let mut e1 = FileEntry::file(p1.to_string_lossy().to_string(), content.len() as u64, 1000);
        e1.hash = Some(hash.clone());
        let mut e2 = FileEntry::file(p2.to_string_lossy().to_string(), content.len() as u64, 1000);
        e2.hash = Some(hash);
        files_seq.push(e1);
        files_par.push(e2);
    }

    let m_seq = make_snapshot(files_seq);
    let m_par = make_snapshot(files_par);

    download_abs_manifest(
        &AbsManifest::Snapshot(m_seq),
        dc.clone(),
        DownloadOptions {
            max_workers: Some(1),
            ..Default::default()
        },
    )
    .unwrap();

    download_abs_manifest(
        &AbsManifest::Snapshot(m_par),
        dc.clone(),
        DownloadOptions {
            max_workers: Some(4),
            ..Default::default()
        },
    )
    .unwrap();

    for i in 0..10 {
        let content = format!("content_{i}");
        let f1 = tmp1.path().join(format!("f{i}.txt"));
        let f2 = tmp2.path().join(format!("f{i}.txt"));
        assert_eq!(std::fs::read_to_string(&f1).unwrap(), content);
        assert_eq!(std::fs::read_to_string(&f2).unwrap(), content);
    }
}

// ---------------------------------------------------------------------------
// Progress metadata tests
// ---------------------------------------------------------------------------

#[test]
fn download_progress_fields_populated() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let dest = tmp.path().join("out.txt");
    let manifest = make_snapshot(vec![hashed_entry(&dest.to_string_lossy(), b"hello", &*dc)]);
    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions::default(),
    )
    .unwrap();
    assert!(result.statistics.total_time > 0.0);
    assert!(result.statistics.rate >= 0.0);
    assert!((result.statistics.progress - 100.0).abs() < 0.01);
}

#[test]
fn download_progress_zero_for_empty_manifest() {
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let manifest = make_snapshot(vec![]);
    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions::default(),
    )
    .unwrap();
    assert_eq!(result.statistics.total_files, 0);
    assert_eq!(result.statistics.progress, 0.0);
}

#[test]
fn download_progress_callback_receives_timing() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let mut files = Vec::new();
    for i in 0..5 {
        let p = tmp.path().join(format!("f{i}.txt"));
        files.push(hashed_entry(
            &p.to_string_lossy(),
            format!("content{i}").repeat(100).as_bytes(),
            &*dc,
        ));
    }
    let manifest = make_snapshot(files);
    let times = Arc::new(std::sync::Mutex::new(Vec::new()));
    let t = times.clone();
    let _ = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions {
            on_progress: Some(Box::new(move |stats| {
                t.lock().unwrap().push(stats.total_time);
                true
            })),
            ..Default::default()
        },
    )
    .unwrap();
    let t = times.lock().unwrap();
    assert!(!t.is_empty());
    for i in 1..t.len() {
        assert!(t[i] >= t[i - 1], "total_time not monotonic");
    }
}

#[test]
fn download_progress_rate_positive() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let dest = tmp.path().join("big.txt");
    let manifest = make_snapshot(vec![hashed_entry(
        &dest.to_string_lossy(),
        &vec![0u8; 10000],
        &*dc,
    )]);
    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions::default(),
    )
    .unwrap();
    assert!(result.statistics.rate > 0.0);
}

// ===== Progress message tests =====

#[test]
fn download_progress_message_contains_rate() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let dest = tmp.path().join("out.txt");
    let manifest = make_snapshot(vec![hashed_entry(
        &dest.to_string_lossy(),
        b"rate test",
        &*dc,
    )]);
    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions::default(),
    )
    .unwrap();
    assert!(
        result.statistics.progress_message.contains("/s)"),
        "message: {}",
        result.statistics.progress_message
    );
}

#[test]
fn download_progress_message_contains_elapsed_time() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let dest = tmp.path().join("out.txt");
    let manifest = make_snapshot(vec![hashed_entry(
        &dest.to_string_lossy(),
        b"time test",
        &*dc,
    )]);
    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions::default(),
    )
    .unwrap();
    // Message should contain "in X.XXs"
    assert!(
        result.statistics.progress_message.contains("in "),
        "message: {}",
        result.statistics.progress_message
    );
    assert!(
        result.statistics.progress_message.contains("s"),
        "message: {}",
        result.statistics.progress_message
    );
}

#[test]
fn download_final_statistics_transfer_rate_calculation() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let dest = tmp.path().join("out.txt");
    let content = vec![0u8; 10000];
    let manifest = make_snapshot(vec![hashed_entry(&dest.to_string_lossy(), &content, &*dc)]);
    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions::default(),
    )
    .unwrap();
    // rate should be approximately total_bytes / total_time
    let expected = result.statistics.total_bytes as f64 / result.statistics.total_time;
    assert!(
        (result.statistics.rate - expected).abs() < 1.0,
        "rate {} != expected {}",
        result.statistics.rate,
        expected
    );
}

// ===== Symlink policy and delete edge cases =====

#[cfg(unix)]
#[test]
fn download_excludes_symlinks_with_exclude_policy() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let file_path = tmp.path().join("file.txt");
    let link_path = tmp.path().join("link.txt");

    let manifest = make_snapshot(vec![
        hashed_entry(&file_path.to_string_lossy(), b"real file", &*dc),
        FileEntry::symlink(
            link_path.to_string_lossy().to_string(),
            file_path.to_string_lossy().to_string(),
        ),
    ]);
    download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions {
            symlink_policy: openjd_snapshots::SymlinkPolicy::ExcludeAll,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(file_path.exists());
    assert!(!link_path.exists());
}

#[cfg(unix)]
#[test]
fn diff_deletes_symlink() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let target = tmp.path().join("target.txt");
    std::fs::write(&target, b"target content").unwrap();
    let link = tmp.path().join("link.txt");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let manifest = make_diff(vec![FileEntry::deleted(link.to_string_lossy().to_string())]);
    download_abs_manifest(
        &AbsManifest::Diff(manifest),
        dc.clone(),
        DownloadOptions {
            apply_deletes: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!link.exists());
    assert!(target.exists());
}

#[test]
fn snapshot_manifest_ignores_apply_deletes() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let existing = tmp.path().join("existing.txt");
    std::fs::write(&existing, b"keep me").unwrap();

    let manifest = make_snapshot(vec![]);
    download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions {
            apply_deletes: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(existing.exists());
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "keep me");
}

// ===== Chunked download tests =====

fn chunked_entry(
    path: &str,
    chunks: &[&[u8]],
    chunk_size: i64,
    dc: &dyn ContentAddressedDataCache,
) -> (FileEntry, AbsSnapshot) {
    let total: u64 = chunks.iter().map(|c| c.len() as u64).sum();
    let chunk_hashes: Vec<String> = chunks
        .iter()
        .map(|c| {
            let h = openjd_snapshots::hash::hash_data(c);
            dc.put_object(&h, "xxh128", c).unwrap();
            h
        })
        .collect();
    let mut entry = FileEntry::file(path, total, 1000);
    entry.chunk_hashes = Some(chunk_hashes);
    let manifest: AbsSnapshot =
        Manifest::new(HashAlgorithm::Xxh128, chunk_size).with_files(vec![entry.clone()]);
    (entry, manifest)
}

#[test]
fn download_mixed_regular_and_chunked_files() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let regular_path = tmp.path().join("regular.txt");
    let regular = hashed_entry(&regular_path.to_string_lossy(), b"whole file", &*dc);

    let chunked_path = tmp.path().join("chunked.bin");
    let chunks: &[&[u8]] = &[b"AA", b"BB", b"CC"];
    let chunk_hashes: Vec<String> = chunks
        .iter()
        .map(|c| {
            let h = openjd_snapshots::hash::hash_data(c);
            dc.put_object(&h, "xxh128", c).unwrap();
            h
        })
        .collect();
    let mut chunked = FileEntry::file(chunked_path.to_string_lossy().to_string(), 6, 1000);
    chunked.chunk_hashes = Some(chunk_hashes);

    let manifest: AbsSnapshot =
        Manifest::new(HashAlgorithm::Xxh128, 2).with_files(vec![regular, chunked]);
    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(&regular_path).unwrap(),
        "whole file"
    );
    assert_eq!(std::fs::read(&chunked_path).unwrap(), b"AABBCC");
    assert_eq!(result.statistics.downloaded_files, 2);
}

#[test]
fn download_chunked_file_conflict_skip() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let dest = tmp.path().join("chunked.bin");
    std::fs::write(&dest, b"old data").unwrap();

    let (_, manifest) = chunked_entry(&dest.to_string_lossy(), &[b"new", b"dat"], 3, &*dc);

    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions {
            file_conflict_resolution: FileConflictResolution::Skip,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old data");
    assert_eq!(result.statistics.skipped_files, 1);
}

#[test]
fn download_chunked_file_conflict_overwrite() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let dest = tmp.path().join("chunked.bin");
    std::fs::write(&dest, b"old data").unwrap();

    let (_, manifest) = chunked_entry(&dest.to_string_lossy(), &[b"new", b"dat"], 3, &*dc);

    download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    assert_eq!(std::fs::read(&dest).unwrap(), b"newdat");
}

#[test]
fn download_chunked_file_preserves_mtime() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let dest = tmp.path().join("chunked.bin");
    let (_, manifest) = chunked_entry(&dest.to_string_lossy(), &[b"XX", b"YY"], 2, &*dc);

    let result = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        Default::default(),
    )
    .unwrap();

    let actual_mtime = result.manifest.files()[0].mtime.unwrap();
    assert_ne!(actual_mtime, 1000);
    assert!(actual_mtime > 0);
}

// ===== Chunked hash cache tests =====

#[test]
fn chunked_file_downloaded_when_hash_mismatch() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let hc_dir = TempDir::new().unwrap();
    let hc = Arc::new(openjd_snapshots::HashCache::new(hc_dir.path()).unwrap());

    let dest = tmp.path().join("chunked.bin");

    // First download with old content
    let (_, manifest_old) = chunked_entry(&dest.to_string_lossy(), &[b"old", b"dat"], 3, &*dc);
    download_abs_manifest(
        &AbsManifest::Snapshot(manifest_old),
        dc.clone(),
        DownloadOptions {
            hash_cache: Some(hc.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"olddat");

    // Now create a new manifest with different chunk content
    let (_, manifest_new) = chunked_entry(&dest.to_string_lossy(), &[b"NEW", b"DAT"], 3, &*dc);

    let r = download_abs_manifest(
        &AbsManifest::Snapshot(manifest_new),
        dc.clone(),
        DownloadOptions {
            hash_cache: Some(hc),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(r.statistics.downloaded_files, 1);
    assert_eq!(std::fs::read(&dest).unwrap(), b"NEWDAT");
}

#[test]
fn hash_cache_updated_after_chunked_download() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let hc_dir = TempDir::new().unwrap();
    let hc = Arc::new(openjd_snapshots::HashCache::new(hc_dir.path()).unwrap());

    let dest = tmp.path().join("chunked.bin");
    let (_, manifest) = chunked_entry(&dest.to_string_lossy(), &[b"AA", b"BB"], 2, &*dc);

    // First download populates hash cache
    let r1 = download_abs_manifest(
        &AbsManifest::Snapshot(manifest.clone()),
        dc.clone(),
        DownloadOptions {
            hash_cache: Some(hc.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r1.statistics.downloaded_files, 1);

    // Second download should skip via hash cache
    let r2 = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions {
            hash_cache: Some(hc),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r2.statistics.skipped_files, 1);
    assert_eq!(r2.statistics.downloaded_files, 0);
}

#[test]
fn second_download_skips_chunked_file() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let hc_dir = TempDir::new().unwrap();
    let hc = Arc::new(openjd_snapshots::HashCache::new(hc_dir.path()).unwrap());

    let dest = tmp.path().join("chunked.bin");
    let (_, manifest) = chunked_entry(&dest.to_string_lossy(), &[b"abc", b"def", b"gh"], 3, &*dc);

    download_abs_manifest(
        &AbsManifest::Snapshot(manifest.clone()),
        dc.clone(),
        DownloadOptions {
            hash_cache: Some(hc.clone()),
            ..Default::default()
        },
    )
    .unwrap();

    let content_after_first = std::fs::read(&dest).unwrap();
    assert_eq!(content_after_first, b"abcdefgh");

    let r2 = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions {
            hash_cache: Some(hc),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(r2.statistics.downloaded_files, 0);
    assert_eq!(r2.statistics.skipped_files, 1);
    // File content unchanged
    assert_eq!(std::fs::read(&dest).unwrap(), b"abcdefgh");
}

// ===== Validation tests =====

#[test]
fn download_relative_path_raises_error() {
    // AbsSnapshot (Manifest<Abs, Full>) validates paths via validate().
    // A relative path in an absolute manifest should be rejected.
    let entry = FileEntry::file("relative/path.txt", 5, 1000);
    let manifest: AbsSnapshot =
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![entry]);
    let result = manifest.validate();
    assert!(result.is_err(), "expected error for relative path");
    assert!(
        result.unwrap_err().to_string().contains("absolute"),
        "error should mention absolute path requirement"
    );
}

// ===== Progress tests =====

#[test]
fn download_progress_with_hash_cache_hits() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);
    let hc_dir = TempDir::new().unwrap();
    let hc = Arc::new(openjd_snapshots::HashCache::new(hc_dir.path()).unwrap());

    let dest = tmp.path().join("file.txt");
    let entry = hashed_entry(&dest.to_string_lossy(), b"cached content", &*dc);
    let manifest = make_snapshot(vec![entry]);

    // First download to populate cache
    download_abs_manifest(
        &AbsManifest::Snapshot(manifest.clone()),
        dc.clone(),
        DownloadOptions {
            hash_cache: Some(hc.clone()),
            ..Default::default()
        },
    )
    .unwrap();

    // Second download — should be a cache hit (skipped)
    let r2 = download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions {
            hash_cache: Some(hc),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(r2.statistics.skipped_files, 1);
    assert_eq!(r2.statistics.downloaded_files, 0);
    assert_eq!(r2.statistics.skipped_bytes, 14);
}

#[test]
fn download_progress_total_time_increases_monotonically() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dc = new_data_cache(&cache_dir);

    let mut files = Vec::new();
    for i in 0..10 {
        let p = tmp.path().join(format!("f{i}.txt"));
        files.push(hashed_entry(
            &p.to_string_lossy(),
            format!("data_{i}").repeat(50).as_bytes(),
            &*dc,
        ));
    }
    let manifest = make_snapshot(files);

    let times = Arc::new(std::sync::Mutex::new(Vec::new()));
    let t = times.clone();

    download_abs_manifest(
        &AbsManifest::Snapshot(manifest),
        dc.clone(),
        DownloadOptions {
            on_progress: Some(Box::new(move |stats| {
                t.lock().unwrap().push(stats.total_time);
                true
            })),
            max_workers: Some(1),
            ..Default::default()
        },
    )
    .unwrap();

    let t = times.lock().unwrap();
    assert!(!t.is_empty(), "progress callback should have been called");
    for i in 1..t.len() {
        assert!(
            t[i] >= t[i - 1],
            "total_time not monotonically increasing: {} < {}",
            t[i],
            t[i - 1]
        );
    }
}
