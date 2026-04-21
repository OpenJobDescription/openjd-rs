// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Error message quality tests for openjd-snapshots.
//!
//! Every error variant's Display output is pinned here to catch regressions
//! in error message quality.

use openjd_snapshots::SnapshotError;

// ═══════════════════════════════════════════════════════════════════════
// Io
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn io_error() {
    let err = SnapshotError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "No such file or directory",
    ));
    assert_eq!(err.to_string(), "IO error: No such file or directory");
}

// ═══════════════════════════════════════════════════════════════════════
// Validation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validation_error() {
    let err = SnapshotError::Validation("duplicate path: foo/bar.txt".into());
    assert_eq!(
        err.to_string(),
        "Manifest validation error: duplicate path: foo/bar.txt"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// FileNotFound
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn file_not_found_error() {
    let err = SnapshotError::FileNotFound("/nonexistent/dir".into());
    assert_eq!(err.to_string(), "File not found: /nonexistent/dir");
}

// ═══════════════════════════════════════════════════════════════════════
// Cancelled
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cancelled_error() {
    let err = SnapshotError::Cancelled;
    assert_eq!(err.to_string(), "Operation cancelled");
}

// ═══════════════════════════════════════════════════════════════════════
// Cache
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cache_error() {
    let err = SnapshotError::Cache("database is locked".into());
    assert_eq!(err.to_string(), "Cache error: database is locked");
}

// ═══════════════════════════════════════════════════════════════════════
// S3
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn s3_error() {
    let err = SnapshotError::S3("AccessDenied: bucket policy".into());
    assert_eq!(err.to_string(), "S3 error: AccessDenied: bucket policy");
}

// ═══════════════════════════════════════════════════════════════════════
// Task
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn task_error() {
    let err = SnapshotError::Task("task panicked".into());
    assert_eq!(err.to_string(), "Task error: task panicked");
}

// ═══════════════════════════════════════════════════════════════════════
// Other
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn other_error() {
    let err = SnapshotError::Other("unexpected condition".into());
    assert_eq!(err.to_string(), "unexpected condition");
}
