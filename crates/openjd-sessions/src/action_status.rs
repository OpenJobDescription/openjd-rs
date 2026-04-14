// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Action status tracking.

use crate::action::ActionState;
use std::time::SystemTime;

/// Status of the currently running or most recently completed action.
#[derive(Debug, Clone)]
pub struct ActionStatus {
    pub state: ActionState,
    pub progress: Option<f64>,
    pub status_message: Option<String>,
    pub fail_message: Option<String>,
    pub exit_code: Option<i32>,
    /// When the action started (subprocess launched or action began).
    pub started_at: Option<SystemTime>,
    /// When the action ended (subprocess exited or action completed).
    pub ended_at: Option<SystemTime>,
}
