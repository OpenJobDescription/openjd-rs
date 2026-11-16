// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Peak heap use of the manifest decoders, measured with a counting global
//! allocator.
//!
//! This is its own test binary so the allocator sees nothing but these
//! decodes, and it holds a single `#[test]` so no other test allocates
//! concurrently.

use openjd_snapshots::{
    decode_manifest, decode_v2023, decode_v2023_as_diff, decode_v2023_try_for_each_file,
    decode_v2023_try_for_each_file_from_reader, decode_v2025, encode_snapshot_v2023,
    encode_snapshot_v2025, FileEntry, HashAlgorithm, Snapshot, SnapshotError,
    DEFAULT_FILE_CHUNK_SIZE, WHOLE_FILE_CHUNK_SIZE,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grew(by: usize) {
    let now = CURRENT.fetch_add(by, Relaxed) + by;
    PEAK.fetch_max(now, Relaxed);
}

// SAFETY: every method forwards to `System` with the caller's arguments, and
// only updates counters besides.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded unchanged; the caller upholds `alloc`'s contract.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            grew(layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded unchanged; the caller upholds `dealloc`'s contract.
        unsafe { System.dealloc(ptr, layout) };
        CURRENT.fetch_sub(layout.size(), Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: forwarded unchanged; the caller upholds `realloc`'s contract.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            if new_size >= layout.size() {
                grew(new_size - layout.size());
            } else {
                CURRENT.fetch_sub(layout.size() - new_size, Relaxed);
            }
        }
        p
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Peak bytes allocated while `f` runs, above what was live when it started.
fn peak_during<T>(f: impl FnOnce() -> T) -> usize {
    let base = CURRENT.load(Relaxed);
    PEAK.store(base, Relaxed);
    drop(f());
    PEAK.load(Relaxed) - base
}

/// A relative snapshot with `n` hashed files spread over nested directories,
/// shaped like a render's output frames.
fn snapshot(n: u64, chunk_size: i64) -> Snapshot {
    let files = (0..n)
        .map(|i| {
            let path = format!("shot_{:03}/layer_beauty/frame_{i:06}.exr", i / 1000);
            let mut f = FileEntry::file(path, 1000 + i, 1_700_000_000_000_000 + i);
            f.hash = Some(format!("{:032x}", i.wrapping_mul(0x9e37_79b9_7f4a_7c15)));
            f
        })
        .collect();
    let mut m = Snapshot::new(HashAlgorithm::Xxh128, chunk_size).with_files(files);
    m.recompute_total_size();
    m
}

/// Decoding reads only the fields it needs, in one parse, so its peak heap use
/// stays a small multiple of the text's size: the parsed fields plus the
/// manifest's entries built from them.
#[test]
fn decoding_peaks_at_a_small_multiple_of_the_manifest_size() {
    // Measured with this test's manifests: 2.6x for v2023 and 3.9x for v2025.
    // Decoding through a `Value` tree peaks at 7.8x and 9.7x, and
    // `decode_manifest` parsing twice at 13.7x and 16.4x. The limits leave
    // headroom for platform differences and still fail if either comes back.
    const V2023_LIMIT: f64 = 3.5;
    const V2025_LIMIT: f64 = 5.0;

    let n = 20_000;
    let v2023 = encode_snapshot_v2023(&snapshot(n, WHOLE_FILE_CHUNK_SIZE)).unwrap();
    let v2025 = encode_snapshot_v2025(&snapshot(n, DEFAULT_FILE_CHUNK_SIZE)).unwrap();

    let ratio = |text: &str, peak: usize| peak as f64 / text.len() as f64;
    let cases: [(&str, &str, f64, usize); 5] = [
        (
            "decode_v2023",
            &v2023,
            V2023_LIMIT,
            peak_during(|| decode_v2023(&v2023).unwrap()),
        ),
        (
            "decode_v2023_as_diff",
            &v2023,
            V2023_LIMIT,
            peak_during(|| decode_v2023_as_diff(&v2023).unwrap()),
        ),
        (
            "decode_manifest (v2023)",
            &v2023,
            V2023_LIMIT,
            peak_during(|| decode_manifest(&v2023).unwrap()),
        ),
        (
            "decode_v2025",
            &v2025,
            V2025_LIMIT,
            peak_during(|| decode_v2025(&v2025).unwrap()),
        ),
        (
            "decode_manifest (v2025)",
            &v2025,
            V2025_LIMIT,
            peak_during(|| decode_manifest(&v2025).unwrap()),
        ),
    ];

    let report: Vec<String> = cases
        .iter()
        .map(|(name, text, limit, peak)| {
            format!(
                "{name}: peak {:.2}x of {} bytes (limit {limit}x)",
                ratio(text, *peak),
                text.len()
            )
        })
        .collect();
    println!("{}", report.join("\n"));
    for (name, text, limit, peak) in cases {
        assert!(
            ratio(text, peak) <= limit,
            "{name} peaked at {:.2}x the manifest size, over the {limit}x limit\n{}",
            ratio(text, peak),
            report.join("\n")
        );
    }

    // Streaming keeps one entry at a time, so its peak does not depend on the
    // number of entries: it is measured in bytes, not as a ratio.
    const STREAM_LIMIT: usize = 16 * 1024;
    let ignore = |_: HashAlgorithm, _: FileEntry| Ok::<_, SnapshotError>(());
    let streamed = [
        (
            "decode_v2023_try_for_each_file",
            peak_during(|| decode_v2023_try_for_each_file(&v2023, ignore).unwrap()),
        ),
        (
            "decode_v2023_try_for_each_file_from_reader",
            peak_during(|| {
                let reader = std::io::BufReader::new(v2023.as_bytes());
                decode_v2023_try_for_each_file_from_reader(reader, ignore).unwrap()
            }),
        ),
    ];
    for (name, peak) in streamed {
        println!(
            "{name}: peak {peak} bytes for {} bytes of text",
            v2023.len()
        );
        assert!(
            peak <= STREAM_LIMIT,
            "{name} peaked at {peak} bytes, over the {STREAM_LIMIT} byte limit"
        );
    }
}
