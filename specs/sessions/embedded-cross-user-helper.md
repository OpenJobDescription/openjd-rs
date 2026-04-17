# Embedded Cross-User Helper Binary

## Overview

Eliminate per-action `sudo -i` overhead by embedding a small helper binary in
the `openjd-sessions` crate. The helper is written to disk once at session start,
launched via `sudo -u job-user -i /path/to/helper`, and persists for the session
lifetime. All subprocess execution routes through it over a stdin/stdout protocol.

## Architecture

```
Session::with_config(user=job-user)
  │
  ├─ Write helper binary to session working directory
  │    /sessions/<session-id>/openjd_helper
  │    chmod 750, chown :job-user-group
  │
  ├─ Spawn: sudo -u job-user -i /sessions/<session-id>/openjd_helper
  │    (1 second login cost, paid once)
  │
  └─ Dup helper stdin fd → cancel_writer (for cancel_action from any thread)
       │
       └─ Helper process (running as job-user)
            ├─ Single BufReader on stdin shared between main loop and runner
            ├─ Main loop reads commands, dispatches to runner
            ├─ Runner uses poll(2) to multiplex stdin (cancel) + child stdout
            ├─ Sends exit code when child exits
            └─ Returns to main loop for next command

session.enter_environment(...)   ─── stdin JSON ──→ helper ──→ fork/exec onEnter
session.run_task(...)            ─── stdin JSON ──→ helper ──→ fork/exec command
session.cancel_action(...)       ─── cancel_writer ──→ helper stdin ──→ killpg child
session.cleanup()                ─── "shutdown" ──→ helper exits
```

## Embedded Binary

The helper is a small Rust binary (~425KB) compiled for the target platform
and embedded in the `openjd-sessions` crate using `include_bytes!`:

```rust
const HELPER_BINARY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/openjd_helper"));
```

At session start, written to the session working directory, cleaned up
automatically when the working directory is deleted.

## Wire Protocol

Newline-delimited JSON over stdin (commands) and stdout (responses).

### Commands (stdin → helper)

```json
{"command": "bash", "args": ["-c", "echo hello"], "env": {"PATH": "/usr/bin"}, "cwd": "/sessions/abc"}

{"cancel": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": 5}

{"cancel": "TERMINATE"}

"shutdown"
```

### Responses (helper → stdout)

```json
{"pid": 1234}

{"out": "hello"}

{"exited": 0}

{"error": "No such file or directory"}
```

Only one command runs at a time. The protocol is implicitly sequential:
send a command, read responses until `exited` or `error`, then send the next.

## Helper Implementation

### Critical design: shared stdin reader

The main loop and runner **must share the same `BufReader`** on stdin. If they
use separate readers, buffering conflicts cause cancel commands to be consumed
by the main loop's buffer and never seen by the runner's poll loop.

```rust
// src/helper/main.rs
fn main() {
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());

    loop {
        // Read next command
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 { break; }
        let cmd: Command = serde_json::from_str(line.trim())?;

        match cmd {
            Command::Run(run) => {
                // Pass the SAME reader to run_command for cancel reads
                match runner::run_command(&run, &mut reader) {
                    Ok(code) => send(&Response::Exited { exited: code }),
                    Err(e) => send(&Response::Error { error: e }),
                }
            }
            Command::Shutdown => break,
            Command::Cancel(_) => {} // only meaningful inside run_command
        }
    }
}
```

### Runner: poll(2) for concurrent stdin + child stdout

```rust
// src/helper/runner.rs
fn run_command(cmd: &RunCommand, stdin_buf: &mut BufReader<StdinLock>) -> Result<i32, String> {
    let mut child = Command::new(&cmd.command)
        .args(&cmd.args)
        .envs(&cmd.env)
        .current_dir(&cmd.cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .pre_exec(|| { dup2(1, 2); Ok(()) })  // merge stderr → stdout
        .process_group(0)                       // new process group for tree kill
        .spawn()?;

    send(&Response::Pid { pid: child.id() });

    let mut child_buf = BufReader::new(child.stdout.take().unwrap());
    let mut child_killed = false;

    loop {
        // Short poll timeout after kill to detect exit via try_wait
        let timeout = if child_killed { 100ms } else { NONE };
        poll([stdin_fd, child_fd], timeout);

        // Cancel on stdin → parse as Command::Cancel(signal), killpg(child_pgid, signal)
        if stdin ready { read line, parse Command::Cancel(sig), killpg, child_killed = true }

        // Child output → send {"out": line}
        if child ready { read line, send }

        // Child exited (POLLHUP or try_wait after kill)
        if child done { return child.wait().code() }
    }
}
```

Key details:
- After `killpg`, uses `try_wait()` with a 100ms poll timeout to detect exit
  even when POLLHUP isn't delivered
- Checks `POLLHUP | POLLERR` for child stdout close
- Drains remaining buffered output before returning exit code

## Session Integration

### CrossUserHelper struct

```rust
struct CrossUserHelper {
    child: std::process::Child,                    // the sudo process
    stdin: BufWriter<std::process::ChildStdin>,    // for sending commands
    stdout: BufReader<std::process::ChildStdout>,  // for reading responses
}
```

Methods: `spawn`, `send_command`, `read_response`, `shutdown`.

### Cancel via dup'd stdin fd

At spawn time, the helper's stdin fd is `dup()`d into a separate `File` handle
stored as `Session.cancel_writer`. This allows `cancel_action` to write cancel
commands to the helper's stdin from any thread, even while the helper struct
is owned by a runner during action execution.

```rust
// In CrossUserHelper::spawn:
let raw_fd = child_stdin.as_raw_fd();
let dup_fd = nix::unistd::dup(raw_fd)?;
let cancel_writer = File::from_raw_fd(dup_fd);

// In Session::cancel_action:
if let Some(ref mut writer) = self.cancel_writer {
    writeln!(writer, "{{\"cancel\":\"NOTIFY_THEN_TERMINATE\",\"notifyPeriodInSeconds\":5}}")?;
    writer.flush()?;
}
```

### Timeout

Action timeouts use the same cancel_writer mechanism via a timer thread.
A `Condvar` ensures the thread stops immediately when the command completes,
preventing orphaned timeout threads from cancelling subsequent commands:

```rust
if let Some(timeout) = config.timeout {
    let mut writer = cancel_writer.try_clone()?;
    let done = done.clone(); // Arc<(Mutex<bool>, Condvar)>
    std::thread::spawn(move || {
        let (lock, cvar) = &*done;
        let guard = lock.lock().unwrap();
        let (guard, _) = cvar.wait_timeout_while(guard, timeout, |d| !*d).unwrap();
        if *guard { return; } // command finished before timeout
        writeln!(writer, "{{\"cancel\":\"NOTIFY_THEN_TERMINATE\",\"notifyPeriodInSeconds\":5}}");
        // Grace period — also cancellable
        let guard = lock.lock().unwrap();
        let (guard, _) = cvar.wait_timeout_while(guard, 5s, |d| !*d).unwrap();
        if *guard { return; }
        writeln!(writer, "{{\"cancel\":\"TERMINATE\"}}");
    });
}
// DoneGuard sets *done = true and notifies the condvar on any return path
```

### Helper ownership during actions

The helper struct is moved to runners via a take/give-back pattern to avoid
borrow conflicts with `&mut self` on Session:

```rust
// In Session::enter_environment:
let mut runner = EnvironmentScriptRunner::new(...)
    .with_helper(self.helper.take().unwrap());

let result = self.drive_action(runner.enter(...), &mut rx, &id).await?;

self.helper = runner.take_helper();
```

### Shared run_via_helper function

A `pub(crate) fn run_via_helper()` function contains the helper communication
logic (send command, read responses, feed through ActionFilter). Used by both
`Session::run_subprocess_via_helper` and the script runners.

Both call sites wrap `run_via_helper` in `tokio::task::block_in_place` to avoid
blocking the async runtime during the synchronous helper I/O loop. This tells
the multi-thread runtime to move other tasks to different worker threads while
the current thread performs blocking I/O. Unlike `spawn_blocking`, `block_in_place`
works with `&mut` references since it runs on the current thread.

### Routing

All action methods route through the helper when available:
- `Session::run_subprocess` — checks `self.helper.is_some()`, calls `run_subprocess_via_helper`
- `EnvironmentScriptRunner::enter/exit` — checks `self.helper.is_some()`, calls `run_via_helper`
- `StepScriptRunner::run` — checks `self.helper.is_some()`, calls `run_via_helper`

The existing per-action sudo path remains as fallback for non-helper sessions.

## Performance

| Metric | Current (sudo per action) | With helper |
|--------|--------------------------|-------------|
| First action startup | ~1.0s | ~1.0s (one-time) |
| Subsequent action startup | ~1.0s | ~1ms (pipe write) |
| 20-action session overhead | ~20s | ~1s |
| Binary write to disk | N/A | ~1ms (425KB) |

## Testing

All 13 cross-user tests in `tests/test_cross_user.rs` pass in the Docker
localuser container, validating the helper end-to-end:

```bash
cd ~/openjd-rs
docker build -t openjd-cross-user -f testing_containers/localuser_sudo_environment/Dockerfile .
docker run --rm openjd-cross-user
# test result: ok. 13 passed; 0 failed
```

Tests cover: basic execution, stdout streaming, notify-then-terminate cancel,
immediate terminate cancel, process tree kill, CAP_KILL, uid/gid verification,
env vars, env isolation, cleanup, permissions, and disjoint user rejection.

## Resolved Decisions

- **stderr**: Merged into stdout via `dup2(1, 2)` in `pre_exec`
- **Helper crash**: Fail the session (no restart)
- **Signal forwarding**: `killpg` on child process group, matching non-cross-user path
- **Environment**: Job-user login env from `sudo -i` + per-action overrides
- **Stdin sharing**: Single `BufReader` shared between main loop and runner (critical for cancel)
- **Cancel mechanism**: Dup'd stdin fd on Session, writable from any thread

## Build System

### Crate structure

```
openjd-sessions/
  Cargo.toml            ← build = "build.rs"
  build.rs              ← compiles helper, copies to OUT_DIR
  src/
    lib.rs              ← #[cfg(unix)] pub(crate) mod helper_binary;
    helper_binary.rs    ← include_bytes! + write_helper()
    session.rs          ← CrossUserHelper, run_via_helper, cancel_writer
    helper/
      Cargo.toml        ← standalone crate with [workspace] = {}
      src/
        main.rs         ← shared stdin reader, command dispatch
        protocol.rs     ← Command/Response serde types
        runner.rs       ← poll(2) loop, child management, cancel
```

### build.rs

Compiles the helper as a standalone binary via `cargo build --release`,
targeting `$OUT_DIR/helper_build` with `--target $TARGET` for cross-compilation.
On non-unix targets, writes an empty placeholder so `include_bytes!` doesn't fail.

### Binary size

425KB with `opt-level = "s"`, LTO, strip, `panic = "abort"`.
Dependencies: serde, serde_json, nix.

### PyO3 integration

`include_bytes!` embeds the helper in the `.so` file. No separate binary
to distribute — extracted to session working directory at runtime.
