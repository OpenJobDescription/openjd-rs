# openjd-sessions Architecture

## Crate Purpose

The `openjd-sessions` crate provides the runtime for executing OpenJD sessions — the
sequence of environment enter/exit and task run actions that constitute a worker's
execution of a job. It is the Rust equivalent of the Python `openjd-sessions-for-python`
library, designed as a drop-in replacement for use by the Deadline Cloud worker agent.

## Dependencies

```
openjd-sessions
├── openjd-model    # Template/job types, format string resolution, let bindings
├── openjd-expr     # ExprValue, SymbolTable, FunctionLibrary, PathMappingRule
├── tokio           # Async runtime (rt-multi-thread, process, io-util, time, sync)
├── tokio-util      # CancellationToken for cooperative cancelation
├── nix             # POSIX signals (killpg), process groups (setsid), user IDs
├── serde/json/yaml # Serialization for path mapping rules, cancel_info.json
├── regex           # openjd_* directive parsing in ActionFilter
├── thiserror       # SessionError enum derivation
├── shlex           # Shell-safe argument quoting for cross-user scripts
├── log             # Logging facade with kv feature for structured metadata
├── bitflags        # LogContent flag type
└── uuid            # Session ID generation in tests
```

## Module Layout

```
src/
├── lib.rs                  # Public API re-exports
├── session.rs              # Session struct, state machine, lifecycle
├── action.rs               # ActionState, ActionMessage, ActionResult types
├── action_status.rs        # ActionStatus struct (progress, status, fail, exit_code)
├── action_filter.rs        # Directive parsing from stdout lines, redaction
├── subprocess.rs           # Async subprocess execution via tokio::process
├── runner/
│   ├── mod.rs              # CancelMethod, ScriptRunnerState, resolve_action_args()
│   ├── env_script.rs       # EnvironmentScriptRunner (enter/exit)
│   └── step_script.rs      # StepScriptRunner (run)
├── embedded_files.rs       # Two-phase file materialization
├── let_bindings.rs         # Re-exports evaluate_let_bindings from openjd_model
├── session_user.rs         # SessionUser trait, PosixSessionUser
├── tempdir.rs              # Secure temp directory creation
├── logging.rs              # LogContent bitflags, session_log! macro, banners
└── error.rs                # SessionError enum
```

```
build.rs                    # Compiles embedded cross-user helper binary
helper/                     # Standalone helper binary crate
├── Cargo.toml              # Independent workspace ([workspace] = {})
└── src/
    ├── main.rs             # Shared stdin reader, command dispatch loop
    ├── protocol.rs         # Command/Response JSON serde types
    ├── runner.rs           # poll(2) loop, child management, cancel handling
    └── runner_win.rs       # Windows runner (placeholder)
```

## Public API Surface

Re-exported from `lib.rs`:

```rust
// Core session
pub use session::{Session, SessionState, SessionConfig, EnvironmentIdentifier};
pub use action::{ActionState, ActionResult, ActionMessage};
pub use action_status::ActionStatus;
pub use error::SessionError;

// Subprocess
pub use subprocess::SubprocessResult;
pub use runner::{CancelMethod, ScriptRunnerState};

// Environment and path mapping
pub use openjd_expr::{PathFormat, PathMappingRule};  // re-export

// Logging
pub use logging::LogContent;

// Cross-user (POSIX)
pub use session_user::{SessionUser, PosixSessionUser};
pub use tempdir::TempDir;

// Cross-user (Windows)
#[cfg(windows)]
pub use session_user::{WindowsSessionUser, BadCredentialsError};
```

### External Cancellation

`SessionConfig.cancel_token` accepts an optional `tokio_util::sync::CancellationToken`.
When provided, all action cancel tokens are created as children of this token via
`parent.child_token()`. Canceling the parent cascades to all current and future actions
in the session. This enables the worker agent to cancel an entire session from outside
the session's async context. A tokio token can't be reset once canceled, so this
cancellation is permanent; `Session::clear_ending_only()` refuses with
`SessionError::SessionCanceled` once it has fired (see
[session.md § Brittle Sessions](session.md#brittle-sessions)).

## Data Flow

A typical session lifecycle flows through these modules:

```
SessionConfig ──► Session::with_config()
                      │
                      ├── TempDir::new_session_working_dir() ──► working_directory
                      ├── TempDir::new() ──► files_directory
                      ├── build_symbol_table() ──► SymbolTable with Param.*, Session.*
                      └── materialize_path_mapping() ──► JSON file + Session.HasPathMappingRules
                      │
                      ▼
                 enter_environment()
                      │
                      ├── evaluate_env_vars() ──► cumulative env var map
                      ├── EnvironmentScriptRunner::enter()
                      │       ├── EmbeddedFiles::allocate_file_paths()
                      │       ├── evaluate_let_bindings()
                      │       ├── EmbeddedFiles::write_file_contents()
                      │       └── resolve_action_args() ──► SubprocessConfig
                      │
                      └── run_subprocess()
                              ├── tokio::process::Command (setsid, sudo for cross-user)
                              ├── stdout ──► ActionFilter ──► ActionMessage via mpsc
                              └── Session::drive_action() receives messages, invokes callback
                      │
                      ▼
                 run_task()  [same pattern: StepScriptRunner → subprocess]
                      │
                      ▼
                 exit_environment()  [reverse order, EnvironmentScriptRunner::exit()]
                      │
                      ▼
                 cleanup()  ──► TempDir::cleanup() + cross-user sudo rm
```

## Key Design Decisions

### Async-first with tokio

The Python library uses `ThreadPoolExecutor` + daemon threads + `Queue` + `Lock` for
non-blocking execution — a complex arrangement driven by Python's lack of native async
subprocess I/O. Rust with tokio eliminates this complexity:

- `tokio::process::Command` provides async stdout streaming natively
- `tokio::select!` replaces the entire `LoggingSubprocess` + `Timer` + `Lock` + `Queue`
  apparatus
- `CancellationToken` replaces `threading.Event` — no lock coordination needed
- `tokio::time::sleep` replaces `Timer` threads

The public API is async. A blocking wrapper for PyO3 bindings is planned but not yet
implemented.

### Channel-based message streaming

The Python library uses a `logging.Filter` attached to the module logger to intercept
`openjd_*` directives mid-stream. This couples directive processing to Python's logging
infrastructure.

The Rust crate uses `tokio::sync::mpsc::unbounded_channel` to stream `ActionMessage`
values from the subprocess stdout loop to the session. This decouples parsing (in
`ActionFilter`) from processing (in `Session::drive_action`), and avoids the need for
shared mutable state between the subprocess and session.

### Ownership-driven API

The Python library stores the current runner as `self._runner` and mutates session state
from callbacks. The Rust crate avoids interior mutability by having `Session` own the
action lifecycle through `&mut self` methods. The `drive_action` method holds `&mut self`
while concurrently processing messages from the channel, which is safe because the
subprocess runs in a separate future joined via `tokio::select!`.

### Windows support

The Python library supports both POSIX and Windows with extensive platform-specific code
(ACLs, `CreateProcessWithLogonW`, `PopenWindowsAsUser`, etc.). The Rust crate implements
both platforms:

- Same-user subprocess execution: implemented (`subprocess.rs` Windows platform module),
  with pre-spawn executable resolution (`win32_locate.rs` — PATHEXT-aware,
  working-directory-first, no fallback to the worker's own PATH)
- Cross-user subprocess execution: implemented end-to-end — `WindowsSessionUser`
  (password and logon-token modes), the embedded helper binary spawned via
  `CreateProcessWithLogonW`/`CreateProcessAsUserW` (`win32.rs`), CTRL_BREAK notify and
  Job-Object/`CreateToolhelp32Snapshot` process-tree termination inside the helper
  (`helper/src/runner_win.rs`)
- Win32 helpers: `win32.rs` (logon, user lookup, environment blocks),
  `win32_permissions.rs` (ACL management), `win32_remove.rs` (the cleanup removal
  pass, which deletes what it can and reports what it cannot), `win32_file_locks.rs`
  (which processes hold those files open, through the Restart Manager),
  `win32_locate.rs` (executable resolution,
  called from both spawn paths)
- Temp directory and embedded file permissions: Windows ACL paths implemented
- Integration testing on Windows: dedicated CI job (`cross-user-windows`) runs the
  cross-user and permissions suites on `windows-latest` with a temporary test user,
  and the standard test matrix includes Windows

## Python-vs-Rust Design Comparison

This section consolidates the key design differences between the Python
`openjd-sessions-for-python` library and this Rust crate. Other spec documents
reference this section rather than repeating the comparison.

| Aspect | Python | Rust |
|--------|--------|------|
| Concurrency | `ThreadPoolExecutor` + daemon threads + `Queue` + `Lock` | `tokio::select!` + `mpsc::unbounded_channel` + `CancellationToken` |
| Subprocess I/O | `logging.Filter` on module logger intercepts stdout | `ActionFilter` struct parses lines, sends `ActionMessage` via channel |
| State mutation | `logging.Filter` callback mutates session state (GIL-safe) | `Session::drive_action` processes messages with `&mut self` (no locks) |
| Cancelation | `threading.Event` + lock coordination | `CancellationToken` (child tokens cascade from parent) |
| Cross-user launch | `sudo -u <user> -i` per action (~1s overhead each) | Embedded helper binary, `sudo -i` once per session (~1ms subsequent) |
| Error types | Exceptions (`RuntimeError`, `OSError`) | `SessionError` enum with `thiserror` (`#[non_exhaustive]`) |
| Callback | `Callable[[str, ActionStatus], None]` | `Box<dyn Fn(&str, &ActionStatus) + Send + Sync>` |
| Temp directory cleanup | Explicit `cleanup()`, no `__del__` | Explicit `cleanup()` + `Drop` safety net |
| Brittle sessions | openjd-sessions 0.14.0: a Failed, Canceled or Timeout action moves the session to `READY_ENDING` but doesn't set `_ending_only`, so a later `exit_environment(keep_session_running=True)` returns it to `READY`. That is its only way back to `READY`: it has no `clear_ending_only()`, and once `_ending_only` is set nothing clears it | The action sets `ending_only`, so the session stays `ReadyEnding` whatever `keep_session_running` says. Only an explicit `clear_ending_only()` returns it to `Ready` (see [session.md § Brittle Sessions](session.md#brittle-sessions), issue #287) |
| Failures before an action starts | A failure after the call's up-front checks (for example an undeserializable `resolved_symtab` or an environment variable that can't be resolved) fails the action and moves the session to `READY_ENDING`; the method returns normally (`enter_environment` returns the identifier) | The same outcome: a `Failed` action status with the error as its message, and `ReadyEnding`. The method also returns the error, as every failed action in `enter_environment`/`exit_environment` does. Rust also fails the action for inputs Python doesn't reject: a job or task parameter value it can't coerce to its declared type, a `PATH` parameter over 32 KiB, and an environment variable value with a NUL byte or over 2048 characters. The `fail_message` wording is Rust's own (for example it doesn't name the environment on a variable-resolution failure), so it isn't byte-for-byte Python's. `run_task` reports a `Running` callback before the `Failed` one for these pre-start failures, as it already does for its other setup failures; `enter_environment` and `exit_environment` report only `Failed` (see [session.md § Failures before an action starts](session.md#failures-before-an-action-starts)) |
