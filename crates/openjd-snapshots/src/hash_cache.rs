use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const WHOLE_FILE_RANGE_END: i64 = -1;

pub struct HashCache {
    conn: Mutex<rusqlite::Connection>,
}

impl HashCache {
    pub fn new(cache_dir: impl AsRef<Path>) -> crate::Result<Self> {
        let dir = cache_dir.as_ref();
        std::fs::create_dir_all(dir)?;
        let db_path = dir.join("hash_cache.db");
        let conn = rusqlite::Connection::open(&db_path)
            .map_err(|e| crate::SnapshotError::Other(e.to_string()))?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| crate::SnapshotError::Other(e.to_string()))?;
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
        .map_err(|e| crate::SnapshotError::Other(e.to_string()))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_default() -> crate::Result<Self> {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        Self::new(PathBuf::from(home).join(".deadline/job_attachments"))
    }

    pub fn get(
        &self,
        file_path: &Path,
        algorithm: &str,
        range_start: i64,
        range_end: i64,
    ) -> Option<(String, u64)> {
        let path_bytes = file_path.to_string_lossy().into_owned().into_bytes();
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

    pub fn put(
        &self,
        file_path: &Path,
        algorithm: &str,
        range_start: i64,
        range_end: i64,
        hash: &str,
        mtime: u64,
    ) -> crate::Result<()> {
        let path_bytes = file_path.to_string_lossy().into_owned().into_bytes();
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
        .map_err(|e| crate::SnapshotError::Other(e.to_string()))?;
        Ok(())
    }

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
