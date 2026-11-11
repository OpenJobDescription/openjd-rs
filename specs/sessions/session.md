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
    ReadyEnding,  // Ending-only — only exit_environment, cleanup and clear_ending_only allowed
    Ended,        // cleanup() called, session is done
}
```

`Session` never stores `ReadyEnding`. Its private `state` field is a `StoredState`, which
has only `Ready`, `Running`, `Canceling` and `Ended`, and ending-only mode lives only in
the private `ending_only` flag. `Session::state()` derives `ReadyEnding`: it reports a
stored `Ready` as `ReadyEnding` while `ending_only` is set, and reports every other state
as stored. The flag is the one source of truth for ending-only mode, so the state and
the flag can't disagree. The flag has to exist anyway, because it is what remembers
ending-only mode through `Running` and `Canceling`. Every state check in `Session`, and
the `current` state in every `SessionError::InvalidState`, uses `state()`.

### Transitions

```
Ready ──────────► Running         (enter_environment / exit_environment / run_task / run_subprocess)
Running ────────► Ready           (action succeeded, session not in ending-only mode)
Running ────────► ReadyEnding     (action failed/canceled/timed out, OR session is in ending-only mode)
Running ────────► Canceling       (cancel_action called)
Canceling ──────► Ready           (action finished Success before the cancel reached it, session not in ending-only mode)
Canceling ──────► ReadyEnding     (action ended Canceled/Failed/Timeout, OR session is in ending-only mode)
Ready ──────────► ReadyEnding     (enter_environment / exit_environment failed building the symbol table or resolving variables, before Running)
ReadyEnding ───► Running          (exit_environment started)
ReadyEnding ───► Ready            (clear_ending_only)
Ready ──────────► Ended           (cleanup)
ReadyEnding ───► Ended            (cleanup)
```

These are the states `state()` reports. Going to `ReadyEnding` stores `Ready` with
`ending_only` set, and `ReadyEnding ───► Ready` clears the flag and leaves the stored
state alone. The per-operation steps in the rest of this document use the same
convention: "state" means what `state()` reports, so "state → `ReadyEnding`" means the
stored state becomes `Ready` with `ending_only` set.

### Brittle Sessions

After any action failure or cancelation, the session transitions to `ReadyEnding`. In
this state, only `exit_environment()`, `cleanup()` and `clear_ending_only()` are allowed.
This prevents running new tasks or entering new environments after a failure unless the
caller explicitly asks for it. The Python library follows the same rule, except in the
case described in
[architecture.md § Python-vs-Rust Design Comparison](architecture.md#python-vs-rust-design-comparison).

The `ending_only` flag is set when:
- An action completes with `ActionState::Failed`, `Canceled`, or `Timeout`. This covers
  every way an action can end that way: a non-zero exit or `openjd_fail`, a cancel
  (including one with `mark_action_failed`, which reports `Failed`), a timeout, a runner
  error such as a command that can't be started, a failure while setting up the action
  before its subprocess starts, and the same outcomes for `run_subprocess` through the
  cross-user helper
- `exit_environment()` is called without `keep_session_running` (the default)

`keep_session_running = true` means only "don't make the session ending-only on this
exit". An `onExit` that ends `Failed`, `Canceled` or `Timeout` still makes it ending-only,
like any other action, and
`keep_session_running` never clears `ending_only`. When an action ends, the stored state
goes back to `Ready`, and `state()` reports `ReadyEnding` if `ending_only` is set and
`Ready` otherwise, so `keep_session_running` can't bring an ending-only session back to
`Ready`. All of these paths settle the state through one private helper,
`settle_after_action`, which sets the flag for a `Failed`, `Canceled` or `Timeout` action
and then stores `Ready`. Because `ReadyEnding` is derived from the flag rather than
stored beside it, `state() == Ready` means the session isn't ending-only, and the `Ready`
checks in `run_task`, `enter_environment` and `run_subprocess` enforce the contract
without checking `ending_only` themselves.

This design reflects the worker agent's pattern: after a task fails, the agent exits all
environments in reverse order and cleans up.

The only way back to `Ready` is an explicit call to the public method
`clear_ending_only()`:

- It's allowed only in `ReadyEnding`. In any other state (`Ready`, `Running`, `Canceling`,
  `Ended`) it returns `SessionError::InvalidState` with `expected: [ReadyEnding]` and
  changes nothing.
- It clears `ending_only` whatever set it (a failed, canceled or timed-out action, or a
  default `exit_environment`). That is all it changes: the stored state is already
  `Ready`, so `state()` then reports `Ready`.
- It runs no action and changes nothing else: the entered environments, the last action's
  status and the session's environment variables stay as they were. A later failure
  makes the session ending-only again.
- It refuses, and changes nothing, when the session-wide
  `SessionConfig::cancel_token` exists and has been canceled: it returns
  `SessionError::SessionCanceled` and leaves the session `ReadyEnding`. That cancellation
  is permanent — a tokio token can't be reset — so every later action would end `Canceled`
  and make the session ending-only again; `clear_ending_only()` won't return the session to
  `Ready` under a cancellation it can't undo, so what the method does matches what it says.
  Only `exit_environment()` and `cleanup()` remain. A per-action cancel (`cancel_action()`
  or a `SessionCancelHandle`) does not block it — only the session-wide token does.

It's for callers that deliberately want to keep running work after a failure or an
ending exit, and they take responsibility for the session's state. A failed or canceled
action can leave its work half done, and the session neither checks nor undoes it.

### Failures before an action starts

`enter_environment`, `exit_environment` and `run_task` first check that the call can be
accepted. A call that fails one of these checks is **rejected**: it returns the error and
changes nothing (state, entered environments, the last action's status), and no callback
fires. The rejections are:

| Call | Rejections |
|------|------------|
| `enter_environment` | `InvalidState` (not `Ready`), `DuplicateEnvironment` (an explicit `identifier` already entered), `MultipleWrapEnvironments` (RFC 0008 single wrap layer) |
| `exit_environment` | `InvalidState` (not `Ready` or `ReadyEnding`), `UnknownEnvironment`, `LifoViolation` |
| `run_task` | `InvalidState` (not `Ready`) |
| `run_subprocess` | `InvalidState` (not `Ready`), and `Runtime` for an empty command or a zero timeout |

These are the checks the Python library also makes before it touches the session, and
they run before anything that could fail an action, so a rejected call is rejected even
if its other input would also fail.

Once a call is accepted, any failure before its action's subprocess starts **fails the
action**, as in the Python library (`Session._fail_action_before_start`). The action
status becomes `Failed`, with the error's message as its `fail_message` and no exit code;
the callback reports it; and the session settles through `settle_after_action(Failed)`,
so it is ending-only (`ReadyEnding`). Two of these run before the action is reported
`Running`, so they occur even for an environment with no `onEnter`/`onExit`: building the
symbol table (an undeserializable `resolved_symtab`, a job parameter value that can't be
coerced to its declared type, a `PATH` or `LIST[PATH]` value over the path mapping limit)
and resolving an environment's `variables`. The rest run after `Running`: writing the path mapping rules file and resolving or
writing an RFC 0008 wrap hook's symbols and embedded files (in the `onEnter`/`onExit`
action branch), and, in `run_task` only, building the symbol table and coercing task
parameter values (`run_task` reports `Running` before it builds the table). (The Python
runtime writes the path mapping rules file unconditionally, so it can fail that write for
an environment with no `onEnter`/`onExit` where Rust does not — a pre-existing
divergence.) Failures after the subprocess is dispatched,
such as an embedded file that can't be written or a command that can't be started, were
already failed actions (see [drive_action](#drive_action)).

The call still returns the error. That is how every failed action in
`enter_environment` and `exit_environment` already reports itself
(`EnvironmentScriptFailed` for a failed `onEnter`/`onExit`), and how `run_task` already
reports a setup failure. The Python methods are non-blocking and report every outcome
through `action_status`, so they return normally instead (`enter_environment` returns the
identifier). A Rust caller that reads `action_status()` sees what a Python caller sees. To
tell a rejection from a failed action, check `state()` or `action_status()`, or the
variant: only the rejection variants above leave the session unchanged.

A failed `enter_environment` leaves the environment on `environments_entered`, as in
Python, and the caller must still exit it. When the caller didn't pass an `identifier`,
the environment is `environments_entered().last()`. A failed `exit_environment` has
already removed the environment, so it still counts as an exit.

Which callbacks a pre-start failure fires depends on when it happens relative to the
`Running` notification. `enter_environment` and `exit_environment` report only a `Failed`
callback for a failure while building the symbol table, and `enter_environment` also for
resolving an environment's `variables` — these run before the action is reported
`Running`. Their later setup steps (writing the path mapping rules file, resolving or
writing an RFC 0008 wrap hook's symbols and embedded files) run after `Running`, so they
report `Running` and then `Failed`. `run_task` reports `Running` and then `Failed` for
every pre-start failure, because it reports `Running` before it builds the symbol table.
The terminal outcome is the same in every case, and the Python runtime reports only the
`Failed` callback for all of them. The `fail_message` is the error's own text — Rust's
wording, not identical to the Python runtime's for the same input.

When the resolved action runs and ends non-`Success` (a failed, canceled or timed-out
`onEnter`/`onExit`), `enter_environment` and `exit_environment` return
`SessionError::EnvironmentScriptFailed`, and the action status is the script's own
terminal state rather than `Failed` with an error message. The session is ending-only
either way.

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
    /// Whether to echo `openjd_*` directive lines (e.g. `openjd_progress`,
    /// `openjd_status`, `openjd_env`, `openjd_redacted_env`, …) from
    /// subprocess stdout to the session log. Default: `true`, matching
    /// the Python reference implementation. See [`echo_openjd_directives`](#echo_openjd_directives).
    pub echo_openjd_directives: bool,
    /// Caller-policy caps and evaluation budgets enforced by this session
    /// (see [limits](#limits)). Default: no limits beyond the spec.
    pub limits: SessionLimits,
}
```

### `limits` — run-time enforcement of caller policy

Of the spec's three processing stages (Template Schemas §7.4: template
validation, job creation, task execution on the worker host), task
execution is the **enforcement boundary** for resolved-value limits
(`SessionLimits`, from `openjd_sessions::limits`): a worker can receive
a job that never passed through this client's template validation or
job creation, so the checks must hold here on the final resolved values
regardless of what earlier stages did. The first four fields mirror the
corresponding `openjd_model::CallerLimits` fields a submitting service
would set:

- `max_resolved_arg_len` — the resolved action `command` and every final
  argv entry (after null-skip and list-flatten) — see
  [runners.md](runners.md);
- `max_resolved_data_len` — every resolved embedded-file `data` value —
  see [embedded-files.md](embedded-files.md);
- `max_eval_memory_bytes` / `max_eval_operations` — bound every
  format-string expression evaluation the session performs (env var
  values, command/args, timeout/cancelation fields, embedded data),
  defaulting to the Expression Language spec's 100 MB / 10 M.
- `max_path_mapping_trie_bytes` — limits the estimated size of the session's
  path mapping trie (`PathMapper::with_max_trie_bytes`); `None` uses 30 MiB.
  Any number of rules is allowed. It has no `CallerLimits` counterpart, and a
  violation is `SessionError::InvalidPathMappingRule` naming the rule that passed
  the limit.

All default to `None`. That means no restriction beyond the spec (§5.1/§5.2/§6.1.2
set no maxima of their own), except that the path mapping trie uses its 30 MiB
default. Violations of the first two surface as
`SessionError::FormatString` with reason
`resolved value is {n} characters, exceeding the maximum of {max}.`,
following the `notifyPeriodInSeconds ≤ 600` and env-var §4.4.2
precedents. (The §4.4.2 2048-char env-value cap is spec-mandated and
always on — it is not part of `SessionLimits`.)

### Why `session_root_directory` instead of `working_directory`

The Python library accepts `session_root_directory` and creates the actual working
directory internally via `TempDir`. This is important because:

1. The session needs to control the directory name (8 random characters, retried on
   collision; see [tempdir.md](tempdir.md#session-working-directory-name))
2. The session needs to set permissions (0o700, or 0o770 for cross-user)
3. The session creates both a working directory and a files subdirectory within it

Accepting a pre-created directory would require the caller to know these details.

### Why `callback` is `Option<Box<dyn Fn>>` not a trait

The Python library uses `Callable[[str, ActionStatus], None]`. A boxed closure is the
closest Rust equivalent and avoids forcing consumers to define a named type. The callback
must be `Send + Sync` because it's invoked from the async task processing `ActionMessage`
values.

The callback fires on:
- Action start (state = Running) — but not for an `enter_environment`/`exit_environment`
  action that has no `onEnter`/`onExit` (it records `Success` without a `Running`
  callback), nor for a failure before that action is reported `Running` (building the
  symbol table, or resolving an environment's `variables`), which reports only the
  `Failed` state. `run_task` always reports `Running` first (see
  [Failures before an action starts](#failures-before-an-action-starts))
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

A tokio `CancellationToken` can't be reset once canceled, so this cancellation is
permanent: every action after it ends `Canceled`. Because of that, once the session-wide
token is canceled, `clear_ending_only()` refuses with `SessionError::SessionCanceled`
rather than return the session to `Ready` (see [Brittle Sessions](#brittle-sessions)).
A per-action cancel (`cancel_action()` or a `SessionCancelHandle`) installs a fresh token
per action and does not have this effect.

## Session Construction

`Session::with_config(config: SessionConfig) -> Result<Self, SessionError>`:

1. Builds the session's `PathMapper` from `config.path_mapping_rules`, failing with
   `SessionError::InvalidPathMappingRule` before any directory is created when
   a rule is invalid (a relative source path, or a path longer than 32 KiB) or the rules' trie would pass its size limit
2. Creates the session root directory via `custom_gettempdir()` if not provided
3. Creates the working directory via `TempDir::new_session_working_dir()` with the
   session user for cross-user permissions. Its name is 8 random characters from
   `[a-z0-9_]` and carries no part of the session ID (see
   [tempdir.md](tempdir.md#session-working-directory-name))
4. Creates the files subdirectory within the working directory
5. Validates sticky bit on parent directories (POSIX security check)
6. Logs host info (version, platform, architecture)
7. Stores job parameter values, the path mapper, and other config for later use

A `Session::new_for_test(working_directory)` constructor is available when the
`test-utils` feature is enabled or in `#[cfg(test)]` builds. It creates a minimal
session with an empty session ID, no callback, no path mapping, and `debug_collect_stdout`
set to `true`. Production code should always use `Session::with_config`.

## Environment Management

### Enter

`enter_environment(env, resolved_symtab, identifier, os_env_vars) -> Result<String, SessionError>`,
where `resolved_symtab`, `identifier` and `os_env_vars` are all optional:

1. Validates state is `Ready`, that an explicit `identifier` isn't already entered, and
   the RFC 0008 single wrap layer rule. A call that fails these is rejected and changes
   nothing (see [Failures before an action starts](#failures-before-an-action-starts))
2. Stores the environment, folding the `resolved_symtab` argument onto the stored copy's
   `Environment::resolved_symtab` field. A caller may supply an environment's resolved
   symbol table either way, and the two are equivalent: readers that run after this call
   returns — notably the RFC 0008 wrap hooks, which resolve a wrap environment's own
   scope off the stored `Environment` — see the same table whichever route was used.
   When a caller supplies both, the argument wins, being the more specific input for
   this entry.
3. Pushes the environment onto `environments_entered` (LIFO stack). From here on, a
   failure fails the action and leaves the environment on the stack
4. Builds the symbol table (see [Symbol Table Construction](#symbol-table-construction))
5. Resolves environment `variables` format strings against that symbol table,
   with target type `string` for whole-field expressions (Expression Language §1.3.2),
   and rejects a resolved value that contains a NUL byte or exceeds 2048 characters
   (Template Schemas §4.4.2 — the session is the enforcement stage for values that
   are unknown at template validation; the limit is kept in sync with the model's
   `EffectiveLimits::max_env_var_value_len`). Variables resolve in sorted key order and
   all resolve before any is set, so a failure sets none of them and reports a
   deterministic error when more than one is invalid
6. Without an `onEnter`, records a `Success` action and returns. Otherwise sets state to
   `Running`, creates an `EnvironmentScriptRunner` and calls `enter()` (runs the
   `onEnter` action, or an outer environment's `onWrapEnvEnter` in its place)
7. Processes `ActionMessage` values via `drive_action()` — env var changes from
   `openjd_env`/`openjd_unset_env` are applied to the environment's change set
8. On success: state → `Ready` (or `ReadyEnding` if `ending_only`)
9. On failure the session is ending-only and the call returns the error: a failure
   before the subprocess starts records a `Failed` action with the error's message,
   while an `onEnter` that runs and ends non-`Success` leaves the action in its own
   terminal state (`Failed`, `Canceled` or `Timeout`) and returns
   `EnvironmentScriptFailed`

### Exit

`exit_environment(identifier, resolved_symtab, keep_session_running, os_env_vars)`:

1. Validates state is `Ready` or `ReadyEnding`, that the identifier is known, and that it
   is the most recently entered environment (LIFO order). A call that fails these is
   rejected and changes nothing
2. Evaluates the action's environment variables, which still include the exiting
   environment's own
3. Sets `ending_only = true` unless `keep_session_running` is true
4. Pops the environment from `environments_entered` and removes it, so its variable
   changes no longer apply. This happens before anything that can fail, so a failed exit
   still counts as an exit
5. Builds the symbol table. A failure here, or anywhere before the `onExit` starts,
   fails the action (see [Failures before an action starts](#failures-before-an-action-starts))
6. Runs the `onExit` action (or an outer environment's `onWrapEnvExit` in its place) via
   `EnvironmentScriptRunner::exit()`, if there is one
7. On completion: state → `Ready` or `ReadyEnding` as for any other action. With
   `keep_session_running` true, the state is `Ready` only if the session wasn't already
   ending-only; `keep_session_running` never clears ending-only mode, and only
   `clear_ending_only()` does (see [Brittle Sessions](#brittle-sessions))

### Why LIFO enforcement

Environments represent nested scopes — a step environment entered after a job environment
depends on the job environment's variables. Exiting out of order would leave the session
in an inconsistent state. The Python library enforces this, and the Rust crate matches.

## Task Execution

`run_task(script, task_parameter_values, resolved_symtab, os_env_vars)`:

1. Validates state is `Ready`; otherwise the call is rejected and changes nothing
2. Sets state to `Running`
3. Builds the symbol table, including `Task.Param.*` and `Task.RawParam.*`. A failure
   here, or anywhere before the subprocess starts, fails the action (see
   [Failures before an action starts](#failures-before-an-action-starts))
4. Creates a `StepScriptRunner` and calls `run()` (runs `onRun` action)
5. Processes `ActionMessage` values via `drive_action()`
6. On completion: state → `Ready` or `ReadyEnding` based on result

When a wrap hook is dispatched, the borrow of the active environment stack is
released by copying only the wrapper data needed for symbol seeding: its name,
frozen resolved symbol table, script `let` bindings, and selected hook action.
The frozen table is read off the *stored* `Environment`, which is why §Enter
folds the `resolved_symtab` argument onto it — a wrapper's own `Param.*` reaches
its hooks regardless of which route the caller used to supply the table.
The wrapper's embedded files are handled separately via a per-wrap-environment
cache (see [embedded-files.md](embedded-files.md#wrap-environment-file-path-caching)):
file paths are allocated once and reused across invocations; file contents are
re-written each time so `data` expressions resolve against the current scope.

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
    -> Result<(), SessionError>
```

Appends additional path mapping rules after the session's existing rules and
rebuilds the session's `PathMapper` and function library. Rules keep the order
given. The rule whose source path has the most names wins, and between rules for
the same location the one given last wins (see
[expr path-mapping.md § Which rule wins](../expr/path-mapping.md#which-rule-wins)),
so an added rule takes precedence over an existing rule for the same location
(`path_mapping_rules()` still returns both).

Returns `SessionError::InvalidPathMappingRule`, and leaves the rules unchanged,
when an added rule is invalid (a relative source path, or a path longer than 32 KiB) or the rules' trie would pass its size limit. The error's
index counts from the first existing rule.

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
applied to each element, whether the value is a list of strings, a list of paths, or a
single path, and the same way whether or not a base symbol table (`resolved_symtab`)
is given. A PATH or LIST_PATH value longer than 32 KiB
(`openjd_expr::path_mapping::MAX_PATH_LEN`) fails symbol table construction with
`SessionError::Runtime`: `Failed to apply path mapping to <key>: the path is <n>
bytes long, more than the limit of 32768 bytes`, as `apply_path_mapping` does. The
session's path mapping trie is limited by `SessionLimits::max_path_mapping_trie_bytes`
(default 30 MiB). All other parameter values are coerced to their declared
parameter type when seeded, for `Param.*`, `RawParam.*`, `Task.Param.*`, and
`Task.RawParam.*` alike: values cross the language boundary as strings (an INT parameter
arrives as `"5"`), and typed expression evaluation requires typed symbols, so the value
is converted via `ExprValue::coerce` (`"5"` → `Int(5)`, including element-wise coercion
for list types). `CHUNK_INT` task parameters are exempt and remain strings — chunk range
expressions are not numeric values. A value that cannot be coerced to its declared type
(e.g. INT `"abc"`) fails symbol table construction with a `SessionError::Runtime` naming
the parameter.

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
path mapping rules themselves. The rules are written in precedence order
(`PathMapper::rules_by_precedence`): most source path names first, then WINDOWS
before POSIX before URI, then the one given last first. A rule replaced by a later rule
for the same location is left out. A reader that applies the first matching rule, or
one that follows the specification's order (most names, then WINDOWS over POSIX, last
rule for a location), therefore gets the same result as the session, as long as it
splits and compares paths the same way (see
[expr path-mapping.md § Divergence from Python](../expr/path-mapping.md#divergence-from-python)
for where `pathlib` differs).

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

### SessionCancelHandle

`cancel_action` requires `&mut Session`, which is unavailable while an action-driving
thread owns the `Session` (the embedding pattern used by the Python bindings). For that
case, `Session::cancel_handle()` returns a `SessionCancelHandle` — a thread-safe,
reusable handle over the shared per-action cancel state (token, cancel-request watch
channel, `mark_failed` flag, all behind one `Arc<Mutex<..>>`).

`SessionCancelHandle::cancel(time_limit, mark_action_failed) -> bool`:

- Returns `false` if no action is running (no per-action token installed).
- Otherwise delivers exactly what `cancel_action` delivers — the helper pipe cancel
  command (cross-user sessions), the `time_limit` over the watch channel, and the
  token cancel — while holding the shared lock so the target action cannot change
  mid-delivery.
- Unlike `cancel_action`, it cannot set the session's transient `Canceling` state
  (the `Session` is owned elsewhere); the state moves directly to the terminal value
  when the canceled action finishes.

Every action installs fresh cancel state, so a cancel never poisons later actions
(contrast with `SessionConfig::cancel_token`, whose cancellation is permanent) and
the same handle works for the life of the session.

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
   owned by that user, then `TempDir::cleanup()` as the process user for any
   remainder, as in step 3
3. For same-user sessions: `TempDir::cleanup()` on the working directory, which
   also removes the embedded files directory inside it. On Windows this can block
   for at least about 31 s while it waits for open files to be released, logging a
   warning after each failed pass (see tempdir.md "Cleanup retries on Windows").
   If it still fails, logs the path and reason at error level and continues
4. Sets state to `Ended`
5. Skips directory removal if `retain_working_dir` is `true`

### Drop safety net

If `cleanup()` is never called, `Session`'s `Drop` impl:
1. Logs a warning ("Session '<id>' was dropped without calling cleanup(). Working
   directory may not have been cleaned up.")
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
| `CancelMarkFailed` | Sets `action_status.fail_message` (replacing any earlier `Fail` text), triggers cancelation with `mark_action_failed = true` |

After each message, the user callback is invoked with the updated `ActionStatus`.


## Builder Methods

After constructing a `Session` via `with_config`, several builder methods configure
optional features before the first action:

### with_path_mapping

```rust
pub fn with_path_mapping(self, rules: Vec<PathMappingRule>) -> Result<Self, SessionError>
```

Replaces the session's path mapping rules, keeping the order given. Fails with
`SessionError::InvalidPathMappingRule` when a rule is invalid (a relative source path, or a path longer than 32 KiB) or the rules' trie would pass its size limit. Called immediately after construction when path mapping rules are
known at session creation time. `Session::with_config` builds the session's
`PathMapper` from `SessionConfig::path_mapping_rules` the same way and fails with
the same error.

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
| `path_mapping_rules()` | `&[PathMappingRule]` | Current path mapping rules, in the order given |
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
entered in the session, it returns `SessionError::DuplicateEnvironment`, whose message is
`"Environment {id} has already been entered in this Session."`, and changes nothing. This
prevents accidentally entering the same environment twice, which would corrupt the LIFO
stack.

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

## echo_openjd_directives

`SessionConfig.echo_openjd_directives: bool` (default `true`) controls whether
recognised `openjd_*` directive lines from subprocess stdout are echoed to the
session log. The default matches the Python reference implementation
(`openjd-sessions-for-python`'s `ActionMonitoringFilter` defaults to echoing
directives). Recognised directives are:

| Directive | Effect |
|-----------|--------|
| `openjd_progress: <float>` | Update action progress percentage |
| `openjd_status: <string>` | Update human-readable status |
| `openjd_fail: <string>` | Set failure reason and cancel |
| `openjd_env: NAME=VALUE` | Set environment variable for subsequent actions |
| `openjd_redacted_env: NAME=VALUE` | Set env var and redact value in logs |
| `openjd_unset_env: NAME` | Unset environment variable |
| `openjd_session_runtime_loglevel: LEVEL` | Change runtime log level |

When `true`, the directive line is logged after redaction is applied (so the
secret value in `openjd_redacted_env` is replaced with `********` before reach
the log). When `false`, recognised directives are still parsed and acted on
(progress/status updates, env-var changes, redacted-value registration, …) but
the directive lines themselves are filtered out of the log stream — only
non-directive command output reaches the log.

Implementation: the flag is forwarded down to `ScriptRunnerBase` via
`with_echo_openjd_directives` and passed directly as
`ActionFilter::new`'s `echo_openjd_directives` argument.

### Redaction interaction

Setting `echo_openjd_directives = false` is **not** a security feature. Values
from `openjd_redacted_env` directives are added to the session's redaction set
*before* the originating line is considered for pass-through, so:

* When echoing is on (the default), the directive line itself is rewritten to
  `NAME=********` before it ever reaches the log handler.
* Subsequent occurrences of the secret value in any log line — directive or
  not — are also redacted on the way through `apply_redaction`, regardless of
  this flag.

The flag's purpose is operator UX: removing the noisy directive lines from the
log when a job emits many `openjd_progress` updates per second, for example,
or when an operator wants the log to reflect only the underlying tool's output
and not the OpenJD orchestration metadata. Use redaction (the
`REDACTED_ENV_VARS` extension) to keep secrets out of logs; use this flag to
keep the log readable.
