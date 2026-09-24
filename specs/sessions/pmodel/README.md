# P model of `openjd-sessions`

A [P](https://p-org.github.io/P/) formal model of the `openjd-sessions` runtime
(`crates/openjd-sessions`). Sessions are a small set of concurrent state
machines exchanging events — a Session driving one action at a time, an abstract
Subprocess, and (optionally) a persistent cross-user Helper — so they map
naturally onto P `machine`s, `event`s, and `spec` monitors.

The model is an **abstraction**, not a port: it captures the control-flow and
ordering contracts documented in [`specs/sessions/`](../) and asserts the
invariants against every interleaving the P checker explores. It deliberately
drops bytes, real paths, crypto, and wall-clock time (see
[Out of scope](#out-of-scope-for-the-model)).

Cross-references to the Rust source (`session.rs:NNNN`) mark decisions that were
verified against the implementation rather than the prose spec — a few of these
diverged from the informal specs and the model follows the code.

## Layout

```
pmodel/
├── OpenJDSessions.pproj        # P project file (PSrc + PSpec + PTst)
├── Dockerfile                  # P toolchain image (from p-org/P PR #983)
├── PSrc/                       # the model
│   ├── Events.p                # events + shared types (SessionState, ActionState, …)
│   ├── Session.p               # the SessionState machine (the core)
│   ├── Subprocess.p            # abstract async subprocess (same-user path)
│   └── CrossUserHelper.p       # persistent cross-user helper + token protocol
├── PSpec/                      # invariant monitors
│   ├── MonitorEvents.p         # events the Session announces to the monitors
│   ├── SessionStateSpec.p      # legal transitions, ≤1 action, brittle-session
│   ├── EnvStackSpec.p          # LIFO exits, no duplicate env, clean teardown
│   ├── ActionLivenessSpec.p    # every started action eventually terminates
│   └── CancelDeliverySpec.p    # an issued cancel is never silently dropped
└── PTst/                       # drivers + test cases
    ├── Drivers.p               # WorkerAgent / Cancel / ExternalCancel / HelperToken
    └── TestScripts.p           # test declarations (tc*)
```

## Status

**Compiles and model-checks clean** with P CLI 3.1.0. All six test cases report
0 bugs (2000+ schedules each) at the default settings. Two independent
fault-injection knobs confirm the monitors are live, not vacuous:

- Removing the exit-time stack pop makes `EnvStackSpec` fail.
- `CROSS_USER_HONORS_TOKEN_CANCEL()` in `PSrc/Session.p` defaults to `true` (the
  cross-user path observes a cancel over either delivery channel). Flipping it to
  `false` makes the cross-user path ignore a token-only cancel;
  `tcExternalCancelCrossUser` then fails `CancelDeliverySpec` while
  `tcExternalCancelSameUser` and `tcWorkerAgentCrossUser` stay green — showing
  `CancelDeliverySpec` has teeth and is specific to the cross-user token-cancel
  path. See [Two-channel cancel delivery](#two-channel-cancel-delivery).

## Building & running the checker

There is no P toolchain checked into this repo. Use the toolchain image built
from [p-org/P PR #983](https://github.com/p-org/P/pull/983) (bundles the .NET
SDK, JDK 17, Maven, graphviz, and the `p` CLI).

**Build the toolchain image** — the `Dockerfile` here is a verbatim copy of the
one added in that PR; it builds `p` from a **P checkout**, so build it with a
clone of `p-org/P` as the context, not from this directory. Any container
runtime works (Docker Desktop, colima, Rancher, …):

```bash
git clone https://github.com/p-org/P.git /tmp/P
# use the PR's Dockerfile (or check out the PR branch, which already contains it)
docker build -t p -f specs/sessions/pmodel/Dockerfile /tmp/P
```

**Compile and model-check** the model by mounting this directory as the
workspace:

```bash
cd specs/sessions/pmodel
docker run --rm -it -v "$PWD":/workspace p \
    bash -lc 'p compile && p check -tc tcWorkerAgentSameUser'
```

Useful invocations (inside the container, or with a native `p` on `PATH`):

| Command | What it does |
|---------|--------------|
| `p compile` | Compile `OpenJDSessions.pproj` |
| `p check` | Run all test cases |
| `p check -tc tcWorkerAgentSameUser` | Same-user lifecycle |
| `p check -tc tcWorkerAgentCrossUser` | Cross-user lifecycle (routes through the helper) |
| `p check -tc tcCancelBrittle` | cancel_action → brittle-session contract |
| `p check -tc tcExternalCancelCrossUser` | External token cancel on a cross-user session (turns red under the fault-injection knob) |
| `p check -tc tcHelperTokenSecurity` | Bad-token cancel must not stop a run |
| `p check -tc tcWorkerAgentSameUser -i 10000` | 10k schedules (more coverage) |

## Test cases

| Test case | Drivers | Specs asserted |
|-----------|---------|----------------|
| `tcWorkerAgentSameUser` | `WorkerAgentDriver(crossUser=false)` | State, EnvStack, Liveness, CancelDelivery |
| `tcWorkerAgentCrossUser` | `WorkerAgentDriver(crossUser=true)` | State, EnvStack, Liveness, CancelDelivery |
| `tcCancelBrittle` | `CancelDriver` (via `cancel_action`) | State, EnvStack, Liveness, CancelDelivery |
| `tcExternalCancelSameUser` | `ExternalCancelDriver(crossUser=false)` | State, EnvStack, Liveness, CancelDelivery |
| `tcExternalCancelCrossUser` | `ExternalCancelDriver(crossUser=true)` | State, EnvStack, Liveness, CancelDelivery |
| `tcHelperTokenSecurity` | `HelperTokenDriver` | Liveness (+ inline security asserts) |

---

## What the model covers

### Machines (P `machine`s)

- **Session** — the driver. Holds `SessionState` {Ready, Running, Canceling,
  ReadyEnding, Ended}, a LIFO env stack, the cumulative env-var change set, and
  the `ending_only` brittle flag.
- **Subprocess** — one per action on the same-user path. Emits a
  nondeterministic sequence of `openjd_*` directives, then reaches a terminal
  `ActionState` via normal exit, timeout→SIGKILL, or cancel→grace→SIGKILL.
- **CrossUserHelper** — persistent process on the cross-user path; sequential
  command loop; token check; abstracts the poll-multiplex of cancel-stdin vs
  child-stdout.
- **Drivers** (`PTst`) — stand in for the worker agent / CLI: issue
  enter/exit/run/cancel/cleanup and the external `cancel_token`.

Environments are modeled as **data** (stack entries carrying their env-var
contribution), not machines; their `onEnter`/`onExit` scripts reuse the action
path.

### Events (P `event`s)

- **Lifecycle commands** (Driver→Session): `eEnterEnv`, `eExitEnv(keepRunning)`,
  `eRunTask`, `eRunSubprocess`, `eCancelAction(markFailed)`,
  `eSessionCancelToken`, `eCleanup`.
- **Action messages** (Subprocess/Filter→Session, the `ActionMessage` enum):
  `eProgress`, `eStatus`, `eFail`, `eSetEnv`, `eUnsetEnv`, `eRedactedEnv`,
  `eCancelMarkFailed`.
- **Subprocess lifecycle / signals**: `eProcessExited(state, code)`,
  `eCancelRequest(method)`, `eGraceExpired`, `eExitGrace`.
- **Helper wire protocol** (Session↔Helper): `eHelperRun{token}`,
  `eHelperCancel{method, token}`, `eHelperShutdown{token}`; responses
  `eHelperPid`, `eHelperOut`, `eHelperExited`, `eHelperError`,
  `eHelperInvalidToken`.

### Invariants asserted (P `spec` monitors)

**State machine** (`SessionStateSpec`):
- Only the documented `SessionState` transitions occur.
- Exactly one action `Running` at a time.
- After any action Failed/Canceled/Timeout the session is brittle: it never
  returns to plain `Ready` — only `ReadyEnding` or `Ended`.
- `Ended` is terminal.

**Environment stack** (`EnvStackSpec`):
- Exits are strictly LIFO; `exit(id)` matches the top of stack.
- No duplicate env identifier is ever on the stack.
- `cleanup()` with a non-empty stack is flagged (onExit scripts skipped).
- The env-var change set is the exact fold of per-env changes in entry order
  (last-writer-wins within an env); popping an env removes exactly its
  contribution. Only `onEnter`-time changes are attributed; task / onExit /
  ad-hoc `openjd_env` changes are discarded.

**Cancellation** (in `Session` + drivers):
- `cancel_action` valid only from `Running` → `Canceling`.
- Every action installs **fresh** cancel state, so a cancel never poisons a
  later action — *except* `SessionConfig.cancel_token`, whose cancel is
  permanent and cascades to all current and future actions.
- Cancel classification is a session-level overlay applied in `finalizeAction`
  from whether the worker's detection channel observed the cancel (sticky
  same-user token / cross-user watch channel), not a raceable in-band message.
  `Timeout` still wins; the only rewrite on top is Canceled→Failed under
  `mark_action_failed`.
- A cancel with a wrong/missing token never terminates the running child
  (`HelperTokenDriver`).

**Cancel delivery** (`CancelDeliverySpec`):
- An issued cancel (any channel) is never silently dropped: a canceled action
  must not complete `Success`. This is the invariant the cross-user
  token-cancel bug violates — see below.

**Liveness** (`ActionLivenessSpec`):
- Every started action eventually terminates (exit, timeout, or
  cancel→grace→kill).

**Helper protocol** (`CrossUserHelper`):
- Strictly sequential: after a run command, only that action's output then one
  exit/error before the next command.
- Token verified on every command; bad token ⇒ `eHelperInvalidToken` and the
  helper stays alive (no crash / no DoS) and the current run is untouched.

### Two-channel cancel delivery

Cancellation is delivered over **two channels**: the watch channel
(`cancel_request_rx`, mirrored to the cross-user helper over `cancel_writer`) and
the per-action `CancellationToken`. `cancel_action` fires **both**; a bare
external `SessionConfig.cancel_token.cancel()` — the parent token a caller holds
to cancel a whole session from outside its async context — fires **only the
token**. Both paths must ultimately produce a `Canceled` outcome, whichever
channel carried the cancel.

`CancelDeliverySpec` asserts exactly this: any issued cancel eventually takes
effect (a canceled action never completes `Success`). The
`CROSS_USER_HONORS_TOKEN_CANCEL()` knob in `PSrc/Session.p` lets the cross-user
path be made deliberately blind to the token-only channel — a fault injection
that flips `tcExternalCancelCrossUser` red while same-user cases stay green,
demonstrating the spec is specific to this channel/path combination. It defaults
to honoring both channels.

Rationale for individual modeling choices that might surprise a reader (e.g.
cancel classification is a session-level overlay rather than a raceable message;
only `onEnter`-time `openjd_env` changes are attributed) is documented inline at
the relevant point in the `.p` sources.

### P language notes (why some names look odd)

A few names deviate from the Rust source to satisfy the P grammar; they carry
the same meaning:

- `state` is a P keyword, so the Session's state variable is `sstate` and the
  `SessionState` payload fields are `fromState`/`toState` (not `from`/`to`,
  which are also reserved).
- P has **no single-field named tuples**, so single-value event payloads are
  bare types (`event eRunTask : machine;`, `event eActionDone : tActionState;`)
  rather than `(client: machine)` etc.
- `ActionState` payload fields are named `st` (again, `state` is reserved).
- Streaming machines yield via a self-sent `eTick` between emitted lines; a
  plain `goto`-loop runs to completion in P and would prevent an incoming cancel
  from ever interleaving mid-stream.

## Out of scope for the model

- **OS/filesystem semantics** — TempDir, sticky-bit checks, `remove_dir_all`,
  `sudo rm -rf`, DACL/permission bits, chown/chmod. File ownership appears only
  as an abstract token for the helper security invariant.
- **Byte-level I/O** — UTF-8 lossy decoding, 64KB line truncation, BufReader
  buffer-before-poll. A "line" is an opaque directive choice.
- **Cryptography** — token entropy/CSPRNG/constant-time compare. The token is an
  abstract equality-only value; only its behavioral consequences are asserted.
- **Format-string / expression evaluation** — symbol table, path mapping, let
  bindings, EXPR extension (these live in `openjd-expr`/`openjd-model`).
- **Redaction / logging** — `********` substitution, `echo_openjd_directives`,
  log content (purely observational).
- **Platform mechanics** — Win32 console/`CTRL_BREAK`, Job Objects,
  `CreateProcessAsUser`, `setsid`/`killpg`/`dup2`, `CreateEnvironmentBlock`
  retry race. Abstracted to notify-signal / terminate-signal / kill-tree.
- **Real time** — timeouts and grace periods are nondeterministic *events*, not
  wall-clock; the 5s/30s/120s/300s constants are policy, not correctness.
- **Callback performance / blocking**, `debug_collect_stdout` accumulation, and
  the `Drop` best-effort safety net (the model asserts only that state reaches
  `Ended`).
- **PyO3 bindings** — `SessionCancelHandle`, `clone_cancel_writer`,
  `override_action_state`. The cancel-from-another-thread concurrency is modeled
  (via `eSessionCancelToken` / helper `cancel_writer`); the FFI is not.
