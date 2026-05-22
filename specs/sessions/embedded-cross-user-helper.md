# Embedded Cross-User Helper Binary

> **Related**: The [cross-user execution](cross-user.md) document describes the overall
> cross-user design including the `SessionUser` trait, file ownership, and Windows
> support. This document focuses on the persistent helper binary that optimizes
> POSIX cross-user subprocess execution.

## Overview

Eliminate per-action `sudo -i` overhead by embedding a small helper binary in
the `openjd-sessions` crate. The helper is written to disk once at session start,
launched via `sudo -u job-user -i /path/to/helper --auth-token <TOKEN>`, and
persists for the session lifetime. All subprocess execution routes through it
over a stdin/stdout protocol, and every request carries a shared random
authentication token that the helper verifies before acting.

## Architecture

```
Session::with_config(user=job-user)
  │
  ├─ Generate auth_token = 22 chars from the 64-char URL-safe alphabet
  │    (128 bits of entropy, 6 bits per char via byte & 0x3F)
  │
  ├─ Create helpers directory in session working directory
  │    /sessions/<session-id>/.helpers-<uuid>/
  │    mkdir 700, chown :job-user-group, chmod 750
  │
  ├─ Write helper binary with randomized name
  │    /sessions/<session-id>/.helpers-<uuid>/h-<uuid>
  │    chmod 750, chown :job-user-group
  │
  ├─ Spawn: sudo -u job-user -i /sessions/<session-id>/.helpers-<uuid>/h-<uuid> \
  │           --auth-token <TOKEN>
  │    (1 second login cost, paid once; token passed as CLI arg)
  │
  └─ Dup helper stdin fd → cancel_writer (for cancel_action from any thread)
       │
       └─ Helper process (running as job-user)
            ├─ Parses --auth-token from argv at startup
            ├─ Single BufReader on stdin shared between main loop and runner
            ├─ Main loop reads commands, verifies token, dispatches to runner
            ├─ Runner uses poll(2) to multiplex stdin (cancel) + child stdout;
            │  each cancel command must also carry the token
            ├─ Sends exit code when child exits
            └─ Returns to main loop for next command

session.enter_environment(...)   ─── stdin JSON (+token) ──→ helper ──→ fork/exec onEnter
session.run_task(...)            ─── stdin JSON (+token) ──→ helper ──→ fork/exec command
session.cancel_action(...)       ─── cancel_writer (+token) ──→ helper stdin ──→ killpg child
session.cleanup()                ─── {"shutdown":true, "token":...} ──→ helper exits
```

## Embedded Binary

The helper is a small Rust binary (~425KB) compiled for the target platform
and embedded in the `openjd-sessions` crate using `include_bytes!`:

```rust
const HELPER_BINARY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/openjd_helper"));
```

At session start, a restricted helpers directory (`.helpers-<uuid>`) is
created inside the session working directory. The helper binary is written
there with a randomized name (`h-<uuid>`), preventing the job user from
predicting or tampering with the binary path.

### POSIX permissions

The directory starts at 0o700, then group ownership is set to the job
user's group, then permissions are widened to 0o750 (owner rwx, group
r-x). The helper binary inside is chmod'd 0o750 with the same group. The
job user can traverse and execute but cannot write or delete.

### Windows permissions

On Windows, the session working directory's DACL grants the session user
Modify access so that job actions can write files into it. Without
additional protection, the helpers directory and the helper binary would
inherit that Modify ACE and the job user could overwrite the helper —
a session-user → process-user escalation path.

To prevent this, both the helpers directory and the helper binary are
given a **protected DACL** (the `PROTECTED_DACL_SECURITY_INFORMATION`
flag, via `SetNamedSecurityInfoW`) that severs inheritance from the
parent. The DACL contains only:

- Process user → Full Control (inheritable to children)
- Session user → Read & Execute (`FILE_GENERIC_READ | FILE_GENERIC_EXECUTE`
  = `0x1200A9`; no `FILE_WRITE_DATA`, `FILE_APPEND_DATA`, `DELETE`, or
  `FILE_DELETE_CHILD`, inheritable to children)

The binary's own DACL is set separately (not just inherited from the
directory) as defense-in-depth — if the directory's DACL is later
weakened by any means, the binary's own protected DACL still refuses
session-user writes.

This is implemented in `win32_permissions::set_permissions_protected`,
a Rust-only variant of the Python-compatible `set_permissions` that
mirrors the Python API's non-protected behavior.

## Authentication Token

Every helper invocation is scoped to a random per-session token that the
session generates, passes to the helper once on the CLI, and includes in
every subsequent request JSON. The helper verifies the token on every
command and refuses anything it does not recognise.

### Why

The pipe-based IPC alone provides no authentication: any process that can
write to the helper's stdin can execute arbitrary commands as the job user.
Pipe inheritance, the randomized binary path, and the token provide
defense-in-depth against file descriptor leaks, accidental pipe
sharing, and future refactors that might expose the helper's stdin more
broadly.

### Token properties

- **Generated on the session side**, fresh for each helper process, with
  no meaning after that helper exits.
- **128 bits of entropy**, rendered as a 22-character ASCII string drawn
  from a fixed 64-character URL-safe alphabet:

  ```
  ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_
  ```

  Each character is produced by drawing a byte from the OS CSPRNG
  (`getrandom::fill`) and indexing via `byte & 0x3F`. A 64-element
  alphabet makes this uniform without rejection sampling, and each
  character contributes exactly 6 bits of entropy (22 × 6 = 132 bits of
  capacity, ≥ 128 used).
- **Alphabet chosen to be drop-in safe** anywhere the token might appear
  — argv, JSON, log lines, filenames — with no `+` / `/` / `=` /
  whitespace / quoting concerns. It matches URL-safe Base64 without
  padding, but it is used as a character set, not as an encoding; there
  is no decode step.
- **Never logged.** Log lines that would otherwise include the token
  substitute the literal string `<redacted>`.

### Transport to the helper

The token is passed to the helper once as the `--auth-token <TOKEN>` CLI
option:

- POSIX: `sudo -u <user> -i <helper_path> --auth-token <TOKEN>`. `sudo -i`
  forwards the trailing arguments to the helper's argv.
- Windows: `[helper_path, "--auth-token", <TOKEN>]` is passed to
  `win32::spawn_as_user_with_stdin` as the argv.

The helper runs as the job user, and on both platforms process argv is
visible only to that same user — so the token is not disclosed beyond
the existing trust boundary.

If `--auth-token` is missing or malformed, the helper exits immediately
with a nonzero status and a one-line error on stderr, without producing
any JSON.

### Presence in every request

Every JSON object the helper accepts must include a `"token"` field whose
value is the exact 22-character string the helper received on the CLI.
This applies to run commands, cancel commands, and shutdown.

### Verification

The helper compares the incoming token against its own copy in constant
time to avoid timing side-channels. The comparison is hand-rolled (length
check first, then XOR across equal-length bytes, OR into an accumulator,
check zero at the end) rather than pulling in a new dependency; the
helper binary size budget is tight, and the primitive is ~6 lines. The
token's fixed length also means the length check reliably rejects
malformed input before the byte comparison even runs.

On mismatch or missing token, the helper sends
`{"error":"invalid token"}` on stdout and ignores the rest of the request.
The current run (if any) is not cancelled: a cancel command with a bad
token cannot be used as an unauthenticated cancel primitive.

Mismatch handling is "log and ignore," not "exit," for two reasons:

1. The session may have sent a malformed request while a legitimate run is
   in flight; exiting would turn a request-level bug into a session-fatal
   one.
2. Forcing the helper to exit on bad input would let any party with write
   access to the pipe crash the helper — a cheap denial of service.

## Wire Protocol

Newline-delimited JSON over stdin (commands) and stdout (responses). Every
command object MUST include `"token"` set to the value the helper received
from `--auth-token`; the helper refuses objects that lack it or carry the
wrong value. Responses do not carry the token — they travel on a pipe the
helper itself owns, and the session doesn't need to authenticate the
helper (it spawned it).

### Commands (stdin → helper)

```json
{"token": "<TOKEN>", "command": "bash", "args": ["-c", "echo hello"], "env": {"PATH": "/usr/bin"}, "cwd": "/sessions/abc"}

{"token": "<TOKEN>", "cancel": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": 5}

{"token": "<TOKEN>", "cancel": "TERMINATE"}

{"token": "<TOKEN>", "shutdown": true}
```

`<TOKEN>` is the literal 22-character ASCII string described in the
[Authentication Token](#authentication-token) section; it is not a
placeholder for some other encoding.

### Responses (helper → stdout)

```json
{"pid": 1234}

{"out": "hello"}

{"exited": 0}

{"error": "No such file or directory"}

{"error": "invalid token"}
```

Only one command runs at a time. The protocol is implicitly sequential:
send a command, read responses until `exited` or `error`, then send the next.

## Helper Implementation

### Critical design: child stdin isolation

The helper spawns child processes with `stdin(Stdio::null())`. This prevents
child processes (especially interactive shells like `bash`) from consuming
cancel commands intended for the helper. Without this, a script like
`trap 'exit 0' SIGINT; bash; sleep 300` would have the inner `bash` read
cancel messages as shell input, and the helper would never see them.

### Critical design: shared stdin reader

The main loop and runner **must share the same `BufReader`** on stdin. If they
use separate readers, buffering conflicts cause cancel commands to be consumed
by the main loop's buffer and never seen by the runner's poll loop.

The runner also checks `stdin_buf.buffer().is_empty()` before calling `poll(2)`
to handle data already buffered by the main loop's `read_line`.

```rust
// src/helper/main.rs
fn main() {
    // Parse --auth-token from argv. Exit immediately on missing/malformed.
    // Token is a 22-char ASCII string from the URL-safe alphabet.
    let expected_token: [u8; 22] = parse_auth_token_arg_or_exit();

    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());

    loop {
        // Read next command
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 { break; }
        let cmd: Command = serde_json::from_str(line.trim())?;

        // Every command must carry the shared token. Bad token → log and skip.
        if !constant_time_eq(cmd.token_bytes(), &expected_token) {
            send(&Response::Error { error: "invalid token".into() });
            continue;
        }

        match cmd {
            Command::Run(run) => {
                // Pass the SAME reader AND the expected token to run_command;
                // cancel commands read during the run are verified the same way.
                match runner::run_command(&run, &mut reader, &expected_token) {
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
fn run_command(
    cmd: &RunCommand,
    stdin_buf: &mut BufReader<StdinLock>,
    expected_token: &[u8; 22],
) -> Result<i32, String> {
    let mut child = Command::new(&cmd.command)
        .args(&cmd.args)
        .envs(&cmd.env)
        .current_dir(&cmd.cwd)
        .stdin(Stdio::null())                       // prevent child from consuming cancel messages
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .pre_exec(|| { dup2(1, 2); Ok(()) })  // merge stderr → stdout
        .process_group(0)                       // new process group for tree kill
        .spawn()?;

    send(&Response::Pid { pid: child.id() });

    let mut child_buf = BufReader::new(child.stdout.take().unwrap());
    let mut child_killed = false;

    loop {
        // Check BufReader buffer before poll — data may be buffered from main loop
        let stdin_has_buffered = !stdin_buf.buffer().is_empty();
        let timeout = if child_killed { 100ms } else if stdin_has_buffered { skip poll } else { NONE };

        if !stdin_has_buffered { poll([stdin_fd, child_fd], timeout); }

        // Cancel on stdin → parse, verify token, then killpg(child_pgid, signal).
        // A cancel with a bad token is dropped (send {"error":"invalid token"})
        // so an attacker with write access to the pipe cannot unauthenticatedly
        // cancel a running action.
        if stdin ready { read line, parse Command::Cancel(sig), verify token, killpg, child_killed = true }

        // Child output → send {"out": line}
        if child ready { read line, send }

        // Child exited (POLLHUP or try_wait after kill)
        // Kill process group to clean up descendants (e.g. grandchild shells)
        if child done { killpg(child_pgid, SIGKILL); return child.wait().code() }
    }
}
```

Key details:
- `stdin(Stdio::null())` prevents child processes from reading cancel messages
- Checks `stdin_buf.buffer()` before `poll(2)` to handle buffered data
- After `killpg`, uses `try_wait()` with a 100ms poll timeout to detect exit
  even when POLLHUP isn't delivered
- Checks `POLLHUP | POLLERR` for child stdout close
- Drains remaining buffered output before returning exit code
- Kills the child's process group on exit to clean up descendants
- Every cancel line read inside the runner is passed through the same
  constant-time token check the main loop uses. The Windows runner
  (`runner_win.rs`) receives already-validated `CancelMethod` values
  over its `mpsc::Receiver`, so token validation for it stays in
  `main.rs` where stdin is actually read.

### Windows runner: Job Object kill-on-close

The Windows path uses a different mechanism for descendant cleanup
because `TerminateProcess` does not propagate to children, and POSIX-
style `killpg` has no direct equivalent.

At startup, `run_windows()` in `main.rs` creates a Windows
[Job Object][win-job] with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and
assigns the **helper itself** to it. Since Windows 8, every process
spawned by a job member is automatically associated with the job
(jobs can nest and a child inherits all of its parent's jobs by
default). This means every workload subsequently spawned by
`runner_win::run_command` is in the helper's job.

[win-job]: https://learn.microsoft.com/windows/win32/procthread/job-objects

When the helper exits — cleanly via `Shutdown`, abnormally via
`TerminateProcess`, or because the OS killed it for a memory limit —
the kernel closes the helper's last handle to the job, which fires
`KILL_ON_JOB_CLOSE` and terminates every workload still in the job.

This is the cleanup path on Windows. The
`kill_process_tree(child_pid)` walk in `runner_win::handle_cancel`
handles the *cooperative* cancel case (`Cancel::Terminate` /
`NotifyThenTerminate`) and exists primarily so the runner can return a
prompt `exited` response to the session; the Job Object is what
guarantees no leaks on the *abnormal-exit* path.

`runner_win::run_command` also explicitly calls
`AssignProcessToJobObject(job, workload_handle)` after spawn. This is
redundant given the inheritance rule above, but provides defence in
depth: any future change that adds `CREATE_BREAKAWAY_FROM_JOB` to the
workload spawn flags would otherwise silently re-introduce the
descendant-leak failure mode. Re-assigning surfaces a clear
`AssignProcessToJobObject failed` error instead.

If `CreateJobObjectW` or `SetInformationJobObject` fails (e.g. in a
sandbox that forbids job-object operations), the helper logs a one-
line warning to stderr and continues without the guard. The helper
remains functional; it just loses the descendant-cleanup guarantee on
crash, matching the pre-Job-Object behaviour.

## Session Integration

### CrossUserHelper struct

```rust
struct CrossUserHelper {
    child: std::process::Child,
    stdin: BufWriter<std::process::ChildStdin>,
    async_reader: UnixAsyncHelperReader,  // async channel-based reader
    /// 22-char auth token shared with the helper. Included in every
    /// command sent over `stdin` and over the dup'd `cancel_writer`.
    auth_token: String,
}
```

Methods: `spawn`, `send_command`, `shutdown`, `auth_token`.

`spawn` generates the token (22 draws of `alphabet[byte & 0x3F]` over a
fresh 22-byte `getrandom` buffer), formats the helper argv as
`[helper_path, "--auth-token", <TOKEN>]`, and stores the string on the
struct. `send_command` takes the caller's serde_json body and merges
`"token"` into it before writing; callers cannot forget to include it.
`auth_token` exposes the string so `run_via_helper` can include it when
writing cancel commands through the dup'd `cancel_writer` (which doesn't
go through `send_command`).

Reading is done via the `AsyncHelperReader` trait (see below).

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

// In Session::cancel_action (token is the helper's 22-char auth_token):
if let Some(ref mut writer) = self.cancel_writer {
    writeln!(
        writer,
        r#"{{"token":"{token}","cancel":"NOTIFY_THEN_TERMINATE","notifyPeriodInSeconds":5}}"#,
        token = self.helper_auth_token,
    )?;
    writer.flush()?;
}
```

The session stores the token string alongside the `cancel_writer` so the
cancel path doesn't depend on the helper struct (which may be moved into
a runner). The token is copied once at helper spawn and never mutated.
Because the alphabet excludes `"` and `\`, the token can be embedded
verbatim in the JSON literal — no escaping required.

### Timeout

Action timeouts use `tokio::time::sleep` in a `select!` branch within the
async `run_via_helper`. When the timeout fires, a cancel command is written
to the helper's stdin via the `cancel_writer`. No OS thread needed.

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

### Async helper I/O

Helper stdout is read asynchronously so that `drive_action`'s `select!` loop
can process `ActionMessage`s (progress, status, env vars) in real-time while
the helper runs. Without this, `openjd_progress:` and other messages would be
buffered until the action completes.

Both platforms use a reader thread + `tokio::sync::mpsc` channel:

```rust
trait AsyncHelperReader: Send {
    fn next_response(&mut self) -> NextResponseFuture<'_>;
}

struct UnixAsyncHelperReader {
    rx: tokio::sync::mpsc::UnboundedReceiver<Result<Value, SessionError>>,
    _thread: Option<JoinHandle<()>>,
}

impl UnixAsyncHelperReader {
    fn new(stdout: ChildStdout) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let thread = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => { if tx.send(serde_json::from_str(&line)).is_err() { break; } }
                    Err(_) => break,
                }
            }
        });
        Self { rx, _thread: Some(thread) }
    }
}
```

The reader thread performs blocking I/O on the anonymous pipe. The async code
receives from the channel with `.recv().await`, which yields to the tokio
runtime between responses. This allows `drive_action`'s `select!` loop to
interleave message processing naturally.

Windows uses the identical pattern (`WindowsAsyncHelperReader`) over its
anonymous pipe, preserving the same security model as `std::process::Stdio::piped()`.

### Async `run_via_helper`

`run_via_helper` is an `async fn` that reads from the `AsyncHelperReader` and
handles timeouts via `tokio::time::sleep` in a `select!` branch:

```rust
async fn run_via_helper(
    helper: &mut dyn AsyncHelper,
    config: &SubprocessConfig,
    filter: &mut ActionFilter,
    session_id: &str,
    message_tx: UnboundedSender<ActionMessage>,
    cancel_writer: Option<&File>,
) -> Result<SubprocessResult, SessionError> {
    helper.send_command(&cmd)?;

    let timeout_fut = match config.timeout {
        Some(d) => tokio::time::sleep(d).boxed(),
        None => futures_util::future::pending().boxed(),
    };
    tokio::pin!(timeout_fut);

    loop {
        tokio::select! {
            resp = helper.async_reader().next_response() => {
                // process pid/out/exited/error, feed through ActionFilter
            }
            _ = &mut timeout_fut, if !timed_out => {
                // write cancel to helper via cancel_writer
            }
        }
    }
}
```

No `block_in_place` or `spawn_blocking` needed — the function is fully async.

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
# test result: ok. 12 passed; 0 failed
```

Tests cover: basic execution, stdout streaming, notify-then-terminate cancel,
immediate terminate cancel, process tree kill, uid/gid verification,
env vars, env isolation, cleanup, permissions, and disjoint user rejection.

Additional token-focused tests added in `helper/tests/test_helper_protocol.rs`:

- **`test_helper_rejects_missing_auth_token_cli`** — spawn the helper with
  no `--auth-token` argument; expect a nonzero exit and a diagnostic on
  stderr, with no JSON on stdout.
- **`test_helper_rejects_missing_token_field`** — send a run command
  without a `"token"` field; expect `{"error":"invalid token"}` and no
  child spawn.
- **`test_helper_rejects_wrong_token`** — send a run command with the
  wrong token; expect `{"error":"invalid token"}` and that a legitimate
  request with the correct token afterwards still works (the helper stays
  alive, stream stays aligned).
- **`test_helper_cancel_rejects_wrong_token`** — start a long-running
  command with the correct token, send a cancel with the wrong token,
  verify the command is still running, then send a cancel with the
  correct token and verify the command exits.
- **`test_helper_happy_path_includes_token`** — the pre-existing echo /
  terminate / nonexistent-command tests updated to include the token on
  every request.

## Resolved Decisions

- **stderr**: Merged into stdout via `dup2(1, 2)` in `pre_exec`
- **Helper crash**: Fail the session (no restart)
- **Signal forwarding**: `killpg` on child process group, matching non-cross-user path
- **Environment**: Job-user login env from `sudo -i` + per-action overrides
- **Stdin sharing**: Single `BufReader` shared between main loop and runner (critical for cancel)
- **Cancel mechanism**: Dup'd stdin fd on Session, writable from any thread
- **Authentication**: Random 128-bit per-helper token rendered as 22 ASCII
  characters from a 64-char URL-safe alphabet (direct `byte & 0x3F` draw,
  no rejection sampling, no encoder dependency); passed once via
  `--auth-token` CLI option, echoed on every request; verified in
  constant time; missing/bad token rejected without affecting the
  current run
- **Token transport**: CLI argv
- **Shutdown wire form**: `{"token":"...","shutdown":true}` object, so
  shutdown carries a token like every other command

## Build System

### Crate structure

```
openjd-sessions/
  Cargo.toml            ← build = "build.rs"
  build.rs              ← compiles helper, copies to OUT_DIR
  src/
    lib.rs              ← #[cfg(unix)] pub(crate) mod helper_binary;
    helper_binary.rs    ← include_bytes! + create_helpers_dir() + write_helper()
    session.rs          ← CrossUserHelper, run_via_helper, cancel_writer
    helper/
      Cargo.toml        ← standalone crate with [workspace] = {}
      src/
        main.rs         ← argv parse (--auth-token), shared stdin reader,
                          command dispatch with token verification
        protocol.rs     ← Command/Response serde types, token field
        runner.rs       ← poll(2) loop, child management, cancel with
                          per-command token check
```

### build.rs

Compiles the helper as a standalone binary via `cargo build --release`,
targeting `$OUT_DIR/helper_build` with `--target $TARGET` for cross-compilation.
On non-unix targets, writes an empty placeholder so `include_bytes!` doesn't fail.

### Binary size

425KB with `opt-level = "s"`, LTO, strip, `panic = "abort"`.
Dependencies: serde, serde_json, nix. The token comparison is hand-rolled
(no `subtle` or similar crate) to keep the dependency set unchanged.

### Session-side dependencies

The session generates the token with the OS CSPRNG. `openjd-sessions`
already depends on `uuid` with the `v4` feature, which pulls in
`getrandom` transitively. The token generation uses `getrandom::fill`
directly on a `[u8; 22]` stack buffer, then indexes each byte
(`byte & 0x3F`) into the 64-char URL-safe alphabet to produce the final
22-char `String`. No `base64`, `rand`, or `subtle` crate is added.

### PyO3 integration

`include_bytes!` embeds the helper in the `.so` file. No separate binary
to distribute — extracted to session working directory at runtime.
