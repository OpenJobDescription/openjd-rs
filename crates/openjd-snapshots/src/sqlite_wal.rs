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
/// either: the cache stays usable without WAL, as the Python `deadline-cloud`
/// cache does (it does not check the result). A warning naming `db_path` and the mode in effect
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
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;
    use tracing::field::{Field, Visit};
    use tracing::{span, Event, Metadata, Subscriber};

    /// Minimal subscriber that records each event from this crate as
    /// `LEVEL:field=value;field=value;...`, so tests can assert the warning.
    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<String>>>);

    struct Fields(String);

    impl Visit for Fields {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.0.push_str(&format!("{}={:?};", field.name(), value));
        }
    }

    impl Subscriber for Capture {
        fn enabled(&self, metadata: &Metadata<'_>) -> bool {
            metadata.target().starts_with("openjd_snapshots")
        }
        fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
            span::Id::from_u64(1)
        }
        fn record(&self, _: &span::Id, _: &span::Record<'_>) {}
        fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}
        fn event(&self, event: &Event<'_>) {
            let mut fields = Fields(format!("{}:", event.metadata().level()));
            event.record(&mut fields);
            self.0.lock().unwrap().push(fields.0);
        }
        fn enter(&self, _: &span::Id) {}
        fn exit(&self, _: &span::Id) {}
    }

    /// Run `f` with a capturing subscriber, returning its result and the
    /// events it logged.
    fn capture<T>(f: impl FnOnce() -> T) -> (T, Vec<String>) {
        let cap = Capture::default();
        let out = tracing::subscriber::with_default(cap.clone(), f);
        let events = cap.0.lock().unwrap().clone();
        (out, events)
    }

    #[test]
    fn file_database_switches_to_wal_without_warning() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("test.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let (mode, events) = capture(|| enable_wal(&conn, &db_path).unwrap());
        assert_eq!(mode, "wal");
        assert_eq!(events, Vec::<String>::new());
    }

    /// An in-memory database cannot use WAL. SQLite keeps the `memory`
    /// journal mode without an error, and `enable_wal` reports that mode
    /// and logs a warning rather than failing, so a cache that cannot use
    /// WAL stays usable and the fallback is visible.
    #[test]
    fn unsupported_database_falls_back_with_warning() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let (mode, events) = capture(|| enable_wal(&conn, Path::new(":memory:")).unwrap());
        assert_eq!(mode, "memory");
        assert_eq!(
            events,
            vec![
                "WARN:message=SQLite could not enable WAL journaling for the cache; \
                 continuing in the reported journal mode;path=:memory:;journal_mode=memory;"
                    .to_string()
            ]
        );
    }
}
