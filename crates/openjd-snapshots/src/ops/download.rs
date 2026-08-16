// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use super::memory_pool::{default_max_memory_bytes, MemoryPool};
use super::rate::SlidingWindowRate;
use crate::data_cache::AsyncDataCache;
use crate::hash::hash_data;
use crate::hash_cache::{HashCache, WHOLE_FILE_RANGE_END};
use crate::manifest::{AbsManifest, DirEntry, FileEntry, Manifest, SymlinkPolicy};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;
use tracing::debug;

/// Generate a temp file path with a random suffix: `<path>.tmp<random_hex>`.
fn temp_download_path(target: &Path) -> PathBuf {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(std::process::id() as u64);
    let suffix = hasher.finish();
    target.with_extension(format!(
        "{}tmp{:08x}",
        target
            .extension()
            .map(|e| format!("{}.", e.to_string_lossy()))
            .unwrap_or_default(),
        suffix
    ))
}

/// Atomically move `tmp` to `target`, removing `tmp` on error.
fn atomic_replace(tmp: &Path, target: &Path) -> std::io::Result<()> {
    if let Err(e) = std::fs::rename(tmp, target) {
        let _ = std::fs::remove_file(tmp);
        return Err(e);
    }
    Ok(())
}

/// Pre-allocate a file to the given size using platform-specific sparse allocation.
///
/// On Linux, uses `posix_fallocate` for efficient sparse allocation.
/// On Windows, uses `SetFilePointerEx` + `SetEndOfFile` to avoid the slow
/// zero-fill that `set_len()`/`truncate()` causes (150-260ms per 50MB).
/// On other platforms (macOS), uses `set_len` (ftruncate).
fn preallocate_file(path: &std::path::Path, size: u64) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let f = std::fs::File::create(path)?;
    if size == 0 {
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsRawFd;
        let ret = unsafe { libc::posix_fallocate(f.as_raw_fd(), 0, size as libc::off_t) };
        if ret != 0 {
            // posix_fallocate can fail on filesystems that don't support it (e.g. ZFS,
            // NFS, tmpfs). Fall back to set_len which uses ftruncate.
            f.set_len(size)?;
        }
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::Storage::FileSystem::{SetEndOfFile, SetFilePointerEx, FILE_BEGIN};
        let handle = f.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }
        let mut new_pos: i64 = 0;
        let ok = unsafe { SetFilePointerEx(handle, size as i64, &mut new_pos, FILE_BEGIN) };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let ok = unsafe { SetEndOfFile(handle) };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        f.set_len(size)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileConflictResolution {
    Skip,
    Overwrite,
    CreateCopy,
}

pub struct DownloadOptions {
    pub hash_cache: Option<Arc<HashCache>>,
    pub file_conflict_resolution: FileConflictResolution,
    pub apply_deletes: bool,
    pub symlink_policy: SymlinkPolicy,
    pub on_progress: Option<Box<super::ProgressFn<DownloadStatistics>>>,
    pub max_workers: Option<usize>,
    pub max_memory_bytes: Option<usize>,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            hash_cache: None,
            file_conflict_resolution: FileConflictResolution::Overwrite,
            apply_deletes: true,
            symlink_policy: SymlinkPolicy::Preserve,
            on_progress: None,
            max_workers: None,
            max_memory_bytes: None,
        }
    }
}

#[derive(Debug)]
pub struct DownloadResult {
    pub manifest: AbsManifest,
    pub statistics: DownloadStatistics,
}

#[derive(Debug, Default, Clone)]
pub struct DownloadStatistics {
    pub total_files: usize,
    pub total_bytes: u64,
    pub downloaded_files: usize,
    pub downloaded_bytes: u64,
    pub skipped_files: usize,
    pub skipped_bytes: u64,
    /// Elapsed time since operation start, in seconds.
    pub total_time: f64,
    /// Current processing rate in bytes per second.
    pub rate: f64,
    /// Progress percentage (0.0 to 100.0).
    pub progress: f64,
    /// Human-readable progress summary.
    pub progress_message: String,
}

/// Downloads files from a data cache to the local filesystem.
pub async fn download_abs_manifest(
    manifest: &AbsManifest,
    data_cache: Arc<dyn AsyncDataCache>,
    options: DownloadOptions,
) -> crate::Result<DownloadResult> {
    match manifest {
        AbsManifest::Snapshot(s) => {
            let (result, stats) = download_manifest(s, data_cache, options).await?;
            Ok(DownloadResult {
                manifest: AbsManifest::Snapshot(result),
                statistics: stats,
            })
        }
        AbsManifest::Diff(d) => {
            let (result, stats) = download_manifest(d, data_cache, options).await?;
            Ok(DownloadResult {
                manifest: AbsManifest::Diff(result),
                statistics: stats,
            })
        }
    }
}

/// Evaluate a single file entry for skip/download and push onto work_items.
#[allow(clippy::too_many_arguments)]
fn collect_work_item(
    index: usize,
    file: &FileEntry,
    file_chunk_size_bytes: i64,
    alg_str: &str,
    hash_cache: &Option<Arc<HashCache>>,
    conflict_res: FileConflictResolution,
    stats: &mut DownloadStatistics,
    work_items: &mut Vec<(usize, bool, u64)>,
) {
    let file_size = file.size.unwrap_or(0);
    stats.total_files += 1;
    stats.total_bytes += file_size;

    let path = Path::new(&file.path);

    // Check hash cache for skip
    if let Some(ref cache) = hash_cache {
        if file.mtime.is_some() && path.exists() {
            if let Ok(actual_mtime) = get_mtime(path) {
                if let Some(ref file_hash) = file.hash {
                    if let Some(cached_hash) =
                        cache.get_if_fresh(path, alg_str, 0, WHOLE_FILE_RANGE_END, actual_mtime)
                    {
                        if cached_hash == *file_hash {
                            stats.skipped_files += 1;
                            stats.skipped_bytes += file_size;
                            work_items.push((index, true, actual_mtime));
                            return;
                        }
                    }
                }
                if let Some(ref chunk_hashes) = file.chunk_hashes {
                    let cs = file_chunk_size_bytes as u64;
                    if cs > 0 && !chunk_hashes.is_empty() {
                        let mut all_cached = true;
                        let mut offset: u64 = 0;
                        for expected in chunk_hashes {
                            let end = std::cmp::min(offset + cs, file_size);
                            if let Some(cached) = cache.get_if_fresh(
                                path,
                                alg_str,
                                offset as i64,
                                end as i64,
                                actual_mtime,
                            ) {
                                if cached != *expected {
                                    all_cached = false;
                                    break;
                                }
                            } else {
                                all_cached = false;
                                break;
                            }
                            offset = end;
                        }
                        if all_cached {
                            stats.skipped_files += 1;
                            stats.skipped_bytes += file_size;
                            work_items.push((index, true, actual_mtime));
                            return;
                        }
                    }
                }
            }
        }
    }

    // Check conflict resolution
    if path.exists() && conflict_res == FileConflictResolution::Skip {
        debug!(path = %file.path, "file exists, skipping per conflict resolution policy");
        stats.skipped_files += 1;
        stats.skipped_bytes += file_size;
        let mtime = get_mtime(path).ok();
        work_items.push((index, true, mtime.unwrap_or(0)));
        return;
    }

    work_items.push((index, false, 0));
}

async fn download_manifest<P: Clone + Send + Sync + 'static, K: Clone + Send + Sync + 'static>(
    manifest: &Manifest<P, K>,
    data_cache: Arc<dyn AsyncDataCache>,
    options: DownloadOptions,
) -> crate::Result<(Manifest<P, K>, DownloadStatistics)> {
    let start_time = std::time::Instant::now();
    let alg_str = manifest.hash_alg.extension();
    let mut result = manifest.clone();

    // Extract options fields up front so we can move on_progress into an Arc later
    let hash_cache = options.hash_cache;
    let conflict_res = options.file_conflict_resolution;
    let apply_deletes = options.apply_deletes;
    let symlink_policy = options.symlink_policy;
    let max_workers = options.max_workers;
    let max_memory_bytes = options.max_memory_bytes;

    // Handle deleted entries
    if apply_deletes {
        apply_delete_entries(&manifest.files, &manifest.dirs);
    }

    // Create symlinks in topological order
    for idx in symlink_creation_order(&result.files) {
        handle_symlink(&mut result.files[idx], symlink_policy)?;
    }

    // Create target directories and evaluate each file for skip/download
    let (work_items, mut stats) = build_download_work_list(
        &result.files,
        &manifest.dirs,
        manifest.file_chunk_size_bytes,
        alg_str,
        &hash_cache,
        conflict_res,
    )?;

    // Separate skipped from needing download
    let download_items: Vec<DownloadWorkItem> = work_items
        .iter()
        .filter(|(_, skipped, _)| !skipped)
        .map(|(i, _, _)| DownloadWorkItem {
            index: *i,
            file_size: result.files[*i].size.unwrap_or(0),
        })
        .collect();

    // Apply skipped mtime updates
    for &(i, skipped, mtime) in &work_items {
        if skipped && mtime != 0 {
            result.files[i].mtime = Some(mtime);
        }
    }

    let on_progress: Option<Arc<super::ProgressFn<DownloadStatistics>>> =
        options.on_progress.map(|f| Arc::from(f));

    if download_items.is_empty() {
        finish_empty_download(&mut stats, &on_progress, manifest.file_chunk_size_bytes);
        return Ok((result, stats));
    }

    // Download files in parallel using tokio
    let max_memory = max_memory_bytes.unwrap_or_else(default_max_memory_bytes);
    let num_workers = max_workers.unwrap_or(10);
    let ctx = Arc::new(DownloadCtx {
        data_cache,
        memory_pool: Arc::new(MemoryPool::new(max_memory)),
        worker_semaphore: Arc::new(tokio::sync::Semaphore::new(num_workers)),
        cancelled: Arc::new(AtomicBool::new(false)),
        // std::sync::Mutex is intentional here: the lock is held only for nanosecond-scale
        // field updates and never across .await points, so it's cheaper than tokio::sync::Mutex
        // which would yield to the scheduler even when uncontended.
        progress_stats: Arc::new(Mutex::new(stats.clone())),
        rate_calc: Arc::new(Mutex::new(SlidingWindowRate::new())),
        on_progress: on_progress.clone(),
        alg: alg_str.to_string(),
        conflict_res,
        manifest_chunk_size: result.file_chunk_size_bytes,
        start: start_time,
    });

    let download_results = run_download_tasks(&ctx, &result.files, download_items).await;

    // Apply mtime restoration and cache writes sequentially
    for r in download_results {
        let (index, target_path) = r?;
        finalize_downloaded_file(
            &mut result.files[index],
            &target_path,
            &hash_cache,
            alg_str,
            result.file_chunk_size_bytes,
        )?;
    }

    let stats = finalize_download_stats(
        &ctx.progress_stats,
        &ctx.rate_calc,
        start_time,
        manifest.file_chunk_size_bytes,
    );

    if let Some(ref cb) = on_progress {
        let _ = cb(&stats);
    }

    Ok((result, stats))
}

/// A file that still needs downloading: its manifest index and size in bytes.
struct DownloadWorkItem {
    index: usize,
    file_size: u64,
}

/// Shared, per-operation state used by every download worker task.
struct DownloadCtx {
    data_cache: Arc<dyn AsyncDataCache>,
    memory_pool: Arc<MemoryPool>,
    worker_semaphore: Arc<tokio::sync::Semaphore>,
    cancelled: Arc<AtomicBool>,
    progress_stats: Arc<Mutex<DownloadStatistics>>,
    rate_calc: Arc<Mutex<SlidingWindowRate>>,
    on_progress: Option<Arc<super::ProgressFn<DownloadStatistics>>>,
    alg: String,
    conflict_res: FileConflictResolution,
    manifest_chunk_size: i64,
    start: std::time::Instant,
}

/// Per-file inputs for one download task, cloned out of the manifest so the
/// spawned task owns its data.
struct FileDownloadSpec {
    index: usize,
    path: String,
    hash: Option<String>,
    chunk_hashes: Option<Vec<String>>,
    size: u64,
}

/// Remove deleted files, then deleted directories (deepest first), from the
/// local filesystem. Removal failures are ignored (best effort).
fn apply_delete_entries(files: &[FileEntry], dirs: &[DirEntry]) {
    for file in files {
        if file.deleted {
            let path = Path::new(&file.path);
            if path.exists() {
                if path.is_symlink() {
                    debug!(path = %file.path, "deleting symlink");
                } else {
                    debug!(path = %file.path, "deleting file");
                }
                let _ = std::fs::remove_file(path);
            }
        }
    }
    let mut del_dirs: Vec<&str> = dirs
        .iter()
        .filter(|d| d.deleted)
        .map(|d| d.path.as_str())
        .collect();
    del_dirs.sort();
    del_dirs.reverse();
    for dir_path in del_dirs {
        debug!(path = dir_path, "deleting directory");
        let _ = std::fs::remove_dir(dir_path);
    }
}

/// Order symlink entry indices so that targets which are themselves symlinks
/// are created before the links that point at them (Kahn's algorithm).
/// Entries participating in a cycle are appended in manifest order.
fn symlink_creation_order(files: &[FileEntry]) -> Vec<usize> {
    let symlink_indices: Vec<usize> = files
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.deleted && f.symlink_target.is_some())
        .map(|(i, _)| i)
        .collect();

    let symlink_paths: std::collections::HashSet<&str> = symlink_indices
        .iter()
        .map(|&i| files[i].path.as_str())
        .collect();

    let mut in_degree: HashMap<usize, usize> = symlink_indices.iter().map(|&i| (i, 0)).collect();
    let mut dependents: HashMap<usize, Vec<usize>> = HashMap::new();
    let path_to_idx: HashMap<&str, usize> = symlink_indices
        .iter()
        .map(|&i| (files[i].path.as_str(), i))
        .collect();

    for &i in &symlink_indices {
        let target = files[i].symlink_target.as_deref().unwrap();
        if symlink_paths.contains(target) {
            let &dep = path_to_idx.get(target).unwrap();
            *in_degree.entry(i).or_default() += 1;
            dependents.entry(dep).or_default().push(i);
        }
    }

    let mut queue: std::collections::VecDeque<usize> = symlink_indices
        .iter()
        .filter(|&&i| in_degree[&i] == 0)
        .copied()
        .collect();
    let mut sorted = Vec::with_capacity(symlink_indices.len());
    let mut sorted_set = std::collections::HashSet::with_capacity(symlink_indices.len());
    while let Some(idx) = queue.pop_front() {
        sorted.push(idx);
        sorted_set.insert(idx);
        if let Some(deps) = dependents.get(&idx) {
            for &d in deps {
                let deg = in_degree.get_mut(&d).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    queue.push_back(d);
                }
            }
        }
    }
    for &i in &symlink_indices {
        if !sorted_set.contains(&i) {
            sorted.push(i);
        }
    }
    sorted
}

/// Skip-evaluation outcome for one file: `(manifest index, skipped, mtime)`.
type SkipEvaluation = (usize, bool, u64);

/// Create target directories and evaluate every regular file for
/// skip-or-download, interleaving each directory's creation with the
/// evaluation of the files inside it. Returns `(index, skipped, mtime)`
/// work tuples plus the initial statistics.
fn build_download_work_list(
    files: &[FileEntry],
    dirs: &[DirEntry],
    file_chunk_size_bytes: i64,
    alg_str: &str,
    hash_cache: &Option<Arc<HashCache>>,
    conflict_res: FileConflictResolution,
) -> crate::Result<(Vec<SkipEvaluation>, DownloadStatistics)> {
    // Collect manifest directories (sorted so parents come before children)
    let mut dir_paths: Vec<&str> = dirs
        .iter()
        .filter(|d| !d.deleted)
        .map(|d| d.path.as_str())
        .collect();
    dir_paths.sort();

    // Track which directories have been created to avoid redundant syscalls
    let mut created_dirs: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Group file indices by parent directory for interleaved creation
    let mut files_by_parent: std::collections::BTreeMap<String, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (i, file) in files.iter().enumerate() {
        if file.deleted || file.symlink_target.is_some() {
            continue;
        }
        let parent = Path::new(&file.path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        files_by_parent.entry(parent).or_default().push(i);
    }

    let mut stats = DownloadStatistics::default();
    let mut work_items = Vec::new();

    // Helper: ensure a directory exists, deduplicating calls
    let ensure_dir =
        |dir: &str, created: &mut std::collections::HashSet<String>| -> crate::Result<()> {
            if !dir.is_empty() && created.insert(dir.to_string()) {
                std::fs::create_dir_all(dir)?;
            }
            Ok(())
        };

    // Process manifest directories interleaved with their files
    for dir_path in &dir_paths {
        ensure_dir(dir_path, &mut created_dirs)?;
        if let Some(indices) = files_by_parent.remove(*dir_path) {
            for i in indices {
                collect_work_item(
                    i,
                    &files[i],
                    file_chunk_size_bytes,
                    alg_str,
                    hash_cache,
                    conflict_res,
                    &mut stats,
                    &mut work_items,
                );
            }
        }
    }

    // Process remaining files whose parent wasn't a manifest directory
    for (parent, indices) in files_by_parent {
        ensure_dir(&parent, &mut created_dirs)?;
        for i in indices {
            collect_work_item(
                i,
                &files[i],
                file_chunk_size_bytes,
                alg_str,
                hash_cache,
                conflict_res,
                &mut stats,
                &mut work_items,
            );
        }
    }

    Ok((work_items, stats))
}

/// Finish statistics for the fast path where nothing needs downloading.
fn finish_empty_download(
    stats: &mut DownloadStatistics,
    on_progress: &Option<Arc<super::ProgressFn<DownloadStatistics>>>,
    chunk_size: i64,
) {
    if stats.total_bytes > 0 {
        stats.progress = 100.0;
    }
    if let Some(ref cb) = on_progress {
        let _ = cb(stats);
    }
    let unit = if chunk_size <= 0 { "files" } else { "chunks" };
    stats.progress_message = format!(
        "Downloaded {} ({} {}) in 0.00s",
        crate::hash::human_readable_file_size(stats.total_bytes),
        stats.total_files,
        unit
    );
}

/// Spawn one download task per work item and await them all, preserving
/// spawn order in the returned results.
async fn run_download_tasks(
    ctx: &Arc<DownloadCtx>,
    files: &[FileEntry],
    download_items: Vec<DownloadWorkItem>,
) -> Vec<crate::Result<(usize, PathBuf)>> {
    let mut handles = Vec::new();
    for item in download_items {
        let file = &files[item.index];
        let spec = FileDownloadSpec {
            index: item.index,
            path: file.path.clone(),
            hash: file.hash.clone(),
            chunk_hashes: file.chunk_hashes.clone(),
            size: item.file_size,
        };
        handles.push(tokio::spawn(download_one_file(ctx.clone(), spec)));
    }

    let mut results = Vec::new();
    for handle in handles {
        match handle.await {
            Ok(r) => results.push(r),
            Err(e) => results.push(Err(crate::SnapshotError::Task(e.to_string()))),
        }
    }
    results
}

/// Download one file end to end: acquire worker and memory permits, fetch it
/// with the appropriate strategy (chunked, multipart, or whole), write it to
/// disk, and record progress. Returns the manifest index and target path.
async fn download_one_file(
    ctx: Arc<DownloadCtx>,
    spec: FileDownloadSpec,
) -> crate::Result<(usize, PathBuf)> {
    let _worker_permit = ctx
        .worker_semaphore
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| crate::SnapshotError::Task(e.to_string()))?;

    if ctx.cancelled.load(Ordering::Relaxed) {
        return Err(crate::SnapshotError::Cancelled);
    }

    let _mem_permit = ctx.memory_pool.acquire(spec.size as usize).await;

    let target_path = resolve_target_path(&spec.path, ctx.conflict_res);

    let part_size = ctx.data_cache.multipart_part_size();
    let multipart_threshold = 2 * part_size as u64;
    let supports_range = ctx.data_cache.as_range_read().is_some();

    if let Some(chunk_hashes) = spec.chunk_hashes {
        download_chunks_to_file(
            &ctx,
            chunk_hashes,
            spec.size,
            &target_path,
            part_size,
            multipart_threshold,
            supports_range,
        )
        .await?;
    } else if let Some(ref hash) = spec.hash {
        if spec.size >= multipart_threshold && supports_range {
            download_multipart_to_file(
                &ctx.data_cache,
                hash,
                &ctx.alg,
                spec.size,
                part_size,
                target_path.clone(),
            )
            .await?;
        } else {
            let data = fetch_verified_whole(&ctx.data_cache, hash, &ctx.alg, &spec.path).await?;
            write_file_atomically(target_path.clone(), data).await?;
        }
    } else {
        return Err(crate::SnapshotError::Validation(format!(
            "file has no hash or chunk_hashes: {}",
            spec.path
        )));
    }

    record_downloaded_file(&ctx, spec.size)?;

    Ok((spec.index, target_path))
}

/// Choose the on-disk target path, diverting to a `name (N).ext` copy when
/// the file already exists and the policy is [`FileConflictResolution::CreateCopy`].
fn resolve_target_path(file_path: &str, conflict_res: FileConflictResolution) -> PathBuf {
    let path = Path::new(file_path);
    if path.exists() && conflict_res == FileConflictResolution::CreateCopy {
        create_copy_path(path)
    } else {
        path.to_path_buf()
    }
}

/// Download a chunked file: preallocate a temp file, fetch every chunk in
/// parallel (splitting large chunks into byte-range parts when supported),
/// then atomically rename the temp file onto the target.
async fn download_chunks_to_file(
    ctx: &DownloadCtx,
    chunk_hashes: Vec<String>,
    file_size: u64,
    target_path: &Path,
    part_size: usize,
    multipart_threshold: u64,
    supports_range: bool,
) -> crate::Result<()> {
    // Write chunks to a temp file, then atomic rename
    let tmp_path = temp_download_path(target_path);
    let tp = tmp_path.clone();
    let fs = file_size;
    tokio::task::spawn_blocking(move || preallocate_file(&tp, fs))
        .await
        .map_err(|e| crate::SnapshotError::Task(e.to_string()))?
        .map_err(crate::SnapshotError::Io)?;

    let mut file_offset: u64 = 0;
    let mut chunk_handles: Vec<tokio::task::JoinHandle<std::io::Result<u64>>> = Vec::new();
    for h in chunk_hashes {
        let cs = ctx.manifest_chunk_size as u64;
        let remaining = file_size - file_offset;
        let this_chunk_size = remaining.min(cs);

        if this_chunk_size >= multipart_threshold && supports_range {
            // Split large chunk into parallel byte-range parts
            let num_parts = (this_chunk_size as usize).div_ceil(part_size);
            for part_idx in 0..num_parts {
                let part_start = part_idx as u64 * part_size as u64;
                let part_end =
                    std::cmp::min(part_start + part_size as u64 - 1, this_chunk_size - 1);
                let write_offset = file_offset + part_start;
                let dc = ctx.data_cache.clone();
                let alg = ctx.alg.clone();
                let h = h.clone();
                let tp = tmp_path.clone();
                chunk_handles.push(tokio::spawn(async move {
                    dc.as_range_read()
                        .expect("RangeReadDataCache support verified above")
                        .stream_range_to_file_at_offset(
                            &h,
                            &alg,
                            part_start,
                            part_end,
                            &tp,
                            write_offset,
                        )
                        .await
                }));
            }
        } else {
            // Small chunk - single GetObject with inline hash verification
            let dc = ctx.data_cache.clone();
            let alg = ctx.alg.clone();
            let tp = tmp_path.clone();
            let off = file_offset;
            let expected_hash = h.clone();
            chunk_handles.push(tokio::spawn(fetch_chunk_and_write(
                dc,
                expected_hash,
                alg,
                tp,
                off,
            )));
        }
        file_offset += this_chunk_size;
    }
    for handle in chunk_handles {
        handle
            .await
            .map_err(|e| crate::SnapshotError::Task(e.to_string()))?
            .map_err(crate::SnapshotError::Io)?;
    }

    // Atomic rename from temp to target
    let tmp = tmp_path.clone();
    let tgt = target_path.to_path_buf();
    tokio::task::spawn_blocking(move || atomic_replace(&tmp, &tgt))
        .await
        .map_err(|e| crate::SnapshotError::Task(e.to_string()))?
        .map_err(crate::SnapshotError::Io)?;
    Ok(())
}

/// Fetch one small chunk, verify its hash, and write it into the temp file
/// at the given offset. Returns the number of bytes written.
async fn fetch_chunk_and_write(
    dc: Arc<dyn AsyncDataCache>,
    expected_hash: String,
    alg: String,
    tmp_path: PathBuf,
    off: u64,
) -> std::io::Result<u64> {
    let data = dc.get_object(&expected_hash, &alg).await?;
    let actual_hash = hash_data(&data);
    if actual_hash != expected_hash {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "hash mismatch for chunk at offset {off}: expected {expected_hash}, got {actual_hash}"
            ),
        ));
    }
    let len = data.len() as u64;
    tokio::task::spawn_blocking(move || {
        use std::io::{Seek, SeekFrom, Write};
        let mut f = std::fs::OpenOptions::new().write(true).open(&tmp_path)?;
        f.seek(SeekFrom::Start(off))?;
        f.write_all(&data)?;
        Ok::<_, std::io::Error>(len)
    })
    .await
    .map_err(std::io::Error::other)?
}

/// Fetch a whole small file via `get_object` and verify its hash against
/// the manifest entry, returning the file contents.
async fn fetch_verified_whole(
    dc: &Arc<dyn AsyncDataCache>,
    hash: &str,
    alg: &str,
    file_path: &str,
) -> crate::Result<Vec<u8>> {
    let data = dc
        .get_object(hash, alg)
        .await
        .map_err(crate::SnapshotError::Io)?;
    let actual_hash = hash_data(&data);
    if actual_hash != hash {
        return Err(crate::SnapshotError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "hash mismatch for {}: expected {hash}, got {actual_hash}",
                file_path
            ),
        )));
    }
    Ok(data)
}

/// Write file data to a temp path on a blocking thread, then atomically
/// rename it onto the target.
async fn write_file_atomically(target_path: PathBuf, data: Vec<u8>) -> crate::Result<()> {
    tokio::task::spawn_blocking(move || {
        let tmp = temp_download_path(&target_path);
        if let Err(e) = std::fs::write(&tmp, &data) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        atomic_replace(&tmp, &target_path)
    })
    .await
    .map_err(|e| crate::SnapshotError::Task(e.to_string()))?
    .map_err(crate::SnapshotError::Io)
}

/// Record one completed file in the shared statistics and invoke the
/// progress callback, flagging cancellation if the callback returns false.
fn record_downloaded_file(ctx: &DownloadCtx, file_size: u64) -> crate::Result<()> {
    let mut s = ctx.progress_stats.lock().unwrap();
    s.downloaded_files += 1;
    s.downloaded_bytes += file_size;
    let elapsed = ctx.start.elapsed().as_secs_f64();
    s.total_time = elapsed;
    {
        let mut rc = ctx.rate_calc.lock().unwrap();
        s.rate = rc.update(elapsed, s.downloaded_bytes + s.skipped_bytes);
    }
    if s.total_bytes > 0 {
        s.progress = ((s.downloaded_bytes + s.skipped_bytes) as f64 / s.total_bytes as f64) * 100.0;
    }
    if let Some(ref cb) = ctx.on_progress {
        if !cb(&s) {
            ctx.cancelled.store(true, Ordering::Relaxed);
            return Err(crate::SnapshotError::Cancelled);
        }
    }
    Ok(())
}

/// Restore the downloaded file's mtime from the manifest, read back the
/// actual on-disk mtime into the entry, and record its hashes in the
/// hash cache.
fn finalize_downloaded_file(
    file: &mut FileEntry,
    target_path: &Path,
    hash_cache: &Option<Arc<HashCache>>,
    alg_str: &str,
    chunk_size: i64,
) -> crate::Result<()> {
    // 1. Set the file's mtime to the manifest value
    if let Some(manifest_mtime) = file.mtime {
        let system_time = UNIX_EPOCH + std::time::Duration::from_micros(manifest_mtime);
        // Opening with write(true) is required on Windows for set_modified to succeed.
        let _ = std::fs::OpenOptions::new()
            .write(true)
            .open(target_path)
            .and_then(|f| f.set_modified(system_time));
    }

    // 2. Read back the actual mtime (filesystem precision may have adjusted it)
    let actual_mtime = get_mtime(target_path)?;

    // 3. Store the read-back mtime so the returned manifest matches disk exactly
    file.mtime = Some(actual_mtime);

    if let Some(ref cache) = hash_cache {
        if let Some(ref hash) = file.hash {
            let _ = cache.put(
                target_path,
                alg_str,
                0,
                WHOLE_FILE_RANGE_END,
                hash,
                actual_mtime,
            );
        }
        if let Some(ref chunk_hashes) = file.chunk_hashes {
            let cs = chunk_size as u64;
            if cs > 0 {
                let file_size = file.size.unwrap_or(0);
                let mut offset: u64 = 0;
                for h in chunk_hashes {
                    let end = std::cmp::min(offset + cs, file_size);
                    let _ = cache.put(
                        target_path,
                        alg_str,
                        offset as i64,
                        end as i64,
                        h,
                        actual_mtime,
                    );
                    offset = end;
                }
            }
        }
    }
    Ok(())
}

/// Fold the shared progress state into final statistics: total time, rate,
/// percentage, and the human-readable summary message.
fn finalize_download_stats(
    progress_stats: &Mutex<DownloadStatistics>,
    rate_calc: &Mutex<SlidingWindowRate>,
    start_time: std::time::Instant,
    chunk_size: i64,
) -> DownloadStatistics {
    let mut stats = progress_stats.lock().unwrap().clone();

    stats.total_time = start_time.elapsed().as_secs_f64();
    {
        let mut rc = rate_calc.lock().unwrap();
        stats.rate = rc.update(
            stats.total_time,
            stats.downloaded_bytes + stats.skipped_bytes,
        );
    }
    if stats.total_bytes > 0 {
        stats.progress = ((stats.downloaded_bytes + stats.skipped_bytes) as f64
            / stats.total_bytes as f64)
            * 100.0;
    }

    let unit = if chunk_size <= 0 { "files" } else { "chunks" };
    let mut parts = vec![
        format!(
            "Downloaded {}",
            crate::hash::human_readable_file_size(stats.total_bytes)
        ),
        format!("({} {})", stats.total_files, unit),
        format!("in {:.2}s", stats.total_time),
    ];
    if stats.total_time > 0.0 {
        parts.push(format!(
            "({}/s)",
            crate::hash::human_readable_file_size(stats.rate as u64)
        ));
    }
    stats.progress_message = parts.join(" ");
    stats
}

async fn download_multipart_to_file(
    data_cache: &Arc<dyn AsyncDataCache>,
    hash: &str,
    alg_str: &str,
    file_size: u64,
    part_size: usize,
    target_path: std::path::PathBuf,
) -> crate::Result<()> {
    // Pre-allocate sparse temp file
    let tmp_path = temp_download_path(&target_path);
    let tp = tmp_path.clone();
    let fs = file_size;
    tokio::task::spawn_blocking(move || preallocate_file(&tp, fs))
        .await
        .map_err(|e| crate::SnapshotError::Task(e.to_string()))?
        .map_err(crate::SnapshotError::Io)?;

    // Download parts in parallel, write each at its offset in the temp file
    let num_parts = (file_size as usize).div_ceil(part_size);
    let mut handles = Vec::with_capacity(num_parts);

    for part_idx in 0..num_parts {
        let start = part_idx as u64 * part_size as u64;
        let end = std::cmp::min(start + part_size as u64 - 1, file_size - 1);
        let dc = data_cache.clone();
        let h = hash.to_string();
        let a = alg_str.to_string();
        let tp = tmp_path.clone();

        handles.push(tokio::spawn(async move {
            dc.as_range_read()
                .expect("download_multipart_to_file requires RangeReadDataCache support")
                .stream_range_to_file_at_offset(&h, &a, start, end, &tp, start)
                .await
                .map_err(crate::SnapshotError::Io)?;
            Ok::<_, crate::SnapshotError>(())
        }));
    }

    for handle in handles {
        handle
            .await
            .map_err(|e| crate::SnapshotError::Task(e.to_string()))??;
    }

    // Atomic rename from temp to target
    let tmp = tmp_path;
    let tgt = target_path;
    tokio::task::spawn_blocking(move || atomic_replace(&tmp, &tgt))
        .await
        .map_err(|e| crate::SnapshotError::Task(e.to_string()))?
        .map_err(crate::SnapshotError::Io)?;

    Ok(())
}

fn handle_symlink(file: &mut FileEntry, symlink_policy: SymlinkPolicy) -> crate::Result<()> {
    if symlink_policy == SymlinkPolicy::ExcludeAll {
        return Ok(());
    }

    let target = file.symlink_target.as_ref().unwrap();
    let link_path = Path::new(&file.path);

    if let Some(parent) = link_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if link_path.symlink_metadata().is_ok() {
        let _ = std::fs::remove_file(link_path);
    }

    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link_path)?;
    debug!(link = %file.path, target = %target, "created symlink");
    #[cfg(windows)]
    {
        let target_path = Path::new(target);
        if target_path.is_dir() {
            std::os::windows::fs::symlink_dir(target, link_path)?;
        } else {
            std::os::windows::fs::symlink_file(target, link_path)?;
        }
    }

    Ok(())
}

fn get_mtime(path: &Path) -> crate::Result<u64> {
    let meta = std::fs::metadata(path)?;
    let mtime = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(|e| crate::SnapshotError::Task(e.to_string()))?
        .as_micros() as u64;
    Ok(mtime)
}

fn create_copy_path(path: &Path) -> std::path::PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let parent = path.parent().unwrap_or(Path::new("."));

    for i in 1.. {
        let candidate = parent.join(format!("{stem} ({i}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_cache::{AsyncDataCache, FileSystemDataCache};
    use crate::hash::{hash_data, HashAlgorithm};
    use crate::manifest::{AbsManifest, AbsSnapshot, AbsSnapshotDiff, DirEntry, Manifest};
    use crate::DEFAULT_FILE_CHUNK_SIZE;
    use std::sync::Arc;
    use tempfile::TempDir;

    async fn setup_data_cache_with_file(cache: &FileSystemDataCache, content: &[u8]) -> String {
        let hash = hash_data(content);
        cache
            .put_object(&hash, "xxh128", content.to_vec())
            .await
            .unwrap();
        hash
    }

    #[tokio::test]
    async fn download_single_file() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        let data_cache = FileSystemDataCache::new(cache_dir.path().join("data")).unwrap();

        let hash = setup_data_cache_with_file(&data_cache, b"hello world").await;
        let dest = tmp.path().join("output.txt");

        let mut entry = FileEntry::file(dest.to_string_lossy().to_string(), 11, 1000);
        entry.hash = Some(hash);

        let manifest: AbsSnapshot =
            Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![entry]);

        let result = download_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            Arc::new(data_cache),
            DownloadOptions::default(),
        )
        .await
        .unwrap();

        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "hello world");
        assert_eq!(result.statistics.downloaded_files, 1);
        assert_eq!(result.statistics.downloaded_bytes, 11);
    }

    #[tokio::test]
    async fn download_creates_parent_dirs() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        let data_cache = FileSystemDataCache::new(cache_dir.path().join("data")).unwrap();

        let hash = setup_data_cache_with_file(&data_cache, b"nested").await;
        let dest = tmp.path().join("a/b/c/file.txt");

        let mut entry = FileEntry::file(dest.to_string_lossy().to_string(), 6, 1000);
        entry.hash = Some(hash);

        let manifest: AbsSnapshot =
            Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![entry]);

        download_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            Arc::new(data_cache),
            DownloadOptions::default(),
        )
        .await
        .unwrap();

        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "nested");
    }

    #[tokio::test]
    async fn download_skip_conflict() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        let data_cache = FileSystemDataCache::new(cache_dir.path().join("data")).unwrap();

        let hash = setup_data_cache_with_file(&data_cache, b"new content").await;
        let dest = tmp.path().join("existing.txt");
        std::fs::write(&dest, b"old content").unwrap();

        let mut entry = FileEntry::file(dest.to_string_lossy().to_string(), 11, 1000);
        entry.hash = Some(hash);

        let manifest: AbsSnapshot =
            Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![entry]);

        let result = download_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            Arc::new(data_cache),
            DownloadOptions {
                file_conflict_resolution: FileConflictResolution::Skip,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old content");
        assert_eq!(result.statistics.skipped_files, 1);
    }

    #[tokio::test]
    async fn download_overwrite_conflict() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        let data_cache = FileSystemDataCache::new(cache_dir.path().join("data")).unwrap();

        let hash = setup_data_cache_with_file(&data_cache, b"new content").await;
        let dest = tmp.path().join("existing.txt");
        std::fs::write(&dest, b"old content").unwrap();

        let mut entry = FileEntry::file(dest.to_string_lossy().to_string(), 11, 1000);
        entry.hash = Some(hash);

        let manifest: AbsSnapshot =
            Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![entry]);

        download_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            Arc::new(data_cache),
            DownloadOptions::default(),
        )
        .await
        .unwrap();

        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "new content");
    }

    #[tokio::test]
    async fn download_create_copy_conflict() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        let data_cache = FileSystemDataCache::new(cache_dir.path().join("data")).unwrap();

        let hash = setup_data_cache_with_file(&data_cache, b"new").await;
        let dest = tmp.path().join("file.txt");
        std::fs::write(&dest, b"old").unwrap();

        let mut entry = FileEntry::file(dest.to_string_lossy().to_string(), 3, 1000);
        entry.hash = Some(hash);

        let manifest: AbsSnapshot =
            Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![entry]);

        download_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            Arc::new(data_cache),
            DownloadOptions {
                file_conflict_resolution: FileConflictResolution::CreateCopy,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old");
        let copy = tmp.path().join("file (1).txt");
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), "new");
    }

    #[tokio::test]
    async fn download_applies_deletes() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        let data_cache = FileSystemDataCache::new(cache_dir.path().join("data")).unwrap();

        let file_to_delete = tmp.path().join("gone.txt");
        std::fs::write(&file_to_delete, b"bye").unwrap();

        let manifest: AbsSnapshotDiff =
            Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![
                FileEntry::deleted(file_to_delete.to_string_lossy().to_string()),
            ]);

        download_abs_manifest(
            &AbsManifest::Diff(manifest),
            Arc::new(data_cache),
            DownloadOptions::default(),
        )
        .await
        .unwrap();

        assert!(!file_to_delete.exists());
    }

    #[tokio::test]
    async fn download_chunked_file() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        let data_cache = FileSystemDataCache::new(cache_dir.path().join("data")).unwrap();

        // Store 4 chunks of 3 bytes each
        let chunks: Vec<&[u8]> = vec![b"aaa", b"bbb", b"ccc", b"ddd"];
        let mut chunk_hashes: Vec<String> = Vec::with_capacity(chunks.len());
        for c in &chunks {
            let h = hash_data(c);
            data_cache
                .put_object(&h, "xxh128", c.to_vec())
                .await
                .unwrap();
            chunk_hashes.push(h);
        }

        let dest = tmp.path().join("chunked.bin");
        let mut entry = FileEntry::file(dest.to_string_lossy().to_string(), 12, 1000);
        entry.chunk_hashes = Some(chunk_hashes);

        let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, 3).with_files(vec![entry]);

        download_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            Arc::new(data_cache),
            DownloadOptions::default(),
        )
        .await
        .unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"aaabbbcccddd");
    }

    #[tokio::test]
    async fn download_updates_mtime_in_manifest() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        let data_cache = FileSystemDataCache::new(cache_dir.path().join("data")).unwrap();

        let hash = setup_data_cache_with_file(&data_cache, b"data").await;
        let dest = tmp.path().join("file.txt");

        let mut entry =
            FileEntry::file(dest.to_string_lossy().to_string(), 4, 1_577_836_800_000_000);
        entry.hash = Some(hash);

        let manifest: AbsSnapshot =
            Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![entry]);

        let result = download_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            Arc::new(data_cache),
            DownloadOptions::default(),
        )
        .await
        .unwrap();

        // mtime should be restored to the manifest value (or close, depending on fs precision)
        let actual_mtime = result.manifest.files()[0].mtime.unwrap();
        let diff = actual_mtime.abs_diff(1_577_836_800_000_000);
        assert!(
            diff < 1_000_000,
            "mtime should be restored to manifest value, got {actual_mtime}"
        );
    }

    #[tokio::test]
    async fn download_creates_manifest_dirs() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        let data_cache = FileSystemDataCache::new(cache_dir.path().join("data")).unwrap();

        let dir_path = tmp.path().join("new_dir");
        let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_dirs(vec![DirEntry::new(dir_path.to_string_lossy().to_string())]);

        download_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            Arc::new(data_cache),
            DownloadOptions::default(),
        )
        .await
        .unwrap();

        assert!(dir_path.is_dir());
    }
}
