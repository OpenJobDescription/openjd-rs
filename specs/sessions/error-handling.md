# Error Handling

## Overview

`SessionError` in `error.rs` is the crate's error type, derived via `thiserror`. All
public session methods return `Result<_, SessionError>`.

## SessionError

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SessionError {
    #[error("Session must be in {} state, current: {current}", format_expected(.expected))]
    InvalidState {
        expected: Vec<SessionState>,
        current: SessionState,
    },

    #[error("Environment '{name}' {action} failed: {reason}")]
    EnvironmentScriptFailed { name: String, action: String, reason: String },

    #[error("Failed to resolve {context}: {reason}")]
    FormatString { context: String, reason: String },

    #[error("Failed to write embedded file '{name}': {source}")]
    EmbeddedFile { name: String, #[source] source: std::io::Error },

    #[error("Failed to create working directory {path}: {source}")]
    WorkingDirectory { path: PathBuf, #[source] source: std::io::Error },

    #[error("Failed to start subprocess '{command}': {source}")]
    SubprocessStart { command: String, #[source] source: std::io::Error },

    #[error("Failed to create temp directory in {path}: {source}")]
    TempDir { path: PathBuf, #[source] source: std::io::Error },

    #[error("{0}")]
    Runtime(String),

    #[error("Environment {id} has already been entered in this Session.")]
    DuplicateEnvironment { id: String },

    #[error("Unknown environment identifier: {identifier}")]
    UnknownEnvironment { identifier: String },

    #[error("Failed to set permissions on '{path}': {reason}")]
    PathPermissions { path: String, reason: String },

    #[error("Cross-user helper error: {0}")]
    HelperCommunication(String),

    #[error("Must exit the most recently entered environment first. Expected {expected}, got {got}")]
    LifoViolation { expected: String, got: String },

    #[error("The session was canceled through its session-wide cancel token and cannot be continued.")]
    SessionCanceled,
}
```

The enum is `#[non_exhaustive]`, allowing new variants to be added without breaking
consumers.

## Variant Design Rationale

### InvalidState

Guards every state transition. The `expected` field is `Vec<SessionState>` because some
operations accept multiple valid states (e.g., `exit_environment` accepts both `Ready`
and `ReadyEnding`). A `format_expected()` helper joins the variants with `" or "` for
display:

```rust
fn format_expected(states: &[SessionState]) -> String {
    match states {
        [single] => single.to_string(),
        _ => states.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(" or "),
    }
}
```

This produces messages like `"Session must be in READY state"` or
`"Session must be in READY or READY_ENDING state"`.

### EnvironmentScriptFailed

Captures the environment name and action (`"enter"` or `"exit"`) for diagnostics.
The `reason` is typically the subprocess exit code or a format string resolution error.

### FormatString

Wraps errors from `openjd_model::resolve_format_string()` and
`openjd_expr::evaluate_expression()`. The `context` field describes what was being
resolved (e.g., `"action command"`, `"embedded file data for 'MyScript'"`) to help
users locate the problem in their template.

### EmbeddedFile

Wraps `std::io::Error` from file creation, writing, or permission changes. The `name`
field is the embedded file's `name` from the template, not the filesystem path.

### WorkingDirectory and TempDir

Both wrap `std::io::Error` with the path that failed. They're separate variants because
they occur at different lifecycle points (construction vs. cleanup) and may need different
handling by consumers.

### SubprocessStart

Wraps `std::io::Error` from `tokio::process::Command::spawn()`. The `command` field is
the resolved command string, helping diagnose "command not found" errors.

### Runtime

Catch-all for errors that don't fit other variants. Used sparingly — most errors should
have a specific variant. Several former `Runtime` uses have been promoted to structured
variants (`DuplicateEnvironment`, `UnknownEnvironment`, `LifoViolation`,
`PathPermissions`, `HelperCommunication`). Remaining uses cover:
- Unexpected internal state
- Validation errors not yet promoted to structured variants

### DuplicateEnvironment

Returned when `enter_environment` is called with an identifier that is already in the
session's entered-environments stack. Prevents corrupting the LIFO stack.

### UnknownEnvironment

Returned when an operation references an environment identifier that doesn't exist in
the session.

### PathPermissions

Wraps `chown`/`chmod` failures during cross-user file ownership setup. The `path` field
is the filesystem path; `reason` describes the OS error.

### HelperCommunication

Wraps IPC failures between the session and the embedded cross-user helper binary.
Covers protocol errors, unexpected helper exit, and stdin/stdout pipe failures.

### LifoViolation

Returned when `exit_environment` is called out of LIFO order. The `expected` field is
the identifier that should be exited next; `got` is the identifier that was requested.

### SessionCanceled

Returned by `clear_ending_only()` when the session-wide
`SessionConfig::cancel_token` has been canceled. A tokio `CancellationToken` can't be
reset once canceled, so every later action would end `Canceled` and make the session
ending-only again. Rather than return the session to `Ready` under a cancellation it
can't undo, `clear_ending_only()` refuses and leaves the session `ReadyEnding`; only
`exit_environment()` and `cleanup()` remain. Per-action cancels (`cancel_action()` and
`SessionCancelHandle`) don't raise this — they install a fresh token per action.

## Error Propagation Patterns

### Runner → Session

Runners return `Result<SubprocessResult, SessionError>`. The session maps these to
state transitions:
- `Ok(result)` with `result.state == Success` → `Ready` (or `ReadyEnding` if the session
  is ending-only)
- `Ok(result)` with `result.state` `Failed`, `Canceled` or `Timeout` → ending-only,
  `ReadyEnding`
- `Err(e)` → ending-only, `ReadyEnding` + error logged

See [session.md § Brittle Sessions](session.md#brittle-sessions).

### Rejections and failed actions

An error from `enter_environment`, `exit_environment`, `run_task` or `run_subprocess` is
one of two kinds:

- **A rejection**, from the up-front checks: `InvalidState`, `DuplicateEnvironment`,
  `MultipleWrapEnvironments`, `UnknownEnvironment`, `LifoViolation`, and `run_subprocess`'s
  `Runtime` argument checks. It changes nothing: the state, the entered environments and
  the last action's status are as they were, and no callback fires.
- **A failed action**, for any error once the call is accepted. For a failure before the
  subprocess starts (building the symbol table, resolving an environment's `variables`,
  path mapping, wrap-hook setup) the action status is `Failed` with the error's message;
  this matches the Python library, which reports these through `action_status` without
  raising. For an `onEnter`/`onExit` that runs and ends non-`Success`, the error is
  `EnvironmentScriptFailed` and the action status is the script's own terminal state
  (`Failed`, `Canceled` or `Timeout`); `run_task` instead returns `Ok` with that state.
  Either way the session is ending-only. One exception: `exit_environment` doesn't fail
  when only its own `resolved_symtab` can't build the symbol table. It falls back to the
  table the environment was entered with, logs a warning, and runs the `onExit`
  ([session.md § Exit](session.md#exit) step 5).

The table in
[session.md § Failures before an action starts](session.md#failures-before-an-action-starts)
lists the rejections per call.

### Format string errors

Format string resolution can fail at multiple points:
- `resolve_action_args()` — command/args resolution
- environment `variables` resolution in `enter_environment`
- `write_file_contents()` — embedded file data resolution

All are wrapped in `SessionError::FormatString` with context describing the operation.

### I/O errors

File system operations (temp dir creation, embedded file writes, cleanup) produce
`std::io::Error` values wrapped in the appropriate `SessionError` variant with the
relevant path or name for diagnostics.

## Why thiserror

The `thiserror` crate provides derive macros for `std::error::Error` with `Display`
and `source()` implementations. This is the standard approach in the Rust ecosystem
for library error types — it's zero-cost at runtime and produces clean error messages.

The alternative (`anyhow`) is designed for applications, not libraries, and erases
type information that consumers need for error handling.
