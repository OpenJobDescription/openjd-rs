# openjd-sessions Crate Quality Evaluation Report

**Date:** 2026-04-15
**Crate:** `openjd-sessions` (`crates/openjd-sessions`)
**Scope:** Specifications, implementation source, and tests

---

## Executive Summary

The `openjd-sessions` crate is a well-structured Rust implementation of the OpenJD sessions runtime, faithfully mirroring the Python `openjd-sessions-for-python` library. It compiles cleanly with zero warnings, all 315 tests pass (13 cross-user tests correctly ignored without Docker), and clippy reports no lints. The async architecture using tokio channels is sound, and the cross-user helper binary is an impressive performance optimization.

However, the evaluation found **2 confirmed bugs** (UTF-8 panic on line truncation, and cross-user tests broken by wrong tokio runtime flavor — fixed during this evaluation), **several spec-implementation misalignments** (some significant), **test coverage gaps** in edge cases, and **code quality improvements** that would strengthen the crate.

---

## 1. Build & Test Results

### Standard Tests

| Check | Result |
|-------|--------|
| `cargo build --package openjd-sessions` | ✅ Clean, no warnings |
| `cargo test --package openjd-sessions` | ✅ 315 passed, 0 failed, 13 ignored |
| `cargo clippy --package openjd-sessions -- -W clippy::all` | ✅ Clean, no lints |

Test breakdown:
- Unit tests (lib.rs): 126 passed
- test_session.rs: 89 passed
- test_path_mapping.rs: 33 passed
- test_session_env_step.rs: 20 passed
- test_session_scenarios.rs: 18 passed
- test_embedded_files.rs: 13 passed
- test_tempdir_os.rs: 10 passed
- test_helper.rs: 8 passed
- test_path_mapping_materialize.rs: 5 passed
- test_cross_user.rs: 13 ignored (require Docker)
- Doc-tests: 6 passed

### Docker Cross-User Tests (localuser environment)

| Check | Result |
|-------|--------|
| Cross-user tests (Docker localuser) | ✅ 13 passed, 0 failed |
| CAP_KILL effective+permitted | ✅ 1 passed |
| CAP_KILL permitted-only (elevation test) | ✅ 1 passed |

**Bug found and fixed during testing:** All 9 cross-user tests that exercise the helper
binary were failing with `can call blocking only when running on the multi-threaded runtime`.
The tests used `#[tokio::test]` (single-threaded `current_thread` runtime), but the
cross-user helper code uses `tokio::task::block_in_place()` which requires a multi-threaded
runtime. Fixed by changing to `#[tokio::test(flavor = "multi_thread")]` on the 10 tests
that create a `Session` with a cross-user configuration. The 3 TempDir-only tests don't
go through the helper path and correctly use the default single-threaded runtime.

**Pre-existing failures (unrelated):** 2 scenario tests (`scenario_env_file_let_bindings`,
`scenario_let_host_context`) fail inside Docker because their templates use `python` as
the command, which is not installed in the Rust Docker image. These are not cross-user
issues — they fail the same way outside Docker when Python is unavailable.

---

## 2. Confirmed Bugs

### BUG-1: UTF-8 Panic on 64KB Line Truncation (High)

**Files:** `subprocess.rs` line 909, `cross_user_helper.rs` line 233

Both sites truncate long output lines using byte-index slicing:

```rust
// subprocess.rs
let line = if line.len() > LOG_LINE_MAX_LENGTH {
    line[..LOG_LINE_MAX_LENGTH].to_string()
} else {
    line
};

// cross_user_helper.rs
let line = if line.len() > 64 * 1024 {
    &line[..64 * 1024]
} else {
    line
};
```

`String` indexing in Rust panics if the byte index falls in the middle of a multi-byte UTF-8 character. If a subprocess produces a line exceeding 64KB with multi-byte characters (e.g., internationalized log messages, emoji) near the boundary, this will panic with `byte index N is not a char boundary`.

**Fix:** Use `str::floor_char_boundary()` (stable since Rust 1.82):
```rust
let line = &line[..line.floor_char_boundary(LOG_LINE_MAX_LENGTH)];
```

**Additionally:** `cross_user_helper.rs` uses a magic number `64 * 1024` instead of the `LOG_LINE_MAX_LENGTH` constant from `subprocess.rs`. These should share the constant.

### BUG-2: Cross-User Tests Panic with `current_thread` Runtime (High)

**File:** `tests/test_cross_user.rs`

All 9 cross-user tests that exercise the helper binary used `#[tokio::test]` which
defaults to a single-threaded (`current_thread`) runtime. The cross-user helper code
at `session.rs` line 1105 uses `tokio::task::block_in_place()` which panics on
single-threaded runtimes with: `can call blocking only when running on the multi-threaded runtime`.

This meant **all cross-user subprocess, runner, and session tests were broken** — they
would panic immediately when run in Docker. Only the 3 TempDir tests and 1 cleanup test
(which don't go through the helper path) were passing.

**Fix applied:** Changed `#[tokio::test]` to `#[tokio::test(flavor = "multi_thread")]`
on the 10 tests that create a `Session` with a cross-user user configuration. All 13
cross-user tests now pass, including both CAP_KILL capability variants.

---

## 3. Potential Issues (Medium Severity)

### ISSUE-1: `sudo rm -rf` Missing `--` Separator

**File:** `session.rs`, cleanup method

File paths from the working directory are passed directly as arguments to `sudo rm -rf` without a `--` separator:

```rust
let mut args = vec![
    "-u".to_string(), user.user().to_string(),
    "-i".to_string(), "rm".to_string(), "-rf".to_string(),
];
args.extend(files);
```

If a subprocess created a file with a name starting with `-` (e.g., `-rf` or `--no-preserve-root`), it could be interpreted as a flag to `rm`. Adding `"--".to_string()` before `args.extend(files)` would prevent this.

### ISSUE-2: `is_malformed_env_command` False Positives

**File:** `action_filter.rs` lines 68-73

The malformed command detector matches any line starting with `openjd_env`, `openjd_redacted_env`, or `openjd_unset_env` (case-insensitive). A legitimate log line like `openjd_environment_setup complete` would trigger `CancelMarkFailed`, canceling the entire action. The check should require a colon or end-of-string after the directive name.

### ISSUE-3: Timeout Breaks Stdout Loop Without Draining

**File:** `subprocess.rs`, timeout arm in the `select!` loop

When a timeout fires, the loop `break`s immediately without draining remaining buffered stdout lines. In contrast, the cancel path continues reading until EOF. This means diagnostic output emitted just before the timeout killed the process is silently lost.

### ISSUE-4: `Session::redact` Inconsistent with `ActionFilter::apply_redaction`

**File:** `session.rs`

`Session::redact()` uses sequential `str::replace()` calls in arbitrary `HashSet` iteration order. `ActionFilter::apply_redaction()` uses a proper segment-merge algorithm that handles overlapping redacted values correctly. If two redacted values overlap in text (e.g., "FOOBAR" and "BAR"), `Session::redact()` can produce different results depending on iteration order.

### ISSUE-5: Malformed `openjd_redacted_env` Silently Ignored When Redactions Enabled

**File:** `action_filter.rs`, `handle_redacted_env` error path

When `redactions_enabled` is `true` and the `openjd_redacted_env` payload is malformed, no callback is pushed — the error is silently swallowed. When `redactions_enabled` is `false`, a cancel callback IS pushed. This asymmetry means malformed redacted env commands are silently ignored in the exact configuration where they matter most.

### ISSUE-6: `find_sudo_child_pgid` Fails with Multiple Sudo Children

**File:** `subprocess.rs`, `find_child_pid_procfs`

Returns `None` if sudo has 0 or 2+ children. If sudo forks an intermediate process (e.g., PAM session helper), the actual child's PGID is never discovered, meaning SIGKILL during cancellation only hits the sudo process. Child processes become orphans.

---

## 4. Specification Alignment

### 4.1 High-Severity Misalignments

| # | Spec | Code | Issue |
|---|------|------|-------|
| 1 | session.md says Drop does NOT attempt cleanup | Code DOES call `remove_dir_all` in Drop | Spec contradicts implementation |
| 2 | session.md shows `SubprocessConfig.env_vars: HashMap<String, String>` | Code uses `HashMap<String, Option<String>>` | Type mismatch — `None` means "unset" |
| 3 | session.md shows `cancel_request_rx: oneshot::Receiver<CancelRequest>` | Code uses `watch::Receiver<Option<Duration>>` | Different channel type and payload |
| 4 | subprocess.md shows `run_subprocess(config, filter, session_id, message_tx)` | Code has 5th parameter: `cancel_token` | Missing parameter in spec |

### 4.2 Medium-Severity Misalignments

| # | Spec | Code | Issue |
|---|------|------|-------|
| 5 | action-filter.md describes regex-based parsing | Code uses string prefix matching | Implementation approach changed, spec not updated |
| 6 | action-filter.md implies ALL directive types checked for malformation | Code only checks env-related directives | Malformed `openjd_fail`/`openjd_progress` silently ignored |
| 7 | session.md `enter_environment(env, identifier, os_env_vars, resolved_bindings)` | Code: `enter_environment(&mut self, env, resolved_symtab, identifier, os_env_vars)` | Parameter order and naming differ |
| 8 | session.md `exit_environment(identifier, os_env_vars, keep_session_running)` | Code adds `resolved_symtab` parameter, reorders | Extra parameter, different order |
| 9 | session.md `run_task(step_script, task_parameter_values, os_env_vars, resolved_bindings)` | Code: `run_task(&mut self, script, task_parameter_values, resolved_symtab, os_env_vars)` | Naming and order differ |
| 10 | embedded-files.md `EmbeddedFiles::new(scope)` | Code: `new(scope, session_files_directory, session_id)` | Extra constructor parameters |
| 11 | runners.md shows `notify_period` in `NotifyThenTerminate` | Code uses `terminate_delay` | Field name mismatch |
| 12 | subprocess.md describes 5s grace for stdout drain after exit | Code applies 5s timeout to `c.wait()` | Grace time applied to different thing |

### 4.3 Undocumented Implementation Features

The following code features have no corresponding spec documentation:

- `enter_environment_with_output()` method
- `with_path_mapping()`, `with_library()`, `with_revision_extensions()` builder methods
- `get_enabled_extensions()` method
- `redact()` method on Session
- Windows env var normalization (`normalize_env_key`)
- Duplicate environment identifier rejection
- `format_command_for_log()` and `process_line()` public functions
- CAP_KILL capability elevation for cross-user SIGKILL
- Windows cross-user via `CreateProcessAsUserW`
- Windows `CTRL_BREAK_EVENT` and process tree killing
- `resolve_action_timeout()` function
- All runner builder methods (`with_redactions`, `with_initial_redacted_values`, etc.)

---

## 5. Code Quality Assessment

### 5.1 Strengths

- **Clean compilation**: Zero warnings, zero clippy lints
- **Well-organized module structure**: Clear separation of concerns across 15+ modules
- **Correct async architecture**: The `drive_action` pattern using `tokio::select!` with biased polling and channel-based message passing avoids shared mutable state elegantly
- **Cross-user helper binary**: Impressive optimization reducing per-action overhead from ~1s to ~1ms
- **Comprehensive error types**: `SessionError` with `#[non_exhaustive]` and `thiserror` is idiomatic
- **Platform separation**: Clean `#[cfg(unix)]`/`#[cfg(windows)]` boundaries
- **Security awareness**: Sticky bit validation, 0o700 permissions, redaction support

### 5.2 Issues Found

#### Missing Trait Implementations

| Type | Missing Trait | Impact |
|------|--------------|--------|
| `ScriptRunnerState` | `Display` | Public enum, requires Debug for display |
| `CancelMethod` | `Display` | Public enum, requires Debug for display |
| `ActionState` | `Display` | Public enum, requires Debug for display |
| `ActionMessage` | `Display` | Public enum, requires Debug for display |
| `TempDir` | `Debug` | Non-idiomatic for public type |
| `TempDir` | `AsRef<Path>` | Common Rust pattern for path wrappers |
| `ActionStatus` | `Default` | Verbose construction without it |

#### Code Duplication

- **Runner builder methods**: `EnvironmentScriptRunner` and `StepScriptRunner` have near-identical `new()`, `with_redactions()`, `with_initial_redacted_values()`, `with_cancel_token()`, `with_cancel_request_rx()`, `with_helper()`, `take_helper()`, `cancel()`, `state()` methods. A macro or shared trait would eliminate this.
- **Line truncation**: `64 * 1024` magic number in `cross_user_helper.rs` duplicates `LOG_LINE_MAX_LENGTH` from `subprocess.rs`.
- **`env_script.rs` four-arm match**: The `(let_bindings, embedded_files)` match duplicates `EmbeddedFiles` setup across all arms. The step runner handles this more cleanly with sequential `if let` blocks.

#### Silently Discarded Errors

- `chown_for_user()` in `embedded_files.rs`: Both Unix and Windows paths use `let _ =` to discard chown/permission errors. A failed chown in cross-user mode will cause permission denied errors later.
- `write_helper()` in `helper_binary.rs`: Same pattern — `let _ = nix::unistd::chown(...)`.
- These should at minimum log at warn level.

#### API Design Concerns

- **`#[allow(clippy::too_many_arguments)]`** on `run_action` (8 params) and `run_env_action` (8 params): A config struct would improve readability.
- **`parse_end_of_line` is an identity function**: `fn parse_end_of_line(eol: Option<EndOfLine>) -> Option<EndOfLine> { eol }` does nothing and should be removed.
- **`ActionResult.stderr` is always empty**: The crate merges stderr into stdout at the subprocess level, so this field is misleading.
- **`PosixSessionUser` fields are `pub`**: Allows external mutation bypassing validation. Should be private with accessors.
- **`SessionError::Runtime(String)` overused**: Used for ~10+ distinct error conditions. Dedicated variants like `HelperProtocol`, `PermissionDenied` would enable programmatic error handling.
- **`CrossUserHelper` lacks `Drop` impl**: If dropped without `shutdown()`, the child process is orphaned.
- **`symtab_key()` is public**: Leaks internal implementation detail.

#### Naming

- `custom_gettempdir`: Python-style naming. Rust idiom would be `openjd_temp_dir()`.
- `_runnable` parameter in `write_embedded_file_with_options`: Underscore prefix on a parameter that IS used on Unix. Should use `#[cfg_attr(windows, allow(unused))]` instead.

---

## 6. Test Coverage Assessment

### 6.1 Well-Covered Areas

- **Environment variable lifecycle**: Set, override, unset, redact, restore on exit — very thorough (20+ tests)
- **Path mapping**: All 4 direction combinations (POSIX↔POSIX, POSIX↔Windows, Windows↔POSIX, Windows↔Windows) with 33 tests
- **Session state machine**: Ready → Running → ReadyEnding → Ended transitions, LIFO enforcement, invalid state errors
- **Callback coverage**: Fires in ALL code paths — enter/exit with/without script, task success/failure/command-not-found
- **Let bindings**: All parameter types including PATH, LIST[PATH], RANGE_EXPR, with 18 scenario tests
- **Cross-user execution**: 13 tests covering subprocess identity, signal delivery, process tree kill, permissions, cleanup
- **Helper binary protocol**: 8 tests covering startup/shutdown, sequential commands, cancel, crash, env vars, protocol errors

### 6.2 Coverage Gaps

| Gap | Impact | Recommendation |
|-----|--------|----------------|
| No Windows execution tests | All tests use `sh`/`bash`; Windows paths untested | Add Windows-specific scenarios |
| No concurrent session tests | Thread safety untested | Add multi-session parallel tests |
| No large output tests | Memory pressure, 64KB truncation untested | Add tests with >64KB lines |
| `cancelation` field on `Action` never tested | Grace period, notification command untested | Add tests with `cancelation` set |
| Timeout with format string resolution untested | Only literal timeout values tested | Add format string timeout tests |
| `retain_working_dir = true` never tested | Always false in tests | Add retention test |
| Error message content not asserted | Most tests check `is_err()` only, per AGENTS.md should assert full messages | Add message assertions |
| Embedded file cleanup not verified | Files created but cleanup not checked | Add cleanup verification |
| Multiple path mapping rules matching same path | Only single-rule matching tested | Add longest-prefix-match test |
| End-of-line conversion in session context | Only tested at utility level | Add integration EOL test |
| Progress boundary values | Invalid values (-0.001, 100.001) not tested at session level | Add boundary tests |

### 6.3 Exploratory Test Results

I wrote and ran 13 exploratory edge-case tests. Results:

| Test | Result | Finding |
|------|--------|---------|
| Empty redaction value | ✅ Pass | No crash on empty string |
| Redacted value multiple occurrences | ✅ Pass* | Redaction applies to log output, not raw stdout (by design) |
| Overlapping redacted values | ✅ Pass* | Same — raw stdout is intentionally unredacted |
| Extremely long output line | ✅ Pass | Truncated at 64KB (by design, but UTF-8 panic risk exists) |
| Output with no trailing newline | ✅ Pass | Captured correctly |
| Env var with special chars (hyphen) | ✅ Pass | Properly rejected |
| Env var with special chars (dot) | ✅ Pass | Properly rejected |
| Progress at 0.0 | ✅ Pass | Accepted |
| Progress at 100.0 | ✅ Pass | Accepted |
| Progress at -0.001 | ⚠️ Note | Error annotation in filter, not in raw stdout |
| Progress at 100.001 | ⚠️ Note | Same — error annotation is in log stream |
| Environment re-entry after exit | ✅ Pass | Works correctly |
| Cleanup after workdir deleted | ✅ Pass | No panic |

*Note: The redaction tests initially appeared to fail because `SubprocessResult.stdout` contains raw (unredacted) output by design. Redaction is applied only to the log stream via `ActionFilter.apply_redaction()`. This is correct behavior but could be better documented.

---

## 7. Specifications Assessment

### 7.1 Strengths

- **Comprehensive coverage**: 17 spec documents covering all major subsystems
- **Clear architecture documentation**: Module layout, data flow diagrams, dependency lists
- **Design rationale**: Key decisions (async-first, channel-based messaging, POSIX-first) are well-explained
- **Cross-user documentation**: Thorough coverage of sudo-based execution, helper binary, and Docker test infrastructure
- **Python comparison table**: Useful for understanding design differences

### 7.2 Weaknesses

- **Stale function signatures**: Multiple specs show outdated parameter lists, types, and names (see Section 4)
- **Missing Windows documentation**: Windows cross-user execution, `CTRL_BREAK_EVENT`, process tree killing, and ACL-based permissions are implemented but not spec'd
- **Regex vs string parsing**: action-filter.md describes regex-based parsing that was replaced with string matching
- **Undocumented public API**: ~15 public methods/functions have no spec coverage
- **Drop behavior contradiction**: session.md explicitly says Drop doesn't attempt cleanup, but the code does

---

## 8. Recommendations

### Priority 1 — Fix Bugs

1. **Fix UTF-8 panic in line truncation** (BUG-1): Replace `line[..LOG_LINE_MAX_LENGTH]` with `line[..line.floor_char_boundary(LOG_LINE_MAX_LENGTH)]` in both `subprocess.rs` and `cross_user_helper.rs`. Share the `LOG_LINE_MAX_LENGTH` constant.

2. ~~**Fix cross-user test runtime flavor** (BUG-2)~~: **FIXED** during this evaluation. Changed `#[tokio::test]` to `#[tokio::test(flavor = "multi_thread")]` on 10 cross-user tests.

3. **Add `--` separator to `sudo rm -rf`** (ISSUE-1): Prevents filenames starting with `-` from being interpreted as flags.

### Priority 2 — Fix Spec Misalignments

3. **Update function signatures in specs**: session.md, subprocess.md, embedded-files.md, and runners.md all have outdated signatures. Update to match current code.

4. **Resolve Drop behavior contradiction**: Either update session.md to document that Drop does attempt cleanup, or remove the cleanup from Drop to match the spec.

5. **Update action-filter.md**: Replace regex description with the actual string-matching approach used in the code.

6. **Document Windows support**: Add Windows-specific sections to subprocess.md, cross-user.md, and embedded-files.md.

7. **Document undocumented public API**: Add spec coverage for the ~15 undocumented public methods.

### Priority 3 — Improve Code Quality

8. **Add `Display` impls** for `ScriptRunnerState`, `CancelMethod`, `ActionState`, `ActionMessage`.

9. **Add `Debug` for `TempDir`** and `AsRef<Path>` for ergonomic path usage.

10. **Deduplicate runner builder methods**: Use a macro or trait to eliminate the copy-paste between `EnvironmentScriptRunner` and `StepScriptRunner`.

11. **Log silently discarded errors**: Replace `let _ =` in `chown_for_user()` and `write_helper()` with `warn!` logging.

12. **Add `Drop` impl for `CrossUserHelper`**: Safety net to shut down the helper process if dropped without explicit `shutdown()`.

13. **Remove `parse_end_of_line` identity function** and remove or document `ActionResult.stderr`.

14. **Narrow `is_malformed_env_command`**: Require colon or end-of-string after directive name to avoid false positives.

15. **Fix malformed `openjd_redacted_env` handling**: Push a cancel callback when `redactions_enabled` is true (matching the behavior when it's false).

### Priority 4 — Improve Test Coverage

16. **Add error message content assertions**: Per AGENTS.md standard, assert full error messages, not just `is_err()`.

17. **Add `cancelation` field tests**: Test grace period and notification command on `Action`.

18. **Add `retain_working_dir = true` test**.

19. **Add format string timeout resolution test**.

20. **Add longest-prefix-match path mapping test** with multiple overlapping rules.

21. **Add end-to-end embedded file EOL conversion test** through the session API.

---

## 9. Detailed Module Review

### 9.1 `session.rs` (~700 lines)

The central module implementing the session state machine. Well-structured with clear state transitions and comprehensive environment variable tracking. The `drive_action` pattern using `tokio::select!` is sound — biased polling ensures cancel/timeout aren't starved by stdout floods, and the single-task model avoids the need for locks.

**Reviewed items:**
- SessionState enum and transitions: ✅ Correct
- SessionConfig fields and defaults: ✅ Well-designed
- Environment LIFO enforcement: ✅ Correct, with proper pop-before-exit for failed exits
- Symbol table construction: ✅ Handles all parameter types including PATH mapping
- Cancellation flow: ✅ Token cascading works correctly
- Cleanup: ⚠️ Missing `--` in sudo rm (ISSUE-1)
- Drop: ⚠️ Contradicts spec (attempts cleanup)

### 9.2 `subprocess.rs` (~800 lines)

The async subprocess execution engine. Handles same-user and cross-user execution on both POSIX and Windows. The biased `select!` loop with cancel > timeout > stdout priority is well-designed.

**Reviewed items:**
- Process group isolation via setsid: ✅ Correct
- Stderr merging via dup2: ✅ Correct
- Biased select loop: ✅ Sound design
- Line truncation: ❌ UTF-8 panic risk (BUG-1)
- Cross-user PGID discovery: ⚠️ Fragile with multiple sudo children (ISSUE-6)
- Signal delivery: ✅ Correct with CAP_KILL fallback
- 5-second grace time: ⚠️ Applied to process exit, not stdout drain (spec mismatch)

### 9.3 `action_filter.rs` (~900 lines including tests)

The directive parser for `openjd_*` protocol messages. Comprehensive handling of all directive types with proper redaction support.

**Reviewed items:**
- Directive parsing: ✅ Correct for all directive types
- Redaction algorithm (segment merge): ✅ Correct, handles overlapping values
- Malformed command detection: ⚠️ False positive risk (ISSUE-2)
- JSON-encoded env vars: ✅ Correct
- Dynamic log level: ✅ Correct
- Malformed redacted_env when enabled: ⚠️ Silently ignored (ISSUE-5)
- 126 unit tests inline: ✅ Thorough

### 9.4 `runner/` (mod.rs, env_script.rs, step_script.rs)

Script runner infrastructure with shared base and specialized runners for environment and step scripts.

**Reviewed items:**
- Two-phase embedded file flow: ✅ Correct, handles circular dependency
- Format string resolution: ✅ Handles null (skip), list (expand), scalar
- Cancel method mapping: ✅ Correct
- Code duplication between runners: ⚠️ Significant (see Section 5.2)
- Too-many-arguments: ⚠️ 8-parameter methods

### 9.5 `embedded_files.rs`

Two-phase file materialization with proper permission handling.

**Reviewed items:**
- Allocate/write two-phase flow: ✅ Correct
- Line ending conversion: ✅ Handles LF, CRLF, Auto
- Cross-user permissions: ⚠️ Errors silently discarded
- Identity function `parse_end_of_line`: ⚠️ Dead code

### 9.6 `tempdir.rs`

Secure temporary directory management with RAII cleanup.

**Reviewed items:**
- Random name generation: ✅ UUID-based, unique
- Permission setting: ✅ 0o700 same-user, 0o770 cross-user
- Sticky bit validation: ✅ Defense-in-depth
- Drop safety net: ✅ Best-effort cleanup
- Missing Debug/AsRef<Path>: ⚠️ Non-idiomatic

### 9.7 `cross_user_helper.rs` and `helper/`

Persistent cross-user helper binary that eliminates per-action sudo overhead.

**Reviewed items:**
- Wire protocol (JSON over stdin/stdout): ✅ Correct
- Timeout via Condvar: ✅ Cancellable, no orphaned threads
- Cancel via dup'd stdin fd: ✅ Clever design, avoids borrow conflicts
- Missing Drop impl: ⚠️ Orphan risk
- Line truncation: ❌ Same UTF-8 panic risk as subprocess.rs

### 9.8 `session_user.rs`

Session user identity types for POSIX and Windows.

**Reviewed items:**
- SessionUser trait: ✅ Send + Sync, extensible
- PosixSessionUser: ⚠️ Public fields allow mutation
- WindowsSessionUser: ⚠️ Password stored as plain String
- `is_process_user()` syscall on every call: ⚠️ Could be cached

### 9.9 `logging.rs`

Structured logging with bitflags and session-aware macros.

**Reviewed items:**
- LogContent bitflags: ✅ Clean design
- session_log! macro: ✅ Preserves caller location
- Banner helpers: ✅ Match Python output format
- timestamp_usec `as u64` cast: ⚠️ Technically lossy (won't matter until year 586,524 AD)

### 9.10 `error.rs`

Error types with thiserror derivation.

**Reviewed items:**
- SessionError variants: ✅ Well-designed, #[non_exhaustive]
- Runtime(String) catch-all: ⚠️ Overused, loses type information
- Error propagation: ✅ Consistent Result<T, SessionError> throughout

### 9.11 `capabilities.rs`

Linux CAP_KILL support with RAII guard.

**Reviewed items:**
- CapKillGuard pattern: ✅ Idiomatic RAII
- Non-Linux stub: ✅ Appropriate

### 9.12 `win32.rs`, `win32_permissions.rs`, `win32_locate.rs`

Windows platform support.

**Reviewed items:**
- CreateProcessAsUserW/WithLogonW: ✅ Correct Win32 usage
- DACL permission setting: ✅ Correct
- win32_locate: ⚠️ Known bug documented in spec (PATH fallback resolves to empty string), `#[allow(dead_code)]` pending integration

---

## 10. Performance Assessment

No algorithmic performance issues found. Key observations:

- **Cross-user helper**: Reduces per-action overhead from ~1s to ~1ms — excellent optimization
- **Biased select loop**: Prevents stdout floods from starving cancel/timeout — correct priority
- **Unbounded channel**: Prevents stdout backpressure deadlocks — appropriate for the use case
- **No O(N²) algorithms detected**: Redaction uses segment merge (O(N log N)), path mapping uses linear scan (appropriate for small rule sets)
- **Unnecessary clones**: `env_vars.clone()` in `run_action`, `symtab.clone()` in env_script.rs (4 arms), `file.clone()` in `allocate_file_paths` — minor but could be optimized

---

## 11. Summary Scorecard

| Category | Score | Notes |
|----------|-------|-------|
| Compilation | ⭐⭐⭐⭐⭐ | Zero warnings, zero clippy lints |
| Test pass rate | ⭐⭐⭐⭐⭐ | 315/315 pass, 13 correctly ignored |
| Test coverage | ⭐⭐⭐⭐ | Strong core coverage, gaps in edge cases and Windows |
| Spec alignment | ⭐⭐⭐ | Multiple stale signatures, one behavioral contradiction |
| Code quality | ⭐⭐⭐⭐ | Well-structured, some duplication and missing traits |
| Error handling | ⭐⭐⭐⭐ | Good types, some silently discarded errors |
| Performance | ⭐⭐⭐⭐⭐ | No algorithmic issues, excellent helper optimization |
| Security | ⭐⭐⭐⭐ | Good practices, minor sudo rm issue |
| API ergonomics | ⭐⭐⭐⭐ | Clean public surface, some too-many-arguments methods |
| Rust idioms | ⭐⭐⭐⭐ | Mostly idiomatic, missing some standard trait impls |
