# Windows Executable Location

## Purpose

On Windows, subprocess commands specified in OpenJD templates may use bare
executable names (e.g., `python`) that need to be resolved to full paths before
launching. This module mirrors the Python implementation's
`_win32/_locate_executable.py` to resolve executables using the working directory
and PATH environment variable.

## Why pre-spawn resolution is required

Rust's `std::process::Command` alone is not a faithful Windows command search:

1. **No PATHEXT awareness.** `Command::new("tool")` only probes `tool.exe`
   per PATH directory. Canonical Windows search (cmd.exe, `where.exe`,
   Python's `shutil.which`) tries every PATHEXT extension in each directory
   before moving to the next — so a `tool.bat` in an earlier PATH directory
   must beat a `tool.exe` in a later one. Without pre-resolution, the later
   `.exe` wins, and a bare name whose only match is a `.bat`/`.cmd` fails
   with "program not found".
2. **Legacy fallback search.** When the child environment's PATH has no
   match, `CreateProcessW` falls back to the application directory, system
   directories, and the *parent process's* PATH — even after `env_clear()`.
   An action's command could silently resolve to a binary the action's
   environment never referenced.
3. **Working directory.** The Python implementation searches the session
   working directory first (`working_dir;PATH`), so scripts materialized as
   embedded files are runnable by bare name. `std::process::Command` never
   searches the cwd.

Resolving to an absolute path before spawn eliminates all three: absolute
paths bypass the fallback machinery entirely.

## Function

```rust
pub(crate) fn locate_windows_executable(
    args: &[String],
    os_env_vars: Option<&HashMap<String, Option<String>>>,
    working_dir: &Path,
) -> Result<Vec<String>, String>
```

Returns a copy of `args` with `args[0]` resolved to an absolute path, or
`Err("Could not find executable file: <command>")` — the same message the
Python implementation raises. The error surfaces as
`SessionError::SubprocessStart` with `std::io::ErrorKind::NotFound`.

## Resolution rules

1. **Absolute paths** — returned as-is. The OS handles extension resolution
   (e.g., `C:\Python\python` → `C:\Python\python.exe`).

2. **All other commands** — resolved with `shutil.which` semantics via the
   shared `win32_which::locate_in` (see "Shared search implementation"
   below) using a search path constructed as `{working_dir};{PATH}`. The
   working directory is prepended so that executables in the session
   working directory take precedence. Within each directory every
   candidate PATHEXT extension is tried before moving to the next
   directory (earliest directory wins).

3. **PATHEXT semantics** — the candidate extensions come from the
   action's PATHEXT, not the resolving process's:
   - A bare name tries each PATHEXT extension in order per directory.
   - A command that already ends with a *listed* extension
     (case-insensitive) is looked up as-is.
   - A command whose explicit extension is **not** in PATHEXT is not
     runnable and reports not-found even if the file exists — matching
     `shutil.which` and cmd.exe. (`script.ps1` under the default PATHEXT
     is an error, not a spawn attempt that later fails with
     `%1 is not a valid Win32 application`.)
   - An empty or absent PATHEXT selects the `shutil.which` default list
     (`.COM;.EXE;.BAT;.CMD;.VBS;.JS;.WS;.MSC`).

4. **PATH/PATHEXT source selection** — each comes from exactly one
   place, chosen once before searching (never a chain where a failed
   search retries against the other source):
   - If `os_env_vars` contains the key (case-insensitive), that value is
     used **exclusively** — for PATH, even if it is empty, in which case
     only the working directory is searched. The process environment is
     never consulted.
   - An **explicit unset** (`Some(None)` in the same-user path's
     `HashMap<String, Option<String>>` — e.g. from an
     `openjd_unset_env: PATH` directive; the spawn merge removes the
     variable from the child) counts as *present*: PATH resolves as
     empty (working directory only) and PATHEXT as the default
     extension list. Falling back to the process value here would
     resolve against directories the action deliberately dropped.
     (The helper path is unaffected: its protocol env map is
     `String → String` with unsets already filtered out before
     dispatch.)
   - Only when `os_env_vars` has no such key at all is the process
     environment's value used instead.

   This mirrors the environment merge applied at spawn: an
   action-supplied value *overwrites* the base value for the child, an
   unset *removes* it, so resolution searches exactly what the child
   will see. Note: Python's `_get_path_var_for_shutil_which` conflates
   unset with absent and falls back to the process PATH; this is an
   intentional divergence in favor of the merge semantics.

5. **Not found** — a hard error (Python parity), never a fallthrough to
   `CreateProcessW`'s legacy search.

## Shared search implementation

The search itself lives in `helper/src/win32_which.rs` and is compiled
into **both** binaries: the session crate includes it via
`#[path = "helper/src/win32_which.rs"]` from `win32_locate.rs`, and the
embedded helper compiles it as a regular module. One source file, two
binaries — the two spawn paths cannot drift apart.

It is hand-written rather than using the `which` crate because `which`
cannot honor the action's environment: it always reads PATHEXT from the
resolving process (so an action setting `PATHEXT=.EXE` would still match
a `.bat`), and it accepts any existing file when the command has an
explicit extension (so `script.ps1` would resolve and then fail at spawn
with error 193 instead of the not-found error `shutil.which` gives).

## Call sites

Both Windows spawn paths resolve the command before launch, with
identical search semantics (both compile the shared `win32_which.rs`):

- **Same-user** — `win32_locate::locate_windows_executable`, called from
  `run_subprocess()` in `subprocess.rs` before building the merged
  environment. Mirrors the Python `_runner_base.py` call site. PATH
  fallback: the session process's environment.
- **Cross-user** — `locate_executable` in the embedded helper
  (`helper/src/runner_win.rs`), called before `Command::new` for each
  dispatched action. Resolution failures return over the stdin/stdout
  protocol as `{"error": "Could not find executable file: …"}`, which
  `run_via_helper` maps to `SessionError::SubprocessStart`.

## Cross-user behavior

Cross-user resolution runs **inside the helper — as the target user** —
which makes it correct by construction on two axes:

1. **Permissions.** Executables in directories only the target user can
   read resolve correctly. This matches the intent of Python's
   `_locate_for_other_user`, which spawns a `shutil.which` probe as the
   target user; the Rust helper does the probe in-process since it already
   runs as that user (no extra subprocess, no probe overhead).
2. **PATH/PATHEXT source.** The same either/or selection as the
   same-user path: action-supplied values are used exclusively; only
   when the action's env vars define no such key at all does the helper
   use its own environment — which is the target user's environment
   block (`CreateEnvironmentBlock` at helper spawn), i.e. the exact base
   environment the workload inherits when the value isn't overridden.
   In both cases resolution searches with what the child actually runs
   under, never a union of the two. A host-side resolver would use the
   *service user's* values in the no-override case, resolving against
   an environment the child never sees.

## Tests

- Unit tests in `win32_which.rs` cover the search itself: PATHEXT
  precedence, action-PATHEXT restriction (PATHEXT=.EXE excludes `.bat`),
  explicit-extension-outside-PATHEXT not-found, case-insensitive
  extension matching, empty-PATHEXT default, and pathy-command
  cwd-only resolution. They run in both the session crate's test suite
  and the helper's standalone suite.
- Unit tests in `win32_locate.rs` cover the wrapper: working-dir
  precedence, case-insensitive PATH key lookup, process-PATH fallback,
  absolute-path passthrough, and the not-found error.
- Integration tests in `tests/integration/test_win32_locate.rs` pin the
  behaviors end-to-end through `Session::run_subprocess` (same-user),
  including the "absent from action PATH must fail" guarantee that proves
  the legacy fallback search is bypassed, action-PATHEXT restriction,
  and `.ps1`-outside-PATHEXT not-found.
- `tests/integration/test_helper.rs` drives the helper binary directly
  over its protocol and pins the helper-side resolution (PATHEXT
  precedence, action-PATHEXT restriction, `.ps1` not-found, cwd-first,
  not-found error, helper-PATH fallback).
- `tests/integration/test_cross_user_windows.rs` covers the full
  cross-user path with a real second user (working-dir `.bat` by bare
  name, not-found via the protocol, resolution in a user-readable
  directory with a protected DACL).
