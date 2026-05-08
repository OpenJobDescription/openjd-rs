// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use xxhash_rust::xxh3::{xxh3_128, Xxh3Default};

pub const DEFAULT_FILE_CHUNK_SIZE: i64 = 256 * 1024 * 1024;
pub const WHOLE_FILE_CHUNK_SIZE: i64 = -1;
pub const DEFAULT_S3_MULTIPART_PART_SIZE: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HashAlgorithm {
    #[serde(rename = "xxh128")]
    Xxh128,
}

impl HashAlgorithm {
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Xxh128 => "xxh128",
        }
    }
}

impl std::fmt::Display for HashAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.extension())
    }
}

/// Computes xxh128 hash of data, returns lowercase hex string.
pub fn hash_data(data: &[u8]) -> String {
    format!("{:032x}", xxh3_128(data))
}

/// Reads file in streaming fashion and computes xxh128 hash.
pub fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Xxh3Default::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:032x}", hasher.digest128()))
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
/// Uses `read_exact` to ensure chunk boundaries are determined by `chunk_size`,
/// not by how many bytes a single `read()` call returns.
pub fn hash_file_chunked(
    path: &Path,
    chunk_size: u64,
    expected_size: u64,
) -> std::io::Result<Vec<String>> {
    if chunk_size == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "hash_file_chunked requires chunk_size > 0",
        ));
    }
    let file = File::open(path)?;
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

    let mut file = file;
    let full_chunks = actual_size / chunk_size;
    let remainder_len = (actual_size % chunk_size) as usize;
    let mut hashes = Vec::with_capacity(full_chunks as usize + 1);
    let mut buf = vec![0u8; chunk_size as usize];

    for _ in 0..full_chunks {
        file.read_exact(&mut buf)?;
        hashes.push(hash_data(&buf));
    }
    if remainder_len > 0 {
        buf.truncate(remainder_len);
        file.read_exact(&mut buf)?;
        hashes.push(hash_data(&buf));
    }
    if hashes.is_empty() {
        hashes.push(hash_data(&[]));
    }
    Ok(hashes)
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
}
