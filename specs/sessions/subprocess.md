# Async Subprocess Execution

## Overview

`subprocess.rs` implements async subprocess execution via `tokio::process::Command`. It
spawns a child process, streams stdout/stderr through the `ActionFilter` in real time,
sends parsed `ActionMessage` values through an mpsc channel, and handles cancelation,
timeout, and cross-user execution.

This document covers the internal `run_subprocess` function used by script runners. For
the public `Session::run_subprocess` method (ad-hoc subprocess execution for the worker
agent), see [session.md § Ad-hoc Subprocess](session.md#ad-hoc-subprocess).

## SubprocessConfig

```rust
pub struct SubprocessConfig {
    pub args: Vec<String>,              // Command + arguments
    pub env_vars: HashMap<String, String>, // Full environment variable map
    pub working_dir: PathBuf,           // Working directory for the subprocess
    pub timeout: Option<Duration>,      // Action timeout (None = no timeout)
    pub user: Option<PosixSessionUser>, // Cross-user execution target
    pub cancel_method: CancelMethod,    // Terminate vs NotifyThenTerminate
    pub cancel_request_rx: Option<tokio::sync::oneshot::Receiver<CancelRequest>>,
}
```

## run_subprocess

```rust
pub async fn run_subprocess(
    config: SubprocessConfig,
    filter: &mut ActionFilter,
    session_id: &str,
    message_tx: mpsc::UnboundedSender<ActionMessage>,
) -> Result<SubprocessResult, SessionError>
```

### Why it takes `message_tx` instead of a callback

The Python library passes a callback that fires on each directive. This works in Python
because the callback can close over mutable session state (GIL protects concurrent access).

In Rust, a callback that mutates `Session` would require `Arc<Mutex<Session>>` or similar
interior mutability. Instead, the subprocess sends `ActionMessage` values through an mpsc
channel, and the `Session::drive_action()` method receives them with `&mut self`. This
avoids shared mutable state entirely.

### Process Group Isolation

On POSIX, the subprocess is placed in its own process group via `setsid` in a `pre_exec`
hook:

```rust
unsafe {
    command.pre_exec(|| {
        nix::unistd::setsid().map_err(|e| std::io::Error::new(...))?;
        Ok(())
    });
}
```

This mirrors Python's `start_new_session=True` and ensures that signals sent to the
process group don't propagate to the parent (the session runtime). It also enables
killing the entire process tree via `killpg`.

For cross-user execution, `setsid` is not set in `pre_exec` because sudo's child creates
its own process group. The `setsid -w` flag in the sudo command handles this instead.

### Stderr Handling

Stderr is merged into stdout processing via `dup2` in a `pre_exec` hook:

```rust
unsafe {
    command.pre_exec(|| {
        nix::unistd::dup2(1, 2)?; // stderr → stdout
        Ok(())
    });
}
```

This matches the Python library's behavior and ensures all output flows through the
`ActionFilter` for directive parsing. A separate stderr pipe would require coordinating
two streams and deciding which gets priority — merging avoids this complexity.

### Stdout Streaming Loop

```rust
let stdout = child.stdout.take().unwrap();
let reader = BufReader::new(stdout);
let mut lines = reader.lines();

loop {
    tokio::select! {
        biased;

        // Cancelation has highest priority
        cancel = &mut cancel_rx => { /* send SIGTERM/SIGKILL */ }

        // Timeout has second priority
        _ = &mut timeout_sleep => { /* send SIGKILL */ }

        // Stdout processing
        line = lines.next_line() => {
            match line {
                Ok(Some(line)) => {
                    let (messages, pass_through, modified) = filter.filter_message(&line);
                    for msg in messages {
                        let _ = message_tx.send(msg);
                    }
                    if pass_through {
                        session_log!(INFO, session_id, LogContent::COMMAND_OUTPUT, "{}", modified);
                    }
                }
                Ok(None) => break, // EOF
                Err(_) => break,
            }
        }
    }
}
```

### Why `biased` in select!

The `tokio::select!` macro is `biased`, meaning branches are checked in order rather than
randomly. This ensures:

1. Cancel requests are processed immediately, even if stdout has buffered data
2. Timeouts fire promptly, not delayed by a burst of stdout lines
3. Stdout is processed only when no higher-priority event is pending

Without `biased`, a flood of stdout could starve cancel/timeout handling due to tokio's
random branch selection.

### 64KB Line Length Limit

Lines longer than 64KB are truncated. This prevents a misbehaving subprocess from
consuming unbounded memory. The Python library has the same limit.

### 5-Second Stdout Grace Time

After the child process exits, grandchild processes may still hold stdout open. The
subprocess waits up to 5 seconds for EOF on stdout before proceeding:

```rust
match tokio::time::timeout(Duration::from_secs(5), drain_remaining_lines(&mut lines)).await {
    Ok(_) => { /* all lines read */ }
    Err(_) => { /* timeout, proceed anyway */ }
}
```

This matches the Python library's behavior and prevents the session from hanging
indefinitely on orphaned processes.

## Signal Delivery

### Same-user

| Signal | Method | Purpose |
|--------|--------|---------|
| SIGTERM | `nix::sys::signal::killpg(pgid, SIGTERM)` | Notify (graceful shutdown) |
| SIGKILL | `nix::sys::signal::killpg(pgid, SIGKILL)` | Terminate (forced kill) |

Signals are sent to the process group (`killpg`), not the individual process. This
ensures child processes spawned by the action are also signaled.

### Cross-user

For processes running as a different user, direct signal delivery may fail with EPERM.
The fallback chain is:

1. Try direct `killpg()` — works if the process has CAP_KILL
2. Fall back to `sudo -u <user> -i kill -s <signal> -- -<pgid>`

The process group ID for the sudo child is found via `find_sudo_child_pgid()`, which
reads `/proc/<pid>/task/*/children` (Linux procfs) with a `pgrep -P` fallback.

## Cancelation Flow

### CancelMethod::Terminate

Immediate SIGKILL to the process group. Used for actions with `TerminateCancelMethod`.

### CancelMethod::NotifyThenTerminate

1. Write `cancel_info.json` to the session working directory (per
   [§5.3.2 `<CancelationMethodNotifyThenTerminate>`](https://github.com/OpenJobDescription/openjd-specifications/wiki/2023-09-Template-Schemas#532-cancelationmethodnotifythenterminate)):
   ```json
   {"NotifyEnd": "<yyyy>-<mm>-<dd>T<hh>:<mm>:<ss>Z"}
   ```
   The `NotifyEnd` value is an ISO 8601 UTC timestamp indicating when the notify
   period will end and the process will be forcefully terminated.
2. Send SIGTERM to the process (or process group)
3. Start a grace period timer (default: 120s for `onRun`, 30s for env scripts)
4. If the process doesn't exit within the grace period, send SIGKILL

The `cancel_info.json` file allows the action script to read the deadline and perform
graceful shutdown (save state, flush buffers, etc.).

## Cross-User Subprocess Launch

When `SubprocessConfig.user` is set and the user differs from the process user:

1. Generate a shell script wrapper:
   ```bash
   #!/bin/bash
   export VAR1='value1'
   export VAR2='value2'
   cd '/path/to/working/dir'
   exec /path/to/command arg1 arg2
   ```
2. Write the script to a temp file in the session working directory
3. Set permissions to allow the target user to execute it
4. Launch via `sudo -u <user> -i setsid -w <script_path>`

### Why a shell script wrapper

Environment variables can't be passed directly through `sudo -i` because the login shell
resets the environment. The Python library uses the same approach — generating a script
that exports env vars, changes directory, and execs the command.

The `shlex` crate is used to safely quote values in the generated script, preventing
shell injection.

## SubprocessResult

```rust
pub struct SubprocessResult {
    pub state: ActionState,    // Success, Failed, Canceled, Timeout
    pub exit_code: Option<i32>,
    pub stdout: String,        // Captured stdout (for non-streaming callers)
}
```

The `stdout` field captures all output for backward compatibility with the synchronous
API path. In the async path, stdout is streamed via the channel and this field is
typically empty.
