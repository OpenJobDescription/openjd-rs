// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use super::memory_pool::{default_max_memory_bytes, MemoryPool};
use super::rate::SlidingWindowRate;
use crate::data_cache::AsyncDataCache;
use crate::hash::{hash_data, WHOLE_FILE_CHUNK_SIZE};
use crate::hash_cache::{HashCache, WHOLE_FILE_RANGE_END};
use crate::manifest::{AbsManifest, Manifest};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tracing::debug;

/// Concurrent upload deduplication map. When multiple tasks hash to the same
/// value, only the first one uploads; the rest subscribe and wait.
///
/// The lock is held only for the brief HashMap insert/lookup (nanoseconds),
/// so this does not become a bottleneck even with large thread pools. The
/// actual upload happens outside the lock, and waiters use a lock-free
/// broadcast channel.
type UploadDedup = Arc<Mutex<HashMap<String, tokio::sync::broadcast::Sender<()>>>>;

/// Try to upload data for `key`, deduplicating concurrent uploads of the same key.
/// Returns true if this call performed the upload, false if another task did.
async fn dedup_upload(
    dedup: &UploadDedup,
    key: &str,
    data_cache: &Arc<dyn AsyncDataCache>,
    hash: &str,
    alg: &str,
    data: Vec<u8>,
) -> crate::Result<bool> {
    // Fast path: check if already in the data cache (from a previous operation).
    if data_cache.object_exists(hash, alg).await.unwrap_or(false) {
        return Ok(false);
    }

    // Check the dedup map (lock held briefly).
    let mut rx = {
        let mut map = dedup.lock().unwrap();
        if let Some(tx) = map.get(key) {
            // Another task is already uploading this hash — subscribe and wait.
            Some(tx.subscribe())
        } else {
            // We are the first — insert a broadcast channel and proceed.
            let (tx, _) = tokio::sync::broadcast::channel(1);
            map.insert(key.to_string(), tx);
            None
        }
    };

    if let Some(ref mut rx) = rx {
        // Wait for the uploading task to finish, then return "not uploaded by us".
        let _ = rx.recv().await;
        return Ok(false);
    }

    // We own this hash — upload it.
    let result = data_cache.put_object(hash, alg, data).await;

    // Notify waiters and remove from map (lock held briefly).
    {
        let mut map = dedup.lock().unwrap();
        if let Some(tx) = map.remove(key) {
            let _ = tx.send(());
        }
    }

    result.map_err(crate::SnapshotError::Io)?;
    Ok(true)
}

#[derive(Default)]
pub struct HashUploadOptions {
    pub hash_cache: Option<Arc<HashCache>>,
    pub force_rehash: bool,
    pub file_chunk_size_bytes: Option<i64>,
    pub on_progress: Option<Box<super::ProgressFn<UploadStatistics>>>,
    pub max_workers: Option<usize>,
    pub max_memory_bytes: Option<usize>,
}

#[derive(Debug)]
pub struct UploadResult {
    pub manifest: AbsManifest,
    pub statistics: UploadStatistics,
}

#[derive(Debug, Default, Clone)]
pub struct UploadStatistics {
    pub total_files: usize,
    pub total_bytes: u64,
    pub hashed_files: usize,
    pub hashed_bytes: u64,
    pub uploaded_files: usize,
    pub uploaded_bytes: u64,
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

/// Hashes files and uploads their content to a data cache in a single pass.
pub async fn hash_upload_abs_manifest(
    manifest: &AbsManifest,
    data_cache: Arc<dyn AsyncDataCache>,
    options: HashUploadOptions,
) -> crate::Result<UploadResult> {
    match manifest {
        AbsManifest::Snapshot(s) => {
            let (result, stats) = hash_upload_manifest(s, data_cache, options).await?;
            Ok(UploadResult {
                manifest: AbsManifest::Snapshot(result),
                statistics: stats,
            })
        }
        AbsManifest::Diff(d) => {
            let (result, stats) = hash_upload_manifest(d, data_cache, options).await?;
            Ok(UploadResult {
                manifest: AbsManifest::Diff(result),
                statistics: stats,
            })
        }
    }
}

enum FileResult {
    Whole {
        hash: String,
        uploaded: bool,
        size: u64,
    },
    Chunked {
        hashes: Vec<String>,
        uploaded: bool,
        hashed_bytes: u64,
    },
    /// Cache hit: hash was known and object already exists in data cache.
    Skipped {
        size: u64,
        whole_hash: Option<String>,
        chunk_hashes: Option<Vec<String>>,
    },
}

async fn hash_upload_manifest<P: Clone + Send + Sync, K: Clone + Send + Sync>(
    manifest: &Manifest<P, K>,
    data_cache: Arc<dyn AsyncDataCache>,
    options: HashUploadOptions,
) -> crate::Result<(Manifest<P, K>, UploadStatistics)> {
    let start_time = std::time::Instant::now();

    // Validate no regular files already have hashes
    super::validate_files_not_hashed(&manifest.files)?;

    let chunk_size = options
        .file_chunk_size_bytes
        .unwrap_or(manifest.file_chunk_size_bytes);
    let alg_str = manifest.hash_alg.extension();
    let mut result = manifest.clone();

    let on_progress: Option<Arc<super::ProgressFn<UploadStatistics>>> =
        options.on_progress.map(|f| Arc::from(f));

    let num_workers = options.max_workers.unwrap_or(10);
    let max_memory = options
        .max_memory_bytes
        .unwrap_or_else(default_max_memory_bytes);

    // Build work items for all regular files — cache checks happen inside each worker task
    let (work_items, mut stats) = build_upload_work_list(&result.files, chunk_size);

    if work_items.is_empty() {
        finish_empty_upload(&mut stats, &on_progress, chunk_size);
        return Ok((result, stats));
    }

    // Process work items in parallel using tokio — each task does its own cache checks
    let ctx = Arc::new(UploadCtx {
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
        chunk_size,
        start: start_time,
        dedup: Arc::new(Mutex::new(HashMap::new())),
        hash_cache: options.hash_cache.clone(),
        force_rehash: options.force_rehash,
    });

    let file_results = run_upload_tasks(&ctx, work_items).await;

    // Apply results and write cache entries sequentially
    for r in file_results {
        let (index, fr) = r?;
        apply_file_result(
            &mut result.files[index],
            fr,
            &options.hash_cache,
            alg_str,
            chunk_size,
        );
    }

    let stats = finalize_upload_stats(&ctx.progress_stats, &ctx.rate_calc, start_time, chunk_size);

    if let Some(ref cb) = on_progress {
        let _ = cb(&stats);
    }

    Ok((result, stats))
}

/// Inputs describing one file to hash and (possibly) upload.
struct UploadWorkItem {
    index: usize,
    path: String,
    mtime: u64,
    use_chunks: bool,
    file_size: u64,
}

/// Shared, per-operation state used by every hash+upload worker task.
struct UploadCtx {
    data_cache: Arc<dyn AsyncDataCache>,
    memory_pool: Arc<MemoryPool>,
    worker_semaphore: Arc<tokio::sync::Semaphore>,
    cancelled: Arc<AtomicBool>,
    progress_stats: Arc<Mutex<UploadStatistics>>,
    rate_calc: Arc<Mutex<SlidingWindowRate>>,
    on_progress: Option<Arc<super::ProgressFn<UploadStatistics>>>,
    alg: String,
    chunk_size: i64,
    start: std::time::Instant,
    dedup: UploadDedup,
    hash_cache: Option<Arc<HashCache>>,
    force_rehash: bool,
}

/// Build the work item list for all regular (non-symlink, non-deleted)
/// files, tallying total file and byte counts into fresh statistics.
fn build_upload_work_list(
    files: &[crate::manifest::FileEntry],
    chunk_size: i64,
) -> (Vec<UploadWorkItem>, UploadStatistics) {
    let mut stats = UploadStatistics::default();
    let mut work_items = Vec::new();
    for (i, file) in files.iter().enumerate() {
        if file.symlink_target.is_some() || file.deleted {
            continue;
        }
        let file_size = file.size.unwrap_or(0);
        stats.total_files += 1;
        stats.total_bytes += file_size;
        let use_chunks =
            chunk_size > 0 && chunk_size != WHOLE_FILE_CHUNK_SIZE && file_size as i64 > chunk_size;
        work_items.push(UploadWorkItem {
            index: i,
            path: file.path.clone(),
            mtime: file.mtime.unwrap_or(0),
            use_chunks,
            file_size,
        });
    }
    (work_items, stats)
}

/// Finish statistics for the fast path where nothing needs hashing.
fn finish_empty_upload(
    stats: &mut UploadStatistics,
    on_progress: &Option<Arc<super::ProgressFn<UploadStatistics>>>,
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
        "Hashed/uploaded {} ({} {}) in 0.00s",
        crate::hash::human_readable_file_size(stats.total_bytes),
        stats.total_files,
        unit
    );
}

/// Spawn one hash+upload task per work item and await them all, preserving
/// spawn order in the returned results.
async fn run_upload_tasks(
    ctx: &Arc<UploadCtx>,
    work_items: Vec<UploadWorkItem>,
) -> Vec<crate::Result<(usize, FileResult)>> {
    let mut handles = Vec::new();
    for item in work_items {
        handles.push(tokio::spawn(hash_upload_one_file(ctx.clone(), item)));
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

/// Hash and upload one file end to end: acquire a worker permit, try a full
/// cache skip, otherwise acquire memory and hash+upload with the appropriate
/// strategy (chunked, multipart, or whole), recording progress either way.
async fn hash_upload_one_file(
    ctx: Arc<UploadCtx>,
    item: UploadWorkItem,
) -> crate::Result<(usize, FileResult)> {
    let _worker_permit = ctx
        .worker_semaphore
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| crate::SnapshotError::Task(e.to_string()))?;

    if ctx.cancelled.load(Ordering::Relaxed) {
        return Err(crate::SnapshotError::Cancelled);
    }

    // Steps 1+2: check hash cache and data cache for a full skip
    // (inside the task, parallelized)
    if let Some(fr) = check_cache_skip(&ctx, &item).await {
        update_progress(
            &ctx.progress_stats,
            &ctx.rate_calc,
            &ctx.on_progress,
            &ctx.cancelled,
            &fr,
            ctx.start,
        )?;
        return Ok((item.index, fr));
    }

    // Step 3: Need to hash+upload — acquire memory
    let _mem_permit = ctx.memory_pool.acquire(item.file_size as usize).await;

    let part_size = ctx.data_cache.multipart_part_size();
    let multipart_threshold = 2 * part_size as u64;

    let fr = if item.use_chunks {
        process_chunked_async(
            item.path,
            ctx.chunk_size as u64,
            ctx.alg.clone(),
            ctx.data_cache.clone(),
            ctx.dedup.clone(),
        )
        .await?
    } else if item.file_size >= multipart_threshold && ctx.data_cache.as_multipart().is_some() {
        process_whole_multipart(
            item.path,
            item.file_size,
            ctx.alg.clone(),
            ctx.data_cache.clone(),
            part_size,
            ctx.dedup.clone(),
        )
        .await?
    } else {
        process_whole_async(
            item.path,
            item.file_size,
            ctx.alg.clone(),
            ctx.data_cache.clone(),
            ctx.dedup.clone(),
        )
        .await?
    };

    update_progress(
        &ctx.progress_stats,
        &ctx.rate_calc,
        &ctx.on_progress,
        &ctx.cancelled,
        &fr,
        ctx.start,
    )?;
    Ok((item.index, fr))
}

/// Check the hash cache, then the data cache, for a full skip. Returns
/// `Some(FileResult::Skipped)` only when the file's hashes are fresh in the
/// hash cache and every referenced object already exists in the data cache.
async fn check_cache_skip(ctx: &UploadCtx, item: &UploadWorkItem) -> Option<FileResult> {
    let cache = ctx.hash_cache.as_ref()?;
    if ctx.force_rehash {
        return None;
    }
    let path = Path::new(&item.path);
    if item.use_chunks {
        let cached_hashes = super::cached_chunk_hashes(
            cache,
            path,
            &ctx.alg,
            ctx.chunk_size as u64,
            item.file_size,
            item.mtime,
        )?;
        if !all_objects_exist(&ctx.data_cache, &cached_hashes, &ctx.alg).await {
            return None;
        }
        debug!(path = %item.path, "skipped (cache hit)");
        Some(FileResult::Skipped {
            size: item.file_size,
            whole_hash: None,
            chunk_hashes: Some(cached_hashes),
        })
    } else {
        let cached_hash =
            cache.get_if_fresh(path, &ctx.alg, 0, WHOLE_FILE_RANGE_END, item.mtime)?;
        if !ctx
            .data_cache
            .object_exists(&cached_hash, &ctx.alg)
            .await
            .unwrap_or(false)
        {
            return None;
        }
        debug!(path = %item.path, "skipped (cache hit)");
        Some(FileResult::Skipped {
            size: item.file_size,
            whole_hash: Some(cached_hash),
            chunk_hashes: None,
        })
    }
}

/// Return true only when every hash already exists in the data cache;
/// existence-check errors count as "missing".
async fn all_objects_exist(
    data_cache: &Arc<dyn AsyncDataCache>,
    hashes: &[String],
    alg: &str,
) -> bool {
    for h in hashes {
        if !data_cache.object_exists(h, alg).await.unwrap_or(false) {
            return false;
        }
    }
    true
}

/// Store a task's [`FileResult`] into the manifest entry and record the
/// resulting hashes in the hash cache (skipped files were cached already).
fn apply_file_result(
    file: &mut crate::manifest::FileEntry,
    fr: FileResult,
    hash_cache: &Option<Arc<HashCache>>,
    alg_str: &str,
    chunk_size: i64,
) {
    let path = Path::new(&file.path);
    let mtime = file.mtime.unwrap_or(0);
    let file_size = file.size.unwrap_or(0);

    match fr {
        FileResult::Whole { hash, .. } => {
            if let Some(ref cache) = hash_cache {
                let _ = cache.put(path, alg_str, 0, WHOLE_FILE_RANGE_END, &hash, mtime);
            }
            file.hash = Some(hash);
        }
        FileResult::Chunked { hashes, .. } => {
            if let Some(ref cache) = hash_cache {
                super::put_chunk_hashes(
                    cache,
                    path,
                    alg_str,
                    chunk_size as u64,
                    file_size,
                    &hashes,
                    mtime,
                );
            }
            file.chunk_hashes = Some(hashes);
        }
        FileResult::Skipped {
            whole_hash,
            chunk_hashes,
            ..
        } => {
            if let Some(h) = whole_hash {
                file.hash = Some(h);
            } else if let Some(hs) = chunk_hashes {
                file.chunk_hashes = Some(hs);
            }
        }
    }
}

/// Fold the shared progress state into final statistics: total time, rate,
/// percentage, and the human-readable summary message.
fn finalize_upload_stats(
    progress_stats: &Mutex<UploadStatistics>,
    rate_calc: &Mutex<SlidingWindowRate>,
    start_time: std::time::Instant,
    chunk_size: i64,
) -> UploadStatistics {
    let mut stats = progress_stats.lock().unwrap().clone();

    stats.total_time = start_time.elapsed().as_secs_f64();
    {
        let mut rc = rate_calc.lock().unwrap();
        stats.rate = rc.update(stats.total_time, stats.hashed_bytes + stats.skipped_bytes);
    }
    if stats.total_bytes > 0 {
        stats.progress =
            ((stats.hashed_bytes + stats.skipped_bytes) as f64 / stats.total_bytes as f64) * 100.0;
    }

    let unit = if chunk_size <= 0 { "files" } else { "chunks" };
    let mut parts = vec![
        format!(
            "Hashed/uploaded {}",
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

fn update_progress(
    progress_stats: &Arc<Mutex<UploadStatistics>>,
    rate_calc: &Arc<Mutex<SlidingWindowRate>>,
    on_progress: &Option<Arc<super::ProgressFn<UploadStatistics>>>,
    cancelled: &Arc<AtomicBool>,
    fr: &FileResult,
    start: std::time::Instant,
) -> crate::Result<()> {
    let mut s = progress_stats.lock().unwrap();
    match fr {
        FileResult::Whole { size, uploaded, .. } => {
            s.hashed_files += 1;
            s.hashed_bytes += size;
            if *uploaded {
                s.uploaded_files += 1;
                s.uploaded_bytes += size;
            } else {
                s.skipped_files += 1;
                s.skipped_bytes += size;
            }
        }
        FileResult::Chunked {
            uploaded,
            hashed_bytes,
            ..
        } => {
            s.hashed_files += 1;
            s.hashed_bytes += hashed_bytes;
            if *uploaded {
                s.uploaded_files += 1;
            }
        }
        FileResult::Skipped { size, .. } => {
            s.skipped_files += 1;
            s.skipped_bytes += size;
        }
    }
    let elapsed = start.elapsed().as_secs_f64();
    s.total_time = elapsed;
    {
        let mut rc = rate_calc.lock().unwrap();
        s.rate = rc.update(elapsed, s.hashed_bytes + s.skipped_bytes);
    }
    if s.total_bytes > 0 {
        s.progress = ((s.hashed_bytes + s.skipped_bytes) as f64 / s.total_bytes as f64) * 100.0;
    }
    if let Some(ref cb) = on_progress {
        if !cb(&s) {
            cancelled.store(true, Ordering::Relaxed);
            return Err(crate::SnapshotError::Cancelled);
        }
    }
    Ok(())
}

async fn process_whole_async(
    path: String,
    file_size: u64,
    alg_str: String,
    data_cache: Arc<dyn AsyncDataCache>,
    dedup: UploadDedup,
) -> crate::Result<FileResult> {
    // Stage 1: CPU-bound read + hash
    let (hash, data) = tokio::task::spawn_blocking(move || {
        let data = std::fs::read(&path).map_err(|e| {
            crate::SnapshotError::Io(std::io::Error::new(e.kind(), format!("{path}: {e}")))
        })?;
        let hash = hash_data(&data);
        Ok::<_, crate::SnapshotError>((hash, data))
    })
    .await
    .map_err(|e| crate::SnapshotError::Task(e.to_string()))??;

    // Stage 2: Deduplicated upload
    let key = format!("{hash}.{alg_str}");
    let uploaded = dedup_upload(&dedup, &key, &data_cache, &hash, &alg_str, data).await?;

    Ok(FileResult::Whole {
        hash,
        uploaded,
        size: file_size,
    })
}

async fn process_whole_multipart(
    path: String,
    file_size: u64,
    alg_str: String,
    data_cache: Arc<dyn AsyncDataCache>,
    part_size: usize,
    dedup: UploadDedup,
) -> crate::Result<FileResult> {
    // Stage 1: Streaming hash
    let path2 = path.clone();
    let ps = part_size;
    let hash = tokio::task::spawn_blocking(move || {
        use std::io::Read;
        use xxhash_rust::xxh3::Xxh3Default;
        let mut f = std::fs::File::open(&path2).map_err(|e| {
            crate::SnapshotError::Io(std::io::Error::new(e.kind(), format!("{path2}: {e}")))
        })?;
        let mut hasher = Xxh3Default::new();
        let mut buf = vec![0u8; ps];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok::<_, crate::SnapshotError>(format!("{:032x}", hasher.digest128()))
    })
    .await
    .map_err(|e| crate::SnapshotError::Task(e.to_string()))??;

    // Stage 2: Check data cache, then dedup map
    if data_cache
        .object_exists(&hash, &alg_str)
        .await
        .unwrap_or(false)
    {
        return Ok(FileResult::Whole {
            hash,
            uploaded: false,
            size: file_size,
        });
    }

    let key = format!("{hash}.{alg_str}");
    let rx = {
        let mut map = dedup.lock().unwrap();
        if let Some(tx) = map.get(&key) {
            Some(tx.subscribe())
        } else {
            let (tx, _) = tokio::sync::broadcast::channel(1);
            map.insert(key.clone(), tx);
            None
        }
    };

    if let Some(mut rx) = rx {
        let _ = rx.recv().await;
        return Ok(FileResult::Whole {
            hash,
            uploaded: false,
            size: file_size,
        });
    }

    // Stage 3: Multipart upload (we own this hash)
    let mp = data_cache
        .as_multipart()
        .expect("process_whole_multipart requires MultipartDataCache support");
    let upload_id = mp
        .create_multipart_upload(&hash, &alg_str)
        .await
        .map_err(crate::SnapshotError::Io)?;

    let upload_result = async {
        let num_parts = (file_size as usize).div_ceil(part_size) as i32;
        let mut upload_handles = Vec::new();

        for part_num in 1..=num_parts {
            let offset = (part_num as u64 - 1) * part_size as u64;
            let this_part_size = std::cmp::min(part_size as u64, file_size - offset) as usize;
            let path_clone = path.clone();
            let dc = data_cache.clone();
            let h = hash.clone();
            let a = alg_str.clone();
            let uid = upload_id.clone();

            upload_handles.push(tokio::spawn(async move {
                let part_data = tokio::task::spawn_blocking(move || {
                    use std::io::{Read, Seek, SeekFrom};
                    let mut f = std::fs::File::open(&path_clone)?;
                    f.seek(SeekFrom::Start(offset))?;
                    let mut buf = vec![0u8; this_part_size];
                    f.read_exact(&mut buf)?;
                    Ok::<_, std::io::Error>(buf)
                })
                .await
                .map_err(|e| crate::SnapshotError::Task(e.to_string()))?
                .map_err(crate::SnapshotError::Io)?;

                let etag = dc
                    .as_multipart()
                    .expect("MultipartDataCache support verified above")
                    .upload_part(&h, &a, &uid, part_num, part_data)
                    .await
                    .map_err(crate::SnapshotError::Io)?;
                Ok::<_, crate::SnapshotError>((part_num, etag))
            }));
        }

        let mut parts: Vec<(i32, String)> = Vec::new();
        for handle in upload_handles {
            let (part_num, etag) = handle
                .await
                .map_err(|e| crate::SnapshotError::Task(e.to_string()))??;
            parts.push((part_num, etag));
        }
        parts.sort_by_key(|(num, _)| *num);

        mp.complete_multipart_upload(&hash, &alg_str, &upload_id, parts)
            .await
            .map_err(crate::SnapshotError::Io)?;

        Ok::<_, crate::SnapshotError>(())
    }
    .await;

    // Abort the multipart upload on failure before notifying waiters
    if let Err(ref _e) = upload_result {
        let _ = mp.abort_multipart_upload(&hash, &alg_str, &upload_id).await;
    }

    // Notify waiters and clean up dedup map regardless of success/failure
    {
        let mut map = dedup.lock().unwrap();
        if let Some(tx) = map.remove(&key) {
            let _ = tx.send(());
        }
    }

    upload_result?;

    Ok(FileResult::Whole {
        hash,
        uploaded: true,
        size: file_size,
    })
}

async fn process_chunked_async(
    path: String,
    chunk_size: u64,
    alg_str: String,
    data_cache: Arc<dyn AsyncDataCache>,
    dedup: UploadDedup,
) -> crate::Result<FileResult> {
    // Stage 1: Read and hash all chunks in blocking thread
    let chunks: Vec<(String, Vec<u8>)> = tokio::task::spawn_blocking(move || {
        use std::io::{Read, Seek};
        let mut f = std::fs::File::open(&path).map_err(|e| {
            crate::SnapshotError::Io(std::io::Error::new(e.kind(), format!("{path}: {e}")))
        })?;
        let mut result = Vec::new();
        let mut buf = vec![0u8; chunk_size as usize];
        loop {
            match f.read_exact(&mut buf) {
                Ok(()) => {
                    let hash = hash_data(&buf);
                    result.push((hash, buf.clone()));
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    let consumed = result.len() as u64 * chunk_size;
                    f.seek(std::io::SeekFrom::Start(consumed))?;
                    let mut remainder = Vec::new();
                    f.read_to_end(&mut remainder)?;
                    if !remainder.is_empty() {
                        let hash = hash_data(&remainder);
                        result.push((hash, remainder));
                    }
                    break;
                }
                Err(e) => return Err(crate::SnapshotError::Io(e)),
            }
        }
        if result.is_empty() {
            result.push((hash_data(&[]), vec![]));
        }
        Ok::<_, crate::SnapshotError>(result)
    })
    .await
    .map_err(|e| crate::SnapshotError::Task(e.to_string()))??;

    // Stage 2: Upload chunks with deduplication
    let hashed_bytes: u64 = chunks.iter().map(|(_, c)| c.len() as u64).sum();
    let mut upload_handles = Vec::with_capacity(chunks.len());
    for (hash, chunk) in chunks {
        let dc = data_cache.clone();
        let alg = alg_str.clone();
        let dd = dedup.clone();
        upload_handles.push(tokio::spawn(async move {
            let key = format!("{hash}.{alg}");
            let uploaded = dedup_upload(&dd, &key, &dc, &hash, &alg, chunk).await?;
            Ok::<_, crate::SnapshotError>((hash, uploaded))
        }));
    }
    let mut hashes = Vec::with_capacity(upload_handles.len());
    let mut any_uploaded = false;
    for handle in upload_handles {
        let (hash, uploaded) = handle
            .await
            .map_err(|e| crate::SnapshotError::Task(e.to_string()))??;
        if uploaded {
            any_uploaded = true;
        }
        hashes.push(hash);
    }

    Ok(FileResult::Chunked {
        hashes,
        uploaded: any_uploaded,
        hashed_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_cache::FileSystemDataCache;
    use crate::hash::HashAlgorithm;
    use crate::manifest::{AbsManifest, AbsSnapshot, AbsSnapshotDiff, FileEntry, Manifest};
    use crate::DEFAULT_FILE_CHUNK_SIZE;
    use std::time::UNIX_EPOCH;
    use tempfile::TempDir;

    fn make_test_file(dir: &Path, name: &str, content: &[u8]) -> (String, u64) {
        let p = dir.join(name);
        std::fs::write(&p, content).unwrap();
        let meta = std::fs::metadata(&p).unwrap();
        let mtime = meta
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;
        (p.to_string_lossy().into_owned(), mtime)
    }

    #[tokio::test]
    async fn hash_upload_produces_hashes_and_stores_data() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        let (path, mtime) = make_test_file(tmp.path(), "a.txt", b"hello");

        let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(vec![FileEntry::file(&path, 5, mtime)]);

        let data_cache: Arc<dyn AsyncDataCache> =
            Arc::new(FileSystemDataCache::new(cache_dir.path().join("data")).unwrap());
        let result = hash_upload_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            data_cache.clone(),
            HashUploadOptions::default(),
        )
        .await
        .unwrap();

        let hash = result.manifest.files()[0].hash.as_ref().unwrap();
        assert!(data_cache.object_exists(hash, "xxh128").await.unwrap());
        let stored = data_cache.get_object(hash, "xxh128").await.unwrap();
        assert_eq!(stored, b"hello");
        assert_eq!(result.statistics.uploaded_files, 1);
        assert_eq!(result.statistics.uploaded_bytes, 5);
    }

    #[tokio::test]
    async fn second_upload_skips() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        let (path, mtime) = make_test_file(tmp.path(), "a.txt", b"hello");

        let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(vec![FileEntry::file(&path, 5, mtime)]);

        let data_cache: Arc<dyn AsyncDataCache> =
            Arc::new(FileSystemDataCache::new(cache_dir.path().join("data")).unwrap());

        let _ = hash_upload_abs_manifest(
            &AbsManifest::Snapshot(manifest.clone()),
            data_cache.clone(),
            HashUploadOptions::default(),
        )
        .await
        .unwrap();

        let result = hash_upload_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            data_cache.clone(),
            HashUploadOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(result.statistics.uploaded_files, 0);
        assert_eq!(result.statistics.skipped_files, 1);
    }

    #[tokio::test]
    async fn hash_cache_enables_full_skip() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        let hc_dir = TempDir::new().unwrap();
        let (path, mtime) = make_test_file(tmp.path(), "a.txt", b"hello");

        let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(vec![FileEntry::file(&path, 5, mtime)]);

        let data_cache: Arc<dyn AsyncDataCache> =
            Arc::new(FileSystemDataCache::new(cache_dir.path().join("data")).unwrap());
        let hash_cache = Arc::new(HashCache::new(hc_dir.path()).unwrap());

        let _ = hash_upload_abs_manifest(
            &AbsManifest::Snapshot(manifest.clone()),
            data_cache.clone(),
            HashUploadOptions {
                hash_cache: Some(hash_cache.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let result = hash_upload_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            data_cache.clone(),
            HashUploadOptions {
                hash_cache: Some(hash_cache),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(result.statistics.skipped_files, 1);
        assert_eq!(result.statistics.hashed_files, 0);
        assert_eq!(result.statistics.uploaded_files, 0);
    }

    #[tokio::test]
    async fn symlinks_and_deleted_pass_through() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        let (path, mtime) = make_test_file(tmp.path(), "real.txt", b"data");

        let manifest: AbsSnapshotDiff =
            Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![
                FileEntry::file(&path, 4, mtime),
                FileEntry::symlink("/tmp/link", "/tmp/target"),
                FileEntry::deleted("/tmp/gone"),
            ]);

        let data_cache: Arc<dyn AsyncDataCache> =
            Arc::new(FileSystemDataCache::new(cache_dir.path().join("data")).unwrap());
        let result = hash_upload_abs_manifest(
            &AbsManifest::Diff(manifest),
            data_cache.clone(),
            HashUploadOptions::default(),
        )
        .await
        .unwrap();

        assert!(result.manifest.files()[0].hash.is_some());
        assert!(result.manifest.files()[1].hash.is_none());
        assert!(result.manifest.files()[2].hash.is_none());
        assert_eq!(result.statistics.total_files, 1);
    }

    #[tokio::test]
    async fn rejects_already_hashed_files() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        let (path, mtime) = make_test_file(tmp.path(), "a.txt", b"hello");
        let mut entry = FileEntry::file(&path, 5, mtime);
        entry.hash = Some("existing_hash".into());

        let manifest: AbsSnapshot =
            Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![entry]);

        let data_cache: Arc<dyn AsyncDataCache> =
            Arc::new(FileSystemDataCache::new(cache_dir.path().join("data")).unwrap());
        let result = hash_upload_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            data_cache.clone(),
            HashUploadOptions::default(),
        )
        .await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("already has hashes set"));
    }

    #[tokio::test]
    async fn chunked_upload() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("chunked_upload.bin");
        let data = vec![42u8; 1024];
        std::fs::write(&file_path, &data).unwrap();
        let meta = std::fs::metadata(&file_path).unwrap();
        let mtime = meta
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;

        let cache_dir = TempDir::new().unwrap();
        let chunk_size = 256i64;
        let manifest: AbsSnapshot =
            Manifest::new(HashAlgorithm::Xxh128, chunk_size).with_files(vec![FileEntry::file(
                file_path.to_string_lossy().to_string(),
                1024,
                mtime,
            )]);

        let data_cache: Arc<dyn AsyncDataCache> =
            Arc::new(FileSystemDataCache::new(cache_dir.path().join("data")).unwrap());
        let result = hash_upload_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            data_cache.clone(),
            HashUploadOptions {
                file_chunk_size_bytes: Some(chunk_size),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let f = &result.manifest.files()[0];
        assert!(f.hash.is_none());
        let chunks = f.chunk_hashes.as_ref().unwrap();
        assert_eq!(chunks.len(), 4);

        for h in chunks {
            assert!(data_cache.object_exists(h, "xxh128").await.unwrap());
            assert_eq!(data_cache.get_object(h, "xxh128").await.unwrap().len(), 256);
        }
    }
}
