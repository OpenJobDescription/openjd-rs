// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Consolidated integration-test binary.
//!
//! Each `tests/integration/test_*.rs` file is included as a module here so
//! cargo links one test executable for this crate instead of one per file.
//!
//! The Windows cross-user and permissions tests are `#![cfg(windows)]` gated
//! inside their files and are filtered in CI via substring match on the
//! module path (e.g. `cargo test --test integration cross_user_windows`).
//! The Unix cross-user tests are `#![cfg(unix)]` gated similarly.

#[path = "integration/test_cross_user.rs"]
mod test_cross_user;
#[path = "integration/test_cross_user_windows.rs"]
mod test_cross_user_windows;
#[path = "integration/test_embedded_files.rs"]
mod test_embedded_files;
#[path = "integration/test_error_messages.rs"]
mod test_error_messages;
#[path = "integration/test_helper.rs"]
mod test_helper;
#[path = "integration/test_path_mapping.rs"]
mod test_path_mapping;
#[path = "integration/test_path_mapping_materialize.rs"]
mod test_path_mapping_materialize;
#[path = "integration/test_session.rs"]
mod test_session;
#[path = "integration/test_session_env_step.rs"]
mod test_session_env_step;
#[path = "integration/test_session_scenarios.rs"]
mod test_session_scenarios;
#[path = "integration/test_tempdir_os.rs"]
mod test_tempdir_os;
#[path = "integration/test_windows_permissions.rs"]
mod test_windows_permissions;
#[path = "integration/test_wrap_actions.rs"]
mod test_wrap_actions;
