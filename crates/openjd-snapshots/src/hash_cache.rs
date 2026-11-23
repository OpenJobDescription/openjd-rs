// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub const WHOLE_FILE_RANGE_END: i64 = -1;

/// Normalize a path to a consistent cache key.
/// On Windows, backslashes are converted to forward slashes so that
/// paths from the manifest (normalized) match paths from the filesystem.
fn normalize_cache_key(path: &Path) -> Vec<u8> {
    #[allow(unused_mut)]
    let mut s = path.to_string_lossy().into_owned();
    #[cfg(windows)]
    {
        s = s.replace('\\', "/");
    }
    s.into_bytes()
}

#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub struct HashCache {
    conn: Mutex<rusqlite::Connection>,
}

impl HashCache {
    #[allow(
        missing_docs,
        reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
    )]
    pub fn new(cache_dir: impl AsRef<Path>) -> crate::Result<Self> {
        let dir = cache_dir.as_ref();
        std::fs::create_dir_all(dir)?;
        let db_path = dir.join("hash_cache.db");
        let conn = rusqlite::Connection::open(&db_path)
            .map_err(|e| crate::SnapshotError::cache_db(&db_path, e))?;
        crate::sqlite_wal::enable_wal(&conn, &db_path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS hashesV4(
                file_path blob,
                hash_algorithm text,
                range_start integer,
                range_end integer,
                file_hash text,
                last_modified_time timestamp,
                PRIMARY KEY (file_path, hash_algorithm, range_start, range_end)
            );",
        )
        .map_err(|e| crate::SnapshotError::cache_db(&db_path, e))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    #[allow(
        missing_docs,
        reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
    )]
    pub fn open_default() -> crate::Result<Self> {
        let home = std::env::var("HOME").map_err(|_| {
            crate::SnapshotError::Cache("$HOME is not set, cannot locate default cache".into())
        })?;
        Self::new(PathBuf::from(home).join(".deadline/job_attachments"))
    }

    #[allow(
        missing_docs,
        reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
    )]
    pub fn get(
        &self,
        file_path: &Path,
        algorithm: &str,
        range_start: i64,
        range_end: i64,
    ) -> Option<(String, u64)> {
        let path_bytes = normalize_cache_key(file_path);
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT file_hash, last_modified_time FROM hashesV4
                 WHERE file_path = ?1 AND hash_algorithm = ?2
                   AND range_start = ?3 AND range_end = ?4",
            rusqlite::params![path_bytes, algorithm, range_start, range_end],
            |row| {
                let hash: String = row.get(0)?;
                let mtime: u64 = match row.get_ref(1)?.as_str() {
                    Ok(s) => s.parse::<u64>().unwrap_or(0),
                    Err(_) => row.get::<_, i64>(1)? as u64,
                };
                Ok((hash, mtime))
            },
        )
        .ok()
    }

    #[allow(
        missing_docs,
        reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
    )]
    pub fn put(
        &self,
        file_path: &Path,
        algorithm: &str,
        range_start: i64,
        range_end: i64,
        hash: &str,
        mtime: u64,
    ) -> crate::Result<()> {
        let path_bytes = normalize_cache_key(file_path);
        let mtime_text = rusqlite::types::Value::Text(mtime.to_string());
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO hashesV4
                 (file_path, hash_algorithm, range_start, range_end, file_hash, last_modified_time)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                path_bytes,
                algorithm,
                range_start,
                range_end,
                hash,
                mtime_text
            ],
        )
        .map_err(|e| crate::SnapshotError::Cache(e.to_string()))?;
        Ok(())
    }

    #[allow(
        missing_docs,
        reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
    )]
    pub fn get_if_fresh(
        &self,
        file_path: &Path,
        algorithm: &str,
        range_start: i64,
        range_end: i64,
        current_mtime: u64,
    ) -> Option<String> {
        let (hash, cached_mtime) = self.get(file_path, algorithm, range_start, range_end)?;
        if cached_mtime == current_mtime {
            Some(hash)
        } else {
            None
        }
    }

    /// The cached chunk hashes of a chunked file, one per range of
    /// [`split_ranges`](crate::hash::split_ranges)`(file_size, chunk_size)`,
    /// if every range is cached and fresh for `current_mtime`. `None` if any
    /// range misses or `chunk_size` is 0.
    ///
    /// The `hash`, `hash_upload`, and `download` operations all key chunk
    /// entries through this and [`put_chunks`](Self::put_chunks).
    pub(crate) fn get_chunks_if_fresh(
        &self,
        file_path: &Path,
        algorithm: &str,
        file_size: u64,
        chunk_size: u64,
        current_mtime: u64,
    ) -> Option<Vec<String>> {
        let hashes: Option<Vec<String>> = crate::hash::split_ranges(file_size, chunk_size)
            .map(|(start, end)| {
                self.get_if_fresh(
                    file_path,
                    algorithm,
                    start as i64,
                    end as i64,
                    current_mtime,
                )
            })
            .collect();
        hashes.filter(|h| !h.is_empty())
    }

    /// Caches a chunked file's chunk hashes under the ranges
    /// [`get_chunks_if_fresh`](Self::get_chunks_if_fresh) reads. Writes
    /// nothing if `hashes` does not hold exactly one hash per range, so a
    /// hash list that does not fit the file cannot be cached under the
    /// wrong ranges. Individual write failures are ignored, as a cache miss
    /// later only costs a rehash.
    pub(crate) fn put_chunks(
        &self,
        file_path: &Path,
        algorithm: &str,
        file_size: u64,
        chunk_size: u64,
        hashes: &[String],
        mtime: u64,
    ) {
        let ranges = crate::hash::split_ranges(file_size, chunk_size);
        if ranges.clone().count() != hashes.len() {
            return;
        }
        for ((start, end), hash) in ranges.zip(hashes) {
            let _ = self.put(file_path, algorithm, start as i64, end as i64, hash, mtime);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_cache() -> (TempDir, HashCache) {
        let tmp = TempDir::new().unwrap();
        let cache = HashCache::new(tmp.path()).unwrap();
        (tmp, cache)
    }

    #[test]
    fn put_and_get() {
        let (_tmp, cache) = make_cache();
        let path = Path::new("/tmp/test.txt");
        cache
            .put(path, "xxh128", 0, WHOLE_FILE_RANGE_END, "abc123", 1000)
            .unwrap();
        let (hash, mtime) = cache.get(path, "xxh128", 0, WHOLE_FILE_RANGE_END).unwrap();
        assert_eq!(hash, "abc123");
        assert_eq!(mtime, 1000);
    }

    #[test]
    fn get_if_fresh_matching_mtime() {
        let (_tmp, cache) = make_cache();
        let path = Path::new("/tmp/test.txt");
        cache
            .put(path, "xxh128", 0, WHOLE_FILE_RANGE_END, "abc123", 1000)
            .unwrap();
        let hash = cache
            .get_if_fresh(path, "xxh128", 0, WHOLE_FILE_RANGE_END, 1000)
            .unwrap();
        assert_eq!(hash, "abc123");
    }

    #[test]
    fn get_if_fresh_mismatched_mtime() {
        let (_tmp, cache) = make_cache();
        let path = Path::new("/tmp/test.txt");
        cache
            .put(path, "xxh128", 0, WHOLE_FILE_RANGE_END, "abc123", 1000)
            .unwrap();
        assert!(cache
            .get_if_fresh(path, "xxh128", 0, WHOLE_FILE_RANGE_END, 2000)
            .is_none());
    }

    #[test]
    fn missing_entry_returns_none() {
        let (_tmp, cache) = make_cache();
        assert!(cache
            .get(
                Path::new("/no/such/file"),
                "xxh128",
                0,
                WHOLE_FILE_RANGE_END
            )
            .is_none());
    }

    /// The spec (snapshot_hash_cache.md, "Hash Cache" properties) requires WAL
    /// journaling. WAL mode is persistent in the database file, so check it
    /// through an independent connection rather than the cache's own.
    #[test]
    fn new_enables_wal_journaling() {
        let (tmp, _cache) = make_cache();
        let conn = rusqlite::Connection::open(tmp.path().join("hash_cache.db")).unwrap();
        let mode: String = conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    /// A failed WAL PRAGMA makes `new()` fail (snapshot_hash_cache.md,
    /// "Journal Mode"). The database already has the cache table but is in
    /// rollback-journal mode, and another connection holds a read
    /// transaction, so only the PRAGMA needs a lock that it cannot get. This
    /// waits out the connection's 5-second busy timeout.
    #[test]
    fn new_fails_when_wal_pragma_fails() {
        let tmp = TempDir::new().unwrap();
        drop(HashCache::new(tmp.path()).unwrap());
        let reader = rusqlite::Connection::open(tmp.path().join("hash_cache.db")).unwrap();
        reader
            .pragma_update(None, "journal_mode", "DELETE")
            .unwrap();
        reader.execute_batch("BEGIN;").unwrap();
        let _: i64 = reader
            .query_row("SELECT count(*) FROM hashesV4", [], |row| row.get(0))
            .unwrap();

        match HashCache::new(tmp.path()) {
            Ok(_) => panic!("HashCache::new succeeded while the WAL PRAGMA was blocked"),
            Err(e) => assert_eq!(
                e.to_string(),
                format!(
                    "Cache error: {}: database is locked",
                    tmp.path().join("hash_cache.db").display()
                )
            ),
        }
    }

    /// Errors opening the database name the database file.
    #[test]
    fn new_open_error_names_the_file() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("hash_cache.db");
        std::fs::create_dir(&db_path).unwrap();
        match HashCache::new(tmp.path()) {
            Ok(_) => panic!("HashCache::new succeeded with a directory as its database"),
            Err(e) => assert_eq!(
                e.to_string(),
                format!(
                    "Cache error: {}: unable to open database file",
                    db_path.display()
                )
            ),
        }
    }

    /// Errors creating the table name the database file. The database is
    /// already in WAL mode, so the PRAGMA succeeds, and another connection
    /// holds a write transaction, so `CREATE TABLE` waits out the 5-second
    /// busy timeout and fails.
    #[test]
    fn new_create_table_error_names_the_file() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("hash_cache.db");
        let writer = rusqlite::Connection::open(&db_path).unwrap();
        writer.pragma_update(None, "journal_mode", "WAL").unwrap();
        writer
            .execute_batch("CREATE TABLE other(x); BEGIN IMMEDIATE;")
            .unwrap();
        match HashCache::new(tmp.path()) {
            Ok(_) => panic!("HashCache::new succeeded while CREATE TABLE was blocked"),
            Err(e) => assert_eq!(
                e.to_string(),
                format!("Cache error: {}: database is locked", db_path.display())
            ),
        }
    }

    #[test]
    fn whole_file_and_range_coexist() {
        let (_tmp, cache) = make_cache();
        let path = Path::new("/tmp/big.bin");
        cache
            .put(path, "xxh128", 0, WHOLE_FILE_RANGE_END, "whole_hash", 500)
            .unwrap();
        cache
            .put(path, "xxh128", 0, 1024, "chunk_hash", 500)
            .unwrap();

        let (h1, _) = cache.get(path, "xxh128", 0, WHOLE_FILE_RANGE_END).unwrap();
        let (h2, _) = cache.get(path, "xxh128", 0, 1024).unwrap();
        assert_eq!(h1, "whole_hash");
        assert_eq!(h2, "chunk_hash");
    }
}
