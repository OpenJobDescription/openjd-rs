// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use super::memory_pool::{default_max_memory_bytes, MemoryPool};
use super::rate::SlidingWindowRate;
use crate::data_cache::{AsyncDataCache, CopyResult, MultipartDataCache, RangeReadDataCache};
use crate::manifest::ManifestRef;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct CacheSyncOptions {
    pub max_workers: Option<usize>,
    pub max_memory_bytes: Option<usize>,
    pub on_progress: Option<Box<super::ProgressFn<CacheSyncStatistics>>>,
}

#[derive(Debug)]
pub struct CacheSyncResult {
    pub statistics: CacheSyncStatistics,
}

#[derive(Debug, Default, Clone)]
pub struct CacheSyncStatistics {
    pub total_objects: usize,
    pub total_bytes: u64,
    pub copied_objects: usize,
    pub copied_bytes: u64,
    pub skipped_objects: usize,
    pub skipped_bytes: u64,
    pub total_time: f64,
    pub rate: f64,
    pub progress: f64,
    pub progress_message: String,
}

/// Transfer a single object from source to destination cache.
/// Uses multipart upload for objects >= 2 * part_size when both sides support it;
/// otherwise falls back to single get+put.
async fn transfer_object(
    src: &dyn AsyncDataCache,
    dst: &dyn AsyncDataCache,
    hash: &str,
    alg: &str,
    size_est: u64,
    memory_pool: &MemoryPool,
) -> crate::Result<u64> {
    let part_size = dst.multipart_part_size();
    let multipart_threshold = 2 * part_size as u64;

    if size_est >= multipart_threshold {
        if let (Some(dst_mp), Some(src_rr)) = (dst.as_multipart(), src.as_range_read()) {
            return transfer_object_multipart(
                src_rr,
                dst_mp,
                hash,
                alg,
                size_est,
                part_size,
                memory_pool,
            )
            .await;
        }
    }
    let _mem_permit = memory_pool.acquire(size_est as usize).await;
    let data = src
        .get_object(hash, alg)
        .await
        .map_err(crate::SnapshotError::Io)?;
    let actual_size = data.len() as u64;
    dst.put_object(hash, alg, data)
        .await
        .map_err(crate::SnapshotError::Io)?;
    Ok(actual_size)
}

async fn transfer_object_multipart(
    src: &dyn RangeReadDataCache,
    dst: &dyn MultipartDataCache,
    hash: &str,
    alg: &str,
    size_est: u64,
    part_size: usize,
    memory_pool: &MemoryPool,
) -> crate::Result<u64> {
    use futures_util::stream::{FuturesUnordered, StreamExt};

    let upload_id = dst
        .create_multipart_upload(hash, alg)
        .await
        .map_err(crate::SnapshotError::Io)?;

    let num_parts = (size_est as usize).div_ceil(part_size) as i32;
    let mut futures = FuturesUnordered::new();
    let mut parts = Vec::with_capacity(num_parts as usize);
    let uid: &str = &upload_id;

    for part_num in 1..=num_parts {
        let start = (part_num as u64 - 1) * part_size as u64;
        let end = std::cmp::min(start + part_size as u64 - 1, size_est.saturating_sub(1));

        futures.push(async move {
            let _permit = memory_pool.acquire(part_size).await;
            let data = src
                .get_object_range(hash, alg, start, end)
                .await
                .map_err(crate::SnapshotError::Io)?;
            let etag = dst
                .upload_part(hash, alg, uid, part_num, data)
                .await
                .map_err(crate::SnapshotError::Io)?;
            Ok::<_, crate::SnapshotError>((part_num, etag))
        });
    }

    let result: crate::Result<Vec<(i32, String)>> = async {
        while let Some(res) = futures.next().await {
            parts.push(res?);
        }
        parts.sort_by_key(|(num, _)| *num);
        Ok(parts)
    }
    .await;

    match result {
        Ok(parts) => {
            dst.complete_multipart_upload(hash, alg, &upload_id, parts)
                .await
                .map_err(crate::SnapshotError::Io)?;
            Ok(size_est)
        }
        Err(e) => {
            let _ = dst.abort_multipart_upload(hash, alg, &upload_id).await;
            Err(e)
        }
    }
}

/// Copies data between two AsyncDataCache instances for all hashes referenced by manifest files.
pub async fn cache_sync_manifest(
    manifests: &[&dyn ManifestRef],
    source: Arc<dyn AsyncDataCache>,
    destination: Arc<dyn AsyncDataCache>,
    options: CacheSyncOptions,
) -> crate::Result<CacheSyncResult> {
    let start_time = std::time::Instant::now();

    // Extract unique (hash, alg_ext, size_estimate) triples across all manifests
    let mut seen = HashSet::new();
    let mut work_items: Vec<(String, String, u64)> = Vec::new();

    for manifest in manifests {
        let alg_ext = manifest.hash_alg().extension().to_string();
        let file_chunk_size_bytes = manifest.file_chunk_size_bytes();

        for file in manifest.files() {
            if file.symlink_target.is_some() || file.deleted {
                continue;
            }
            if let Some(ref hash) = file.hash {
                let key = (hash.clone(), alg_ext.clone());
                if seen.insert(key) {
                    work_items.push((hash.clone(), alg_ext.clone(), file.size.unwrap_or(0)));
                }
            } else if let Some(ref chunks) = file.chunk_hashes {
                let num_chunks = chunks.len().max(1) as u64;
                let chunk_est = if file_chunk_size_bytes > 0 {
                    file_chunk_size_bytes as u64
                } else {
                    file.size.unwrap_or(0) / num_chunks
                };
                for ch in chunks {
                    let key = (ch.clone(), alg_ext.clone());
                    if seen.insert(key) {
                        work_items.push((ch.clone(), alg_ext.clone(), chunk_est));
                    }
                }
            }
        }
    }

    let mut stats = CacheSyncStatistics {
        total_objects: work_items.len(),
        total_bytes: work_items.iter().map(|(_, _, s)| *s).sum(),
        ..Default::default()
    };

    let on_progress: Option<Arc<super::ProgressFn<CacheSyncStatistics>>> =
        options.on_progress.map(|f| Arc::from(f));

    if work_items.is_empty() {
        stats.progress = 100.0;
        stats.progress_message = "Synced 0 B (0 objects) in 0.00s".into();
        if let Some(ref cb) = on_progress {
            let _ = cb(&stats);
        }
        return Ok(CacheSyncResult { statistics: stats });
    }

    let num_workers = options.max_workers.unwrap_or(10);
    let max_memory = options
        .max_memory_bytes
        .unwrap_or_else(default_max_memory_bytes);

    let cancelled = Arc::new(AtomicBool::new(false));
    let progress_stats = Arc::new(Mutex::new(stats.clone()));
    let rate_calc = Arc::new(Mutex::new(SlidingWindowRate::new()));
    let memory_pool = Arc::new(MemoryPool::new(max_memory));
    let worker_semaphore = Arc::new(tokio::sync::Semaphore::new(num_workers));

    let mut handles = Vec::new();

    for (hash, alg, size_est) in work_items {
        let src = source.clone();
        let dst = destination.clone();
        let pool = memory_pool.clone();
        let cancelled = cancelled.clone();
        let progress_stats = progress_stats.clone();
        let rate_calc = rate_calc.clone();
        let on_progress = on_progress.clone();
        let worker_sem = worker_semaphore.clone();
        let start = start_time;

        handles.push(tokio::spawn(async move {
            let _worker_permit = worker_sem
                .acquire_owned()
                .await
                .map_err(|e| crate::SnapshotError::Task(e.to_string()))?;

            if cancelled.load(Ordering::Relaxed) {
                return Err(crate::SnapshotError::Cancelled);
            }

            // Check if destination already has this object
            if dst.object_exists(&hash, &alg).await.unwrap_or(false) {
                let mut s = progress_stats.lock().unwrap();
                s.skipped_objects += 1;
                s.skipped_bytes += size_est;
                let elapsed = start.elapsed().as_secs_f64();
                s.total_time = elapsed;
                {
                    let mut rc = rate_calc.lock().unwrap();
                    s.rate = rc.update(elapsed, s.copied_bytes + s.skipped_bytes);
                }
                if s.total_bytes > 0 {
                    s.progress =
                        ((s.copied_bytes + s.skipped_bytes) as f64 / s.total_bytes as f64) * 100.0;
                }
                if let Some(ref cb) = on_progress {
                    if !cb(&s) {
                        cancelled.store(true, Ordering::Relaxed);
                        return Err(crate::SnapshotError::Cancelled);
                    }
                }
                return Ok(());
            }

            // Try server-side copy first (no memory permit needed)
            match dst.copy_from(src.as_ref(), &hash, &alg).await {
                Ok(CopyResult::ServerSideCopy) => {
                    let mut s = progress_stats.lock().unwrap();
                    s.copied_objects += 1;
                    s.copied_bytes += size_est;
                    let elapsed = start.elapsed().as_secs_f64();
                    s.total_time = elapsed;
                    {
                        let mut rc = rate_calc.lock().unwrap();
                        s.rate = rc.update(elapsed, s.copied_bytes + s.skipped_bytes);
                    }
                    if s.total_bytes > 0 {
                        s.progress = ((s.copied_bytes + s.skipped_bytes) as f64
                            / s.total_bytes as f64)
                            * 100.0;
                    }
                    if let Some(ref cb) = on_progress {
                        if !cb(&s) {
                            cancelled.store(true, Ordering::Relaxed);
                            return Err(crate::SnapshotError::Cancelled);
                        }
                    }
                    return Ok(());
                }
                Ok(CopyResult::NotSupported) | Err(_) => {
                    // Fall through to get+put
                }
            }

            let actual_size =
                transfer_object(src.as_ref(), dst.as_ref(), &hash, &alg, size_est, &pool).await?;

            {
                let mut s = progress_stats.lock().unwrap();
                s.copied_objects += 1;
                s.copied_bytes += actual_size;
                let elapsed = start.elapsed().as_secs_f64();
                s.total_time = elapsed;
                {
                    let mut rc = rate_calc.lock().unwrap();
                    s.rate = rc.update(elapsed, s.copied_bytes + s.skipped_bytes);
                }
                if s.total_bytes > 0 {
                    s.progress =
                        ((s.copied_bytes + s.skipped_bytes) as f64 / s.total_bytes as f64) * 100.0;
                }
                if let Some(ref cb) = on_progress {
                    if !cb(&s) {
                        cancelled.store(true, Ordering::Relaxed);
                        return Err(crate::SnapshotError::Cancelled);
                    }
                }
            }

            Ok(())
        }));
    }

    let mut results = Vec::new();
    for handle in handles {
        match handle.await {
            Ok(r) => results.push(r),
            Err(e) => results.push(Err(crate::SnapshotError::Task(e.to_string()))),
        }
    }

    // Check for errors
    for r in results {
        r?;
    }

    stats = progress_stats.lock().unwrap().clone();
    stats.total_time = start_time.elapsed().as_secs_f64();
    {
        let mut rc = rate_calc.lock().unwrap();
        stats.rate = rc.update(stats.total_time, stats.copied_bytes + stats.skipped_bytes);
    }
    if stats.total_bytes > 0 {
        stats.progress =
            ((stats.copied_bytes + stats.skipped_bytes) as f64 / stats.total_bytes as f64) * 100.0;
    }

    let mut parts = vec![
        format!(
            "Synced {}",
            crate::hash::human_readable_file_size(stats.total_bytes)
        ),
        format!("({} objects)", stats.total_objects),
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

    Ok(CacheSyncResult { statistics: stats })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_cache::FileSystemDataCache;
    use crate::hash::HashAlgorithm;
    use crate::manifest::{FileEntry, Manifest, RelManifest};
    use tempfile::TempDir;

    fn test_manifest(files: Vec<FileEntry>) -> RelManifest {
        RelManifest::Snapshot(Manifest::new(HashAlgorithm::Xxh128, -1).with_files(files))
    }

    fn make_caches() -> (
        TempDir,
        TempDir,
        Arc<dyn AsyncDataCache>,
        Arc<dyn AsyncDataCache>,
    ) {
        let src_dir = TempDir::new().unwrap();
        let dst_dir = TempDir::new().unwrap();
        let src: Arc<dyn AsyncDataCache> =
            Arc::new(FileSystemDataCache::new(src_dir.path().join("data")).unwrap());
        let dst: Arc<dyn AsyncDataCache> =
            Arc::new(FileSystemDataCache::new(dst_dir.path().join("data")).unwrap());
        (src_dir, dst_dir, src, dst)
    }

    async fn put(cache: &Arc<dyn AsyncDataCache>, hash: &str, alg: &str, data: &[u8]) {
        cache.put_object(hash, alg, data.to_vec()).await.unwrap();
    }

    async fn exists(cache: &Arc<dyn AsyncDataCache>, hash: &str, alg: &str) -> bool {
        cache.object_exists(hash, alg).await.unwrap()
    }

    async fn get(cache: &Arc<dyn AsyncDataCache>, hash: &str, alg: &str) -> Vec<u8> {
        cache.get_object(hash, alg).await.unwrap()
    }

    #[tokio::test]
    async fn sync_copies_data() {
        let (_sd, _dd, src, dst) = make_caches();
        put(&src, "hash_a", "xxh128", b"hello").await;
        put(&src, "hash_b", "xxh128", b"world").await;

        let manifest = test_manifest(vec![
            {
                let mut e = FileEntry::new("a.txt");
                e.hash = Some("hash_a".into());
                e.size = Some(5);
                e
            },
            {
                let mut e = FileEntry::new("b.txt");
                e.hash = Some("hash_b".into());
                e.size = Some(5);
                e
            },
        ]);

        let result = cache_sync_manifest(
            &[&manifest as &dyn ManifestRef],
            src,
            dst.clone(),
            CacheSyncOptions::default(),
        )
        .await
        .unwrap();

        assert_eq!(result.statistics.copied_objects, 2);
        assert!(exists(&dst, "hash_a", "xxh128").await);
        assert_eq!(get(&dst, "hash_b", "xxh128").await, b"world");
    }

    #[tokio::test]
    async fn sync_skips_existing() {
        let (_sd, _dd, src, dst) = make_caches();
        put(&src, "hash_a", "xxh128", b"hello").await;
        put(&src, "hash_b", "xxh128", b"world").await;
        put(&dst, "hash_a", "xxh128", b"hello").await;

        let manifest = test_manifest(vec![
            {
                let mut e = FileEntry::new("a.txt");
                e.hash = Some("hash_a".into());
                e.size = Some(5);
                e
            },
            {
                let mut e = FileEntry::new("b.txt");
                e.hash = Some("hash_b".into());
                e.size = Some(5);
                e
            },
        ]);

        let result = cache_sync_manifest(
            &[&manifest as &dyn ManifestRef],
            src,
            dst,
            CacheSyncOptions::default(),
        )
        .await
        .unwrap();

        assert_eq!(result.statistics.skipped_objects, 1);
        assert_eq!(result.statistics.copied_objects, 1);
    }

    #[tokio::test]
    async fn sync_handles_chunk_hashes() {
        let (_sd, _dd, src, dst) = make_caches();
        put(&src, "chunk_0", "xxh128", b"aaa").await;
        put(&src, "chunk_1", "xxh128", b"bbb").await;

        let manifest =
            RelManifest::Snapshot(Manifest::new(HashAlgorithm::Xxh128, 3).with_files(vec![{
                let mut e = FileEntry::new("big.bin");
                e.size = Some(6);
                e.chunk_hashes = Some(vec!["chunk_0".into(), "chunk_1".into()]);
                e
            }]));

        let result = cache_sync_manifest(
            &[&manifest as &dyn ManifestRef],
            src,
            dst.clone(),
            CacheSyncOptions::default(),
        )
        .await
        .unwrap();

        assert_eq!(result.statistics.copied_objects, 2);
        assert!(exists(&dst, "chunk_0", "xxh128").await);
        assert!(exists(&dst, "chunk_1", "xxh128").await);
    }

    #[tokio::test]
    async fn sync_skips_symlinks_deleted_unhashed() {
        let (_sd, _dd, src, dst) = make_caches();
        put(&src, "hash_real", "xxh128", b"data").await;

        let manifest = test_manifest(vec![
            {
                let mut e = FileEntry::new("real.txt");
                e.hash = Some("hash_real".into());
                e.size = Some(4);
                e
            },
            FileEntry::symlink("link", "target"),
            FileEntry::deleted("gone"),
            FileEntry::new("unhashed.txt"),
        ]);

        let result = cache_sync_manifest(
            &[&manifest as &dyn ManifestRef],
            src,
            dst,
            CacheSyncOptions::default(),
        )
        .await
        .unwrap();

        assert_eq!(result.statistics.total_objects, 1);
        assert_eq!(result.statistics.copied_objects, 1);
    }

    #[tokio::test]
    async fn sync_deduplicates_hashes() {
        let (_sd, _dd, src, dst) = make_caches();
        put(&src, "same_hash", "xxh128", b"dup").await;

        let manifest = test_manifest(vec![
            {
                let mut e = FileEntry::new("a.txt");
                e.hash = Some("same_hash".into());
                e.size = Some(3);
                e
            },
            {
                let mut e = FileEntry::new("b.txt");
                e.hash = Some("same_hash".into());
                e.size = Some(3);
                e
            },
        ]);

        let result = cache_sync_manifest(
            &[&manifest as &dyn ManifestRef],
            src,
            dst,
            CacheSyncOptions::default(),
        )
        .await
        .unwrap();

        assert_eq!(result.statistics.total_objects, 1);
        assert_eq!(result.statistics.copied_objects, 1);
    }

    #[tokio::test]
    async fn sync_uses_multipart_for_large_objects() {
        use std::collections::HashMap;
        use std::sync::atomic::{AtomicUsize, Ordering};

        /// Wrapper that sets a small part_size so small test data triggers multipart.
        struct SmallPartCache {
            inner: Arc<dyn AsyncDataCache>,
            upload_part_calls: Arc<AtomicUsize>,
            #[allow(clippy::type_complexity)]
            parts: Arc<Mutex<HashMap<String, Vec<(i32, Vec<u8>)>>>>,
        }

        #[async_trait::async_trait]
        impl AsyncDataCache for SmallPartCache {
            fn object_key(&self, h: &str, a: &str) -> String {
                self.inner.object_key(h, a)
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn multipart_part_size(&self) -> usize {
                5
            }
            fn as_multipart(&self) -> Option<&dyn MultipartDataCache> {
                Some(self)
            }
            fn as_range_read(&self) -> Option<&dyn RangeReadDataCache> {
                Some(self)
            }
            async fn object_exists(&self, h: &str, a: &str) -> std::io::Result<bool> {
                self.inner.object_exists(h, a).await
            }
            async fn put_object(&self, h: &str, a: &str, d: Vec<u8>) -> std::io::Result<String> {
                self.inner.put_object(h, a, d).await
            }
            async fn get_object(&self, h: &str, a: &str) -> std::io::Result<Vec<u8>> {
                self.inner.get_object(h, a).await
            }
        }

        #[async_trait::async_trait]
        impl MultipartDataCache for SmallPartCache {
            async fn create_multipart_upload(&self, _h: &str, _a: &str) -> std::io::Result<String> {
                Ok("test-upload-id".into())
            }
            async fn upload_part(
                &self,
                h: &str,
                a: &str,
                _uid: &str,
                pn: i32,
                data: Vec<u8>,
            ) -> std::io::Result<String> {
                self.upload_part_calls.fetch_add(1, Ordering::Relaxed);
                let key = format!("{h}.{a}");
                self.parts
                    .lock()
                    .unwrap()
                    .entry(key)
                    .or_default()
                    .push((pn, data));
                Ok(format!("etag-{pn}"))
            }
            async fn complete_multipart_upload(
                &self,
                h: &str,
                a: &str,
                _uid: &str,
                parts: Vec<(i32, String)>,
            ) -> std::io::Result<()> {
                let key = format!("{h}.{a}");
                let combined = {
                    let stored = self.parts.lock().unwrap();
                    let part_data = stored.get(&key).unwrap();
                    assert_eq!(parts.len(), part_data.len());
                    let mut sorted: Vec<_> = part_data.clone();
                    sorted.sort_by_key(|(n, _)| *n);
                    sorted.into_iter().flat_map(|(_, d)| d).collect::<Vec<u8>>()
                };
                self.inner.put_object(h, a, combined).await?;
                Ok(())
            }
            async fn abort_multipart_upload(
                &self,
                _h: &str,
                _a: &str,
                _uid: &str,
            ) -> std::io::Result<()> {
                Ok(())
            }
        }

        #[async_trait::async_trait]
        impl RangeReadDataCache for SmallPartCache {
            async fn get_object_range(
                &self,
                h: &str,
                a: &str,
                s: u64,
                e: u64,
            ) -> std::io::Result<Vec<u8>> {
                let data = self.inner.get_object(h, a).await?;
                let end = std::cmp::min(e as usize + 1, data.len());
                Ok(data[s as usize..end].to_vec())
            }
        }

        let (_sd, _dd, src, dst_inner) = make_caches();
        // 20 bytes, part_size=5, threshold=10 → triggers multipart (4 parts)
        put(&src, "big_hash", "xxh128", b"01234567890123456789").await;

        let upload_part_calls = Arc::new(AtomicUsize::new(0));
        let dst: Arc<dyn AsyncDataCache> = Arc::new(SmallPartCache {
            inner: dst_inner.clone(),
            upload_part_calls: upload_part_calls.clone(),
            parts: Arc::new(Mutex::new(HashMap::new())),
        });

        let manifest = test_manifest(vec![{
            let mut e = FileEntry::new("big.bin");
            e.hash = Some("big_hash".into());
            e.size = Some(20);
            e
        }]);

        // Use src as the source but wrap it to support get_object_range
        let src_wrapped: Arc<dyn AsyncDataCache> = Arc::new(SmallPartCache {
            inner: src.clone(),
            upload_part_calls: Arc::new(AtomicUsize::new(0)),
            parts: Arc::new(Mutex::new(HashMap::new())),
        });

        let result = cache_sync_manifest(
            &[&manifest as &dyn ManifestRef],
            src_wrapped,
            dst,
            CacheSyncOptions::default(),
        )
        .await
        .unwrap();

        assert_eq!(result.statistics.copied_objects, 1);
        assert_eq!(upload_part_calls.load(Ordering::Relaxed), 4);
        // Verify data arrived correctly via the inner cache
        assert_eq!(
            get(&dst_inner, "big_hash", "xxh128").await,
            b"01234567890123456789"
        );
    }

    #[tokio::test]
    async fn sync_uses_copy_from_when_server_side_copy() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        struct MockServerSideCopyCache {
            inner: Arc<dyn AsyncDataCache>,
            copy_from_called: Arc<AtomicBool>,
            get_object_calls: Arc<AtomicUsize>,
        }

        #[async_trait::async_trait]
        impl AsyncDataCache for MockServerSideCopyCache {
            fn object_key(&self, hash: &str, algorithm: &str) -> String {
                self.inner.object_key(hash, algorithm)
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            async fn object_exists(&self, hash: &str, algorithm: &str) -> std::io::Result<bool> {
                self.inner.object_exists(hash, algorithm).await
            }
            async fn put_object(
                &self,
                hash: &str,
                algorithm: &str,
                data: Vec<u8>,
            ) -> std::io::Result<String> {
                self.inner.put_object(hash, algorithm, data).await
            }
            async fn get_object(&self, hash: &str, algorithm: &str) -> std::io::Result<Vec<u8>> {
                self.get_object_calls.fetch_add(1, Ordering::Relaxed);
                self.inner.get_object(hash, algorithm).await
            }
            async fn copy_from(
                &self,
                _source: &dyn AsyncDataCache,
                hash: &str,
                algorithm: &str,
            ) -> std::io::Result<CopyResult> {
                self.copy_from_called.store(true, Ordering::Relaxed);
                // Simulate server-side copy by doing the actual copy via inner
                let data = _source.get_object(hash, algorithm).await?;
                self.inner.put_object(hash, algorithm, data).await?;
                Ok(CopyResult::ServerSideCopy)
            }
        }

        let (_sd, _dd, src, dst_inner) = make_caches();
        put(&src, "hash_a", "xxh128", b"hello").await;

        let copy_from_called = Arc::new(AtomicBool::new(false));
        let get_object_calls = Arc::new(AtomicUsize::new(0));
        let dst: Arc<dyn AsyncDataCache> = Arc::new(MockServerSideCopyCache {
            inner: dst_inner,
            copy_from_called: copy_from_called.clone(),
            get_object_calls: get_object_calls.clone(),
        });

        let manifest = test_manifest(vec![{
            let mut e = FileEntry::new("a.txt");
            e.hash = Some("hash_a".into());
            e.size = Some(5);
            e
        }]);

        let result = cache_sync_manifest(
            &[&manifest as &dyn ManifestRef],
            src,
            dst.clone(),
            CacheSyncOptions::default(),
        )
        .await
        .unwrap();

        assert!(
            copy_from_called.load(Ordering::Relaxed),
            "copy_from should have been called"
        );
        assert_eq!(
            get_object_calls.load(Ordering::Relaxed),
            0,
            "get_object on dst should not be called"
        );
        assert_eq!(result.statistics.copied_objects, 1);
        assert!(exists(&dst, "hash_a", "xxh128").await);
    }
}
