use super::memory_pool::{default_max_memory_bytes, num_cpus, MemoryPool};
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
pub fn hash_upload_abs_manifest(
    manifest: &AbsManifest,
    data_cache: Arc<dyn AsyncDataCache>,
    options: HashUploadOptions,
) -> crate::Result<UploadResult> {
    match manifest {
        AbsManifest::Snapshot(s) => {
            let (result, stats) = hash_upload_manifest(s, data_cache, options)?;
            Ok(UploadResult {
                manifest: AbsManifest::Snapshot(result),
                statistics: stats,
            })
        }
        AbsManifest::Diff(d) => {
            let (result, stats) = hash_upload_manifest(d, data_cache, options)?;
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

fn hash_upload_manifest<P: Clone + Send + Sync, K: Clone + Send + Sync>(
    manifest: &Manifest<P, K>,
    data_cache: Arc<dyn AsyncDataCache>,
    options: HashUploadOptions,
) -> crate::Result<(Manifest<P, K>, UploadStatistics)> {
    let start_time = std::time::Instant::now();

    // Validate no regular files already have hashes
    for file in &manifest.files {
        if file.symlink_target.is_none()
            && !file.deleted
            && (file.hash.is_some() || file.chunk_hashes.is_some())
        {
            return Err(crate::SnapshotError::Validation(format!(
                "file already has hashes set, cannot re-hash: {}",
                file.path
            )));
        }
    }

    let chunk_size = options
        .file_chunk_size_bytes
        .unwrap_or(manifest.file_chunk_size_bytes);
    let alg_str = manifest.hash_alg.extension();
    let mut result = manifest.clone();
    let mut stats = UploadStatistics::default();

    let on_progress: Option<Arc<super::ProgressFn<UploadStatistics>>> =
        options.on_progress.map(|f| Arc::from(f));

    let num_workers = options.max_workers.unwrap_or(10);
    let max_memory = options
        .max_memory_bytes
        .unwrap_or_else(default_max_memory_bytes);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(num_cpus().max(4))
        .enable_all()
        .build()
        .map_err(|e| crate::SnapshotError::Task(e.to_string()))?;

    struct WorkItem {
        index: usize,
        path: String,
        mtime: u64,
        use_chunks: bool,
        file_size: u64,
    }

    // Build work items for all regular files — cache checks happen inside each worker task
    let mut work_items = Vec::new();
    for (i, file) in result.files.iter().enumerate() {
        if file.symlink_target.is_some() || file.deleted {
            continue;
        }
        let file_size = file.size.unwrap_or(0);
        stats.total_files += 1;
        stats.total_bytes += file_size;
        let use_chunks =
            chunk_size > 0 && chunk_size != WHOLE_FILE_CHUNK_SIZE && file_size as i64 > chunk_size;
        work_items.push(WorkItem {
            index: i,
            path: file.path.clone(),
            mtime: file.mtime.unwrap_or(0),
            use_chunks,
            file_size,
        });
    }

    if work_items.is_empty() {
        if stats.total_bytes > 0 {
            stats.progress = 100.0;
        }
        if let Some(ref cb) = on_progress {
            let _ = cb(&stats);
        }
        let unit = if chunk_size <= 0 { "files" } else { "chunks" };
        stats.progress_message = format!(
            "Hashed/uploaded {} ({} {}) in 0.00s",
            crate::hash::human_readable_file_size(stats.total_bytes),
            stats.total_files,
            unit
        );
        return Ok((result, stats));
    }

    // Process work items in parallel using tokio — each task does its own cache checks
    let cancelled = Arc::new(AtomicBool::new(false));
    let progress_stats = Arc::new(Mutex::new(stats.clone()));
    let rate_calc = Arc::new(Mutex::new(SlidingWindowRate::new()));
    let memory_pool = Arc::new(MemoryPool::new(max_memory));
    let worker_semaphore = Arc::new(tokio::sync::Semaphore::new(num_workers));
    let upload_dedup: UploadDedup = Arc::new(Mutex::new(HashMap::new()));
    let hash_cache_arc: Option<Arc<HashCache>> = options.hash_cache.clone();
    let force_rehash = options.force_rehash;

    let file_results: Vec<crate::Result<(usize, FileResult)>> = rt.block_on(async {
        let mut handles = Vec::new();

        for item in work_items {
            let dc = data_cache.clone();
            let pool = memory_pool.clone();
            let cancelled = cancelled.clone();
            let progress_stats = progress_stats.clone();
            let rate_calc = rate_calc.clone();
            let on_progress = on_progress.clone();
            let worker_sem = worker_semaphore.clone();
            let alg = alg_str.to_string();
            let cs = chunk_size;
            let start = start_time;
            let dedup = upload_dedup.clone();
            let hc = hash_cache_arc.clone();

            let handle = tokio::spawn(async move {
                let _worker_permit = worker_sem
                    .acquire_owned()
                    .await
                    .map_err(|e| crate::SnapshotError::Task(e.to_string()))?;

                if cancelled.load(Ordering::Relaxed) {
                    return Err(crate::SnapshotError::Cancelled);
                }

                // Step 1: Check hash cache (inside the task, parallelized)
                if let Some(ref cache) = hc {
                    if !force_rehash {
                        let path = Path::new(&item.path);
                        if item.use_chunks {
                            let csu = cs as u64;
                            let mut all_cached = true;
                            let mut cached_hashes = Vec::new();
                            let mut offset: u64 = 0;
                            while offset < item.file_size {
                                let end = std::cmp::min(offset + csu, item.file_size);
                                if let Some(h) = cache.get_if_fresh(
                                    path,
                                    &alg,
                                    offset as i64,
                                    end as i64,
                                    item.mtime,
                                ) {
                                    cached_hashes.push(h);
                                } else {
                                    all_cached = false;
                                    break;
                                }
                                offset = end;
                            }
                            if all_cached && !cached_hashes.is_empty() {
                                // Step 2: Check if all chunks exist in data cache
                                let mut all_exist = true;
                                for h in &cached_hashes {
                                    if !dc.object_exists(h, &alg).await.unwrap_or(false) {
                                        all_exist = false;
                                        break;
                                    }
                                }
                                if all_exist {
                                    debug!(path = %item.path, "skipped (cache hit)");
                                    let fr = FileResult::Skipped {
                                        size: item.file_size,
                                        whole_hash: None,
                                        chunk_hashes: Some(cached_hashes),
                                    };
                                    update_progress(
                                        &progress_stats,
                                        &rate_calc,
                                        &on_progress,
                                        &cancelled,
                                        &fr,
                                        start,
                                    )?;
                                    return Ok((item.index, fr));
                                }
                            }
                        } else if let Some(cached_hash) =
                            cache.get_if_fresh(path, &alg, 0, WHOLE_FILE_RANGE_END, item.mtime)
                        {
                            // Step 2: Check if object exists in data cache
                            if dc.object_exists(&cached_hash, &alg).await.unwrap_or(false) {
                                debug!(path = %item.path, "skipped (cache hit)");
                                let fr = FileResult::Skipped {
                                    size: item.file_size,
                                    whole_hash: Some(cached_hash),
                                    chunk_hashes: None,
                                };
                                update_progress(
                                    &progress_stats,
                                    &rate_calc,
                                    &on_progress,
                                    &cancelled,
                                    &fr,
                                    start,
                                )?;
                                return Ok((item.index, fr));
                            }
                        }
                    }
                }

                // Step 3: Need to hash+upload — acquire memory
                let _mem_permit = pool.acquire(item.file_size as usize).await;

                let part_size = dc.multipart_part_size();
                let multipart_threshold = 2 * part_size as u64;

                let fr = if item.use_chunks {
                    process_chunked_async(item.path, cs as u64, alg, dc, dedup).await?
                } else if item.file_size >= multipart_threshold {
                    process_whole_multipart(item.path, item.file_size, alg, dc, part_size, dedup)
                        .await?
                } else {
                    process_whole_async(item.path, item.file_size, alg, dc, dedup).await?
                };

                update_progress(
                    &progress_stats,
                    &rate_calc,
                    &on_progress,
                    &cancelled,
                    &fr,
                    start,
                )?;
                Ok((item.index, fr))
            });

            handles.push(handle);
        }

        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(r) => results.push(r),
                Err(e) => results.push(Err(crate::SnapshotError::Task(e.to_string()))),
            }
        }
        results
    });

    // Apply results and write cache entries sequentially
    for r in file_results {
        let (index, fr) = r?;
        let file = &mut result.files[index];
        let path = Path::new(&file.path);
        let mtime = file.mtime.unwrap_or(0);
        let file_size = file.size.unwrap_or(0);

        match fr {
            FileResult::Whole { hash, .. } => {
                if let Some(ref cache) = options.hash_cache {
                    let _ = cache.put(path, alg_str, 0, WHOLE_FILE_RANGE_END, &hash, mtime);
                }
                file.hash = Some(hash);
            }
            FileResult::Chunked { hashes, .. } => {
                if let Some(ref cache) = options.hash_cache {
                    let cs = chunk_size as u64;
                    let mut offset: u64 = 0;
                    for h in &hashes {
                        let end = std::cmp::min(offset + cs, file_size);
                        let _ = cache.put(path, alg_str, offset as i64, end as i64, h, mtime);
                        offset = end;
                    }
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

    stats = progress_stats.lock().unwrap().clone();

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

    if let Some(ref cb) = on_progress {
        let _ = cb(&stats);
    }

    Ok((result, stats))
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
    let upload_id = data_cache
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

        data_cache
            .complete_multipart_upload(&hash, &alg_str, &upload_id, parts)
            .await
            .map_err(crate::SnapshotError::Io)?;

        Ok::<_, crate::SnapshotError>(())
    }
    .await;

    // Abort the multipart upload on failure before notifying waiters
    if let Err(ref _e) = upload_result {
        let _ = data_cache
            .abort_multipart_upload(&hash, &alg_str, &upload_id)
            .await;
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

    #[test]
    fn hash_upload_produces_hashes_and_stores_data() {
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
        .unwrap();

        let hash = result.manifest.files()[0].hash.as_ref().unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert!(rt
            .block_on(data_cache.object_exists(hash, "xxh128"))
            .unwrap());
        let stored = rt.block_on(data_cache.get_object(hash, "xxh128")).unwrap();
        assert_eq!(stored, b"hello");
        assert_eq!(result.statistics.uploaded_files, 1);
        assert_eq!(result.statistics.uploaded_bytes, 5);
    }

    #[test]
    fn second_upload_skips() {
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
        .unwrap();

        let result = hash_upload_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            data_cache.clone(),
            HashUploadOptions::default(),
        )
        .unwrap();
        assert_eq!(result.statistics.uploaded_files, 0);
        assert_eq!(result.statistics.skipped_files, 1);
    }

    #[test]
    fn hash_cache_enables_full_skip() {
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
        .unwrap();

        let result = hash_upload_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            data_cache.clone(),
            HashUploadOptions {
                hash_cache: Some(hash_cache),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result.statistics.skipped_files, 1);
        assert_eq!(result.statistics.hashed_files, 0);
        assert_eq!(result.statistics.uploaded_files, 0);
    }

    #[test]
    fn symlinks_and_deleted_pass_through() {
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
        .unwrap();

        assert!(result.manifest.files()[0].hash.is_some());
        assert!(result.manifest.files()[1].hash.is_none());
        assert!(result.manifest.files()[2].hash.is_none());
        assert_eq!(result.statistics.total_files, 1);
    }

    #[test]
    fn rejects_already_hashed_files() {
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
        );
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("already has hashes set"));
    }

    #[test]
    fn chunked_upload() {
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
        .unwrap();

        let f = &result.manifest.files()[0];
        assert!(f.hash.is_none());
        let chunks = f.chunk_hashes.as_ref().unwrap();
        assert_eq!(chunks.len(), 4);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        for h in chunks {
            assert!(rt.block_on(data_cache.object_exists(h, "xxh128")).unwrap());
            assert_eq!(
                rt.block_on(data_cache.get_object(h, "xxh128"))
                    .unwrap()
                    .len(),
                256
            );
        }
    }
}
