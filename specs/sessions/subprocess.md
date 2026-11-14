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
    pub env_vars: HashMap<String, Option<String>>, // Full environment variable map (None = unset)
    pub working_dir: Option<PathBuf>,   // Working directory for the subprocess
    pub timeout: Option<Duration>,      // Action timeout (None = no timeout)
    pub user: Option<Arc<dyn SessionUser>>, // Cross-user execution target
    pub cancel_method: CancelMethod,    // Terminate vs NotifyThenTerminate
    pub cancel_request_rx: Option<tokio::sync::watch::Receiver<Option<Duration>>>,
    /// Whether to accumulate stdout into `SubprocessResult.stdout`.
    /// Intended for debugging only.
    pub debug_collect_stdout: bool,
}
```

## run_subprocess

```rust
pub async fn run_subprocess(
    config: SubprocessConfig,
    filter: &mut ActionFilter,
    session_id: &str,
    message_tx: mpsc::UnboundedSender<ActionMessage>,
    cancel_token: CancellationToken,
) -> Result<SubprocessResult, SessionError>
```

### Why it takes `message_tx` instead of a callback

The Python library passes a callback that fires on each directive. This works in Python
because the callback can close over mutable session state (GIL protects concurrent access).

In Rust, a callback that mutates `Session` would require `Arc<Mutex<Session>>` or similar
interior mutability. Instead, the subprocess sends `ActionMessage` values through an mpsc
channel, and the `Session::drive_action()` method receives them with `&mut self`. This
avoids shared mutable state entirely.

### Windows executable resolution

On Windows, `run_subprocess` resolves `args[0]` to an absolute path before
spawning, via `win32_locate::locate_windows_executable` (see
[win32-locate.md](win32-locate.md)): PATHEXT-aware search over
`{working_dir};{action PATH}`, earliest directory wins, hard
`SubprocessStart` error ("Could not find executable file: …") when there is
no match. This gives canonical Windows search semantics (a `.bat` earlier in
PATH beats an `.exe` later) and prevents `CreateProcessW`'s legacy fallback
search from silently resolving commands through the worker process's own
PATH, application directory, or system directories. The cross-user path
performs the same resolution inside the helper — as the target user — see
[win32-locate.md](win32-locate.md).

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
let mut reader = BufReader::new(stdout);
let mut line_buf = Vec::new();

loop {
    tokio::select! {
        biased;

        // Cancelation has highest priority
        _ = cancel_token.cancelled() => { /* send SIGTERM/SIGKILL */ }

        // Timeout has second priority (continues draining, does not break)
        _ = &mut timeout_sleep => { /* send SIGKILL, continue reading until EOF */ }

        // Stdout processing: reads raw bytes, escapes undecodable ones
        n = reader.read_until(b'\n', &mut line_buf) => {
            match n {
                Ok(0) => break, // EOF
                Ok(_) => {
                    if line_buf.last() == Some(&b'\n') { line_buf.pop(); }
                    let line = decode_backslashreplace(&line_buf);
                    let line = truncate_line(&line).to_string();
                    line_buf.clear();
                    let (callbacks, pass_through, modified) = filter.filter_message(&line, session_id);
                    for cb in callbacks {
                        let msg = map_callback_to_message(cb);
                        let _ = message_tx.send(msg);
                    }
                    if pass_through {
                        session_log!(info, session_id, LogContent::COMMAND_OUTPUT, "{}", modified);
                    }
                }
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

### Decoding Undecodable Output Bytes

A subprocess can write bytes that are not valid UTF-8. The common case is a Windows
DCC application emitting its output in the system code page: for example Unreal
Engine writing the cp1252 em dash `0x97`, which is not a valid UTF-8 start byte.

`decode_backslashreplace` decodes each line as UTF-8 and escapes every byte that is
not valid UTF-8 as `\xNN`, using lowercase hex. Valid UTF-8, including multi-byte
sequences, passes through unmodified, and a fully valid line is returned borrowed
without allocating.

This matches the Python library, which decodes subprocess output with
`errors="backslashreplace"`. Escaping rather than replacing preserves the original
byte values in the session log, which helps identify the code page the subprocess is
emitting. CPython escapes each byte of an undecodable sequence individually, so a
3-byte invalid sequence such as an encoded surrogate produces three escapes
(`\xed\xa0\x80`); the Rust implementation matches this, and the conformance table in
`subprocess.rs` pins the expected output against values generated by CPython.

Escaping expands each undecodable byte from 1 byte to 4 characters, so a line
consisting largely of undecodable bytes can grow up to fourfold before the 64KB
truncation above is applied.

### 5-Second Process Exit Grace Time

After the stdout loop ends (EOF), the subprocess waits up to 5 seconds for the child
process to exit via `child.wait()`. If the process hasn't exited within 5 seconds, it
is forcefully killed:

```rust
match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
    Ok(Ok(status)) => { /* normal exit */ }
    Ok(Err(_)) => { /* wait error, send SIGKILL */ }
    Err(_) => { /* timeout, send SIGKILL */ }
}
```

This prevents the session from hanging indefinitely on processes that ignore signals
or are stuck in uninterruptible I/O.

### Why 5 seconds

The 5-second grace period balances two concerns: (1) giving the process enough time to
flush buffers and exit cleanly after stdout closes, and (2) not blocking the session
for an unreasonable time if the process is stuck. Most well-behaved processes exit
within milliseconds of stdout closing. The 5-second limit catches processes that are
stuck in uninterruptible I/O or have leaked file descriptors keeping stdout open.
The Python library uses the same timeout.

## Signal Delivery

### Same-user

| Signal | Method | Purpose |
|--------|--------|---------|
| SIGTERM | `nix::sys::signal::killpg(pgid, SIGTERM)` | Notify (graceful shutdown) |
| SIGKILL | `nix::sys::signal::killpg(pgid, SIGKILL)` | Terminate (forced kill) |

Signals are sent to the process group (`killpg`), not the individual process. This
ensures child processes spawned by the action are also signaled.

### Cross-user

Cross-user signal delivery is handled entirely by the embedded cross-user helper
(see `cross-user.md` and `embedded-cross-user-helper.md`). The helper process runs
as the target user and signals its child's process group locally via `killpg`,
so the worker-agent process never needs to deliver cross-user signals and no
special capabilities (e.g. `CAP_KILL`) are required.

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
   #!/bin/sh
   export VAR1='value1'
   export VAR2='value2'
   cd '/path/to/working/dir'
   exec /path/to/command arg1 arg2
   ```
2. Write the script to a randomized path in the helpers directory
   (`<helpers_dir>/_run_<uuid>.sh`). The helpers directory is 0o750
   (owner rwx, group r-x), preventing the job user from modifying
   scripts between write and execution.
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

When `SubprocessConfig.debug_collect_stdout` is `false` (the default), the `stdout` field is
always empty — lines are still streamed through the filter and channel in real time.

## Internal Helper Functions

### format_command_for_log

```rust
pub(crate) fn format_command_for_log(args: &[String]) -> String
```

Formats a command argument list for log output using `shlex::try_join` for proper
quoting. Redacts any `openjd_redacted_env` values that appear in the command line
before logging, preventing secrets from appearing in log output.

### process_line

```rust
pub(crate) fn process_line(
    line: &str,
    filter: &mut ActionFilter,
    session_id: &str,
    message_tx: &UnboundedSender<ActionMessage>,
    saw_fail: &mut bool,
) -> (String, bool)
```

Processes a single stdout line through the `ActionFilter`, maps `FilterCallback` values
to `ActionMessage` variants, and sends them through the mpsc channel. Returns the
(possibly redacted) display string and whether the line should be passed through to
logging. Sets `saw_fail` when an `openjd_fail` directive is encountered.

This function is shared between `run_subprocess` (async path) and `run_via_helper`
(cross-user synchronous path).

## Windows Signal Delivery

### CTRL_BREAK_EVENT

Windows has no POSIX signals. For graceful cancellation (`NotifyThenTerminate`), the
subprocess module sends `CTRL_BREAK_EVENT` via the Win32 console API:

1. Detach from the current console (`FreeConsole`)
2. Attach to the target process's console (`AttachConsole(pid)`)
3. Send `GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid)`
4. Detach and re-attach to the parent console

When running as a Windows service (Session 0), console APIs don't work reliably.
The code detects this via `is_session_zero()` and falls back to immediate termination.

### Process Tree Killing

Windows doesn't have process groups like POSIX. Termination uses
`CreateToolhelp32Snapshot` to enumerate all processes, find descendants of the target
PID, and kill them leaf-to-root via `TerminateProcess`. This mirrors Python's
`_windows_process_killer.py`.

Two Windows tree kills follow the rules below: the in-process one
(`platform::kill_process_tree` in `subprocess.rs`) and the cross-user helper's
(`kill_process_tree` in `src/helper/src/runner_win.rs`, see
[windows-cross-user-helper.md](../windows-cross-user-helper.md)). In the helper
the Job Object is the backstop for anything the walk misses.

#### Why the walk validates against PID reuse

`th32ParentProcessID` is recorded when a child is created and is never updated
when the parent exits, and Windows reuses PIDs. Without validation, a live
process whose dead creator's PID now belongs to a process in the tree looks like
a descendant and would be killed. Reuse can also make the recorded parent graph
cyclic, when a dead ancestor's PID is reused by one of its descendants. An
earlier recursive walk had no cycle guard and overflowed its thread's stack
(`0xc00000fd`) on such a cycle.

The validation mirrors the PID-reuse guard in psutil's `Process.children()`,
which the Python implementation relies on and the first Rust port dropped.

Every rule here **fails closed**: when a process's identity can't be
established, it is not killed. The failure mode the rules accept is an
under-kill (a descendant left running), never terminating an unrelated process.

#### Snapshot and stamp

The walk reads one `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` as a list of
`(pid, ppid)` pairs, and takes a wall-clock stamp right after the call returns,
before enumerating it. The snapshot copies the process list at call time and
`Process32FirstW`/`Process32NextW` iterate that frozen copy, so every listed
process was created before the stamp. Taking the stamp before the enumeration,
not after, keeps the window in which a reused PID can pass the
[edge check](#edge-validation) as small as it can soundly be. A failed snapshot
yields no pairs and a stamp of 0.

The stamp is in `FILETIME` ticks (100 ns since 1601-01-01 UTC), the base
`GetProcessTimes` reports creation times in, so the two compare directly. It is
computed from `std::time::SystemTime` rather than `GetSystemTimeAsFileTime`,
which reads the same UTC clock and needs no extra `windows` crate feature.

#### Root identity

`collect_tree_validated(root_pid, parents, snapshot_time, expected_root_ct,
creation_time)` first settles the root's creation time. `expected_root_ct` is a
pinned identity read before the call; `None` means the caller pins the PID
another way.

| `expected_root_ct` | Fresh root read | Result |
|---|---|---|
| `Some(exp)` | `Some(exp)` | Walk from the root, recorded with `exp`. |
| `Some(exp)` | `Some(other)` | Empty: the PID was reassigned. |
| `Some(exp)` | `None` | Walk from the root, recorded with `exp`. |
| `None` | `Some(rc)` | Walk from the root, recorded with `rc`. |
| `None` | `None` | Empty: nothing to validate against. |

- **Mismatch aborts instead of seeding.** When the fresh read differs from the
  pin, the PID belongs to a new live process. Its children were created after
  `exp` and before the snapshot, so they would pass every edge check, and
  seeding the walk would kill them. Aborting only leaks our own orphans.
- **Exited root seeds the walk.** A pinned root that has exited reads `None`.
  Reaping its orphaned descendants is the point of the delayed kill, so the
  walk is seeded with `exp`. The kill itself no-ops on the dead root.
- **The root is exempt from the stamp.** Its identity never comes from a
  recorded parent edge, which is what the stamp guards: it is pinned by a live
  `Child` handle or matched against `exp`. A stamp check on the root couldn't
  catch reuse, and under a backward clock step it would reject the root and turn
  a legitimate cancel into a no-op that leaves the session waiting on a live
  tree. Descendants still obey the stamp.

Callers:

- `send_terminate` (immediate) and every kill in the cross-user helper pass
  `None`. Each holds the workload's live `Child`, whose handle keeps its PID
  from being reused.
- `spawn_delayed_terminate` reads the root's creation time when it is called,
  while the caller still holds the `Child`, and passes it as `Some(ct)` after
  the delay, so a PID recycled during the grace period is never walked. The
  held handle keeps even an exited process's object, and its creation time,
  readable, so a `None` at that point means the query was denied (for example a
  protected process), not that the process is gone. With nothing to pin, the
  delayed kill doesn't walk the tree at all; it logs a warning and returns.

#### Edge validation

The walk is breadth-first from the root, with a visited set that is its cycle
and duplicate guard: each PID is considered once. A pair `(child, ppid)` is
considered only when `ppid` is a node already accepted. A child whose creation
time can be read is accepted when

    parent.creation_time <= child_ct <= snapshot_time

and recorded with `child_ct`.

- A true child is never older than its parent, so a child older than its
  recorded parent marks a stale edge whose parent PID was reused. Equal times
  are accepted.
- A child newer than the stamp marks a PID that was reused after the snapshot:
  the edge was genuine when captured, the child exited, and an unrelated process
  took its PID before its creation time was read.
- A rejected child is not traversed, so its whole subtree is pruned.

Breadth-first order lists every parent before its children, so killing in
reverse order kills leaves before their ancestors.

#### Unreadable children

A child under an accepted parent whose creation time can't be read is kept as a
traversable node, recorded with its parent's creation time, which is a sound
lower bound for its own children. This keeps a live subtree under a dead
intermediate reachable (`cmd` → `sh` → renderer, where `sh` exits first). The
node itself is never terminated, because the [kill-time
check](#kill-time-revalidation) can't open or match it. Its readable children
are validated against the inherited bound like any other.

#### Kill-time revalidation

Between collecting the tree and terminating it, a collected PID could be reused.
`kill_process_checked` opens each process with
`PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE`, re-reads its creation
time on that handle, and calls `TerminateProcess` on the same handle only when
the time equals the recorded one (`process_identity_matches`). Because the check
and the kill share a handle, the PID can't change between them. If the open
fails or the times differ, the process is skipped.

Requesting both rights in one `OpenProcess` is all-or-nothing: a process whose
DACL grants terminate but not query can no longer be killed, where a
terminate-only open would have succeeded. That is deliberate, because giving up
the same-handle read reopens the gap it closes. Protected (PPL) processes can
also refuse the query and are skipped.

When `collect_tree_validated` returns empty, nothing is killed. A raw-PID kill
of the root would risk a reused PID, and wouldn't reliably widen coverage, for
the reason above.

#### Logging skipped kills

The in-process kill logs each skipped process, and an empty tree, at `info` on
`openjd.sessions`, so misfires in the field can be diagnosed. The cross-user
helper logs nothing for them: on Windows its stderr is merged into the stdout
pipe that carries JSON responses, and the session's reader treats any line that
isn't JSON as a protocol error.

#### Clock adjustments

Creation times and the stamp are wall-clock, not monotonic, so the rules are
exact only while the clock moves forward. An NTP step-back, a manual change or
a VM resume between a parent's and its child's creation can make a true child
read older than its parent. One between a child's creation and the stamp can
make it read newer than the stamp. Either prunes the child's subtree, which is
an under-kill. The root's [stamp exemption](#root-identity) means a backward step
never stops the root itself from being killed.

#### Residual windows

Two narrow cases can still terminate a process that isn't ours. Both are left
for a rework that tracks the workload with a Job Object instead of walking PIDs,
which removes them structurally.

- **Interim holder of a dead PID.** Seeding an exited root and traversing
  unreadable intermediates follows recorded edges of PIDs that have been freed.
  Children of a process that held such a PID in the meantime pass validation if
  their creation times fall in `[exp, snapshot_time]`. The window is bounded and
  no wider than the walk without validation.
- **Unreadable but alive child of a reused parent PID.** If a recorded parent
  PID was freed and reassigned to our root, a process that denies the query and
  carries that stale parent becomes a child of the root. Because it is
  unreadable it skips the age check, and its own children can fall inside the
  window. Telling a dead PID (`OpenProcess` fails with
  `ERROR_INVALID_PARAMETER`) from a denied one (`ERROR_ACCESS_DENIED`) would
  close this, but needs a richer `creation_time` callback.

#### Shared implementation

The rules above live in one function, `collect_tree_validated` in
`src/helper/src/process_tree.rs`, with `process_identity_matches` beside it.
When each crate had its own walk, the PID-reuse fix reached only one of them.

The module uses only `std` and makes no Windows calls: callers supply the
snapshot and a creation-time callback. That lets its unit tests run on every
platform, and lets two crates compile it. The helper builds it as a normal
module. `openjd-sessions` includes it with `#[path]`, as `win32_locate.rs`
includes `win32_which.rs`, because the helper is a separate crate that can't
depend on `openjd-sessions`. Both include it under `cfg(any(windows, test))`.
Each crate keeps its own snapshot, creation-time read and `kill_process_checked`.

## Windows Cross-User Execution

On Windows, cross-user subprocess execution uses `CreateProcessAsUserW` (when a logon
token is available) or `CreateProcessWithLogonW` (username + password). The
`win32.rs` module handles:

- Logon token acquisition via `LogonUserW`
- Environment block construction for the target user
- Stdin/stdout pipe creation with inheritable handles
- Process creation with `CREATE_NEW_PROCESS_GROUP` flag

### CreateEnvironmentBlock profile-unload race

`environment_for_user()` retries `CreateEnvironmentBlock` on transient
profile-unload-race errors. The API reads from the user's registry hive at
`HKEY_USERS\<SID>`; Windows unloads that hive asynchronously when the
profile's refcount drops to zero. Back-to-back logons for the same user can
race with an in-progress unload and surface as:

- `0x800703FA` `ERROR_KEY_DELETED` — "Illegal operation attempted on a
  registry key that has been marked for deletion" (the symptom observed in
  the cross-user integration tests).
- `0x80070005` `E_ACCESSDENIED` — documented by Microsoft as a related
  symptom of the same race.

Retries are bounded at 5 attempts with geometric backoff (20/40/80/160 ms,
worst-case ~300 ms total). Non-transient errors return immediately so real
failures aren't hidden. The upstream Python implementation has a first-hand
comment describing this same class of error in `_win32/_locate_executable.py`
but uses a different mitigation (forcing handle cleanup between calls).
