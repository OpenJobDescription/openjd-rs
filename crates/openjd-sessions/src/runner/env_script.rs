// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Environment script runner — handles enter/exit actions.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use openjd_expr::function_library::FunctionLibrary;
use openjd_model::job::{Action, Environment};
use openjd_model::symbol_table::SymbolTable;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{ScriptRunnerBase, ScriptRunnerState};
use crate::action::ActionMessage;
use crate::action::ActionState;
use crate::embedded_files::{EmbeddedFiles, EmbeddedFilesScope};
use crate::error::SessionError;
use crate::let_bindings::evaluate_let_bindings;
use crate::session_user::SessionUser;
use crate::subprocess::SubprocessResult;

/// Default timeout for environment exit actions (5 minutes), matching Python's _ENV_EXIT_DEFAULT_TIMEOUT.
const ENV_EXIT_DEFAULT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

pub struct EnvironmentScriptRunner {
    base: ScriptRunnerBase,
}

impl EnvironmentScriptRunner {
    pub fn new(
        session_id: &str,
        working_directory: PathBuf,
        files_directory: PathBuf,
        user: Option<Arc<dyn SessionUser>>,
    ) -> Self {
        Self {
            base: ScriptRunnerBase::new(session_id, working_directory, files_directory, user),
        }
    }

    pub fn with_redactions(mut self, enabled: bool) -> Self {
        self.base.redactions_enabled = enabled;
        self
    }

    pub fn with_collect_stdout(mut self, collect: bool) -> Self {
        self.base.collect_stdout = collect;
        self
    }

    pub fn with_initial_redacted_values(mut self, values: Vec<String>) -> Self {
        self.base.initial_redacted_values = values;
        self
    }

    pub fn with_cancel_token(mut self, token: CancellationToken) -> Self {
        self.base.cancel_token = token;
        self
    }

    pub fn with_cancel_request_rx(
        mut self,
        rx: tokio::sync::watch::Receiver<Option<Duration>>,
    ) -> Self {
        self.base.cancel_request_rx = Some(rx);
        self
    }

    #[cfg(unix)]
    pub(crate) fn with_helper(mut self, helper: crate::cross_user_helper::CrossUserHelper) -> Self {
        self.base.helper = Some(helper);
        self
    }

    #[cfg(unix)]
    pub(crate) fn take_helper(&mut self) -> Option<crate::cross_user_helper::CrossUserHelper> {
        self.base.helper.take()
    }

    #[cfg(windows)]
    pub(crate) fn with_helper(
        mut self,
        helper: crate::cross_user_helper::CrossUserHelperWin,
    ) -> Self {
        self.base.helper = Some(helper);
        self
    }

    #[cfg(windows)]
    pub(crate) fn take_helper(&mut self) -> Option<crate::cross_user_helper::CrossUserHelperWin> {
        self.base.helper.take()
    }

    pub(crate) fn with_cancel_writer(mut self, writer: std::fs::File) -> Self {
        self.base.cancel_writer = Some(writer);
        self
    }

    /// Run the environment's onEnter action.
    pub async fn enter(
        &mut self,
        env: &Environment,
        symtab: &SymbolTable,
        library: Option<&FunctionLibrary>,
        env_vars: &HashMap<String, Option<String>>,
        message_tx: mpsc::UnboundedSender<ActionMessage>,
    ) -> Result<SubprocessResult, SessionError> {
        let action = env
            .script
            .as_ref()
            .and_then(|s| s.actions.on_enter.as_ref());
        self.run_env_action(env, action, symtab, library, env_vars, message_tx, None)
            .await
    }

    /// Run the environment's onExit action.
    pub async fn exit(
        &mut self,
        env: &Environment,
        symtab: &SymbolTable,
        library: Option<&FunctionLibrary>,
        env_vars: &HashMap<String, Option<String>>,
        message_tx: mpsc::UnboundedSender<ActionMessage>,
    ) -> Result<SubprocessResult, SessionError> {
        let action = env.script.as_ref().and_then(|s| s.actions.on_exit.as_ref());
        self.run_env_action(
            env,
            action,
            symtab,
            library,
            env_vars,
            message_tx,
            Some(ENV_EXIT_DEFAULT_TIMEOUT),
        )
        .await
    }

    pub fn cancel(&self) {
        self.base.cancel_token.cancel();
    }

    pub fn state(&self) -> ScriptRunnerState {
        self.base.state
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_env_action(
        &mut self,
        env: &Environment,
        action: Option<&Action>,
        symtab: &SymbolTable,
        library: Option<&FunctionLibrary>,
        env_vars: &HashMap<String, Option<String>>,
        message_tx: mpsc::UnboundedSender<ActionMessage>,
        default_timeout: Option<Duration>,
    ) -> Result<SubprocessResult, SessionError> {
        let action = match action {
            Some(a) => a,
            None => {
                self.base.state = ScriptRunnerState::Success;
                return Ok(SubprocessResult {
                    state: ActionState::Success,
                    exit_code: None,
                    stdout: String::new(),
                });
            }
        };

        let let_bindings = env.script.as_ref().and_then(|s| s.let_bindings.as_ref());
        let embedded_files = env.script.as_ref().and_then(|s| s.embedded_files.as_ref());

        let final_symtab = match (let_bindings, embedded_files) {
            (Some(bindings), Some(files)) => {
                let mut st = symtab.clone();
                let mut ef = EmbeddedFiles::new(
                    EmbeddedFilesScope::Env,
                    self.base.files_directory.clone(),
                    &self.base.session_id,
                )
                .with_user(self.base.user.clone());
                ef.allocate_file_paths(files, &mut st)?;
                let st =
                    evaluate_let_bindings(bindings, &st, library, openjd_expr::PathFormat::host())
                        .map_err(|e| SessionError::FormatString {
                            context: "let bindings".into(),
                            reason: e.to_string(),
                        })?;
                ef.write_file_contents(&st, library)?;
                st
            }
            (Some(bindings), None) => {
                evaluate_let_bindings(bindings, symtab, library, openjd_expr::PathFormat::host())
                    .map_err(|e| SessionError::FormatString {
                        context: "let bindings".into(),
                        reason: e.to_string(),
                    })?
            }
            (None, Some(files)) => {
                let mut st = symtab.clone();
                let mut ef = EmbeddedFiles::new(
                    EmbeddedFilesScope::Env,
                    self.base.files_directory.clone(),
                    &self.base.session_id,
                )
                .with_user(self.base.user.clone());
                ef.allocate_file_paths(files, &mut st)?;
                ef.write_file_contents(&st, library)?;
                st
            }
            (None, None) => symtab.clone(),
        };

        self.base
            .run_action(
                action,
                &final_symtab,
                library,
                env_vars,
                message_tx,
                default_timeout,
                Duration::from_secs(30),
            )
            .await
    }
}
