// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

#![cfg_attr(not(feature = "bench"), allow(unused))]

use clap::Parser;
use openjd_snapshots::*;
use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

const SMALL_FILE_SIZE: usize = 1024;
const MEDIUM_FILE_SIZE: usize = 50 * 1024 * 1024;
const LARGE_FILE_SIZE: usize = 1024 * 1024 * 1024;

#[derive(Parser)]
#[command(
    name = "snapshots-bench",
    about = "Benchmark openjd-snapshots operations"
)]
struct Cli {
    #[arg(long, default_value_t = false)]
    local_only: bool,
    #[arg(long, default_value_t = 400)]
    small_files: usize,
    #[arg(long, default_value_t = 40)]
    medium_files: usize,
    #[arg(long, default_value_t = 2)]
    large_files: usize,
    #[arg(long, default_value_t = 1000)]
    subdirectories: usize,
    #[arg(long, default_value_t = 10)]
    max_nesting_depth: usize,
    #[arg(long, default_value = "10")]
    max_workers: String,
    #[arg(long)]
    max_memory: Option<usize>,
    #[arg(long, default_value_t = 256)]
    chunk_size: usize,
    #[arg(long, default_value_t = false)]
    no_chunking: bool,
    #[arg(long, default_value_t = false)]
    no_verify: bool,
    #[arg(long, default_value_t = false)]
    skip_download: bool,
    #[arg(long)]
    source_dir: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    keep_files: bool,
    #[arg(long)]
    preset: Option<String>,
    #[arg(long, default_value_t = false)]
    no_hash_cache: bool,
}

#[allow(dead_code)]
struct TimingResult {
    operation: String,
    duration: f64,
    files: usize,
    bytes: u64,
    throughput_mb_s: f64,
    throughput_paths_s: f64,
}

fn gen_content(seed: u64, size: usize) -> Vec<u8> {
    let mut rng = StdRng::seed_from_u64(seed);
    let mut buf = vec![0u8; size];
    rng.fill_bytes(&mut buf);
    buf
}

fn xxh128_bytes(data: &[u8]) -> String {
    format!("{:032x}", xxhash_rust::xxh3::xxh3_128(data))
}

fn xxh128_file(path: &Path) -> String {
    use std::io::Read;
    let mut f = std::fs::File::open(path).unwrap();
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    format!("{:032x}", hasher.digest128())
}

fn create_test_files(root: &Path, cli: &Cli) -> (usize, u64, HashMap<String, String>) {
    let mut total_files = 0usize;
    let mut total_bytes = 0u64;
    let mut checksums: HashMap<String, String> = HashMap::new();

    std::fs::create_dir_all(root).unwrap();

    // Create subdirectories with deterministic random nesting
    let mut subdirs: Vec<PathBuf> = Vec::new();
    if cli.subdirectories > 0 {
        let small_root = root.join("small");
        std::fs::create_dir_all(&small_root).unwrap();
        let mut rng = StdRng::seed_from_u64(42);
        for i in 0..cli.subdirectories {
            let depth = (rng.next_u64() % cli.max_nesting_depth.max(1) as u64) + 1;
            let mut current = small_root.clone();
            for d in 0..depth {
                current = current.join(format!("d{:04}_l{}", i, d));
            }
            std::fs::create_dir_all(&current).unwrap();
            subdirs.push(current);
            if (i + 1) % 100 == 0 || i == cli.subdirectories - 1 {
                eprint!("\r    Dirs: {}/{}", i + 1, cli.subdirectories);
            }
        }
        eprintln!();
    }

    // Small files with skewed distribution
    if cli.small_files > 0 && !subdirs.is_empty() {
        eprintln!(
            "  Creating {} small files ({} bytes each)...",
            cli.small_files, SMALL_FILE_SIZE
        );
        let hot_count = (cli.small_files as f64 * 0.70) as usize;
        let warm_count = (cli.small_files as f64 * 0.10) as usize;
        let remaining = cli.small_files - hot_count - warm_count;

        let hot_dir = &subdirs[0];
        let warm_dir = if subdirs.len() > 1 {
            &subdirs[1]
        } else {
            &subdirs[0]
        };
        let other_dirs = if subdirs.len() > 2 {
            &subdirs[2..]
        } else {
            &subdirs[..1]
        };

        let mut file_idx = 0u64;
        let mut write_small = |dir: &Path, count: usize| {
            for _ in 0..count {
                let path = dir.join(format!("small_{:08}.dat", file_idx));
                let content = gen_content(file_idx, SMALL_FILE_SIZE);
                let hash = xxh128_bytes(&content);
                if !path.exists() {
                    std::fs::write(&path, &content).unwrap();
                }
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .replace('\\', "/");
                checksums.insert(rel, hash);
                total_files += 1;
                total_bytes += SMALL_FILE_SIZE as u64;
                file_idx += 1;
            }
        };

        write_small(hot_dir, hot_count);
        write_small(warm_dir, warm_count);
        if remaining > 0 {
            let per_dir = remaining / other_dirs.len();
            let extra = remaining % other_dirs.len();
            for (i, d) in other_dirs.iter().enumerate() {
                write_small(d, per_dir + if i < extra { 1 } else { 0 });
            }
        }
        eprintln!("    Small: {}", file_idx);
    }

    // Medium files
    if cli.medium_files > 0 {
        let medium_dir = root.join("medium");
        std::fs::create_dir_all(&medium_dir).unwrap();
        eprintln!(
            "  Creating {} medium files ({}MB each)...",
            cli.medium_files,
            MEDIUM_FILE_SIZE / (1024 * 1024)
        );
        for i in 0..cli.medium_files {
            let path = medium_dir.join(format!("medium_{:04}.dat", i));
            let content = gen_content(10_000_000 + i as u64, MEDIUM_FILE_SIZE);
            let hash = xxh128_bytes(&content);
            if !path.exists() {
                std::fs::write(&path, &content).unwrap();
            }
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            checksums.insert(rel, hash);
            total_files += 1;
            total_bytes += MEDIUM_FILE_SIZE as u64;
            if (i + 1) % 10 == 0 || i == cli.medium_files - 1 {
                eprint!("\r    Medium: {}/{}", i + 1, cli.medium_files);
            }
        }
        eprintln!();
    }

    // Large files
    if cli.large_files > 0 {
        let large_dir = root.join("large");
        std::fs::create_dir_all(&large_dir).unwrap();
        let large_size = LARGE_FILE_SIZE;
        eprintln!(
            "  Creating {} large files ({}GB each)...",
            cli.large_files,
            large_size / (1024 * 1024 * 1024)
        );
        for i in 0..cli.large_files {
            let path = large_dir.join(format!("large_{:02}.dat", i));
            let mut hasher = xxhash_rust::xxh3::Xxh3::new();
            if !path.exists() {
                let mut f = std::fs::File::create(&path).unwrap();
                let mut remaining = large_size;
                let mut chunk_num = 0u64;
                while remaining > 0 {
                    let sz = remaining.min(64 * 1024 * 1024);
                    let content = gen_content(20_000_000 + i as u64 * 1000 + chunk_num, sz);
                    hasher.update(&content);
                    std::io::Write::write_all(&mut f, &content).unwrap();
                    remaining -= sz;
                    chunk_num += 1;
                }
            } else {
                use std::io::Read;
                let mut f = std::fs::File::open(&path).unwrap();
                let mut buf = vec![0u8; 64 * 1024 * 1024];
                loop {
                    let n = f.read(&mut buf).unwrap();
                    if n == 0 {
                        break;
                    }
                    hasher.update(&buf[..n]);
                }
            }
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            checksums.insert(rel, format!("{:032x}", hasher.digest128()));
            total_files += 1;
            total_bytes += large_size as u64;
        }
    }

    eprintln!(
        "  Total: {} files, {:.2} MB",
        total_files,
        total_bytes as f64 / (1024.0 * 1024.0)
    );
    (total_files, total_bytes, checksums)
}

fn format_duration(secs: f64) -> String {
    if secs < 1.0 {
        format!("0:{:04.1}", secs)
    } else {
        format!("{}:{:02.0}", (secs / 60.0) as u64, secs % 60.0)
    }
}

fn main() {
    let mut cli = Cli::parse();

    // Apply presets
    match cli.preset.as_deref() {
        Some("tiny") => {
            cli.small_files = 400;
            cli.medium_files = 40;
            cli.large_files = 2;
        }
        Some("small") => {
            cli.small_files = 1500;
            cli.medium_files = 400;
            cli.large_files = 5;
        }
        Some("medium") => {
            cli.small_files = 20000;
            cli.medium_files = 1000;
            cli.large_files = 10;
        }
        Some("large") => {
            cli.small_files = 1_000_000;
            cli.medium_files = 10000;
            cli.large_files = 10;
        }
        Some(p) => {
            eprintln!("Unknown preset: {}", p);
            std::process::exit(1);
        }
        None => {}
    }

    let worker_counts: Vec<usize> = cli
        .max_workers
        .split(',')
        .map(|s| s.trim().parse().expect("invalid worker count"))
        .collect();
    let is_scaling = worker_counts.len() > 1;

    let chunk_size_bytes: i64 = if cli.no_chunking {
        WHOLE_FILE_CHUNK_SIZE
    } else {
        cli.chunk_size as i64 * 1024 * 1024
    };
    let max_memory_bytes = cli.max_memory.map(|mb| mb * 1024 * 1024);

    println!("============================================================");
    println!("SNAPSHOTS LIBRARY BENCHMARK (Rust)");
    println!("============================================================");
    println!("Configuration:");
    println!(
        "  Small files: {} x {} bytes",
        cli.small_files, SMALL_FILE_SIZE
    );
    println!(
        "  Medium files: {} x {} MB",
        cli.medium_files,
        MEDIUM_FILE_SIZE / (1024 * 1024)
    );
    println!(
        "  Large files: {} x {} GB",
        cli.large_files,
        LARGE_FILE_SIZE / (1024 * 1024 * 1024)
    );
    println!("  Subdirectories: {}", cli.subdirectories);
    if is_scaling {
        println!("  Max workers: {:?} (SCALING TEST)", worker_counts);
    } else {
        println!("  Max workers: {}", worker_counts[0]);
    }
    if cli.no_chunking {
        println!("  Chunk size: disabled");
    } else {
        println!("  Chunk size: {} MB", cli.chunk_size);
    }
    println!("  Verify correctness: {}", !cli.no_verify);

    let start_all = Instant::now();

    // Setup source directory
    let (source_root, _tmpdir) = if let Some(ref dir) = cli.source_dir {
        println!("\nUsing existing source directory: {}", dir.display());
        (dir.clone(), None::<tempfile::TempDir>)
    } else {
        let tmp = if cli.keep_files {
            let p = std::env::temp_dir().join("snapshots_bench_data");
            std::fs::create_dir_all(&p).unwrap();
            p
        } else {
            let t = tempfile::tempdir().unwrap();
            let p = t.path().to_path_buf();
            // Return the TempDir to keep it alive
            return run_with_tmpdir(
                cli,
                worker_counts,
                is_scaling,
                chunk_size_bytes,
                max_memory_bytes,
                p,
                Some(t),
                start_all,
            );
        };
        (tmp, None)
    };

    run_with_tmpdir(
        cli,
        worker_counts,
        is_scaling,
        chunk_size_bytes,
        max_memory_bytes,
        source_root,
        None::<tempfile::TempDir>,
        start_all,
    );
}

#[allow(clippy::too_many_arguments)]
fn run_with_tmpdir(
    cli: Cli,
    worker_counts: Vec<usize>,
    is_scaling: bool,
    chunk_size_bytes: i64,
    max_memory_bytes: Option<usize>,
    source_root: PathBuf,
    _tmpdir: Option<tempfile::TempDir>,
    start_all: Instant,
) {
    // Generate test data
    let checksums = if cli.source_dir.is_none() {
        let (_, _, cs) = create_test_files(&source_root, &cli);
        Some(cs)
    } else {
        None
    };

    // Collect once (shared across scaling runs)
    println!("\n============================================================");
    println!("TEST: COLLECT (directory scanning)");
    println!("============================================================");
    let t = Instant::now();
    let manifest = collect_abs_snapshot(
        &[&source_root],
        &[] as &[&str],
        CollectOptions {
            symlink_policy: SymlinkPolicy::CollapseEscaping,
            file_chunk_size_bytes: Some(chunk_size_bytes),
            ..Default::default()
        },
    )
    .unwrap();
    let collect_dur = t.elapsed().as_secs_f64();
    let total_paths = manifest.files.len() + manifest.dirs.len();
    println!("  Files collected: {}", manifest.files.len());
    println!("  Directories collected: {}", manifest.dirs.len());
    println!(
        "  Total size: {:.2} MB",
        manifest.total_size as f64 / (1024.0 * 1024.0)
    );
    println!("  Duration: {:.2} seconds", collect_dur);
    println!(
        "  Throughput: {:.0} paths/s",
        total_paths as f64 / collect_dur
    );

    let abs_manifest = AbsManifest::Snapshot(manifest.clone());

    // Run for each worker count
    let mut all_scaling: Vec<(usize, Vec<TimingResult>)> = Vec::new();

    for &workers in &worker_counts {
        let label = if is_scaling {
            format!(" (w={})", workers)
        } else {
            String::new()
        };
        let mut results: Vec<TimingResult> = Vec::new();

        let tmp = tempfile::tempdir().unwrap();
        let data_cache_root = tmp.path().join("data_cache");
        let hash_cache_dir = tmp.path().join("hash_cache");
        let download_root = tmp.path().join("download");
        let dl_hash_cache_dir = tmp.path().join("dl_hash_cache");

        std::fs::create_dir_all(&data_cache_root).unwrap();
        std::fs::create_dir_all(&hash_cache_dir).unwrap();

        let data_cache = Arc::new(FileSystemDataCache::new(&data_cache_root).unwrap());

        // HASH_UPLOAD cold
        {
            println!("\n============================================================");
            println!(
                "TEST: HASH_UPLOAD (filesystem, max_workers={}){}",
                workers, label
            );
            println!("============================================================");

            let hash_cache = if cli.no_hash_cache {
                None
            } else {
                Some(Arc::new(HashCache::new(&hash_cache_dir).unwrap()))
            };

            let t = Instant::now();
            let upload_result = hash_upload_abs_manifest(
                &abs_manifest,
                data_cache.clone() as Arc<dyn AsyncDataCache>,
                HashUploadOptions {
                    hash_cache: hash_cache.clone(),
                    force_rehash: cli.no_hash_cache,
                    file_chunk_size_bytes: Some(chunk_size_bytes),
                    max_workers: Some(workers),
                    max_memory_bytes,
                    ..Default::default()
                },
            )
            .unwrap();
            let dur = t.elapsed().as_secs_f64();
            let stats = &upload_result.statistics;
            let tb = stats.total_bytes;
            let tp = (tb as f64 / (1024.0 * 1024.0)) / dur;

            println!(
                "  Files processed: {} ({:.2} MB)",
                stats.uploaded_files,
                stats.uploaded_bytes as f64 / (1024.0 * 1024.0)
            );
            println!(
                "  Files skipped: {} ({:.2} MB)",
                stats.skipped_files,
                stats.skipped_bytes as f64 / (1024.0 * 1024.0)
            );
            println!("  Duration: {:.2} seconds", dur);
            println!("  Throughput: {:.2} MB/s", tp);

            results.push(TimingResult {
                operation: format!("UPLOAD cold{}", label),
                duration: dur,
                files: stats.total_files,
                bytes: tb,
                throughput_mb_s: tp,
                throughput_paths_s: 0.0,
            });

            // HASH_UPLOAD warm
            println!("\n  [Upload Pass 2: WARM - all caches]");
            let t = Instant::now();
            let _ = hash_upload_abs_manifest(
                &abs_manifest,
                data_cache.clone() as Arc<dyn AsyncDataCache>,
                HashUploadOptions {
                    hash_cache,
                    force_rehash: false,
                    file_chunk_size_bytes: Some(chunk_size_bytes),
                    max_workers: Some(workers),
                    max_memory_bytes,
                    ..Default::default()
                },
            )
            .unwrap();
            let dur = t.elapsed().as_secs_f64();
            let tp = (tb as f64 / (1024.0 * 1024.0)) / dur;
            println!("  Duration: {:.2} seconds", dur);
            println!("  Throughput: {:.2} MB/s", tp);

            results.push(TimingResult {
                operation: format!("UPLOAD warm{}", label),
                duration: dur,
                files: stats.total_files,
                bytes: tb,
                throughput_mb_s: tp,
                throughput_paths_s: 0.0,
            });

            // Extract hashed snapshot for download and diff
            let hashed_snapshot = match &upload_result.manifest {
                AbsManifest::Snapshot(s) => s,
                _ => panic!("expected snapshot"),
            };

            // Download
            if !cli.skip_download {
                let rel = subtree_snapshot(
                    hashed_snapshot,
                    source_root.to_str().unwrap(),
                    SymlinkPolicy::CollapseEscaping,
                )
                .unwrap();
                let dl_manifest = join_snapshot(&rel, download_root.to_str().unwrap()).unwrap();
                let dl_abs = AbsManifest::Snapshot(dl_manifest);

                // DOWNLOAD cold
                {
                    println!("\n============================================================");
                    println!(
                        "TEST: DOWNLOAD (filesystem, max_workers={}){}",
                        workers, label
                    );
                    println!("============================================================");

                    std::fs::create_dir_all(&download_root).unwrap();
                    std::fs::create_dir_all(&dl_hash_cache_dir).unwrap();

                    let dl_hash_cache = if cli.no_hash_cache {
                        None
                    } else {
                        Some(Arc::new(HashCache::new(&dl_hash_cache_dir).unwrap()))
                    };

                    let t = Instant::now();
                    let dl_result = download_abs_manifest(
                        &dl_abs,
                        data_cache.clone() as Arc<dyn AsyncDataCache>,
                        DownloadOptions {
                            hash_cache: dl_hash_cache.clone(),
                            max_workers: Some(workers),
                            max_memory_bytes,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                    let dur = t.elapsed().as_secs_f64();
                    let ds = &dl_result.statistics;
                    let tp = (ds.downloaded_bytes as f64 / (1024.0 * 1024.0)) / dur;

                    println!("  Files downloaded: {}", ds.downloaded_files);
                    println!("  Files skipped: {}", ds.skipped_files);
                    println!(
                        "  Total bytes: {:.2} MB",
                        ds.downloaded_bytes as f64 / (1024.0 * 1024.0)
                    );
                    println!("  Duration: {:.2} seconds", dur);
                    println!("  Throughput: {:.2} MB/s", tp);

                    results.push(TimingResult {
                        operation: format!("DOWNLOAD cold{}", label),
                        duration: dur,
                        files: ds.downloaded_files,
                        bytes: ds.downloaded_bytes,
                        throughput_mb_s: tp,
                        throughput_paths_s: 0.0,
                    });

                    // DOWNLOAD warm
                    println!("\n  [Download Pass 2: WARM - files exist]");
                    let t = Instant::now();
                    let dl_result2 = download_abs_manifest(
                        &dl_abs,
                        data_cache.clone() as Arc<dyn AsyncDataCache>,
                        DownloadOptions {
                            hash_cache: dl_hash_cache,
                            max_workers: Some(workers),
                            max_memory_bytes,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                    let dur = t.elapsed().as_secs_f64();
                    let ds2 = &dl_result2.statistics;
                    let tp = (ds.total_bytes as f64 / (1024.0 * 1024.0)) / dur;
                    println!("  Files skipped: {}", ds2.skipped_files);
                    println!("  Duration: {:.2} seconds", dur);
                    println!("  Throughput: {:.2} MB/s", tp);

                    results.push(TimingResult {
                        operation: format!("DOWNLOAD warm{}", label),
                        duration: dur,
                        files: ds2.total_files,
                        bytes: ds.total_bytes,
                        throughput_mb_s: tp,
                        throughput_paths_s: 0.0,
                    });
                }

                // Verify correctness
                if !cli.no_verify {
                    if let Some(ref expected) = checksums {
                        println!("\nVerifying downloaded files...");
                        let mut ok = 0;
                        let mut fail = 0;
                        for (rel, expected_hash) in expected {
                            let file_path = download_root.join(rel);
                            if !file_path.exists() {
                                eprintln!("  MISSING: {}", rel);
                                fail += 1;
                            } else {
                                let actual = xxh128_file(&file_path);
                                if actual != *expected_hash {
                                    eprintln!(
                                        "  MISMATCH: {} (expected {}..., got {}...)",
                                        rel,
                                        &expected_hash[..16],
                                        &actual[..16]
                                    );
                                    fail += 1;
                                } else {
                                    ok += 1;
                                }
                            }
                        }
                        println!("  Verification: {} OK, {} FAILED", ok, fail);
                        if fail > 0 {
                            std::process::exit(1);
                        }
                    }
                }
            }

            // DIFF
            {
                println!("\n============================================================");
                println!("TEST: DIFF (manifest comparison)");
                println!("============================================================");
                let t = Instant::now();
                let diff =
                    diff_snapshots(hashed_snapshot, hashed_snapshot, &DiffOptions::default())
                        .unwrap();
                let dur = t.elapsed().as_secs_f64();
                let added = diff.files.iter().filter(|f| !f.deleted).count();
                let deleted = diff.files.iter().filter(|f| f.deleted).count();
                println!("  Files added/modified: {}", added);
                println!("  Files deleted: {}", deleted);
                println!("  Duration: {:.4} seconds", dur);
            }
        }

        all_scaling.push((workers, results));
    }

    // Print scaling summary
    if is_scaling && !all_scaling.is_empty() {
        println!("\n====================================================================================================");
        println!("SCALING TEST SUMMARY (Duration as M:SS)");
        println!("====================================================================================================");

        let col_names: Vec<&str> = all_scaling[0]
            .1
            .iter()
            .map(|r| {
                let op = r.operation.split(" (w=").next().unwrap();
                match op {
                    "UPLOAD cold" => "UPLOAD cold",
                    "UPLOAD warm" => "UPLOAD warm",
                    "DOWNLOAD cold" => "DOWNLOAD cold",
                    "DOWNLOAD warm" => "DOWNLOAD warm",
                    _ => op,
                }
            })
            .collect();

        print!("| {:>8} |", "Workers");
        for c in &col_names {
            print!(" {:>16} |", c);
        }
        println!();

        print!("|{:->10}:|", "");
        for _ in &col_names {
            print!("{:->17}:|", "");
        }
        println!();

        for (workers, results) in &all_scaling {
            print!("| {:>8} |", workers);
            for r in results {
                print!(" {:>16} |", format_duration(r.duration));
            }
            println!();
        }
    }

    // Summary
    println!("\n============================================================");
    println!("SUMMARY");
    println!("============================================================");
    println!(
        "\n  {:30} {:>10} {:>10} {:>12}",
        "Operation", "Duration", "Files", "Throughput"
    );
    println!(
        "  {:30} {:>10} {:>10} {:>12}",
        "-".repeat(30),
        "-".repeat(10),
        "-".repeat(10),
        "-".repeat(12)
    );

    println!(
        "  {:30} {:>10} {:>10} {:>12}",
        "COLLECT",
        format!("{:.2}s", collect_dur),
        total_paths,
        format!("{:.0} paths/s", total_paths as f64 / collect_dur)
    );

    if let Some((_, ref results)) = all_scaling.last() {
        for r in results {
            let tp = if r.throughput_mb_s > 0.0 {
                format!("{:.1} MB/s", r.throughput_mb_s)
            } else {
                "N/A".to_string()
            };
            println!(
                "  {:30} {:>10} {:>10} {:>12}",
                r.operation,
                format!("{:.2}s", r.duration),
                r.files,
                tp
            );
        }
    }

    println!(
        "\n  Total test time: {:.2} seconds",
        start_all.elapsed().as_secs_f64()
    );
    println!("\n✓ ALL TESTS PASSED");
}
