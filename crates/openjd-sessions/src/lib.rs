// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Open Job Description sessions — local job execution runtime.
//!
//! Mirrors the Python `openjd-sessions-for-python` library.

pub mod action;
pub(crate) mod action_filter;
pub mod action_status;
pub(crate) mod cross_user_helper;
pub mod embedded_files;
pub mod error;
pub(crate) mod helper_binary;
pub mod let_bindings;
pub mod limits;
pub mod logging;
pub mod runner;
pub mod session;
pub mod session_user;
pub(crate) mod subprocess;
pub mod tempdir;
#[cfg(windows)]
pub mod win32;
#[cfg(windows)]
pub(crate) mod win32_locate;
#[cfg(all(windows, not(feature = "test-utils")))]
pub(crate) mod win32_permissions;
// Under the `test-utils` feature, expose win32_permissions to integration
// tests so they can drive DACL setup through the library's own API
// (matching the Python reference's use of `WindowsPermissionHelper`).
#[cfg(all(windows, feature = "test-utils"))]
pub mod win32_permissions;

// The bounded line framer lives in the helper's source tree (std-only) and
// compiles into both runners. Included here under test so this crate can
// unit-test it and pin its `decode_backslashreplace` byte-for-byte to
// `subprocess::decode_backslashreplace`. Same `#[path]` technique as
// win32_locate.rs: the helper is a nested crate, not a shared dependency.
#[cfg(test)]
#[path = "helper/src/framer.rs"]
mod helper_framer;

/// Crate-internal parsers exposed for the fuzz harness in `fuzz/`.
///
/// Subprocess stdout is the most attacker-controlled input the session
/// runtime sees, but the code that parses it is crate-private. This module
/// re-exports exactly that code so `fuzz/` can drive it directly instead of
/// spawning a process per input. Not part of the public API; it exists only
/// under the `fuzzing` feature and may change without notice.
#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub mod fuzzing {
    pub use crate::action_filter::{
        redact_openjd_redacted_env_requests, ActionFilter, ActionMessageKind, ActionMessageValue,
        FilterCallback,
    };

    /// Max bytes of one subprocess output line (`subprocess::LOG_LINE_MAX_LENGTH`).
    pub const LOG_LINE_MAX_LENGTH: usize = crate::subprocess::LOG_LINE_MAX_LENGTH;

    /// `subprocess::decode_backslashreplace`: decode raw child output bytes.
    pub fn decode_backslashreplace(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
        crate::subprocess::decode_backslashreplace(bytes)
    }

    /// `subprocess::truncate_line`: cap a line at `LOG_LINE_MAX_LENGTH` bytes.
    pub fn truncate_line(line: &str) -> &str {
        crate::subprocess::truncate_line(line)
    }

    /// `subprocess::process_line`: run one decoded output line through the
    /// filter exactly as the subprocess reader does. Returns the display
    /// line, whether it passes through to the log, every `ActionMessage`
    /// the line produced, and whether it reported `openjd_fail`.
    pub fn process_line(
        line: &str,
        filter: &mut ActionFilter,
        session_id: &str,
    ) -> (String, bool, Vec<crate::ActionMessage>, bool) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut saw_fail = false;
        let (display, pass_through) =
            crate::subprocess::process_line(line, filter, session_id, &tx, &mut saw_fail);
        drop(tx);
        let mut messages = Vec::new();
        while let Ok(m) = rx.try_recv() {
            messages.push(m);
        }
        (display, pass_through, messages, saw_fail)
    }
}

// Re-export path mapping from openjd-expr (mirrors Python where sessions re-exports from expr)
pub use openjd_expr::path_mapping;

pub use action::{ActionMessage, ActionResult, ActionState};
pub use action_status::ActionStatus;
pub use error::SessionError;
pub use limits::SessionLimits;
pub use logging::LogContent;
pub use openjd_expr::path_mapping::{PathFormat, PathMappingRule};
pub use runner::{CancelMethod, ScriptRunnerState};
pub use session::{
    EnvironmentIdentifier, Session, SessionCancelHandle, SessionConfig, SessionState,
};
#[cfg(windows)]
pub use session_user::BadCredentialsError;
#[cfg(unix)]
pub use session_user::PosixSessionUser;
pub use session_user::SessionUser;
#[cfg(windows)]
pub use session_user::WindowsSessionUser;
pub use subprocess::SubprocessResult;
pub use tempdir::StickyBitPolicy;
pub use tempdir::TempDir;
