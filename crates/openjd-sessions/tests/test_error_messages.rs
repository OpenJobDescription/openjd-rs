// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Error message quality tests for openjd-sessions.
//!
//! Every error variant's Display output is pinned here to catch regressions
//! in error message quality and ensure cross-implementation parity.

use openjd_sessions::{SessionError, SessionState};

// ═══════════════════════════════════════════════════════════════════════
// InvalidState
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn invalid_state_single_expected() {
    let err = SessionError::InvalidState {
        expected: vec![SessionState::Ready],
        current: SessionState::Running,
    };
    assert_eq!(
        err.to_string(),
        "Session must be in READY state, current: RUNNING"
    );
}

#[test]
fn invalid_state_multiple_expected() {
    let err = SessionError::InvalidState {
        expected: vec![SessionState::Ready, SessionState::ReadyEnding],
        current: SessionState::Running,
    };
    assert_eq!(
        err.to_string(),
        "Session must be in READY or READY_ENDING state, current: RUNNING"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// EnvironmentScriptFailed
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn environment_script_failed() {
    let err = SessionError::EnvironmentScriptFailed {
        name: "my-env".into(),
        action: "enter".into(),
        reason: "exit code 1".into(),
    };
    assert_eq!(
        err.to_string(),
        "Environment 'my-env' enter failed: exit code 1"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// FormatString
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn format_string_error() {
    let err = SessionError::FormatString {
        context: "action command".into(),
        reason: "Undefined variable 'Param.X'".into(),
    };
    assert_eq!(
        err.to_string(),
        "Failed to resolve action command: Undefined variable 'Param.X'"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// EmbeddedFile
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn embedded_file_error() {
    let err = SessionError::EmbeddedFile {
        name: "MyScript".into(),
        source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Permission denied"),
    };
    assert_eq!(
        err.to_string(),
        "Failed to write embedded file 'MyScript': Permission denied"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// WorkingDirectory
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn working_directory_error() {
    let err = SessionError::WorkingDirectory {
        path: "/tmp/session-abc".into(),
        source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Permission denied"),
    };
    assert_eq!(
        err.to_string(),
        "Failed to create working directory /tmp/session-abc: Permission denied"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// SubprocessStart
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn subprocess_start_error() {
    let err = SessionError::SubprocessStart {
        command: "/usr/bin/missing".into(),
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory"),
    };
    assert_eq!(
        err.to_string(),
        "Failed to start subprocess '/usr/bin/missing': No such file or directory"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TempDir
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn temp_dir_error() {
    let err = SessionError::TempDir {
        path: "/tmp".into(),
        source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Permission denied"),
    };
    assert_eq!(
        err.to_string(),
        "Failed to create temp directory in /tmp: Permission denied"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Runtime
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn runtime_error() {
    let err = SessionError::Runtime("something went wrong".into());
    assert_eq!(err.to_string(), "something went wrong");
}

// ═══════════════════════════════════════════════════════════════════════
// LifoViolation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn lifo_violation_error() {
    let err = SessionError::LifoViolation {
        expected: "env-2".into(),
        got: "env-1".into(),
    };
    assert_eq!(
        err.to_string(),
        "Must exit the most recently entered environment first. Expected env-2, got env-1"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// DuplicateEnvironment
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn duplicate_environment_error() {
    let err = SessionError::DuplicateEnvironment {
        id: "my-env".into(),
    };
    assert_eq!(
        err.to_string(),
        "Environment my-env has already been entered in this Session."
    );
}

// ═══════════════════════════════════════════════════════════════════════
// UnknownEnvironment
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn unknown_environment_error() {
    let err = SessionError::UnknownEnvironment {
        identifier: "nonexistent".into(),
    };
    assert_eq!(
        err.to_string(),
        "Unknown environment identifier: nonexistent"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// PathPermissions
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn permissions_error() {
    let err = SessionError::PathPermissions {
        path: "/tmp/session/script.sh".into(),
        reason: "Operation not permitted".into(),
    };
    assert_eq!(
        err.to_string(),
        "Failed to set permissions on '/tmp/session/script.sh': Operation not permitted"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// HelperCommunication
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn helper_communication_error() {
    let err = SessionError::HelperCommunication("parse error: invalid JSON".into());
    assert_eq!(
        err.to_string(),
        "Cross-user helper error: parse error: invalid JSON"
    );
}
