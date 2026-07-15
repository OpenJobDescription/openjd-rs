// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Step script runner — handles onRun actions.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use openjd_expr::function_library::FunctionLibrary;
use openjd_model::job::StepScript;
use openjd_model::symbol_table::SymbolTable;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{ScriptRunnerBase, ScriptRunnerState};
use crate::action::ActionMessage;
use crate::embedded_files::{EmbeddedFiles, EmbeddedFilesScope};
use crate::error::SessionError;
use crate::let_bindings::evaluate_let_bindings;
use crate::session_user::SessionUser;
use crate::subprocess::SubprocessResult;

pub struct StepScriptRunner {
    base: ScriptRunnerBase,
}

impl StepScriptRunner {
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

    pub fn with_debug_collect_stdout(mut self, collect: bool) -> Self {
        self.base.debug_collect_stdout = collect;
        self
    }

    /// Whether to echo `openjd_*` directive lines (e.g. `openjd_progress`,
    /// `openjd_status`, `openjd_env`, …) to the log. Defaults to `true`.
    /// See [`crate::session::SessionConfig::echo_openjd_directives`].
    pub fn with_echo_openjd_directives(mut self, echo: bool) -> Self {
        self.base.echo_openjd_directives = echo;
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

    pub(crate) fn with_helpers_directory(mut self, dir: PathBuf) -> Self {
        self.base.helpers_directory = Some(dir);
        self
    }

    /// Run the step script's onRun action.
    pub async fn run(
        &mut self,
        script: &StepScript,
        symtab: &SymbolTable,
        library: Option<&FunctionLibrary>,
        env_vars: &HashMap<String, Option<String>>,
        message_tx: mpsc::UnboundedSender<ActionMessage>,
    ) -> Result<SubprocessResult, SessionError> {
        // Step scripts: allocate embedded file paths first — `filename` is a
        // plain string (never an expression) so allocation has no dependency
        // on `let` values, and it defines the `Task.File.*` symbols that
        // `let` bindings may reference. File *contents* are written after
        // `let` evaluation so `data` expressions can use let-bound values.
        // This matches the environment runner's ordering for `Env.File.*`.
        let mut final_symtab = symtab.clone();
        let ef = if let Some(files) = &script.embedded_files {
            let mut ef = EmbeddedFiles::new(
                EmbeddedFilesScope::Step,
                self.base.files_directory.clone(),
                &self.base.session_id,
            )
            .with_user(self.base.user.clone());
            ef.allocate_file_paths(files, &mut final_symtab)?;
            Some(ef)
        } else {
            None
        };

        if let Some(bindings) = &script.let_bindings {
            final_symtab = evaluate_let_bindings(
                bindings,
                &final_symtab,
                library,
                openjd_expr::PathFormat::host(),
            )
            .map_err(|e| SessionError::FormatString {
                context: "let bindings".into(),
                reason: e.to_string(),
            })?;
        }

        if let Some(ef) = ef {
            ef.write_file_contents(&final_symtab, library)?;
        }

        self.base
            .run_action(
                &script.actions.on_run,
                &final_symtab,
                library,
                env_vars,
                message_tx,
                None,
                Duration::from_secs(120),
            )
            .await
    }

    pub fn cancel(&self) {
        self.base.cancel_token.cancel();
    }

    pub fn state(&self) -> ScriptRunnerState {
        self.base.state
    }
}
