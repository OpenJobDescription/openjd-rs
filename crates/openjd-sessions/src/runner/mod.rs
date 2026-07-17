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
    /// Whether to echo `openjd_*` directive lines to the log. See
    /// [`crate::session::SessionConfig::echo_openjd_directives`]. Defaults
    /// to `true` to match the Python reference implementation.
    pub echo_openjd_directives: bool,
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
            echo_openjd_directives: true,
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
        let cancel_method =
            cancel_method_for_action(&action.cancelation, symtab, library, default_cancel_period)?;
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
        let mut filter = ActionFilter::new(
            &self.session_id,
            self.echo_openjd_directives,
            self.redactions_enabled,
        );
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

/// An action's `<Cancelation>` with every format string resolved — the
/// run-time counterpart of [`CancelationMode`].
///
/// # What is the problem this solves?
///
/// Format strings are normally delay-processed: the parser stores "this is
/// a format string" and the value resolves right before the action runs.
/// But a cancelation's `mode` is the *schema selector* — the parser needs
/// it to know which object shape it is reading — while a forwarded value
/// like `mode: "{{WrappedAction.Cancelation.Mode}}"` (RFC 0008 round-trip
/// forwarding) only exists at run time. The model therefore carries such a
/// mode as [`CancelationMode::DeferredMode`], and *this* type is where the
/// deferred decision finally lands: right before the action launches, when
/// the symbol table has the `WrappedAction.*` values, the mode expression
/// resolves to `"TERMINATE"`, `"NOTIFY_THEN_TERMINATE"`, or null — and a
/// null mode means the whole cancelation object is treated as never
/// declared (the runtime default applies).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EffectiveCancelation {
    /// No `<Cancelation>` declared, or a deferred mode that resolved to
    /// null. Canceled with the runtime default (terminate).
    Undeclared,
    Terminate,
    NotifyThenTerminate {
        /// `None` when the field was omitted or its whole-field expression
        /// resolved to null — the caller applies the positional schema
        /// default (120 for a task's onRun, 30 otherwise).
        notify_period_seconds: Option<i64>,
    },
}

/// Resolve an action's cancelation config against the symbol table,
/// deciding a deferred (format-string) mode and resolving any
/// notifyPeriodInSeconds format string to its numeric value.
pub(crate) fn resolve_effective_cancelation(
    cancelation: &Option<CancelationMode>,
    symtab: &SymbolTable,
    library: Option<&FunctionLibrary>,
) -> Result<EffectiveCancelation, SessionError> {
    match cancelation {
        None => Ok(EffectiveCancelation::Undeclared),
        Some(CancelationMode::Terminate) => Ok(EffectiveCancelation::Terminate),
        Some(CancelationMode::NotifyThenTerminate {
            notify_period_in_seconds,
        }) => Ok(EffectiveCancelation::NotifyThenTerminate {
            notify_period_seconds: resolve_notify_period_seconds(
                notify_period_in_seconds.as_ref(),
                symtab,
                library,
            )?,
        }),
        Some(CancelationMode::DeferredMode {
            mode,
            notify_period_in_seconds,
        }) => {
            let value = mode
                .resolve_with(
                    symtab,
                    &openjd_expr::FormatStringOptions::new().with_library(library),
                )
                .map_err(|e| SessionError::FormatString {
                    context: "cancelation mode".into(),
                    reason: e.to_string(),
                })?;
            match value {
                openjd_expr::ExprValue::Null => {
                    // Null mode drops the ENTIRE cancelation object: mode is
                    // the object's required discriminator, so an "omitted"
                    // mode cannot leave a partial object behind. The action
                    // behaves exactly as if no <Cancelation> were declared.
                    Ok(EffectiveCancelation::Undeclared)
                }
                openjd_expr::ExprValue::String(s) if s == "TERMINATE" => {
                    // Post-resolution the object must validate against the
                    // resolved variant's shape: TERMINATE admits no notify
                    // period, so a non-null period is an error.
                    let period = resolve_notify_period_seconds(
                        notify_period_in_seconds.as_ref(),
                        symtab,
                        library,
                    )?;
                    if period.is_some() {
                        return Err(SessionError::FormatString {
                            context: "cancelation mode".into(),
                            reason: "mode resolved to TERMINATE, which does not accept \
                                     notifyPeriodInSeconds"
                                .into(),
                        });
                    }
                    Ok(EffectiveCancelation::Terminate)
                }
                openjd_expr::ExprValue::String(s) if s == "NOTIFY_THEN_TERMINATE" => {
                    Ok(EffectiveCancelation::NotifyThenTerminate {
                        notify_period_seconds: resolve_notify_period_seconds(
                            notify_period_in_seconds.as_ref(),
                            symtab,
                            library,
                        )?,
                    })
                }
                other => Err(SessionError::FormatString {
                    context: "cancelation mode".into(),
                    reason: format!(
                        "must resolve to TERMINATE, NOTIFY_THEN_TERMINATE, or null; got {other:?}"
                    ),
                }),
            }
        }
    }
}

/// Determine the cancel method from an action's cancelation field,
/// resolving format strings (a deferred mode, or a FEATURE_BUNDLE_1
/// notifyPeriodInSeconds) against the symbol table.
pub(crate) fn cancel_method_for_action(
    cancelation: &Option<CancelationMode>,
    symtab: &SymbolTable,
    library: Option<&FunctionLibrary>,
    default_notify_period: Duration,
) -> Result<CancelMethod, SessionError> {
    Ok(
        match resolve_effective_cancelation(cancelation, symtab, library)? {
            EffectiveCancelation::Undeclared | EffectiveCancelation::Terminate => {
                CancelMethod::Terminate
            }
            EffectiveCancelation::NotifyThenTerminate {
                notify_period_seconds,
            } => CancelMethod::NotifyThenTerminate {
                terminate_delay: notify_period_seconds
                    .map(|n| Duration::from_secs(n as u64))
                    .unwrap_or(default_notify_period),
            },
        },
    )
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
            let value = fmt
                .resolve_with(
                    symtab,
                    &openjd_expr::FormatStringOptions::new().with_library(library),
                )
                .map_err(|e| SessionError::FormatString {
                    context: "timeout".into(),
                    reason: e.to_string(),
                })?;
            let secs: u64 = match value {
                // Whole-field expression resolved to null (e.g. forwarding
                // `timeout: "{{WrappedAction.Timeout}}"` when the wrapped
                // action specified no timeout): the field is treated as
                // not provided, so the positional default applies.
                openjd_expr::ExprValue::Null => return Ok(default),
                openjd_expr::ExprValue::Int(n) if n > 0 => n as u64,
                openjd_expr::ExprValue::String(ref s) => match s.parse::<u64>() {
                    Ok(n) if n > 0 => n,
                    _ => {
                        return Err(SessionError::FormatString {
                            context: "timeout".into(),
                            reason: format!("timeout must be a positive integer, got '{s}'"),
                        })
                    }
                },
                other => {
                    return Err(SessionError::FormatString {
                        context: "timeout".into(),
                        reason: format!("timeout must be a positive integer, got {other:?}"),
                    })
                }
            };
            Ok(Some(Duration::from_secs(secs)))
        }
        None => Ok(default),
    }
}

/// Resolve an optional `notifyPeriodInSeconds` FormatString into an
/// optional positive integer number of seconds.
///
/// Full FormatString resolution against the supplied symbol table. The
/// field is `int?` under whole-field expression semantics (Template
/// Schemas §5.3.2): `Ok(None)` means the field was omitted or its
/// whole-field expression resolved to null — the caller applies the
/// positional schema default. Zero and negative values are rejected,
/// matching the schema's `<posinteger>` typing.
///
/// Used by both the WRAP_ACTIONS seed path (populating
/// `WrappedAction.Cancelation.NotifyPeriodInSeconds` per RFC 0008) and the
/// enforcement path (`cancel_method_for_action`), so the value a wrap
/// script *sees* is always the value the runtime *enforces*.
pub(crate) fn resolve_notify_period_seconds(
    fs: Option<&openjd_model::FormatString>,
    symtab: &SymbolTable,
    library: Option<&FunctionLibrary>,
) -> Result<Option<i64>, SessionError> {
    let Some(fs) = fs else {
        return Ok(None);
    };
    let value = fs
        .resolve_with(
            symtab,
            &openjd_expr::FormatStringOptions::new().with_library(library),
        )
        .map_err(|e| SessionError::FormatString {
            context: "notifyPeriodInSeconds".into(),
            reason: e.to_string(),
        })?;
    let n: i64 = match value {
        // Whole-field expression resolved to null: the field is treated as
        // not provided (schema defaults apply).
        openjd_expr::ExprValue::Null => return Ok(None),
        openjd_expr::ExprValue::Int(n) => n,
        openjd_expr::ExprValue::String(s) => s.parse().map_err(|_| SessionError::FormatString {
            context: "notifyPeriodInSeconds".into(),
            reason: format!("notifyPeriodInSeconds must be a positive integer, got '{s}'"),
        })?,
        other => {
            return Err(SessionError::FormatString {
                context: "notifyPeriodInSeconds".into(),
                reason: format!("notifyPeriodInSeconds must be a positive integer, got {other:?}"),
            })
        }
    };
    if n <= 0 {
        return Err(SessionError::FormatString {
            context: "notifyPeriodInSeconds".into(),
            reason: format!("notifyPeriodInSeconds must be positive, got '{n}'"),
        });
    }
    // Mirror the static validator's cap on literal values (Template
    // Schemas §5.3.2 maximum: 600): format-string values could not be
    // checked at parse time, so the resolved value is bounds-checked here.
    if n > 600 {
        return Err(SessionError::FormatString {
            context: "notifyPeriodInSeconds".into(),
            reason: format!("notifyPeriodInSeconds must not exceed 600, got '{n}'"),
        });
    }
    Ok(Some(n))
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

    fn fs(s: &str) -> openjd_model::FormatString {
        openjd_model::FormatString::new(s).unwrap()
    }

    #[test]
    fn notify_period_resolves_literal_and_bounds() {
        let symtab = SymbolTable::default();
        // In-range values resolve.
        assert_eq!(
            resolve_notify_period_seconds(Some(&fs("45")), &symtab, None).unwrap(),
            Some(45)
        );
        // Omitted field is None.
        assert_eq!(
            resolve_notify_period_seconds(None, &symtab, None).unwrap(),
            None
        );
        // The Template Schemas §5.3.2 cap applies to resolved values just
        // as the static validator applies it to literals: format-string
        // values could not be checked at parse time.
        let err = resolve_notify_period_seconds(Some(&fs("9999")), &symtab, None).unwrap_err();
        assert!(
            err.to_string().contains("must not exceed 600"),
            "expected cap error; got: {err}"
        );
        // Non-positive values are rejected.
        assert!(resolve_notify_period_seconds(Some(&fs("0")), &symtab, None).is_err());
    }

    #[test]
    fn notify_period_whole_field_null_is_none() {
        let mut symtab = SymbolTable::default();
        symtab
            .set("X", openjd_expr::ExprValue::Null)
            .expect("symtab");
        assert_eq!(
            resolve_notify_period_seconds(Some(&fs("{{X}}")), &symtab, None).unwrap(),
            None
        );
    }
}
