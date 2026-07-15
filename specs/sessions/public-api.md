# Public API

[README](README.md) · Public API

This document is the authoritative reference for the `openjd-sessions` crate's
public API. All public types, functions, traits, and constants are listed here
with their signatures, organized by module role.

The crate is the Rust-side runtime for executing OpenJD sessions as defined in
the [How Jobs Are Run][spec-how-run] specification: a session is a worker-host
environment created to run a set of tasks from the same job, with zero or more
named environments (with `onEnter` / `onExit` actions and environment variables)
entered in order at the start and exited in reverse LIFO order at the end.

A typical caller (e.g. the Deadline Cloud worker agent) flow is:

1. Build a [`SessionConfig`] with the job parameters, path-mapping rules, and
   (optionally) a cross-user [`SessionUser`].
2. `Session::with_config(config)?` to create the session.
3. For each job/step environment, `session.enter_environment(env, resolved_symtab, …).await?`.
4. For each task, `session.run_task(script, task_params, resolved_symtab, …).await?`.
5. For each environment in reverse LIFO order, `session.exit_environment(id, …).await?`.
6. `session.cleanup()` when done.

Real-time progress, status, and env-var updates surface through the
`SessionConfig.callback` closure, which is invoked on every `openjd_*`
directive emitted to stdout by a running action and on every action state
transition.

[spec-how-run]: https://github.com/OpenJobDescription/openjd-specifications/wiki/How-Jobs-Are-Run

## Module Structure

```
openjd_sessions                 — crate root (most public items re-exported here)
├── action                      — ActionState, ActionMessage, ActionResult
├── action_status               — ActionStatus
├── embedded_files              — EmbeddedFiles, EndOfLine re-export
├── error                       — SessionError
├── let_bindings                — re-exports openjd_model::evaluate_let_bindings
├── logging                     — LogContent, session_log!, banner helpers
├── path_mapping                — re-exported from openjd_expr
├── runner                      — CancelMethod, ScriptRunnerState, runner modules
│   ├── env_script              — EnvironmentScriptRunner
│   └── step_script             — StepScriptRunner
├── session                     — Session, SessionConfig, SessionState, EnvironmentIdentifier
├── session_user                — SessionUser trait, PosixSessionUser, WindowsSessionUser
├── tempdir                     — TempDir, StickyBitPolicy, openjd_temp_dir
├── win32 (Windows only)        — Windows helpers (mostly crate-internal; get_process_user is public)
└── win32_permissions (feature = "test-utils")
                                — DACL helpers for integration tests
```

SubprocessResult is re-exported at the crate root. The `subprocess`,
`action_filter`, `cross_user_helper`, `helper_binary`, `win32_locate`,
and (outside `test-utils`) `win32_permissions` modules are
crate-internal.

The `subprocess` module is crate-internal. Its result type
([`SubprocessResult`]) is re-exported at the crate root so it can
appear in public return types, but the entry-point function
(`run_subprocess`) and configuration struct (`SubprocessConfig`) are
not part of the public API. Callers drive subprocess execution via
[`Session::run_task`] or [`Session::run_subprocess`] rather than
calling subprocess primitives directly.

## Constants

```rust
/// Default notify period for `NOTIFY_THEN_TERMINATE` cancellation when
/// the action itself does not specify a `notifyPeriodInSeconds`.
/// Matches the OpenJD spec default.
pub const session::DEFAULT_CANCEL_NOTIFY_PERIOD_SECS: u64 = 5;
```

## Entry Points

### `Session`

The top-level type. A session owns a working directory, an optional
files directory, a symbol table, the entered-environments stack, and
(for cross-user work) a single long-lived helper subprocess.

```rust
pub struct Session { /* private fields */ }

impl Session {
    /// Primary constructor. Validates paths, creates a working directory
    /// (via a `TempDir` that gets cleaned up on drop), materializes
    /// path-mapping rules into a JSON file referenced by the
    /// `Session.PathMappingRulesFile` template variable, and — for
    /// cross-user sessions — spawns the embedded helper process.
    pub fn with_config(config: SessionConfig) -> Result<Self, SessionError>;

    // ── Accessors ──

    pub fn session_id(&self) -> &str;
    pub fn state(&self) -> SessionState;
    pub fn working_directory(&self) -> &Path;
    pub fn files_directory(&self) -> &Path;
    pub fn environments_entered(&self) -> &[EnvironmentIdentifier];
    pub fn job_parameter_values(&self) -> &JobParameterValues;
    pub fn path_mapping_rules(&self) -> &[PathMappingRule];

    /// Latest action status — `None` before the first action runs.
    /// The `Session` calls the `SessionConfig.callback` on every update;
    /// this method lets callers poll the status at will.
    pub fn action_status(&self) -> Option<ActionStatus>;

    // ── Lifecycle actions (async) ──

    /// Enter an environment (runs its `onEnter` action, if any).
    /// Returns the identifier used to later `exit_environment`.
    ///
    /// `resolved_symtab` is the filtered symbol table produced by
    /// `openjd_model::convert_environment_with_symtab`; it carries
    /// only the symbols this environment references.
    ///
    /// `os_env_vars` supplements the session-level env var map for
    /// this action only.
    pub async fn enter_environment(
        &mut self,
        env: &Environment,
        resolved_symtab: Option<&SerializedSymbolTable>,
        identifier: Option<&str>,
        os_env_vars: Option<&HashMap<String, String>>,
    ) -> Result<String, SessionError>;

    /// Same, but also returns the stdout captured from the onEnter
    /// script. Use when the caller needs to inspect or log the full
    /// output stream (e.g. openjd_cli's debug mode).
    pub async fn enter_environment_with_output(
        &mut self,
        env: &Environment,
        resolved_symtab: Option<&SerializedSymbolTable>,
        identifier: Option<&str>,
        os_env_vars: Option<&HashMap<String, String>>,
    ) -> Result<(String, String), SessionError>;

    /// Exit an environment. Must be called in LIFO order — the
    /// last-entered environment first. Fails with
    /// `SessionError::LifoViolation` if called out of order.
    ///
    /// `keep_session_running: true` keeps the session in the Ready
    /// state for subsequent `run_task` / `enter_environment` calls.
    /// `false` transitions the session to ReadyEnding after this
    /// exit completes, blocking further work.
    pub async fn exit_environment(
        &mut self,
        identifier: &EnvironmentIdentifier,
        resolved_symtab: Option<&SerializedSymbolTable>,
        keep_session_running: bool,
        os_env_vars: Option<&HashMap<String, String>>,
    ) -> Result<String, SessionError>;

    /// Run a step's `onRun` action.
    ///
    /// `task_parameter_values` supplies `Task.Param.*` / `Task.RawParam.*`
    /// for this task's symbol table. `resolved_symtab` is the step-scope
    /// symbol table from `Step::resolved_symtab` (the session layers
    /// Task.* and Session.* over this base).
    pub async fn run_task(
        &mut self,
        script: &StepScript,
        task_parameter_values: Option<&TaskParameterSet>,
        resolved_symtab: Option<&SerializedSymbolTable>,
        os_env_vars: Option<&HashMap<String, String>>,
    ) -> Result<ActionResult, SessionError>;

    /// Run an ad-hoc subprocess within the session's environment.
    /// Unlike `run_task`, does not evaluate format strings, materialize
    /// embedded files, or apply path mapping. Used by the worker agent
    /// for install/sync operations.
    pub async fn run_subprocess(
        &mut self,
        command: &str,
        args: Option<&[String]>,
        timeout: Option<Duration>,
        os_env_vars: Option<&HashMap<String, String>>,
        use_session_env_vars: bool,
        log_banner_message: Option<&str>,
    ) -> Result<SubprocessResult, SessionError>;

    // ── Cancellation and cleanup ──

    /// Cancel the currently running action. `time_limit` controls
    /// the cancel method: `Some(zero)` forces immediate terminate;
    /// `Some(d)` sends a notify followed by terminate after `d`;
    /// `None` uses the action's declared `notifyPeriodInSeconds` (or
    /// `DEFAULT_CANCEL_NOTIFY_PERIOD_SECS` if not specified).
    ///
    /// `mark_action_failed: true` reports a cancelled action as
    /// Failed rather than Canceled in the final `ActionResult`. This
    /// matches the spec's "openjd_fail from stdout → cancel + mark
    /// failed" behavior.
    pub fn cancel_action(
        &mut self,
        time_limit: Option<Duration>,
        mark_action_failed: bool,
    ) -> Result<(), SessionError>;

    /// Thread-safe handle for cancelling the running action while the
    /// `Session` value is owned by the thread driving it. Reusable for
    /// the life of the session; see session.md § SessionCancelHandle.
    pub fn cancel_handle(&self) -> SessionCancelHandle;

    /// Remove the working directory and associated temp files.
    /// Called automatically on drop as a safety net, but callers
    /// should call it explicitly at the end of a session to surface
    /// cleanup errors.
    pub fn cleanup(&mut self);

    // ── Builder-style configuration (post-construction) ──

    /// Replace the session's path-mapping rules. Rebuilds the derived
    /// `FunctionLibrary` used for evaluating `apply_path_mapping`.
    pub fn with_path_mapping(self, rules: Vec<PathMappingRule>) -> Self;

    /// Append to the existing rules (in-place).
    pub fn extend_path_mapping_rules(&mut self, additional: Vec<PathMappingRule>);

    /// Replace the session's profile (revision + extensions).
    /// Rebuilds the derived `FunctionLibrary`.
    pub fn with_profile(self, profile: openjd_model::ModelProfile) -> Self;

    /// The profile's enabled extensions as their spec-string names.
    pub fn get_enabled_extensions(&self) -> Vec<String>;

    // ── Miscellaneous ──

    /// Apply the session's redaction rules to a piece of text. Used
    /// by callers who generate log output themselves (rather than
    /// receiving it through the callback) and want to redact secrets.
    pub fn redact(&self, text: &str) -> String;

    /// Overwrite the current action's state — intended for the
    /// worker agent's mark-as-failed bookkeeping, not general use.
    pub fn override_action_state(&mut self, state: ActionState);

    /// Compute the cumulative env var map: process env + session
    /// env var changes (set by entered environments) + optional
    /// extras. Includes `OPENJD_SESSION_WORKING_DIR` automatically.
    pub fn evaluate_env_vars(
        &self,
        extra: Option<&HashMap<String, String>>,
    ) -> HashMap<String, Option<String>>;

    /// Build the action-time symbol table: Param.*, RawParam.*,
    /// Task.Param.*, Task.RawParam.*, Session.WorkingDirectory, and
    /// (if base is provided) everything already in the resolved
    /// step-scope symbol table.
    pub fn build_symbol_table(
        &self,
        task_parameter_values: Option<&TaskParameterSet>,
        base: Option<&SerializedSymbolTable>,
    ) -> Result<SymbolTable, SessionError>;
}
```

#### Test-only constructors

These are technically public but exist only for test instrumentation
(they allow building a `Session` that skips the full `SessionConfig`
pipeline). Production callers should use `with_config`.

```rust
impl Session {
    pub fn new_for_test(working_directory: PathBuf) -> Self;
    pub fn set_state_for_test(&mut self, state: SessionState);
    pub fn set_cancel_writer_for_test(&mut self, writer: std::fs::File);
    pub fn set_helper_auth_token_for_test(&mut self, token: String);
    pub fn clone_cancel_writer(&self) -> Option<std::fs::File>;
}
```

### `SessionConfig`

Every field is public so callers can build a config with struct syntax.
There is no constructor — callers fill in the fields they need and
accept defaults (via `Default` on the field types) for the rest.

```rust
pub struct SessionConfig {
    /// Opaque identifier used as a log correlation key. Not
    /// structurally meaningful to the session runtime.
    pub session_id: String,

    /// Resolved job parameter values. These become `Param.*` in the
    /// session's symbol table. Produce this via
    /// `openjd_model::preprocess_job_parameters`.
    pub job_parameter_values: JobParameterValues,

    /// Path-mapping rules from the spec's Path Mapping JSON object.
    /// Registered as `apply_path_mapping(...)` in the session's
    /// function library at construction time.
    pub path_mapping_rules: Option<Vec<PathMappingRule>>,

    /// If true, `cleanup()` skips the working-directory removal —
    /// useful for post-mortem debugging of a failed job.
    pub retain_working_dir: bool,

    /// Invoked on every action state transition (Running, Success,
    /// Failed, Canceled, Timeout) and on every `openjd_*` directive
    /// parsed from stdout. The first argument is the session_id; the
    /// second is the current ActionStatus.
    pub callback: Option<SessionCallbackType>,

    /// Extra environment variables applied to every action in the
    /// session. Layered under per-environment `openjd_env` changes
    /// and per-action `os_env_vars` overrides.
    pub os_env_vars: Option<HashMap<String, String>>,

    /// Parent directory under which the session working directory
    /// will be created. Defaults to the platform-specific OpenJD
    /// temp directory (see `openjd_temp_dir`).
    pub session_root_directory: Option<PathBuf>,

    /// Cross-user execution: when set, actions run as this user via
    /// the embedded cross-user helper process. See [Cross-User Execution](#cross-user-execution).
    pub user: Option<Arc<dyn SessionUser>>,

    /// Revision + extensions profile. Drives which expression
    /// functions are available (via
    /// `profile.to_expr_profile(HostContext::WithRules(rules))`) and
    /// whether redaction is active (REDACTED_ENV_VARS extension).
    /// `None` means "current revision, no extensions."
    pub profile: Option<openjd_model::ModelProfile>,

    /// External cancellation token. When cancelled, cancels the
    /// current action (if any) and prevents new actions from
    /// starting. Action cancel tokens are constructed as child
    /// tokens of this parent, so cancelling the parent cascades.
    pub cancel_token: Option<CancellationToken>,

    /// Policy for parent-directory sticky-bit checks (POSIX only;
    /// ignored on Windows). Default: `Strict` (fail-closed).
    pub sticky_bit_policy: StickyBitPolicy,

    /// Debug aid — accumulate subprocess stdout into result strings.
    /// Production callers should leave this false and observe
    /// output via the real-time callback instead.
    pub debug_collect_stdout: bool,

    /// Whether to echo `openjd_*` directive lines (e.g.
    /// `openjd_progress`, `openjd_status`, `openjd_env`,
    /// `openjd_redacted_env`, …) from subprocess stdout to the
    /// session log. Default: `true`, matching the Python
    /// `openjd-sessions` reference implementation.
    ///
    /// When `false`, recognised directives are still parsed and
    /// acted on (progress updates, env-var changes, redacted-value
    /// registration, …) but the directive lines themselves are
    /// filtered out of the log stream.
    ///
    /// **Redaction interaction**: regardless of this flag, values
    /// from `openjd_redacted_env` directives are added to the
    /// session's redaction set *before* the originating line is
    /// considered for pass-through. So when
    /// `echo_openjd_directives = true`, the directive line that
    /// introduces a secret is logged in its already-redacted form
    /// (`NAME=********`); subsequent occurrences of the secret in
    /// any log line are also redacted. Setting this flag to
    /// `false` does not improve security — it just removes the
    /// directive lines from operator-facing output.
    pub echo_openjd_directives: bool,
}
```

#### `SessionCallbackType`

```rust
pub type SessionCallbackType =
    Box<dyn Fn(&str, &ActionStatus) + Send + Sync>;
```

Called with `(session_id, action_status)` on every state transition
and every parsed directive. Must be cheap and non-blocking — it runs
on the session's action-driving task. The Deadline Cloud worker agent
uses it to forward updates to the control plane.

### `SessionState`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Can start new actions or enter environments.
    Ready,
    /// An action is running.
    Running,
    /// Cancellation has been requested; the subprocess is being torn down.
    Canceling,
    /// Environments can still be exited but no new actions can start.
    ReadyEnding,
    /// All environments exited; `cleanup()` may be called.
    Ended,
}
```

Displays as the SCREAMING_SNAKE_CASE spec name (`Ready` → `"READY"`,
etc.) for interop with the Python reference implementation.

### `EnvironmentIdentifier`

```rust
pub type EnvironmentIdentifier = String;
```

Opaque identifier used to refer to an entered environment. Returned
by `enter_environment`, consumed by `exit_environment`. The caller may
supply its own identifier string or let the session auto-generate one.

## Action Types

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionState {
    Running,
    Success,
    Failed,
    Canceled,
    Timeout,
}

/// Parsed `openjd_*` directive from a running action's stdout.
#[derive(Debug, Clone)]
pub enum ActionMessage {
    /// `openjd_progress: <number>`
    Progress(f64),
    /// `openjd_status: <message>`
    Status(String),
    /// `openjd_fail: <message>`
    Fail(String),
    /// `openjd_env: <var>=<value>` — only valid in onEnter actions.
    SetEnv { name: String, value: String },
    /// `openjd_unset_env: <var>` — only valid in onEnter actions.
    UnsetEnv { name: String },
    /// `openjd_redacted_env: <var>=<value>` — value added to the
    /// redaction set, env var set. REDACTED_ENV_VARS extension.
    RedactedEnv { name: String, value: String },
    /// Internal signal (emitted from ActionFilter when a malformed
    /// env-directive is detected): cancel this action and mark it
    /// failed.
    CancelMarkFailed { fail_message: String },
}

/// Result of a completed action. `stdout` is non-empty only when
/// `SessionConfig.debug_collect_stdout` is true; otherwise output is
/// observable through the callback only.
#[derive(Debug)]
pub struct ActionResult {
    pub state: ActionState,
    pub exit_code: Option<i32>,
    pub stdout: String,
}

#[derive(Debug, Clone)]
pub struct ActionStatus {
    pub state: ActionState,
    pub progress: Option<f64>,
    pub status_message: Option<String>,
    pub fail_message: Option<String>,
    pub exit_code: Option<i32>,
    pub started_at: Option<SystemTime>,
    pub ended_at: Option<SystemTime>,
}
```

All of `ActionState`, `ActionMessage`, and `ActionStatus` implement
`Display` / `Debug`. `ActionStatus` also implements `Default`.

## Subprocess Results

```rust
/// Result of running a subprocess action.
#[derive(Debug)]
pub struct SubprocessResult {
    pub state: ActionState,
    pub exit_code: Option<i32>,
    /// Accumulated stdout. Empty unless
    /// `SessionConfig.debug_collect_stdout` was true.
    pub stdout: String,
}
```

`SubprocessConfig` and `run_subprocess` are **not** part of the public
API — they are crate-internal and subject to change. Callers drive
subprocesses via `Session::run_task` / `Session::run_subprocess`.

## Runners

Two runners exist, one per action kind. Both are primarily internal
(the session calls them), but they are exposed so callers that need
to drive a single action without a full session can do so — for
example, `openjd-cli`'s unit tests and tools that debug a single
action against a mock session.

### `EnvironmentScriptRunner`

```rust
pub struct EnvironmentScriptRunner { /* private fields */ }

impl EnvironmentScriptRunner {
    pub fn new(
        session_id: &str,
        working_directory: PathBuf,
        files_directory: PathBuf,
        user: Option<Arc<dyn SessionUser>>,
    ) -> Self;

    // Builder-style configuration
    pub fn with_redactions(self, enabled: bool) -> Self;
    pub fn with_debug_collect_stdout(self, collect: bool) -> Self;
    pub fn with_initial_redacted_values(self, values: Vec<String>) -> Self;
    pub fn with_cancel_token(self, token: CancellationToken) -> Self;
    pub fn with_cancel_request_rx(
        self,
        rx: tokio::sync::watch::Receiver<Option<Duration>>,
    ) -> Self;

    /// Run the environment's onEnter action. Returns immediately if
    /// the environment has no `onEnter`.
    pub async fn enter(
        &mut self,
        env: &Environment,
        symtab: &SymbolTable,
        library: Option<&FunctionLibrary>,
        env_vars: &HashMap<String, Option<String>>,
        message_tx: mpsc::UnboundedSender<ActionMessage>,
    ) -> Result<SubprocessResult, SessionError>;

    /// Run the environment's onExit action. Enforces a default timeout
    /// of 5 minutes if `action.timeout` is not specified.
    pub async fn exit(
        &mut self,
        env: &Environment,
        symtab: &SymbolTable,
        library: Option<&FunctionLibrary>,
        env_vars: &HashMap<String, Option<String>>,
        message_tx: mpsc::UnboundedSender<ActionMessage>,
    ) -> Result<SubprocessResult, SessionError>;

    pub fn cancel(&self);
    pub fn state(&self) -> ScriptRunnerState;
}
```

### `StepScriptRunner`

```rust
pub struct StepScriptRunner { /* private fields */ }

impl StepScriptRunner {
    pub fn new(
        session_id: &str,
        working_directory: PathBuf,
        files_directory: PathBuf,
        user: Option<Arc<dyn SessionUser>>,
    ) -> Self;

    // Same builder methods as EnvironmentScriptRunner
    pub fn with_redactions(self, enabled: bool) -> Self;
    pub fn with_debug_collect_stdout(self, collect: bool) -> Self;
    pub fn with_initial_redacted_values(self, values: Vec<String>) -> Self;
    pub fn with_cancel_token(self, token: CancellationToken) -> Self;
    pub fn with_cancel_request_rx(
        self,
        rx: tokio::sync::watch::Receiver<Option<Duration>>,
    ) -> Self;

    /// Evaluate let bindings, materialize embedded files, then run
    /// the `onRun` action.
    pub async fn run(
        &mut self,
        script: &StepScript,
        symtab: &SymbolTable,
        library: Option<&FunctionLibrary>,
        env_vars: &HashMap<String, Option<String>>,
        message_tx: mpsc::UnboundedSender<ActionMessage>,
    ) -> Result<SubprocessResult, SessionError>;

    pub fn cancel(&self);
    pub fn state(&self) -> ScriptRunnerState;
}
```

### `ScriptRunnerState`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptRunnerState {
    Ready, Running, Canceling, Canceled, Timeout, Failed, Success,
}
```

Implements `Display`.

### `CancelMethod`

```rust
#[derive(Debug, Clone)]
pub enum CancelMethod {
    /// Immediately terminate via SIGKILL / TerminateProcess.
    Terminate,
    /// Send SIGTERM / CTRL_BREAK_EVENT, wait for grace period, then
    /// SIGKILL / TerminateProcess.
    NotifyThenTerminate { terminate_delay: Duration },
}
```

The session derives this from an action's `cancelation` field per
spec §5.3. Displays as `"Terminate"` or `"NotifyThenTerminate(30s)"`.

## Session User (Cross-User Execution)

```rust
pub trait SessionUser: Send + Sync + std::fmt::Debug {
    fn user(&self) -> &str;
    fn group(&self) -> &str;
    fn is_process_user(&self) -> bool;
    fn as_any(&self) -> &dyn std::any::Any;
}
```

Implementations are platform-specific.

### POSIX: `PosixSessionUser`

```rust
#[cfg(unix)]
#[derive(Debug, Clone)]
pub struct PosixSessionUser {
    pub user: String,
    pub group: String,
}

#[cfg(unix)]
impl PosixSessionUser {
    /// Create a PosixSessionUser. If `group` is None, defaults to
    /// the current process's effective group.
    pub fn new(user: &str, group: Option<&str>) -> Self;
}
```

### Windows: `WindowsSessionUser`

Two authentication modes:

- **Password mode** (non-Session 0): `WindowsSessionUser::with_password(user, password)`.
  Credentials are validated immediately via `LogonUserW`.
- **Logon token mode** (Session 0, services, SSH): `WindowsSessionUser::with_logon_token(user, token)`.
  Used when the caller already has a logon token from the OS.
- **Same-user** (no credentials needed): `WindowsSessionUser::for_process_user()`.

```rust
#[cfg(windows)]
pub struct WindowsSessionUser { /* private fields */ }

#[cfg(windows)]
impl WindowsSessionUser {
    pub fn for_process_user() -> Result<Self, String>;

    pub fn with_password(
        user: &str, password: &str,
    ) -> Result<Self, BadCredentialsError>;

    pub fn with_logon_token(
        user: &str,
        token: windows::Win32::Foundation::HANDLE,
    ) -> Result<Self, String>;

    pub fn password(&self) -> Option<&str>;
    pub fn logon_token(&self) -> Option<windows::Win32::Foundation::HANDLE>;
}

#[cfg(windows)]
#[derive(Debug, thiserror::Error)]
pub enum BadCredentialsError {
    #[error("The username or password is incorrect.")]
    LogonFailure,
    #[error("{0}")]
    Other(String),
}
```

`WindowsSessionUser` is `Send + Sync` via explicit `unsafe impl` —
the `HANDLE` it carries is a kernel object that is process-wide and
safe to share across threads, even though `windows-rs` marks `HANDLE`
as `!Send` by convention. See the source for the full safety
argument.

## Embedded Files

```rust
pub mod embedded_files {
    /// Scope for embedded file symbol table entries — controls the
    /// variable prefix (`Task.File.*` vs `Env.File.*`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum EmbeddedFilesScope { Step, Env }

    impl EmbeddedFilesScope {
        pub fn prefix(&self) -> &'static str;  // "Task.File" or "Env.File"
    }

    /// Re-exported from openjd_model::types::EndOfLine.
    pub use openjd_model::types::EndOfLine;

    /// Convert line endings in data based on the specified mode.
    /// `EndOfLine::Auto` maps to `Crlf` on Windows, `Lf` elsewhere.
    pub fn convert_line_endings(data: &str, eol: EndOfLine) -> Vec<u8>;

    /// Write raw content to a file, creating parent directories as
    /// needed. On POSIX, sets permissions to 0o600.
    pub fn write_embedded_file(
        path: &Path, data: &str,
    ) -> Result<(), std::io::Error>;

    /// Write with `runnable` (0o700 on POSIX) and `end_of_line` options.
    pub fn write_embedded_file_with_options(
        path: &Path,
        data: &str,
        runnable: bool,
        end_of_line: Option<EndOfLine>,
    ) -> Result<(), std::io::Error>;

    /// Adjust ownership/permissions on a file for cross-user access
    /// (POSIX: chown to user's group + 0o660/0o770; Windows: set DACL
    /// granting the session user Modify access).
    pub fn chown_for_user(
        path: &Path,
        user: &dyn SessionUser,
        runnable: bool,
    ) -> Result<(), SessionError>;

    /// Compute the symbol table key for an embedded file:
    /// `{scope.prefix()}.{name}`.
    pub fn symtab_key(scope: EmbeddedFilesScope, name: &str) -> String;

    /// Two-phase embedded file materializer.
    ///
    /// Phase 1: `allocate_file_paths()` — allocates paths and sets
    /// `Task.File.X` / `Env.File.X` entries in the symbol table.
    /// This must run before let bindings evaluate, so the bindings
    /// can reference the file paths.
    ///
    /// Phase 2: `write_file_contents()` — evaluates each file's
    /// `data` format string (which may reference let bindings) and
    /// writes the content to disk.
    pub struct EmbeddedFiles { /* private fields */ }

    impl EmbeddedFiles {
        pub fn new(
            scope: EmbeddedFilesScope,
            session_files_directory: PathBuf,
            session_id: &str,
        ) -> Self;

        pub fn with_user(self, user: Option<Arc<dyn SessionUser>>) -> Self;

        pub fn allocate_file_paths(
            &mut self,
            files: &[EmbeddedFile],
            symtab: &mut SymbolTable,
        ) -> Result<(), SessionError>;

        pub fn write_file_contents(
            &self,
            symtab: &SymbolTable,
            library: Option<&FunctionLibrary>,
        ) -> Result<(), SessionError>;
    }
}
```

## Let Bindings

```rust
pub mod let_bindings {
    /// Re-exported from openjd_model. Evaluates `"name = expr"`
    /// bindings in order against a base symbol table and returns an
    /// extended table with the new entries.
    pub use openjd_model::evaluate_let_bindings;
}
```

## Temp Directory

```rust
/// Platform-specific base dir for OpenJD session working directories:
/// Returns the OpenJD temp directory, creating it if needed.
///
/// `base_dir` is the OpenJD directory itself.
/// - `None` → derive from the environment:
///   - POSIX: `<std::env::temp_dir()>/OpenJD` (typically `/tmp/OpenJD`).
///   - Windows: `%PROGRAMDATA%\Amazon\OpenJD` (with a warning and fallback to
///     `C:\ProgramData\Amazon\OpenJD` if `PROGRAMDATA` is unset).
/// - `Some(p)` → use `p` directly.
///
/// Tests should pass `Some(...)` to avoid mutating process-global env vars.
pub fn tempdir::openjd_temp_dir(base_dir: Option<&Path>) -> Result<PathBuf, SessionError>;

/// Walk ancestors of `root_dir` looking for a world-writable directory
/// missing the sticky bit — a security risk that lets other users
/// rename or delete the session's files. Returns the first offending
/// path, or `None` if all ancestors are safe.
#[cfg(unix)]
pub fn tempdir::find_missing_sticky_bit(root_dir: &Path) -> Option<PathBuf>;

/// Sticky-bit policy: what to do if `find_missing_sticky_bit` returns
/// a path when creating a session working directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StickyBitPolicy {
    #[default]
    /// Refuse to create the session. Default.
    Strict,
    /// Log a warning but proceed.
    Warn,
    /// Skip the check entirely.
    Disabled,
}

/// RAII temp directory. Removes the directory on `cleanup()` — or,
/// best-effort, on drop if `cleanup()` was not called explicitly.
pub struct TempDir { /* private fields */ }

impl TempDir {
    /// `dir`: parent directory (defaults to `openjd_temp_dir(None)`).
    /// `prefix`: name prefix for the new subdirectory.
    /// `user`: if cross-user, sets group ownership and 0o770 perms.
    pub fn new(
        dir: Option<&Path>,
        prefix: Option<&str>,
        user: Option<&dyn SessionUser>,
    ) -> Result<Self, SessionError>;

    pub fn path(&self) -> &Path;

    pub fn cleanup(&mut self) -> Result<(), SessionError>;
}

impl AsRef<Path> for TempDir { /* ... */ }
impl Drop for TempDir { /* best-effort cleanup */ }
impl std::fmt::Debug for TempDir { /* ... */ }
```

## Logging

```rust
/// Bitflag set describing the nature of a log record. The worker agent
/// routes records by content: only EXCEPTION_INFO | PROCESS_CONTROL |
/// HOST_INFO go to the worker log; all records go to CloudWatch via
/// the session log stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogContent: u32;

impl LogContent {
    pub const BANNER: LogContent          = 1 << 0;
    pub const FILE_PATH: LogContent       = 1 << 1;
    pub const FILE_CONTENTS: LogContent   = 1 << 2;
    pub const COMMAND_OUTPUT: LogContent  = 1 << 3;
    pub const EXCEPTION_INFO: LogContent  = 1 << 4;
    pub const PROCESS_CONTROL: LogContent = 1 << 5;
    pub const PARAMETER_INFO: LogContent  = 1 << 6;
    pub const HOST_INFO: LogContent       = 1 << 7;
}
```

Implements `log::kv::ToValue` so it can be attached to log records as
the `openjd_log_content` key.

```rust
/// Return the current time as microseconds since the Unix epoch.
/// Used by `session_log!` as the `openjd_timestamp_usec` value.
pub fn logging::timestamp_usec() -> u64;

/// Log a major section banner (surrounded by `=` lines).
pub fn logging::log_section_banner(session_id: &str, title: &str);

/// Log a minor section banner (surrounded by `-` lines).
pub fn logging::log_subsection_banner(session_id: &str, title: &str);
```

```rust
/// Emit a structured log record with session_id, openjd_log_content,
/// and a precise timestamp. Thin wrapper over `log::<level>!` that
/// fills in the structured kv fields openjd-sessions consumers expect.
///
/// Usage:
///   session_log!(info, session_id, LogContent::HOST_INFO, "message {}", arg);
#[macro_export]
macro_rules! session_log { /* ... */ }
```

## Error Type

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SessionError {
    /// Action requested from the wrong state (e.g. run_task in Ended).
    InvalidState { expected: Vec<SessionState>, current: SessionState },

    /// An onEnter/onExit script failed.
    EnvironmentScriptFailed { name: String, action: String, reason: String },

    /// Failed to resolve a format string.
    FormatString { context: String, reason: String },

    /// Failed to write an embedded file.
    EmbeddedFile { name: String, source: std::io::Error },

    /// Embedded file's `filename` field resolved to an unsafe value
    /// (path separators, null bytes, `.`/`..`). Defense-in-depth:
    /// the model layer already rejects these at template validation
    /// time per spec §6.1.1, this is a second barrier before the
    /// filename joins the target directory.
    EmbeddedFilePath { name: String, filename: String, reason: String },

    /// Working directory creation failed.
    WorkingDirectory { path: PathBuf, source: std::io::Error },

    /// Subprocess couldn't be spawned.
    SubprocessStart { command: String, source: std::io::Error },

    /// Temp directory creation/cleanup failed.
    TempDir { path: PathBuf, source: std::io::Error },

    /// Generic runtime error with a human-readable message.
    Runtime(String),

    /// `enter_environment` called with an identifier already in use.
    DuplicateEnvironment { id: String },

    /// Referenced an environment identifier not in the session.
    UnknownEnvironment { identifier: String },

    /// chown/chmod/DACL failure (cross-user file setup).
    PathPermissions { path: String, reason: String },

    /// Cross-user helper IPC failed.
    HelperCommunication(String),

    /// Tried to exit an environment that wasn't the most recently
    /// entered one.
    LifoViolation { expected: String, got: String },
}
```

## Re-exports

```rust
pub use openjd_expr::path_mapping;           // the module
pub use openjd_expr::path_mapping::PathFormat;
pub use openjd_expr::path_mapping::PathMappingRule;
```

`PathFormat` and `PathMappingRule` are the types used on
`SessionConfig.path_mapping_rules`. They are re-exported here so
downstream callers don't need to depend on `openjd-expr` directly.

## Cross-User Execution

The crate supports running actions as a different OS user. This is
backed by an embedded cross-user helper binary:

- POSIX: a single `sudo -i <helper>` is spawned at session creation.
  Subsequent actions are dispatched through the helper's stdin/stdout
  IPC, avoiding the ~1s sudo startup cost per action. Cancellation
  flows through a separate `cancel_writer` stdin FD.
- Windows: similar design using `CreateProcessWithLogonW` or
  `CreateProcessAsUserW` depending on which `WindowsSessionUser` mode
  is in use.

The helper binary itself is not part of the public API — it's
embedded at build time by `build.rs`. The public surface is just the
`SessionUser` trait and its implementations.

## Platform Support

Primary target: POSIX / Linux (the main Deadline Cloud worker
platform). Windows support covers same-user execution, cross-user
execution with both password and logon-token `WindowsSessionUser`
modes, temp dir and embedded file permissions via ACLs, and process
tree kill via `CreateToolhelp32Snapshot`. A Windows-specific
`win32::get_process_user()` helper is public for callers that need
to identify the current process's user name; most of the
`win32` / `win32_permissions` module is crate-internal.

Cross-user integration tests run under Docker on Linux CI and with a
temporary test user on Windows CI. Both platforms are exercised by
the workspace's clippy, build, and test jobs.

## Versioning and Stability Conventions

This crate tracks revision 2023-09 of the OpenJD specification.
Revision gating lives in `ModelProfile` (from `openjd-model`), which
is passed in via `SessionConfig.profile` and used to derive the
expression-language profile for each action.

Enums that are `#[non_exhaustive]`:

- `SessionError`

Enums that are intentionally closed (their variant sets map to
finite, well-defined concepts):

- `SessionState`, `ActionState`, `ScriptRunnerState` (state machine
  shapes)
- `CancelMethod` (spec §5.3 defines exactly two cancellation modes)
- `StickyBitPolicy` (a three-valued policy with no room to grow)
- `LogContent` (a `bitflags` type — new variants added at the
  bit-flag level require a new constant, which is a minor/patch
  bump)
- `EmbeddedFilesScope` (step vs env — and spec §6 defines no other
  embedded-file scopes)

`ActionMessage` is not `#[non_exhaustive]` today but probably should
be — `openjd_*` directives are an open-ended spec-extension point.
