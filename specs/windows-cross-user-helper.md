# Windows Cross-User Helper Subprocess

Port the existing POSIX cross-user helper to Windows, reusing the same binary
and JSON protocol.

## Background

On POSIX, cross-user subprocess execution uses a persistent helper process
launched via `sudo -u <user> -i <helper_path>`. The helper communicates over
newline-delimited JSON on stdin/stdout. This avoids paying the ~1s sudo login
cost per action and provides reliable cancel via `killpg`.

On Windows, cross-user execution currently uses `CreateProcessAsUserW` /
`CreateProcessWithLogonW` directly from the session code. This works for
launching processes, but **cancellation is broken** because:

1. `CTRL_BREAK_EVENT` requires `AttachConsole` which doesn't work from
   Session 0 (Windows services) or across user boundaries
2. The Python openjd-sessions works around this by spawning a separate
   Python script (`_signal_win_subprocess.py`) to send the signal from
   the target user's context

The fix: use the same persistent helper architecture as POSIX. The helper
runs as the target user, so console APIs and process termination work
correctly from within the right user/session context.

## Current POSIX Architecture

```
Session (agent user)          Helper (job user, via sudo)
    |                              |
    |-- {"command":..., "args":..., "env":..., "cwd":...} -->
    |                              |-- spawns child process
    |                              |   (process group 0)
    |<-- {"pid": 1234} -----------|
    |<-- {"out": "line..."} ------|   (stdout lines)
    |                              |
    |-- {"cancel":"NOTIFY_THEN_TERMINATE","notifyPeriodInSeconds":5} -->|-- killpg(child, SIGTERM)
    |                              |   (grace period, then SIGKILL if still alive)
    |                              |
    |<-- {"exited": 0} ----------|
    |                              |
    |-- "shutdown" -------------->|-- exits
```

### Key files

- `cross_user_helper.rs` — session-side: spawns helper, sends commands,
  reads responses via `run_via_helper()`
- `helper/src/main.rs` — helper binary: reads commands, dispatches to runner
- `helper/src/runner.rs` — runs child process, multiplexes stdin cancel +
  child stdout via `poll()`
- `helper/src/protocol.rs` — shared JSON types (Command, Response)

## What Changes for Windows

### Protocol — platform-agnostic cancel

The cancel protocol uses spec-aligned names from the OpenJD
`cancelationMethod` schema, not platform-specific signal names:

| Cancel method | Wire format |
|---------------|-------------|
| Graceful (notify then kill) | `{"cancel": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": <secs>}` |
| Immediate kill | `{"cancel": "TERMINATE"}` |

The helper maps these to the appropriate OS primitives internally:
- `NOTIFY_THEN_TERMINATE` → SIGTERM on POSIX, CTRL_BREAK on Windows;
  escalates to SIGKILL / kill_process_tree after the notify period.
- `TERMINATE` → SIGKILL on POSIX, kill_process_tree on Windows.

### Helper binary (`helper/src/runner.rs`) — add `#[cfg(windows)]`

The POSIX runner uses `poll()` on raw fds to multiplex stdin and child stdout.
Windows doesn't have `poll()` on pipe handles. The Windows runner uses two
threads instead:

```
                    Helper process (job-user)
                    ┌─────────────────────────────┐
  stdin ──────────> │  Main thread                │
  (cancel cmds)     │  - reads stdin lines        │
                    │  - on cancel: signal child  │
                    │                             │
                    │  Stdout thread              │
                    │  - reads child stdout       │ ──> stdout
                    │  - sends {"out":...} lines  │     (to session)
                    │                             │
                    │  Child process (job-user)   │
                    │  - CREATE_NEW_PROCESS_GROUP │
                    └─────────────────────────────┘
```

**Child spawning**: Use `std::process::Command` with `CREATE_NEW_PROCESS_GROUP`.
The helper already runs as the target user, so no `CreateProcessAsUserW` needed
inside the helper.

**Cancel handling**:
- `{"cancel": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": <secs>}` →
  `GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, child_pid)`. If the child doesn't
  exit within the notify period, escalates to `kill_process_tree(child_pid)`.
  Works because the helper is in the same user/session as the child.
- `{"cancel": "TERMINATE"}` → `kill_process_tree(child_pid)` using
  `TerminateProcess` on each process in the tree.

**I/O multiplexing**: Two threads sharing a channel:
- Thread 1 (main): reads stdin for cancel commands, signals child on cancel
- Thread 2: reads child stdout line-by-line, sends `{"out":...}` responses
- Main thread joins stdout thread after child exits, then sends `{"exited":...}`

### Helper launch (`cross_user_helper.rs`) — add `#[cfg(windows)]` spawn

Replace `sudo -u <user> -i <helper_path>` with:

```rust
#[cfg(windows)]
pub fn spawn(
    helper_path: &Path,
    user: &dyn SessionUser,
) -> Result<(Self, /* cancel_writer */), SessionError> {
    let wu = user.as_any().downcast_ref::<WindowsSessionUser>().unwrap();
    let spawned = win32::spawn_as_user(
        &[helper_path.to_string_lossy().to_string()],
        &HashMap::new(),  // env
        None,             // working_dir
        wu.password(),
        wu.user(),
        wu.logon_token(),
    )?;
    // spawned.process_handle, spawned.stdout_read, spawned.pid
    // Need to also create a stdin pipe for sending commands
    ...
}
```

**Stdin pipe**: `win32::spawn_as_user` currently only creates a stdout pipe.
It needs to be extended to also create an stdin pipe (or the helper launch
code creates the pipes and passes them to `CreateProcessAsUserW`).

**Cancel writer dup**: Use `DuplicateHandle` instead of `nix::unistd::dup`
to create a second handle to the stdin pipe for sending cancel commands.

### Session integration (`subprocess.rs`) — wire up helper path

The `run_subprocess` function currently has two Windows paths:
1. Same-user: `tokio::process::Command` (works fine)
2. Cross-user: `win32::spawn_as_user` + in-process cancel (broken)

Change path 2 to use the helper via `run_via_helper()`, matching POSIX.
The `run_via_helper` function in `cross_user_helper.rs` is already
platform-agnostic — it just sends JSON and reads JSON.

### `win32::spawn_as_user` — add stdin pipe support

Currently `spawn_as_user` creates a stdout pipe but no stdin pipe. For the
helper, we need bidirectional communication. Add an optional stdin pipe:

```rust
pub struct SpawnedProcess {
    pub process_handle: HANDLE,
    pub pid: u32,
    pub stdout_read: OwnedHandle,
    pub stdin_write: Option<OwnedHandle>,  // NEW
}
```

The stdin pipe is created the same way as stdout (via `CreatePipe` with
inheritable read end), and the read end is passed as `hStdInput` in
`STARTUPINFOW`.

## Implementation Plan

### Step 1: Extend `win32::spawn_as_user` with stdin pipe ✅

Added `spawn_as_user_with_stdin` and `create_stdin_pipe`. The read end goes
to the child's `hStdInput`, the write end is returned in
`SpawnedProcess.stdin_write`.

### Step 2: Add `#[cfg(windows)]` runner in `helper/src/runner_win.rs` ✅

Implemented `run_command` for Windows using two threads for I/O multiplexing.
Handle cancel commands by calling `GenerateConsoleCtrlEvent` or
`kill_process_tree`. Three integration tests pass (echo, cancel, nonexistent).

### Step 3: Add `#[cfg(windows)]` spawn in `cross_user_helper.rs` ✅

Added `CrossUserHelperWin` struct that launches the helper via
`spawn_as_user_with_stdin`. Uses `DuplicateHandle` to create the cancel
writer (analogous to POSIX `dup()`). Extracted `HelperIO` trait so
`run_via_helper` works with both POSIX and Windows helper types.

### Step 4: Wire up in `session.rs` and runner modules ✅

Removed all `#[cfg(unix)]` guards on helper usage. The session now spawns
`CrossUserHelperWin` on Windows (via `helper_binary::write_helper` +
`CrossUserHelperWin::spawn`), passes it to runners, and routes all
cross-user subprocess execution through `run_via_helper`.

Updated `build.rs` to compile the helper binary on Windows too, using
`include_bytes!` just like POSIX.

### Step 5: Remove in-process `send_ctrl_break` for cross-user

The in-process `AttachConsole` + `GenerateConsoleCtrlEvent` code in
`subprocess.rs` is no longer needed for cross-user. It remains as a
fallback for same-user processes (where it works fine). The session's
`cancel_action` now sends platform-agnostic cancel commands
(`NOTIFY_THEN_TERMINATE` or `TERMINATE`) through the helper's stdin pipe.
The helper maps these to the appropriate OS primitives internally.

## Testing

- Unit test: `test_cancel_terminate_on_windows` (already exists, same-user)
- Integration test: helper protocol round-trip (spawn helper, send command,
  read output, send cancel, verify exit)
- E2E: the existing worker agent cancellation tests that currently fail
