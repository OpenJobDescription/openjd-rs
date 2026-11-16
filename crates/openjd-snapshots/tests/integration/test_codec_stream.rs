// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! `decode_v2023_for_each` and `decode_v2023_for_each_from_reader`: they
//! deliver the entries `decode_v2023` builds, with its error messages, and
//! differ from it only where streaming must (key order, document-order
//! errors, no duplicate-path check).

use openjd_snapshots::{
    decode_v2023, decode_v2023_for_each, decode_v2023_for_each_from_reader, encode_snapshot_v2023,
    FileEntry, HashAlgorithm, Snapshot, SnapshotError, V2023Summary, WHOLE_FILE_CHUNK_SIZE,
};
use std::io;

type Decoded = Result<(Vec<FileEntry>, V2023Summary), String>;

/// Decodes `json` with both streaming entry points, checks they agree, and
/// returns the delivered entries and summary, or the error message.
fn stream(json: &str) -> Decoded {
    let from_str = collect(|f| decode_v2023_for_each(json, f));
    let from_reader = collect(|f| decode_v2023_for_each_from_reader(json.as_bytes(), f));
    // serde_json can place a syntax error one column apart when reading from
    // a reader, so positions are not compared.
    let unplaced = |d: &Decoded| d.clone().map_err(|e| without_position(&e));
    assert_eq!(
        unplaced(&from_str),
        unplaced(&from_reader),
        "the entry points disagree on {json}"
    );
    from_str
}

fn without_position(message: &str) -> String {
    match message.rfind(" at line ") {
        Some(i) => message[..i].to_string(),
        None => message.to_string(),
    }
}

fn collect(
    decode: impl FnOnce(
        &mut dyn FnMut(HashAlgorithm, FileEntry) -> Result<(), SnapshotError>,
    ) -> Result<V2023Summary, SnapshotError>,
) -> Decoded {
    let mut entries = Vec::new();
    let summary = decode(&mut |alg, e| {
        assert_eq!(alg, HashAlgorithm::Xxh128);
        entries.push(e);
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    Ok((entries, summary))
}

fn stream_err(json: &str) -> String {
    stream(json).unwrap_err()
}

/// A v2023 document with the given `paths` and extra top-level members.
fn v2023(paths: &str, extra: &str) -> String {
    format!(
        r#"{{"hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":{paths},"totalSize":0{extra}}}"#
    )
}

const ENTRY: &str = r#"{"hash":"h","mtime":1,"path":"a.txt","size":2}"#;

// ===== Same results as decode_v2023 =====

#[test]
fn delivers_the_entries_decode_v2023_builds_in_order() {
    let files = (0..50u64)
        .map(|i| {
            let mut f = FileEntry::file(format!("d{}/f{i}.txt", i % 7), i, 1_000 + i);
            f.hash = Some(format!("{i:032x}"));
            f
        })
        .collect();
    let mut m = Snapshot::new(HashAlgorithm::Xxh128, WHOLE_FILE_CHUNK_SIZE).with_files(files);
    m.recompute_total_size();
    let json = encode_snapshot_v2023(&m).unwrap();

    let batch = decode_v2023(&json).unwrap();
    let (entries, summary) = stream(&json).unwrap();
    assert_eq!(entries, batch.files);
    assert_eq!(summary.hash_alg, batch.hash_alg);
    assert_eq!(summary.total_size, batch.total_size);
    assert_eq!(summary.file_count, 50);
}

#[test]
fn reads_values_with_the_batch_rules() {
    // Unknown members are ignored, the last duplicate key inside an entry
    // wins, and a total size before the header is fine.
    let json = r#"{"totalSize":9,"comment":{"a":[1]},"hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":[{"hash":"x","hash":"h","mtime":1,"path":"a.txt","size":2,"x":null}]}"#;
    let (entries, summary) = stream(json).unwrap();
    assert_eq!(entries, decode_v2023(json).unwrap().files);
    assert_eq!(entries[0].hash.as_deref(), Some("h"));
    assert_eq!(summary.total_size, 9);
}

#[test]
fn header_errors_match_decode_v2023() {
    let cases = [
        (
            r#"{"hashAlg":"xxh128","manifestVersion":20230303,"paths":[],"totalSize":0}"#,
            "missing manifestVersion",
        ),
        (
            r#"{"hashAlg":"xxh128","manifestVersion":"2023-03-04","paths":[],"totalSize":0}"#,
            "expected manifestVersion '2023-03-03', got '2023-03-04'",
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
            r#"{"hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":[],"totalSize":-1}"#,
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
        (
            r#"{"hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":{"a":[1,{"b":2}],"c":"d"},"totalSize":0}"#,
            "missing or invalid paths",
        ),
        (
            r#"{"hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":5,"totalSize":0}"#,
            "missing or invalid paths",
        ),
        (
            r#"{"hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":null,"totalSize":0}"#,
            "missing or invalid paths",
        ),
        (
            r#"{"hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":"x","totalSize":0}"#,
            "missing or invalid paths",
        ),
        // totalSize is checked before paths, as decode_v2023 checks it.
        (
            r#"{"hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":5,"totalSize":"0"}"#,
            "missing or invalid totalSize",
        ),
        (
            r#"{"hashAlg":"xxh128","totalSize":0}"#,
            "missing manifestVersion",
        ),
        (r#"{"manifestVersion":"2023-03-03"}"#, "missing hashAlg"),
        ("[]", "missing manifestVersion"),
        (
            r#"[1,{"manifestVersion":"2023-03-03"},[true]]"#,
            "missing manifestVersion",
        ),
        ("7", "missing manifestVersion"),
    ];
    for (json, message) in cases {
        let expected = format!("Manifest validation error: {message}");
        assert_eq!(stream_err(json), expected, "streaming {json}");
        assert_eq!(
            decode_v2023(json).unwrap_err().to_string(),
            expected,
            "batch {json}"
        );
    }
}

#[test]
fn entry_errors_match_decode_v2023() {
    let cases = [
        (
            r#"{"hash":"h","mtime":1,"size":2}"#,
            "path entry missing 'path'",
        ),
        (
            r#"{"mtime":1,"path":"a","size":2}"#,
            "path entry missing 'hash'",
        ),
        (
            r#"{"hash":"h","mtime":1,"path":"a","size":-2}"#,
            "path entry missing 'size'",
        ),
        (
            r#"{"hash":"h","path":"a","size":2}"#,
            "path entry missing 'mtime'",
        ),
        ("1", "path entry missing 'path'"),
        (
            r#"{"hash":"h","mtime":1,"path":"","size":2}"#,
            "path must not be empty",
        ),
        (
            r#"{"hash":"h","mtime":1,"path":"/abs/a","size":2}"#,
            "expected relative path, got: /abs/a",
        ),
    ];
    for (entry, message) in cases {
        let json = v2023(&format!("[{ENTRY},{entry}]"), "");
        let expected = format!("Manifest validation error: {message}");
        assert_eq!(stream_err(&json), expected, "streaming entry {entry}");
        assert_eq!(
            decode_v2023(&json).unwrap_err().to_string(),
            expected,
            "batch entry {entry}"
        );
    }
}

#[test]
fn invalid_json_anywhere_is_rejected() {
    let cases = [
        ("{", "EOF while parsing an object at line 1 column 1"),
        (
            &format!("{} x", v2023("[]", "")),
            "trailing characters at line 1 column 78",
        ),
        (
            &v2023("[]", r#","extra":1e400"#),
            "number out of range at line 1 column 89",
        ),
        // Values that are skipped are still parsed in full: in a top-level
        // array, and in a paths value that is not an array.
        ("[1,1e400]", "number out of range at line 1 column 8"),
        (
            &v2023(r#"{"a":1e400}"#, ""),
            "number out of range at line 1 column 69",
        ),
    ];
    for (json, message) in cases {
        assert_eq!(
            stream_err(json),
            format!("Manifest validation error: Invalid JSON: {message}"),
            "{json}"
        );
    }
}

// ===== Where streaming differs =====

#[test]
fn the_header_must_come_before_paths() {
    let cases = [
        (
            r#"{"paths":[],"hashAlg":"xxh128","manifestVersion":"2023-03-03","totalSize":0}"#,
            "missing manifestVersion before paths",
        ),
        (
            r#"{"hashAlg":"xxh128","paths":[],"manifestVersion":"2023-03-03","totalSize":0}"#,
            "missing manifestVersion before paths",
        ),
        (
            r#"{"manifestVersion":"2023-03-03","paths":[],"hashAlg":"xxh128","totalSize":0}"#,
            "missing hashAlg before paths",
        ),
        // Without the key at all, streaming cannot tell whether it comes
        // later, so the message says where it was expected.
        (
            r#"{"hashAlg":"xxh128","paths":[],"totalSize":0}"#,
            "missing manifestVersion before paths",
        ),
        (r#"{"paths":5}"#, "missing manifestVersion before paths"),
        // A header key repeated after paths could change what was delivered.
        (
            &v2023("[]", r#","hashAlg":"xxh128""#),
            "hashAlg must come before paths",
        ),
        (
            &v2023("[]", r#","manifestVersion":"2023-03-03""#),
            "manifestVersion must come before paths",
        ),
    ];
    for (json, message) in cases {
        assert_eq!(
            stream_err(json),
            format!("Manifest validation error: {message}"),
            "{json}"
        );
    }
    // decode_v2023 reads the same reordered document.
    let reordered = format!(
        r#"{{"paths":[{ENTRY}],"hashAlg":"xxh128","manifestVersion":"2023-03-03","totalSize":2}}"#
    );
    assert_eq!(decode_v2023(&reordered).unwrap().files.len(), 1);

    // A wrong version before paths is reported as decode_v2023 reports it,
    // even when hashAlg comes after paths.
    assert_eq!(
        stream_err(r#"{"manifestVersion":"2023-03-04","paths":[],"hashAlg":"xxh128"}"#),
        "Manifest validation error: expected manifestVersion '2023-03-03', got '2023-03-04'"
    );
}

#[test]
fn a_repeated_paths_key_is_an_error() {
    let json = v2023("[]", &format!(r#","paths":[{ENTRY}]"#));
    assert_eq!(
        stream_err(&json),
        "Manifest validation error: paths appears more than once"
    );
    // decode_v2023 uses the last array.
    assert_eq!(decode_v2023(&json).unwrap().files.len(), 1);
}

#[test]
fn errors_are_reported_in_document_order() {
    // totalSize comes after paths, so a bad entry is reported first.
    let json = r#"{"hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":[{"hash":"h"}]}"#;
    assert_eq!(
        stream_err(json),
        "Manifest validation error: path entry missing 'path'"
    );
    assert_eq!(
        decode_v2023(json).unwrap_err().to_string(),
        "Manifest validation error: missing or invalid totalSize"
    );
}

#[test]
fn an_error_ahead_of_a_repeated_key_is_reported() {
    // decode_v2023 takes the last value of each key and accepts both, but
    // streaming checks what it has read when it reaches it.
    let cases = [
        (
            r#"{"hashAlg":"xxh128","manifestVersion":"bad","paths":[],"manifestVersion":"2023-03-03","totalSize":0}"#,
            "expected manifestVersion '2023-03-03', got 'bad'",
        ),
        (
            r#"{"hashAlg":"md5","manifestVersion":"2023-03-03","paths":[],"hashAlg":"xxh128","totalSize":0}"#,
            "Unsupported hash algorithm: md5",
        ),
        (
            r#"{"hashAlg":"xxh128","manifestVersion":"2023-03-03","paths":[{"hash":"h"}],"paths":[],"totalSize":0}"#,
            "path entry missing 'path'",
        ),
    ];
    for (json, message) in cases {
        assert_eq!(
            stream_err(json),
            format!("Manifest validation error: {message}"),
            "{json}"
        );
        assert!(decode_v2023(json).is_ok(), "{json}");
    }
}

#[test]
fn the_first_bad_entry_is_reported() {
    // Streaming checks each entry in full before reading the next.
    // decode_v2023 reports the first entry missing a member before its
    // other checks.
    let json = v2023(
        r#"[{"hash":"a","mtime":1,"path":"/abs","size":1},{"mtime":1,"path":"b","size":1}]"#,
        "",
    );
    assert_eq!(
        stream_err(&json),
        "Manifest validation error: expected relative path, got: /abs"
    );
    assert_eq!(
        decode_v2023(&json).unwrap_err().to_string(),
        "Manifest validation error: path entry missing 'hash'"
    );
}

#[test]
fn entries_before_an_error_are_delivered() {
    let json = v2023(
        &format!(r#"[{ENTRY},{{"hash":"h","mtime":1,"path":"b.txt","size":2}},{{"hash":"h"}}]"#),
        "",
    );
    let mut delivered = Vec::new();
    let err = decode_v2023_for_each(&json, |_, e| {
        delivered.push(e.path);
        Ok::<_, SnapshotError>(())
    })
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Manifest validation error: path entry missing 'path'"
    );
    assert_eq!(delivered, ["a.txt", "b.txt"]);
}

#[test]
fn duplicate_paths_are_not_checked() {
    let json = v2023(&format!("[{ENTRY},{ENTRY}]"), "");
    let (entries, summary) = stream(&json).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(summary.file_count, 2);
    assert_eq!(
        decode_v2023(&json).unwrap_err().to_string(),
        "Manifest validation error: duplicate path: a.txt"
    );
}

// ===== The callback =====

#[derive(Debug, PartialEq)]
enum AppError {
    Full(String),
    Decode(String),
}

impl From<SnapshotError> for AppError {
    fn from(e: SnapshotError) -> Self {
        AppError::Decode(e.to_string())
    }
}

#[test]
fn a_callback_error_stops_the_decode_and_is_returned_unchanged() {
    let json = v2023(
        &format!(r#"[{ENTRY},{{"hash":"h","mtime":1,"path":"b.txt","size":2}},bad json"#),
        "",
    );
    let mut seen = 0;
    let err = decode_v2023_for_each(&json, |_, e| {
        seen += 1;
        Err(AppError::Full(e.path))
    })
    .unwrap_err();
    assert_eq!(err, AppError::Full("a.txt".into()));
    assert_eq!(seen, 1);
}

#[test]
fn decode_errors_convert_into_the_callers_error_type() {
    let err = decode_v2023_for_each("{", |_, _| Ok::<_, AppError>(())).unwrap_err();
    assert_eq!(
        err,
        AppError::Decode(
            "Manifest validation error: Invalid JSON: EOF while parsing an object at line 1 column 1"
                .into()
        )
    );
}

// ===== The reader =====

/// Yields `data`, then fails.
struct FailsAfter<'a>(&'a [u8]);

impl io::Read for FailsAfter<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.0.is_empty() {
            return Err(io::Error::other("connection reset"));
        }
        let n = buf.len().min(self.0.len());
        buf[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}

#[test]
fn a_read_error_is_an_io_error() {
    let json = v2023(&format!("[{ENTRY},{ENTRY}]"), "");
    // The text up to the end of the first entry and its comma.
    let cut = json.find(ENTRY).unwrap() + ENTRY.len() + 1;
    let mut delivered = 0;
    let err = decode_v2023_for_each_from_reader(FailsAfter(&json.as_bytes()[..cut]), |_, _| {
        delivered += 1;
        Ok::<_, SnapshotError>(())
    })
    .unwrap_err();
    assert!(matches!(err, SnapshotError::Io(_)), "{err:?}");
    assert_eq!(err.to_string(), "IO error: connection reset");
    assert_eq!(delivered, 1);
}
