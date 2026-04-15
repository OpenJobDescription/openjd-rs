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
├── caps            # Linux CAP_KILL capability management (linux-only)
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
├── capabilities.rs         # Linux CAP_KILL support (no-op on non-Linux)
└── error.rs                # SessionError enum
```

## Public API Surface

Re-exported from `lib.rs`:

```rust
// Core session
pub use session::{Session, SessionState, SessionConfig};
pub use action::{ActionState, ActionResult, ActionMessage};
pub use action_status::ActionStatus;
pub use error::SessionError;

// Subprocess
pub use subprocess::SubprocessResult;
pub use runner::{CancelMethod, ScriptRunnerState};

// Environment and path mapping
pub use session::EnvironmentIdentifier;
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
the session's async context.

## Data Flow

A typical session lifecycle flows through these modules:

```
SessionConfig ──► Session::with_config()
                      │
                      ├── TempDir::new() ──► working_directory, files_directory
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

### POSIX-first, Windows partially implemented

The Python library supports both POSIX and Windows with extensive platform-specific code
(ACLs, `CreateProcessWithLogonW`, `PopenWindowsAsUser`, etc.). The Rust crate implements
POSIX/Linux as the primary target since Linux workers are the primary deployment.

Windows has partial support:
- Same-user subprocess execution: implemented (`subprocess.rs` Windows platform module)
- Cross-user subprocess execution: partially implemented (`WindowsSessionUser` with
  `CreateProcessWithLogonW`/`CreateProcessAsUserW`, process tree kill via
  `CreateToolhelp32Snapshot`)
- Win32 helpers: `win32.rs` (logon, user lookup), `win32_permissions.rs` (ACL management),
  `win32_locate.rs` (executable resolution, not yet integrated)
- Temp directory and embedded file permissions: Windows ACL paths implemented
- Integration testing on Windows: pending
