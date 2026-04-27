# Session State Machine

## Overview

The `Session` struct in `session.rs` is the central type of the crate. It manages the
full lifecycle of an OpenJD session: creating a working directory, entering/exiting
environments, running tasks, tracking environment variable changes, and cleaning up.

## SessionState

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Ready,        // Can start any action
    Running,      // An action is in progress
    Canceling,    // Cancel requested, waiting for subprocess to exit
    ReadyEnding,  // Failed/canceled — only exit_environment allowed
    Ended,        // cleanup() called, session is done
}
```

### Transitions

```
Ready ──────────► Running         (enter_environment / exit_environment / run_task / run_subprocess)
Running ────────► Ready           (action succeeded, session not in ending-only mode)
Running ────────► ReadyEnding     (action failed/canceled, OR session is in ending-only mode)
Running ────────► Canceling       (cancel_action called)
Canceling ──────► Ready           (canceled action completes, session not ending)
Canceling ──────► ReadyEnding     (canceled action completes, session is ending)
ReadyEnding ───► Running          (exit_environment started)
Ready ──────────► Ended           (cleanup)
ReadyEnding ───► Ended            (cleanup)
```

### Brittle Sessions

After any action failure or cancelation, the session transitions to `ReadyEnding`. In
this state, only `exit_environment()` and `cleanup()` are allowed. This prevents running
new tasks or entering new environments after a failure, matching the Python library's
behavior.

The `ending_only` flag is set when:
- An action completes with `ActionState::Failed`, `Canceled`, or `Timeout`
- `exit_environment()` is called without `keep_session_running` (the default)

This design reflects the worker agent's pattern: after a task fails, the agent exits all
environments in reverse order and cleans up. There's no recovery path.

## SessionConfig

```rust
pub struct SessionConfig {
    pub session_id: String,
    pub job_parameter_values: JobParameterValues,
    pub path_mapping_rules: Option<Vec<PathMappingRule>>,
    pub retain_working_dir: bool,
    pub callback: Option<Box<dyn Fn(&str, &ActionStatus) + Send + Sync>>,
    pub os_env_vars: Option<HashMap<String, String>>,
    pub session_root_directory: Option<PathBuf>,
    pub user: Option<Arc<dyn SessionUser>>,
    pub revision_extensions: Option<ValidationContext>,
    pub cancel_token: Option<CancellationToken>,
    /// Whether to accumulate subprocess stdout into result strings.
    /// Intended for debugging only. Default is `false`.
    pub debug_collect_stdout: bool,
}
```

### Why `session_root_directory` instead of `working_directory`

The Python library accepts `session_root_directory` and creates the actual working
directory internally via `TempDir`. This is important because:

1. The session needs to control the directory name (random hex suffix for uniqueness)
2. The session needs to set permissions (0o700, or 0o770 for cross-user)
3. The session creates both a working directory and a files subdirectory within it

Accepting a pre-created directory would require the caller to know these details.

### Why `callback` is `Option<Box<dyn Fn>>` not a trait

The Python library uses `Callable[[str, ActionStatus], None]`. A boxed closure is the
closest Rust equivalent and avoids forcing consumers to define a named type. The callback
must be `Send + Sync` because it's invoked from the async task processing `ActionMessage`
values.

The callback fires on:
- Action start (state = Running)
- Every `openjd_*` directive (progress, status, fail)
- Action completion (state = Success/Failed/Canceled/Timeout)

The worker agent uses this to forward real-time progress to the Deadline service. The
callback must be fast — it runs inline with stdout processing, so delays block message
delivery.

### Why `cancel_token` is `Option<CancellationToken>`

The optional `cancel_token` provides external cancellation support. When provided, all
action cancel tokens are created as children of this token via `parent.child_token()`.
Canceling the parent cascades to all current and future actions in the session.

This enables the worker agent to cancel an entire session from outside the session's
async context (e.g., when the Deadline service sends a cancel request). Without this,
the agent would need to track and cancel each action individually.

## Session Construction

`Session::with_config(config: SessionConfig) -> Result<Self, SessionError>`:

1. Creates the session root directory via `custom_gettempdir()` if not provided
2. Creates the working directory via `TempDir::new()` with the session user for
   cross-user permissions
3. Creates the files subdirectory within the working directory
4. Validates sticky bit on parent directories (POSIX security check)
5. Logs host info (version, platform, architecture)
6. Stores job parameter values, path mapping rules, and other config for later use

A `Session::new_for_test(working_directory)` constructor is available when the
`test-utils` feature is enabled or in `#[cfg(test)]` builds. It creates a minimal
session with an empty session ID, no callback, no path mapping, and `debug_collect_stdout`
set to `true`. Production code should always use `Session::with_config`.

## Environment Management

### Enter

`enter_environment(env, resolved_symtab, identifier, os_env_vars)`:

1. Validates state is `Ready`
2. Sets state to `Running`
3. Resolves environment `variables` format strings against the current symbol table
4. Pushes the environment onto `environments_entered` (LIFO stack)
5. Creates an `EnvironmentScriptRunner` and calls `enter()` (runs `onEnter` action)
6. Processes `ActionMessage` values via `drive_action()` — env var changes from
   `openjd_env`/`openjd_unset_env` are applied to the environment's change set
7. On success: state → `Ready` (or `ReadyEnding` if `ending_only`)
8. On failure: state → `ReadyEnding`

### Exit

`exit_environment(identifier, resolved_symtab, keep_session_running, os_env_vars)`:

1. Validates state is `Ready` or `ReadyEnding`
2. Validates the identifier matches the most recently entered environment (LIFO order)
3. Sets `ending_only = true` unless `keep_session_running` is true
4. Runs `onExit` action via `EnvironmentScriptRunner::exit()`
5. Pops the environment from `environments_entered`
6. Removes the environment's variable changes from the cumulative set

### Why LIFO enforcement

Environments represent nested scopes — a step environment entered after a job environment
depends on the job environment's variables. Exiting out of order would leave the session
in an inconsistent state. The Python library enforces this, and the Rust crate matches.

## Task Execution

`run_task(script, task_parameter_values, resolved_symtab, os_env_vars)`:

1. Validates state is `Ready`
2. Sets state to `Running`
3. Adds `Task.Param.*` and `Task.RawParam.*` to the symbol table
4. Creates a `StepScriptRunner` and calls `run()` (runs `onRun` action)
5. Processes `ActionMessage` values via `drive_action()`
6. On completion: state → `Ready` or `ReadyEnding` based on result

## Ad-hoc Subprocess

```rust
pub async fn run_subprocess(
    &mut self,
    command: &str,
    args: Option<&[String]>,
    timeout: Option<Duration>,
    os_env_vars: Option<&HashMap<String, String>>,
    use_session_env_vars: bool,
    log_banner_message: Option<&str>,
) -> Result<SubprocessResult, SessionError>
```

Runs an arbitrary command within the session context without format string resolution,
embedded file materialization, or path mapping. Used by the worker agent for host
configuration scripts (install, sync, etc.).

### Validation

- State must be `Ready` (returns `InvalidState` otherwise)
- `command` must be non-empty (returns `Runtime` error)
- `timeout`, if provided, must be positive (returns `Runtime` error)

### Environment variable handling

The `use_session_env_vars` flag controls which env vars the subprocess inherits:

- `true`: Full session env vars — process env + `SessionConfig.os_env_vars` + per-action
  `os_env_vars` + cumulative `openjd_env`/`openjd_unset_env` changes from entered
  environments. This is the same env var set that `run_task` uses.
- `false`: Minimal env vars — only process env + `SessionConfig.os_env_vars` + per-action
  `os_env_vars`. No environment script changes are included.

### Cancelation

Always uses `CancelMethod::Terminate` (immediate kill). There is no
`NotifyThenTerminate` option for ad-hoc subprocesses.

### Cross-user routing

When a cross-user helper is active, `run_subprocess` routes through
`run_subprocess_via_helper` instead of spawning a new process directly. This avoids
the per-action `sudo -i` overhead.

### Differences from run_task

| Aspect | `run_task` | `run_subprocess` |
|--------|-----------|-----------------|
| Format strings | Resolved | Not resolved |
| Embedded files | Materialized | Not materialized |
| Path mapping | Materialized to JSON | Not materialized |
| Let bindings | Evaluated | Not evaluated |
| Cancel method | From action template | Always `Terminate` |
| State after failure | `ReadyEnding` | `ReadyEnding` |

## Path Mapping Rules

### extend_path_mapping_rules

```rust
pub fn extend_path_mapping_rules(&mut self, additional: Vec<PathMappingRule>)
```

Appends additional path mapping rules to the session's existing rules. Rules are
re-sorted by source path length (longest first) after extending, ensuring that more
specific rules take precedence over shorter ones.

Used by the worker agent when additional path mapping rules become available after
session creation (e.g., from job attachment manifests).

## Symbol Table Construction

```rust
pub fn build_symbol_table(
    &self,
    task_parameter_values: Option<&TaskParameterSet>,
    base: Option<&SerializedSymbolTable>,
) -> Result<SymbolTable, SessionError>
```

Populates a `SymbolTable` with:

| Symbol | Value | Source |
|--------|-------|--------|
| `Session.WorkingDirectory` | Working dir path (path-mapped) | Session creation |
| `Session.HasPathMappingRules` | `"true"` or `"false"` (bool with EXPR) | Path mapping config |
| `Session.PathMappingRulesFile` | Path to JSON rules file | `materialize_path_mapping()` |
| `Param.<name>` | Job parameter value (path-mapped for PATH types) | `SessionConfig.job_parameter_values` |
| `RawParam.<name>` | Job parameter value (no path mapping) | `SessionConfig.job_parameter_values` |
| `Task.Param.<name>` | Task parameter value (path-mapped) | `run_task()` call |
| `Task.RawParam.<name>` | Task parameter value (no path mapping) | `run_task()` call |

PATH-type parameters have path mapping rules applied. LIST_PATH parameters have rules
applied to each element. Values are stored as `ExprValue` (typed) when the EXPR extension
is active, or as strings for the base spec.

### Pre-resolved symbol table path

When the `base` parameter is provided (a `SerializedSymbolTable` from `create_job`), the
method skips building `Param.*`, `RawParam.*`, `Job.Name`, `Step.Name`, and let bindings
from scratch. Instead it deserializes the pre-resolved symbol table via
`base.to_symtab(PathFormat::host())`, which converts path values from the template's
storage format (Posix) to the host's native path format.

The worker agent uses this path when it receives a `resolved_symtab` from `create_job` —
the job creation phase has already resolved template-scope format strings, evaluated let
bindings, and populated `Job.Name`/`Step.Name`. The session then layers `Session.*` and
`Task.*` symbols on top.

PATH and LIST_PATH `Param.*` values are excluded from the base symtab — they are omitted
in template scope because their concrete values depend on session-time path mapping. The
session populates these from `self.job_parameter_values` with the session's path mapping
rules applied.

## Path Mapping Materialization

`materialize_path_mapping()` writes the path mapping rules to a JSON file in the working
directory using the `pathmapping-1.0` format. This file is referenced by
`Session.PathMappingRulesFile` in the symbol table, allowing actions to read and apply
path mapping rules themselves.

## Environment Variable Evaluation

`evaluate_env_vars()` computes the cumulative environment variable map:

1. Start with the process environment (`std::env::vars()`)
2. Apply `SessionConfig.os_env_vars` overrides
3. Apply per-action `os_env_vars` overrides
4. For each entered environment (in entry order):
   - Apply `set` changes (from environment `variables` + `openjd_env` directives)
   - Apply `unset` changes (from `openjd_unset_env` directives)

Unset takes precedence over set within the same environment, matching the spec.

## Cancelation

`cancel_action(time_limit, mark_action_failed)`:

1. Validates state is `Running`
2. Sets state to `Canceling`
3. Cancels the `CancellationToken` — the subprocess loop responds by sending the
   appropriate cancel command to the helper (`NOTIFY_THEN_TERMINATE` with a notify
   period for graceful cancel, or `TERMINATE` for immediate kill)
4. If `mark_action_failed` is true, the action result is `Failed` instead of `Canceled`

The `time_limit` parameter sets a deadline for the cancelation to complete. If the
subprocess doesn't exit within the limit, it's forcefully killed.

## Cleanup

See [Session Lifecycle Contract](#session-lifecycle-contract) for the full cleanup
and teardown guarantees.

`cleanup()` must be called after all environments have been exited. Calling `cleanup()`
while environments are still entered will remove the working directory but leave the
session in an inconsistent state — the environments' `onExit` scripts will not have run.

## Session Lifecycle Contract

A session manages a stack of environments. After construction, the caller can
enter environments, run tasks, and exit environments in any order — as long as
exits follow LIFO order (most recently entered first). A typical worker agent
pattern is: enter job envs → enter step envs → run tasks → exit step envs →
enter different step envs → run more tasks → exit all → cleanup. This section
collects the cleanup and teardown guarantees in one place.

### Explicit cleanup via `cleanup()`

`cleanup()` is the primary teardown API. It:
1. Shuts down the cross-user helper (if active) via the `"shutdown"` protocol message
2. For cross-user sessions: runs `sudo rm -rf` as the session user to remove files
   owned by that user, then `remove_dir_all` as the process user for any remainder
3. For same-user sessions: `remove_dir_all` on the working directory
4. Sets state to `Ended`
5. Skips directory removal if `retain_working_dir` is `true`

### Drop safety net

If `cleanup()` is never called, `Session`'s `Drop` impl:
1. Logs a warning ("Session::cleanup() was not called")
2. Performs a best-effort `remove_dir_all` on the working directory
3. Does **not** perform cross-user cleanup (no async, no sudo)
4. Does **not** shut down the cross-user helper
5. Skips removal if `retain_working_dir` is `true`

Callers should always call `cleanup()` explicitly. The `Drop` safety net exists only
to prevent leaked temp directories during development and debugging.

### retain_working_dir

When `SessionConfig.retain_working_dir` is `true`, both `cleanup()` and `Drop` skip
directory removal. The session still transitions to `Ended` state. This is used for
debugging — the operator can inspect the working directory contents after the session.

## drive_action

`drive_action()` is the core async loop that runs an action to completion:

```rust
async fn drive_action(&mut self, action_future, message_rx) -> ActionResult {
    tokio::pin!(action_future);
    loop {
        tokio::select! {
            result = &mut action_future => { /* drain remaining messages, return */ }
            Some(msg) = message_rx.recv() => { self.apply_message(msg); }
        }
    }
}
```

This runs the subprocess future concurrently with message processing. When the subprocess
exits, any remaining messages in the channel are drained before returning. The `&mut self`
borrow is safe because the subprocess runs in a separate future — only the channel
connects them.

### apply_message

Handles each `ActionMessage` variant:

| Message | Effect |
|---------|--------|
| `Progress(v)` | Updates `action_status.progress` |
| `Status(s)` | Updates `action_status.status_message` |
| `Fail(s)` | Updates `action_status.fail_message` |
| `SetEnv { name, value }` | Adds to current environment's change set |
| `UnsetEnv { name }` | Adds unset to current environment's change set |
| `RedactedEnv { name, value }` | Same as SetEnv + adds value to redaction set |
| `CancelMarkFailed` | Sets `action_fail_message`, triggers cancelation with `mark_action_failed = true` |

After each message, the user callback is invoked with the updated `ActionStatus`.


## Builder Methods

After constructing a `Session` via `with_config`, several builder methods configure
optional features before the first action:

### with_path_mapping

```rust
pub fn with_path_mapping(self, rules: Vec<PathMappingRule>) -> Self
```

Sets the session's path mapping rules. Rules are sorted by source path length
(longest first) so more specific rules take precedence. Called immediately after
construction when path mapping rules are known at session creation time.

### with_library

```rust
pub fn with_library(self, library: FunctionLibrary) -> Self
```

Sets the expression function library for format string evaluation. Required when
templates use the EXPR extension. Without this, format strings can only use simple
`{{Param.Name}}` references — function calls, arithmetic, and comprehensions will fail.

### with_revision_extensions

```rust
pub fn with_revision_extensions(self, ctx: ValidationContext) -> Self
```

Sets the specification revision and enabled extensions for the session. This controls:
- Whether `openjd_redacted_env` directives are processed (requires `REDACTED_ENV_VARS`
  extension or revision > v2023_09)
- Which features are available in format string evaluation

### get_enabled_extensions

```rust
pub fn get_enabled_extensions(&self) -> Vec<String>
```

Returns the sorted list of enabled extension names for this session. Returns an empty
vec if no `revision_extensions` were set. Used by the worker agent to report which
extensions are active.

## Public Accessors

Read-only accessors on `Session`:

| Accessor | Return type | Description |
|----------|-------------|-------------|
| `session_id()` | `&str` | The session's unique identifier |
| `state()` | `SessionState` | Current lifecycle state |
| `working_directory()` | `&Path` | Path to the session's working directory |
| `files_directory()` | `&Path` | Path to the embedded files subdirectory |
| `environments_entered()` | `&[EnvironmentIdentifier]` | Stack of entered environment IDs (entry order) |
| `path_mapping_rules()` | `&[PathMappingRule]` | Current path mapping rules (sorted by source path length, longest first) |
| `action_status()` | `Option<ActionStatus>` | Snapshot of the current/most-recent action status. `None` if no action has run. |
| `clone_cancel_writer()` | `Option<std::fs::File>` | Clone the cancel-info file writer for the cross-user helper. Used by PyO3 bindings to send cancel commands from a background thread. |
| `override_action_state(state)` | `()` | Override the action state. Used by PyO3 bindings to convert Failed → Canceled when cancel was requested externally. |

## enter_environment_with_output

```rust
pub async fn enter_environment_with_output(
    &mut self,
    env: &Environment,
    resolved_symtab: Option<&SerializedSymbolTable>,
    identifier: Option<&str>,
    os_env_vars: Option<&HashMap<String, String>>,
) -> Result<(String, String), SessionError>
```

Variant of `enter_environment` that returns both the environment identifier and the
captured stdout from the `onEnter` script as `(identifier, stdout)`. The regular
`enter_environment` wraps this and discards the stdout.

Used by the CLI's `run` command to display a unified output stream showing what each
environment and task printed. The worker agent uses the regular `enter_environment`
since it observes output through the real-time callback.

Note: stdout is only captured when `SessionConfig.debug_collect_stdout` is `true`.

## Redaction

### redact

```rust
pub fn redact(&self, text: &str) -> String
```

Replaces all known redacted values in `text` with `"********"`. Values are sorted by
length descending before replacement so longer matches take precedence over shorter
overlapping ones (e.g., `"FOOBAR"` is replaced before `"BAR"`).

The redacted value set is populated from `openjd_redacted_env` directives processed
during environment entry and task execution.

## Duplicate Environment Identifier Rejection

When `enter_environment` is called with an explicit `identifier` that has already been
entered in the session, it returns `SessionError::Runtime` with the message
`"Environment {id} has already been entered in this Session."`. This prevents
accidentally entering the same environment twice, which would corrupt the LIFO stack.

## Environment Variable Normalization

On Windows, environment variable names are case-insensitive. The internal
`normalize_env_key()` function uppercases all env var keys on Windows to prevent
mixed-case duplicates from causing undefined behavior in the Win32 API. On POSIX,
keys are passed through unchanged.

## debug_collect_stdout

`SessionConfig.debug_collect_stdout: bool` (default `false`) controls whether subprocess
output is accumulated into `SubprocessResult.stdout` / `ActionResult.stdout`. When
`false`, output is still streamed through the `ActionFilter` and callback in real time,
but the collected `String` stays empty — preventing unbounded memory growth.

Intended for debugging only. Production callers should leave this `false` and observe
output through the real-time callback.
