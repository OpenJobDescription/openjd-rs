// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Session management — core state machine.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use openjd_expr::function_library::FunctionLibrary;
use openjd_expr::path_mapping::PathMappingRule;
use openjd_model::job::{Environment, StepScript};
use openjd_model::symbol_table::SymbolTable;
use openjd_model::types::JobParameterValues;
use tokio_util::sync::CancellationToken;

use crate::action::{ActionMessage, ActionResult, ActionState};
use crate::action_status::ActionStatus;
use crate::cross_user_helper::run_via_helper;

/// Default notify period (in seconds) for `NOTIFY_THEN_TERMINATE` cancel when
/// no explicit time_limit is provided. Matches the OpenJD spec default.
pub const DEFAULT_CANCEL_NOTIFY_PERIOD_SECS: u64 = 5;
#[cfg(unix)]
use crate::cross_user_helper::CrossUserHelper;
#[cfg(windows)]
use crate::cross_user_helper::CrossUserHelperWin;
use crate::error::SessionError;
use crate::logging::{log_section_banner, LogContent};
use crate::runner::env_script::EnvironmentScriptRunner;
use crate::runner::step_script::StepScriptRunner;
use crate::session_log;
use crate::session_user::SessionUser;

/// Session lifecycle state.
///
/// ```
/// use openjd_sessions::SessionState;
///
/// let state = SessionState::Ready;
/// assert_eq!(format!("{state}"), "READY");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Ready,
    Running,
    Canceling,
    ReadyEnding,
    Ended,
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionState::Ready => write!(f, "READY"),
            SessionState::Running => write!(f, "RUNNING"),
            SessionState::Canceling => write!(f, "CANCELING"),
            SessionState::ReadyEnding => write!(f, "READY_ENDING"),
            SessionState::Ended => write!(f, "ENDED"),
        }
    }
}

/// Identifier for an environment within a session.
pub type EnvironmentIdentifier = String;

/// Callback invoked when action status changes.
pub type SessionCallbackType = Box<dyn Fn(&str, &ActionStatus) + Send + Sync>;

/// Configuration for creating a new Session.
pub struct SessionConfig {
    pub session_id: String,
    pub job_parameter_values: JobParameterValues,
    pub path_mapping_rules: Option<Vec<PathMappingRule>>,
    pub retain_working_dir: bool,
    pub callback: Option<SessionCallbackType>,
    pub os_env_vars: Option<HashMap<String, String>>,
    pub session_root_directory: Option<PathBuf>,
    pub user: Option<Arc<dyn SessionUser>>,
    pub revision_extensions: Option<openjd_model::types::ValidationContext>,
    /// Optional external cancellation token. When cancelled, all running and
    /// future actions will be cancelled via the spec's cancellation sequence.
    pub cancel_token: Option<CancellationToken>,
    /// Whether to accumulate subprocess stdout into result strings.
    /// Intended for debugging only — production callers should leave this
    /// `false` and observe output through the real-time callback instead.
    /// Default is `false` — output is still streamed through the callback in
    /// real time, but `ActionResult.stdout` and similar fields stay empty.
    pub debug_collect_stdout: bool,
}

fn format_exit_code(code: Option<i32>) -> String {
    match code {
        Some(c) => format!("exit code: {c}"),
        None => "exit code: N/A".to_string(),
    }
}

/// Normalize an environment variable name for the current platform.
/// On Windows, env vars are case-insensitive, so we uppercase all keys
/// to avoid undefined behavior from mixed-case duplicates in the Win32 API.
fn normalize_env_key(name: &str) -> String {
    #[cfg(windows)]
    {
        name.to_uppercase()
    }
    #[cfg(not(windows))]
    {
        name.to_string()
    }
}

/// Tracks env var changes made during an environment's lifecycle.
/// Uses a HashMap for automatic deduplication — last write wins,
/// matching Python's SimplifiedEnvironmentVariableChanges dict.
/// `None` value means "unset this variable".
type EnvVarChanges = HashMap<String, Option<String>>;

/// Tracks the status of the currently running (or most recently completed) action.
struct ActionStatusFields {
    state: Option<ActionState>,
    progress: Option<f64>,
    status_message: Option<String>,
    fail_message: Option<String>,
    exit_code: Option<i32>,
    started_at: Option<std::time::SystemTime>,
    ended_at: Option<std::time::SystemTime>,
}

impl ActionStatusFields {
    fn new() -> Self {
        Self {
            state: None,
            progress: None,
            status_message: None,
            fail_message: None,
            exit_code: None,
            started_at: None,
            ended_at: None,
        }
    }

    /// Reset all fields for a new action.
    fn reset(&mut self) {
        self.state = Some(ActionState::Running);
        self.started_at = Some(std::time::SystemTime::now());
        self.ended_at = None;
        self.progress = None;
        self.status_message = None;
        self.fail_message = None;
        self.exit_code = None;
    }
}

/// Cancellation state for the current action and external cancellation support.
struct CancelFields {
    /// Token for the current action (cancelled to abort the subprocess).
    token: Option<CancellationToken>,
    /// Channel to send cancel requests (with optional time limit) to the subprocess.
    request_tx: Option<tokio::sync::watch::Sender<Option<Duration>>>,
    /// When true, a Canceled action result is reported as Failed.
    mark_failed: bool,
    /// External cancellation token from the caller; action tokens are children of this.
    parent_token: Option<CancellationToken>,
}

impl CancelFields {
    fn new(parent_token: Option<CancellationToken>) -> Self {
        Self {
            token: None,
            request_tx: None,
            mark_failed: false,
            parent_token,
        }
    }
}

/// Cross-user execution state.
struct CrossUserFields {
    user: Option<Arc<dyn SessionUser>>,
    #[cfg(unix)]
    helper: Option<CrossUserHelper>,
    #[cfg(windows)]
    helper: Option<CrossUserHelperWin>,
    cancel_writer: Option<std::fs::File>,
}

/// Build the derived function library: `base` (or default) merged with a
/// host-context registration that captures the current path-mapping `rules`.
///
/// Called whenever the user-supplied library or the rules change so that
/// `apply_path_mapping` always reflects the session's current rules.
fn derive_library(
    base: Option<&FunctionLibrary>,
    rules: &Arc<Vec<PathMappingRule>>,
) -> Arc<FunctionLibrary> {
    let lib = match base {
        Some(b) => b.clone(),
        None => openjd_expr::default_library::get_default_library().clone(),
    };
    Arc::new(lib.with_host_context(rules.clone()))
}

pub struct Session {
    session_id: String,
    state: SessionState,
    ending_only: bool,
    working_directory: PathBuf,
    files_directory: PathBuf,
    retain_working_dir: bool,
    cleanup_called: bool,
    // TempDir ownership (only when created via with_config)
    _working_dir: Option<crate::tempdir::TempDir>,
    _files_dir: Option<crate::tempdir::TempDir>,
    // Environment tracking
    environments: HashMap<EnvironmentIdentifier, Environment>,
    environments_entered: Vec<EnvironmentIdentifier>,
    // Env var tracking
    env_vars: HashMap<String, String>,
    process_env: HashMap<String, String>,
    created_env_vars: HashMap<EnvironmentIdentifier, EnvVarChanges>,
    // Expression evaluation
    //
    // `library_base` is the user-supplied function library (without host
    // context). `library` is the cached derived library that has
    // `apply_path_mapping` registered for the current `path_mapping_rules`.
    // Whenever `path_mapping_rules` change, `library` must be rebuilt.
    library_base: Option<Arc<FunctionLibrary>>,
    library: Arc<FunctionLibrary>,
    path_mapping_rules: Arc<Vec<PathMappingRule>>,
    job_parameter_values: JobParameterValues,
    // Grouped fields
    action: ActionStatusFields,
    cancel: CancelFields,
    cross_user: CrossUserFields,
    // Callback
    callback: Option<SessionCallbackType>,
    // Redaction
    redacted_values: HashSet<String>,
    revision_extensions: Option<openjd_model::types::ValidationContext>,
    debug_collect_stdout: bool,
}

impl Session {
    /// Test-only constructor that accepts a pre-existing working directory.
    /// Production code should use `Session::with_config`.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn new_for_test(working_directory: PathBuf) -> Self {
        let files_directory = working_directory.join("embedded_files");
        Self {
            session_id: String::new(),
            state: SessionState::Ready,
            ending_only: false,
            working_directory,
            files_directory,
            retain_working_dir: false,
            cleanup_called: false,
            _working_dir: None,
            _files_dir: None,
            environments: HashMap::new(),
            environments_entered: Vec::new(),
            env_vars: HashMap::new(),
            process_env: HashMap::new(),
            created_env_vars: HashMap::new(),
            library_base: None,
            library: derive_library(None, &Arc::new(Vec::new())),
            path_mapping_rules: Arc::new(Vec::new()),
            job_parameter_values: HashMap::new(),
            action: ActionStatusFields::new(),
            cancel: CancelFields::new(None),
            cross_user: CrossUserFields {
                user: None,
                helper: None,
                cancel_writer: None,
            },
            callback: None,
            redacted_values: HashSet::new(),
            revision_extensions: None,
            debug_collect_stdout: true, // test constructor — tests need captured stdout
        }
    }

    /// Test-only: inject a cancel_writer (e.g. the write end of a pipe) so a test
    /// can observe what `cancel_action` writes.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn set_cancel_writer_for_test(&mut self, writer: std::fs::File) {
        self.cross_user.cancel_writer = Some(writer);
    }

    /// Test-only: force the session state (typically to `Running`) so `cancel_action`
    /// will proceed without needing a full action run.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn set_state_for_test(&mut self, state: SessionState) {
        self.state = state;
    }

    /// Full constructor from SessionConfig.
    pub fn with_config(config: SessionConfig) -> Result<Self, SessionError> {
        let root_dir = match &config.session_root_directory {
            Some(d) => d.clone(),
            None => crate::tempdir::openjd_temp_dir()?,
        };

        #[cfg(unix)]
        crate::tempdir::validate_sticky_bit(&root_dir);

        let working_dir = crate::tempdir::TempDir::new(
            Some(&root_dir),
            Some(&config.session_id),
            config.user.as_deref(),
        )?;
        let files_dir = crate::tempdir::TempDir::new(
            Some(working_dir.path()),
            Some("embedded_files"),
            config.user.as_deref(),
        )?;

        let working_directory = working_dir.path().to_path_buf();
        let files_directory = files_dir.path().to_path_buf();

        let mut path_mapping_rules = config.path_mapping_rules.unwrap_or_default();
        path_mapping_rules.sort_by_key(|r| std::cmp::Reverse(r.source_path.len()));
        let path_mapping_rules = Arc::new(path_mapping_rules);
        let library = derive_library(None, &path_mapping_rules);
        let process_env = config.os_env_vars.unwrap_or_default();

        // Spawn cross-user helper if needed
        #[cfg(unix)]
        let (helper, cancel_writer) = if let Some(ref user) = config.user {
            if !user.is_process_user() {
                let helper_path =
                    crate::helper_binary::write_helper(&working_directory, user.as_ref())?;
                let (h, cw) = CrossUserHelper::spawn(&helper_path, user.as_ref())?;
                (Some(h), Some(cw))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        #[cfg(windows)]
        let (helper, cancel_writer) = if let Some(ref user) = config.user {
            if !user.is_process_user() {
                let helper_path =
                    crate::helper_binary::write_helper(&working_directory, user.as_ref())?;
                let (h, cw) = CrossUserHelperWin::spawn(&helper_path, user.as_ref())?;
                (Some(h), Some(cw))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        // Log host info (mirrors Python Session.__init__)
        session_log!(
            info,
            &config.session_id,
            LogContent::HOST_INFO,
            "openjd-sessions Library Version: {}",
            env!("CARGO_PKG_VERSION")
        );
        session_log!(
            info,
            &config.session_id,
            LogContent::HOST_INFO,
            "Platform: {}",
            std::env::consts::OS
        );
        session_log!(
            info,
            &config.session_id,
            LogContent::HOST_INFO,
            "Architecture: {}",
            std::env::consts::ARCH
        );
        log::info!(target: "openjd.sessions", session_id = config.session_id.as_str(); "Initializing Open Job Description Session: {}", &config.session_id);
        session_log!(
            info,
            &config.session_id,
            LogContent::FILE_PATH,
            "Session Working Directory: {}",
            working_directory.display()
        );
        session_log!(
            info,
            &config.session_id,
            LogContent::FILE_PATH,
            "Session's Embedded Files Directory: {}",
            files_directory.display()
        );

        Ok(Self {
            session_id: config.session_id,
            state: SessionState::Ready,
            ending_only: false,
            working_directory,
            files_directory,
            retain_working_dir: config.retain_working_dir,
            cleanup_called: false,
            _working_dir: Some(working_dir),
            _files_dir: Some(files_dir),
            environments: HashMap::new(),
            environments_entered: Vec::new(),
            env_vars: HashMap::new(),
            process_env,
            created_env_vars: HashMap::new(),
            library_base: None,
            library,
            path_mapping_rules,
            job_parameter_values: config.job_parameter_values,
            action: ActionStatusFields::new(),
            cancel: CancelFields::new(config.cancel_token),
            cross_user: CrossUserFields {
                user: config.user,
                helper,
                cancel_writer,
            },
            callback: config.callback,
            redacted_values: HashSet::new(),
            revision_extensions: config.revision_extensions,
            debug_collect_stdout: config.debug_collect_stdout,
        })
    }

    pub fn with_path_mapping(mut self, mut rules: Vec<PathMappingRule>) -> Self {
        rules.sort_by_key(|r| std::cmp::Reverse(r.source_path.len()));
        self.path_mapping_rules = Arc::new(rules);
        self.library = derive_library(self.library_base.as_deref(), &self.path_mapping_rules);
        self
    }

    /// Extend the session's path mapping rules with additional rules.
    /// Rules are re-sorted by source path length (longest first) after extending.
    pub fn extend_path_mapping_rules(&mut self, additional: Vec<PathMappingRule>) {
        let mut rules = (*self.path_mapping_rules).clone();
        rules.extend(additional);
        rules.sort_by_key(|r| std::cmp::Reverse(r.source_path.len()));
        self.path_mapping_rules = Arc::new(rules);
        self.library = derive_library(self.library_base.as_deref(), &self.path_mapping_rules);
    }

    /// Get the current path mapping rules.
    pub fn path_mapping_rules(&self) -> &[PathMappingRule] {
        &self.path_mapping_rules
    }

    pub fn with_library(mut self, library: FunctionLibrary) -> Self {
        self.library_base = Some(Arc::new(library));
        self.library = derive_library(self.library_base.as_deref(), &self.path_mapping_rules);
        self
    }

    pub fn with_revision_extensions(mut self, ctx: openjd_model::types::ValidationContext) -> Self {
        self.revision_extensions = Some(ctx);
        self
    }

    /// Check whether redacted env vars are enabled, mirroring Python's `_redactions_enabled()`.
    /// True if spec revision > v2023_09 OR "REDACTED_ENV_VARS" extension is present.
    fn redactions_enabled(&self) -> bool {
        match &self.revision_extensions {
            Some(ctx) => {
                ctx.revision > openjd_model::types::SpecificationRevision::V2023_09
                    || ctx.has_extension(openjd_model::types::KnownExtension::RedactedEnvVars)
            }
            None => false,
        }
    }

    fn lib(&self) -> Option<&FunctionLibrary> {
        Some(&self.library)
    }

    /// Fire the callback with the current action status.
    /// Called when transitioning to RUNNING so callers see the state change immediately.
    fn notify_callback(&self) {
        if let Some(cb) = &self.callback {
            if let Some(status) = self.action_status() {
                cb(&self.session_id, &status);
            }
        }
    }

    // --- Properties ---

    /// Returns the list of enabled extensions for this session.
    /// If no revision_extensions are set, returns an empty vec.
    pub fn get_enabled_extensions(&self) -> Vec<String> {
        match &self.revision_extensions {
            Some(ctx) => {
                let mut exts: Vec<String> = ctx
                    .extensions
                    .iter()
                    .map(|e| e.as_str().to_string())
                    .collect();
                exts.sort();
                exts
            }
            None => Vec::new(),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }
    pub fn state(&self) -> SessionState {
        self.state
    }
    pub fn working_directory(&self) -> &Path {
        &self.working_directory
    }
    pub fn files_directory(&self) -> &Path {
        &self.files_directory
    }
    pub fn environments_entered(&self) -> &[EnvironmentIdentifier] {
        &self.environments_entered
    }

    /// Clone the cancel_writer file handle, if one exists.
    /// Used by Python bindings to send cancel commands when the session is
    /// taken by a background thread.
    pub fn clone_cancel_writer(&self) -> Option<std::fs::File> {
        self.cross_user
            .cancel_writer
            .as_ref()
            .and_then(|f| f.try_clone().ok())
    }

    /// Get the current action status, if any action has been run.
    pub fn action_status(&self) -> Option<ActionStatus> {
        self.action.state.map(|state| ActionStatus {
            state,
            progress: self.action.progress,
            status_message: self.action.status_message.clone(),
            fail_message: self.action.fail_message.clone(),
            exit_code: self.action.exit_code,
            started_at: self.action.started_at,
            ended_at: self.action.ended_at,
        })
    }

    /// Override the action state. Used by the pyo3 binding to convert
    /// Failed → Canceled when cancel was requested externally.
    pub fn override_action_state(&mut self, state: ActionState) {
        self.action.state = Some(state);
    }

    /// Redact sensitive values from output text.
    pub fn redact(&self, text: &str) -> String {
        // Sort by length descending so longer matches are replaced first,
        // preventing partial replacements when values overlap (e.g. "FOOBAR" and "BAR").
        let mut vals: Vec<&str> = self.redacted_values.iter().map(|s| s.as_str()).collect();
        vals.sort_by_key(|s| std::cmp::Reverse(s.len()));
        let mut result = text.to_string();
        for val in vals {
            result = result.replace(val, "********");
        }
        result
    }

    /// Create a cancellation token for an action. If a parent token was
    /// provided at session construction, the action token is a child of it
    /// so that cancelling the parent cascades to all actions.
    fn new_action_cancel_token(&self) -> CancellationToken {
        match &self.cancel.parent_token {
            Some(parent) => parent.child_token(),
            None => CancellationToken::new(),
        }
    }

    /// Cancel the currently running async action.
    pub fn cancel_action(
        &mut self,
        time_limit: Option<Duration>,
        mark_action_failed: bool,
    ) -> Result<(), SessionError> {
        if self.state != SessionState::Running {
            return Err(SessionError::InvalidState {
                expected: vec![SessionState::Running],
                current: self.state,
            });
        }
        self.state = SessionState::Canceling;
        if mark_action_failed {
            self.cancel.mark_failed = true;
        }

        // Send cancel to the helper process via the dup'd stdin fd.
        if let Some(ref mut writer) = self.cross_user.cancel_writer {
            use std::io::Write;
            let is_terminate = matches!(time_limit, Some(d) if d.is_zero());
            let cmd = if is_terminate {
                r#"{"cancel":"TERMINATE"}"#.to_string()
            } else {
                let notify_period = time_limit
                    .unwrap_or(Duration::from_secs(DEFAULT_CANCEL_NOTIFY_PERIOD_SECS))
                    .as_secs();
                format!(
                    r#"{{"cancel":"NOTIFY_THEN_TERMINATE","notifyPeriodInSeconds":{notify_period}}}"#
                )
            };
            let _ = writer.write_all(cmd.as_bytes());
            let _ = writer.write_all(b"\n");
            let _ = writer.flush();
        }

        if let Some(tx) = &self.cancel.request_tx {
            let _ = tx.send(time_limit);
        }
        if let Some(token) = &self.cancel.token {
            token.cancel();
        }
        Ok(())
    }

    /// Clean up the session. Deletes working directory if not retained.
    pub fn cleanup(&mut self) {
        if self.cleanup_called {
            return;
        }
        self.cleanup_called = true;

        if !self.retain_working_dir {
            log_section_banner(&self.session_id, "Session Cleanup");

            // Shut down the cross-user helper before deleting the working directory
            if let Some(ref mut helper) = self.cross_user.helper {
                helper.shutdown();
            }

            session_log!(
                info,
                &self.session_id,
                LogContent::FILE_PATH,
                "Deleting working directory: {}",
                self.working_directory.display()
            );

            // Cross-user cleanup: delete files owned by session user first,
            // since files created via sudo may not be deletable by the process owner.
            #[cfg(unix)]
            if let Some(ref user) = self.cross_user.user {
                if !user.is_process_user() {
                    if let Ok(entries) = std::fs::read_dir(&self.working_directory) {
                        let files: Vec<String> = entries
                            .filter_map(|e| e.ok())
                            .map(|e| e.path().to_string_lossy().to_string())
                            .collect();
                        if !files.is_empty() {
                            let mut args = vec![
                                "-u".to_string(),
                                user.user().to_string(),
                                "-i".to_string(),
                                "rm".to_string(),
                                "-rf".to_string(),
                                "--".to_string(),
                            ];
                            args.extend(files);
                            let _ = std::process::Command::new("sudo")
                                .args(&args)
                                .stdin(std::process::Stdio::null())
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::null())
                                .status();
                        }
                    }
                }
            }

            // If we own TempDirs, clean them up; otherwise fall back to remove_dir_all
            if let Some(ref mut files_dir) = self._files_dir {
                let _ = files_dir.cleanup();
            }
            if let Some(ref mut working_dir) = self._working_dir {
                let _ = working_dir.cleanup();
            } else {
                let _ = std::fs::remove_dir_all(&self.working_directory);
            }
        }
        self.state = SessionState::Ended;
    }

    /// Enter an environment asynchronously (non-blocking).
    /// Returns the environment identifier on success.
    pub async fn enter_environment(
        &mut self,
        env: &Environment,
        resolved_symtab: Option<&openjd_expr::SerializedSymbolTable>,
        identifier: Option<&str>,
        os_env_vars: Option<&HashMap<String, String>>,
    ) -> Result<String, SessionError> {
        let (id, _stdout) = self
            .enter_environment_with_output(env, resolved_symtab, identifier, os_env_vars)
            .await?;
        Ok(id)
    }

    /// Enter an environment, returning both the identifier and the stdout from the onEnter script.
    pub async fn enter_environment_with_output(
        &mut self,
        env: &Environment,
        resolved_symtab: Option<&openjd_expr::SerializedSymbolTable>,
        identifier: Option<&str>,
        os_env_vars: Option<&HashMap<String, String>>,
    ) -> Result<(String, String), SessionError> {
        if self.state != SessionState::Ready {
            return Err(SessionError::InvalidState {
                expected: vec![SessionState::Ready],
                current: self.state,
            });
        }

        let symtab = self.build_symbol_table(None, resolved_symtab)?;

        let identifier = match identifier {
            Some(id) => {
                if self.environments.contains_key(id) {
                    return Err(SessionError::DuplicateEnvironment { id: id.to_string() });
                }
                id.to_string()
            }
            None => format!("{}:{}", self.session_id, uuid::Uuid::new_v4().simple()),
        };
        self.environments.insert(identifier.clone(), env.clone());
        self.environments_entered.push(identifier.clone());
        self.created_env_vars
            .insert(identifier.clone(), HashMap::new());

        // Set static variables
        if let Some(vars) = &env.variables {
            for (key, fmt_str) in vars {
                let value = fmt_str
                    .resolve_string_with(
                        &symtab,
                        &openjd_expr::FormatStringOptions::new().with_library(self.lib()),
                    )
                    .map_err(|e| SessionError::FormatString {
                        context: format!("env var '{key}'"),
                        reason: e.to_string(),
                    })?;
                let norm_key = normalize_env_key(key);
                self.env_vars.insert(norm_key.clone(), value.clone());
                if let Some(changes) = self.created_env_vars.get_mut(&identifier) {
                    changes.insert(norm_key, Some(value));
                }
            }
        }

        let output = if env
            .script
            .as_ref()
            .and_then(|s| s.actions.on_enter.as_ref())
            .is_some()
        {
            self.action.reset();
            self.state = SessionState::Running;
            self.notify_callback();

            log_section_banner(
                &self.session_id,
                &format!("Entering Environment: {}", env.name),
            );

            let cancel_token = self.new_action_cancel_token();
            let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(None);
            self.cancel.token = Some(cancel_token.clone());
            self.cancel.request_tx = Some(cancel_tx);

            let env_vars = self.evaluate_env_vars(os_env_vars);
            let mut action_symtab = symtab.clone();
            self.materialize_path_mapping(&mut action_symtab)?;
            #[allow(unused_mut)]
            let mut runner = EnvironmentScriptRunner::new(
                &self.session_id,
                self.working_directory.clone(),
                self.files_directory.clone(),
                self.cross_user.user.clone(),
            )
            .with_redactions(self.redactions_enabled())
            .with_debug_collect_stdout(self.debug_collect_stdout)
            .with_initial_redacted_values(self.redacted_values.iter().cloned().collect())
            .with_cancel_token(cancel_token)
            .with_cancel_request_rx(cancel_rx);
            let mut runner = match self.cross_user.helper.take() {
                Some(h) => {
                    let r = runner.with_helper(h);
                    match self
                        .cross_user
                        .cancel_writer
                        .as_ref()
                        .and_then(|f| f.try_clone().ok())
                    {
                        Some(w) => r.with_cancel_writer(w),
                        None => r,
                    }
                }
                None => runner,
            };

            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

            let lib = self.library.clone();
            let runner_fut = runner.enter(env, &action_symtab, Some(&lib), &env_vars, tx);
            let result = self.drive_action(runner_fut, &mut rx, &identifier).await;
            self.cross_user.helper = runner.take_helper();
            let result = result?;

            if result.state != ActionState::Success {
                return Err(SessionError::EnvironmentScriptFailed {
                    name: env.name.clone(),
                    action: "onEnter".into(),
                    reason: format_exit_code(result.exit_code),
                });
            }
            result.stdout.clone()
        } else {
            let now = std::time::SystemTime::now();
            self.action.state = Some(ActionState::Success);
            self.action.started_at = Some(now);
            self.action.progress = None;
            self.action.status_message = None;
            self.action.fail_message = None;
            self.action.exit_code = None;

            log_section_banner(
                &self.session_id,
                &format!("Entering Environment: {}", env.name),
            );

            self.action.ended_at = Some(std::time::SystemTime::now());

            if let Some(cb) = &self.callback {
                if let Some(status) = self.action_status() {
                    cb(&self.session_id, &status);
                }
            }

            String::new()
        };

        if self.state == SessionState::Running {
            self.state = SessionState::Ready;
        }
        Ok((identifier, output))
    }

    /// Exit an environment asynchronously, identified by its environment identifier.
    ///
    /// Environments must be exited in LIFO order (last entered, first exited).
    /// The `keep_session_running` parameter controls whether the session transitions
    /// to `ReadyEnding` after exit — if `false`, the session becomes ending-only.
    pub async fn exit_environment(
        &mut self,
        identifier: &EnvironmentIdentifier,
        resolved_symtab: Option<&openjd_expr::SerializedSymbolTable>,
        keep_session_running: bool,
        os_env_vars: Option<&HashMap<String, String>>,
    ) -> Result<String, SessionError> {
        if self.state != SessionState::Ready && self.state != SessionState::ReadyEnding {
            return Err(SessionError::InvalidState {
                expected: vec![SessionState::Ready, SessionState::ReadyEnding],
                current: self.state,
            });
        }

        // Validate identifier exists
        let env = self
            .environments
            .get(identifier)
            .ok_or_else(|| SessionError::UnknownEnvironment {
                identifier: identifier.to_string(),
            })?
            .clone();

        // Validate LIFO order
        if self.environments_entered.last() != Some(identifier) {
            return Err(SessionError::LifoViolation {
                expected: self
                    .environments_entered
                    .last()
                    .cloned()
                    .unwrap_or_default(),
                got: identifier.clone(),
            });
        }

        let symtab = self.build_symbol_table(None, resolved_symtab)?;

        // Evaluate env vars BEFORE removing from tracking (matching Python)
        let env_vars = self.evaluate_env_vars(os_env_vars);

        // Unless overridden by the caller, once we've started exiting environments
        // we can only exit environments.
        if !keep_session_running {
            self.ending_only = true;
        }

        // Remove environment from tracking BEFORE running the exit script.
        // This matches the Python session behavior — a failed exit is still an exit,
        // and subsequent exits must be able to proceed in LIFO order.
        self.environments.remove(identifier);
        self.environments_entered.pop();

        let output = if env
            .script
            .as_ref()
            .and_then(|s| s.actions.on_exit.as_ref())
            .is_some()
        {
            self.action.reset();
            self.state = SessionState::Running;
            self.notify_callback();

            log_section_banner(
                &self.session_id,
                &format!("Exiting Environment: {}", env.name),
            );

            let cancel_token = self.new_action_cancel_token();
            let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(None);
            self.cancel.token = Some(cancel_token.clone());
            self.cancel.request_tx = Some(cancel_tx);

            let mut action_symtab = symtab.clone();
            self.materialize_path_mapping(&mut action_symtab)?;
            #[allow(unused_mut)]
            let mut runner = EnvironmentScriptRunner::new(
                &self.session_id,
                self.working_directory.clone(),
                self.files_directory.clone(),
                self.cross_user.user.clone(),
            )
            .with_redactions(self.redactions_enabled())
            .with_debug_collect_stdout(self.debug_collect_stdout)
            .with_initial_redacted_values(self.redacted_values.iter().cloned().collect())
            .with_cancel_token(cancel_token)
            .with_cancel_request_rx(cancel_rx);
            let mut runner = match self.cross_user.helper.take() {
                Some(h) => {
                    let r = runner.with_helper(h);
                    match self
                        .cross_user
                        .cancel_writer
                        .as_ref()
                        .and_then(|f| f.try_clone().ok())
                    {
                        Some(w) => r.with_cancel_writer(w),
                        None => r,
                    }
                }
                None => runner,
            };

            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

            let lib = self.library.clone();
            let runner_fut = runner.exit(&env, &action_symtab, Some(&lib), &env_vars, tx);
            let result = self.drive_action(runner_fut, &mut rx, identifier).await;
            self.cross_user.helper = runner.take_helper();
            let result = result?;

            if result.state != ActionState::Success {
                self.state = SessionState::ReadyEnding;
                return Err(SessionError::EnvironmentScriptFailed {
                    name: env.name.clone(),
                    action: "onExit".into(),
                    reason: format_exit_code(result.exit_code),
                });
            }
            result.stdout.clone()
        } else {
            // No exit script — set state based on ending_only (drive_action not called)
            let now = std::time::SystemTime::now();
            self.action.state = Some(ActionState::Success);
            self.action.started_at = Some(now);
            self.action.progress = None;
            self.action.status_message = None;
            self.action.fail_message = None;
            self.action.exit_code = None;
            self.state = if self.ending_only {
                SessionState::ReadyEnding
            } else {
                SessionState::Ready
            };

            log_section_banner(
                &self.session_id,
                &format!("Exiting Environment: {}", env.name),
            );

            self.action.ended_at = Some(std::time::SystemTime::now());

            if let Some(cb) = &self.callback {
                if let Some(status) = self.action_status() {
                    cb(&self.session_id, &status);
                }
            }

            String::new()
        };

        Ok(output)
    }

    /// Run a step action asynchronously.
    pub async fn run_task(
        &mut self,
        script: &StepScript,
        task_parameter_values: Option<&openjd_model::types::TaskParameterSet>,
        resolved_symtab: Option<&openjd_expr::SerializedSymbolTable>,
        os_env_vars: Option<&HashMap<String, String>>,
    ) -> Result<ActionResult, SessionError> {
        if self.state != SessionState::Ready {
            return Err(SessionError::InvalidState {
                expected: vec![SessionState::Ready],
                current: self.state,
            });
        }

        let symtab = self.build_symbol_table(task_parameter_values, resolved_symtab)?;

        self.action.reset();
        self.state = SessionState::Running;
        self.notify_callback();

        log_section_banner(&self.session_id, "Running Task");

        let cancel_token = self.new_action_cancel_token();
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(None);
        self.cancel.token = Some(cancel_token.clone());
        self.cancel.request_tx = Some(cancel_tx);

        let env_vars = self.evaluate_env_vars(os_env_vars);
        let mut action_symtab = symtab.clone();
        self.materialize_path_mapping(&mut action_symtab)?;
        #[allow(unused_mut)]
        let mut runner = StepScriptRunner::new(
            &self.session_id,
            self.working_directory.clone(),
            self.files_directory.clone(),
            self.cross_user.user.clone(),
        )
        .with_redactions(self.redactions_enabled())
        .with_debug_collect_stdout(self.debug_collect_stdout)
        .with_initial_redacted_values(self.redacted_values.iter().cloned().collect())
        .with_cancel_token(cancel_token)
        .with_cancel_request_rx(cancel_rx);
        let mut runner = match self.cross_user.helper.take() {
            Some(h) => {
                let r = runner.with_helper(h);
                match self
                    .cross_user
                    .cancel_writer
                    .as_ref()
                    .and_then(|f| f.try_clone().ok())
                {
                    Some(w) => r.with_cancel_writer(w),
                    None => r,
                }
            }
            None => runner,
        };

        let step_identifier = format!("{}:step:{}", self.session_id, uuid::Uuid::new_v4().simple());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let lib = self.library.clone();
        let runner_fut = runner.run(script, &action_symtab, Some(&lib), &env_vars, tx);
        let result = self
            .drive_action(runner_fut, &mut rx, &step_identifier)
            .await;
        self.cross_user.helper = runner.take_helper();
        let result = result?;

        Ok(ActionResult {
            state: result.state,
            exit_code: result.exit_code,
            stdout: result.stdout,
        })
    }

    /// Run an ad-hoc subprocess within the Session.
    ///
    /// Unlike `run_task`, this runs a raw command without format string
    /// evaluation, embedded file materialization, or path mapping. Used by the
    /// worker agent for install/sync operations.
    pub async fn run_subprocess(
        &mut self,
        command: &str,
        args: Option<&[String]>,
        timeout: Option<Duration>,
        os_env_vars: Option<&HashMap<String, String>>,
        use_session_env_vars: bool,
        log_banner_message: Option<&str>,
    ) -> Result<crate::subprocess::SubprocessResult, SessionError> {
        if self.state != SessionState::Ready {
            return Err(SessionError::InvalidState {
                expected: vec![SessionState::Ready],
                current: self.state,
            });
        }
        if command.is_empty() || command.trim().is_empty() {
            return Err(SessionError::Runtime(
                "command must be a non-empty string".into(),
            ));
        }
        if let Some(t) = timeout {
            if t.is_zero() {
                return Err(SessionError::Runtime("timeout must be positive".into()));
            }
        }

        if let Some(msg) = log_banner_message {
            log_section_banner(&self.session_id, msg);
        }

        self.action.reset();
        self.state = SessionState::Running;
        self.notify_callback();

        let env_vars = if use_session_env_vars {
            self.evaluate_env_vars(os_env_vars)
        } else {
            let mut result: HashMap<String, Option<String>> = self
                .process_env
                .iter()
                .map(|(k, v)| (k.clone(), Some(v.clone())))
                .collect();
            if let Some(extra) = os_env_vars {
                for (k, v) in extra {
                    result.insert(k.clone(), Some(v.clone()));
                }
            }
            result
        };

        let mut cmd_args = vec![command.to_string()];
        if let Some(a) = args {
            cmd_args.extend(a.iter().cloned());
        }

        // Route through the persistent helper process when cross-user helper is active.
        if self.cross_user.helper.is_some() {
            return self
                .run_subprocess_via_helper(&cmd_args, env_vars, timeout)
                .await;
        }

        let cancel_token = self.new_action_cancel_token();
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(None);
        self.cancel.token = Some(cancel_token.clone());
        self.cancel.request_tx = Some(cancel_tx);

        let config = crate::subprocess::SubprocessConfig {
            args: cmd_args,
            env_vars,
            working_dir: Some(self.working_directory.clone()),
            timeout,
            user: self.cross_user.user.clone(),
            cancel_method: crate::runner::CancelMethod::Terminate,
            cancel_request_rx: Some(cancel_rx),
            debug_collect_stdout: self.debug_collect_stdout,
        };
        let mut filter = crate::action_filter::ActionFilter::new(&self.session_id, true, false);
        let subprocess_identifier = format!(
            "{}:subprocess:{}",
            self.session_id,
            uuid::Uuid::new_v4().simple()
        );
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sid = self.session_id.clone();
        let runner_fut =
            crate::subprocess::run_subprocess(config, &mut filter, &sid, tx, cancel_token);
        self.drive_action(runner_fut, &mut rx, &subprocess_identifier)
            .await
    }

    /// Execute a subprocess via the persistent cross-user helper process.
    ///
    /// Instead of spawning `sudo` (POSIX) or `CreateProcessAsUserW` (Windows)
    /// per action, sends a run command over the helper's stdin and reads
    /// streamed responses from its stdout.
    async fn run_subprocess_via_helper(
        &mut self,
        args: &[String],
        env_vars: std::collections::HashMap<String, Option<String>>,
        _timeout: Option<Duration>,
    ) -> Result<crate::subprocess::SubprocessResult, SessionError> {
        let mut filter = crate::action_filter::ActionFilter::new(&self.session_id, true, false);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let subprocess_identifier = format!(
            "{}:subprocess:{}",
            self.session_id,
            uuid::Uuid::new_v4().simple()
        );

        let config = crate::subprocess::SubprocessConfig {
            args: args.to_vec(),
            env_vars,
            working_dir: Some(self.working_directory.clone()),
            timeout: _timeout,
            user: self.cross_user.user.clone(),
            cancel_method: crate::runner::CancelMethod::Terminate,
            cancel_request_rx: None,
            debug_collect_stdout: self.debug_collect_stdout,
        };

        let helper = self
            .cross_user
            .helper
            .as_mut()
            .expect("caller checked helper.is_some()");
        let result = run_via_helper(
            helper,
            &config,
            &mut filter,
            &self.session_id,
            tx,
            self.cross_user.cancel_writer.as_ref(),
        )
        .await;

        // Process any remaining messages.
        while let Ok(msg) = rx.try_recv() {
            self.apply_message(msg, &subprocess_identifier);
        }

        let r = result?;
        self.action.state = Some(r.state);
        self.action.ended_at = Some(std::time::SystemTime::now());
        self.action.exit_code = r.exit_code;
        self.cancel.token = None;
        self.cancel.request_tx = None;
        self.cancel.mark_failed = false;
        self.state = if r.state == ActionState::Success {
            SessionState::Ready
        } else {
            SessionState::ReadyEnding
        };
        self.notify_callback();
        Ok(r)
    }

    // --- Internal helpers ---

    /// Run an action future while concurrently processing messages from the channel
    /// in real-time. This ensures callbacks fire as stdout lines are parsed, not
    /// after the action completes.
    async fn drive_action<F>(
        &mut self,
        action_fut: F,
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<ActionMessage>,
        identifier: &str,
    ) -> Result<crate::subprocess::SubprocessResult, SessionError>
    where
        F: std::future::Future<Output = Result<crate::subprocess::SubprocessResult, SessionError>>,
    {
        tokio::pin!(action_fut);
        let mut result = None;

        loop {
            tokio::select! {
                biased;
                msg = rx.recv(), if result.is_none() => {
                    // Channel returns None when closed; runner will finish soon.
                    if let Some(msg) = msg { self.apply_message(msg, identifier) }
                }
                r = &mut action_fut, if result.is_none() => {
                    result = Some(r);
                }
                else => break,
            }
            if result.is_some() {
                // Drain any remaining messages after the runner completes
                while let Ok(msg) = rx.try_recv() {
                    self.apply_message(msg, identifier);
                }
                break;
            }
        }

        let r = match result.expect("loop guarantees result is Some") {
            Ok(r) => r,
            Err(e) => {
                // The subprocess failed to start or the runner encountered an error.
                // Update session state so callers see Failed instead of stuck Running.
                self.action.state = Some(ActionState::Failed);
                self.action.ended_at = Some(std::time::SystemTime::now());
                self.action.exit_code = None;
                self.cancel.token = None;
                self.cancel.request_tx = None;
                self.cancel.mark_failed = false;
                self.state = SessionState::ReadyEnding;

                if let Some(cb) = &self.callback {
                    if let Some(status) = self.action_status() {
                        cb(&self.session_id, &status);
                    }
                }

                return Err(e);
            }
        };

        // If the action was canceled but mark_action_failed is set,
        // report it as Failed instead of Canceled (matches Python behavior)
        let final_state = if self.cancel.mark_failed && r.state == ActionState::Canceled {
            ActionState::Failed
        } else {
            r.state
        };

        self.action.state = Some(final_state);
        self.action.ended_at = Some(std::time::SystemTime::now());
        self.action.exit_code = r.exit_code;
        self.cancel.token = None;
        self.cancel.request_tx = None;
        self.cancel.mark_failed = false;

        self.state = if self.ending_only || final_state != ActionState::Success {
            SessionState::ReadyEnding
        } else {
            SessionState::Ready
        };

        if let Some(cb) = &self.callback {
            if let Some(status) = self.action_status() {
                cb(&self.session_id, &status);
            }
        }

        Ok(crate::subprocess::SubprocessResult {
            state: final_state,
            exit_code: r.exit_code,
            stdout: r.stdout,
        })
    }

    /// Apply a single ActionMessage to session state.
    fn apply_message(&mut self, msg: ActionMessage, identifier: &str) {
        match msg {
            ActionMessage::Progress(v) => {
                self.action.progress = Some(v);
            }
            ActionMessage::Status(s) => {
                self.action.status_message = Some(s);
            }
            ActionMessage::Fail(s) => {
                self.action.fail_message = Some(s);
            }
            ActionMessage::SetEnv { name, value } => {
                let key = normalize_env_key(&name);
                self.env_vars.insert(key.clone(), value.clone());
                if let Some(changes) = self.created_env_vars.get_mut(identifier) {
                    changes.insert(key, Some(value));
                }
            }
            ActionMessage::UnsetEnv { name } => {
                let key = normalize_env_key(&name);
                self.env_vars.remove(&key);
                if let Some(changes) = self.created_env_vars.get_mut(identifier) {
                    changes.insert(key, None);
                }
            }
            ActionMessage::RedactedEnv { name, value } => {
                if self.redactions_enabled() {
                    let key = normalize_env_key(&name);
                    self.env_vars.insert(key.clone(), value.clone());
                    if let Some(changes) = self.created_env_vars.get_mut(identifier) {
                        changes.insert(key, Some(value.clone()));
                    }
                }
                self.redacted_values.insert(value);
            }
            ActionMessage::CancelMarkFailed { fail_message } => {
                self.action.fail_message = Some(fail_message);
                let _ = self.cancel_action(None, true);
            }
        }
        if let Some(cb) = &self.callback {
            if let Some(status) = self.action_status() {
                cb(&self.session_id, &status);
            }
        }
    }

    /// Materialize path mapping rules to a JSON file and set symbol table entries.
    fn materialize_path_mapping(&self, symtab: &mut SymbolTable) -> Result<(), SessionError> {
        let has_rules = !self.path_mapping_rules.is_empty();
        let rules_json = if has_rules {
            let rules: Vec<serde_json::Value> = self
                .path_mapping_rules
                .iter()
                .map(|rule| {
                    serde_json::json!({
                        "source_path_format": match rule.source_path_format {
                            openjd_expr::path_mapping::PathFormat::Posix => "POSIX",
                            openjd_expr::path_mapping::PathFormat::Windows => "WINDOWS",
                            openjd_expr::path_mapping::PathFormat::Uri => "URI",
                        },
                        "source_path": &rule.source_path,
                        "destination_path": &rule.destination_path,
                    })
                })
                .collect();
            serde_json::json!({"version": "pathmapping-1.0", "path_mapping_rules": rules})
                .to_string()
        } else {
            serde_json::json!({}).to_string()
        };

        symtab
            .set(
                "Session.HasPathMappingRules",
                openjd_expr::ExprValue::Bool(has_rules),
            )
            .map_err(|e| {
                SessionError::Runtime(format!("Failed to set HasPathMappingRules: {e}"))
            })?;

        let filename = self.working_directory.join(format!(
            "pathmapping_{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::write(&filename, &rules_json).map_err(|e| SessionError::WorkingDirectory {
            path: filename.clone(),
            source: e,
        })?;

        symtab
            .set(
                "Session.PathMappingRulesFile",
                openjd_expr::ExprValue::new_path(
                    filename.to_string_lossy().to_string(),
                    openjd_expr::path_mapping::PathFormat::host(),
                ),
            )
            .map_err(|e| {
                SessionError::Runtime(format!("Failed to set PathMappingRulesFile: {e}"))
            })?;

        Ok(())
    }

    /// Evaluate the cumulative env vars from process_env + extra + per-environment changes.
    /// Mirrors Python `_evaluate_current_session_env_vars`.
    pub fn evaluate_env_vars(
        &self,
        extra: Option<&HashMap<String, String>>,
    ) -> HashMap<String, Option<String>> {
        let mut result: HashMap<String, Option<String>> = self
            .process_env
            .iter()
            .map(|(k, v)| (normalize_env_key(k), Some(v.clone())))
            .collect();
        // Per spec: expose the session working directory as an environment variable
        // so nested subprocesses can access it without template variable syntax.
        result.insert(
            "OPENJD_SESSION_WORKING_DIR".to_string(),
            Some(self.working_directory.to_string_lossy().into_owned()),
        );
        if let Some(extra) = extra {
            for (k, v) in extra {
                result.insert(normalize_env_key(k), Some(v.clone()));
            }
        }
        for id in &self.environments_entered {
            if let Some(changes) = self.created_env_vars.get(id) {
                for (name, value) in changes {
                    result.insert(name.clone(), value.clone());
                }
            }
        }
        result
    }

    /// Get the job parameter values.
    pub fn job_parameter_values(&self) -> &JobParameterValues {
        &self.job_parameter_values
    }

    /// Build a SymbolTable for running actions, mirroring Python's Session._symbol_table().
    /// Populates job parameters (Param.* and RawParam.*), task parameters (Task.Param.* and Task.RawParam.*),
    /// and Session.WorkingDirectory.
    ///
    /// If `base` is provided, clones it as the starting point (it already contains Param.*, RawParam.*,
    /// Job.Name, Step.Name, and let bindings) and only layers Session.WorkingDirectory and Task.* on top.
    /// If `base` is None, builds from scratch using self.job_parameter_values.
    pub fn build_symbol_table(
        &self,
        task_parameter_values: Option<&openjd_model::types::TaskParameterSet>,
        base: Option<&openjd_expr::SerializedSymbolTable>,
    ) -> Result<SymbolTable, SessionError> {
        use openjd_model::types::TaskParameterType;

        let mut symtab = if let Some(base) = base {
            // Deserialize with host path format — this is the template→session boundary.
            // Path values stored as Posix in template scope get normalized to host format.
            let mut s = base
                .to_symtab(openjd_expr::path_mapping::PathFormat::host())
                .map_err(|e| {
                    SessionError::Runtime(format!("Failed to deserialize resolved_symtab: {e}"))
                })?;
            // Re-apply path mapping to Param.* PATH values from the base symtab.
            // The base (resolved_symtab from create_job) has unmapped paths; the session
            // knows the worker's path mapping rules.
            for (name, param) in &self.job_parameter_values {
                use openjd_model::types::JobParameterType;
                match param.param_type {
                    JobParameterType::Path => {
                        let raw = match &param.value {
                            openjd_expr::ExprValue::String(s) => s.as_str(),
                            openjd_expr::ExprValue::Path { value, .. } => value.as_str(),
                            _ => continue,
                        };
                        let mapped = self.apply_path_mapping_to_string(raw);
                        let key = format!("Param.{name}");
                        s.set(
                            &key,
                            openjd_expr::ExprValue::new_path(
                                mapped,
                                openjd_expr::path_mapping::PathFormat::host(),
                            ),
                        )
                        .map_err(|e| SessionError::Runtime(format!("Failed to set {key}: {e}")))?;
                    }
                    JobParameterType::ListPath => {
                        if let openjd_expr::ExprValue::ListString(ref elements, _) = param.value {
                            let mapped: Vec<openjd_expr::ExprValue> = elements
                                .iter()
                                .map(|s| {
                                    let m = self.apply_path_mapping_to_string(s);
                                    openjd_expr::ExprValue::new_path(
                                        m,
                                        openjd_expr::path_mapping::PathFormat::host(),
                                    )
                                })
                                .collect();
                            let key = format!("Param.{name}");
                            s.set(
                                &key,
                                openjd_expr::ExprValue::make_list(
                                    mapped,
                                    openjd_expr::ExprType::PATH,
                                )
                                .unwrap(),
                            )
                            .map_err(|e| {
                                SessionError::Runtime(format!("Failed to set {key}: {e}"))
                            })?;
                        }
                    }
                    _ => {}
                }
            }
            s
        } else {
            let mut s = SymbolTable::new();
            for (name, param) in &self.job_parameter_values {
                let raw_key = format!("RawParam.{name}");
                s.set(&raw_key, param.value.clone())
                    .map_err(|e| SessionError::Runtime(format!("Failed to set {raw_key}: {e}")))?;
                let key = format!("Param.{name}");
                use openjd_model::types::JobParameterType;
                let mapped_value = match param.param_type {
                    JobParameterType::Path => {
                        let raw = match &param.value {
                            openjd_expr::ExprValue::String(s) => s.as_str(),
                            openjd_expr::ExprValue::Path { value, .. } => value.as_str(),
                            _ => "",
                        };
                        let mapped = self.apply_path_mapping_to_string(raw);
                        openjd_expr::ExprValue::new_path(
                            mapped,
                            openjd_expr::path_mapping::PathFormat::host(),
                        )
                    }
                    JobParameterType::ListPath => match &param.value {
                        openjd_expr::ExprValue::ListString(elements, _) => {
                            let mapped: Vec<openjd_expr::ExprValue> = elements
                                .iter()
                                .map(|s| {
                                    let m = self.apply_path_mapping_to_string(s);
                                    openjd_expr::ExprValue::new_path(
                                        m,
                                        openjd_expr::path_mapping::PathFormat::host(),
                                    )
                                })
                                .collect();
                            openjd_expr::ExprValue::make_list(mapped, openjd_expr::ExprType::PATH)
                                .unwrap()
                        }
                        other => self.apply_path_mapping_to_value(other),
                    },
                    _ => self.apply_path_mapping_to_value(&param.value),
                };
                s.set(&key, mapped_value)
                    .map_err(|e| SessionError::Runtime(format!("Failed to set {key}: {e}")))?;
            }
            s
        };

        let host_path_format = openjd_expr::path_mapping::PathFormat::host();
        symtab
            .set(
                "Session.WorkingDirectory",
                openjd_expr::ExprValue::new_path(
                    self.working_directory.to_string_lossy().to_string(),
                    host_path_format,
                ),
            )
            .map_err(|e| SessionError::Runtime(format!("Failed to set WorkingDirectory: {e}")))?;

        if let Some(task_params) = task_parameter_values {
            for (name, tv) in task_params {
                // Task.RawParam.* — raw string value
                let raw_key = format!("Task.RawParam.{name}");
                symtab
                    .set(&raw_key, tv.value.clone())
                    .map_err(|e| SessionError::Runtime(format!("Failed to set {raw_key}: {e}")))?;

                // Task.Param.* — typed value with path mapping applied for PATH types
                let key = format!("Task.Param.{name}");
                let param_value = match tv.param_type {
                    TaskParameterType::Path => {
                        let s = match &tv.value {
                            openjd_expr::ExprValue::String(s) => s.as_str(),
                            openjd_expr::ExprValue::Path { value, .. } => value.as_str(),
                            _ => "",
                        };
                        let mapped = self.apply_path_mapping_to_string(s);
                        openjd_expr::ExprValue::new_path(
                            mapped,
                            openjd_expr::path_mapping::PathFormat::host(),
                        )
                    }
                    _ => tv.value.clone(),
                };
                symtab
                    .set(&key, param_value)
                    .map_err(|e| SessionError::Runtime(format!("Failed to set {key}: {e}")))?;
            }
        }

        Ok(symtab)
    }

    /// Apply path mapping rules to a string, returning the mapped result.
    fn apply_path_mapping_to_string(&self, path: &str) -> String {
        for rule in self.path_mapping_rules.iter() {
            if let Some(mapped) = rule.apply(path) {
                return mapped;
            }
        }
        path.to_string()
    }

    /// Apply path mapping rules to a value if it's a Path or ListPath type.
    fn apply_path_mapping_to_value(
        &self,
        value: &openjd_expr::ExprValue,
    ) -> openjd_expr::ExprValue {
        match value {
            openjd_expr::ExprValue::Path {
                value: path_str,
                format,
                ..
            } => {
                for rule in self.path_mapping_rules.iter() {
                    if let Some(mapped) = rule.apply(path_str) {
                        return openjd_expr::ExprValue::new_path(mapped, *format);
                    }
                }
                value.clone()
            }
            openjd_expr::ExprValue::ListPath(elements, fmt, _) => {
                let mapped: Vec<openjd_expr::ExprValue> = elements
                    .iter()
                    .map(|s| {
                        let mapped_s =
                            openjd_expr::path_mapping::apply_rules(&self.path_mapping_rules, s);
                        openjd_expr::ExprValue::new_path(mapped_s, *fmt)
                    })
                    .collect();
                openjd_expr::ExprValue::make_list(mapped, openjd_expr::ExprType::PATH).unwrap()
            }
            _ => value.clone(),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if !self.cleanup_called {
            log::warn!(
                target: "openjd.sessions",
                "Session '{}' was dropped without calling cleanup(). \
                 Working directory may not have been cleaned up.",
                self.session_id
            );
            if !self.retain_working_dir {
                let _ = std::fs::remove_dir_all(&self.working_directory);
            }
        }
    }
}
