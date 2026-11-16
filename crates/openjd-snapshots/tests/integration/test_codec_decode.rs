// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! How the decoders read malformed and unusual JSON: every decode error
//! message, the order the checks run in, and the reading rules (a value of the
//! wrong type is absent, the last duplicate key wins, unknown keys are
//! ignored, ignored values must still be valid JSON).

use openjd_snapshots::{
    decode_manifest, decode_v2023, decode_v2023_as_diff, decode_v2025, DecodedManifest, Snapshot,
    DEFAULT_FILE_CHUNK_SIZE,
};

/// A v2023 document with the given `paths` and extra top-level members.
fn v2023(paths: &str, extra: &str) -> String {
    format!(
        r#"{{"hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":{paths},"totalSize":0{extra}}}"#
    )
}

/// A relative v2025 snapshot with the given `dirs`, `files`, and extra members.
fn v2025(dirs: &str, files: &str, extra: &str) -> String {
    format!(
        r#"{{"dirs":{dirs},"fileChunkSizeBytes":-1,"files":{files},"hashAlg":"xxh128","specificationVersion":"relative-manifest-snapshot-beta-2025-12","totalSize":0{extra}}}"#
    )
}

const ENTRY: &str = r#"{"hash":"h","mtime":1,"path":"a.txt","size":2}"#;

fn err<T: std::fmt::Debug>(r: openjd_snapshots::Result<T>) -> String {
    r.unwrap_err().to_string()
}

fn rel_snapshot(d: DecodedManifest) -> Snapshot {
    match d {
        DecodedManifest::Snapshot(s) => s,
        other => panic!("expected a relative snapshot, got {other:?}"),
    }
}

// ===== Invalid JSON =====

#[test]
fn syntax_errors_are_reported_with_their_location() {
    assert_eq!(
        err(decode_manifest("{")),
        "Manifest validation error: Invalid JSON: EOF while parsing an object at line 1 column 1"
    );
    assert_eq!(
        err(decode_v2023(&format!("{} x", v2023("[]", "")))),
        "Manifest validation error: Invalid JSON: trailing characters at line 1 column 78"
    );
}

#[test]
fn an_out_of_range_number_in_an_ignored_member_is_invalid_json() {
    let json = v2023("[]", r#","extra":1e400"#);
    let expected =
        "Manifest validation error: Invalid JSON: number out of range at line 1 column 89";
    assert_eq!(err(decode_v2023(&json)), expected);
    assert_eq!(err(decode_manifest(&json)), expected);
}

#[test]
fn an_invalid_escape_in_an_ignored_member_is_invalid_json() {
    let json = v2025("[]", "[]", r#","extra":"\udc00x""#);
    assert_eq!(
        err(decode_v2025(&json)),
        "Manifest validation error: Invalid JSON: lone leading surrogate in hex escape at line 1 column 159"
    );
}

#[test]
fn an_invalid_value_in_an_ignored_entry_member_is_invalid_json() {
    let json = v2023(
        r#"[{"hash":"h","mtime":1,"path":"a","size":2,"x":-1e400}]"#,
        "",
    );
    assert_eq!(
        err(decode_v2023(&json)),
        "Manifest validation error: Invalid JSON: number out of range at line 1 column 112"
    );
}

#[test]
fn nesting_past_the_recursion_limit_in_an_ignored_member_is_invalid_json() {
    let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
    let json = v2023("[]", &format!(r#","extra":{deep}"#));
    assert_eq!(
        err(decode_v2023(&json)),
        "Manifest validation error: Invalid JSON: recursion limit exceeded at line 1 column 211"
    );
}

// ===== Format detection =====

#[test]
fn a_document_with_neither_version_key_is_an_unknown_format() {
    let expected = "Manifest validation error: Unknown manifest format: no manifestVersion or specificationVersion";
    assert_eq!(err(decode_manifest(r#"{"paths":[]}"#)), expected);
    assert_eq!(err(decode_manifest("[]")), expected);
    assert_eq!(err(decode_manifest("7")), expected);
}

#[test]
fn format_detection_goes_by_key_presence_not_value() {
    assert_eq!(
        err(decode_manifest(r#"{"manifestVersion":null}"#)),
        "Manifest validation error: missing manifestVersion"
    );
    assert_eq!(
        err(decode_manifest(r#"{"specificationVersion":5}"#)),
        "Manifest validation error: missing specificationVersion"
    );
}

#[test]
fn a_document_with_both_version_keys_is_read_as_v2023() {
    let json = v2023(&format!("[{ENTRY}]"), r#","specificationVersion":"bogus""#);
    let decoded = rel_snapshot(decode_manifest(&json).unwrap());
    assert_eq!(decoded.files.len(), 1);
    assert_eq!(decoded.files[0].path, "a.txt");
}

// ===== v2023 errors =====

#[test]
fn v2023_header_errors() {
    let cases = [
        (
            r#"{"hashAlg":"xxh128","paths":[],"totalSize":0}"#,
            "missing manifestVersion",
        ),
        (
            r#"{"hashAlg":"xxh128","manifestVersion":20230303,"paths":[],"totalSize":0}"#,
            "missing manifestVersion",
        ),
        (
            r#"{"hashAlg":"xxh128","manifestVersion":"2023-03-04","paths":[],"totalSize":0}"#,
            "expected manifestVersion '2023-03-03', got '2023-03-04'",
        ),
        (
            r#"{"manifestVersion":"2023-03-03","paths":[],"totalSize":0}"#,
            "missing hashAlg",
        ),
        (
            r#"{"hashAlg":"md5","manifestVersion":"2023-03-03","paths":[],"totalSize":0}"#,
            "Unsupported hash algorithm: md5",
        ),
        (
            r#"{"hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":[]}"#,
            "missing or invalid totalSize",
        ),
        (
            r#"{"hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":[],"totalSize":"0"}"#,
            "missing or invalid totalSize",
        ),
        (
            r#"{"hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":[],"totalSize":-1}"#,
            "missing or invalid totalSize",
        ),
        (
            r#"{"hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":[],"totalSize":1.0}"#,
            "missing or invalid totalSize",
        ),
        (
            r#"{"hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":[],"totalSize":18446744073709551616}"#,
            "missing or invalid totalSize",
        ),
        (
            r#"{"hashAlg":"xxh128","manifestVersion":"2023-03-03","totalSize":0}"#,
            "missing or invalid paths",
        ),
        (
            r#"{"hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":{},"totalSize":0}"#,
            "missing or invalid paths",
        ),
    ];
    for (json, message) in cases {
        let expected = format!("Manifest validation error: {message}");
        assert_eq!(err(decode_v2023(json)), expected, "decode_v2023({json})");
        assert_eq!(
            err(decode_v2023_as_diff(json)),
            expected,
            "decode_v2023_as_diff({json})"
        );
        // Without the key, decode_manifest cannot tell the document is v2023.
        if json.contains("manifestVersion") {
            assert_eq!(
                err(decode_manifest(json)),
                expected,
                "decode_manifest({json})"
            );
        }
    }
}

#[test]
fn v2023_entry_errors() {
    let cases = [
        (
            r#"{"hash":"h","mtime":1,"size":2}"#,
            "path entry missing 'path'",
        ),
        (
            r#"{"hash":"h","mtime":1,"path":7,"size":2}"#,
            "path entry missing 'path'",
        ),
        (
            r#"{"mtime":1,"path":"a","size":2}"#,
            "path entry missing 'hash'",
        ),
        (
            r#"{"hash":null,"mtime":1,"path":"a","size":2}"#,
            "path entry missing 'hash'",
        ),
        (
            r#"{"hash":"h","mtime":1,"path":"a"}"#,
            "path entry missing 'size'",
        ),
        (
            r#"{"hash":"h","mtime":1,"path":"a","size":"2"}"#,
            "path entry missing 'size'",
        ),
        (
            r#"{"hash":"h","mtime":1,"path":"a","size":-2}"#,
            "path entry missing 'size'",
        ),
        (
            r#"{"hash":"h","mtime":1,"path":"a","size":2e0}"#,
            "path entry missing 'size'",
        ),
        (
            r#"{"hash":"h","path":"a","size":2}"#,
            "path entry missing 'mtime'",
        ),
        (
            r#"{"hash":"h","mtime":true,"path":"a","size":2}"#,
            "path entry missing 'mtime'",
        ),
        // An entry that is not an object has no fields.
        ("1", "path entry missing 'path'"),
        (r#"["a.txt"]"#, "path entry missing 'path'"),
    ];
    for (entry, message) in cases {
        let json = v2023(&format!("[{ENTRY},{entry}]"), "");
        assert_eq!(
            err(decode_v2023(&json)),
            format!("Manifest validation error: {message}"),
            "entry {entry}"
        );
    }
}

#[test]
fn v2023_header_errors_are_reported_before_entry_errors() {
    let json = r#"{"hashAlg":"md5","manifestVersion":"2023-03-03","paths":[1],"totalSize":0}"#;
    assert_eq!(
        err(decode_v2023(json)),
        "Manifest validation error: Unsupported hash algorithm: md5"
    );
}

// ===== v2025 errors =====

#[test]
fn v2025_header_errors() {
    let cases = [
        (
            r#"{"hashAlg":"xxh128","totalSize":0}"#,
            "missing specificationVersion",
        ),
        (
            r#"{"specificationVersion":"relative-manifest-snapshot-beta-2025-12","totalSize":0}"#,
            "missing hashAlg",
        ),
        (
            r#"{"hashAlg":"xxh128","specificationVersion":"relative-manifest-snapshot-beta-2025-12"}"#,
            "missing or invalid totalSize",
        ),
        (
            r#"{"hashAlg":"xxh128","specificationVersion":"bogus","totalSize":0}"#,
            "Unknown specificationVersion: bogus",
        ),
    ];
    for (json, message) in cases {
        let expected = format!("Manifest validation error: {message}");
        assert_eq!(err(decode_v2025(json)), expected, "decode_v2025({json})");
    }
}

#[test]
fn v2025_entry_errors() {
    let file = r#"{"hash":"h","mtime":1,"name":"a.txt","size":2}"#;
    let cases = [
        ("[{}]", "[]", "dir entry missing 'name'"),
        (r#"[{"name":3}]"#, "[]", "dir entry missing 'name'"),
        ("[1]", "[]", "dir entry missing 'name'"),
        ("[]", r#"[{"hash":"h"}]"#, "file entry missing 'name'"),
        ("[]", "[null]", "file entry missing 'name'"),
        (
            "[]",
            r#"[{"name":"d/a.txt"}]"#,
            "Invalid path format 'd/a.txt': paths with '/' must use $N/ reference",
        ),
        (
            "[]",
            r#"[{"name":"$x/a.txt"}]"#,
            "Invalid directory reference '$x/a.txt': expected $N/component format",
        ),
        (
            r#"[{"name":"d"}]"#,
            r#"[{"name":"$1/a.txt"}]"#,
            "Invalid directory reference '$1/a.txt': index 1 not found",
        ),
        (
            "[]",
            r#"[{"name":"l","symlink":{"name":"$4/t"}}]"#,
            "Invalid directory reference '$4/t': index 4 not found",
        ),
        // A directory may refer only to directories listed before it.
        (
            r#"[{"name":"$1/b"},{"name":"a"}]"#,
            "[]",
            "Invalid directory reference '$1/b': index 1 not found",
        ),
    ];
    for (dirs, files, message) in cases {
        // A valid entry first, so the error comes from a later one.
        let files = match files.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            Some("") | None => format!("[{file}]"),
            Some(rest) => format!("[{file},{rest}]"),
        };
        let json = v2025(dirs, &files, "");
        assert_eq!(
            err(decode_v2025(&json)),
            format!("Manifest validation error: {message}"),
            "dirs {dirs} files {files}"
        );
    }
}

#[test]
fn v2025_entry_errors_are_reported_before_an_unknown_specification_version() {
    let json = r#"{"files":[{"hash":"h"}],"hashAlg":"xxh128","specificationVersion":"bogus","totalSize":0}"#;
    assert_eq!(
        err(decode_v2025(json)),
        "Manifest validation error: file entry missing 'name'"
    );
}

// ===== Reading rules =====

#[test]
fn the_last_duplicate_key_wins() {
    let json = r#"{"hashAlg":"md5","hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":[{"hash":"a","hash":"b","mtime":1,"path":"x","path":"a.txt","size":2}],"totalSize":5,"totalSize":7}"#;
    let decoded = decode_v2023(json).unwrap();
    assert_eq!(decoded.total_size, 7);
    assert_eq!(decoded.files[0].path, "a.txt");
    assert_eq!(decoded.files[0].hash.as_deref(), Some("b"));

    // A repeated array replaces the earlier one, including its bad entries.
    let json = v2023("[1]", &format!(r#","paths":[{ENTRY}]"#));
    assert_eq!(decode_v2023(&json).unwrap().files.len(), 1);

    let json = v2025(
        "[]",
        r#"[{"hash":"h","mtime":1,"name":"a.txt","runnable":true,"runnable":false,"size":2}]"#,
        "",
    );
    assert!(!rel_snapshot(decode_v2025(&json).unwrap()).files[0].runnable);
}

#[test]
fn unknown_members_are_ignored() {
    let json = v2023(
        r#"[{"extra":{"deep":[1,{"x":null}]},"hash":"h","mtime":1,"path":"a.txt","size":2}]"#,
        r#","comment":"hi","nested":{"paths":"no"}"#,
    );
    let decoded = decode_v2023(&json).unwrap();
    assert_eq!(decoded.files.len(), 1);
    assert_eq!(decoded.files[0].size, Some(2));
}

#[test]
fn v2025_members_of_the_wrong_type_are_absent() {
    let json = r#"{"dirs":5,"fileChunkSizeBytes":"big","files":"x","hashAlg":"xxh128","parentManifestHash":5,"specificationVersion":"relative-manifest-snapshot-beta-2025-12","totalSize":0}"#;
    let decoded = rel_snapshot(decode_v2025(json).unwrap());
    assert!(decoded.dirs.is_empty());
    assert!(decoded.files.is_empty());
    assert_eq!(decoded.file_chunk_size_bytes, DEFAULT_FILE_CHUNK_SIZE);
    assert_eq!(decoded.parent_manifest_hash, None);

    // Too large for i64, so it reads as absent and takes the default.
    let json = json.replace(r#""big""#, "9223372036854775808");
    let decoded = rel_snapshot(decode_v2025(&json).unwrap());
    assert_eq!(decoded.file_chunk_size_bytes, DEFAULT_FILE_CHUNK_SIZE);
}

#[test]
fn v2025_entry_members_of_the_wrong_type_are_absent() {
    let json = v2025(
        r#"[{"delete":"yes","name":"d"}]"#,
        r#"[{"delete":1,"hash":"h","mtime":1,"name":"a.txt","runnable":"yes","size":2,"symlink":"t"},{"hash":"h","mtime":1,"name":"b.txt","size":2,"symlink":{"name":5}}]"#,
        "",
    );
    let decoded = rel_snapshot(decode_v2025(&json).unwrap());
    assert!(!decoded.dirs[0].deleted);
    let a = &decoded.files[0];
    assert!(!a.deleted && !a.runnable && a.symlink_target.is_none());
    assert_eq!(decoded.files[1].symlink_target, None);

    // A size that is not an unsigned integer is absent, which validation rejects.
    let json = v2025(
        "[]",
        r#"[{"hash":"h","mtime":1,"name":"a.txt","size":"2"}]"#,
        "",
    );
    assert_eq!(
        err(decode_v2025(&json)),
        "Manifest validation error: regular file must have size and mtime: a.txt"
    );
}

#[test]
fn v2025_entry_members_are_decoded() {
    let json = r#"{"dirs":[{"name":"d"},{"delete":true,"name":"gone"}],"fileChunkSizeBytes":-1,"files":[{"hash":"h1","mtime":1,"name":"$0/a.txt","size":2},{"delete":true,"name":"old.txt"}],"hashAlg":"xxh128","specificationVersion":"relative-manifest-diff-beta-2025-12","totalSize":2}"#;
    let decoded = match decode_v2025(json).unwrap() {
        DecodedManifest::SnapshotDiff(d) => d,
        other => panic!("expected a relative diff, got {other:?}"),
    };
    let dirs: Vec<(&str, bool)> = decoded
        .dirs
        .iter()
        .map(|d| (d.path.as_str(), d.deleted))
        .collect();
    assert_eq!(dirs, [("d", false), ("gone", true)]);
    let files: Vec<(&str, Option<&str>, bool)> = decoded
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.hash.as_deref(), f.deleted))
        .collect();
    assert_eq!(
        files,
        [("d/a.txt", Some("h1"), false), ("old.txt", None, true)]
    );
}

#[test]
fn v2025_non_string_chunk_hashes_are_dropped() {
    let json = r#"{"dirs":[],"fileChunkSizeBytes":256,"files":[{"chunkhashes":["c1",2,null,"c2"],"mtime":1,"name":"big","size":512}],"hashAlg":"xxh128","specificationVersion":"relative-manifest-snapshot-beta-2025-12","totalSize":512}"#;
    let decoded = rel_snapshot(decode_v2025(json).unwrap());
    assert_eq!(
        decoded.files[0].chunk_hashes,
        Some(vec!["c1".to_string(), "c2".to_string()])
    );
}
