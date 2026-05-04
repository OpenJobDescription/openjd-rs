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
#[cfg(windows)]
pub(crate) mod win32_permissions;

// Re-export path mapping from openjd-expr (mirrors Python where sessions re-exports from expr)
pub use openjd_expr::path_mapping;

pub use action::{ActionMessage, ActionResult, ActionState};
pub use action_status::ActionStatus;
pub use error::SessionError;
pub use logging::LogContent;
pub use openjd_expr::path_mapping::{PathFormat, PathMappingRule};
pub use runner::{CancelMethod, ScriptRunnerState};
pub use session::{EnvironmentIdentifier, Session, SessionConfig, SessionState};
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
