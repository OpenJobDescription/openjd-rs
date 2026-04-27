// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const ENTRY_EXPIRY_DAYS: u64 = 30;

pub struct S3CheckCache {
    conn: Mutex<rusqlite::Connection>,
}

impl S3CheckCache {
    pub fn new(cache_dir: impl AsRef<Path>) -> crate::Result<Self> {
        let dir = cache_dir.as_ref();
        std::fs::create_dir_all(dir)?;
        let db_path = dir.join("s3_check_cache.db");
        let conn = rusqlite::Connection::open(&db_path)
            .map_err(|e| crate::SnapshotError::Cache(e.to_string()))?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| crate::SnapshotError::Cache(e.to_string()))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS s3checkV1(
                s3_key text primary key,
                last_seen_time timestamp
            );",
        )
        .map_err(|e| crate::SnapshotError::Cache(e.to_string()))?;

        // Prune expired entries (older than 30 days) on open
        let cutoff = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64()
            - (ENTRY_EXPIRY_DAYS as f64 * 86400.0);
        conn.execute(
            "DELETE FROM s3checkV1 WHERE CAST(last_seen_time AS REAL) < ?1",
            rusqlite::params![cutoff],
        )
        .map_err(|e| crate::SnapshotError::Cache(e.to_string()))?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_default() -> crate::Result<Self> {
        let home = std::env::var("HOME").map_err(|_| {
            crate::SnapshotError::Cache("$HOME is not set, cannot locate default cache".into())
        })?;
        Self::new(PathBuf::from(home).join(".deadline/job_attachments"))
    }

    /// Check if an entry exists and hasn't expired (30 days).
    pub fn get_entry(&self, s3_key: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        let result: String = conn
            .query_row(
                "SELECT last_seen_time FROM s3checkV1 WHERE s3_key = ?1",
                rusqlite::params![s3_key],
                |row| match row.get_ref(0)?.as_str() {
                    Ok(s) => Ok(s.to_string()),
                    Err(_) => {
                        let f: f64 = row.get(0)?;
                        Ok(f.to_string())
                    }
                },
            )
            .ok()?;

        let last_seen: f64 = result.parse().ok()?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_secs_f64();
        if (now - last_seen) / 86400.0 < ENTRY_EXPIRY_DAYS as f64 {
            Some(result)
        } else {
            None
        }
    }

    /// Insert or replace an entry with the current timestamp.
    pub fn put_entry(&self, s3_key: &str) -> crate::Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| crate::SnapshotError::Cache(e.to_string()))?
            .as_secs_f64();
        let time_str = rusqlite::types::Value::Text(now.to_string());
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO s3checkV1 VALUES(?1, ?2)",
            rusqlite::params![s3_key, time_str],
        )
        .map_err(|e| crate::SnapshotError::Cache(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn put_and_get() {
        let tmp = TempDir::new().unwrap();
        let cache = S3CheckCache::new(tmp.path()).unwrap();
        cache.put_entry("bucket/Data/abc123.xxh128").unwrap();
        assert!(cache.get_entry("bucket/Data/abc123.xxh128").is_some());
    }

    #[test]
    fn missing_entry_returns_none() {
        let tmp = TempDir::new().unwrap();
        let cache = S3CheckCache::new(tmp.path()).unwrap();
        assert!(cache.get_entry("nonexistent").is_none());
    }

    #[test]
    fn expired_entry_returns_none() {
        let tmp = TempDir::new().unwrap();
        let cache = S3CheckCache::new(tmp.path()).unwrap();
        let old_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64()
            - (31.0 * 86400.0);
        let time_str = rusqlite::types::Value::Text(old_time.to_string());
        let conn = cache.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO s3checkV1 VALUES(?1, ?2)",
            rusqlite::params!["old_key", time_str],
        )
        .unwrap();
        drop(conn);
        assert!(cache.get_entry("old_key").is_none());
    }

    #[test]
    fn recent_entry_not_expired() {
        let tmp = TempDir::new().unwrap();
        let cache = S3CheckCache::new(tmp.path()).unwrap();
        let recent_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64()
            - 86400.0;
        let time_str = rusqlite::types::Value::Text(recent_time.to_string());
        let conn = cache.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO s3checkV1 VALUES(?1, ?2)",
            rusqlite::params!["recent_key", time_str],
        )
        .unwrap();
        drop(conn);
        assert!(cache.get_entry("recent_key").is_some());
    }
}
