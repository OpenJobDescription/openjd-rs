// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Comprehensive v2023 canonicalization tests using pre-generated Python fixtures.
//!
//! These fixtures are the canonical representation produced by the Python implementation.
//! The tests verify that the Rust encoder produces byte-for-byte identical output.

use openjd_snapshots::{
    decode_manifest, decode_v2023, encode_snapshot_v2023, DecodedManifest, Snapshot,
};

// --- Fixtures ---

const SINGLE_FILE: &str = include_str!("data/v2023/single_file.json");
const MULTIPLE_FILES_SORTED: &str = include_str!("data/v2023/multiple_files_sorted.json");
const NESTED_DIRS: &str = include_str!("data/v2023/nested_dirs.json");
const UNICODE_FILENAMES: &str = include_str!("data/v2023/unicode_filenames.json");
const LARGE_MANIFEST: &str = include_str!("data/v2023/large_manifest.json");
const SPECIAL_CHARS: &str = include_str!("data/v2023/special_chars.json");
const SYMLINKS_COLLAPSED: &str = include_str!("data/v2023/symlinks_collapsed.json");
const ZERO_SIZE_FILE: &str = include_str!("data/v2023/zero_size_file.json");
const EXTREME_VALUES: &str = include_str!("data/v2023/extreme_values.json");
const CASE_SENSITIVE_SORT: &str = include_str!("data/v2023/case_sensitive_sort.json");
const UTF16BE_SORT_ORDER: &str = include_str!("data/v2023/utf16be_sort_order.json");

const ALL_FIXTURES: &[(&str, &str)] = &[
    ("single_file", SINGLE_FILE),
    ("multiple_files_sorted", MULTIPLE_FILES_SORTED),
    ("nested_dirs", NESTED_DIRS),
    ("unicode_filenames", UNICODE_FILENAMES),
    ("large_manifest", LARGE_MANIFEST),
    ("special_chars", SPECIAL_CHARS),
    ("symlinks_collapsed", SYMLINKS_COLLAPSED),
    ("zero_size_file", ZERO_SIZE_FILE),
    ("extreme_values", EXTREME_VALUES),
    ("case_sensitive_sort", CASE_SENSITIVE_SORT),
    ("utf16be_sort_order", UTF16BE_SORT_ORDER),
];

// --- Helper ---

fn decode_to_snapshot(json: &str) -> Snapshot {
    match decode_manifest(json).unwrap() {
        DecodedManifest::Snapshot(s) => s,
        other => panic!(
            "Expected Snapshot, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

// ============================================================
// 1. Bitwise round-trip tests
// ============================================================

macro_rules! round_trip_test {
    ($name:ident, $fixture:expr) => {
        #[test]
        fn $name() {
            let canonical = $fixture;
            let snapshot = decode_v2023(canonical).unwrap();
            let re_encoded = encode_snapshot_v2023(&snapshot).unwrap();
            assert_eq!(
                re_encoded,
                canonical,
                "Round-trip mismatch for {}",
                stringify!($name)
            );
        }
    };
}

round_trip_test!(round_trip_single_file, SINGLE_FILE);
round_trip_test!(round_trip_multiple_files_sorted, MULTIPLE_FILES_SORTED);
round_trip_test!(round_trip_nested_dirs, NESTED_DIRS);
round_trip_test!(round_trip_unicode_filenames, UNICODE_FILENAMES);
round_trip_test!(round_trip_large_manifest, LARGE_MANIFEST);
round_trip_test!(round_trip_special_chars, SPECIAL_CHARS);
round_trip_test!(round_trip_symlinks_collapsed, SYMLINKS_COLLAPSED);
round_trip_test!(round_trip_zero_size_file, ZERO_SIZE_FILE);
round_trip_test!(round_trip_extreme_values, EXTREME_VALUES);
round_trip_test!(round_trip_case_sensitive_sort, CASE_SENSITIVE_SORT);
round_trip_test!(round_trip_utf16be_sort_order, UTF16BE_SORT_ORDER);

// ============================================================
// 2. Scrambled order tests
// ============================================================

macro_rules! scrambled_test {
    ($name:ident, $fixture:expr) => {
        #[test]
        fn $name() {
            let canonical = $fixture;
            let mut snapshot = decode_v2023(canonical).unwrap();
            snapshot.files.reverse();
            let re_encoded = encode_snapshot_v2023(&snapshot).unwrap();
            assert_eq!(
                re_encoded,
                canonical,
                "Scrambled order mismatch for {}",
                stringify!($name)
            );
        }
    };
}

scrambled_test!(scrambled_multiple_files_sorted, MULTIPLE_FILES_SORTED);
scrambled_test!(scrambled_nested_dirs, NESTED_DIRS);
scrambled_test!(scrambled_unicode_filenames, UNICODE_FILENAMES);
scrambled_test!(scrambled_large_manifest, LARGE_MANIFEST);
scrambled_test!(scrambled_special_chars, SPECIAL_CHARS);
scrambled_test!(scrambled_symlinks_collapsed, SYMLINKS_COLLAPSED);
scrambled_test!(scrambled_case_sensitive_sort, CASE_SENSITIVE_SORT);
scrambled_test!(scrambled_utf16be_sort_order, UTF16BE_SORT_ORDER);
scrambled_test!(scrambled_extreme_values, EXTREME_VALUES);

// ============================================================
// 3. Key order tests
// ============================================================

#[test]
fn key_order_top_level() {
    for (name, fixture) in ALL_FIXTURES {
        let parsed: serde_json::Value = serde_json::from_str(fixture).unwrap();
        let keys: Vec<&String> = parsed.as_object().unwrap().keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "Top-level keys not sorted in {name}");
    }
}

#[test]
fn key_order_path_entries() {
    for (name, fixture) in ALL_FIXTURES {
        let parsed: serde_json::Value = serde_json::from_str(fixture).unwrap();
        for (i, entry) in parsed["paths"].as_array().unwrap().iter().enumerate() {
            let keys: Vec<&String> = entry.as_object().unwrap().keys().collect();
            let mut sorted = keys.clone();
            sorted.sort();
            assert_eq!(keys, sorted, "Path entry {i} keys not sorted in {name}");
        }
    }
}

// ============================================================
// 4. No whitespace tests (compact format)
// ============================================================

#[test]
fn no_whitespace() {
    for (name, fixture) in ALL_FIXTURES {
        // Check for structural whitespace: outside of JSON string values
        let mut in_string = false;
        let mut prev = '\0';
        for ch in fixture.chars() {
            if in_string {
                if ch == '"' && prev != '\\' {
                    in_string = false;
                }
            } else {
                if ch == '"' {
                    in_string = true;
                } else {
                    assert!(
                        !ch.is_ascii_whitespace(),
                        "Found structural whitespace '{}' in {name}",
                        ch.escape_debug()
                    );
                }
            }
            prev = ch;
        }
    }
}

// ============================================================
// 5. UTF-16 BE sort order tests
// ============================================================

fn utf16_be_bytes(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
}

#[test]
fn utf16be_sort_order_verified() {
    let snapshot = decode_to_snapshot(UTF16BE_SORT_ORDER);
    let paths: Vec<&str> = snapshot.files.iter().map(|f| f.path.as_str()).collect();

    // Verify paths are sorted by UTF-16 BE byte order
    for window in paths.windows(2) {
        let a_bytes = utf16_be_bytes(window[0]);
        let b_bytes = utf16_be_bytes(window[1]);
        assert!(
            a_bytes <= b_bytes,
            "UTF-16 BE sort violation: {:?} should come before {:?}",
            window[0],
            window[1]
        );
    }

    // Verify the expected order: ASCII < non-ASCII, uppercase < lowercase in UTF-16 BE
    assert_eq!(
        paths,
        vec![
            "Atop.txt",
            "ztop.txt",
            "~tilde.txt",
            "\u{00e9}accent.txt",
            "\u{0100}macron.txt"
        ]
    );
}

// ============================================================
// 6. Case sensitivity tests
// ============================================================

#[test]
fn case_sensitive_sort_verified() {
    let snapshot = decode_to_snapshot(CASE_SENSITIVE_SORT);
    let paths: Vec<&str> = snapshot.files.iter().map(|f| f.path.as_str()).collect();

    // Verify sorted by UTF-16 BE
    for window in paths.windows(2) {
        let a_bytes = utf16_be_bytes(window[0]);
        let b_bytes = utf16_be_bytes(window[1]);
        assert!(
            a_bytes <= b_bytes,
            "Case-sensitive sort violation: {:?} should come before {:?}",
            window[0],
            window[1]
        );
    }

    // In UTF-16 BE, uppercase letters (U+0041-005A) come before lowercase (U+0061-007A)
    assert_eq!(paths, vec!["FILE.txt", "File.txt", "fILE.txt", "file.txt"]);
}

// ============================================================
// 7. Determinism tests
// ============================================================

#[test]
fn determinism_100_encodes() {
    let snapshot = decode_v2023(MULTIPLE_FILES_SORTED).unwrap();
    let first = encode_snapshot_v2023(&snapshot).unwrap();
    for i in 1..100 {
        let encoded = encode_snapshot_v2023(&snapshot).unwrap();
        assert_eq!(encoded, first, "Determinism failure on iteration {i}");
    }
}

// ============================================================
// 8. Symlink collapsing verification
// ============================================================

#[test]
fn symlinks_collapsed_no_symlink_entries() {
    let snapshot = decode_to_snapshot(SYMLINKS_COLLAPSED);
    for f in &snapshot.files {
        assert!(
            f.symlink_target.is_none(),
            "Found symlink entry in collapsed manifest: {}",
            f.path
        );
        assert!(!f.deleted, "Found deleted entry: {}", f.path);
        assert!(f.hash.is_some(), "Missing hash for: {}", f.path);
        assert!(f.size.is_some(), "Missing size for: {}", f.path);
        assert!(f.mtime.is_some(), "Missing mtime for: {}", f.path);
    }
}

// ============================================================
// 9. Extreme values test
// ============================================================

#[test]
fn extreme_values_round_trip_precision() {
    let snapshot = decode_to_snapshot(EXTREME_VALUES);

    // Verify large values survived decode
    let huge = snapshot
        .files
        .iter()
        .find(|f| f.path == "huge.dat")
        .unwrap();
    assert_eq!(huge.size, Some(1_099_511_627_776)); // 1 TiB
    assert_eq!(huge.mtime, Some(9_999_999_999_999_999));

    let tiny = snapshot
        .files
        .iter()
        .find(|f| f.path == "tiny.txt")
        .unwrap();
    assert_eq!(tiny.size, Some(0));
    assert_eq!(tiny.mtime, Some(0));

    assert_eq!(snapshot.total_size, 1_099_511_627_777);

    // Re-encode and verify byte-for-byte match
    let re_encoded = encode_snapshot_v2023(&snapshot).unwrap();
    assert_eq!(re_encoded, EXTREME_VALUES);
}

// ============================================================
// 10. Cross-implementation compatibility
// ============================================================

#[test]
fn cross_implementation_all_fixtures() {
    for (name, fixture) in ALL_FIXTURES {
        // Python produced it, Rust can decode it
        let snapshot =
            decode_v2023(fixture).unwrap_or_else(|e| panic!("Failed to decode {name}: {e}"));

        // Rust re-encodes to identical output
        let re_encoded = encode_snapshot_v2023(&snapshot)
            .unwrap_or_else(|e| panic!("Failed to encode {name}: {e}"));

        assert_eq!(
            re_encoded, *fixture,
            "Cross-implementation mismatch for {name}:\n  Rust produced: {}\n  Python canonical: {}",
            &re_encoded[..re_encoded.len().min(200)],
            &fixture[..fixture.len().min(200)]
        );
    }
}
