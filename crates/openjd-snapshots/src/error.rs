// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use thiserror::Error;

/// Errors that can occur during snapshot operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SnapshotError {
    /// An I/O error occurred during file or directory operations.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Manifest content failed validation (bad paths, missing hashes, etc.).
    #[error("Manifest validation error: {0}")]
    Validation(String),

    /// A required file or directory was not found.
    #[error("File not found: {0}")]
    FileNotFound(String),

    /// The operation was cancelled via the cancellation flag.
    #[error("Operation cancelled")]
    Cancelled,

    /// A cache (hash cache or S3 check cache) operation failed.
    #[error("Cache error: {0}")]
    Cache(String),

    /// An S3 or STS API call failed.
    #[error("S3 error: {0}")]
    S3(String),

    /// A background task (tokio spawn/runtime) failed.
    #[error("Task error: {0}")]
    Task(String),
}

#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub type Result<T> = std::result::Result<T, SnapshotError>;

impl SnapshotError {
    /// Wrap an SQLite error from opening or setting up the cache database at
    /// `db_path` as [`SnapshotError::Cache`], with the path prepended in the
    /// same way I/O errors are (`Cache error: <path>: <sqlite message>`).
    pub(crate) fn cache_db(db_path: &std::path::Path, err: rusqlite::Error) -> Self {
        let path = db_path.display().to_string();
        let msg = err.to_string();
        // rusqlite appends ": <path>" to errors from opening the database
        // (SQLITE_CANTOPEN). Drop that copy so the path appears once.
        let msg = msg
            .strip_suffix(path.as_str())
            .and_then(|m| m.strip_suffix(": "))
            .unwrap_or(&msg);
        SnapshotError::Cache(format!("{path}: {msg}"))
    }
}
