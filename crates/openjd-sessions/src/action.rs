// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Action state and result types.

/// State of an action execution.
///
/// ```
/// use openjd_sessions::ActionState;
///
/// let state = ActionState::Success;
/// assert_eq!(state, ActionState::Success);
/// assert_ne!(state, ActionState::Failed);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionState {
    Running,
    Success,
    Failed,
    Canceled,
    Timeout,
}

impl std::fmt::Display for ActionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running => write!(f, "Running"),
            Self::Success => write!(f, "Success"),
            Self::Failed => write!(f, "Failed"),
            Self::Canceled => write!(f, "Canceled"),
            Self::Timeout => write!(f, "Timeout"),
        }
    }
}

/// A parsed openjd stdout message from a running action.
#[derive(Debug, Clone)]
pub enum ActionMessage {
    /// `openjd_progress: <number>`
    Progress(f64),
    /// `openjd_status: <message>`
    Status(String),
    /// `openjd_fail: <message>`
    Fail(String),
    /// `openjd_env: <var>=<value>`
    SetEnv { name: String, value: String },
    /// `openjd_unset_env: <var>`
    UnsetEnv { name: String },
    /// `openjd_redacted_env: <var>=<value>`
    RedactedEnv { name: String, value: String },
    /// Request to cancel the action and mark it as failed (from malformed env commands)
    CancelMarkFailed { fail_message: String },
}

impl std::fmt::Display for ActionMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Progress(v) => write!(f, "Progress({v})"),
            Self::Status(s) => write!(f, "Status({s})"),
            Self::Fail(s) => write!(f, "Fail({s})"),
            Self::SetEnv { name, .. } => write!(f, "SetEnv({name})"),
            Self::UnsetEnv { name } => write!(f, "UnsetEnv({name})"),
            Self::RedactedEnv { name, .. } => write!(f, "RedactedEnv({name})"),
            Self::CancelMarkFailed { fail_message } => {
                write!(f, "CancelMarkFailed({fail_message})")
            }
        }
    }
}

/// Result of running an action.
#[derive(Debug)]
pub struct ActionResult {
    pub state: ActionState,
    pub exit_code: Option<i32>,
    pub stdout: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_state_display() {
        assert_eq!(ActionState::Running.to_string(), "Running");
        assert_eq!(ActionState::Success.to_string(), "Success");
        assert_eq!(ActionState::Failed.to_string(), "Failed");
        assert_eq!(ActionState::Canceled.to_string(), "Canceled");
        assert_eq!(ActionState::Timeout.to_string(), "Timeout");
    }

    #[test]
    fn action_message_display() {
        assert_eq!(ActionMessage::Progress(50.0).to_string(), "Progress(50)");
        assert_eq!(ActionMessage::Status("ok".into()).to_string(), "Status(ok)");
        assert_eq!(ActionMessage::Fail("err".into()).to_string(), "Fail(err)");
        assert_eq!(
            ActionMessage::SetEnv {
                name: "K".into(),
                value: "V".into()
            }
            .to_string(),
            "SetEnv(K)"
        );
        assert_eq!(
            ActionMessage::UnsetEnv { name: "K".into() }.to_string(),
            "UnsetEnv(K)"
        );
        assert_eq!(
            ActionMessage::RedactedEnv {
                name: "K".into(),
                value: "V".into()
            }
            .to_string(),
            "RedactedEnv(K)"
        );
        assert_eq!(
            ActionMessage::CancelMarkFailed {
                fail_message: "bad".into()
            }
            .to_string(),
            "CancelMarkFailed(bad)"
        );
    }
}
