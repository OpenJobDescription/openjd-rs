// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Error types for openjd-sessions.

use std::path::PathBuf;

use crate::session::SessionState;

/// Errors that can occur during session operations.
///
/// ```
/// use openjd_sessions::SessionError;
///
/// let err = SessionError::Runtime("something went wrong".into());
/// assert_eq!(err.to_string(), "something went wrong");
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SessionError {
    /// Session is not in the expected state for the requested operation.
    #[error("Session must be in {} state, current: {current}", format_expected(.expected))]
    InvalidState {
        expected: Vec<SessionState>,
        current: SessionState,
    },

    /// An environment's onEnter or onExit script failed.
    #[error("Environment '{name}' {action} failed: {reason}")]
    EnvironmentScriptFailed {
        name: String,
        action: String,
        reason: String,
    },

    /// Failed to resolve a format string expression.
    #[error("Failed to resolve {context}: {reason}")]
    FormatString { context: String, reason: String },

    /// Failed to write an embedded file.
    #[error("Failed to write embedded file '{name}': {source}")]
    EmbeddedFile {
        name: String,
        #[source]
        source: std::io::Error,
    },

    /// Failed to create or access the working directory.
    #[error("Failed to create working directory {path}: {source}")]
    WorkingDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Subprocess failed to start.
    #[error("Failed to start subprocess '{command}': {source}")]
    SubprocessStart {
        command: String,
        #[source]
        source: std::io::Error,
    },

    /// Failed to create or manage a temp directory.
    #[error("Failed to create temp directory in {path}: {source}")]
    TempDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// A generic runtime error.
    #[error("{0}")]
    Runtime(String),

    /// Attempted to enter an environment that is already active in this session.
    #[error("Environment {id} has already been entered in this Session.")]
    DuplicateEnvironment { id: String },

    /// Referenced an environment identifier that does not exist in this session.
    #[error("Unknown environment identifier: {identifier}")]
    UnknownEnvironment { identifier: String },

    /// Failed to set file ownership or permissions (chown/chmod).
    #[error("Failed to set permissions on '{path}': {reason}")]
    PathPermissions { path: String, reason: String },

    /// Cross-user helper IPC communication failure.
    #[error("Cross-user helper error: {0}")]
    HelperCommunication(String),

    /// Attempted to exit an environment out of LIFO order.
    #[error(
        "Must exit the most recently entered environment first. Expected {expected}, got {got}"
    )]
    LifoViolation { expected: String, got: String },
}

fn format_expected(states: &[SessionState]) -> String {
    match states {
        [single] => single.to_string(),
        _ => states
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join(" or "),
    }
}
