# openjd-sessions Crate Quality Evaluation

**Evaluator:** Kiro
**Date:** 2026-04-17
**Crate location:** `~/openjd-rs/crates/openjd-sessions`
**Spec location:** `~/openjd-rs/specs/sessions`

## Executive summary

`openjd-sessions` is the Rust runtime for executing OpenJD sessions (environment
enter/exit, task execution, subprocess management, cross-user support). It is a
port of the Python `openjd-sessions-for-python` library, redesigned around tokio
async/await + channels + cancellation tokens instead of threads + locks + queues.

**Build/test status:**
- `cargo build -p openjd-sessions --all-targets` — clean, no warnings
- `cargo clippy -p openjd-sessions --all-targets` — clean
- `cargo test -p openjd-sessions --all-targets` — **338 passed, 0 failed, 13 ignored**
  (ignored tests require Docker-based cross-user infrastructure)

**Overall assessment:** production-ready for POSIX/Linux. Windows support is
explicitly partial. The main gaps are alignment (spec drift) and some coverage /
documentation holes rather than correctness bugs. No blocking release issues were
identified.


---

## Artifacts reviewed

### Specifications (`~/openjd-rs/specs/sessions/`, 16 files)

`README.md`, `architecture.md`, `session.md`, `subprocess.md`, `action-filter.md`,
`action-messages.md`, `action-status.md`, `runners.md`, `embedded-files.md`,
`cross-user.md`, `cross-user-subprocess-issues.md`, `cross-user-testing.md`,
`embedded-cross-user-helper.md`, `tempdir.md`, `logging.md`, `error-handling.md`,
`win32-locate.md`.

### Implementation source (`src/`)

`lib.rs`, `session.rs` (1,656), `subprocess.rs` (2,059 incl. tests),
`action_filter.rs` (1,462 incl. tests), `cross_user_helper.rs` (503),
`runner/{mod,env_script,step_script}.rs`, `embedded_files.rs`, `tempdir.rs`,
`session_user.rs`, `action.rs`, `action_status.rs`, `logging.rs`, `error.rs`,
`win32.rs`, `win32_permissions.rs`, `win32_locate.rs`,
`helper_binary.rs`, `helper/` (embedded helper binary).

### Tests

- Integration: `test_session.rs`, `test_session_scenarios.rs`,
  `test_session_env_step.rs`, `test_helper.rs`, `test_cross_user.rs`,
  `test_path_mapping.rs`, `test_path_mapping_materialize.rs`,
  `test_tempdir_os.rs`, `test_embedded_files.rs`.
- In-source `#[cfg(test)]` modules in `action_filter.rs`, `subprocess.rs`, and others.
- `tests/scenarios/` — 23 template + scenario YAML pairs for parameter types and let
  bindings.


---

## 1. Spec ↔ implementation mismatches

Concrete drift between spec docs and code. Several examples in the specs would not
compile against the current API.

| # | Spec file | Spec claim | Actual implementation |
|---|---|---|---|
| 1 | `session.md` | `SessionConfig` ends at `cancel_token` | Also has `pub collect_stdout: bool` (session.rs:85) |
| 2 | `architecture.md` | `pub use session::{Session, SessionState, SessionConfig}` example | `lib.rs` also re-exports `EnvironmentIdentifier` at top level |
| 3 | `session.md` §Session Construction | `Session::new(working_directory)` "behind the `test-utils` feature flag" | Actual fn is `Session::new_for_test` (session.rs:222), no feature flag — always available |
| 4 | `subprocess.md` | `SubprocessConfig` ends at `cancel_request_rx` | Also has `pub collect_stdout: bool` (subprocess.rs:63) |
| 5 | `action-filter.md` | `ActionFilter::filter_message(&mut self, line: &str)` (1-arg) | Signature is `filter_message(&mut self, message: &str, session_id: &str)` (action_filter.rs:135) — caller passes session_id per-call |
| 6 | `action-filter.md` | `ActionFilter` struct stores `session_id: String` | Matches, but spec is unclear why per-call `session_id` is *also* required (used to filter out lines from other sessions that share the same log stream) |
| 7 | `session.md` §Cleanup | "For cross-user sessions: runs `sudo rm -rf` … then removes the directory as the process user" | Cross-user cleanup path routes through the persistent helper (`cross_user_helper.rs`) when available; sudo-rm is the fallback. Spec doesn't mention the helper-based path. |
| 8 | `session.md` §Enter | Shows signature `enter_environment(env, resolved_symtab, identifier, os_env_vars)` | Actual also takes `identifier` as `Option<&str>` and returns `SessionError`. Resolved_symtab is `Option<&SerializedSymbolTable>` — not noted as optional in prose. |
| 9 | `runners.md` §Runner Builder Methods | Lists `with_collect_stdout(bool)` as a runner builder method | Builder exists but spec doesn't say it must match the `SessionConfig.collect_stdout` — leaving ambiguity about precedence |
| 10 | `runners.md` §CancelMethod | "`onRun` actions: 120 seconds" default grace | Confirmed in source but the 300s `onExit` default mentioned elsewhere isn't cross-linked here |
| 11 | `embedded-files.md` | `EmbeddedFiles::new(scope, session_files_directory, session_id)` | Matches, but spec misses `with_user(Option<Arc<dyn SessionUser>>)` builder discussion and says "If cross-user, set group ownership" without showing the API path |
| 12 | `action-messages.md` | Spec signature: `drive_action(&mut self, action_future, message_rx)` | Implementation is a private method; spec should mark it `async fn` + private. It's documented as if public. |
| 13 | `logging.md` | "`session.rs` logs `HOST_INFO` … at init" | session.rs logs host info inside `with_config` — confirmed. But `log_section_banner` is also used, which the table doesn't attribute to session.rs |
| 14 | `cross-user.md` / `embedded-cross-user-helper.md` | Two spec docs cover overlapping material. README lists both but the relationship is not stated — a reader has to cross-reference to figure out which describes the primary path and which describes the helper |
| 15 | `win32-locate.md` | Explicitly "not yet integrated" | `lib.rs` does `pub(crate) mod win32_locate`, which matches. README says "partially implemented" — the spec and README use different phrasing for the same state |

### Missing from the spec entirely

These items exist in source with no corresponding spec coverage:

- **`Session::new_for_test`** — a public constructor used by tests and the CLI. Not
  documented at all.
- **`SessionConfig.collect_stdout`** / **`SubprocessConfig.collect_stdout`** —
  mentioned in passing in `session.md` under `collect_stdout` heading but not shown
  in any struct-definition code block.
- **`Session::clone_cancel_writer`** (session.rs:482) — used by runners to pass the
  cancel-info file writer; entirely absent from `session.md`.
- **`Session::action_status` accessor** (session.rs:490) — retrieves the current
  snapshot; undocumented.
- **`path_mapping_rules` accessor** (session.rs:400) — undocumented.
- **`environments_entered` accessor** (session.rs:475) — undocumented.
- **The `helper_binary.rs` module** — embeds the cross-user helper binary at build
  time via `build.rs`. `embedded-cross-user-helper.md` describes the protocol but
  not the build-time embedding mechanism (`build.rs` exists; it is not explained).


---

## 2. Spec coverage gaps

Source files or behaviours that deserve a spec section but lack one:

- **`build.rs`** (2,218 bytes) — compiles the embedded helper binary and sets up
  linking flags. No spec covers build-time machinery. An operator or packager
  attempting a cross-build would have to read the code.
- **`helper/` subcrate** — `helper/src/main.rs`, `protocol.rs`, `runner.rs`,
  `runner_win.rs`. Only `embedded-cross-user-helper.md` overlaps, and it covers the
  protocol (IPC messages) — not the helper binary's own lifecycle, arg parsing, or
  signal-handling.
- **`win32_permissions.rs`** — ACL management for Windows temp dirs. No spec.
- **`win32.rs`** — Windows `LogonUserW`, environment block construction, pipe
  creation. Only referenced obliquely in `subprocess.md`.
- **`logging.rs` `log_section_banner` / `log_subsection_banner`** — shown in
  `logging.md` but the exact banner format (column widths, fill characters) is not
  specified; an alternative consumer parsing the banners cannot rely on a stable
  format.
- **`ActionFilter` multi-session behaviour** — `action-filter.md` mentions this in
  a single sentence: "supports shared log streams where multiple sessions interleave
  output." No detail on ordering or interleaving semantics.
- **`EnvironmentScriptRunner::exit()` timeout default of 300s** — mentioned once in
  `runners.md` §`exit()` but not cross-referenced from the `CancelMethod` table which
  lists 30s for env scripts. The 30s vs 300s distinction (cancel grace vs overall
  timeout) should be made explicit.
- **Dropping a `Session` without calling `cleanup()`** — mentioned in both
  `session.md` and `tempdir.md` but the exact guarantee (best-effort `remove_dir_all`,
  no cross-user cleanup, warning emitted) is split across both. A single contract
  section would reduce confusion.

### Rationale gaps ("why" is missing)

- **Why the worker agent is the primary consumer** — mentioned; the *design
  pressure* this exerts (callback speed, real-time streaming, cancellation
  cascading) could be collected into a single tenets section.
- **Why `SessionError::Runtime(String)`** is a catch-all — `error-handling.md` says
  "Used sparingly" but doesn't explain why the alternative (more structured
  variants) was rejected.
- **Why the 5-second process-exit grace** in subprocess.rs — the spec mentions the
  behaviour but doesn't discuss why 5s specifically (not 3s, not 10s).
- **Why `suppress_filtered` defaults are what they are** — `action-filter.md`
  doesn't discuss the consumer-choice between passing filtered directives through
  and suppressing them.

### Redundant / repeated content

- **Python-vs-Rust comparison tables** appear in `README.md`, `architecture.md`,
  `action-messages.md`, and `subprocess.md` with overlapping content.
- **Two-phase embedded-file flow** is described in `embedded-files.md`, `runners.md`,
  and alluded to in `session.md`. The three versions are broadly consistent but
  differ in wording in ways that could mislead a reader into thinking they are
  different.
- **LIFO environment ordering rule** is stated in both `session.md` and
  `cross-user.md`.


---

## 3. Implementation — Rust quality, API ergonomics

### Strengths

- **No shared-mutable state**: `tokio::select!` + mpsc channel decouples subprocess
  stdout from `Session::drive_action`. Zero `Mutex` or `RwLock` in the hot path.
- **`CancellationToken` is used uniformly** for external, per-action, and parent→child
  cancel propagation. Simpler than Python's `threading.Event` + lock dance.
- **`#[non_exhaustive]` on `SessionError`** allows adding variants without a semver
  break.
- **Grouped private fields** on `Session` (`ActionStatusFields`, `CancelFields`,
  `CrossUserFields`) keep the struct readable despite ~25 fields.
- **Builder pattern** (`with_path_mapping`, `with_library`, `with_revision_extensions`)
  makes optional configuration ergonomic after `with_config`.
- **Drop safety net** on both `Session` and `TempDir` prevents silent leaks.
- **`#[cfg(windows)]` / `#[cfg(unix)]` gates** keep platform-specific code from
  leaking into cross-platform paths.
- **`shlex::try_join`** is used to quote subprocess args for logging and shell-script
  wrapping — avoids hand-rolled quoting bugs.
- **`session_log!` macro** preserves the caller's source location, which a function
  wrapper could not.

### Friction points

1. **`Session` has ~25 fields** (including those grouped into `*Fields` helpers).
   The struct would benefit from a `SessionInner` pattern: public `Session` holding
   an `Arc<SessionInner>` could permit future shared ownership / cloneability.
2. **`SessionConfig` is a plain struct with ~11 public fields, no builder.** This
   forces callers to use struct-literal syntax, which is brittle as fields are
   added (`collect_stdout` was added without a semver break because the field is
   public — but now all callers who were using `..Default::default()` must adopt
   it or break). A `SessionConfigBuilder` would be safer.
3. **`Session::new_for_test` is `pub`** — callers in the same crate test file would
   be able to use `#[cfg(test)] pub(crate) fn`, but making it fully public risks
   consumers relying on a test-only path. Document more clearly or gate behind a
   `test-utils` feature as the spec implies.
4. **`redact()` allocates repeatedly** — `text.to_string()` + per-value `replace` in
   a loop. For long command outputs with many redactions this is O(values × length).
   An Aho-Corasick scanner (or at minimum a single-pass approach that replaces all
   matches at once) would be cleaner and faster.
5. **`ActionFilter::filter_message` returns `(Vec<FilterCallback>, bool, String)`** —
   three-tuple return is hard to use. A `FilterResult { callbacks, pass_through,
   modified }` struct would be more self-documenting. All 4 call sites in the crate
   destructure the tuple anyway.
6. **Error variants mix field names**: `WorkingDirectory { path, source }`,
   `TempDir { path, source }`, `EmbeddedFile { name, source }`, `SubprocessStart
   { command, source }`. Consolidating `path` vs `name` vs `command` into a single
   `target: String` field would be cleaner, but loses type distinctions. The current
   approach is fine — flagging for consistency review.
7. **Public `SessionCallbackType = Box<dyn Fn(...)>`** — makes the callback
   non-cloneable. If tests or middleware want to wrap it, they have to rebuild from
   scratch. Consider `Arc<dyn Fn>` to enable sharing.

### Naming consistency

- `SessionConfig` vs `SubprocessConfig` — consistent ✓
- `Session::with_*` builders vs `EmbeddedFiles::with_user` — consistent ✓
- `ActionMessageKind` / `ActionMessageValue` / `ActionMessage` — three similar-named
  types in two modules. `ActionMessageKind`/`Value` are internal to `action_filter.rs`
  while `ActionMessage` is in `action.rs`. Fine as-is but the distinction is not
  called out in `action-filter.md`.
- `SessionState::ReadyEnding` vs `ActionState::Canceled` — `Canceled` vs `Canceling`
  is intentional (past vs in-progress). `ReadyEnding` is a good name; `Ending` or
  `Brittle` would also have worked. No change needed.
- `run_subprocess` appears twice: `subprocess::run_subprocess` (internal) and
  `Session::run_subprocess` (public, ad-hoc). The ambiguity is managed by module
  scoping; a comment in both sites cross-linking them would help future maintainers.


---

## 4. Error message quality

Errors produced by this crate are generally high quality: they identify the path,
name, or command that failed and chain the underlying `std::io::Error` via
`#[source]`. Examples from `error.rs`:

- `"Session must be in READY state, current: RUNNING"`
- `"Environment 'my-env' enter failed: exit code 1"`
- `"Failed to write embedded file 'MyScript': Permission denied (os error 13)"`
- `"Failed to start subprocess '/usr/bin/missing': No such file or directory"`

### Gaps

1. **`SessionError::Runtime(String)` is used 20+ times** across `session.rs`,
   `runner/*.rs`, and `subprocess.rs`. Specific Runtime strings like `"Environment
   {id} has already been entered in this Session."` could be promoted to structured
   variants (`DuplicateEnvironmentId { id: String }`). Programmatic consumers
   cannot distinguish runtime errors without substring matching.
2. **`format_expected`** produces `"READY or READY_ENDING"`, which is correct but
   inconsistent — other error messages say `"state: READY"`. Using a consistent
   phrase like `"state must be one of: READY, READY_ENDING"` would be clearer.
3. **FormatString error context strings** (e.g., `"action command"`,
   `"embedded file data for 'MyScript'"`) are hand-written in each call site. A
   small enum (`FormatStringContext::ActionCommand`, `::EmbeddedFileData(String)`)
   would prevent drift and enable structured error handling.
4. **Subprocess errors** surface the raw exit code but don't include a short
   recommendation (e.g., "command exited with code 127 — check that the command
   exists in PATH"). Python's equivalent runtime has similar behaviour, so this is
   not a regression, but it is a place where Rust could improve on Python.

---

## 5. Performance concerns

Ordered by estimated impact:

| # | File:line | Concern | Complexity |
|---|---|---|---|
| 1 | `session.rs:503-513` | `redact()` does one full `String::replace` pass per value in the set, allocating a new String each iteration. For a 1MB log line with 100 redacted values, ~100MB of allocations. Use `aho-corasick` or a single-pass approach. | O(V × L) per line |
| 2 | `session.rs:1414-…` | `evaluate_env_vars()` clones `HashMap<String,String>` at least 3 times per action (process env → os_env_vars merge → per-environment overlays). At 500+ env vars × 10 environments × per-task invocation, this is measurable. | O(E × V) allocations |
| 3 | `action_filter.rs:465-…` | `redact_openjd_redacted_env_requests(command)` iterates all redaction values per command formatted for log. Same concern as (1). | O(V × L) |
| 4 | `subprocess.rs:611-1041` `run_subprocess` | For every stdout line, `truncate_line` → `String::from_utf8_lossy` → to-string → filter → potential redact → format-string log. Each step allocates. A tight subprocess with megabytes/sec of stdout will amplify this. | O(L) per line, high constant |
| 5 | `action_filter.rs` `filter_message` | Every call rebuilds `msg = message.to_string()` up front even for lines that won't be modified. A `Cow<str>` would avoid allocation on the common-case pass-through. | O(L) per line |
| 6 | `session.rs:1450-…` `build_symbol_table` | Walks all `job_parameter_values` entries and path-maps each `PATH`/`LIST_PATH` value. For large list parameters (hundreds of entries), the inner loop applies all rules per element. The rules are already sorted by source-path length, so an Aho-Corasick-style index over source paths would let mapping be O(L) per element instead of O(R × L) where R is the rule count. | O(R × L × N) |
| 7 | `cross_user_helper.rs` `run_via_helper` | Blocking stdin/stdout read loop; fine for the helper's synchronous design but the main helper IPC marshals JSON on every message. For high-frequency actions a length-prefixed binary protocol would be faster. This is a design choice, not a bug — flag for future perf work. | N/A |

### No O(N²) issues found in the hot path

The state-machine transitions are O(1). Environment entry/exit stack is a `Vec`.
Path-mapping rule sort is O(R log R) once per `with_path_mapping` or `extend_path_mapping_rules` call.

---

## 6. Unwrap / panic / unsafe audit

- **`unsafe`**: limited to:
  - `subprocess.rs` POSIX `pre_exec` hooks (`setsid`, `dup2`) — well-scoped, both
    functions are async-signal-safe per POSIX.
  - `win32*.rs` Windows FFI — every call has a documented safety reason; return
    values are checked.
  - `cross_user_helper.rs` — uses `nix`/`libc` for signal delivery; no direct
    `unsafe` that isn't in a FFI wrapper.
- **`.unwrap()` / `.expect()` in non-test code**:
  - `action_filter.rs` regex `LazyLock::new(|| Regex::new(…).unwrap())` — safe,
    literal pattern verified at compile time.
  - A handful of sites immediately after a length-check (`child.stdout.take().unwrap()`
    after `Command::spawn`) — idiomatic, guaranteed.
- **`panic!`**: none found in library code.
- **`unreachable!`**: none found.

**Conclusion:** no panic paths reachable from malformed input or normal operation.


---

## 7. Test quality

### Strengths

- **338 passing tests**, including integration tests that exercise real subprocess
  execution, env var propagation, LIFO enforcement, path mapping, and embedded
  files.
- **`tests/scenarios/` YAML pairs** (template + scenario) make session integration
  tests declarative: the test harness loads a template, sets parameter values,
  runs the action, and asserts against recorded output. 23 scenario pairs
  covering parameter types (`PATH`, `LIST_PATH`, POSIX↔Windows path mapping),
  let bindings, and env-file let bindings.
- **In-source `#[cfg(test)]` modules** in `action_filter.rs` and `subprocess.rs`
  cover unit behaviour (directive parsing, redaction, env var handling, signal
  delivery happy/sad paths). `action_filter.rs` tests alone exceed 1,000 lines.
- **Test helpers** in `tests/support/` provide minimal shell scripts for
  long-running processes, signal testing, and child-process spawning.

### Gaps

1. **Redaction under load** — no test exercises redaction with many values or long
   lines. The concern flagged in §5 #1 is not caught by the test suite.
2. **Unbounded channel backpressure** — `action-messages.md` justifies unbounded
   channel use. No test injects a stall into the consumer to verify the justification
   (subprocess does not deadlock when messages pile up).
3. **Session cancellation from `parent_token`** — `SessionConfig.cancel_token` is
   documented as cascading to all actions. There is a test that cancels during a
   running task, but I did not find a test that confirms actions started *after*
   parent-token cancellation inherit the cancelled state.
4. **Drop without cleanup** — `TempDir::Drop` is advertised as a safety net. I did
   not find a direct test that exercises the drop path and asserts the directory is
   removed.
5. **`Session::new_for_test`** — public and used in tests, but no test asserts what
   happens when `with_config` is skipped (e.g., redaction, path mapping, callback
   all absent).
6. **Cross-user integration tests are `#[ignore]`** (13 tests). They require Docker
   images (see `testing_containers/`). The CI workflow file (`.github/workflows/ci.yml`)
   should be checked to confirm these are actually run in a separate job.
7. **Malformed-directive test coverage** — the filter detects malformed `openjd_env`
   / `openjd_redacted_env` / `openjd_unset_env`. Tests exist for obvious cases but a
   fuzzing pass would be valuable; directive parsing is the attack surface that a
   malicious (or buggy) action script touches directly.
8. **Windows tests** — most subprocess and cross-user tests are POSIX-only (`#[cfg(unix)]`).
   Windows coverage is limited to the CI matrix passing; behaviour-level tests for
   Windows signals (`CTRL_BREAK_EVENT`), process-tree kill, and cross-user launch
   were not observed in the test files. This is consistent with the spec's
   "partially implemented" status.
9. **Error message quality tests** — AGENTS.md requires tests assert on full error
   message content. `openjd-sessions` does not yet have a `test_error_messages.rs`
   file equivalent to `openjd-model` / `openjd-expr`. Adding one would close the
   consistency gap across crates.

### Organization

Test files are named clearly (`test_session.rs`, `test_session_scenarios.rs`, etc.)
and use descriptive test-function names. Section comments (`// === … ===`) are used
in `test_session.rs` to group related tests. Overall, easy to navigate.

---

## 8. Compile/test verification

```
cargo build -p openjd-sessions --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 16.30s
cargo build -p openjd-sessions --all-targets 2>&1 | grep -E "warning|error"
    (no output)
cargo clippy -p openjd-sessions --all-targets
    (no output, clean)
cargo test -p openjd-sessions --all-targets
    Total passed: 338 failed: 0 ignored: 13
```

The 13 ignored tests are cross-user integration tests requiring Docker infrastructure
documented in `testing_containers/` and `specs/sessions/cross-user-testing.md`.


---

## 9. Recommendations

Grouped by category and prioritized within each group.

### Alignment (specs ↔ code)

1. **Add `collect_stdout` to every `SessionConfig` / `SubprocessConfig` struct
   snippet in the spec.** This is a user-visible field that affects behaviour.
2. **Fix `Session::new` vs `Session::new_for_test`** — pick one name and remove the
   "test-utils feature" language from the spec if the fn is always public. Ideally
   gate it behind a feature flag as the spec implies.
3. **Update `action-filter.md` to show the correct `filter_message` signature** with
   the `session_id` parameter and explain why (multi-session log streams).
4. **Cross-reference `embedded-cross-user-helper.md` and `cross-user.md`** — a
   single paragraph at the top of each pointing to the other and clarifying which
   is the primary path would help readers orient.
5. **Document `Session::new_for_test`, `clone_cancel_writer`, `action_status`,
   `environments_entered`, and `path_mapping_rules` accessors** in `session.md`.
6. **Document the `build.rs` embedded-helper-binary mechanism** in either
   `architecture.md` or `embedded-cross-user-helper.md`. A cross-platform packager
   must know how the helper is built and where it ends up.

### Spec quality

7. **Collapse repeated Python-vs-Rust comparison tables** into a single
   `architecture.md` section; other files reference it instead.
8. **Add rationale sections** for: 5-second process exit grace, unbounded channel
   choice (already present — good), default cancel grace periods (120s/30s/300s),
   and when to use `Runtime(String)` vs a structured variant.
9. **Add a "Session lifecycle contract" section** in `session.md` that collects
   `Drop` behaviour, `cleanup()` behaviour, cross-user cleanup path, and
   `retain_working_dir` in one place — it's currently distributed.
10. **Specify the banner format precisely** in `logging.md` — column widths,
    padding, so consumers can rely on it or at least know they shouldn't.

### API ergonomics

11. **Replace `filter_message` tuple return with a `FilterResult` struct.** Low-risk
    refactor; 4 call sites.
12. **Introduce `SessionConfigBuilder`** — protects against future field additions
    breaking struct-literal usage.
13. **Promote the most common `SessionError::Runtime` uses to structured variants**
    (e.g., `DuplicateEnvironmentId`, `EnvironmentNotEntered`,
    `InvalidCancelTimeLimit`). Include at least the top 5 by call-site count.
14. **Make `SessionCallbackType` an `Arc<dyn Fn>`** so middleware can share/wrap it.
15. **Consider a `SessionInner` pattern** — refactor `Session` into
    `Session { inner: Arc<SessionInner> }`. Enables cheap cloning (e.g., for
    monitoring wrappers) without a major API change.

### Performance

16. **Redaction via Aho-Corasick** — build the automaton once when redactions
    change; re-use across all lines. Closes the O(V × L) concern.
17. **Pass-through lines avoid allocation** — in `filter_message`, switch the
    return path to `Cow<str>`; allocate only on redaction / annotation.
18. **Path-mapping index for large `LIST_PATH` parameters** — trie or Aho-Corasick
    over source-path prefixes.
19. **Re-use env var HashMaps** — `evaluate_env_vars` could build once per
    environment entry and incrementally diff on each action rather than rebuilding.

### Tests

20. **Add a redaction stress test** asserting that 1MB of log lines with 100
    redacted values completes in a reasonable time budget. Will catch any
    regression from (16).
21. **Add a subagent-stalled backpressure test** to confirm `unbounded_channel`
    does not introduce deadlocks in practice.
22. **Add a `TempDir::Drop` cleanup test** that drops the dir without calling
    `cleanup()` and asserts the directory is removed.
23. **Add `test_error_messages.rs`** for `openjd-sessions`, following the
    AGENTS.md error-message test pattern used in `openjd-model` and
    `openjd-expr`. Assert on full error strings to lock down quality.
24. **Expand Windows behaviour tests** as Windows support matures — behavioural
    tests for CTRL_BREAK_EVENT, process-tree kill, and cross-user launch.
25. ~~**Cover the `WindowsSessionUser::with_password` error mapping in unit tests.**
    The current Windows integration test (`test_windows_permissions.rs`) asserts
    that a non-existent user produces *some* error, but does not assert that
    `ERROR_LOGON_FAILURE` maps to `BadCredentialsError::LogonFailure` (vs the
    catch-all `BadCredentialsError::Other`). The Python binding layer maps
    `LogonFailure` to the user-facing `BadCredentialsException` and `Other` to
    `RuntimeError`, so that distinction is observable to consumers. Add a
    small Windows-only unit test (or extend the existing integration test) that
    pattern-matches on the error variant.~~ **Resolved.** Added a Windows
    unit test in `session_user.rs` covering the process-owner → `Other`
    rejection path; added two integration tests in
    `test_cross_user_windows.rs` covering wrong-password → `LogonFailure`
    (requires the standard `OPENJD_TEST_WIN_USER_NAME` / `_PASSWORD`
    fixtures, gated `#[ignore]`) and nonexistent-user → `LogonFailure`
    (runs in Windows CI without external fixtures). Also tightened the
    existing `test_tempdir_windows_nonvalid_principal_raises_error` in
    `test_windows_permissions.rs` to assert the variant rather than just
    `is_err()`.

---

## 10. Summary

**Rating:** production-ready for POSIX/Linux deployment; Windows support is
explicitly partial and tracks that way.

**Blocking issues:** none.

**High-value next steps:**
1. Close spec drift (items 1–6).
2. Replace the tuple return from `ActionFilter::filter_message` (item 11).
3. Address the redaction performance concern with Aho-Corasick (item 16).
4. Add `test_error_messages.rs` to align with other crates (item 23).

**Nice-to-have:**
- `SessionConfigBuilder` for forward-compatibility (item 12).
- Promote common `Runtime(String)` uses to structured variants (item 13).
- Rationale pass over the spec (item 8).

The codebase reflects careful engineering — the state machine, async cancellation,
and cross-user path are all cleanly modeled. Most findings here are about tightening
alignment between code and specs, not fixing bugs.
