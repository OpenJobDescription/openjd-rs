// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Script runners for environment and step actions.

pub mod env_script;
pub mod step_script;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use openjd_expr::function_library::FunctionLibrary;
use openjd_expr::ExprValue;
use openjd_model::job::Action;
use openjd_model::job::CancelationMode;
use openjd_model::symbol_table::SymbolTable;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::action::ActionMessage;
use crate::action::ActionState;
use crate::action_filter::ActionFilter;
use crate::error::SessionError;
use crate::logging::log_subsection_banner;
use crate::session_user::SessionUser;
use crate::subprocess::{run_subprocess, SubprocessConfig, SubprocessResult};

/// Method for canceling a running action.
///
/// ```
/// use openjd_sessions::CancelMethod;
/// use std::time::Duration;
///
/// let method = CancelMethod::NotifyThenTerminate {
///     terminate_delay: Duration::from_secs(30),
/// };
/// assert!(matches!(method, CancelMethod::NotifyThenTerminate { .. }));
/// ```
#[derive(Debug, Clone)]
pub enum CancelMethod {
    /// Immediately terminate via SIGKILL.
    Terminate,
    /// Send SIGTERM, wait for grace period, then SIGKILL.
    NotifyThenTerminate { terminate_delay: Duration },
}

impl std::fmt::Display for CancelMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Terminate => write!(f, "Terminate"),
            Self::NotifyThenTerminate { terminate_delay } => {
                write!(f, "NotifyThenTerminate({}s)", terminate_delay.as_secs())
            }
        }
    }
}

/// State of a script runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptRunnerState {
    Ready,
    Running,
    Canceling,
    Canceled,
    Timeout,
    Failed,
    Success,
}

impl std::fmt::Display for ScriptRunnerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready => write!(f, "Ready"),
            Self::Running => write!(f, "Running"),
            Self::Canceling => write!(f, "Canceling"),
            Self::Canceled => write!(f, "Canceled"),
            Self::Timeout => write!(f, "Timeout"),
            Self::Failed => write!(f, "Failed"),
            Self::Success => write!(f, "Success"),
        }
    }
}

/// Shared infrastructure for script runners.
///
/// Both `EnvironmentScriptRunner` and `StepScriptRunner` compose this struct
/// to avoid duplicating constructor, builder, cancel/state, and subprocess
/// execution logic.
pub(crate) struct ScriptRunnerBase {
    pub state: ScriptRunnerState,
    pub cancel_token: CancellationToken,
    pub cancel_request_rx: Option<tokio::sync::watch::Receiver<Option<Duration>>>,
    pub session_id: String,
    pub working_directory: PathBuf,
    pub files_directory: PathBuf,
    pub helpers_directory: Option<PathBuf>,
    pub user: Option<Arc<dyn SessionUser>>,
    pub redactions_enabled: bool,
    pub initial_redacted_values: Vec<String>,
    pub debug_collect_stdout: bool,
    #[cfg(unix)]
    pub helper: Option<crate::cross_user_helper::CrossUserHelper>,
    #[cfg(windows)]
    pub helper: Option<crate::cross_user_helper::CrossUserHelperWin>,
    pub cancel_writer: Option<std::fs::File>,
}

impl ScriptRunnerBase {
    pub fn new(
        session_id: &str,
        working_directory: PathBuf,
        files_directory: PathBuf,
        user: Option<Arc<dyn SessionUser>>,
    ) -> Self {
        Self {
            state: ScriptRunnerState::Ready,
            cancel_token: CancellationToken::new(),
            cancel_request_rx: None,
            session_id: session_id.to_string(),
            working_directory,
            files_directory,
            helpers_directory: None,
            user,
            redactions_enabled: false,
            initial_redacted_values: Vec::new(),
            debug_collect_stdout: false,
            helper: None,
            cancel_writer: None,
        }
    }

    /// Run a resolved action as a subprocess, updating runner state.
    #[allow(clippy::too_many_arguments)]
    pub async fn run_action(
        &mut self,
        action: &Action,
        symtab: &SymbolTable,
        library: Option<&FunctionLibrary>,
        env_vars: &HashMap<String, Option<String>>,
        message_tx: mpsc::UnboundedSender<ActionMessage>,
        default_timeout: Option<Duration>,
        default_cancel_period: Duration,
    ) -> Result<SubprocessResult, SessionError> {
        self.state = ScriptRunnerState::Running;
        log_subsection_banner(&self.session_id, "Phase: Running action");
        let args = resolve_action_args(action, symtab, library)?;
        let timeout = resolve_action_timeout(action, symtab, library, default_timeout)?;
        let cancel_method = cancel_method_for_action(&action.cancelation, default_cancel_period);
        let config = SubprocessConfig {
            args,
            env_vars: env_vars.clone(),
            working_dir: Some(self.working_directory.clone()),
            timeout,
            user: self.user.clone(),
            cancel_method,
            cancel_request_rx: self.cancel_request_rx.clone(),
            debug_collect_stdout: self.debug_collect_stdout,
        };
        let mut filter = ActionFilter::new(&self.session_id, true, self.redactions_enabled);
        filter.add_redacted_values(&self.initial_redacted_values);

        if let Some(ref mut helper) = self.helper {
            let result = crate::cross_user_helper::run_via_helper(
                helper,
                &config,
                &mut filter,
                &self.session_id,
                message_tx,
                self.cancel_writer.as_ref(),
            )
            .await?;
            self.state = state_from_action(result.state);
            return Ok(result);
        }

        let result = run_subprocess(
            config,
            &mut filter,
            &self.session_id,
            message_tx,
            self.cancel_token.clone(),
        )
        .await?;

        self.state = state_from_action(result.state);
        Ok(result)
    }
}

fn state_from_action(action_state: ActionState) -> ScriptRunnerState {
    match action_state {
        ActionState::Success => ScriptRunnerState::Success,
        ActionState::Canceled => ScriptRunnerState::Canceled,
        ActionState::Timeout => ScriptRunnerState::Timeout,
        _ => ScriptRunnerState::Failed,
    }
}

/// Determine the cancel method from an action's cancelation field.
pub(crate) fn cancel_method_for_action(
    cancelation: &Option<CancelationMode>,
    default_notify_period: Duration,
) -> CancelMethod {
    match cancelation {
        None | Some(CancelationMode::Terminate) => CancelMethod::Terminate,
        Some(CancelationMode::NotifyThenTerminate {
            notify_period_in_seconds,
        }) => {
            let period = notify_period_in_seconds
                .as_ref()
                .and_then(|fs| fs.raw().parse::<u64>().ok())
                .map(Duration::from_secs)
                .unwrap_or(default_notify_period);
            CancelMethod::NotifyThenTerminate {
                terminate_delay: period,
            }
        }
    }
}

/// Resolve an Action's timeout field to a Duration, falling back to a default.
pub(crate) fn resolve_action_timeout(
    action: &Action,
    symtab: &SymbolTable,
    library: Option<&FunctionLibrary>,
    default: Option<Duration>,
) -> Result<Option<Duration>, SessionError> {
    match &action.timeout {
        Some(fmt) => {
            let s = fmt
                .resolve_string_with(
                    symtab,
                    &openjd_expr::FormatStringOptions::new().with_library(library),
                )
                .map_err(|e| SessionError::FormatString {
                    context: "timeout".into(),
                    reason: e.to_string(),
                })?;
            let secs: u64 = s.parse().map_err(|_| SessionError::FormatString {
                context: "timeout".into(),
                reason: format!("timeout must be a positive integer, got '{s}'"),
            })?;
            if secs == 0 {
                return Err(SessionError::FormatString {
                    context: "timeout".into(),
                    reason: "timeout must be a positive integer, got '0'".into(),
                });
            }
            Ok(Some(Duration::from_secs(secs)))
        }
        None => Ok(default),
    }
}

/// Resolve an Action's command and args into a flat argument list.
pub(crate) fn resolve_action_args(
    action: &Action,
    symtab: &SymbolTable,
    library: Option<&FunctionLibrary>,
) -> Result<Vec<String>, SessionError> {
    let command = action
        .command
        .resolve_string_with(
            symtab,
            &openjd_expr::FormatStringOptions::new().with_library(library),
        )
        .map_err(|e| SessionError::FormatString {
            context: "command".into(),
            reason: e.to_string(),
        })?;
    let mut args = vec![command];
    if let Some(arg_fmts) = &action.args {
        for fs in arg_fmts {
            if let Ok(val) = fs.resolve_with(
                symtab,
                &openjd_expr::FormatStringOptions::new().with_library(library),
            ) {
                match val {
                    ExprValue::Null => continue,
                    val if val.is_list() => {
                        if let Some(elements) = val.list_elements() {
                            for elem in &elements {
                                args.push(elem.to_display_string());
                            }
                        }
                        continue;
                    }
                    val => args.push(val.to_display_string()),
                }
            } else {
                let s = fs
                    .resolve_string_with(
                        symtab,
                        &openjd_expr::FormatStringOptions::new().with_library(library),
                    )
                    .map_err(|e| SessionError::FormatString {
                        context: "argument".into(),
                        reason: e.to_string(),
                    })?;
                args.push(s);
            }
        }
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_runner_state_display() {
        assert_eq!(ScriptRunnerState::Ready.to_string(), "Ready");
        assert_eq!(ScriptRunnerState::Running.to_string(), "Running");
        assert_eq!(ScriptRunnerState::Canceling.to_string(), "Canceling");
        assert_eq!(ScriptRunnerState::Canceled.to_string(), "Canceled");
        assert_eq!(ScriptRunnerState::Timeout.to_string(), "Timeout");
        assert_eq!(ScriptRunnerState::Failed.to_string(), "Failed");
        assert_eq!(ScriptRunnerState::Success.to_string(), "Success");
    }

    #[test]
    fn cancel_method_display() {
        assert_eq!(CancelMethod::Terminate.to_string(), "Terminate");
        assert_eq!(
            CancelMethod::NotifyThenTerminate {
                terminate_delay: Duration::from_secs(30)
            }
            .to_string(),
            "NotifyThenTerminate(30s)"
        );
    }
}
