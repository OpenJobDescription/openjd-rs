// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use xxhash_rust::xxh3::{xxh3_128, Xxh3Default};

#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub const DEFAULT_FILE_CHUNK_SIZE: i64 = 256 * 1024 * 1024;
#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub const WHOLE_FILE_CHUNK_SIZE: i64 = -1;
#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub const DEFAULT_S3_MULTIPART_PART_SIZE: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub enum HashAlgorithm {
    #[serde(rename = "xxh128")]
    Xxh128,
}

impl HashAlgorithm {
    /// Every supported algorithm.
    pub(crate) const ALL: [Self; 1] = [Self::Xxh128];

    #[allow(
        missing_docs,
        reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
    )]
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Xxh128 => "xxh128",
        }
    }

    /// The algorithm whose [`extension`](Self::extension) is `name`, as the
    /// manifest codecs spell it in `hashAlg`.
    pub(crate) fn from_extension(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.extension() == name)
    }
}

impl std::fmt::Display for HashAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.extension())
    }
}

/// The text form of a content hash: 32 lowercase hex digits.
fn digest_hex(digest: u128) -> String {
    format!("{digest:032x}")
}

/// Computes xxh128 hash of data, returns lowercase hex string.
pub fn hash_data(data: &[u8]) -> String {
    digest_hex(xxh3_128(data))
}

/// Opens `path` for reading, naming the path in any error.
pub(crate) fn open_file(path: &Path) -> std::io::Result<File> {
    File::open(path).map_err(|e| std::io::Error::new(e.kind(), format!("{}: {e}", path.display())))
}

/// Hashes everything `reader` yields, returning the hash and the byte count.
fn hash_reader(mut reader: impl Read) -> std::io::Result<(String, u64)> {
    let mut hasher = Xxh3Default::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    Ok((digest_hex(hasher.digest128()), total))
}

/// Reads file in streaming fashion and computes xxh128 hash.
pub fn hash_file(path: &Path) -> std::io::Result<String> {
    hash_reader(open_file(path)?).map(|(hash, _)| hash)
}

/// Hashes the `len` bytes of the file at `path` starting at `offset`, the
/// same hash [`hash_data`] gives those bytes. Fails with `UnexpectedEof` if
/// the file ends first.
pub(crate) fn hash_file_range(path: &Path, offset: u64, len: u64) -> std::io::Result<String> {
    use std::io::{Seek, SeekFrom};
    let mut file = open_file(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let (hash, read) = hash_reader(file.take(len))?;
    if read != len {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            format!(
                "{}: expected {len} bytes at offset {offset}, read {read}",
                path.display()
            ),
        ));
    }
    Ok(hash)
}

/// Splits `0..total` into consecutive half-open ranges of `piece` bytes,
/// the last one shorter if `piece` does not divide `total`. `total == 0` or
/// `piece == 0` gives no ranges.
///
/// This is the one rule for both file chunks (`piece` = the manifest's
/// chunk size; the ranges are the hash-cache keys of a chunked file) and
/// multipart transfer parts (`piece` = the part size).
pub(crate) fn split_ranges(total: u64, piece: u64) -> impl Iterator<Item = (u64, u64)> + Clone {
    let count = if piece == 0 { 0 } else { total.div_ceil(piece) };
    (0..count).map(move |i| {
        let start = i * piece;
        (start, total.min(start + piece))
    })
}

/// Returns true if a file of `file_size` bytes is hashed in chunks under a
/// manifest chunk size of `chunk_size`: chunking is on (a positive chunk
/// size, not [`WHOLE_FILE_CHUNK_SIZE`]) and the file is larger than one
/// chunk. A file of exactly one chunk gets a whole-file hash.
pub(crate) fn uses_chunks(file_size: u64, chunk_size: i64) -> bool {
    chunk_size > 0 && file_size > chunk_size as u64
}

/// Reads the file at `path` chunk by chunk, calling `each` with every chunk
/// in order: [`split_ranges`]`(size, chunk_size)`, or one empty chunk for an
/// empty file. [`hash_file_chunked`] and the upload path both read chunks
/// through this, so both enforce `expected_size`.
///
/// Fails with `InvalidInput` if `chunk_size` is 0 and with `InvalidData` if
/// the file's size on disk is not `expected_size`.
pub(crate) fn read_chunks(
    path: &Path,
    chunk_size: u64,
    expected_size: u64,
    mut each: impl FnMut(&[u8]),
) -> std::io::Result<()> {
    if chunk_size == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "hash_file_chunked requires chunk_size > 0",
        ));
    }
    let mut file = open_file(path)?;
    let actual_size = file.metadata()?.len();
    if actual_size != expected_size {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "file size mismatch for {}: expected {expected_size}, found {actual_size}",
                path.display()
            ),
        ));
    }
    if actual_size == 0 {
        each(&[]);
        return Ok(());
    }
    let mut buf = vec![0u8; chunk_size.min(actual_size) as usize];
    for (start, end) in split_ranges(actual_size, chunk_size) {
        let chunk = &mut buf[..(end - start) as usize];
        file.read_exact(chunk)?;
        each(chunk);
    }
    Ok(())
}

/// Hashes file in chunks, returns vec of hex hash strings.
///
/// `chunk_size` must be strictly positive. Pass a positive value in bytes.
/// `WHOLE_FILE_CHUNK_SIZE` is not a valid argument — callers should use
/// [`hash_file`] for whole-file hashing.
///
/// `expected_size` is the file size the caller expects on disk (typically
/// from the manifest entry). If the actual file size differs, this function
/// returns an `InvalidData` error — content-addressed hashing requires the
/// file on disk to match what the manifest claims.
///
/// Chunk boundaries are determined by `chunk_size`, not by how many bytes a
/// single `read()` call returns.
pub fn hash_file_chunked(
    path: &Path,
    chunk_size: u64,
    expected_size: u64,
) -> std::io::Result<Vec<String>> {
    let mut hashes = Vec::new();
    read_chunks(path, chunk_size, expected_size, |chunk| {
        hashes.push(hash_data(chunk))
    })?;
    Ok(hashes)
}

/// Hashes the file at `path` in one read, returning its whole-file hash and
/// the hash of each [`split_ranges`]`(expected_size, part_size)` part. The
/// whole-file multipart upload hashes the file with this before it uploads,
/// then checks each part it reads again against its part hash, so a file
/// that changes in between is not stored under the earlier hash.
///
/// Fails as [`read_chunks`] does.
pub(crate) fn hash_file_and_parts(
    path: &Path,
    part_size: u64,
    expected_size: u64,
) -> std::io::Result<(String, Vec<String>)> {
    let mut whole = Xxh3Default::new();
    let mut parts = Vec::new();
    read_chunks(path, part_size, expected_size, |part| {
        whole.update(part);
        parts.push(hash_data(part));
    })?;
    Ok((digest_hex(whole.digest128()), parts))
}

/// Formats a byte count as a human-readable string (e.g., "1.5 MB").
pub fn human_readable_file_size(bytes: u64) -> String {
    let mut size = bytes as f64;
    for unit in &["B", "KB", "MB", "GB", "TB", "PB", "EB"] {
        let rounded = (size * 100.0).round() / 100.0;
        if rounded < 1000.0 {
            if *unit == "B" {
                return format!("{} {}", rounded as u64, unit);
            }
            return format!("{rounded} {unit}");
        }
        size /= 1000.0;
    }
    format!("{} EB", (size * 100.0).round() / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn hash_known_data() {
        let h = hash_data(b"hello world");
        assert_eq!(h.len(), 32);
        // Deterministic — same input always produces same hash
        assert_eq!(h, hash_data(b"hello world"));
        // Different input produces different hash
        assert_ne!(h, hash_data(b"goodbye"));
    }

    #[test]
    fn hash_empty_data() {
        let h = hash_data(b"");
        assert_eq!(h.len(), 32);
    }

    #[test]
    fn hash_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("test.txt");
        std::fs::write(&p, b"file content").unwrap();
        let h = hash_file(&p).unwrap();
        assert_eq!(h, hash_data(b"file content"));
    }

    #[test]
    fn hash_chunked_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("chunked.bin");
        let mut f = File::create(&p).unwrap();
        // Write 10 bytes, chunk size 4 => 3 chunks (4+4+2)
        f.write_all(&[0u8; 10]).unwrap();
        drop(f);
        let hashes = hash_file_chunked(&p, 4, 10).unwrap();
        assert_eq!(hashes.len(), 3);
        assert_eq!(hashes[0], hash_data(&[0u8; 4]));
        assert_eq!(hashes[2], hash_data(&[0u8; 2]));
    }

    #[test]
    fn hash_chunked_file_is_deterministic() {
        // Chunk hashing must be deterministic — two hashings of the same file
        // must produce identical chunk-hash vectors.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("testfile");
        let chunk_size: u64 = 1024;
        let data: Vec<u8> = (0..3 * chunk_size).map(|i| (i % 256) as u8).collect();
        std::fs::write(&p, &data).unwrap();

        let h1 = hash_file_chunked(&p, chunk_size, data.len() as u64).unwrap();
        let h2 = hash_file_chunked(&p, chunk_size, data.len() as u64).unwrap();
        assert_eq!(h1.len(), 3);
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_chunked_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("empty.bin");
        File::create(&p).unwrap();
        let hashes = hash_file_chunked(&p, 4, 0).unwrap();
        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0], hash_data(b""));
    }

    #[test]
    fn hash_chunked_rejects_zero_chunk_size() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.bin");
        std::fs::write(&p, b"data").unwrap();
        let err = hash_file_chunked(&p, 0, 4).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("chunk_size > 0"));
    }

    #[test]
    fn hash_chunked_size_mismatch_longer_on_disk() {
        // Manifest says 5 bytes but file is 10 bytes. Must error.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.bin");
        std::fs::write(&p, [0u8; 10]).unwrap();
        let err = hash_file_chunked(&p, 4, 5).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("size"), "{err}");
    }

    #[test]
    fn hash_chunked_size_mismatch_shorter_on_disk() {
        // Manifest says 10 bytes but file is 5 bytes. Must error.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.bin");
        std::fs::write(&p, [0u8; 5]).unwrap();
        let err = hash_file_chunked(&p, 4, 10).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("size"), "{err}");
    }

    #[test]
    fn hash_algorithm_serde() {
        let json = serde_json::to_string(&HashAlgorithm::Xxh128).unwrap();
        assert_eq!(json, "\"xxh128\"");
        let parsed: HashAlgorithm = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, HashAlgorithm::Xxh128);
    }

    #[test]
    fn hash_algorithm_extension() {
        assert_eq!(HashAlgorithm::Xxh128.extension(), "xxh128");
    }

    #[test]
    fn human_readable_bytes() {
        assert_eq!(human_readable_file_size(0), "0 B");
        assert_eq!(human_readable_file_size(1), "1 B");
        assert_eq!(human_readable_file_size(999), "999 B");
    }

    #[test]
    fn human_readable_kilobytes() {
        assert_eq!(human_readable_file_size(1_000), "1 KB");
        assert_eq!(human_readable_file_size(1_500), "1.5 KB");
    }

    #[test]
    fn human_readable_megabytes() {
        assert_eq!(human_readable_file_size(1_000_000), "1 MB");
        assert_eq!(human_readable_file_size(256 * 1024 * 1024), "268.44 MB");
    }

    #[test]
    fn human_readable_gigabytes() {
        assert_eq!(human_readable_file_size(1_000_000_000), "1 GB");
    }

    #[test]
    fn human_readable_terabytes() {
        assert_eq!(human_readable_file_size(1_000_000_000_000), "1 TB");
    }

    #[test]
    fn human_readable_petabytes() {
        assert_eq!(human_readable_file_size(1_000_000_000_000_000), "1 PB");
    }

    #[test]
    fn human_readable_exabytes() {
        assert_eq!(human_readable_file_size(1_000_000_000_000_000_000), "1 EB");
        assert_eq!(human_readable_file_size(u64::MAX), "18.45 EB");
    }

    #[test]
    fn split_ranges_covers_total() {
        let r: Vec<_> = split_ranges(10, 4).collect();
        assert_eq!(r, [(0, 4), (4, 8), (8, 10)]);
        let r: Vec<_> = split_ranges(8, 4).collect();
        assert_eq!(r, [(0, 4), (4, 8)]);
        assert_eq!(split_ranges(3, 4).collect::<Vec<_>>(), [(0, 3)]);
        assert_eq!(split_ranges(0, 4).count(), 0);
        assert_eq!(split_ranges(5, 0).count(), 0);
    }

    #[test]
    fn uses_chunks_needs_positive_size_and_more_than_one_chunk() {
        assert!(uses_chunks(5, 4));
        assert!(!uses_chunks(4, 4));
        assert!(!uses_chunks(3, 4));
        assert!(!uses_chunks(u64::MAX, WHOLE_FILE_CHUNK_SIZE));
        assert!(!uses_chunks(5, 0));
        // A size past i64::MAX still compares as a size, not a negative.
        assert!(uses_chunks(u64::MAX, 4));
    }

    #[test]
    fn hash_file_and_parts_matches_whole_and_chunked_hashes() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"aaaabbbbcc").unwrap();
        f.flush().unwrap();
        let (whole, parts) = hash_file_and_parts(f.path(), 4, 10).unwrap();
        assert_eq!(whole, hash_file(f.path()).unwrap());
        assert_eq!(parts, hash_file_chunked(f.path(), 4, 10).unwrap());
        let err = hash_file_and_parts(f.path(), 4, 9).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "file size mismatch for {}: expected 9, found 10",
                f.path().display()
            )
        );
    }

    #[test]
    fn hash_file_range_matches_hash_data() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"aaaabbbbcc").unwrap();
        f.flush().unwrap();
        assert_eq!(hash_file_range(f.path(), 4, 4).unwrap(), hash_data(b"bbbb"));
        assert_eq!(
            hash_file_range(f.path(), 0, 10).unwrap(),
            hash_data(b"aaaabbbbcc")
        );
        let err = hash_file_range(f.path(), 8, 4).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
        assert_eq!(
            err.to_string(),
            format!(
                "{}: expected 4 bytes at offset 8, read 2",
                f.path().display()
            )
        );
    }

    #[test]
    fn hash_file_names_the_missing_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let missing = dir.path().join("missing.bin");
        let err = hash_file(&missing).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert!(
            err.to_string()
                .starts_with(&format!("{}: ", missing.display())),
            "{err}"
        );
    }

    /// Pins the algorithm names: the serde name (`hashAlg` in manifests),
    /// [`HashAlgorithm::extension`], and [`HashAlgorithm::from_extension`]
    /// agree for every algorithm.
    #[test]
    fn algorithm_names_agree() {
        for alg in HashAlgorithm::ALL {
            let name = alg.extension();
            assert_eq!(serde_json::to_value(alg).unwrap(), name);
            assert_eq!(
                serde_json::from_value::<HashAlgorithm>(name.into()).unwrap(),
                alg
            );
            assert_eq!(HashAlgorithm::from_extension(name), Some(alg));
            assert_eq!(alg.to_string(), name);
        }
        assert_eq!(HashAlgorithm::from_extension("md5"), None);
    }
}
