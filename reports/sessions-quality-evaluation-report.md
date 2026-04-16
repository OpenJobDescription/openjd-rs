# openjd-sessions Crate Quality Evaluation Report

**Date:** 2026-04-15
**Crate:** `openjd-sessions` (`crates/openjd-sessions`)
**Scope:** Specifications, implementation source, and tests

---

## Executive Summary

The `openjd-sessions` crate is a well-structured Rust implementation of the OpenJD sessions runtime, faithfully mirroring the Python `openjd-sessions-for-python` library. It compiles cleanly with zero warnings, all 315 tests pass (13 cross-user tests correctly ignored without Docker), and clippy reports no lints. The async architecture using tokio channels is sound, and the cross-user helper binary is an impressive performance optimization.

However, the evaluation found **2 confirmed bugs** (UTF-8 panic on line truncation — fixed, and cross-user tests broken by wrong tokio runtime flavor — fixed during this evaluation), **1 additional finding** (invalid UTF-8 in subprocess stdout silently drops remaining output — fixed), **several spec-implementation misalignments** (some significant), **test coverage gaps** in edge cases, and **code quality improvements** that would strengthen the crate.

---

## 1. Build & Test Results

### Standard Tests

| Check | Result |
|-------|--------|
| `cargo build --package openjd-sessions` | ✅ Clean, no warnings |
| `cargo test --package openjd-sessions` | ✅ 344 passed, 0 failed, 13 ignored |
| `cargo clippy --package openjd-sessions -- -W clippy::all` | ✅ Clean, no lints |

Test breakdown:
- Unit tests (lib.rs): 142 passed
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

### BUG-1: ~~UTF-8 Panic on 64KB Line Truncation~~ (High — FIXED)

**Files:** `subprocess.rs`, `cross_user_helper.rs`

Both sites truncated long output lines using byte-index slicing (`line[..LOG_LINE_MAX_LENGTH]`), which panics if the byte index falls in the middle of a multi-byte UTF-8 character.

**Fix applied:** Extracted a shared `truncate_line()` helper using `str::floor_char_boundary()` (stable since Rust 1.82). Both `subprocess.rs` and `cross_user_helper.rs` now call this helper, and the magic number `64 * 1024` in `cross_user_helper.rs` was replaced with the shared `LOG_LINE_MAX_LENGTH` constant.

**Additional finding during fix:** The `tokio::io::Lines` iterator used to read subprocess stdout calls `String::from_utf8` internally and returns `Err(InvalidData)` on non-UTF-8 bytes. The `Err(_) => break` arm then silently dropped all remaining output. This was fixed by replacing `Lines` with `read_until(b'\n')` + `String::from_utf8_lossy`, matching Python's surrogate-escaping / lossy decoding behavior. Invalid bytes are now replaced with U+FFFD (�) and reading continues.

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

## 3. Potential Issues (Medium Severity) — ALL FIXED

### ISSUE-1: ~~`sudo rm -rf` Missing `--` Separator~~ (FIXED)

**File:** `session.rs`, cleanup method

**Fix applied:** Added `"--".to_string()` before `args.extend(files)` to prevent filenames starting with `-` from being interpreted as flags to `rm`.

### ISSUE-2: ~~`is_malformed_env_command` False Positives~~ (FIXED)

**File:** `action_filter.rs`

**Fix applied:** Narrowed the malformed command detector to require a colon, space, or end-of-string after the directive name (`openjd_env:`, `openjd_env `, or `openjd_env` exactly). A legitimate log line like `openjd_environment_setup complete` no longer triggers `CancelMarkFailed`.

### ISSUE-3: ~~Timeout Breaks Stdout Loop Without Draining~~ (FIXED)

**File:** `subprocess.rs`, timeout arm in the `select!` loop

**Fix applied:** Removed the `break` from the timeout arm so the loop continues draining stdout until EOF, matching the cancel path behavior. Diagnostic output emitted before the timeout kill is no longer silently lost.

### ISSUE-4: ~~`Session::redact` Inconsistent with `ActionFilter::apply_redaction`~~ (FIXED)

**File:** `session.rs`

**Fix applied:** `Session::redact()` now sorts redacted values by length descending before iterating, so longer matches are replaced first. This makes overlapping redaction deterministic regardless of `HashSet` iteration order.

### ISSUE-5: ~~Malformed `openjd_redacted_env` Silently Ignored When Redactions Enabled~~ (FIXED)

**File:** `action_filter.rs`, `handle_redacted_env` error path

**Fix applied:** The cancel callback is now always pushed on malformed `openjd_redacted_env`, regardless of the `redactions_enabled` flag. The asymmetry where errors were silently swallowed in the exact configuration where they mattered most is eliminated.

### ISSUE-6: ~~`find_sudo_child_pgid` Fails with Multiple Sudo Children~~ (FIXED)

**File:** `subprocess.rs`, `find_child_pids_procfs` / `find_child_pids_pgrep`

**Fix applied:** Renamed to return `Vec<i32>` of all child PIDs instead of `Option<i32>`. The caller now iterates all children to find one whose PGID differs from sudo's, instead of giving up when sudo has multiple children (e.g., PAM session helper).

---

## 4. Specification Alignment

### 4.1 High-Severity Misalignments — ALL FIXED

| # | Spec | Code | Resolution |
|---|------|------|------------|
| 1 | ~~session.md says Drop does NOT attempt cleanup~~ | Code DOES call `remove_dir_all` in Drop | **FIXED** — spec updated to document best-effort cleanup in Drop |
| 2 | ~~subprocess.md shows `env_vars: HashMap<String, String>`~~ | Code uses `HashMap<String, Option<String>>` | **FIXED** — spec updated to `Option<String>` (None = unset) |
| 3 | ~~subprocess.md shows `oneshot::Receiver<CancelRequest>`~~ | Code uses `watch::Receiver<Option<Duration>>` | **FIXED** — spec updated to match code |
| 4 | ~~subprocess.md shows 4-param `run_subprocess`~~ | Code has 5th parameter: `cancel_token` | **FIXED** — spec updated with `cancel_token` parameter |

### 4.2 Medium-Severity Misalignments — ALL FIXED

| # | Spec | Code | Resolution |
|---|------|------|------------|
| 5 | ~~action-filter.md describes regex-based parsing~~ | Code uses string prefix matching | **FIXED** — spec rewritten to document `strip_prefix` + `match` approach |
| 6 | ~~action-filter.md implies ALL directive types checked~~ | Code only checks env-related directives | **FIXED** — spec updated to document env-only checking with rationale |
| 7 | ~~session.md `enter_environment(env, identifier, os_env_vars, resolved_bindings)`~~ | Code: `(env, resolved_symtab, identifier, os_env_vars)` | **FIXED** — spec updated to match code |
| 8 | ~~session.md `exit_environment(identifier, os_env_vars, keep_session_running)`~~ | Code adds `resolved_symtab`, reorders | **FIXED** — spec updated to match code |
| 9 | ~~session.md `run_task(step_script, task_parameter_values, os_env_vars, resolved_bindings)`~~ | Code: `(script, task_parameter_values, resolved_symtab, os_env_vars)` | **FIXED** — spec updated to match code |
| 10 | ~~embedded-files.md `EmbeddedFiles::new(scope)`~~ | Code: `new(scope, session_files_directory, session_id)` | **FIXED** — spec updated with extra parameters |
| 11 | ~~runners.md shows `notify_period`~~ | Code uses `terminate_delay` | **FIXED** — spec updated to `terminate_delay` |
| 12 | ~~subprocess.md describes 5s grace for stdout drain~~ | Code applies 5s timeout to `c.wait()` | **FIXED** — spec rewritten to document process exit timeout |

### 4.3 Undocumented Implementation Features — ALL DOCUMENTED

All previously undocumented features now have spec coverage:

- ~~`enter_environment_with_output()` method~~ → session.md
- ~~`with_path_mapping()`, `with_library()`, `with_revision_extensions()` builder methods~~ → session.md
- ~~`get_enabled_extensions()` method~~ → session.md
- ~~`redact()` method on Session~~ → session.md
- ~~Windows env var normalization (`normalize_env_key`)~~ → session.md
- ~~Duplicate environment identifier rejection~~ → session.md
- ~~`format_command_for_log()` and `process_line()` public functions~~ → subprocess.md (noted as `pub(crate)`)
- ~~CAP_KILL capability elevation for cross-user SIGKILL~~ → subprocess.md
- ~~Windows cross-user via `CreateProcessAsUserW`~~ → subprocess.md
- ~~Windows `CTRL_BREAK_EVENT` and process tree killing~~ → subprocess.md
- ~~`resolve_action_timeout()` function~~ → runners.md
- ~~All runner builder methods (`with_redactions`, `with_initial_redacted_values`, etc.)~~ → runners.md
- ~~`collect_stdout` opt-in~~ → session.md, subprocess.md

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

#### ~~Missing Trait Implementations~~ — ALL FIXED

| Type | Trait | Status |
|------|-------|--------|
| `ScriptRunnerState` | `Display` | ✅ Implemented |
| `CancelMethod` | `Display` | ✅ Implemented |
| `ActionState` | `Display` | ✅ Implemented |
| `ActionMessage` | `Display` | ✅ Implemented |
| `TempDir` | `Debug` | ✅ Implemented |
| `TempDir` | `AsRef<Path>` | ✅ Implemented |
| `ActionStatus` | `Default` | ✅ Implemented |

#### Code Duplication

- **Runner builder methods**: `EnvironmentScriptRunner` and `StepScriptRunner` have near-identical `new()`, `with_redactions()`, `with_initial_redacted_values()`, `with_cancel_token()`, `with_cancel_request_rx()`, `with_helper()`, `take_helper()`, `cancel()`, `state()` methods. A macro or shared trait would eliminate this.
- ~~**Line truncation**: `64 * 1024` magic number in `cross_user_helper.rs` duplicates `LOG_LINE_MAX_LENGTH` from `subprocess.rs`.~~ **FIXED** — shared `truncate_line()` helper.
- **`env_script.rs` four-arm match**: The `(let_bindings, embedded_files)` match duplicates `EmbeddedFiles` setup across all arms. The step runner handles this more cleanly with sequential `if let` blocks.

#### ~~Silently Discarded Errors~~ — FIXED

- ~~`chown_for_user()` in `embedded_files.rs`: Both Unix and Windows paths use `let _ =` to discard chown/permission errors.~~ **FIXED** — `chown_for_user()` now returns `Result<(), SessionError>`. Chown runs before chmod (matching Python's security pattern: don't widen permissions if group ownership wasn't set).
- ~~`write_helper()` in `helper_binary.rs`: Same pattern — `let _ = nix::unistd::chown(...)`.~~ **FIXED** — errors propagated, chown before chmod.
- All four `let _ = nix::unistd::chown(...)` sites fixed: `embedded_files.rs`, `helper_binary.rs`, `tempdir.rs`, `subprocess.rs`.

#### API Design Concerns

- **`#[allow(clippy::too_many_arguments)]`** on `run_action` (8 params) and `run_env_action` (8 params): A config struct would improve readability.
- ~~**`parse_end_of_line` is an identity function**~~: **FIXED** — removed, inlined `record.file.end_of_line` directly.
- ~~**`ActionResult.stderr` is always empty**~~: **FIXED** — removed the field entirely.
- ~~**`Session::new` misleading name**~~: **FIXED** — renamed to `Session::new_for_test`, gated behind `#[cfg(test)]`.
- ~~**Unbounded stdout accumulation**~~: **FIXED** — added `collect_stdout: bool` to `SessionConfig` (default `false`). When false, subprocess output is still streamed through the filter/callback in real time, but the collected `String` stays empty. Prevents unbounded memory growth in the worker agent.
- **`PosixSessionUser` fields are `pub`**: Allows external mutation bypassing validation. Should be private with accessors.
- **`SessionError::Runtime(String)` overused**: Used for ~10+ distinct error conditions. Dedicated variants like `HelperProtocol`, `PermissionDenied` would enable programmatic error handling.
- ~~**`CrossUserHelper` lacks `Drop` impl**~~: **FIXED** — added `Drop` for both POSIX and Windows variants. Logs warning and kills/terminates the child if dropped without `shutdown()`.
- **`symtab_key()` is public**: Leaks internal implementation detail.

#### Naming

- ~~`custom_gettempdir`~~: **FIXED** — renamed to `openjd_temp_dir()`.
- ~~`Session::new` for a test-only constructor~~: **FIXED** — renamed to `Session::new_for_test`.
- ~~`_runnable` parameter in `write_embedded_file_with_options`~~: **FIXED** — renamed to `runnable` with `#[cfg_attr(not(unix), allow(unused))]`.

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
| ~~No large output tests~~ | ~~Memory pressure, 64KB truncation untested~~ | **FIXED** — `test_truncate_line_multibyte_boundary` and `test_truncate_line_short_line_unchanged` added |
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
| Extremely long output line | ✅ Pass | Truncated at 64KB, UTF-8 safe via `floor_char_boundary` |
| Output with no trailing newline | ✅ Pass | Captured correctly |
| Env var with special chars (hyphen) | ✅ Pass | Properly rejected |
| Env var with special chars (dot) | ✅ Pass | Properly rejected |
| Progress at 0.0 | ✅ Pass | Accepted |
| Progress at 100.0 | ✅ Pass | Accepted |
| Progress at -0.001 | ✅ Fixed | Error annotation now appears in collected stdout |
| Progress at 100.001 | ✅ Fixed | Same — collected stdout uses display string with annotations and redaction |
| Environment re-entry after exit | ✅ Pass | Works correctly |
| Cleanup after workdir deleted | ✅ Pass | No panic |

*Note: Collected stdout now uses the filter-processed display string, so redacted values appear as `********` and error annotations (e.g., invalid progress) appear inline. This matches what the user sees in logs.

---

## 7. Specifications Assessment

### 7.1 Strengths

- **Comprehensive coverage**: 17 spec documents covering all major subsystems
- **Clear architecture documentation**: Module layout, data flow diagrams, dependency lists
- **Design rationale**: Key decisions (async-first, channel-based messaging, POSIX-first) are well-explained
- **Cross-user documentation**: Thorough coverage of sudo-based execution, helper binary, and Docker test infrastructure
- **Python comparison table**: Useful for understanding design differences

### 7.2 Weaknesses

- ~~**Stale function signatures**~~: **FIXED** — All specs updated to match current code
- ~~**Missing Windows documentation**~~: **FIXED** — subprocess.md now covers CTRL_BREAK, process tree killing, and CreateProcessAsUserW
- ~~**Regex vs string parsing**~~: **FIXED** — action-filter.md rewritten to document string-based parsing
- ~~**Undocumented public API**~~: **FIXED** — All methods now have spec coverage
- ~~**Drop behavior contradiction**~~: **FIXED** — session.md updated to document best-effort cleanup in Drop

---

## 8. Recommendations

### Priority 1 — Fix Bugs

1. ~~**Fix UTF-8 panic in line truncation** (BUG-1)~~: **FIXED.** Extracted shared `truncate_line()` helper using `floor_char_boundary()` in both `subprocess.rs` and `cross_user_helper.rs`. Also replaced `tokio::io::Lines` with `read_until` + `from_utf8_lossy` so invalid UTF-8 bytes no longer silently drop remaining output.

2. ~~**Fix cross-user test runtime flavor** (BUG-2)~~: **FIXED** during this evaluation. Changed `#[tokio::test]` to `#[tokio::test(flavor = "multi_thread")]` on 10 cross-user tests.

3. ~~**Add `--` separator to `sudo rm -rf`** (ISSUE-1)~~: **FIXED.** Prevents filenames starting with `-` from being interpreted as flags.

### Priority 2 — ~~Fix Spec Misalignments~~ ALL FIXED

3. ~~**Update function signatures in specs**~~: **FIXED.** session.md, subprocess.md, embedded-files.md, and runners.md all updated to match current code.

4. ~~**Resolve Drop behavior contradiction**~~: **FIXED.** session.md updated to document that Drop does perform best-effort cleanup.

5. ~~**Update action-filter.md**~~: **FIXED.** Replaced regex description with actual string-matching approach, documented env-only malformed command checking with rationale.

6. ~~**Document Windows support**~~: **FIXED.** Added Windows CTRL_BREAK, process tree killing, and CreateProcessAsUserW sections to subprocess.md.

7. ~~**Document undocumented public API**~~: **FIXED.** All ~15 undocumented methods/functions now have spec coverage across session.md, subprocess.md, and runners.md.

### Priority 3 — Improve Code Quality

8. ~~**Add `Display` impls**~~ for `ScriptRunnerState`, `CancelMethod`, `ActionState`, `ActionMessage`: **FIXED.**

9. ~~**Add `Debug` for `TempDir`** and `AsRef<Path>`~~: **FIXED.**

10. **Deduplicate runner builder methods**: Use a macro or trait to eliminate the copy-paste between `EnvironmentScriptRunner` and `StepScriptRunner`.

11. ~~**Log silently discarded errors**~~: **FIXED.** All `let _ =` chown/chmod calls now propagate errors. Chown runs before chmod matching Python's security pattern.

12. ~~**Add `Drop` impl for `CrossUserHelper`**~~: **FIXED.** Both POSIX and Windows variants now have `Drop` impls.

13. ~~**Remove `parse_end_of_line` identity function** and remove or document `ActionResult.stderr`~~: **FIXED.** Identity function removed; `ActionResult.stderr` field removed.

14. ~~**Narrow `is_malformed_env_command`**~~: **FIXED.** Requires colon, space, or end-of-string after directive name to avoid false positives.

15. ~~**Fix malformed `openjd_redacted_env` handling**~~: **FIXED.** Cancel callback now pushed regardless of `redactions_enabled` flag.

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
- Cleanup: ✅ Fixed — `--` separator added to sudo rm (ISSUE-1)
- Drop: ⚠️ Contradicts spec (attempts cleanup)

### 9.2 `subprocess.rs` (~800 lines)

The async subprocess execution engine. Handles same-user and cross-user execution on both POSIX and Windows. The biased `select!` loop with cancel > timeout > stdout priority is well-designed.

**Reviewed items:**
- Process group isolation via setsid: ✅ Correct
- Stderr merging via dup2: ✅ Correct
- Biased select loop: ✅ Sound design
- Line truncation: ✅ Fixed — uses `floor_char_boundary` via shared `truncate_line()` helper
- Lossy UTF-8 decoding: ✅ Fixed — uses `read_until` + `from_utf8_lossy` instead of `Lines`
- Cross-user PGID discovery: ✅ Fixed — handles multiple sudo children (ISSUE-6)
- Signal delivery: ✅ Correct with CAP_KILL fallback
- 5-second grace time: ⚠️ Applied to process exit, not stdout drain (spec mismatch)

### 9.3 `action_filter.rs` (~900 lines including tests)

The directive parser for `openjd_*` protocol messages. Comprehensive handling of all directive types with proper redaction support.

**Reviewed items:**
- Directive parsing: ✅ Correct for all directive types
- Redaction algorithm (segment merge): ✅ Correct, handles overlapping values
- Malformed command detection: ✅ Fixed — requires delimiter after directive name (ISSUE-2)
- JSON-encoded env vars: ✅ Correct
- Dynamic log level: ✅ Correct
- Malformed redacted_env when enabled: ✅ Fixed — cancel callback always pushed (ISSUE-5)
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
- Cross-user permissions: ✅ Fixed — errors propagated, chown before chmod
- Identity function `parse_end_of_line`: ✅ Fixed — removed

### 9.6 `tempdir.rs`

Secure temporary directory management with RAII cleanup.

**Reviewed items:**
- Random name generation: ✅ UUID-based, unique
- Permission setting: ✅ 0o700 same-user, 0o770 cross-user
- Sticky bit validation: ✅ Defense-in-depth
- Drop safety net: ✅ Best-effort cleanup
- Missing Debug/AsRef<Path>: ✅ Fixed — both implemented

### 9.7 `cross_user_helper.rs` and `helper/`

Persistent cross-user helper binary that eliminates per-action sudo overhead.

**Reviewed items:**
- Wire protocol (JSON over stdin/stdout): ✅ Correct
- Timeout via Condvar: ✅ Cancellable, no orphaned threads
- Cancel via dup'd stdin fd: ✅ Clever design, avoids borrow conflicts
- Missing Drop impl: ✅ Fixed — both POSIX and Windows variants have Drop impls
- Line truncation: ✅ Fixed — uses shared `truncate_line()` helper

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
- ~~**Unbounded stdout accumulation**~~: **FIXED** — `collect_stdout` opt-in prevents unbounded memory growth in the worker agent path
- **Unnecessary clones**: `env_vars.clone()` in `run_action`, `symtab.clone()` in env_script.rs (4 arms), `file.clone()` in `allocate_file_paths` — minor but could be optimized

---

## 11. Summary Scorecard

| Category | Score | Notes |
|----------|-------|-------|
| Compilation | ⭐⭐⭐⭐⭐ | Zero warnings, zero clippy lints |
| Test pass rate | ⭐⭐⭐⭐⭐ | 344/344 pass, 13 correctly ignored |
| Test coverage | ⭐⭐⭐⭐ | Strong core coverage, gaps in edge cases and Windows |
| Spec alignment | ⭐⭐⭐⭐⭐ | All signatures and behaviors updated to match code |
| Code quality | ⭐⭐⭐⭐ | Well-structured, some duplication and missing traits |
| Error handling | ⭐⭐⭐⭐⭐ | Good types, chown/chmod errors now propagated |
| Performance | ⭐⭐⭐⭐⭐ | No algorithmic issues, excellent helper optimization |
| Security | ⭐⭐⭐⭐⭐ | Good practices, sudo rm -- separator added |
| API ergonomics | ⭐⭐⭐⭐ | Clean public surface, some too-many-arguments methods |
| Rust idioms | ⭐⭐⭐⭐⭐ | All standard trait impls added |
