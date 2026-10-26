// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! WAL journaling setup shared by the SQLite-backed caches.

use std::path::Path;

use tracing::warn;

/// Ask SQLite to switch `conn` to WAL journaling, and return the journal
/// mode it reports afterwards (lowercase, e.g. `"wal"`).
///
/// SQLite does not fail when it cannot enter WAL mode (for example on a
/// filesystem without the shared-memory support WAL needs, or for an
/// in-memory database). It keeps the previous mode and reports that mode
/// instead. The caches are an optimization, so that is not an error here
/// either: the cache stays usable without WAL, which matches the Python
/// `deadline-cloud` cache. A warning naming `db_path` and the mode in effect
/// is logged so the fallback is visible. Errors from the PRAGMA itself are
/// returned as [`crate::SnapshotError::Cache`].
pub(crate) fn enable_wal(conn: &rusqlite::Connection, db_path: &Path) -> crate::Result<String> {
    let mode: String = conn
        .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))
        .map_err(|e| crate::SnapshotError::Cache(e.to_string()))?;
    let mode = mode.to_ascii_lowercase();
    if mode != "wal" {
        warn!(
            path = %db_path.display(),
            journal_mode = %mode,
            "SQLite could not enable WAL journaling for the cache; continuing in the reported journal mode"
        );
    }
    Ok(mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn file_database_switches_to_wal() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("test.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        assert_eq!(enable_wal(&conn, &db_path).unwrap(), "wal");
    }

    /// An in-memory database cannot use WAL. SQLite keeps the `memory`
    /// journal mode without an error, and `enable_wal` reports that mode
    /// rather than failing, so a cache that cannot use WAL stays usable.
    #[test]
    fn unsupported_database_falls_back_without_error() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        assert_eq!(enable_wal(&conn, Path::new(":memory:")).unwrap(), "memory");
    }
}
