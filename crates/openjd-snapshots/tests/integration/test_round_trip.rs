// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use openjd_snapshots::{
    collect_abs_snapshot, download_abs_manifest, hash_upload_abs_manifest, join_snapshot,
    subtree_snapshot, AbsManifest, AsyncDataCache, CollectOptions, DownloadOptions, FileEntry,
    FileSystemDataCache, HashAlgorithm, HashCache, HashUploadOptions, Manifest, SymlinkPolicy,
    DEFAULT_FILE_CHUNK_SIZE,
};
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

/// Returns true if the OS supports creating symlinks.
/// On Unix this is always true. On Windows it requires Developer Mode or elevated privileges.
fn symlinks_supported() -> bool {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    std::fs::write(&target, b"").unwrap();
    let link = dir.path().join("link");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target, &link).is_ok()
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(&target, &link).is_ok()
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

fn create_test_tree(dir: &Path) {
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("hello.txt"), b"hello world").unwrap();
    std::fs::write(dir.join("sub/data.bin"), b"binary data here").unwrap();
    std::fs::write(dir.join("empty.txt"), b"").unwrap();
}

async fn upload_and_remap(
    src_dir: &Path,
    dst_dir: &Path,
    data_cache: &Arc<FileSystemDataCache>,
) -> openjd_snapshots::AbsSnapshot {
    let snapshot = collect_abs_snapshot(
        std::slice::from_ref(&src_dir),
        &[] as &[std::path::PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let upload_result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(snapshot),
        data_cache.clone(),
        HashUploadOptions::default(),
    )
    .await
    .unwrap();

    let abs_snap = match upload_result.manifest {
        AbsManifest::Snapshot(s) => s,
        _ => panic!("expected snapshot"),
    };

    let rel = subtree_snapshot(
        &abs_snap,
        &src_dir.to_string_lossy(),
        SymlinkPolicy::ExcludeAll,
    )
    .unwrap();
    join_snapshot(&rel, &dst_dir.to_string_lossy()).unwrap()
}

#[tokio::test]
async fn round_trip_collect_upload_download() {
    let src_dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();

    create_test_tree(src_dir.path());

    let data_cache = Arc::new(FileSystemDataCache::new(cache_dir.path().join("data")).unwrap());
    let abs_dl = upload_and_remap(src_dir.path(), dst_dir.path(), &data_cache).await;

    let download_result = download_abs_manifest(
        &AbsManifest::Snapshot(abs_dl),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(dst_dir.path().join("hello.txt")).unwrap(),
        "hello world"
    );
    assert_eq!(
        std::fs::read(dst_dir.path().join("sub/data.bin")).unwrap(),
        b"binary data here"
    );
    assert_eq!(
        std::fs::read(dst_dir.path().join("empty.txt")).unwrap(),
        b""
    );
    assert_eq!(download_result.statistics.downloaded_files, 3);
}

#[tokio::test]
async fn upload_skip_on_second_run() {
    let src_dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();

    std::fs::write(src_dir.path().join("a.txt"), b"aaa").unwrap();
    std::fs::write(src_dir.path().join("b.txt"), b"bbb").unwrap();

    let snapshot = collect_abs_snapshot(
        &[src_dir.path().to_path_buf()],
        &[] as &[std::path::PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let data_cache = Arc::new(FileSystemDataCache::new(cache_dir.path().join("data")).unwrap());

    let r1 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(snapshot.clone()),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        HashUploadOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(r1.statistics.uploaded_files, 2);

    let r2 = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(snapshot),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        HashUploadOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(r2.statistics.uploaded_files, 0);
    assert_eq!(r2.statistics.skipped_files, 2);
}

#[tokio::test]
async fn download_skip_with_hash_cache() {
    let src_dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let hc_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();

    std::fs::write(src_dir.path().join("file.txt"), b"content").unwrap();

    let data_cache = Arc::new(FileSystemDataCache::new(cache_dir.path().join("data")).unwrap());
    let hash_cache = Arc::new(HashCache::new(hc_dir.path()).unwrap());
    let abs_dl = upload_and_remap(src_dir.path(), dst_dir.path(), &data_cache).await;

    // First download
    let r1 = download_abs_manifest(
        &AbsManifest::Snapshot(abs_dl.clone()),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions {
            hash_cache: Some(hash_cache.clone()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(r1.statistics.downloaded_files, 1);

    // Second download - hash cache hit, should skip
    let r2 = download_abs_manifest(
        &AbsManifest::Snapshot(abs_dl),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions {
            hash_cache: Some(hash_cache),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(r2.statistics.skipped_files, 1);
    assert_eq!(r2.statistics.downloaded_files, 0);
}

#[tokio::test]
async fn chunked_round_trip() {
    let src_dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();

    let data = (0..1024u16).map(|i| (i % 256) as u8).collect::<Vec<_>>();
    std::fs::write(src_dir.path().join("big.bin"), &data).unwrap();

    let chunk_size = 256i64;
    let snapshot = collect_abs_snapshot(
        &[src_dir.path().to_path_buf()],
        &[] as &[std::path::PathBuf],
        CollectOptions {
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .unwrap();

    let data_cache = Arc::new(FileSystemDataCache::new(cache_dir.path().join("data")).unwrap());
    let upload_result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(snapshot),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        HashUploadOptions {
            file_chunk_size_bytes: Some(chunk_size),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let abs_snap = match upload_result.manifest {
        AbsManifest::Snapshot(s) => s,
        _ => panic!(),
    };
    assert!(abs_snap.files[0].chunk_hashes.is_some());
    assert_eq!(abs_snap.files[0].chunk_hashes.as_ref().unwrap().len(), 4);

    let rel = subtree_snapshot(
        &abs_snap,
        &src_dir.path().to_string_lossy(),
        SymlinkPolicy::ExcludeAll,
    )
    .unwrap();
    let abs_dl = join_snapshot(&rel, &dst_dir.path().to_string_lossy()).unwrap();

    download_abs_manifest(
        &AbsManifest::Snapshot(abs_dl),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(std::fs::read(dst_dir.path().join("big.bin")).unwrap(), data);
}

#[tokio::test]
async fn delete_via_diff_manifest() {
    let cache_dir = TempDir::new().unwrap();
    let work_dir = TempDir::new().unwrap();

    std::fs::write(work_dir.path().join("keep.txt"), b"keep").unwrap();
    std::fs::write(work_dir.path().join("remove.txt"), b"gone").unwrap();
    std::fs::create_dir(work_dir.path().join("empty_dir")).unwrap();

    let data_cache = Arc::new(FileSystemDataCache::new(cache_dir.path().join("data")).unwrap());

    let keep_hash = {
        let h = openjd_snapshots::hash::hash_data(b"keep");
        openjd_snapshots::AsyncDataCache::put_object(&*data_cache, &h, "xxh128", b"keep".to_vec())
            .await
            .unwrap();
        h
    };

    let keep_path = work_dir.path().join("keep.txt");
    let remove_path = work_dir.path().join("remove.txt");

    let mut keep_entry = FileEntry::file(keep_path.to_string_lossy().to_string(), 4, 1000);
    keep_entry.hash = Some(keep_hash);

    let diff: openjd_snapshots::AbsSnapshotDiff =
        Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(vec![
                keep_entry,
                FileEntry::deleted(remove_path.to_string_lossy().to_string()),
            ])
            .with_dirs(vec![openjd_snapshots::DirEntry {
                path: work_dir
                    .path()
                    .join("empty_dir")
                    .to_string_lossy()
                    .to_string(),
                deleted: true,
            }]);

    download_abs_manifest(
        &AbsManifest::Diff(diff),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions {
            apply_deletes: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert!(keep_path.exists());
    assert!(!remove_path.exists());
    assert!(!work_dir.path().join("empty_dir").exists());
}

#[tokio::test]
async fn statistics_are_accurate() {
    let src_dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();

    std::fs::write(src_dir.path().join("a.txt"), b"aaaa").unwrap();
    std::fs::write(src_dir.path().join("b.txt"), b"bb").unwrap();

    let snapshot = collect_abs_snapshot(
        &[src_dir.path().to_path_buf()],
        &[] as &[std::path::PathBuf],
        CollectOptions::default(),
    )
    .unwrap();

    let data_cache = Arc::new(FileSystemDataCache::new(cache_dir.path().join("data")).unwrap());
    let result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(snapshot),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        HashUploadOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(result.statistics.total_files, 2);
    assert_eq!(result.statistics.total_bytes, 6);
    assert_eq!(result.statistics.hashed_files, 2);
    assert_eq!(result.statistics.hashed_bytes, 6);
    assert_eq!(result.statistics.uploaded_files, 2);
    assert_eq!(result.statistics.uploaded_bytes, 6);
    assert_eq!(result.statistics.skipped_files, 0);
}

#[tokio::test]
async fn round_trip_with_symlinks() {
    if !symlinks_supported() {
        eprintln!("skipping: symlinks not supported on this OS/configuration");
        return;
    }

    let src_dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();

    // Create source tree with a regular file and a symlink to it
    std::fs::write(src_dir.path().join("real.txt"), b"real content").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        src_dir.path().join("real.txt"),
        src_dir.path().join("link.txt"),
    )
    .unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(
        src_dir.path().join("real.txt"),
        src_dir.path().join("link.txt"),
    )
    .unwrap();

    // Collect with CollapseEscaping policy — non-escaping symlinks stay as symlinks
    let snapshot = collect_abs_snapshot(
        &[src_dir.path().to_path_buf()],
        &[] as &[std::path::PathBuf],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            ..Default::default()
        },
    )
    .unwrap();

    // Verify collect found the file and preserved the symlink
    let real = snapshot
        .files
        .iter()
        .find(|f| f.path.ends_with("real.txt"))
        .unwrap();
    let link = snapshot
        .files
        .iter()
        .find(|f| f.path.ends_with("link.txt"))
        .unwrap();
    assert!(real.symlink_target.is_none());
    assert!(link.symlink_target.is_some());

    // Upload (only the real file gets hashed/uploaded, symlinks pass through)
    let data_cache = Arc::new(FileSystemDataCache::new(cache_dir.path().join("data")).unwrap());
    let upload_result = hash_upload_abs_manifest(
        &AbsManifest::Snapshot(snapshot),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        HashUploadOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(upload_result.statistics.uploaded_files, 1);

    // Remap to destination
    let abs_snap = match upload_result.manifest {
        AbsManifest::Snapshot(s) => s,
        _ => panic!("expected snapshot"),
    };
    let rel = subtree_snapshot(
        &abs_snap,
        &src_dir.path().to_string_lossy(),
        SymlinkPolicy::CollapseEscaping,
    )
    .unwrap();
    let abs_dl = join_snapshot(&rel, &dst_dir.path().to_string_lossy()).unwrap();

    // Verify the symlink survived subtree+join
    let dl_link = abs_dl
        .files
        .iter()
        .find(|f| f.path.ends_with("link.txt"))
        .unwrap();
    assert!(dl_link.symlink_target.is_some());

    // Download with symlink support
    download_abs_manifest(
        &AbsManifest::Snapshot(abs_dl),
        data_cache.clone() as Arc<dyn AsyncDataCache>,
        DownloadOptions {
            symlink_policy: SymlinkPolicy::Preserve,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Verify: real file has correct content
    assert_eq!(
        std::fs::read_to_string(dst_dir.path().join("real.txt")).unwrap(),
        "real content"
    );
    // Verify: symlink exists and is actually a symlink
    let link_meta = dst_dir.path().join("link.txt").symlink_metadata().unwrap();
    assert!(link_meta.file_type().is_symlink());
}
