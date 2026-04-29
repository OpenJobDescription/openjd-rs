// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use super::rate::SlidingWindowRate;
use crate::hash::{hash_file, hash_file_chunked};
use crate::hash_cache::{HashCache, WHOLE_FILE_RANGE_END};
use crate::manifest::{AbsManifest, AbsSnapshot, AbsSnapshotDiff, Manifest};
use rayon::prelude::*;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tracing::debug;

#[derive(Default)]
pub struct HashOptions {
    pub hash_cache: Option<Arc<HashCache>>,
    pub force_rehash: bool,
    pub file_chunk_size_bytes: Option<i64>,
    pub on_progress: Option<Box<super::ProgressFn<HashStatistics>>>,
    pub max_workers: Option<usize>,
}

#[derive(Debug, Default, Clone)]
pub struct HashStatistics {
    pub total_files: usize,
    pub total_bytes: u64,
    pub hashed_files: usize,
    pub hashed_bytes: u64,
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

/// The return type of the HASH operation.
///
/// Generic over the manifest type `M` so the concrete-typed entry points
/// ([`hash_abs_snapshot`], [`hash_abs_snapshot_diff`]) can preserve the
/// input type, while the enum-dispatching entry point ([`hash_abs_manifest`])
/// returns an [`AbsManifest`]. The default `M = AbsManifest` keeps the bare
/// `HashResult` name usable as a shorthand for the enum-dispatch case.
#[derive(Debug)]
pub struct HashResult<M = AbsManifest> {
    pub manifest: M,
    pub statistics: HashStatistics,
}

/// Computes content hashes for all unhashed files in an [`AbsSnapshot`].
///
/// Concrete-typed entry point that preserves the input type in the result.
/// Prefer this over [`hash_abs_manifest`] when you hold an [`AbsSnapshot`]
/// directly, to avoid an enum unwrap on the result.
pub fn hash_abs_snapshot(
    manifest: &AbsSnapshot,
    options: HashOptions,
) -> crate::Result<HashResult<AbsSnapshot>> {
    let (m, statistics) = hash_manifest(manifest, &options)?;
    Ok(HashResult {
        manifest: m,
        statistics,
    })
}

/// Computes content hashes for all unhashed files in an [`AbsSnapshotDiff`].
///
/// Concrete-typed entry point that preserves the input type in the result.
/// Prefer this over [`hash_abs_manifest`] when you hold an [`AbsSnapshotDiff`]
/// directly, to avoid an enum unwrap on the result.
pub fn hash_abs_snapshot_diff(
    manifest: &AbsSnapshotDiff,
    options: HashOptions,
) -> crate::Result<HashResult<AbsSnapshotDiff>> {
    let (m, statistics) = hash_manifest(manifest, &options)?;
    Ok(HashResult {
        manifest: m,
        statistics,
    })
}

/// Computes content hashes for all unhashed files in an absolute-path manifest.
///
/// Files are hashed in parallel using rayon. A hash cache can be provided
/// to skip re-hashing unchanged files. Returns the manifest with hashes
/// filled in and statistics about the operation.
///
/// This enum-dispatching entry point is useful for callers that hold an
/// [`AbsManifest`] without knowing whether it is a snapshot or a diff.
/// Callers that hold a concrete [`AbsSnapshot`] or [`AbsSnapshotDiff`] should
/// prefer [`hash_abs_snapshot`] or [`hash_abs_snapshot_diff`] to avoid an
/// enum match on the returned manifest.
pub fn hash_abs_manifest(
    manifest: &AbsManifest,
    options: HashOptions,
) -> crate::Result<HashResult<AbsManifest>> {
    match manifest {
        AbsManifest::Snapshot(s) => {
            let HashResult {
                manifest,
                statistics,
            } = hash_abs_snapshot(s, options)?;
            Ok(HashResult {
                manifest: AbsManifest::Snapshot(manifest),
                statistics,
            })
        }
        AbsManifest::Diff(d) => {
            let HashResult {
                manifest,
                statistics,
            } = hash_abs_snapshot_diff(d, options)?;
            Ok(HashResult {
                manifest: AbsManifest::Diff(manifest),
                statistics,
            })
        }
    }
}

enum CacheResult {
    WholeFile(String),
    Chunked(Vec<String>),
}

fn hash_manifest<P: Clone, K: Clone>(
    manifest: &Manifest<P, K>,
    options: &HashOptions,
) -> crate::Result<(Manifest<P, K>, HashStatistics)> {
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

    // Build work list and do cache lookups sequentially (fast SQLite reads)
    struct WorkItem {
        index: usize,
        path: String,
        use_chunks: bool,
        file_size: u64,
    }

    let mut work_items = Vec::new();
    let mut stats = HashStatistics::default();

    for (i, file) in result.files.iter_mut().enumerate() {
        if file.symlink_target.is_some() || file.deleted {
            continue;
        }

        let file_size = file.size.unwrap_or(0);
        let mtime = file.mtime.unwrap_or(0);
        let path = file.path.clone();
        let use_chunks = chunk_size > 0 && file_size as i64 > chunk_size;

        stats.total_files += 1;
        stats.total_bytes += file_size;

        // Try cache lookup
        if let Some(ref cache) = options.hash_cache {
            if !options.force_rehash {
                if use_chunks {
                    let cs = chunk_size as u64;
                    let mut all_cached = true;
                    let mut cached_hashes = Vec::new();
                    let mut offset: u64 = 0;
                    while offset < file_size {
                        let end = std::cmp::min(offset + cs, file_size);
                        if let Some(h) = cache.get_if_fresh(
                            Path::new(&path),
                            alg_str,
                            offset as i64,
                            end as i64,
                            mtime,
                        ) {
                            cached_hashes.push(h);
                        } else {
                            all_cached = false;
                            break;
                        }
                        offset = end;
                    }
                    if all_cached && !cached_hashes.is_empty() {
                        file.chunk_hashes = Some(cached_hashes);
                        stats.skipped_files += 1;
                        stats.skipped_bytes += file_size;
                        continue;
                    }
                } else if let Some(h) =
                    cache.get_if_fresh(Path::new(&path), alg_str, 0, WHOLE_FILE_RANGE_END, mtime)
                {
                    file.hash = Some(h);
                    stats.skipped_files += 1;
                    stats.skipped_bytes += file_size;
                    continue;
                }
            }
        }

        work_items.push(WorkItem {
            index: i,
            path,
            use_chunks,
            file_size,
        });
    }

    if work_items.is_empty() {
        if stats.total_bytes > 0 {
            stats.progress = 100.0;
        }
        if let Some(ref cb) = options.on_progress {
            let _ = cb(&stats);
        }
        let unit = if chunk_size <= 0 { "files" } else { "chunks" };
        stats.progress_message = format!(
            "Hashed {} ({} {}) in 0.00s",
            crate::hash::human_readable_file_size(stats.total_bytes),
            stats.total_files,
            unit
        );
        return Ok((result, stats));
    }

    // Hash uncached files in parallel with rayon
    let cancelled = Arc::new(AtomicBool::new(false));
    let progress_stats = Arc::new(Mutex::new(stats.clone()));
    let rate_calc = Arc::new(Mutex::new(SlidingWindowRate::new()));

    let num_threads = options.max_workers.unwrap_or(0); // 0 = rayon default
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(if num_threads == 0 {
            rayon::current_num_threads()
        } else {
            num_threads
        })
        .build()
        .map_err(|e| crate::SnapshotError::Task(e.to_string()))?;

    let on_progress = &options.on_progress;
    let start = start_time;

    let hash_results: Vec<crate::Result<(usize, CacheResult)>> = pool.install(|| {
        work_items
            .par_iter()
            .map(|item| {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(crate::SnapshotError::Cancelled);
                }

                let path = Path::new(&item.path);
                let cr = if item.use_chunks {
                    let hashes = hash_file_chunked(path, chunk_size as u64, item.file_size)?;
                    debug!(path = %item.path, chunks = hashes.len(), "hashed (chunked)");
                    CacheResult::Chunked(hashes)
                } else {
                    let h = hash_file(path)?;
                    debug!(path = %item.path, "hashed");
                    CacheResult::WholeFile(h)
                };

                // Update progress
                {
                    let mut s = progress_stats.lock().unwrap();
                    s.hashed_files += 1;
                    s.hashed_bytes += item.file_size;
                    let elapsed = start.elapsed().as_secs_f64();
                    s.total_time = elapsed;
                    {
                        let mut rc = rate_calc.lock().unwrap();
                        s.rate = rc.update(elapsed, s.hashed_bytes + s.skipped_bytes);
                    }
                    if s.total_bytes > 0 {
                        s.progress = ((s.hashed_bytes + s.skipped_bytes) as f64
                            / s.total_bytes as f64)
                            * 100.0;
                    }
                    if let Some(ref cb) = on_progress {
                        if !cb(&s) {
                            cancelled.store(true, Ordering::Relaxed);
                            return Err(crate::SnapshotError::Cancelled);
                        }
                    }
                }

                Ok((item.index, cr))
            })
            .collect()
    });

    // Apply results back and write cache entries sequentially
    for r in hash_results {
        let (index, cache_result) = r?;
        let file = &mut result.files[index];
        let path = Path::new(&file.path);
        let mtime = file.mtime.unwrap_or(0);
        let file_size = file.size.unwrap_or(0);

        match cache_result {
            CacheResult::WholeFile(h) => {
                if let Some(ref cache) = options.hash_cache {
                    let _ = cache.put(path, alg_str, 0, WHOLE_FILE_RANGE_END, &h, mtime);
                }
                file.hash = Some(h);
            }
            CacheResult::Chunked(hashes) => {
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
            "Hashed {}",
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

    if let Some(ref cb) = options.on_progress {
        let _ = cb(&stats);
    }

    Ok((result, stats))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::HashAlgorithm;
    use crate::manifest::{AbsManifest, AbsSnapshot, AbsSnapshotDiff, FileEntry, Manifest};
    use crate::DEFAULT_FILE_CHUNK_SIZE;
    use tempfile::TempDir;

    fn make_test_file(dir: &Path, name: &str, content: &[u8]) -> (String, u64) {
        let p = dir.join(name);
        std::fs::write(&p, content).unwrap();
        let meta = std::fs::metadata(&p).unwrap();
        let mtime = meta
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;
        (p.to_string_lossy().into_owned(), mtime)
    }

    #[test]
    fn hash_manifest_produces_hashes() {
        let tmp = TempDir::new().unwrap();
        let (path, mtime) = make_test_file(tmp.path(), "a.txt", b"hello");

        let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(vec![FileEntry::file(&path, 5, mtime)]);

        let result =
            hash_abs_manifest(&AbsManifest::Snapshot(manifest), HashOptions::default()).unwrap();

        let files = result.manifest.files();
        assert_eq!(files.len(), 1);
        assert!(files[0].hash.is_some());
    }

    #[test]
    fn hash_cache_populated_after_hashing() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        let (path, mtime) = make_test_file(tmp.path(), "a.txt", b"hello");
        let cache = Arc::new(HashCache::new(cache_dir.path()).unwrap());

        let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(vec![FileEntry::file(&path, 5, mtime)]);

        let _ = hash_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            HashOptions {
                hash_cache: Some(cache.clone()),
                ..Default::default()
            },
        )
        .unwrap();

        let cached = cache.get(Path::new(&path), "xxh128", 0, WHOLE_FILE_RANGE_END);
        assert!(cached.is_some());
    }

    #[test]
    fn hash_cache_hit_skips_rehashing() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        let (path, mtime) = make_test_file(tmp.path(), "a.txt", b"hello");
        let cache = Arc::new(HashCache::new(cache_dir.path()).unwrap());

        // Pre-populate cache with a known hash
        cache
            .put(
                Path::new(&path),
                "xxh128",
                0,
                WHOLE_FILE_RANGE_END,
                "cached_hash_value",
                mtime,
            )
            .unwrap();

        let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(vec![FileEntry::file(&path, 5, mtime)]);

        let result = hash_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            HashOptions {
                hash_cache: Some(cache),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(
            result.manifest.files()[0].hash.as_deref(),
            Some("cached_hash_value")
        );
    }

    #[test]
    fn force_rehash_ignores_cache() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        let (path, mtime) = make_test_file(tmp.path(), "a.txt", b"hello");
        let cache = Arc::new(HashCache::new(cache_dir.path()).unwrap());

        cache
            .put(
                Path::new(&path),
                "xxh128",
                0,
                WHOLE_FILE_RANGE_END,
                "stale_hash",
                mtime,
            )
            .unwrap();

        let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(vec![FileEntry::file(&path, 5, mtime)]);

        let result = hash_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            HashOptions {
                hash_cache: Some(cache),
                force_rehash: true,
                ..Default::default()
            },
        )
        .unwrap();

        // Should NOT be the stale cached value
        assert_ne!(
            result.manifest.files()[0].hash.as_deref(),
            Some("stale_hash")
        );
        assert!(result.manifest.files()[0].hash.is_some());
    }

    #[test]
    fn symlinks_and_deleted_pass_through() {
        let tmp = TempDir::new().unwrap();
        let (path, mtime) = make_test_file(tmp.path(), "real.txt", b"data");

        let manifest: AbsSnapshotDiff =
            Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![
                FileEntry::file(&path, 4, mtime),
                FileEntry::symlink("/tmp/link", "/tmp/target"),
                FileEntry::deleted("/tmp/gone"),
            ]);

        let result =
            hash_abs_manifest(&AbsManifest::Diff(manifest), HashOptions::default()).unwrap();

        let files = result.manifest.files();
        assert!(files[0].hash.is_some()); // real file hashed
        assert!(files[1].hash.is_none()); // symlink untouched
        assert!(files[2].hash.is_none()); // deleted untouched
    }

    #[test]
    fn rejects_already_hashed_files() {
        let tmp = TempDir::new().unwrap();
        let (path, mtime) = make_test_file(tmp.path(), "a.txt", b"hello");
        let mut entry = FileEntry::file(&path, 5, mtime);
        entry.hash = Some("existing_hash".into());

        let manifest: AbsSnapshot =
            Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![entry]);

        let result = hash_abs_manifest(&AbsManifest::Snapshot(manifest), HashOptions::default());
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("already has hashes set"));
    }

    #[test]
    fn chunked_hashing_for_large_files() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("large_file.bin");
        // Create a file larger than chunk size (use small chunk for test)
        let data = vec![0u8; 1024];
        std::fs::write(&file_path, &data).unwrap();
        let meta = std::fs::metadata(&file_path).unwrap();
        let mtime = meta
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;

        let chunk_size = 256i64; // small chunk for testing
        let manifest: AbsSnapshot =
            Manifest::new(HashAlgorithm::Xxh128, chunk_size).with_files(vec![FileEntry::file(
                file_path.to_string_lossy().to_string(),
                1024,
                mtime,
            )]);

        let result = hash_abs_manifest(
            &AbsManifest::Snapshot(manifest),
            HashOptions {
                file_chunk_size_bytes: Some(chunk_size),
                ..Default::default()
            },
        )
        .unwrap();

        let f = &result.manifest.files()[0];
        assert!(f.hash.is_none()); // no whole-file hash
        let chunks = f.chunk_hashes.as_ref().unwrap();
        assert_eq!(chunks.len(), 4); // 1024 / 256 = 4 chunks
    }

    #[test]
    fn hash_abs_snapshot_preserves_concrete_type() {
        let tmp = TempDir::new().unwrap();
        let (path, mtime) = make_test_file(tmp.path(), "a.txt", b"hello");

        let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(vec![FileEntry::file(&path, 5, mtime)]);

        // Concrete entry point: result.manifest is AbsSnapshot directly — no enum unwrap.
        let result: HashResult<AbsSnapshot> =
            hash_abs_snapshot(&manifest, HashOptions::default()).unwrap();

        // Direct field access works with no enum matching.
        assert_eq!(result.manifest.files.len(), 1);
        assert!(result.manifest.files[0].hash.is_some());
        // Can chain further operations that require AbsSnapshot (e.g. validate).
        result.manifest.validate().unwrap();
    }

    #[test]
    fn hash_abs_snapshot_diff_preserves_concrete_type() {
        let tmp = TempDir::new().unwrap();
        let (path, mtime) = make_test_file(tmp.path(), "a.txt", b"hello");

        let manifest: AbsSnapshotDiff =
            Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE).with_files(vec![
                FileEntry::file(&path, 5, mtime),
                FileEntry::deleted("/tmp/gone"),
            ]);

        let result: HashResult<AbsSnapshotDiff> =
            hash_abs_snapshot_diff(&manifest, HashOptions::default()).unwrap();

        assert_eq!(result.manifest.files.len(), 2);
        assert!(result.manifest.files[0].hash.is_some());
        assert!(result.manifest.files[1].deleted);
        result.manifest.validate().unwrap();
    }

    #[test]
    fn hash_abs_snapshot_and_enum_produce_identical_hashes() {
        // The concrete and enum entry points must be semantically equivalent:
        // same files hashed, same hash values, same statistics.
        let tmp = TempDir::new().unwrap();
        let (path, mtime) = make_test_file(tmp.path(), "a.txt", b"hello world");

        let manifest: AbsSnapshot = Manifest::new(HashAlgorithm::Xxh128, DEFAULT_FILE_CHUNK_SIZE)
            .with_files(vec![FileEntry::file(&path, 11, mtime)]);

        let concrete = hash_abs_snapshot(&manifest, HashOptions::default()).unwrap();
        let via_enum =
            hash_abs_manifest(&AbsManifest::Snapshot(manifest), HashOptions::default()).unwrap();

        let enum_files = via_enum.manifest.files();
        assert_eq!(concrete.manifest.files[0].hash, enum_files[0].hash);
        assert_eq!(
            concrete.statistics.total_files,
            via_enum.statistics.total_files
        );
        assert_eq!(
            concrete.statistics.total_bytes,
            via_enum.statistics.total_bytes
        );
        assert_eq!(
            concrete.statistics.hashed_files,
            via_enum.statistics.hashed_files
        );
    }
}
