# Action Status

## Purpose

`ActionStatus` is a snapshot of the current or most recently completed action's
state. It is the primary data structure passed to the user callback after every
`openjd_*` directive and at action start/completion, enabling real-time monitoring
of action progress by the worker agent.

## Struct

```rust
#[derive(Debug, Clone)]
pub struct ActionStatus {
    pub state: ActionState,
    pub progress: Option<f64>,
    pub status_message: Option<String>,
    pub fail_message: Option<String>,
    pub exit_code: Option<i32>,
    pub started_at: Option<SystemTime>,
    pub ended_at: Option<SystemTime>,
}
```

## Fields

| Field | Type | Set by | Description |
|---|---|---|---|
| `state` | `ActionState` | Session | Current lifecycle state of the action |
| `progress` | `Option<f64>` | `openjd_progress` | 0.0–100.0 progress percentage |
| `status_message` | `Option<String>` | `openjd_status` | Human-readable status text |
| `fail_message` | `Option<String>` | `openjd_fail` / `CancelMarkFailed`, or the session on a failure before the action starts | Reason for failure |
| `exit_code` | `Option<i32>` | Subprocess exit | Process exit code, `None` while running |
| `started_at` | `Option<SystemTime>` | Session | When the action began |
| `ended_at` | `Option<SystemTime>` | Session | When the action completed |

## ActionState

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionState {
    Running,
    Success,
    Failed,
    Canceled,
    Timeout,
}
```

Terminal states are `Success`, `Failed`, `Canceled`, and `Timeout`. Only `Running`
is non-terminal.

## Lifecycle

1. **Action starts**: Session sets `state = Running`, `started_at = now()`,
   all other fields `None`. Callback fires. An `enter_environment`/`exit_environment`
   call that fails before its action is reported `Running` (building the symbol table,
   or — for enter — resolving an environment's `variables`) skips this step: the session
   sets `state = Failed` with the error's message as `fail_message` itself and fires one
   callback. `run_task` always reports `Running` first. An enter/exit action with no
   `onEnter`/`onExit` records `Success` directly, also without a `Running` callback (see
   [session.md § Failures before an action starts](session.md#failures-before-an-action-starts)).

2. **Directives arrive**: `apply_message` updates the relevant field and fires
   the callback after each message:
   - `Progress(v)` → `progress = Some(v)`
   - `Status(s)` → `status_message = Some(s)`
   - `Fail(s)` → `fail_message = Some(s)`
   - `CancelMarkFailed { fail_message }` → `fail_message = Some(fail_message)`,
     triggers cancellation

3. **Action ends**: Session sets `state` to the terminal state, `exit_code` from
   the subprocess, `ended_at = now()`. Callback fires.

## Reset between actions

`progress`, `status_message`, and `fail_message` are reset to `None` at the start
of each new action. This prevents stale values from a previous action leaking into
the next one's callback invocations.

## Callback signature

```rust
pub type SessionCallbackType = Box<dyn Fn(&str, &ActionStatus) + Send + Sync>;
```

The callback receives the session ID and a reference to the current `ActionStatus`.
It is invoked synchronously within `apply_message` and at action start/end. The
callback must not block — it should enqueue work rather than perform I/O directly.

## Why Option fields instead of defaults

Using `Option<f64>` for progress (rather than defaulting to 0.0) distinguishes
"no progress reported" from "0% progress". The worker agent uses this distinction
to decide whether to display a progress bar. Similarly, `Option<String>` for
messages distinguishes "no message" from "empty message".
